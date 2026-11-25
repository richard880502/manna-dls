import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { Icon } from "../components/Icon";
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
import { useConfirmFocus } from "../lib/useConfirmFocus";
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
import { ClassChip, NO_CLASS_LABEL, TemplateChip, useActiveClasses } from "../components/prep/ContextChips";
import { KitDownload } from "../components/prep/KitDownload";
import { FOLDED_COLUMNS, useSplitResize } from "../components/prep/useSplitResize";
import { CHAT_COLLAPSED_KEY, HEAD_OPEN_KEY, useStoredFlag } from "../components/prep/useStoredFlag";
import { SiopCheckPanel } from "../components/prep/SiopCheckPanel";
import { IMAGE_PRIVACY_NOTE } from "../lib/prepImages";
import { scrollToEl, scrollToIdWhenReady, whenVisible } from "../lib/scroll";
import { AttachButton, AttachedThumbs, PrepImageThumbs, useImageAttach, useImageDrop } from "../components/prep/ImageAttach";
import { ActivityCard, AskCard, ChatCard, ResourcesCard, ReviseCard, WorksheetTurnCard } from "../components/prep/TurnCards";
import {
  SheetEditor,
  SheetModeToggle,
  loadSheetMode,
  saveSheetMode,
  type SheetFlush,
  type SheetMode,
} from "../components/prep/SheetEditPane";
import { WorksheetCard } from "../components/prep/WorksheetCard";
import { MemorySuggestionCards, habitWordsSent, useMemorySuggestions } from "../components/memory/MemorySuggestions";
import { useReadyTemplates } from "../components/template/TemplateSelect";
import {
  AI_DISCLAIMER,
  DELETE_CONFIRM,
  PREP_SESSIONS_CHANGED,
  KIND_LABEL,
  NEXT_LABEL,
  CHAT_PLACEHOLDER,
  WORKING_LINE,
  activityOf,
  answerOf,
  chatResult,
  hasWorksheet,
  isNextKind,
  isWorksheetKind,
  prepConfig,
  replyOf,
  resourcesOf,
  sessionTitle,
} from "../lib/prep";

/** Below this width the conversation and the worksheet are two tabs of one column (prep.css matches). */
const NARROW_QUERY = "(max-width: 68.6875rem)";

function useNarrow(): boolean {
  const read = () => {
    try {
      return typeof window.matchMedia === "function" && window.matchMedia(NARROW_QUERY).matches;
    } catch {
      return false;
    }
  };
  const [narrow, setNarrow] = useState(read);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const query = window.matchMedia(NARROW_QUERY);
    const on = () => setNarrow(query.matches);
    on();
    query.addEventListener?.("change", on);
    return () => query.removeEventListener?.("change", on);
  }, []);
  return narrow;
}

/** The worksheet preview's block, scrolled into view (the preview's blocks carry `sheet-block-<id>`). */
function scrollToBlock(blockId: string) {
  scrollToEl(document.getElementById(`sheet-block-${blockId}`), "center");
}

/** How long changed blocks stay highlighted after a revision. */
const FLASH_MS = 4000;
const FLASH_FALLBACK_MS = 60_000;

/** The two panes below the split width, in tab order. */
const SPLIT_TABS = [
  ["chat", "對話"],
  ["sheet", "學習單"],
] as const;

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
    if ((t.kind === "revise" || t.kind === "chat") && current !== null) target.set(t.id, current);
  }
  return target;
}

