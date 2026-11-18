import { useId } from "react";
import {
  ASK_MANNA,
  CONTENT_OBJECTIVE_LABEL,
  CONTENT_OBJECTIVE_PLACEHOLDER,
  LANGUAGE_OBJECTIVE_LABEL,
  LANGUAGE_OBJECTIVE_PLACEHOLDER,
  OBJECTIVES_HINT,
  OBJECTIVE_MAX,
} from "../../lib/objectives";

interface ObjectiveFieldsProps {
  content: string;
  language: string;
  disabled: boolean;
  onContent: (value: string) => void;
  onLanguage: (value: string) => void;
  /** Said under the two fields (the generator's: Manna drafts them when both are empty). */
  hint?: string;
  /** 請 Manna 擬一組, beside the first label (the generator's: drafts without pressing 產生). */
  onAsk?: () => void;
}

/**
 * G1: 內容目標 and 語言目標, two optional fields (one objective per line). Used by every /create
 * tool and the worksheet's options view.
 */
export function ObjectiveFields({ content, language, disabled, onContent, onLanguage, hint = OBJECTIVES_HINT, onAsk }: ObjectiveFieldsProps) {
  const ids = useId();
  return (
    <div className="objective-fields">
      <div className="objective-fields-head">
        <label className="field-label" htmlFor={`${ids}-content`}>
          {CONTENT_OBJECTIVE_LABEL}
        </label>
        {onAsk && (
          <button type="button" className="text-link is-plain" disabled={disabled} onClick={onAsk}>
            {ASK_MANNA}
          </button>
        )}
      </div>
      <textarea
        id={`${ids}-content`}
        rows={2}
        maxLength={OBJECTIVE_MAX}
        value={content}
        placeholder={CONTENT_OBJECTIVE_PLACEHOLDER}
        disabled={disabled}
        aria-describedby={`${ids}-hint`}
        onChange={(e) => onContent(e.target.value)}
      />
      <label className="field-label" htmlFor={`${ids}-language`}>
        {LANGUAGE_OBJECTIVE_LABEL}
      </label>
      <textarea
        id={`${ids}-language`}
        rows={2}
        maxLength={OBJECTIVE_MAX}
        value={language}
        placeholder={LANGUAGE_OBJECTIVE_PLACEHOLDER}
        disabled={disabled}
        aria-describedby={`${ids}-hint`}
        onChange={(e) => onLanguage(e.target.value)}
      />
      <p className="field-hint" id={`${ids}-hint`}>
        {hint}
      </p>
    </div>
  );
}
