import { useState } from "react";
import type { SopContent, SopMaterial } from "../../api/types";
import { STAGES, stageSummary, type StageKey } from "../../lib/sop";
import { Icon } from "../Icon";

interface RailProps {
  stage: StageKey;
  onStage: (stage: StageKey) => void;
  material: SopMaterial | null;
  content: SopContent;
  /** Where the seven steps lead: 產生學習單. Omitted, the rail ends with step 7. */
  onWorksheet?: () => void;
  worksheetBusy?: boolean;
}

/**
 * The progress rail of the lesson workspace (direction A): the seven steps, each with what it
 * already holds, then where they lead, 產生學習單. Any step can be opened at any time; a tick only
 * says the step has something in it, never that it is required. Under 760px it folds into one line.
 */
export function Rail({ stage, onStage, material, content, onWorksheet, worksheetBusy = false }: RailProps) {
  const [open, setOpen] = useState(false);
  const go = (next: StageKey) => {
    setOpen(false);
    onStage(next);
  };

  return (
    <nav className={`rail${open ? " is-open" : ""}`} aria-label="備課步驟">
      <p className="rail-title">
        <span>第 {stage} 步，共 7 步</span>
        <button
          type="button"
          className="text-link is-plain rail-toggle"
          aria-expanded={open}
          aria-controls="rail-steps"
          onClick={() => setOpen(!open)}
        >
          {open ? "收起步驟" : "看全部步驟"}
        </button>
      </p>
      <div id="rail-steps" className="rail-steps">
        <ol>
          {STAGES.map((s) => {
            const summary = stageSummary(s.step, material, content);
            const now = stage === s.step;
            return (
              <li key={s.step}>
                <button
                  type="button"
                  className={`rail-step${now ? " is-now" : ""}${summary ? " is-done" : ""}`}
                  aria-current={now ? "step" : undefined}
                  onClick={() => go(s.step)}
                >
                  <span className="dot" aria-hidden="true">
                    {summary && !now ? <Icon name="check" className="is-glyph" /> : s.step}
                  </span>
                  <span>
                    <span className="sr-only">第 {s.step} 步：</span>
                    {s.name}
                    {(summary || now) && <span className="state">{summary ?? "現在"}</span>}
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
        {onWorksheet && (
          <button
            type="button"
            className="rail-step rail-end"
            disabled={worksheetBusy}
            onClick={() => {
              setOpen(false);
              onWorksheet();
            }}
          >
            <span className="dot" aria-hidden="true" />
            <span>
              {worksheetBusy ? "建立中…" : "產生學習單"}
              <span className="state">用這節課備好的內容</span>
            </span>
          </button>
        )}
        {/* Said once on the page: the steps themselves do not repeat it. Saving is said in the head. */}
        <p className="rail-aside">可以跳著做，也可以略過。</p>
      </div>
    </nav>
  );
}
