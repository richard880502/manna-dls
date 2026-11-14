import { useState, type FormEvent } from "react";
import { api, errorMessage } from "../../api/client";
import type { ObjectiveType, SopCurriculumLink, SopObjective, SopObjectiveSuggestion } from "../../api/types";
import { OBJECTIVE_TYPES, newId, objectiveTypeLabel } from "../../lib/sop";
import { ErrorText } from "../common";
import { AskManna, MannaWell } from "./parts";
import type { SopController } from "./useSop";

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
      <span className="code">{link.item_code}</span> {link.title}
      <button type="button" className="text-link is-plain" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? "收起原文" : "看原文"}
      </button>
      {onRemove && (
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`取消連結 ${link.item_code}`}
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

/** The teacher's own way to link an objective: keyword search over the knowledge base, no AI. */
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
      setResults(await api.searchCurriculum(query.trim()));
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
                <span className="code">{item.item_code}</span> <strong>{item.title}</strong>
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
                  aria-label={`連結 ${item.item_code}`}
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

/** Writing or rewriting one objective. The type can be changed: it is the teacher's call. */
function ObjectiveForm({
  idPrefix,
  initial,
  submitLabel,
  onDone,
  onCancel,
}: {
  idPrefix: string;
  initial: ObjectiveDraft;
  submitLabel: string;
  onDone: (value: ObjectiveDraft) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <form
      className="stage-form"
      onSubmit={(e) => {
        e.preventDefault();
        onDone(value);
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

interface StageObjectivesProps {
  sop: SopController;
  /** The plan's 「今天要教什麼？」 as it is on the page right now. */
  todayObjective: string;
}

/**
 * Step 3, 內容目標與語言目標 (規格 §2.1). The two kinds are kept apart. Manna's recommendations
 * (the second validated AI stage) are listed after the teacher's own objectives of the same kind,
 * each with its reason and its curriculum link; the official text and source come from the
 * knowledge base, and an objective nothing supports is marked 尚未完成課綱連結.
 */
export function StageObjectives({ sop, todayObjective }: StageObjectivesProps) {
  const { content, update, decide } = sop;
  const job = sop.jobs.objectives;
  const suggestion = job?.status === "done" && job.result?.status === "shown" ? job.result.output : null;
  const offered = (suggestion?.objectives ?? [])
    .map((objective, index) => ({ objective, index }))
    .filter(({ index }) => !job?.decided.includes(index));

  /** "new", an objective id being edited, or `s<index>` for a suggestion being edited before adopting. */
  const [editing, setEditing] = useState<string | null>(null);
  const [linking, setLinking] = useState<string | null>(null);

  const setObjectives = (change: (list: SopObjective[]) => SopObjective[]) =>
    update((c) => ({ ...c, objectives: change(c.objectives) }));
  const patchObjective = (id: string, patch: Partial<SopObjective>) =>
    setObjectives((list) => list.map((o) => (o.id === id ? { ...o, ...patch } : o)));

  function addOwn(draft: ObjectiveDraft) {
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
        origin: "teacher",
      },
    ]);
  }

  function adopt(s: SopObjectiveSuggestion, index: number, edited?: ObjectiveDraft) {
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
    decide("objectives", index, edited ? "revise" : "adopt", edited?.text.trim());
  }

  // The old single objective is offered here, not filed for the teacher: it never said which kind it was.
  const today = todayObjective.trim();
  const todayCarried = today !== "" && content.objectives.some((o) => o.text.trim() === today);

  const shownNote = suggestion
    ? suggestion.curriculum_available === 0
      ? "知識庫目前沒有適用的課綱條目，所以這些建議都還沒有課綱連結。"
      : `Manna 從 ${suggestion.curriculum_available} 條課綱條目裡找對應；條文是知識庫裡的原文。`
    : null;

  return (
    <>
      <p className="lead">這節課學生要理解什麼、要用英文做到什麼？兩種目標分開寫。</p>
      <AskManna
        sop={sop}
        kind="objectives"
        label="請 Manna 推薦目標"
        againLabel="請 Manna 再推薦一次"
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
        const suggested = offered.filter(({ objective }) => objective.type === kind.value);
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
                  onCancel={() => setEditing(null)}
                  onDone={(draft) => {
                    patchObjective(o.id, {
                      type: draft.type,
                      text: draft.text.trim(),
                      student_facing_en: draft.student_facing_en.trim(),
                      origin: o.origin === "teacher" ? "teacher" : "ai_edited",
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
            {suggested.map(({ objective: s, index }) =>
              editing === `s${index}` ? (
                <ObjectiveForm
                  key={index}
                  idPrefix={`suggestion-${index}`}
                  initial={{ type: s.type, text: s.text, student_facing_en: s.student_facing_en }}
                  submitLabel="採用修改後的目標"
                  onCancel={() => setEditing(null)}
                  onDone={(draft) => {
                    if (!draft.text.trim()) return;
                    adopt(s, index, draft);
                    setEditing(null);
                  }}
                />
              ) : (
                <MannaWell
                  key={index}
                  label={objectiveTypeLabel(s.type)}
                  onAdopt={() => adopt(s, index)}
                  onEdit={() => setEditing(`s${index}`)}
                  onDismiss={() => decide("objectives", index, "reject")}
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
                            <span className="code">{link.item_code}</span> {link.title}
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
              ),
            )}
          </section>
        );
      })}

      {editing === "new" ? (
        <ObjectiveForm
          idPrefix="objective-new"
          initial={{ type: "content", text: "", student_facing_en: "" }}
          submitLabel="加入目標"
          onCancel={() => setEditing(null)}
          onDone={(draft) => {
            addOwn(draft);
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
    </>
  );
}
