import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import {
  SOP_VOCABULARY_ITEM_INDEX,
  type ExitKind,
  type InfoFunction,
  type SopBlockSuggestion,
  type SopExitItem,
  type SopExitSuggestion,
  type SopGoalSuggestion,
  type SopGoalsSuggestion,
  type SopJob,
  type SopParagraph,
  type SopParagraphGoal,
  type SopTask,
  type SopTaskBlock,
  type SopTasksSuggestion,
  type SopVocabularyItem,
  type SopWarmupActivity,
  type SopWarmupSuggestion,
} from "../../api/types";
import {
  EXIT_KINDS,
  INFO_FUNCTIONS,
  emptyScaffolds,
  exitKindLabel,
  infoFunctionLabel,
  newId,
  objectiveLabels,
  paragraphRange,
} from "../../lib/sop";
import { ANSWER_FORMATS, STUDENT_TASKS, toggleIn } from "../../lib/worksheet";
import { ErrorText } from "../common";
import { AnswerFormatChips, StudentTaskChips } from "../worksheet/Chips";
import { GenerationChoice, SupportChoice, WorksheetChoices } from "./Choices";
import { AskManna, MannaWell, NoMaterialNote, Quote, RedoPending, ScaffoldFields, ScaffoldList } from "./parts";
import { redoFor, type SopController } from "./useSop";

interface StageProps {
  sop: SopController;
  onGoToStage: (step: 1 | 3) => void;
}

const blank = (s: string | null | undefined) => !s || !s.trim();
const union = (a: readonly string[] | undefined, b: readonly string[]) => [...(a ?? []), ...b.filter((x) => !(a ?? []).includes(x))];

// ---------------------------------------------------------------- step 4

type GoalOffer =
  | { kind: "pending"; job: SopJob }
  | { kind: "entry"; job: SopJob<SopGoalsSuggestion>; entry: SopGoalSuggestion };

/**
 * The suggestion a paragraph shows now: the newest of the whole run and of 這一段再建議一次, as long
 * as the teacher has not decided on it.
 */
function goalOffer(sop: SopController, paragraph: SopParagraph): GoalOffer | null {
  const whole = sop.jobs.goals;
  const redo = redoFor(sop.jobs, "goals", null, `p:${paragraph.id}`) as SopJob<SopGoalsSuggestion> | null;
  const source = redo && (!whole || redo.id > whole.id) ? redo : whole;
  if (!source) return null;
  if (source.status === "running" || source.result?.status !== "shown") {
    return source === redo ? { kind: "pending", job: source } : null;
  }
  const entry = source.result.output?.paragraphs.find((g) => g.paragraph_id === paragraph.id);
  if (!entry) return null;
  if (source.decided.includes(entry.paragraph_no)) return null;
  return { kind: "entry", job: source, entry };
}

