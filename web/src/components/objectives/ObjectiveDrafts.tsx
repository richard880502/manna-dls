import { useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  ObjectiveDraft,
  ObjectiveDraftRequest,
  ObjectiveDraftResult,
  RunProgress,
} from "../../api/types";
import { RunProgressSlot } from "../RunProgress";
import { ocrBusyError } from "../../lib/ocr";
import {
  ANOTHER_SET,
  CATEGORY_LABEL,
  DECIDED_CONTENT_HEADING,
  DRAFTING_LANGUAGE_LINE,
  DRAFTS_CONTENT_LEAD,
  DRAFTS_CONTENT_READY_LINE,
  DRAFTS_LANGUAGE_LEAD,
  LANGUAGE_FAILED,
  onlyContent,
  toLanguage,
  NOTHING_CHECKED,
  NOTHING_CHECKED_GENERATE,
  REDRAW_FAILED,
  REDRAW_UNCHECKED,
  checkedSetNote,
  decisionsAt,
  mergeRedraw,
  useCheckboxLabel,
  useDrafts,
  type DraftDecision,
  type DraftSource,
  DRAFTING_LINE,
  DRAFTS_HEADING,
  DRAFTS_LEAD,
  DRAFTS_READY_FILL_LINE,
  DRAFTS_READY_LINE,
  DRAFT_FAILED,
  OBJECTIVE_KIND_LABEL,
  OBJECTIVE_MAX,
  SKIP_DRAFTS,
  SKIP_DRAFTS_FILL,
  choiceFor,
  decisionsFor,
  fieldsFrom,
  hasDrafts,
} from "../../lib/objectives";
import { scrollToEl } from "../../lib/scroll";

export type DraftState =
  /** `key`: the progress key sent with the request; `progress`: its steps, once the server has them; `stage`: the kind asked for. */
  | { phase: "drafting"; key: string; progress: RunProgress | null; stage: ObjectiveStage }
  | {
      phase: "ready";
      /** The kind this set drafts: the 內容目標 first, the 語言目標 after the teacher decided them. */
      stage: ObjectiveStage;
      /** `stage: "language"`: the 內容目標 the 語言目標 were derived from. */
      decided: string[];
      /** `stage: "language"`: the 內容目標 came from the step before in this panel (they go with the outcome). */
      carried: boolean;
      result: ObjectiveDraftResult;
      /** For each draft shown: the draft call and the `index` it has there (the decisions go there). */
      sources: Record<number, DraftSource>;
      /** Lines the teacher already edited (kept through 換掉沒勾的), by draft index. */
      edits: Record<number, string>;
      /** Drafts that start unchecked (back from a redraw that did not happen). */
      unchecked: number[];
      /** A new set each time: the panel starts over with it. */
      round: number;
    }
  | { phase: "failed"; message: string };

export type ObjectiveStage = "content" | "language";

/** What 請 Manna 擬語言目標 needs from the 內容目標 step: the lines decided on, and how the panel stood. */
export interface LanguagePlan {
  /** The checked 內容目標 as they now read. */
  content: string[];
  /** The decisions on the 內容目標 drafts (recorded once the 語言目標 are in). */
  decisions: DraftDecision[];
  /** Every draft's line as the teacher has it now, and the unchecked ones (kept if the 語言目標 do not come). */
  texts: Record<number, string>;
  unchecked: number[];
}

/** How often the drafting's progress is asked for. Tests shorten it. */
export const draftConfig = { pollMs: 1000 };

/** A key for one drafting request: 8–64 of A–Z a–z 0–9 - _. */
function progressKey(): string {
  const random = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `draft-${random.replace(/[^A-Za-z0-9_-]/g, "")}`.slice(0, 64);
}

/** What the generator does once the teacher is done with the drafts. */
export interface DraftOutcome {
  /** The two fields as they now are (one objective per line). */
  content: string;
  language: string;
  /** Which fields the drafts spoke for (a field with no draft is left as it was). */
  has: { content: boolean; language: boolean };
}

