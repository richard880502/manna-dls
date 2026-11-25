// 備課頁左邊的「預覽｜編輯」：同一個編輯器放進學習單窗格，對話在右邊不離開。
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PrepSession, PrepTurn, SaveWorksheetRequest, WorksheetState, WsAudience, WsBlock, WsItem, WsSheet } from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn, type MockCall } from "../test/mockApi";

const S7 = "/api/prep-sessions/7";
const WS = "/api/worksheets/11";
const MODE_KEY = "manna.prepSheetMode";

beforeEach(() => {
  signIn();
  window.localStorage.removeItem(MODE_KEY);
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  window.localStorage.removeItem(MODE_KEY);
  vi.unstubAllGlobals();
});

const item = (id: string, prompt: string): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: "",
  evidence: "",
  purpose: null,
  levels: ["standard"],
  origin: "ai",
});

const block = (prompt: string): WsBlock => ({
  id: "b2",
  kind: "activity",
  enabled: true,
  title: "Paragraph 1",
  instructions: "Read it.",
  paragraph_ids: [],
  info_function: null,
  student_tasks: [],
  objective_ids: [],
  vocabulary: [],
  items: [item("b2-1", prompt)],
  scaffolds: [],
  teacher_notes: "",
  origin: "ai",
  message: null,
  sop_block_id: null,
});

const worksheet = (prompt: string, version = 3, running = false): WorksheetState => ({
  id: 11,
  title: "學習單一",
  filename: "學習單一",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: version,
  created_at: "2026-11-25T08:00:00Z",
  updated_at: "2026-11-25T08:00:00Z",
  material: {
    title: "學習單一",
    paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
    source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 },
  },
  settings: emptySettings(),
  suggested: emptySettings(),
  brief: emptyBrief(),
  versions_count: version,
  saved_at: "2026-11-25T08:00:00Z",
  content: { ...emptyWorksheetContent(), levels: ["standard"], blocks: [block(prompt)] },
  jobs: [],
  running,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
});

const sheet = (audience: WsAudience): WsSheet => ({
  title: "學習單一",
  level: "standard",
  audience,
  objectives: [],
  blocks: [
    {
      id: "b2",
      kind: "activity",
      label: "C",
      title: "Paragraph 1",
      instructions: "Read it.",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ id: "b2-1", no: 1, answer_format: "簡答題", prompt: "Why?", options: [], lines: 2 }],
    },
  ],
});

const chatTurn = (status: PrepTurn["status"]): PrepTurn => ({
  id: 202,
  seq: 2,
  kind: "chat",
  request: "改一下",
  status,
  error: null,
  worksheet: null,
  result: null,
  interaction_id: null,
  created_at: "2026-11-25T08:00:00Z",
  finished_at: null,
});

const session = (turns: PrepTurn[]): PrepSession => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: turns.length + 1,
  last_kind: "worksheet",
  updated_at: "2026-11-25T09:00:00Z",
  created_at: "2026-11-25T08:00:00Z",
  turns: [
    {
      id: 101,
      seq: 1,
      kind: "worksheet",
      request: "做學習單",
      status: "done",
      error: null,
      worksheet: { id: 11, title: "學習單一", version_no: 3, running: false },
      result: null,
      interaction_id: null,
      created_at: "2026-11-25T08:00:00Z",
      finished_at: "2026-11-25T08:01:00Z",
    },
    ...turns,
  ],
  running: turns.some((t) => t.status === "running"),
  next: [],
});

const saved = (call: MockCall) => ({
  ...worksheet("x"),
  ...(call.body as SaveWorksheetRequest),
  version_no: 3,
  saved_at: "2026-11-25T09:30:00Z",
});

const routes = (extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  [`GET ${WS}/comments`]: [],
  [`GET ${WS}/proposals`]: [],
  [`GET ${WS}`]: worksheet("Why did she start it?"),
  [`PUT ${WS}`]: saved,
  [`GET ${WS}/sheet`]: (c: MockCall) =>
    sheet((new URL(c.url, "http://x").searchParams.get("audience") ?? "student") as WsAudience),
  [`GET ${S7}`]: session([]),
  [`POST ${S7}/turns`]: session([]),
  ...extra,
});

