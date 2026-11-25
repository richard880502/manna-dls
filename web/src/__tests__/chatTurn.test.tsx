// 備課對話：工具式對話（kind "chat"）、並排的對話與學習單，以及舊的 ask／revise 輪次。
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PrepChatResult,
  PrepKind,
  PrepSession,
  PrepTurn,
  RunProgress,
  WorksheetState,
  WsAudience,
  WsSheet,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

const S7 = "/api/prep-sessions/7";
const PLACEHOLDER = "問問題，或說要怎麼改學習單（例如：把 C 區第 2 題改成比較表）";

beforeEach(() => {
  signIn();
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  vi.unstubAllGlobals();
});

// ---------------------------------------------------------------- fixtures

let nextId = 500;
const turn = (kind: PrepKind, overrides: Partial<PrepTurn> = {}): PrepTurn => ({
  id: ++nextId,
  seq: 1,
  kind,
  request: "說明",
  status: "done",
  error: null,
  worksheet: null,
  result: null,
  interaction_id: null,
  created_at: "2026-11-25T08:00:00Z",
  finished_at: "2026-11-25T08:01:00Z",
  ...overrides,
});

const sheetTurn = (id: number, title: string, version = 3): PrepTurn =>
  turn("worksheet", { id, request: "做學習單", worksheet: { id: id === 102 ? 12 : 11, title, version_no: version, running: false } });

const session = (turns: PrepTurn[], overrides: Partial<PrepSession> = {}): PrepSession => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: turns.length,
  last_kind: "worksheet",
  updated_at: "2026-11-25T09:00:00Z",
  created_at: "2026-11-25T08:00:00Z",
  turns: turns.map((t, i) => ({ ...t, seq: i + 1 })),
  running: turns.some((t) => t.status === "running"),
  next: [],
  ...overrides,
});

const worksheet = (id: number, title: string, version = 3): WorksheetState => ({
  id,
  title,
  filename: title,
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: version,
  created_at: "2026-11-25T08:00:00Z",
  updated_at: "2026-11-25T08:00:00Z",
  material: {
    title,
    paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
    source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 },
  },
  settings: emptySettings(),
  suggested: emptySettings(),
  brief: emptyBrief(),
  versions_count: version,
  saved_at: "2026-11-25T08:00:00Z",
  content: { ...emptyWorksheetContent(), levels: ["standard"] },
  jobs: [],
  running: false,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
});

