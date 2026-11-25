import { printedOption, keyVocabularyText, type SheetBlock } from "./worksheet";

/** One block of the editor as plain text, for 複製 (the title, instructions, steps, words and questions). */
export function editorBlockText(view: SheetBlock): string {
  const { block } = view;
  const lines: string[] = [`${view.label ? `${view.label}. ` : ""}${block.title}`];
  if (block.instructions.trim()) lines.push(block.instructions);
  view.scaffolds.forEach((s) => {
    if (s.type === "procedural") {
      s.steps.forEach((step, i) => lines.push(`${i + 1}. ${step}`));
    } else {
      lines.push(`${s.kind}: ${s.content}`);
    }
  });
  lines.push(
    ...keyVocabularyText(
      block.vocabulary.map((v, i) => ({
        id: `${v.word}-${i}`,
        word: v.word,
        pos: v.pos ?? "",
        task: v.task_word === true,
        meaning: v.definition,
      })),
    ),
  );
  view.items.forEach(({ item, no }) => {
    lines.push(`${no}. ${item.prompt}`);
    item.options.forEach((o, i) => lines.push(`   ${printedOption(o, i)}`));
    if (item.hint?.trim()) lines.push(`   Hint: ${item.hint}`);
    if (item.frame?.trim()) lines.push(`   Sentence frame: ${item.frame}`);
  });
  return lines.filter((l) => l.trim() !== "").join("\n");
}
