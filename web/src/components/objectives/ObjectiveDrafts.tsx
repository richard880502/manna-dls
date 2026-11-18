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
  CHOICE_LABEL,
  DRAFTING_LINE,
  DRAFTS_HEADING,
  DRAFTS_LEAD,
  DRAFT_FAILED,
  OBJECTIVE_KIND_LABEL,
  SKIP_DRAFTS,
  USE_ALL,
  USE_THESE,
  decisionsFor,
  fieldsFrom,
  finalText,
  hasDrafts,
  type DraftChoice,
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

interface DraftRowProps {
  draft: ObjectiveDraft;
  choice: DraftChoice | undefined;
  disabled: boolean;
  onChoose: (choice: DraftChoice | undefined) => void;
}

/** One draft: the students' line, the teacher's Chinese, (語言目標) its kind and relation, how to check, and 採用／修改／改寫. */
function DraftRow({ draft, choice, disabled, onChoose }: DraftRowProps) {
  const ids = useId();
  const [editing, setEditing] = useState<"revise" | "rewrite" | null>(null);
  const [text, setText] = useState("");
  const editRef = useRef<HTMLTextAreaElement>(null);
  const kind = OBJECTIVE_KIND_LABEL[draft.kind];
  const name = `${kind} ${draft.code}`;
  const shown = finalText(draft, choice);
  // 修改／改寫 opens the box with the cursor in it.
  useEffect(() => {
    if (editing) editRef.current?.focus();
  }, [editing]);

  function edit(mode: "revise" | "rewrite") {
    setEditing(mode);
    setText(mode === "revise" ? (choice && choice.decision !== "adopt" ? choice.text : draft.text) : "");
  }

  return (
    <div className="manna-well objective-draft" role="group" aria-label={name}>
      <p className="manna-label">
        <span className="star" aria-hidden="true" />
        {kind}
        {draft.category ? ` · ${CATEGORY_LABEL[draft.category]}` : ""}
        {choice && <span className="soft-tag">{CHOICE_LABEL[choice.decision]}</span>}
      </p>
      {choice && choice.decision !== "adopt" ? (
        <p className="objective-draft-text" lang="en">
          {shown ?? <span className="quiet-note">（不放這一項）</span>}
        </p>
      ) : (
        <>
          <p className="objective-draft-text" lang="en">
            {draft.text}
          </p>
          <p className="objective-draft-zh">{draft.zh}</p>
          {draft.relation_zh && <p className="why">{draft.relation_zh}</p>}
          {draft.check_zh && <p className="why">怎麼檢核：{draft.check_zh}</p>}
        </>
      )}
      {editing ? (
        <form
          className="objective-draft-edit"
          onSubmit={(e) => {
            e.preventDefault();
            onChoose({ decision: editing, text });
            setEditing(null);
          }}
        >
          <label className="sr-only" htmlFor={`${ids}-edit`}>
            {editing === "revise" ? `修改${name}` : `改寫${name}`}
          </label>
          <textarea
            id={`${ids}-edit`}
            ref={editRef}
            rows={2}
            lang="en"
            value={text}
            placeholder={editing === "rewrite" ? `寫你自己的${kind}（留空＝不放這一項）` : undefined}
            onChange={(e) => setText(e.target.value)}
          />
          <div className="decide">
            <button type="submit" className="decide-button is-take">
              確定
            </button>
            <button type="button" className="decide-button" onClick={() => setEditing(null)}>
              取消
            </button>
          </div>
        </form>
      ) : (
        <div className="decide">
          {choice ? (
            <button type="button" className="decide-button" disabled={disabled} onClick={() => onChoose(undefined)}>
              重選
            </button>
          ) : (
            <>
              <button
                type="button"
                className="decide-button is-take"
                aria-label={`採用${name}`}
                disabled={disabled}
                onClick={() => onChoose({ decision: "adopt" })}
              >
                採用
              </button>
              <button type="button" className="decide-button" aria-label={`修改${name}`} disabled={disabled} onClick={() => edit("revise")}>
                修改
              </button>
              <button type="button" className="decide-button" aria-label={`改寫${name}`} disabled={disabled} onClick={() => edit("rewrite")}>
                改寫
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

interface ObjectiveDraftsProps {
  state: DraftState;
  disabled: boolean;
  /** 採用並產生 (undecided drafts count as adopted). */
  onUse: (outcome: DraftOutcome, decisions: DecisionRequest[], interactionId: number | null) => void;
  /** 不用目標，直接產生. */
  onSkip: (decisions: DecisionRequest[], interactionId: number | null) => void;
  onAnother: () => void;
  /** 收起: back to the form, nothing decided (nothing recorded). */
  onCancel: () => void;
}

function ReadyDrafts({
  result,
  disabled,
  onUse,
  onSkip,
  onAnother,
}: Omit<ObjectiveDraftsProps, "state" | "onCancel"> & { result: ObjectiveDraftResult }) {
  const [choices, setChoices] = useState<Record<number, DraftChoice>>({});
  const drafts = result.objectives;
  const undecided = drafts.filter((d) => !choices[d.index]).length;
  return (
    <>
      <p className="lead">{DRAFTS_LEAD}</p>
      {result.message && <p className="quiet-note">{result.message}</p>}
      {(["content", "language"] as const).map((kind) =>
        drafts
          .filter((d) => d.kind === kind)
          .map((d) => (
            <DraftRow
              key={d.index}
              draft={d}
              choice={choices[d.index]}
              disabled={disabled}
              onChoose={(choice) =>
                setChoices((all) => {
                  const next = { ...all };
                  if (choice) next[d.index] = choice;
                  else delete next[d.index];
                  return next;
                })
              }
            />
          )),
      )}
      <div className="button-row objective-drafts-actions">
        <button
          type="button"
          className="primary-button"
          disabled={disabled}
          onClick={() => onUse(fieldsFrom(drafts, choices), decisionsFor(drafts, choices, false), result.interaction_id)}
        >
          {undecided === drafts.length ? USE_ALL : USE_THESE}
        </button>
        <button
          type="button"
          className="secondary-button"
          disabled={disabled}
          onClick={() => onSkip(decisionsFor(drafts, choices, true), result.interaction_id)}
        >
          {SKIP_DRAFTS}
        </button>
        <button type="button" className="text-link is-plain" disabled={disabled} onClick={onAnother}>
          {ANOTHER_SET}
        </button>
      </div>
    </>
  );
}

/**
 * G2/G3 on the generator: Manna's drafts (only the ones that passed the R1 and R3 checks), each
 * to 採用、修改 or 改寫, then 全部採用並產生 in one click — or 不用目標，直接產生.
 */
export function ObjectiveDrafts({ state, disabled, onUse, onSkip, onAnother, onCancel }: ObjectiveDraftsProps) {
  const ids = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    headingRef.current?.scrollIntoView?.({ block: "center", behavior: "smooth" });
  }, [state.phase]);
  return (
    <section className="create-card objective-drafts" aria-labelledby={`${ids}-heading`} aria-busy={state.phase === "drafting"}>
      <div className="create-card-head">
        <h2 id={`${ids}-heading`} ref={headingRef}>
          {DRAFTS_HEADING}
        </h2>
        <button type="button" className="text-link is-plain" disabled={disabled} onClick={onCancel}>
          收起
        </button>
      </div>
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
          <div className="button-row objective-drafts-actions">
            <button type="button" className="secondary-button" onClick={() => onSkip([], null)}>
              {SKIP_DRAFTS}
            </button>
          </div>
        </>
      )}
      {state.phase === "failed" && (
        <>
          <p className="quiet-note" role="status">
            {state.message}
          </p>
          <div className="button-row objective-drafts-actions">
            <button type="button" className="secondary-button" disabled={disabled} onClick={() => onSkip([], null)}>
              {SKIP_DRAFTS}
            </button>
          </div>
        </>
      )}
      {state.phase === "ready" && (
        <ReadyDrafts
          key={state.result.interaction_id ?? 0}
          result={state.result}
          disabled={disabled}
          onUse={onUse}
          onSkip={onSkip}
          onAnother={onAnother}
        />
      )}
    </section>
  );
}
