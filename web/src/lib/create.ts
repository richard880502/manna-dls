// 建立 (the generator, /create/:tool) and the home page's quick box: the tools, their words, which
// settings each one takes, and the typed-keyword shortcut. Fixed labels are the contract's
// (docs: bloom-contract, "Shell and home" and Addendum 1; mimic-contract, "Frontend"), word for word,
// except the home tiles' three titles (HOME_NAME), which the PI renamed in 2026-11 (docs/DESIGN.md).
import type { CreatePrepSessionRequest } from "../api/types";

/**
 * The generator's tools, in the home page's tile order. 照我的學習單做 (mimic) comes
 * first: it is the teacher's own worksheet as the model, and makes a 完整學習單 (kind worksheet).
 */
export const TOOLS = ["mimic", "comprehension", "tiered", "activity", "kit", "worksheet"] as const;
export type Tool = (typeof TOOLS)[number];

/** The tools that are a session kind of their own (the home page's tiles follow 照我的學習單做 with these). */
export const KIND_TOOLS = ["comprehension", "tiered", "activity", "kit", "worksheet"] as const;
export type KindTool = (typeof KIND_TOOLS)[number];

/**
 * The tools the home page shows as tiles, in order. The other four stay reachable (the
 * conversation's 接下來可以 chips, typed words in the quick box, and /create/:tool).
 */
export const HOME_TOOLS = ["worksheet", "mimic", "stepwise"] as const satisfies readonly (Tool | "stepwise")[];

/**
 * 逐步設計 is the third way to prep: the seven-step lesson workspace (/lessons/:id), where the
 * teacher decides at every step. It is a home tile only, not a generator tool: it is not in TOOLS
 * and has no /create page. Choosing it opens the unit dialog instead of making a prep session.
 */
export type HomeTool = (typeof HOME_TOOLS)[number];
export const isStepwise = (tile: Tool | HomeTool | null): tile is "stepwise" => tile === "stepwise";

export const isTool = (value: string | undefined): value is Tool => (TOOLS as readonly string[]).includes(value ?? "");

/** The session kind a tool makes (照我的學習單做 makes a 完整學習單). */
export const kindOf = (tool: Tool): KindTool => (tool === "mimic" ? "worksheet" : tool);

/**
 * The contract's bare chip labels. Wire values only, never shown: they are the request text sent
 * when the teacher typed nothing, so the server can tell a bare tool choice from a real request
 * (the server recognises exactly these; do not rename them). Every visible label is TOOL_NAME.
 */
export const TOOL_CHIP: Record<KindTool, string> = {
  comprehension: "閱讀理解題",
  // Formerly 分層練習 (the server still takes the old name as a bare chip label).
  tiered: "差異化學習單",
  activity: "課堂活動",
  kit: "教學套件",
  worksheet: "學習單",
};

/** The one visible name of a tool: the home page's tiles and the generator's heading. */
export const TOOL_NAME: Record<Tool, string> = { mimic: "照我的學習單做", ...TOOL_CHIP, worksheet: "完整學習單" };

/** One line per tool: under its name on the home page's tiles. */
export const TOOL_BLURB: Record<Tool, string> = {
  mimic: "上傳你以前用過的學習單，Manna 照它的版面和樣式，為新課文做一份",
  comprehension: "依 Bloom 六個層次出題，附答案與文本依據",
  tiered: "同一篇文章，基礎／一般／進階三個版本",
  activity: "引起動機到統整的一節課流程",
  kit: "活動流程＋差異化學習單＋教師版答案，一次下載",
  worksheet: "由 Manna 設計版面：暖身、逐段任務，到 Exit ticket",
};

/** The home tiles' visible names and one-line blurbs: the generator's, and 逐步設計's own. */
export const HOME_NAME: Record<HomeTool, string> = {
  worksheet: "AI 幫我設計",
  mimic: "照我的範本設計",
  stepwise: "我想逐步設計",
};
/** The 我的作品 tag of a lesson made in the step-by-step workspace. */
export const STEPWISE_TAG = "逐步設計";
export const HOME_BLURB: Record<HomeTool, string> = {
  worksheet: TOOL_BLURB.worksheet,
  mimic: TOOL_BLURB.mimic,
  stepwise: "跟著七個步驟一步步設計，每一步由你決定，Manna 從旁建議",
};
/** 逐步設計's box: the text is optional, and only names the lesson. */
export const STEPWISE_PLACEHOLDER = "可以先寫這節課的名稱，也可以不寫，直接按送出";

