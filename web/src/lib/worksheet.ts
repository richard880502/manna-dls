// 學習單產生器 (docs/API.md Phase 2.3). The label sets are the product spec's §5, word for word:
// they must never be renamed, merged or split, and nothing may be added to them.
import type {
  ExitKind,
  InfoFunction,
  WorksheetBrief,
  WorksheetContent,
  WorksheetSettings,
  WsAudience,
  WsAutoField,
  WsBlock,
  WsBlockKind,
  WsItem,
  WsItemTable,
  WsJobStep,
  WsLevel,
  WsOrigin,
  WsScaffold,
  WsScaffoldType,
  WsSupportKind,
  WsWordBank,
} from "../api/types";
import { withOrganizerFor } from "./organizer";

/** §5.1 素材類型 */
export const MATERIAL_TYPES = ["閱讀文本", "聽力素材", "影片素材", "圖像素材", "資料／數據"] as const;

/** §5.2 學生任務: the five sub-headings only arrange the chips; every label is one flat option. */
export const STUDENT_TASK_GROUPS: ReadonlyArray<{ title: string; labels: readonly string[] }> = [
  { title: "語言與內容學習", labels: ["理解內容", "字彙練習", "文法／句型練習"] },
  { title: "思考與資訊處理", labels: ["比較／分類", "推論／預測", "評估／提出理由", "蒐集資料", "整理／分析資料"] },
  { title: "互動與合作", labels: ["討論／分享", "小組共同完成"] },
  { title: "表達與成果", labels: ["寫作", "製作成果", "成果發表"] },
  { title: "反思與評量", labels: ["學習反思", "自我評量", "同儕互評"] },
];
export const STUDENT_TASKS: readonly string[] = STUDENT_TASK_GROUPS.flatMap((g) => g.labels);

/** §5.3 作答方式 */
export const ANSWER_FORMATS = [
  "填空題",
  "選擇題",
  "是非題",
  "配對題",
  "簡答題",
  "表格整理",
  "圖像組織",
  "段落寫作",
] as const;

/** §5.4 產生設定 (the seventh label is the heading of the three version chips). */
export const GENERATION_SETTINGS = [
  "加入暖身活動",
  "顯示學習目標",
  "提供引導問題",
  "顯示作答示例",
  "提供字詞表",
  "提供句型框架",
] as const;
export const VERSIONS_HEADING = "產生基礎版／一般版／進階版";

export const LEVELS: readonly WsLevel[] = ["basic", "standard", "advanced"];
export const LEVEL_LABEL: Record<WsLevel, string> = { basic: "基礎版", standard: "一般版", advanced: "進階版" };
export const AUDIENCE_LABEL: Record<WsAudience, string> = { student: "學生版", teacher: "教師版" };

// ---------------------------------------------------------------- 差異化學習單 (docs/SIOP_DESIGN_RULES.md 三)

/** The settings' note on the versions of a 差異化學習單 (T0, T9). */
export const TIERED_VERSIONS_NOTE =
  "差異化學習單固定做基礎版、一般版、進階版三個版本：三個版本目標與題目相同，差在提示與支持的多寡。學生版只印 A、B、C 版，不印程度名稱。";

/** The answer formats a 差異化學習單 uses (open answers only). */
export const TIERED_FORMATS = ["簡答題", "表格整理", "圖像組織", "段落寫作"] as const;

/**
 * The neutral code a version's student sheet prints (T9): 「A」「B」「C」 by its place in the
 * worksheet's `student_order` (random for a 差異化學習單) when that lists exactly its versions,
 * else among `levels`; null when there is only one version (the server's rule).
 */
export function versionCode(
  levels: readonly WsLevel[],
  level: WsLevel,
  studentOrder: readonly WsLevel[] = [],
): string | null {
  if (levels.length < 2 || !levels.includes(level)) return null;
  const sameVersions =
    studentOrder.length === levels.length && levels.every((l) => studentOrder.filter((o) => o === l).length === 1);
  const order = sameVersions ? studentOrder : levels;
  const i = order.indexOf(level);
  return i < 0 ? null : String.fromCharCode(65 + i);
}

/** What the student sheet prints: 「Version A」 (「Version」 keeps it apart from the block letters). */
export const studentVersionName = (code: string) => `Version ${code}`;

/** The code in the teacher's words: 「A 版」. */
export const versionLabel = (code: string) => `${code} 版`;

/** The support forms, from most to least (T3), in the teacher's words. */
export const SUPPORT_KIND_LABEL: Record<WsSupportKind, string> = {
  cloze: "預填克漏字",
  word_bank: "字庫",
  sentence_frame: "句型框架",
  sentence_starter: "句首",
  none: "不給支持",
};

/** The English heading printed above a version's support (the server's). */
export const SUPPORT_KIND_HEADING: Record<WsSupportKind, string> = {
  cloze: "Fill in the blanks",
  word_bank: "Word bank",
  sentence_frame: "Sentence frame",
  sentence_starter: "Start like this",
  none: "",
};

