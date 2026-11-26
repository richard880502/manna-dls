// 圖像組織 (docs/API.md「圖像組織」): the eight organizer types, their limits and default labels
// (the server's, api/src/worksheet/organizer.rs), and what one version prints for one audience.
import type {
  WsItem,
  WsLevel,
  WsOrganizer,
  WsOrganizerCell,
  WsOrganizerSection,
  WsOrganizerType,
  WsSheetOrganizer,
} from "../api/types";

export const FMT_ORGANIZER = "圖像組織";

export const ORGANIZER_TYPES: readonly WsOrganizerType[] = [
  "t_chart",
  "venn",
  "sequence",
  "cause_effect",
  "problem_solution",
  "story_map",
  "kwl",
  "mind_map",
];

/** The teacher's names of the types. */
export const ORGANIZER_LABEL: Record<WsOrganizerType, string> = {
  t_chart: "T 字表",
  venn: "文氏圖",
  sequence: "順序鏈",
  cause_effect: "因果鏈",
  problem_solution: "問題與解決",
  story_map: "故事地圖",
  kwl: "KWL 表",
  mind_map: "心智圖",
};

/** The English names (accessibility text, as the server's `name`). */
export const ORGANIZER_NAME: Record<WsOrganizerType, string> = {
  t_chart: "T-chart",
  venn: "Venn diagram",
  sequence: "Sequence chain",
  cause_effect: "Cause and effect chain",
  problem_solution: "Problem and solution",
  story_map: "Story map",
  kwl: "KWL chart",
  mind_map: "Mind map",
};

/** How a type is laid out: columns, boxes with arrows, two circles, a web, labelled rows. */
export type OrganizerFamily = "columns" | "chain" | "venn" | "web" | "rows";

export function organizerFamily(type: WsOrganizerType): OrganizerFamily {
  switch (type) {
    case "t_chart":
    case "kwl":
      return "columns";
    case "sequence":
    case "cause_effect":
    case "problem_solution":
      return "chain";
    case "venn":
      return "venn";
    case "mind_map":
      return "web";
    case "story_map":
      return "rows";
  }
}

export const MAX_SECTIONS = 6;
export const MAX_CELLS = 6;
/** Boxes per row of a chain. */
export const CHAIN_PER_ROW = 3;

/** Fewest and most sections of a type. */
export function sectionRange(type: WsOrganizerType): [number, number] {
  switch (type) {
    case "t_chart":
      return [2, 2];
    case "venn":
    case "kwl":
      return [3, 3];
    case "sequence":
    case "mind_map":
      return [3, 6];
    case "cause_effect":
      return [2, 6];
    case "problem_solution":
      return [2, 4];
    case "story_map":
      return [5, 5];
  }
}

/** Most cells of one section of a type. */
export function maxCells(type: WsOrganizerType): number {
  switch (organizerFamily(type)) {
    case "columns":
      return MAX_CELLS;
    case "venn":
      return 5;
    case "rows":
      return 4;
    case "web":
      return 3;
    case "chain":
      return 2;
  }
}

function newCells(type: WsOrganizerType, index: number): number {
  if (type === "t_chart" || type === "kwl" || type === "venn") return 3;
  if (type === "story_map" && index === 3) return 3;
  if (type === "mind_map") return 2;
  return 1;
}

/** The label a section at `index` of `count` gets when it has none (the server's). */
export function defaultLabel(type: WsOrganizerType, index: number, count: number): string {
  const pick = (list: string[]) => list[index] ?? "";
  switch (type) {
    case "t_chart":
      return pick(["Topic A", "Topic B"]);
    case "venn":
      return pick(["Topic A", "Both", "Topic B"]);
    case "kwl":
      return pick(["What I Know", "What I Want to Know", "What I Learned"]);
    case "story_map":
      return pick(["Setting", "Characters", "Problem", "Events", "Solution"]);
    case "sequence":
      if (index === 0) return "First";
      if (index + 1 === count) return "Finally";
      return pick(["", "Next", "Then", "After that", "Later"]);
    case "cause_effect":
      return index === 0 ? "Cause" : "Effect";
    case "problem_solution":
      if (index === 0) return "Problem";
      if (index + 1 === count && count >= 3) return "Result";
      return "Solution";
    case "mind_map":
      return `Idea ${index + 1}`;
  }
}

