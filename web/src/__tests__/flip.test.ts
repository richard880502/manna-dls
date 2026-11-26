import { afterEach, describe, expect, it, vi } from "vitest";
import { FLIP_EASING, flip } from "../lib/flip";

function el(top: () => number) {
  const e = document.createElement("div");
  e.getBoundingClientRect = () => ({ top: top() }) as DOMRect;
  e.animate = vi.fn(() => ({ cancel: vi.fn() }) as unknown as Animation);
  document.body.append(e);
  return e;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("flip", () => {
  it("plays the distance each moved element travelled, transform only", () => {
    let swapped = false;
    const a = el(() => (swapped ? 100 : 0));
    const b = el(() => (swapped ? 0 : 100));
    const c = el(() => 200);
    flip({ items: () => [a, b, c], change: () => (swapped = true), animate: true });
    expect(a.animate).toHaveBeenCalledWith([{ transform: "translateY(-100px)" }, { transform: "none" }], {
      duration: 200,
      easing: FLIP_EASING,
    });
    expect(b.animate).toHaveBeenCalledWith(
      [{ transform: "translateY(100px)" }, { transform: "none" }],
      expect.anything(),
    );
    expect(c.animate).not.toHaveBeenCalled();
  });

  it("only applies the change when not animating", () => {
    const a = el(() => 0);
    const change = vi.fn();
    flip({ items: () => [a], change, animate: false });
    expect(change).toHaveBeenCalledOnce();
    expect(a.animate).not.toHaveBeenCalled();
  });

  it("cancels a running animation before measuring again", () => {
    const cancel = vi.fn();
    let top = 0;
    const a = el(() => top);
    a.animate = vi.fn(() => ({ cancel }) as unknown as Animation);
    flip({ items: () => [a], change: () => (top = 50), animate: true });
    flip({ items: () => [a], change: () => (top = 0), animate: true });
    expect(cancel).toHaveBeenCalledOnce();
  });

  it("copes with no WAAPI and with elements that left the page", () => {
    let top = 0;
    const a = el(() => top);
    const gone = el(() => top);
    (a as { animate?: unknown }).animate = undefined;
    expect(() =>
      flip({
        items: () => [a, gone],
        change: () => {
          top = 40;
          gone.remove();
        },
        animate: true,
      }),
    ).not.toThrow();
    expect(gone.animate).not.toHaveBeenCalled();
  });
});
