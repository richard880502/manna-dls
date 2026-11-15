import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Handling, PlanStepRef, ReviewRequest, RunDetail, StudentBehavior } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { EventControls, SyncStatus } from "../components/record/EventControls";
import { PolarityIcon } from "../components/record/RecordIcons";
import {
  BEHAVIOR_LABELS,
  DEVICE_TYPE_LABELS,
  HANDLINGS,
  HANDLING_LABELS,
  POLARITY_LABELS,
  STUDENT_BEHAVIORS,
  VIA_LABELS,
  formatClock,
  formatDateTime,
} from "../lib/labels";
import { editOp, removeOp, useRecordSync, type RecordSync, type ViewEvent } from "../lib/recordQueue";
import { runQueueScope } from "../lib/runs";
import { logScaffold } from "../lib/scaffold";

export const FOLLOWUP_MAX_CHARS = 2000;

function stepLabel(s: PlanStepRef): string {
  const head = s.activity_type.trim() || s.content.trim().slice(0, 24) || "（未填內容）";
  return `步驟 ${s.position}：${head}`;
}

/** What the page needs from a card to save its pending 補寫 before completing or leaving. */
interface ReviewCardHandle {
  /** Saves the card if it has unsaved edits; resolves to an error message, or null. */
  save: () => Promise<string | null>;
}

interface ReviewCardProps {
  runId: number;
  event: ViewEvent;
  steps: PlanStepRef[];
  sync: RecordSync;
  /** Registers (or, with null, removes) the card's handle. Stable. */
  register: (deviceEventId: string, handle: ReviewCardHandle | null) => void;
  /** Reports unsaved edits. Stable. */
  onDirtyChange: (deviceEventId: string, dirty: boolean) => void;
  /** The privacy reminder sits under the first card's field only, not under every card. */
  showPrivacyHint: boolean;
}

