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
import { GenerationChoice } from "./Choices";
import { AskManna,MannaWell,ScaffoldFields,ScaffoldList } from "./parts";
import { type SopController } from "./useSop";


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
