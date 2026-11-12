import { useCallback, useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { LessonDetail, LessonStep, Unit } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { CasesPanel } from "../components/plan/CasesPanel";
import { CoreLinksPanel } from "../components/plan/CoreLinksPanel";
import { SiopPanel } from "../components/plan/SiopPanel";
import { StepsEditor, newStep, type EditableStep } from "../components/plan/StepsEditor";
import { SuggestionsPanel } from "../components/plan/SuggestionsPanel";
import { formatDate, formatDateTime } from "../lib/labels";

function isBlankStep(s: EditableStep): boolean {
  return !s.content.trim() && !s.activity_type.trim() && s.icap_level === null;
}

export function LessonPage() {
  const lessonId = Number(useParams().lessonId);
  const [lesson, setLesson] = useState<LessonDetail | null>(null);
  const [unit, setUnit] = useState<Unit | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [objective, setObjective] = useState("");
  const [steps, setSteps] = useState<EditableStep[]>([]);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedMessage, setSavedMessage] = useState<string | null>(null);
  const [versionNo, setVersionNo] = useState<number | null>(null);
  const [versionsCount, setVersionsCount] = useState(0);

  useEffect(() => {
    let alive = true;
    api
      .getLesson(lessonId)
      .then((detail) => {
        if (!alive) return;
        setLesson(detail);
        setObjective(detail.latest_version?.objective ?? "");
        setSteps(
          [...(detail.latest_version?.steps ?? [])]
            .sort((a, b) => a.position - b.position)
            .map((s) => newStep({ activity_type: s.activity_type, icap_level: s.icap_level, content: s.content })),
        );
        setVersionNo(detail.latest_version?.version_no ?? null);
        setVersionsCount(detail.versions_count);
        api
          .getUnit(detail.unit_id)
          .then((u) => alive && setUnit(u))
          .catch(() => undefined);
      })
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [lessonId]);

  const onObjective = useCallback((value: string) => {
    setObjective(value);
    setDirty(true);
    setSavedMessage(null);
  }, []);

  const onSteps = useCallback((next: EditableStep[]) => {
    setSteps(next);
    setDirty(true);
    setSavedMessage(null);
  }, []);

  const appendStep = useCallback((step: Omit<EditableStep, "key">) => {
    setSteps((prev) => [...prev, newStep(step)]);
    setDirty(true);
    setSavedMessage(null);
  }, []);

  async function save() {
    setSaving(true);
    setSaveError(null);
    const payloadSteps: LessonStep[] = steps
      .filter((s) => !isBlankStep(s))
      .map((s, i) => ({
        position: i + 1,
        activity_type: s.activity_type.trim(),
        icap_level: s.icap_level,
        content: s.content,
      }));
    try {
      const version = await api.saveLessonVersion(lessonId, { objective, steps: payloadSteps });
      setVersionNo(version.version_no);
      setVersionsCount((n) => Math.max(n + 1, version.version_no));
      setDirty(false);
      setSavedMessage(`已儲存為第 ${version.version_no} 版（${formatDateTime(version.created_at)}）。`);
    } catch (err) {
      setSaveError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!lesson) return <Loading />;

  return (
    <section aria-labelledby="lesson-heading">
      <p className="breadcrumb">
        <Link to="/plan">Study &amp; Plan</Link> / <Link to={`/units/${lesson.unit_id}`}>{unit?.title ?? "單元"}</Link> /{" "}
        {lesson.title}
      </p>
      <div className="page-intro">
        <p className="eyebrow warm">
          課例備課 · {versionNo !== null ? `目前第 ${versionNo} 版（共 ${versionsCount} 版）` : "尚未儲存教案"}
        </p>
        <h2 id="lesson-heading">{lesson.title}</h2>
        <p>
          {lesson.lesson_date ? `上課日期 ${formatDate(lesson.lesson_date)}` : "未設定上課日期"}
          {lesson.class_description ? ` · ${lesson.class_description}` : ""}
        </p>
      </div>

      {lesson.previous_next_change && (
        <Notice tone="warn">
          <strong>前一課的下次要改：</strong>
          {lesson.previous_next_change}
        </Notice>
      )}

      <div className="lesson-layout">
        <div className="lesson-main">
          <CoreLinksPanel lessonId={lesson.id} unitId={lesson.unit_id} initialLinked={lesson.core_focus_ids} />

          <section className="panel-card form-card" aria-labelledby="plan-editor-heading">
            <div className="card-heading">
              <h3 id="plan-editor-heading">教案</h3>
              {dirty ? (
                <span className="status-pill">尚未儲存</span>
              ) : (
                versionNo !== null && <span className="saved-label">● 第 {versionNo} 版</span>
              )}
            </div>
            <StepsEditor objective={objective} steps={steps} onObjective={onObjective} onSteps={onSteps} />
            <div className="plan-save-row">
              <button type="button" className="primary-button" onClick={() => void save()} disabled={saving}>
                {saving ? "儲存中…" : "儲存教案（產生新版本）"}
              </button>
              {savedMessage && <Notice tone="success">{savedMessage}</Notice>}
              <ErrorText message={saveError} />
            </div>
          </section>
        </div>

        <div className="lesson-side">
          <SuggestionsPanel lessonId={lesson.id} onAppendStep={appendStep} />
          <SiopPanel lessonId={lesson.id} initialSelected={lesson.siop_feature_ids} />
          <CasesPanel />
        </div>
      </div>
    </section>
  );
}
