import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api } from "../../api/client";
import { QUESTION_IDS, type CoreFocus, type QuestionId, type Unit } from "../../api/types";

/** The eight CoRe questions in a few words each, for the ledger beside the lessons. */
export const IDEA_LABELS: Record<QuestionId, string> = {
  Q1: "要學會什麼",
  Q2: "為什麼重要",
  Q3: "這次先不教什麼",
  Q4: "可能哪裡難教",
  Q5: "學生可能怎麼想",
  Q6: "還有什麼會影響教學",
  Q7: "準備怎麼教",
  Q8: "怎麼知道學生懂了",
};

/** How many questions the companion shows before 「看全部 8 題」. */
const SHOWN = 5;

function answerOf(focus: CoreFocus | null, q: QuestionId): string {
  const response = focus?.responses[q];
  return response && response.status === "filled" ? response.answer.trim() : "";
}

/**
 * 單元想法 (CoRe, docs/DESIGN_OVERHAUL.md): a light companion beside the lessons, never a gate.
 * It shows what the teacher has written so far for the first learning focus and offers to write a
 * little more; everything is edited on the 單元想法 page. A unit whose CoRe was never opened is
 * not fetched (reading it would create the first empty version), it just shows the empty ledger.
 */
export function UnitIdeas({ unit, hint }: { unit: Unit; hint?: string }) {
  const [focuses, setFocuses] = useState<CoreFocus[] | null>(null);
  const started = unit.core_status !== "none";
  const coreUrl = `/units/${unit.id}/core`;

  useEffect(() => {
    if (!started) return;
    let alive = true;
    api
      .getCore(unit.id)
      .then((core) => alive && setFocuses(core.focuses))
      // Without it the companion still links to the page where everything is.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [unit.id, started]);

  const focus = focuses?.[0] ?? null;
  const written = QUESTION_IDS.filter((q) => answerOf(focus, q)).length;
  const loading = started && focuses === null;

  return (
    <aside className="companion" aria-labelledby="unit-ideas-heading">
      <h2 id="unit-ideas-heading">單元想法</h2>
      <p className="progress">
        <span className="en">CoRe</span> ·{" "}
        {loading
          ? "載入中…"
          : written === 0
            ? "還沒寫。"
            : `${QUESTION_IDS.length} 題寫了 ${written} 題。`}
        想到再寫，不影響備課。
      </p>
      {focus?.big_idea && <p className="companion-focus">學習重點：{focus.big_idea}</p>}
      <ol className="ledger">
        {QUESTION_IDS.slice(0, SHOWN).map((q) => {
          const answer = answerOf(focus, q);
          return (
            <li key={q} className={answer ? "filled" : "empty"}>
              <span className="q">{q}</span>
              {answer ? (
                <span className="ledger-answer">
                  {IDEA_LABELS[q]}：{answer}
                </span>
              ) : (
                <span>
                  {IDEA_LABELS[q]}
                  <Link className="text-link is-plain" to={coreUrl} aria-label={`${q} ${IDEA_LABELS[q]}：寫一點`}>
                    寫一點
                  </Link>
                </span>
              )}
            </li>
          );
        })}
      </ol>
      <p className="companion-more">
        <Link to={coreUrl}>
          {started ? `看全部 ${QUESTION_IDS.length} 題` : `打開單元想法（${QUESTION_IDS.length} 題）`}
        </Link>
        {focuses && focuses.length > 1 ? `　共 ${focuses.length} 個學習重點` : ""}
      </p>
      {hint && <p className="quiet-note">{hint}</p>}
    </aside>
  );
}
