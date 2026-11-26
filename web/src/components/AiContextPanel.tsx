import { useId, useState } from "react";
import type { AiContextItem } from "../api/types";
import { useApp } from "../context/AppContext";
import { hasHardest } from "../lib/preferences";
import { logScaffold, usePreferenceScaffold } from "../lib/scaffold";
import { Icon } from "./Icon";

/**
 * 「Manna 已帶入的資訊」: what the platform sent along with this AI call (AiResult.context).
 * Open by default for teachers whose P3 includes 跟 AI 溝通; collapsed (but openable) for everyone else.
 */
export function AiContextPanel({ items }: { items: AiContextItem[] | null | undefined }) {
  const { preferences } = useApp();
  const byPreference = hasHardest(preferences, "ai_communication");
  const [open, setOpen] = useState(byPreference);
  const regionId = useId();
  const list = Array.isArray(items) ? items : [];

  usePreferenceScaffold(byPreference && list.length > 0, "ai_communication");

  if (list.length === 0) return null;

  function toggle() {
    const next = !open;
    setOpen(next);
    if (next) logScaffold("SC-CONTEXT", "expand", { trigger: byPreference ? "reopen" : "manual" });
  }

  return (
    <div className={`ai-context${open ? " is-open" : ""}`}>
      <button
        type="button"
        className="ai-context-toggle"
        aria-expanded={open}
        aria-controls={regionId}
        onClick={toggle}
      >
        <Icon name={open ? "chevron" : "next"} className="is-glyph" /> Manna 已帶入的資訊
        <span className="ai-context-count">{list.length} 項</span>
      </button>
      {open && (
        <div id={regionId} className="ai-context-body" role="region" aria-label="Manna 已帶入的資訊">
          <p className="field-hint">這些資訊 Manna 已自動帶入，你不必自己寫指令。</p>
          <dl>
            {list.map((item, index) => (
              <div key={`${item.label}-${index}`} className="ai-context-row">
                <dt>{item.label}</dt>
                <dd>{item.preview || "（空白）"}</dd>
              </div>
            ))}
          </dl>
        </div>
      )}
    </div>
  );
}
