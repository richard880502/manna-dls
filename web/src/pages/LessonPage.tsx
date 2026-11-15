import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { LessonDetail, LessonRun, LessonStep, Unit } from "../api/types";
import { StartClassDialog } from "../components/classes/StartClassDialog";
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
import { Rail } from "../components/sop/Rail";
import { StageAnalysis } from "../components/sop/StageAnalysis";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { StageMaterial } from "../components/sop/StageMaterial";
import { StageObjectives } from "../components/sop/StageObjectives";
import { StageExit, StageIntegrate, StageTasks, StageWarmup } from "../components/sop/StagesManual";
import { useSop } from "../components/sop/useSop";
import { useApp } from "../context/AppContext";
import { useActiveClass } from "../lib/classes";
import { formatClock, formatDate } from "../lib/labels";
import { effectiveRoute, hasHardest } from "../lib/preferences";
import { startOrResumeRun } from "../lib/runs";
import { logScaffold, usePreferenceScaffold } from "../lib/scaffold";
import { STAGES, hasSopContent, stageName, stageSummary, type StageKey } from "../lib/sop";
import { useAutosave } from "../lib/useAutosave";

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

const stageStorageKey = (lessonId: number) => `manna.lesson.${lessonId}.stage`;

function readStoredStage(lessonId: number): StageKey | null {
  try {
    const raw = window.sessionStorage.getItem(stageStorageKey(lessonId));
    if (raw === "overview") return "overview";
    const step = Number(raw);
    return STAGES.some((s) => s.step === step) ? (step as StageKey) : null;
  } catch {
    return null;
  }
}

/**
 * 課例頁: the seven-step preparation workspace (docs/DESIGN_OVERHAUL.md step 2, direction A).
 * A progress rail on the left, one step at a time in the middle, Manna's suggestions inside the
 * step they belong to, and the material sliding in beside it on request.
 *
 * 「本節概要」 at the top of the rail is the first view from docs/UX_REVIEW.md R1: it asks one
 * thing, 「今天要教什麼？」, with 「開始上課」 right below it, and keeps the teaching steps and the
 * other aids as collapsed rows. A lesson without any SOP content opens there, so a teacher can
 * still write one line and start. 「開始上課」 is on every step and never looks at the SOP.
 *
 * Nothing has a save button: the plan (today's line and the steps) and the SOP are each saved a
 * moment after the last edit, and at once when the teacher leaves a field. Starting the lesson,
 * leaving the page and asking Manna save what is pending first. Only when that save fails does
 * leaving ask (儲存後離開／留在這頁／不儲存，直接離開), so nothing is dropped silently (R2).
 */
export function LessonPage() {
  const lessonId = Number(useParams().lessonId);
  // One workspace per lesson: its drafts and timers never carry over to another lesson.
  return <LessonWorkspace key={lessonId} lessonId={lessonId} />;
}

