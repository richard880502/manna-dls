import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { LessonDetail, LessonRun, Unit, WorksheetSummary } from "../api/types";
import { StartClassDialog } from "../components/classes/StartClassDialog";
import { WorksheetPlan } from "../components/sop/WorksheetSummary";
import { ErrorText, Loading, Notice } from "../components/common";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { Rail } from "../components/sop/Rail";
import { StageAnalysis } from "../components/sop/StageAnalysis";
import { StageMaterial } from "../components/sop/StageMaterial";
import { StageObjectives } from "../components/sop/StageObjectives";
import { StageExit, StageIntegrate, StageTasks, StageWarmup } from "../components/sop/StagesManual";
import { useSop } from "../components/sop/useSop";
import { useActiveClass } from "../lib/classes";
import { formatClock, formatDate } from "../lib/labels";
import { startOrResumeRun } from "../lib/runs";
import { STAGES, stageName, type StageKey } from "../lib/sop";

const stageStorageKey = (lessonId: number) => `manna.lesson.${lessonId}.stage`;

function readStoredStage(lessonId: number): StageKey | null {
  try {
    const step = Number(window.sessionStorage.getItem(stageStorageKey(lessonId)));
    return STAGES.some((s) => s.step === step) ? (step as StageKey) : null;
  } catch {
    return null;
  }
}

/**
 * 課例頁: the seven-step preparation workspace (docs/DESIGN_OVERHAUL.md step 2, direction A), and
 * nothing else. A progress rail on the left, one step at a time in the middle, Manna's suggestions
 * inside the step they belong to, and the material sliding in beside it on request. The steps
 * lead to 產生學習單: the rail ends with it, and it is the filled button of step 7.
 *
 * The page opens on the step of the last visit, else on step 1. 「開始上課」 is a secondary button
 * in the page head on every step and never looks at the SOP.
 *
 * Nothing has a save button: the SOP is saved a moment after the last edit, and at once when the
 * teacher leaves a field. Starting the lesson, making a worksheet, leaving the page and asking
 * Manna save what is pending first. Only when that save fails does leaving ask (儲存後離開／留在這頁／
 * 不儲存，直接離開), so nothing is dropped silently (R2).
 *
 * The earlier first view (今天要教什麼？, 教學步驟, 單元想法的重點, 本課背景, SIOP 提示, Manna 備課建議)
 * is no longer shown; its data stays on the server, and a line written in 今天要教什麼？ is still
 * offered in step 3.
 */
export function LessonPage() {
  const lessonId = Number(useParams().lessonId);
  // One workspace per lesson: its drafts and timers never carry over to another lesson.
  return <LessonWorkspace key={lessonId} lessonId={lessonId} />;
}

