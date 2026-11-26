import type { WsSheet, WsSheetBlock, WsSheetWord } from "../../api/types";
import { blockBloomLine, isBloom } from "../../lib/bloom";
import {
  AUDIENCE_LABEL,
  BLOCK_KIND_LABEL,
  LEVEL_LABEL,
  type KeyWordRow,
  printedAnswer,
  printedOption,
  studentVersionName,
  versionLabel,
} from "../../lib/worksheet";
import { BloomChip } from "../worksheet/Bloom";
import { KeyVocabulary } from "../worksheet/KeyVocabulary";
import { GlossaryBox } from "../worksheet/Glossary";
import { OrganizerView } from "../worksheet/Organizer";
import { printedObjective } from "../../lib/objectives";
import { SlotPicture, viewOfSheetImage } from "../worksheet/SlotPicture";

/** A block's name in the teacher's words: 「A 暖身」, 「Exit ticket」. */
const sheetBlockName = (b: Pick<WsSheetBlock, "kind" | "label">) =>
  b.label ? `${b.label} ${BLOCK_KIND_LABEL[b.kind]}` : BLOCK_KIND_LABEL[b.kind];

/** A row of the sheet's Key Vocabulary. */
const sheetWordRow = (v: WsSheetWord, i: number): KeyWordRow => ({
  id: `${v.word}-${i}`,
  word: v.word,
  pos: v.pos ?? "",
  task: v.task_word === true,
  meaning: v.definition,
});

function SheetKeyVocabulary({ words, label }: { words: WsSheetWord[]; label: string }) {
  return <KeyVocabulary label={label} rows={words.map(sheetWordRow)} />;
}

type WsSheetItem = WsSheetBlock["items"][number];

/** The answer, scoring note and evidence of one question, as the teacher sheet draws them. */
function AnswerBox({ it }: { it: WsSheetItem }) {
  if (!it.answer && !it.evidence) return null;
  return (
    <div className="ws-answer" lang="zh-Hant">
      {it.answer && (
        <div className="ws-answer-row">
          <span>參考答案</span>
          <span lang="en">{printedAnswer(it.answer_format, it.answer, it.options)}</span>
        </div>
      )}
      {it.scoring && (
        <div className="ws-answer-row">
          <span>評分說明</span>
          <span>{it.scoring}</span>
        </div>
      )}
      {it.evidence && (
        <div className="ws-answer-row">
          <span>依據（教材原文）</span>
          <span lang="en">{it.evidence}</span>
        </div>
      )}
    </div>
  );
}

/**
 * The printed sheet, read-only, as the server assembles it (the same projection the exports use).
 * Its pictures are read through `pictures` (the sheet's own worksheet id and how to fetch one
 * picture; stable between renders).
 */
