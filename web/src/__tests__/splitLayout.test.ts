// The prep page's split: how far the divider can move at laptop widths, and when the sidebar folds by itself.
import { afterEach, describe, expect, it } from "vitest";
import { MIN_CHAT_REM, MIN_SHEET_REM, boundsFor } from "../components/prep/useSplitResize";
import { autoCollapsesSidebar } from "../components/Layout";
import { loadSidebarPreference, saveSidebarCollapsed } from "../lib/storage";

const REM = 16; // jsdom has no computed font size: the hook falls back to 16px
const FULL_SIDEBAR = 15.5 * REM;
const ICON_SIDEBAR = 4.25 * REM;
const px = (viewport: number, sidebar: number) => viewport - sidebar;
const range = (width: number) => {
  const { min, max } = boundsFor(width);
  return { min, max, spanPx: ((max - min) / 100) * width };
};

describe("boundsFor (the divider's range)", () => {
  it("keeps the conversation at 22rem and the worksheet at 32rem", () => {
    expect(MIN_CHAT_REM).toBe(22);
    expect(MIN_SHEET_REM).toBe(32);
    const width = 1200;
    const { min, max } = boundsFor(width);
    expect((min / 100) * width).toBeCloseTo(32 * REM);
    expect(((100 - max) / 100) * width).toBeCloseTo(22 * REM);
  });

  it("is only the default when both minimums do not fit (1100px with the full sidebar)", () => {
    const { min, max } = boundsFor(px(1100, FULL_SIDEBAR));
    expect(min).toBe(max);
  });

  it("leaves a usable drag range with the folded sidebar at 1100px (>= 160px)", () => {
    expect(range(px(1100, ICON_SIDEBAR)).spanPx).toBeGreaterThan(160);
  });

  it("leaves a usable range at 1280px, full or folded sidebar", () => {
    expect(range(px(1280, FULL_SIDEBAR)).spanPx).toBeGreaterThan(160);
    expect(range(px(1280, ICON_SIDEBAR)).spanPx).toBeGreaterThan(280);
  });

  it("falls back to 40-80 before the container is measured", () => {
    expect(boundsFor(0)).toEqual({ min: 40, max: 80 });
  });
});

describe("the sidebar's automatic fold", () => {
  afterEach(() => window.localStorage.clear());

  it("folds only on a prep session page narrower than 80rem", () => {
    expect(autoCollapsesSidebar("/prep/12", true)).toBe(true);
    expect(autoCollapsesSidebar("/prep/12", false)).toBe(false);
    expect(autoCollapsesSidebar("/prep", true)).toBe(false);
    expect(autoCollapsesSidebar("/", true)).toBe(false);
  });

  it("tells her own choice from never having chosen", () => {
    expect(loadSidebarPreference()).toBeNull();
    saveSidebarCollapsed(false);
    expect(loadSidebarPreference()).toBe(false);
    saveSidebarCollapsed(true);
    expect(loadSidebarPreference()).toBe(true);
  });
});