/**
 * G2: the drafting step before generation. `start` asks Manna for drafts; it resolves `false`
 * when there is nothing to show (AI is off) so the generator goes on at once. A material still
 * being read (OCR) is thrown for the generator to say beside its button; any other failure is
 * shown in the panel, with 「不用目標，直接產生」.
 */
export function useObjectiveDraft() {
  const [state, setState] = useState<DraftState | null>(null);
  const seq = useRef(0);
  const rounds = useRef(0);
  const last = useRef<ObjectiveDraftRequest | null>(null);
  /** The 語言目標 step follows a 內容目標 step of this panel (its lines are part of the outcome). */
  const carried = useRef(false);

  // While Manna drafts: ask how far she is (docs/API.md「產生進度」).
  const drafting = state?.phase === "drafting" ? state.key : null;
  useEffect(() => {
    if (drafting === null) return undefined;
    let alive = true;
    const timer = setInterval(() => {
      api
        .objectiveDraftProgress(drafting)
        .then((res) => {
          if (!alive || !res.progress) return;
          setState((now) => (now?.phase === "drafting" && now.key === drafting ? { ...now, progress: res.progress } : now));
        })
        .catch(() => undefined);
    }, draftConfig.pollMs);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [drafting]);

  function readyFrom(result: ObjectiveDraftResult, body: ObjectiveDraftRequest): DraftState {
    rounds.current += 1;
    return {
      phase: "ready",
      stage: body.stage ?? "content",
      decided: body.content ?? [],
      carried: carried.current,
      result,
      sources: Object.fromEntries(result.objectives.map((d) => [d.index, { interaction_id: result.interaction_id, index: d.index }])),
      edits: {},
      unchecked: [],
      round: rounds.current,
    };
  }

  async function run(body: ObjectiveDraftRequest): Promise<boolean> {
    const id = ++seq.current;
    last.current = body;
    const key = progressKey();
    setState({ phase: "drafting", key, progress: null, stage: body.stage ?? "content" });
    try {
      const result = await api.draftObjectives({ ...body, progress_id: key });
      if (id !== seq.current) return true;
      if (result.status === "disabled") {
        setState(null);
        return false;
      }
      setState(hasDrafts(result) ? readyFrom(result, body) : { phase: "failed", message: result.message ?? DRAFT_FAILED });
      return true;
    } catch (err) {
      if (id !== seq.current) return true;
      if (ocrBusyError(err)) {
        setState(null);
        throw err;
      }
      setState({ phase: "failed", message: errorMessage(err) });
      return true;
    }
  }

  /**
   * 換掉沒勾的: only the unchecked drafts are written again. The checked ones (as edited) go with
   * the request as `keep`, so the new ones do not repeat or clash with them; they stay in place,
   * and the unchecked ones are recorded as skipped. When Manna cannot, the panel is as it was.
   */
  async function redraw(plan: RedrawPlan): Promise<boolean> {
    const base = last.current;
    const before = state;
    if (!base || before?.phase !== "ready") return true;
    const drafts = before.result.objectives;
    const off = new Set(plan.redraw);
    const text = (d: ObjectiveDraft) => plan.texts[d.index] ?? d.text;
    const asWas = (message: string | null): DraftState => ({
      ...before,
      result: { ...before.result, message },
      edits: plan.texts,
      unchecked: [...plan.unchecked],
    });
    const id = ++seq.current;
    const key = progressKey();
    setState({ phase: "drafting", key, progress: null, stage: before.stage });
    try {
      const result = await api.draftObjectives({
        ...base,
        avoid: [...new Set(drafts.filter((d) => off.has(d.index)).flatMap((d) => [text(d), d.text]))],
        keep: drafts.filter((d) => !off.has(d.index)).map((d) => ({ code: d.code, text: text(d).trim() })),
        progress_id: key,
      });
      if (id !== seq.current) return true;
      if (!hasDrafts(result)) {
        setState(asWas(result.status === "disabled" ? REDRAW_FAILED : (result.message ?? REDRAW_FAILED)));
        return true;
      }
      const merged = mergeRedraw(drafts, off, result.objectives);
      const sources: Record<number, DraftSource> = {};
      const edits: Record<number, string> = {};
      for (const d of merged.drafts) {
        const from = merged.fresh[d.index];
        if (from === undefined) {
          sources[d.index] = before.sources[d.index] ?? { interaction_id: null, index: d.index };
          edits[d.index] = plan.texts[d.index] ?? d.text;
        } else {
          sources[d.index] = { interaction_id: result.interaction_id, index: from };
        }
      }
      void recordDecisions(
        decisionsAt(
          drafts.filter((d) => off.has(d.index)).map((d) => ({ item_index: d.index, decision: "skip" as const })),
          before.sources,
        ),
      );
      rounds.current += 1;
      setState({
        phase: "ready",
        stage: before.stage,
        decided: before.decided,
        carried: before.carried,
        result: { ...result, objectives: merged.drafts },
        sources,
        edits,
        unchecked: [],
        round: rounds.current,
      });
      return true;
    } catch (err) {
      if (id !== seq.current) return true;
      if (ocrBusyError(err)) {
        setState(null);
        throw err;
      }
      setState(asWas(errorMessage(err)));
      return true;
    }
  }

  /**
   * 請 Manna 擬語言目標: the 內容目標 she kept are sent, and the 語言目標 are drafted from them. The
   * decisions on the 內容目標 are recorded once they are in; when Manna cannot, the 內容目標 step
   * is as it was (the lines she edited, the boxes she unchecked), with a note.
   */
  async function toLanguageStep(plan: LanguagePlan): Promise<void> {
    const base = last.current;
    const before = state;
    if (!base || before?.phase !== "ready") return;
    const body: ObjectiveDraftRequest = { ...base, stage: "language", content: plan.content };
    delete body.avoid;
    delete body.keep;
    const id = ++seq.current;
    const key = progressKey();
    const asWas = (message: string): DraftState => {
      rounds.current += 1;
      return {
        ...before,
        result: { ...before.result, message },
        edits: plan.texts,
        unchecked: plan.unchecked,
        round: rounds.current,
      };
    };
    setState({ phase: "drafting", key, progress: null, stage: "language" });
    try {
      const result = await api.draftObjectives({ ...body, progress_id: key });
      if (id !== seq.current) return;
      if (!hasDrafts(result)) {
        setState(asWas(result.status === "disabled" ? LANGUAGE_FAILED : (result.message ?? LANGUAGE_FAILED)));
        return;
      }
      last.current = body;
      carried.current = true;
      void recordDecisions(plan.decisions);
      setState(readyFrom(result, body));
    } catch (err) {
      if (id !== seq.current) return;
      if (ocrBusyError(err)) {
        setState(null);
        throw err;
      }
      setState(asWas(errorMessage(err)));
    }
  }

  return {
    state,
    start: (body: ObjectiveDraftRequest) => {
      carried.current = false;
      return run({ stage: "content", ...body });
    },
    toLanguage: toLanguageStep,
    /**
     * 換一組: the same request, with the lines shown now to be written other than. With a `plan`
     * (some drafts checked, some not) only the unchecked ones are redrawn.
     */
    another: (plan?: RedrawPlan) => {
      const body = last.current;
      if (!body || state?.phase !== "ready") return Promise.resolve(true);
      if (plan) return redraw(plan);
      return run({ ...body, avoid: state.result.objectives.map((o) => o.text) });
    },
    /** Closes the panel; a draft still on its way is ignored. */
    close: () => {
      seq.current += 1;
      setState(null);
    },
  };
}

/** Records the teacher's choices; a failure never holds up the generation. */
export async function recordDecisions(decisions: DraftDecision[]): Promise<void> {
  if (decisions.length === 0) return;
  await Promise.allSettled(decisions.map((d) => api.sendDecision(d.interaction_id, d.request)));
}

/** What 換掉沒勾的 needs from the panel: which drafts to write again, and the lines as they read now. */
export interface RedrawPlan {
  /** `index` of the drafts to replace (unchecked, or checked with an empty box). */
  redraw: number[];
  /** Every draft's line as the teacher has it now. */
  texts: Record<number, string>;
  /** `index` of the unchecked ones (they stay unchecked if the redraw does not happen). */
  unchecked: number[];
}

interface ObjectiveDraftsProps {
  state: DraftState;
  disabled: boolean;
  /** 用勾選的 N 個目標(產生): the checked drafts as they now read (unchanged: adopted, edited: revised; unchecked: skipped). */
  onUse: (outcome: DraftOutcome, decisions: DraftDecision[]) => void;
  /** 不用目標，直接產生. */
  onSkip: (decisions: DraftDecision[]) => void;
  /** 換一組 (no plan) or 換掉沒勾的 (with one). */
  onAnother: (plan?: RedrawPlan) => void;
  /** The 內容目標 step's primary: draft the 語言目標 from the checked 內容目標. */
  onLanguage: (plan: LanguagePlan) => void;
  /** 收起: back to the form, nothing decided (nothing recorded). */
  onCancel: () => void;
  /** Inside a card of the page (/create), not a card of its own. */
  embedded?: boolean;
  /** Opened by 請 Manna 擬一組: the primary fills the two fields instead of generating. */
  fill?: boolean;
}

/** One draft: a checkbox to use it, Manna's line in an editable field (gold; dimmed when unchecked), with the students' Chinese and how to check it. */
function DraftField({
  draft,
  value,
  checked,
  disabled,
  onChange,
  onCheck,
}: {
  draft: ObjectiveDraft;
  value: string;
  checked: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
  onCheck: (checked: boolean) => void;
}) {
  const id = useId();
  const kind = OBJECTIVE_KIND_LABEL[draft.kind];
  const name = `${kind} ${draft.code}`;
  return (
    <div className={checked ? "manna-well objective-draft" : "manna-well objective-draft is-off"} role="group" aria-label={name}>
      <div className="objective-draft-head">
        <label className="manna-label" htmlFor={id}>
          <span className="star" aria-hidden="true" />
          {name}
          {draft.category ? `：${CATEGORY_LABEL[draft.category]}` : ""}
        </label>
        <label className="objective-draft-use">
          <input
            type="checkbox"
            checked={checked}
            disabled={disabled}
            aria-label={useCheckboxLabel(draft.kind, draft.code)}
            onChange={(e) => onCheck(e.target.checked)}
          />
          使用
        </label>
      </div>
      <textarea
        id={id}
        rows={2}
        lang="en"
        maxLength={OBJECTIVE_MAX}
        value={value}
        disabled={disabled}
        aria-describedby={`${id}-zh`}
        onChange={(e) => onChange(e.target.value)}
      />
      <div id={`${id}-zh`}>
        <p className="objective-draft-zh">{draft.zh}</p>
        {draft.relation_zh && <p className="why">{draft.relation_zh}</p>}
        {draft.check_zh && <p className="why">怎麼檢核：{draft.check_zh}</p>}
      </div>
    </div>
  );
}

function ReadyDrafts({
  state,
  disabled,
  fill,
  onUse,
  onSkip,
  onAnother,
  onLanguage,
}: Pick<ObjectiveDraftsProps, "disabled" | "fill" | "onUse" | "onSkip" | "onAnother" | "onLanguage"> & {
  state: Extract<DraftState, { phase: "ready" }>;
}) {
  const { result, sources } = state;
  const drafts = result.objectives;
  const noteId = useId();
  const [texts, setTexts] = useState<Record<number, string>>(() => ({
    ...Object.fromEntries(drafts.map((d) => [d.index, d.text])),
    ...state.edits,
  }));
  const [off, setOff] = useState<ReadonlySet<number>>(() => new Set(state.unchecked));
  const text = (d: ObjectiveDraft) => texts[d.index] ?? d.text;
  // A checked box with nothing written in it is not a goal to use.
  const using = drafts.filter((d) => !off.has(d.index) && text(d).trim() !== "");
  const n = using.length;
  const partial = n > 0 && n < drafts.length;
  const choices = () => Object.fromEntries(drafts.map((d) => [d.index, choiceFor(d, text(d))]));
  const unused = new Set(drafts.filter((d) => !using.includes(d)).map((d) => d.index));
  const note = checkedSetNote(drafts, using);
  // The 內容目標 step: there is no 語言目標 in the set yet, and it is drafted from the ones decided on here.
  const awaitingLanguage = state.stage === "content" && !drafts.some((d) => d.kind === "language");
  const inLanguage = state.stage === "language";
  const outcome = (): DraftOutcome => ({
    ...fieldsFrom(drafts, choices(), off),
    // The 語言目標 step leaves the 內容目標 as they were decided (they came with the step before).
    ...(inLanguage ? { content: state.decided.join("\n") } : {}),
    has: {
      content: inLanguage ? state.carried : drafts.some((d) => d.kind === "content"),
      language: drafts.some((d) => d.kind === "language"),
    },
  });
  const toggle = (index: number, on: boolean) =>
    setOff((all) => {
      const next = new Set(all);
      if (on) next.delete(index);
      else next.add(index);
      return next;
    });
  return (
    <>
      <p className="lead">{awaitingLanguage ? DRAFTS_CONTENT_LEAD : inLanguage ? DRAFTS_LANGUAGE_LEAD : DRAFTS_LEAD}</p>
      {inLanguage && (
        <div className="objective-decided" role="group" aria-label={DECIDED_CONTENT_HEADING}>
          <p className="manna-label">{DECIDED_CONTENT_HEADING}</p>
          <ul>
            {state.decided.map((line) => (
              <li key={line} lang="en">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}
      {result.message && <p className="quiet-note">{result.message}</p>}
      {(["content", "language"] as const).map((kind) =>
        drafts
          .filter((d) => d.kind === kind)
          .map((d) => (
            <DraftField
              key={d.index}
              draft={d}
              value={text(d)}
              checked={!off.has(d.index)}
              disabled={disabled}
              onChange={(value) => setTexts((all) => ({ ...all, [d.index]: value }))}
              onCheck={(on) => toggle(d.index, on)}
            />
          )),
      )}
      {note && (
        <p className="quiet-note" role="note">
          {note}
        </p>
      )}
      <div className="button-row objective-drafts-actions">
        <button
          type="button"
          className="primary-button"
          disabled={disabled || n === 0}
          aria-describedby={n === 0 ? noteId : undefined}
          onClick={() => {
            const decisions = decisionsAt(decisionsFor(drafts, choices(), false, off), sources);
            if (awaitingLanguage) {
              onLanguage({
                content: using.map((d) => text(d).trim()),
                decisions,
                texts: Object.fromEntries(drafts.map((d) => [d.index, text(d)])),
                unchecked: [...off],
              });
            } else {
              onUse(outcome(), decisions);
            }
          }}
        >
          {awaitingLanguage ? toLanguage(n) : useDrafts(n, fill ?? false)}
        </button>
        {awaitingLanguage && (
          <button
            type="button"
            className="text-link is-plain"
            disabled={disabled || n === 0}
            onClick={() => onUse(outcome(), decisionsAt(decisionsFor(drafts, choices(), false, off), sources))}
          >
            {onlyContent(n, fill ?? false)}
          </button>
        )}
        <button
          type="button"
          className="text-link is-plain"
          disabled={disabled}
          onClick={() =>
            onAnother(
              partial
                ? { redraw: [...unused], texts: Object.fromEntries(drafts.map((d) => [d.index, text(d)])), unchecked: [...off] }
                : undefined,
            )
          }
        >
          {partial ? REDRAW_UNCHECKED : ANOTHER_SET}
        </button>
        <button
          type="button"
          className="text-link is-plain"
          disabled={disabled}
          onClick={() => onSkip(decisionsAt(decisionsFor(drafts, choices(), true), sources))}
        >
          {fill ? SKIP_DRAFTS_FILL : SKIP_DRAFTS}
        </button>
      </div>
      {n === 0 && (
        <p className="quiet-note" id={noteId}>
          {fill ? NOTHING_CHECKED : NOTHING_CHECKED_GENERATE}
        </p>
      )}
    </>
  );
}

/**
 * G2/G3: Manna's drafts (only the ones that passed the R1 and R3 checks), each an editable line:
 * each with a checkbox (checked: used, unchanged = adopt, edited = revise; unchecked: skipped,
 * dimmed but still editable). One primary — 用勾選的 N 個目標(產生) — with 換一組 (換掉沒勾的 when
 * some are checked) and 不用目標，直接產生 as quiet links.
 */
export function ObjectiveDrafts({
  state,
  disabled,
  onUse,
  onSkip,
  onAnother,
  onLanguage,
  onCancel,
  embedded = false,
  fill = false,
}: ObjectiveDraftsProps) {
  const ids = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  // Opening (and each change of phase) takes focus to the heading and brings the panel into view.
  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
    scrollToEl(headingRef.current, "center");
  }, [state.phase]);
  const Tag = embedded ? "div" : "section";
  return (
    <Tag
      className={embedded ? "objective-drafts is-embedded" : "create-card objective-drafts"}
      {...(embedded ? { role: "region" } : {})}
      aria-labelledby={`${ids}-heading`}
      aria-busy={state.phase === "drafting"}
    >
      <div className="create-card-head">
        <h2 id={`${ids}-heading`} ref={headingRef} tabIndex={-1}>
          {DRAFTS_HEADING}
        </h2>
        <button type="button" className="text-link is-plain" disabled={disabled} onClick={onCancel}>
          收起
        </button>
      </div>
      <p className="sr-only" role="status">
        {state.phase === "ready"
          ? state.stage === "content" && !state.result.objectives.some((d) => d.kind === "language")
            ? DRAFTS_CONTENT_READY_LINE
            : fill
              ? DRAFTS_READY_FILL_LINE
              : DRAFTS_READY_LINE
          : ""}
      </p>
      {state.phase === "drafting" && (
        <>
          <RunProgressSlot
            progress={state.progress}
            runKey={state.key}
            fallback={
              <p className="manna-label" role="status">
                <span className="star is-working" aria-hidden="true" />
                {state.stage === "language" ? DRAFTING_LANGUAGE_LINE : DRAFTING_LINE}
              </p>
            }
          />
          {!fill && (
            <div className="button-row objective-drafts-actions">
              <button type="button" className="text-link is-plain" onClick={() => onSkip([])}>
                {SKIP_DRAFTS}
              </button>
            </div>
          )}
        </>
      )}
      {state.phase === "failed" && (
        <>
          <p className="quiet-note" role="status">
            {state.message}
          </p>
          {!fill && (
            <div className="button-row objective-drafts-actions">
              <button type="button" className="primary-button" disabled={disabled} onClick={() => onSkip([])}>
                {SKIP_DRAFTS}
              </button>
            </div>
          )}
        </>
      )}
      {state.phase === "ready" && (
        <ReadyDrafts
          key={state.round}
          state={state}
          disabled={disabled}
          fill={fill}
          onUse={onUse}
          onSkip={onSkip}
          onAnother={onAnother}
          onLanguage={onLanguage}
        />
      )}
    </Tag>
  );
}
