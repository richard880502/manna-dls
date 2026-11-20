import { ANSWER_FORMATS, STUDENT_TASK_GROUPS } from "../../lib/worksheet";

/** One of the generator's option chips; a star (✦) marks what Manna chose first. */
export function Chip({
  label,
  checked,
  starred,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  starred: boolean;
  disabled: boolean;
  onChange: () => void;
}) {
  return (
    <label className={`ws-chip${starred ? " is-suggested" : ""}`}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={onChange} />
      <span>
        {starred && <span className="star" aria-hidden="true" />}
        {label}
        {starred && <span className="sr-only">（Manna 先選的）</span>}
      </span>
    </label>
  );
}

interface RowProps {
  selected: readonly string[];
  starred?: readonly string[];
  disabled?: boolean;
  onToggle: (label: string) => void;
}

/** A row of chips for `labels`: `selected` are ticked, `starred` carry the star. */
export function ChipRow({ labels, selected, starred = [], disabled = false, onToggle }: RowProps & { labels: readonly string[] }) {
  return (
    <div className="ws-chips">
      {labels.map((label) => (
        <Chip
          key={label}
          label={label}
          checked={selected.includes(label)}
          starred={starred.includes(label)}
          disabled={disabled}
          onChange={() => onToggle(label)}
        />
      ))}
    </div>
  );
}

/** 學生任務 (spec §5.2): the generator's five groups of chips. */
export function StudentTaskChips(props: RowProps) {
  return (
    <div>
      {STUDENT_TASK_GROUPS.map((group) => (
        <div key={group.title} role="group" aria-label={group.title}>
          <p className="ws-sub">{group.title}</p>
          <ChipRow labels={group.labels} {...props} />
        </div>
      ))}
    </div>
  );
}

/** 作答方式 (spec §5.3). */
export function AnswerFormatChips(props: RowProps) {
  return <ChipRow labels={ANSWER_FORMATS} {...props} />;
}
