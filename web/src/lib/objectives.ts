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
export const OBJECTIVES_HINT =
  "一行一項，各寫一項就好。先決定內容目標，再依它寫語言目標（學生要用讀、聽、說、寫做出什麼，才看得出達成內容目標）。兩格都空著，按「產生」時 Manna 會先依文章擬內容目標，你決定之後再擬語言目標。";

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
export const DRAFTING_LINE = "Manna 正在依文章草擬內容目標，並逐項檢查…";
export const DRAFTING_LANGUAGE_LINE = "Manna 正在依你決定的內容目標草擬語言目標，並逐項檢查…";
export const DRAFTS_LEAD = "Manna 依文章擬了一組，都通過了 SIOP 的檢查。勾選要用的目標，也可以在框裡改。";
export const DRAFTS_CONTENT_LEAD =
  "Manna 先依文章擬了內容目標，都通過了 SIOP 的檢查。勾選要用的目標，也可以在框裡改。決定好內容目標之後，Manna 才會依它擬語言目標。";
export const DRAFTS_LANGUAGE_LEAD =
  "Manna 依你決定的內容目標擬了語言目標：學生要用英語讀、聽、說、寫做出什麼，老師看到成果，就知道學生有沒有學會內容目標。勾選要用的目標，也可以在框裡改。";
export const DECIDED_CONTENT_HEADING = "你決定的內容目標";
export const DRAFTS_READY_LINE = "Manna 擬了一組目標，看一下再產生。";
export const DRAFTS_READY_FILL_LINE = "Manna 擬了一組目標，看一下再填入。";
export const DRAFTS_CONTENT_READY_LINE = "Manna 擬了內容目標，看一下，決定後再擬語言目標。";
/** The primary says how many are checked (「用勾選的 2 個目標產生」). */
export const useDrafts = (n: number, fill: boolean) => `用勾選的 ${n} 個目標${fill ? "" : "產生"}`;
export const NOTHING_CHECKED = "還沒勾選任何目標。勾一個，或按「不用目標」。";
export const NOTHING_CHECKED_GENERATE = "還沒勾選任何目標。勾一個，或按「不用目標，直接產生」。";
export const useCheckboxLabel = (kind: ObjectiveType, code: string) => `使用 ${OBJECTIVE_KIND_LABEL[kind]} ${code}`;
export const REDRAW_UNCHECKED = "換掉沒勾的";
export const REDRAW_FAILED = "這次沒有換成，目標維持原樣。你可以再試一次，或自己改。";
export const SKIP_DRAFTS = "不用目標，直接產生";
export const SKIP_DRAFTS_FILL = "不用目標";
export const ANOTHER_SET = "換一組";
export const ASK_MANNA = "請 Manna 擬內容目標";
export const ASK_MANNA_LANGUAGE = "請 Manna 依內容目標擬語言目標";
/** Under the language field while there is no 內容目標 to derive it from. */
export const LANGUAGE_AFTER_CONTENT = "先決定內容目標，才能請 Manna 依它擬語言目標。";
/** 用勾選的內容目標，請 Manna 擬語言目標: the primary of the 內容目標 step. */
export const toLanguage = (n: number) => `用勾選的 ${n} 個內容目標，請 Manna 擬語言目標`;
/** The quiet way out of the 內容目標 step: no 語言目標 for now. */
export const onlyContent = (n: number, fill: boolean) => `只用勾選的 ${n} 個內容目標${fill ? "" : "產生"}`;
export const LANGUAGE_FAILED = "這次沒有擬出語言目標，內容目標維持原樣。你可以再試一次，或自己寫語言目標。";
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
  unchecked: ReadonlySet<number> = new Set(),
): { content: string; language: string } {
  const lines = (kind: ObjectiveType) =>
    drafts
      .filter((d) => d.kind === kind && !unchecked.has(d.index))
      .map((d) => finalText(d, choices[d.index]))
      .filter((t): t is string => t !== null)
      .join("\n");
  return { content: lines("content"), language: lines("language") };
}

