// 圖像組織 (docs/API.md「圖像組織」): the eight organizer types drawn on the sheet (editor and session
// preview), edited in place (type, title, labels, pre-filled cells, answers), a question switched to
// 圖像組織 getting a T-chart, and an old organizer question that still prints a box.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  SaveWorksheetRequest,
  WorksheetContent,
  WorksheetState,
  WsItem,
  WsLevel,
  WsOrganizer,
  WsOrganizerType,
} from "../api/types";
import { OrganizerEditor, OrganizerView } from "../components/worksheet/Organizer";
import { sheetBlockText } from "../components/prep/SheetPreview";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import {
  ORGANIZER_NAME,
  ORGANIZER_TYPES,
  addSection,
  convertOrganizer,
  newOrganizer,
  organizerFamily,
  projectOrganizer,
  removeSection,
  sectionRange,
  togglePrefill,
  withOrganizerFor,
} from "../lib/organizer";
import { autosaveConfig } from "../lib/useAutosave";
import { answerLines, emptyBrief, emptySettings, emptyWorksheetContent, newItem, withAnswerFormat } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn, type MockCall } from "../test/mockApi";

const WS = "/api/worksheets/5";

beforeEach(() => {
  signIn();
  autosaveConfig.delayMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  autosaveConfig.delayMs = 1500;
  worksheetConfig.pollMs = 2000;
});

/** An organizer of `type` whose labels are 「L1」…, the first cell pre-filled everywhere, the others
 * with answers 「Key i.j」. */
function filled(type: WsOrganizerType): WsOrganizer {
  const org = newOrganizer(type);
  return {
    ...org,
    title: "Organize the story",
    center: type === "mind_map" ? "Umbrellas" : "",
    sections: org.sections.map((s, i) => ({
      label: `L${i + 1}`,
      cells: s.cells.map((_, j) =>
        i === 0 && j === 0
          ? { answer: "Given one", given: "Given one", prefilled: ["basic", "standard", "advanced"] }
          : { answer: `Key ${i}.${j}`, given: "", prefilled: [] },
      ),
    })),
  };
}

