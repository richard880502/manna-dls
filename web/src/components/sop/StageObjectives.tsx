import { useState, type FormEvent } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  KnowledgeItem,
  ObjectiveType,
  SopCurriculumLink,
  SopJob,
  SopObjective,
  SopObjectiveSuggestion,
  SopObjectivesSuggestion,
} from "../../api/types";
import { useApp } from "../../context/AppContext";
import { logScaffold } from "../../lib/scaffold";
import { LANGUAGE_AFTER_CONTENT, OBJECTIVE_TYPES, newId, objectiveTypeLabel } from "../../lib/sop";
import { ErrorText } from "../common";
import { GenerationChoice } from "./Choices";
import { AskManna, MannaWell, RedoPending } from "./parts";
import { redoFor, type SopController, type SopJobs } from "./useSop";

/** The code a teacher knows the item by: the one printed in the curriculum. */
const shownCode = (link: SopCurriculumLink) => link.official_code || link.item_code;

/** Code and title of an item. Imported titles usually start with the code: it is not said twice. */
function ItemName({ link }: { link: SopCurriculumLink }) {
  const code = shownCode(link);
  if (link.title.includes(code)) return <span className="item-name">{link.title}</span>;
  return (
    <span className="item-name">
      <span className="code">{code}</span> {link.title}
    </span>
  );
}

/** A knowledge-base item as a link (the server fills these fields in again when it saves). */
const linkFromItem = (item: KnowledgeItem): SopCurriculumLink => ({
  item_code: item.item_code,
  why: "",
  official_code: item.official_code,
  title: item.title,
  text: item.content,
  category: item.category,
  source_doc: item.source_doc,
  source_version: item.source_version,
  source_page: item.source_page,
});

/** Where a curriculum item comes from, as far as the knowledge base knows. */
function sourceLine(link: SopCurriculumLink): string {
  return [link.source_doc, link.source_version, link.source_page ? `第 ${link.source_page} 頁` : ""]
    .filter((part) => part && part.trim())
    .join("，");
}

/** One linked curriculum item: code and title, and behind 看原文 the official text with its source. */
function CurriculumLinkLine({ link, onRemove }: { link: SopCurriculumLink; onRemove?: () => void }) {
  const [open, setOpen] = useState(false);
  const source = sourceLine(link);
  return (
    <span className="curriculum-link">
      <ItemName link={link} />
      <button type="button" className="text-link is-plain" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? "收起原文" : "看原文"}
      </button>
      {onRemove && (
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`取消連結 ${shownCode(link)}`}
          onClick={onRemove}
        >
          取消連結
        </button>
      )}
      {open && (
        <span className="curriculum-text">
          {link.text}
          {source && <span className="curriculum-source">來源：{source}</span>}
        </span>
      )}
    </span>
  );
}

/**
 * The teacher's own way to link an objective: the knowledge base's curriculum search (senior
 * high, English subjects of the teacher's school track), no AI.
 */
