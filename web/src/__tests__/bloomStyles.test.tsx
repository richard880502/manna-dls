// The 建立 page's Bloom picker and the worksheet's Bloom controls (the bar that filters the editor,
// the 出題比例 cards) are separate components with separate stylesheets. They once shared
// .bloom-bar, .bloom-seg, .bloom-preset(s) and .bloom-legend, and home.css, loaded after
// worksheet.css, squashed the worksheet's clickable bar into an 8px strip with its counts cut off.
// jsdom applies no CSS, so these tests guard the names instead: no Bloom class in both, and each
// one styled only in its own component's stylesheet.
import { render, screen, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { describe, expect, it } from "vitest";
import { BloomPicker } from "../components/bloom/BloomPicker";
import { emptyCounts } from "../components/bloom/presets";
import { BloomChip, BloomDistribution, BloomSelect } from "../components/worksheet/Bloom";
import { BloomSettings } from "../components/worksheet/BloomSettings";
import { planFor, tally } from "../lib/bloom";

// node:fs, typed for the two calls made here (the web package has no Node types).
interface Fs {
  readdirSync(dir: URL, options: { recursive: true }): string[];
  readFileSync(file: URL, encoding: "utf8"): string;
}
const fs = (await import("node:fs" as string)) as Fs;

const noop = () => undefined;
const SHEET = tally([
  { bloom: "remember" },
  { bloom: "remember" },
  { bloom: "remember" },
  { bloom: "understand" },
  { bloom: "analyze" },
  { bloom: "analyze" },
  { bloom: null },
]);

/** The bloom-* classes on everything these render. */
function bloomClasses(...uis: ReactElement[]): Set<string> {
  const names = uis.flatMap((ui) => {
    const { container, unmount } = render(ui);
    const found = [...container.querySelectorAll("[class]")].flatMap((el) => [...el.classList]);
    unmount();
    return found;
  });
  return new Set(names.filter((c) => c.startsWith("bloom-")));
}

const pickerClasses = () =>
  bloomClasses(
    <BloomPicker preset="balanced" custom={emptyCounts()} onPreset={noop} onCustom={noop} />,
    <BloomPicker preset="custom" custom={{ ...emptyCounts(), remember: 2, apply: 1 }} onPreset={noop} onCustom={noop} />,
  );

const worksheetClasses = () =>
  bloomClasses(
    <BloomDistribution tally={SHEET} onFilter={noop} />,
    <BloomDistribution tally={SHEET} />,
    <BloomSettings value={planFor("balanced", 10, SHEET.counts)} onChange={noop} />,
    <BloomSettings value={planFor("custom", 0, SHEET.counts)} onChange={noop} />,
    <BloomChip level="apply" />,
    <BloomSelect label="Bloom 層次" value="apply" disabled={false} onChange={noop} />,
  );

/** Every stylesheet under src (as "styles/home.css") and the bloom-* classes its rules style. Rules
 *  that only set custom properties (the one Bloom palette, in worksheet.css) style nothing. */
function styledBloomClasses(): Map<string, Set<string>> {
  // Through a variable: Vite rewrites `new URL("…", import.meta.url)` into an asset URL.
  const here = import.meta.url;
  const src = new URL("../", here);
  const sheets = fs.readdirSync(src, { recursive: true }).filter((f) => f.endsWith(".css"));
  return new Map(
    sheets.map((file) => {
      const css = fs.readFileSync(new URL(file, src), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
      const styled = new Set<string>();
      for (const [, selector = "", body = ""] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
        const declarations = body.split(";").map((d) => d.trim()).filter(Boolean);
        if (declarations.every((d) => d.startsWith("--"))) continue;
        for (const [, name = ""] of selector.matchAll(/\.(bloom-[\w-]+)/g)) styled.add(name);
      }
      return [file.replaceAll("\\", "/"), styled] as const;
    }),
  );
}

describe("Bloom styles: the 建立 picker and the worksheet's controls stay apart", () => {
  it("they share no class name", () => {
    const worksheet = worksheetClasses();
    const picker = pickerClasses();
    expect(picker.size).toBeGreaterThan(5);
    expect(worksheet.has("bloom-bar")).toBe(true);
    expect([...picker].filter((c) => worksheet.has(c))).toEqual([]);
  });

  it("each one's classes are styled only in its own stylesheet", () => {
    const styled = styledBloomClasses();
    expect(styled.get("styles/home.css")).toContain("bloom-picker-bar");
    expect(styled.get("styles/worksheet.css")).toContain("bloom-bar");
    const elsewhere = (classes: Set<string>, own: string) =>
      [...classes].flatMap((c) =>
        [...styled].filter(([file, names]) => file !== own && names.has(c)).map(([file]) => `.${c} in ${file}`),
      );
    expect(elsewhere(pickerClasses(), "styles/home.css")).toEqual([]);
    expect(elsewhere(worksheetClasses(), "styles/worksheet.css")).toEqual([]);
  });

  it("the worksheet's bar is one button per level, named for it, showing its count", () => {
    render(<BloomDistribution tally={SHEET} onFilter={noop} />);
    const bar = within(screen.getByRole("group", { name: "Bloom 層次分布" }));
    expect(bar.getAllByRole("button", { name: /^只看/ }).map((b) => [b.getAttribute("aria-label"), b.textContent])).toEqual([
      ["只看記憶（3 題）", "3"],
      ["只看理解（1 題）", "1"],
      ["只看分析（2 題）", "2"],
      ["只看未標（1 題）", "1"],
    ]);
  });
});
