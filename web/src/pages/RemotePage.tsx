import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { ApiError, errorMessage, remoteApi } from "../api/client";
import type { DeviceType, RecordOp, RemoteRun } from "../api/types";
import { DemoBanner, ErrorText, Loading, Notice } from "../components/common";
import { SyncStatus } from "../components/record/EventControls";
import { TapPad } from "../components/record/TapPad";
import { detectDeviceType, useRecordSync } from "../lib/recordQueue";
import { useTapper } from "../lib/useTapper";
import { useDocumentTitle } from "../lib/useDocumentTitle";

const REMOTE_KEY = "manna_dls_remote";
export const REMOTE_POLL_MS = 30_000;

export interface RemoteSession {
  token: string;
  run_id: number;
  lesson_title: string;
}

/** Only used while localStorage refuses writes. */
let memorySession: RemoteSession | null = null;

export function loadRemoteSession(): RemoteSession | null {
  try {
    const raw = window.localStorage.getItem(REMOTE_KEY);
    if (!raw) return memorySession;
    const s = JSON.parse(raw) as Partial<RemoteSession>;
    if (typeof s.token === "string" && typeof s.run_id === "number") {
      return { token: s.token, run_id: s.run_id, lesson_title: s.lesson_title ?? "" };
    }
    return null;
  } catch {
    return memorySession;
  }
}

function saveRemoteSession(s: RemoteSession | null): void {
  try {
    if (s) window.localStorage.setItem(REMOTE_KEY, JSON.stringify(s));
    else window.localStorage.removeItem(REMOTE_KEY);
    memorySession = null;
  } catch {
    memorySession = s;
  }
}

/** Reads `#pair=<code>` from the QR link. */
export function pairingCodeFromHash(hash: string): string | null {
  const m = /(?:^#|&)pair=([^&]+)/.exec(hash);
  if (!m?.[1]) return null;
  try {
    return decodeURIComponent(m[1]);
  } catch {
    return null;
  }
}

function RemoteShell({ title, children }: { title: string; children: ReactNode }) {
  return (
    <>
      <DemoBanner />
      <main className="remote-shell">
        <header className="remote-head">
          <div className="brand-mark star-mascot" aria-hidden="true">
            <span className="star" />
          </div>
          <div>
            <strong>Manna 遙控器</strong>
            <span>{title}</span>
          </div>
        </header>
        {children}
      </main>
    </>
  );
}

function RemoteConsole({
  session,
  deviceType,
  onExpired,
}: {
  session: RemoteSession;
  deviceType: DeviceType;
  onExpired: (message: string) => void;
}) {
  const send = useCallback((ops: RecordOp[]) => remoteApi.sync(session.token, ops), [session.token]);
  const [state, sync] = useRecordSync(`remote:${session.run_id}`, send);
  const tapper = useTapper(sync, state.events, deviceType);
  const [run, setRun] = useState<RemoteRun | null>(null);

  const refresh = useCallback(() => {
    const stamp = sync.nextStamp();
    return remoteApi
      .getRun(session.token)
      .then((r) => {
        setRun(r);
        sync.setServerEvents(r.events, r.ended_at, stamp);
      })
      .catch((err: unknown) => {
        if (err instanceof ApiError && err.status === 401) onExpired(err.message);
      });
  }, [session.token, sync, onExpired]);

  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), REMOTE_POLL_MS);
    return () => clearInterval(id);
  }, [refresh]);

  const ended = (state.endedAt ?? run?.ended_at ?? null) !== null;

  return (
    <RemoteShell title={run?.lesson_title || session.lesson_title}>
      {ended ? (
        <Notice tone="info">本節課已結束。課後請在電腦上補寫書籤。</Notice>
      ) : (
        <TapPad onTap={(p) => tapper.tap(p)} onUndo={tapper.undo} lastEvent={tapper.lastEvent} />
      )}
      <SyncStatus state={state} onRetry={() => void sync.retry()} />
      <p className="remote-count">這個遙控器已記錄 {state.events.length} 筆</p>
      <p className="field-hint">
        遙控器只能新增書籤、復原自己按的書籤；修改與補寫請在電腦上進行。建議把這個網頁「加入主畫面」：Safari
        久未使用會清除暫存，沒網路時書籤會先存在手機。
      </p>
    </RemoteShell>
  );
}

/** /remote — the phone remote opened from the QR code. No teacher login. */
export function RemotePage() {
  useDocumentTitle("Manna 遙控器");
  const deviceType = useMemo(() => detectDeviceType(), []);
  const [code] = useState(() => pairingCodeFromHash(window.location.hash));
  const [session, setSession] = useState<RemoteSession | null>(() => (code ? null : loadRemoteSession()));
  const [pairing, setPairing] = useState(code !== null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!code) return;
    // Drop the one-time code from the address bar and history.
    try {
      window.history.replaceState(null, "", window.location.pathname + window.location.search);
    } catch {
      // ignore
    }
    let alive = true;
    remoteApi
      .pair(code, deviceType)
      .then((res) => {
        const s = { token: res.token, run_id: res.run.run_id, lesson_title: res.run.lesson_title };
        saveRemoteSession(s);
        if (alive) setSession(s);
      })
      .catch((err: unknown) => {
        if (alive) {
          setError(errorMessage(err));
          setSession(loadRemoteSession());
        }
      })
      .finally(() => {
        if (alive) setPairing(false);
      });
    return () => {
      alive = false;
    };
  }, [code, deviceType]);

  const onExpired = useCallback((message: string) => {
    saveRemoteSession(null);
    setSession(null);
    setError(message);
  }, []);

  if (pairing) {
    return (
      <RemoteShell title="配對中">
        <Loading label="正在配對這支手機…" />
      </RemoteShell>
    );
  }
  if (!session) {
    return (
      <RemoteShell title="尚未配對">
        <ErrorText message={error} />
        <p>請在電腦的上課畫面按「用手機當遙控器」，再用相機掃描 QR Code。</p>
      </RemoteShell>
    );
  }
  return <RemoteConsole key={session.token} session={session} deviceType={deviceType} onExpired={onExpired} />;
}