const PROMPT = "Why did she start it?";
const editorBox = () => screen.findByRole("textbox", { name: "題目 1" });
const modeRadio = (name: string) => screen.findByRole("radio", { name });

describe("預覽｜編輯 in the worksheet pane", () => {
  it("starts in 預覽, switches to the editor, and remembers the choice", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    expect(await modeRadio("預覽")).toBeChecked();
    expect(screen.queryByRole("textbox", { name: "題目 1" })).not.toBeInTheDocument();

    await user.click(await modeRadio("編輯"));
    expect(await editorBox()).toHaveValue(PROMPT);
    expect(window.localStorage.getItem(MODE_KEY)).toBe("edit");
    // The link to the full editor page stays, in the preview that remains underneath.
    expect(screen.getByText("在完整編輯頁開啟", { selector: "a" })).toHaveAttribute("href", "/worksheets/11");
  });

  it("opens in 編輯 when that was the last choice, and 預覽 brings the preview back", async () => {
    window.localStorage.setItem(MODE_KEY, "edit");
    mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    expect(await editorBox()).toHaveValue(PROMPT);
    expect(await modeRadio("編輯")).toBeChecked();
    await user.click(await modeRadio("預覽"));
    await waitFor(() => expect(screen.queryByRole("textbox", { name: "題目 1" })).not.toBeInTheDocument());
    expect(window.localStorage.getItem(MODE_KEY)).toBe("preview");
  });

  it("saves what is typed like the editor page does, and saves before leaving 編輯", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await modeRadio("編輯"));
    const box = await editorBox();
    fireEvent.change(box, { target: { value: "Why did Mrs. Lin start it?" } });
    // Leaving 編輯 saves first.
    await user.click(await modeRadio("預覽"));
    await waitFor(() => expect(api.callsTo("PUT", WS)).toHaveLength(1));
    const body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.base_version_no).toBe(3);
    expect(body.content?.blocks[0]?.items[0]?.prompt).toBe("Why did Mrs. Lin start it?");
  });

  it("sending a message saves the pending edit first", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await modeRadio("編輯"));
    fireEvent.change(await editorBox(), { target: { value: "Edited by hand" } });
    expect(api.callsTo("PUT", WS)).toHaveLength(0);

    await user.type(await screen.findByRole("textbox", { name: "給 Manna 的話" }), "再加一題{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    const order = api.calls.map((c) => `${c.method} ${c.path}`);
    expect(order.indexOf(`PUT ${WS}`)).toBeGreaterThan(-1);
    expect(order.indexOf(`PUT ${WS}`)).toBeLessThan(order.indexOf(`POST ${S7}/turns`));
  });

  it("is read-only with a notice while a chat turn runs, then reads the new version", async () => {
    window.localStorage.setItem(MODE_KEY, "edit");
    let phase: "running" | "done" = "running";
    mockApi(
      routes({
        [`GET ${S7}`]: () => session([chatTurn(phase === "running" ? "running" : "done")]),
        [`GET ${WS}`]: () => (phase === "running" ? worksheet(PROMPT) : worksheet("Rewritten by Manna", 4)),
      }),
    );
    renderApp("/prep/7");
    const box = await editorBox();
    expect(box).toBeDisabled();
    expect(screen.getByText("Manna 正在改這份學習單，完成後就能繼續編輯。")).toBeInTheDocument();

    act(() => {
      phase = "done";
    });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "題目 1" })).toHaveValue("Rewritten by Manna"));
    expect(screen.getByRole("textbox", { name: "題目 1" })).toBeEnabled();
    expect(screen.queryByText("Manna 正在改這份學習單，完成後就能繼續編輯。")).not.toBeInTheDocument();
  });
});
