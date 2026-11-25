// 備課頁左邊永遠是可以直接改的學習單工作區（工具列、編輯器、彈出式功能），對話在右邊不離開。
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PrepSession, PrepTurn, SaveWorksheetRequest, WorksheetState, WsAudience, WsBlock, WsItem, WsSheet } from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { autosaveConfig } from "../lib/useAutosave";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn, type MockCall } from "../test/mockApi";
import { openBarPopover } from "../test/menus";

const S7 = "/api/prep-sessions/7";
const WS = "/api/worksheets/11";

beforeEach(() => {
  signIn();
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  autosaveConfig.delayMs = 1500;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
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

const chatTurn = (status: PrepTurn["status"], result: PrepTurn["result"] = null): PrepTurn => ({
  id: 202,
  seq: 2,
  kind: "chat",
  request: "改一下",
  status,
  error: null,
  worksheet: null,
  result,
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

describe("the worksheet pane of the 備課對話", () => {
  it("is the editor from the start: no 預覽｜編輯, the toolbar's tools are all there", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    expect(await editorBox()).toHaveValue(PROMPT);
    expect(screen.queryByRole("radio", { name: "預覽" })).not.toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "編輯" })).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "學生版" })).toBeChecked();
    const bar = within(screen.getByRole("group", { name: "學習單工具列" }));
    expect(bar.getByLabelText("版本")).toHaveTextContent("第 3 版");
    for (const name of ["版本紀錄", "顯示", "詞彙檢查", "關於這份學習單", "更多…"]) {
      expect(bar.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(bar.getByRole("button", { name: /匯出/ })).toBeEnabled();
    // 在完整編輯頁開啟 is in 更多…; 調整設定 only exists on the full page.
    const more = await openBarPopover(user, "更多…");
    expect(more.getByRole("link", { name: "在完整編輯頁開啟" })).toHaveAttribute("href", "/worksheets/11");
    expect(more.queryByRole("button", { name: "調整設定" })).not.toBeInTheDocument();
    expect(window.localStorage.getItem("manna.prepSheetMode")).toBeNull();
  });

  it("saves what is typed, and says so in the toolbar", async () => {
    autosaveConfig.delayMs = 20;
    const api = mockApi(routes());
    renderApp("/prep/7");
    fireEvent.change(await editorBox(), { target: { value: "Why did Mrs. Lin start it?" } });
    expect(await screen.findByText("尚未儲存")).toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("PUT", WS)).toHaveLength(1));
    const body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.base_version_no).toBe(3);
    expect(body.content?.blocks[0]?.items[0]?.prompt).toBe("Why did Mrs. Lin start it?");
    expect(await screen.findByText(/^已自動儲存/)).toBeInTheDocument();
  });

  it("sending a message saves the pending edit first", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    fireEvent.change(await editorBox(), { target: { value: "Edited by hand" } });
    expect(api.callsTo("PUT", WS)).toHaveLength(0);

    await user.type(await screen.findByRole("textbox", { name: "給 Manna 的話" }), "再加一題{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    const order = api.calls.map((c) => `${c.method} ${c.path}`);
    expect(order.indexOf(`PUT ${WS}`)).toBeGreaterThan(-1);
    expect(order.indexOf(`PUT ${WS}`)).toBeLessThan(order.indexOf(`POST ${S7}/turns`));
  });

  it("is read-only with a notice while a chat turn runs, then reads the new version", async () => {
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

  it("lights up the blocks the last chat turn changed, and 「這一輪改了」 scrolls to the block", async () => {
    const scrolled: Array<string | undefined> = [];
    Element.prototype.scrollIntoView = function (this: Element) {
      scrolled.push(this.id);
    };
    const changed = {
      answer: "好。",
      follow_ups: [],
      changes: [{ target: "block", block_id: "b2", label: "A 區", summary: "改了" }],
      not_applied: [],
      version_no: 4,
      undoable: true,
      tool_calls: 1,
    } as unknown as PrepTurn["result"];
    let reads = 0;
    mockApi(
      routes({
        [`GET ${S7}`]: () => (++reads < 3 ? session([chatTurn("running")]) : session([chatTurn("done", changed)])),
        [`GET ${WS}`]: () => (reads < 3 ? worksheet(PROMPT) : worksheet(PROMPT, 4)),
      }),
    );
    const user = userEvent.setup();
    renderApp("/prep/7");
    await editorBox();
    await waitFor(() => expect(document.getElementById("sheet-block-b2")).toHaveClass("is-changed"));
    await waitFor(() => expect(scrolled).toContain("sheet-block-b2"));

    scrolled.length = 0;
    await user.click(await screen.findByRole("button", { name: "A 區" }));
    await waitFor(() => expect(scrolled).toContain("sheet-block-b2"));
  });

  it("opens popovers that close with Escape and give focus back to their button", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await editorBox();
    const button = screen.getByRole("button", { name: "顯示" });
    await user.click(button);
    const panel = within(screen.getByRole("group", { name: "顯示設定" }));
    for (const name of ["顯示學習目標", "字詞表放在學習單最後", "字詞表的字義用中文"]) {
      expect(panel.getByRole("checkbox", { name })).toBeInTheDocument();
    }
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "顯示設定" })).not.toBeInTheDocument();
    expect(button).toHaveFocus();

    const info = screen.getByRole("button", { name: "關於這份學習單" });
    await user.click(info);
    expect(within(screen.getByRole("group", { name: "關於這份學習單" })).getByText(/Manna 產生的草稿/)).toBeInTheDocument();
    // Moving on with Tab closes it too.
    await user.tab();
    await waitFor(() => expect(screen.queryByRole("group", { name: "關於這份學習單" })).not.toBeInTheDocument());
  });

  it("列印樣式預覽 shows the printed sheet read-only; closing returns focus to 更多…", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await editorBox();
    const more = screen.getByRole("button", { name: "更多…" });
    await user.click((await openBarPopover(user, "更多…")).getByRole("button", { name: "列印樣式預覽" }));
    const dialog = within(await screen.findByRole("dialog", { name: "列印樣式預覽" }));
    const article = await dialog.findByRole("article", { name: /學生版/ });
    expect(within(article).getByText("Why?")).toBeInTheDocument();
    // Read-only: no block tools.
    expect(dialog.queryByRole("button", { name: /^改這一區/ })).not.toBeInTheDocument();
    expect(dialog.queryByRole("textbox")).not.toBeInTheDocument();

    await user.click(dialog.getByRole("radio", { name: "教師版" }));
    await dialog.findByRole("article", { name: /教師版/ });
    expect(api.callsTo("GET", `${WS}/sheet`).some((c) => c.url.includes("audience=teacher"))).toBe(true);

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "列印樣式預覽" })).not.toBeInTheDocument());
    expect(more).toHaveFocus();
  });

  it("詞彙檢查 opens the check in a dialog; closing returns focus and leaves the worksheet alone", async () => {
    const api = mockApi(routes({ [`GET ${WS}/checks`]: [] }));
    const user = userEvent.setup();
    renderApp("/prep/7");
    await editorBox();
    const button = screen.getByRole("button", { name: "詞彙檢查" });
    await user.click(button);
    const dialog = within(await screen.findByRole("dialog", { name: "詞彙級別檢查" }));
    expect(await dialog.findByRole("button", { name: "分析" })).toBeInTheDocument();
    expect(dialog.queryByRole("link", { name: "回學習單" })).not.toBeInTheDocument();
    const reads = api.callsTo("GET", WS).length;
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "詞彙級別檢查" })).not.toBeInTheDocument());
    expect(button).toHaveFocus();
    // Nothing was changed by the check: the worksheet is not read again.
    expect(api.callsTo("GET", WS)).toHaveLength(reads);
  });
});