export function SheetPreview({
  sheet,
  pictures,
}: {
  sheet: WsSheet;
  pictures: { worksheetId: number; load: (imageId: number) => Promise<Blob> };
}) {
  const teacher = sheet.audience === "teacher";
  // T9: a student sheet is named by its neutral code, never by the level.
  const code = sheet.version_code ?? null;
  const name =
    !teacher && code ? `${versionLabel(code)} ${AUDIENCE_LABEL.student}` : `${LEVEL_LABEL[sheet.level]} ${AUDIENCE_LABEL[sheet.audience]}`;
  return (
    <article className="ws prep-sheet" lang="en" aria-label={name}>
      <p className="prep-sheet-title">{sheet.title}</p>
      <p className="ws-meta">
        <span>Class ______</span>
        <span>No. ______</span>
        <span>Name ____________</span>
        {code && <span className="ws-version">{studentVersionName(code)}</span>}
      </p>
      {teacher && sheet.tiering_note && (
        <p className="ws-bloom-line" lang="zh-Hant">
          {sheet.tiering_note}
        </p>
      )}
      {sheet.objectives.length > 0 && (
        <section className="ws-block ws-objectives" aria-label="學習目標">
          <h3>Today I can …</h3>
          <ol>
            {sheet.objectives.map((o) => (
              <li key={o}>{printedObjective(o)}</li>
            ))}
          </ol>
        </section>
      )}
      {sheet.blocks.map((b) => {
        const name = sheetBlockName(b);
        const bloomLine = teacher ? blockBloomLine(b.items) : "";
        return (
          <section
            key={b.id}
            className="ws-block prep-sheet-block"
            aria-label={`區塊 ${name}`}
          >
            <h3 className="ws-block-head" tabIndex={-1}>
              {b.label && <span className="ws-letter">{b.label}.</span>}
              <span>{b.title}</span>
            </h3>
            {bloomLine && (
              <p className="ws-bloom-line" lang="zh-Hant">
                {bloomLine}
              </p>
            )}
            {b.instructions.trim() && (
              <p className="prep-sheet-steps">
                {b.instructions}
              </p>
            )}
            {(b.images ?? []).map((img) => (
              <SlotPicture
                key={img.slot_id}
                worksheetId={pictures.worksheetId}
                name={name}
                teacher={false}
                view={viewOfSheetImage(img, undefined)}
                actions={null}
                load={pictures.load}
              />
            ))}
            {b.steps.length > 0 && (
              <div className="ws-procedure">
                {b.steps_lead && <p className="ws-steps-lead">{b.steps_lead}</p>}
                <ol>
                  {b.steps.map((s, i) => (
                    // oxlint-disable-next-line react/no-array-index-key -- plain strings in print order
                    <li key={i}>{s}</li>
                  ))}
                </ol>
              </div>
            )}
            {b.vocabulary.length > 0 && <SheetKeyVocabulary words={b.vocabulary} label={`字詞表 ${name}`} />}
            {(b.original_text?.length ?? 0) > 0 && (
              <div className="ws-support">
                <p className="ws-support-head">Original text (read this first; the simplified text below is a help)</p>
                {b.original_text?.map((p) => (
                  <p key={p} className="prep-pre">
                    {p}
                  </p>
                ))}
              </div>
            )}
            {b.supports.map((s, i) => (
              // oxlint-disable-next-line react/no-array-index-key -- supports have no id on the sheet
              <div key={i} className="ws-support">
                <p className="ws-support-head">{s.heading ?? s.kind}</p>
                <p className="prep-pre">{s.content}</p>
              </div>
            ))}
            {b.items.length > 0 && (
              <ol className="ws-items">
                {b.items.map((it) => {
                  return (
                    <li key={it.no} value={it.no}>
                      <p className="prep-pre">
                        <span>{it.prompt}</span>
                        {/* Bloom tags: the teacher version only (the student sheet never has them). */}
                        {teacher && isBloom(it.bloom) && (
                          <>
                            {" "}
                            <BloomChip level={it.bloom} />
                          </>
                        )}
                      </p>
                      {it.options.length > 0 && (
                        <ul className="prep-sheet-options">
                          {it.options.map((o, i) => (
                            <li key={o}>{printedOption(o, i)}</li>
                          ))}
                        </ul>
                      )}
                      {it.hint?.trim() && <p className="ws-hint-print">Hint: {it.hint}</p>}
                      {it.frame?.trim() && (
                        <p className="ws-hint-print ws-frame-print" aria-label={`Sentence frame ${it.no}`}>
                          <strong>Sentence frame:</strong> {it.frame}
                        </p>
                      )}
                      {it.support && (
                        <div className="ws-hint-print ws-item-support" aria-label={`${it.support.heading} ${it.no}`}>
                          {it.support.kind === "word_bank" ? (
                            <p>
                              <strong>{it.support.heading}:</strong> {it.support.word_bank.join(" / ")}
                            </p>
                          ) : (
                            <p>
                              <strong>{it.support.heading}:</strong> {it.support.lines[0] ?? ""}
                            </p>
                          )}
                          {(it.support.kind === "word_bank" ? it.support.lines : it.support.lines.slice(1)).map((l) => (
                            <p key={l}>{l}</p>
                          ))}
                          {it.support.kind !== "word_bank" && it.support.word_bank.length > 0 && (
                            <p>
                              <strong>Word bank:</strong> {it.support.word_bank.join(" / ")}
                            </p>
                          )}
                        </div>
                      )}
                      {it.table && (
                        <table className="ws-answer-grid" aria-label={`作答表格 ${it.no}`}>
                          {it.table.columns.some((c) => c.trim() !== "") && (
                            <thead>
                              <tr>
                                {it.table.columns.map((c, i) => (
                                  // oxlint-disable-next-line react/no-array-index-key -- columns in print order
                                  <th key={i} scope="col">
                                    {c}
                                  </th>
                                ))}
                              </tr>
                            </thead>
                          )}
                          <tbody>
                            {Array.from({ length: it.table.rows }, (_, r) => (
                              // oxlint-disable-next-line react/no-array-index-key -- empty rows to write in
                              <tr key={r}>
                                {it.table?.columns.map((_, c) =>
                                  c === 0 && (it.table?.row_labels ?? []).length > 0 ? (
                                    // oxlint-disable-next-line react/no-array-index-key -- the row's label, in the first column
                                    <th key={c} scope="row">
                                      {it.table?.row_labels?.[r] ?? ""}
                                    </th>
                                  ) : (
                                    // oxlint-disable-next-line react/no-array-index-key, jsx-a11y/control-has-associated-label -- empty cells to write in
                                    <td key={c} />
                                  ),
                                )}
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      )}
                      {it.organizer ? (
                        <OrganizerView org={it.organizer} />
                      ) : (
                        it.answer_format === "圖像組織" && (
                          <div className="ws-organizer" role="img" aria-label={`圖像組織作答框 ${it.no}`} />
                        )
                      )}
                      {!teacher &&
                        Array.from({ length: it.lines }, (_, i) => <span key={i} className="ws-line" aria-hidden="true" />)}
                      {teacher && <AnswerBox it={it} />}
                    </li>
                  );
                })}
              </ol>
            )}
            <GlossaryBox glosses={b.glosses ?? []} name={name} />
            {teacher && b.teacher_notes?.trim() && (
              <div className="ws-answer ws-notes" lang="zh-Hant">
                <div className="ws-answer-row">
                  <span>教師備註</span>
                  <span>{b.teacher_notes}</span>
                </div>
              </div>
            )}
          </section>
        );
      })}
      {(sheet.end_words ?? []).length > 0 && (
        <section className="ws-block" aria-label="字詞表（學習單最後）">
          <SheetKeyVocabulary words={sheet.end_words ?? []} label="字詞表" />
        </section>
      )}
    </article>
  );
}