function LessonWorkspace({ lessonId }: { lessonId: number }) {
  const navigate = useNavigate();
  const { preferences, routeChoiceEnabled } = useApp();
  const [lesson, setLesson] = useState<LessonDetail | null>(null);
  const [unit, setUnit] = useState<Unit | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [objective, setObjective] = useState("");
  const [steps, setSteps] = useState<EditableStep[]>([]);
  // What the next save sends: read from refs, so a save started by a timer has the latest text.
  const objectiveRef = useRef(objective);
  const stepsRef = useRef(steps);

  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  /** 「這節課在哪一班上？」: open when several classes use the unit and the teacher came from the library. */
  const [choosingClass, setChoosingClass] = useState(false);
  const { classes: unitClasses, active: activeClass, choose: chooseClass } = useActiveClass(unit);
  const activeClassId = activeClass?.id ?? null;

  const [open, setOpen] = useState<OpenSections>(ALL_CLOSED);
  const [linkedCount, setLinkedCount] = useState(0);
  const [siopCount, setSiopCount] = useState<number | null>(null);
  const [siopSelected, setSiopSelected] = useState(0);
  const [casesCount, setCasesCount] = useState<number | null>(null);
  const [contextDirty, setContextDirty] = useState(false);
  const contextRef = useRef<LessonContextHandle>(null);

  const sop = useSop(lessonId, activeClassId);
  /** The step the teacher went to in this visit (null: none yet, the page decides where to open). */
  const [chosenStage, setChosenStage] = useState<StageKey | null>(null);
  const [storedStage] = useState(() => readStoredStage(lessonId));
  const [viewerOpen, setViewerOpen] = useState(false);
  const stageHeadingRef = useRef<HTMLHeadingElement>(null);
  const todayHeadingRef = useRef<HTMLHeadingElement>(null);
  const movedStage = useRef(false);

  const savePlan = useCallback(async () => {
    const payloadSteps: LessonStep[] = stepsRef.current
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
    const version = await api.saveLessonVersion(lessonId, { objective: objectiveRef.current, steps: payloadSteps });
    return version.created_at;
  }, [lessonId]);
  const planSave = useAutosave(savePlan);
  const touchPlan = planSave.touch;

  const unsaved = planSave.dirty || contextDirty || sop.dirty;
  const guard = useLeaveGuard(unsaved);
  /** Leaving was held and the save it triggered failed: now the page asks. */
  const [leaveFailed, setLeaveFailed] = useState(false);

  // Preference-driven defaults (偏好只改預設展開，所有輔助都能手動打開).
  // Where the page opens: the step of the last visit, else step 1 when there is SOP content, else
  // the overview (so a new lesson still starts with 今天要教什麼？). Unknown until both are loaded.
  const sopHasContent = sop.status === "ready" && hasSopContent(sop.material, sop.content);
  const stage: StageKey | null =
    chosenStage ??
    (lesson !== null && sop.status !== "loading" ? (storedStage ?? (sopHasContent ? 1 : "overview")) : null);

  const diffByPreference = hasHardest(preferences, "differentiation");
  const ragOnTop = hasHardest(preferences, "material");
  /** null = follow the default; true/false = the teacher toggled it. */
  const [diffToggle, setDiffToggle] = useState<boolean | null>(null);
  const showDiff = diffToggle ?? (diffByPreference || steps.some(hasDiffData));
  const onOverview = stage === "overview";

  // The differentiation fields are only seen once the steps are open.
  usePreferenceScaffold(diffByPreference && lesson !== null && onOverview && open.steps, "differentiation");
  usePreferenceScaffold(ragOnTop && lesson !== null && onOverview, "material");

  useEffect(() => {
    let alive = true;
    api
      .getLesson(lessonId)
      .then((detail) => {
        if (!alive) return;
        const loaded = [...(detail.latest_version?.steps ?? [])].sort((a, b) => a.position - b.position);
        const loadedSteps = loaded.map(stepFromServer);
        setLesson(detail);
        objectiveRef.current = detail.latest_version?.objective ?? "";
        stepsRef.current = loadedSteps;
        setObjective(objectiveRef.current);
        setSteps(loadedSteps);
        setOpen({ ...ALL_CLOSED, steps: loaded.length > 0 });
        setLinkedCount(detail.core_focus_ids.length);
        setSiopSelected(detail.siop_feature_ids.length);
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

  // In a class the effective class profile is the class's (unless the lesson overrides it).
  const lessonLoaded = lesson !== null;
  useEffect(() => {
    if (!lessonLoaded || activeClassId === null) return;
    let alive = true;
    api
      .getLesson(lessonId, activeClassId)
      .then((detail) => {
        if (!alive) return;
        setLesson((prev) =>
          prev
            ? {
                ...prev,
                effective_class_profile: detail.effective_class_profile ?? prev.effective_class_profile,
                class_profile_source: detail.class_profile_source,
              }
            : prev,
        );
      })
      // The page works without it: only the displayed profile would be the unit's.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [lessonId, lessonLoaded, activeClassId]);

  const flushSop = sop.flush;
  const goStage = useCallback(
    (next: StageKey) => {
      // Moving on is a moment to save, without waiting for the timer.
      void flushSop();
      movedStage.current = true;
      setChosenStage(next);
      if (next === "overview" || next === 1) setViewerOpen(false);
      try {
        window.sessionStorage.setItem(stageStorageKey(lessonId), String(next));
      } catch {
        // private mode: the page just opens on the default step next time
      }
    },
    [lessonId, flushSop],
  );

  // A new step starts at its top, with focus on its title (not on first load).
  useEffect(() => {
    if (!movedStage.current || stage === null) return;
    try {
      window.scrollTo({ top: 0 });
    } catch {
      // jsdom / old browsers
    }
    (stage === "overview" ? todayHeadingRef : stageHeadingRef).current?.focus();
  }, [stage]);

  const onObjective = useCallback(
    (value: string) => {
      objectiveRef.current = value;
      setObjective(value);
      touchPlan();
    },
    [touchPlan],
  );

  const onSteps = useCallback(
    (next: EditableStep[]) => {
      stepsRef.current = next;
      setSteps(next);
      touchPlan();
    },
    [touchPlan],
  );

  const appendStep = useCallback(
    (step: Omit<EditableStep, "key">) => {
      const next = [...stepsRef.current, newStep(step)];
      stepsRef.current = next;
      setSteps(next);
      // An adopted suggestion lands in the steps: show them.
      setOpen((prev) => ({ ...prev, steps: true }));
      touchPlan();
    },
    [touchPlan],
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

  const flushPlan = planSave.flush;
  /** Saves whatever is not saved yet: the plan, 本課背景, then the SOP. Resolves to an error message, or null. */
  const saveAll = useCallback(async (): Promise<string | null> => {
    const planFailed = await flushPlan();
    if (planFailed) return planFailed;
    const contextFailed = (await contextRef.current?.save()) ?? null;
    if (contextFailed) return contextFailed;
    return flushSop();
  }, [flushPlan, flushSop]);

  /** Leaving a field saves the plan and the SOP now, without waiting for the timer. */
  const saveOnBlur = useCallback(() => {
    void flushPlan().then((failed) => (failed ? failed : flushSop()));
  }, [flushPlan, flushSop]);

  // An in-app navigation with something unsaved is held: save, then go on. The page only asks
  // when that save fails.
  const { blocker, release } = guard;
  const leaving = blocker.state === "blocked";
  useEffect(() => {
    if (!leaving) return;
    let alive = true;
    void saveAll().then((failed) => {
      if (!alive) return;
      if (failed) {
        setLeaveFailed(true);
      } else {
        release();
        blocker.proceed?.();
      }
    });
    return () => {
      alive = false;
    };
    // `blocker` is a new object on every render; its state is what matters.
  }, [leaving]); // eslint-disable-line react-hooks/exhaustive-deps

  /**
   * 開始上課 (or 回到課堂): what is pending is saved first; if that fails, nothing starts.
   * The run belongs to `classId` (default: the class in play). When several classes use the unit
   * and none is in play, the dialog asks which class, once. The SOP is never checked.
   */
  async function start(classId: number | null = activeClassId) {
    if (classId === null && unitClasses.length > 1) {
      setChoosingClass(true);
      setStartError(null);
      return;
    }
    setStarting(true);
    setStartError(null);
    const failed = await saveAll();
    if (failed) {
      setChoosingClass(false);
      setStartError(`沒有儲存成功，所以還沒開始上課：${failed}`);
      setStarting(false);
      return;
    }
    try {
      const current = runs?.find((r) => r.ended_at === null && (r.class_id ?? null) === classId);
      const runId = current ? current.id : await startOrResumeRun(lessonId, classId);
      // Everything was just saved; whatever was typed during the save is saved by its own timer.
      guard.release();
      navigate(`/runs/${runId}`);
    } catch (err) {
      setChoosingClass(false);
      setStartError(errorMessage(err));
      setStarting(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!lesson || stage === null) return <Loading />;

  const route = effectiveRoute(routeChoiceEnabled, unit?.prep_route);
  const activityFirst = route === "activity";
  // The open run of the class in play; a lesson can be open in another class at the same time.
  const openRun = runs?.find((r) => r.ended_at === null && (r.class_id ?? null) === activeClassId);
  const pastRuns = (runs ?? []).filter((r) => r.ended_at !== null);
  const startLabel = starting ? "開始中…" : openRun ? "回到課堂" : "開始上課";

  // Saving in plain words. A failure stays on screen with a way to try again.
  const saveFailed = planSave.error ?? sop.saveError;
  const savedAt = [planSave.savedAt, sop.savedAt].filter((t): t is string => !!t).sort().pop() ?? null;
  const savedLine = saveFailed
    ? `沒有存到：${saveFailed}`
    : planSave.saving || sop.saving
      ? "儲存中…"
      : unsaved
        ? "尚未儲存"
        : savedAt
          ? `已自動儲存 ${formatClock(savedAt, false)}`
          : "會自動儲存";

  const sopStepsDone = STAGES.filter((s) => stageSummary(s.step, sop.material, sop.content) !== null).length;

  const suggestions = (
    <SuggestionsPanel lessonId={lesson.id} classId={activeClassId} onAppendStep={appendStep} pinned={ragOnTop} />
  );

  const today = (
    <section className="lesson-today" aria-labelledby="today-heading">
      <h2 id="today-heading" className="stage-title" tabIndex={-1} ref={todayHeadingRef}>
        <label htmlFor="lesson-objective">今天要教什麼？</label>
      </h2>
      <p className="quiet-note" id="lesson-objective-hint">
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
        <button type="button" className="primary-button big-button" onClick={() => void start()} disabled={starting}>
          {startLabel}
        </button>
      </div>
      <p className="quiet-note">
        {activeClass ? `上課紀錄會記在「${activeClass.name}」。` : ""}
        開始上課時會先幫你儲存。上課中不會呼叫 AI。
      </p>
      <ErrorText message={startError} />
    </section>
  );

  const linksSection = (
    <PlanSection
      id="links"
      title="單元想法的重點"
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
    <PlanSection id="steps" title="教學步驟" count={steps.length} open={open.steps} onToggle={() => toggleSection("steps")}>
      {activityFirst && (
        <p className="quiet-note">活動優先：先寫活動點子與教學步驟，寫完再連結單元想法的重點。</p>
      )}
      <StepsEditor
        steps={steps}
        onSteps={onSteps}
        showDiff={showDiff}
        diffByPreference={diffByPreference && diffToggle === null}
        onToggleDiff={toggleDiff}
      />
    </PlanSection>
  );

  // Activity-first: write the steps, then link the unit's focuses. Keys keep sections mounted on reorder.
  const prepOrder: Array<[string, ReactNode]> = activityFirst
    ? [
        ["steps", stepsSection],
        ["links", linksSection],
      ]
    : [
        ["links", linksSection],
        ["steps", stepsSection],
      ];

  const overview = (
    <>
      {today}
      {ragOnTop && suggestions}

      <section className="section lesson-prep" aria-labelledby="prep-heading">
        <h2 id="prep-heading">想多準備一點？</h2>
        <p className="sop-entry">
          <button type="button" className="text-link" onClick={() => goStage(1)}>
            一步一步備課
          </button>
          <span className="quiet-note">
            從教材開始，七個步驟帶你到目標、提問、暖身和 Exit ticket。
            {sopStepsDone > 0 ? `已經做了 ${sopStepsDone} 步。` : "每一步都可以略過。"}
          </span>
        </p>
        <div className="plan-sections">
          {prepOrder.map(([key, node]) => (
            <Fragment key={key}>{node}</Fragment>
          ))}
          {pastRuns.length > 0 && (
            <PlanSection
              id="runs"
              title="上課紀錄與課後補寫"
              count={pastRuns.length}
              open={open.runs}
              onToggle={() => toggleSection("runs")}
            >
              <PastRuns runs={pastRuns} />
            </PlanSection>
          )}
          <PlanSection
            id="context"
            title="本課背景：班級特性與我的範例"
            open={open.context}
            onToggle={() => toggleSection("context")}
          >
            <LessonContextCard
              ref={contextRef}
              lesson={lesson}
              unitProfile={unit?.class_profile ?? ""}
              activeClass={activeClass}
              onDirtyChange={setContextDirty}
              onSaved={(detail) =>
                setLesson((prev) =>
                  prev
                    ? {
                        ...prev,
                        class_description: detail.class_description ?? prev.class_description,
                        my_example: detail.my_example ?? prev.my_example,
                        effective_class_profile: detail.effective_class_profile ?? prev.effective_class_profile,
                        class_profile_source: detail.class_profile_source ?? prev.class_profile_source,
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
        </div>
      </section>

      {!ragOnTop && suggestions}
    </>
  );

  function renderStep(step: 1 | 2 | 3 | 4 | 5 | 6 | 7) {
    if (sop.status === "loading") return <Loading />;
    if (sop.status === "error") {
      return (
        <div className="stage-block">
          <ErrorText message={sop.loadError} />
          <p className="quiet-note">
            這一步暫時打不開；「本節概要」和開始上課都不受影響。
            <button type="button" className="text-link is-plain" onClick={sop.reload}>
              再試一次
            </button>
          </p>
        </div>
      );
    }
    switch (step) {
      case 1:
        return <StageMaterial sop={sop} lessonId={lessonId} />;
      case 2:
        return <StageAnalysis sop={sop} onGoToMaterial={() => goStage(1)} />;
      case 3:
        return <StageObjectives sop={sop} todayObjective={objective} />;
      case 4:
        return <StageIntegrate sop={sop} onGoToStage={goStage} />;
      case 5:
        return <StageTasks sop={sop} onGoToStage={goStage} />;
      case 6:
        return <StageWarmup sop={sop} />;
      default:
        return <StageExit sop={sop} />;
    }
  }

  const step = stage === "overview" ? null : stage;
  const withViewer = viewerOpen && step !== null && step >= 2;

  return (
    <>
      <div className="head-row page-head lesson-head">
        <div>
          <p className="crumbs">
            {activeClass ? (
              <>
                <Link to="/">班級</Link> / <Link to={`/classes/${activeClass.id}`}>{activeClass.name}</Link>
              </>
            ) : (
              <Link to="/library">備課庫</Link>
            )}{" "}
            / <Link to={`/units/${lesson.unit_id}`}>{unit?.title ?? "單元"}</Link>
          </p>
          <h1 id="lesson-heading">{lesson.title}</h1>
          {lesson.lesson_date && <p className="lead">上課日期 {formatDate(lesson.lesson_date)}</p>}
        </div>
        <div className="lesson-head-side">
          <p className={`saved${saveFailed ? " is-failed" : ""}`} role="status">
            {savedLine}
            {saveFailed && (
              <>
                {" "}
                <button type="button" className="text-link is-plain" onClick={() => void saveAll()}>
                  再試一次
                </button>
              </>
            )}
          </p>
          {step !== null && (
            <button type="button" className="secondary-button" onClick={() => void start()} disabled={starting}>
              {startLabel}
            </button>
          )}
        </div>
      </div>
      {step !== null && <ErrorText message={startError} />}

      {lesson.previous_next_change && (
        <Notice tone="warn">
          <strong>前一課的下次要改：</strong>
          {lesson.previous_next_change}
        </Notice>
      )}

      <div className={`guided${withViewer ? " with-viewer" : ""}`} onBlur={saveOnBlur}>
        <Rail stage={stage} onStage={goStage} material={sop.material} content={sop.content} />

        <div className="stage">
          {/* The overview stays mounted while a step is open, so half-written 本課背景 is not lost. */}
          <div hidden={step !== null}>{overview}</div>
          {step !== null && (
            <>
              <div className="stage-head">
                <h2 className="stage-title" tabIndex={-1} ref={stageHeadingRef}>
                  {stageName(step)}
                </h2>
                {step >= 2 && (
                  <button
                    type="button"
                    className="text-link is-plain"
                    aria-expanded={viewerOpen}
                    aria-controls="material-viewer"
                    onClick={() => setViewerOpen(!viewerOpen)}
                  >
                    {viewerOpen ? "收起教材" : "對照教材"}
                  </button>
                )}
              </div>
              {renderStep(step)}
              <div className="stage-nav">
                <button
                  type="button"
                  className="text-link is-plain"
                  onClick={() => goStage(step === 1 ? "overview" : ((step - 1) as StageKey))}
                >
                  {step === 1 ? "回到本節概要" : "上一步"}
                </button>
                <span className="grow" />
                {step < 7 && (
                  <button type="button" className="text-link is-plain" onClick={() => goStage((step + 1) as StageKey)}>
                    先略過這一步
                  </button>
                )}
                <button
                  type="button"
                  className="primary-button"
                  onClick={() => goStage(step < 7 ? ((step + 1) as StageKey) : "overview")}
                >
                  {step < 7 ? `下一步：${stageName(step + 1)}` : "完成，回到本節概要"}
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      <MaterialViewer
        mode="panel"
        materialId={sop.material?.id ?? null}
        material={sop.material}
        open={withViewer}
        onClose={() => setViewerOpen(false)}
        empty={
          <p className="quiet-note">
            {sop.materialDeleted ? "教材已刪除。" : "還沒有教材。"}
            <button type="button" className="text-link is-plain" onClick={() => goStage(1)}>
              到第 1 步{sop.materialDeleted ? "放進新的教材" : "貼上教材"}
            </button>
          </p>
        }
      />

      {choosingClass && (
        <StartClassDialog
          classes={unitClasses}
          runs={runs ?? []}
          starting={starting}
          onStart={(classId) => {
            chooseClass(classId);
            void start(classId);
          }}
          onCancel={() => setChoosingClass(false)}
        />
      )}

      {leaveFailed && (
        <LeaveConfirm
          guard={guard}
          message="有內容沒有存到。現在離開，這些內容就會遺失。"
          onSave={saveAll}
        />
      )}
    </>
  );
}
