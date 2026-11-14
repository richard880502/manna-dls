import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { errorMessage } from "../../api/client";
import type { ClassNextAction } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { nextLessonTitle } from "../../lib/classes";
import { startOrResumeRun } from "../../lib/runs";
import { ErrorText } from "../common";

/** Starts a lesson in a class, which is already known here: nothing is asked. */
export function useStartInClass(classId: number) {
  const navigate = useNavigate();
  const { setCurrentClassId } = useApp();
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function start(lessonId: number) {
    setStarting(true);
    setError(null);
    setCurrentClassId(classId);
    try {
      navigate(`/runs/${await startOrResumeRun(lessonId, classId)}`);
    } catch (err) {
      setError(errorMessage(err));
      setStarting(false);
    }
  }

  return { start, starting, error };
}

interface NextLessonProps {
  classId: number;
  /** The quiet line above the title, e.g. 「接下來 · 高一 3 班」. */
  kicker: string;
  next: Exclude<ClassNextAction, { kind: "add_unit" }>;
  /** One quiet sentence under the title, e.g. 「上一節 11/12 上完。」. */
  detail?: string;
  headingId: string;
}

/**
 * The one focal block of the home page and the class page: the class's next lesson and its one
 * filled action, 「開始上課」. The class comes from where the teacher is, so starting asks nothing.
 */
export function NextLesson({ classId, kicker, next, detail, headingId }: NextLessonProps) {
  const { setCurrentClassId } = useApp();
  const { start, starting, error } = useStartInClass(classId);
  const enter = () => setCurrentClassId(classId);

  return (
    <section className="next" aria-labelledby={headingId}>
      <div>
        <p className="kicker">{kicker}</p>
        <h1 id={headingId}>{next.kind === "plan_unit" ? next.unit_title : nextLessonTitle(next)}</h1>
        {next.kind === "plan_unit" ? <p>這個單元還沒有課例。先備第一節課。</p> : detail ? <p>{detail}</p> : null}
        <ErrorText message={error} />
      </div>
      <div className="actions">
        {next.kind === "plan_unit" ? (
          <Link className="primary-button big-button link-button" to={`/units/${next.unit_id}`} onClick={enter}>
            去備課
          </Link>
        ) : (
          <>
            <Link className="text-link" to={`/lessons/${next.lesson_id}`} onClick={enter}>
              {next.kind === "resume_run" ? "看教案" : "繼續備課"}
            </Link>
            {next.kind === "resume_run" ? (
              <Link className="primary-button big-button link-button" to={`/runs/${next.run_id}`} onClick={enter}>
                回到課堂
              </Link>
            ) : (
              <button
                type="button"
                className="primary-button big-button"
                disabled={starting}
                onClick={() => void start(next.lesson_id)}
              >
                {starting ? "開始中…" : "開始上課"}
              </button>
            )}
          </>
        )}
      </div>
    </section>
  );
}
