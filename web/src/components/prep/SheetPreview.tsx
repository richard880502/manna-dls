import type { ReactNode } from "react";
import type { WsSheet, WsSheetBlock } from "../../api/types";
import { BLOOM_EN, BLOOM_LABEL, blockBloomLine, isBloom } from "../../lib/bloom";
import {
  AUDIENCE_LABEL,
  BLOCK_KIND_LABEL,
  LEVEL_LABEL,
  printedAnswer,
  printedOption,
  studentVersionName,
  versionLabel,
} from "../../lib/worksheet";
import { BloomChip } from "../worksheet/Bloom";
import { GlossaryBox, glossText } from "../worksheet/Glossary";
import { OrganizerView } from "../worksheet/Organizer";
import { organizerText } from "../../lib/organizer";
import { printedObjective } from "../../lib/objectives";
import { SlotPicture, viewOfSheetImage, type PictureActions } from "../worksheet/SlotPicture";
import { SectionTools } from "./SectionTools";

/** A block's name in the teacher's words: 「A 暖身」, 「Exit ticket」. */
export const sheetBlockName = (b: Pick<WsSheetBlock, "kind" | "label">) =>
  b.label ? `${b.label} ${BLOCK_KIND_LABEL[b.kind]}` : BLOCK_KIND_LABEL[b.kind];

/** One block as plain text, for 複製. */
export function sheetBlockText(b: WsSheetBlock): string {
  const lines: string[] = [`${b.label ? `${b.label}. ` : ""}${b.title}`];
  if (b.instructions.trim()) lines.push(b.instructions);
  b.steps.forEach((s, i) => lines.push(`${i + 1}. ${s}`));
  b.vocabulary.forEach((v) => lines.push(`${v.word}: ${v.definition}`));
  b.images?.forEach((img) => {
    if (img.caption.trim()) lines.push(`（圖）${img.caption}`);
  });
  if (b.original_text?.length) lines.push("Original text:", ...b.original_text);
  b.supports.forEach((s) => lines.push(`${s.heading ?? s.kind}: ${s.content}`));
  b.items.forEach((it) => {
    const tag = isBloom(it.bloom) ? ` 〔Bloom：${BLOOM_LABEL[it.bloom]} ${BLOOM_EN[it.bloom]}〕` : "";
    lines.push(`${it.no}. ${it.prompt}${tag}`);
    it.options.forEach((o, i) => lines.push(`   ${printedOption(o, i)}`));
    if (it.table?.columns.some((c) => c.trim() !== "")) lines.push(`   | ${it.table.columns.join(" | ")} |`);
    it.table?.row_labels?.forEach((l) => l.trim() && lines.push(`   | ${l} |`));
    if (it.hint?.trim()) lines.push(`   Hint: ${it.hint}`);
    if (it.frame?.trim()) lines.push(`   Sentence frame: ${it.frame}`);
    if (it.support) {
      const [first = "", ...rest] = it.support.lines;
      lines.push(`   ${it.support.heading}: ${it.support.kind === "word_bank" ? it.support.word_bank.join(" / ") : first}`);
      rest.forEach((l) => lines.push(`   ${l}`));
      if (it.support.kind !== "word_bank" && it.support.word_bank.length > 0)
        lines.push(`   Word bank: ${it.support.word_bank.join(" / ")}`);
    }
    if (it.organizer) lines.push(...organizerText(it.organizer));
    if (it.answer) lines.push(`   參考答案：${printedAnswer(it.answer_format, it.answer, it.options)}`);
    if (it.scoring) lines.push(`   評分說明：${it.scoring}`);
  });
  if (b.glosses?.length) lines.push("Glossary:", ...b.glosses.map((g) => `${g.word} ${glossText(g)}`));
  if (b.teacher_notes?.trim()) lines.push(`教師備註：${b.teacher_notes}`);
  return lines.filter((l) => l.trim() !== "").join("\n");
}

/** What a page adds to a block of the preview: more tools, and what goes under the block. */
export interface SheetBlockExtras {
  tools?: ReactNode;
  below?: ReactNode;
  /** The block is the one the bottom bar will revise. */
  targeted?: boolean;
}

