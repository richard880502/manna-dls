/** 「− 8 ＋」: a count the teacher nudges one at a time (題數, and a Bloom level in 自訂). */
export function Stepper({
  label,
  value,
  min,
  max,
  disabled,
  onChange,
}: {
  /** Names the group and both buttons (「題數：多一題」). */
  label: string;
  value: number;
  min: number;
  max: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div className="stepper" role="group" aria-label={label}>
      <button
        type="button"
        className="stepper-button"
        aria-label={`${label}：少一題`}
        disabled={disabled || value <= min}
        onClick={() => onChange(Math.max(min, value - 1))}
      >
        <span aria-hidden="true">−</span>
      </button>
      <span className="stepper-value" aria-live="polite">
        {value}
      </span>
      <button
        type="button"
        className="stepper-button"
        aria-label={`${label}：多一題`}
        disabled={disabled || value >= max}
        onClick={() => onChange(Math.min(max, value + 1))}
      >
        <span aria-hidden="true">＋</span>
      </button>
    </div>
  );
}
