import { useState } from "react";
import { api,errorMessage } from "../../api/client";
import {
type InfoFunction,
type SopBlockSuggestion,
type SopParagraph,
type SopTask,
type SopTaskBlock,
type SopTasksSuggestion
} from "../../api/types";
import {
INFO_FUNCTIONS,
emptyScaffolds,
infoFunctionLabel,
newId,
objectiveLabels,
paragraphRange
} from "../../lib/sop";
import { ANSWER_FORMATS,STUDENT_TASKS,toggleIn } from "../../lib/worksheet";
import { ErrorText } from "../common";
import { BloomSelect } from "../worksheet/Bloom";
import { AnswerFormatChips,StudentTaskChips } from "../worksheet/Chips";
import { SupportChoice } from "./Choices";
import { EnglishFix } from "./EnglishFix";
import { AskManna,MannaWell,NoMaterialNote,Quote,RedoPending,ScaffoldFields,ScaffoldList } from "./parts";
import { followRedo,retryRedo } from "./useSop";

import { blank,union,type StageProps } from "./manualShared";

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
      <EnglishFix
        lessonId={lessonId}
        field="task"
        text={task.text}
        paragraphIds={block.paragraph_ids}
        onUse={(english) => onChange({ ...task, text: english, origin: "ai_edited" })}
      />
      <BloomSelect label="Bloom 層次" value={task.bloom} disabled={false} onChange={(bloom) => onChange({ ...task, bloom })} />
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
  /** Where each suggested block stands now (a 換一個 may have replaced it), at the paragraph it belongs to. */
  const offered = (job && suggestion ? suggestion.blocks.map((block, home) => ({ block, home, ...followRedo(sop.jobs, "tasks", job, home) })) : [])
    .filter((slot) => slot.pending || !slot.job.decided.includes(slot.index));
  type Offered = (typeof offered)[number];
  /** The job of a decision: the whole run's, or the redo job the suggestion came from. */
  const jobOf = (slot: Offered) => (slot.job.id === job?.id ? undefined : slot.job.id);

  /** 換一個: this block is set aside (logged as 不用) and Manna designs another over the same paragraphs. */
  function swap(slot: Offered) {
    decide("tasks", slot.index, "reject", undefined, jobOf(slot));
    void sop.startJob("tasks", { parent_job_id: slot.job.id, item_index: slot.index });
  }

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
  function take(sb: SopBlockSuggestion, slot: Offered, edit: boolean) {
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
      bloom: t.bloom,
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
    decide("tasks", slot.index, edit ? "revise" : "adopt", edit ? tasks[0]?.text : undefined, jobOf(slot));
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
      .filter((slot) => hostOf(slot.block) === paragraphId)
      .map((slot) => {
        const sb = slot.pending ? null : slot.job.result?.output?.blocks[slot.index];
        if (!sb) {
          const members = slot.block.paragraph_nos;
          return (
            <RedoPending
              key={slot.home}
              label={members.length > 0 ? `第 ${members.join("、")} 段的問題或任務` : "問題或任務"}
              job={slot.pending}
              onCancelled={() => {
                if (slot.pending) void sop.refreshJob(slot.pending.id);
              }}
              onAgain={() => retryRedo(sop, "tasks", slot.pending)}
            />
          );
        }
        const redoSuggestion = slot.job.result?.output ?? suggestion;
        const members = sb.paragraph_ids
          .map((pid) => byId.get(pid))
          .filter((x): x is SopParagraph => !!x)
          .sort((a, b) => a.no - b.no);
        return (
          <MannaWell
            key={slot.home}
            label={paragraphRange(members) || "問題或任務"}
            onAdopt={() => take(sb, slot, false)}
            onEdit={() => take(sb, slot, true)}
            onDismiss={() => decide("tasks", slot.index, "reject", undefined, jobOf(slot))}
            onAgain={() => swap(slot)}
            againLabel="換一個"
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
            {!redoSuggestion?.scaffolds_ok && (
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
