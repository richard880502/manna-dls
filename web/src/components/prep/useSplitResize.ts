// 學習單（左）與對話（右）並排時，中間可拖動的分隔線：記住左邊學習單佔容器寬度的百分比。
import { useCallback, useEffect, useState, type KeyboardEvent, type PointerEvent } from "react";

export const DEFAULT_SHARE = 60; // the worksheet on the left, 3:2
export const SPLIT_KEY = "manna.prepSplitSheetShare";
const MIN_CHAT_REM = 20;
const MIN_SHEET_REM = 32;
const STEP = 2;
const BIG_STEP = 10;

function remPx(): number {
  try {
    return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  } catch {
    return 16;
  }
}

/** The worksheet (left) pane's allowed share, percent. Too narrow for both minimums → only the default. */
export function boundsFor(width: number): { min: number; max: number } {
  if (!(width > 0)) return { min: 40, max: 80 };
  const rem = remPx();
  const min = ((MIN_SHEET_REM * rem) / width) * 100;
  const max = 100 - ((MIN_CHAT_REM * rem) / width) * 100;
  return min > max ? { min: DEFAULT_SHARE, max: DEFAULT_SHARE } : { min, max };
}

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

function readStored(): number {
  try {
    const n = parseFloat(window.localStorage.getItem(SPLIT_KEY) ?? "");
    return Number.isFinite(n) ? n : DEFAULT_SHARE;
  } catch {
    return DEFAULT_SHARE;
  }
}

function writeStored(share: number) {
  try {
    window.localStorage.setItem(SPLIT_KEY, String(share));
  } catch {
    /* storage unavailable: the split just isn't remembered */
  }
}

/** Attach `containerRef` to the split container (it may mount late, once the page has loaded). */
export function useSplitResize() {
  const [container, containerRef] = useState<HTMLDivElement | null>(null);
  const [wanted, setWanted] = useState(readStored);
  const [bounds, setBounds] = useState(() => boundsFor(0));
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    const el = container;
    if (!el) return undefined;
    const measure = () => setBounds(boundsFor(el.getBoundingClientRect().width));
    measure();
    window.addEventListener("resize", measure);
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    observer?.observe(el);
    return () => {
      window.removeEventListener("resize", measure);
      observer?.disconnect();
    };
  }, [container]);

  const share = clamp(wanted, bounds.min, bounds.max);

  const commit = useCallback((next: number) => {
    setWanted(next);
    writeStored(next);
  }, []);
  const reset = () => commit(DEFAULT_SHARE);

  const fromPointer = (e: PointerEvent) => {
    const rect = container?.getBoundingClientRect();
    if (!rect || !(rect.width > 0)) return null;
    const b = boundsFor(rect.width);
    return clamp(((e.clientX - rect.left) / rect.width) * 100, b.min, b.max);
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    setDragging(true);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    const next = fromPointer(e);
    if (next !== null) setWanted(next);
  };
  const endDrag = (e: PointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    setDragging(false);
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    commit(share);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? BIG_STEP : STEP;
    let next: number;
    if (e.key === "ArrowLeft") next = share - step;
    else if (e.key === "ArrowRight") next = share + step;
    else if (e.key === "Home") next = bounds.min;
    else if (e.key === "End") next = bounds.max;
    else if (e.key === "Enter") next = DEFAULT_SHARE;
    else return;
    e.preventDefault();
    commit(clamp(next, bounds.min, bounds.max));
  };

  return {
    containerRef,
    share,
    dragging,
    min: bounds.min,
    max: bounds.max,
    dividerProps: {
      onPointerDown,
      onPointerMove,
      onPointerUp: endDrag,
      onPointerCancel: endDrag,
      onKeyDown,
      onDoubleClick: reset,
    },
  };
}
