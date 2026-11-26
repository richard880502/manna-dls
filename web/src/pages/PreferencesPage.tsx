import { TeacherMemories } from "../components/prefs/TeacherMemories";
import { useState } from "react";
import { api, errorMessage } from "../api/client";
import type { PreferenceAnswers } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { PreferenceField } from "../components/prefs/PreferenceField";
import { usePreferenceQuestions } from "../components/prefs/usePreferenceQuestions";
import { useApp } from "../context/AppContext";
import { formatDateTime } from "../lib/labels";
import { PREFERENCE_SOURCE_LABELS, answersFrom } from "../lib/preferences";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { MetaList } from "../components/MetaList";

/**
 * 我的備課偏好: edit P1–P5; every save creates a new version (source "settings"). It is optional
 * and reached from the top bar; nothing asks for it first. P6 (班級特性) is not asked here any
 * more: class traits are written once, on the class page. An earlier P6 answer is kept as it was
 * and saved again unchanged with every new version.
 */
export function PreferencesPage() {
  useDocumentTitle("我的備課偏好");
  const { preferences, preferencesStatus, setPreferences } = useApp();
  const { questions: allQuestions, loaded } = usePreferenceQuestions();
  const questions = allQuestions.filter((q) => q.id !== "P6");
  const [answers, setAnswers] = useState<PreferenceAnswers>(() => answersFrom(preferences));
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  function change(next: PreferenceAnswers) {
    setAnswers(next);
    setDirty(true);
    setMessage(null);
  }

  async function save() {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const saved = await api.savePreferences({ answers, skipped: false, source: "settings" });
      setPreferences(saved);
      setAnswers(answersFrom(saved));
      setDirty(false);
      setMessage(`已儲存為第 ${saved.version_no} 版。之後建立的單元與 AI 建議會依新的偏好調整。`);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  const versionFacts = preferences
    ? [
        `目前第 ${preferences.version_no} 版`,
        <time key="saved" dateTime={preferences.created_at}>
          {formatDateTime(preferences.created_at)}
        </time>,
        PREFERENCE_SOURCE_LABELS[preferences.source] ?? preferences.source,
        preferences.skipped ? "當時略過" : null,
      ]
    : ["尚未設定"];

  return (
    <section aria-labelledby="prefs-heading">
      <div className="page-intro">
        <h1 id="prefs-heading">我的備課偏好</h1>
        <MetaList className="quiet-note" items={versionFacts} />
        <p>
          想填再填。這些回答幫 Manna 調整建議的方式與長度；偏好只改「預設」，不會隱藏任何功能，每次儲存都會保留舊版本。班級特性寫在各班的班級頁。
        </p>
      </div>

      {preferencesStatus === "error" && (
        <Notice tone="warn">目前無法讀取你先前的偏好，下面顯示的是空白設定。</Notice>
      )}

      {!loaded ? (
        <Loading />
      ) : (
        <div className="form-card prefs-form">
          {questions.map((q) => (
            <PreferenceField key={q.id} question={q} answers={answers} onChange={change} disabled={saving} />
          ))}
          <div className="plan-save-row">
            <button type="button" className="primary-button" onClick={() => void save()} disabled={saving}>
              {saving ? "儲存中…" : "儲存偏好（產生新版本）"}
            </button>
            {dirty && <span className="status-pill prefs-dirty">尚未儲存</span>}
            {message && <Notice tone="success">{message}</Notice>}
            <ErrorText message={error} />
          </div>
        </div>
      )}
      <TeacherMemories />
    </section>
  );
}
