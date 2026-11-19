// 備課對話 (prep sessions): the words the home page and the session page use, and small helpers
// that read a turn's result. The chip labels are the spec's, word for word.
import type {
  ActivityGoal,
  ActivityGoalKind,
  ActivityPlan,
  ActivityStep,
  LanguageGoalCategory,
  PrepAskResult,
  PrepKind,
  PrepResources,
  PrepReviseResult,
  PrepSession,
  PrepTurn,
  SiopCheck,
  SiopCheckItem,
  SiopRating,
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
  ask: "問 Manna",
  revise: "修改學習單",
  comprehension: "閱讀理解題",
  tiered: "差異化學習單",
  kit: "教學套件",
};

/**
 * 接下來可以 on the session page, in the server's `next` order (comprehension, tiered, activity,
 * kit, worksheet). assessment and resources are still valid kinds, no longer suggested; an older
 * server may still list them.
 */
export const NEXT_LABEL: Record<"comprehension" | "tiered" | "activity" | "kit" | "worksheet" | "assessment" | "resources", string> = {
  comprehension: "閱讀理解題",
  tiered: "差異化學習單",
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
export const DELETE_CONFIRM = "刪除這份備課？裡面的學習單也會一起刪除。";
/** Said on window after a prep session is deleted: the sidebar's 最近 reads its list again. */
export const PREP_SESSIONS_CHANGED = "manna:prep-sessions-changed";
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

// ---------------------------------------------------------------- 課堂活動（SIOP）

/** The words of a 課堂活動 (prep_activity version 4: SIOP A1, A2, A3, A6). */
export const PLAN_WORDS = {
  prepare: "課前準備",
  keyVocabulary: "關鍵字彙",
  materials: "教材與教具",
  questions: "高層次問題",
  background: "背景連結",
  grouping: "分組",
  roles: "分工",
  monitoring: "教師巡視",
  skills: "語言技能",
  scaffolding: "鷹架",
  reviewAssessment: "複習與評量",
  review: "複習",
  assessment: "評量",
  fromTeacher: "教師提供",
  /** 「支持內容目標 1」 */
  supports: (n: number) => `支持內容目標 ${n}`,
} as const;

/** 語言目標的四類（SIOP），學習策略優先。 */
export const LANGUAGE_CATEGORY_LABEL: Record<LanguageGoalCategory, string> = {
  strategy: "學習策略",
  function: "語言功能",
  structure: "句型文法",
  vocabulary: "學術字彙",
};

/** Whether a plan has any of what version 4 adds before the steps. */
export function hasPrepare(plan: ActivityPlan): boolean {
  return (
    (plan.key_vocabulary?.length ?? 0) > 0 ||
    (plan.materials?.length ?? 0) > 0 ||
    (plan.higher_order_questions?.length ?? 0) > 0 ||
    (plan.background ?? "").trim() !== ""
  );
}

/** 課前準備 as plain text, for 複製: 關鍵字彙, 教材與教具, 高層次問題, 背景連結. */
export function prepareText(plan: ActivityPlan): string {
  const lines: string[] = [];
  const words = plan.key_vocabulary ?? [];
  if (words.length > 0) {
    lines.push(`【${PLAN_WORDS.keyVocabulary}】`, ...words.map((w) => `${w.word}：${w.zh}`));
  }
  const materials = plan.materials ?? [];
  if (materials.length > 0) lines.push(`【${PLAN_WORDS.materials}】`, materials.join("、"));
  const questions = plan.higher_order_questions ?? [];
  if (questions.length > 0) {
    lines.push(`【${PLAN_WORDS.questions}】`, ...questions.map((q, i) => `${i + 1}. ${q.question}（${BLOOM_LABEL[q.bloom]}）`));
  }
  if ((plan.background ?? "").trim()) lines.push(`【${PLAN_WORDS.background}】`, plan.background!.trim());
  return lines.join("\n");
}

/** 複習與評量 as plain text, for 複製. */
export function reviewText(plan: ActivityPlan): string {
  return [
    (plan.review ?? "").trim() ? `${PLAN_WORDS.review}：${plan.review!.trim()}` : "",
    (plan.assessment ?? "").trim() ? `${PLAN_WORDS.assessment}：${plan.assessment!.trim()}` : "",
  ]
    .filter(Boolean)
    .join("\n");
}

/** One plan step as plain text, for 複製. */
export function stepText(step: ActivityStep): string {
  const bloom = BLOOM_ORDER.filter((l) => step.bloom?.includes(l)).map((l) => BLOOM_LABEL[l]);
  const lines = [`【${step.phase}】${step.title}（${step.minutes} 分鐘）`];
  const tags = [
    step.grouping ? `${PLAN_WORDS.grouping}：${step.grouping}` : "",
    step.skills && step.skills.length > 0 ? `${PLAN_WORDS.skills}：${step.skills.join("、")}` : "",
  ].filter(Boolean);
  if (tags.length > 0) lines.push(tags.join("｜"));
  lines.push(step.idea);
  if (bloom.length > 0) lines.push(`Bloom：${bloom.join("、")}`);
  if (step.roles && step.roles.length > 0) {
    lines.push(`${PLAN_WORDS.roles}：${step.roles.map((r) => `${r.role}：${r.task}`).join("；")}`);
  }
  if (step.monitoring?.trim()) lines.push(`${PLAN_WORDS.monitoring}：${step.monitoring.trim()}`);
  if (step.scaffolding?.trim()) lines.push(`${PLAN_WORDS.scaffolding}：${step.scaffolding.trim()}`);
  step.options.forEach((o, i) => {
    lines.push(`${OPTION_NAMES[i] ?? `選項 ${i + 1}`}：${o.title}（${o.minutes} 分鐘）`, o.idea, ...o.script);
  });
  if (step.script.length > 0) lines.push("建議腳本：", ...step.script);
  if (step.stuck_tip.trim()) lines.push(`卡住了怎麼辦：${step.stuck_tip}`);
  return lines.filter((l) => l.trim() !== "").join("\n");
}

/** 學習目標的兩組（SIOP），依顯示順序；說明沿用課例 SOP 第 3 步的字眼。 */
export const GOAL_GROUPS: ReadonlyArray<{ kind: ActivityGoalKind; label: string; hint: string }> = [
  { kind: "content", label: "內容目標", hint: "學生要理解或完成什麼" },
  { kind: "language", label: "語言目標", hint: "為了做到，需要用什麼語言" },
];

/**
 * A goal as shown: the Chinese line, the English line ("" for a plan made before), its codes; a
 * 語言目標 of version 4 also its kind, the 內容目標 it serves (from 1) and how; a goal the teacher
 * gave is marked.
 */
export interface ShownGoal {
  zh: string;
  en: string;
  curriculum_codes: string[];
  category: LanguageGoalCategory | null;
  supports: number | null;
  relation: string;
  fromTeacher: boolean;
}

/** One group of goals as shown; `kind`, `label` and `hint` are null for the goals of a plan made before the groups. */
export interface GoalGroup {
  kind: ActivityGoalKind | null;
  label: string | null;
  hint: string | null;
  goals: ShownGoal[];
}

const shownGoal = (g: ActivityGoal): ShownGoal => {
  const v4 = "zh" in g ? g : null;
  return {
    zh: v4 && typeof v4.zh === "string" ? v4.zh : "text" in g && typeof g.text === "string" ? g.text : "",
    en: v4 && typeof v4.en === "string" ? v4.en : "",
    curriculum_codes: Array.isArray(g.curriculum_codes) ? g.curriculum_codes : [],
    category: v4?.category && v4.category in LANGUAGE_CATEGORY_LABEL ? v4.category : null,
    supports: typeof v4?.supports === "number" ? v4.supports : null,
    relation: typeof v4?.relation === "string" ? v4.relation : "",
    fromTeacher: v4?.from_teacher === true,
  };
};

/**
 * A plan's goals in the groups they are shown in: first the goals without a group (a plan made
 * before 內容目標／語言目標, one Chinese line each, shown as one list as before), then 內容目標 and
 * 語言目標. Empty groups are left out.
 */
export function goalGroups(plan: ActivityPlan): GoalGroup[] {
  const known = (g: ActivityGoal) => GOAL_GROUPS.some((group) => group.kind === g.kind);
  const groups: GoalGroup[] = [
    { kind: null, label: null, hint: null, goals: plan.goals.filter((g) => !known(g)).map(shownGoal) },
    ...GOAL_GROUPS.map((group) => ({
      ...group,
      goals: plan.goals.filter((g) => g.kind === group.kind).map(shownGoal),
    })),
  ];
  return groups.filter((group) => group.goals.length > 0);
}

/**
 * The goals as plain text, for 複製: each group under its label, every goal in Chinese with its
 * English under it; a 語言目標 led by its kind, with how it serves its 內容目標 under it.
 */
export function goalsText(plan: ActivityPlan): string {
  return goalGroups(plan)
    .flatMap((group) => {
      const lines = group.label ? [`【${group.label}】`] : [];
      group.goals.forEach((g, i) => {
        const codes = g.curriculum_codes.length ? `（${g.curriculum_codes.join("、")}）` : "";
        const category = g.category ? `〔${LANGUAGE_CATEGORY_LABEL[g.category]}〕` : "";
        const teacher = g.fromTeacher ? `（${PLAN_WORDS.fromTeacher}）` : "";
        lines.push(`${i + 1}. ${category}${g.zh.trim() || g.en}${teacher}${codes}`);
        if (g.en.trim() && g.zh.trim()) lines.push(`   ${g.en}`);
        if (g.relation.trim()) lines.push(`   ${g.relation}`);
      });
      return lines;
    })
    .join("\n");
}

// ---------------------------------------------------------------- SIOP 自我檢核

/** The words of the SIOP self-check panel (docs/SIOP_DESIGN_RULES.md section 八). */
export const SIOP_WORDS = {
  title: "SIOP 自我檢核",
  /** Said once at the top of the panel: advice, the teacher decides. */
  advice: "Manna 依 SIOP 觀察量表，逐項看這份產出上看得到的地方。這是給你的建議，不是評分；要不要調整，由你判斷。",
  running: "Manna 正在做 SIOP 自我檢核…",
  none: "這份產出還沒有做 SIOP 自我檢核。",
  start: "開始檢核",
  failed: "這次的 SIOP 自我檢核沒有完成，產出不受影響。",
  retry: "再試一次",
  /** 「學習單之後改過了（第 5 版）」 */
  stale: (version: number) => `這是第 ${version} 版的檢核；學習單之後改過了。`,
  recheck: "重新檢核",
  partial: "只能部分判斷",
  classroom: "需課堂觀察",
  classroomLine: "這幾項要在課堂上看，AI 不評：",
  notJudged: "這次沒有判斷",
} as const;

export const RATING_LABEL: Record<SiopRating, string> = {
  met: "符合",
  partial: "部分符合",
  not_seen: "未見",
};

/** Whether a turn's check was made from an earlier version of its worksheet. */
export function checkIsStale(check: SiopCheck, worksheetVersion: number | null | undefined): boolean {
  return (
    check.worksheet_version_no !== null &&
    typeof worksheetVersion === "number" &&
    worksheetVersion > check.worksheet_version_no
  );
}

/** The judged items (from the page, or only their visible part) and the classroom ones, in order. */
export function checkGroups(check: SiopCheck): { judged: SiopCheckItem[]; classroom: SiopCheckItem[] } {
  return {
    judged: check.items.filter((i) => i.scope !== "classroom"),
    classroom: check.items.filter((i) => i.scope === "classroom"),
  };
}
