import { describe, expect, it } from "vitest";
import {
  defaultParagraphs,
  groupLabel,
  initialSelection,
  paragraphRuns,
  selectionToSave,
  tickState,
  toggleMany,
  toggleRange,
  usedParagraphCount,
} from "../lib/longMaterial";

type P = { id: string; group?: string; front?: boolean };
const ordinary: P[] = ["a", "b", "c", "d"].map((id) => ({ id }));
const exam: P[] = [
  { id: "f1", front: true },
  { id: "f2", front: true },
  { id: "g1", group: "6-10" },
  { id: "g2", group: "6-10" },
  { id: "g3", group: "6-10" },
  { id: "s1" },
  { id: "h1", group: "11-12" },
  { id: "h2", group: "11-12" },
];
const ids = (set: Set<string>) => [...set].sort();

describe("paragraph picker selection", () => {
  it("an ordinary material behaves as before", () => {
    expect(ids(initialSelection(ordinary, null))).toEqual(["a", "b", "c", "d"]);
    expect(ids(initialSelection(ordinary, []))).toEqual(["a", "b", "c", "d"]);
    expect(selectionToSave(ordinary, new Set(["a", "b", "c", "d"]))).toBeNull();
    expect(selectionToSave(ordinary, new Set(["d", "a"]))).toEqual(["a", "d"]);
    expect(usedParagraphCount(ordinary)).toBe(4);
    expect(paragraphRuns(ordinary).every((r) => r.kind === "plain")).toBe(true);
  });

  it("the default leaves out the front paragraphs, unless all are front", () => {
    expect(ids(initialSelection(exam, null))).toEqual(["g1", "g2", "g3", "h1", "h2", "s1"]);
    expect(usedParagraphCount(exam)).toBe(6);
    const allFront: P[] = [{ id: "x", front: true }, { id: "y", front: true }];
    expect(defaultParagraphs(allFront)).toHaveLength(2);
    expect(selectionToSave(allFront, new Set(["x", "y"]))).toBeNull();
  });

  it("an explicit initial selection is kept", () => {
    expect(ids(initialSelection(exam, ["f1", "g1"]))).toEqual(["f1", "g1"]);
  });

  it("saving the default gives null; ticking everything incl. front gives the full list", () => {
    expect(selectionToSave(exam, initialSelection(exam, null))).toBeNull();
    const all = new Set(exam.map((p) => p.id));
    expect(selectionToSave(exam, all)).toEqual(exam.map((p) => p.id));
    expect(selectionToSave(exam, new Set(["h2", "g1"]))).toEqual(["g1", "h2"]);
  });

  it("groups consecutive paragraphs into runs", () => {
    const runs = paragraphRuns(exam);
    expect(runs.map((r) => [r.kind, r.items.map((p) => p.id)])).toEqual([
      ["front", ["f1", "f2"]],
      ["group", ["g1", "g2", "g3"]],
      ["plain", ["s1"]],
      ["group", ["h1", "h2"]],
    ]);
    // The same key apart is two groups.
    const split = paragraphRuns<P>([{ id: "1", group: "1-2" }, { id: "2" }, { id: "3", group: "1-2" }]);
    expect(split.map((r) => r.kind)).toEqual(["group", "plain", "group"]);
  });

  it("toggles a whole group and reports its tick state", () => {
    const groupIds = ["g1", "g2", "g3"];
    let chosen = new Set(["g1"]);
    expect(tickState(groupIds, chosen)).toBe("some");
    chosen = toggleMany(chosen, groupIds, true);
    expect(tickState(groupIds, chosen)).toBe("all");
    chosen = toggleMany(chosen, groupIds, false);
    expect(tickState(groupIds, chosen)).toBe("none");
  });

  it("shift range sets everything between, in both directions", () => {
    const none = new Set<string>();
    expect(ids(toggleRange(ordinary, none, "a", "c", true))).toEqual(["a", "b", "c"]);
    expect(ids(toggleRange(ordinary, none, "d", "b", true))).toEqual(["b", "c", "d"]);
    const all = new Set(["a", "b", "c", "d"]);
    expect(ids(toggleRange(ordinary, all, "c", "a", false))).toEqual(["d"]);
    expect(ids(toggleRange(ordinary, none, "a", "zz", true))).toEqual([]);
  });

  it("labels a group", () => {
    expect(groupLabel("6-10", 7)).toBe("第 6–10 題題組（7 段）");
    expect(groupLabel("6", 2)).toBe("第 6 題題組（2 段）");
  });
});
