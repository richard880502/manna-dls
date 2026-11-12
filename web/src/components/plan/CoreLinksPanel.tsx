import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import { QUESTION_IDS, type CoreFocus, type CoreQuestion } from "../../api/types";
import { FALLBACK_QUESTIONS, MAX_FOCUSES } from "../../lib/core";
import { ErrorText } from "../common";

interface CoreLinksPanelProps {
  lessonId: number;
  unitId: number;
  initialLinked: number[];
  /** Activity-first route: after writing the plan, link focuses and go back to fill in CoRe. */
  backfillHint?: boolean;
}

export function CoreLinksPanel({ lessonId, unitId, initialLinked, backfillHint = false }: CoreLinksPanelProps) {
  const [focuses, setFocuses] = useState<CoreFocus[] | null>(null);
  const [questions, setQuestions] = useState<CoreQuestion[]>(FALLBACK_QUESTIONS);
  const [linked, setLinked] = useState<number[]>(initialLinked);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .getCore(unitId)
      .then((v) => alive && setFocuses([...v.focuses].sort((a, b) => a.position - b.position)))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    api
      .coreQuestions()
      .then((qs) => alive && qs.length > 0 && setQuestions(qs))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [unitId]);

  async function toggle(id: number, on: boolean) {
    if (on && linked.length >= MAX_FOCUSES) return;
    const previous = linked;
    const next = on ? [...linked, id] : linked.filter((x) => x !== id);
    setLinked(next);
    setError(null);
    try {
      const res = await api.setCoreLinks(lessonId, next);
      setLinked(res.focus_ids ?? next);
    } catch (err) {
      setLinked(previous);
      setError(errorMessage(err));
    }
  }

  const promptOf = (id: string) => questions.find((q) => q.id === id)?.prompt ?? id;
  const linkedFocuses = (focuses ?? []).filter((f) => linked.includes(f.id));

  return (
    <section className="panel-card core-links" aria-labelledby="core-links-heading">
      <div className="card-heading">
        <h3 id="core-links-heading">本課連結的 CoRe 學習重點</h3>
        <span className="optional-label">可略過</span>
      </div>
      <ErrorText message={error} />
      {backfillHint && (
        <p className="field-hint route-hint">
          寫完活動後，勾選 1–3 個相關的學習重點，再 <Link to={`/units/${unitId}/core`}>回頭補寫 CoRe →</Link>
        </p>
      )}
      {focuses && focuses.length === 0 && (
        <p className="empty-preview">
          這個單元的 CoRe 還沒有學習重點。可以略過，或先到 <Link to={`/units/${unitId}/core`}>單元 CoRe</Link> 補寫。
        </p>
      )}
      {focuses && focuses.length > 0 && (
        <div className="core-links-grid">
          <fieldset className="core-link-choices">
            <legend className="field-hint">選 0–3 個與本課相關的重點</legend>
            {focuses.map((f, i) => (
              <label key={f.id} className="siop-choice">
                <input
                  type="checkbox"
                  checked={linked.includes(f.id)}
                  disabled={!linked.includes(f.id) && linked.length >= MAX_FOCUSES}
                  onChange={(e) => void toggle(f.id, e.target.checked)}
                />
                <span>
                  學習重點 {i + 1}：{f.big_idea || "（未命名）"}
                </span>
              </label>
            ))}
          </fieldset>
          <div className="core-answers" aria-label="相關 CoRe 答案">
            {linkedFocuses.length === 0 && <p className="field-hint">勾選後，相關的 CoRe 答案會顯示在這裡供參考。</p>}
            {linkedFocuses.map((f) => (
              <details key={f.id} className="source-detail core-answer-block" open>
                <summary>{f.big_idea || "（未命名的學習重點）"}</summary>
                <dl>
                  {QUESTION_IDS.map((q) => {
                    const r = f.responses?.[q];
                    let text = "（留白）";
                    if (r?.status === "no_idea") text = "目前沒有想法";
                    else if (r?.answer?.trim()) text = r.answer;
                    return (
                      <div key={q} className="core-answer-row">
                        <dt>
                          {q} {promptOf(q)}
                        </dt>
                        <dd>{text}</dd>
                      </div>
                    );
                  })}
                </dl>
              </details>
            ))}
          </div>
        </div>
      )}
    </section>
  );
}
