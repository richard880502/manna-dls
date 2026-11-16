import { useId } from "react";
import type { Bloom } from "../../api/types";
import {
  BLOOM_EN,
  BLOOM_LABEL,
  BLOOM_ORDER,
  BLOOM_PRESETS,
  LEVEL_MAX,
  TOTAL_MAX,
  TOTAL_MIN,
  countsTotal,
  planFor,
  presetMinTotal,
  type BloomPlan,
  type BloomPreset,
  type BloomPresetId,
} from "../../lib/bloom";
import { BloomChip, bloomClass } from "./Bloom";

/** − n ＋, one question at a time. */
export function Stepper({
  label,
  value,
  min,
  max,
  disabled,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  disabled: boolean;
  onChange: (next: number) => void;
}) {
  return (
    <span className="bloom-stepper" role="group" aria-label={label}>
      <button
        type="button"
        aria-label={`${label}少一題`}
        disabled={disabled || value <= min}
        onClick={() => onChange(value - 1)}
      >
        −
      </button>
      <output aria-live="polite">{value}</output>
      <button
        type="button"
        aria-label={`${label}多一題`}
        disabled={disabled || value >= max}
        onClick={() => onChange(value + 1)}
      >
        ＋
      </button>
    </span>
  );
}

/** A preset's shares as a stacked bar in the six level colours (decorative; the text says it too). */
function PresetBar({ preset }: { preset: BloomPreset }) {
  const percent = preset.percent;
  if (!percent) return <span className="bloom-preset-bar is-custom" aria-hidden="true" />;
  return (
    <span className="bloom-preset-bar" aria-hidden="true">
      {BLOOM_ORDER.filter((l) => percent[l] > 0).map((l) => (
        <span key={l} className={`bloom-swatch ${bloomClass(l)}`} style={{ flexGrow: percent[l] }} />
      ))}
    </span>
  );
}

const shareLine = (preset: BloomPreset) =>
  preset.percent
    ? BLOOM_ORDER.filter((l) => preset.percent![l] > 0)
        .map((l) => `${BLOOM_LABEL[l]} ${preset.percent![l]}%`)
        .join("、")
    : "每個層次自己調題數";

/**
 * 出題比例 (Addendum 1, like Eduaide's cognitive-demand presets): preset cards with their stacked
 * bars, a legend, and the total; 自訂 gives a stepper per level. Controlled: `value` in, the whole
 * next value out (the counts are always worked out here, by largest remainder).
 */
export function BloomSettings({
  value,
  onChange,
  disabled = false,
}: {
  value: BloomPlan;
  onChange: (next: BloomPlan) => void;
  disabled?: boolean;
}) {
  const ids = useId();
  const custom = value.preset === "custom";
  const total = countsTotal(value.counts);

  const pick = (preset: BloomPresetId) => onChange(planFor(preset, value.total, value.counts));
  const setLevel = (level: Bloom, n: number) => {
    const counts = { ...value.counts, [level]: n };
    onChange({ preset: "custom", counts, total: countsTotal(counts) });
  };

  return (
    <div className="bloom-settings">
      <div className="bloom-presets" role="radiogroup" aria-label="出題比例">
        {BLOOM_PRESETS.map((p) => (
          <label key={p.id} className={`bloom-preset${value.preset === p.id ? " is-current" : ""}`}>
            <input
              type="radio"
              name={`${ids}-preset`}
              checked={value.preset === p.id}
              disabled={disabled}
              aria-describedby={`${ids}-${p.id}`}
              onChange={() => pick(p.id)}
            />
            <span className="bloom-preset-name">{p.label}</span>
            <PresetBar preset={p} />
            <span className="bloom-preset-share" id={`${ids}-${p.id}`}>
              {shareLine(p)}
            </span>
          </label>
        ))}
      </div>
      <ul className="bloom-legend" aria-hidden="true">
        {BLOOM_ORDER.map((l) => (
          <li key={l} title={BLOOM_EN[l]}>
            <span className={`bloom-swatch ${bloomClass(l)}`} />
            {BLOOM_LABEL[l]}
          </li>
        ))}
      </ul>

      {custom ? (
        <ul className="bloom-levels" aria-label="每個層次的題數">
          {BLOOM_ORDER.map((l) => (
            <li key={l}>
              <BloomChip level={l} />
              <Stepper
                label={BLOOM_LABEL[l]}
                value={value.counts[l]}
                // The whole stays 3–30: at 3 nothing can go down, at 30 nothing can go up.
                min={total > TOTAL_MIN ? 0 : value.counts[l]}
                max={Math.min(LEVEL_MAX, value.counts[l] + (TOTAL_MAX - total))}
                disabled={disabled}
                onChange={(n) => setLevel(l, n)}
              />
            </li>
          ))}
        </ul>
      ) : (
        <p className="bloom-total">
          <span aria-hidden="true">題數</span>
          <Stepper
            label="題數"
            value={total}
            min={presetMinTotal(value.preset)}
            max={TOTAL_MAX}
            disabled={disabled}
            onChange={(n) => onChange(planFor(value.preset, n, value.counts))}
          />
        </p>
      )}
      {custom && total <= TOTAL_MIN && <p className="field-hint">至少要 {TOTAL_MIN} 題。</p>}
    </div>
  );
}
