import { useEffect, useRef } from "react";

/**
 * Focus care for an in-page confirm (「要復原嗎？ 確定／不要」) that replaces its trigger button:
 * opening it moves focus to the safe choice (`safeRef`); closing it hands focus back to the
 * trigger (`triggerRef`), or to `fallback` when the trigger is gone (it was confirmed and the
 * thing is deleted). Focus is only moved when it fell to `<body>` (the clicked button was
 * removed), so it never steals focus the teacher already moved elsewhere.
 */
export function useConfirmFocus<T extends HTMLElement = HTMLButtonElement>(
  asking: boolean,
  fallback?: () => HTMLElement | null,
) {
  const triggerRef = useRef<T>(null);
  const safeRef = useRef<T>(null);
  const was = useRef(asking);
  const fallbackRef = useRef(fallback);
  useEffect(() => {
    fallbackRef.current = fallback;
  });
  useEffect(() => {
    if (asking && !was.current) {
      safeRef.current?.focus();
    } else if (!asking && was.current) {
      const active = document.activeElement;
      if (!active || active === document.body) {
        const trigger = triggerRef.current;
        const target = trigger && !(trigger as unknown as HTMLButtonElement).disabled ? trigger : fallbackRef.current?.();
        target?.focus();
      }
    }
    was.current = asking;
  }, [asking]);
  return { triggerRef, safeRef };
}
