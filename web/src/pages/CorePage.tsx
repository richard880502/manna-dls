import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type {
  AssistLevel,
  ContentSource,
  CoreQuestion,
  CoreVersion,
  PrepRoute,
  QuestionId,
  Thumbs,
  Unit,
} from "../api/types";
import { ErrorText, Loading, Notice, PrivacyHint } from "../components/common";
import { CoreCell } from "../components/core/CoreCell";
import { HelpPanel, type HelpState } from "../components/core/HelpPanel";
import { useApp } from "../context/AppContext";
import { useActiveClass } from "../lib/classes";
import {
  CORE_PRE_QUESTION,
  FALLBACK_QUESTIONS,
  MAX_FOCUSES,
  buildDraft,
  emptyFocus,
  focusFromServer,
  type CellState,
  type FocusState,
} from "../lib/core";
import { formatDateTime } from "../lib/labels";
import { PREP_ROUTE_LABELS, ROUTE_CORE_HIGHLIGHTS, effectiveRoute } from "../lib/preferences";
import { logScaffold } from "../lib/scaffold";
import { useDocumentTitle } from "../lib/useDocumentTitle";

const ROUTE_CORE_NOTES: Record<PrepRoute, string> = {
  goal: "建議先寫標示「建議先寫」的 Q1–Q3：學生要學會什麼、為什麼要學、這次先不教什麼。其他題可以之後再補。",
  material: "從教材候選重點放進來的學習重點會出現在這裡；你可以改寫、移除，或自己新增。",
  student: "建議先寫標示「建議先寫」的 Q5、Q6：學生可能怎麼想或誤解、還有哪些因素會影響教學。",
  activity: "活動優先：可以先在課例寫活動，連結單元想法的重點後再回來補寫這裡。",
};

export const AUTOSAVE_DELAY_MS = 1500;

type SaveStatus = "idle" | "saving" | "saved" | "error";
type VersionMeta = Pick<CoreVersion, "id" | "version_no" | "completed_at" | "phase">;

const cellKey = (focusKey: string, q: QuestionId) => `${focusKey}:${q}`;

