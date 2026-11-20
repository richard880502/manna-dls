// 平台教材、我的教材、學習單範本 (library contract, "Frontend: teacher side"): the fixed lists, the
// filter chips and what a card says. The server validates the lists again; the labels are the
// contract's, word for word.
import type {
  BrowseFileType,
  MaterialBrowseQuery,
  MaterialCard,
  TemplateBrowseQuery,
  TemplateCard,
  WsInstructionLanguage,
} from "../../api/types";
import { OCR_NOT_YET, ocrOpen } from "../../lib/ocr";
import type { IconName } from "../Icon";

/** 平台教材's categories (platform materials), in this order. */
export const REFERENCE_CATEGORIES = ["學測英文", "統測英文", "課綱", "詞彙表", "考試說明", "其他"] as const;

/** 學習單範本's categories (worksheet samples and templates), in this order. */
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

export const ALL_LABEL = "全部";
export const FILTER_LABEL = "只看：";
export const NO_MATERIALS = "沒有符合的教材";
export const NO_TEMPLATES = "沒有符合的範本";
export const RESET_LABEL = "清除條件";
export const LOAD_MORE = "載入更多";

/** The picker's buttons and titles. */
export const PICK_MATERIAL = "從教材挑選";
export const PICK_TEMPLATE = "從範本挑選";
export const CHOOSE = "選用";
export const USE_TEMPLATE = "照這個範本做";
export const USE_MATERIAL = "用這篇出題";
export const VIEW_TEMPLATE = "看版型";
export const VIEW_SOURCE = "看原檔";
export const PREVIEW = "預覽";
export const COPY_TEMPLATE = "另存成我的範本";
export const UPLOAD_SAMPLE = "上傳我的學習單";

export type MaterialArea = MaterialBrowseQuery["area"];
export type TemplateArea = TemplateBrowseQuery["area"];

export const MATERIAL_AREA_LABEL: Record<MaterialArea, string> = { reference: "平台教材", mine: "我的教材" };
export const TEMPLATE_AREA_LABEL: Record<TemplateArea, string> = { platform: "平台範本", mine: "我的範本" };

/** A filter chip under 「只看：」: its words, its icon and what it sets in the query. */
export interface MaterialFilter {
  id: string;
  label: string;
  icon: IconName;
  /** File types exclude each other (the query takes one); 可直接出題 goes with any of them. */
  fileType?: BrowseFileType;
  fitsLesson?: true;
}

export const MATERIAL_FILTERS: readonly MaterialFilter[] = [
  { id: "pdf", label: "PDF", icon: "file", fileType: "pdf" },
  { id: "docx", label: "Word", icon: "worksheet", fileType: "docx" },
  { id: "text", label: "文字", icon: "paste", fileType: "text" },
  { id: "fits", label: "有文字、可直接出題", icon: "ready", fitsLesson: true },
];

export const TEMPLATE_FILTERS: ReadonlyArray<{ id: string; label: string; icon: IconName; lang: WsInstructionLanguage }> = [
  { id: "en", label: "英文指示", icon: "speech", lang: "英文" },
  { id: "zh", label: "中文指示", icon: "speech", lang: "中文" },
];

/** The badge in a card's corner. */
export const FILE_TYPE_LABEL: Record<string, string> = {
  pdf: "PDF",
  docx: "Word",
  text: "文字",
  image: "圖片",
  audio: "音訊",
  video: "影片",
  link: "網址",
};

export const fileTypeLabel = (fileType: string): string => FILE_TYPE_LABEL[fileType] ?? fileType.toUpperCase();

/** 「約 2,900 字」: rounded, since it is a count of words read from a file. */
export function aboutWords(n: number): string {
  const rounded = n < 100 ? n : n < 1000 ? Math.round(n / 10) * 10 : Math.round(n / 100) * 100;
  return `約 ${rounded.toLocaleString("en-US")} 字`;
}

export const cardTitle = (c: { title: string }) => c.title.trim() || "未命名教材";
export const templateCardTitle = (c: { title: string }) => c.title.trim() || "未命名的範本";

/** 「PDF · 7 頁 · 約 2,900 字」, 「貼上的文字 · 6 段」, 「圖片」. */
export function materialCardMeta(c: MaterialCard): string {
  const hasText = c.text_status === "ready";
  if (c.kind === "text") return ["貼上的文字", hasText ? `${c.paragraph_count} 段` : null].filter(Boolean).join(" · ");
  const parts: string[] = [];
  if (c.kind === "document" && c.file_type === "text") parts.push("文字檔");
  else parts.push(fileTypeLabel(c.file_type));
  if (c.page_count) parts.push(`${c.page_count} 頁`);
  else if (hasText && c.paragraph_count > 0) parts.push(`${c.paragraph_count} 段`);
  if (hasText && c.word_count > 0) parts.push(aboutWords(c.word_count));
  // While Manna reads a scan, the badge says so; 「沒有可選取的文字」 would soon be wrong.
  if (c.text_status === "no_text" && !ocrOpen(c.ocr)) parts.push("沒有可選取的文字");
  if (c.text_status === "failed") parts.push("文字讀不出來");
  return parts.join(" · ");
}

/** 「英文指示」, 「中英並用指示」. */
export const langLabel = (lang: string) => (lang ? `${lang}指示` : "");

/** 「4 區 · 英文指示 · 閱讀理解」; 「讀取中」 and 「沒有讀成功」 while it is not ready. */
export function templateCardMeta(c: TemplateCard): string {
  if (c.status === "extracting") return "讀取中";
  if (c.status === "failed") return "沒有讀成功";
  return [`${c.section_count} 區`, langLabel(c.instruction_language), c.category].filter(Boolean).join(" · ");
}

/** A material can go into an article slot only with text, and not while OCR is reading it. */
export const usableMaterial = (c: Pick<MaterialCard, "text_status" | "ocr">) =>
  c.text_status === "ready" && !ocrOpen(c.ocr);

/** Why 選用 (or 用這篇出題) is off for this card, as its tooltip; undefined when it is on. */
export const unusableReason = (c: Pick<MaterialCard, "text_status" | "ocr">): string | undefined =>
  ocrOpen(c.ocr) ? OCR_NOT_YET : c.text_status === "ready" ? undefined : "讀不到文字，不能出題";

/** The categories to show as chips: the fixed list's order first, then any other key the server counted. */
export function categoryChips(
  fixed: readonly string[],
  counted: ReadonlyArray<{ key: string; count: number }>,
  active: string,
): Array<{ key: string; count: number }> {
  const count = new Map(counted.filter((f) => f.key !== "").map((f) => [f.key, f.count]));
  const keys = [...fixed.filter((k) => count.has(k) || k === active), ...[...count.keys()].filter((k) => !fixed.includes(k))];
  if (active && !keys.includes(active)) keys.push(active);
  return keys.map((key) => ({ key, count: count.get(key) ?? 0 }));
}
