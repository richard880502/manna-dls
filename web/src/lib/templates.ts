// 我的學習單版型 (docs/WORKSHEET_TEMPLATES_CONTRACT.md): what the template screens say and the
// small rules they check before sending. The server validates again; the fixed labels are the
// worksheet spec's, never renamed.
import type {
  Material,
  MaterialLibrary,
  WsTemplateSection,
  WsTemplateSummary,
  WsTemplateSupport,
  WsTemplateSupportRole,
} from "../api/types";

/** How often a template being read is asked for. Tests shorten it. */
export const templateConfig = { pollMs: 3000 };

export const EXTRACTING_LINE = "Manna 正在讀這份學習單的設計…（約 20 秒）";
export const TEMPLATES_INTRO = "上傳一份你做過的學習單，Manna 學它的設計，之後新課可以照這個版型產生。";
export const NO_TEXT_MESSAGE = "這份檔案讀不到文字（可能是掃描檔），請換一份有文字的檔案。";
export const NO_TEMPLATE_LABEL = "不使用版型";

export const MIN_ITEMS = 1;
export const MAX_ITEMS = 12;
export const MAX_STYLE_NOTES = 8;
export const MAX_NOTE_CHARS = 200;

export const templateTitle = (t: { title: string }) => t.title.trim() || "未命名的版型";

/** What a support box of the sample is for, in the teacher's words. */
export const SUPPORT_ROLE_LABEL: Record<WsTemplateSupportRole, string> = {
  directions: "作答步驟",
  sentence_frame: "句型框架",
  sharing_frame: "分享句型",
  word_bank: "字詞表",
  hints: "提示",
  guiding_questions: "引導問題",
  example: "作答示例",
};

/** 「Sentence Frame for Sharing（分享句型）」: a support box as the template page lists it. */
export const supportLine = (s: WsTemplateSupport) => `${s.heading}（${SUPPORT_ROLE_LABEL[s.role] ?? s.role}）`;

/** 「4 區 · 第 2 版」, 「讀取中」, 「沒有讀成功」: what a row says under the title. */
export function templateMeta(t: WsTemplateSummary): string {
  if (t.status === "extracting") return "讀取中";
  if (t.status === "failed") return "沒有讀成功";
  const parts = [`${t.section_count} 區`];
  if (t.version_no) parts.push(`第 ${t.version_no} 版`);
  parts.push(t.source_material ? `來自「${t.source_material.title.trim() || "未命名教材"}」` : "原本的檔案已刪除");
  return parts.join(" · ");
}

/** Only materials with text can be read as a worksheet or used as a worksheet's material. */
export function withTextOnly(library: MaterialLibrary): MaterialLibrary {
  return { ...library, materials: library.materials.filter((m: Material) => m.text_status === "ready") };
}

/** One line of the read-only skeleton: 「Paragraph [n]: Find（逐段，每段 3 題）」. */
export function sectionLine(s: WsTemplateSection): string {
  const notes: string[] = [];
  if (s.per_paragraph) notes.push("逐段");
  notes.push(`${s.per_paragraph ? "每段 " : ""}${s.item_count} 題`);
  if (s.needs_external_material) notes.push("需要你提供素材");
  return `${s.title_pattern.trim() || "（沒有標題）"}（${notes.join("，")}）`;
}

/** Style notes edited as lines: one note per non-empty line. */
export const notesFromText = (text: string): string[] =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "");

/** Why the notes cannot be saved as they are, or null. */
export function notesProblem(notes: string[]): string | null {
  if (notes.length > MAX_STYLE_NOTES) return `風格備註最多 ${MAX_STYLE_NOTES} 則，現在有 ${notes.length} 則。`;
  const long = notes.findIndex((n) => n.length > MAX_NOTE_CHARS);
  if (long >= 0) return `第 ${long + 1} 則風格備註超過 ${MAX_NOTE_CHARS} 字。`;
  return null;
}

/** Moves the element at `from` to `to` (both in range). */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const next = [...list];
  const [taken] = next.splice(from, 1);
  if (taken !== undefined) next.splice(to, 0, taken);
  return next;
}

/**
 * A block from a 需要我提供素材 section carries the server's message 「這一區需要你提供素材（…），
 * Manna 不會自己編造。」; such a block is marked so the teacher sees it at a glance.
 */
export const needsMaterial = (message: string | null | undefined): boolean =>
  typeof message === "string" && message.includes("需要你提供素材");
