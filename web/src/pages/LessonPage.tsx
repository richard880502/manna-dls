import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { LessonDetail, LessonRun, LessonStep, Unit } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { CasesPanel } from "../components/plan/CasesPanel";
import { CoreLinksPanel } from "../components/plan/CoreLinksPanel";
import { LessonContextCard, type LessonContextHandle } from "../components/plan/LessonContextCard";
import { PastRuns } from "../components/plan/PastRuns";
import { PlanSection } from "../components/plan/PlanSection";
import { SiopPanel } from "../components/plan/SiopPanel";
import { StepsEditor, hasDiffData, newStep, type EditableStep } from "../components/plan/StepsEditor";
import { SuggestionsPanel } from "../components/plan/SuggestionsPanel";
import { useApp } from "../context/AppContext";
import { formatDate, formatDateTime } from "../lib/labels";
import { effectiveRoute, hasHardest } from "../lib/preferences";
import { startOrResumeRun } from "../lib/runs";
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

type SectionKey = "links" | "steps" | "context" | "siop" | "cases" | "runs";
type OpenSections = Record<SectionKey, boolean>;

const ALL_CLOSED: OpenSections = { links: false, steps: false, context: false, siop: false, cases: false, runs: false };

/** Opening or closing these sections is logged against their scaffold (SC-SIOP, SC-CASES). */
const SECTION_SCAFFOLD: Partial<Record<SectionKey, "SC-SIOP" | "SC-CASES">> = { siop: "SC-SIOP", cases: "SC-CASES" };

/**
 * 課例備課 (docs/UX_REVIEW.md R1, R2). The first view asks one thing, 「今天要教什麼？」, with
 * 「開始本節課」 right below it. CoRe links, teaching steps and the other aids are collapsed
 * sections with counts; the steps open by default only when the lesson already has steps.
 * Starting saves unsaved plan changes first, and leaving with unsaved changes asks first.
 */
