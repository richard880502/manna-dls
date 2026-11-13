import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { SiopFeature } from "../../api/types";
import { logScaffold } from "../../lib/scaffold";
import { ErrorText } from "../common";

const INITIAL_VISIBLE = 5;

interface SiopPanelProps {
  lessonId: number;
  initialSelected: number[];
  /** Reports how many hints exist once loaded (for the 「SIOP 提示（n）」 count). Pass a stable function. */
  onLoaded?: (count: number) => void;
  /** Reports the selected ids. Pass a stable function. */
  onSelectedChange?: (ids: number[]) => void;
}

/**
 * SIOP 提示, inside the lesson page's 「更多備課工具」. The section starts collapsed, so loading
 * the hints logs `SC-SIOP show` with trigger `collapsed`; opening and closing the section is
 * logged by the lesson page (`expand` / `skip`, trigger `section`).
 */
export function SiopPanel({ lessonId, initialSelected, onLoaded, onSelectedChange }: SiopPanelProps) {
  const [features, setFeatures] = useState<SiopFeature[] | null>(null);
  const [selected, setSelected] = useState<number[]>(initialSelected);
  const [showAll, setShowAll] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shownLogged = useRef(false);

  useEffect(() => {
    let alive = true;
    api
      .listSiop()
      .then((list) => {
        if (!alive) return;
        setFeatures(list);
        onLoaded?.(list.length);
        if (list.length > 0 && !shownLogged.current) {
          shownLogged.current = true;
          logScaffold("SC-SIOP", "show", { trigger: "collapsed" });
        }
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [onLoaded]);

  useEffect(() => {
    onSelectedChange?.(selected);
  }, [selected, onSelectedChange]);

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

  const visible = features ? (showAll ? features : features.slice(0, INITIAL_VISIBLE)) : [];

  return (
    <div className="siop-panel">
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
      <ErrorText message={error} />
    </div>
  );
}
