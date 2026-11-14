import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Polarity, RunDetail } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { TopBarSlot } from "../components/Layout";
import { EventControls, SyncStatus } from "../components/record/EventControls";
import { NoteField } from "../components/record/NoteField";
import { PipControl } from "../components/record/PipControl";
import { QrPanel, logQrOpened } from "../components/record/QrPanel";
import { PolarityIcon } from "../components/record/RecordIcons";
import { TapPad } from "../components/record/TapPad";
import { POLARITY_LABELS, formatClock, formatElapsed } from "../lib/labels";
import { detectDeviceType, editOp, removeOp, useRecordSync } from "../lib/recordQueue";
import { runQueueScope } from "../lib/runs";
import { logScaffold } from "../lib/scaffold";
import { useTapper } from "../lib/useTapper";

/** The console re-reads the run this often while class is on (bookmarks from a paired phone). */
export const RUN_POLL_MS = 20_000;

/**
 * 上課中 (the in-class console, docs/design/overhaul/console.html) for one open run. While class
 * is on, the top bar shows only 「上課中」 and 「結束本節課」; the page is the clock, the two big
 * buttons and what has been marked so far.
 */
export function RunConsolePage() {
  const runId = Number(useParams().runId);
  const navigate = useNavigate();
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const send = useCallback((ops: Parameters<typeof api.syncRunEvents>[1]) => api.syncRunEvents(runId, ops), [runId]);
  const [state, sync] = useRecordSync(runQueueScope(runId), send);
  const deviceType = useMemo(() => detectDeviceType(), []);
  const tapper = useTapper(sync, state.events, deviceType);
  const [now, setNow] = useState(() => Date.now());
  const [showQr, setShowQr] = useState(false);
  const [confirmEnd, setConfirmEnd] = useState(false);
  const [ending, setEnding] = useState(false);
  const [endError, setEndError] = useState<string | null>(null);
  const shownLogged = useRef(false);
  const usedLogged = useRef(false);
  const qrLogged = useRef(false);

  const refresh = useCallback(() => {
    const stamp = sync.nextStamp();
    return api.getRun(runId).then((d) => {
      setDetail(d);
      sync.setServerEvents(d.events, d.ended_at, stamp);
    });
  }, [runId, sync]);

  useEffect(() => {
    let alive = true;
    refresh().catch((err: unknown) => {
      if (alive) setLoadError(errorMessage(err));
    });
    return () => {
      alive = false;
    };
  }, [refresh]);

  const open = detail !== null && detail.ended_at === null;

  useEffect(() => {
    if (!open) return;
    const poll = setInterval(() => void refresh().catch(() => undefined), RUN_POLL_MS);
    const clock = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      clearInterval(poll);
      clearInterval(clock);
    };
  }, [open, refresh]);

  useEffect(() => {
    if (open && !shownLogged.current) {
      shownLogged.current = true;
      logScaffold("SC-RECORD", "show");
    }
  }, [open]);

  const tap = (polarity: Polarity, via?: "console" | "pip") => {
    tapper.tap(polarity, via);
    if (!usedLogged.current) {
      usedLogged.current = true;
      logScaffold("SC-RECORD", "use", { trigger: via ?? "console" });
    }
  };

  async function endRun() {
    setEnding(true);
    setEndError(null);
    await sync.flush().catch(() => undefined);
    try {
      await api.endRun(runId);
      navigate(`/runs/${runId}/review`);
    } catch (err) {
      setEndError(`${errorMessage(err)} 書籤都還保存在這台裝置，連上網路後再按一次。`);
      setEnding(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!detail) return <Loading />;

  const context = [detail.class_name, detail.unit_title].filter(Boolean).join(" · ");

  if (!open) {
    return (
      <section aria-labelledby="run-heading">
        <p className="crumbs">
          {detail.class_id ? (
            <>
              <Link to="/">班級</Link> / <Link to={`/classes/${detail.class_id}`}>{detail.class_name}</Link>
            </>
          ) : (
            <Link to="/record">上課紀錄</Link>
          )}{" "}
          / <Link to={`/lessons/${detail.lesson_id}`}>{detail.lesson_title}</Link>
        </p>
        <div className="page-head">
          <h1 id="run-heading">{detail.lesson_title}</h1>
          <p className="lead">{context}</p>
        </div>
        <Notice tone="info">
          這節課已在 {formatClock(detail.ended_at, false)} 結束。書籤可以在回顧頁補寫或更正。{" "}
          <Link to={`/runs/${runId}/review`}>前往課後補寫</Link>
        </Notice>
      </section>
    );
  }

  const events = state.events;

  return (
    <section aria-labelledby="run-heading">
      <TopBarSlot>
        <span className="top-status">上課中 · {state.pending === 0 ? "已同步" : `${state.pending} 筆待同步`}</span>
        <button
          type="button"
          className="text-link is-plain top-exit"
          aria-expanded={confirmEnd}
          onClick={() => setConfirmEnd(true)}
        >
          結束本節課
        </button>
      </TopBarSlot>

      {confirmEnd && (
        <div className="notice notice-info end-confirm" role="group" aria-label="結束本節課">
          <p>確定結束本節課？結束後會到課後補寫，書籤都會留著。</p>
          <div className="button-row">
            <button type="button" className="secondary-button" onClick={() => void endRun()} disabled={ending}>
              {ending ? "結束中…" : "確定結束並回顧"}
            </button>
            <button type="button" className="text-link is-plain" onClick={() => setConfirmEnd(false)} disabled={ending}>
              繼續上課
            </button>
          </div>
          <ErrorText message={endError} />
        </div>
      )}

      <div className="console-top">
        <div>
          <p className="quiet-note">{context}</p>
          <h1 id="run-heading">{detail.lesson_title}</h1>
        </div>
        <time className="clock" aria-label="已上課時間">
          {formatElapsed(now - Date.parse(detail.started_at))}
        </time>
      </div>

      <SyncStatus state={state} onRetry={() => void sync.retry()} />

      <TapPad onTap={(p) => tap(p)} onUndo={tapper.undo} lastEvent={tapper.lastEvent} />
      <NoteField target={tapper.noteTarget} onCommit={tapper.commitNote} />

      <section className="console-log" aria-labelledby="log-heading">
        <div className="head-row">
          <h2 id="log-heading">{events.length === 0 ? "這節課還沒有記錄" : `這節課記了 ${events.length} 次`}</h2>
          <div className="record-tools">
            <button
              type="button"
              className="text-link is-plain"
              aria-expanded={showQr}
              onClick={() => {
                setShowQr((v) => !v);
                if (!qrLogged.current) {
                  qrLogged.current = true;
                  logQrOpened();
                }
              }}
            >
              {showQr ? "收起手機遙控" : "用手機當遙控器"}
            </button>
            <PipControl>
              <strong className="pip-title">{detail.lesson_title}</strong>
              <TapPad compact onTap={(p) => tap(p, "pip")} onUndo={tapper.undo} lastEvent={tapper.lastEvent} />
              <small className="pip-sync">{state.pending === 0 ? "全部已同步" : `${state.pending} 筆待同步`}</small>
            </PipControl>
          </div>
        </div>

        {showQr && <QrPanel runId={runId} remotes={detail.remotes} onChanged={() => void refresh().catch(() => undefined)} />}

        {events.length === 0 ? (
          <p className="empty-line">還沒有書籤。沒有書籤也沒關係。</p>
        ) : (
          <ol className="live-log record-log">
            {[...events].reverse().map((ev) => (
              <li key={ev.device_event_id} aria-label={`${formatClock(ev.device_ts)} ${POLARITY_LABELS[ev.polarity]}`}>
                <time>{formatClock(ev.device_ts)}</time>
                <div className="event-main">
                  <span className={`record-pill record-pill-${ev.polarity}`}>
                    <PolarityIcon polarity={ev.polarity} size={16} />
                    {POLARITY_LABELS[ev.polarity]}
                  </span>
                  {ev.note && <span className="record-note">{ev.note}</span>}
                  {ev.pending && <span className="pending-tag">待同步</span>}
                </div>
                <EventControls
                  event={ev}
                  onEdit={(change) => sync.enqueue(editOp(ev.device_event_id, change))}
                  onDelete={() => sync.enqueue(removeOp("delete", ev.device_event_id))}
                />
              </li>
            ))}
          </ol>
        )}
        <p className="quiet-note console-foot">
          現在按一下就好，細節下課後再補寫。上課中不會呼叫 AI，也不錄音、不拍照。只記錄整體課堂觀察，不要寫學生姓名。書籤先存在這台裝置；Safari
          久未使用會清除網站暫存，建議「加入主畫面」並在下課後連上網路。
        </p>
      </section>
    </section>
  );
}
