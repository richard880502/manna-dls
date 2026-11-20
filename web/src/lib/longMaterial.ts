// 長課文與「回上一步」: the words and small rules shared by the home page, the conversation page
// and the worksheet page when a material is long or generation fails because of it.
import type { PrepSession } from "../api/types";
import type { MaterialChoice } from "../components/material/MaterialChooser";
import type { Sample } from "../components/template/SampleChooser";
import { TOOL_CHIP, type HomeTool, type Tool } from "./create";

/** A material with more paragraphs than this is longer than one lesson: the notice shows. */
export const LONG_MATERIAL_PARAGRAPHS = 40;

export const isLongMaterial = (paragraphs: number | null | undefined): boolean =>
  typeof paragraphs === "number" && paragraphs > LONG_MATERIAL_PARAGRAPHS;

export const longMaterialLine = (paragraphs: number): string =>
  `這份課文有 ${paragraphs} 段，比一般一節課長；產生會比較久，後面的段落也比較可能寫得簡略。`;

export const EDIT_TEXT = "修改課文";
export const PICK_PARAGRAPHS = "只選要用的段落";
export const GENERATE_ANYWAY = "照樣產生";
export const BACK_STEP = "回上一步";
export const REGENERATE = "重新產生";
export const RETRY = "再試一次";
export const CHANGE_MATERIAL = "換一份教材";
export const KEPT_LINE = "你輸入的課文和要求都還在。";

/** 「只用其中 12 段（共 52 段）」 */
export const selectedLine = (selected: number, total: number): string => `只用其中 ${selected} 段（共 ${total} 段）`;

// ---------------------------------------------------------------- the home page's draft

/** Everything the home quick box holds: kept so that Back, or 回上一步, finds it all again. */
export interface HomeDraft {
  text: string;
  choice: MaterialChoice | null;
  sample: Sample | null;
  /** The tile the teacher chose (`null` inside: the tile was cleared); absent: typed words choose. */
  clicked: { tool: Tool | HomeTool | null } | null;
  /** Materials the teacher said to keep as the article (ids). */
  keptAsText: number[];
  /** 只選要用的段落 (ids of the attached library material); `null`: all of it. */
  selection: string[] | null;
  /** The teacher saw the long-material notice and said 照樣產生 (the material's id). */
  longOk: number | null;
}

export const emptyHomeDraft = (): HomeDraft => ({
  text: "",
  choice: null,
  sample: null,
  clicked: null,
  keptAsText: [],
  selection: null,
  longOk: null,
});

const DRAFT_PREFIX = "manna:home-draft:";
const ORIGIN_PREFIX = "manna:prep-origin:";

function read(key: string): unknown {
  try {
    const raw = window.sessionStorage.getItem(key);
    return raw ? (JSON.parse(raw) as unknown) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: unknown) {
  try {
    window.sessionStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Private windows and full storage: the draft just is not kept; the page still works.
  }
}

/** A stored value is taken only when it looks like a draft (anything else reads as no draft). */
function asDraft(value: unknown): HomeDraft | null {
  if (!value || typeof value !== "object") return null;
  const v = value as Partial<HomeDraft>;
  if (typeof v.text !== "string") return null;
  return {
    ...emptyHomeDraft(),
    ...v,
    keptAsText: Array.isArray(v.keptAsText) ? v.keptAsText : [],
  };
}

/** The draft of one history entry (`location.key`): Back to the home page finds it again. */
export const saveHomeDraft = (locationKey: string, draft: HomeDraft) => write(DRAFT_PREFIX + locationKey, draft);
export const loadHomeDraft = (locationKey: string): HomeDraft | null => asDraft(read(DRAFT_PREFIX + locationKey));

/** What the home page held when it made this session: 回上一步 on the conversation page restores it. */
export const rememberOrigin = (sessionId: number, draft: HomeDraft) => write(ORIGIN_PREFIX + sessionId, draft);
export const loadOrigin = (sessionId: number): HomeDraft | null => asDraft(read(ORIGIN_PREFIX + sessionId));

/**
 * The home box as far as a conversation still tells it, for 回上一步 when the box's own draft is
 * gone (another browser, cleared storage): the first request and the material.
 */
export function draftFromSession(s: PrepSession): HomeDraft {
  const first = s.turns[0]?.request ?? "";
  const labels: string[] = Object.values(TOOL_CHIP);
  const selected = s.options?.paragraph_ids;
  return {
    ...emptyHomeDraft(),
    text: labels.includes(first) ? "" : first,
    choice: s.material
      ? {
          way: "library",
          picked: {
            id: s.material.id,
            title: s.material.title,
            ...(typeof s.material_paragraphs === "number" ? { paragraph_count: s.material_paragraphs } : {}),
          },
          title: "",
          text: "",
        }
      : null,
    selection: selected && selected.length > 0 ? selected : null,
  };
}

/** Router state that makes the home page start from a draft (回上一步). */
export interface HomeRestore {
  restore?: HomeDraft;
}
