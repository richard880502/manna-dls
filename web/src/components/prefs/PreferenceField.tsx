import type { PreferenceAnswers, PreferenceQuestion } from "../../api/types";
import { CLASS_PROFILE_MAX_CHARS, answerValue, isTextQuestion, withAnswer } from "../../lib/preferences";

const PRIVACY_HINT = "不要寫學生姓名或個人資料。";

interface PreferenceFieldProps {
  question: PreferenceQuestion;
  answers: PreferenceAnswers;
  onChange: (next: PreferenceAnswers) => void;
  disabled?: boolean;
}

/** One survey question (P1–P6): radio group, checkbox group (multi) or the P6 free-text class profile. */
export function PreferenceField({ question, answers, onChange, disabled = false }: PreferenceFieldProps) {
  const value = answerValue(answers, question.id);
  const fieldId = `pref-${question.id}`;

  if (isTextQuestion(question)) {
    const text = typeof value === "string" ? value : "";
    // The server's P6 prompt already carries the privacy hint; only add it when it is missing.
    const promptHasHint = question.prompt.includes("不要寫學生姓名");
    return (
      <div className="pref-question">
        <label className="field-label pref-prompt" htmlFor={fieldId}>
          <span className="pref-qid">{question.id}</span>
          {question.prompt}
        </label>
        <textarea
          id={fieldId}
          rows={3}
          value={text}
          maxLength={CLASS_PROFILE_MAX_CHARS}
          disabled={disabled}
          placeholder="例如：高一普通班，程度落差大；多數學生閱讀可以，但口說比較害羞。"
          onChange={(e) => onChange(withAnswer(answers, question.id, e.target.value))}
        />
        <p className="field-hint">
          {promptHasHint ? "" : PRIVACY_HINT}
          這段文字會成為新單元的預設班級特性，並在 AI 呼叫時自動帶入。（{text.length.toLocaleString()} /{" "}
          {CLASS_PROFILE_MAX_CHARS.toLocaleString()} 字）
        </p>
      </div>
    );
  }

  const selected = Array.isArray(value) ? value : value ? [value] : [];

  function toggle(optionValue: string, checked: boolean) {
    if (question.multi) {
      const next = checked ? [...selected, optionValue] : selected.filter((v) => v !== optionValue);
      onChange(withAnswer(answers, question.id, next));
    } else {
      onChange(withAnswer(answers, question.id, checked ? optionValue : ""));
    }
  }

  return (
    <fieldset className="pref-question" disabled={disabled}>
      <legend className="field-label pref-prompt">
        <span className="pref-qid">{question.id}</span>
        {question.prompt}
      </legend>
      <div className="pref-options">
        {question.options.map((option) => (
          <label key={option.value} className="pref-option">
            <input
              type={question.multi ? "checkbox" : "radio"}
              name={fieldId}
              value={option.value}
              checked={selected.includes(option.value)}
              onChange={(e) => toggle(option.value, e.target.checked)}
            />
            <span>{option.label}</span>
          </label>
        ))}
      </div>
      {question.multi && <p className="field-hint">可複選，也可以都不選。</p>}
    </fieldset>
  );
}