/**
 * The decisions to record for the drafts: 採用 adopt; 修改 revise with the edited line (adopt
 * when nothing changed); 改寫 reject with her own line; 不用目標 skip for every draft; a draft
 * she did not check (`unchecked`) skip, whatever its box says.
 */
export function decisionsFor(
  drafts: ObjectiveDraft[],
  choices: Record<number, DraftChoice>,
  skipped: boolean,
  unchecked: ReadonlySet<number> = new Set(),
): DecisionRequest[] {
  return drafts.map((d) => {
    if (skipped || unchecked.has(d.index)) return { item_index: d.index, decision: "skip" };
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

/** A decision for one draft, against the draft call that wrote it (a set after 換掉沒勾的 has drafts of more than one). */
export interface DraftDecision {
  interaction_id: number;
  request: DecisionRequest;
}

/** Where a draft shown in the panel comes from: the draft call and its `index` in it. */
export interface DraftSource {
  interaction_id: number | null;
  index: number;
}

/** The decisions with the call each belongs to (a draft without a call is not recorded). */
export function decisionsAt(requests: DecisionRequest[], sources: Record<number, DraftSource>): DraftDecision[] {
  return requests.flatMap((request) => {
    const src = request.item_index === undefined ? undefined : sources[request.item_index];
    return src && src.interaction_id !== null
      ? [{ interaction_id: src.interaction_id, request: { ...request, item_index: src.index } }]
      : [];
  });
}

/**
 * 換掉沒勾的: the set after the unchecked drafts (`redraw`) are replaced by the new ones of the
 * same kind (the one with the same code first, then in order); the kept drafts stay where they are. A new line equal to a kept one is not
 * used, and a place with no new draft of its kind left is dropped. A new draft takes the code of
 * the place it fills and a fresh `index` (`fresh` maps it to its index in the new call).
 */
export function mergeRedraw(
  current: ObjectiveDraft[],
  redraw: ReadonlySet<number>,
  incoming: ObjectiveDraft[],
): { drafts: ObjectiveDraft[]; fresh: Record<number, number> } {
  const norm = (t: string) => t.trim().toLowerCase();
  const keptLines = new Set(current.filter((d) => !redraw.has(d.index)).map((d) => norm(d.text)));
  const pool: Record<ObjectiveType, ObjectiveDraft[]> = { content: [], language: [] };
  for (const d of incoming) if (!keptLines.has(norm(d.text))) pool[d.kind].push(d);
  let next = Math.max(-1, ...current.map((d) => d.index)) + 1;
  const drafts: ObjectiveDraft[] = [];
  const fresh: Record<number, number> = {};
  for (const d of current) {
    if (!redraw.has(d.index)) {
      drafts.push(d);
      continue;
    }
    const same = pool[d.kind].findIndex((n) => n.code === d.code);
    const n = pool[d.kind].splice(same >= 0 ? same : 0, 1)[0];
    if (!n) continue;
    fresh[next] = n.index;
    drafts.push({ ...n, index: next++, code: d.code });
  }
  return { drafts, fresh };
}

/**
 * SIOP R1 (both kinds, and the 語言目標 serves a 內容目標): what to tell the teacher when the
 * checked drafts have only one kind while the set has both. A hint, not a block: she decides
 * (G2); the generators work with either field.
 */
export function checkedSetNote(drafts: ObjectiveDraft[], checked: ObjectiveDraft[]): string | null {
  const has = (list: ObjectiveDraft[], kind: ObjectiveType) => list.some((d) => d.kind === kind);
  if (checked.length === 0) return null;
  if (!has(checked, "content") && has(drafts, "content"))
    return "只勾了語言目標。SIOP 建議內容目標與語言目標各寫一個以上：語言目標是用來幫學生達成內容目標的。";
  if (!has(checked, "language") && has(drafts, "language"))
    return "只勾了內容目標。SIOP 建議內容目標與語言目標各寫一個以上，語言目標幫學生用得出內容目標需要的語言。";
  return null;
}
