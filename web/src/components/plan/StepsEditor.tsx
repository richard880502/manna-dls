import type { IcapLevel } from "../../api/types";
import { ACTIVITY_TYPE_OPTIONS, ICAP_LABELS, ICAP_LEVELS } from "../../lib/labels";
import { PrivacyHint } from "../common";

export interface EditableStep {
  key: string;
  activity_type: string;
  icap_level: IcapLevel | null;
  content: string;
}

let stepCounter = 0;
export function newStep(partial: Partial<Omit<EditableStep, "key">> = {}): EditableStep {
  stepCounter += 1;
  return {
    key: `s${stepCounter}`,
    activity_type: partial.activity_type ?? "",
    icap_level: partial.icap_level ?? null,
    content: partial.content ?? "",
  };
}

interface StepsEditorProps {
  objective: string;
  steps: EditableStep[];
  onObjective: (value: string) => void;
  onSteps: (steps: EditableStep[]) => void;
}

export function StepsEditor({ objective, steps, onObjective, onSteps }: StepsEditorProps) {
  const update = (key: string, change: Partial<EditableStep>) =>
    onSteps(steps.map((s) => (s.key === key ? { ...s, ...change } : s)));
  const move = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= steps.length) return;
    const next = [...steps];
    const [item] = next.splice(index, 1);
    if (item) next.splice(target, 0, item);
    onSteps(next);
  };

  return (
    <div className="steps-editor">
      <label className="field-label" htmlFor="lesson-objective">
        本課教學目標
      </label>
      <textarea
        id="lesson-objective"
        rows={2}
        value={objective}
        onChange={(e) => onObjective(e.target.value)}
        placeholder="例如：學生能比較一般語序與句首倒裝，並用一句話說明倒裝的語意功能。"
      />

      <datalist id="activity-type-options">
        {ACTIVITY_TYPE_OPTIONS.map((o) => (
          <option key={o} value={o} aria-label={o} />
        ))}
      </datalist>

      <div className="field-label">教案步驟</div>
      {steps.length === 0 && <p className="empty-preview">還沒有步驟。按「新增步驟」開始，或從右側建議採用。</p>}
      <ol className="step-list">
        {steps.map((step, index) => (
          <li key={step.key} className="step-row" aria-label={`步驟 ${index + 1}`}>
            <div className="step-row-head">
              <span className="step-number">步驟 {index + 1}</span>
              <div className="step-row-buttons">
                <button
                  type="button"
                  className="ghost-button small-button"
                  aria-label={`步驟 ${index + 1} 上移`}
                  onClick={() => move(index, -1)}
                  disabled={index === 0}
                >
                  ↑
                </button>
                <button
                  type="button"
                  className="ghost-button small-button"
                  aria-label={`步驟 ${index + 1} 下移`}
                  onClick={() => move(index, 1)}
                  disabled={index === steps.length - 1}
                >
                  ↓
                </button>
                <button
                  type="button"
                  className="ghost-button small-button"
                  aria-label={`刪除步驟 ${index + 1}`}
                  onClick={() => onSteps(steps.filter((s) => s.key !== step.key))}
                >
                  刪除
                </button>
              </div>
            </div>
            <div className="two-fields">
              <div>
                <label className="field-label" htmlFor={`${step.key}-type`}>
                  活動型態
                </label>
                <input
                  id={`${step.key}-type`}
                  list="activity-type-options"
                  value={step.activity_type}
                  onChange={(e) => update(step.key, { activity_type: e.target.value })}
                  placeholder="例如：兩兩討論"
                />
              </div>
              <div>
                <label className="field-label" htmlFor={`${step.key}-icap`}>
                  ICAP 層次
                </label>
                <select
                  id={`${step.key}-icap`}
                  value={step.icap_level ?? ""}
                  onChange={(e) =>
                    update(step.key, { icap_level: e.target.value ? (e.target.value as IcapLevel) : null })
                  }
                >
                  <option value="">未設定</option>
                  {ICAP_LEVELS.map((level) => (
                    <option key={level} value={level}>
                      {ICAP_LABELS[level]}
                    </option>
                  ))}
                </select>
              </div>
            </div>
            <label className="field-label" htmlFor={`${step.key}-content`}>
              步驟內容
            </label>
            <textarea
              id={`${step.key}-content`}
              rows={3}
              value={step.content}
              onChange={(e) => update(step.key, { content: e.target.value })}
            />
          </li>
        ))}
      </ol>
      <button type="button" className="secondary-button" onClick={() => onSteps([...steps, newStep()])}>
        ＋ 新增步驟
      </button>
      <PrivacyHint />
    </div>
  );
}
