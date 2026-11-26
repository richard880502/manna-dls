import { useId } from "react";
import { Stepper } from "../Stepper";
import {
  BLOOM_LEVELS,
  BLOOM_PRESETS,
  BLOOM_ZH,
  CUSTOM_LABEL,
  LEVEL_MAX,
  TOTAL_MAX,
  TOTAL_MIN,
  countsTotal,
  type BloomCounts,
  type PresetId,
} from "./presets";

/** A stacked bar in the six Bloom colours; each part as wide as its share. */
function BloomBar({ shares }: { shares: readonly number[] }) {
  const sum = shares.reduce((a, b) => a + b, 0);
  return (
    <span className={`bloom-picker-bar${sum === 0 ? " is-empty" : ""}`} aria-hidden="true">
      {BLOOM_LEVELS.map((level, i) =>
        (shares[i] ?? 0) > 0 ? (
          <span key={level} className={`bloom-picker-seg is-${level}`} style={{ flexGrow: shares[i] }} />
        ) : null,
      )}
    </span>
  );
}

/** 「記憶 40%、理解 40%、應用 20%」, read out with each preset. */
const shareText = (shares: readonly number[], unit: string) =>
  BLOOM_LEVELS.map((l, i) => ((shares[i] ?? 0) > 0 ? `${BLOOM_ZH[l]} ${shares[i]}${unit}` : null))
    .filter(Boolean)
    .join("、");

/**
 * 認知層次（Bloom）: preset cards (基礎鞏固／均衡／思考挑戰／深度思考／自訂), each a stacked bar
 * with a legend under them; 自訂 opens a stepper per level.
 */
export function BloomPicker({
  preset,
  custom,
  disabled,
  onPreset,
  onCustom,
}: {
  preset: PresetId;
  /** The per-level counts used while 自訂 is chosen. */
  custom: BloomCounts;
  disabled?: boolean;
  onPreset: (id: PresetId) => void;
  onCustom: (counts: BloomCounts) => void;
}) {
  const ids = useId();
  const customShares = BLOOM_LEVELS.map((l) => custom[l]);
  const customTotal = countsTotal(custom);
  const customOff = customTotal < TOTAL_MIN || customTotal > TOTAL_MAX;

  return (
    <fieldset className="bloom-picker" disabled={disabled}>
      <legend className="field-label">認知層次（Bloom）</legend>
      <div className="bloom-picker-presets">
        {BLOOM_PRESETS.map((p) => (
          <label key={p.id} className="bloom-picker-preset">
            <input
              type="radio"
              name={`${ids}-preset`}
              value={p.id}
              checked={preset === p.id}
              aria-describedby={`${ids}-${p.id}`}
              onChange={() => onPreset(p.id)}
            />
            <span className="bloom-picker-name">{p.label}</span>
            <BloomBar shares={p.pct} />
          </label>
        ))}
        <label className="bloom-picker-preset">
          <input
            type="radio"
            name={`${ids}-preset`}
            value="custom"
            checked={preset === "custom"}
            onChange={() => onPreset("custom")}
          />
          <span className="bloom-picker-name">{CUSTOM_LABEL}</span>
          <BloomBar shares={preset === "custom" ? customShares : []} />
        </label>
      </div>

      {/* Each preset's shares, read out as its description (outside the labels, so not in the names). */}
      <div hidden>
        {BLOOM_PRESETS.map((p) => (
          <span key={p.id} id={`${ids}-${p.id}`}>
            {shareText(p.pct, "%")}
          </span>
        ))}
      </div>

      <ul className="bloom-picker-legend" aria-label="顏色說明">
        {BLOOM_LEVELS.map((l) => (
          <li key={l}>
            <span className={`bloom-dot is-${l}`} aria-hidden="true" />
            {BLOOM_ZH[l]}
          </li>
        ))}
      </ul>

      {preset === "custom" && (
        <div className="bloom-custom" role="group" aria-label="每個層次的題數">
          {BLOOM_LEVELS.map((l) => (
            <div key={l} className="bloom-custom-row">
              <span className="bloom-custom-name">
                <span className={`bloom-dot is-${l}`} aria-hidden="true" />
                {BLOOM_ZH[l]}
              </span>
              <Stepper
                label={BLOOM_ZH[l]}
                value={custom[l]}
                min={0}
                max={LEVEL_MAX}
                onChange={(n) => onCustom({ ...custom, [l]: n })}
              />
            </div>
          ))}
          <p className={customOff ? "error-text" : "field-hint"} role={customOff ? "alert" : undefined}>
            {customOff ? `合計 ${customTotal} 題；要在 ${TOTAL_MIN} 到 ${TOTAL_MAX} 題之間。` : `合計 ${customTotal} 題`}
          </p>
        </div>
      )}
    </fieldset>
  );
}
