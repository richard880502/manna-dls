import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { PrepRoute, Unit } from "../api/types";
import { useApp } from "../context/AppContext";
import { CORE_STATUS_LABELS, formatDate } from "../lib/labels";
import { PREP_ROUTE_LABELS, effectiveRoute } from "../lib/preferences";
import { ErrorText, Loading } from "./common";
import { RouteOptions } from "./unit/RouteOptions";

export function UnitsPanel() {
  const { routeChoiceEnabled, preferences } = useApp();
  const navigate = useNavigate();
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  /** null = teacher has not picked; the default comes from 我的備課偏好 (P2). */
  const [pickedRoute, setPickedRoute] = useState<PrepRoute | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

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
    setCreating(true);
    setCreateError(null);
    try {
      const unit = await api.createUnit(routeChoiceEnabled ? { title: trimmed, prep_route: route } : { title: trimmed });
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

  return (
    <section className="units-panel" aria-labelledby="units-heading">
      <div className="section-heading">
        <h3 id="units-heading">我的單元</h3>
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
        <p className="empty-preview">還沒有單元。先建立一個單元，再寫單元前 CoRe。</p>
      )}
      {units && units.length > 0 && (
        <ul className="unit-list">
          {units.map((unit) => (
            <li key={unit.id} className="unit-card panel-card">
              <div className="unit-card-main">
                <strong>{unit.title}</strong>
                <span className="unit-meta">建立於 {formatDate(unit.created_at)}</span>
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
                <Link className="secondary-button link-button" to={`/units/${unit.id}/core`}>
                  單元 CoRe
                </Link>
                <Link className="primary-button link-button" to={`/units/${unit.id}`}>
                  課例備課
                </Link>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