/** Step 4, 逐段整合目標: which objectives each paragraph serves, and a note on how. */
export function StageIntegrate({ sop, onGoToStage }: StageProps) {
  const { material, content, update, decide } = sop;
  const paragraphs = material?.paragraphs ?? [];
  const labels = objectiveLabels(content.objectives);
  const objectives = content.objectives.filter((o) => o.text.trim());
  const goals = new Map(content.paragraph_goals.map((g) => [g.paragraph_id, g]));
  const whole = sop.jobs.goals;
  const wholeShown = whole?.status === "done" && whole.result?.status === "shown" ? whole.result.output : null;

  const setGoal = (paragraphId: string, patch: Partial<SopParagraphGoal>) =>
    update((c) => {
      const current = c.paragraph_goals.find((g) => g.paragraph_id === paragraphId) ?? {
        paragraph_id: paragraphId,
        objective_ids: [],
        note: "",
        origin: "teacher" as const,
      };
      const rest = c.paragraph_goals.filter((g) => g.paragraph_id !== paragraphId);
      // The teacher changing what Manna wrote makes it a shared piece of work.
      const origin = patch.origin ?? (current.origin === "ai_adopted" ? "ai_edited" : (current.origin ?? "teacher"));
      return { ...c, paragraph_goals: [...rest, { ...current, ...patch, origin }] };
    });

  /** What a paragraph's goal becomes when a suggestion is taken: what the teacher chose or wrote stays. */
  function take(p: SopParagraph, offer: Extract<GoalOffer, { kind: "entry" }>, edit: boolean) {
    const existing = goals.get(p.id);
    const had = !!existing && (existing.objective_ids.length > 0 || !blank(existing.note));
    setGoal(p.id, {
      objective_ids: union(existing?.objective_ids, offer.entry.objective_ids),
      note: blank(existing?.note) ? offer.entry.note : (existing?.note ?? ""),
      origin: !had && !edit ? "ai_adopted" : "ai_edited",
    });
    decide(
      "goals",
      offer.entry.paragraph_no,
      edit ? "revise" : "adopt",
      edit ? offer.entry.note : undefined,
      offer.job.id === whole?.id ? undefined : offer.job.id,
    );
    if (edit) setTimeout(() => document.getElementById(`goal-note-${p.id}`)?.focus(), 0);
  }

  function again(p: SopParagraph, offer: GoalOffer | null) {
    if (offer?.kind === "entry") {
      decide("goals", offer.entry.paragraph_no, "reject", undefined, offer.job.id === whole?.id ? undefined : offer.job.id);
    }
    void sop.startJob("goals", { paragraph_id: p.id, parent_job_id: whole?.id });
  }

  return (
    <>
      <p className="lead">一段一段讀：這一段可以帶到哪一項內容目標、哪一項語言目標？</p>
      <AskManna
        sop={sop}
        kind="goals"
        label="請 Manna 逐段建議"
        againLabel="請 Manna 再建議一次"
        shownNote={
          wholeShown
            ? `Manna 看了 ${wholeShown.paragraphs.length} 段，建議在各段下方。${
                wholeShown.missing_paragraph_ids.length > 0
                  ? `有 ${wholeShown.missing_paragraph_ids.length} 段這次沒有建議到，可以自己選或再請她試一次。`
                  : ""
              }`
            : null
        }
      />
      {objectives.length === 0 && (
        <p className="quiet-note">
          還沒有目標可以對應。
          <button type="button" className="text-link is-plain" onClick={() => onGoToStage(3)}>
            到第 3 步寫目標
          </button>
          ，或先只寫每一段的筆記。
        </p>
      )}
      {paragraphs.length === 0 && <NoMaterialNote onGoToMaterial={() => onGoToStage(1)} />}
      {paragraphs.map((p) => {
        const goal = goals.get(p.id);
        const chosen = goal?.objective_ids ?? [];
        const offer = goalOffer(sop, p);
        return (
          <section key={p.id} className="sop-para" aria-label={`第 ${p.no} 段`}>
            <Quote paragraphs={[p]} />
            {objectives.length > 0 && (
              <fieldset className="goal-choices">
                <legend className="field-label">這一段對應的目標</legend>
                {objectives.map((o) => (
                  <label key={o.id}>
                    <input
                      type="checkbox"
                      checked={chosen.includes(o.id)}
                      onChange={(e) =>
                        setGoal(p.id, {
                          objective_ids: e.target.checked ? [...chosen, o.id] : chosen.filter((id) => id !== o.id),
                        })
                      }
                    />
                    <span>
                      <span className="quiet-term">{labels.get(o.id)}</span>
                      {o.text}
                    </span>
                  </label>
                ))}
              </fieldset>
            )}
            <label className="field-label" htmlFor={`goal-note-${p.id}`}>
              這一段要怎麼帶
            </label>
            <input
              id={`goal-note-${p.id}`}
              type="text"
              value={goal?.note ?? ""}
              onChange={(e) => setGoal(p.id, { note: e.target.value })}
              placeholder="例如：先找出轉折語，再比較前後兩句"
            />
            {offer?.kind === "pending" && (
              <RedoPending label={`第 ${p.no} 段`} job={offer.job} onAgain={() => again(p, null)} />
            )}
            {offer?.kind === "entry" && (
              <MannaWell
                label={`第 ${p.no} 段`}
                onAdopt={() => take(p, offer, false)}
                onEdit={() => take(p, offer, true)}
                onDismiss={() =>
                  decide(
                    "goals",
                    offer.entry.paragraph_no,
                    "reject",
                    undefined,
                    offer.job.id === whole?.id ? undefined : offer.job.id,
                  )
                }
                onAgain={() => again(p, offer)}
                againLabel="這一段再建議一次"
              >
                {offer.entry.objective_ids.length > 0 ? (
                  <ul className="goal-offer">
                    {offer.entry.objective_ids.map((id) => (
                      <li key={id}>
                        <span className="quiet-term">{labels.get(id) ?? "目標"}</span>
                        {content.objectives.find((o) => o.id === id)?.text}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="why">Manna 認為這一段不特別對應目標。</p>
                )}
                <p>{offer.entry.note}</p>
                <p className="why">採用只會補上你還沒選的目標，不會改掉你已經寫的筆記。</p>
              </MannaWell>
            )}
            {!offer && wholeShown && (
              <p className="para-actions">
                <button
                  type="button"
                  className="text-link is-plain"
                  aria-label={`請 Manna 再建議第 ${p.no} 段`}
                  onClick={() => again(p, null)}
                >
                  這一段再建議一次
                </button>
              </p>
            )}
          </section>
        );
      })}
    </>
  );
}

// ---------------------------------------------------------------- step 5

const blankTask = (): SopTask => ({
  id: newId("t"),
  function: null,
  text: "",
  scaffolds: emptyScaffolds(),
  origin: "teacher",
  answer_format: "",
  options: [],
  answer: "",
  evidence: "",
});

const isEmptyTask = (t: SopTask) =>
  !t.text.trim() && !t.function && !t.scaffolds.verbal.trim() && !t.scaffolds.instructional.trim() && !t.scaffolds.procedural.trim();

/** The 學生任務 and 作答方式 of a block, the generator's own chips (spec §7: set at step 5). */
function BlockLabels({
  block,
  onChange,
}: {
  block: SopTaskBlock;
  onChange: (patch: Partial<SopTaskBlock>) => void;
}) {
  const tasks = block.student_tasks ?? [];
  const formats = block.answer_formats ?? [];
  const summary = [
    tasks.length > 0 ? `學生任務：${tasks.join("、")}` : "",
    formats.length > 0 ? `作答方式：${formats.join("、")}` : "",
  ]
    .filter(Boolean)
    .join("；");
  return (
    <details className="block-labels">
      <summary>學生任務與作答方式{summary ? `（${summary}）` : "（還沒選，Manna 做學習單時會自己判斷）"}</summary>
      <h4 className="sub-head">學生任務</h4>
      <StudentTaskChips
        selected={tasks}
        starred={block.ai_student_tasks ?? []}
        onToggle={(label) => onChange({ student_tasks: toggleIn(tasks, label, STUDENT_TASKS) })}
      />
      <h4 className="sub-head">作答方式</h4>
      <AnswerFormatChips
        selected={formats}
        starred={block.ai_answer_formats ?? []}
        onToggle={(label) => onChange({ answer_formats: toggleIn(formats, label, ANSWER_FORMATS) })}
      />
    </details>
  );
}

function TaskEditor({
  task,
  block,
  lessonId,
  onChange,
  onDone,
  onDelete,
}: {
  task: SopTask;
  block: SopTaskBlock;
  lessonId: number;
  onChange: (next: SopTask) => void;
  onDone: () => void;
  onDelete: () => void;
}) {
  const id = `task-${task.id}`;
  const [steps, setSteps] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [making, setMaking] = useState(false);

  /** 產生程序鷹架: the program's steps for this task (not a model). Her own steps are never replaced silently. */
  async function makeSteps() {
    setMaking(true);
    setError(null);
    try {
      const made = await api.sopProceduralSteps(lessonId, {
        paragraph_ids: block.paragraph_ids,
        student_tasks: block.student_tasks ?? [],
        answer_formats: [task.answer_format ?? "", ...(block.answer_formats ?? [])].filter(Boolean),
      });
      if (blank(task.scaffolds.procedural)) {
        onChange({ ...task, scaffolds: { ...task.scaffolds, procedural: made.text } });
      } else {
        setSteps(made.text);
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setMaking(false);
    }
  }

  return (
    <div className="stage-form" role="group" aria-label="問題或任務">
      <label className="field-label" htmlFor={`${id}-function`}>
        這一題要學生做什麼
      </label>
      <select
        id={`${id}-function`}
        value={task.function ?? ""}
        onChange={(e) => onChange({ ...task, function: (e.target.value || null) as InfoFunction | null })}
      >
        <option value="">還沒決定</option>
        {INFO_FUNCTIONS.map((f) => (
          <option key={f.value} value={f.value}>
            {f.label}
          </option>
        ))}
      </select>
      <label className="field-label" htmlFor={`${id}-text`}>
        問題或任務
      </label>
      <textarea
        id={`${id}-text`}
        className="print-input"
        rows={2}
        lang="en"
        value={task.text}
        onChange={(e) => onChange({ ...task, text: e.target.value })}
        placeholder="例如：Why did Mrs. Lin start the umbrella stand? Find the sentence that tells you."
      />
      <label className="field-label" htmlFor={`${id}-format`}>
        作答方式
      </label>
      <select
        id={`${id}-format`}
        value={task.answer_format ?? ""}
        onChange={(e) => onChange({ ...task, answer_format: e.target.value })}
      >
        <option value="">還沒決定</option>
        {ANSWER_FORMATS.map((f) => (
          <option key={f} value={f}>
            {f}
          </option>
        ))}
      </select>
      {(task.options ?? []).length > 0 && (
        <>
          <label className="field-label" htmlFor={`${id}-options`}>
            選項（一行一個）
          </label>
          <textarea
            id={`${id}-options`}
            rows={4}
            lang="en"
            value={(task.options ?? []).join("\n")}
            onChange={(e) => onChange({ ...task, options: e.target.value.split("\n") })}
          />
        </>
      )}
      <label className="field-label" htmlFor={`${id}-answer`}>
        參考答案（教師版）
      </label>
      <input
        id={`${id}-answer`}
        type="text"
        lang="en"
        value={task.answer ?? ""}
        onChange={(e) => onChange({ ...task, answer: e.target.value })}
      />
      <ScaffoldFields idPrefix={id} value={task.scaffolds} onChange={(scaffolds) => onChange({ ...task, scaffolds })} />
      <p className="para-actions">
        <button type="button" className="text-link is-plain" disabled={making} onClick={() => void makeSteps()}>
          {making ? "產生中…" : "產生程序鷹架"}
        </button>
        <span className="field-hint">分步指示由系統依這一題自動產生，可以再改。</span>
      </p>
      <ErrorText message={error} />
      {steps !== null && (
        <div className="manna-well english-proposal" role="group" aria-label="系統產生的程序鷹架">
          <p className="manna-label">
            <span className="star" aria-hidden="true" />
            系統產生的程序鷹架
          </p>
          <p className="pre-line" lang="en">
            {steps}
          </p>
          <div className="decide">
            <button
              type="button"
              className="decide-button is-take"
              onClick={() => {
                onChange({ ...task, scaffolds: { ...task.scaffolds, procedural: steps } });
                setSteps(null);
              }}
            >
              用這個
            </button>
            <button type="button" className="decide-button" onClick={() => setSteps(null)}>
              保留我的
            </button>
          </div>
        </div>
      )}
      <div className="button-row">
        <button type="button" className="secondary-button" onClick={onDone}>
          完成
        </button>
        <button type="button" className="text-link is-plain" onClick={onDelete}>
          刪除這一題
        </button>
      </div>
    </div>
  );
}

function TaskView({ task, onEdit }: { task: SopTask; onEdit: () => void }) {
  return (
    <div className="mine task-view">
      <div>
        {task.function && <span className="fn-tag">{infoFunctionLabel(task.function)}</span>}
        {task.answer_format && <span className="soft-tag">{task.answer_format}</span>}
        <p className="task-text" lang="en">
          {task.text || "（還沒寫題目）"}
        </p>
        {(task.options ?? []).length > 0 && (
          <ul className="task-options" lang="en">
            {(task.options ?? []).map((o) => (
              <li key={o}>{o}</li>
            ))}
          </ul>
        )}
        {!blank(task.answer) && (
          <p className="why" lang="en">
            <span className="quiet-term">參考答案</span>
            {task.answer}
          </p>
        )}
        <ScaffoldList value={task.scaffolds} />
      </div>
      <span className="tools">
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`修改「${task.text || "這一題"}」`}
          onClick={onEdit}
        >
          修改
        </button>
      </span>
    </div>
  );
}

/**
 * Step 5, 逐段問題或任務: per paragraph, or for neighbouring paragraphs merged into one block.
 * Each task has an information function (發現／整理／分析／比較／創造資訊) and the three scaffolds;
 * each block carries the 學生任務 and 作答方式 the worksheet will use (the generator's own chips).
 * Manna's suggestions (steps 5 of the SOP: tasks with scaffolds, the procedural one made by the
 * program) sit in gold under the paragraph, after the teacher's own.
 */
export function StageTasks({ sop, onGoToStage, lessonId }: StageProps & { lessonId: number }) {
  const { material, content, update, decide } = sop;
  const paragraphs = material?.paragraphs ?? [];
  const byId = new Map(paragraphs.map((p) => [p.id, p]));
  const labels = objectiveLabels(content.objectives);
  const [editingTask, setEditingTask] = useState<string | null>(null);

  const blockOf = new Map<string, SopTaskBlock>();
  for (const b of content.task_blocks) for (const pid of b.paragraph_ids) if (!blockOf.has(pid)) blockOf.set(pid, b);
  const general = content.task_blocks.find((b) => b.paragraph_ids.length === 0);
  const orphans = content.task_blocks.filter(
    (b) => b.paragraph_ids.length > 0 && b.paragraph_ids.every((pid) => !byId.has(pid)),
  );

  const job = sop.jobs.tasks;
  const suggestion: SopTasksSuggestion | null =
    job?.status === "done" && job.result?.status === "shown" ? (job.result.output ?? null) : null;
  const orderOf = (id: string) => paragraphs.findIndex((p) => p.id === id);
  /** The paragraph whose section shows a suggested block: the first of it (or of the teacher's block it joins). */
  const hostOf = (sb: SopBlockSuggestion): string | null => {
    const first = [...sb.paragraph_ids].sort((a, b) => orderOf(a) - orderOf(b))[0];
    if (!first) return null;
    const mine = blockOf.get(first);
    const members = mine
      ? mine.paragraph_ids.filter((id) => byId.has(id)).sort((a, b) => orderOf(a) - orderOf(b))
      : [first];
    return members[0] ?? first;
  };
  const offered = (suggestion?.blocks ?? [])
    .map((block, index) => ({ block, index }))
    .filter(({ index }) => !job?.decided.includes(index));

  const setBlocks = (change: (blocks: SopTaskBlock[]) => SopTaskBlock[]) =>
    update((c) => ({ ...c, task_blocks: change(c.task_blocks) }));
  const patchBlock = (blockId: string, patch: Partial<SopTaskBlock>) =>
    setBlocks((blocks) => blocks.map((b) => (b.id === blockId ? { ...b, ...patch } : b)));

  /** Adds an empty task (opened for writing) to the block of these paragraphs, creating it if needed. */
  function addTask(paragraphIds: string[], blockId?: string) {
    const task = blankTask();
    setBlocks((blocks) => {
      const existing = blockId ? blocks.find((b) => b.id === blockId) : undefined;
      if (existing) return blocks.map((b) => (b.id === existing.id ? { ...b, tasks: [...b.tasks, task] } : b));
      return [
        ...blocks,
        {
          id: newId("b"),
          paragraph_ids: paragraphIds,
          tasks: [task],
          student_tasks: [],
          answer_formats: [],
          ai_student_tasks: [],
          ai_answer_formats: [],
        },
      ];
    });
    setEditingTask(task.id);
  }

  const changeTask = (blockId: string, next: SopTask) =>
    setBlocks((blocks) =>
      blocks.map((b) => (b.id === blockId ? { ...b, tasks: b.tasks.map((t) => (t.id === next.id ? next : t)) } : b)),
    );

  /** Removes a task; a block left without tasks goes too (its paragraphs are free again). */
  const removeTask = (blockId: string, taskId: string) =>
    setBlocks((blocks) =>
      blocks
        .map((b) => (b.id === blockId ? { ...b, tasks: b.tasks.filter((t) => t.id !== taskId) } : b))
        .filter((b) => b.tasks.length > 0),
    );

  function finishTask(blockId: string, task: SopTask) {
    if (isEmptyTask(task)) removeTask(blockId, task.id);
    setEditingTask(null);
  }

  /** Puts paragraph `p` into the block of the paragraph before it (making that block if needed). */
  function mergeWithPrevious(p: SopParagraph, previous: SopParagraph) {
    const target = blockOf.get(previous.id);
    if (target) {
      setBlocks((blocks) =>
        blocks.map((b) => (b.id === target.id ? { ...b, paragraph_ids: [...b.paragraph_ids, p.id] } : b)),
      );
    } else {
      addTask([previous.id, p.id]);
    }
  }

  const splitOff = (block: SopTaskBlock, paragraphId: string) =>
    setBlocks((blocks) =>
      blocks.map((b) =>
        b.id === block.id ? { ...b, paragraph_ids: b.paragraph_ids.filter((pid) => pid !== paragraphId) } : b,
      ),
    );

  /**
   * Takes a suggested block. The teacher's own tasks stay: the tasks join the block she already has
   * for these paragraphs (her labels stay, Manna's are added with their star), or become a new one.
   */
  function take(sb: SopBlockSuggestion, index: number, edit: boolean) {
    const tasks: SopTask[] = sb.tasks.map((t) => ({
      id: newId("t"),
      function: t.function,
      text: t.text,
      scaffolds: t.scaffolds,
      origin: edit ? "ai_edited" : "ai_adopted",
      answer_format: t.answer_format,
      options: t.options,
      answer: t.answer,
      evidence: t.evidence,
    }));
    const first = sb.paragraph_ids[0];
    setBlocks((blocks) => {
      const target = first ? blocks.find((b) => b.paragraph_ids.includes(first)) : undefined;
      if (target) {
        return blocks.map((b) =>
          b.id === target.id
            ? {
                ...b,
                tasks: [...b.tasks, ...tasks],
                student_tasks: union(b.student_tasks, sb.student_tasks),
                answer_formats: union(b.answer_formats, sb.answer_formats),
                ai_student_tasks: union(b.ai_student_tasks, sb.student_tasks),
                ai_answer_formats: union(b.ai_answer_formats, sb.answer_formats),
              }
            : b,
        );
      }
      return [
        ...blocks,
        {
          id: newId("b"),
          paragraph_ids: sb.paragraph_ids,
          tasks,
          student_tasks: sb.student_tasks,
          answer_formats: sb.answer_formats,
          ai_student_tasks: sb.student_tasks,
          ai_answer_formats: sb.answer_formats,
        },
      ];
    });
    decide("tasks", index, edit ? "revise" : "adopt", edit ? tasks[0]?.text : undefined);
    if (edit && tasks[0]) setEditingTask(tasks[0].id);
  }

  function renderTasks(block: SopTaskBlock) {
    return block.tasks.map((task) =>
      editingTask === task.id ? (
        <TaskEditor
          key={task.id}
          task={task}
          block={block}
          lessonId={lessonId}
          onChange={(next) => changeTask(block.id, next)}
          onDone={() => finishTask(block.id, task)}
          onDelete={() => {
            removeTask(block.id, task.id);
            setEditingTask(null);
          }}
        />
      ) : (
        <TaskView key={task.id} task={task} onEdit={() => setEditingTask(task.id)} />
      ),
    );
  }

  function linkedObjectives(paragraphIds: string[]): string {
    const ids = new Set<string>();
    for (const g of content.paragraph_goals) {
      if (paragraphIds.includes(g.paragraph_id)) for (const id of g.objective_ids) ids.add(id);
    }
    return content.objectives
      .filter((o) => ids.has(o.id))
      .map((o) => labels.get(o.id))
      .join("、");
  }

  function renderSuggestions(paragraphId: string) {
    return offered
      .filter(({ block }) => hostOf(block) === paragraphId)
      .map(({ block: sb, index }) => {
        const members = sb.paragraph_ids
          .map((pid) => byId.get(pid))
          .filter((x): x is SopParagraph => !!x)
          .sort((a, b) => a.no - b.no);
        return (
          <MannaWell
            key={index}
            label={paragraphRange(members) || "問題或任務"}
            onAdopt={() => take(sb, index, false)}
            onEdit={() => take(sb, index, true)}
            onDismiss={() => decide("tasks", index, "reject")}
          >
            {sb.reason && <p className="why">為什麼這樣設計：{sb.reason}</p>}
            {sb.tasks.map((t) => (
              <div key={t.text} className="well-task">
                {t.function && <span className="fn-tag">{infoFunctionLabel(t.function)}</span>}
                {t.answer_format && <span className="soft-tag">{t.answer_format}</span>}
                <p className="task-text" lang="en">
                  {t.text}
                </p>
                {t.options.length > 0 && (
                  <ul className="task-options" lang="en">
                    {t.options.map((o) => (
                      <li key={o}>{o}</li>
                    ))}
                  </ul>
                )}
                {t.answer && (
                  <p className="why" lang="en">
                    <span className="quiet-term">參考答案</span>
                    {t.answer}
                  </p>
                )}
                {t.evidence && (
                  <p className="evidence" lang="en">
                    {t.evidence}
                  </p>
                )}
                <ScaffoldList value={t.scaffolds} />
              </div>
            ))}
            <p className="why">
              <span className="star" aria-hidden="true" />
              Manna 先選了學生任務：{sb.student_tasks.join("、") || "（沒有）"}；作答方式：
              {sb.answer_formats.join("、") || "（沒有）"}。採用後可以在這一段改。
            </p>
            {!suggestion?.scaffolds_ok && (
              <p className="why">這次鷹架沒有設計成功，只有系統加上的程序鷹架；可以自己補。</p>
            )}
            <p className="why">採用會把這些任務加在你寫的任務後面，不會改掉你寫的。</p>
          </MannaWell>
        );
      });
  }

  return (
    <>
      <p className="lead">一段一段看：要學生發現、整理、分析、比較，還是創造？可以幾段合在一起設計。</p>
      <AskManna
        sop={sop}
        kind="tasks"
        label="請 Manna 逐段設計"
        againLabel="請 Manna 再設計一次"
        shownNote={
          suggestion
            ? `Manna 設計了 ${suggestion.blocks.length} 個區塊，建議在各段下方；學生任務與作答方式已先選好（✦），可以改。${
                suggestion.missing_paragraph_ids.length > 0
                  ? `有 ${suggestion.missing_paragraph_ids.length} 段這次沒有設計到。`
                  : ""
              }`
            : null
        }
      />
      {paragraphs.length === 0 && <NoMaterialNote onGoToMaterial={() => onGoToStage(1)} />}
      {paragraphs.map((p, index) => {
        const block = blockOf.get(p.id);
        const previous = index > 0 ? paragraphs[index - 1] : undefined;
        if (block) {
          const members = block.paragraph_ids
            .map((pid) => byId.get(pid))
            .filter((x): x is SopParagraph => !!x)
            .sort((a, b) => a.no - b.no);
          // A merged block is shown once, at its first paragraph.
          if (members[0]?.id !== p.id) return null;
          const range = paragraphRange(members);
          const linked = linkedObjectives(block.paragraph_ids);
          const last = members[members.length - 1];
          return (
            <section key={block.id} className="sop-para" aria-label={range}>
              <Quote paragraphs={members} />
              <p className="para-links">
                {members.length > 1 && <span>{range}合併設計</span>}
                {linked && <span>對應：{linked}</span>}
              </p>
              {renderTasks(block)}
              <BlockLabels block={block} onChange={(patch) => patchBlock(block.id, patch)} />
              <p className="para-actions">
                <button
                  type="button"
                  className="text-link is-plain"
                  aria-label={`在${range}再加一個問題或任務`}
                  onClick={() => addTask(block.paragraph_ids, block.id)}
                >
                  ＋ 再加一個問題或任務
                </button>
                {members.length > 1 && last && (
                  <button type="button" className="text-link is-plain" onClick={() => splitOff(block, last.id)}>
                    把第 {last.no} 段拆出去
                  </button>
                )}
              </p>
              {renderSuggestions(p.id)}
            </section>
          );
        }
        return (
          <section key={p.id} className="sop-para" aria-label={`第 ${p.no} 段`}>
            <Quote paragraphs={[p]} />
            <p className="para-links">
              這一段還沒有問題或任務。
              <button
                type="button"
                className="text-link is-plain"
                aria-label={`自己寫第 ${p.no} 段的問題或任務`}
                onClick={() => addTask([p.id])}
              >
                自己寫
              </button>
              {previous && (
                <button type="button" className="text-link is-plain" onClick={() => mergeWithPrevious(p, previous)}>
                  和第 {previous.no} 段合併
                </button>
              )}
            </p>
            {renderSuggestions(p.id)}
          </section>
        );
      })}

      <section className="stage-block" aria-labelledby="tasks-general-heading">
        <h3 id="tasks-general-heading">
          不分段的問題或任務 <small>整篇讀完之後的討論或產出</small>
        </h3>
        {general && renderTasks(general)}
        {general && <BlockLabels block={general} onChange={(patch) => patchBlock(general.id, patch)} />}
        <p className="para-actions">
          <button type="button" className="text-link is-plain" onClick={() => addTask([], general?.id)}>
            ＋ 新增不分段的問題或任務
          </button>
        </p>
      </section>

      {orphans.length > 0 && (
        <section className="stage-block" aria-labelledby="tasks-orphans-heading">
          <h3 id="tasks-orphans-heading">原段落已變更</h3>
          <p className="quiet-note">教材改過之後，下面的問題或任務找不到原本的段落。內容還在，可以修改或刪除。</p>
          {orphans.map((b) => (
            <div key={b.id}>{renderTasks(b)}</div>
          ))}
        </section>
      )}

      <SupportChoice sop={sop} />
    </>
  );
}

// ---------------------------------------------------------------- step 6

/** Step 6, 暖身: waking up background knowledge, and the key vocabulary. Both use scaffolds. */
export function StageWarmup({ sop }: { sop: SopController }) {
  const { content, update, decide } = sop;
  const warmup = content.warmup;
  const job = sop.jobs.warmup;
  const suggestion: SopWarmupSuggestion | null =
    job?.status === "done" && job.result?.status === "shown" ? (job.result.output ?? null) : null;
  const undecided = (index: number) => suggestion !== null && !job?.decided.includes(index);
  // The field to focus once the edit just made has been drawn.
  const focusNext = useRef<string | null>(null);
  useEffect(() => {
    if (!focusNext.current) return;
    document.getElementById(focusNext.current)?.focus();
    focusNext.current = null;
  });

  const setBackground = (change: (list: SopWarmupActivity[]) => SopWarmupActivity[]) =>
    update((c) => ({ ...c, warmup: { ...c.warmup, background: change(c.warmup.background) } }));
  const setVocabulary = (change: (list: SopVocabularyItem[]) => SopVocabularyItem[]) =>
    update((c) => ({ ...c, warmup: { ...c.warmup, vocabulary: change(c.warmup.vocabulary) } }));

  function takeActivity(index: number, edit: boolean) {
    const s = suggestion?.background[index];
    if (!s) return;
    const id = newId("w");
    setBackground((list) => [
      ...list,
      { id, text: s.text, scaffolds: s.scaffolds, origin: edit ? "ai_edited" : "ai_adopted" },
    ]);
    decide("warmup", index, edit ? "revise" : "adopt", edit ? s.text : undefined);
    if (edit) focusNext.current = `warmup-${id}`;
  }

  function takeVocabulary(edit: boolean) {
    if (!suggestion) return;
    const have = new Set(warmup.vocabulary.map((v) => v.word.trim().toLowerCase()));
    const added: SopVocabularyItem[] = suggestion.vocabulary
      .filter((v) => !have.has(v.word.trim().toLowerCase()))
      .map((v) => ({
        id: newId("v"),
        word: v.word,
        note: v.definition_en,
        origin: edit ? "ai_edited" : "ai_adopted",
        level: v.level,
      }));
    setVocabulary((list) => [...list, ...added]);
    decide("warmup", SOP_VOCABULARY_ITEM_INDEX, edit ? "revise" : "adopt", edit ? added.map((v) => v.word).join("、") : undefined);
    if (edit && added[0]) focusNext.current = `vocab-${added[0].id}-note`;
  }

  const levelText = (level: number | null | undefined) =>
    level === null || level === undefined ? "詞彙表裡沒有這個字" : `大考中心詞彙表第 ${level} 級`;

  return (
    <>
      <p className="lead">
        主要的問題與任務設計好了，再回頭想怎麼開場。學習單上暖身會排在最前面。
      </p>
      <AskManna
        sop={sop}
        kind="warmup"
        label="請 Manna 設計暖身"
        againLabel="請 Manna 再設計一次"
        shownNote={
          suggestion
            ? suggestion.candidates_available === 0
              ? "教材裡找不到適合挑的關鍵字，所以沒有字彙建議。"
              : `關鍵字彙是程式先從教材挑出 ${suggestion.candidates_available} 個候選字，再由 Manna 選；${
                  suggestion.levels_available ? "級別是詞彙表上的。" : "詞彙表還沒有匯入，所以沒有級別。"
                }`
            : null
        }
      />

      <section className="stage-block" aria-labelledby="warmup-background-heading">
        <h3 id="warmup-background-heading">
          喚醒背景知識 <small>不必先讀完課文就能做的事</small>
        </h3>
        {warmup.background.map((activity, index) => (
          <div key={activity.id} className="stage-form" role="group" aria-label={`暖身活動 ${index + 1}`}>
            <label className="field-label" htmlFor={`warmup-${activity.id}`}>
              暖身活動 {index + 1}
            </label>
            <textarea
              id={`warmup-${activity.id}`}
              rows={2}
              value={activity.text}
              onChange={(e) =>
                setBackground((list) =>
                  list.map((a) =>
                    a.id === activity.id
                      ? { ...a, text: e.target.value, origin: a.origin === "ai_adopted" ? "ai_edited" : a.origin }
                      : a,
                  ),
                )
              }
              placeholder="例如：看標題和圖片，猜這篇文章在講什麼；或問學生借過傘的經驗"
            />
            <ScaffoldFields
              idPrefix={`warmup-${activity.id}`}
              value={activity.scaffolds}
              onChange={(scaffolds) =>
                setBackground((list) => list.map((a) => (a.id === activity.id ? { ...a, scaffolds } : a)))
              }
            />
            <button
              type="button"
              className="text-link is-plain"
              onClick={() => setBackground((list) => list.filter((a) => a.id !== activity.id))}
            >
              刪除這個活動
            </button>
          </div>
        ))}
        <p className="para-actions">
          <button
            type="button"
            className="text-link"
            onClick={() =>
              setBackground((list) => [
                ...list,
                { id: newId("w"), text: "", scaffolds: emptyScaffolds(), origin: "teacher" },
              ])
            }
          >
            ＋ 新增暖身活動
          </button>
        </p>
        {suggestion?.background.map((s, index) =>
          undecided(index) ? (
            // oxlint-disable-next-line react/no-array-index-key -- the decision is logged by this position
            <MannaWell
              key={index}
              label={`暖身活動 ${index + 1}`}
              onAdopt={() => takeActivity(index, false)}
              onEdit={() => takeActivity(index, true)}
              onDismiss={() => decide("warmup", index, "reject")}
            >
              <p className="pre-line" lang="en">
                {s.text}
              </p>
              {s.purpose && <p className="why">為什麼這樣設計：{s.purpose}</p>}
              <ScaffoldList value={s.scaffolds} />
            </MannaWell>
          ) : null,
        )}
      </section>

      <section className="stage-block" aria-labelledby="warmup-vocabulary-heading">
        <h3 id="warmup-vocabulary-heading">
          關鍵字彙 <small>讀之前先處理的字</small>
        </h3>
        {warmup.vocabulary.map((item, index) => (
          <div key={item.id} className="vocab-row" role="group" aria-label={`關鍵字彙 ${index + 1}`}>
            <div>
              <label className="field-label" htmlFor={`vocab-${item.id}-word`}>
                字彙 {index + 1}
              </label>
              <input
                id={`vocab-${item.id}-word`}
                type="text"
                lang="en"
                value={item.word}
                onChange={(e) =>
                  setVocabulary((list) => list.map((v) => (v.id === item.id ? { ...v, word: e.target.value } : v)))
                }
              />
              {item.level !== null && item.level !== undefined && (
                <span className="quiet-term vocab-level">第 {item.level} 級</span>
              )}
            </div>
            <div>
              <label className="field-label" htmlFor={`vocab-${item.id}-note`}>
                怎麼帶
              </label>
              <input
                id={`vocab-${item.id}-note`}
                type="text"
                value={item.note}
                onChange={(e) =>
                  setVocabulary((list) => list.map((v) => (v.id === item.id ? { ...v, note: e.target.value } : v)))
                }
                placeholder="例如：從課文的句子猜意思"
              />
            </div>
            <button
              type="button"
              className="text-link is-plain"
              aria-label={`刪除字彙 ${item.word || index + 1}`}
              onClick={() => setVocabulary((list) => list.filter((v) => v.id !== item.id))}
            >
              刪除
            </button>
          </div>
        ))}
        <p className="para-actions">
          <button
            type="button"
            className="text-link"
            onClick={() =>
              setVocabulary((list) => [...list, { id: newId("v"), word: "", note: "", origin: "teacher" }])
            }
          >
            ＋ 新增關鍵字彙
          </button>
        </p>
        {suggestion && suggestion.vocabulary.length > 0 && undecided(SOP_VOCABULARY_ITEM_INDEX) && (
          <MannaWell
            label="關鍵字彙"
            onAdopt={() => takeVocabulary(false)}
            onEdit={() => takeVocabulary(true)}
            onDismiss={() => decide("warmup", SOP_VOCABULARY_ITEM_INDEX, "reject")}
          >
            <ul className="feature-list">
              {suggestion.vocabulary.map((v) => (
                <li key={v.word}>
                  <strong lang="en">{v.word}</strong>
                  <span className="quiet-term">{levelText(v.level)}</span>
                  <span className="explanation" lang="en">
                    {v.definition_en}
                  </span>
                  {v.evidence && (
                    <span className="evidence" lang="en">
                      {v.evidence}
                    </span>
                  )}
                </li>
              ))}
            </ul>
            <p className="why">字是程式從教材挑的候選字，級別是詞彙表上的；採用只會加上你還沒有的字。</p>
          </MannaWell>
        )}
      </section>

      <GenerationChoice
        sop={sop}
        heading="學習單上要有暖身嗎？"
        labels={["加入暖身活動"]}
        hint="暖身會排在學習單最前面。"
      />
    </>
  );
}

// ---------------------------------------------------------------- step 7

/** Step 7, Exit ticket: a few short items for formative checking, reflection or collecting questions. */
export function StageExit({ sop }: { sop: SopController }) {
  const { content, update, decide } = sop;
  const items = content.exit_ticket.items;
  const labels = objectiveLabels(content.objectives);
  const objectives = content.objectives.filter((o) => o.text.trim());
  const job = sop.jobs.exit_ticket;
  const suggestion: SopExitSuggestion | null =
    job?.status === "done" && job.result?.status === "shown" ? (job.result.output ?? null) : null;
  // The field to focus once the edit just made has been drawn.
  const focusNext = useRef<string | null>(null);
  useEffect(() => {
    if (!focusNext.current) return;
    document.getElementById(focusNext.current)?.focus();
    focusNext.current = null;
  });

  const setItems = (change: (list: SopExitItem[]) => SopExitItem[]) =>
    update((c) => ({ ...c, exit_ticket: { items: change(c.exit_ticket.items) } }));
  const patchItem = (id: string, patch: Partial<SopExitItem>) =>
    setItems((list) =>
      list.map((i) =>
        i.id === id ? { ...i, ...patch, origin: i.origin === "ai_adopted" ? "ai_edited" : i.origin } : i,
      ),
    );

  function take(index: number, edit: boolean) {
    const s = suggestion?.items[index];
    if (!s) return;
    const id = newId("e");
    setItems((list) => [
      ...list,
      {
        id,
        kind: s.kind,
        text: s.text,
        origin: edit ? "ai_edited" : "ai_adopted",
        // A link to an objective the teacher has deleted since is not carried over.
        objective_id: objectives.some((o) => o.id === s.objective_id) ? s.objective_id : "",
        answer: s.answer,
      },
    ]);
    decide("exit_ticket", index, edit ? "revise" : "adopt", edit ? s.text : undefined);
    if (edit) focusNext.current = `exit-${id}-text`;
  }

  const uncovered = (suggestion?.uncovered_objective_ids ?? [])
    .map((id) => labels.get(id))
    .filter((l): l is string => !!l);

  return (
    <>
      <p className="lead">下課前兩三分鐘，想知道什麼？題數少一點。學習單上會放在最後，可以拿掉。</p>
      <AskManna
        sop={sop}
        kind="exit_ticket"
        label="請 Manna 設計 Exit ticket"
        againLabel="請 Manna 再設計一次"
        shownNote={
          suggestion
            ? `建議在下方，形成性評量的題目都連到你的目標，也附參考答案。${
                uncovered.length > 0 ? `這次沒有檢核到：${uncovered.join("、")}。` : ""
              }`
            : null
        }
      />
      <section className="stage-block" aria-label="Exit ticket 的題目">
        {items.length === 0 && <p className="quiet-note">還沒有題目。</p>}
        {items.map((item, index) => (
          <div key={item.id} className="stage-form" role="group" aria-label={`Exit ticket 第 ${index + 1} 題`}>
            <label className="field-label" htmlFor={`exit-${item.id}-kind`}>
              第 {index + 1} 題的用途
            </label>
            <select
              id={`exit-${item.id}-kind`}
              value={item.kind}
              onChange={(e) => patchItem(item.id, { kind: e.target.value as ExitKind })}
            >
              {EXIT_KINDS.map((k) => (
                <option key={k.value} value={k.value}>
                  {k.label}：{k.hint}
                </option>
              ))}
            </select>
            <label className="field-label" htmlFor={`exit-${item.id}-text`}>
              題目
            </label>
            <textarea
              id={`exit-${item.id}-text`}
              className="print-input"
              rows={2}
              lang="en"
              value={item.text}
              onChange={(e) => patchItem(item.id, { text: e.target.value })}
            />
            {item.kind === "formative" && (
              <>
                <label className="field-label" htmlFor={`exit-${item.id}-objective`}>
                  檢核哪一項目標
                </label>
                <select
                  id={`exit-${item.id}-objective`}
                  value={item.objective_id ?? ""}
                  onChange={(e) => patchItem(item.id, { objective_id: e.target.value })}
                >
                  <option value="">沒有指定</option>
                  {objectives.map((o) => (
                    <option key={o.id} value={o.id}>
                      {labels.get(o.id)}：{o.text}
                    </option>
                  ))}
                </select>
                <label className="field-label" htmlFor={`exit-${item.id}-answer`}>
                  參考答案（教師版）
                </label>
                <input
                  id={`exit-${item.id}-answer`}
                  type="text"
                  lang="en"
                  value={item.answer ?? ""}
                  onChange={(e) => patchItem(item.id, { answer: e.target.value })}
                />
              </>
            )}
            <button
              type="button"
              className="text-link is-plain"
              aria-label={`刪除第 ${index + 1} 題（${exitKindLabel(item.kind)}）`}
              onClick={() => setItems((list) => list.filter((i) => i.id !== item.id))}
            >
              刪除這一題
            </button>
          </div>
        ))}
        <p className="para-actions">
          {EXIT_KINDS.map((k) => (
            <button
              key={k.value}
              type="button"
              className="text-link is-plain"
              onClick={() =>
                setItems((list) => [
                  ...list,
                  { id: newId("e"), kind: k.value, text: "", origin: "teacher", objective_id: "", answer: "" },
                ])
              }
            >
              ＋ {k.label}
            </button>
          ))}
        </p>
        {suggestion?.items.map((s, index) =>
          !job?.decided.includes(index) ? (
            // oxlint-disable-next-line react/no-array-index-key -- the decision is logged by this position
            <MannaWell
              key={index}
              label={exitKindLabel(s.kind)}
              onAdopt={() => take(index, false)}
              onEdit={() => take(index, true)}
              onDismiss={() => decide("exit_ticket", index, "reject")}
            >
              <p lang="en">{s.text}</p>
              {s.objective_id && (
                <p className="why">
                  <span className="quiet-term">檢核</span>
                  {labels.get(s.objective_id)}：{content.objectives.find((o) => o.id === s.objective_id)?.text}
                </p>
              )}
              {s.answer && (
                <p className="why" lang="en">
                  <span className="quiet-term">參考答案</span>
                  {s.answer}
                </p>
              )}
            </MannaWell>
          ) : null,
        )}
      </section>

      <WorksheetChoices sop={sop} />
    </>
  );
}