/**
 * The printed sheet, read-only, as the server assembles it (the same projection the exports use).
 * Each block is a section with 複製 and 有幫助／不合用; changed blocks can be highlighted. Blocks,
 * questions and the texts a selection is measured in carry `data-block`, `data-item` and
 * `data-field`, so a selection can be turned into a note ([`selectionOnSheet`]).
 */
export function SheetPreview({
  sheet,
  highlight,
  interactionFor,
  extras,
  pictures = null,
  readOnly,
}: {
  sheet: WsSheet;
  highlight: ReadonlySet<string>;
  interactionFor: (block: WsSheetBlock) => number | null;
  extras?: (block: WsSheetBlock) => SheetBlockExtras;
  /** The worksheet's pictures and what the teacher can do with them; null: none shown. */
  pictures?: PictureActions | null;
  /**
   * Somebody else's worksheet (社群): no copy or rating tools, and its pictures are read through
   * `load` (the sheet's own worksheet id and how to fetch one picture; stable between renders).
   */
  readOnly?: { worksheetId: number; load: (imageId: number) => Promise<Blob> };
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
        const extra = extras?.(b) ?? {};
        return (
          <section
            key={b.id}
            id={`sheet-block-${b.id}`}
            data-block={b.id}
            className={`ws-block prep-sheet-block${highlight.has(b.id) ? " is-changed" : ""}${extra.targeted ? " is-targeted" : ""}`}
            aria-label={`區塊 ${name}`}
          >
            <h3 className="ws-block-head">
              {b.label && <span className="ws-letter">{b.label}.</span>}
              <span data-field="title">{b.title}</span>
            </h3>
            {bloomLine && (
              <p className="ws-bloom-line" lang="zh-Hant">
                {bloomLine}
              </p>
            )}
            {b.instructions.trim() && (
              <p className="prep-sheet-steps" data-field="instructions">
                {b.instructions}
              </p>
            )}
            {readOnly &&
              (b.images ?? []).map((img) => (
                <SlotPicture
                  key={img.slot_id}
                  worksheetId={readOnly.worksheetId}
                  name={name}
                  teacher={false}
                  view={viewOfSheetImage(img, undefined)}
                  actions={null}
                  load={readOnly.load}
                />
              ))}
            {pictures &&
              (b.images ?? []).map((img) => (
                <SlotPicture
                  key={img.slot_id}
                  worksheetId={pictures.worksheetId}
                  name={name}
                  teacher={teacher}
                  view={viewOfSheetImage(
                    img,
                    pictures.images?.slots.find((s) => s.slot_id === img.slot_id),
                  )}
                  actions={pictures}
                />
              ))}
            {b.steps.length > 0 && (
              <div className="ws-procedure">
                <ol>
                  {b.steps.map((s, i) => (
                    // oxlint-disable-next-line react/no-array-index-key -- plain strings in print order
                    <li key={i}>{s}</li>
                  ))}
                </ol>
              </div>
            )}
            {b.vocabulary.length > 0 && (
              <table className="ws-vocab" aria-label={`字詞表 ${name}`}>
                <thead>
                  <tr>
                    <th scope="col">Word</th>
                    <th scope="col">Meaning</th>
                  </tr>
                </thead>
                <tbody>
                  {b.vocabulary.map((v) => (
                    <tr key={v.word}>
                      <td>{v.word}</td>
                      <td>{v.definition}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
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
                {b.items.map((it) => (
                  <li key={it.no} value={it.no} data-item={it.id}>
                    <p className="prep-pre">
                      <span data-field="item">{it.prompt}</span>
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
                    {teacher && (it.answer || it.evidence) && (
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
                    )}
                  </li>
                ))}
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
            {extra.below}
            {!readOnly && (
              <div lang="zh-Hant">
                <SectionTools name={`區塊 ${name}`} text={sheetBlockText(b)} interactionId={interactionFor(b)}>
                  {extra.tools}
                </SectionTools>
              </div>
            )}
          </section>
        );
      })}
      {(sheet.end_words ?? []).length > 0 && (
        <section className="ws-block" aria-label="字詞表（學習單最後）">
          <table className="ws-vocab" aria-label="字詞表">
            <thead>
              <tr>
                <th scope="col">Word</th>
                <th scope="col">Meaning</th>
              </tr>
            </thead>
            <tbody>
              {(sheet.end_words ?? []).map((v) => (
                <tr key={v.word}>
                  <td>{v.word}</td>
                  <td>{v.definition}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </article>
  );
}
