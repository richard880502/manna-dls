// 教材與範本管理: the fixed lists and what a bulk upload fills in from a file's name and the folders
// it came from (docs/API.md「Phase 2.9」, "Frontend: admin side"). The server validates again.

/** 知識庫教材 (平台教材) categories; one is required. */
export const MATERIAL_CATEGORIES = ["學測英文", "統測英文", "課綱", "詞彙表", "考試說明", "其他"] as const;
/** 平台範本 (worksheet sample) categories; optional, "" = 自動判斷 (the server suggests one). */
export const SAMPLE_CATEGORIES = [
  "字彙",
  "文法句型",
  "閱讀理解",
  "課文講義",
  "自學講義",
  "寫作",
  "聽說",
  "學習反思",
  "其他",
] as const;
export const AUTO_CATEGORY_LABEL = "自動判斷";

export const MAX_TAGS = 12;
export const MAX_TAG_CHARS = 20;
export const MAX_TITLE_CHARS = 200;

/** What the server accepts for a platform material (api/src/materials/mod.rs FILE_TYPES). */
export const MATERIAL_EXTENSIONS = [
  "txt",
  "docx",
  "pdf",
  "png",
  "jpg",
  "jpeg",
  "webp",
  "gif",
  "mp3",
  "m4a",
  "wav",
  "mp4",
  "webm",
];
/** A template is read from text, so only documents. */
export const TEMPLATE_EXTENSIONS = ["txt", "docx", "pdf"];

/** Polling and upload settings. Tests shorten the poll. */
export const adminLibraryConfig = { pollMs: 3000, concurrency: 2 };

/**
 * Keyword → 平台教材 category, most specific first: a 考試說明 for 學測 is a 考試說明, and a
 * 詞彙表 kept in a 學測 folder is a 詞彙表.
 */
const MATERIAL_CATEGORY_KEYWORDS: Array<[string, string[]]> = [
  ["考試說明", ["考試說明"]],
  ["課綱", ["課綱", "課程綱要", "領綱"]],
  ["詞彙表", ["詞彙"]],
  ["學測英文", ["學測", "學科能力測驗"]],
  ["統測英文", ["統測", "統一入學測驗"]],
];

/** Textbook publishers, kept as tags. */
const PUBLISHERS = ["三民", "龍騰", "翰林", "南一", "遠東", "康軒", "全華", "東大", "育達", "泰宇", "旗立", "奇鼎"];

/** "a/b/c.pdf" → ["c.pdf", "b", "a"]: the file name first, then its folders from the nearest. */
function segments(path: string): string[] {
  return path
    .split(/[\\/]+/)
    .map((s) => s.trim())
    .filter((s) => s !== "")
    .reverse();
}

export function fileNameOf(path: string): string {
  return segments(path)[0] ?? path;
}

/** The extension in lower case, without the dot ("" when there is none). */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/** Files a dropped folder carries that are never meant to be uploaded (.DS_Store, Word's ~$ locks). */
export function isSystemFile(path: string): boolean {
  const parts = segments(path);
  const name = parts[0] ?? "";
  if (name.startsWith(".") || name.startsWith("~$")) return true;
  if (/^(thumbs\.db|desktop\.ini)$/i.test(name)) return true;
  return parts.some((p) => p === "__MACOSX" || p.startsWith("."));
}

/** The default title: the file name without its extension. */
export function titleFromPath(path: string): string {
  const name = fileNameOf(path);
  const dot = name.lastIndexOf(".");
  const base = (dot > 0 ? name.slice(0, dot) : name).trim();
  return base.slice(0, MAX_TITLE_CHARS);
}

/** The 平台教材 category the path's words point to, or "" when none does. */
export function guessMaterialCategory(path: string): string {
  const text = segments(path).join("/");
  for (const [category, words] of MATERIAL_CATEGORY_KEYWORDS) {
    if (words.some((w) => text.includes(w))) return category;
  }
  return "";
}

/**
 * Tags from the path: the year (a 3-digit number like 114, from the file name before its folders),
 * 試題／答案／參考答案, the publisher, and a book-and-lesson code such as B1L5.
 */