const blankCell = (): WsOrganizerCell => ({ answer: "", given: "", prefilled: [] });

/** A new organizer of a type: the fewest sections, default labels, empty cells. */
export function newOrganizer(type: WsOrganizerType = "t_chart"): WsOrganizer {
  const [min] = sectionRange(type);
  return {
    type,
    title: "",
    center: type === "mind_map" ? "Main idea" : "",
    sections: Array.from({ length: min }, (_, i) => ({
      label: defaultLabel(type, i, min),
      cells: Array.from({ length: newCells(type, i) }, blankCell),
    })),
  };
}

/** Whether `label` is one the type gives some section by default (the teacher has not written it). */
function isDefaultLabel(type: WsOrganizerType, label: string): boolean {
  const t = label.trim();
  if (t === "") return true;
  for (let count = 1; count <= MAX_SECTIONS; count++)
    for (let i = 0; i < count; i++) if (defaultLabel(type, i, count) === t) return true;
  return false;
}

/** Labels still at a default follow the section's new place (a sequence's last box is "Finally"). */
function relabel(org: WsOrganizer, from: WsOrganizerType): WsOrganizer {
  const n = org.sections.length;
  return {
    ...org,
    sections: org.sections.map((s, i) =>
      isDefaultLabel(from, s.label) ? { ...s, label: defaultLabel(org.type, i, n) } : s,
    ),
  };
}

/** The organizer fitted to its type: sections and cells within its limits, every section labelled
 * and with a cell, the centre only on a mind map. */
export function fitOrganizer(org: WsOrganizer): WsOrganizer {
  const [min, max] = sectionRange(org.type);
  const sections = org.sections.slice(0, max);
  while (sections.length < min)
    sections.push({ label: "", cells: Array.from({ length: newCells(org.type, sections.length) }, blankCell) });
  const n = sections.length;
  const cap = maxCells(org.type);
  return {
    ...org,
    center: org.type === "mind_map" ? org.center.trim() || org.title.trim() || "Main idea" : "",
    sections: sections.map((s, i) => {
      const cells = s.cells.slice(0, cap);
      return {
        label: s.label.trim() === "" ? defaultLabel(org.type, i, n) : s.label,
        cells: cells.length > 0 ? cells : [blankCell()],
      };
    }),
  };
}

/** The organizer as another type: what the teacher wrote stays where it fits, labels still at the
 * old type's defaults become the new type's. */
export function convertOrganizer(org: WsOrganizer, type: WsOrganizerType): WsOrganizer {
  if (org.type === type) return org;
  // The new type's number of sections first, so default labels follow their final places.
  const [min, max] = sectionRange(type);
  const sections = org.sections.slice(0, max);
  while (sections.length < min) sections.push({ label: "", cells: [blankCell()] });
  return fitOrganizer(relabel({ ...org, type, sections }, org.type));
}

/** One more section (box, branch, column) at the end, when the type allows it. */
export function addSection(org: WsOrganizer): WsOrganizer {
  if (org.sections.length >= sectionRange(org.type)[1]) return org;
  const next: WsOrganizer = {
    ...org,
    sections: [...org.sections, { label: "", cells: Array.from({ length: newCells(org.type, org.sections.length) }, blankCell) }],
  };
  return fitOrganizer(relabel(next, org.type));
}

/** Removes a section, when the type has more than its fewest. */
export function removeSection(org: WsOrganizer, index: number): WsOrganizer {
  if (org.sections.length <= sectionRange(org.type)[0]) return org;
  return relabel({ ...org, sections: org.sections.filter((_, i) => i !== index) }, org.type);
}

