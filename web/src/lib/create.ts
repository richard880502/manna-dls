// 建立 (the generator, /create/:tool) and the home page's quick box: the five tools, their words,
// which settings each one takes, and the typed-keyword shortcut. Fixed labels are the contract's
// (docs: bloom-contract, "Shell and home" and Addendum 1), word for word.
import type { CreatePrepSessionRequest } from "../api/types";

export const TOOLS = ["comprehension", "tiered", "activity", "kit", "worksheet"] as const;
export type Tool = (typeof TOOLS)[number];

export const isTool = (value: string | undefined): value is Tool => (TOOLS as readonly string[]).includes(value ?? "");

/**
 * The contract's chip labels: the home page's chips and tool cards, and the request text sent
 * when the teacher typed nothing (so the server can tell a bare chip from a real request).
 */
export const TOOL_CHIP: Record<Tool, string> = {
  comprehension: "閱讀理解題",
  tiered: "分層練習",
  activity: "課堂活動",
  kit: "教學套件",
  worksheet: "學習單",
};

/** The name in the sidebar's 建立 group and in the generator's heading. */
export const TOOL_NAME: Record<Tool, string> = { ...TOOL_CHIP, worksheet: "完整學習單" };

/** One line per tool: the home page's cards. */
export const TOOL_BLURB: Record<Tool, string> = {
  comprehension: "依 Bloom 六個層次出題，附答案與文本依據",
  tiered: "同一篇文章，基礎／一般／進階三個版本",
  activity: "引起動機到統整的一節課流程",
  kit: "活動流程＋分層學習單＋教師版答案，一次下載",
  worksheet: "完整學習單，可以照你的版型",
};

/** The generator's subtitle: what the tool makes from what. */
export const TOOL_SUBTITLE: Record<Tool, string> = {
  comprehension: "附上一篇文章，寫下學習目標（可以不寫），Manna 依 Bloom 認知層次出閱讀理解題，附答案與文本依據。",
  tiered: "同一篇文章做成基礎／一般／進階三個版本；基礎版的題目附提示。",
  activity: "附上文章、寫下學習目標，Manna 規劃從引起動機到統整的一節課流程。",
  kit: "一次做好活動流程、分層學習單和教師版答案，下載成一份 DOCX。",
  worksheet: "暖身、逐段任務到 Exit ticket 的完整學習單，可以照你的版型排版。",
};

/** Which settings each tool takes (課堂活動: only 班級 and 學習目標). */
export const TOOL_FIELDS: Record<Tool, { bloom: boolean; formats: boolean; template: boolean }> = {
  comprehension: { bloom: true, formats: true, template: false },
  tiered: { bloom: true, formats: true, template: false },
  activity: { bloom: false, formats: false, template: false },
  kit: { bloom: true, formats: false, template: false },
  worksheet: { bloom: true, formats: true, template: true },
};

/** 題數 when the teacher has not changed it. */
export const defaultCount = (tool: Tool): number => (tool === "tiered" ? 12 : 8);

/** 題型 (fixed labels; none chosen = 自動). */
export const FORMAT_CHOICES = ["選擇題", "是非題", "配對題", "填空題", "簡答題", "表格整理", "段落寫作"] as const;

export const MATERIAL_REQUIRED = "先附上文章或教材";
export const TEXT_MAX = 30000;
export const OBJECTIVE_MAX = 300;
export const REQUEST_MAX = 2000;

/** Typed words that pick a chip, in the contract's order (the first row that matches wins). */
const KEYWORDS: ReadonlyArray<readonly [Tool, readonly string[]]> = [
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
export type StartKind = Tool | "ask";
export type StartRequest = Omit<CreatePrepSessionRequest, "kind"> & { kind: StartKind };

export const asCreateRequest = (body: StartRequest): CreatePrepSessionRequest =>
  body as unknown as CreatePrepSessionRequest;

/**
 * 「用範例試試」: a short original passage (B1) to try a tool with. Written for Manna; not taken
 * from any textbook.
 */
export const EXEMPLAR_TEXT = [
  "Most people think of bees as small insects that make honey and sometimes sting. But bees do a much bigger job. When a bee visits a flower, tiny grains of pollen stick to its body. When it flies to the next flower, some of that pollen falls off. This helps the plant make seeds and fruit. Scientists call this pollination.",
  "A large part of the food we eat depends on pollination. Apples, strawberries, pumpkins and even the coffee beans in many adults' morning drink need help from insects like bees. Without them, farmers would grow much less food, and the fruit in our shops would cost a lot more.",
  "Sadly, the number of bees is falling in many countries. Some farmers use chemicals to kill the insects that eat their crops, but these chemicals can hurt bees, too. As cities grow, there are fewer wild flowers for bees to visit. Hotter summers and sudden changes in the weather make their lives even harder.",
  "The good news is that ordinary people can help. You can grow flowers on a balcony or in a school garden. If you choose flowers that open at different times of the year, bees can find food from spring to autumn. Some schools build small \"bee hotels\" out of wood and bamboo, where wild bees can lay their eggs safely.",
  "Next time a bee flies near you, do not wave your arms or try to hit it. Stand still, and it will usually fly away. This small insect is busy working for all of us. A world without bees would be a quieter world, and a hungrier one.",
].join("\n\n");
