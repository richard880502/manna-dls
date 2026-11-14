import { useId } from "react";
import { Link } from "react-router";
import type { Unit } from "../../api/types";
import { useActiveClass } from "../../lib/classes";

/**
 * 目前班級: one quiet line on the unit, CoRe and lesson pages saying which class the teacher is
 * working in, switchable among the classes that use this unit. The unit design itself is shared:
 * switching only changes whose teaching records and class profile are in play.
 */
export function ClassContextBar({ unit }: { unit: Unit | null }) {
  const id = useId();
  const { classes, active, choose } = useActiveClass(unit);
  if (!unit) return null;

  if (classes.length === 0) {
    return (
      <p className="class-context is-empty">
        還沒有班級取用這個單元。<Link to="/library">到備課庫加入班級</Link>
      </p>
    );
  }

  return (
    <div className="class-context" role="group" aria-label="目前班級">
      {classes.length === 1 && active ? (
        <p className="class-context-line">
          目前班級：<Link to={`/classes/${active.id}`}>{active.name}</Link>
        </p>
      ) : (
        <>
          <label htmlFor={id}>目前班級</label>
          <select
            id={id}
            value={active ? String(active.id) : ""}
            onChange={(e) => choose(e.target.value ? Number(e.target.value) : null)}
          >
            {!active && <option value="">尚未選擇（{classes.length} 個班級在用）</option>}
            {classes.map((c) => (
              <option key={c.id} value={String(c.id)}>
                {c.name}
              </option>
            ))}
          </select>
          {active && (
            <Link className="class-context-link" to={`/classes/${active.id}`}>
              班級頁
            </Link>
          )}
        </>
      )}
      {active?.note && <span className="soft-tag class-context-note">這班的備註：{active.note}</span>}
    </div>
  );
}
