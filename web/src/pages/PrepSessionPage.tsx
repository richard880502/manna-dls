import { useCallback, useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreatePrepTurnRequest, PrepSession, PrepTurn, UpdatePrepSessionRequest } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { ClassChip, TemplateChip, useActiveClasses } from "../components/prep/ContextChips";
import { KitDownload } from "../components/prep/KitDownload";
import { ActivityCard, AskCard, ResourcesCard, ReviseCard } from "../components/prep/TurnCards";
import { WorksheetCard } from "../components/prep/WorksheetCard";
import { useReadyTemplates } from "../components/template/TemplateSelect";
import {
  AI_DISCLAIMER,
  DELETE_CONFIRM,
  FOLLOW_PLACEHOLDER,
  KIND_LABEL,
  NEXT_LABEL,
  REVISE_PLACEHOLDER,
  WORKING_LINE,
  activityOf,
  answerOf,
  hasWorksheet,
  isNextKind,
  isWorksheetKind,
  prepConfig,
  replyOf,
  resourcesOf,
  sessionTitle,
} from "../lib/prep";

/** How long changed blocks stay highlighted after a revision. */
const FLASH_MS = 4000;

export function PrepSessionPage() {
  const { sessionId } = useParams();
  const id = Number(sessionId);
  if (!Number.isInteger(id) || id <= 0) return <ErrorText message="找不到這次備課對話。" />;
  return <PrepSessionView key={id} id={id} />;
}

/** For each worksheet turn, the revise turns that apply to it (the latest worksheet before them). */
function reviseTargets(turns: PrepTurn[]): Map<number, number> {
  const target = new Map<number, number>();
  let current: number | null = null;
  for (const t of turns) {
    if (isWorksheetKind(t.kind) && t.worksheet) current = t.worksheet.id;
    if (t.kind === "revise" && current !== null) target.set(t.id, current);
  }
  return target;
}