/** The forms each version may use. */
export const SUPPORT_KINDS_FOR: Record<WsLevel, readonly WsSupportKind[]> = {
  basic: ["cloze", "word_bank", "sentence_frame"],
  standard: ["word_bank", "sentence_frame", "sentence_starter"],
  advanced: ["sentence_starter", "none"],
};

export const BLOCK_TITLES = ["素材類型", "學生任務", "作答方式", "產生設定"] as const;

export const AUTO_FIELD_LABEL: Record<WsAutoField, string> = {
  material_types: "素材類型",
  student_tasks: "學生任務",
  answer_formats: "作答方式",
  generation: "產生設定",
  levels: "版本",
  minutes: "份量",
};

export const STEP_LABEL: Record<WsJobStep, string> = {
  tasks: "逐段的問題與任務",
  warmup: "暖身活動",
  scaffolds: "鷹架",
  exit_ticket: "Exit ticket",
  template: "照版型產生各區塊",
  bloom: "依 Bloom 層次出題",
  bloom_check: "檢查題目的 Bloom 層次",
};

export const SCAFFOLD_TYPE_LABEL: Record<WsScaffoldType, string> = {
  verbal: "語言鷹架",
  instructional: "教學鷹架",
  procedural: "程序鷹架",
};

export const INFO_FUNCTION_LABEL: Record<InfoFunction, string> = {
  discover: "發現資訊",
  organize: "整理資訊",
  analyze: "分析資訊",
  compare: "比較資訊",
  create: "創造資訊",
};

export const EXIT_KIND_LABEL: Record<ExitKind, string> = {
  formative: "形成性評量",
  reflection: "學生反思",
  questions: "蒐集問題",
};

export const BLOCK_KIND_LABEL: Record<WsBlockKind, string> = {
  warmup: "暖身",
  activity: "活動",
  exit_ticket: "Exit ticket",
};

export const emptySettings = (): WorksheetSettings => ({
  material_types: [],
  student_tasks: [],
  answer_formats: [],
  generation: [],
  levels: [],
  minutes: null,
  output: "worksheet",
  bloom: [],
  question_count: null,
  bloom_counts: null,
  content_objective: null,
  language_objective: null,
});

export const emptyBrief = (): WorksheetBrief => ({ tasks: [], warmup: [], vocabulary: [], exit_ticket: [] });

export const emptyWorksheetContent = (): WorksheetContent => ({
  objectives: [],
  show_objectives: true,
  blocks: [],
  levels: [],
  chosen: null,
  notices: [],
});

export function briefIsEmpty(brief: WorksheetBrief): boolean {
  return (
    brief.tasks.length === 0 &&
    brief.warmup.length === 0 &&
    brief.vocabulary.length === 0 &&
    brief.exit_ticket.length === 0
  );
}

/** Adds or removes `value`, keeping the order of `order` (the spec's order). */
export function toggleIn<T extends string>(list: readonly T[], value: T, order: readonly T[]): T[] {
  const next = list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
  return order.filter((v) => next.includes(v));
}

