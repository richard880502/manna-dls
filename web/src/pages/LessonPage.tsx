import { Fragment, useCallback, useEffect, useState, type ReactNode } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { LessonDetail, LessonStep, Unit } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { CasesPanel } from "../components/plan/CasesPanel";
import { CoreLinksPanel } from "../components/plan/CoreLinksPanel";
import { LessonContextCard } from "../components/plan/LessonContextCard";
import { RunsPanel } from "../components/plan/RunsPanel";
import { SiopPanel } from "../components/plan/SiopPanel";
import { StepsEditor, hasDiffData, newStep, type EditableStep } from "../components/plan/StepsEditor";
import { SuggestionsPanel } from "../components/plan/SuggestionsPanel";
import { useApp } from "../context/AppContext";
import { formatDate, formatDateTime } from "../lib/labels";
import { effectiveRoute, hasHardest } from "../lib/preferences";
import { logScaffold, usePreferenceScaffold } from "../lib/scaffold";

function isBlankStep(s: EditableStep): boolean {
  return !s.content.trim() && !s.activity_type.trim() && s.icap_level === null && !hasDiffData(s);
}

function stepFromServer(s: LessonStep): EditableStep {
  return newStep({
    activity_type: s.activity_type,
    icap_level: s.icap_level,
    content: s.content,
    duration_min: typeof s.duration_min === "number" ? s.duration_min : null,
    support_note: s.support_note ?? "",
    extension_note: s.extension_note ?? "",
  });
}

export function LessonPage() {
  const lessonId = Number(useParams().lessonId);
  const { preferences, routeChoiceEnabled } = useApp();
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

  // Preference-driven defaults (偏好只改預設展開，所有輔助都能手動打開).
  const diffByPreference = hasHardest(preferences, "differentiation");
  const ragOnTop = hasHardest(preferences, "material");
  /** null = follow the default; true/false = the teacher toggled it. */
  const [diffToggle, setDiffToggle] = useState<boolean | null>(null);
  const showDiff = diffToggle ?? (diffByPreference || steps.some(hasDiffData));

  usePreferenceScaffold(diffByPreference && lesson !== null, "differentiation");
  usePreferenceScaffold(ragOnTop && lesson !== null, "material");

  useEffect(() => {
    let alive = true;
    api
      .getLesson(lessonId)
      .then((detail) => {
        if (!alive) return;
        setLesson(detail);
        setObjective(detail.latest_version?.objective ?? "");
        setSteps(
          [...(detail.latest_version?.steps ?? [])].sort((a, b) => a.position - b.position).map(stepFromServer),
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

  function toggleDiff() {
    const next = !showDiff;
    setDiffToggle(next);
    logScaffold("SC-PREF", next ? "expand" : "skip", { trigger: "manual:differentiation" });
  }

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
        duration_min: s.duration_min,
        support_note: s.support_note,
        extension_note: s.extension_note,
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

  const route = effectiveRoute(routeChoiceEnabled, unit?.prep_route);
  const activityFirst = route === "activity";
  const effectiveProfile =
    lesson.effective_class_profile ?? (lesson.class_description || unit?.class_profile || "");

  const suggestions = <SuggestionsPanel lessonId={lesson.id} onAppendStep={appendStep} pinned={ragOnTop} />;

  const coreLinks = (
    <CoreLinksPanel
      lessonId={lesson.id}
      unitId={lesson.unit_id}
      initialLinked={lesson.core_focus_ids}
      backfillHint={activityFirst}
    />
  );

  const planEditor = (
    <section className="panel-card form-card" aria-labelledby="plan-editor-heading">
      <div className="card-heading">
        <h3 id="plan-editor-heading">教案</h3>
        {dirty ? (
          <span className="status-pill">尚未儲存</span>
        ) : (
          versionNo !== null && <span className="saved-label">● 第 {versionNo} 版</span>
        )}
      </div>
      {activityFirst && (
        <p className="field-hint route-hint">活動優先：先寫活動點子與教案步驟，寫完再到下方連結 CoRe 重點。</p>
      )}
      <StepsEditor
        objective={objective}
        steps={steps}
        onObjective={onObjective}
        onSteps={onSteps}
        showDiff={showDiff}
        diffByPreference={diffByPreference && diffToggle === null}
        onToggleDiff={toggleDiff}
      />
      <div className="plan-save-row">
        <button type="button" className="primary-button" onClick={() => void save()} disabled={saving}>
          {saving ? "儲存中…" : "儲存教案（產生新版本）"}
        </button>
        {savedMessage && <Notice tone="success">{savedMessage}</Notice>}
        <ErrorText message={saveError} />
      </div>
    </section>
  );

  // Activity-first: write the plan, then link CoRe focuses and back-fill CoRe. Keys keep panels mounted on reorder.
  const mainOrder: Array<[string, ReactNode]> = activityFirst
    ? [
        ["plan", planEditor],
        ["links", coreLinks],
      ]
    : [
        ["links", coreLinks],
        ["plan", planEditor],
      ];

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
          {effectiveProfile ? ` · 班級特性：${effectiveProfile}` : ""}
        </p>
      </div>

      {lesson.previous_next_change && (
        <Notice tone="warn">
          <strong>前一課的下次要改：</strong>
          {lesson.previous_next_change}
        </Notice>
      )}

      {ragOnTop && <div className="lesson-top">{suggestions}</div>}

      <div className="lesson-layout">
        <div className="lesson-main">
          {mainOrder.map(([key, node]) => (
            <Fragment key={key}>{node}</Fragment>
          ))}
          {/* After planning comes teaching: 開始本節課 / 課後補寫 for this lesson. */}
          <RunsPanel lessonId={lesson.id} />
        </div>

        <div className="lesson-side">
          {!ragOnTop && suggestions}
          <LessonContextCard
            lesson={lesson}
            unitProfile={unit?.class_profile ?? ""}
            onSaved={(detail) =>
              setLesson((prev) =>
                prev
                  ? {
                      ...prev,
                      class_description: detail.class_description ?? prev.class_description,
                      my_example: detail.my_example ?? prev.my_example,
                      effective_class_profile: detail.effective_class_profile ?? prev.effective_class_profile,
                    }
                  : prev,
              )
            }
          />
          <SiopPanel lessonId={lesson.id} initialSelected={lesson.siop_feature_ids} />
          <CasesPanel />
        </div>
      </div>
    </section>
  );
}