/** The generator's subtitle: what the tool makes from what. */
export const TOOL_SUBTITLE: Record<Tool, string> = {
  mimic: "上傳你以前的學習單，Manna 照它的樣式為新課文出題。",
  comprehension: "附上一篇文章，寫下學習目標（可以不寫），Manna 依 Bloom 認知層次出閱讀理解題，附答案與文本依據。",
  tiered: "同一篇文章做成基礎／一般／進階三個版本。三個版本目標相同，差在提示與支持的多寡。",
  activity: "附上文章、寫下學習目標，Manna 規劃從引起動機到統整的一節課流程。",
  kit: "一次做好活動流程、差異化學習單和教師版答案，下載成一份 DOCX。",
  worksheet: "暖身、逐段任務到 Exit ticket 的完整學習單，可以照你的版型排版。",
};

/**
 * Which settings each tool takes (課堂活動: only 班級 and 學習目標). 照我的學習單做 takes only 班級
 * and 學習目標 too: its sample decides the questions.
 */
export const TOOL_FIELDS: Record<Tool, { bloom: boolean; formats: boolean; template: boolean }> = {
  mimic: { bloom: false, formats: false, template: false },
  comprehension: { bloom: true, formats: true, template: false },
  tiered: { bloom: true, formats: true, template: false },
  activity: { bloom: false, formats: false, template: false },
  kit: { bloom: true, formats: false, template: false },
  worksheet: { bloom: true, formats: true, template: true },
};

/**
 * 題數 when the teacher has not changed it. A 差異化學習單 prints every question in all three
 * versions, so it starts at one per Bloom level.
 */
export const defaultCount = (tool: Tool): number => (tool === "tiered" ? 6 : 8);

/** 題型 (fixed labels; none chosen = 自動). */
export const FORMAT_CHOICES = ["選擇題", "是非題", "配對題", "填空題", "簡答題", "表格整理", "圖像組織", "段落寫作"] as const;

/**
 * The 題型 a tool offers: a 差異化學習單 only uses open answers, where the versions' support and
 * expected answers (a phrase in 基礎版, full sentences above) mean something.
 */
export const formatChoices = (tool: Tool): readonly string[] =>
  tool === "tiered" ? FORMAT_CHOICES.filter((f) => ["簡答題", "表格整理", "圖像組織", "段落寫作"].includes(f)) : FORMAT_CHOICES;

export const MATERIAL_REQUIRED = "先附上文章或教材";

export const TEXT_MAX = 30000;
export const OBJECTIVE_MAX = 300;
export const REQUEST_MAX = 2000;

// ---------------------------------------------------------------- 照我的學習單做 (mimic-contract)

export const SAMPLE_HEADING = "你的學習單範本";
export const LESSON_TEXT_HEADING = "這節課的課文";
export const SAMPLE_LINE = "Manna 會讀這份學習單的題型、順序和作答方式，新的學習單照同樣的樣式出題。第一次約需 20 秒。";
export const NEED_SAMPLE = "先放你的學習單範本";
export const NEED_LESSON_TEXT = "先放這節課的課文";
/** 「照「L2 學習單」的樣式」: the sticky bar's summary and a worksheet card's header. */
export const styleLine = (name: string) => `照「${name}」的樣式`;

/** The notice when a material put in the article slot looks like a worksheet. */
export const LOOKS_LIKE_WORKSHEET = "這份看起來是一份學習單。要照它的樣式為新課文出題嗎？";
export const USE_AS_SAMPLE = "照這份的樣式做";
export const KEEP_AS_TEXT = "不用，當成課文";
/** On 完整學習單, beside the 版型 select. */
export const MIMIC_LINK = "或上傳你的學習單範本 →";