describe("organizer types", () => {
  it("every type has its fewest sections, default labels and a layout", () => {
    expect(ORGANIZER_TYPES).toHaveLength(8);
    for (const type of ORGANIZER_TYPES) {
      const org = newOrganizer(type);
      expect(org.sections).toHaveLength(sectionRange(type)[0]);
      expect(org.sections.every((s) => s.label !== "" && s.cells.length > 0)).toBe(true);
    }
    expect(newOrganizer("kwl").sections.map((s) => s.label)).toEqual([
      "What I Know",
      "What I Want to Know",
      "What I Learned",
    ]);
    expect(newOrganizer("mind_map").center).toBe("Main idea");
  });

  it("switching the type keeps the teacher's labels and fits the new type", () => {
    const chart = newOrganizer("t_chart");
    chart.sections[0]!.label = "Mrs. Lin";
    const story = convertOrganizer(chart, "story_map");
    expect(story.sections.map((s) => s.label)).toEqual(["Mrs. Lin", "Characters", "Problem", "Events", "Solution"]);
    // A default label follows the type; too many sections are cut, too many cells too.
    const seq = convertOrganizer(newOrganizer("t_chart"), "sequence");
    expect(seq.sections.map((s) => s.label)).toEqual(["First", "Next", "Finally"]);
    expect(seq.sections.every((s) => s.cells.length <= 2)).toBe(true);
    expect(convertOrganizer(story, "t_chart").sections).toHaveLength(2);
    expect(convertOrganizer(chart, "mind_map").center).toBe("Main idea");
    // Boxes: within the type's range; a sequence's last box stays "Finally".
    const more = addSection(seq);
    expect(more.sections.map((s) => s.label)).toEqual(["First", "Next", "Then", "Finally"]);
    expect(removeSection(seq, 0)).toBe(seq);
    expect(addSection(newOrganizer("venn")).sections).toHaveLength(3);
  });

  it("pre-filling is per version on a 差異化學習單 question and everywhere otherwise", () => {
    const cell = { answer: "rain", given: "", prefilled: [] as WsLevel[] };
    const all: WsLevel[] = ["basic", "standard", "advanced"];
    const on = togglePrefill(cell, "basic", all, true);
    expect(on).toEqual({ answer: "rain", given: "rain", prefilled: ["basic"] });
    expect(togglePrefill(on, "basic", all, true).prefilled).toEqual([]);
    const everywhere = togglePrefill(cell, "standard", all, false);
    expect(everywhere.prefilled).toEqual(all);
    expect(togglePrefill(everywhere, "advanced", all, false).prefilled).toEqual([]);
  });

  it("students see pre-filled cells, teachers the answers; an organizer replaces the lines", () => {
    const org = filled("t_chart");
    const student = projectOrganizer(org, "standard", false);
    expect(student.sections[0]!.cells[0]).toEqual({ given: "Given one" });
    expect(student.sections[1]!.cells[0]).toEqual({ given: "" });
    expect(projectOrganizer(org, "standard", true).sections[1]!.cells[0]).toEqual({ given: "", answer: "Key 1.0" });
    // No writing lines for 圖像組織: the organizer is drawn, or (without one) a box.
    expect(answerLines({ answer_format: "圖像組織" }, "standard")).toBe(0);
    const switched = withOrganizerFor({ answer_format: "圖像組織" } as WsItem);
    expect(switched.organizer?.type).toBe("t_chart");
    expect(switched.organizer?.sections.map((s) => s.label)).toEqual(["Topic A", "Topic B"]);
    // The editor's format switch gives the same T-chart, and keeps an organizer the question has.
    const plain = { ...newItem(["standard"]), answer_format: "簡答題" };
    const viaSelect = withAnswerFormat(plain, "圖像組織");
    expect(viaSelect.organizer?.type).toBe("t_chart");
    expect(withAnswerFormat({ ...plain, organizer: org }, "圖像組織").organizer).toBe(org);
    expect(withAnswerFormat({ ...viaSelect, answer_format: "圖像組織" }, "簡答題").organizer).toBe(viaSelect.organizer);
  });

  it.each(ORGANIZER_TYPES)("%s is drawn with every label, the given cell and no answers for students", (type) => {
    const org = filled(type);
    const { container, unmount } = render(<OrganizerView org={projectOrganizer(org, "standard", false)} />);
    const fig = screen.getByRole("figure", { name: ORGANIZER_NAME[type] });
    expect(within(fig).getByText("Organize the story")).toBeInTheDocument();
    for (const s of org.sections) expect(within(fig).getByText(s.label)).toBeInTheDocument();
    expect(within(fig).getByText("Given one")).toBeInTheDocument();
    expect(fig.textContent).not.toContain("Key ");
    const family = organizerFamily(type);
    if (family === "venn") expect(container.querySelectorAll("ellipse")).toHaveLength(2);
    if (family === "chain") {
      expect(container.querySelectorAll(".ws-org-arrow").length).toBe(Math.min(org.sections.length, 3) - 1);
      expect(within(fig).getByText("1.")).toBeInTheDocument();
    }
    if (family === "web") {
      expect(within(fig).getByText("Umbrellas")).toBeInTheDocument();
      expect(container.querySelectorAll("line").length).toBe(org.sections.length);
    }
    if (family === "columns")
      expect(container.querySelectorAll(".ws-org-cell.is-grid")).toHaveLength(
        org.sections.length * Math.max(...org.sections.map((s) => s.cells.length)),
      );
    unmount();
    render(<OrganizerView org={projectOrganizer(org, "standard", true)} />);
    expect(screen.getByText("Key 1.0")).toBeInTheDocument();
  });
});

/** The editor alone, holding its organizer in state like the worksheet does. */
function Harness({ type, teacher, onSave }: { type: WsOrganizerType; teacher: boolean; onSave: (o: WsOrganizer) => void }) {
  const [org, setOrg] = useState(filled(type));
  return (
    <div className="ws">
      <OrganizerEditor
        org={org}
        no={1}
        level="standard"
        levels={["standard"]}
        perVersion={false}
        teacher={teacher}
        disabled={false}
        onChange={(next) => {
          setOrg(next);
          onSave(next);
        }}
      />
      <OrganizerView org={projectOrganizer(org, "standard", false)} />
    </div>
  );
}

