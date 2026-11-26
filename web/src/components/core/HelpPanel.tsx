import type { AiResult, AssistLevel, CoreAssistOutput, CoreQuestion, Thumbs } from "../../api/types";
import { ASSIST_LEVEL_LABELS } from "../../lib/core";
import { AiContextPanel } from "../AiContextPanel";
import { AiStatusMessage, AiUnavailableNote, ErrorText } from "../common";

export interface HelpEntry {
  level: AssistLevel;
  result: AiResult<CoreAssistOutput>;
  shownAt: number;
  decision?: "answer" | "skip";
  thumbs?: Thumbs;
  decisionError?: string | null;
}

export interface HelpState {
  entries: HelpEntry[];
  loading: boolean;
  error: string | null;
}

interface HelpPanelProps {
  question: CoreQuestion | null;
  focusLabel: string;
  help: HelpState | undefined;
  aiEnabled: boolean;
  onContinue: (level: AssistLevel) => void;
  onDecision: (entryIndex: number, decision: "answer" | "skip") => void;
  onThumbs: (entryIndex: number, thumbs: Thumbs) => void;
  onClose: () => void;
}

/** Rating icon for the 有幫助／沒有幫助 buttons (their names come from aria-label). */
function ThumbIcon({ down = false }: { down?: boolean }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={down ? { transform: "rotate(180deg)" } : undefined}
    >
      <path d="M6.5 9v8h-2a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1zm0 0 3-5.5c1.2 0 2.1 1 1.9 2.2L11 8h4a1.5 1.5 0 0 1 1.5 1.8l-1.1 5.6A2 2 0 0 1 13.4 17H6.5" />
    </svg>
  );
}

export function HelpPanel({
  question,
  focusLabel,
  help,
  aiEnabled,
  onContinue,
  onDecision,
  onThumbs,
  onClose,
}: HelpPanelProps) {
  if (!question) {
    return (
      <aside className="help-panel is-idle" aria-label="需要協助">
        <div className="manna-dialog">
          <div className="manna-avatar star-mascot" aria-hidden="true">
            <span className="star" />
          </div>
          <div>
            <strong>需要協助時再找我</strong>
            <p>按任一題旁的「需要協助」，題意說明與 Manna 的提示會出現在這裡。Manna 不會替你填寫答案。</p>
          </div>
        </div>
      </aside>
    );
  }

  const entries = help?.entries ?? [];
  const last = entries[entries.length - 1];
  const nextLevel = last && last.level < 3 && last.result.status !== "disabled" ? ((last.level + 1) as AssistLevel) : null;

  return (
    <aside className="help-panel is-open" aria-label="需要協助" aria-live="polite">
      <div className="card-heading">
        <div>
          <p className="eyebrow warm">{focusLabel}</p>
          <h3>
            {question.id} {question.prompt}
          </h3>
        </div>
        <button type="button" className="ghost-button small-button" onClick={onClose}>
          關閉
        </button>
      </div>

      <div className="help-explain">
        <span>題意說明</span>
        <p>{question.help}</p>
      </div>

      {!aiEnabled && entries.length === 0 && <AiUnavailableNote />}

      {entries.map((entry, index) => (
        <article key={`${entry.level}-${index}`} className="help-entry" aria-label={`第 ${entry.level} 層`}>
          <div className="help-entry-head">
            <span className="help-level">第 {entry.level} 層</span>
            <strong>{ASSIST_LEVEL_LABELS[entry.level]}</strong>
          </div>
          {entry.result.status === "shown" && entry.result.output ? (
            <>
              <p className="ai-text" data-testid="ai-help-text">
                {entry.result.output.text}
              </p>
              <p className="field-hint">這是提示，不是答案；請用你自己的話寫在這一題的答題欄位。</p>
              {entry.result.interaction_id !== null && (
                <div className="ai-decision">
                  <button
                    type="button"
                    className={`decision-button${entry.decision === "answer" ? " is-selected" : ""}`}
                    aria-pressed={entry.decision === "answer"}
                    onClick={() => onDecision(index, "answer")}
                  >
                    回答
                  </button>
                  <button
                    type="button"
                    className={`decision-button${entry.decision === "skip" ? " is-selected" : ""}`}
                    aria-pressed={entry.decision === "skip"}
                    onClick={() => onDecision(index, "skip")}
                  >
                    略過
                  </button>
                  <span className="thumbs-label">這個提示有幫助嗎？（選填）</span>
                  <button
                    type="button"
                    className={`decision-button thumb${entry.thumbs === "up" ? " is-selected" : ""}`}
                    aria-label="有幫助"
                    aria-pressed={entry.thumbs === "up"}
                    onClick={() => onThumbs(index, "up")}
                  >
                    <ThumbIcon />
                  </button>
                  <button
                    type="button"
                    className={`decision-button thumb${entry.thumbs === "down" ? " is-selected" : ""}`}
                    aria-label="沒有幫助"
                    aria-pressed={entry.thumbs === "down"}
                    onClick={() => onThumbs(index, "down")}
                  >
                    <ThumbIcon down />
                  </button>
                </div>
              )}
              <ErrorText message={entry.decisionError} />
              <AiContextPanel items={entry.result.context} />
            </>
          ) : (
            <AiStatusMessage status={entry.result.status} message={entry.result.message} />
          )}
        </article>
      ))}

      {help?.loading && <p className="loading-text">Manna 思考中…</p>}
      <ErrorText message={help?.error} />

      {aiEnabled && !help?.loading && entries.length === 0 && (
        <div className="help-continue">
          <button type="button" className="secondary-button" onClick={() => onContinue(1)}>
            請 Manna {ASSIST_LEVEL_LABELS[1]}
          </button>
        </div>
      )}

      {aiEnabled && !help?.loading && nextLevel && (
        <div className="help-continue">
          <p className="field-hint">要不要繼續下一層協助？由你決定。</p>
          <button type="button" className="secondary-button" onClick={() => onContinue(nextLevel)}>
            繼續：{ASSIST_LEVEL_LABELS[nextLevel]}
          </button>
          <button type="button" className="ghost-button" onClick={onClose}>
            先到這裡
          </button>
        </div>
      )}
    </aside>
  );
}