let idCounter = 0;
/** A short id, unique in this document for all practical purposes. */
export function newId(prefix: string): string {
  idCounter += 1;
  return `${prefix}${Date.now().toString(36)}${idCounter.toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

/** Editing something Manna produced makes it 「Manna 產生後教師改過」; the teacher's own stays theirs. */
export function editedOrigin(origin: WsOrigin): WsOrigin {
  return origin === "ai" ? "ai_edited" : origin;
}

export const isFromManna = (origin: WsOrigin) => origin !== "teacher";

/** The form each version of a new 差異化學習單 question starts with (the most each version may use). */
const NEW_TIER_KIND: Record<WsLevel, WsSupportKind> = {
  basic: "sentence_frame",
  standard: "sentence_starter",
  advanced: "none",
};

/**
 * A question the teacher adds. In a 差異化學習單 (`tiered`) it has its part in every version, so
 * the version's support form can be chosen and filled in like the generated ones.
 */
export function newItem(levels: WsLevel[], tiered = false): WsItem {
  return {
    id: newId("i"),
    answer_format: "簡答題",
    prompt: "",
    options: [],
    answer: "",
    evidence: "",
    purpose: null,
    levels: [...levels],
    origin: "teacher",
    bloom: null,
    hint: "",
    ...(tiered
      ? {
          tiers: levels.map((level) => ({
            level,
            support_kind: NEW_TIER_KIND[level],
            model_sentence: "",
            support: [],
            word_bank: [],
            answer: "",
            scoring: "",
          })),
        }
      : {}),
  };
}

// ---------------------------------------------------------------- answer formats (§5.3)

/** The formats that print their `options` (the server's `uses_choices`); a question without a
 * format (older data) prints them too. Every other format keeps them, unprinted. */
const CHOICE_FORMATS = new Set(["選擇題", "是非題", "配對題"]);
export const usesChoices = (format: string | null) => format === null || CHOICE_FORMATS.has(format);

/** The label printed before the `index`-th choice: 「(A)」「(B)」… (DOCX, PDF, the editor and the
 * preview all print it; options are stored without it). The server's `option_label`. */
export const optionLabel = (index: number) => `(${String.fromCharCode(65 + (index % 26))})`;

/**
 * A letter label at the start of a choice the model (or a teacher) wrote: 「(A) …」「（A）…」, 「A. …」
 * 「A) …」「A: …」 (a space after these) or 「A．…」「A、…」「A：…」. The server uses the same pattern
 * (`OPTION_LABEL` in api/src/worksheet/assemble.rs).
 */
const OPTION_LABEL =
  /^[ \t]*(?:[(（][ \t]*([A-Za-z])[ \t]*[)）][ \t]*|([A-Za-z])(?:[.):][ \t]+|[．、：][ \t]*))([^ \t][\s\S]*)$/;

function splitOptionLabel(text: string): { letter: string; rest: string } | null {
  const m = OPTION_LABEL.exec(text);
  const letter = m?.[1] ?? m?.[2];
  return m && letter && m[3] !== undefined ? { letter: letter.toUpperCase(), rest: m[3] } : null;
}

/** The `index`-th choice without a letter label of its own (「A. Paris」 as the first choice is
 * 「Paris」), so that it is not lettered twice. Only the label of its own position goes. */
export function withoutOptionLabel(text: string, index: number): string {
  const label = splitOptionLabel(text);
  return label && label.letter === String.fromCharCode(65 + (index % 26)) ? label.rest : text;
}

/** The `index`-th choice as printed: 「(A) Paris」. */
export const printedOption = (text: string, index: number) => `${optionLabel(index)} ${withoutOptionLabel(text, index)}`;

/**
 * The answer of a 選擇題 as the teacher version prints it: with the label of the choice it is
 * (「(B) London」), so the key matches the letters on the sheet. Any other answer, and the answer of
 * every other format, prints as it is. The server's `printed_answer`.
 */
export function printedAnswer(format: string | null, answer: string, options: readonly string[]): string {
  if (format !== "選擇題") return answer;
  const bare = (splitOptionLabel(answer)?.rest ?? answer).trim().toLowerCase();
  for (const [i, o] of options.entries()) {
    const text = withoutOptionLabel(o, i).trim();
    if (text !== "" && text.toLowerCase() === bare) return `${optionLabel(i)} ${text}`;
  }
  return answer;
}

export const TRUE_FALSE = ["True", "False"] as const;
/** Empty choices a format starts with when the question has none: the generator writes four
 * options for 選擇題 and at least three for 配對題. */
const STARTING_CHOICES: Record<string, number> = { 選擇題: 4, 配對題: 3 };

const isTrueFalse = (options: readonly string[]) =>
  options.length === 2 &&
  options[0]?.trim().toLowerCase() === "true" &&
  options[1]?.trim().toLowerCase() === "false";
const allBlank = (list: readonly string[]) => list.every((o) => o.trim() === "");

/**
 * The question in another answer format, keeping what still fits and setting aside what does not
 * (never dropping it):
 * - 選擇題／配對題 take the choices the question has; one without any (or with only True／False)
 *   gets back the choices set aside for a 是非題, else empty ones to fill in;
 * - 是非題 prints True／False; other choices are set aside in `kept_options`;
 * - every other format keeps the choices unprinted, ready for a switch back;
 * - 表格整理 needs no data of its own: [`tableOf`] gives the table the prompt names until the
 *   teacher edits it. A table the question has is kept whatever the format;
 * - 圖像組織 gets a T-chart to edit when the question has no organizer yet ([`withOrganizerFor`]);
 *   an organizer the question has is kept whatever the format.
 * The prompt, answer, evidence, hint and versions are never touched.
 */
export function withAnswerFormat(item: WsItem, format: string | null): WsItem {
  const next: WsItem = { ...item, answer_format: format };
  const kept = item.kept_options ?? [];
  if (format === "是非題") {
    if (!isTrueFalse(item.options)) {
      next.options = [...TRUE_FALSE];
      if (!allBlank(item.options)) next.kept_options = item.options;
    }
  } else if (format !== null && format in STARTING_CHOICES) {
    if (allBlank(item.options) || isTrueFalse(item.options)) {
      next.options = kept.length > 0 ? kept : Array.from({ length: STARTING_CHOICES[format] ?? 4 }, () => "");
      if (kept.length > 0) next.kept_options = [];
    }
  }
  return withOrganizerFor(next);
}

export const TABLE_ROWS = 3;
export const MAX_TABLE_COLUMNS = 6;
export const MAX_TABLE_ROWS = 12;

/**
 * The columns a prompt lists after its last colon, separated by 「/」 or 「|」 (the generator's way:
 * "Complete the table: Who? / What happened? / Why?"): two to six short headings, else null. A
 * closing "(Question 3)" is not part of the list. The same rule as the server's
 * `columns_from_prompt`, so the editor shows the table that prints.
 */
export function columnsFromPrompt(prompt: string): string[] | null {
  let s = prompt.trim();
  if (s.endsWith(")")) {
    const open = s.lastIndexOf("(");
    if (open >= 0) s = s.slice(0, open).trimEnd();
  }
  const colon = Math.max(s.lastIndexOf(":"), s.lastIndexOf("："));
  if (colon < 0) return null;
  const parts = s
    .slice(colon + 1)
    .split(/[/|／]/)
    .map((p) => p.trim().replace(/[.。]+$/, "").trim());
  const fits =
    parts.length >= 2 &&
    parts.length <= MAX_TABLE_COLUMNS &&
    parts.every((p) => p !== "" && [...p].length <= 40 && p.split(/\s+/).length <= 6);
  return fits ? parts : null;
}

const NUMBER_WORDS: Record<string, number> = { two: 2, "2": 2, three: 3, "3": 3, four: 4, "4": 4, five: 5, "5": 5, six: 6, "6": 6 };
const NAMED_COLUMNS = /\b(?:(\w+)\s+)?columns?\s*(?:(?:labell?ed|titled|called|headed|named)\b\s*[:：]?|[:：])\s*([^\n]+)/i;
const PLACEHOLDER_HEADING =
  /^(?:(?:column|col|header|heading|row|item|field|label|title|category|topic|name)\s*[#_-]?\s*\d+|column(?:\s+(?:name|header))?|header|heading|tbd|n\/?a|none|blank|untitled|\.{2,}|…+|\?+|-+|_+|x+|(?:欄位?|列|標題|欄名)\s*\d*)$/i;

/** A heading or row label that says nothing: empty, "Column 1", "Header", "Row 2", "…". The same
 * rule as the server's `table::is_placeholder`. */
export function isPlaceholderHeading(text: string): boolean {
  const t = text.trim();
  return t === "" || PLACEHOLDER_HEADING.test(t);
}

/**
 * The headings a question names in its own words: "Complete a T-chart with two columns:
 * Unconventional Path as a Positive Choice and Speaker Justifying His Decision. For each …". The list
 * after "column(s)" up to the end of the sentence, split at "/", "|", ";" or commas, else at a single
 * "and"; a stated number ("two columns") must match. Two to six short headings, else null. The same
 * rule as the server's `columns_named_in_prompt`.
 */
export function columnsNamedInPrompt(prompt: string): string[] | null {
  const m = NAMED_COLUMNS.exec(prompt);
  if (!m) return null;
  const stated = m[1] ? NUMBER_WORDS[m[1].toLowerCase()] : undefined;
  let rest = (m[2] ?? "").trimEnd();
  if (rest.endsWith(")")) {
    const open = rest.lastIndexOf("(");
    if (open >= 0) rest = rest.slice(0, open).trimEnd();
  }
  const end = /[.!?](?:\s+\p{Lu}|\s*$)/u.exec(rest);
  const list = (end ? rest.slice(0, end.index) : rest).trim().replace(/[.。]+$/, "");
  let parts = list
    .split(/[/|／;；,]/)
    .map((p) => {
      const t = p.trim().replace(/^and\s+/i, "");
      return t.replace(/^["'“”‘’]+|["'“”‘’]+$/g, "").trim();
    })
    .filter((p) => p !== "");
  if (parts.length === 1) {
    const at = (parts[0] ?? "").toLowerCase().indexOf(" and ");
    if (at >= 0) parts = [(parts[0] ?? "").slice(0, at).trim(), (parts[0] ?? "").slice(at + 5).trim()];
  }
  const fits =
    parts.length >= 2 &&
    parts.length <= MAX_TABLE_COLUMNS &&
    (stated === undefined || stated === parts.length) &&
    parts.every((p) => [...p].length <= 60 && p.split(/\s+/).length <= 8 && !isPlaceholderHeading(p));
  return fits ? parts : null;
}

/** The headings the question lists ("…: Who? / Why?") or names ("two columns: A and B"), else null. */
export const namedColumns = (prompt: string): string[] | null => columnsFromPrompt(prompt) ?? columnsNamedInPrompt(prompt);

/**
 * Placeholder headings ("Column 1") never print: the headings the question names take their place,
 * else only the placeholders go (empty ones are the teacher's own and stay). The same rule as the
 * server's `ItemTable::without_placeholders`.
 */
export function withoutPlaceholders(columns: string[], prompt: string): string[] {
  const bad = (c: string) => c.trim() !== "" && isPlaceholderHeading(c);
  if (!columns.some(bad)) return columns;
  return namedColumns(prompt) ?? columns.map((c) => (bad(c) ? "" : c));
}

/** The table a 表格整理 question prints: its own, else the one its prompt names, else two columns
 * without headings and three rows (what the sheet has always printed). A placeholder heading an
 * older worksheet carries is shown the way it prints ([`withoutPlaceholders`]). */
export function tableOf(item: Pick<WsItem, "table" | "prompt">): WsItemTable {
  const own = item.table ?? { columns: namedColumns(item.prompt) ?? ["", ""], rows: TABLE_ROWS };
  return { ...own, columns: withoutPlaceholders(own.columns, item.prompt) };
}

/** Whether a table prints a first column of row labels. */
export const hasRowLabels = (t: Pick<WsItemTable, "row_labels">): boolean => (t.row_labels ?? []).length > 0;

// ---------------------------------------------------------------- 差異化學習單 support (T3, T4)

/** Words a sentence frame keeps: the frame of the sentence (pronouns, be and the helping verbs,
 * linking and question words, prepositions, the verbs of an opinion, comparison words). The rest
 * are what the student writes. */
const FRAME_WORDS = new Set(
  (
    "i you he she it we they me him us them my your his her its our their this that these those " +
    "am is are was were be been being do does did have has had will would can could should shall may might must " +
    "not don't doesn't didn't can't won't isn't aren't wasn't weren't " +
    "and but or so because although though however therefore when while if since after before then " +
    "first next finally also than as like how why what who whom whose where which " +
    "to of in on at for with from by about into " +
    "think thinks thought feel feels felt believe believes believed agree agrees disagree disagrees " +
    "more less most both same different similar"
  ).split(" "),
);
const ARTICLES = new Set(["a", "an", "the"]);
/** Words a sentence starter does not end on. */
const STARTER_TAIL = new Set([
  ...ARTICLES,
  ..."my your his her its our their this these those to of in on at for with from by about into".split(" "),
]);

/** A word of a sentence with the punctuation around it. */
interface Token {
  lead: string;
  core: string;
  trail: string;
}

function tokensOf(text: string): Token[] {
  return text
    .trim()
    .split(/\s+/)
    .filter((t) => t !== "")
    .map((t) => {
      const m = /^([^\p{L}\p{N}]*)(.*?)([^\p{L}\p{N}]*)$/u.exec(t);
      return { lead: m?.[1] ?? "", core: m?.[2] ?? t, trail: m?.[3] ?? "" };
    });
}

const lower = (s: string) => s.toLowerCase();
const isArticle = (t: Token | undefined) => t !== undefined && ARTICLES.has(lower(t.core));
/** A word the student writes (not part of the frame). */
const isContent = (t: Token | undefined) =>
  t !== undefined && /[\p{L}\p{N}]/u.test(t.core) && !FRAME_WORDS.has(lower(t.core)) && !isArticle(t);

/** Which blank each word goes into (`null`: printed as it is). */
type Blanks = Array<number | null>;
/** Punctuation after a word that ends a blank (a full stop does not: 「Mrs. Lin」 is one blank). */
const ENDS_BLANK = /[,;:!?()"“”‘’—–]/;

/** The printed blanks: the words each 「___」 stands for (consecutive words of the same blank). */
function runsOf(toks: Token[], blanks: Blanks): number[][] {
  const runs: number[][] = [];
  toks.forEach((_, i) => {
    const b = blanks[i] ?? null;
    if (b === null) return;
    const run = runs[runs.length - 1];
    if (run && run[run.length - 1] === i - 1 && blanks[i - 1] === b && !ENDS_BLANK.test(toks[i - 1]?.trail ?? "")) {
      run.push(i);
    } else runs.push([i]);
  });
  return runs;
}

/** The sentence with each blank as one 「___」, keeping the punctuation around it. */
function render(toks: Token[], blanks: Blanks): string {
  const runs = new Map(runsOf(toks, blanks).map((run) => [run[0], run[run.length - 1] ?? run[0]]));
  const out: string[] = [];
  for (let i = 0; i < toks.length; ) {
    const t = toks[i] as Token;
    const end = runs.get(i);
    if (end === undefined) {
      out.push(`${t.lead}${t.core}${t.trail}`);
      i += 1;
    } else {
      out.push(`${t.lead}___${toks[end]?.trail ?? ""}`);
      i = end + 1;
    }
  }
  return out.join(" ");
}

/** What the blanks took out, one entry per blank (without a leading article). */
function takenOut(toks: Token[], blanks: Blanks): string[] {
  return uniqueWords(
    runsOf(toks, blanks).map((run) => {
      let words = run.map((i) => toks[i] as Token);
      while (words.length > 1 && isArticle(words[0])) words = words.slice(1);
      if (words.every((t) => isArticle(t))) return "";
      return words.map((t, k) => (k === words.length - 1 ? t.core : `${t.core}${t.trail}`)).join(" ");
    }),
  );
}

const uniqueWords = (words: readonly string[]) => {
  const seen = new Set<string>();
  return words.filter((w) => {
    const k = lower(w);
    if (w === "" || seen.has(k)) return false;
    seen.add(k);
    return true;
  });
};

/** Every occurrence of a word-bank entry (a word or a phrase) as a blank of its own. */
function bankBlanks(toks: Token[], bank: readonly string[]): Blanks {
  const blanks: Blanks = toks.map(() => null);
  const entries = bank
    .map((w) => tokensOf(w).map((t) => lower(t.core)))
    .filter((e) => e.length > 0 && e.every((w) => w !== ""))
    .sort((a, b) => b.length - a.length);
  let next = 0;
  for (let i = 0; i < toks.length; ) {
    const entry = entries.find((e) => e.every((w, k) => lower(toks[i + k]?.core ?? "") === w));
    if (!entry) {
      i += 1;
      continue;
    }
    for (let k = 0; k < entry.length; k += 1) blanks[i + k] = next;
    next += 1;
    i += entry.length;
  }
  return blanks;
}

/** A cloze's own key words when the bank names none of the sentence's words: its longest content
 * words, one for every three (one to three), each a blank of its own. */
function keyWordBlanks(toks: Token[]): Blanks {
  const content = toks.map((t, i) => ({ t, i })).filter(({ t }) => isContent(t));
  const n = Math.min(3, Math.max(1, Math.round(content.length / 3)));
  const keys = content
    .sort((a, b) => [...b.t.core].length - [...a.t.core].length || a.i - b.i)
    .slice(0, n)
    .map(({ i }) => i);
  return toks.map((_, i) => (keys.includes(i) ? i : null));
}

/** A starter: the opening words up to the first content word from the third word on (at most
 * six), not ending on an article or a preposition, then 「…」. */
function starterOf(toks: Token[]): string {
  let end = toks.findIndex((t, i) => i >= 2 && isContent(t));
  if (end < 0) end = Math.max(1, toks.length - 1);
  let words = toks.slice(0, Math.min(end, 6));
  while (words.length > 1 && STARTER_TAIL.has(lower(words[words.length - 1]?.core ?? ""))) words = words.slice(0, -1);
  if (words.length < 2 && toks.length > 2) words = toks.slice(0, 2);
  const last = words.length - 1;
  return `${words.map((t, i) => (i === last ? `${t.lead}${t.core}` : `${t.lead}${t.core}${t.trail}`)).join(" ")} …`;
}

export type SupportBuild =
  | { ok: true; support: string[]; word_bank: string[] }
  /** model_sentence: the form is cut from a model sentence the version does not have; words: a
   * word bank without words and without a model sentence to take them from. */
  | { ok: false; missing: "model_sentence" | "words" };

/**
 * One version's support in another form, made from its model sentence and word bank
 * (docs/SIOP_DESIGN_RULES.md T3, T4). Deterministic: the same sentence and bank always give the
 * same lines. The editor stores the result in the version, so the preview, the printed sheet and
 * the exports print exactly these lines; the teacher can still change them by hand.
 * - 預填克漏字 (most support): the model sentence with each word-bank entry blanked; when the bank
 *   names none of its words, its one to three longest words, which join the bank.
 * - 字庫: the bank, over the answer lines (from the model sentence's words when the bank is empty).
 * - 句型框架: the model sentence with the target words blanked, keeping its frame (pronouns, the
 *   helping verbs, linking, question and comparison words, prepositions, think／feel): the bank's
 *   words and every other word, each run one 「___」. With no bank, what was taken out makes it (T4).
 * - 句首: the opening words of the model sentence. The bank stays (a starter does not print it).
 * - 不給支持: nothing to make (null); the lines stay, unprinted.
 */
export function buildSupport(
  kind: WsSupportKind,
  modelSentence: string,
  wordBank: readonly string[],
): SupportBuild | null {
  if (kind === "none") return null;
  const bank = uniqueWords(wordBank.map((w) => w.trim()));
  const toks = tokensOf(modelSentence);
  if (kind === "word_bank") {
    const words = bank.length > 0 ? bank : uniqueWords(toks.filter(isContent).map((t) => t.core)).slice(0, 8);
    return words.length > 0 ? { ok: true, support: [], word_bank: words } : { ok: false, missing: "words" };
  }
  if (toks.length === 0) return { ok: false, missing: "model_sentence" };
  if (kind === "sentence_starter") return { ok: true, support: [starterOf(toks)], word_bank: bank };
  const fromBank = bankBlanks(toks, bank);
  const cloze = (): SupportBuild => {
    if (fromBank.some((b) => b !== null)) return { ok: true, support: [render(toks, fromBank)], word_bank: bank };
    const keys = keyWordBlanks(toks);
    return { ok: true, support: [render(toks, keys)], word_bank: uniqueWords([...takenOut(toks, keys), ...bank]) };
  };
  if (kind === "cloze") return cloze();
  // 句型框架: one blank for each run of words the student writes; an article goes with its noun.
  const frame: Blanks = toks.map((t, i) =>
    fromBank[i] !== null || isContent(t) || (isArticle(t) && (isContent(toks[i + 1]) || (fromBank[i + 1] ?? null) !== null))
      ? 0
      : null,
  );
  // A sentence with nothing of a frame to keep (or nothing to blank) is cut like a cloze.
  if (frame.every((b) => b !== null) || frame.every((b) => b === null)) return cloze();
  return {
    ok: true,
    support: [render(toks, frame)],
    word_bank: bank.length > 0 ? bank : takenOut(toks, frame).slice(0, 10),
  };
}

export function newBlock(levels: WsLevel[]): WsBlock {
  return {
    id: newId("b"),
    kind: "activity",
    enabled: true,
    title: "",
    instructions: "",
    paragraph_ids: [],
    info_function: null,
    student_tasks: [],
    objective_ids: [],
    vocabulary: [],
    items: [newItem(levels)],
    scaffolds: [],
    teacher_notes: "",
    origin: "teacher",
    message: null,
    sop_block_id: null,
  };
}

const LINES: Record<WsLevel, number> = { basic: 2, standard: 3, advanced: 4 };

/**
 * Writing lines under a question, as the server prints them (`answer_lines`): 簡答題 (and a
 * question without a format) by the version, 段落寫作 two more. The other formats are answered
 * on the choices, in the blanks, in the table (表格整理) or in the box (圖像組織).
 */
export function answerLines(item: Pick<WsItem, "answer_format">, level: WsLevel): number {
  if (item.answer_format === null || item.answer_format === "簡答題") return LINES[level];
  if (item.answer_format === "段落寫作") return LINES[level] + 2;
  return 0;
}

export interface SheetItem {
  item: WsItem;
  /** Continuous through the sheet, from 1. */
  no: number;
  lines: number;
}

export interface SheetBlock {
  block: WsBlock;
  /** "A", "B", … for the warm-up and the activity blocks that print; "" for the exit ticket and
   * for a block that does not print. */
  label: string;
  items: SheetItem[];
  scaffolds: WsScaffold[];
  /** False when the block has nothing to print in this version besides its heading (e.g. a step
   * that did not succeed): the editor still shows it, the printed sheet and exports leave it out. */
  printed: boolean;
}

export interface ProjectedSheet {
  level: WsLevel;
  audience: WsAudience;
  objectives: string[];
  blocks: SheetBlock[];
}

const letter = (i: number) => (i < 26 ? String.fromCharCode(65 + i) : `A${String.fromCharCode(65 + (i % 26))}`);

/** Whether a block has anything to print in one version besides its heading (the server's rule). */
function printable(block: WsBlock, items: SheetItem[], scaffolds: WsScaffold[], audience: WsAudience): boolean {
  return (
    items.length > 0 ||
    scaffolds.some((s) =>
      s.type === "procedural" ? s.steps.some((step) => step.trim() !== "") : s.content.trim() !== "",
    ) ||
    block.vocabulary.some((v) => v.word.trim() !== "") ||
    block.instructions.trim() !== "" ||
    (audience === "teacher" && block.teacher_notes.trim() !== "")
  );
}

/**
 * What one version prints, the way the server assembles it: enabled blocks in order, the items
 * and scaffolds of that version, letters for the warm-up and activity blocks that print and one
 * running item number. Blocks with nothing to print stay in the list (the editor shows them) with
 * `printed: false` and no letter. `audience` is carried along; whether answers are drawn is the
 * view's decision (the student view never renders them).
 */
export function projectSheet(content: WorksheetContent, level: WsLevel, audience: WsAudience): ProjectedSheet {
  let no = 0;
  let lettered = 0;
  const blocks: SheetBlock[] = [];
  for (const block of content.blocks) {
    if (!block.enabled) continue;
    const items = block.items
      .filter((it) => it.levels.includes(level))
      .map((item) => ({ item, no: ++no, lines: answerLines(item, level) }));
    // Steps Manna put on an exit ticket do not print (mirrors `assemble::exit_ticket_default_steps`).
    const scaffolds = block.scaffolds.filter(
      (s) =>
        s.levels.includes(level) && !(block.kind === "exit_ticket" && s.type === "procedural" && s.origin === "ai"),
    );
    const printed = printable(block, items, scaffolds, audience);
    const label = block.kind === "exit_ticket" || !printed ? "" : letter(lettered++);
    blocks.push({ block, label, items, scaffolds, printed });
  }
  return {
    level,
    audience,
    objectives: content.show_objectives
      ? content.objectives.map((o) => o.student_facing_en.trim()).filter((t) => t !== "")
      : [],
    blocks,
  };
}

/** Printed above a block's numbered steps (mirrors `assemble::STEPS_LEAD`). */
export const STEPS_LEAD = "Follow the steps below.";

/** A block's own number of a question: the `n` of its id `<block>-i<n>` (mirrors `assemble::own_number`). */
export function ownNumber(itemId: string): number | null {
  const at = itemId.lastIndexOf("-i");
  if (at < 0) return null;
  const n = Number(itemId.slice(at + 2));
  return Number.isInteger(n) && n > 0 && /^\d+$/.test(itemId.slice(at + 2)) ? n : null;
}

/** A block's own question numbers and the numbers one version prints for them. */
export function printedNumbers(items: ReadonlyArray<{ item: { id: string }; no: number }>): Map<number, number> {
  const map = new Map<number, number>();
  for (const { item, no } of items) {
    const own = ownNumber(item.id);
    if (own !== null) map.set(own, no);
  }
  return map;
}

const ITEM_REF =
  /\b(items?|questions?|q)(\s*#?\s*)(\d{1,2}(?:\s*(?:,\s*(?:and\s+|or\s+)?|and|or|&|–|—|-|to)\s*\d{1,2})*)\b/gi;

/**
 * The text as the sheet prints it: 「item 2」 of a block is the question its version numbers 5
 * (mirrors `assemble::renumber_refs`). A number with no question of the block stays.
 */
export function renumberRefs(text: string, printed: ReadonlyMap<number, number>): string {
  if (printed.size === 0 || [...printed].every(([own, no]) => own === no)) return text;
  return text.replace(ITEM_REF, (_m, word: string, gap: string, numbers: string) =>
    `${word}${gap}${numbers.replace(/\d{1,2}/g, (d) => String(printed.get(Number(d)) ?? d))}`,
  );
}

/** The name of a block in the teacher's words, e.g. 「A 暖身」. */
export function blockName(block: WsBlock, label: string): string {
  const kind = BLOCK_KIND_LABEL[block.kind];
  return label ? `${label} ${kind}` : kind;
}

export function moveBlock(blocks: WsBlock[], id: string, toIndex: number): WsBlock[] {
  const from = blocks.findIndex((b) => b.id === id);
  if (from < 0 || toIndex < 0 || toIndex >= blocks.length || from === toIndex) return blocks;
  const next = [...blocks];
  const [moved] = next.splice(from, 1);
  if (!moved) return blocks;
  next.splice(toIndex, 0, moved);
  return next;
}

export function formatClock(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getMonth() + 1}/${d.getDate()} ${formatClock(iso)}`;
}

