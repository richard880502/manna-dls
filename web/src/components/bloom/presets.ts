// Bloom presets for the generator (contract Addendum 1, "Frontend: Bloom presets"): a preset and a
// total become per-level counts by largest remainder. The level ids and Chinese labels are the
// contract's fixed set; lib/bloom.ts (agent D) holds the shared copy, deduped at integration.

export const BLOOM_LEVELS = ["remember", "understand", "apply", "analyze", "evaluate", "create"] as const;
export type BloomLevel = (typeof BLOOM_LEVELS)[number];

export const BLOOM_ZH: Record<BloomLevel, string> = {
  remember: "記憶",
  understand: "理解",
  apply: "應用",
  analyze: "分析",
  evaluate: "評鑑",
  create: "創造",
};

export type BloomCounts = Record<BloomLevel, number>;

export type PresetId = "foundation" | "balanced" | "challenge" | "higher" | "custom";

export interface BloomPreset {
  id: Exclude<PresetId, "custom">;
  label: string;
  /** Percentages in Bloom order; they add up to 100. */
  pct: readonly [number, number, number, number, number, number];
}

export const BLOOM_PRESETS: readonly BloomPreset[] = [
  { id: "foundation", label: "基礎鞏固", pct: [40, 40, 20, 0, 0, 0] },
  { id: "balanced", label: "均衡（預設）", pct: [15, 25, 20, 20, 10, 10] },
  { id: "challenge", label: "思考挑戰", pct: [0, 15, 20, 30, 20, 15] },
  { id: "higher", label: "深度思考", pct: [0, 0, 0, 30, 35, 35] },
];

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
 * A preset's percentages and a total as whole counts. Largest remainder (ties go to the lower
 * level), then every level with a percentage above 0 gets at least one question, taken from the
 * level with the most. When the total is smaller than the number of such levels, the levels with
 * the largest percentages get one each.
 */
export function presetCounts(pct: BloomPreset["pct"], total: number): BloomCounts {
  const levels = BLOOM_LEVELS.map((level, i) => ({ level, i, pct: pct[i] ?? 0 }));
  const wanted = levels.filter((l) => l.pct > 0);
  const counts = emptyCounts();
  if (total <= 0 || wanted.length === 0) return counts;

  if (total < wanted.length) {
    [...wanted]
      .sort((a, b) => b.pct - a.pct || a.i - b.i)
      .slice(0, total)
      .forEach((l) => {
        counts[l.level] = 1;
      });
    return counts;
  }

  // Integer arithmetic: share = total * pct / 100, kept as a floor and a remainder out of 100.
  const remainder: Record<BloomLevel, number> = emptyCounts();
  let given = 0;
  for (const l of levels) {
    counts[l.level] = Math.floor((total * l.pct) / 100);
    remainder[l.level] = (total * l.pct) % 100;
    given += counts[l.level];
  }
  const byRemainder = [...wanted].sort((a, b) => remainder[b.level] - remainder[a.level] || a.i - b.i);
  for (let k = 0; given < total; k += 1) {
    const next = byRemainder[k % byRemainder.length];
    if (!next) break;
    counts[next.level] += 1;
    given += 1;
  }

  for (const l of wanted) {
    if (counts[l.level] > 0) continue;
    // Take one from the level with the most questions (the later level when tied).
    let donor: BloomLevel | null = null;
    for (const d of BLOOM_LEVELS) {
      if (counts[d] > 1 && (donor === null || counts[d] >= counts[donor])) donor = d;
    }
    if (donor === null) break;
    counts[donor] -= 1;
    counts[l.level] = 1;
  }
  return counts;
}

export const countsTotal = (counts: BloomCounts): number => BLOOM_LEVELS.reduce((sum, l) => sum + counts[l], 0);

/** Only the levels that get questions, for the request body. */
export function nonZeroCounts(counts: BloomCounts): Partial<BloomCounts> {
  const out: Partial<BloomCounts> = {};
  for (const l of BLOOM_LEVELS) if (counts[l] > 0) out[l] = counts[l];
  return out;
}

/** 「記憶 1・理解 2・應用 2」 */
export const countsLine = (counts: BloomCounts): string =>
  BLOOM_LEVELS.filter((l) => counts[l] > 0)
    .map((l) => `${BLOOM_ZH[l]} ${counts[l]}`)
    .join("・");

/** 「約 20 分鐘」 */
export const minutesLine = (total: number): string => `約 ${Math.round(total * MINUTES_PER_QUESTION)} 分鐘`;

/** The counts a choice gives: a preset at this total, or the teacher's own counts under 自訂. */
export function countsFor(preset: PresetId, total: number, custom: BloomCounts | null): BloomCounts {
  if (preset === "custom") return custom ?? presetCounts(BALANCED_PCT, total);
  return presetCounts(presetById(preset)?.pct ?? BALANCED_PCT, total);
}
