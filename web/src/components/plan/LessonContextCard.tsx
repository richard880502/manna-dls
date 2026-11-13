import { useEffect, useImperativeHandle, useState, type Ref } from "react";
import { api, errorMessage } from "../../api/client";
import type { LessonDetail } from "../../api/types";
import { MY_EXAMPLE_MAX_CHARS } from "../../lib/preferences";
import { ErrorText, Notice } from "../common";

/** Lets the lesson page save pending edits before it starts the lesson or leaves. */
export interface LessonContextHandle {
  /** Saves pending edits; resolves to an error message, or null when there was nothing to save or it saved. */
  save: () => Promise<string | null>;
}

interface LessonContextCardProps {
  lesson: LessonDetail;
  unitProfile: string;
  onSaved: (detail: LessonDetail) => void;
  /** Reports unsaved edits. Pass a stable function. */
  onDirtyChange?: (dirty: boolean) => void;
  ref?: Ref<LessonContextHandle>;
}

/**
 * 本課背景：課例層級的班級描述（覆寫單元班級特性）與「我的範例」, inside the lesson page's
 * 「更多備課工具」. Both are sent to Manna with every suggestion request; 我的範例 stays in this
 * lesson only.
 */
export function LessonContextCard({ lesson, unitProfile, onSaved, onDirtyChange, ref }: LessonContextCardProps) {
  const [classDescription, setClassDescription] = useState(lesson.class_description ?? "");
  const [myExample, setMyExample] = useState(lesson.my_example ?? "");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const effective = lesson.effective_class_profile ?? (lesson.class_description || unitProfile);
  const overridden = !!lesson.class_description?.trim();

  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);

  async function save(): Promise<string | null> {
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const detail = await api.updateLesson(lesson.id, {
        class_description: classDescription.trim(),
        my_example: myExample,
      });
      onSaved(detail);
      setClassDescription(detail.class_description ?? classDescription.trim());
      setMyExample(detail.my_example ?? myExample);
      setDirty(false);
      setSaved(true);
      return null;
    } catch (err) {
      const message = errorMessage(err);
      setError(message);
      return message;
    } finally {
      setSaving(false);
    }
  }

  useImperativeHandle(ref, () => ({ save: () => (dirty ? save() : Promise.resolve(null)) }));

  return (
    <div className="lesson-context-card">
      <div className="effective-profile" aria-label="目前生效的班級特性">
        <span>目前生效的班級特性{overridden ? "（本課覆寫）" : "（沿用單元設定）"}</span>
        <p>{effective || "尚未設定。可以在單元頁或下面填寫。"}</p>
      </div>

      <label className="field-label" htmlFor="lesson-class-override">
        本課班級描述（覆寫單元設定）
      </label>
      <textarea
        id="lesson-class-override"
        rows={2}
        value={classDescription}
        disabled={saving}
        placeholder="留白則沿用單元的班級特性"
        onChange={(e) => {
          setClassDescription(e.target.value);
          setDirty(true);
          setSaved(false);
        }}
      />

      <label className="field-label" htmlFor="lesson-my-example">
        我的範例
      </label>
      <textarea
        id="lesson-my-example"
        rows={4}
        value={myExample}
        maxLength={MY_EXAMPLE_MAX_CHARS}
        disabled={saving}
        placeholder="貼上自己的舊教案或學習單文字，Manna 會參考它的風格提供建議。"
        onChange={(e) => {
          setMyExample(e.target.value);
          setDirty(true);
          setSaved(false);
        }}
      />
      <p className="field-hint">
        我的範例只存在這個課例，不會進入共用知識庫；請刪掉學生姓名、座號、成績等學生資料後再貼上。（
        {myExample.length.toLocaleString()} / {MY_EXAMPLE_MAX_CHARS.toLocaleString()} 字）
      </p>

      <div className="button-row">
        <button type="button" className="secondary-button" onClick={() => void save()} disabled={saving || !dirty}>
          {saving ? "儲存中…" : "儲存本課背景"}
        </button>
        {dirty && <span className="status-pill">尚未儲存</span>}
      </div>
      {saved && <Notice tone="success">已儲存本課背景。</Notice>}
      <ErrorText message={error} />
    </div>
  );
}
