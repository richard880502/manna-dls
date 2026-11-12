import { waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../api/client";
import type { RecordOp, SyncResponse } from "../api/types";
import {
  RecordSync,
  applyOps,
  createOp,
  detectDeviceType,
  editOp,
  loadQueue,
  newClientId,
  removeOp,
  saveQueue,
} from "../lib/recordQueue";
import { fakeRecordServer as fakeServer, recordEvent as serverEvent } from "../test/fakeRecordServer";

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

afterEach(() => {
  vi.useRealTimers();
});

describe("device-side ids and device type", () => {
  it("makes UUID v4 ids, also without crypto.randomUUID (non-HTTPS)", () => {
    const a = newClientId();
    expect(a).toMatch(UUID_V4);
    expect(newClientId()).not.toBe(a);
    vi.spyOn(globalThis.crypto, "randomUUID").mockImplementation(() => {
      throw new Error("insecure context");
    });
    const b = newClientId();
    expect(b).toMatch(UUID_V4);
    expect(newClientId()).not.toBe(b);
  });

  it("detects phone / tablet / desktop", () => {
    const iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148";
    const ipad = "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15";
    const ipadOs = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/18.0 Safari/605.1.15";
    const androidPhone = "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 Chrome/131 Mobile Safari/537.36";
    const androidTab = "Mozilla/5.0 (Linux; Android 14; SM-X710) AppleWebKit/537.36 Chrome/131 Safari/537.36";
    const windows = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36";
    expect(detectDeviceType(iphone, 5)).toBe("phone");
    expect(detectDeviceType(ipad, 5)).toBe("tablet");
    expect(detectDeviceType(ipadOs, 5)).toBe("tablet");
    expect(detectDeviceType(ipadOs, 0)).toBe("desktop");
    expect(detectDeviceType(androidPhone, 5)).toBe("phone");
    expect(detectDeviceType(androidTab, 10)).toBe("tablet");
    expect(detectDeviceType(windows, 0)).toBe("desktop");
  });

  it("stamps creates with the device time and type", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-12T01:41:05.123Z"));
    const op = createOp("stuck", "phone", "pip");
    expect(op).toMatchObject({
      kind: "create",
      device_ts: "2026-11-12T01:41:05.123Z",
      polarity: "stuck",
      device_type: "phone",
      via: "pip",
    });
    expect(op.op_id).toMatch(UUID_V4);
    expect(op.device_event_id).toMatch(UUID_V4);
    expect(op.op_id).not.toBe(op.device_event_id);
    expect(createOp("good", "desktop")).not.toHaveProperty("via");
  });
});

describe("applyOps", () => {
  it("shows the server state with pending ops on top, in device-time order", () => {
    const server = [
      serverEvent("b", "2026-11-12T01:00:05Z", { polarity: "stuck" }),
      serverEvent("a", "2026-11-12T01:00:05.500Z"),
    ];
    const c = { ...createOp("good", "desktop"), device_event_id: "c", device_ts: "2026-11-12T00:59:00.000Z" };
    const ops: RecordOp[] = [
      c,
      editOp("b", { note: "例句太難" }),
      removeOp("undo", "a"),
      // a retried create of something the server already has does not duplicate it
      { ...createOp("stuck", "desktop"), device_event_id: "b", device_ts: "2026-11-12T03:00:00Z" },
      editOp("missing", { note: "x" }),
    ];
    const view = applyOps(server, ops);
    expect(view.map((e) => e.device_event_id)).toEqual(["c", "b"]);
    expect(view[0]).toMatchObject({ polarity: "good", pending: true });
    expect(view[0]?.server).toBeUndefined();
    expect(view[1]).toMatchObject({ polarity: "stuck", note: "例句太難", pending: true });
    expect(view[1]?.server?.device_event_id).toBe("b");
    expect(applyOps(server, []).map((e) => e.device_event_id)).toEqual(["b", "a"]);
  });
});

describe("device storage", () => {
  it("survives a blocked localStorage with an in-memory copy", () => {
    const op = createOp("good", "desktop");
    saveQueue("test:1", [op]);
    expect(loadQueue("test:1")).toEqual([op]);
    expect(JSON.parse(window.localStorage.getItem("manna_dls_record_queue:test:1") ?? "[]")).toEqual([op]);

    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    const op2 = createOp("stuck", "desktop");
    expect(() => saveQueue("test:2", [op2])).not.toThrow();
    expect(loadQueue("test:2")).toEqual([op2]);
  });

  it("ignores garbage in storage", () => {
    window.localStorage.setItem("manna_dls_record_queue:test:3", "{not json");
    expect(loadQueue("test:3")).toEqual([]);
    window.localStorage.setItem("manna_dls_record_queue:test:4", JSON.stringify([{ nope: 1 }]));
    expect(loadQueue("test:4")).toEqual([]);
  });
});

