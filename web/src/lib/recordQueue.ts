// Offline-first bookmark queue (docs/API.md「Phase 1.2」, spec §4.3 / §7.1).
//
// Every tap / undo / edit / delete is written to the device first as an operation with its own
// op_id, then sent to the server in order. The server upserts creates by
// (run, device_event_id) and dedupes changes by op_id, so resending after a lost response never
// duplicates anything. An op leaves the device only after the server answered it.

import { useEffect, useMemo, useSyncExternalStore } from "react";
import { ApiError } from "../api/client";
import type { DeviceType, Polarity, RecordEvent, RecordOp, RecordVia, SyncResponse } from "../api/types";

/** 「加一句話」 collapses after this many seconds without typing (spec: 秒數待定). */
export const NOTE_IDLE_SECONDS = 8;
/** Retry delays after a failed sync; the last one repeats. Reconnecting retries at once. */
export const RETRY_DELAYS_MS: readonly number[] = [5_000, 10_000, 30_000, 60_000];
/** Server limit is 500 per request. */
export const MAX_BATCH = 200;

const QUEUE_PREFIX = "manna_dls_record_queue:";

// ---------------------------------------------------------------- ids, device type

/** Device-side id: UUID v4. Falls back to getRandomValues (randomUUID needs HTTPS). */
export function newClientId(): string {
  const c: Crypto | undefined = typeof globalThis.crypto === "object" ? globalThis.crypto : undefined;
  try {
    if (c && typeof c.randomUUID === "function") return c.randomUUID();
  } catch {
    // insecure context: fall through
  }
  const bytes = new Uint8Array(16);
  if (c && typeof c.getRandomValues === "function") {
    c.getRandomValues(bytes);
  } else {
    for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** phone / tablet / desktop from the user agent (iPadOS reports a Mac UA with touch points). */
export function detectDeviceType(
  userAgent: string = typeof navigator === "undefined" ? "" : navigator.userAgent,
  maxTouchPoints: number = typeof navigator === "undefined" ? 0 : (navigator.maxTouchPoints ?? 0),
): DeviceType {
  if (/iPad|Tablet|PlayBook|Silk|Kindle/i.test(userAgent)) return "tablet";
  if (/Macintosh/i.test(userAgent) && maxTouchPoints > 1) return "tablet";
  if (/Android/i.test(userAgent) && !/Mobile/i.test(userAgent)) return "tablet";
  if (/iPhone|iPod|Android|Mobi|Windows Phone/i.test(userAgent)) return "phone";
  return "desktop";
}

// ---------------------------------------------------------------- local view

/** A bookmark as shown on this device: the server's latest state with pending ops applied. */
export interface ViewEvent {
  device_event_id: string;
  device_ts: string;
  polarity: Polarity;
  note: string;
  device_type: DeviceType;
  via: RecordVia;
  /** Has operations not yet confirmed by the server. */
  pending: boolean;
  /** Server copy (review fields etc.); undefined until the create has synced. */
  server?: RecordEvent;
}

function time(ts: string): number {
  const t = Date.parse(ts);
  return Number.isNaN(t) ? 0 : t;
}

/** Applies queued ops on top of the server's events (deleted bookmarks disappear). */
export function applyOps(server: readonly RecordEvent[], ops: readonly RecordOp[]): ViewEvent[] {
  const view = new Map<string, ViewEvent>();
  for (const e of server) {
    view.set(e.device_event_id, {
      device_event_id: e.device_event_id,
      device_ts: e.device_ts,
      polarity: e.polarity,
      note: e.note,
      device_type: e.device_type,
      via: e.via,
      pending: false,
      server: e,
    });
  }
  for (const op of ops) {
    const current = view.get(op.device_event_id);
    switch (op.kind) {
      case "create":
        if (!current) {
          view.set(op.device_event_id, {
            device_event_id: op.device_event_id,
            device_ts: op.device_ts,
            polarity: op.polarity,
            note: op.note ?? "",
            device_type: op.device_type,
            via: op.via ?? "console",
            pending: true,
          });
        }
        break;
      case "edit":
        if (current) {
          view.set(op.device_event_id, {
            ...current,
            polarity: op.polarity ?? current.polarity,
            note: op.note ?? current.note,
            pending: true,
          });
        }
        break;
      case "undo":
      case "delete":
        view.delete(op.device_event_id);
        break;
    }
  }
  return [...view.values()].sort(
    (a, b) => time(a.device_ts) - time(b.device_ts) || a.device_event_id.localeCompare(b.device_event_id),
  );
}

// ---------------------------------------------------------------- device storage

/** Only used while localStorage refuses writes (private mode, quota, blocked storage). */
const memoryQueues = new Map<string, RecordOp[]>();

function isOp(value: unknown): value is RecordOp {
  if (!value || typeof value !== "object") return false;
  const op = value as Record<string, unknown>;
  return typeof op.op_id === "string" && typeof op.device_event_id === "string" && typeof op.kind === "string";
}

/** Pending ops for a scope. localStorage may throw (private mode, blocked storage): memory copy then. */
export function loadQueue(scope: string): RecordOp[] {
  const key = QUEUE_PREFIX + scope;
  try {
    const raw = window.localStorage.getItem(key);
    if (raw === null) return memoryQueues.get(key) ?? [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isOp) : [];
  } catch {
    return memoryQueues.get(key) ?? [];
  }
}

export function saveQueue(scope: string, ops: readonly RecordOp[]): void {
  const key = QUEUE_PREFIX + scope;
  try {
    if (ops.length === 0) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, JSON.stringify(ops));
    memoryQueues.delete(key);
  } catch {
    memoryQueues.set(key, [...ops]);
  }
}

// ---------------------------------------------------------------- sync engine

export interface SyncState {
  events: ViewEvent[];
  /** Ops written on this device and not yet confirmed by the server. */
  pending: number;
  syncing: boolean;
  /** The last sync attempt failed (network / server); will retry. */
  offline: boolean;
  /** Message for the teacher (rejections, or why syncing stopped). */
  error: string | null;
  /** Syncing stopped after a 4xx; ops stay on the device until retry(). */
  stopped: boolean;
  endedAt: string | null;
}

type Sender = (ops: RecordOp[]) => Promise<SyncResponse>;

export class RecordSync {
  private ops: RecordOp[];
  private server: RecordEvent[] = [];
  private endedAt: string | null = null;
  private inFlight = false;
  private again = false;
  private offline = false;
  private stopped = false;
  private error: string | null = null;
  private retryIndex = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private active = false;
  private stamp = 0;
  private appliedStamp = 0;
  private listeners = new Set<() => void>();
  private snapshot: SyncState;
  private readonly onOnline = () => {
    this.retryIndex = 0;
    void this.flush();
  };
  private readonly onVisible = () => {
    if (typeof document !== "undefined" && document.visibilityState === "visible") void this.flush();
  };

  constructor(
    readonly scope: string,
    private send: Sender | null = null,
  ) {
    this.ops = loadQueue(scope);
    this.snapshot = this.buildSnapshot();
  }

  /** Where ops are sent (the teacher's run endpoint or the remote's). */
  setSender(send: Sender): void {
    this.send = send;
  }

  // -- useSyncExternalStore
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): SyncState => this.snapshot;

  private buildSnapshot(): SyncState {
    return {
      events: applyOps(this.server, this.ops),
      pending: this.ops.length,
      syncing: this.inFlight,
      offline: this.offline,
      error: this.error,
      stopped: this.stopped,
      endedAt: this.endedAt,
    };
  }

  private notify(): void {
    this.snapshot = this.buildSnapshot();
    for (const l of this.listeners) l();
  }

  // -- lifecycle
  start(): void {
    this.active = true;
    if (typeof window !== "undefined") {
      window.addEventListener("online", this.onOnline);
      document.addEventListener("visibilitychange", this.onVisible);
    }
    void this.flush();
  }

  dispose(): void {
    this.active = false;
    this.clearTimer();
    if (typeof window !== "undefined") {
      window.removeEventListener("online", this.onOnline);
      document.removeEventListener("visibilitychange", this.onVisible);
    }
  }

  /** Call before fetching the run; pass the result to setServerEvents. */
  nextStamp(): number {
    this.stamp += 1;
    return this.stamp;
  }

  /** Server snapshot from a GET. Ignored when a newer snapshot (e.g. a sync response) already landed. */
  setServerEvents(events: RecordEvent[], endedAt: string | null, stamp: number = this.nextStamp()): void {
    if (stamp < this.appliedStamp) return;
    this.appliedStamp = stamp;
    this.server = events;
    this.endedAt = endedAt;
    this.notify();
  }

  /** Replaces one server event (e.g. after saving its review). */
  upsertServerEvent(event: RecordEvent): void {
    const others = this.server.filter((e) => e.device_event_id !== event.device_event_id);
    this.server = [...others, event];
    this.notify();
  }

  // -- writing
  enqueue(op: RecordOp): void {
    this.ops = [...this.ops, op];
    saveQueue(this.scope, this.ops);
    this.notify();
    void this.flush();
  }

  /** Manual retry (button): also resumes after a 4xx stop. */
  retry(): Promise<void> {
    this.stopped = false;
    this.retryIndex = 0;
    return this.flush();
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  private scheduleRetry(): void {
    this.clearTimer();
    if (!this.active) return;
    const delay = RETRY_DELAYS_MS[Math.min(this.retryIndex, RETRY_DELAYS_MS.length - 1)] ?? 60_000;
    this.retryIndex += 1;
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.flush();
    }, delay);
  }

  /** Sends pending ops (in order). Safe to call any time; concurrent calls coalesce. */
  async flush(): Promise<void> {
    const send = this.send;
    if (!this.active || this.stopped || !send) return;
    if (this.inFlight) {
      this.again = true;
      return;
    }
    if (this.ops.length === 0) return;
    this.clearTimer();
    this.inFlight = true;
    this.notify();
    const batch = this.ops.slice(0, MAX_BATCH);
    const stamp = this.nextStamp();
    let succeeded = false;
    try {
      const res = await send(batch);
      const answered = new Set(res.results.map((r) => r.op_id));
      this.ops = this.ops.filter((op) => !answered.has(op.op_id));
      saveQueue(this.scope, this.ops);
      if (stamp >= this.appliedStamp) {
        this.appliedStamp = stamp;
        this.server = res.events;
        this.endedAt = res.ended_at;
      }
      const rejected = res.results.filter((r) => r.status === "rejected");
      this.error =
        rejected.length > 0
          ? `有 ${rejected.length} 個操作未被接受：${rejected[0]?.message ?? "格式不正確"}`
          : null;
      this.offline = false;
      this.retryIndex = 0;
      succeeded = true;
    } catch (err) {
      if (err instanceof ApiError && err.status >= 400 && err.status < 500) {
        this.stopped = true;
        this.offline = false;
        this.error = `${err.message}（書籤仍保存在這台裝置）`;
      } else {
        this.offline = true;
        this.error = null;
        this.scheduleRetry();
      }
    } finally {
      this.inFlight = false;
      this.notify();
    }
    const more = this.again || this.ops.length > 0;
    this.again = false;
    if (succeeded && more) await this.flush();
  }
}