const sheet = (title: string, audience: WsAudience): WsSheet => ({
  title,
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

const chat = (overrides: Partial<PrepChatResult> = {}): PrepChatResult => ({
  answer: "第一段說明。\n\n第二段說明。",
  follow_ups: [],
  changes: [],
  not_applied: [],
  version_no: null,
  undoable: false,
  tool_calls: 0,
  ...overrides,
});

const sheetRoutes = (): Record<string, unknown> => ({
  "GET /api/worksheets/11/comments": [],
  "GET /api/worksheets/11/proposals": [],
  "GET /api/worksheets/12/comments": [],
  "GET /api/worksheets/12/proposals": [],
  "GET /api/worksheets/11": worksheet(11, "學習單一"),
  "GET /api/worksheets/11/sheet": (c: MockCall) =>
    sheet("學習單一", (new URL(c.url, "http://x").searchParams.get("audience") ?? "student") as WsAudience),
  "GET /api/worksheets/12": worksheet(12, "學習單二", 1),
  "GET /api/worksheets/12/sheet": (c: MockCall) =>
    sheet("學習單二", (new URL(c.url, "http://x").searchParams.get("audience") ?? "student") as WsAudience),
});

const box = () => screen.findByRole("textbox", { name: "給 Manna 的話" });

// ---------------------------------------------------------------- the bar

describe("the bottom bar sends chat turns", () => {
  it("has no mode toggle; free text posts kind chat and tells the habit finder", async () => {
    const done = session([sheetTurn(101, "學習單一")]);
    const api = mockApi({ ...baseRoutes(), ...sheetRoutes(), [`GET ${S7}`]: done, [`POST ${S7}/turns`]: done });
    const habit = vi.fn();
    window.addEventListener("manna:habit-words-sent", habit);
    const user = userEvent.setup();
    renderApp("/prep/7");
    const bar = await box();
    expect(bar).toHaveAttribute("placeholder", PLACEHOLDER);
    expect(screen.queryByRole("radio", { name: "問 Manna" })).not.toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "改學習單" })).not.toBeInTheDocument();
    expect(screen.getByText("目前的學習單：學習單一")).toBeInTheDocument();

    await user.type(bar, "把 C 區第 2 題改成比較表{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "chat", request: "把 C 區第 2 題改成比較表" });
    await waitFor(() => expect(habit).toHaveBeenCalledTimes(1));
    window.removeEventListener("manna:habit-words-sent", habit);
  });

  it("a conversation without a worksheet asks too, and shows no worksheet pane", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([turn("activity")]),
      [`POST ${S7}/turns`]: session([turn("activity")]),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.type(await box(), "這課要上幾節？{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "chat", request: "這課要上幾節？" });
    expect(screen.queryByRole("complementary", { name: "目前的學習單" })).not.toBeInTheDocument();
    expect(screen.queryByText(/目前的學習單：/)).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the card

describe("a chat turn", () => {
  it("while it runs shows its steps, or a calm line when it has none yet", async () => {
    const progress: RunProgress = {
      steps: [
        { key: "read", label: "讀 C 區", status: "done" },
        { key: "edit", label: "修改 C 區", status: "running" },
      ],
      current: 1,
      status: "running",
    };
    const turns = [sheetTurn(101, "學習單一"), turn("chat", { status: "running", result: null, progress: null })];
    let reads = 0;
    mockApi({
      ...baseRoutes(),
      ...sheetRoutes(),
      [`GET ${S7}`]: () => {
        reads += 1;
        return session([turns[0]!, { ...turns[1]!, progress: reads > 1 ? progress : null }]);
      },
    });
    renderApp("/prep/7");
    expect((await screen.findAllByText("Manna 正在處理…")).length).toBeGreaterThan(0);
    const panel = await screen.findByRole("region", { name: "進度" });
    expect(within(panel).getByText("讀 C 區")).toBeInTheDocument();
    expect(within(panel).getByText("修改 C 區").closest("li")).toHaveAttribute("aria-current", "step");
  });

  it("when done shows paragraphs, changes, not-applied lines, and follow-ups that send a chat turn", async () => {
    const t = turn("chat", {
      request: "改 C 區",
      result: chat({
        follow_ups: ["再加一題"],
        changes: [
          { target: "block", block_id: "b2", label: "C 區", summary: "第 2 題改成比較表" },
          { target: "vocabulary", block_id: null, label: "Key Vocabulary", summary: "加了 umbrella" },
        ],
        not_applied: [{ label: "B 區", reason: "題目數量不對" }],
        version_no: 4,
        undoable: true,
        tool_calls: 3,
      }),
    });
    const done = session([sheetTurn(101, "學習單一"), t]);
    const api = mockApi({ ...baseRoutes(), ...sheetRoutes(), [`GET ${S7}`]: done, [`POST ${S7}/turns`]: done });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const result = within(await screen.findByRole("region", { name: /和 Manna 對話/ }));
    expect(result.getByText("第一段說明。")).toBeInTheDocument();
    expect(result.getByText("第二段說明。")).toBeInTheDocument();
    const changes = within(result.getByRole("region", { name: "這一輪改了" }));
    expect(changes.getByText("第 2 題改成比較表", { exact: false })).toBeInTheDocument();
    expect(changes.getByRole("button", { name: "C 區" })).toBeInTheDocument();
    expect(changes.getByText("Key Vocabulary")).toBeInTheDocument();
    expect(result.getByText("沒有改成：B 區（題目數量不對）")).toBeInTheDocument();

    await user.click(result.getByRole("button", { name: "再加一題" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "chat", request: "再加一題" });
  });

  it("復原這一輪 asks in the page, posts the undo and reads the session again", async () => {
    const t = turn("chat", { result: chat({ changes: [{ target: "block", block_id: "b2", label: "C 區", summary: "改了" }], version_no: 4, undoable: true }) });
    let undone = false;
    const api = mockApi({
      ...baseRoutes(),
      ...sheetRoutes(),
      [`GET ${S7}`]: () =>
        undone
          ? session([sheetTurn(101, "學習單一"), { ...t, result: chat({ version_no: 5, undoable: false, answer: "已復原。" }) }])
          : session([sheetTurn(101, "學習單一"), t]),
      [`POST ${S7}/turns/${t.id}/undo`]: () => {
        undone = true;
        return t;
      },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "復原這一輪" }));
    expect(api.callsTo("POST", `${S7}/turns/${t.id}/undo`)).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "確定復原" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns/${t.id}/undo`)).toHaveLength(1));
    // The session was read again; the turn no longer offers 復原.
    await waitFor(() => expect(screen.queryByRole("button", { name: "復原這一輪" })).not.toBeInTheDocument());
    expect(api.callsTo("GET", S7).length).toBeGreaterThan(1);
  });

  it("shows the server's reason when 復原 is refused with 409", async () => {
    const t = turn("chat", { result: chat({ version_no: 4, undoable: true }) });
    mockApi({
      ...baseRoutes(),
      ...sheetRoutes(),
      [`GET ${S7}`]: session([sheetTurn(101, "學習單一"), t]),
      [`POST ${S7}/turns/${t.id}/undo`]: () => respond(409, { error: { code: "conflict", message: "C 區在這一輪之後又改過了，不能復原。" } }),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "復原這一輪" }));
    await user.click(screen.getByRole("button", { name: "確定復原" }));
    expect(await screen.findByText("C 區在這一輪之後又改過了，不能復原。")).toBeInTheDocument();
  });

  it("failed: the failed card with 再試一次", async () => {
    const t = turn("chat", { request: "改 C 區", status: "failed", error: "這個功能需要 OpenAI 模型。" });
    const base = session([t]);
    const api = mockApi({ ...baseRoutes(), [`GET ${S7}`]: base, [`POST ${S7}/turns`]: base });
    const user = userEvent.setup();
    renderApp("/prep/7");
    expect(await screen.findByText("這個功能需要 OpenAI 模型。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "再試一次" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "chat", request: "改 C 區" });
  });

  it("old ask and revise turns still render as before", async () => {
    mockApi({
      ...baseRoutes(),
      ...sheetRoutes(),
      [`GET ${S7}`]: session([
        sheetTurn(101, "學習單一"),
        turn("ask", { request: "怎麼開場？", result: { answer: "先問經驗。", follow_ups: ["給我開場白"] } }),
        turn("revise", { request: "B 區簡單點", result: { reply: "已改簡單。", changed_block_ids: ["b2"], version_no: 4 } }),
      ]),
    });
    renderApp("/prep/7");
    expect(await screen.findByText("先問經驗。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "給我開場白" })).toBeInTheDocument();
    expect(screen.getByText("已改簡單。")).toBeInTheDocument();
    expect(screen.getByText(/已修改：/)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- side by side

describe("the conversation and the worksheet side by side", () => {
  it("shows the worksheet in a pane of its own and a compact card in the conversation", async () => {
    mockApi({ ...baseRoutes(), ...sheetRoutes(), [`GET ${S7}`]: session([sheetTurn(101, "學習單一")]) });
    renderApp("/prep/7");
    const pane = within(await screen.findByRole("complementary", { name: "目前的學習單" }));
    expect(await pane.findByRole("article", { name: /學生版/ })).toBeInTheDocument();
    const chatPane = within(screen.getByRole("region", { name: "對話" }));
    // The turn's card is a line, not the preview.
    expect(chatPane.getByText("學習單一")).toBeInTheDocument();
    expect(chatPane.getByText("已顯示在右邊")).toBeInTheDocument();
    expect(chatPane.queryByRole("article")).not.toBeInTheDocument();
    expect(screen.queryByRole("tablist", { name: "對話或學習單" })).not.toBeInTheDocument();
  });

  it("with several worksheets a selector switches the target, and the bar says which one", async () => {
    const api = mockApi({
      ...baseRoutes(),
      ...sheetRoutes(),
      [`GET ${S7}`]: session([sheetTurn(101, "學習單一"), sheetTurn(102, "學習單二", 1)]),
      [`POST ${S7}/turns`]: session([sheetTurn(101, "學習單一"), sheetTurn(102, "學習單二", 1)]),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const pick = await screen.findByRole("combobox", { name: "學習單" });
    expect(screen.getByText("目前的學習單：學習單二")).toBeInTheDocument();
    await user.selectOptions(pick, screen.getByRole("option", { name: /學習單一/ }));
    expect(screen.getByText("目前的學習單：學習單一")).toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("GET", "/api/worksheets/11/sheet").length).toBeGreaterThan(0));
    await user.type(await box(), "改簡單{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "chat", request: "改簡單", worksheet_id: 11 });
    // The compact card of the other worksheet puts it in the pane.
    await user.click(screen.getAllByRole("button", { name: "在右邊看" })[0]!);
    expect(screen.getByText("目前的學習單：學習單二")).toBeInTheDocument();
  });

  it("after a chat turn that changed the worksheet the pane reads the new version", async () => {
    const before = [sheetTurn(101, "學習單一"), turn("chat", { status: "running" })];
    const after = [
      sheetTurn(101, "學習單一"),
      { ...before[1]!, status: "done" as const, result: chat({ changes: [{ target: "block", block_id: "b2", label: "C 區", summary: "改了" }], version_no: 4, undoable: true }) },
    ];
    let reads = 0;
    const api = mockApi({
      ...baseRoutes(),
      ...sheetRoutes(),
      [`GET ${S7}`]: () => (++reads < 3 ? session(before) : session(after)),
    });
    renderApp("/prep/7");
    await screen.findByRole("article", { name: /學生版/ });
    const reads0 = api.callsTo("GET", "/api/worksheets/11/sheet").length;
    expect(await screen.findByRole("button", { name: "復原這一輪" })).toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("GET", "/api/worksheets/11/sheet").length).toBeGreaterThan(reads0));
  });

  it("on a narrow screen the two are tabs, and a change offers 學習單已更新，去看", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: true,
      media: query,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
    }));
    const t = turn("chat", { result: chat({ changes: [{ target: "block", block_id: "b2", label: "C 區", summary: "改了" }], version_no: 4, undoable: true }) });
    mockApi({ ...baseRoutes(), ...sheetRoutes(), [`GET ${S7}`]: session([sheetTurn(101, "學習單一"), t]) });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const tabs = within(await screen.findByRole("tablist", { name: "對話或學習單" }));
    expect(tabs.getByRole("tab", { name: "對話" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("complementary", { name: "目前的學習單" })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "學習單已更新，去看" }));
    expect(tabs.getByRole("tab", { name: "學習單" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("complementary", { name: "目前的學習單" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "對話" })).not.toBeInTheDocument();
  });
});
