import type { ActivityPlan, ActivityStep, PrepAskResult, PrepResources, PrepReviseResult } from "../../api/types";
import { BLOOM_LABEL } from "../../lib/bloom";
import {
  LANGUAGE_CATEGORY_LABEL,
  OPTION_NAMES,
  PLAN_WORDS,
  changedBlocksLine,
  goalGroups,
  goalsText,
  hasPrepare,
  prepareText,
  reviewText,
  stepText,
} from "../../lib/prep";
import { BloomChips } from "../worksheet/Bloom";
import { PictureOutcomeList } from "./ImageAttach";
import { SectionTools } from "./SectionTools";

/** A step's 分組 and 語言技能 as tags, then 分工, 教師巡視 and 鷹架 (version 4; nothing before). */
function StepSiop({ step }: { step: ActivityStep }) {
  const roles = step.roles ?? [];
  const skills = step.skills ?? [];
  const rows: Array<[string, string]> = [
    [PLAN_WORDS.monitoring, step.monitoring?.trim() ?? ""],
    [PLAN_WORDS.scaffolding, step.scaffolding?.trim() ?? ""],
  ];
  const hasTags = Boolean(step.grouping) || skills.length > 0;
  if (!hasTags && roles.length === 0 && rows.every(([, v]) => !v)) return null;
  return (
    <>
      {hasTags && (
        <p className="prep-step-tags">
          {step.grouping && (
            <span className="soft-tag prep-grouping" title={PLAN_WORDS.grouping}>
              {step.grouping}
            </span>
          )}
          {skills.length > 0 && (
            <span className="prep-skills" aria-label={`${PLAN_WORDS.skills}：${skills.join("、")}`}>
              {skills.map((s) => (
                <span key={s} className="prep-skill" aria-hidden="true">
                  {s}
                </span>
              ))}
            </span>
          )}
        </p>
      )}
      <dl className="prep-step-meta">
        {roles.length > 0 && (
          <div>
            <dt>{PLAN_WORDS.roles}</dt>
            <dd>
              <ul className="prep-roles">
                {roles.map((r) => (
                  <li key={`${r.role}-${r.task}`}>
                    <strong>{r.role}</strong>：{r.task}
                  </li>
                ))}
              </ul>
            </dd>
          </div>
        )}
        {rows
          .filter(([, v]) => v)
          .map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
      </dl>
    </>
  );
}

/** 課前準備 (version 4): 關鍵字彙, 教材與教具, 高層次問題, 背景連結. */
function PlanPrepare({ plan, interactionId }: { plan: ActivityPlan; interactionId: number | null }) {
  const words = plan.key_vocabulary ?? [];
  const materials = plan.materials ?? [];
  const questions = plan.higher_order_questions ?? [];
  const background = plan.background?.trim() ?? "";
  return (
    <section className="prep-section prep-prepare" aria-label={PLAN_WORDS.prepare}>
      <h4 className="prep-subhead">{PLAN_WORDS.prepare}</h4>
      <dl className="prep-plan-fields">
        {background && (
          <div>
            <dt>{PLAN_WORDS.background}</dt>
            <dd>{background}</dd>
          </div>
        )}
        {words.length > 0 && (
          <div>
            <dt>{PLAN_WORDS.keyVocabulary}</dt>
            <dd>
              <ul className="prep-words">
                {words.map((w) => (
                  <li key={w.word}>
                    <span lang="en" className="prep-word">
                      {w.word}
                    </span>
                    <span className="prep-word-zh">{w.zh}</span>
                  </li>
                ))}
              </ul>
            </dd>
          </div>
        )}
        {materials.length > 0 && (
          <div>
            <dt>{PLAN_WORDS.materials}</dt>
            <dd>{materials.join("、")}</dd>
          </div>
        )}
        {questions.length > 0 && (
          <div>
            <dt>{PLAN_WORDS.questions}</dt>
            <dd>
              <ol className="prep-questions">
                {questions.map((q) => (
                  <li key={q.question}>
                    <span lang="en">{q.question}</span>
                    <span className="soft-tag prep-question-level">{BLOOM_LABEL[q.bloom]}</span>
                  </li>
                ))}
              </ol>
            </dd>
          </div>
        )}
      </dl>
      <SectionTools name={PLAN_WORDS.prepare} text={prepareText(plan)} interactionId={interactionId} />
    </section>
  );
}

/**
 * 規劃教學活動: the goals as a table, then each step as a card, as on 築學網. The goals come in
 * two labelled groups, 內容目標 and 語言目標, each goal in Chinese with its English under it; a
 * 語言目標 (version 4) shows its kind and how it serves its 內容目標 (「學生要用……，才能……」), and a
 * goal the teacher gave is marked. A plan made before the groups shows its goals as one list, as
 * before.
 *
 * Version 4 (SIOP A6) adds 課前準備 (背景連結, 關鍵字彙, 教材與教具, 高層次問題) above the steps,
 * each step's 分組, 語言技能, 分工, 教師巡視 and 鷹架, and 複習與評量 below them. A plan made
 * before shows none of these.
 */
export function ActivityCard({ plan, interactionId }: { plan: ActivityPlan; interactionId: number | null }) {
  const cited = new Map(plan.curriculum.map((c) => [c.code, c.text]));
  const groups = goalGroups(plan);
  const review = reviewText(plan);
  return (
    <div className="prep-activity">
      {plan.title.trim() && <h3 className="prep-result-title">{plan.title}</h3>}
      {groups.length > 0 && (
        <section className="prep-section" aria-label="學習目標">
          <table className="prep-goals">
            <thead>
              <tr>
                <th scope="col">目標</th>
                <th scope="col">對應課綱</th>
              </tr>
            </thead>
            {groups.map((group) => (
              <tbody key={group.kind ?? "goals"} aria-label={group.label ?? undefined}>
                {group.label && (
                  <tr className="prep-goal-group">
                    <th scope="rowgroup" colSpan={2}>
                      {group.label}
                      <span className="prep-goal-hint">{group.hint}</span>
                    </th>
                  </tr>
                )}
                {group.goals.map((g, i) => (
                  // oxlint-disable-next-line react/no-array-index-key -- goals have no id; order is the identity
                  <tr key={i}>
                    <td>
                      {(g.category || g.fromTeacher) && (
                        <span className="prep-goal-tags">
                          {g.category && <span className="soft-tag">{LANGUAGE_CATEGORY_LABEL[g.category]}</span>}
                          {g.fromTeacher && <span className="soft-tag prep-from-teacher">{PLAN_WORDS.fromTeacher}</span>}
                        </span>
                      )}
                      {g.zh.trim() && <span className="prep-goal-zh">{g.zh}</span>}
                      {g.en.trim() && (
                        <span className="prep-goal-en" lang="en">
                          {g.en}
                        </span>
                      )}
                      {g.relation.trim() && (
                        <span className="prep-goal-relation">
                          {g.supports !== null && (
                            <span className="prep-goal-supports">{PLAN_WORDS.supports(g.supports)}：</span>
                          )}
                          {g.relation}
                        </span>
                      )}
                    </td>
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
            ))}
          </table>
          <SectionTools name="學習目標" text={goalsText(plan)} interactionId={interactionId} />
        </section>
      )}

      {hasPrepare(plan) && <PlanPrepare plan={plan} interactionId={interactionId} />}

      <ol className="prep-steps">
        {plan.steps.map((step, i) => (
          // oxlint-disable-next-line react/no-array-index-key -- a plan's steps have no id; order is the identity
          <li key={i} className="prep-step">
            <section aria-label={`${step.phase}：${step.title}`}>
              <p className="prep-step-head">
                <span className="soft-tag prep-phase">{step.phase}</span>
                <strong>{step.title}</strong>
                <span className="prep-minutes">{step.minutes} 分鐘</span>
                <BloomChips levels={step.bloom} label={`「${step.title}」的 Bloom 層次`} />
              </p>
              {step.idea.trim() && (
                <div className="prep-idea">
                  <p className="prep-idea-label">教學思路</p>
                  <p className="prep-pre">{step.idea}</p>
                </div>
              )}
              <StepSiop step={step} />
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

      {review && (
        <section className="prep-section prep-review" aria-label={PLAN_WORDS.reviewAssessment}>
          <h4 className="prep-subhead">{PLAN_WORDS.reviewAssessment}</h4>
          <dl className="prep-plan-fields">
            {plan.review?.trim() && (
              <div>
                <dt>{PLAN_WORDS.review}</dt>
                <dd>{plan.review.trim()}</dd>
              </div>
            )}
            {plan.assessment?.trim() && (
              <div>
                <dt>{PLAN_WORDS.assessment}</dt>
                <dd>{plan.assessment.trim()}</dd>
              </div>
            )}
          </dl>
          <SectionTools name={PLAN_WORDS.reviewAssessment} text={review} interactionId={interactionId} />
        </section>
      )}
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
      <PictureOutcomeList outcomes={result.pictures} />
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
