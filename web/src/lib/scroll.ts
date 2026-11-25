// Scrolling that respects the teacher: motion only when she has not asked for less of it, and a
// pending scroll gives way as soon as she scrolls, points or types.

/** True when the teacher asked the system for less motion. */
export function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Brings an element into view; instant under 「減少動態效果」, smooth otherwise. */
export function scrollToEl(el: Element | null | undefined, block: ScrollLogicalPosition = "center") {
  if (!el || typeof el.scrollIntoView !== "function") return;
  el.scrollIntoView({ behavior: prefersReducedMotion() ? "auto" : "smooth", block });
}

/** What counts as the teacher taking over the scroll position. */
const TAKEOVER = ["wheel", "pointerdown", "keydown"] as const;

export interface ScrollWhenReadyOptions {
  block?: ScrollLogicalPosition;
  /** How long the block may take to be (re)drawn before giving up. */
  waitMs?: number;
  /** Called with the element that was scrolled to (for example to start a highlight). */
  onScrolled?: (el: HTMLElement) => void;
}

/**
 * Scrolls to `#id` once it is in the DOM. If the block is drawn again soon after (a new version
 * replaces its node), the new node is scrolled to once more, unless the teacher has scrolled,
 * pointed or typed in the meantime. Returns a function that cancels everything pending.
 */
export function scrollToIdWhenReady(id: string, options: ScrollWhenReadyOptions = {}): () => void {
  const { block = "center", waitMs = 2000, onScrolled } = options;
  let done = false;
  let last: HTMLElement | null = null;
  let observer: MutationObserver | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const stop = () => {
    done = true;
    observer?.disconnect();
    if (timer !== undefined) clearTimeout(timer);
    TAKEOVER.forEach((type) => document.removeEventListener(type, stop, true));
  };
  const attempt = () => {
    if (done) return;
    const el = document.getElementById(id);
    if (!el || el === last) return;
    last = el;
    scrollToEl(el, block);
    onScrolled?.(el);
  };

  TAKEOVER.forEach((type) => document.addEventListener(type, stop, { capture: true, once: true }));
  attempt();
  if (typeof MutationObserver === "function") {
    observer = new MutationObserver(attempt);
    observer.observe(document.body, { childList: true, subtree: true });
  }
  timer = setTimeout(stop, waitMs);
  return stop;
}

/** Runs `onVisible` once, when the element first shows on screen (at once without IntersectionObserver). */
export function whenVisible(el: Element, onVisible: () => void): () => void {
  if (typeof IntersectionObserver !== "function") {
    onVisible();
    return () => undefined;
  }
  const io = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting)) {
      io.disconnect();
      onVisible();
    }
  });
  io.observe(el);
  return () => io.disconnect();
}
