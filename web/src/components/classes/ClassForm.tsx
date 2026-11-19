import { useId, useState, type FormEvent } from "react";
import type { CreateClassRequest, SchoolTrack } from "../../api/types";
import {
  CLASS_LABEL_MAX_CHARS,
  CLASS_NAME_MAX_CHARS,
  CLASS_SHORT_MAX_CHARS,
} from "../../lib/classes";
import { SCHOOL_TRACK_LABELS } from "../../lib/labels";
import { CLASS_PROFILE_MAX_CHARS } from "../../lib/preferences";
import { ErrorText, PrivacyHint } from "../common";
import { useDialogGuard } from "../Dialog";

export interface ClassFormValues {
  name: string;
  grade: string;
  school_track: SchoolTrack;
  course_label: string;
  class_profile: string;
  school_year: string;
  term: string;
}

interface ClassFormProps {
  initial: ClassFormValues;
  submitLabel: string;
  /** Resolves to an error message, or null when it saved. */
  onSubmit: (body: CreateClassRequest) => Promise<string | null>;
  onCancel?: () => void;
  cancelLabel?: string;
  /** Shown above the class profile when it was prefilled (e.g. from the preference survey). */
  profileHint?: string;
  /** The profile is edited elsewhere (class page): leave it out of this form. */
  hideProfile?: boolean;
}

/**
 * The fields of a class. Only the name is required; a class never holds student data, so the
 * profile is one free-text description of the group.
 */
export function ClassForm({
  initial,
  submitLabel,
  onSubmit,
  onCancel,
  cancelLabel = "取消",
  profileHint,
  hideProfile = false,
}: ClassFormProps) {
  const id = useId();
  const [values, setValues] = useState<ClassFormValues>(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // In a dialog: no closing while saving, and typed input is asked about first.
  const dirty = (Object.keys(values) as Array<keyof ClassFormValues>).some((k) => values[k] !== initial[k]);
  const requestClose = useDialogGuard({ busy: saving, dirty });
  const cancel = requestClose ?? onCancel;

  const set = <K extends keyof ClassFormValues>(key: K, value: ClassFormValues[K]) =>
    setValues((prev) => ({ ...prev, [key]: value }));

  async function submit(event: FormEvent) {
    event.preventDefault();
    const name = values.name.trim();
    if (!name) {
      setError("請輸入班級名稱。");
      return;
    }
    setSaving(true);
    setError(null);
    const body: CreateClassRequest = {
      name,
      grade: values.grade.trim(),
      school_track: values.school_track,
      course_label: values.course_label.trim(),
      school_year: values.school_year.trim(),
      term: values.term.trim(),
    };
    if (!hideProfile) body.class_profile = values.class_profile.trim();
    const failed = await onSubmit(body);
    setSaving(false);
    if (failed) setError(failed);
  }

  return (
    <form className="class-form" onSubmit={submit}>
      <div className="two-fields">
        <div>
          <label className="field-label" htmlFor={`${id}-name`}>
            班級名稱
          </label>
          <input
            id={`${id}-name`}
            value={values.name}
            maxLength={CLASS_NAME_MAX_CHARS}
            placeholder="例如：高一 3 班"
            disabled={saving}
            onChange={(e) => set("name", e.target.value)}
          />
        </div>
        <div>
          <label className="field-label" htmlFor={`${id}-grade`}>
            年級（選填）
          </label>
          <input
            id={`${id}-grade`}
            value={values.grade}
            maxLength={CLASS_SHORT_MAX_CHARS}
            placeholder="例如：高一"
            disabled={saving}
            onChange={(e) => set("grade", e.target.value)}
          />
        </div>
      </div>

      {!hideProfile && (
        <>
          <label className="field-label" htmlFor={`${id}-profile`}>
            班級特性（選填）
          </label>
          <textarea
            id={`${id}-profile`}
            rows={3}
            value={values.class_profile}
            maxLength={CLASS_PROFILE_MAX_CHARS}
            placeholder="例如：普通班 34 人，程度落差大；多數學生不太敢開口。"
            disabled={saving}
            aria-describedby={`${id}-profile-hint`}
            onChange={(e) => set("class_profile", e.target.value)}
          />
          <p className="field-hint" id={`${id}-profile-hint`}>
            {profileHint ? `${profileHint} ` : ""}在這個班級備課時，Manna 的每次協助都會帶入這段描述。
          </p>
          <PrivacyHint />
        </>
      )}

      <details className="class-form-more">
        <summary>課程名稱、學年度等（選填）</summary>
        <div className="two-fields">
          <div>
            <label className="field-label" htmlFor={`${id}-course`}>
              課程名稱
            </label>
            <input
              id={`${id}-course`}
              value={values.course_label}
              maxLength={CLASS_LABEL_MAX_CHARS}
              placeholder="例如：英文閱讀選修；一般英文課可留白"
              disabled={saving}
              onChange={(e) => set("course_label", e.target.value)}
            />
          </div>
          <div>
            <label className="field-label" htmlFor={`${id}-track`}>
              學校類型
            </label>
            <select
              id={`${id}-track`}
              value={values.school_track}
              disabled={saving}
              onChange={(e) => set("school_track", e.target.value as SchoolTrack)}
            >
              {(Object.keys(SCHOOL_TRACK_LABELS) as SchoolTrack[]).map((t) => (
                <option key={t} value={t}>
                  {SCHOOL_TRACK_LABELS[t]}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="field-label" htmlFor={`${id}-year`}>
              學年度
            </label>
            <input
              id={`${id}-year`}
              value={values.school_year}
              maxLength={CLASS_SHORT_MAX_CHARS}
              placeholder="例如：115"
              disabled={saving}
              onChange={(e) => set("school_year", e.target.value)}
            />
          </div>
          <div>
            <label className="field-label" htmlFor={`${id}-term`}>
              學期
            </label>
            <input
              id={`${id}-term`}
              value={values.term}
              maxLength={CLASS_SHORT_MAX_CHARS}
              placeholder="例如：上學期"
              disabled={saving}
              onChange={(e) => set("term", e.target.value)}
            />
          </div>
        </div>
      </details>

      <ErrorText message={error} />
      <div className="button-row">
        <button type="submit" className="primary-button" disabled={saving}>
          {saving ? "儲存中…" : submitLabel}
        </button>
        {cancel && (
          <button type="button" className="ghost-button" onClick={cancel} disabled={saving}>
            {cancelLabel}
          </button>
        )}
      </div>
    </form>
  );
}

export function emptyClassValues(track: SchoolTrack, classProfile = ""): ClassFormValues {
  return {
    name: "",
    grade: "",
    school_track: track,
    course_label: "",
    class_profile: classProfile,
    school_year: "",
    term: "",
  };
}