export function patchSection(
  org: WsOrganizer,
  index: number,
  change: (s: WsOrganizerSection) => WsOrganizerSection,
): WsOrganizer {
  return { ...org, sections: org.sections.map((s, i) => (i === index ? change(s) : s)) };
}

export function patchCell(
  org: WsOrganizer,
  si: number,
  ci: number,
  change: (c: WsOrganizerCell) => WsOrganizerCell,
): WsOrganizer {
  return patchSection(org, si, (s) => ({ ...s, cells: s.cells.map((c, i) => (i === ci ? change(c) : c)) }));
}

/** Whether the student sheet of `level` prints this cell filled in. */
export const givenIn = (cell: WsOrganizerCell, level: WsLevel) =>
  cell.prefilled.includes(level) && cell.given.trim() !== "";

/**
 * Turns the pre-filling of a cell on or off in the version shown. With several versions and a
 * 差異化學習單 question (`perVersion`) only this version changes; otherwise every version does. Turned
 * on, a cell without its own text prints its answer.
 */
export function togglePrefill(
  cell: WsOrganizerCell,
  level: WsLevel,
  allLevels: readonly WsLevel[],
  perVersion: boolean,
): WsOrganizerCell {
  const on = cell.prefilled.includes(level);
  const levels: readonly WsLevel[] = perVersion ? [level] : allLevels.length > 0 ? allLevels : [level];
  const order: WsLevel[] = ["basic", "standard", "advanced"];
  const prefilled = on
    ? cell.prefilled.filter((l) => !levels.includes(l))
    : order.filter((l) => cell.prefilled.includes(l) || levels.includes(l));
  const given = !on && cell.given.trim() === "" ? cell.answer : cell.given;
  return { ...cell, given, prefilled };
}

/** What one version prints for one audience (the server's projection). */
export function projectOrganizer(org: WsOrganizer, level: WsLevel, teacher: boolean): WsSheetOrganizer {
  return {
    type: org.type,
    name: ORGANIZER_NAME[org.type],
    title: org.title.trim(),
    center: org.center.trim(),
    sections: org.sections.map((s) => ({
      label: s.label.trim(),
      cells: s.cells.map((c) => {
        const given = givenIn(c, level);
        return {
          given: given ? c.given.trim() : "",
          ...(teacher && !given && c.answer.trim() !== "" ? { answer: c.answer.trim() } : {}),
        };
      }),
    })),
  };
}

/** The organizer the sheet draws: the item's, while its format is 圖像組織. */
export function printedOrganizer(item: Pick<WsItem, "answer_format" | "organizer">): WsOrganizer | null {
  return item.answer_format === FMT_ORGANIZER && item.organizer ? item.organizer : null;
}

/** An item switched to 圖像組織 without an organizer gets a T-chart with editable headings. */
export function withOrganizerFor(item: WsItem): WsItem {
  return item.answer_format === FMT_ORGANIZER && !item.organizer ? { ...item, organizer: newOrganizer() } : item;
}

/** The mind map's branches above and below the centre (indices): the first half (rounded up) above. */
export function webRows(count: number): [number[], number[]] {
  const top = Math.ceil(count / 2);
  return [
    Array.from({ length: top }, (_, i) => i),
    Array.from({ length: count - top }, (_, i) => top + i),
  ];
}

/** The column (0–2) each of `n` boxes in a row of a mind map takes. */
export function webSlots(n: number): number[] {
  if (n <= 0) return [];
  if (n === 1) return [1];
  if (n === 2) return [0, 2];
  return [0, 1, 2].slice(0, Math.min(n, 3));
}

/** The rows of a columns layout: the most cells of any section. */
export const organizerRows = (org: WsSheetOrganizer) =>
  Math.max(1, ...org.sections.map((s) => s.cells.length));
