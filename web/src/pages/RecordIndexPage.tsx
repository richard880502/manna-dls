import { useEffect, useId, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../api/client";
import type { LessonRun, SchoolClass } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { formatShortDate } from "../lib/labels";

function RunRow({ run, showClass }: { run: LessonRun; showClass: boolean }) {
  const open = run.ended_at === null;
  const to = open ? `/runs/${run.id}` : `/runs/${run.id}/review`;
  const action = open ? "回到課堂" : run.review_completed_at ? "回顧" : "補寫";
  return (
    <li className="row">
      <div>
        <h3>
          <Link to={to}>
            {showClass && run.class_name ? `${run.class_name}　` : ""}
            {run.unit_title}　{run.lesson_title}
          </Link>
        </h3>
        <p className="meta">
          {formatShortDate(run.started_at)} · 順利 {run.good_count} 次 · 卡住 {run.stuck_count} 次 ·{" "}
          {open ? "上課中" : run.review_completed_at ? "已補寫" : "還沒補寫"}
        </p>
      </div>
      <p className="side">
        <Link
          className={`text-link${action === "回顧" ? " is-plain" : ""}`}
          to={to}
          aria-label={`${action}：${run.lesson_title} ${formatShortDate(run.started_at)}`}
        >
          {action}
        </Link>
      </p>
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

/**
 * 全部上課紀錄: every lesson the teacher has taught, grouped by class, each a way into its
 * write-up or review. Lessons are started from a class (or from the lesson itself), not here;
 * each class page shows its own records, and this page also holds the ones without a class.
 */
export function RecordIndexPage() {
  const ids = useId();
  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [classes, setClasses] = useState<SchoolClass[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .listRuns()
      .then((r) => alive && setRuns(r))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    api
      .listClasses()
      .then((list) => alive && setClasses(list))
      // Without the classes the runs are still listed, in the order they come.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  if (loadError) return <ErrorText message={loadError} />;
  if (!runs) return <Loading />;

  const openRuns = runs.filter((r) => r.ended_at === null);
  const pastRuns = runs.filter((r) => r.ended_at !== null);
  const groups = groupRunsByClass(pastRuns, classes);

  return (
    <>
      <p className="crumbs">
        <Link to="/">班級</Link> / 上課紀錄
      </p>
      <div className="page-head">
        <h1 id="record-heading">上課紀錄</h1>
        <p className="lead">
          上課時按「順利」或「卡住」記下那一刻，下課後再回顧、補寫。沒有書籤或沒有補寫，都沒關係。上課從班級頁或課例頁開始。
        </p>
      </div>

      {runs.length === 0 && <p className="empty-line">還沒有上過的課。</p>}

      {openRuns.length > 0 && (
        <section className="section" aria-labelledby="open-runs-heading">
          <h2 id="open-runs-heading">上課中</h2>
          <ul className="rows">
            {openRuns.map((r) => (
              <RunRow key={r.id} run={r} showClass />
            ))}
          </ul>
        </section>
      )}

      {groups.map((g) => (
        <section key={g.key} className="section" aria-labelledby={`${ids}-group-${g.key}`}>
          <h2 id={`${ids}-group-${g.key}`}>{g.name ?? "沒有記在班級的課"}</h2>
          <ul className="rows">
            {g.runs.map((r) => (
              <RunRow key={r.id} run={r} showClass={false} />
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}
