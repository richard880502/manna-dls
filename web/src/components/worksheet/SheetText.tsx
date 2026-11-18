import { useLayoutEffect, useRef } from "react";

/** A text field that reads as text on the sheet and grows with what is typed. */
export function SheetText({
  label,
  value,
  onChange,
  disabled,
  placeholder,
  className = "",
  lang = "en",
}: {
  label: string;
  value: string;
  onChange: (next: string) => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
  lang?: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !el.scrollHeight) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);
  return (
    <textarea
      ref={ref}
      className={`ws-text ${className}`}
      rows={1}
      lang={lang}
      aria-label={label}
      value={value}
      placeholder={placeholder}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
