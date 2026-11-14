import { useEffect, useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Lesson, LessonRun, SchoolClass, Unit } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { useApp } from "../context/AppContext";
import { formatDateTime } from "../lib/labels";
import { startOrResumeRun } from "../lib/runs";

function RunRow({ run, showClass }: { run: LessonRun; showClass: boolean }) {
  const open = run.ended_at === null;
  return (
    <li>
      <Link to={open ? `/runs/${run.id}` : `/runs/${run.id}/review`}>
        <strong>
          {run.lesson_title}
          <span className="run-unit">
            {" "}
            · {run.unit_title}
            {showClass && run.class_name ? ` · ${run.class_name}` : ""}
          </span>
        </strong>
        <span>
          {formatDateTime(run.started_at)} · 順利 {run.good_count} · 卡住 {run.stuck_count} ·{" "}
          {open ? "上課中，回到課堂紀錄" : run.review_completed_at ? "已完成補寫" : "課後補寫"}
        </span>
      </Link>
    </li>
  );
}

interface RunGroup {
  key: string;
  /** null: runs without a class. */
  name: string | null;
  runs: LessonRun[];
}

/** Past runs grouped by class: the teacher's classes in their order, then runs without a class. */
export function groupRunsByClass(runs: LessonRun[], classes: SchoolClass[]): RunGroup[] {
  const groups: RunGroup[] = [];
  const order = classes.map((c) => c.id);
  const ids = [...new Set(runs.map((r) => r.class_id).filter((id): id is number => typeof id === "number"))].sort(
    (a, b) => {
      const ia = order.indexOf(a);
      const ib = order.indexOf(b);
      return (ia < 0 ? order.length : ia) - (ib < 0 ? order.length : ib) || a - b;
    },
  );
  for (const id of ids) {
    const mine = runs.filter((r) => r.class_id === id);
    groups.push({ key: String(id), name: mine[0]?.class_name ?? "班級", runs: mine });
  }
  const none = runs.filter((r) => typeof r.class_id !== "number");
  if (none.length > 0) groups.push({ key: "none", name: null, runs: none });
  return groups;
}

/** Teach & Record：開始本節課、進行中的課、最近的課（依班級分組，課後補寫）。 */
export function RecordIndexPage() {
  const navigate = useNavigate();
  const ids = useId();
  const { currentClassId, setCurrentClassId } = useApp();
  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [classes, setClasses] = useState<SchoolClass[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  /** "" = 不指定班級. */
  const [classId, setClassId] = useState(currentClassId ? String(currentClassId) : "");
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
    api
      .listClasses()
      .then((list) => alive && setClasses(list))
      // Without the classes the page still lists runs and starts lessons.
      .catch(() => undefined);
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

  if (loadError) return <ErrorText message={loadError} />;
  if (!runs || !units) return <Loading />;

  // A remembered class that is gone (archived) counts as no class.
  const selectedClass = classes.find((c) => String(c.id) === classId) ?? null;
  const unitsOfClass = selectedClass
    ? units.filter((u) => (u.classes ?? []).some((c) => c.id === selectedClass.id))
    : units;
  const unit = unitsOfClass.find((u) => String(u.id) === unitId) ?? null;
  // Several classes use the unit: the run needs to know which one.
  const needsClass = !selectedClass && (unit?.classes ?? []).length > 1;

  function pickClass(value: string) {
    setClassId(value);
    setCurrentClassId(value ? Number(value) : null);
    setUnitId("");
    setLessonId("");
    setLessons(null);
  }

  async function start() {
    if (!lessonId || needsClass) return;
    setStarting(true);
    setStartError(null);
    try {
      navigate(`/runs/${await startOrResumeRun(Number(lessonId), selectedClass?.id)}`);
    } catch (err) {
      setStartError(errorMessage(err));
      setStarting(false);
    }
  }

  const openRuns = runs.filter((r) => r.ended_at === null);
  const pastRuns = runs.filter((r) => r.ended_at !== null);
  const groups = groupRunsByClass(pastRuns, classes);
  const grouped = groups.some((g) => g.name !== null);

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
                <RunRow key={r.id} run={r} showClass />
              ))}
            </ul>
          </section>
        )}

        <section className="panel-card" aria-labelledby="start-run-heading">
          <h3 id="start-run-heading">開始本節課</h3>
          {units.length === 0 ? (
            <p className="empty-preview">
              還沒有單元與課例。先到 <Link to="/library">備課庫</Link> 建立課例，也可以在課例頁直接開始。
            </p>
          ) : (
            <>
              {classes.length > 0 && (
                <div className="two-fields single-field">
                  <div>
                    <label className="field-label" htmlFor={`${ids}-class`}>
                      班級
                    </label>
                    <select id={`${ids}-class`} value={selectedClass ? classId : ""} onChange={(e) => pickClass(e.target.value)}>
                      <option value="">不指定班級（列出備課庫所有單元）</option>
                      {classes.map((c) => (
                        <option key={c.id} value={String(c.id)}>
                          {c.name}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>
              )}
              <div className="two-fields">
                <div>
                  <label className="field-label" htmlFor={`${ids}-unit`}>
                    單元
                  </label>
                  <select
                    id={`${ids}-unit`}
                    value={unit ? unitId : ""}
                    onChange={(e) => {
                      setUnitId(e.target.value);
                      setLessonId("");
                      setLessons(null);
                    }}
                  >
                    <option value="">
                      {selectedClass && unitsOfClass.length === 0 ? "這個班級還沒有單元" : "請選擇單元"}
                    </option>
                    {unitsOfClass.map((u) => (
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
                <button
                  type="button"
                  className="primary-button"
                  onClick={() => void start()}
                  disabled={!lessonId || starting || needsClass}
                >
                  {starting ? "開始中…" : "開始本節課"}
                </button>
              </div>
              {needsClass && (
                <p className="field-hint" role="status">
                  這個單元有 {(unit?.classes ?? []).length} 個班級在用，請先在上面選擇班級。
                </p>
              )}
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
          ) : grouped ? (
            groups.map((g) => (
              <section key={g.key} className="run-group" aria-labelledby={`${ids}-group-${g.key}`}>
                <h4 id={`${ids}-group-${g.key}`}>
                  {g.name ?? "未指定班級"}
                  <span className="soft-tag">{g.runs.length} 節</span>
                </h4>
                <ul className="lesson-list run-list">
                  {g.runs.map((r) => (
                    <RunRow key={r.id} run={r} showClass={false} />
                  ))}
                </ul>
              </section>
            ))
          ) : (
            <ul className="lesson-list run-list">
              {pastRuns.map((r) => (
                <RunRow key={r.id} run={r} showClass={false} />
              ))}
            </ul>
          )}
        </section>
      </div>
    </section>
  );
}