function PrepSessionView({ id }: { id: number }) {
  const ids = useId();
  const navigate = useNavigate();
  const classes = useActiveClasses();
  const templates = useReadyTemplates();
  const [session, setSession] = useState<PrepSession | null>(null);
  // Habit suggestions found in what the teacher typed (Manna asks; she decides): those of this
  // conversation's messages, and of 「改這一區」 on its worksheets (they carry the worksheet only,
  // so the teacher's pending ones are read and kept to this conversation below).
  const memory = useMemorySuggestions(
    {},
    session ? session.turns.map((t) => `${t.id}:${t.status}`).join(",") : "",
  );
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  // A taller composer for long instructions (the expand corner, as in chat composers).
  const [composerTall, setComposerTall] = useState(false);
  const [draft, setDraft] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [headError, setHeadError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const { triggerRef: deleteTriggerRef, safeRef: deleteSafeRef } = useConfirmFocus(confirmDelete);
  // The turn whose worksheet card first had its sheet (see `holdsPanel`).
  const [sheetReadyTurn, setSheetReadyTurn] = useState<number | null>(null);
  const [viewing, setViewing] = useState<number | null>(null);
  const [labels, setLabels] = useState<Record<number, Record<string, string>>>({});
  const [flash, setFlash] = useState<{ worksheetId: number; ids: ReadonlySet<string> } | null>(null);
  /** 版本紀錄 (opened from a chat card) restored a version: the preview and the session are read again. */
  function afterVersionRestore() {
    setUndone((n) => n + 1);
    refreshSession();
  }

  // 「改這一區」: the bottom bar revises one block of one worksheet instead of the whole worksheet.
  const [target, setTarget] = useState<{ worksheetId: number; blockId: string; name: string } | null>(null);
  const [undone, setUndone] = useState(0);
  // The worksheet shown beside the conversation: the turn the teacher chose, else the latest one.
  const [pickedTurn, setPickedTurn] = useState<number | null>(null);
  const narrow = useNarrow();
  const { containerRef, share, dragging, canResize, columns, edge, min, max, dividerProps } = useSplitResize();
  const [tab, setTab] = useState<"chat" | "sheet">("chat");
  // 預覽｜編輯 in the worksheet pane. The editor registers how to save its pending edits, so a
  // message is sent from the saved version; `editRev` makes the preview read the sheet again after edits.
  const [sheetMode, setSheetMode] = useState<SheetMode>(loadSheetMode);
  // Where the embedded editor says 已自動儲存: on the right of the 預覽｜編輯 row.
  const [savedSlot, setSavedSlot] = useState<HTMLDivElement | null>(null);
  const editFlush = useRef<SheetFlush | null>(null);
  const [editRev, setEditRev] = useState(0);
  // Wide layout: the conversation can fold to a strip on the right edge; its header settings fold too.
  const [chatCollapsed, setChatCollapsed] = useStoredFlag(CHAT_COLLAPSED_KEY, false);
  const [headOpenWide, setHeadOpenWide] = useStoredFlag(HEAD_OPEN_KEY, false);
  const [newReply, setNewReply] = useState(false);
  const collapseRef = useRef<HTMLButtonElement | null>(null);
  const expandRef = useRef<HTMLButtonElement | null>(null);
  const focusAfter = useRef<"collapse" | "expand" | null>(null);
  const wasRunning = useRef(false);
  // Narrow screens share one page scroll between the two tabs: each tab keeps its own position.
  const tabScrolls = useRef<Partial<Record<"chat" | "sheet", number>>>({});
  const shownTab = useRef(tab);
  const lastScrollY = useRef(0);
  const tabRefs = useRef<Partial<Record<"chat" | "sheet", HTMLButtonElement | null>>>({});
  useEffect(() => {
    const onScroll = () => {
      lastScrollY.current = window.scrollY;
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);
  useLayoutEffect(() => {
    if (shownTab.current === tab) return;
    tabScrolls.current[shownTab.current] = lastScrollY.current;
    shownTab.current = tab;
    const y = tabScrolls.current[tab];
    if (y !== undefined) {
      window.scrollTo({ top: y, behavior: "instant" });
      lastScrollY.current = y;
    }
  }, [tab]);
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
      if (fresh && isWorksheetKind(fresh.kind)) setPickedTurn(null);
      const targets = reviseTargets(next.turns);
      for (const t of next.turns) {
        const was = before.turns.find((p) => p.id === t.id);
        const reply = replyOf(t);
        const chat = chatResult(t);
        const changedIds =
          reply?.changed_block_ids ?? (chat ? chat.changes.flatMap((c) => (c.block_id ? [c.block_id] : [])) : []);
        const wsId = targets.get(t.id);
        if (t.status === "done" && was?.status !== "done" && changedIds.length > 0 && wsId) {
          setFlash({ worksheetId: wsId, ids: new Set(changedIds) });
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
  const refreshSession = useCallback(() => {
    api
      .getPrepSession(id)
      .then(apply)
      .catch((err: unknown) => setSendError(errorMessage(err)));
  }, [id, apply]);
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
    scrollToEl(document.getElementById(`prep-turn-${arrived}`), "start");
  }, [arrived]);

  // A chat turn changed blocks: the preview scrolls to the first one once its new version is in
  // the page (once; it gives way when the teacher scrolls, points or types), and the highlight
  // runs for FLASH_MS from the moment that block is on screen, not from the end of the turn.
  useEffect(() => {
    const first = flash ? [...flash.ids][0] : undefined;
    if (!first) return undefined;
    let unwatch: (() => void) | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const clear = () => setFlash(null);
    const cancel = scrollToIdWhenReady(`sheet-block-${first}`, {
      onScrolled: (el) => {
        unwatch?.();
        unwatch = whenVisible(el, () => {
          if (timer !== undefined) clearTimeout(timer);
          timer = setTimeout(clear, FLASH_MS);
        });
      },
    });
    // Never seen (a hidden tab, a block that is gone): the highlight does not stay forever.
    const fallback = setTimeout(clear, FLASH_FALLBACK_MS);
    return () => {
      cancel();
      unwatch?.();
      clearTimeout(fallback);
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [flash]);

  const onLabels = useCallback((worksheetId: number, map: Record<string, string>) => {
    setLabels((prev) => ({ ...prev, [worksheetId]: map }));
  }, []);

  // Pictures go with a worksheet's changes: only when the conversation has a worksheet.
  const canAttach = session !== null && hasWorksheet(session);
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
  const turnRunning = !!session?.running;
  const foldedWide = chatCollapsed && !narrow;
  useEffect(() => {
    // A turn that finishes while the conversation is folded leaves a dot on the strip.
    if (wasRunning.current && !turnRunning && foldedWide) setNewReply(true);
    wasRunning.current = turnRunning;
  }, [turnRunning, foldedWide]);
  useEffect(() => {
    const to = focusAfter.current;
    if (!to) return;
    focusAfter.current = null;
    (to === "expand" ? expandRef : collapseRef).current?.focus();
  }, [chatCollapsed]);

  if (!session) return <Loading />;

  const running = session.running;
  const locked = running || sending;
  const targets = reviseTargets(session.turns);
  const next = session.next.filter(isNextKind);
  const sheetIds = new Set(session.turns.flatMap((t) => (t.worksheet ? [t.worksheet.id] : [])));
  // The worksheets of this conversation, and the one the right pane (and the bar) works on.
  const sheetTurns = session.turns.filter((t) => isWorksheetKind(t.kind) && (t.worksheet || t.status === "running"));
  const latestSheet = sheetTurns[sheetTurns.length - 1] ?? null;
  const sheetTurn = sheetTurns.find((t) => t.id === pickedTurn) ?? latestSheet;
  const split = sheetTurn !== null;
  const folded = split && !narrow && chatCollapsed; // conversation folded to the strip
  const compactHead = split && !narrow; // the header's settings sit in a disclosure beside the worksheet
  const headOpen = !compactHead || headOpenWide;
  const currentSheet = sheetTurn?.worksheet ?? null;
  const reviseSuggestions = memory.items.filter(
    (c) => c.source === "block_revise" && c.worksheet_id !== null && sheetIds.has(c.worksheet_id),
  );

  async function send(body: CreatePrepTurnRequest): Promise<boolean> {
    setSending(true);
    setSendError(null);
    try {
      // Edits made by hand are saved first: the turn starts from the version the teacher sees.
      const unsaved = await editFlush.current?.();
      if (unsaved) {
        setSendError(`學習單的修改還沒存好，所以先不送出：${unsaved}`);
        return false;
      }
      apply(await api.createPrepTurn(id, body));
      // What the teacher typed is also read for habits (the suggestions ask again).
      if (body.kind === "chat") habitWordsSent();
      return true;
    } catch (err) {
      setSendError(errorMessage(err));
      return false;
    } finally {
      setSending(false);
    }
  }

  /** 預覽｜編輯: leaving 編輯 saves first (and stays when that fails, the editor says why). */
  async function chooseSheetMode(next: SheetMode) {
    if (next === sheetMode) return;
    if (sheetMode === "edit") {
      if (await editFlush.current?.()) return;
      setEditRev((n) => n + 1);
    }
    setSheetMode(next);
    saveSheetMode(next);
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
      habitWordsSent();
      setProposalsKey((prev) => ({ ...prev, [t.worksheetId]: (prev[t.worksheetId] ?? 0) + 1 }));
      setTarget(null);
      setDraft("");
      scrollToEl(document.getElementById(`sheet-block-${t.blockId}`), "center");
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
    // Free text is always a chat turn: Manna answers, changes the worksheet, or both.
    const body: CreatePrepTurnRequest = {
      kind: "chat",
      request: text,
      ...(sheetTurn?.worksheet && latestSheet && sheetTurn.id !== latestSheet.id
        ? { worksheet_id: sheetTurn.worksheet.id }
        : {}),
      ...(canAttach && attach.imageIds.length > 0 ? { image_ids: attach.imageIds } : {}),
    };
    if (await send(body)) {
      setDraft("");
      attach.clear();
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    // While Manna works the teacher may keep typing; only sending waits (sendDraft also checks).
    if (locked) return;
    void sendDraft();
  }

  /** 復原這一輪: the worksheet goes back; the session and the preview are read again. */
  async function undoTurn(turnId: number) {
    await api.undoPrepTurn(id, turnId);
    setUndone((n) => n + 1);
    apply(await api.getPrepSession(id));
  }

  /** 「在左邊看」: this worksheet becomes the one beside the conversation (a tab of its own on a narrow screen). */
  function showSheet(turnId: number) {
    setPickedTurn(turnId);
    if (narrow) setTab("sheet");
  }

  /** A change in 「這一輪改了」: the preview scrolls to that block. */
  function showBlock(blockId: string) {
    if (narrow) setTab("sheet");
    setTimeout(() => scrollToBlock(blockId), narrow ? 80 : 0);
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
  const contextSummary = [
    session.material ? `教材：${session.material.title.trim() || "未命名教材"}` : session.topic ? `主題：${session.topic}` : null,
    classOptions.length > 0 ? `班級：${classOptions.find((c) => c.id === session.class?.id)?.name ?? NO_CLASS_LABEL}` : null,
    session.template ? `版型：${session.template.title}` : null,
  ]
    .filter(Boolean)
    .join("・");
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
  function panelShown(turn: PrepTurn) {
    return turn.id === lastTurn?.id && lastPanel.visible && !!turn.progress;
  }

  /**
   * The pane's panel keeps its steps, all done, until the sheet it makes is on screen (then it folds
   * into one line in that same frame), so the panel, a loading line and the sheet never stack.
   */
  function holdsPanel(turn: PrepTurn) {
    return panelShown(turn) && turn.progress?.status === "done" && ownsPane(turn) && sheetReadyTurn !== turn.id;
  }

  /** The panel is still the run's live status (steps running or failed), so working lines stay quiet. */
  function stepsShown(turn: PrepTurn) {
    return panelShown(turn) && (turn.progress?.status !== "done" || holdsPanel(turn));
  }

  /** A worksheet turn's 「進度」 panel shows in the right pane, beside the sheet it makes. */
  function ownsPane(turn: PrepTurn) {
    return isWorksheetKind(turn.kind) && turn.kind !== "kit" && turn.id === sheetTurn?.id;
  }

  function worksheetCard(turn: PrepTurn) {
    const wsId = turn.worksheet?.id;
    const lastRevise = [...session!.turns]
      .reverse()
      .find((t) => (t.kind === "revise" || t.kind === "chat") && targets.get(t.id) === wsId);
    const lastVersion = lastRevise ? (chatResult(lastRevise)?.version_no ?? "") : "";
    const versionKey = `${turn.worksheet?.version_no ?? 0}-${lastRevise?.id ?? 0}-${lastRevise?.status ?? ""}-${lastVersion}-${undone}`;
    const editing = sheetMode === "edit" && !!wsId && turn.id === sheetTurn?.id;
    return (
      <>
        {editing && wsId && (
          <SheetEditor worksheetId={wsId} chatBusy={locked} versionKey={versionKey} flushRef={editFlush} savedSlot={savedSlot} />
        )}
        <div hidden={editing}>{previewCard(turn, wsId, `${versionKey}-${editRev}`)}</div>
      </>
    );
  }

  function previewCard(turn: PrepTurn, wsId: number | undefined, refreshKey: string) {
    return (
      <WorksheetCard
        turn={turn}
        stepsShown={stepsShown(turn)}
        onReady={() => setSheetReadyTurn(turn.id)}
        refreshKey={refreshKey}
        chatBusy={locked}
        highlight={flash && flash.worksheetId === wsId ? flash.ids : EMPTY}
        onLabels={onLabels}
        target={target && target.worksheetId === wsId ? target.blockId : null}
        onTarget={(worksheetId, blockId, name) => {
          setTarget({ worksheetId, blockId, name });
          setTab("chat");
          setTimeout(() => draftRef.current?.focus(), 0);
        }}
        proposalsKey={wsId ? proposalsKey[wsId] : undefined}
      />
    );
  }

  /** The worksheet turn in the conversation: a line, not the preview (that is beside it). */
  function sheetSummary(turn: PrepTurn) {
    return (
      <WorksheetTurnCard
        turn={turn}
        current={sheetTurn?.id === turn.id}
        split={split}
        narrow={narrow}
        onShow={() => showSheet(turn.id)}
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
          {(turn.worksheet || turn.status === "running") && sheetSummary(turn)}
          {turn.status === "failed" && failed(turn)}
          <SiopCheckPanel sessionId={id} turn={turn} quiet={stepsShown(turn)} />
        </div>
      );
    }
    if (isWorksheetKind(turn.kind) && (turn.status !== "failed" || turn.worksheet)) {
      return (
        <>
          {sheetSummary(turn)}
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
    const chat = chatResult(turn);
    if (chat) {
      return (
        <ChatCard
          result={chat}
          interactionId={turn.interaction_id}
          disabled={locked}
          onFollowUp={(q) => void send({ kind: "chat", request: q })}
          onUndo={() => undoTurn(turn.id)}
          worksheetId={targets.get(turn.id) ?? null}
          onVersionRestored={afterVersionRestore}
          onShowBlock={showBlock}
          onShowSheet={narrow ? (blockId) => {
            setTab("sheet");
            if (blockId) setTimeout(() => scrollToBlock(blockId), 80);
          } : undefined}
        />
      );
    }
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
    <div
      ref={containerRef}
      className={`prep-session${viewing !== null ? " with-viewer" : ""}${split ? " is-split" : ""}${dragging ? " is-resizing" : ""}${folded ? " is-chat-folded" : ""}`}
      style={split && !narrow ? { gridTemplateColumns: folded ? FOLDED_COLUMNS : columns } : undefined}
    >
      {split && narrow && (
        <div className="prep-split-tabs" role="tablist" aria-label="對話或學習單">
          {SPLIT_TABS.map(([key, label], i) => (
            <button
              key={key}
              ref={(el) => {
                tabRefs.current[key] = el;
              }}
              type="button"
              role="tab"
              id={`${ids}-tab-${key}`}
              aria-selected={tab === key}
              aria-controls={`${ids}-pane-${key}`}
              tabIndex={tab === key ? 0 : -1}
              className={tab === key ? "is-current" : ""}
              onClick={() => setTab(key)}
              onKeyDown={(e) => {
                const step =
                  e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : e.key === "Home" ? -i : e.key === "End" ? 1 - i : null;
                if (step === null) return;
                e.preventDefault();
                const next = SPLIT_TABS[(i + step + SPLIT_TABS.length) % SPLIT_TABS.length];
                if (!next) return;
                setTab(next[0]);
                tabRefs.current[next[0]]?.focus();
              }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      {sheetTurn && (
        <aside
          id={`${ids}-pane-sheet`}
          className="prep-split-sheet"
          aria-label="目前的學習單"
          {...(split && narrow ? { role: "tabpanel", "aria-labelledby": `${ids}-tab-sheet` } : {})}
          hidden={narrow && tab !== "sheet"}
        >
          {sheetTurns.length > 1 && (
            <p className="prep-sheet-picker">
              <label htmlFor={`${ids}-sheet-pick`}>學習單</label>
              <select
                id={`${ids}-sheet-pick`}
                value={sheetTurn.id}
                onChange={(e) => setPickedTurn(Number(e.target.value))}
              >
                {sheetTurns.map((t) => (
                  <option key={t.id} value={t.id}>
                    {`第 ${t.seq} 則：${t.worksheet?.title.trim() || KIND_LABEL[t.kind]}`}
                  </option>
                ))}
              </select>
            </p>
          )}
          {panelShown(sheetTurn) && sheetTurn.progress && ownsPane(sheetTurn) && (
            <RunProgressPanel
              progress={sheetTurn.progress}
              error={sheetTurn.error}
              onClose={lastPanel.close}
              onCancelled={refreshSession}
              collapseDone={!holdsPanel(sheetTurn)}
              className="prep-turn-progress"
            />
          )}
          {currentSheet && (
            <div className="prep-sheet-modebar">
              <SheetModeToggle mode={sheetMode} onChange={(m) => void chooseSheetMode(m)} />
              <div className="prep-sheet-modebar-status" ref={setSavedSlot} />
            </div>
          )}
          <div key={sheetTurn.id}>{worksheetCard(sheetTurn)}</div>
        </aside>
      )}

      {split && !narrow && !folded && (
        <div
          className="prep-split-divider"
          role="separator"
          aria-orientation="vertical"
          aria-label="調整對話與學習單的寬度"
          aria-controls={`${ids}-pane-sheet`}
          aria-valuenow={Math.round(share)}
          aria-valuemin={Math.ceil(min)}
          aria-valuemax={Math.floor(max)}
          aria-valuetext={`學習單 ${Math.round(share)}%，對話 ${100 - Math.round(share)}%`}
          aria-disabled={canResize ? undefined : true}
          data-edge={edge ?? undefined}
          title="拖曳調整寬度；按兩下或 Enter 還原"
          tabIndex={canResize ? 0 : -1}
          {...dividerProps}
        />
      )}

      {folded && (
        <aside className="prep-chat-strip" aria-label="對話">
          <button
            type="button"
            className="material-pane-open prep-chat-open"
            ref={expandRef}
            aria-expanded={false}
            aria-controls={`${ids}-pane-chat`}
            onClick={() => {
              focusAfter.current = "collapse";
              setNewReply(false);
              setChatCollapsed(false);
            }}
          >
            <Icon name="speech" />
            <span className="rail-label">展開對話</span>
            {newReply && (
              <span className="prep-chat-new" role="status" aria-label="有新回覆">
                <span className="prep-chat-dot" aria-hidden="true" />
              </span>
            )}
          </button>
        </aside>
      )}

      <section
        id={`${ids}-pane-chat`}
        className="prep-split-chat"
        aria-label="對話"
        {...(split && narrow ? { role: "tabpanel", "aria-labelledby": `${ids}-tab-chat` } : {})}
        hidden={(split && narrow && tab !== "chat") || folded}
      >
      <div className="page-head prep-head">
        <div className="prep-head-top">
          <p className="crumbs">
            <Link to="/prep">我的作品</Link>
          </p>
          {compactHead && (
            <button
              type="button"
              className="text-link is-plain prep-chat-fold"
              ref={collapseRef}
              aria-expanded={true}
              aria-controls={`${ids}-pane-chat`}
              onClick={() => {
                focusAfter.current = "expand";
                setChatCollapsed(true);
              }}
            >
              收起對話
            </button>
          )}
        </div>
        {renaming === null ? (
          <div className="prep-title-row">
            <h1 className={compactHead ? "is-clamped" : undefined} title={compactHead ? sessionTitle(session) : undefined}>
              {sessionTitle(session)}
            </h1>
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

        {compactHead && (
          <button
            type="button"
            className="prep-head-toggle"
            aria-expanded={headOpen}
            aria-controls={`${ids}-head-settings`}
            onClick={() => setHeadOpenWide(!headOpen)}
          >
            <span className="prep-head-toggle-label">對話設定</span>
            {!headOpen && <span className="prep-head-summary">{contextSummary}</span>}
          </button>
        )}
        <div
          id={`${ids}-head-settings`}
          className="prep-context"
          role="group"
          aria-label="備課情境"
          hidden={!headOpen}
        >
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
                <button type="button" className="text-link is-plain" ref={deleteSafeRef} onClick={() => setConfirmDelete(false)}>
                  不刪
                </button>
              </span>
            ) : (
              <button type="button" className="text-link is-plain" ref={deleteTriggerRef} onClick={() => setConfirmDelete(true)}>
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
              {panelShown(turn) && turn.progress && !ownsPane(turn) && (
                <RunProgressPanel
                  progress={turn.progress}
                  error={turn.error}
                  onClose={lastPanel.close}
                  onCancelled={refreshSession}
                  className="prep-turn-progress"
                />
              )}
              {result(turn)}
            </section>
            <MemorySuggestionCards
              items={memory.items.filter((c) => c.prep_turn_id === turn.id)}
              onDone={memory.drop}
              onRemembered={memory.reload}
            />
          </li>
        ))}
      </ol>
      {/* With the conversation, not in the sticky bar: it scrolls away with the turns. */}
      <p className="prep-disclaimer">{AI_DISCLAIMER}</p>

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
        {/* From 「改這一區」 (on a card above or from this bar): asked here, where she is typing. */}
        <MemorySuggestionCards items={reviseSuggestions} onDone={memory.drop} onRemembered={memory.reload} />
        <ErrorText message={sendError} />
        {/* Beside the conversation the worksheet itself is in view; only its own tab needs the name. */}
        {currentSheet && !target && (!split || narrow) && (
          <p className="prep-current-sheet">目前的學習單：{currentSheet.title}</p>
        )}
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
            rows={2}
            maxLength={2000}
            value={draft}
            readOnly={sending}
            placeholder={
              running
                ? WORKING_LINE
                : target
                  ? `想怎麼改 ${target.name}？例如：第 2 題太難，改簡單一點`
                  : CHAT_PLACEHOLDER
            }
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
            onPaste={drop.onPaste}
          />
          <div className="prep-composer-tools">
            {canAttach && <AttachButton onFiles={attach.add} disabled={locked} />}
            {!running && (
              <span className="prep-composer-hint" aria-hidden="true">
                Enter 送出・Shift＋Enter 換行
              </span>
            )}
            {/* Always present, so the line is announced when it appears. */}
            <span className="prep-composer-wait" role="status">
              {running ? "Manna 做完後就能送出" : ""}
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
        {canAttach && (attach.imageIds.length > 0 || attach.uploading || drop.dragging) && (
          <p className="prep-attach-note">{IMAGE_PRIVACY_NOTE}</p>
        )}
      </form>
      </section>

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
