// Bloom 認知層次 (docs/API.md Phase 2.7; contract "focus on worksheet generation with Bloom levels").
// The six ids are a fixed set the server validates; the order is always remember → create. The
// Chinese labels are what teachers see; the English name goes in a title (tooltip).
import type { Bloom, BloomCounts, WorksheetSettings, WsOutput } from "../api/types";

export const BLOOM_ORDER: readonly Bloom[] = ["remember", "understand", "apply", "analyze", "evaluate", "create"];

export const BLOOM_LABEL: Record<Bloom, string> = {
  remember: "記憶",
  understand: "理解",
  apply: "應用",
  analyze: "分析",
  evaluate: "評鑑",
  create: "創造",
};

export const BLOOM_EN: Record<Bloom, string> = {
  remember: "Remember",
  understand: "Understand",
  apply: "Apply",
  analyze: "Analyze",
  evaluate: "Evaluate",
  create: "Create",
};

/** An item without a level (older worksheets, or one the teacher cleared). */
export const BLOOM_NONE_LABEL = "未標";

export const isBloom = (value: unknown): value is Bloom =>
  typeof value === "string" && (BLOOM_ORDER as readonly string[]).includes(value);

/** The tooltip of a level: 「Bloom：Analyze」. */
export const bloomTitle = (level: Bloom | null | undefined) =>
  level ? `Bloom：${BLOOM_EN[level]}` : `Bloom：${BLOOM_NONE_LABEL}`;

/** 要做什麼 (the settings view; the home page's output chips use the same words). */
export const OUTPUT_LABEL: Record<WsOutput, string> = {
  worksheet: "學習單",
  comprehension: "閱讀理解題",
  tiered: "分層練習",
};
export const OUTPUTS: readonly WsOutput[] = ["worksheet", "comprehension", "tiered"];

export const outputOf = (settings: Pick<WorksheetSettings, "output"> | null | undefined): WsOutput =>
  settings?.output ?? "worksheet";

// ---------------------------------------------------------------- counts

/** 題數 limits (the server's): 3–30 in all, 0–12 for one level. */
export const TOTAL_MIN = 3;
export const TOTAL_MAX = 30;
export const LEVEL_MAX = 12;
/** Minutes per question for the summary line (the same 2.5 the worksheet budget uses). */
export const MINUTES_PER_ITEM = 2.5;

/** What the server does when 題數 is 自動. */
export const defaultTotal = (output: WsOutput) => (output === "tiered" ? 12 : 8);

export type FullCounts = Record<Bloom, number>;

export const zeroCounts = (): FullCounts => ({
  remember: 0,
  understand: 0,
  apply: 0,
  analyze: 0,
  evaluate: 0,
  create: 0,
});

/** Every level, missing ones as 0. */
export const fullCounts = (counts: BloomCounts | null | undefined): FullCounts => {
  const out = zeroCounts();
  for (const level of BLOOM_ORDER) {
    const n = counts?.[level];
    out[level] = typeof n === "number" && Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  }
  return out;
};

export const countsTotal = (counts: BloomCounts) => BLOOM_ORDER.reduce((sum, l) => sum + (counts[l] ?? 0), 0);

/** The levels with at least one question, in Bloom order. */
export const levelsOf = (counts: BloomCounts) => BLOOM_ORDER.filter((l) => (counts[l] ?? 0) > 0);

/** Only the levels with a count (what is sent as `bloom_counts`). */
export const compactCounts = (counts: BloomCounts): BloomCounts =>
  Object.fromEntries(levelsOf(counts).map((l) => [l, counts[l]])) as BloomCounts;

/**
 * The server's fill when only the levels are known (no `bloom_counts`): every level gets one, the
 * rest go round-robin from the lowest level up (8 over six levels → 2, 2, 1, 1, 1, 1).
 */
export function fillCounts(levels: readonly Bloom[], total: number): FullCounts {
  const chosen = BLOOM_ORDER.filter((l) => levels.includes(l));
  const out = zeroCounts();
  if (chosen.length === 0) return out;
  for (const l of chosen) out[l] = 1;
  for (let rest = total - chosen.length, i = 0; rest > 0; rest -= 1, i += 1) {
    const level = chosen[i % chosen.length]!;
    out[level] += 1;
  }
  return out;
}

