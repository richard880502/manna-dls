import type { ActivityPlan, PrepAskResult, PrepResources, PrepReviseResult } from "../../api/types";
import { OPTION_NAMES, changedBlocksLine, goalsText, stepText } from "../../lib/prep";
import { SectionTools } from "./SectionTools";

/** 規劃教學活動: the goals as a table, then each step as a card, as on 築學網. */
export function ActivityCard({ plan, interactionId }: { plan: ActivityPlan; interactionId: number | null }) {
  const cited = new Map(plan.curriculum.map((c) => [c.code, c.text]));
  return (
    <div className="prep-activity">
      {plan.title.trim() && <h3 className="prep-result-title">{plan.title}</h3>}
      {plan.goals.length > 0 && (
        <section className="prep-section" aria-label="學習目標">
          <table className="prep-goals">
            <thead>
              <tr>
                <th scope="col">目標</th>
                <th scope="col">對應課綱</th>
              </tr>
            </thead>
            <tbody>
              {plan.goals.map((g) => (
                <tr key={g.text}>
                  <td>{g.text}</td>
                  <td>
                    <span className="prep-codes">
                      {g.curriculum_codes.map((code) => (
                        <code key={code} className="prep-code" title={cited.get(code)}>
                          {code}
                        </code>
                      ))}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <SectionTools name="學習目標" text={goalsText(plan)} interactionId={interactionId} />
        </section>
      )}

      <ol className="prep-steps">
        {plan.steps.map((step, i) => (
          // oxlint-disable-next-line react/no-array-index-key -- a plan's steps have no id; order is the identity
          <li key={i} className="prep-step">
            <section aria-label={`${step.phase}：${step.title}`}>
              <p className="prep-step-head">
                <span className="soft-tag prep-phase">{step.phase}</span>
                <strong>{step.title}</strong>
                <span className="prep-minutes">{step.minutes} 分鐘</span>
              </p>
              {step.idea.trim() && (
                <div className="prep-idea">
                  <p className="prep-idea-label">教學思路</p>
                  <p className="prep-pre">{step.idea}</p>
                </div>
              )}
              {step.options.length > 0 && (
                <ul className="prep-options">
                  {step.options.map((o, j) => (
                    // oxlint-disable-next-line react/no-array-index-key -- options are numbered 選項一／二／三
                    <li key={j} className="prep-option">
                      <p className="prep-step-head">
                        <span className="soft-tag">{OPTION_NAMES[j] ?? `選項 ${j + 1}`}</span>
                        <strong>{o.title}</strong>
                        <span className="prep-minutes">{o.minutes} 分鐘</span>
                      </p>
                      <p className="prep-pre">{o.idea}</p>
                      {o.script.length > 0 && (
                        <details className="prep-more">
                          <summary>展開查看建議腳本</summary>
                          <ul className="prep-script" lang="en">
                            {o.script.map((line) => (
                              <li key={line}>{line}</li>
                            ))}
                          </ul>
                        </details>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {step.script.length > 0 && (
                <details className="prep-more">
                  <summary>展開查看建議腳本</summary>
                  <ul className="prep-script" lang="en">
                    {step.script.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                </details>
              )}
              {step.stuck_tip.trim() && (
                <details className="prep-more">
                  <summary>卡住了怎麼辦</summary>
                  <p className="prep-pre">{step.stuck_tip}</p>
                </details>
              )}
              <SectionTools name={`「${step.title}」`} text={stepText(step)} interactionId={interactionId} />
            </section>
          </li>
        ))}
      </ol>
    </div>
  );
}

/** 找教學資源: curriculum rows, the teacher's own materials, and the note. */
export function ResourcesCard({
  resources,
  onOpenMaterial,
}: {
  resources: PrepResources;
  onOpenMaterial: (id: number) => void;
}) {
  return (
    <div className="prep-resources">
      {resources.curriculum.length > 0 && (
        <section className="prep-section" aria-label="課綱條目">
          <h3 className="prep-result-title">課綱條目</h3>
          <ul className="prep-curriculum">
            {resources.curriculum.map((c) => (
              <li key={c.code}>
                <code className="prep-code">{c.code}</code>
                <span>{c.text}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {resources.materials.length > 0 && (
        <section className="prep-section" aria-label="你的教材">
          <h3 className="prep-result-title">你的教材</h3>
          <ul className="prep-material-links">
            {resources.materials.map((m) => (
              <li key={m.id}>
                <button type="button" className="text-link" onClick={() => onOpenMaterial(m.id)}>
                  {m.title.trim() || "未命名教材"}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {resources.note && <p className="quiet-note">{resources.note}</p>}
    </div>
  );
}

/** 直接問 AI: the answer, with the follow-up questions as chips. */
export function AskCard({
  result,
  interactionId,
  disabled,
  onFollowUp,
}: {
  result: PrepAskResult;
  interactionId: number | null;
  disabled: boolean;
  onFollowUp: (question: string) => void;
}) {
  return (
    <div className="prep-answer">
      <p className="prep-pre">{result.answer}</p>
      <SectionTools name="這個回答" text={result.answer} interactionId={interactionId} />
      {result.follow_ups.length > 0 && (
        <div className="prep-chips is-quiet" role="group" aria-label="接著問">
          {result.follow_ups.map((q) => (
            <button key={q} type="button" className="prep-chip" disabled={disabled} onClick={() => onFollowUp(q)}>
              {q}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** A revision by typed instruction: Manna's reply and which blocks changed in which version. */
export function ReviseCard({
  result,
  labels,
  interactionId,
}: {
  result: PrepReviseResult;
  labels: Record<string, string> | undefined;
  interactionId: number | null;
}) {
  return (
    <div className="prep-answer">
      <p className="prep-pre">{result.reply}</p>
      {result.changed_block_ids.length > 0 && (
        <p className="prep-changed" role="status">
          已修改：{changedBlocksLine(result.changed_block_ids, labels)}
          {result.version_no !== null ? `（第 ${result.version_no} 版）` : ""}
        </p>
      )}
      <SectionTools name="這次修改" text={result.reply} interactionId={interactionId} />
    </div>
  );
}
