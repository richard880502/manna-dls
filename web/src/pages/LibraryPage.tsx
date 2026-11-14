import { useEffect, useId, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateUnitRequest, PrepRoute, SchoolClass, Unit } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { RouteOptions } from "../components/unit/RouteOptions";
import { useApp } from "../context/AppContext";
import { usedByText } from "../lib/classes";
import { UNIT_IDEAS_STATUS } from "../lib/labels";
import { effectiveRoute } from "../lib/preferences";

/** 加到班級: which classes use one unit. Each tick is saved at once. */
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
    <fieldset className="row-more assign-classes">
      <legend>「{unit.title}」給哪些班級用？</legend>
      {classes.length === 0 ? (
        <p className="field-hint">
          還沒有班級。<Link to="/">到班級頁新增班級</Link>
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
      <p className="field-hint">取消勾選只是讓那個班級不再取用；單元與那班上過的紀錄都保留。</p>
      <ErrorText message={error} />
    </fieldset>
  );
}

/**
 * 備課庫: the teacher's own units as ruled rows (lessons, 單元想法, which classes use it). One
 * unit, reusable across classes. Coming here leaves the class context: a unit opened from the
 * library has no class in play unless only one class uses it.
 */
export function LibraryPage() {
  const { routeChoiceEnabled, preferences, setCurrentClassId } = useApp();
  const navigate = useNavigate();
  const ids = useId();
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [classes, setClasses] = useState<SchoolClass[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  /** null = teacher has not picked; the default comes from 備課偏好 (P2). */
  const [pickedRoute, setPickedRoute] = useState<PrepRoute | null>(null);
  /** "" = 先放在備課庫，不加入班級. */
  const [classId, setClassId] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  /** The unit whose 加到班級 choices are open. */
  const [assigning, setAssigning] = useState<number | null>(null);

  const defaultRoute = effectiveRoute(true, preferences?.answers?.start_from ?? null);
  const route = pickedRoute ?? defaultRoute;

  useEffect(() => {
    setCurrentClassId(null);
  }, [setCurrentClassId]);

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

  // An empty library opens straight on the form: there is nothing else to do here yet.
  const formOpen = adding || (units !== null && units.length === 0);

  return (
    <section aria-labelledby="library-heading">
      <div className="head-row page-head">
        <div>
          <h1 id="library-heading">備課庫</h1>
          <p className="lead">你準備好的單元。一份設計可以給好幾個班用，在哪一班改都是同一份。</p>
        </div>
        {!formOpen && (
          <button type="button" className="primary-button" onClick={() => setAdding(true)}>
            新增單元
          </button>
        )}
      </div>

      {formOpen && (
        <form className="inline-form-block create-unit-form" onSubmit={onCreate} aria-labelledby={`${ids}-new`}>
          <h2 id={`${ids}-new`}>新增單元</h2>
          <label className="field-label" htmlFor="new-unit-title">
            單元名稱
          </label>
          <input
            id="new-unit-title"
            placeholder="例如：龍騰 B1 L3 Not only… but also 倒裝句"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            disabled={creating}
          />
          {classes.length > 0 && (
            <div className="create-unit-class">
              <label className="field-label" htmlFor={`${ids}-class`}>
                給哪個班級用？
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
          <div className="button-row">
            <button className="primary-button" type="submit" disabled={creating}>
              {creating ? "建立中…" : "建立單元"}
            </button>
            {adding && (
              <button type="button" className="text-link is-plain" disabled={creating} onClick={() => setAdding(false)}>
                取消
              </button>
            )}
          </div>
        </form>
      )}

      {loadError && <ErrorText message={loadError} />}
      {!units && !loadError && <Loading />}
      {units && units.length === 0 && <p className="empty-line">備課庫還沒有單元。建立第一個單元就能開始備課。</p>}
      {units && units.length > 0 && (
        <ul className="rows is-first" aria-label="單元">
          {units.map((unit) => {
            const used = unit.classes ?? [];
            return (
              <li key={unit.id} className="row">
                <div>
                  <h3>
                    <Link to={`/units/${unit.id}`}>{unit.title}</Link>
                  </h3>
                  <p className="meta">
                    {unit.lesson_count === 0 ? "還沒有課例" : `${unit.lesson_count} 節課例`} ·{" "}
                    {UNIT_IDEAS_STATUS[unit.core_status]}
                  </p>
                </div>
                <p className="side">
                  <span>{usedByText(used)}</span>
                  <button
                    type="button"
                    className="text-link"
                    aria-expanded={assigning === unit.id}
                    aria-label={`把「${unit.title}」加到班級`}
                    onClick={() => setAssigning((cur) => (cur === unit.id ? null : unit.id))}
                  >
                    加到班級
                  </button>
                </p>
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
