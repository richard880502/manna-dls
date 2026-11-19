// 內容目標與語言目標 (docs/SIOP_DESIGN_RULES.md G1–G3; docs/API.md「SIOP 目標」): the two optional
// fields of every generator and Manna's drafts when the teacher leaves both empty. The teacher's
// choice on every draft — left (adopt), edited (revise) or cleared (reject) — is recorded with POST /api/ai/:id/decision.
import type {
  DecisionRequest,
  WsObjective,
  ObjectiveCategory,
  ObjectiveDraft,
  ObjectiveDraftResult,
  ObjectiveType,
} from "../api/types";

export const OBJECTIVE_MAX = 300;
export const CONTENT_OBJECTIVE_LABEL = "內容目標（選填）";
export const LANGUAGE_OBJECTIVE_LABEL = "語言目標（選填）";
export const CONTENT_OBJECTIVE_PLACEHOLDER = "例如：I can explain how the writer uses examples to support the main idea.";
export const LANGUAGE_OBJECTIVE_PLACEHOLDER = "例如：I can underline signal words like for example and such as.";
export const OBJECTIVES_HINT = "一行一項，各寫一項就好。兩格都空著，按「產生」時 Manna 會先依文章擬一組，讓你看過再產生。";

/**
 * "I can" at the start of a students' line: any case, spaces or tabs between and after the words,
 * or a colon ("I can: identify …"), followed by something printable. The exports use the same
 * pattern (`printed_objective`, api/src/objectives/mod.rs).
 */
const I_CAN_LEAD = /^[ \t]*[Ii][ \t]+[Cc][Aa][Nn](?:[ \t]*[:：][ \t]*|[ \t]+)([^ \t][\s\S]*)$/;

/**
 * A students' line as it prints under the heading 「Today I can …」: without its own leading "I can",
 * which the heading already says. The stored line keeps it (SIOP: objectives are written
 * "I can …"); a line that does not start with "I can", or has nothing after it, prints unchanged.
 */
export function printedObjective(line: string): string {
  return I_CAN_LEAD.exec(line)?.[1] ?? line;
}

export const OBJECTIVE_KIND_LABEL: Record<ObjectiveType, string> = { content: "內容目標", language: "語言目標" };

/** R2's four kinds, in Manna's order (a learning strategy first). */
export const CATEGORY_LABEL: Record<ObjectiveCategory, string> = {
  strategy: "學習策略",
  function: "語言功能",
  structure: "語言結構或文法",
  vocabulary: "學術字彙",
};

/** The panel's words (fixed). */
export const DRAFTS_HEADING = "Manna 先幫你草擬目標";
export const DRAFTING_LINE = "Manna 正在依文章草擬內容目標與語言目標，並逐項檢查…";
export const DRAFTS_LEAD = "Manna 依文章擬了一組，都通過了 SIOP 的檢查。直接用，或在框裡改；清空那一項就不放。";
export const DRAFTS_READY_LINE = "Manna 擬了一組目標，看一下再產生。";
export const DRAFTS_READY_FILL_LINE = "Manna 擬了一組目標，看一下再填入。";
export const USE_DRAFTS = "用這組目標產生";
export const USE_DRAFTS_FILL = "用這組目標";
export const SKIP_DRAFTS = "不用目標，直接產生";
export const SKIP_DRAFTS_FILL = "不用目標";
export const ANOTHER_SET = "換一組";
export const ASK_MANNA = "請 Manna 擬一組";
export const DRAFT_FAILED = "Manna 這次沒有草擬出目標。你可以自己寫，或直接產生。";

/** What the teacher chose for one draft, before it is sent. */
export type DraftChoice =
  | { decision: "adopt" }
  /** 修改: the draft, edited. */
  | { decision: "revise"; text: string }
  /** 改寫: her own objective instead (empty: none in its place). */
  | { decision: "rewrite"; text: string };

/**
 * What the line left in a draft's box says: unchanged is an adoption, changed a revision, and
 * cleared a draft turned down with nothing in its place.
 */
