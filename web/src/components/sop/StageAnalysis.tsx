import { useState } from "react";
import type {
  SopAnalysisSuggestion,
  SopFeature,
  SopLanguageFocus,
  SopParagraph,
  SopParagraphAnalysis,
} from "../../api/types";
import { newId } from "../../lib/sop";
import { GenerationChoice } from "./Choices";
import { AskManna, MannaWell, NoMaterialNote, Quote } from "./parts";
import type { SopController } from "./useSop";

type ParagraphSuggestion = SopAnalysisSuggestion["paragraphs"][number];

const squash = (s: string) => s.replace(/\s+/g, " ").trim();
const isBlank = (a: SopParagraphAnalysis | undefined) =>
  !a || (!a.function.trim() && !a.summary.trim() && a.features.length === 0);

/** What a paragraph's analysis becomes when a suggestion is taken: the teacher's words stay. */
function withSuggestion(existing: SopParagraphAnalysis | undefined, s: ParagraphSuggestion): SopParagraphAnalysis {
  return {
    paragraph_id: s.paragraph_id,
    function: existing?.function.trim() ? existing.function : s.function,
    summary: existing?.summary.trim() ? existing.summary : s.summary,
    features: [...(existing?.features ?? []), ...s.features.map((f) => ({ id: newId("f"), ...f }))],
    origin: isBlank(existing) ? "ai_adopted" : "ai_edited",
  };
}

interface ParagraphEditorProps {
  paragraph: SopParagraph;
  initial: SopParagraphAnalysis;
  onDone: (value: SopParagraphAnalysis) => void;
  onCancel: () => void;
}

function ParagraphEditor({ paragraph, initial, onDone, onCancel }: ParagraphEditorProps) {
  const [value, setValue] = useState(initial);
  const id = `analysis-${paragraph.id}`;
  const source = squash(paragraph.text);
  const setFeature = (featureId: string, patch: Partial<SopFeature>) =>
    setValue((v) => ({ ...v, features: v.features.map((f) => (f.id === featureId ? { ...f, ...patch } : f)) }));

  return (
    <div className="stage-form" role="group" aria-label={`第 ${paragraph.no} 段的分析`}>
      <label className="field-label" htmlFor={`${id}-function`}>
        這一段在全文中的功能
      </label>
      <input
        id={`${id}-function`}
        type="text"
        value={value.function}
        onChange={(e) => setValue({ ...value, function: e.target.value })}
        placeholder="例如：以故事開場、提出問題、轉折、總結"
      />
      <label className="field-label" htmlFor={`${id}-summary`}>
        一句話摘要
      </label>
      <input
        id={`${id}-summary`}
        type="text"
        value={value.summary}
        onChange={(e) => setValue({ ...value, summary: e.target.value })}
      />
      {value.features.map((f, index) => {
        const quoted = squash(f.evidence);
        const notInParagraph = quoted.length > 0 && !source.includes(quoted);
        return (
          <fieldset key={f.id} className="feature-fields">
            <legend className="field-label">寫作特色 {index + 1}</legend>
            <label className="sr-only" htmlFor={`${id}-${f.id}-feature`}>
              寫作特色 {index + 1}
            </label>
            <input
              id={`${id}-${f.id}-feature`}
              type="text"
              value={f.feature}
              onChange={(e) => setFeature(f.id, { feature: e.target.value })}
              placeholder="例如：轉折語、主題句在段首、舉例"
            />
            <label className="field-label" htmlFor={`${id}-${f.id}-evidence`}>
              原文裡的句子
            </label>
            <input
              id={`${id}-${f.id}-evidence`}
              type="text"
              lang="en"
              className="print-input"
              value={f.evidence}
              onChange={(e) => setFeature(f.id, { evidence: e.target.value })}
            />
            {notInParagraph && <p className="field-hint">這段文字和第 {paragraph.no} 段的原文不完全一樣。</p>}
            <label className="field-label" htmlFor={`${id}-${f.id}-explanation`}>
              說明
            </label>
            <input
              id={`${id}-${f.id}-explanation`}
              type="text"
              value={f.explanation}
              onChange={(e) => setFeature(f.id, { explanation: e.target.value })}
            />
            <button
              type="button"
              className="text-link is-plain"
              onClick={() => setValue((v) => ({ ...v, features: v.features.filter((x) => x.id !== f.id) }))}
            >
              刪除這個特色
            </button>
          </fieldset>
        );
      })}
      <p>
        <button
          type="button"
          className="text-link is-plain"
          onClick={() =>
            setValue((v) => ({
              ...v,
              features: [...v.features, { id: newId("f"), feature: "", evidence: "", explanation: "" }],
            }))
          }
        >
          ＋ 加一個寫作特色
        </button>
      </p>
      <div className="button-row">
        <button type="button" className="secondary-button" onClick={() => onDone(value)}>
          完成
        </button>
        <button type="button" className="text-link is-plain" onClick={onCancel}>
          取消
        </button>
      </div>
    </div>
  );
}

