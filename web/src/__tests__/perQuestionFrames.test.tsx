// A sentence frame belongs to one question: written under it as a little paragraph the student
// can say aloud, edited or deleted like other sheet text, printed in the preview and copied.
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  SaveWorksheetRequest,
  WorksheetContent,
  WorksheetState,
  WsBlock,
  WsItem,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings, emptyWorksheetContent, frameShown } from "../lib/worksheet";
import { sheetBlockText } from "../components/prep/SheetPreview";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

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

// ---------------------------------------------------------------- fixtures

const item = (id: string, prompt: string, extra: Partial<WsItem> = {}): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: `Answer to ${id}`,
  evidence: "",
  purpose: null,
  levels: ["basic", "standard"],
  origin: "ai",
  bloom: null,
  hint: "",
  ...extra,
});

const block = (id: string, title: string, items: WsItem[], extra: Partial<WsBlock> = {}): WsBlock => ({
  id,
  kind: "activity",
  enabled: true,
  title,
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
  ...extra,
});

function state(content: WorksheetContent, extra: Partial<WorksheetState> = {}): WorksheetState {
  return {
    id: 5,
    title: "The Umbrella Stand",
    filename: "The Umbrella Stand_學習單",
    lesson: null,
    class_id: null,
    generated: true,
    levels: content.levels,
    version_no: 2,
    created_at: "2026-11-18T08:00:00Z",
    updated_at: "2026-11-18T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-18T08:00:00Z",
    content,
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    request: "",
    ...extra,
  };
}


/**
 * A server that keeps what it is sent: a save is checked against the version it was edited from
 * (409 when another version came in between, like the real one) and a reload gets the latest.
 */
function server(initial: WorksheetState) {
  let current = initial;
  const saves: SaveWorksheetRequest[] = [];
  const api = mockApi({
    ...baseRoutes(),
    "GET /api/worksheets": [],
    [`GET ${WS}`]: () => current,
    [`PUT ${WS}`]: (call: MockCall) => {
      const body = call.body as SaveWorksheetRequest;
      saves.push(body);
      if (body.content && body.base_version_no !== current.version_no) {
        return respond(409, { error: { code: "conflict", message: "請重新載入" } });
      }
      const { base_version_no: _base, ...rest } = body;
      current = {
        ...current,
        ...(rest as Partial<WorksheetState>),
        content: body.content ?? current.content,
        version_no: body.content ? current.version_no + 1 : current.version_no,
        saved_at: "2026-11-18T09:30:00Z",
      };
      return current;
    },
  });
  return { api, saves, now: () => current };
}

async function settle() {
  window.dispatchEvent(new Event("blur"));
  await screen.findByText(/^已自動儲存 \d\d:\d\d/);
}

const FRAME = "Hello, everyone. My answer to the question is ___ because ___. Thank you.";

const content = (): WorksheetContent => ({
  ...emptyWorksheetContent(),
  show_objectives: false,
  levels: ["basic", "standard"],
  blocks: [
    block("b1", "Look Closer", [
      item("q1", "Why did the neighbors laugh?", { frame: FRAME }),
      item("q2", "Which sentence comes from paragraph 1?", {
        answer_format: "選擇題",
        options: ["Mrs. Lin put a stand outside.", "Nobody laughed."],
      }),
      item("q3", "What would you do?"),
    ]),
  ],
});

const question = (no: number) => within(screen.getByRole("textbox", { name: `題目 ${no}` }).closest("li") as HTMLElement);

describe("a sentence frame under its own question", () => {
  it("is written under the question, edited in place and deleted with 刪除", async () => {
    const srv = server(state(content()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await screen.findByRole("textbox", { name: "題目 1" });
    expect(question(1).getByRole("textbox", { name: "句型框架 1" })).toHaveValue(FRAME);
    // An open question without one offers an empty box; a choice question does not.
    expect(question(3).getByRole("textbox", { name: "句型框架 3" })).toHaveValue("");
    expect(question(2).queryByRole("textbox", { name: "句型框架 2" })).not.toBeInTheDocument();

    await user.type(question(3).getByRole("textbox", { name: "句型框架 3" }), "I would ___ because ___.");
    await user.click(question(1).getByRole("button", { name: "刪除句型框架 1" }));
    await settle();
    const items = srv.now().content.blocks[0]?.items ?? [];
    expect(items[0]?.frame).toBe("");
    expect(items[2]?.frame).toBe("I would ___ because ___.");
    expect(items[1]?.frame).toBeUndefined();
  });

  it("an old worksheet without frames still loads", async () => {
    server(state({ ...content(), blocks: [block("b1", "Old", [item("q1", "Why?")])] }));
    renderApp("/worksheets/5");
    expect(await screen.findByRole("textbox", { name: "題目 1" })).toBeInTheDocument();
  });
});

describe("the sheet's own frame", () => {
  it("frameShown follows the server: not in 進階版 unless it is the only version", () => {
    expect(frameShown("basic", ["basic", "standard", "advanced"])).toBe(true);
    expect(frameShown("standard", ["basic", "standard", "advanced"])).toBe(true);
    expect(frameShown("advanced", ["basic", "standard", "advanced"])).toBe(false);
    expect(frameShown("advanced", ["advanced"])).toBe(true);
  });

  it("is in the copied text under its question", () => {
    const text = sheetBlockText({
      id: "b1",
      kind: "activity",
      label: "A",
      title: "Look Closer",
      instructions: "",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [
        { id: "q1", no: 1, answer_format: "簡答題", prompt: "Why?", options: [], lines: 3, frame: FRAME, hint: "Look again." },
      ],
    });
    const lines = text.split("\n");
    expect(lines.indexOf("1. Why?")).toBeGreaterThan(-1);
    expect(lines).toContain(`   Sentence frame: ${FRAME}`);
    expect(lines.indexOf(`   Sentence frame: ${FRAME}`)).toBeGreaterThan(lines.indexOf("   Hint: Look again."));
  });
});
