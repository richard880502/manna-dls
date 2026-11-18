import { useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  DecisionRequest,
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
  USE_DRAFTS,
  USE_DRAFTS_FILL,
  choiceFor,
  decisionsFor,
  fieldsFrom,
  hasDrafts,
} from "../../lib/objectives";

export type DraftState =
  /** `key`: the progress key sent with the request; `progress`: its steps, once the server has them. */
  | { phase: "drafting"; key: string; progress: RunProgress | null }
  | { phase: "ready"; result: ObjectiveDraftResult }
  | { phase: "failed"; message: string };

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
  const last = useRef<ObjectiveDraftRequest | null>(null);

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

  async function run(body: ObjectiveDraftRequest): Promise<boolean> {
    const id = ++seq.current;
    last.current = body;
    const key = progressKey();
    setState({ phase: "drafting", key, progress: null });
    try {
      const result = await api.draftObjectives({ ...body, progress_id: key });
      if (id !== seq.current) return true;
      if (result.status === "disabled") {
        setState(null);
        return false;
      }
      setState(hasDrafts(result) ? { phase: "ready", result } : { phase: "failed", message: result.message ?? DRAFT_FAILED });
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

  return {
    state,
    start: run,
    /** 換一組: the same request, with the lines shown now to be written other than. */
    another: () => {
      const body = last.current;
      if (!body || state?.phase !== "ready") return Promise.resolve(true);
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
export async function recordDecisions(interactionId: number | null, decisions: DecisionRequest[]): Promise<void> {
  if (interactionId === null || decisions.length === 0) return;
  await Promise.allSettled(decisions.map((d) => api.sendDecision(interactionId, d)));
}

interface ObjectiveDraftsProps {
  state: DraftState;
  disabled: boolean;
  /** 用這組目標(產生): the drafts as they now read (unchanged: adopted, edited: revised, cleared: rejected). */
  onUse: (outcome: DraftOutcome, decisions: DecisionRequest[], interactionId: number | null) => void;
  /** 不用目標，直接產生. */
  onSkip: (decisions: DecisionRequest[], interactionId: number | null) => void;
  onAnother: () => void;
  /** 收起: back to the form, nothing decided (nothing recorded). */
  onCancel: () => void;
  /** Inside a card of the page (/create), not a card of its own. */
  embedded?: boolean;
  /** Opened by 請 Manna 擬一組: the primary fills the two fields instead of generating. */
  fill?: boolean;
}

/** One draft: Manna's line in an editable field (gold), with the students' Chinese and how to check it. */
function DraftField({
  draft,
  value,
  disabled,
  onChange,
}: {
  draft: ObjectiveDraft;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const id = useId();
  const kind = OBJECTIVE_KIND_LABEL[draft.kind];
  const name = `${kind} ${draft.code}`;
  return (
    <div className="manna-well objective-draft" role="group" aria-label={name}>
      <label className="manna-label" htmlFor={id}>
        <span className="star" aria-hidden="true" />
        {name}
        {draft.category ? ` · ${CATEGORY_LABEL[draft.category]}` : ""}
      </label>
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
  result,
  disabled,
  fill,
  onUse,
  onSkip,
  onAnother,
}: Pick<ObjectiveDraftsProps, "disabled" | "fill" | "onUse" | "onSkip" | "onAnother"> & { result: ObjectiveDraftResult }) {
  const drafts = result.objectives;
  const [texts, setTexts] = useState<Record<number, string>>(() => Object.fromEntries(drafts.map((d) => [d.index, d.text])));
  const choices = () => Object.fromEntries(drafts.map((d) => [d.index, choiceFor(d, texts[d.index] ?? d.text)]));
  return (
    <>
      <p className="lead">{DRAFTS_LEAD}</p>
      {result.message && <p className="quiet-note">{result.message}</p>}
      {(["content", "language"] as const).map((kind) =>
        drafts
          .filter((d) => d.kind === kind)
          .map((d) => (
            <DraftField
              key={d.index}
              draft={d}
              value={texts[d.index] ?? d.text}
              disabled={disabled}
              onChange={(value) => setTexts((all) => ({ ...all, [d.index]: value }))}
            />
          )),
      )}
      <div className="button-row objective-drafts-actions">
        <button
          type="button"
          className="primary-button"
          disabled={disabled}
          onClick={() =>
            onUse(
              {
                ...fieldsFrom(drafts, choices()),
                has: { content: drafts.some((d) => d.kind === "content"), language: drafts.some((d) => d.kind === "language") },
              },
              decisionsFor(drafts, choices(), false),
              result.interaction_id,
            )
          }
        >
          {fill ? USE_DRAFTS_FILL : USE_DRAFTS}
        </button>
        <button type="button" className="text-link is-plain" disabled={disabled} onClick={onAnother}>
          {ANOTHER_SET}
        </button>
        <button
          type="button"
          className="text-link is-plain"
          disabled={disabled}
          onClick={() => onSkip(decisionsFor(drafts, choices(), true), result.interaction_id)}
        >
          {fill ? SKIP_DRAFTS_FILL : SKIP_DRAFTS}
        </button>
      </div>
    </>
  );
}

/**
 * G2/G3: Manna's drafts (only the ones that passed the R1 and R3 checks), each an editable line:
 * leave it (adopt), edit it (revise) or clear it (not used). One primary — 用這組目標(產生) —
 * with 換一組 and 不用目標，直接產生 as quiet links.
 */
export function ObjectiveDrafts({ state, disabled, onUse, onSkip, onAnother, onCancel, embedded = false, fill = false }: ObjectiveDraftsProps) {
  const ids = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  // Opening (and each change of phase) takes focus to the heading and brings the panel into view.
  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
    headingRef.current?.scrollIntoView?.({ block: "center", behavior: "smooth" });
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
        {state.phase === "ready" ? (fill ? DRAFTS_READY_FILL_LINE : DRAFTS_READY_LINE) : ""}
      </p>
      {state.phase === "drafting" && (
        <>
          <RunProgressSlot
            progress={state.progress}
            runKey={state.key}
            fallback={
              <p className="manna-label" role="status">
                <span className="star is-working" aria-hidden="true" />
                {DRAFTING_LINE}
              </p>
            }
          />
          {!fill && (
            <div className="button-row objective-drafts-actions">
              <button type="button" className="text-link is-plain" onClick={() => onSkip([], null)}>
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
              <button type="button" className="primary-button" disabled={disabled} onClick={() => onSkip([], null)}>
                {SKIP_DRAFTS}
              </button>
            </div>
          )}
        </>
      )}
      {state.phase === "ready" && (
        <ReadyDrafts
          key={state.result.interaction_id ?? 0}
          result={state.result}
          disabled={disabled}
          fill={fill}
          onUse={onUse}
          onSkip={onSkip}
          onAnother={onAnother}
        />
      )}
    </Tag>
  );
}
