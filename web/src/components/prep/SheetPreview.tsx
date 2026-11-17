import type { ReactNode } from "react";
import type { WsSheet, WsSheetBlock } from "../../api/types";
import { BLOOM_EN, BLOOM_LABEL, blockBloomLine, isBloom } from "../../lib/bloom";
import { AUDIENCE_LABEL, BLOCK_KIND_LABEL, LEVEL_LABEL } from "../../lib/worksheet";
import { BloomChip } from "../worksheet/Bloom";
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
  b.supports.forEach((s) => lines.push(`${s.heading ?? s.kind}: ${s.content}`));
  b.items.forEach((it) => {
    const tag = isBloom(it.bloom) ? ` 〔Bloom：${BLOOM_LABEL[it.bloom]} ${BLOOM_EN[it.bloom]}〕` : "";
    lines.push(`${it.no}. ${it.prompt}${tag}`);
    it.options.forEach((o) => lines.push(`   ${o}`));
    if (it.hint?.trim()) lines.push(`   Hint: ${it.hint}`);
    if (it.answer) lines.push(`   參考答案：${it.answer}`);
  });
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
}: {
  sheet: WsSheet;
  highlight: ReadonlySet<string>;
  interactionFor: (block: WsSheetBlock) => number | null;
  extras?: (block: WsSheetBlock) => SheetBlockExtras;
}) {
  const teacher = sheet.audience === "teacher";
  return (
    <article className="ws prep-sheet" lang="en" aria-label={`${LEVEL_LABEL[sheet.level]} ${AUDIENCE_LABEL[sheet.audience]}`}>
      <p className="prep-sheet-title">{sheet.title}</p>
      <p className="ws-meta">
        <span>Class ______</span>
        <span>No. ______</span>
        <span>Name ____________</span>
      </p>
      {sheet.objectives.length > 0 && (
        <section className="ws-block ws-objectives" aria-label="學習目標">
          <h3>Today I can …</h3>
          <ol>
            {sheet.objectives.map((o) => (
              <li key={o}>{o}</li>
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
                        {it.options.map((o) => (
                          <li key={o}>{o}</li>
                        ))}
                      </ul>
                    )}
                    {it.hint?.trim() && <p className="ws-hint-print">Hint: {it.hint}</p>}
                    {!teacher &&
                      Array.from({ length: it.lines }, (_, i) => <span key={i} className="ws-line" aria-hidden="true" />)}
                    {teacher && (it.answer || it.evidence) && (
                      <div className="ws-answer" lang="zh-Hant">
                        {it.answer && (
                          <div className="ws-answer-row">
                            <span>參考答案</span>
                            <span lang="en">{it.answer}</span>
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
            {teacher && b.teacher_notes?.trim() && (
              <div className="ws-answer ws-notes" lang="zh-Hant">
                <div className="ws-answer-row">
                  <span>教師備註</span>
                  <span>{b.teacher_notes}</span>
                </div>
              </div>
            )}
            {extra.below}
            <div lang="zh-Hant">
              <SectionTools name={`區塊 ${name}`} text={sheetBlockText(b)} interactionId={interactionFor(b)}>
                {extra.tools}
              </SectionTools>
            </div>
          </section>
        );
      })}
    </article>
  );
}