export function guessTags(path: string): string[] {
  const parts = segments(path);
  const text = parts.join("/");
  const tags: string[] = [];
  for (const part of parts) {
    const year = /(?<![0-9])1[0-9]{2}(?![0-9])/.exec(part);
    if (year) {
      tags.push(year[0]);
      break;
    }
  }
  if (text.includes("試題")) tags.push("試題");
  if (text.includes("參考答案")) tags.push("參考答案");
  else if (text.includes("答案")) tags.push("答案");
  for (const p of PUBLISHERS) if (text.includes(p)) tags.push(p);
  const lesson = /(?<![A-Za-z])B\s?([0-9]{1,2})\s?L\s?([0-9]{1,2})(?![0-9])/i.exec(text);
  if (lesson) tags.push(`B${lesson[1]}L${lesson[2]}`);
  return [...new Set(tags)].slice(0, MAX_TAGS);
}

/** Tags as typed: separated by commas (half or full width) or 、. */
export function parseTags(text: string): string[] {
  return [
    ...new Set(
      text
        .split(/[,，、]/)
        .map((t) => t.trim())
        .filter((t) => t !== ""),
    ),
  ];
}

export const tagsText = (tags: string[]): string => tags.join("、");

/** Why these tags cannot be sent, or null. */
export function tagsProblem(tags: string[]): string | null {
  if (tags.length > MAX_TAGS) return `標籤最多 ${MAX_TAGS} 個。`;
  const long = tags.find((t) => [...t].length > MAX_TAG_CHARS);
  if (long !== undefined) return `標籤「${long}」超過 ${MAX_TAG_CHARS} 字。`;
  return null;
}

/** Why this title cannot be sent, or null. */
export function titleProblem(title: string): string | null {
  if (title.trim() === "") return "請填標題。";
  if ([...title.trim()].length > MAX_TITLE_CHARS) return `標題最多 ${MAX_TITLE_CHARS} 字。`;
  return null;
}

// ---------------------------------------------------------------- what a row says

const FILE_TYPE_LABEL: Record<string, string> = {
  pdf: "PDF",
  docx: "Word",
  text: "文字",
  image: "圖片",
  audio: "音訊",
  video: "影片",
  link: "網址",
};

export const fileTypeLabel = (t: string) => FILE_TYPE_LABEL[t] ?? t.toUpperCase();

/** 2,912 → 「約 2,900 字」; short texts keep their count. */
function aboutWords(n: number): string {
  const rounded = n >= 1000 ? Math.round(n / 100) * 100 : n;
  return `約 ${rounded.toLocaleString("en-US")} 字`;
}

/** 「PDF · 7 頁 · 約 2,900 字」, or what is wrong with the text. */
export function materialMeta(m: {
  file_type: string;
  page_count: number | null;
  paragraph_count: number;
  word_count: number;
  text_status: string;
  ocr?: { status: string };
}): string {
  const parts = [fileTypeLabel(m.file_type)];
  if (m.page_count) parts.push(`${m.page_count} 頁`);
  if (m.text_status === "ready") parts.push(m.word_count > 0 ? aboutWords(m.word_count) : `${m.paragraph_count} 段`);
  // While OCR reads a scan, its row says 「辨識中」 instead.
  const reading = m.ocr?.status === "queued" || m.ocr?.status === "running";
  if (m.text_status === "no_text" && !reading) parts.push("讀不到文字（可能是掃描檔）");
  if (m.text_status === "failed") parts.push("文字讀不出來");
  return parts.join(" · ");
}

const LANG_LABEL: Record<string, string> = { 英文: "英文指示", 中文: "中文指示", 中英並用: "中英並用" };

/** 「4 區 · 英文指示 · 閱讀理解」 */
export function templateMeta(t: { section_count: number; instruction_language: string; category: string }): string {
  const parts = [`${t.section_count} 區`];
  if (t.instruction_language) parts.push(LANG_LABEL[t.instruction_language] ?? t.instruction_language);
  parts.push(t.category || AUTO_CATEGORY_LABEL);
  return parts.join(" · ");
}