describe("RecordSync", () => {
  it("keeps ops on the device until answered and retries on reconnect without duplicates", async () => {
    const server = fakeServer();
    let online = false;
    const send = vi.fn(async (ops: RecordOp[]) => {
      if (!online) throw new ApiError(0, "network", "offline");
      return server.apply(ops);
    });
    const sync = new RecordSync("run:1", send);
    sync.start();
    const op = createOp("good", "phone");
    sync.enqueue(op);
    // Written to the device before anything was sent.
    expect(loadQueue("run:1")).toEqual([op]);
    await waitFor(() => expect(sync.getSnapshot().offline).toBe(true));
    expect(sync.getSnapshot()).toMatchObject({ pending: 1, stopped: false });
    expect(sync.getSnapshot().events[0]).toMatchObject({ device_event_id: op.device_event_id, pending: true });

    // Another tap while offline joins the queue.
    const op2 = createOp("stuck", "phone");
    sync.enqueue(op2);
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    expect(sync.getSnapshot().pending).toBe(2);

    online = true;
    window.dispatchEvent(new Event("online"));
    await waitFor(() => expect(sync.getSnapshot().pending).toBe(0));
    expect(send.mock.calls.at(-1)?.[0]).toEqual([op, op2]);
    expect(server.events.size).toBe(2);
    expect(loadQueue("run:1")).toEqual([]);
    expect(sync.getSnapshot().events.every((e) => !e.pending)).toBe(true);
    expect(sync.getSnapshot().offline).toBe(false);
    sync.dispose();
  });

  it("a lost response is resent and answered as a duplicate", async () => {
    const server = fakeServer();
    let calls = 0;
    const sync = new RecordSync("run:2", async (ops) => {
      calls += 1;
      const res = server.apply(ops);
      if (calls === 1) throw new ApiError(502, "internal", "bad gateway"); // applied, but the answer never arrived
      return res;
    });
    sync.start();
    sync.enqueue(createOp("good", "desktop"));
    await waitFor(() => expect(sync.getSnapshot().offline).toBe(true));
    await sync.retry();
    expect(calls).toBe(2);
    expect(server.events.size).toBe(1);
    expect(sync.getSnapshot()).toMatchObject({ pending: 0, error: null });
    expect(sync.getSnapshot().events).toHaveLength(1);
    sync.dispose();
  });

  it("schedules timed retries after a failure", async () => {
    vi.useFakeTimers();
    const send = vi.fn(async (): Promise<SyncResponse> => {
      throw new ApiError(0, "network", "offline");
    });
    const sync = new RecordSync("run:3", send);
    sync.start();
    sync.enqueue(createOp("good", "desktop"));
    await vi.advanceTimersByTimeAsync(0);
    expect(send).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(send).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(send).toHaveBeenCalledTimes(3);
    sync.dispose();
    await vi.advanceTimersByTimeAsync(120_000);
    expect(send).toHaveBeenCalledTimes(3);
  });

  it("stops on a 4xx, keeps the ops, and reports rejected ops", async () => {
    const server = fakeServer();
    let fail = true;
    const sync = new RecordSync("run:4", async (ops) => {
      if (fail) throw new ApiError(404, "not_found", "找不到這節課的紀錄。");
      return server.apply(ops);
    });
    sync.start();
    sync.enqueue(createOp("good", "desktop"));
    await waitFor(() => expect(sync.getSnapshot().stopped).toBe(true));
    expect(sync.getSnapshot().pending).toBe(1);
    expect(sync.getSnapshot().error).toContain("找不到這節課的紀錄");
    // reconnecting does not hammer a 4xx
    window.dispatchEvent(new Event("online"));
    expect(sync.getSnapshot().stopped).toBe(true);

    fail = false;
    sync.enqueue(editOp("never-created", { note: "x" }));
    await sync.retry();
    expect(sync.getSnapshot()).toMatchObject({ pending: 0, stopped: false });
    expect(sync.getSnapshot().error).toContain("有 1 個操作未被接受");
    sync.dispose();
  });

  it("ignores a server snapshot older than the latest sync", async () => {
    const server = fakeServer();
    const sync = new RecordSync("run:5", async (ops) => server.apply(ops));
    sync.start();
    const stale = sync.nextStamp(); // a GET that started before the sync below
    sync.enqueue(createOp("good", "desktop"));
    await waitFor(() => expect(sync.getSnapshot().pending).toBe(0));
    sync.setServerEvents([], null, stale);
    expect(sync.getSnapshot().events).toHaveLength(1);
    sync.setServerEvents([], null); // a fresh one wins
    expect(sync.getSnapshot().events).toHaveLength(0);
    sync.dispose();
  });
});
