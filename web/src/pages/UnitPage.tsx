import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Lesson, LessonRun, Unit } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { Dialog, useDialogGuard } from "../components/Dialog";
import { UnitIdeas } from "../components/unit/UnitIdeas";
import { usedByText, useActiveClass } from "../lib/classes";
import { formatDate } from "../lib/labels";
import { useDocumentTitle } from "../lib/useDocumentTitle";

/** 「高一 3 班、高一 7 班上過」 for one lesson, from the teacher's finished runs. */
function taughtBy(runs: LessonRun[], lessonId: number): string {
  const mine = runs.filter((r) => r.lesson_id === lessonId && r.ended_at !== null);
  if (mine.length === 0) return "還沒上";
  const names = [...new Set(mine.map((r) => r.class_name).filter((n): n is string => !!n))];
  return names.length > 0 ? `${names.join("、")}上過` : "上過";
}

/** 新增一節課例: a name and an optional date. Lives in a dialog. */
function NewLessonForm({
  unitId,
  onCreated,
  onCancel,
}: {
  unitId: number;
  onCreated: (lesson: Lesson) => void;
  onCancel: () => void;
}) {
  const ids = useId();
  const [title, setTitle] = useState("");
  const [lessonDate, setLessonDate] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const requestClose = useDialogGuard({ busy: creating, dirty: title.trim() !== "" || lessonDate !== "" });

  async function onCreate(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) {
      setCreateError("請輸入課例名稱。");
      return;
    }
    setCreating(true);
    setCreateError(null);
    try {
      onCreated(
        await api.createLesson(unitId, {
          title: trimmed,
          ...(lessonDate ? { lesson_date: lessonDate } : {}),
        }),
      );
    } catch (err) {
      setCreateError(errorMessage(err));
      setCreating(false);
    }
  }

  return (
    <form className="create-lesson-form" onSubmit={onCreate} aria-label="新增一節課例">
      <div className="two-fields">
        <div>
          <label className="field-label" htmlFor={`${ids}-title`}>
            課例名稱
          </label>
          <input
            id={`${ids}-title`}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="例如：第 2 節：句型練習"
            disabled={creating}
          />
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-date`}>
            上課日期（可以不填）
          </label>
          <input
            id={`${ids}-date`}
            type="date"
            value={lessonDate}
            onChange={(e) => setLessonDate(e.target.value)}
            disabled={creating}
          />
        </div>
      </div>
      <ErrorText message={createError} />
      <div className="button-row">
        <button className="primary-button" type="submit" disabled={creating}>
          {creating ? "建立中…" : "建立課例"}
        </button>
        <button type="button" className="text-link is-plain" disabled={creating} onClick={requestClose ?? onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/**
 * 單元頁: the unit's lessons as ruled rows, the page's main content, with one filled action
 * (carry on with the next lesson). 單元想法 (CoRe) follows as one summary line that opens on
 * click. The class profile lives on the class page and materials in 教材 and step 1 of a lesson,
 * so neither is repeated here.
 */
export function UnitPage() {
  const unitId = Number(useParams().unitId);
  const [unit, setUnit] = useState<Unit | null>(null);
  useDocumentTitle(unit?.title ?? "單元");
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [addingLesson, setAddingLesson] = useState(false);
  /** 新增一節課例 opens a dialog; focus goes back to the button when it closes. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);

  useEffect(() => {
    let alive = true;
    Promise.all([api.getUnit(unitId), api.listLessons(unitId)])
      .then(([u, l]) => {
        if (!alive) return;
        setUnit(u);
        setLessons(l);
      })
      .catch((err: unknown) => {
        if (alive) setLoadError(errorMessage(err));
      });
    api
      .listRuns()
      .then((list) => alive && setRuns(list))
      // Only the quiet 「上過」 facts; the page works without them.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [unitId]);

  const { active: activeClass } = useActiveClass(unit);

  if (loadError) return <ErrorText message={loadError} />;
  if (!unit || !lessons) return <Loading />;

  // The lesson to carry on with: the first one not taught yet (in this class, when there is one).
  const taught = new Set(
    (runs ?? [])
      .filter((r) => r.ended_at !== null && (!activeClass || r.class_id === activeClass.id))
      .map((r) => r.lesson_id),
  );
  const continueLesson = lessons.find((l) => !taught.has(l.id)) ?? lessons[lessons.length - 1] ?? null;

  return (
    <>
      <p className="crumbs">
        <Link to="/classes">班級</Link> /{" "}
        {activeClass ? (
          <Link to={`/classes/${activeClass.id}`}>{activeClass.name}</Link>
        ) : (
          <Link to="/units">備課庫</Link>
        )}{" "}
        / {unit.title}
      </p>
      <div className="head-row page-head">
        <div>
          <h1 id="unit-heading">{unit.title}</h1>
          <p className="lead">
            {usedByText(unit.classes)}
            {unit.classes.length === 0 && (
              <>
                。<Link to="/units">到備課庫加到班級</Link>
              </>
            )}
          </p>
        </div>
        {continueLesson && (
          <Link className="primary-button link-button" to={`/lessons/${continueLesson.id}`}>
            繼續備課：{continueLesson.title}
          </Link>
        )}
      </div>

      <section className="unit-lessons" aria-labelledby="lessons-heading">
        <h2 id="lessons-heading">課例</h2>
        {lessons.length === 0 ? (
          <p className="empty-line">還沒有課例。建立第一節課例開始備課。</p>
        ) : (
          <ol className="rows">
            {lessons.map((lesson, i) => (
              <li key={lesson.id} className="row">
                <div>
                  <h3>
                    <span className="num">{i + 1}</span>
                    <Link to={`/lessons/${lesson.id}`}>{lesson.title}</Link>
                  </h3>
                  {lesson.lesson_date && <p className="meta">{formatDate(lesson.lesson_date)}</p>}
                </div>
                {runs && <p className="side">{taughtBy(runs, lesson.id)}</p>}
              </li>
            ))}
          </ol>
        )}

        <p className="after-rows">
          <button
            type="button"
            className={lessons.length === 0 ? "primary-button" : "text-link"}
            aria-haspopup="dialog"
            onClick={(event) => {
              opener.current = event.currentTarget;
              setAddingLesson(true);
            }}
          >
            新增一節課例
          </button>
        </p>
        {addingLesson && (
          <Dialog title="新增一節課例" returnFocus={backToOpener} onClose={() => setAddingLesson(false)}>
            <NewLessonForm
              unitId={unitId}
              onCancel={() => setAddingLesson(false)}
              onCreated={(lesson) => {
                setLessons((prev) => [...(prev ?? []), lesson]);
                setAddingLesson(false);
              }}
            />
          </Dialog>
        )}
      </section>

      <UnitIdeas unit={unit} />
    </>
  );
}