export function LessonPage() {
  const lessonId = Number(useParams().lessonId);
  const navigate = useNavigate();
  const { preferences, routeChoiceEnabled } = useApp();
  const [lesson, setLesson] = useState<LessonDetail | null>(null);
  const [unit, setUnit] = useState<Unit | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [objective, setObjective] = useState("");
  const [steps, setSteps] = useState<EditableStep[]>([]);
  const [dirty, setDirty] = useState(false);
  /** Bumped on every plan edit, so a save only clears `dirty` when nothing changed meanwhile. */
  const editSeq = useRef(0);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedMessage, setSavedMessage] = useState<string | null>(null);
  const [versionNo, setVersionNo] = useState<number | null>(null);
  const [versionsCount, setVersionsCount] = useState(0);

  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);

  const [open, setOpen] = useState<OpenSections>(ALL_CLOSED);
  const [linkedCount, setLinkedCount] = useState(0);
  const [siopCount, setSiopCount] = useState<number | null>(null);
  const [siopSelected, setSiopSelected] = useState(0);
  const [casesCount, setCasesCount] = useState<number | null>(null);
  const [contextDirty, setContextDirty] = useState(false);
  const contextRef = useRef<LessonContextHandle>(null);

  const guard = useLeaveGuard(dirty || contextDirty);

  // Preference-driven defaults (偏好只改預設展開，所有輔助都能手動打開).
  const diffByPreference = hasHardest(preferences, "differentiation");
  const ragOnTop = hasHardest(preferences, "material");
  /** null = follow the default; true/false = the teacher toggled it. */
  const [diffToggle, setDiffToggle] = useState<boolean | null>(null);
  const showDiff = diffToggle ?? (diffByPreference || steps.some(hasDiffData));

  // The differentiation fields are only seen once the steps are open.
  usePreferenceScaffold(diffByPreference && lesson !== null && open.steps, "differentiation");
  usePreferenceScaffold(ragOnTop && lesson !== null, "material");

  useEffect(() => {
    let alive = true;
    api
      .getLesson(lessonId)
      .then((detail) => {
        if (!alive) return;
        const loaded = [...(detail.latest_version?.steps ?? [])].sort((a, b) => a.position - b.position);
        setLesson(detail);
        setObjective(detail.latest_version?.objective ?? "");
        setSteps(loaded.map(stepFromServer));
        setOpen({ ...ALL_CLOSED, steps: loaded.length > 0 });
        setLinkedCount(detail.core_focus_ids.length);
        setSiopSelected(detail.siop_feature_ids.length);
        setVersionNo(detail.latest_version?.version_no ?? null);
        setVersionsCount(detail.versions_count);
        api
          .getUnit(detail.unit_id)
          .then((u) => alive && setUnit(u))
          .catch(() => undefined);
      })
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    api
      .listLessonRuns(lessonId)
      .then((r) => alive && setRuns(r))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [lessonId]);

  const markEdited = useCallback(() => {
    editSeq.current += 1;
    setDirty(true);
    setSavedMessage(null);
  }, []);

  const onObjective = useCallback(
    (value: string) => {
      setObjective(value);
      markEdited();
    },
    [markEdited],
  );

  const onSteps = useCallback(
    (next: EditableStep[]) => {
      setSteps(next);
      markEdited();
    },
    [markEdited],
  );

  const appendStep = useCallback(
    (step: Omit<EditableStep, "key">) => {
      setSteps((prev) => [...prev, newStep(step)]);
      // An adopted suggestion lands in the steps: show them.
      setOpen((prev) => ({ ...prev, steps: true }));
      markEdited();
    },
    [markEdited],
  );

  const onLinkedChange = useCallback((ids: number[]) => setLinkedCount(ids.length), []);
  const onSiopSelected = useCallback((ids: number[]) => setSiopSelected(ids.length), []);

  function toggleSection(key: SectionKey) {
    const next = !open[key];
    setOpen((prev) => ({ ...prev, [key]: next }));
    const code = SECTION_SCAFFOLD[key];
    if (code) logScaffold(code, next ? "expand" : "skip", { trigger: "section" });
  }

  function toggleDiff() {
    const next = !showDiff;
    setDiffToggle(next);
    logScaffold("SC-PREF", next ? "expand" : "skip", { trigger: "manual:differentiation" });
  }

  /** Saves the plan (objective + steps) as a new version. Resolves to an error message, or null. */
  async function savePlan(): Promise<string | null> {
    const seq = editSeq.current;
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
      if (editSeq.current === seq) setDirty(false);
      setSavedMessage(`已儲存為第 ${version.version_no} 版（${formatDateTime(version.created_at)}）。`);
      return null;
    } catch (err) {
      return errorMessage(err);
    } finally {
      setSaving(false);
    }
  }

  async function onSave() {
    setStartError(null);
    setSaveError(await savePlan());
  }

  /** Saves whatever is unsaved on the page: the plan, then 本課背景. */
  async function saveAll(): Promise<string | null> {
    if (dirty) {
      const failed = await savePlan();
      if (failed) return failed;
    }
    if (contextDirty) {
      const failed = (await contextRef.current?.save()) ?? null;
      if (failed) return failed;
    }
    return null;
  }

  /** 開始本節課 (or 回到課堂紀錄): unsaved changes are saved first; if that fails, nothing starts. */
  async function start() {
    const seq = editSeq.current;
    setStarting(true);
    setStartError(null);
    setSaveError(null);
    const failed = await saveAll();
    if (failed) {
      setStartError(`教案沒有儲存成功，所以還沒開始上課：${failed}`);
      setStarting(false);
      return;
    }
    try {
      const current = runs?.find((r) => r.ended_at === null);
      const runId = current ? current.id : await startOrResumeRun(lessonId);
      // Everything is saved, unless the teacher typed while it was saving: then the guard asks.
      if (editSeq.current === seq) guard.release();
      navigate(`/runs/${runId}`);
    } catch (err) {
      setStartError(errorMessage(err));
      setStarting(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!lesson) return <Loading />;

  const route = effectiveRoute(routeChoiceEnabled, unit?.prep_route);
  const activityFirst = route === "activity";
  const effectiveProfile =
    lesson.effective_class_profile ?? (lesson.class_description || unit?.class_profile || "");
  const openRun = runs?.find((r) => r.ended_at === null);
  const pastRuns = (runs ?? []).filter((r) => r.ended_at !== null);

  const suggestions = <SuggestionsPanel lessonId={lesson.id} onAppendStep={appendStep} pinned={ragOnTop} />;

  const today = (
    <section className="panel-card lesson-today" aria-labelledby="today-heading">
      <div className="card-heading">
        <h3 id="today-heading">
          <label htmlFor="lesson-objective">今天要教什麼？</label>
        </h3>
        {dirty ? (
          <span className="status-pill">尚未儲存</span>
        ) : (
          versionNo !== null && <span className="saved-label">● 第 {versionNo} 版</span>
        )}
      </div>
      <p className="field-hint" id="lesson-objective-hint">
        一句話就夠；留白也可以直接開始上課。
      </p>
      <textarea
        id="lesson-objective"
        rows={2}
        value={objective}
        disabled={starting}
        aria-describedby="lesson-objective-hint"
        onChange={(e) => onObjective(e.target.value)}
        placeholder="例如：學生能比較一般語序與句首倒裝，並用一句話說明倒裝的語意功能。"
      />
      <div className="today-actions">
        <button type="button" className="primary-button" onClick={() => void start()} disabled={starting || saving}>
          {starting ? "開始中…" : openRun ? "回到課堂紀錄" : "開始本節課"}
        </button>
        <button type="button" className="secondary-button" onClick={() => void onSave()} disabled={saving || starting}>
          {saving && !starting ? "儲存中…" : "儲存教案（產生新版本）"}
        </button>
      </div>
      <p className="field-hint">按「開始本節課」時，尚未儲存的內容會先存成新版本。課中不呼叫 AI。</p>
      {savedMessage && <Notice tone="success">{savedMessage}</Notice>}
      <ErrorText message={saveError} />
      <ErrorText message={startError} />
    </section>
  );

  const linksSection = (
    <PlanSection
      id="links"
      title="連結的 CoRe 重點"
      count={linkedCount}
      open={open.links}
      onToggle={() => toggleSection("links")}
    >
      <CoreLinksPanel
        lessonId={lesson.id}
        unitId={lesson.unit_id}
        initialLinked={lesson.core_focus_ids}
        backfillHint={activityFirst}
        onLinkedChange={onLinkedChange}
      />
    </PlanSection>
  );

  const stepsSection = (
    <PlanSection
      id="steps"
      title="教學步驟"
      count={steps.length}
      tag={dirty && !open.steps ? <span className="status-pill">尚未儲存</span> : null}
      open={open.steps}
      onToggle={() => toggleSection("steps")}
    >
      {activityFirst && (
        <p className="field-hint route-hint">活動優先：先寫活動點子與教學步驟，寫完再連結 CoRe 重點。</p>
      )}
      <StepsEditor
        steps={steps}
        onSteps={onSteps}
        showDiff={showDiff}
        diffByPreference={diffByPreference && diffToggle === null}
        onToggleDiff={toggleDiff}
      />
      <div className="plan-save-row">
        <button type="button" className="secondary-button" onClick={() => void onSave()} disabled={saving || starting}>
          {saving && !starting ? "儲存中…" : "儲存教案（產生新版本）"}
        </button>
        {dirty && <span className="status-pill">尚未儲存</span>}
      </div>
    </PlanSection>
  );

  // Activity-first: write the steps, then link CoRe focuses. Keys keep sections mounted on reorder.
  const prepOrder: Array<[string, ReactNode]> = activityFirst
    ? [
        ["steps", stepsSection],
        ["links", linksSection],
      ]
    : [
        ["links", linksSection],
        ["steps", stepsSection],
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

      <div className="lesson-layout">
        <div className="lesson-main">
          {today}
          {ragOnTop && suggestions}

          <section className="panel-card lesson-prep" aria-labelledby="prep-heading">
            <div className="card-heading">
              <h3 id="prep-heading">想多準備一點？</h3>
              <span className="optional-label">都可略過</span>
            </div>
            {prepOrder.map(([key, node]) => (
              <Fragment key={key}>{node}</Fragment>
            ))}
          </section>

          {pastRuns.length > 0 && (
            <section className="panel-card lesson-runs">
              <PlanSection
                id="runs"
                level={3}
                title="上課紀錄與課後補寫"
                count={pastRuns.length}
                open={open.runs}
                onToggle={() => toggleSection("runs")}
              >
                <PastRuns runs={pastRuns} />
              </PlanSection>
            </section>
          )}
        </div>

        <div className="lesson-side">
          {!ragOnTop && suggestions}
          <section className="panel-card lesson-tools" aria-labelledby="tools-heading">
            <div className="card-heading">
              <h3 id="tools-heading">更多備課工具</h3>
              <span className="optional-label">選用</span>
            </div>
            <PlanSection
              id="context"
              title="本課背景：班級特性與我的範例"
              tag={contextDirty ? <span className="status-pill">尚未儲存</span> : null}
              open={open.context}
              onToggle={() => toggleSection("context")}
            >
              <LessonContextCard
                ref={contextRef}
                lesson={lesson}
                unitProfile={unit?.class_profile ?? ""}
                onDirtyChange={setContextDirty}
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
            </PlanSection>
            {siopCount !== 0 && (
              <PlanSection
                id="siop"
                title="SIOP 提示"
                count={siopCount}
                tag={siopSelected > 0 ? <span className="soft-tag">已選 {siopSelected}</span> : null}
                open={open.siop}
                onToggle={() => toggleSection("siop")}
              >
                <SiopPanel
                  lessonId={lesson.id}
                  initialSelected={lesson.siop_feature_ids}
                  onLoaded={setSiopCount}
                  onSelectedChange={onSiopSelected}
                />
              </PlanSection>
            )}
            {casesCount !== 0 && (
              <PlanSection
                id="cases"
                title="其他老師怎麼教"
                count={casesCount}
                open={open.cases}
                onToggle={() => toggleSection("cases")}
              >
                <CasesPanel onLoaded={setCasesCount} />
              </PlanSection>
            )}
          </section>
        </div>
      </div>

      <LeaveConfirm
        guard={guard}
        message="離開這頁，尚未儲存的教案或本課背景就會遺失。"
        onSave={saveAll}
      />
    </section>
  );
}
