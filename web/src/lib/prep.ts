// 備課對話 (prep sessions): the words the home page and the session page use, and small helpers
// that read a turn's result. The chip labels are the spec's, word for word.
import type {
  ActivityPlan,
  PrepAskResult,
  PrepKind,
  PrepResources,
  PrepReviseResult,
  PrepSession,
  PrepTurn,
} from "../api/types";
import { BLOOM_LABEL, BLOOM_ORDER } from "./bloom";

/** How often a session with a running turn is asked for again. Tests shorten it. */
export const prepConfig = { pollMs: 2000 };

/** The action chips (home page) and next-step chips (session page). */
export const ACTION_LABEL: Record<"worksheet" | "activity" | "assessment" | "resources", string> = {
  worksheet: "做學習單",
  activity: "規劃教學活動",
  assessment: "設計評量",
  resources: "找教學資源",
};

export const ACTION_KINDS = ["worksheet", "activity", "assessment", "resources"] as const;
export type ActionKind = (typeof ACTION_KINDS)[number];

export const isActionKind = (kind: PrepKind): kind is ActionKind =>
  (ACTION_KINDS as readonly string[]).includes(kind);

export const OTHER_LABEL = "我想做別的…";

/** What a session's last step was, as the list says it. */
export const KIND_LABEL: Record<PrepKind, string> = {
  ...ACTION_LABEL,
  ask: "問 AI",
  revise: "修改學習單",
  comprehension: "閱讀理解題",
  tiered: "分層練習",
  kit: "教學套件",
};

/**
 * 接下來可以 on the session page, in the server's `next` order (comprehension, tiered, activity,
 * kit, worksheet). assessment and resources are still valid kinds, no longer suggested; an older
 * server may still list them.
 */
export const NEXT_LABEL: Record<"comprehension" | "tiered" | "activity" | "kit" | "worksheet" | "assessment" | "resources", string> = {
  comprehension: "閱讀理解題",
  tiered: "分層練習",
  activity: "課堂活動",
  kit: "教學套件",
  worksheet: "學習單",
  assessment: "設計評量",
  resources: "找教學資源",
};
export type NextKind = keyof typeof NEXT_LABEL;
export const isNextKind = (kind: PrepKind): kind is NextKind => kind in NEXT_LABEL;

export const ASK_PLACEHOLDER = "說說你想做什麼，例如：幫我想三個這一課的暖身活動";
export const REVISE_PLACEHOLDER = "針對上面產出的內容，你有什麼想修改的嗎？";
export const FOLLOW_PLACEHOLDER = "還想問什麼？";
export const WORKING_LINE = "Manna 正在處理…";
/** A worksheet turn whose template (the teacher's own worksheet) is still being read. */
export const READING_SAMPLE_LINE = "Manna 正在讀你的學習單設計…（約 20 秒）";
export const AI_DISCLAIMER = "AI 產生的內容可能有錯，請核對重要資訊。";
export const DELETE_CONFIRM = "刪除這次備課對話？學習單會保留。";
export const OPTION_NAMES = ["選項一", "選項二", "選項三"];

export const sessionTitle = (s: { title: string }) => s.title.trim() || "未命名的備課";

/** Turns that make a worksheet (a 教學套件 makes one beside its activity plan). */
export const isWorksheetKind = (kind: PrepKind) =>
  kind === "worksheet" || kind === "assessment" || kind === "comprehension" || kind === "tiered" || kind === "kit";

/** The session has a worksheet the bottom bar's words would revise. */
export const hasWorksheet = (session: PrepSession) =>
  session.turns.some((t) => isWorksheetKind(t.kind) && t.worksheet !== null);

// A turn's result, read by its kind (the server sends one of four shapes).
export function activityOf(turn: PrepTurn): ActivityPlan | null {
  const r = turn.result as Partial<ActivityPlan> | null;
  return (turn.kind === "activity" || turn.kind === "kit") && r && Array.isArray(r.steps) ? (r as ActivityPlan) : null;
}

export function resourcesOf(turn: PrepTurn): PrepResources | null {
  const r = turn.result as Partial<PrepResources> | null;
  return turn.kind === "resources" && r && Array.isArray(r.curriculum) ? (r as PrepResources) : null;
}

export function answerOf(turn: PrepTurn): PrepAskResult | null {
  const r = turn.result as Partial<PrepAskResult> | null;
  return turn.kind === "ask" && r && typeof r.answer === "string" ? (r as PrepAskResult) : null;
}

export function replyOf(turn: PrepTurn): PrepReviseResult | null {
  const r = turn.result as Partial<PrepReviseResult> | null;
  return turn.kind === "revise" && r && typeof r.reply === "string" ? (r as PrepReviseResult) : null;
}

/** 「B、C 區」 from block ids, with the letters the sheet printed (Exit ticket has none). */
export function changedBlocksLine(ids: string[], labels: Record<string, string> | undefined): string {
  const names = ids.map((id) => {
    const label = labels?.[id];
    if (label === undefined) return null;
    return label === "" ? "Exit ticket" : label;
  });
  if (names.some((n) => n === null) || names.length === 0) return `${ids.length} 個區塊`;
  return `${names.join("、")} 區`;
}

/** One plan step as plain text, for 複製. */
export function stepText(step: ActivityPlan["steps"][number]): string {
  const bloom = BLOOM_ORDER.filter((l) => step.bloom?.includes(l)).map((l) => BLOOM_LABEL[l]);
  const lines = [`【${step.phase}】${step.title}（${step.minutes} 分鐘）`, step.idea];
  if (bloom.length > 0) lines.push(`Bloom：${bloom.join("、")}`);
  step.options.forEach((o, i) => {
    lines.push(`${OPTION_NAMES[i] ?? `選項 ${i + 1}`}：${o.title}（${o.minutes} 分鐘）`, o.idea, ...o.script);
  });
  if (step.script.length > 0) lines.push("建議腳本：", ...step.script);
  if (step.stuck_tip.trim()) lines.push(`卡住了怎麼辦：${step.stuck_tip}`);
  return lines.filter((l) => l.trim() !== "").join("\n");
}

export function goalsText(plan: ActivityPlan): string {
  return plan.goals
    .map((g, i) => `${i + 1}. ${g.text}${g.curriculum_codes.length ? `（${g.curriculum_codes.join("、")}）` : ""}`)
    .join("\n");
}