export const percent = (ratio: number) => `${Math.round(ratio * 1000) / 10}%`;

/**
 * Whether `level` prints the questions' own sentence frames (mirrors `assemble::frame_shown`):
 * 基礎版 and 一般版, and 進階版 only when it is the only kind of version.
 */
export function frameShown(level: WsLevel, levels: readonly WsLevel[]): boolean {
  return level !== "advanced" || !levels.some((l) => l !== "advanced");
}

// ---------------------------------------------------------------- Key Vocabulary

/** The parts of speech a row may carry (the server keeps the same set). */
export const POS_OPTIONS = ["n.", "v.", "adj.", "adv.", "prep.", "conj.", "pron.", "det.", "aux.", "interj.", "phr."] as const;

/** One row of Key Vocabulary as every printed form shows it. */
export interface KeyWordRow {
  /** Unique within the table. */
  id: string;
  word: string;
  pos: string;
  /** Academic column (the task words of the instructions); otherwise Content. */
  task: boolean;
  meaning: string;
}

/** The two groups of Key Vocabulary, a group without rows left out. */
export function keyVocabularyGroups<T extends { task: boolean }>(rows: readonly T[]): Array<{ title: "Content" | "Academic"; rows: T[] }> {
  return [
    { title: "Content" as const, rows: rows.filter((r) => !r.task) },
    { title: "Academic" as const, rows: rows.filter((r) => r.task) },
  ].filter((g) => g.rows.length > 0);
}