/** One bookmark: time, 順利／卡住, the in-class sentence, corrections and the optional 補寫 fields. */
function ReviewCard({ runId, event, steps, sync, register, onDirtyChange, showPrivacyHint }: ReviewCardProps) {
  const ids = useId();
  const server = event.server;
  const [loadedFrom, setLoadedFrom] = useState<number | null>(server ? server.id : null);
  const [stepId, setStepId] = useState(server?.activity_step_id ? String(server.activity_step_id) : "");
  const [behaviors, setBehaviors] = useState<StudentBehavior[]>(server?.behaviors ?? []);
  const [handling, setHandling] = useState<Handling | "">(server?.handling ?? "");
  const [followup, setFollowup] = useState(server?.followup_text ?? "");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A bookmark tapped offline gets its server copy later: load its fields once.
  if (server && loadedFrom === null) {
    setLoadedFrom(server.id);
    setStepId(server.activity_step_id ? String(server.activity_step_id) : "");
    setBehaviors(server.behaviors);
    setHandling(server.handling ?? "");
    setFollowup(server.followup_text);
  }

  const touch = () => {
    setDirty(true);
    setSaved(false);
  };

  async function save(): Promise<string | null> {
    const body: ReviewRequest = {
      activity_step_id: stepId ? Number(stepId) : null,
      behaviors: STUDENT_BEHAVIORS.filter((b) => behaviors.includes(b)),
      handling: handling || null,
      followup_text: followup.trim(),
    };
    setSaving(true);
    setError(null);
    try {
      const updated = await api.reviewEvent(runId, event.device_event_id, body);
      sync.upsertServerEvent(updated);
      setDirty(false);
      setSaved(true);
      logScaffold("SC-RECORD", "use", { trigger: "review" });
      return null;
    } catch (err) {
      const message = errorMessage(err);
      setError(message);
      return message;
    } finally {
      setSaving(false);
    }
  }

  const id = event.device_event_id;
  // Re-registered every render so the page always calls the save with the current fields.
  useEffect(() => {
    register(id, { save: () => (dirty ? save() : Promise.resolve(null)) });
  });
  useEffect(() => () => register(id, null), [id, register]);
  useEffect(() => {
    onDirtyChange(id, dirty);
  }, [id, dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange(id, false), [id, onDirtyChange]);

  const time = formatClock(event.device_ts);
  const unknownStep = stepId !== "" && !steps.some((s) => String(s.id) === stepId);

  return (
    <article className="panel-card review-card" aria-label={`${time} ${POLARITY_LABELS[event.polarity]}`}>
      <div className="review-card-head">
        <time>{time}</time>
        <span className={`record-pill record-pill-${event.polarity}`}>
          <PolarityIcon polarity={event.polarity} size={16} />
          {POLARITY_LABELS[event.polarity]}
        </span>
        <span className="unit-meta">
          {DEVICE_TYPE_LABELS[event.device_type]} · {VIA_LABELS[event.via]}
        </span>
        {event.pending && <span className="pending-tag">待同步</span>}
      </div>
      {event.note ? <p className="review-note">「{event.note}」</p> : <p className="field-hint">課中沒有加一句話。</p>}
      <EventControls
        event={event}
        onEdit={(change) => sync.enqueue(editOp(event.device_event_id, change))}
        onDelete={() => sync.enqueue(removeOp("delete", event.device_event_id))}
      />

      <fieldset className="review-fields" disabled={!server || saving}>
        <label className="field-label" htmlFor={`${ids}-step`}>
          活動歸屬（選填）
        </label>
        <select
          id={`${ids}-step`}
          value={stepId}
          onChange={(e) => {
            setStepId(e.target.value);
            touch();
          }}
        >
          <option value="">未指定</option>
          {steps.map((s) => (
            <option key={s.id} value={String(s.id)}>
              {stepLabel(s)}
            </option>
          ))}
          {unknownStep && <option value={stepId}>舊版教案的步驟</option>}
        </select>

        <fieldset className="review-choice">
          <legend className="field-label">學生可觀察行為（可複選，選填）</legend>
          <div className="pref-options">
            {STUDENT_BEHAVIORS.map((b) => (
              <label key={b} className="pref-option">
                <input
                  type="checkbox"
                  checked={behaviors.includes(b)}
                  onChange={(e) => {
                    setBehaviors((prev) => (e.target.checked ? [...prev, b] : prev.filter((x) => x !== b)));
                    touch();
                  }}
                />
                {BEHAVIOR_LABELS[b]}
              </label>
            ))}
          </div>
        </fieldset>

        <fieldset className="review-choice">
          <legend className="field-label">處理方式（選填）</legend>
          <div className="pref-options">
            <label className="pref-option">
              <input
                type="radio"
                name={`${ids}-handling`}
                checked={handling === ""}
                onChange={() => {
                  setHandling("");
                  touch();
                }}
              />
              未選
            </label>
            {HANDLINGS.map((h) => (
              <label key={h} className="pref-option">
                <input
                  type="radio"
                  name={`${ids}-handling`}
                  checked={handling === h}
                  onChange={() => {
                    setHandling(h);
                    touch();
                  }}
                />
                {HANDLING_LABELS[h]}
              </label>
            ))}
          </div>
        </fieldset>

        <label className="field-label" htmlFor={`${ids}-followup`}>
          補寫（選填）
        </label>
        <textarea
          id={`${ids}-followup`}
          rows={3}
          value={followup}
          maxLength={FOLLOWUP_MAX_CHARS}
          placeholder="當時發生了什麼？你怎麼判斷、怎麼處理？"
          onChange={(e) => {
            setFollowup(e.target.value);
            touch();
          }}
        />
        <p className="field-hint">
          {showPrivacyHint ? "不要寫學生姓名。" : ""}可以用手機鍵盤內建的語音輸入。
        </p>

        <div className="review-save-row">
          <button type="button" className="secondary-button" onClick={() => void save()} disabled={!dirty || saving}>
            {saving ? "儲存中…" : "儲存這張書籤"}
          </button>
          {saved && <span className="saved-label">● 已儲存</span>}
          {dirty && <span className="status-pill">尚未儲存</span>}
        </div>
      </fieldset>
      {!server && <p className="field-hint">這個書籤還沒同步，同步後才能補寫。</p>}
      <ErrorText message={error} />
    </article>
  );
}

/** 課後補寫：bookmarks in time order next to the lesson's latest plan steps. Everything optional. */
export function RunReviewPage() {
  const runId = Number(useParams().runId);
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const send = useCallback((ops: Parameters<typeof api.syncRunEvents>[1]) => api.syncRunEvents(runId, ops), [runId]);
  const [state, sync] = useRecordSync(runQueueScope(runId), send);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const shownLogged = useRef(false);

  // Cards with unsaved 補寫 (R2): saved before 完成課後補寫, and leaving asks first.
  const cards = useRef(new Map<string, ReviewCardHandle>());
  const [dirtyCards, setDirtyCards] = useState<ReadonlySet<string>>(() => new Set());
  const registerCard = useCallback((deviceEventId: string, handle: ReviewCardHandle | null) => {
    if (handle) cards.current.set(deviceEventId, handle);
    else cards.current.delete(deviceEventId);
  }, []);
  const onCardDirty = useCallback((deviceEventId: string, dirty: boolean) => {
    setDirtyCards((prev) => {
      if (prev.has(deviceEventId) === dirty) return prev;
      const next = new Set(prev);
      if (dirty) next.add(deviceEventId);
      else next.delete(deviceEventId);
      return next;
    });
  }, []);
  const guard = useLeaveGuard(dirtyCards.size > 0);

  /** Saves every card with unsaved 補寫. Resolves to an error message, or null when all saved. */
  async function savePendingCards(): Promise<string | null> {
    const failures: string[] = [];
    for (const id of dirtyCards) {
      const failed = (await cards.current.get(id)?.save()) ?? null;
      if (failed) failures.push(failed);
    }
    if (failures.length === 0) return null;
    return `${failures.length} 張書籤的補寫沒有存成功（${failures[0]}）`;
  }

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

  useEffect(() => {
    if (detail && !shownLogged.current) {
      shownLogged.current = true;
      logScaffold("SC-RECORD", "show", { trigger: "review" });
    }
  }, [detail]);

  /** 完成課後補寫: pending card edits are saved first; if one fails, nothing is completed. */
  async function complete() {
    setBusy(true);
    setActionError(null);
    const failed = await savePendingCards();
    if (failed) {
      setActionError(`${failed}，所以還沒完成課後補寫。請看該張書籤的訊息後再試一次。`);
      setBusy(false);
      return;
    }
    try {
      const run = await api.completeReview(runId);
      setDetail((d) => (d ? { ...d, ...run } : d));
    } catch (err) {
      setActionError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function endNow() {
    setBusy(true);
    setActionError(null);
    try {
      await sync.flush();
      const run = await api.endRun(runId);
      setDetail((d) => (d ? { ...d, ...run } : d));
    } catch (err) {
      setActionError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!detail) return <Loading />;

  const open = detail.ended_at === null;
  const steps = detail.plan?.steps ?? [];
  const events = state.events;

  return (
    <section aria-labelledby="review-heading">
      <p className="crumbs">
        {detail.class_id ? (
          <>
            <Link to="/">班級</Link> / <Link to={`/classes/${detail.class_id}`}>{detail.class_name}</Link>
          </>
        ) : (
          <Link to="/record">上課紀錄</Link>
        )}{" "}
        / <Link to={`/lessons/${detail.lesson_id}`}>{detail.lesson_title}</Link> / 課後補寫
      </p>
      <div className="page-head">
        <p className="kicker">
          課後補寫 · {detail.class_name ? `${detail.class_name} · ` : ""}
          {formatDateTime(detail.started_at)}
          {detail.ended_at ? ` – ${formatClock(detail.ended_at, false)}` : "（進行中）"}
        </p>
        <h1 id="review-heading">{detail.lesson_title}</h1>
        <p className="lead">
          書籤依時間排列，旁邊是這節課的教案步驟。每張書籤的欄位都可以留白；沒有書籤，也可以直接完成。
        </p>
      </div>

      {open && (
        <Notice tone="warn">
          本節課還在進行中。<Link to={`/runs/${runId}`}>回到課堂</Link>
          <button type="button" className="ghost-button small-button review-end" onClick={() => void endNow()} disabled={busy}>
            結束本節課
          </button>
        </Notice>
      )}

      <SyncStatus state={state} onRetry={() => void sync.retry()} />

      <div className="review-layout">
        <aside className="review-plan" aria-labelledby="plan-steps-heading">
          <div className="card-heading">
            <h2 id="plan-steps-heading">教案步驟</h2>
            {detail.plan && <span className="quiet-note">第 {detail.plan.version_no} 版</span>}
          </div>
          {detail.plan?.objective && <p className="review-objective">目標：{detail.plan.objective}</p>}
          {steps.length === 0 ? (
            <p className="empty-preview">這節課還沒有教案步驟，活動歸屬可以留白。</p>
          ) : (
            <ol className="review-steps">
              {steps.map((s) => (
                <li key={s.id}>
                  <span className="route-step-number">{s.position}</span>
                  <div>
                    <strong>{s.activity_type || "（未填活動型態）"}</strong>
                    {s.content && <p>{s.content}</p>}
                  </div>
                </li>
              ))}
            </ol>
          )}
        </aside>

        <div className="review-cards">
          <div className="card-heading">
            <h2>書籤</h2>
            <span className="soft-tag">
              順利 {events.filter((e) => e.polarity === "good").length} · 卡住{" "}
              {events.filter((e) => e.polarity === "stuck").length}
            </span>
          </div>
          {events.length === 0 ? (
            <p className="empty-preview">這節課沒有書籤。沒有書籤也沒關係，可以直接完成。</p>
          ) : (
            events.map((ev, index) => (
              <ReviewCard
                showPrivacyHint={index === 0}
                key={ev.device_event_id}
                runId={runId}
                event={ev}
                steps={steps}
                sync={sync}
                register={registerCard}
                onDirtyChange={onCardDirty}
              />
            ))
          )}
        </div>
      </div>

      <section className="review-complete" aria-label="完成課後補寫">
        {detail.review_completed_at ? (
          <Notice tone="success">
            已在 {formatDateTime(detail.review_completed_at)} 完成課後補寫。之後仍可回來修改。
          </Notice>
        ) : (
          <div className="button-row">
            <button type="button" className="primary-button" onClick={() => void complete()} disabled={busy || open}>
              {busy && dirtyCards.size > 0 ? "儲存並完成中…" : "完成課後補寫"}
            </button>
          </div>
        )}
        {dirtyCards.size > 0 && (
          <p className="field-hint pending-review-hint" role="status">
            有 {dirtyCards.size} 張書籤的補寫還沒儲存
            {detail.review_completed_at ? "，請按該張書籤的「儲存這張書籤」。" : "；按「完成課後補寫」會先把它們存起來。"}
          </p>
        )}
        <p className="field-hint">
          沒有填寫的欄位都可以留白，不會影響完成。
        </p>
        <ErrorText message={actionError} />
      </section>

      <LeaveConfirm guard={guard} message="離開這頁，還沒儲存的書籤補寫就會遺失。" onSave={savePendingCards} />
    </section>
  );
}
