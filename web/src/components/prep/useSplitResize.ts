// 學習單（左）與對話（右）並排時，中間可拖動的分隔線：記住左邊學習單佔容器寬度的百分比。
import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

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

/** worksheet | 0-width divider track | conversation */
export const splitColumns = (share: number) => `minmax(0, ${share}%) 0 minmax(0, 1fr)`;

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
  const canResize = bounds.max - bounds.min > 0.01; // too narrow for both minimum widths → nothing to drag

  const commit = useCallback((next: number) => {
    setWanted(next);
    writeStored(next);
  }, []);
  const reset = () => commit(DEFAULT_SHARE);

  // Drag path: measure once on pointerdown, then write the container's grid-template-columns directly
  // (one write per animation frame, no React state, no inherited CSS variable). Only pointerup commits.
  const drag = useRef<{
    el: HTMLDivElement;
    rect: DOMRect;
    min: number;
    max: number;
    offset: number;
    next: number;
    moved: boolean;
    raf: number;
  } | null>(null);

  const writeColumns = (d: NonNullable<typeof drag.current>) => {
    d.el.style.gridTemplateColumns = splitColumns(d.next);
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !container || !canResize) return;
    const rect = container.getBoundingClientRect();
    if (!(rect.width > 0)) return;
    e.preventDefault(); // no text selection from the gap between panes
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const handle = e.currentTarget.getBoundingClientRect();
    const b = boundsFor(rect.width);
    drag.current = {
      el: container,
      rect,
      min: b.min,
      max: b.max,
      offset: e.clientX - (handle.left + handle.width / 2), // grab offset: no jump on the first move
      next: share,
      moved: false,
      raf: 0,
    };
    setDragging(true); // the only render at the start of a drag
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    d.next = clamp(((e.clientX - d.offset - d.rect.left) / d.rect.width) * 100, d.min, d.max);
    d.moved = true;
    if (d.raf) return;
    d.raf = requestAnimationFrame(() => {
      d.raf = 0;
      writeColumns(d);
    });
  };
  const endDrag = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    if (d.raf) cancelAnimationFrame(d.raf);
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    if (d.moved) {
      writeColumns(d); // the final value, even if the last frame hasn't run
      commit(d.next); // the only state + storage write of the drag
    }
    setDragging(false);
  };

  useEffect(
    () => () => {
      if (drag.current?.raf) cancelAnimationFrame(drag.current.raf);
    },
    [],
  );

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
    canResize,
    columns: splitColumns(share),
    edge: !canResize ? null : share <= bounds.min + 0.01 ? ("min" as const) : share >= bounds.max - 0.01 ? ("max" as const) : null,
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
