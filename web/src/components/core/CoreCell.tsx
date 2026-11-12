import type { ContentSource, CoreQuestion } from "../../api/types";
import type { CellState } from "../../lib/core";
import { CONTENT_SOURCE_LABELS } from "../../lib/labels";

const SOURCES: ContentSource[] = ["self", "self_after_ai", "modified_ai_example", "adopted_ai_example"];

interface CoreCellProps {
  question: CoreQuestion;
  cellId: string;
  cell: CellState;
  isHelpActive: boolean;
  onAnswer: (value: string) => void;
  onToggleNoIdea: (on: boolean) => void;
  onSource: (source: ContentSource) => void;
  onHelp: () => void;
}

export function CoreCell({
  question,
  cellId,
  cell,
  isHelpActive,
  onAnswer,
  onToggleNoIdea,
  onSource,
  onHelp,
}: CoreCellProps) {
  const labelId = `${cellId}-label`;
  return (
    <fieldset
      className={`core-cell${cell.noIdea ? " is-no-idea" : ""}${isHelpActive ? " is-help-active" : ""}`}
      aria-labelledby={labelId}
    >
      <div className="core-cell-head">
        <label id={labelId} htmlFor={`${cellId}-answer`} className="core-question">
          <span className="core-qid">{question.id}</span>
          {question.prompt}
        </label>
        <button
          type="button"
          className="ghost-button help-button"
          onClick={onHelp}
          aria-expanded={isHelpActive}
        >
          需要協助
        </button>
      </div>
      <textarea
        id={`${cellId}-answer`}
        rows={3}
        value={cell.answer}
        disabled={cell.noIdea}
        placeholder={cell.noIdea ? "已標記為「目前沒有想法」，取消後可繼續書寫。" : "可以留白，想到再寫。"}
        onChange={(e) => onAnswer(e.target.value)}
      />
      <div className="core-cell-tools">
        <label className="check-row no-idea-toggle">
          <input
            type="checkbox"
            checked={cell.noIdea}
            onChange={(e) => onToggleNoIdea(e.target.checked)}
          />
          目前沒有想法
        </label>
        {cell.sawAi && (
          <label className="source-select">
            <span>這格內容的來源</span>
            <select
              aria-label="內容來源"
              value={cell.contentSource}
              onChange={(e) => onSource(e.target.value as ContentSource)}
            >
              {SOURCES.map((s) => (
                <option key={s} value={s}>
                  {CONTENT_SOURCE_LABELS[s]}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
    </fieldset>
  );
}