// ---------------------------------------------------------------- presets

export type BloomPresetId = "foundation" | "balanced" | "challenge" | "higher" | "custom";
export type FixedPresetId = Exclude<BloomPresetId, "custom">;

export interface BloomPreset {
  id: BloomPresetId;
  label: string;
  /** Percent per level (sums to 100); null for 自訂. */
  percent: FullCounts | null;
}

const pct = (r: number, u: number, a: number, an: number, e: number, c: number): FullCounts => ({
  remember: r,
  understand: u,
  apply: a,
  analyze: an,
  evaluate: e,
  create: c,
});

/** The contract's preset table (Addendum 1), in picker order. */
export const BLOOM_PRESETS: readonly BloomPreset[] = [
  { id: "foundation", label: "基礎鞏固", percent: pct(40, 40, 20, 0, 0, 0) },
  { id: "balanced", label: "均衡（預設）", percent: pct(15, 25, 20, 20, 10, 10) },
  { id: "challenge", label: "思考挑戰", percent: pct(0, 15, 20, 30, 20, 15) },
  { id: "higher", label: "深度思考", percent: pct(0, 0, 0, 30, 35, 35) },
  { id: "custom", label: "自訂", percent: null },
];

export const DEFAULT_PRESET: FixedPresetId = "balanced";

export function presetById(id: BloomPresetId): BloomPreset {
  return BLOOM_PRESETS.find((p) => p.id === id) ?? BLOOM_PRESETS[1]!;
}

/** The smallest total a preset accepts. Below the number of levels it uses, the levels with the
 * largest share get one question each (see `countsFor`), so every preset takes the global minimum. */
export function presetMinTotal(_id: BloomPresetId): number {
  return TOTAL_MIN;
}

/**
 * A preset and a total → questions per level, by largest remainder, with at least one for every
 * level whose share is above 0. Ties go to the lower level when adding and the higher level when
 * taking away. With fewer questions than levels, the levels with the largest share get one each.
 */
export function countsFor(preset: FixedPresetId, total: number): FullCounts {
  const percent = presetById(preset).percent!;
  const used = BLOOM_ORDER.filter((l) => percent[l] > 0);
  const target = Math.max(1, Math.round(total));
  const out = zeroCounts();
  if (target < used.length) {
    // Fewer questions than levels: one each for the levels with the largest share (ties to the
    // lower level), so the teacher's total is kept.
    [...used]
      .sort((a, b) => percent[b] - percent[a] || BLOOM_ORDER.indexOf(a) - BLOOM_ORDER.indexOf(b))
      .slice(0, target)
      .forEach((l) => {
        out[l] = 1;
      });
    return out;
  }
  const quota = zeroCounts();
  for (const l of used) {
    quota[l] = (percent[l] * target) / 100;
    out[l] = Math.max(1, Math.floor(quota[l]));
  }
  let sum = countsTotal(out);
  while (sum < target) {
    // The most under-served level; ties to the lower level (BLOOM_ORDER is low → high).
    let pick = used[0]!;
    for (const l of used) if (quota[l] - out[l] > quota[pick] - out[pick]) pick = l;
    out[pick] += 1;
    sum += 1;
  }
  while (sum > target) {
    // The most over-served level that can give one back; ties to the higher level.
    const able = used.filter((l) => out[l] > 1);
    if (able.length === 0) break;
    let pick = able[able.length - 1]!;
    for (const l of [...able].reverse()) if (quota[l] - out[l] < quota[pick] - out[pick]) pick = l;
    out[pick] -= 1;
    sum -= 1;
  }
  return out;
}

const sameCounts = (a: BloomCounts, b: BloomCounts) => BLOOM_ORDER.every((l) => (a[l] ?? 0) === (b[l] ?? 0));

/** The preset these counts are exactly, or 自訂. */
export function matchPreset(counts: BloomCounts): BloomPresetId {
  const total = countsTotal(counts);
  for (const p of BLOOM_PRESETS) {
    if (p.id === "custom" || total < presetMinTotal(p.id)) continue;
    if (sameCounts(countsFor(p.id, total), counts)) return p.id;
  }
  return "custom";
}

