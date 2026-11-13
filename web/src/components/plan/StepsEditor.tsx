import type { IcapLevel } from "../../api/types";
import { ACTIVITY_TYPE_OPTIONS, ICAP_LABELS, ICAP_LEVELS } from "../../lib/labels";
import { PrivacyHint } from "../common";

export interface EditableStep {
  key: string;
  activity_type: string;
  icap_level: IcapLevel | null;
  content: string;
  duration_min: number | null;
  support_note: string;
  extension_note: string;
}

let stepCounter = 0;
export function newStep(partial: Partial<Omit<EditableStep, "key">> = {}): EditableStep {
  stepCounter += 1;
  return {
    key: `s${stepCounter}`,
    activity_type: partial.activity_type ?? "",
    icap_level: partial.icap_level ?? null,
    content: partial.content ?? "",
    duration_min: partial.duration_min ?? null,
    support_note: partial.support_note ?? "",
    extension_note: partial.extension_note ?? "",
  };
}

export function hasDiffData(s: EditableStep): boolean {
  return s.duration_min !== null || !!s.support_note.trim() || !!s.extension_note.trim();
}

/** Server accepts 0–600 minutes per step. */
export const MAX_STEP_MINUTES = 600;

/** "" → null; negative / non-numeric → null; otherwise whole minutes, capped at 600. */
export function parseMinutes(raw: string): number | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const n = Number(trimmed);
  if (!Number.isFinite(n) || n < 0) return null;
  return Math.min(MAX_STEP_MINUTES, Math.round(n));
}

export function totalMinutes(steps: EditableStep[]): number {
  return steps.reduce((sum, s) => sum + (s.duration_min ?? 0), 0);
}

interface StepsEditorProps {
  steps: EditableStep[];
  onSteps: (steps: EditableStep[]) => void;
  /** 預估時間／程度較弱的支援／程度較好的延伸 fields visible. */
  showDiff: boolean;
  /** The fields are open because of the teacher's preference (P3 差異化與時間安排). */
  diffByPreference: boolean;
  onToggleDiff: () => void;
}

/** The lesson's teaching steps. The objective (今天要教什麼？) sits above, on the lesson page's first view. */
export function StepsEditor({ steps, onSteps, showDiff, diffByPreference, onToggleDiff }: StepsEditorProps) {
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
  const timed = steps.filter((s) => s.duration_min !== null).length;

  return (
    <div className="steps-editor">
      <datalist id="activity-type-options">
        {ACTIVITY_TYPE_OPTIONS.map((o) => (
          <option key={o} value={o} aria-label={o} />
        ))}
      </datalist>

      <div className="steps-heading">
        <p className="field-hint">每個欄位都可以留白。</p>
        <div className="steps-heading-tools">
          {showDiff && diffByPreference && <span className="optional-label">依你的備課偏好預設顯示</span>}
          <button type="button" className="ghost-button small-button" aria-expanded={showDiff} onClick={onToggleDiff}>
            {showDiff ? "隱藏時間與差異化欄位" : "顯示時間與差異化欄位"}
          </button>
        </div>
      </div>
      {showDiff && steps.length > 0 && (
        <p className="total-minutes" aria-live="polite">
          <span>{`預估總時間：${totalMinutes(steps)} 分鐘`}</span>
          <span className="field-hint">
            {timed}/{steps.length} 個步驟已填時間
          </span>
        </p>
      )}
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
            {showDiff && (
              <div className="diff-fields">
                <div className="diff-duration">
                  <label className="field-label" htmlFor={`${step.key}-duration`}>
                    預估時間（分鐘）
                  </label>
                  <input
                    id={`${step.key}-duration`}
                    type="number"
                    min={0}
                    max={MAX_STEP_MINUTES}
                    step={1}
                    inputMode="numeric"
                    value={step.duration_min ?? ""}
                    onChange={(e) => update(step.key, { duration_min: parseMinutes(e.target.value) })}
                  />
                </div>
                <div className="two-fields">
                  <div>
                    <label className="field-label" htmlFor={`${step.key}-support`}>
                      程度較弱的支援
                    </label>
                    <textarea
                      id={`${step.key}-support`}
                      rows={2}
                      value={step.support_note}
                      placeholder="例如：提供句型框架、關鍵字提示"
                      onChange={(e) => update(step.key, { support_note: e.target.value })}
                    />
                  </div>
                  <div>
                    <label className="field-label" htmlFor={`${step.key}-extension`}>
                      程度較好的延伸
                    </label>
                    <textarea
                      id={`${step.key}-extension`}
                      rows={2}
                      value={step.extension_note}
                      placeholder="例如：改寫成兩種倒裝並說明語氣差異"
                      onChange={(e) => update(step.key, { extension_note: e.target.value })}
                    />
                  </div>
                </div>
              </div>
            )}
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