function PrepSessionView({ id }: { id: number }) {
  const ids = useId();
  const navigate = useNavigate();
  const classes = useActiveClasses();
  const templates = useReadyTemplates();
  const [session, setSession] = useState<PrepSession | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  // With a worksheet in the session the bar edits it by default; 問 Manna asks a question instead.
  const [barMode, setBarMode] = useState<"revise" | "ask">("revise");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [headError, setHeadError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [viewing, setViewing] = useState<number | null>(null);
  const [labels, setLabels] = useState<Record<number, Record<string, string>>>({});
  const [flash, setFlash] = useState<{ worksheetId: number; ids: ReadonlySet<string> } | null>(null);
  const previous = useRef<PrepSession | null>(null);
  // A turn that appeared after the page loaded is scrolled into view, so the teacher sees it start.
  const [arrived, setArrived] = useState<number | null>(null);

  const apply = useCallback((next: PrepSession) => {
    // A revision that just finished: its blocks light up for a moment in the worksheet above.
    const before = previous.current;
    if (before) {
      const fresh = next.turns.find((t) => !before.turns.some((p) => p.id === t.id));
      if (fresh) setArrived(fresh.id);
      const targets = reviseTargets(next.turns);
      for (const t of next.turns) {
        const was = before.turns.find((p) => p.id === t.id);
        const reply = replyOf(t);
        const wsId = targets.get(t.id);
        if (t.status === "done" && was?.status !== "done" && reply && reply.changed_block_ids.length > 0 && wsId) {
          setFlash({ worksheetId: wsId, ids: new Set(reply.changed_block_ids) });
        }
      }
    }
    previous.current = next;
    setSession(next);
  }, []);

  useEffect(() => {
    let alive = true;
    api
      .getPrepSession(id)
      .then((s) => alive && apply(s))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [id, apply]);

  // While a turn runs, ask again every couple of seconds; stop as soon as nothing runs.
  useEffect(() => {
    if (!session?.running) return undefined;
    let alive = true;
    const timer = setTimeout(() => {
      api
        .getPrepSession(id)
        .then((s) => alive && apply(s))
        .catch((err: unknown) => alive && setSendError(errorMessage(err)));
    }, prepConfig.pollMs);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [id, session, apply]);

  useEffect(() => {
    if (arrived === null) return;
    document.getElementById(`prep-turn-${arrived}`)?.scrollIntoView?.({ behavior: "smooth", block: "start" });
  }, [arrived]);

  useEffect(() => {
    if (!flash) return undefined;
    const timer = setTimeout(() => setFlash(null), FLASH_MS);
    return () => clearTimeout(timer);
  }, [flash]);

  const onLabels = useCallback((worksheetId: number, map: Record<string, string>) => {
    setLabels((prev) => ({ ...prev, [worksheetId]: map }));
  }, []);

  if (loadError && !session) {
    return (
      <>
        <ErrorText message={loadError} />
        <p className="after-rows">
          <Link to="/prep">回我的備課</Link>
        </p>
      </>
    );
  }
  if (!session) return <Loading />;

  const running = session.running;
  const locked = running || sending;
  const targets = reviseTargets(session.turns);
  const next = session.next.filter(isNextKind);

  async function send(body: CreatePrepTurnRequest): Promise<boolean> {
    setSending(true);
    setSendError(null);
    try {
      apply(await api.createPrepTurn(id, body));
      return true;
    } catch (err) {
      setSendError(errorMessage(err));
      return false;
    } finally {
      setSending(false);
    }
  }

  async function sendDraft(event?: FormEvent) {
    event?.preventDefault();
    const text = draft.trim();
    if (!text || locked) return;
    // No kind: the server revises the worksheet if there is one, otherwise answers.
    const body: CreatePrepTurnRequest =
      session && hasWorksheet(session) && barMode === "ask" ? { kind: "ask", request: text } : { request: text };
    if (await send(body)) setDraft("");
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void sendDraft();
  }

  async function patch(body: UpdatePrepSessionRequest) {
    setHeadError(null);
    try {
      apply(await api.updatePrepSession(id, body));
      return true;
    } catch (err) {
      setHeadError(errorMessage(err));
      return false;
    }
  }

  async function remove() {
    setHeadError(null);
    try {
      await api.deletePrepSession(id);
      navigate("/prep");
    } catch (err) {
      setHeadError(errorMessage(err));
    }
  }

  // The session's class or template may no longer be among the teacher's active ones; keep it listed.
  const classOptions = [
    ...(classes ?? []),
    ...(session.class && !(classes ?? []).some((c) => c.id === session.class?.id) ? [session.class] : []),
  ];

  function worksheetCard(turn: PrepTurn) {
    const wsId = turn.worksheet?.id;
    const lastRevise = [...session!.turns].reverse().find((t) => t.kind === "revise" && targets.get(t.id) === wsId);
    return (
      <WorksheetCard
        turn={turn}
        refreshKey={`${turn.worksheet?.version_no ?? 0}-${lastRevise?.id ?? 0}-${lastRevise?.status ?? ""}`}
        highlight={flash && flash.worksheetId === wsId ? flash.ids : EMPTY}
        onLabels={onLabels}
      />
    );
  }

  function result(turn: PrepTurn) {
    // 教學套件: the activity plan and the tiered worksheet, one download for both.
    if (turn.kind === "kit") {
      const kitPlan = activityOf(turn);
      return (
        <div className="prep-kit">
          {turn.status !== "failed" && (
            <KitDownload sessionId={id} turnId={turn.id} ready={turn.status === "done"} />
          )}
          {kitPlan ? (
            <ActivityCard plan={kitPlan} interactionId={turn.interaction_id} />
          ) : (
            turn.status === "running" && (
              <p className="manna-label prep-working" role="status">
                <span className="star is-working" aria-hidden="true" />
                Manna 正在規劃教學活動…
              </p>
            )
          )}
          {(turn.worksheet || turn.status === "running") && worksheetCard(turn)}
          {turn.status === "failed" && failed(turn)}
        </div>
      );
    }
    if (isWorksheetKind(turn.kind) && (turn.status !== "failed" || turn.worksheet)) {
      return (
        <>
          {worksheetCard(turn)}
          {turn.status === "failed" && failed(turn)}
        </>
      );
    }
    if (turn.status === "failed") return failed(turn);
    if (turn.status === "running") {
      return (
        <p className="manna-label prep-working" role="status">
          <span className="star is-working" aria-hidden="true" />
          {WORKING_LINE}
        </p>
      );
    }
    const plan = activityOf(turn);
    if (plan) return <ActivityCard plan={plan} interactionId={turn.interaction_id} />;
    const resources = resourcesOf(turn);
    if (resources) return <ResourcesCard resources={resources} onOpenMaterial={setViewing} />;
    const answer = answerOf(turn);
    if (answer) {
      return (
        <AskCard
          result={answer}
          interactionId={turn.interaction_id}
          disabled={locked}
          onFollowUp={(q) => void send({ kind: "ask", request: q })}
        />
      );
    }
    const reply = replyOf(turn);
    if (reply) {
      const wsId = targets.get(turn.id);
      return (
        <ReviseCard result={reply} labels={wsId ? labels[wsId] : undefined} interactionId={turn.interaction_id} />
      );
    }
    return <p className="quiet-note">沒有內容。</p>;
  }

  function failed(turn: PrepTurn) {
    return (
      <div className="prep-failed" role="alert">
        <p>{turn.error || "這一步沒有成功。"}</p>
        <button
          type="button"
          className="secondary-button small-button"
          disabled={locked}
          onClick={() => void send({ kind: turn.kind, request: turn.request })}
        >
          再試一次
        </button>
      </div>
    );
  }

  return (
    <div className="prep-session">
      <div className="page-head prep-head">
        <p className="crumbs">
          <Link to="/prep">我的備課</Link>
        </p>
        {renaming === null ? (
          <div className="prep-title-row">
            <h1>{sessionTitle(session)}</h1>
            <button type="button" className="text-link is-plain" onClick={() => setRenaming(session.title)}>
              改名
            </button>
          </div>
        ) : (
          <form
            className="prep-rename"
            onSubmit={(e) => {
              e.preventDefault();
              const title = renaming.trim();
              if (!title || title === session.title) {
                setRenaming(null);
                return;
              }
              void patch({ title }).then((ok) => ok && setRenaming(null));
            }}
          >
            <label className="sr-only" htmlFor={`${ids}-title`}>
              備課標題
            </label>
            <input
              id={`${ids}-title`}
              type="text"
              maxLength={200}
              value={renaming}
              onChange={(e) => setRenaming(e.target.value)}
            />
            <button type="submit" className="secondary-button small-button">
              儲存
            </button>
            <button type="button" className="text-link is-plain" onClick={() => setRenaming(null)}>
              取消
            </button>
          </form>
        )}

        <div className="prep-context" role="group" aria-label="備課情境">
          {session.material ? (
            <span className="prep-context-item">
              教材：
              <button type="button" className="text-link" onClick={() => setViewing(session.material!.id)}>
                {session.material.title.trim() || "未命名教材"}
              </button>
            </span>
          ) : session.topic ? (
            <span className="prep-context-item">主題：{session.topic}</span>
          ) : null}
          {classOptions.length > 0 && (
            <ClassChip
              classes={classOptions}
              value={session.class?.id ?? null}
              disabled={locked}
              onChange={(classId) => void patch({ class_id: classId })}
            />
          )}
          {templates && templates.length > 0 ? (
            <TemplateChip
              id={`${ids}-template`}
              templates={templates}
              value={session.template?.id ?? null}
              disabled={locked}
              onChange={(templateId) => void patch({ template_id: templateId })}
            />
          ) : session.template ? (
            <span className="prep-context-item">照我的版型：{session.template.title}</span>
          ) : null}
          <span className="prep-delete">
            {confirmDelete ? (
              <span role="group" aria-label="確認刪除">
                {DELETE_CONFIRM}
                <button type="button" className="text-link is-danger" onClick={() => void remove()}>
                  確定刪除
                </button>
                <button type="button" className="text-link is-plain" onClick={() => setConfirmDelete(false)}>
                  不刪
                </button>
              </span>
            ) : (
              <button type="button" className="text-link is-plain" onClick={() => setConfirmDelete(true)}>
                刪除
              </button>
            )}
          </span>
        </div>
        <ErrorText message={headError} />
      </div>

      <ol className="prep-turns" aria-label="備課對話">
        {session.turns.map((turn) => (
          <li key={turn.id} id={`prep-turn-${turn.id}`} className="prep-turn">
            <p className="prep-request">
              <span className="sr-only">你：</span>
              <span className="prep-bubble">{turn.request}</span>
            </p>
            <section className="prep-result" aria-label={`${KIND_LABEL[turn.kind]}（第 ${turn.seq} 則）`}>
              {result(turn)}
            </section>
          </li>
        ))}
      </ol>

      {next.length > 0 && (
        <div className="prep-next">
          <p className="prep-next-label">接下來可以：</p>
          <div className="prep-chips" role="group" aria-label="接下來可以">
            {next.map((kind) => (
              <button
                key={kind}
                type="button"
                className="prep-chip"
                disabled={locked}
                onClick={() => void send({ kind, request: NEXT_LABEL[kind] })}
              >
                {NEXT_LABEL[kind]}
              </button>
            ))}
          </div>
        </div>
      )}

      <form className="prep-bottom" onSubmit={(e) => void sendDraft(e)}>
        <ErrorText message={sendError} />
        {hasWorksheet(session) && (
          <div className="ws-seg prep-bar-mode" role="radiogroup" aria-label="要改學習單還是問問題">
            {(
              [
                ["revise", "改學習單"],
                ["ask", "問 Manna"],
              ] as const
            ).map(([mode, label]) => (
              <label key={mode}>
                <input
                  type="radio"
                  name={`${ids}-mode`}
                  checked={barMode === mode}
                  onChange={() => setBarMode(mode)}
                />
                <span>{label}</span>
              </label>
            ))}
          </div>
        )}
        <div className="prep-bottom-row">
          <label className="sr-only" htmlFor={`${ids}-draft`}>
            給 Manna 的話
          </label>
          <textarea
            id={`${ids}-draft`}
            rows={2}
            maxLength={2000}
            value={draft}
            disabled={locked}
            placeholder={
              running
                ? WORKING_LINE
                : hasWorksheet(session) && barMode === "revise"
                  ? REVISE_PLACEHOLDER
                  : FOLLOW_PLACEHOLDER
            }
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
          />
          <button type="submit" className="primary-button" disabled={locked || draft.trim() === ""}>
            送出
          </button>
        </div>
        {running && (
          <p className="manna-label prep-bottom-status" role="status">
            <span className="star is-working" aria-hidden="true" />
            {WORKING_LINE}
          </p>
        )}
        <p className="prep-disclaimer">{AI_DISCLAIMER}</p>
      </form>

      {viewing !== null && (
        <MaterialViewer mode="panel" materialId={viewing} open onClose={() => setViewing(null)} />
      )}
    </div>
  );
}

const EMPTY: ReadonlySet<string> = new Set();
