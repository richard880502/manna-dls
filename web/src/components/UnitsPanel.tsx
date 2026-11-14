import { useEffect, useId, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateUnitRequest, PrepRoute, SchoolClass, Unit } from "../api/types";
import { useApp } from "../context/AppContext";
import { CORE_STATUS_LABELS, formatDate } from "../lib/labels";
import { PREP_ROUTE_LABELS, effectiveRoute } from "../lib/preferences";
import { ErrorText, Loading } from "./common";
import { RouteOptions } from "./unit/RouteOptions";

/** 加入班級: which classes use one unit design. Each tick is saved at once. */
function AssignClasses({
  unit,
  classes,
  onChanged,
}: {
  unit: Unit;
  classes: SchoolClass[];
  onChanged: (unitId: number, classId: number, assigned: boolean) => void;
}) {
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const used = new Set(unit.classes.map((c) => c.id));

  async function toggle(c: SchoolClass, assign: boolean) {
    setBusy(c.id);
    setError(null);
    try {
      if (assign) await api.assignUnit(c.id, unit.id);
      else await api.unassignUnit(c.id, unit.id);
      onChanged(unit.id, c.id, assign);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <fieldset className="assign-classes">
      <legend>「{unit.title}」給哪些班級用？</legend>
      {classes.length === 0 ? (
        <p className="field-hint">
          還沒有班級。<Link to="/">到工作檯新增班級</Link>
        </p>
      ) : (
        <div className="assign-classes-list">
          {classes.map((c) => (
            <label key={c.id} className="check-row">
              <input
                type="checkbox"
                checked={used.has(c.id)}
                disabled={busy !== null}
                onChange={(e) => void toggle(c, e.target.checked)}
              />
              {c.name}
              {busy === c.id ? "（儲存中…）" : ""}
            </label>
          ))}
        </div>
      )}
      <p className="field-hint">取消勾選只是讓那個班級不再取用；單元設計與那班上過的紀錄都保留。</p>
      <ErrorText message={error} />
    </fieldset>
  );
}

/**
 * The design library's list: every unit design with the classes that use it, 建立新單元
 * (optionally straight into a class) and 加入班級.
 */
export function UnitsPanel() {
  const { routeChoiceEnabled, preferences } = useApp();
  const navigate = useNavigate();
  const ids = useId();
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [classes, setClasses] = useState<SchoolClass[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  /** null = teacher has not picked; the default comes from 我的備課偏好 (P2). */
  const [pickedRoute, setPickedRoute] = useState<PrepRoute | null>(null);
  /** "" = 先放在備課庫，不加入班級. */
  const [classId, setClassId] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  /** The unit whose 加入班級 panel is open. */
  const [assigning, setAssigning] = useState<number | null>(null);

  const defaultRoute = effectiveRoute(true, preferences?.answers?.start_from ?? null);
  const route = pickedRoute ?? defaultRoute;

  useEffect(() => {
    let alive = true;
    api
      .listUnits()
      .then((list) => {
        if (alive) setUnits(list);
      })
      .catch((err: unknown) => {
        if (alive) setLoadError(errorMessage(err));
      });
    api
      .listClasses()
      .then((list) => alive && setClasses(list))
      // Without the classes the library still lists and creates units.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  async function onCreate(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) {
      setCreateError("請輸入單元名稱。");
      return;
    }
    const body: CreateUnitRequest = { title: trimmed };
    if (routeChoiceEnabled) body.prep_route = route;
    if (classId) body.class_id = Number(classId);
    setCreating(true);
    setCreateError(null);
    try {
      const unit = await api.createUnit(body);
      setUnits((prev) => [unit, ...(prev ?? [])]);
      setTitle("");
      setPickedRoute(null);
      // The unit page opens on the first screen of the chosen prep route.
      navigate(`/units/${unit.id}`);
    } catch (err) {
      setCreateError(errorMessage(err));
    } finally {
      setCreating(false);
    }
  }

  function onAssignChanged(unitId: number, changedClassId: number, assigned: boolean) {
    const c = classes.find((x) => x.id === changedClassId);
    setUnits((prev) =>
      (prev ?? []).map((u) => {
        if (u.id !== unitId) return u;
        const others = (u.classes ?? []).filter((x) => x.id !== changedClassId);
        if (!assigned || !c) return { ...u, classes: others };
        const added = { id: c.id, name: c.name, course_label: c.course_label, class_profile: c.class_profile, note: "" };
        // Keep the teacher's class order.
        const order = classes.map((x) => x.id);
        return { ...u, classes: [...others, added].sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id)) };
      }),
    );
  }

  return (
    <section className="units-panel" aria-labelledby="units-heading">
      <div className="section-heading">
        <h3 id="units-heading">我的單元設計</h3>
        {units && <span className="status-pill">{units.length} 個單元</span>}
      </div>

      <form className="panel-card create-unit-form" onSubmit={onCreate}>
        <label className="field-label" htmlFor="new-unit-title">
          建立新單元
        </label>
        <div className="inline-form">
          <input
            id="new-unit-title"
            placeholder="例如：龍騰 B1 L3 Not only… but also 倒裝句"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            disabled={creating}
          />
          <button className="primary-button" type="submit" disabled={creating}>
            {creating ? "建立中…" : "建立單元"}
          </button>
        </div>
        {classes.length > 0 && (
          <div className="create-unit-class">
            <label className="field-label" htmlFor={`${ids}-class`}>
              建立後給哪個班級用？（選填）
            </label>
            <select id={`${ids}-class`} value={classId} onChange={(e) => setClassId(e.target.value)} disabled={creating}>
              <option value="">先放在備課庫</option>
              {classes.map((c) => (
                <option key={c.id} value={String(c.id)}>
                  {c.name}
                </option>
              ))}
            </select>
          </div>
        )}
        {routeChoiceEnabled && (
          <RouteOptions
            name="new-unit-route"
            legend="從哪裡開始備課？"
            value={route}
            onChange={setPickedRoute}
            disabled={creating}
            hint={
              preferences?.answers?.start_from
                ? "預設依你的備課偏好；建立後也可以在單元頁隨時切換。"
                : "建立後也可以在單元頁隨時切換。"
            }
          />
        )}
        <ErrorText message={createError} />
      </form>

      {loadError && <ErrorText message={loadError} />}
      {!units && !loadError && <Loading />}
      {units && units.length === 0 && (
        <p className="empty-preview">備課庫還沒有單元。先建立一個單元，再寫單元前 CoRe。</p>
      )}
      {units && units.length > 0 && (
        <ul className="unit-list">
          {units.map((unit) => {
            const used = unit.classes ?? [];
            return (
              <li key={unit.id} className="unit-card panel-card">
                <div className="unit-card-main">
                  <strong>{unit.title}</strong>
                  <span className="unit-meta">建立於 {formatDate(unit.created_at)}</span>
                  <p className="unit-classes">
                    {used.length === 0 ? (
                      <span className="unit-meta">還沒有班級使用</span>
                    ) : (
                      <>
                        <span className="unit-classes-label">使用中的班級</span>
                        {used.map((c) => (
                          <Link key={c.id} className="soft-tag class-tag" to={`/classes/${c.id}`}>
                            {c.name}
                          </Link>
                        ))}
                      </>
                    )}
                  </p>
                </div>
                <div className="unit-card-tags">
                  <span className={`core-status core-status-${unit.core_status}`}>
                    CoRe：{CORE_STATUS_LABELS[unit.core_status]}
                  </span>
                  <span className="soft-tag">{unit.lesson_count} 個課例</span>
                  {routeChoiceEnabled && (
                    <span className="soft-tag route-tag">{PREP_ROUTE_LABELS[effectiveRoute(true, unit.prep_route)]}</span>
                  )}
                </div>
                <div className="unit-card-actions">
                  <button
                    type="button"
                    className="ghost-button"
                    aria-expanded={assigning === unit.id}
                    aria-label={`把「${unit.title}」加入班級`}
                    onClick={() => setAssigning((cur) => (cur === unit.id ? null : unit.id))}
                  >
                    加入班級
                  </button>
                  <Link className="secondary-button link-button" to={`/units/${unit.id}/core`}>
                    單元 CoRe
                  </Link>
                  <Link className="primary-button link-button" to={`/units/${unit.id}`}>
                    課例備課
                  </Link>
                </div>
                {assigning === unit.id && (
                  <AssignClasses unit={{ ...unit, classes: used }} classes={classes} onChanged={onAssignChanged} />
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
