import { useEffect,useRef } from "react";
import {
SOP_VOCABULARY_ITEM_INDEX,
type SopVocabularyItem,
type SopWarmupActivity,
type SopWarmupSuggestion
} from "../../api/types";
import {
emptyScaffolds,
newId
} from "../../lib/sop";
import { BloomSelect } from "../worksheet/Bloom";
import { GenerationChoice } from "./Choices";
import { EnglishFix } from "./EnglishFix";
import { AskManna,MannaWell,RedoPending,ScaffoldFields,ScaffoldList } from "./parts";
import { followRedo,retryRedo,type SopController } from "./useSop";


// ---------------------------------------------------------------- step 6

/** Step 6, 暖身: waking up background knowledge, and the key vocabulary. Both use scaffolds. */
export function StageWarmup({ sop, lessonId }: { sop: SopController; lessonId: number }) {
  const { content, update, decide } = sop;
  const warmup = content.warmup;
  const job = sop.jobs.warmup;
  const suggestion: SopWarmupSuggestion | null =
    job?.status === "done" && job.result?.status === "shown" ? (job.result.output ?? null) : null;
  /** The job of a decision: the whole run's, or the redo job the suggestion came from. */
  const jobOf = (shownJob: { id: number }) => (shownJob.id === job?.id ? undefined : shownJob.id);
  const activitySlots = (
    job && suggestion
      ? suggestion.background.map((_, home) => ({ home, ...followRedo(sop.jobs, "warmup", job, home) }))
      : []
  ).filter((slot) => slot.pending || !slot.job.decided.includes(slot.index));
  type ActivitySlot = (typeof activitySlots)[number];
  const vocabularySlot =
    job && suggestion && suggestion.vocabulary.length > 0
      ? followRedo(sop.jobs, "warmup", job, SOP_VOCABULARY_ITEM_INDEX, SOP_VOCABULARY_ITEM_INDEX)
      : null;
  const vocabularyOffer = vocabularySlot?.pending
    ? null
    : (vocabularySlot?.job.result?.output?.vocabulary ?? null);

  /** 換一個: this one is set aside (logged as 不用) and Manna thinks of another. */
  function swapActivity(slot: ActivitySlot) {
    decide("warmup", slot.index, "reject", undefined, jobOf(slot.job));
    void sop.startJob("warmup", { parent_job_id: slot.job.id, item_index: slot.index });
  }
  function swapVocabulary() {
    if (!vocabularySlot) return;
    decide("warmup", SOP_VOCABULARY_ITEM_INDEX, "reject", undefined, jobOf(vocabularySlot.job));
    void sop.startJob("warmup", { parent_job_id: vocabularySlot.job.id, item_index: SOP_VOCABULARY_ITEM_INDEX });
  }
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

  function takeActivity(slot: ActivitySlot, edit: boolean) {
    const s = slot.job.result?.output?.background[slot.index];
    if (!s) return;
    const id = newId("w");
    setBackground((list) => [
      ...list,
      {
        id,
        text: s.text,
        scaffolds: s.scaffolds,
        origin: edit ? "ai_edited" : "ai_adopted",
        prompt_blooms: s.prompt_blooms,
      },
    ]);
    decide("warmup", slot.index, edit ? "revise" : "adopt", edit ? s.text : undefined, jobOf(slot.job));
    if (edit) focusNext.current = `warmup-${id}`;
  }

  function takeVocabulary(edit: boolean) {
    if (!vocabularySlot || !vocabularyOffer) return;
    const have = new Set(warmup.vocabulary.map((v) => v.word.trim().toLowerCase()));
    const added: SopVocabularyItem[] = vocabularyOffer
      .filter((v) => !have.has(v.word.trim().toLowerCase()))
      .map((v) => ({
        id: newId("v"),
        word: v.word,
        note: v.definition_en,
        origin: edit ? "ai_edited" : "ai_adopted",
        level: v.level,
      }));
    setVocabulary((list) => [...list, ...added]);
    decide(
      "warmup",
      SOP_VOCABULARY_ITEM_INDEX,
      edit ? "revise" : "adopt",
      edit ? added.map((v) => v.word).join("、") : undefined,
      jobOf(vocabularySlot.job),
    );
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
            <EnglishFix
              lessonId={lessonId}
              field="warmup"
              text={activity.text}
              onUse={(english) =>
                setBackground((list) =>
                  list.map((a) => (a.id === activity.id ? { ...a, text: english, origin: "ai_edited" } : a)),
                )
              }
            />
            <ScaffoldFields
              idPrefix={`warmup-${activity.id}`}
              value={activity.scaffolds}
              onChange={(scaffolds) =>
                setBackground((list) => list.map((a) => (a.id === activity.id ? { ...a, scaffolds } : a)))
              }
            />
            {activity.prompt_blooms?.map((bloom, index) => (
              <BloomSelect
                key={`prompt-${index + 1}`}
                label={`暖身第 ${index + 1} 題的 Bloom 層次`}
                value={bloom}
                disabled={false}
                onChange={(next) =>
                  setBackground((list) => list.map((a) =>
                    a.id === activity.id
                      ? { ...a, prompt_blooms: a.prompt_blooms?.map((level, i) => (i === index ? next : level)) }
                      : a,
                  ))
                }
              />
            ))}
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
        {activitySlots.map((slot) => {
          const s = slot.pending ? null : slot.job.result?.output?.background[slot.index];
          if (!s) {
            return (
              <RedoPending
                key={slot.home}
                label={`暖身活動 ${slot.home + 1}`}
                job={slot.pending}
                onCancelled={() => {
                  if (slot.pending) void sop.refreshJob(slot.pending.id);
                }}
                onAgain={() => retryRedo(sop, "warmup", slot.pending)}
              />
            );
          }
          return (
            <MannaWell
              key={slot.home}
              label={`暖身活動 ${slot.home + 1}`}
              onAdopt={() => takeActivity(slot, false)}
              onEdit={() => takeActivity(slot, true)}
              onDismiss={() => decide("warmup", slot.index, "reject", undefined, jobOf(slot.job))}
              onAgain={() => swapActivity(slot)}
              againLabel="換一個"
            >
              <p className="pre-line" lang="en">
                {s.text}
              </p>
              {s.purpose && <p className="why">為什麼這樣設計：{s.purpose}</p>}
              <ScaffoldList value={s.scaffolds} />
            </MannaWell>
          );
        })}
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
        {vocabularySlot?.pending && (
          <RedoPending
            label="關鍵字彙"
            job={vocabularySlot.pending}
            onCancelled={() => {
              if (vocabularySlot.pending) void sop.refreshJob(vocabularySlot.pending.id);
            }}
            onAgain={() => retryRedo(sop, "warmup", vocabularySlot.pending)}
          />
        )}
        {vocabularySlot && vocabularyOffer && !vocabularySlot.job.decided.includes(SOP_VOCABULARY_ITEM_INDEX) && (
          <MannaWell
            label="關鍵字彙"
            onAdopt={() => takeVocabulary(false)}
            onEdit={() => takeVocabulary(true)}
            onDismiss={() => decide("warmup", SOP_VOCABULARY_ITEM_INDEX, "reject", undefined, jobOf(vocabularySlot.job))}
            onAgain={vocabularySlot.job.result?.output?.more_words === false ? undefined : swapVocabulary}
            againLabel="換一個"
          >
            <ul className="feature-list">
              {vocabularyOffer.map((v) => (
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
