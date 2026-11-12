import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { LessonRun } from "../../api/types";
import { formatDateTime } from "../../lib/labels";
import { startOrResumeRun } from "../../lib/runs";
import { ErrorText } from "../common";

/** Teach & Record entry on the lesson page: 開始本節課 / 回到課堂紀錄, and past runs. */
export function RunsPanel({ lessonId }: { lessonId: number }) {
  const navigate = useNavigate();
  const [runs, setRuns] = useState<LessonRun[]>([]);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .listLessonRuns(lessonId)
      .then((r) => alive && setRuns(r))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [lessonId]);

  async function start() {
    setStarting(true);
    setError(null);
    try {
      navigate(`/runs/${await startOrResumeRun(lessonId)}`);
    } catch (err) {
      setError(errorMessage(err));
      setStarting(false);
    }
  }

  const open = runs.find((r) => r.ended_at === null);
  const past = runs.filter((r) => r.ended_at !== null);

  return (
    <section className="panel-card runs-panel" aria-labelledby="runs-heading">
      <div className="card-heading">
        <h3 id="runs-heading">Teach &amp; Record</h3>
        <span className="soft-tag">{runs.length} 次上課</span>
      </div>
      <p className="field-hint">上課時按「順利」或「卡住」留下書籤，下課後再補寫。課中不呼叫 AI。</p>
      <div className="button-row">
        {open ? (
          <Link className="primary-button link-button" to={`/runs/${open.id}`}>
            回到課堂紀錄
          </Link>
        ) : (
          <button type="button" className="primary-button" onClick={() => void start()} disabled={starting}>
            {starting ? "開始中…" : "開始本節課"}
          </button>
        )}
      </div>
      <ErrorText message={error} />
      {past.length > 0 && (
        <ul className="lesson-list run-list">
          {past.map((r) => (
            <li key={r.id}>
              <Link to={`/runs/${r.id}/review`}>
                <strong>{formatDateTime(r.started_at)}</strong>
                <span>
                  順利 {r.good_count} · 卡住 {r.stuck_count} · {r.review_completed_at ? "已完成補寫" : "課後補寫"}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
