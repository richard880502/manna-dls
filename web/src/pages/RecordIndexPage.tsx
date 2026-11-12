import { useEffect, useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Lesson, LessonRun, Unit } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { formatDateTime } from "../lib/labels";
import { startOrResumeRun } from "../lib/runs";

function RunRow({ run }: { run: LessonRun }) {
  const open = run.ended_at === null;
  return (
    <li>
      <Link to={open ? `/runs/${run.id}` : `/runs/${run.id}/review`}>
        <strong>
          {run.lesson_title}
          <span className="run-unit"> · {run.unit_title}</span>
        </strong>
        <span>
          {formatDateTime(run.started_at)} · 順利 {run.good_count} · 卡住 {run.stuck_count} ·{" "}
          {open ? "上課中，回到課堂紀錄" : run.review_completed_at ? "已完成補寫" : "課後補寫"}
        </span>
      </Link>
    </li>
  );
}

/** Teach & Record：開始本節課、進行中的課、最近的課（課後補寫）。 */
export function RecordIndexPage() {
  const navigate = useNavigate();
  const ids = useId();
  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [unitId, setUnitId] = useState("");
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [lessonId, setLessonId] = useState("");
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([api.listRuns(), api.listUnits()])
      .then(([r, u]) => {
        if (!alive) return;
        setRuns(r);
        setUnits(u);
      })
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (!unitId) return;
    let alive = true;
    api
      .listLessons(Number(unitId))
      .then((l) => alive && setLessons(l))
      .catch((err: unknown) => alive && setStartError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [unitId]);

  async function start() {
    if (!lessonId) return;
    setStarting(true);
    setStartError(null);
    try {
      navigate(`/runs/${await startOrResumeRun(Number(lessonId))}`);
    } catch (err) {
      setStartError(errorMessage(err));
      setStarting(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!runs || !units) return <Loading />;

  const openRuns = runs.filter((r) => r.ended_at === null);
  const pastRuns = runs.filter((r) => r.ended_at !== null);

  return (
    <section aria-labelledby="record-heading">
      <div className="page-intro">
        <p className="eyebrow warm">02 · Teach &amp; Record</p>
        <h2 id="record-heading">課中書籤與課後補寫</h2>
        <p>
          上課時只要按「順利」或「卡住」，就會記下時間；想多記一句話可以打字，也可以略過。下課後再依時間回顧、補寫。課中不呼叫
          AI、不錄音、不拍照；沒有書籤或沒有補寫，都不影響完成。
        </p>
      </div>

      <div className="unit-stack">
        {openRuns.length > 0 && (
          <section className="panel-card" aria-labelledby="open-runs-heading">
            <div className="card-heading">
              <h3 id="open-runs-heading">上課中</h3>
              <span className="status-pill">{openRuns.length} 節</span>
            </div>
            <ul className="lesson-list run-list">
              {openRuns.map((r) => (
                <RunRow key={r.id} run={r} />
              ))}
            </ul>
          </section>
        )}

        <section className="panel-card" aria-labelledby="start-run-heading">
          <h3 id="start-run-heading">開始本節課</h3>
          {units.length === 0 ? (
            <p className="empty-preview">
              還沒有單元與課例。先到 <Link to="/plan">Study &amp; Plan</Link> 建立課例，也可以在課例頁直接開始。
            </p>
          ) : (
            <>
              <div className="two-fields">
                <div>
                  <label className="field-label" htmlFor={`${ids}-unit`}>
                    單元
                  </label>
                  <select
                    id={`${ids}-unit`}
                    value={unitId}
                    onChange={(e) => {
                      setUnitId(e.target.value);
                      setLessonId("");
                      setLessons(null);
                    }}
                  >
                    <option value="">請選擇單元</option>
                    {units.map((u) => (
                      <option key={u.id} value={String(u.id)}>
                        {u.title}
                      </option>
                    ))}
                  </select>
                </div>
                <div>
                  <label className="field-label" htmlFor={`${ids}-lesson`}>
                    課例
                  </label>
                  <select
                    id={`${ids}-lesson`}
                    value={lessonId}
                    onChange={(e) => setLessonId(e.target.value)}
                    disabled={!lessons}
                  >
                    <option value="">{lessons && lessons.length === 0 ? "這個單元還沒有課例" : "請選擇課例"}</option>
                    {(lessons ?? []).map((l) => (
                      <option key={l.id} value={String(l.id)}>
                        {l.title}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <div className="button-row record-start-row">
                <button type="button" className="primary-button" onClick={() => void start()} disabled={!lessonId || starting}>
                  {starting ? "開始中…" : "開始本節課"}
                </button>
              </div>
              <p className="field-hint">開始時需要連上網路；開始之後的書籤即使離線也會先存在裝置。</p>
            </>
          )}
          <ErrorText message={startError} />
        </section>

        <section className="panel-card" aria-labelledby="past-runs-heading">
          <div className="card-heading">
            <h3 id="past-runs-heading">最近的課</h3>
            <span className="soft-tag">{pastRuns.length} 節</span>
          </div>
          {pastRuns.length === 0 ? (
            <p className="empty-preview">還沒有上過的課。</p>
          ) : (
            <ul className="lesson-list run-list">
              {pastRuns.map((r) => (
                <RunRow key={r.id} run={r} />
              ))}
            </ul>
          )}
        </section>
      </div>
    </section>
  );
}
