import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { SiopFeature } from "../../api/types";
import { logScaffold } from "../../lib/scaffold";
import { ErrorText } from "../common";

const INITIAL_VISIBLE = 5;

interface SiopPanelProps {
  lessonId: number;
  initialSelected: number[];
}

export function SiopPanel({ lessonId, initialSelected }: SiopPanelProps) {
  const [features, setFeatures] = useState<SiopFeature[] | null>(null);
  const [selected, setSelected] = useState<number[]>(initialSelected);
  const [showAll, setShowAll] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shownLogged = useRef(false);

  useEffect(() => {
    let alive = true;
    api
      .listSiop()
      .then((list) => {
        if (!alive) return;
        setFeatures(list);
        if (list.length > 0 && !shownLogged.current) {
          shownLogged.current = true;
          logScaffold("SC-SIOP", "show");
        }
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  async function toggle(id: number, on: boolean) {
    const previous = selected;
    const next = on ? [...selected, id] : selected.filter((x) => x !== id);
    setSelected(next);
    setError(null);
    if (on) logScaffold("SC-SIOP", "use", { trigger: String(id) });
    try {
      const res = await api.setLessonSiop(lessonId, next);
      setSelected(res.siop_feature_ids ?? next);
    } catch (err) {
      setSelected(previous);
      setError(errorMessage(err));
    }
  }

  if (features && features.length === 0) return null;

  const visible = features ? (showAll ? features : features.slice(0, INITIAL_VISIBLE)) : [];

  return (
    <section className="panel-card siop-card" aria-labelledby="siop-heading">
      <div className="card-heading">
        <div>
          <p className="eyebrow warm">SIOP</p>
          <h3 id="siop-heading">SIOP 提示（選用）</h3>
        </div>
        {collapsed ? (
          <button
            type="button"
            className="ghost-button small-button"
            onClick={() => {
              setCollapsed(false);
              logScaffold("SC-SIOP", "expand", { trigger: "reopen" });
            }}
          >
            展開
          </button>
        ) : (
          <button
            type="button"
            className="ghost-button small-button"
            onClick={() => {
              setCollapsed(true);
              logScaffold("SC-SIOP", "skip");
            }}
          >
            略過
          </button>
        )}
      </div>
      {!collapsed && (
        <>
          <p className="field-hint">勾選你這節課打算用上的提示即可；不評分，也不需要全部勾完。</p>
          {!features && !error && <p className="loading-text">載入中…</p>}
          <div className="siop-choice-grid siop-list">
            {visible.map((f) => (
              <label key={f.id} className="siop-choice">
                <input
                  type="checkbox"
                  checked={selected.includes(f.id)}
                  onChange={(e) => void toggle(f.id, e.target.checked)}
                />
                <span>
                  <strong>{f.component}</strong>｜{f.summary}
                </span>
              </label>
            ))}
          </div>
          {features && features.length > INITIAL_VISIBLE && !showAll && (
            <button
              type="button"
              className="ghost-button small-button"
              onClick={() => {
                setShowAll(true);
                logScaffold("SC-SIOP", "expand", { trigger: "more" });
              }}
            >
              顯示更多提示
            </button>
          )}
        </>
      )}
      <ErrorText message={error} />
    </section>
  );
}
