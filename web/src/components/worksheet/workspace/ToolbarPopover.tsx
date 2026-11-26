import { useId, type ReactNode } from "react";
import { usePopover } from "../../Popover";

/**
 * A toolbar button that opens a small popover (the disclosure pattern of [`usePopover`]): Escape
 * closes it and puts focus back on the button, so does a click outside or Tab out of it.
 * `children` may be a function that gets `close`, for items that do their job and close the menu.
 */
export function ToolbarPopover({
  label,
  ariaLabel,
  panelLabel,
  className,
  panelClassName,
  children,
}: {
  label: ReactNode;
  /** The button's accessible name when `label` is only a symbol. */
  ariaLabel?: string;
  /** The popover's accessible name. */
  panelLabel: string;
  className?: string;
  panelClassName?: string;
  /** A function gets `close`, for menu items that do their job and close the popover. */
  children: ReactNode | ((close: () => void) => ReactNode);
}) {
  const id = useId();
  const menu = usePopover();
  const { open, toggle, close, rootRef, triggerRef: own, onBlur } = menu;
  return (
    <div className="popover-root ws-bar-popover" ref={rootRef} onBlur={onBlur}>
      <button
        ref={own}
        type="button"
        className={className ?? "ws-bar-tool"}
        aria-expanded={open}
        aria-controls={`${id}-panel`}
        aria-label={ariaLabel}
        onClick={toggle}
      >
        {label}
      </button>
      {open && (
        <div
          className={`popover ws-bar-panel${panelClassName ? ` ${panelClassName}` : ""}`}
          id={`${id}-panel`}
          role="group"
          aria-label={panelLabel}
          lang="zh-Hant"
        >
          {typeof children === "function" ? children(close) : children}
        </div>
      )}
    </div>
  );
}
