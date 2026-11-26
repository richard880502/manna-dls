import type { Bloom } from "../../api/types";
import {
  BLOOM_EN,
  BLOOM_LABEL,
  BLOOM_NONE_LABEL,
  BLOOM_ORDER,
  bloomTitle,
  isBloom,
  type BloomFilter,
  type BloomTally,
} from "../../lib/bloom";

/** The colour class of a level; untagged items are 「未標」 in a neutral, dashed chip. */
export const bloomClass = (level: Bloom | "none" | null | undefined) =>
  `is-${level && level !== "none" ? level : "none"}`;

/** A small pill with the level's Chinese name; the English name is its tooltip. */
export function BloomChip({ level }: { level: Bloom | null | undefined }) {
  const known = isBloom(level) ? level : null;
  return (
    <span className={`bloom-chip ${bloomClass(known)}`} title={bloomTitle(known)} lang="zh-Hant">
      <span className="sr-only">Bloom 層次：</span>
      {known ? BLOOM_LABEL[known] : BLOOM_NONE_LABEL}
    </span>
  );
}

/** Several levels (an activity step), in Bloom order. */
export function BloomChips({ levels, label }: { levels: readonly Bloom[] | undefined; label: string }) {
  const known = BLOOM_ORDER.filter((l) => levels?.includes(l));
  if (known.length === 0) return null;
  return (
    <span className="bloom-chips" role="group" aria-label={label}>
      {known.map((l) => (
        <BloomChip key={l} level={l} />
      ))}
    </span>
  );
}

/** The editor's chip: the same pill, as a select (未標 = null). */
export function BloomSelect({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: Bloom | null | undefined;
  disabled: boolean;
  onChange: (next: Bloom | null) => void;
}) {
  const known = isBloom(value) ? value : null;
  return (
    <select
      className={`bloom-chip bloom-select ${bloomClass(known)}`}
      aria-label={label}
      title={bloomTitle(known)}
      value={known ?? ""}
      disabled={disabled}
      onChange={(e) => onChange(isBloom(e.target.value) ? e.target.value : null)}
    >
      <option value="">{BLOOM_NONE_LABEL}</option>
      {BLOOM_ORDER.map((l) => (
        <option key={l} value={l} title={BLOOM_EN[l]}>
          {BLOOM_LABEL[l]}
        </option>
      ))}
    </select>
  );
}

interface Segment {
  id: Bloom | "none";
  label: string;
  title: string;
  count: number;
}

function segmentsOf(t: BloomTally): Segment[] {
  const levels: Segment[] = BLOOM_ORDER.map((l) => ({
    id: l,
    label: BLOOM_LABEL[l],
    title: BLOOM_EN[l],
    count: t.counts[l],
  }));
  return t.none > 0 ? [...levels, { id: "none", label: BLOOM_NONE_LABEL, title: "Not tagged", count: t.none }] : levels;
}

/**
 * The Bloom distribution: a stacked bar with the count in each segment, and a legend. With
 * `onFilter` the segments are buttons that show only that level's items, and 「全部」 resets;
 * without it the bar is read-only (the session cards).
 */
export function BloomDistribution({
  tally,
  filter = null,
  onFilter,
  label = "Bloom 層次分布",
}: {
  tally: BloomTally;
  filter?: BloomFilter;
  onFilter?: (next: BloomFilter) => void;
  label?: string;
}) {
  const segments = segmentsOf(tally);
  const shown = segments.filter((s) => s.count > 0);
  if (shown.length === 0) return null;
  const interactive = onFilter !== undefined;
  const summary = shown.map((s) => `${s.label} ${s.count}`).join("、");

  return (
    <div className="bloom-dist" role="group" aria-label={label} lang="zh-Hant">
      <div className="bloom-dist-row">
        {interactive ? (
          <div className="bloom-bar">
            {shown.map((s) => (
              <button
                key={s.id}
                type="button"
                className={`bloom-seg ${bloomClass(s.id)}${filter === s.id ? " is-active" : ""}`}
                style={{ flexGrow: s.count }}
                title={`${s.title}：${s.count}`}
                aria-label={`只看${s.label}（${s.count} 題）`}
                aria-pressed={filter === s.id}
                onClick={() => onFilter(filter === s.id ? null : s.id)}
              >
                {s.count}
              </button>
            ))}
          </div>
        ) : (
          <div className="bloom-bar" role="img" aria-label={`${label}：${summary}`}>
            {shown.map((s) => (
              <span
                key={s.id}
                className={`bloom-seg ${bloomClass(s.id)}`}
                style={{ flexGrow: s.count }}
                title={`${s.title}：${s.count}`}
              >
                {s.count}
              </span>
            ))}
          </div>
        )}
        {interactive && (
          <button
            type="button"
            className="text-link is-plain bloom-all"
            aria-pressed={filter === null}
            onClick={() => onFilter(null)}
          >
            全部
          </button>
        )}
      </div>
      <ul className="bloom-legend" aria-hidden="true">
        {segments.map((s) => (
          <li key={s.id} className={s.count === 0 ? "is-zero" : ""} title={s.title}>
            <span className={`bloom-swatch ${bloomClass(s.id)}`} />
            {s.label} {s.count}
          </li>
        ))}
      </ul>
    </div>
  );
}
