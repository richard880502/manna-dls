// FLIP for a list that reorders in one step (上移／下移): measure, change, measure again, then play
// the difference as a transform so the teacher sees where each block went. Transform only; the
// layout itself changes at once.

/** `--ease-in-out` of tokens.css: things that move on screen. */
export const FLIP_EASING = "cubic-bezier(0.65, 0, 0.35, 1)";
export const FLIP_MS = 200;

/** The animation now running on each element, so a second move starts clean. */
const running = new WeakMap<Element, Animation>();

export interface FlipOptions {
  /** The elements of the list, in any order. Called before and after `change`. */
  items: () => HTMLElement[];
  /** Applies the change synchronously (the caller wraps it in `flushSync`). */
  change: () => void;
  /** Play the movement; false just applies the change. */
  animate: boolean;
}

/** Records the tops, applies the change, and animates every element whose top moved. */
export function flip({ items, change, animate }: FlipOptions): void {
  if (!animate) {
    change();
    return;
  }
  // A move still playing would make the measurement include its own transform.
  for (const el of items()) {
    running.get(el)?.cancel();
    running.delete(el);
  }
  const before = new Map(items().map((el) => [el, el.getBoundingClientRect().top]));
  change();
  for (const el of items()) {
    const was = before.get(el);
    // Gone from the page, or new to it: nothing to carry over.
    if (was === undefined || !el.isConnected || typeof el.animate !== "function") continue;
    const dy = was - el.getBoundingClientRect().top;
    if (Math.abs(dy) < 1) continue;
    const anim = el.animate([{ transform: `translateY(${dy}px)` }, { transform: "none" }], {
      duration: FLIP_MS,
      easing: FLIP_EASING,
    });
    running.set(el, anim);
    const done = () => {
      if (running.get(el) === anim) running.delete(el);
    };
    anim.onfinish = done;
    anim.oncancel = done;
  }
}