function ParagraphAnalysisView({ analysis }: { analysis: SopParagraphAnalysis }) {
  return (
    <div className="analysis-view">
      {analysis.function.trim() && (
        <p>
          <span className="quiet-term">功能</span>
          {analysis.function}
        </p>
      )}
      {analysis.summary.trim() && (
        <p>
          <span className="quiet-term">摘要</span>
          {analysis.summary}
        </p>
      )}
      {analysis.features.length > 0 && (
        <ul className="feature-list">
          {analysis.features.map((f) => (
            <li key={f.id}>
              <strong>{f.feature}</strong>
              {f.evidence && (
                <span className="evidence" lang="en">
                  {f.evidence}
                </span>
              )}
              {f.explanation && <span className="explanation">{f.explanation}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

interface StageAnalysisProps {
  sop: SopController;
  onGoToMaterial: () => void;
}

/**
 * Step 2, 分析寫作特色: the whole text first, then paragraph by paragraph. Manna's analysis (one
 * of the two validated AI stages) appears in gold under the paragraph it is about; every piece
 * of evidence she quotes was checked to be a continuous piece of that paragraph.
 */
export function StageAnalysis({ sop, onGoToMaterial }: StageAnalysisProps) {
  const { material, content, update, decide } = sop;
  const paragraphs = material?.paragraphs ?? [];
  const analysis = content.analysis;
  const job = sop.jobs.analysis;
  const suggestion = job?.status === "done" && job.result?.status === "shown" ? job.result.output : null;
  const undecided = (index: number) => suggestion !== null && !job?.decided.includes(index);

  /** The paragraph being edited, and the suggestion (by paragraph number) the edit started from. */
  const [editing, setEditing] = useState<{ id: string; from: number | null; initial: SopParagraphAnalysis } | null>(
    null,
  );
  const [focusItem, setFocusItem] = useState("");
  const [focusNote, setFocusNote] = useState("");

  const byParagraph = new Map(analysis.paragraphs.map((p) => [p.paragraph_id, p]));
  const known = new Set(paragraphs.map((p) => p.id));
  const orphans = analysis.paragraphs.filter((p) => !known.has(p.paragraph_id) && !isBlank(p));

  const setOverall = (patch: Partial<typeof analysis>) =>
    update((c) => ({ ...c, analysis: { ...c.analysis, ...patch } }));

  const putParagraph = (value: SopParagraphAnalysis) =>
    update((c) => {
      const rest = c.analysis.paragraphs.filter((p) => p.paragraph_id !== value.paragraph_id);
      return { ...c, analysis: { ...c.analysis, paragraphs: [...rest, value] } };
    });

  const removeParagraph = (paragraphId: string) =>
    update((c) => ({
      ...c,
      analysis: { ...c.analysis, paragraphs: c.analysis.paragraphs.filter((p) => p.paragraph_id !== paragraphId) },
    }));

  function adoptOverall(decision: "adopt" | "revise") {
    if (!suggestion) return;
    update((c) => {
      const a = c.analysis;
      const have = new Set(a.language_focus.map((f) => f.item.toLowerCase()));
      const added: SopLanguageFocus[] = suggestion.language_focus
        .filter((f) => !have.has(f.item.toLowerCase()))
        .map((f) => ({ id: newId("lf"), ...f }));
      const untouched = !a.text_type.trim() && !a.main_idea.trim() && !a.structure.trim();
      return {
        ...c,
        analysis: {
          ...a,
          // Only what the teacher left empty is filled in.
          text_type: a.text_type.trim() ? a.text_type : suggestion.text_type,
          main_idea: a.main_idea.trim() ? a.main_idea : suggestion.main_idea,
          structure: a.structure.trim() ? a.structure : suggestion.structure,
          overall_origin: untouched && decision === "adopt" ? "ai_adopted" : "ai_edited",
          language_focus: [...a.language_focus, ...added],
        },
      };
    });
    decide("analysis", 0, decision, decision === "revise" ? suggestion.main_idea : undefined);
    if (decision === "revise") document.getElementById("sop-main-idea")?.focus();
  }

  function addFocus() {
    if (!focusItem.trim()) return;
    update((c) => ({
      ...c,
      analysis: {
        ...c.analysis,
        language_focus: [
          ...c.analysis.language_focus,
          { id: newId("lf"), kind: "", item: focusItem.trim(), paragraph_id: null, evidence: "", note: focusNote.trim() },
        ],
      },
    }));
    setFocusItem("");
    setFocusNote("");
  }

  const shownNote = suggestion
    ? [
        `Manna 分析了 ${suggestion.paragraphs.length} 段，建議在各段下方。`,
        suggestion.missing_paragraph_ids.length > 0
          ? `有 ${suggestion.missing_paragraph_ids.length} 段這次沒有分析到，可以自己寫或再請她試一次。`
          : "",
        suggestion.dropped_evidence > 0
          ? `有 ${suggestion.dropped_evidence} 筆引文和原文對不起來，已經拿掉。`
          : "",
      ]
        .filter(Boolean)
        .join("")
    : null;

  return (
    <>
      <p className="lead">每一段在全文裡做什麼、用了什麼寫法？這裡寫下的，會是訂目標和設計提問的依據。</p>
      <AskManna
        sop={sop}
        kind="analysis"
        label="請 Manna 逐段分析"
        againLabel="請 Manna 再分析一次"
        shownNote={shownNote}
      />

      <section className="stage-block" aria-labelledby="analysis-overall-heading">
        <h3 id="analysis-overall-heading">全文</h3>
        <div className="overall-fields">
          <div>
            <label className="field-label" htmlFor="sop-text-type">
              文體
            </label>
            <input
              id="sop-text-type"
              type="text"
              value={analysis.text_type}
              onChange={(e) => setOverall({ text_type: e.target.value })}
              placeholder="例如：記敘文、說明文、論說文"
            />
          </div>
          <div>
            <label className="field-label" htmlFor="sop-main-idea">
              主旨
            </label>
            <textarea
              id="sop-main-idea"
              rows={2}
              value={analysis.main_idea}
              onChange={(e) => setOverall({ main_idea: e.target.value })}
            />
          </div>
          <div>
            <label className="field-label" htmlFor="sop-structure">
              結構
            </label>
            <textarea
              id="sop-structure"
              rows={2}
              value={analysis.structure}
              onChange={(e) => setOverall({ structure: e.target.value })}
              placeholder="例如：開場故事 → 鄰居的預測 → 轉折 → 作者的看法"
            />
          </div>
        </div>

        <h4 className="sub-head">值得教的語言重點</h4>
        {analysis.language_focus.length > 0 && (
          <ul className="focus-list">
            {analysis.language_focus.map((f) => (
              <li key={f.id}>
                <span>
                  <strong lang="en">{f.item}</strong>
                  {f.kind && <span className="quiet-term">{f.kind}</span>}
                  {f.note && <span className="explanation">{f.note}</span>}
                </span>
                <button
                  type="button"
                  className="text-link is-plain"
                  aria-label={`刪除語言重點 ${f.item}`}
                  onClick={() =>
                    update((c) => ({
                      ...c,
                      analysis: {
                        ...c.analysis,
                        language_focus: c.analysis.language_focus.filter((x) => x.id !== f.id),
                      },
                    }))
                  }
                >
                  刪除
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="inline-add">
          <div>
            <label className="field-label" htmlFor="sop-focus-item">
              字彙、句型或篇章標記
            </label>
            <input id="sop-focus-item" type="text" lang="en" value={focusItem} onChange={(e) => setFocusItem(e.target.value)} />
          </div>
          <div>
            <label className="field-label" htmlFor="sop-focus-note">
              為什麼值得教
            </label>
            <input id="sop-focus-note" type="text" value={focusNote} onChange={(e) => setFocusNote(e.target.value)} />
          </div>
          <button type="button" className="secondary-button" onClick={addFocus} disabled={!focusItem.trim()}>
            加入
          </button>
        </div>

        {suggestion && undecided(0) && (
          <MannaWell
            label="全文"
            onAdopt={() => adoptOverall("adopt")}
            onEdit={() => adoptOverall("revise")}
            onDismiss={() => decide("analysis", 0, "reject")}
          >
            <dl className="well-list">
              <div>
                <dt>文體</dt>
                <dd>{suggestion.text_type}</dd>
              </div>
              <div>
                <dt>主旨</dt>
                <dd>{suggestion.main_idea}</dd>
              </div>
              <div>
                <dt>結構</dt>
                <dd>{suggestion.structure}</dd>
              </div>
            </dl>
            {suggestion.language_focus.length > 0 && (
              <ul className="feature-list">
                {suggestion.language_focus.map((f) => (
                  <li key={`${f.item}-${f.paragraph_id}`}>
                    <strong lang="en">{f.item}</strong>
                    <span className="quiet-term">{f.kind}</span>
                    <span className="evidence" lang="en">
                      {f.evidence}
                    </span>
                    <span className="explanation">{f.note}</span>
                  </li>
                ))}
              </ul>
            )}
            <p className="why">採用只會填入你還沒寫的欄位，不會改掉你已經寫的。</p>
          </MannaWell>
        )}
      </section>

      <section className="stage-block" aria-labelledby="analysis-paragraphs-heading">
        <h3 id="analysis-paragraphs-heading">逐段</h3>
        {paragraphs.length === 0 && <NoMaterialNote onGoToMaterial={onGoToMaterial} />}
        {paragraphs.map((p) => {
          const mine = byParagraph.get(p.id);
          const offered = suggestion?.paragraphs.find((s) => s.paragraph_id === p.id);
          const isEditing = editing?.id === p.id;
          const missing = suggestion?.missing_paragraph_ids.includes(p.id) ?? false;
          return (
            <section key={p.id} className="sop-para" aria-label={`第 ${p.no} 段`}>
              <Quote paragraphs={[p]} />
              {isEditing && editing ? (
                <ParagraphEditor
                  paragraph={p}
                  initial={editing.initial}
                  onCancel={() => setEditing(null)}
                  onDone={(value) => {
                    const from = editing.from;
                    putParagraph({
                      ...value,
                      origin: from !== null || value.origin !== "teacher" ? "ai_edited" : "teacher",
                    });
                    if (from !== null) decide("analysis", from, "revise", `${value.function}｜${value.summary}`);
                    setEditing(null);
                  }}
                />
              ) : !isBlank(mine) && mine ? (
                <div className="mine">
                  <ParagraphAnalysisView analysis={mine} />
                  <span className="tools">
                    <button
                      type="button"
                      className="text-link is-plain"
                      aria-label={`修改第 ${p.no} 段的分析`}
                      onClick={() => setEditing({ id: p.id, from: null, initial: mine })}
                    >
                      修改
                    </button>
                    <button
                      type="button"
                      className="text-link is-plain"
                      aria-label={`刪除第 ${p.no} 段的分析`}
                      onClick={() => removeParagraph(p.id)}
                    >
                      刪除
                    </button>
                  </span>
                </div>
              ) : (
                <p className="para-links">
                  這一段還沒有分析。
                  <button
                    type="button"
                    className="text-link is-plain"
                    aria-label={`自己寫第 ${p.no} 段的分析`}
                    onClick={() =>
                      setEditing({
                        id: p.id,
                        from: null,
                        initial: { paragraph_id: p.id, function: "", summary: "", features: [], origin: "teacher" },
                      })
                    }
                  >
                    自己寫
                  </button>
                  {missing && <span>　Manna 這次沒有分析到這一段。</span>}
                </p>
              )}
              {offered && undecided(offered.paragraph_no) && !isEditing && (
                <MannaWell
                  label={`第 ${p.no} 段`}
                  onAdopt={() => {
                    putParagraph(withSuggestion(mine, offered));
                    decide("analysis", offered.paragraph_no, "adopt");
                  }}
                  onEdit={() =>
                    setEditing({ id: p.id, from: offered.paragraph_no, initial: withSuggestion(mine, offered) })
                  }
                  onDismiss={() => decide("analysis", offered.paragraph_no, "reject")}
                >
                  <dl className="well-list">
                    <div>
                      <dt>功能</dt>
                      <dd>{offered.function}</dd>
                    </div>
                    <div>
                      <dt>摘要</dt>
                      <dd>{offered.summary}</dd>
                    </div>
                  </dl>
                  {offered.features.length > 0 && (
                    <ul className="feature-list">
                      {offered.features.map((f) => (
                        <li key={f.evidence}>
                          <strong>{f.feature}</strong>
                          <span className="evidence" lang="en">
                            {f.evidence}
                          </span>
                          <span className="explanation">{f.explanation}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                  <p className="src">引文已核對是這一段原文的連續文字。</p>
                </MannaWell>
              )}
            </section>
          );
        })}
      </section>

      {orphans.length > 0 && (
        <section className="stage-block" aria-labelledby="analysis-orphans-heading">
          <h3 id="analysis-orphans-heading">原段落已變更</h3>
          <p className="quiet-note">教材改過之後，下面的分析找不到原本的段落。內容還在，要不要留由你決定。</p>
          {orphans.map((o) => (
            <div key={o.paragraph_id} className="mine">
              <ParagraphAnalysisView analysis={o} />
              <span className="tools">
                <button type="button" className="text-link is-plain" onClick={() => removeParagraph(o.paragraph_id)}>
                  刪除
                </button>
              </span>
            </div>
          ))}
        </section>
      )}
      <GenerationChoice
        sop={sop}
        heading="分析要帶到學習單嗎？"
        labels={["提供引導問題"]}
        hint="寫作特色可以變成學習單上的引導問題；表格整理與圖像組織在第 5 步選作答方式。"
      />
    </>
  );
}