/** The picker's value: which preset, the questions per level, and the total. */
export interface BloomPlan {
  preset: BloomPresetId;
  counts: FullCounts;
  total: number;
}

export const clampTotal = (n: number, min = TOTAL_MIN) => Math.min(TOTAL_MAX, Math.max(min, Math.round(n)));

/** A plan for a preset at (about) a total; 自訂 keeps `fromCounts`. */
export function planFor(preset: BloomPresetId, total: number, fromCounts: FullCounts): BloomPlan {
  if (preset === "custom") return { preset, counts: fromCounts, total: countsTotal(fromCounts) };
  const t = clampTotal(total, presetMinTotal(preset));
  const counts = countsFor(preset, t);
  return { preset, counts, total: countsTotal(counts) };
}

/**
 * What the settings say, as a plan: explicit `bloom_counts` first; otherwise the chosen levels
 * filled the way the server fills them; otherwise the default preset at the default total.
 */
export function planFromSettings(settings: WorksheetSettings): BloomPlan {
  const output = outputOf(settings);
  const explicit = fullCounts(settings.bloom_counts);
  if (countsTotal(explicit) > 0) {
    return { preset: matchPreset(explicit), counts: explicit, total: countsTotal(explicit) };
  }
  const wanted = clampTotal(settings.question_count ?? defaultTotal(output));
  const levels = (settings.bloom ?? []).filter(isBloom);
  if (levels.length > 0 && levels.length < BLOOM_ORDER.length) {
    const counts = fillCounts(levels, Math.max(wanted, levels.length));
    return { preset: matchPreset(counts), counts, total: countsTotal(counts) };
  }
  return planFor(DEFAULT_PRESET, wanted, zeroCounts());
}

/** The settings fields a plan writes. `bloom_counts` decides; the other two say the same thing. */
export function settingsFromPlan(plan: BloomPlan): Pick<WorksheetSettings, "bloom_counts" | "bloom" | "question_count"> {
  return {
    bloom_counts: compactCounts(plan.counts),
    bloom: levelsOf(plan.counts),
    question_count: countsTotal(plan.counts),
  };
}

/** 「記憶 1・理解 2・應用 2」: the levels with questions. */
export const countsLine = (counts: BloomCounts) =>
  levelsOf(counts)
    .map((l) => `${BLOOM_LABEL[l]} ${counts[l]}`)
    .join("・");

export const estimatedMinutes = (total: number) => Math.round(total * MINUTES_PER_ITEM);

/** 「8 題 · 記憶 1・理解 2… · 約 20 分鐘」 */
export function planSummary(counts: BloomCounts): string {
  const total = countsTotal(counts);
  return [`${total} 題`, countsLine(counts), `約 ${estimatedMinutes(total)} 分鐘`].filter(Boolean).join(" · ");
}

// ---------------------------------------------------------------- distribution

export interface BloomTally {
  counts: FullCounts;
  /** Items without a level. */
  none: number;
  total: number;
}

/** How many of these items sit at each level. */
export function tally(items: ReadonlyArray<{ bloom?: Bloom | null }>): BloomTally {
  const counts = zeroCounts();
  let none = 0;
  for (const item of items) {
    if (isBloom(item.bloom)) counts[item.bloom] += 1;
    else none += 1;
  }
  return { counts, none, total: items.length };
}

/** A filter on the distribution bar: one level, the untagged items, or nothing (全部). */
export type BloomFilter = Bloom | "none" | null;

export const matchesFilter = (bloom: Bloom | null | undefined, filter: BloomFilter) =>
  filter === null || (filter === "none" ? !isBloom(bloom) : bloom === filter);

/** 「Bloom：分析 2、評鑑 1」 for a block header (teacher version); "" when nothing is tagged. */
export function blockBloomLine(items: ReadonlyArray<{ bloom?: Bloom | null }>): string {
  const { counts } = tally(items);
  const line = levelsOf(counts)
    .map((l) => `${BLOOM_LABEL[l]} ${counts[l]}`)
    .join("、");
  return line ? `Bloom：${line}` : "";
}
