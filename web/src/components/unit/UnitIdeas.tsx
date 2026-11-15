import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api } from "../../api/client";
import { QUESTION_IDS, type CoreFocus, type QuestionId, type Unit } from "../../api/types";

/** The eight CoRe questions in a few words each, for the ledger on the unit page. */
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

function answerOf(focus: CoreFocus | null, q: QuestionId): string {
  const response = focus?.responses[q];
  return response && response.status === "filled" ? response.answer.trim() : "";
}

/**
 * 單元想法 (CoRe) on the unit page: one quiet summary line, 「單元想法：寫了 2/8 題」, that opens
 * on click to the eight questions of the first learning focus. It never gates anything; the
 * lessons above it are the page's main content and everything is edited on the 單元想法 page.
 * A unit whose CoRe was never opened is not fetched (reading it would create the first empty
 * version): it just says nothing is written yet.
 */
export function UnitIdeas({ unit }: { unit: Unit }) {
  const [focuses, setFocuses] = useState<CoreFocus[] | null>(null);
  const started = unit.core_status !== "none";
  const coreUrl = `/units/${unit.id}/core`;
  const total = QUESTION_IDS.length;

  useEffect(() => {
    if (!started) return;
    let alive = true;
    api
      .getCore(unit.id)
      .then((core) => alive && setFocuses(core.focuses))
      // Without it the line still links to the page where everything is.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [unit.id, started]);

  const focus = focuses?.[0] ?? null;
  const written = QUESTION_IDS.filter((q) => answerOf(focus, q)).length;
  const loading = started && focuses === null;

  return (
    <details className="unit-ideas section">
      <summary>
        <span className="unit-ideas-title">單元想法</span>
        <span className="unit-ideas-count">
          ：{loading ? "載入中…" : written === 0 ? "還沒寫" : `寫了 ${written}/${total} 題`}
        </span>
        <small className="en">CoRe</small>
      </summary>
      <p className="quiet-note">想到再寫，不影響備課。</p>
      {focus?.big_idea && <p className="companion-focus">學習重點：{focus.big_idea}</p>}
      <ol className="ledger">
        {QUESTION_IDS.map((q) => {
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
        <Link to={coreUrl}>{started ? "打開單元想法" : `開始寫單元想法（${total} 題）`}</Link>
        {focuses && focuses.length > 1 ? `　共 ${focuses.length} 個學習重點` : ""}
      </p>
    </details>
  );
}
