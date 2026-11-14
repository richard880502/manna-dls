import { useState } from "react";
import type { SopContent, SopMaterial } from "../../api/types";
import { STAGES, stageSummary, type StageKey } from "../../lib/sop";

interface RailProps {
  stage: StageKey;
  onStage: (stage: StageKey) => void;
  material: SopMaterial | null;
  content: SopContent;
}

/**
 * The progress rail of the lesson workspace (direction A): 本節概要, then the seven steps, each
 * with what it already holds. Any step can be opened at any time; a tick only says the step has
 * something in it, never that it is required. Under 760px it folds into one line.
 */
export function Rail({ stage, onStage, material, content }: RailProps) {
  const [open, setOpen] = useState(false);
  const go = (next: StageKey) => {
    setOpen(false);
    onStage(next);
  };

  return (
    <nav className={`rail${open ? " is-open" : ""}`} aria-label="備課步驟">
      <p className="rail-title">
        <span>{stage === "overview" ? "本節概要" : `第 ${stage} 步，共 7 步`}</span>
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
        <button
          type="button"
          className={`rail-step rail-overview${stage === "overview" ? " is-now" : ""}`}
          aria-current={stage === "overview" ? "step" : undefined}
          onClick={() => go("overview")}
        >
          <span className="dot" aria-hidden="true" />
          <span>
            本節概要
            <span className="state">今天要教什麼、開始上課</span>
          </span>
        </button>
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
                    {summary && !now ? "✓" : s.step}
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
        <p className="rail-aside">
          可以跳著做，也可以略過。
          <br />
          每一步都會自動儲存。
        </p>
      </div>
    </nav>
  );
}