/** The home quick box with 照我的學習單做: its two attach slots, their chips and what is missing. */
export const ATTACH_SAMPLE = "附上學習單範本";
export const ATTACH_LESSON_TEXT = "附上課文";
export const SAMPLE_CHIP_LABEL = "範本";
export const LESSON_TEXT_CHIP_LABEL = "課文";
export const QUICK_NEED_SAMPLE = "先附上你的學習單範本";
export const QUICK_NEED_LESSON_TEXT = "先附上這節課的課文";
/** The notice on the home quick box: the material moves into the 範本 slot, on the same page. */
export const TAKE_AS_SAMPLE = "把它當範本";

/** Where 照這份的樣式做 goes: the mimic page with this material as the sample. */
export const mimicPath = (materialId: number) => `/create/mimic?sample=${materialId}`;

/** Where 照這個範本做 goes: the mimic page with this template (平台範本 or 我的範本) as the sample. */
export const mimicTemplatePath = (templateId: number) => `/create/mimic?template=${templateId}`;

/** Where 用這篇出題 goes: a tool with this material (平台教材 or 我的教材) in the article slot. */
export const toolMaterialPath = (tool: Tool, materialId: number) => `/create/${tool}?material=${materialId}`;

/**
 * Typed words that pick a tile, in the contract's order (the first row that matches wins).
 * 照我的學習單做 comes first, so 「照我的學習單」 is not just 學習單.
 */
const KEYWORDS: ReadonlyArray<readonly [Tool, readonly string[]]> = [
  ["mimic", ["照我的", "照這份", "範本", "樣式", "模仿"]],
  ["tiered", ["分層", "差異化"]],
  ["activity", ["活動", "教案", "流程"]],
  ["kit", ["套件"]],
  ["worksheet", ["學習單"]],
  ["comprehension", ["閱讀", "理解", "題目", "出題"]],
];

export function toolFromWords(text: string): Tool | null {
  for (const [tool, words] of KEYWORDS) {
    if (words.some((w) => text.includes(w))) return tool;
  }
  return null;
}

/**
 * The kinds a session can start with from these pages. The server's PrepKind gains comprehension,
 * tiered and kit (agent D adds them to the shared types); until then the body is widened here.
 */
export type StartKind = KindTool | "ask";
export type StartRequest = Omit<CreatePrepSessionRequest, "kind"> & { kind: StartKind };

export const asCreateRequest = (body: StartRequest): CreatePrepSessionRequest =>
  body as unknown as CreatePrepSessionRequest;

/**
 * 「用範例試試」: a short original passage (B1) to try a tool with. Written for Manna; not taken
 * from any textbook.
 */
export const EXEMPLAR_TEXT = [
  // A title line first: pasted text without a title is named after its first line.
  "Why Bees Matter",
  "Most people think of bees as small insects that make honey and sometimes sting. But bees do a much bigger job. When a bee visits a flower, tiny grains of pollen stick to its body. When it flies to the next flower, some of that pollen falls off. This helps the plant make seeds and fruit. Scientists call this pollination.",
  "A large part of the food we eat depends on pollination. Apples, strawberries, pumpkins and even the coffee beans in many adults' morning drink need help from insects like bees. Without them, farmers would grow much less food, and the fruit in our shops would cost a lot more.",
  "Sadly, the number of bees is falling in many countries. Some farmers use chemicals to kill the insects that eat their crops, but these chemicals can hurt bees, too. As cities grow, there are fewer wild flowers for bees to visit. Hotter summers and sudden changes in the weather make their lives even harder.",
  "The good news is that ordinary people can help. You can grow flowers on a balcony or in a school garden. If you choose flowers that open at different times of the year, bees can find food from spring to autumn. Some schools build small \"bee hotels\" out of wood and bamboo, where wild bees can lay their eggs safely.",
  "Next time a bee flies near you, do not wave your arms or try to hit it. Stand still, and it will usually fly away. This small insect is busy working for all of us. A world without bees would be a quieter world, and a hungrier one.",
].join("\n\n");