function LessonWorkspace({ lessonId }: { lessonId: number }) {
  const navigate = useNavigate();
  const [lesson, setLesson] = useState<LessonDetail | null>(null);
  const [unit, setUnit] = useState<Unit | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  /** 「這節課在哪一班上？」: open when several classes use the unit and the teacher came from the library. */
  const [choosingClass, setChoosingClass] = useState(false);
  const { classes: unitClasses, active: activeClass, choose: chooseClass } = useActiveClass(unit);
  const activeClassId = activeClass?.id ?? null;

  const sop = useSop(lessonId, activeClassId);
  /** The step the teacher went to in this visit (null: none yet, the page opens on the stored one or step 1). */
  const [chosenStage, setChosenStage] = useState<StageKey | null>(null);
  const [storedStage] = useState(() => readStoredStage(lessonId));
  const stage: StageKey = chosenStage ?? storedStage ?? 1;
  const [viewerOpen, setViewerOpen] = useState(false);
  const stageHeadingRef = useRef<HTMLHeadingElement>(null);
  const movedStage = useRef(false);

  const guard = useLeaveGuard(sop.dirty);
  /** Leaving was held and the save it triggered failed: now the page asks. */
  const [leaveFailed, setLeaveFailed] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .getLesson(lessonId)
      .then((detail) => {
        if (!alive) return;
        setLesson(detail);
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

  const flushSop = sop.flush;
  const goStage = useCallback(
    (next: StageKey) => {
      // Moving on is a moment to save, without waiting for the timer.
      void flushSop();
      movedStage.current = true;
      setChosenStage(next);
      if (next === 1) setViewerOpen(false);
      try {
        window.sessionStorage.setItem(stageStorageKey(lessonId), String(next));
      } catch {
        // private mode: the page just opens on step 1 next time
      }
    },
    [lessonId, flushSop],
  );

  // A new step starts at its top, with focus on its title (not on first load).
  useEffect(() => {
    if (!movedStage.current) return;
    try {
      window.scrollTo({ top: 0 });
    } catch {
      // jsdom / old browsers
    }
    stageHeadingRef.current?.focus();
  }, [stage]);

  /** Saves whatever is not saved yet. Resolves to an error message, or null. */
  const saveAll = useCallback((): Promise<string | null> => flushSop(), [flushSop]);

  // 學習單: made from this lesson at any step (spec §4.1); nothing in the SOP has to be finished.
  const [worksheets, setWorksheets] = useState<WorksheetSummary[]>([]);
  const [makingWorksheet, setMakingWorksheet] = useState(false);
  const [worksheetError, setWorksheetError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .listWorksheets(lessonId)
      .then((rows) => {
        if (alive && Array.isArray(rows)) setWorksheets(rows);
      })
      .catch(() => {
        // The list is a convenience; the lesson page does not depend on it.
      });
    return () => {
      alive = false;
    };
  }, [lessonId]);

  const [summaryOpen, setSummaryOpen] = useState(false);

  /** 產生學習單: saves what is pending, then shows what the worksheet will use (the steps' choices). */
  async function makeWorksheet() {
    setWorksheetError(null);
    setMakingWorksheet(true);
    try {
      const failed = await saveAll();
      if (failed) {
        setWorksheetError(`沒有儲存成功，所以還沒建立學習單：${failed}`);
        return;
      }
      setSummaryOpen(true);
    } finally {
      setMakingWorksheet(false);
    }
  }

  /**
   * 產生 (`generate`): makes the worksheet from the lesson with the choices of the steps and starts
   * it, nothing asked again. 調整設定: makes it and opens the generator's form with the same choices.
   */
  async function createFromLesson(generate: boolean) {
    setWorksheetError(null);
    setMakingWorksheet(true);
    try {
      const failed = await saveAll();
      if (failed) {
        setWorksheetError(`沒有儲存成功，所以還沒建立學習單：${failed}`);
        return;
      }
      const created = await api.createWorksheet({
        lesson_id: lessonId,
        ...(activeClassId !== null ? { class_id: activeClassId } : {}),
      });
      if (generate) {
        try {
          await api.generateWorksheet(created.id);
        } catch {
          // The worksheet exists: its page says why nothing started and offers 產生學習單 again.
        }
      }
      navigate(`/worksheets/${created.id}`);
    } catch (err) {
      setWorksheetError(errorMessage(err));
    } finally {
      setMakingWorksheet(false);
    }
  }

  /** Leaving a field saves the SOP now, without waiting for the timer. */
  const saveOnBlur = useCallback(() => {
    void flushSop();
  }, [flushSop]);

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
  if (!lesson) return <Loading />;

  // The open run of the class in play; a lesson can be open in another class at the same time.
  const openRun = runs?.find((r) => r.ended_at === null && (r.class_id ?? null) === activeClassId);
  const startLabel = starting ? "開始中…" : openRun ? "回到課堂" : "開始上課";
  const worksheetLabel = makingWorksheet ? "建立中…" : "產生學習單";
  // A line written in the earlier 今天要教什麼？ field; step 3 offers to file it as an objective.
  const todayObjective = lesson.latest_version?.objective ?? "";

  // Saving in plain words. A failure stays on screen with a way to try again.
  const saveFailed = sop.saveError;
  const savedLine = saveFailed
    ? `沒有存到：${saveFailed}`
    : sop.saving
      ? "儲存中…"
      : sop.dirty
        ? "尚未儲存"
        : sop.savedAt
          ? `已自動儲存 ${formatClock(sop.savedAt, false)}`
          : "會自動儲存";

  function renderStep(step: StageKey) {
    if (sop.status === "loading") return <Loading />;
    if (sop.status === "error") {
      return (
        <div className="stage-block">
          <ErrorText message={sop.loadError} />
          <p className="quiet-note">
            這一步暫時打不開；開始上課不受影響。
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
        return <StageObjectives sop={sop} todayObjective={todayObjective} lessonId={lessonId} />;
      case 4:
        return <StageIntegrate sop={sop} onGoToStage={goStage} />;
      case 5:
        return <StageTasks sop={sop} onGoToStage={goStage} lessonId={lessonId} />;
      case 6:
        return <StageWarmup sop={sop} />;
      default:
        return <StageExit sop={sop} />;
    }
  }

  const step = stage;
  const withViewer = viewerOpen && step >= 2;

  return (
    <>
      <div className="head-row page-head lesson-head">
        <div>
          <p className="crumbs">
            <Link to="/classes">班級</Link> /{" "}
            {activeClass ? (
              <Link to={`/classes/${activeClass.id}`}>{activeClass.name}</Link>
            ) : (
              <Link to="/units">備課庫</Link>
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
          <button type="button" className="secondary-button" onClick={() => void start()} disabled={starting}>
            {startLabel}
          </button>
        </div>
      </div>
      <ErrorText message={startError} />

      {lesson.previous_next_change && (
        <Notice tone="warn">
          <strong>前一課的下次要改：</strong>
          {lesson.previous_next_change}
        </Notice>
      )}

      <div className={`guided${withViewer ? " with-viewer" : ""}`} onBlur={saveOnBlur}>
        <Rail
          stage={stage}
          onStage={goStage}
          material={sop.material}
          content={sop.content}
          onWorksheet={() => void makeWorksheet()}
          worksheetBusy={makingWorksheet || starting}
        />

        <div className="stage">
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
            {step > 1 && (
              <button type="button" className="text-link is-plain" onClick={() => goStage((step - 1) as StageKey)}>
                上一步
              </button>
            )}
            <span className="grow" />
            {step < 7 ? (
              <>
                <button type="button" className="text-link is-plain" onClick={() => goStage((step + 1) as StageKey)}>
                  先略過這一步
                </button>
                <button type="button" className="primary-button" onClick={() => goStage((step + 1) as StageKey)}>
                  下一步：{stageName(step + 1)}
                </button>
              </>
            ) : (
              <button
                type="button"
                className="primary-button big-button"
                onClick={() => void makeWorksheet()}
                disabled={makingWorksheet || starting}
              >
                {worksheetLabel}
              </button>
            )}
          </div>
          {step === 7 && (
            <p className="quiet-note stage-end-note">
              學習單會帶入這節課的教材、目標與任務，產生後還可以修改。
              {worksheets.length > 0 && (
                <>
                  {" "}
                  這節課做過的學習單：
                  {worksheets.map((w) => (
                    <Link key={w.id} className="text-link is-plain" to={`/worksheets/${w.id}`}>
                      {w.title || "未命名的學習單"}
                    </Link>
                  ))}
                </>
              )}
            </p>
          )}
          <ErrorText message={worksheetError} />
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

      {summaryOpen && (
        <WorksheetPlan
          sop={sop}
          busy={makingWorksheet}
          error={worksheetError}
          onGenerate={() => void createFromLesson(true)}
          onAdjust={() => void createFromLesson(false)}
          onClose={() => setSummaryOpen(false)}
        />
      )}

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
