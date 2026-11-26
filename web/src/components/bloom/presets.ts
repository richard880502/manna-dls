// Bloom presets for the generator (contract Addendum 1, "Frontend: Bloom presets"): a preset and a
// total become per-level counts by largest remainder. The level ids and Chinese labels are the
// contract's fixed set; lib/bloom.ts holds the shared table and rule; this file adapts them for the generator.

import { BLOOM_LABEL, BLOOM_ORDER, BLOOM_PRESETS as SHARED_PRESETS, countsFor as sharedCounts } from "../../lib/bloom";

export const BLOOM_LEVELS = BLOOM_ORDER;
export type BloomLevel = (typeof BLOOM_ORDER)[number];

export const BLOOM_ZH: Record<BloomLevel, string> = BLOOM_LABEL;

export type BloomCounts = Record<BloomLevel, number>;

export type PresetId = "foundation" | "balanced" | "challenge" | "higher" | "custom";

export interface BloomPreset {
  id: Exclude<PresetId, "custom">;
  label: string;
  /** Percentages in Bloom order; they add up to 100. */
  pct: readonly [number, number, number, number, number, number];
}

/** The shared preset table (lib/bloom.ts) without 自訂, as percentage tuples in Bloom order. */
export const BLOOM_PRESETS: readonly BloomPreset[] = SHARED_PRESETS.flatMap((p) =>
  p.percent && p.id !== "custom"
    ? [{ id: p.id, label: p.label, pct: BLOOM_LEVELS.map((l) => p.percent![l]) as unknown as BloomPreset["pct"] }]
    : [],
);

/** 均衡（預設）, the fallback when a preset is unknown. */
const BALANCED_PCT: BloomPreset["pct"] = [15, 25, 20, 20, 10, 10];

export const CUSTOM_LABEL = "自訂";
export const DEFAULT_PRESET: PresetId = "balanced";

/** Limits from the contract: each level 0–12, the whole set 3–30. */
export const LEVEL_MAX = 12;
export const TOTAL_MIN = 3;
export const TOTAL_MAX = 30;

/** 2.5 minutes a question, for the summary line. */
export const MINUTES_PER_QUESTION = 2.5;

export const emptyCounts = (): BloomCounts => ({
  remember: 0,
  understand: 0,
  apply: 0,
  analyze: 0,
  evaluate: 0,
  create: 0,
});

export const presetById = (id: PresetId): BloomPreset | undefined => BLOOM_PRESETS.find((p) => p.id === id);

/**
 * A preset's percentages and a total as whole counts: the shared largest-remainder rule
 * (`countsFor` in lib/bloom.ts), so the generator page and the worksheet settings always agree.
 */
export function presetCounts(pct: BloomPreset["pct"], total: number): BloomCounts {
  const preset = BLOOM_PRESETS.find((p) => p.pct.every((v, i) => v === pct[i]));
  if (total <= 0) return emptyCounts();
  return { ...sharedCounts(preset?.id ?? "balanced", total) };
}

export const countsTotal = (counts: BloomCounts): number => BLOOM_LEVELS.reduce((sum, l) => sum + counts[l], 0);

/** Only the levels that get questions, for the request body. */
export function nonZeroCounts(counts: BloomCounts): Partial<BloomCounts> {
  const out: Partial<BloomCounts> = {};
  for (const l of BLOOM_LEVELS) if (counts[l] > 0) out[l] = counts[l];
  return out;
}

/** 「記憶 1、理解 2、應用 2」 */
export const countsLine = (counts: BloomCounts): string =>
  BLOOM_LEVELS.filter((l) => counts[l] > 0)
    .map((l) => `${BLOOM_ZH[l]} ${counts[l]}`)
    .join("、");

/** 「約 20 分鐘」 */
export const minutesLine = (total: number): string => `約 ${Math.round(total * MINUTES_PER_QUESTION)} 分鐘`;

/** The counts a choice gives: a preset at this total, or the teacher's own counts under 自訂. */
export function countsFor(preset: PresetId, total: number, custom: BloomCounts | null): BloomCounts {
  if (preset === "custom") return custom ?? presetCounts(BALANCED_PCT, total);
  return presetCounts(presetById(preset)?.pct ?? BALANCED_PCT, total);
}