function CurriculumPicker({
  linked,
  onPick,
  onClose,
}: {
  linked: string[];
  onPick: (link: SopCurriculumLink) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SopCurriculumLink[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function search(event: FormEvent) {
    event.preventDefault();
    setSearching(true);
    setError(null);
    try {
      const found = await api.searchCurriculum({ q: query.trim(), limit: 20 });
      setResults((found.items ?? []).map((hit) => linkFromItem(hit.item)));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSearching(false);
    }
  }

  return (
    <div className="stage-form curriculum-picker" role="group" aria-label="連結課綱條目">
      <form className="inline-add" onSubmit={(e) => void search(e)}>
        <div>
          <label className="field-label" htmlFor="curriculum-query">
            找課綱條目
          </label>
          <input
            id="curriculum-query"
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="輸入關鍵字，例如：推論、口語、摘要；留白列出全部"
          />
        </div>
        <button type="submit" className="secondary-button" disabled={searching}>
          {searching ? "搜尋中…" : "搜尋"}
        </button>
        <button type="button" className="text-link is-plain" onClick={onClose}>
          關閉
        </button>
      </form>
      <ErrorText message={error} />
      {results !== null && results.length === 0 && (
        <p className="quiet-note" role="status">
          知識庫裡找不到符合的課綱條目。這項目標會先標示為尚未完成課綱連結。
        </p>
      )}
      {results !== null && results.length > 0 && (
        <ul className="curriculum-results">
          {results.map((item) => (
            <li key={item.item_code}>
              <span>
                <strong>
                  <ItemName link={item} />
                </strong>
                <span className="curriculum-text">
                  {item.text}
                  {sourceLine(item) && <span className="curriculum-source">來源：{sourceLine(item)}</span>}
                </span>
              </span>
              {linked.includes(item.item_code) ? (
                <span className="quiet-term">已連結</span>
              ) : (
                <button
                  type="button"
                  className="text-link"
                  aria-label={`連結 ${shownCode(item)}`}
                  onClick={() => onPick(item)}
                >
                  連結
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

interface ObjectiveDraft {
  type: ObjectiveType;
  text: string;
  student_facing_en: string;
}

interface EnglishProposal {
  interactionId: number | null;
  english: string;
  changed: boolean;
  note: string;
}

/**
 * Writing or rewriting one objective. The type can be changed: it is the teacher's call. 幫我修英文
 * asks Manna for a corrected version of the student-facing English (or, when it is empty, one
 * written from the Chinese objective) and shows it next to what the teacher wrote: nothing is
 * replaced until she presses 用這個.
 */
function ObjectiveForm({
  idPrefix,
  initial,
  submitLabel,
  lessonId,
  onDone,
  onCancel,
}: {
  idPrefix: string;
  initial: ObjectiveDraft;
  submitLabel: string;
  lessonId: number;
  /** `usedAiEnglish`: the teacher took Manna's English. */
  onDone: (value: ObjectiveDraft, usedAiEnglish: boolean) => void;
  onCancel: () => void;
}) {
  const { config } = useApp();
  const aiEnabled = config?.ai_enabled !== false;
  const [value, setValue] = useState(initial);
  const [usedAi, setUsedAi] = useState(false);
  const [asking, setAsking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [proposal, setProposal] = useState<EnglishProposal | null>(null);
  const hasEnglish = value.student_facing_en.trim() !== "";

  async function askEnglish() {
    setAsking(true);
    setProblem(null);
    setProposal(null);
    logScaffold("SC-SOP-ENGLISH", "expand", { trigger: "request" });
    try {
      const result = await api.sopObjectiveEnglish(lessonId, {
        type: value.type,
        text: value.text,
        student_facing_en: value.student_facing_en,
      });
      if (result.status === "shown" && result.output) {
        logScaffold("SC-SOP-ENGLISH", "show", { trigger: "suggestion" });
        setProposal({ interactionId: result.interaction_id, ...result.output });
      } else {
        setProblem(result.message ?? "這次沒有改好，請再試一次。");
      }
    } catch (err) {
      setProblem(errorMessage(err));
    } finally {
      setAsking(false);
    }
  }

  function decideEnglish(decision: "adopt" | "reject") {
    if (!proposal) return;
    if (proposal.interactionId != null) {
      // Research logging never blocks the teacher.
      void api.sendDecision(proposal.interactionId, { item_index: 0, decision }).catch(() => undefined);
    }
    logScaffold("SC-SOP-ENGLISH", decision === "adopt" ? "use" : "skip", { trigger: decision });
    if (decision === "adopt") {
      setValue((v) => ({ ...v, student_facing_en: proposal.english }));
      setUsedAi(true);
    }
    setProposal(null);
  }

  return (
    <form
      className="stage-form"
      onSubmit={(e) => {
        e.preventDefault();
        onDone(value, usedAi);
      }}
    >
      <fieldset className="type-choice">
        <legend className="field-label">這是哪一種目標</legend>
        {OBJECTIVE_TYPES.map((t) => (
          <label key={t.value}>
            <input
              type="radio"
              name={`${idPrefix}-type`}
              checked={value.type === t.value}
              onChange={() => setValue({ ...value, type: t.value })}
            />
            {t.label}
            <span className="quiet-term">{t.hint}</span>
          </label>
        ))}
      </fieldset>
      <label className="field-label" htmlFor={`${idPrefix}-text`}>
        目標
      </label>
      <textarea
        id={`${idPrefix}-text`}
        rows={2}
        value={value.text}
        onChange={(e) => setValue({ ...value, text: e.target.value })}
        placeholder="學生能……"
      />
      <label className="field-label" htmlFor={`${idPrefix}-en`}>
        給學生看的英文版本
      </label>
      <input
        id={`${idPrefix}-en`}
        type="text"
        lang="en"
        value={value.student_facing_en}
        onChange={(e) => setValue({ ...value, student_facing_en: e.target.value })}
        placeholder="I can …"
      />
      <p className="para-actions">
        <button
          type="button"
          className="manna-button"
          disabled={asking || !aiEnabled || !value.text.trim()}
          onClick={() => void askEnglish()}
        >
          <span className="star" aria-hidden="true" />
          {asking ? "Manna 正在看…" : hasEnglish ? "幫我修英文" : "依中文目標寫英文版"}
        </button>
        {!value.text.trim() && <span className="field-hint">先寫中文目標，Manna 才知道要寫什麼。</span>}
      </p>
      <ErrorText message={problem} />
      {proposal && (
        <div className="manna-well english-proposal" role="group" aria-label="Manna 建議的英文">
          <p className="manna-label">
            <span className="star" aria-hidden="true" />
            Manna 建議：給學生看的英文
          </p>
          {hasEnglish && (
            <p className="mine-english" lang="en">
              <span className="quiet-term">你寫的</span>
              {value.student_facing_en}
            </p>
          )}
          <p className="proposed-english student-facing" lang="en">
            {proposal.english}
          </p>
          <p className="why">{proposal.note}</p>
          <div className="decide">
            {proposal.changed && (
              <button type="button" className="decide-button is-take" onClick={() => decideEnglish("adopt")}>
                用這個
              </button>
            )}
            <button type="button" className="decide-button" onClick={() => decideEnglish("reject")}>
              保留我的
            </button>
          </div>
        </div>
      )}
      <div className="button-row">
        <button type="submit" className="secondary-button">
          {submitLabel}
        </button>
        <button type="button" className="text-link is-plain" onClick={onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/** One place for a suggestion of the step: where it stands now (a 換一個 may have replaced it). */
interface Slot {
  /** Index in the stage's whole run: where the slot lives on the page. */
  home: number;
  kind: ObjectiveType;
  /** The job the visible suggestion comes from (the whole run, or a redo job). */
  job: SopJob<SopObjectivesSuggestion>;
  /** `item_index` of the suggestion in that job. */
  index: number;
  suggestion: SopObjectiveSuggestion | null;
  /** A 換一個 that is running or did not work. */
  pending: SopJob | null;
}

function resolveSlot(jobs: SopJobs, whole: SopJob<SopObjectivesSuggestion>, home: number): Slot | null {
  const first = whole.result?.output?.objectives[home];
  if (!first) return null;
  let job = whole;
  let index = home;
  let pending: SopJob | null = null;
  for (;;) {
    const next = redoFor(jobs, "objectives", job.id, `${job.id}:${index}`) as SopJob<SopObjectivesSuggestion> | null;
    if (!next) break;
    if (next.status !== "done" || next.result?.status !== "shown") {
      pending = next;
      break;
    }
    job = next;
    index = 0;
  }
  if (!pending && job.decided.includes(index)) return null;
  return {
    home,
    kind: first.type,
    job,
    index,
    suggestion: pending ? null : (job.result?.output?.objectives[index] ?? null),
    pending,
  };
}

interface StageObjectivesProps {
  sop: SopController;
  /** The plan's 「今天要教什麼？」 as it is on the page right now. */
  todayObjective: string;
  lessonId: number;
}

/**
 * Step 3, 內容目標與語言目標 (規格 §2.1). The two kinds are kept apart. Manna's recommendations
 * (the second validated AI stage) are listed after the teacher's own objectives of the same kind,
 * each with its reason and its curriculum link; the official text and source come from the
 * knowledge base, and an objective nothing supports is marked 尚未完成課綱連結.
 */
export function StageObjectives({ sop, todayObjective, lessonId }: StageObjectivesProps) {
  const { content, update, decide } = sop;
  const job = sop.jobs.objectives;
  const suggestion = job?.status === "done" && job.result?.status === "shown" ? job.result.output : null;
  const slots: Slot[] =
    job && suggestion
      ? suggestion.objectives
          .map((_, home) => resolveSlot(sop.jobs, job, home))
          .filter((slot): slot is Slot => slot !== null)
      : [];
  /** The job of a decision: the whole run's, or the redo job the suggestion came from. */
  const jobOf = (slot: Slot) => (slot.job.id === job?.id ? undefined : slot.job.id);

  /** 換一個: this one is set aside (logged as 不用) and Manna thinks of another of the same type. */
  function swap(slot: Slot) {
    decide("objectives", slot.index, "reject", undefined, jobOf(slot));
    void sop.startJob("objectives", { parent_job_id: slot.job.id, item_index: slot.index });
  }

  /** "new", an objective id being edited, or `s<index>` for a suggestion being edited before adopting. */
  const [editing, setEditing] = useState<string | null>(null);
  const [linking, setLinking] = useState<string | null>(null);

  const setObjectives = (change: (list: SopObjective[]) => SopObjective[]) =>
    update((c) => ({ ...c, objectives: change(c.objectives) }));
  const patchObjective = (id: string, patch: Partial<SopObjective>) =>
    setObjectives((list) => list.map((o) => (o.id === id ? { ...o, ...patch } : o)));

  function addOwn(draft: ObjectiveDraft, usedAiEnglish = false) {
    if (!draft.text.trim()) return;
    setObjectives((list) => [
      ...list,
      {
        id: newId("o"),
        type: draft.type,
        text: draft.text.trim(),
        student_facing_en: draft.student_facing_en.trim(),
        reason: "",
        paragraph_ids: [],
        links: [],
        link_status: "unlinked",
        origin: usedAiEnglish ? "ai_edited" : "teacher",
      },
    ]);
  }

  function adopt(s: SopObjectiveSuggestion, slot: Slot, edited?: ObjectiveDraft) {
    setObjectives((list) => [
      ...list,
      {
        id: newId("o"),
        type: edited?.type ?? s.type,
        text: (edited?.text ?? s.text).trim(),
        student_facing_en: (edited?.student_facing_en ?? s.student_facing_en).trim(),
        reason: s.reason,
        paragraph_ids: s.paragraph_ids,
        links: s.links,
        link_status: s.links.length > 0 ? "linked" : "unlinked",
        origin: edited ? "ai_edited" : "ai_adopted",
      },
    ]);
    decide("objectives", slot.index, edited ? "revise" : "adopt", edited?.text.trim(), jobOf(slot));
  }

  // The old single objective is offered here, not filed for the teacher: it never said which kind it was.
  const today = todayObjective.trim();
  const hasContent = content.objectives.some((o) => o.type === "content" && o.text.trim() !== "");
  const todayCarried = today !== "" && content.objectives.some((o) => o.text.trim() === today);

  const shownNote = suggestion
    ? suggestion.curriculum_available === 0
      ? "知識庫目前沒有適用的課綱條目，所以這些建議都還沒有課綱連結。"
      : `Manna 從 ${suggestion.curriculum_available} 條課綱條目裡找對應；條文是知識庫裡的原文。`
    : null;

  return (
    <>
      <p className="lead">
        這節課學生要理解什麼、要用英文做到什麼？兩種目標分開寫：先決定內容目標，再依它寫語言目標。語言目標是為了達成內容目標，學生需要用什麼語言（讀、聽、說、寫做出什麼），老師看到成果就知道學生有沒有學會內容。
      </p>
      <AskManna
        sop={sop}
        kind="objectives"
        label=""
        againLabel=""
        options={[
          { label: "請 Manna 推薦內容目標", only: "content" },
          {
            label: "請 Manna 依內容目標推薦語言目標",
            only: "language",
            blocked: hasContent ? null : LANGUAGE_AFTER_CONTENT,
          },
        ]}
        shownNote={shownNote}
      />

      {today !== "" && !todayCarried && (
        <div className="carry-over" role="group" aria-label="今天要教什麼裡寫的目標">
          <p>
            <span className="quiet-term">你在「今天要教什麼？」寫的</span>
            {today}
          </p>
          <p className="carry-actions">
            {OBJECTIVE_TYPES.map((t) => (
              <button
                key={t.value}
                type="button"
                className="text-link is-plain"
                onClick={() => addOwn({ type: t.value, text: today, student_facing_en: "" })}
              >
                放進{t.label}
              </button>
            ))}
          </p>
        </div>
      )}

      {OBJECTIVE_TYPES.map((kind) => {
        const mine = content.objectives.filter((o) => o.type === kind.value);
        const suggested = slots.filter((slot) => slot.kind === kind.value);
        return (
          <section key={kind.value} className="stage-block" aria-labelledby={`objectives-${kind.value}-heading`}>
            <h3 id={`objectives-${kind.value}-heading`}>
              {kind.label} <small>{kind.hint}</small>
            </h3>
            {mine.length === 0 && suggested.length === 0 && (
              <p className="quiet-note">還沒有{kind.label}。可以自己新增，或請 Manna 推薦。</p>
            )}
            {mine.map((o) =>
              editing === o.id ? (
                <ObjectiveForm
                  key={o.id}
                  idPrefix={`objective-${o.id}`}
                  initial={{ type: o.type, text: o.text, student_facing_en: o.student_facing_en }}
                  submitLabel="完成"
                  lessonId={lessonId}
                  onCancel={() => setEditing(null)}
                  onDone={(draft, usedAiEnglish) => {
                    patchObjective(o.id, {
                      type: draft.type,
                      text: draft.text.trim(),
                      student_facing_en: draft.student_facing_en.trim(),
                      origin: o.origin === "teacher" && !usedAiEnglish ? "teacher" : "ai_edited",
                    });
                    setEditing(null);
                  }}
                />
              ) : (
                <div key={o.id} className="objective-row">
                  <div className="mine">
                    <div>
                      <p>{o.text}</p>
                      {o.student_facing_en && (
                        <p className="student-facing" lang="en">
                          {o.student_facing_en}
                        </p>
                      )}
                      <p className="objective-links">
                        {o.links.length > 0 ? (
                          <>
                            <span className="quiet-term">已連結 108 課綱</span>
                            {o.links.map((link) => (
                              <CurriculumLinkLine
                                key={link.item_code}
                                link={link}
                                onRemove={() =>
                                  patchObjective(o.id, {
                                    links: o.links.filter((l) => l.item_code !== link.item_code),
                                    link_status: o.links.length > 1 ? "linked" : "unlinked",
                                  })
                                }
                              />
                            ))}
                          </>
                        ) : (
                          <span className="warn-tag">尚未完成課綱連結</span>
                        )}
                        <button
                          type="button"
                          className="text-link is-plain"
                          aria-label={`為「${o.text}」連結課綱條目`}
                          onClick={() => setLinking(linking === o.id ? null : o.id)}
                        >
                          {o.links.length > 0 ? "再連結一條" : "連結課綱條目"}
                        </button>
                      </p>
                    </div>
                    <span className="tools">
                      <button
                        type="button"
                        className="text-link is-plain"
                        aria-label={`修改「${o.text}」`}
                        onClick={() => setEditing(o.id)}
                      >
                        修改
                      </button>
                      <button
                        type="button"
                        className="text-link is-plain"
                        aria-label={`刪除「${o.text}」`}
                        onClick={() => setObjectives((list) => list.filter((x) => x.id !== o.id))}
                      >
                        刪除
                      </button>
                    </span>
                  </div>
                  {linking === o.id && (
                    <CurriculumPicker
                      linked={o.links.map((l) => l.item_code)}
                      onClose={() => setLinking(null)}
                      onPick={(link) =>
                        patchObjective(o.id, { links: [...o.links, { ...link, why: "" }], link_status: "linked" })
                      }
                    />
                  )}
                </div>
              ),
            )}

            {suggested.length > 0 && (
              <p className="suggest-head">
                Manna {mine.length > 0 ? "另外" : ""}想到 {suggested.length} 項
              </p>
            )}
            {suggested.map((slot) => {
              const s = slot.suggestion;
              if (!s) {
                return (
                  <RedoPending
                    key={slot.home}
                    label={objectiveTypeLabel(slot.kind)}
                    job={slot.pending}
                    onCancelled={() => {
                      if (slot.pending) void sop.refreshJob(slot.pending.id);
                    }}
                    onAgain={() => {
                      const failed = slot.pending;
                      if (failed?.parent_job_id != null) {
                        void sop.startJob("objectives", {
                          parent_job_id: failed.parent_job_id,
                          item_index: Number((failed.slot ?? "").split(":")[1] ?? 0),
                        });
                      }
                    }}
                  />
                );
              }
              return editing === `s${slot.home}` ? (
                <ObjectiveForm
                  key={slot.home}
                  idPrefix={`suggestion-${slot.home}`}
                  initial={{ type: s.type, text: s.text, student_facing_en: s.student_facing_en }}
                  submitLabel="採用修改後的目標"
                  lessonId={lessonId}
                  onCancel={() => setEditing(null)}
                  onDone={(draft) => {
                    if (!draft.text.trim()) return;
                    adopt(s, slot, draft);
                    setEditing(null);
                  }}
                />
              ) : (
                <MannaWell
                  key={slot.home}
                  label={objectiveTypeLabel(s.type)}
                  onAdopt={() => adopt(s, slot)}
                  onEdit={() => setEditing(`s${slot.home}`)}
                  onDismiss={() => decide("objectives", slot.index, "reject", undefined, jobOf(slot))}
                  onAgain={() => swap(slot)}
                  againLabel="換一個"
                >
                  <p>{s.text}</p>
                  {s.student_facing_en && (
                    <p className="student-facing" lang="en">
                      {s.student_facing_en}
                    </p>
                  )}
                  <p className="why">為什麼建議：{s.reason}</p>
                  {s.links.length > 0 ? (
                    <div className="src">
                      <span className="quiet-term">連結 108 課綱</span>
                      {s.links.map((link) => (
                        <div key={link.item_code} className="well-link">
                          <p>
                            <ItemName link={link} />
                          </p>
                          <p className="curriculum-text">{link.text}</p>
                          {sourceLine(link) && <p className="curriculum-source">來源：{sourceLine(link)}</p>}
                          {link.why && <p className="link-why">對應的理由：{link.why}</p>}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="src">
                      <span className="warn-tag">尚未完成課綱連結</span>
                      找不到足以支持的課綱條目來源，先保留為建議；你可以自己補上。
                    </p>
                  )}
                </MannaWell>
              );
            })}
          </section>
        );
      })}

      {editing === "new" ? (
        <ObjectiveForm
          idPrefix="objective-new"
          initial={{ type: "content", text: "", student_facing_en: "" }}
          submitLabel="加入目標"
          lessonId={lessonId}
          onCancel={() => setEditing(null)}
          onDone={(draft, usedAiEnglish) => {
            addOwn(draft, usedAiEnglish);
            setEditing(null);
          }}
        />
      ) : (
        <p className="stage-inline-actions">
          <button type="button" className="text-link" onClick={() => setEditing("new")}>
            自己新增一項目標
          </button>
        </p>
      )}

      <GenerationChoice
        sop={sop}
        heading="學習單上要印出學習目標嗎？"
        labels={["顯示學習目標"]}
        hint="印在學習單最上面，給學生看的是上面的英文版本。"
      />
    </>
  );
}
