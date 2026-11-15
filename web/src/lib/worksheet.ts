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
  WsJobStep,
  WsLevel,
  WsOrigin,
  WsScaffold,
  WsScaffoldType,
} from "../api/types";

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
export const AUDIENCE_LABEL: Record<WsAudience, string> = { student: "學生版", teacher: "教師參考版" };

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

export function newItem(levels: WsLevel[]): WsItem {
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

/** Open answer formats get writing lines; how many depends on the version (docs/API.md). */
const OPEN_FORMATS = new Set(["簡答題", "段落寫作", "表格整理", "圖像組織"]);
const LINES: Record<WsLevel, number> = { basic: 2, standard: 3, advanced: 4 };

export function answerLines(item: Pick<WsItem, "answer_format">, level: WsLevel): number {
  if (item.answer_format === null || OPEN_FORMATS.has(item.answer_format)) {
    return item.answer_format === "段落寫作" ? LINES[level] + 2 : LINES[level];
  }
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
    const scaffolds = block.scaffolds.filter((s) => s.levels.includes(level));
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
