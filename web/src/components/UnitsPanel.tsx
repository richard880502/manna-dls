import { useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Unit } from "../api/types";
import { CORE_STATUS_LABELS, formatDate } from "../lib/labels";
import { ErrorText, Loading } from "./common";

export function UnitsPanel() {
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

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
      const unit = await api.createUnit(trimmed);
      setUnits((prev) => [unit, ...(prev ?? [])]);
      setTitle("");
    } catch (err) {
      setCreateError(errorMessage(err));
    } finally {
      setCreating(false);
    }
  }

  return (
    <section className="units-panel" aria-labelledby="units-heading">
      <div className="section-heading">
        <div>
          <p className="eyebrow">My units</p>
          <h3 id="units-heading">我的單元</h3>
        </div>
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
