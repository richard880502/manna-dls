import type {
  ExitKind,
  InfoFunction,
  ObjectiveType,
  SopContent,
  SopMaterial,
  SopObjective,
  SopParagraph,
  SopScaffolds,
} from "../api/types";

/** A stage of the lesson workspace: the overview, or one of the seven SOP steps. */
export type StageKey = "overview" | 1 | 2 | 3 | 4 | 5 | 6 | 7;

export const STAGES: ReadonlyArray<{ step: 1 | 2 | 3 | 4 | 5 | 6 | 7; name: string }> = [
  { step: 1, name: "閱讀教材" },
  { step: 2, name: "分析寫作特色" },
  { step: 3, name: "內容目標與語言目標" },
  { step: 4, name: "逐段整合目標" },
  { step: 5, name: "逐段問題或任務" },
  { step: 6, name: "暖身" },
  { step: 7, name: "Exit ticket" },
];

export const stageName = (step: number): string => STAGES.find((s) => s.step === step)?.name ?? "";

/** 資訊處理功能（規格 §2、§8）, in the order tasks usually progress. */
export const INFO_FUNCTIONS: ReadonlyArray<{ value: InfoFunction; label: string }> = [
  { value: "discover", label: "發現資訊" },
  { value: "organize", label: "整理資訊" },
  { value: "analyze", label: "分析資訊" },
  { value: "compare", label: "比較資訊" },
  { value: "create", label: "創造資訊" },
];

export const infoFunctionLabel = (value: InfoFunction | null): string =>
  INFO_FUNCTIONS.find((f) => f.value === value)?.label ?? "";

export const EXIT_KINDS: ReadonlyArray<{ value: ExitKind; label: string; hint: string }> = [
  { value: "formative", label: "形成性評量", hint: "看學生有沒有達到目標" },
  { value: "reflection", label: "學生反思", hint: "學生回頭看自己今天學到什麼" },
  { value: "questions", label: "蒐集問題", hint: "學生寫下還不懂或想知道的事" },
];

export const exitKindLabel = (value: ExitKind): string => EXIT_KINDS.find((k) => k.value === value)?.label ?? "";

export const OBJECTIVE_TYPES: ReadonlyArray<{ value: ObjectiveType; label: string; hint: string }> = [
  { value: "content", label: "內容目標", hint: "學生要理解或完成什麼" },
  { value: "language", label: "語言目標", hint: "為了做到，需要用什麼語言" },
];

export const objectiveTypeLabel = (value: ObjectiveType): string => (value === "content" ? "內容目標" : "語言目標");

export const SCAFFOLD_FIELDS: ReadonlyArray<{ key: keyof SopScaffolds; label: string; hint: string }> = [
  { key: "verbal", label: "語言鷹架", hint: "句型框架、提示與追問、字詞表、作答示例" },
  { key: "instructional", label: "教學鷹架", hint: "圖像組織、部分完成的大綱、標示關鍵句或字彙" },
  { key: "procedural", label: "程序鷹架", hint: "分步指示、個別或小組安排、完成順序" },
];

export const emptyScaffolds = (): SopScaffolds => ({ verbal: "", instructional: "", procedural: "" });

export function emptyContent(): SopContent {
  return {
    analysis: { text_type: "", main_idea: "", structure: "", overall_origin: "teacher", language_focus: [], paragraphs: [] },
    objectives: [],
    paragraph_goals: [],
    task_blocks: [],
    warmup: { background: [], vocabulary: [] },
    exit_ticket: { items: [] },
  };
}

/** Fills whatever an older or partial document lacks, so the stages never meet `undefined`. */
export function normalizeContent(raw: Partial<SopContent> | null | undefined): SopContent {
  const base = emptyContent();
  if (!raw) return base;
  return {
    analysis: {
      ...base.analysis,
      ...raw.analysis,
      language_focus: raw.analysis?.language_focus ?? [],
      paragraphs: (raw.analysis?.paragraphs ?? []).map((p) => ({ ...p, features: p.features ?? [] })),
    },
    objectives: (raw.objectives ?? []).map((o) => ({ ...o, links: o.links ?? [], paragraph_ids: o.paragraph_ids ?? [] })),
    paragraph_goals: raw.paragraph_goals ?? [],
    task_blocks: (raw.task_blocks ?? []).map((b) => ({
      ...b,
      paragraph_ids: b.paragraph_ids ?? [],
      tasks: (b.tasks ?? []).map((t) => ({ ...t, scaffolds: { ...emptyScaffolds(), ...t.scaffolds } })),
    })),
    warmup: {
      background: (raw.warmup?.background ?? []).map((b) => ({ ...b, scaffolds: { ...emptyScaffolds(), ...b.scaffolds } })),
      vocabulary: raw.warmup?.vocabulary ?? [],
    },
    exit_ticket: { items: raw.exit_ticket?.items ?? [] },
  };
}