export function CorePage() {
  useDocumentTitle("單元想法");
  const unitId = Number(useParams().unitId);
  const { aiEnabled, routeChoiceEnabled } = useApp();

  const [unit, setUnit] = useState<Unit | null>(null);
  const { active: activeClass } = useActiveClass(unit);
  const [questions, setQuestions] = useState<CoreQuestion[]>(FALLBACK_QUESTIONS);
  const [meta, setMeta] = useState<VersionMeta | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [focuses, setFocuses] = useState<FocusState[]>([]);
  const focusesRef = useRef<FocusState[]>([]);
  const [activeFocusKey, setActiveFocusKey] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const [dirty, setDirty] = useState(false);
  const editRev = useRef(0);
  const savedRev = useRef(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const chainRef = useRef<Promise<unknown>>(Promise.resolve());

  const [completing, setCompleting] = useState(false);
  const [completeMessage, setCompleteMessage] = useState<string | null>(null);
  const [completeError, setCompleteError] = useState<string | null>(null);

  const [activeHelp, setActiveHelp] = useState<{ focusKey: string; q: QuestionId } | null>(null);
  const [help, setHelp] = useState<Record<string, HelpState>>({});
  const helpRef = useRef<Record<string, HelpState>>({});

  // ---------- loading ----------
  useEffect(() => {
    let alive = true;
    api
      .getUnit(unitId)
      .then((u) => alive && setUnit(u))
      .catch(() => undefined);
    api
      .coreQuestions()
      .then((qs) => {
        if (alive && Array.isArray(qs) && qs.length > 0) setQuestions(qs);
      })
      .catch(() => undefined);
    api
      .getCore(unitId)
      .then((version) => {
        if (!alive) return;
        const loaded = [...version.focuses].sort((a, b) => a.position - b.position).map(focusFromServer);
        const initial = loaded.length > 0 ? loaded : [emptyFocus()];
        focusesRef.current = initial;
        setFocuses(initial);
        setActiveFocusKey(initial[0]?.key ?? null);
        setMeta(version);
      })
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    logScaffold("SC-CORE", "show");
    return () => {
      alive = false;
    };
  }, [unitId]);

  // ---------- saving ----------
  const setFocusesSilently = useCallback((updater: (prev: FocusState[]) => FocusState[]) => {
    const next = updater(focusesRef.current);
    focusesRef.current = next;
    setFocuses(next);
  }, []);

  const saveNow = useCallback((): Promise<boolean> => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const run = async (): Promise<boolean> => {
      const snapshot = focusesRef.current;
      const rev = editRev.current;
      setSaveStatus("saving");
      setSaveError(null);
      try {
        const version = await api.saveCoreDraft(unitId, buildDraft(snapshot));
        const returned = [...(version.focuses ?? [])].sort((a, b) => a.position - b.position);
        const idByKey = new Map<string, number>();
        snapshot.forEach((f, i) => {
          const r = returned[i];
          if (f.id === undefined && r) idByKey.set(f.key, r.id);
        });
        if (idByKey.size > 0) {
          setFocusesSilently((prev) =>
            prev.map((f) => (f.id === undefined && idByKey.has(f.key) ? { ...f, id: idByKey.get(f.key) } : f)),
          );
        }
        setMeta(version);
        savedRev.current = Math.max(savedRev.current, rev);
        setDirty(editRev.current > savedRev.current);
        setSaveStatus("saved");
        setSavedAt(new Date());
        return true;
      } catch (err) {
        setSaveStatus("error");
        setSaveError(`草稿儲存失敗：${errorMessage(err)}`);
        return false;
      }
    };
    const p = chainRef.current.then(run, run);
    chainRef.current = p;
    return p;
  }, [unitId, setFocusesSilently]);

  const saveNowRef = useRef(saveNow);
  useEffect(() => {
    saveNowRef.current = saveNow;
  }, [saveNow]);

  const scheduleAutosave = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      void saveNowRef.current();
    }, AUTOSAVE_DELAY_MS);
  }, []);

  // Flush pending edits when leaving the page.
  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      if (editRev.current > savedRev.current) void saveNowRef.current();
    };
  }, []);

  const edit = useCallback(
    (updater: (prev: FocusState[]) => FocusState[]) => {
      setFocusesSilently(updater);
      editRev.current += 1;
      setDirty(true);
      setCompleteMessage(null);
      scheduleAutosave();
    },
    [setFocusesSilently, scheduleAutosave],
  );

  const updateCell = useCallback(
    (focusKey: string, q: QuestionId, change: (c: CellState) => CellState) => {
      edit((prev) =>
        prev.map((f) => (f.key === focusKey ? { ...f, cells: { ...f.cells, [q]: change(f.cells[q]) } } : f)),
      );
    },
    [edit],
  );

  // ---------- focuses ----------
  function addFocus() {
    if (focusesRef.current.length >= MAX_FOCUSES) return;
    const f = emptyFocus();
    edit((prev) => [...prev, f]);
    setActiveFocusKey(f.key);
  }

  function removeFocus(key: string) {
    const remaining = focusesRef.current.filter((f) => f.key !== key);
    edit(() => remaining);
    setConfirmRemove(null);
    if (activeHelp?.focusKey === key) setActiveHelp(null);
    setActiveFocusKey(remaining[0]?.key ?? null);
  }

  // ---------- help (SC-CORE-HELP) ----------
  const setHelpFor = useCallback((key: string, change: (h: HelpState) => HelpState) => {
    const current = helpRef.current[key] ?? { entries: [], loading: false, error: null };
    const next = { ...helpRef.current, [key]: change(current) };
    helpRef.current = next;
    setHelp(next);
  }, []);

  async function requestLevel(focusKey: string, q: QuestionId, level: AssistLevel) {
    const key = cellKey(focusKey, q);
    setHelpFor(key, (h) => ({ ...h, loading: true, error: null }));
    let focus = focusesRef.current.find((f) => f.key === focusKey);
    if (focus && focus.id === undefined) {
      await saveNow();
      focus = focusesRef.current.find((f) => f.key === focusKey);
    }
    if (!focus || focus.id === undefined) {
      setHelpFor(key, (h) => ({ ...h, loading: false, error: "請先儲存草稿，再請 Manna 協助。" }));
      return;
    }
    try {
      const result = await api.coreAssist({
        unit_id: unitId,
        focus_id: focus.id,
        question_id: q,
        level,
        ...(activeClass ? { class_id: activeClass.id } : {}),
      });
      setHelpFor(key, (h) => ({
        ...h,
        loading: false,
        entries: [...h.entries, { level, result, shownAt: Date.now() }],
      }));
      if (result.status === "shown") {
        // UI-only flag: reveals the content-source selector; does not change the saved draft.
        setFocusesSilently((prev) =>
          prev.map((f) =>
            f.key === focusKey ? { ...f, cells: { ...f.cells, [q]: { ...f.cells[q], sawAi: true } } } : f,
          ),
        );
        logScaffold("SC-CORE-HELP", "show", { trigger: `${q}-L${level}` });
      }
    } catch (err) {
      setHelpFor(key, (h) => ({ ...h, loading: false, error: errorMessage(err) }));
    }
  }

  function openHelp(focusKey: string, q: QuestionId) {
    setActiveHelp({ focusKey, q });
    logScaffold("SC-CORE-HELP", "expand", { trigger: `${q}-L1` });
    const existing = helpRef.current[cellKey(focusKey, q)];
    if (existing && (existing.entries.length > 0 || existing.loading)) return;
    if (!aiEnabled) return;
    void requestLevel(focusKey, q, 1);
  }

  function continueHelp(level: AssistLevel) {
    if (!activeHelp) return;
    if (level > 1) logScaffold("SC-CORE-HELP", "expand", { trigger: `${activeHelp.q}-L${level}` });
    void requestLevel(activeHelp.focusKey, activeHelp.q, level);
  }

  async function sendHelpDecision(key: string, index: number, decision: "answer" | "skip", thumbs?: Thumbs) {
    const entry = helpRef.current[key]?.entries[index];
    if (!entry || entry.result.interaction_id === null) return;
    try {
      await api.sendDecision(entry.result.interaction_id, {
        decision,
        ...(thumbs ? { thumbs } : {}),
      });
      setHelpFor(key, (h) => ({
        ...h,
        entries: h.entries.map((e, i) => (i === index ? { ...e, decisionError: null } : e)),
      }));
    } catch (err) {
      setHelpFor(key, (h) => ({
        ...h,
        entries: h.entries.map((e, i) => (i === index ? { ...e, decisionError: errorMessage(err) } : e)),
      }));
    }
  }

  function onHelpDecision(index: number, decision: "answer" | "skip") {
    if (!activeHelp) return;
    const key = cellKey(activeHelp.focusKey, activeHelp.q);
    const entry = helpRef.current[key]?.entries[index];
    if (!entry) return;
    setHelpFor(key, (h) => ({
      ...h,
      entries: h.entries.map((e, i) => (i === index ? { ...e, decision } : e)),
    }));
    void sendHelpDecision(key, index, decision, entry.thumbs);
    logScaffold("SC-CORE-HELP", decision === "answer" ? "use" : "skip", {
      trigger: `${activeHelp.q}-L${entry.level}`,
      dwell_ms: Math.max(0, Date.now() - entry.shownAt),
    });
    if (decision === "answer") {
      const textarea = document.getElementById(`${activeHelp.focusKey}-${activeHelp.q}-answer`);
      if (textarea instanceof HTMLTextAreaElement && !textarea.disabled) textarea.focus();
    }
  }

  function onHelpThumbs(index: number, thumbs: Thumbs) {
    if (!activeHelp) return;
    const key = cellKey(activeHelp.focusKey, activeHelp.q);
    const entry = helpRef.current[key]?.entries[index];
    if (!entry) return;
    const nextThumbs = entry.thumbs === thumbs ? undefined : thumbs;
    setHelpFor(key, (h) => ({
      ...h,
      entries: h.entries.map((e, i) => (i === index ? { ...e, thumbs: nextThumbs } : e)),
    }));
    // A decision already sent gets the rating attached; otherwise it goes with 回答／略過.
    if (entry.decision && nextThumbs) void sendHelpDecision(key, index, entry.decision, nextThumbs);
  }

  // ---------- complete ----------
  async function complete() {
    setCompleting(true);
    setCompleteError(null);
    setCompleteMessage(null);
    const saved = await saveNow();
    if (!saved) {
      setCompleting(false);
      setCompleteError("草稿尚未儲存成功，請稍後再按一次「完成單元想法」。");
      return;
    }
    try {
      const version = await api.completeCore(unitId);
      setMeta(version);
      setCompleteMessage(
        `已完成單元想法${version.completed_at ? `（${formatDateTime(version.completed_at)}）` : ""}。之後仍可回來修改。`,
      );
    } catch (err) {
      setCompleteError(errorMessage(err));
    } finally {
      setCompleting(false);
    }
  }

  // ---------- render ----------
  if (loadError) return <ErrorText message={loadError} />;
  if (!meta) return <Loading />;

  const route = effectiveRoute(routeChoiceEnabled, unit?.prep_route);
  const highlights = unit ? ROUTE_CORE_HIGHLIGHTS[route] : [];
  const activeFocusIndex = focuses.findIndex((f) => f.key === activeFocusKey);
  const activeFocus = activeFocusIndex >= 0 ? focuses[activeFocusIndex] : undefined;
  const helpFocusIndex = activeHelp ? focuses.findIndex((f) => f.key === activeHelp.focusKey) : -1;
  const helpQuestion = activeHelp ? (questions.find((q) => q.id === activeHelp.q) ?? null) : null;

  let saveLabel = "";
  if (saveStatus === "saving") saveLabel = "儲存中…";
  else if (dirty) saveLabel = "有尚未儲存的變更（會自動儲存）";
  else if (saveStatus === "saved" && savedAt)
    saveLabel = `已儲存 ${String(savedAt.getHours()).padStart(2, "0")}:${String(savedAt.getMinutes()).padStart(2, "0")}`;

  return (
    <section aria-labelledby="core-heading">
      <p className="crumbs">
        {activeClass ? (
          <>
            <Link to="/classes">班級</Link> / <Link to={`/classes/${activeClass.id}`}>{activeClass.name}</Link>
          </>
        ) : (
          <>
            <Link to="/classes">班級</Link> / <Link to="/units">備課庫</Link>
          </>
        )}{" "}
        / <Link to={`/units/${unitId}`}>{unit?.title ?? "單元"}</Link> / 單元想法
      </p>
      <div className="page-head">
        <h1 id="core-heading">
          單元想法 <small className="en">CoRe</small>
        </h1>
        <p className="core-pre-question">{CORE_PRE_QUESTION}</p>
        <p className="quiet-note">
          八題都可以留白，想到再寫；會自動儲存。
          {meta.completed_at ? `已在 ${formatDateTime(meta.completed_at)} 標為寫好。` : ""}
        </p>
      </div>

      {unit && (
        <Notice>
          {routeChoiceEnabled ? <strong>{PREP_ROUTE_LABELS[route]}：</strong> : null}
          {ROUTE_CORE_NOTES[route]}
        </Notice>
      )}

      <div className="core-layout">
        <div className="core-main">
          <div className="focus-tabs" role="tablist" aria-label="學習重點">
            {focuses.map((f, i) => (
              <button
                key={f.key}
                type="button"
                role="tab"
                aria-selected={f.key === activeFocusKey}
                className={`focus-tab${f.key === activeFocusKey ? " is-selected" : ""}`}
                onClick={() => setActiveFocusKey(f.key)}
              >
                學習重點 {i + 1}
                {f.big_idea.trim() ? `：${f.big_idea.trim().slice(0, 12)}${f.big_idea.trim().length > 12 ? "…" : ""}` : ""}
              </button>
            ))}
            <button
              type="button"
              className="ghost-button add-focus"
              onClick={addFocus}
              disabled={focuses.length >= MAX_FOCUSES}
            >
              ＋ 新增學習重點
            </button>
          </div>

          {focuses.length === 0 && (
            <p className="empty-preview">目前沒有學習重點。可以按「新增學習重點」，或直接完成（允許留白）。</p>
          )}

          {activeFocus && (
            <div
              className="panel-card focus-card"
              role="tabpanel"
              aria-label={`學習重點 ${activeFocusIndex + 1} 的八題`}
            >
              <div className="card-heading">
                <label className="field-label focus-label" htmlFor={`${activeFocus.key}-big-idea`}>
                  學習重點 {activeFocusIndex + 1}
                </label>
                {confirmRemove === activeFocus.key ? (
                  <span className="confirm-row">
                    確定移除這個重點？
                    <button type="button" className="ghost-button small-button" onClick={() => removeFocus(activeFocus.key)}>
                      確定移除
                    </button>
                    <button type="button" className="ghost-button small-button" onClick={() => setConfirmRemove(null)}>
                      取消
                    </button>
                  </span>
                ) : (
                  <button
                    type="button"
                    className="ghost-button small-button"
                    onClick={() => setConfirmRemove(activeFocus.key)}
                  >
                    移除此重點
                  </button>
                )}
              </div>
              <textarea
                id={`${activeFocus.key}-big-idea`}
                rows={2}
                value={activeFocus.big_idea}
                placeholder="例如：學生能用 Not only… but also… 描述一個人的兩項優點，並理解倒裝的語意功能。"
                onChange={(e) => {
                  const value = e.target.value;
                  edit((prev) => prev.map((f) => (f.key === activeFocus.key ? { ...f, big_idea: value } : f)));
                }}
              />
              <PrivacyHint />

              <div className="core-cells">
                {questions.map((question) => (
                  <CoreCell
                    key={question.id}
                    question={question}
                    cellId={`${activeFocus.key}-${question.id}`}
                    cell={activeFocus.cells[question.id]}
                    isHelpActive={activeHelp?.focusKey === activeFocus.key && activeHelp.q === question.id}
                    highlighted={highlights.includes(question.id)}
                    onAnswer={(value) => updateCell(activeFocus.key, question.id, (c) => ({ ...c, answer: value }))}
                    onToggleNoIdea={(on) =>
                      updateCell(activeFocus.key, question.id, (c) =>
                        on
                          ? { ...c, noIdea: true, stash: c.answer, answer: "" }
                          : { ...c, noIdea: false, answer: c.stash, stash: "" },
                      )
                    }
                    onSource={(source: ContentSource) =>
                      updateCell(activeFocus.key, question.id, (c) => ({ ...c, contentSource: source }))
                    }
                    onHelp={() => openHelp(activeFocus.key, question.id)}
                  />
                ))}
              </div>
            </div>
          )}

          <div className="core-actions panel-card">
            <div className="save-state" aria-live="polite">
              {saveLabel}
              <ErrorText message={saveError} />
            </div>
            <div className="button-row">
              <button type="button" className="secondary-button" onClick={() => void saveNow()}>
                儲存草稿
              </button>
              <button type="button" className="primary-button" onClick={() => void complete()} disabled={completing}>
                {completing ? "處理中…" : "完成單元想法"}
              </button>
            </div>
            {completeMessage && <Notice tone="success">{completeMessage}</Notice>}
            <ErrorText message={completeError} />
            <p className="field-hint">留白或選「目前沒有想法」都可以完成；完成後仍可回來修改。</p>
            <p className="core-next">
              <Link to={`/units/${unitId}`}>
                {route === "activity" ? "下一步：回到單元頁繼續寫課例 →" : "下一步：到單元頁建立或打開課例 →"}
              </Link>
            </p>
          </div>
        </div>

        <HelpPanel
          question={helpQuestion}
          focusLabel={helpFocusIndex >= 0 ? `學習重點 ${helpFocusIndex + 1}` : ""}
          help={activeHelp ? help[cellKey(activeHelp.focusKey, activeHelp.q)] : undefined}
          aiEnabled={aiEnabled}
          onContinue={continueHelp}
          onDecision={onHelpDecision}
          onThumbs={onHelpThumbs}
          onClose={() => setActiveHelp(null)}
        />
      </div>
    </section>
  );
}