/** Key Vocabulary as plain text, for 複製. */
export function keyVocabularyText(rows: readonly KeyWordRow[]): string[] {
  if (rows.length === 0) return [];
  const lines = ["Key Vocabulary"];
  keyVocabularyGroups(rows).forEach((g) => {
    lines.push(g.title);
    g.rows.forEach((r) => lines.push(`  ${r.word}${r.pos ? ` (${r.pos})` : ""}: ${r.meaning}`));
  });
  return lines;
}

/** The meaning of a row in the worksheet's language: the Chinese one falls back to the English one. */
export function vocabMeaning(v: { definition: string; meaning_zh?: string }, lang: "en" | "zh" | undefined): string {
  const en = v.definition.trim();
  const zh = (v.meaning_zh ?? "").trim();
  return lang === "zh" && zh ? zh : en || zh;
}

/** The words of a word bank's text (comma separated), the way the server reads it; null for anything else. */
export function parseWordList(text: string): string[] | null {
  let t = text.trim();
  if (/^word bank:/i.test(t)) t = t.slice(10).trim();
  if (t === "" || /[:：]/.test(t)) return null;
  const words: string[] = [];
  for (const raw of t.split(/[,，、;；\n/]/)) {
    const w = raw.trim().replace(/\.$/, "").trim();
    if (w === "") continue;
    if (!/^[A-Za-z' ’-]+$/.test(w) || w.split(/\s+/).length > 4 || w.length > 40) return null;
    if (!words.some((x) => x.toLowerCase() === w.toLowerCase())) words.push(w);
  }
  // One long entry is a sentence, not a list.
  if (words.length === 1 && words[0]!.split(/\s+/).length > 2) return null;
  return words.length > 0 && words.length <= 40 ? words : null;
}

/** A word bank after its text was edited: the words still listed keep what is known about them. */
export function bankWithText(bank: WsWordBank | undefined, text: string): WsWordBank | undefined {
  const words = parseWordList(text);
  if (!words) return undefined;
  const old = bank?.words ?? [];
  return {
    ...(bank?.lang ? { lang: bank.lang } : {}),
    words: words.map((word) => {
      const known = old.find((o) => o.word.toLowerCase() === word.toLowerCase());
      return known ? { ...known, word } : { word };
    }),
  };
}

/** Whether a word bank still lacks a part of speech or any meaning (the server fills them). */
export const bankNeedsFill = (bank: WsWordBank | undefined): boolean =>
  (bank?.words ?? []).some((w) => !(w.pos ?? "").trim() || !((w.definition ?? "").trim() || (w.meaning_zh ?? "").trim()));
