import type {
  AnnotationPosition,
  Material,
  MaterialAnnotation,
  MaterialDetail,
  MaterialFileRule,
  MaterialKind,
  MaterialOrigin,
  MaterialTextStatus,
  SopMaterial,
  SopParagraph,
} from "../api/types";

export const KIND_LABEL: Record<MaterialKind, string> = {
  text: "貼上的文字",
  document: "文件",
  image: "圖片",
  audio: "音訊",
  video: "影片",
  link: "網址",
};

/** Said wherever a teacher can add a material (docs/BLUEPRINT.md §13.3). */
export const UPLOAD_WARNING =
  "請不要上傳學生作業、學生姓名、成績或照片。教材只有你自己看得到：不會提供給其他教師、不會放進共用知識庫，也不會用來訓練模型。上傳前請確認你可以使用這份內容（著作權）。";

/** 1536 → 「1.5 KB」; whole numbers lose the decimal. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const text = value >= 10 || Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1);
  return `${text} ${units[unit]}`;
}

/** 「文件 · 3 段 · 1.2 MB」: what a row in the library says under the title. */
export function materialMeta(m: Material): string {
  const parts: string[] = [KIND_LABEL[m.kind]];
  if (m.text_status === "ready") parts.push(`${m.paragraph_count} 段`);
  if (m.text_status === "no_text") parts.push("沒有可選取的文字");
  if (m.text_status === "failed") parts.push("文字讀不出來");
  if (m.page_count) parts.push(`${m.page_count} 頁`);
  if (m.has_file) parts.push(formatBytes(m.size_bytes));
  if (m.annotation_count > 0) parts.push(`${m.annotation_count} 則注記`);
  return parts.join(" · ");
}

/** 「用在：L3 第 2 節、…」 or 「還沒有用在課例」. */
export function usedInText(m: Material): string {
  if (m.lessons.length === 0 && m.worksheet_count === 0) return "還沒有用在課例";
  const lessons = m.lessons.map((l) => `${l.unit_title}／${l.lesson_title}`);
  const parts = [...lessons];
  if (m.worksheet_count > 0) parts.push(`${m.worksheet_count} 份學習單`);
  return `用在：${parts.join("、")}`;
}

/** `accept` for the file input, from the server's list. */
export function acceptAttribute(rules: MaterialFileRule[]): string {
  const list = rules.map((r) => `.${r.ext}`);
  if (list.includes(".jpg")) list.push(".jpeg");
  return list.join(",");
}

/** 「文件（.txt、.docx、.pdf）每個最大 20 MB；…」 */
export function describeLimits(rules: MaterialFileRule[]): string {
  const order: MaterialKind[] = ["document", "image", "audio", "video"];
  return order
    .map((kind) => {
      const of = rules.filter((r) => r.kind === kind);
      const first = of[0];
      if (!first) return null;
      return `${KIND_LABEL[kind]}（${of.map((r) => `.${r.ext}`).join("、")}）每個最大 ${formatBytes(first.max_bytes)}`;
    })
    .filter((line): line is string => line !== null)
    .join("；");
}

/** The rule a chosen file falls under, or null when its extension is not accepted. */
export function ruleForFile(name: string, rules: MaterialFileRule[]): MaterialFileRule | null {
  const dot = name.lastIndexOf(".");
  if (dot < 0) return null;
  let ext = name.slice(dot + 1).toLowerCase();
  if (ext === "jpeg") ext = "jpg";
  return rules.find((r) => r.ext === ext) ?? null;
}

// ---------------------------------------------------------------- what the viewer shows

/** A material as the viewer needs it, whether it came from the library or from a lesson SOP. */
export interface ViewMaterial {
  id: number;
  revision: number;
  kind: MaterialKind;
  origin: MaterialOrigin;
  title: string;
  source_note: string;
  url: string | null;
  filename: string | null;
  has_file: boolean;
  text_status: MaterialTextStatus;
  text_truncated: boolean;
  page_count: number | null;
  paragraphs: SopParagraph[];
}

export function toViewMaterial(m: MaterialDetail | SopMaterial): ViewMaterial {
  const revision = "revision" in m ? m.revision : m.version_no;
  return {
    id: m.id,
    revision,
    kind: m.kind ?? "text",
    origin: m.origin ?? "paste",
    title: m.title,
    source_note: m.source_note,
    url: m.url ?? null,
    filename: m.filename ?? null,
    has_file: m.has_file ?? false,
    text_status: m.text_status ?? (m.paragraphs.length > 0 ? "ready" : "no_text"),
    text_truncated: m.text_truncated ?? false,
    page_count: m.page_count ?? null,
    paragraphs: m.paragraphs,
  };
}

