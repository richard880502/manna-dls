import { useCallback, useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type {
  CreatePrepTurnRequest,
  PrepSession,
  PrepTurn,
  UpdatePrepSessionRequest,
  WsTemplateSummary,
} from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { RunProgressPanel, useRunPanel } from "../components/RunProgress";
import { TOOL_NAME } from "../lib/create";
import { MaterialViewer } from "../components/material/MaterialViewer";
import {
  LongMaterialNotice,
  MaterialFixButtons,
  MaterialTextDialog,
  ParagraphPickerDialog,
} from "../components/material/LongMaterial";
import {
  BACK_STEP,
  CHANGE_MATERIAL,
  EDIT_TEXT,
  PICK_PARAGRAPHS,
  REGENERATE,
  RETRY,
  draftFromSession,
  isLongMaterial,
  loadOrigin,
  selectedLine,
  type HomeRestore,
} from "../lib/longMaterial";
import { ClassChip, TemplateChip, useActiveClasses } from "../components/prep/ContextChips";
import { KitDownload } from "../components/prep/KitDownload";
import { SiopCheckPanel } from "../components/prep/SiopCheckPanel";
import { IMAGE_PRIVACY_NOTE } from "../lib/prepImages";
import { AttachButton, AttachedThumbs, PrepImageThumbs, useImageAttach, useImageDrop } from "../components/prep/ImageAttach";
import { ActivityCard, AskCard, ResourcesCard, ReviseCard } from "../components/prep/TurnCards";
import { WorksheetCard } from "../components/prep/WorksheetCard";
import { useReadyTemplates } from "../components/template/TemplateSelect";
import {
  AI_DISCLAIMER,
  DELETE_CONFIRM,
  PREP_SESSIONS_CHANGED,
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
  // A taller composer for long instructions (the expand corner, as in chat composers).
  const [composerTall, setComposerTall] = useState(false);
  const [draft, setDraft] = useState("");
  // With a worksheet in the session the bar edits it by default; 問 Manna asks a question instead.
  const [barMode, setBarMode] = useState<"revise" | "ask">("revise");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [headError, setHeadError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [viewing, setViewing] = useState<number | null>(null);
  const [labels, setLabels] = useState<Record<number, Record<string, string>>>({});
  const [flash, setFlash] = useState<{ worksheetId: number; ids: ReadonlySet<string> } | null>(null);
  // 「改這一區」: the bottom bar revises one block of one worksheet instead of the whole worksheet.
  const [target, setTarget] = useState<{ worksheetId: number; blockId: string; name: string } | null>(null);
  const [proposalsKey, setProposalsKey] = useState<Record<number, number>>({});
  const draftRef = useRef<HTMLTextAreaElement>(null);
  // 修改課文 / 只選要用的段落 open here; `materialChanged`: the next try uses what was changed.
  const [fix, setFix] = useState<"edit" | "pick" | null>(null);
  const [materialChanged, setMaterialChanged] = useState(false);
  const [longSeen, setLongSeen] = useState(false);
  const attach = useImageAttach(id);
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

  // While a turn runs, ask again every couple of seconds; stop as soon as nothing runs. The last
  // turn's progress may stay open a little longer (its pictures, its SIOP 自我檢核): follow it too.
  const lastTurn = session?.turns[session.turns.length - 1];
  const following = !!session?.running || lastTurn?.progress?.status === "running";
  // 「進度」 sits in the conversation, in the last turn's place, instead of its working line, and
  // folds away once the run is done.
  const lastPanel = useRunPanel(lastTurn?.progress, lastTurn ? String(lastTurn.id) : null);
  useEffect(() => {
    if (!following) return undefined;
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
  }, [id, session, following, apply]);

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

  // Pictures go only with a revision of the worksheet (not a question to Manna).
  const canAttach = session !== null && hasWorksheet(session) && barMode === "revise";
  const drop = useImageDrop(canAttach && !(session?.running || sending), attach.add);

  if (loadError && !session) {
    return (
      <>
        <ErrorText message={loadError} />
        <p className="after-rows">
          <Link to="/prep">回我的作品</Link>
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

  /** 回上一步: the home box, filled in as it was when this conversation began. Nothing is lost. */
  function goBack() {
    if (!session) return;
    const restore = loadOrigin(id) ?? draftFromSession(session);
    navigate("/", { state: { restore } satisfies HomeRestore });
  }

  /** The material's text was edited: its paragraphs may be others now, so the selection is checked. */
  async function afterEdit(detail: { paragraphs: Array<{ id: string }> }) {
    const now = session?.options?.paragraph_ids ?? [];
    const still = now.filter((pid) => detail.paragraphs.some((p) => p.id === pid));
    setMaterialChanged(true);
    if (now.length > 0 && still.length !== now.length) {
      apply(await api.putPrepMaterialSelection(id, still.length > 0 ? still : null));
    } else {
      apply(await api.getPrepSession(id));
    }
  }

  async function afterPick(ids: string[] | null) {
    apply(await api.putPrepMaterialSelection(id, ids));
    setMaterialChanged(true);
  }

  async function reviseTarget(t: NonNullable<typeof target>, instruction: string) {
    setSending(true);
    setSendError(null);
    try {
      await api.reviseWorksheetBlock(t.worksheetId, t.blockId, {
        instruction,
        ...(attach.imageIds.length > 0 ? { image_ids: attach.imageIds } : {}),
      });
      attach.clear();
      setProposalsKey((prev) => ({ ...prev, [t.worksheetId]: (prev[t.worksheetId] ?? 0) + 1 }));
      setTarget(null);
      setDraft("");
      document.getElementById(`sheet-block-${t.blockId}`)?.scrollIntoView?.({ behavior: "smooth", block: "center" });
    } catch (err) {
      setSendError(errorMessage(err));
    } finally {
      setSending(false);
    }
  }

  async function sendDraft(event?: FormEvent) {
    event?.preventDefault();
    const text = draft.trim();
    if (!text || locked || (canAttach && attach.uploading)) return;
    if (target) {
      await reviseTarget(target, text);
      return;
    }
    // No kind: the server revises the worksheet if there is one, otherwise answers.
    const body: CreatePrepTurnRequest =
      session && hasWorksheet(session) && barMode === "ask"
        ? { kind: "ask", request: text }
        : { request: text, ...(attach.imageIds.length > 0 ? { image_ids: attach.imageIds } : {}) };
    if (await send(body)) {
      setDraft("");
      attach.clear();
    }
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
      window.dispatchEvent(new Event(PREP_SESSIONS_CHANGED));
      navigate("/prep");
    } catch (err) {
      setHeadError(errorMessage(err));
    }
  }

  // The session's class or template may no longer be among the teacher's active ones (or the
  // template was made from the teacher's worksheet after this page loaded); keep it listed.
  const classOptions = [
    ...(classes ?? []),
    ...(session.class && !(classes ?? []).some((c) => c.id === session.class?.id) ? [session.class] : []),
  ];
  const templateOptions: WsTemplateSummary[] =
    templates && templates.length > 0 && session.template && !templates.some((t) => t.id === session.template?.id)
      ? [
          ...templates,
          {
            ...session.template,
            status: "ready",
            section_count: 0,
            source_material: null,
            version_no: null,
            updated_at: session.updated_at,
          },
        ]
      : (templates ?? []);

  /** Whether this turn's 「進度」 panel shows (then its own working lines stay quiet). */
  function stepsShown(turn: PrepTurn) {
    return turn.id === lastTurn?.id && lastPanel.visible && !!turn.progress;
  }

  function worksheetCard(turn: PrepTurn) {
    const wsId = turn.worksheet?.id;
    const lastRevise = [...session!.turns].reverse().find((t) => t.kind === "revise" && targets.get(t.id) === wsId);
    return (
      <WorksheetCard
        turn={turn}
        stepsShown={stepsShown(turn)}
        refreshKey={`${turn.worksheet?.version_no ?? 0}-${lastRevise?.id ?? 0}-${lastRevise?.status ?? ""}`}
        highlight={flash && flash.worksheetId === wsId ? flash.ids : EMPTY}
        onLabels={onLabels}
        target={target && target.worksheetId === wsId ? target.blockId : null}
        onTarget={(worksheetId, blockId, name) => {
          setTarget({ worksheetId, blockId, name });
          setBarMode("revise");
          draftRef.current?.focus();
        }}
        proposalsKey={wsId ? proposalsKey[wsId] : undefined}
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
            turn.status === "running" &&
            !stepsShown(turn) && (
              <p className="manna-label prep-working" role="status">
                <span className="star is-working" aria-hidden="true" />
                Manna 正在規劃教學活動…
              </p>
            )
          )}
          {(turn.worksheet || turn.status === "running") && worksheetCard(turn)}
          {turn.status === "failed" && failed(turn)}
          <SiopCheckPanel sessionId={id} turn={turn} quiet={stepsShown(turn)} />
        </div>
      );
    }
    if (isWorksheetKind(turn.kind) && (turn.status !== "failed" || turn.worksheet)) {
      return (
        <>
          {worksheetCard(turn)}
          {turn.status === "failed" && failed(turn)}
          <SiopCheckPanel sessionId={id} turn={turn} quiet={stepsShown(turn)} />
        </>
      );
    }
    if (turn.status === "failed") return failed(turn);
    if (turn.status === "running") {
      if (stepsShown(turn)) return null;
      return (
        <p className="manna-label prep-working" role="status">
          <span className="star is-working" aria-hidden="true" />
          {WORKING_LINE}
        </p>
      );
    }
    const plan = activityOf(turn);
    if (plan) {
      return (
        <>
          <ActivityCard plan={plan} interactionId={turn.interaction_id} />
          <SiopCheckPanel sessionId={id} turn={turn} quiet={stepsShown(turn)} />
        </>
      );
    }
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
    const hasMaterial = !!session!.material;
    return (
      <div className="prep-failed" role="alert">
        <p>{turn.error || "這一步沒有成功。"}</p>
        {materialChanged && <p className="quiet-note">課文已更新，按「{REGENERATE}」就會用新的課文。</p>}
        <MaterialFixButtons
          disabled={locked}
          onBack={goBack}
          onEdit={hasMaterial ? () => setFix("edit") : undefined}
          onPick={hasMaterial && (session!.material_paragraphs ?? 0) > 1 ? () => setFix("pick") : undefined}
          onRetry={() => void send({ kind: turn.kind, request: turn.request })}
          retryLabel={materialChanged ? REGENERATE : RETRY}
        />
      </div>
    );
  }

  return (
    <div className={`prep-session${viewing !== null ? " with-viewer" : ""}`}>
      <div className="page-head prep-head">
        <p className="crumbs">
          <Link to="/prep">我的作品</Link>
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
              {typeof session.material_selected === "number" && typeof session.material_paragraphs === "number" && (
                <span className="prep-selected"> · {selectedLine(session.material_selected, session.material_paragraphs)}</span>
              )}
              <button type="button" className="text-link is-plain" disabled={locked} onClick={() => setFix("edit")}>
                {EDIT_TEXT}
              </button>
              {(session.material_paragraphs ?? 0) > 1 && (
                <button type="button" className="text-link is-plain" disabled={locked} onClick={() => setFix("pick")}>
                  {typeof session.material_selected === "number" ? "改選段落" : PICK_PARAGRAPHS}
                </button>
              )}
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
          {templateOptions.length > 0 ? (
            <TemplateChip
              id={`${ids}-template`}
              templates={templateOptions}
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
            <PrepImageThumbs sessionId={id} images={turn.images} />
            <section className="prep-result" aria-label={`${KIND_LABEL[turn.kind]}（第 ${turn.seq} 則）`}>
              {stepsShown(turn) && turn.progress && (
                <RunProgressPanel
                  progress={turn.progress}
                  error={turn.error}
                  onClose={lastPanel.close}
                  className="prep-turn-progress"
                />
              )}
              {result(turn)}
            </section>
          </li>
        ))}
      </ol>

      {session.material &&
        !longSeen &&
        !running &&
        session.material_selected == null &&
        isLongMaterial(session.material_paragraphs) && (
          <LongMaterialNotice
            paragraphs={session.material_paragraphs!}
            disabled={locked}
            onEdit={() => setFix("edit")}
            onPick={() => setFix("pick")}
            onGo={() => setLongSeen(true)}
          />
        )}

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
                {kind === "worksheet" ? TOOL_NAME.worksheet : NEXT_LABEL[kind]}
              </button>
            ))}
          </div>
        </div>
      )}

      <form className="prep-bottom" onSubmit={(e) => void sendDraft(e)}>
        <ErrorText message={sendError} />
        {target && (
          <p className="prep-target" role="status">
            <span className="prep-target-chip">
              針對 {target.name}
              <button
                type="button"
                className="text-link is-plain"
                aria-label={`不要只改 ${target.name}`}
                onClick={() => setTarget(null)}
              >
                ✕
              </button>
            </span>
            <span className="field-hint">只改這一區；Manna 改好後先給你看，按「採用」才會寫入學習單。</span>
          </p>
        )}
        {/* One rounded box, like the chat composers teachers already know: the text on top, the
            mode switch bottom-left, the send button bottom-right. */}
        <div
          className="prep-composer"
          data-disabled={locked || undefined}
          data-tall={composerTall || undefined}
          data-dragging={(canAttach && drop.dragging) || undefined}
          onDragEnter={drop.onDragEnter}
          onDragOver={drop.onDragOver}
          onDragLeave={drop.onDragLeave}
          onDrop={drop.onDrop}
        >
          <label className="sr-only" htmlFor={`${ids}-draft`}>
            給 Manna 的話
          </label>
          <button
            type="button"
            className="prep-composer-size"
            aria-pressed={composerTall}
            aria-label={composerTall ? "縮小輸入框" : "放大輸入框"}
            title={composerTall ? "縮小輸入框" : "放大輸入框"}
            onClick={() => {
              setComposerTall((tall) => !tall);
              draftRef.current?.focus();
            }}
          >
            <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
              <path
                d={
                  composerTall
                    ? "M4 14h6v6M20 10h-6V4M10 14l-6 6M14 10l6-6"
                    : "M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"
                }
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
          {canAttach && <AttachedThumbs attach={attach} />}
          {canAttach && <ErrorText message={attach.error} />}
          <textarea
            id={`${ids}-draft`}
            ref={draftRef}
            rows={3}
            maxLength={2000}
            value={draft}
            disabled={locked}
            placeholder={
              running
                ? WORKING_LINE
                : target
                  ? `想怎麼改 ${target.name}？例如：第 2 題太難，改簡單一點`
                  : hasWorksheet(session) && barMode === "revise"
                    ? REVISE_PLACEHOLDER
                    : FOLLOW_PLACEHOLDER
            }
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
            onPaste={drop.onPaste}
          />
          <div className="prep-composer-tools">
            {canAttach && <AttachButton onFiles={attach.add} disabled={locked} />}
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
                      onChange={() => {
                        setBarMode(mode);
                        if (mode === "ask") setTarget(null);
                      }}
                    />
                    <span>{label}</span>
                  </label>
                ))}
              </div>
            )}
            <span className="prep-composer-hint" aria-hidden="true">
              Enter 送出・Shift＋Enter 換行
            </span>
            <button
              type="submit"
              className="prep-send"
              aria-label="送出"
              title="送出（Enter）"
              disabled={locked || draft.trim() === "" || (canAttach && attach.uploading)}
            >
              <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false">
                <path
                  d="M12 19V5M5.5 11.5 12 5l6.5 6.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>
          </div>
        </div>
        {canAttach && (
          <p className="prep-attach-note">{IMAGE_PRIVACY_NOTE}</p>
        )}
        <p className="prep-disclaimer">{AI_DISCLAIMER}</p>
      </form>

      {viewing !== null && (
        <MaterialViewer mode="panel" materialId={viewing} open onClose={() => setViewing(null)} />
      )}
      {fix === "edit" && session.material && (
        <MaterialTextDialog
          materialId={session.material.id}
          from="prep_session"
          onClose={() => setFix(null)}
          onSaved={afterEdit}
          otherWay={{ label: `${BACK_STEP}，${CHANGE_MATERIAL}`, onClick: goBack }}
        />
      )}
      {fix === "pick" && session.material && (
        <ParagraphPickerDialog
          materialId={session.material.id}
          initial={session.options?.paragraph_ids ?? null}
          onClose={() => setFix(null)}
          onSave={afterPick}
        />
      )}
    </div>
  );
}

const EMPTY: ReadonlySet<string> = new Set();
