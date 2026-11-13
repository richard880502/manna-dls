import { Link } from "react-router";
import type { LessonRun } from "../../api/types";
import { formatDateTime } from "../../lib/labels";

/** Finished runs of this lesson, each a link to its 課後補寫. */
export function PastRuns({ runs }: { runs: LessonRun[] }) {
  return (
    <ul className="lesson-list run-list">
      {runs.map((r) => (
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
  );
}
