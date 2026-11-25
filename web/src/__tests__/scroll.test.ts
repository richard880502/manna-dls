import { afterEach, describe, expect, it, vi } from "vitest";
import { scrollToEl, scrollToIdWhenReady } from "../lib/scroll";

function reduced(on: boolean) {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: on && q.includes("prefers-reduced-motion"),
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
}

function block(id: string) {
  const el = document.createElement("div");
  el.id = id;
  el.scrollIntoView = vi.fn();
  document.body.append(el);
  return el;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("scrollToEl", () => {
  it("scrolls smoothly by default and instantly under reduced motion", () => {
    const el = block("a");
    reduced(false);
    scrollToEl(el, "start");
    expect(el.scrollIntoView).toHaveBeenLastCalledWith({ behavior: "smooth", block: "start" });
    reduced(true);
    scrollToEl(el, "center");
    expect(el.scrollIntoView).toHaveBeenLastCalledWith({ behavior: "auto", block: "center" });
  });

  it("ignores a missing element", () => {
    expect(() => scrollToEl(null)).not.toThrow();
  });
});

describe("scrollToIdWhenReady", () => {
  it("scrolls once when the block is there, and again when its node is replaced", async () => {
    reduced(false);
    const first = block("b");
    const stop = scrollToIdWhenReady("b");
    expect(first.scrollIntoView).toHaveBeenCalledTimes(1);
    first.remove();
    const second = block("b");
    await Promise.resolve();
    await Promise.resolve();
    expect(second.scrollIntoView).toHaveBeenCalledTimes(1);
    stop();
  });

  it("waits for a block that is not drawn yet", async () => {
    reduced(false);
    const stop = scrollToIdWhenReady("late");
    const el = block("late");
    await Promise.resolve();
    await Promise.resolve();
    expect(el.scrollIntoView).toHaveBeenCalledTimes(1);
    stop();
  });

  it.each(["wheel", "pointerdown", "keydown"])("a %s from the teacher cancels the pending scroll", async (type) => {
    reduced(false);
    const first = block("c");
    scrollToIdWhenReady("c");
    document.body.dispatchEvent(new Event(type, { bubbles: true }));
    first.remove();
    const second = block("c");
    await Promise.resolve();
    await Promise.resolve();
    expect(second.scrollIntoView).not.toHaveBeenCalled();
  });
});