export function choiceFor(draft: ObjectiveDraft, text: string): DraftChoice {
  const t = text.trim();
  if (t === draft.text.trim()) return { decision: "adopt" };
  return t === "" ? { decision: "rewrite", text: "" } : { decision: "revise", text: t };
}

/** A field's objectives: one per line, blank lines dropped. */
export const objectiveLines = (field: string): string[] =>
  field
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l !== "");

/** The two fields as the request sends them (absent when empty). */
export function objectiveOptions(content: string, language: string): { content_objective?: string; language_objective?: string } {
  const c = objectiveLines(content).join("\n");
  const l = objectiveLines(language).join("\n");
  return { ...(c ? { content_objective: c } : {}), ...(l ? { language_objective: l } : {}) };
}

/** The line a draft ends up as (undecided: adopted); `null` when the teacher rewrote it as nothing. */
export function finalText(draft: ObjectiveDraft, choice: DraftChoice | undefined): string | null {
  if (!choice || choice.decision === "adopt") return draft.text;
  const text = choice.text.trim();
  return text === "" ? null : text;
}

/** The two fields after the teacher's choices: one line per objective that stays. */
export function fieldsFrom(
  drafts: ObjectiveDraft[],
  choices: Record<number, DraftChoice>,
): { content: string; language: string } {
  const lines = (kind: ObjectiveType) =>
    drafts
      .filter((d) => d.kind === kind)
      .map((d) => finalText(d, choices[d.index]))
      .filter((t): t is string => t !== null)
      .join("\n");
  return { content: lines("content"), language: lines("language") };
}

/**
 * The decisions to record for the drafts: 採用 adopt; 修改 revise with the edited line (adopt
 * when nothing changed); 改寫 reject with her own line; 不用目標 skip for every draft.
 */
export function decisionsFor(
  drafts: ObjectiveDraft[],
  choices: Record<number, DraftChoice>,
  skipped: boolean,
): DecisionRequest[] {
  return drafts.map((d) => {
    if (skipped) return { item_index: d.index, decision: "skip" };
    const choice = choices[d.index];
    if (!choice || choice.decision === "adopt") return { item_index: d.index, decision: "adopt" };
    const text = choice.text.trim();
    if (choice.decision === "revise") {
      return text === d.text.trim() ? { item_index: d.index, decision: "adopt" } : { item_index: d.index, decision: "revise", revised_content: text };
    }
    return text === ""
      ? { item_index: d.index, decision: "reject" }
      : { item_index: d.index, decision: "reject", reason: "better_idea", revised_content: text };
  });
}

/** Whether a draft result has anything for the teacher to decide on. */
export const hasDrafts = (r: ObjectiveDraftResult) => r.status === "shown" && r.objectives.length > 0;

/** A worksheet objective that comes from the two fields (the server keeps them as `teacher-c1`, `teacher-l1` …). */
export const isFromFields = (o: WsObjective) => o.id.startsWith("teacher-");

/** The objectives of the two fields as a worksheet keeps them (the server's `teacher-c1`, `teacher-l1` …). */
export function fieldObjectives(content: string | null | undefined, language: string | null | undefined): WsObjective[] {
  const group = (field: string | null | undefined, type: ObjectiveType, code: string): WsObjective[] =>
    objectiveLines(field ?? "").map((line, i) => ({
      id: `teacher-${code}${i + 1}`,
      type,
      text: line,
      student_facing_en: line,
      origin: "teacher",
      sop_id: null,
    }));
  return [...group(content, "content", "c"), ...group(language, "language", "l")];
}

/**
 * What the server does when the two fields change (docs/API.md「SIOP 目標」), done on the
 * editor's own copy too, since it keeps its copy after a save: the objectives from the fields
 * replaced, the others kept first.
 */
export const syncedObjectives = (
  objectives: WsObjective[],
  content: string | null | undefined,
  language: string | null | undefined,
): WsObjective[] => [...objectives.filter((o) => !isFromFields(o)), ...fieldObjectives(content, language)];

/** Whether the fields hold any objective. */
export const hasObjectives = (content: string | null | undefined, language: string | null | undefined) =>
  objectiveLines(content ?? "").length + objectiveLines(language ?? "").length > 0;
