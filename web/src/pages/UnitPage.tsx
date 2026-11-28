import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Lesson, LessonRun, Unit } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { Dialog, useDialogGuard } from "../components/Dialog";
import { MoreMenu } from "../components/library/Cards";
import { DeleteLessonDialog, DeleteUnitDialog } from "../components/unit/DeleteDialogs";
import { RenameDialog } from "../components/unit/RenameDialog";
import { UnitIdeas } from "../components/unit/UnitIdeas";
import { usedByText, useActiveClass } from "../lib/classes";
import { formatDate } from "../lib/labels";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import type { LibraryLocationState } from "./LibraryPage";

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
 * so neither is repeated here. The unit and each lesson have 改名 / 刪除 behind a ⋯; deleting
 * the unit goes back to 備課庫.
 */
export function UnitPage() {
  const unitId = Number(useParams().unitId);
  const navigate = useNavigate();
  const [unit, setUnit] = useState<Unit | null>(null);
  useDocumentTitle(unit?.title ?? "單元");
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [addingLesson, setAddingLesson] = useState(false);
  /** 新增一節課例 opens a dialog; focus goes back to the button when it closes. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);

  const [renamingUnit, setRenamingUnit] = useState(false);
  const [deletingUnit, setDeletingUnit] = useState(false);
  const [renamingLesson, setRenamingLesson] = useState<Lesson | null>(null);
  const [deletingLesson, setDeletingLesson] = useState<Lesson | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** The ⋯ a rename or delete was opened from: focus goes back there, or to 課例 once its row is gone. */
  const menuOpener = useRef<HTMLElement | null>(null);
  const lessonsHeading = useRef<HTMLHeadingElement>(null);
  const backToMenu = useCallback(
    () => (menuOpener.current?.isConnected ? menuOpener.current : lessonsHeading.current),
    [],
  );

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
          <div className="title-with-menu">
            <h1 id="unit-heading">{unit.title}</h1>
            <MoreMenu
              label={`更多動作「${unit.title}」`}
              items={[
                {
                  label: "改名",
                  onSelect: (from) => {
                    menuOpener.current = from;
                    setRenamingUnit(true);
                  },
                },
                {
                  label: "刪除",
                  onSelect: (from) => {
                    menuOpener.current = from;
                    setDeletingUnit(true);
                  },
                },
              ]}
            />
          </div>
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

      {renamingUnit && (
        <RenameDialog
          title="改單元名稱"
          label="單元名稱"
          initial={unit.title}
          emptyMessage="請輸入單元名稱。"
          returnFocus={backToMenu}
          onClose={() => setRenamingUnit(false)}
          onSave={async (title) => {
            setUnit(await api.updateUnit(unit.id, { title }));
            setRenamingUnit(false);
          }}
        />
      )}
      {deletingUnit && (
        <DeleteUnitDialog
          unit={unit}
          returnFocus={backToMenu}
          onClose={() => setDeletingUnit(false)}
          onDeleted={() => {
            const state: LibraryLocationState = { deletedUnit: unit.title };
            navigate("/units", { replace: true, state });
          }}
        />
      )}

      <section className="unit-lessons" aria-labelledby="lessons-heading">
        <h2 id="lessons-heading" ref={lessonsHeading} tabIndex={-1}>
          課例
        </h2>
        {notice && (
          <p className="quiet-note" role="status">
            {notice}
          </p>
        )}
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
                <div className="side">
                  {runs && <span>{taughtBy(runs, lesson.id)}</span>}
                  <MoreMenu
                    label={`更多動作「${lesson.title}」`}
                    items={[
                      {
                        label: "改名",
                        onSelect: (from) => {
                          menuOpener.current = from;
                          setRenamingLesson(lesson);
                        },
                      },
                      {
                        label: "刪除",
                        onSelect: (from) => {
                          menuOpener.current = from;
                          setDeletingLesson(lesson);
                        },
                      },
                    ]}
                  />
                </div>
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
        {renamingLesson && (
          <RenameDialog
            title="改課例名稱"
            label="課例名稱"
            initial={renamingLesson.title}
            emptyMessage="請輸入課例名稱。"
            returnFocus={backToMenu}
            onClose={() => setRenamingLesson(null)}
            onSave={async (title) => {
              const saved = await api.updateLesson(renamingLesson.id, { title });
              setLessons((prev) => (prev ?? []).map((l) => (l.id === saved.id ? { ...l, title: saved.title } : l)));
              setRenamingLesson(null);
              setNotice(null);
            }}
          />
        )}
        {deletingLesson && (
          <DeleteLessonDialog
            lesson={deletingLesson}
            returnFocus={backToMenu}
            onClose={() => setDeletingLesson(null)}
            onDeleted={() => {
              setLessons((prev) => (prev ?? []).filter((l) => l.id !== deletingLesson.id));
              setUnit((prev) => (prev ? { ...prev, lesson_count: Math.max(0, prev.lesson_count - 1) } : prev));
              setDeletingLesson(null);
              setNotice(`已刪除課例「${deletingLesson.title}」。`);
            }}
          />
        )}
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