/** Device-side queue + sync for one scope (e.g. `run:12`, `remote:12`). */
export function useRecordSync(scope: string, send: Sender): [SyncState, RecordSync] {
  const sync = useMemo(() => new RecordSync(scope), [scope]);
  // Declared before start() so the sender is in place for the first flush.
  useEffect(() => {
    sync.setSender(send);
  }, [sync, send]);
  useEffect(() => {
    sync.start();
    return () => sync.dispose();
  }, [sync]);
  const state = useSyncExternalStore(sync.subscribe, sync.getSnapshot);
  return [state, sync];
}

// ---------------------------------------------------------------- op builders

export function createOp(
  polarity: Polarity,
  deviceType: DeviceType,
  via?: "console" | "pip",
): Extract<RecordOp, { kind: "create" }> {
  return {
    op_id: newClientId(),
    kind: "create",
    device_event_id: newClientId(),
    device_ts: new Date().toISOString(),
    polarity,
    device_type: deviceType,
    ...(via ? { via } : {}),
  };
}

export function editOp(deviceEventId: string, change: { polarity?: Polarity; note?: string }): RecordOp {
  return { op_id: newClientId(), kind: "edit", device_event_id: deviceEventId, ...change, at: new Date().toISOString() };
}

export function removeOp(kind: "undo" | "delete", deviceEventId: string): RecordOp {
  return { op_id: newClientId(), kind, device_event_id: deviceEventId, at: new Date().toISOString() };
}