let idCounter = 0;

/** A short id, unique within the document (the server only asks for that). */
export function newId(prefix: string): string {
  idCounter += 1;
  const random = Math.random().toString(36).slice(2, 8);
  return `${prefix}-${Date.now().toString(36)}${idCounter.toString(36)}${random}`;
}

const blank = (s: string | null | undefined) => !s || !s.trim();

/** What each step holds, in a few words for the progress rail; null when the step is still empty. */
export function stageSummary(step: number, material: SopMaterial | null, content: SopContent): string | null {
  switch (step) {
    case 1:
      return material ? `${material.paragraphs.length} 段 · ${material.word_count} 字` : null;
    case 2: {
      const a = content.analysis;
      const n = a.paragraphs.filter((p) => !blank(p.function) || !blank(p.summary) || p.features.length > 0).length;
      if (n > 0) return `已分析 ${n} 段`;
      return !blank(a.main_idea) || !blank(a.structure) || !blank(a.text_type) ? "已寫全文分析" : null;
    }
    case 3: {
      const n = content.objectives.filter((o) => !blank(o.text)).length;
      return n > 0 ? `${n} 項目標` : null;
    }
    case 4: {
      const n = content.paragraph_goals.filter((g) => g.objective_ids.length > 0 || !blank(g.note)).length;
      return n > 0 ? `已對應 ${n} 段` : null;
    }
    case 5: {
      const n = content.task_blocks.reduce((sum, b) => sum + b.tasks.filter((t) => !blank(t.text)).length, 0);
      return n > 0 ? `${n} 個問題或任務` : null;
    }
    case 6: {
      const activities = content.warmup.background.filter((b) => !blank(b.text)).length;
      const words = content.warmup.vocabulary.filter((v) => !blank(v.word)).length;
      if (activities === 0 && words === 0) return null;
      return [activities > 0 ? `${activities} 個活動` : "", words > 0 ? `${words} 個字彙` : ""].filter(Boolean).join(" · ");
    }
    case 7: {
      const n = content.exit_ticket.items.filter((i) => !blank(i.text)).length;
      return n > 0 ? `${n} 題` : null;
    }
    default:
      return null;
  }
}

/** True once the lesson has anything of the SOP: material, or any step with content. */
export function hasSopContent(material: SopMaterial | null, content: SopContent): boolean {
  return STAGES.some((s) => stageSummary(s.step, material, content) !== null);
}

/** 「內容目標 1」「語言目標 2」: how an objective is referred to from the other steps. */
export function objectiveLabels(objectives: SopObjective[]): Map<string, string> {
  const labels = new Map<string, string>();
  const counts: Record<ObjectiveType, number> = { content: 0, language: 0 };
  for (const o of objectives) {
    counts[o.type] += 1;
    labels.set(o.id, `${objectiveTypeLabel(o.type)} ${counts[o.type]}`);
  }
  return labels;
}

/** 「第 1、2 段」 for a set of paragraphs, in reading order. */
export function paragraphRange(paragraphs: SopParagraph[]): string {
  const nos = paragraphs.map((p) => p.no).sort((a, b) => a - b);
  return nos.length === 0 ? "" : `第 ${nos.join("、")} 段`;
}

/** The margin number of a quote: 「1」, or 「1–2」 for merged neighbours. */
export function paragraphMark(paragraphs: SopParagraph[]): string {
  const nos = paragraphs.map((p) => p.no).sort((a, b) => a - b);
  if (nos.length === 0) return "";
  const first = nos[0] as number;
  const last = nos[nos.length - 1] as number;
  const consecutive = last - first === nos.length - 1;
  return nos.length > 1 && consecutive ? `${first}–${last}` : nos.join("、");
}
