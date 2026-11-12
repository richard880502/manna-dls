import { useEffect, useState, type FormEvent } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Lesson, Unit } from "../api/types";
import { ErrorText, Loading, PrivacyHint } from "../components/common";
import { CORE_STATUS_LABELS, formatDate } from "../lib/labels";

export function UnitPage() {
  const unitId = Number(useParams().unitId);
  const [unit, setUnit] = useState<Unit | null>(null);
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [title, setTitle] = useState("");
  const [lessonDate, setLessonDate] = useState("");
  const [classDescription, setClassDescription] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

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
    return () => {
      alive = false;
    };
  }, [unitId]);

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
      const lesson = await api.createLesson(unitId, {
        title: trimmed,
        ...(lessonDate ? { lesson_date: lessonDate } : {}),
        ...(classDescription.trim() ? { class_description: classDescription.trim() } : {}),
      });
      setLessons((prev) => [...(prev ?? []), lesson]);
      setTitle("");
      setLessonDate("");
      setClassDescription("");
    } catch (err) {
      setCreateError(errorMessage(err));
    } finally {
      setCreating(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!unit || !lessons) return <Loading />;

  return (
    <section aria-labelledby="unit-heading">
      <p className="breadcrumb">
        <Link to="/plan">Study &amp; Plan</Link> / {unit.title}
      </p>
      <div className="page-intro">
        <p className="eyebrow warm">Unit</p>
        <h2 id="unit-heading">{unit.title}</h2>
      </div>

      <div className="unit-grid">
        <section className="panel-card" aria-labelledby="unit-core-heading">
          <div className="card-heading">
            <h3 id="unit-core-heading">單元前 CoRe</h3>
            <span className={`core-status core-status-${unit.core_status}`}>
              {CORE_STATUS_LABELS[unit.core_status]}
            </span>
          </div>
          <p>
            寫下這個單元 2–3 個最重要的學習重點，以及每個重點的八題思考。可以留白，也可以選「目前沒有想法」。
          </p>
          <Link className="primary-button link-button" to={`/units/${unit.id}/core`}>
            {unit.core_status === "none" ? "開始寫 CoRe" : "打開 CoRe"}
          </Link>
        </section>

        <section className="panel-card" aria-labelledby="lessons-heading">
          <div className="card-heading">
            <h3 id="lessons-heading">課例</h3>
            <span className="soft-tag">{lessons.length} 個課例</span>
          </div>
          {lessons.length === 0 ? (
            <p className="empty-preview">還沒有課例。建立第一個課例開始備課。</p>
          ) : (
            <ul className="lesson-list">
              {lessons.map((lesson) => (
                <li key={lesson.id}>
                  <Link to={`/lessons/${lesson.id}`}>
                    <strong>{lesson.title}</strong>
                    <span>
                      {lesson.lesson_date ? formatDate(lesson.lesson_date) : "未設定日期"}
                      {lesson.class_description ? ` · ${lesson.class_description}` : ""}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          )}

          <form className="create-lesson-form" onSubmit={onCreate}>
            <h4>建立新課例</h4>
            <label className="field-label" htmlFor="lesson-title">
              課例名稱
            </label>
            <input
              id="lesson-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="例如：第 2 節・句型練習"
              disabled={creating}
            />
            <div className="two-fields">
              <div>
                <label className="field-label" htmlFor="lesson-date">
                  上課日期（選填）
                </label>
                <input
                  id="lesson-date"
                  type="date"
                  value={lessonDate}
                  onChange={(e) => setLessonDate(e.target.value)}
                  disabled={creating}
                />
              </div>
              <div>
                <label className="field-label" htmlFor="lesson-class">
                  班級描述（選填）
                </label>
                <input
                  id="lesson-class"
                  value={classDescription}
                  onChange={(e) => setClassDescription(e.target.value)}
                  placeholder="例如：高一普通班，口說練習需求高"
                  disabled={creating}
                />
              </div>
            </div>
            <PrivacyHint />
            <ErrorText message={createError} />
            <button className="primary-button" type="submit" disabled={creating}>
              {creating ? "建立中…" : "建立課例"}
            </button>
          </form>
        </section>
      </div>
    </section>
  );
}