// ---------------------------------------------------------------- highlights

/** A run of paragraph text and the annotations that cover it. */
export interface Segment {
  start: number;
  end: number;
  text: string;
  ids: number[];
}

/**
 * Cuts a paragraph's text at every annotation boundary. Annotations may overlap; a segment
 * lists all that cover it. Offsets are UTF-16, like `String.prototype.slice`.
 */
export function segmentsFor(text: string, spans: Array<{ id: number; start: number; end: number }>): Segment[] {
  const valid = spans
    .map((s) => ({ ...s, start: Math.max(0, s.start), end: Math.min(text.length, s.end) }))
    .filter((s) => s.start < s.end);
  const cuts = new Set<number>([0, text.length]);
  for (const s of valid) {
    cuts.add(s.start);
    cuts.add(s.end);
  }
  const points = [...cuts].sort((a, b) => a - b);
  const segments: Segment[] = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const start = points[i] as number;
    const end = points[i + 1] as number;
    if (start === end) continue;
    segments.push({
      start,
      end,
      text: text.slice(start, end),
      ids: valid.filter((s) => s.start <= start && s.end >= end).map((s) => s.id),
    });
  }
  return segments;
}

/** The annotations placed in one paragraph, as spans. Lost and (unless asked for) hidden ones are left out. */
export function spansIn(
  annotations: MaterialAnnotation[],
  paragraphId: string,
  includeHidden: boolean,
): Array<{ id: number; start: number; end: number }> {
  return annotations
    .filter((a) => a.anchor.status !== "lost" && a.anchor.paragraph_id === paragraphId && (includeHidden || !a.hidden))
    .map((a) => ({ id: a.id, start: a.anchor.start ?? 0, end: a.anchor.end ?? 0 }));
}

// ---------------------------------------------------------------- selection → position

export interface TextSelection extends AnnotationPosition {
  text: string;
}

/**
 * The browser selection as a position inside one paragraph of `root`. Paragraph text lives in
 * elements carrying `data-pid`; everything else (paragraph numbers, notes) does not count.
 * Returns "multi" when the selection covers text of more than one paragraph (an annotation
 * belongs to one), and null when nothing of the material is selected.
 */
export function selectionInParagraph(root: HTMLElement | null): TextSelection | "multi" | null {
  const selection = typeof window !== "undefined" ? window.getSelection() : null;
  if (!root || !selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  const holders = Array.from(root.querySelectorAll<HTMLElement>("[data-pid]")).filter((el) => {
    if (!range.intersectsNode(el)) return false;
    // Touching a paragraph's edge without taking a character of it does not count.
    const inside = clampToElement(range, el);
    return inside.start < inside.end;
  });
  if (holders.length === 0) return null;
  if (holders.length > 1) return "multi";
  const holder = holders[0] as HTMLElement;
  const full = holder.textContent ?? "";
  const { start, end } = snapToWords(full, clampToElement(range, holder));
  const text = full.slice(start, end);
  if (text.trim() === "") return null;
  return { paragraph_id: holder.dataset.pid ?? "", start, end, text };
}

const WORD_CHAR = /[A-Za-z0-9'’-]/;

/**
 * A selection that starts or ends inside an English word takes the whole word (a mouse drag
 * rarely lands exactly on a word boundary). Spaces at either end are dropped.
 */
export function snapToWords(text: string, range: { start: number; end: number }): { start: number; end: number } {
  let { start, end } = range;
  const isWord = (i: number) => i >= 0 && i < text.length && WORD_CHAR.test(text[i] ?? "");
  while (start < end && /\s/.test(text[start] ?? "")) start += 1;
  while (end > start && /\s/.test(text[end - 1] ?? "")) end -= 1;
  if (start >= end) return range;
  while (isWord(start - 1) && isWord(start)) start -= 1;
  while (isWord(end - 1) && isWord(end)) end += 1;
  return { start, end };
}

/** The part of `range` that lies inside `el`, as offsets into `el`'s text. */
function clampToElement(range: Range, el: HTMLElement): { start: number; end: number } {
  const length = (el.textContent ?? "").length;
  const offsetOf = (node: Node, offset: number, fallback: number): number => {
    if (!el.contains(node)) return fallback;
    const probe = document.createRange();
    probe.selectNodeContents(el);
    probe.setEnd(node, offset);
    return probe.toString().length;
  };
  return {
    start: offsetOf(range.startContainer, range.startOffset, 0),
    end: offsetOf(range.endContainer, range.endOffset, length),
  };
}

/** Clips a quote for a label: 「However, something unexp…」. */
export function clipQuote(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}
