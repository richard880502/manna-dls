import type { ReactNode } from "react";

interface PlanSectionProps {
  /** Prefix for the heading and body ids. */
  id: string;
  title: string;
  /** Shown as 「title（count）」; null or undefined shows the title alone (e.g. while loading). */
  count?: number | null;
  /** A short tag after the toggle, e.g. 已選 2 or 尚未儲存. */
  tag?: ReactNode;
  open: boolean;
  onToggle: () => void;
  /** h4 inside a card that has its own h3; h3 when the section is the card's only head. */
  level?: 3 | 4;
  children: ReactNode;
}

/**
 * One collapsible row of the lesson page (docs/UX_REVIEW.md R1). The body stays mounted while
 * closed, so half-written fields and loaded lists survive a collapse; `hidden` keeps it out of
 * view and out of the accessibility tree.
 */
export function PlanSection({ id, title, count, tag, open, onToggle, level = 3, children }: PlanSectionProps) {
  const label = typeof count === "number" ? `${title}（${count}）` : title;
  const Heading = level === 3 ? "h3" : "h4";
  return (
    <section className={`plan-section${open ? " is-open" : ""}`} aria-labelledby={`${id}-heading`}>
      <div className="plan-section-head">
        <Heading id={`${id}-heading`} className="plan-section-title">
          <button
            type="button"
            className="plan-section-toggle"
            aria-expanded={open}
            aria-controls={`${id}-body`}
            onClick={onToggle}
          >
            {label}
          </button>
        </Heading>
        {tag}
      </div>
      <div id={`${id}-body`} className="plan-section-body" hidden={!open}>
        {children}
      </div>
    </section>
  );
}
