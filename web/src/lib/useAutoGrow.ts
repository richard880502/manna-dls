import { useLayoutEffect, type RefObject } from "react";

/**
 * A textarea that grows with what is typed. Its CSS sets the floor and the ceiling (min-height
 * and max-height in lines); past the ceiling it scrolls.
 */
export function useAutoGrow(ref: RefObject<HTMLTextAreaElement | null>, value: string) {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    if (el.scrollHeight > 0) el.style.height = `${el.scrollHeight}px`;
  }, [ref, value]);
}