describe("organizer editor", () => {
  it.each(ORGANIZER_TYPES)("%s: labels, the title and cells are edited in place and printed at once", async (type) => {
    let saved: WsOrganizer | null = null;
    const user = userEvent.setup();
    render(<Harness type={type} teacher={false} onSave={(o) => (saved = o)} />);
    const editor = screen.getByRole("group", { name: "圖像組織 1" });
    expect(within(editor).getByRole("combobox", { name: "圖表類型 1" })).toHaveValue(type);
    const label = within(editor).getByRole("textbox", { name: "格子標題 1-2" });
    await user.clear(label);
    await user.type(label, "Changed");
    expect(saved!.sections[1]!.label).toBe("Changed");
    // The printed drawing follows.
    const printed = screen.getAllByRole("figure").at(-1)!;
    expect(within(printed).getByText("Changed")).toBeInTheDocument();
    const title = within(editor).getByRole("textbox", { name: "圖表標題 1" });
    await user.type(title, "!");
    expect(saved!.title).toBe("Organize the story!");
    // The pre-filled cell's text, then turning pre-filling on for a blank one.
    const given = within(editor).getByRole("textbox", { name: "預填 1-1-1" });
    await user.type(given, " more");
    expect(saved!.sections[0]!.cells[0]!.given).toBe("Given one more");
    await user.click(within(editor).getByRole("checkbox", { name: "預填在學生版 1-2-1" }));
    expect(saved!.sections[1]!.cells[0]).toMatchObject({ given: "Key 1.0", prefilled: ["standard"] });
    expect(within(printed).getByText("Key 1.0")).toBeInTheDocument();
  });

  it("the teacher view edits the answers; types and boxes change within their limits", async () => {
    let saved: WsOrganizer | null = null;
    const user = userEvent.setup();
    render(<Harness type="sequence" teacher onSave={(o) => (saved = o)} />);
    const editor = screen.getByRole("group", { name: "圖像組織 1" });
    const key = within(editor).getByRole("textbox", { name: "參考答案 1-2-1" });
    expect(key).toHaveValue("Key 1.0");
    await user.type(key, "!");
    expect(saved!.sections[1]!.cells[0]!.answer).toBe("Key 1.0!");
    await user.click(within(editor).getByRole("button", { name: "加一格 1" }));
    expect(saved!.sections).toHaveLength(4);
    await user.click(within(editor).getByRole("button", { name: "加一個空格 1-1" }));
    expect(saved!.sections[0]!.cells).toHaveLength(2);
    expect(within(editor).getByRole("button", { name: "加一個空格 1-1" })).toBeDisabled();
    await user.click(within(editor).getByRole("button", { name: "刪除這一格 1-4" }));
    expect(saved!.sections).toHaveLength(3);
    await user.selectOptions(within(editor).getByRole("combobox", { name: "圖表類型 1" }), "venn");
    expect(saved!.type).toBe("venn");
    expect(saved!.sections).toHaveLength(3);
    expect(within(editor).queryByRole("button", { name: "加一格 1" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("figure", { name: "Venn diagram" })).toHaveLength(2);
  });
});

function content(items: WsItem[]): WorksheetContent {
  return {
    ...emptyWorksheetContent(),
    levels: ["standard"],
    blocks: [
      {
        id: "b1",
        kind: "activity",
        enabled: true,
        title: "Organize",
        instructions: "",
        paragraph_ids: [],
        info_function: null,
        student_tasks: [],
        objective_ids: [],
        vocabulary: [],
        items,
        scaffolds: [],
        teacher_notes: "",
        origin: "ai",
        message: null,
        sop_block_id: null,
      },
    ],
  };
}

const question = (id: string, format: string, organizer?: WsOrganizer): WsItem => ({
  id,
  answer_format: format,
  prompt: `Question ${id}`,
  options: [],
  answer: "",
  evidence: "",
  purpose: null,
  levels: ["standard"],
  origin: "ai",
  bloom: null,
  hint: "",
  ...(organizer ? { organizer } : {}),
});

function wsState(items: WsItem[]): WorksheetState {
  return {
    id: 5,
    title: "The Umbrella Stand",
    filename: "The Umbrella Stand_學習單",
    lesson: null,
    class_id: null,
    generated: true,
    levels: ["standard"],
    version_no: 2,
    created_at: "2026-11-16T08:00:00Z",
    updated_at: "2026-11-16T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-16T08:00:00Z",
    content: content(items),
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    request: "",
  } as WorksheetState;
}

describe("organizers in the worksheet editor", () => {
  it("a question switched to 圖像組織 gets a T-chart; an old one offers to become a chart", async () => {
    const state = wsState([
      question("q1", "簡答題"),
      question("q2", "圖像組織"),
      question("q3", "圖像組織", filled("cause_effect")),
    ]);
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      [`GET ${WS}`]: state,
      [`PUT ${WS}`]: (call: MockCall) => ({ ...state, ...(call.body as SaveWorksheetRequest) }),
    });
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const sheet = await screen.findByRole("article", { name: "一般版 學生版" });
    // The generated chain is drawn and editable.
    const chain = within(sheet).getByRole("group", { name: "圖像組織 3" });
    expect(within(chain).getByRole("combobox", { name: "圖表類型 3" })).toHaveValue("cause_effect");
    expect(within(chain).getByRole("textbox", { name: "預填 3-1-1" })).toHaveValue("Given one");
    // The old organizer question prints a box until the teacher makes it a chart.
    expect(within(sheet).getByText("這一題印成空白方框。")).toBeInTheDocument();
    expect(within(sheet).getByRole("img", { name: "圖像組織作答框 2" })).toBeInTheDocument();
    await user.click(within(sheet).getByRole("button", { name: "改成圖表" }));
    expect(await within(sheet).findByRole("group", { name: "圖像組織 2" })).toBeInTheDocument();
    expect(within(sheet).queryByRole("img", { name: "圖像組織作答框 2" })).not.toBeInTheDocument();

    await user.selectOptions(within(sheet).getByRole("combobox", { name: "作答方式 1" }), "圖像組織");
    const chart = await within(sheet).findByRole("group", { name: "圖像組織 1" });
    expect(within(chart).getByRole("combobox", { name: "圖表類型 1" })).toHaveValue("t_chart");
    const heading = within(chart).getByRole("textbox", { name: "格子標題 1-1" });
    expect(heading).toHaveValue("Topic A");
    await user.clear(heading);
    await user.type(heading, "Then");
    await waitFor(() => {
      const body = api.lastBody("PUT", WS) as SaveWorksheetRequest | undefined;
      const items = body?.content?.blocks[0]?.items ?? [];
      expect(items[0]?.organizer?.sections[0]?.label).toBe("Then");
      expect(items[1]?.organizer?.type).toBe("t_chart");
      expect(items[2]?.organizer?.type).toBe("cause_effect");
    });

    // The teacher view edits the answers.
    await user.click(screen.getByRole("radio", { name: "教師參考版" }));
    await screen.findByRole("article", { name: "一般版 教師參考版" });
    expect(screen.getByRole("textbox", { name: "參考答案 3-2-1" })).toHaveValue("Key 1.0");
  });

  it("the session preview draws the server's organizer and copies it as text", () => {
    const org = projectOrganizer(filled("sequence"), "standard", false);
    const block = {
      id: "b1",
      kind: "activity" as const,
      label: "A",
      title: "Organize",
      instructions: "",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ no: 1, answer_format: "圖像組織", prompt: "Complete the chain.", options: [], lines: 0, organizer: org }],
    };
    const text = sheetBlockText(block);
    expect(text).toContain("1. L1: Given one");
    expect(text).toContain("3. L3: ______");
  });
});
