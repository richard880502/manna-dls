import { useRef, type KeyboardEvent } from "react";

/** Tabs (the tab pattern): arrows move between them. */
export function Tabs<K extends string>({
  label,
  tabs,
  current,
  panelId,
  onChange,
}: {
  label: string;
  tabs: ReadonlyArray<readonly [K, string]>;
  current: K;
  panelId: string;
  onChange: (key: K) => void;
}) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const nextIndex = (index + step + tabs.length) % tabs.length;
    const next = tabs[nextIndex];
    if (!next) return;
    onChange(next[0]);
    refs.current[nextIndex]?.focus();
  };
  return (
    <div className="lib-tabs" role="tablist" aria-label={label}>
      {tabs.map(([key, text], i) => (
        <button
          key={key}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="tab"
          className="lib-tab"
          aria-selected={current === key}
          aria-controls={panelId}
          onClick={() => onChange(key)}
          onKeyDown={(e) => onKeyDown(e, i)}
        >
          {text}
        </button>
      ))}
    </div>
  );
}
