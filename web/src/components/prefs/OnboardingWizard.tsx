import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { PreferenceAnswers } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { emptyAnswers } from "../../lib/preferences";
import { ErrorText, Loading } from "../common";
import { PreferenceField } from "./PreferenceField";
import { usePreferenceQuestions } from "./usePreferenceQuestions";

/**
 * 初次使用設定（P1–P6）. Shown after login while GET /api/me/preferences is null.
 * Every step can be skipped (sends skipped: true); nothing here blocks using the platform.
 */
export function OnboardingWizard({ onDismiss }: { onDismiss: () => void }) {
  const { setPreferences } = useApp();
  const { questions, loaded } = usePreferenceQuestions();
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<PreferenceAnswers>(() => emptyAnswers());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!loaded) return <Loading label="正在準備初次使用設定…" />;

  const total = questions.length;
  const index = Math.min(step, total - 1);
  const question = questions[index];
  const isLast = index === total - 1;

  async function submit(skipped: boolean) {
    setSaving(true);
    setError(null);
    try {
      const saved = await api.savePreferences({ answers, skipped, source: "onboarding" });
      setPreferences(saved);
    } catch (err) {
      if (skipped) {
        // Skipping must never trap the teacher: hide the wizard for this session anyway.
        onDismiss();
        return;
      }
      setError(errorMessage(err));
      setSaving(false);
    }
  }

  return (
    <section className="onboarding" aria-labelledby="onboarding-heading">
      <div className="page-intro">
        <p className="eyebrow warm">
          初次使用設定 · 第 {index + 1} / {total} 題
        </p>
        <h1 id="onboarding-heading">先認識你的備課習慣</h1>
        <p>
          約 3 分鐘。你的回答只用來調整預設的備課順序、預設展開的輔助與 AI 建議的詳細程度；所有功能都還是能手動打開。
          可以略過，之後隨時在「備課偏好」修改。
        </p>
      </div>

      <div className="panel-card onboarding-card">
        <div className="wizard-progress" aria-hidden="true">
          {questions.map((q, i) => (
            <span key={q.id} className={`wizard-dot${i === index ? " is-current" : ""}${i < index ? " is-done" : ""}`} />
          ))}
        </div>

        {question && <PreferenceField question={question} answers={answers} onChange={setAnswers} disabled={saving} />}

        <ErrorText message={error} />

        <div className="wizard-actions">
          <button
            type="button"
            className="ghost-button"
            onClick={() => setStep((s) => Math.max(0, s - 1))}
            disabled={saving || index === 0}
          >
            上一步
          </button>
          {isLast ? (
            <button type="button" className="primary-button" onClick={() => void submit(false)} disabled={saving}>
              {saving ? "儲存中…" : "完成設定"}
            </button>
          ) : (
            <button
              type="button"
              className="primary-button"
              onClick={() => setStep((s) => Math.min(total - 1, s + 1))}
              disabled={saving}
            >
              下一步
            </button>
          )}
          <button type="button" className="secondary-button wizard-skip" onClick={() => void submit(true)} disabled={saving}>
            略過
          </button>
        </div>
        <p className="field-hint">每一題都可以不選；按「略過」會直接進入平台，之後可在「我的備課偏好」補填。</p>
      </div>
    </section>
  );
}
