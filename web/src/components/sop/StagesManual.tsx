import { useState } from "react";
import type {
  ExitKind,
  InfoFunction,
  SopExitItem,
  SopParagraph,
  SopParagraphGoal,
  SopTask,
  SopTaskBlock,
  SopVocabularyItem,
  SopWarmupActivity,
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
import { NoMaterialNote, Quote, ScaffoldFields, ScaffoldList } from "./parts";
import type { SopController } from "./useSop";

interface StageProps {
  sop: SopController;
  onGoToStage: (step: 1 | 3) => void;
}

// These four steps are written by hand for now. Each keeps its items in the SOP document with an
// `origin`, so a later step can let Manna suggest per paragraph or per item the way steps 2 and 3 do.

// ---------------------------------------------------------------- step 4

/** Step 4, 逐段整合目標: which objectives each paragraph serves, and a note on how. */
export function StageIntegrate({ sop, onGoToStage }: StageProps) {
  const { material, content, update } = sop;
  const paragraphs = material?.paragraphs ?? [];
  const labels = objectiveLabels(content.objectives);
  const objectives = content.objectives.filter((o) => o.text.trim());
  const goals = new Map(content.paragraph_goals.map((g) => [g.paragraph_id, g]));

  const setGoal = (paragraphId: string, patch: Partial<SopParagraphGoal>) =>
    update((c) => {
      const current = c.paragraph_goals.find((g) => g.paragraph_id === paragraphId) ?? {
        paragraph_id: paragraphId,
        objective_ids: [],
        note: "",
      };
      const rest = c.paragraph_goals.filter((g) => g.paragraph_id !== paragraphId);
      return { ...c, paragraph_goals: [...rest, { ...current, ...patch }] };
    });

  return (
    <>
      <p className="lead">一段一段讀：這一段可以帶到哪一項內容目標、哪一項語言目標？</p>
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
});

const isEmptyTask = (t: SopTask) =>
  !t.text.trim() && !t.function && !t.scaffolds.verbal.trim() && !t.scaffolds.instructional.trim() && !t.scaffolds.procedural.trim();

function TaskEditor({
  task,
  onChange,
  onDone,
  onDelete,
}: {
  task: SopTask;
  onChange: (next: SopTask) => void;
  onDone: () => void;
  onDelete: () => void;
}) {
  const id = `task-${task.id}`;
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
      <ScaffoldFields idPrefix={id} value={task.scaffolds} onChange={(scaffolds) => onChange({ ...task, scaffolds })} />
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
        <p className="task-text" lang="en">
          {task.text || "（還沒寫題目）"}
        </p>
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
 * the procedural one is a plain field for now.
 */
export function StageTasks({ sop, onGoToStage }: StageProps) {
  const { material, content, update } = sop;
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

  const setBlocks = (change: (blocks: SopTaskBlock[]) => SopTaskBlock[]) =>
    update((c) => ({ ...c, task_blocks: change(c.task_blocks) }));

  /** Adds an empty task (opened for writing) to the block of these paragraphs, creating it if needed. */
  function addTask(paragraphIds: string[], blockId?: string) {
    const task = blankTask();
    setBlocks((blocks) => {
      const existing = blockId ? blocks.find((b) => b.id === blockId) : undefined;
      if (existing) return blocks.map((b) => (b.id === existing.id ? { ...b, tasks: [...b.tasks, task] } : b));
      return [...blocks, { id: newId("b"), paragraph_ids: paragraphIds, tasks: [task] }];
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

  function renderTasks(block: SopTaskBlock) {
    return block.tasks.map((task) =>
      editingTask === task.id ? (
        <TaskEditor
          key={task.id}
          task={task}
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

  return (
    <>
      <p className="lead">一段一段看：要學生發現、整理、分析、比較，還是創造？可以幾段合在一起設計。</p>
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
          </section>
        );
      })}

      <section className="stage-block" aria-labelledby="tasks-general-heading">
        <h3 id="tasks-general-heading">
          不分段的問題或任務 <small>整篇讀完之後的討論或產出</small>
        </h3>
        {general && renderTasks(general)}
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
    </>
  );
}

// ---------------------------------------------------------------- step 6

/** Step 6, 暖身: waking up background knowledge, and the key vocabulary. Both use scaffolds. */
export function StageWarmup({ sop }: { sop: SopController }) {
  const { content, update } = sop;
  const warmup = content.warmup;

  const setBackground = (change: (list: SopWarmupActivity[]) => SopWarmupActivity[]) =>
    update((c) => ({ ...c, warmup: { ...c.warmup, background: change(c.warmup.background) } }));
  const setVocabulary = (change: (list: SopVocabularyItem[]) => SopVocabularyItem[]) =>
    update((c) => ({ ...c, warmup: { ...c.warmup, vocabulary: change(c.warmup.vocabulary) } }));

  return (
    <>
      <p className="lead">
        主要的問題與任務設計好了，再回頭想怎麼開場。學習單上暖身會排在最前面。
      </p>

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
                setBackground((list) => list.map((a) => (a.id === activity.id ? { ...a, text: e.target.value } : a)))
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
      </section>
    </>
  );
}

// ---------------------------------------------------------------- step 7

/** Step 7, Exit ticket: a few short items for formative checking, reflection or collecting questions. */
export function StageExit({ sop }: { sop: SopController }) {
  const { content, update } = sop;
  const items = content.exit_ticket.items;
  const setItems = (change: (list: SopExitItem[]) => SopExitItem[]) =>
    update((c) => ({ ...c, exit_ticket: { items: change(c.exit_ticket.items) } }));

  return (
    <>
      <p className="lead">下課前兩三分鐘，想知道什麼？題數少一點。學習單上會放在最後，可以拿掉。</p>
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
              onChange={(e) =>
                setItems((list) => list.map((i) => (i.id === item.id ? { ...i, kind: e.target.value as ExitKind } : i)))
              }
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
              onChange={(e) =>
                setItems((list) => list.map((i) => (i.id === item.id ? { ...i, text: e.target.value } : i)))
              }
            />
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
              onClick={() => setItems((list) => [...list, { id: newId("e"), kind: k.value, text: "", origin: "teacher" }])}
            >
              ＋ {k.label}
            </button>
          ))}
        </p>
      </section>
    </>
  );
}
