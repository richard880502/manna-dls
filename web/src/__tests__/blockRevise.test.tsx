// 學習單：逐區修改與註記 (docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §4–§5): in the 備課對話 preview a
// block can be targeted from the bottom bar (「針對 B 區」), text can be selected and noted, notes
// are listed under their block, and Manna's proposal waits for 採用／放棄／再改一次; in the editor
// the same with 改這一區, 加註記 and 註記 on a question, the block held while a proposal waits, and
// a save over a newer version offering 重新載入.
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  MemorySuggestion,
  PrepSession,
  PrepTurn,
  WorksheetComment,
  WorksheetContent,
  WorksheetState,
  WsAudience,
  WsBlock,
  WsBlockHistory,
  WsHistoryStep,
  WsItem,
  WsProposal,
  WsSheet,
} from "../api/types";
import { memorySuggestionsConfig } from "../components/memory/MemorySuggestions";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { blockReviseConfig } from "../lib/blockRevise";
import { prepConfig } from "../lib/prep";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";
import { clickBlockMore, openBarPopover, openBlockMore } from "../test/menus";

beforeEach(() => {
  signIn();
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
  blockReviseConfig.pollMs = 20;
  autosaveConfig.delayMs = 20;
  // Habit suggestions are read on load and when asked to, never on a timer here.
  memorySuggestionsConfig.pollAtMs = [];
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  blockReviseConfig.pollMs = 2000;
  autosaveConfig.delayMs = 1500;
  memorySuggestionsConfig.pollAtMs = [2500, 7000, 18000, 40000];
  window.getSelection()?.removeAllRanges();
});

// ---------------------------------------------------------------- fixtures

const WS = "/api/worksheets/11";

const item = (id: string, prompt: string, extra: Partial<WsItem> = {}): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: `Answer to ${id}`,
  evidence: "",
  purpose: null,
  levels: ["standard"],
  origin: "ai",
  ...extra,
});

const block = (id: string, extra: Partial<WsBlock>): WsBlock => ({
  id,
  kind: "activity",
  enabled: true,
  title: "",
  instructions: "",
  paragraph_ids: [],
  info_function: null,
  student_tasks: [],
  objective_ids: [],
  vocabulary: [],
  items: [],
  scaffolds: [],
  teacher_notes: "",
  origin: "ai",
  message: null,
  sop_block_id: null,
  ...extra,
});

const B2_ITEMS = [
  item("b2-1", "Why did Mrs. Lin start the stand?"),
  item("b2-2", "What did the neighbors expect would happen?"),
];

const content = (b2Items: WsItem[] = B2_ITEMS): WorksheetContent => ({
  objectives: [],
  show_objectives: false,
  levels: ["standard"],
  chosen: null,
  notices: [],
  blocks: [
    block("w", { kind: "warmup", title: "Warm-up", items: [item("w-1", "Have you ever borrowed an umbrella?")] }),
    block("b2", { title: "Paragraph 1: Find", instructions: "Read paragraph 1.", items: b2Items }),
  ],
});

const worksheet = (overrides: Partial<WorksheetState> = {}): WorksheetState => ({
  id: 11,
  title: "The Umbrella Stand",
  filename: "The Umbrella Stand_學習單",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: 3,
  created_at: "2026-11-15T08:00:00Z",
  updated_at: "2026-11-15T08:00:00Z",
  material: {
    title: "The Umbrella Stand",
    paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
    source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 },
  },
  settings: emptySettings(),
  suggested: emptySettings(),
  brief: emptyBrief(),
  versions_count: 3,
  saved_at: "2026-11-15T08:00:00Z",
  content: content(),
  jobs: [],
  running: false,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
  ...overrides,
});

const sheet = (audience: WsAudience, b2Items: WsItem[] = B2_ITEMS): WsSheet => ({
  title: "The Umbrella Stand",
  level: "standard",
  audience,
  objectives: [],
  blocks: [
    {
      id: "w",
      kind: "warmup",
      label: "A",
      title: "Warm-up",
      instructions: "",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ id: "w-1", no: 1, answer_format: "簡答題", prompt: "Have you ever borrowed an umbrella?", options: [], lines: 2 }],
    },
    {
      id: "b2",
      kind: "activity",
      label: "B",
      title: "Paragraph 1: Find",
      instructions: "Read paragraph 1.",
      steps: [],
      vocabulary: [],
      supports: [],
      items: b2Items.map((it, k) => ({
        id: it.id,
        no: k + 2,
        answer_format: it.answer_format,
        prompt: it.prompt,
        options: [],
        lines: 3,
      })),
    },
  ],
});

const note = (overrides: Partial<WorksheetComment> = {}): WorksheetComment => ({
  id: 41,
  worksheet_id: 11,
  block_id: "b2",
  field: "item",
  item_id: "b2-2",
  quote: "neighbors",
  note: "這題太難，改簡單一點",
  share_with_ai: true,
  status: "open",
  proposal_id: null,
  anchor: { status: "ok", block_id: "b2", item_id: "b2-2", start: 9, end: 18 },
  created_at: "2026-11-15T09:00:00Z",
  updated_at: "2026-11-15T09:00:00Z",
  ...overrides,
});

const REVISED = [B2_ITEMS[0]!, item("b2-2", "Did the neighbors think the umbrellas would stay?", { origin: "ai" })];

const proposal = (status: WsProposal["status"], overrides: Partial<WsProposal> = {}): WsProposal => {
  const before = content().blocks[1]!;
  const proposed = status === "running" || status === "failed" ? null : { ...before, items: REVISED };
  return {
    id: 90,
    worksheet_id: 11,
    block_id: "b2",
    status,
    instruction: "",
    comment_ids: [41],
    whole_block: false,
    round: 1,
    reply: status === "proposed" ? "已把第 3 題改簡單。" : "",
    error: status === "failed" ? "Manna 沒有改動這一區：你想改成哪一種題型？" : null,
    base_version_no: 3,
    before,
    block: proposed,
    changes: proposed
      ? {
          title: false,
          instructions: false,
          items: [
            { id: "b2-1", change: "kept" },
            { id: "b2-2", change: "modified" },
          ],
        }
      : null,
    interaction_id: 300,
    accepted_version_no: null,
    created_at: "2026-11-15T09:00:00Z",
    finished_at: null,
    decided_at: null,
    ...overrides,
  };
};

const wsTurn = (): PrepTurn => ({
  id: 101,
  seq: 1,
  kind: "worksheet",
  request: "做學習單",
  status: "done",
  error: null,
  worksheet: { id: 11, title: "The Umbrella Stand", version_no: 3, running: false },
  result: null,
  interaction_id: null,
  created_at: "2026-11-15T08:00:00Z",
  finished_at: "2026-11-15T08:01:00Z",
});

const session = (): PrepSession => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: 1,
  last_kind: "worksheet",
  updated_at: "2026-11-15T09:00:00Z",
  created_at: "2026-11-15T08:00:00Z",
  turns: [wsTurn()],
  running: false,
  next: [],
});

const sheetRoute = (items?: WsItem[]) => (call: MockCall) =>
  sheet(new URL(call.url, "http://x").searchParams.get("audience") as WsAudience, items);

const prepBlock = () => within(screen.getByRole("region", { name: /^區塊 B/ }));

// ---------------------------------------------------------------- 備課對話

describe("備課對話: the composer", () => {
  it("the expand corner makes the box taller and back, keeping the text", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/sheet`]: sheetRoute(),
      [`GET ${WS}/comments`]: [],
      [`GET ${WS}/proposals`]: [],
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const bar = await screen.findByRole("textbox", { name: "給 Manna 的話" });
    await user.type(bar, "第 2 題太難");
    const grow = screen.getByRole("button", { name: "放大輸入框" });
    expect(grow).toHaveAttribute("aria-pressed", "false");
    await user.click(grow);
    const shrink = screen.getByRole("button", { name: "縮小輸入框" });
    expect(shrink).toHaveAttribute("aria-pressed", "true");
    expect(bar.closest(".prep-composer")).toHaveAttribute("data-tall");
    expect(bar).toHaveFocus();
    expect(bar).toHaveValue("第 2 題太難");
    await user.click(shrink);
    expect(bar.closest(".prep-composer")).not.toHaveAttribute("data-tall");
  });
});

describe("備課對話: one block of the worksheet", () => {
  it("改這一區 targets the block from the bottom bar; the proposal waits for 採用", async () => {
    let reads = 0;
    let accepted = false;
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: () => (accepted ? worksheet({ version_no: 4, undoable: ["b2"], content: content(REVISED) }) : worksheet()),
      [`GET ${WS}/sheet`]: (call: MockCall) => sheetRoute(accepted ? REVISED : undefined)(call),
      [`GET ${WS}/comments`]: [],
      [`GET ${WS}/proposals`]: () => {
        if (accepted) return [];
        reads += 1;
        return reads < 2 ? [] : reads < 4 ? [proposal("running", { comment_ids: [], instruction: "第 3 題改簡單" })] : [proposal("proposed")];
      },
      [`POST ${WS}/blocks/b2/revise`]: proposal("running", { comment_ids: [], instruction: "第 3 題改簡單" }),
      [`POST ${WS}/proposals/90/accept`]: () => {
        accepted = true;
        return { proposal: proposal("accepted"), worksheet: worksheet({ version_no: 4, undoable: ["b2"], content: content(REVISED) }) };
      },
      [`POST ${WS}/blocks/b2/undo`]: () => {
        accepted = false;
        return worksheet({ version_no: 5 });
      },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "改這一區（區塊 B）" }));
    expect(screen.getByText("針對 B 區")).toBeInTheDocument();
    const bar = screen.getByRole("textbox", { name: "給 Manna 的話" });
    expect(bar).toHaveAttribute("placeholder", "想怎麼改 B 區？例如：第 2 題太難，改簡單一點");
    await user.type(bar, "第 3 題改簡單{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/revise`)).toHaveLength(1));
    expect(api.lastBody("POST", `${WS}/blocks/b2/revise`)).toEqual({ instruction: "第 3 題改簡單" });
    expect(api.callsTo("POST", "/api/prep-sessions/7/turns")).toHaveLength(0);
    expect(screen.queryByText("針對 B 區")).not.toBeInTheDocument();

    expect(await prepBlock().findByText("Manna 正在改這一區…")).toBeInTheDocument();
    const panel = within(await screen.findByRole("region", { name: "Manna 的修改建議" }));
    expect(panel.getByText("已把第 3 題改簡單。")).toBeInTheDocument();
    expect(panel.getByText("Did the neighbors think the umbrellas would stay?")).toBeInTheDocument();
    expect(panel.getByText("What did the neighbors expect would happen?").tagName).toBe("DEL");
    expect(panel.getByText("修改")).toBeInTheDocument();

    await user.click(panel.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/proposals/90/accept`)).toHaveLength(1));
    expect(await prepBlock().findByText("Did the neighbors think the umbrellas would stay?")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Manna 的修改建議" })).not.toBeInTheDocument();
    // Right after 採用: the notice with 復原 under the block.
    expect(prepBlock().getByText("已採用 Manna 的修改。")).toBeInTheDocument();
    // The panel (and its 採用 button) is gone: focus is on the block's heading, not <body>.
    expect(document.activeElement).toHaveClass("ws-block-head");
    await user.click(prepBlock().getByRole("button", { name: /^更多/ }));
    expect(prepBlock().getByRole("button", { name: "復原上一版" })).toBeInTheDocument();
    await user.click(prepBlock().getByRole("button", { name: "復原" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/undo`)).toHaveLength(1));
    await waitFor(() => expect(prepBlock().queryByText("已採用 Manna 的修改。")).not.toBeInTheDocument());
    expect(await prepBlock().findByText("What did the neighbors expect would happen?")).toBeInTheDocument();
  });

  it("lists the notes under their block: 提供給 AI, 略過, 刪除, and 依註記修改", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/sheet`]: sheetRoute(),
      [`GET ${WS}/comments`]: [note(), note({ id: 43, field: "block", item_id: null, quote: "", note: "只給我自己看", share_with_ai: false, anchor: { status: "ok", block_id: "b2", item_id: null, start: null, end: null } })],
      [`GET ${WS}/proposals`]: [],
      [`PATCH ${WS}/comments/41`]: (call: MockCall) => ({ ...note(), ...(call.body as object) }),
      [`DELETE ${WS}/comments/43`]: { deleted: true },
      [`POST ${WS}/blocks/b2/revise`]: proposal("running"),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await screen.findByRole("region", { name: /^區塊 B/ });
    const first = within(await prepBlock().findByRole("group", { name: "我的註記：第 3 題" }));
    expect(first.getByText("待處理")).toBeInTheDocument();
    expect(first.getByText("「neighbors」")).toBeInTheDocument();
    // Only the note for Manna counts.
    await user.click(prepBlock().getByRole("button", { name: "依註記修改（1 則）" }));
    await waitFor(() => expect(api.lastBody("POST", `${WS}/blocks/b2/revise`)).toEqual({ comment_ids: [41] }));

    await user.click(first.getByRole("checkbox", { name: "提供給 AI" }));
    await waitFor(() => expect(api.lastBody("PATCH", `${WS}/comments/41`)).toEqual({ share_with_ai: false }));
    await user.click(first.getByRole("button", { name: "略過" }));
    await waitFor(() => expect(api.lastBody("PATCH", `${WS}/comments/41`)).toEqual({ status: "dismissed" }));
    expect(await first.findByText("已略過")).toBeInTheDocument();

    const second = within(prepBlock().getByRole("group", { name: "我的註記：這一區" }));
    await user.click(second.getByRole("button", { name: "刪除" }));
    await user.click(second.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(api.callsTo("DELETE", `${WS}/comments/43`)).toHaveLength(1));
    await waitFor(() => expect(prepBlock().queryByText("只給我自己看")).not.toBeInTheDocument());
  });

  it("a failed revision says why; 再改一次 asks again, 知道了 closes it", async () => {
    let open: WsProposal[] = [proposal("failed")];
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/sheet`]: sheetRoute(),
      [`GET ${WS}/comments`]: [note()],
      [`GET ${WS}/proposals`]: () => open,
      [`POST ${WS}/blocks/b2/revise`]: () => {
        open = [proposal("proposed", { id: 91, round: 2 })];
        return proposal("running", { id: 91, round: 2 });
      },
      [`POST ${WS}/proposals/91/reject`]: () => {
        open = [];
        return proposal("rejected", { id: 91 });
      },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await screen.findByRole("region", { name: /^區塊 B/ });
    const alert = await prepBlock().findByRole("alert");
    expect(alert).toHaveTextContent("你想改成哪一種題型？");
    await user.click(within(alert).getByRole("button", { name: "再改一次" }));
    await waitFor(() => expect(api.lastBody("POST", `${WS}/blocks/b2/revise`)).toEqual({ comment_ids: [41] }));
    const panel = within(await screen.findByRole("region", { name: "Manna 的修改建議" }));
    expect(panel.getByText(/（第 2 次）/)).toBeInTheDocument();
    await user.click(panel.getByRole("button", { name: "放棄" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/proposals/91/reject`)).toHaveLength(1));
    await waitFor(() => expect(screen.queryByRole("region", { name: "Manna 的修改建議" })).not.toBeInTheDocument());
  });
});

// ---------------------------------------------------------------- the editor

const editorRoutes = (state: WorksheetState, extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  "GET /api/worksheets": [],
  [`GET ${WS}`]: state,
  [`GET ${WS}/comments`]: [],
  [`GET ${WS}/proposals`]: [],
  ...extra,
});

const editorBlock = () => within(screen.getByRole("region", { name: "區塊 B" }));

/** Selects characters [from, to) of a field as the keyboard (Shift+arrows) would. */
function selectChars(field: HTMLElement, from: number, to: number) {
  const ta = field as HTMLTextAreaElement;
  ta.focus();
  ta.setSelectionRange(from, to);
  fireEvent.select(ta);
}

describe("編輯器: 選取文字加註記", () => {
  const open = async (extra: Record<string, unknown> = {}) => {
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`POST ${WS}/comments`]: note({ id: 42 }),
        [`POST ${WS}/blocks/b2/revise`]: proposal("running", { comment_ids: [42] }),
        ...extra,
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    return { api, user };
  };
  const NOTE_BUTTON = { name: "為選取的文字加註記" };

  it("part of a question prompt offers 加註記; it opens the note with the word-snapped quote and saves it", async () => {
    const { api, user } = await open();
    const prompt = await editorBlock().findByRole("textbox", { name: "題目 3" });
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull();
    selectChars(prompt, 13, 19); // "neighb" of "neighbors"
    await user.click(await screen.findByRole("button", NOTE_BUTTON));
    const draft = within(screen.getByRole("form", { name: "新增註記" }));
    expect(draft.getByText("第 3 題")).toBeInTheDocument();
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull();
    await user.type(draft.getByRole("textbox", { name: "註記內容" }), "這題太難，改簡單一點");
    await user.click(draft.getByRole("button", { name: "儲存並請 Manna 改" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/revise`)).toHaveLength(1));
    expect(api.lastBody("POST", `${WS}/comments`)).toEqual({
      block_id: "b2",
      field: "item",
      item_id: "b2-2",
      start: 13,
      end: 22,
      quote: "neighbors",
      note: "這題太難，改簡單一點",
      share_with_ai: true,
    });
    expect(api.lastBody("POST", `${WS}/blocks/b2/revise`)).toEqual({ comment_ids: [42] });
  });

  it("a mouse selection offers it too, next to the pointer; the title and instructions keep their own field", async () => {
    const { api, user } = await open();
    const title = await editorBlock().findByRole("textbox", { name: "區塊標題 B" });
    const value = (title as HTMLTextAreaElement).value;
    fireEvent.mouseDown(title);
    selectChars(title, 0, value.length);
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull(); // still dragging
    fireEvent.mouseUp(title, { clientX: 50, clientY: 40 });
    await user.click(await screen.findByRole("button", NOTE_BUTTON));
    const draft = within(screen.getByRole("form", { name: "新增註記" }));
    expect(draft.getByText("標題")).toBeInTheDocument();
    await user.type(draft.getByRole("textbox", { name: "註記內容" }), "換個標題");
    await user.click(draft.getByRole("button", { name: "儲存註記" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/comments`)).toHaveLength(1));
    expect(api.lastBody("POST", `${WS}/comments`)).toMatchObject({ field: "title", start: 0, end: value.length, quote: value });
  });

  it("is reachable with Tab and Enter, and Escape or a collapsed selection hides it", async () => {
    const { user } = await open();
    const prompt = await editorBlock().findByRole("textbox", { name: "題目 3" });
    selectChars(prompt, 0, 4);
    const button = await screen.findByRole("button", NOTE_BUTTON);
    expect(prompt.nextElementSibling).toBe(button);
    await user.tab();
    expect(button).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("form", { name: "新增註記" })).toBeInTheDocument();

    selectChars(prompt, 0, 4);
    await screen.findByRole("button", NOTE_BUTTON);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull();
    selectChars(prompt, 1, 4);
    await screen.findByRole("button", NOTE_BUTTON);
    selectChars(prompt, 2, 2);
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull();
    selectChars(prompt, 0, 5);
    await screen.findByRole("button", NOTE_BUTTON);
    fireEvent.scroll(window);
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull();
  });

  it("offers nothing while the block is held by a proposal", async () => {
    await open({ [`GET ${WS}/proposals`]: [proposal("proposed")] });
    const prompt = await editorBlock().findByRole("textbox", { name: "題目 3" });
    await waitFor(() => expect(prompt).toBeDisabled());
    selectChars(prompt, 0, 4);
    expect(screen.queryByRole("button", NOTE_BUTTON)).toBeNull();
  });
});

describe("編輯器: 改這一區, notes and saving", () => {
  it("改這一區 asks Manna for this block; the block is held until 採用, which takes the new version", async () => {
    let open: WsProposal[] = [];
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`GET ${WS}/proposals`]: () => open,
        [`POST ${WS}/blocks/b2/revise`]: () => {
          open = [proposal("proposed", { instruction: "兩題都改成選擇題", comment_ids: [] })];
          return proposal("running", { instruction: "兩題都改成選擇題", comment_ids: [] });
        },
        [`POST ${WS}/proposals/90/accept`]: () => {
          open = [];
          return {
            proposal: proposal("accepted"),
            worksheet: worksheet({ version_no: 4, undoable: ["b2"], content: content(REVISED) }),
          };
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    await user.click(await editorBlock().findByRole("button", { name: "改這一區（區塊 B）" }));
    const form = within(screen.getByRole("form", { name: "請 Manna 改區塊 B" }));
    await user.type(form.getByRole("textbox", { name: "想怎麼改區塊 B？" }), "兩題都改成選擇題");
    await user.click(form.getByRole("button", { name: "請 Manna 改這一區" }));
    await waitFor(() => expect(api.lastBody("POST", `${WS}/blocks/b2/revise`)).toEqual({ instruction: "兩題都改成選擇題" }));

    const panel = within(await editorBlock().findByRole("region", { name: "Manna 的修改建議" }));
    // Held: the block cannot be edited while the proposal waits.
    expect(editorBlock().getByRole("textbox", { name: "題目 3" })).toBeDisabled();
    await user.click(panel.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/proposals/90/accept`)).toHaveLength(1));
    await waitFor(() =>
      expect(editorBlock().getByRole("textbox", { name: "題目 3" })).toHaveValue("Did the neighbors think the umbrellas would stay?"),
    );
    expect(editorBlock().getByRole("textbox", { name: "題目 3" })).toBeEnabled();
    expect((await openBlockMore(user, editorBlock())).getByRole("button", { name: "復原上一版" })).toBeInTheDocument();
  });

  it("註記 on a question saves a note on it; 加註記 on the block", async () => {
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`POST ${WS}/comments`]: (call: MockCall) => {
          const body = call.body as { field: string; item_id?: string; note: string };
          return note({
            id: body.field === "item" ? 50 : 51,
            field: body.field as WorksheetComment["field"],
            item_id: body.item_id ?? null,
            quote: "",
            note: body.note,
            anchor: { status: "ok", block_id: "b2", item_id: body.item_id ?? null, start: null, end: null },
          });
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    await user.click(await editorBlock().findByRole("button", { name: "註記題目 3" }));
    const draft = within(screen.getByRole("form", { name: "新增註記" }));
    expect(draft.getByText("第 3 題")).toBeInTheDocument();
    await user.type(draft.getByRole("textbox", { name: "註記內容" }), "改成選擇題");
    await user.click(draft.getByRole("button", { name: "儲存註記" }));
    await waitFor(() =>
      expect(api.lastBody("POST", `${WS}/comments`)).toEqual({
        block_id: "b2",
        field: "item",
        item_id: "b2-2",
        note: "改成選擇題",
        share_with_ai: true,
      }),
    );
    expect(await editorBlock().findByRole("group", { name: "我的註記：第 3 題" })).toBeInTheDocument();

    await user.click(editorBlock().getByRole("button", { name: "加註記（區塊 B）" }));
    const blockDraft = within(screen.getByRole("form", { name: "新增註記" }));
    await user.click(blockDraft.getByRole("checkbox", { name: "提供給 AI" }));
    expect(blockDraft.getByRole("button", { name: "儲存並請 Manna 改" })).toBeDisabled();
    await user.type(blockDraft.getByRole("textbox", { name: "註記內容" }), "只給我自己看");
    await user.click(blockDraft.getByRole("button", { name: "儲存註記" }));
    await waitFor(() =>
      expect(api.lastBody("POST", `${WS}/comments`)).toEqual({
        block_id: "b2",
        field: "block",
        note: "只給我自己看",
        share_with_ai: false,
      }),
    );
  });

  it("autosave sends the version it edited; a 409 offers 重新載入", async () => {
    let loads = 0;
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`GET ${WS}`]: () => {
          loads += 1;
          return loads === 1 ? worksheet() : worksheet({ version_no: 4, title: "Changed elsewhere" });
        },
        [`PUT ${WS}`]: respond(409, {
          error: { code: "conflict", message: "這份學習單在你編輯的同時有了新的版本，所以你最近的修改沒有存進去。請重新載入，再改一次。" },
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    const prompt = await editorBlock().findByRole("textbox", { name: "題目 2" });
    await user.type(prompt, "!");
    await waitFor(() => expect(api.callsTo("PUT", WS).length).toBeGreaterThan(0));
    expect((api.lastBody("PUT", WS) as { base_version_no: number }).base_version_no).toBe(3);
    expect(await screen.findByText(/沒有存到：這份學習單在你編輯的同時有了新的版本/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重新載入" }));
    expect(await screen.findByRole("heading", { name: "Changed elsewhere", level: 1 })).toBeInTheDocument();
    expect(loads).toBe(2);
  });
});

describe("編輯器: 採用之後的復原與同一輪的各次建議", () => {
  it("right after 採用 the block says so with a 復原 button that gives the block back", async () => {
    let open: WsProposal[] = [proposal("proposed", { comment_ids: [] })];
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`GET ${WS}/proposals`]: () => open,
        [`POST ${WS}/proposals/90/accept`]: () => {
          open = [];
          return {
            proposal: proposal("accepted"),
            worksheet: worksheet({ version_no: 4, undoable: ["b2"], content: content(REVISED) }),
          };
        },
        [`POST ${WS}/blocks/b2/undo`]: () => worksheet({ version_no: 5 }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    const panel = within(await editorBlock().findByRole("region", { name: "Manna 的修改建議" }));
    await user.click(panel.getByRole("button", { name: "採用" }));
    const notice = await editorBlock().findByText("已採用 Manna 的修改。");
    expect(notice.closest("[role=status]")).not.toBeNull();
    // The existing 復原上一版 stays among the block tools.
    expect((await openBlockMore(user, editorBlock())).getByRole("button", { name: "復原上一版" })).toBeInTheDocument();
    await user.click(editorBlock().getByRole("button", { name: "復原" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/undo`)).toHaveLength(1));
    await waitFor(() => expect(editorBlock().queryByText("已採用 Manna 的修改。")).toBeNull());
    await waitFor(() =>
      expect(editorBlock().getByRole("textbox", { name: "題目 3" })).toHaveValue("What did the neighbors expect would happen?"),
    );
  });

  it("the box switches between the attempts of the round and 採用 adopts the one shown", async () => {
    const first = proposal("proposed", {
      id: 88,
      round: 1,
      instruction: "改簡單",
      reply: "第一次的說明",
      comment_ids: [],
      decided_at: "2026-11-15T09:01:00Z",
    });
    const newest = proposal("proposed", {
      id: 90,
      round: 2,
      instruction: "改簡單",
      reply: "第二次的說明",
      comment_ids: [],
      earlier: [first],
    });
    let open: WsProposal[] = [newest];
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`GET ${WS}/proposals`]: () => open,
        [`POST ${WS}/proposals/88/accept`]: () => {
          open = [];
          return {
            proposal: proposal("accepted", { id: 88 }),
            worksheet: worksheet({ version_no: 4, undoable: ["b2"], content: content(REVISED) }),
          };
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    const panel = within(await editorBlock().findByRole("region", { name: "Manna 的修改建議" }));
    // The newest shows first.
    expect(panel.getByText("第 2 次（共 2 次）")).toBeInTheDocument();
    expect(panel.getByText("第二次的說明")).toBeInTheDocument();
    expect(panel.getByRole("button", { name: "下一次的建議" })).toBeDisabled();
    await user.click(panel.getByRole("button", { name: "上一次的建議" }));
    expect(panel.getByText("第 1 次（共 2 次）")).toBeInTheDocument();
    expect(panel.getByText("第一次的說明")).toBeInTheDocument();
    expect(panel.getByRole("button", { name: "上一次的建議" })).toBeDisabled();
    await user.click(panel.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/proposals/88/accept`)).toHaveLength(1));
    expect(api.callsTo("POST", `${WS}/proposals/90/accept`)).toHaveLength(0);
    await waitFor(() => expect(editorBlock().queryByRole("region", { name: "Manna 的修改建議" })).toBeNull());
  });
});

describe("備課對話: 更多… menu of a block", () => {
  it("opens 修改歷程 from the menu and puts focus back on 更多… when the dialog closes", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/sheet`]: sheetRoute(),
      [`GET ${WS}/comments`]: [],
      [`GET ${WS}/proposals`]: [],
      [`GET ${WS}/blocks/b2/history`]: { block_id: "b2", version_no: 3, busy: false, steps: [] },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await screen.findByRole("region", { name: /^區塊 B/ });
    // The two main tools stay on the block; the rest is in the menu.
    expect(prepBlock().getByRole("button", { name: /^改這一區/ })).toBeInTheDocument();
    expect(prepBlock().getByRole("button", { name: /^加註記/ })).toBeInTheDocument();
    expect(prepBlock().queryByRole("button", { name: /修改歷程/ })).not.toBeInTheDocument();
    const more = prepBlock().getByRole("button", { name: /^更多/ });
    await user.click(more);
    await user.click(prepBlock().getByRole("button", { name: /修改歷程/ }));
    const dialog = await screen.findByRole("dialog", { name: "修改歷程" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(more).toHaveFocus();
  });
});

describe("編輯器: 修改歷程", () => {
  const step = (over: Partial<WsHistoryStep>): WsHistoryStep => ({
    kind: "original",
    label: "原本",
    at: "2026-11-15T08:00:00Z",
    version_no: 1,
    proposal_id: null,
    current: false,
    detail: "",
    merged_version_nos: [],
    undone: false,
    adopted_later_version_no: null,
    block: content().blocks[1]!,
    pictures: null,
    picture_actions: [],
    adopt: { possible: true, clean: true, warn_older: false, draws_pictures: false, pictures_unknown: true, reason: null },
    ...over,
  });
  const history = (): WsBlockHistory => ({
    block_id: "b2",
    version_no: 3,
    busy: false,
    steps: [
      step({
        kind: "manual",
        label: "你手動修改",
        version_no: 3,
        current: true,
        at: "2026-11-15T10:00:00Z",
        adopt: { possible: false, clean: true, warn_older: false, draws_pictures: false, pictures_unknown: true, reason: "這就是現在的版本。" },
      }),
      step({
        kind: "proposal_rejected",
        label: "Manna 的提案（沒採用）",
        version_no: null,
        proposal_id: 90,
        at: "2026-11-15T09:00:00Z",
        detail: "已把第 3 題改簡單。",
        picture_actions: [{ action: "add", slot_id: "", text: "加一張圖：蜜蜂" }],
        adopt: { possible: true, clean: false, warn_older: true, draws_pictures: true, pictures_unknown: false, reason: null },
      }),
      step({ version_no: 1 }),
    ],
  });

  it("lists the steps, steps through previews, warns before adopting an old proposal and adopts a past version", async () => {
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`GET ${WS}/blocks/b2/history`]: history(),
        [`POST ${WS}/blocks/b2/adopt`]: () => ({
          version_no: 4,
          kind: "version",
          pictures: [],
          worksheet: worksheet({ version_no: 4 }),
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    await clickBlockMore(user, editorBlock(), "修改歷程（區塊 B）");
    const dialog = within(await screen.findByRole("dialog", { name: "修改歷程" }));
    const list = within(await dialog.findByRole("list", { name: "這一區的修改歷程" }));
    expect(list.getAllByRole("button").map((b) => b.textContent)).toEqual([
      expect.stringContaining("你手動修改"),
      expect.stringContaining("Manna 的提案（沒採用）"),
      expect.stringContaining("原本"),
    ]);
    expect(dialog.getByText("第 1 步（共 3 步）")).toBeInTheDocument();
    expect(dialog.getByRole("button", { name: "採用這個版本" })).toBeDisabled();
    // ‹ › step through the previews.
    await user.click(dialog.getByRole("button", { name: "較舊的一步" }));
    expect(dialog.getByText("第 2 步（共 3 步）")).toBeInTheDocument();
    expect(dialog.getByText("已把第 3 題改簡單。")).toBeInTheDocument();
    // A proposal made from older content, with picture actions that never ran: asks first.
    await user.click(dialog.getByRole("button", { name: "採用這個版本" }));
    expect(dialog.getByText(/這個版本是根據比較早的內容改的，採用後會取代這一區之後的修改/)).toBeInTheDocument();
    expect(dialog.getByText(/會用到 AI 產圖/)).toBeInTheDocument();
    expect(api.callsTo("POST", `${WS}/blocks/b2/adopt`)).toHaveLength(0);
    await user.click(dialog.getByRole("button", { name: "先不要" }));
    // A past version adopts at once.
    await user.click(dialog.getByRole("button", { name: "較舊的一步" }));
    await user.click(dialog.getByRole("button", { name: "採用這個版本" }));
    await waitFor(() => expect(api.lastBody("POST", `${WS}/blocks/b2/adopt`)).toEqual({ version_no: 1 }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "修改歷程" })).toBeNull());
  });

  it("adopts a rejected proposal by its id after confirming", async () => {
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`GET ${WS}/blocks/b2/history`]: history(),
        [`POST ${WS}/blocks/b2/adopt`]: () => ({
          version_no: 4,
          kind: "proposal",
          pictures: [],
          worksheet: worksheet({ version_no: 4 }),
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    await clickBlockMore(user, editorBlock(), "修改歷程（區塊 B）");
    const dialog = within(await screen.findByRole("dialog", { name: "修改歷程" }));
    await user.click(await dialog.findByRole("button", { name: /Manna 的提案（沒採用）/ }));
    await user.click(dialog.getByRole("button", { name: "採用這個版本" }));
    await user.click(dialog.getByRole("button", { name: "仍要採用" }));
    await waitFor(() => expect(api.lastBody("POST", `${WS}/blocks/b2/adopt`)).toEqual({ proposal_id: 90 }));
  });
});

// ---------------------------------------------------------------- 鷹架與字詞表

const withExtras = (extra: Partial<WsBlock> = {}): WsBlock => ({
  ...content().blocks[1]!,
  scaffolds: [
    {
      id: "sc-steps",
      type: "procedural",
      kind: "清楚的分步指示及完成順序",
      content: "",
      steps: ["Read paragraph 1.", "Underline the first event."],
      levels: ["standard"],
      auto: true,
      origin: "ai",
    },
    {
      id: "sc-frame",
      type: "verbal",
      kind: "句型框架",
      content: "The first event is ___.",
      steps: [],
      levels: ["standard"],
      auto: false,
      origin: "ai",
    },
  ],
  vocabulary: [
    { word: "umbrella", level: 2, definition: "雨傘", context: "", paragraph_id: "p1" },
    { word: "identify", level: null, definition: "找出、指出", context: "", paragraph_id: null, task_word: true },
  ],
  ...extra,
});

const contentWith = (b2: WsBlock): WorksheetContent => ({ ...content(), blocks: [content().blocks[0]!, b2] });

describe("編輯器: 鷹架與字詞表", () => {
  it("shows Follow the steps below. and what a step that names a question by number prints", async () => {
    // The warm-up prints question 1, so block B's own questions 1–2 print as 2–3.
    const b2 = withExtras({
      items: [item("b2-i1", "Who started the stand?"), item("b2-i2", "Use your answer to item 1. Why?")],
    });
    b2.scaffolds[0] = { ...b2.scaffolds[0]!, steps: ["Read paragraph 1.", "Answer item 2 in plain English."] };
    mockApi(editorRoutes(worksheet({ content: contentWith(b2) })));
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    const block = editorBlock();
    expect(await block.findByText("Follow the steps below.")).toBeInTheDocument();
    expect(block.getByText("Answer item 3 in plain English.")).toBeInTheDocument();
    expect(block.getByText("Use your answer to item 2. Why?")).toBeInTheDocument();
    // A step without a question number gets no note.
    expect(block.queryByText("Read paragraph 1.", { selector: ".ws-printed-as span" })).toBeNull();
  });

  it("the proposal lists what happens to the scaffolds and the word table, and 採用 applies it", async () => {
    const before = withExtras();
    const after = {
      ...before,
      vocabulary: [],
      scaffolds: [before.scaffolds[0]!, { ...before.scaffolds[1]!, content: "The opening event is ___.", origin: "ai" as const }],
    };
    const p = proposal("proposed", {
      instruction: "不要單字表格",
      comment_ids: [],
      before,
      block: after,
      changes: {
        title: false,
        instructions: false,
        items: [
          { id: "b2-1", change: "kept" },
          { id: "b2-2", change: "kept" },
        ],
        extras: [
          { group: "scaffold", id: "sc-frame", label: "句型框架", change: "modified" },
          { group: "word", id: "umbrella", label: "umbrella", change: "removed" },
          { group: "word", id: "identify", label: "identify", change: "removed" },
        ],
      },
    });
    let open: WsProposal[] = [p];
    const api = mockApi(
      editorRoutes(worksheet({ content: contentWith(before) }), {
        [`GET ${WS}/proposals`]: () => open,
        [`POST ${WS}/proposals/90/accept`]: () => {
          open = [];
          return {
            proposal: { ...p, status: "accepted" },
            worksheet: worksheet({ version_no: 4, undoable: ["b2"], content: contentWith(after) }),
          };
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    // The table is on the sheet before.
    expect(await editorBlock().findAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(2);
    const panel = within(await editorBlock().findByRole("region", { name: "Manna 的修改建議" }));
    const list = within(panel.getByRole("list", { name: "鷹架與字詞表的修改" }));
    const rows = list.getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("修改 鷹架（句型框架）：The opening event is ___.");
    expect(rows[0]).toHaveTextContent("原本：The first event is ___.");
    expect(rows[1]).toHaveTextContent("刪除 字詞表（umbrella）：雨傘");
    expect(rows[2]).toHaveTextContent("刪除 字詞表（identify）");
    expect(within(rows[1]!).getByText("雨傘").tagName).toBe("DEL");

    await user.click(panel.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/proposals/90/accept`)).toHaveLength(1));
    await waitFor(() => expect(editorBlock().queryAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(0));
    expect(editorBlock().getByRole("textbox", { name: "句型框架（B）" })).toHaveValue("The opening event is ___.");
  });

  it("the 刪除 next to the word table removes it without asking Manna", async () => {
    const before = withExtras();
    const api = mockApi(
      editorRoutes(worksheet({ content: contentWith(before) }), {
        [`PUT ${WS}`]: (call: MockCall) => worksheet({ version_no: 4, content: (call.body as { content: WorksheetContent }).content }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    const tables = await editorBlock().findAllByRole("table", { name: /^字詞表 B/ });
    // Content and Academic side by side: a header row and one word each.
    expect(tables.map((t) => within(t).getAllByRole("row").length)).toEqual([2, 2]);
    // Every other 刪除 on the block is for something else: the table has its own.
    await user.click(editorBlock().getByRole("button", { name: "刪除字詞表 B" }));
    await waitFor(() => expect(editorBlock().queryAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(0));
    await waitFor(() => expect(api.callsTo("PUT", WS).length).toBeGreaterThan(0));
    const saved = (api.lastBody("PUT", WS) as { content: WorksheetContent }).content.blocks[1]!;
    expect(saved.vocabulary).toEqual([]);
    expect(saved.scaffolds).toEqual(before.scaffolds);
    // The 程序鷹架 and the 句型框架 still have theirs.
    expect(editorBlock().getByRole("button", { name: "刪除程序鷹架" })).toBeInTheDocument();
    expect(editorBlock().getByRole("button", { name: "刪除鷹架 句型框架" })).toBeInTheDocument();
  });
});

describe("編輯器: 字詞表的位置與恢復", () => {
  it("字詞表放在學習單最後 moves every table after the last block and is saved with the content", async () => {
    const before = withExtras();
    const api = mockApi(
      editorRoutes(worksheet({ content: contentWith(before) }), {
        [`PUT ${WS}`]: (call: MockCall) => worksheet({ version_no: 4, content: (call.body as { content: WorksheetContent }).content }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    expect(editorBlock().getAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(2);
    expect(screen.queryByRole("region", { name: "字詞表（學習單最後）" })).toBeNull();

    await user.click((await openBarPopover(user, "顯示")).getByRole("checkbox", { name: "字詞表放在學習單最後" }));
    const end = await screen.findByRole("region", { name: "字詞表（學習單最後）" });
    expect(within(end).getAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(2);
    expect(editorBlock().queryAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(0);
    // Still editable where it is now.
    expect(within(end).getByRole("textbox", { name: "umbrella 的解釋" })).toHaveValue("雨傘");
    await waitFor(() => expect(api.callsTo("PUT", WS).length).toBeGreaterThan(0));
    const saved = (api.lastBody("PUT", WS) as { content: WorksheetContent }).content;
    expect(saved.vocabulary_at_end).toBe(true);
    expect(saved.blocks[1]!.vocabulary).toEqual(before.vocabulary);

    // And back.
    await user.click((await openBarPopover(user, "顯示")).getByRole("checkbox", { name: "字詞表放在學習單最後" }));
    await waitFor(() => expect(editorBlock().getAllByRole("table", { name: /^字詞表 B/ })).toHaveLength(2));
    expect(screen.queryByRole("region", { name: "字詞表（學習單最後）" })).toBeNull();
  });

  it("a deleted table keeps its rows and 恢復字詞表 asks the server to bring it back", async () => {
    const before = withExtras();
    const restored = { ...before, vocabulary: before.vocabulary.map((v) => ({ ...v, level: 3 })) };
    const api = mockApi(
      editorRoutes(worksheet({ content: contentWith(before) }), {
        [`PUT ${WS}`]: (call: MockCall) => worksheet({ version_no: 4, content: (call.body as { content: WorksheetContent }).content }),
        [`POST ${WS}/blocks/b2/restore-word-table`]: () => worksheet({ version_no: 5, content: contentWith(restored) }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    expect(editorBlock().queryByRole("button", { name: "恢復字詞表 B" })).toBeNull();
    await user.click(editorBlock().getByRole("button", { name: "刪除字詞表 B" }));
    await user.click(await editorBlock().findByRole("button", { name: "恢復字詞表 B" }));
    // The deleted state was saved first (rows kept), then the server restored.
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/restore-word-table`)).toHaveLength(1));
    const saved = (api.lastBody("PUT", WS) as { content: WorksheetContent }).content.blocks[1]!;
    expect(saved.vocabulary).toEqual([]);
    expect(saved.vocabulary_removed).toEqual(before.vocabulary);
    const tables = await editorBlock().findAllByRole("table", { name: /^字詞表 B/ });
    // Content and Academic side by side: a header row and one word each.
    expect(tables.map((t) => within(t).getAllByRole("row").length)).toEqual([2, 2]);
    expect(editorBlock().queryByRole("button", { name: "恢復字詞表 B" })).toBeNull();
  });
});

// ---------------------------------------------------------------- 習慣建議

describe("習慣建議: 改這一區之後", () => {
  const habit = (overrides: Partial<MemorySuggestion> = {}): MemorySuggestion => ({
    id: 41,
    kind: "choice_options",
    scope: null,
    statement: "選擇題使用 3 個選項。",
    replaces_id: null,
    replaces_statement: null,
    source: "block_revise",
    prep_session_id: null,
    prep_turn_id: null,
    worksheet_id: 11,
    proposal_id: 90,
    created_at: "2026-11-24T01:00:00Z",
    expires_at: "2026-11-24T01:15:00Z",
    intro: false,
    ...overrides,
  });
  const CANDIDATES = "/api/me/memories/candidates";
  const SAID = "之後選擇題都用三個選項，這一區改成選擇題";

  it("on the worksheet page the question comes right after the request and stays in view", async () => {
    let found: MemorySuggestion[] = [];
    const api = mockApi(
      editorRoutes(worksheet(), {
        [`POST ${WS}/blocks/b2/revise`]: () => {
          found = [habit()];
          return proposal("running", { instruction: SAID, comment_ids: [] });
        },
        [`GET ${CANDIDATES}`]: () => ({ candidates: found }),
        [`POST ${CANDIDATES}/41/shown`]: { ok: true },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("region", { name: "區塊 B" });
    await waitFor(() => expect(api.callsTo("GET", CANDIDATES)).toHaveLength(1));
    await user.click(await editorBlock().findByRole("button", { name: "改這一區（區塊 B）" }));
    const form = within(screen.getByRole("form", { name: "請 Manna 改區塊 B" }));
    await user.type(form.getByRole("textbox", { name: "想怎麼改區塊 B？" }), SAID);
    await user.click(form.getByRole("button", { name: "請 Manna 改這一區" }));
    // The worksheet's version did not change (the proposal waits for 採用), yet it is asked.
    const ask = await screen.findByText(/要記住這個習慣嗎？/);
    expect(api.callsTo("GET", CANDIDATES).length).toBeGreaterThanOrEqual(2);
    expect(ask.closest(".memory-suggestions")).toHaveClass("is-floating");
  });

  it("in a 備課對話 it is asked in the bottom bar, only for this conversation's worksheets", async () => {
    let found: MemorySuggestion[] = [];
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/sheet`]: sheetRoute(),
      [`GET ${WS}/comments`]: [],
      [`GET ${WS}/proposals`]: [],
      [`POST ${WS}/blocks/b2/revise`]: () => {
        found = [habit(), habit({ id: 42, worksheet_id: 99, statement: "別的學習單的習慣。" })];
        return proposal("running", { instruction: SAID, comment_ids: [] });
      },
      [`GET ${CANDIDATES}`]: () => ({ candidates: found }),
      [`POST ${CANDIDATES}/41/shown`]: { ok: true },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "改這一區（區塊 B）" }));
    await user.type(screen.getByRole("textbox", { name: "給 Manna 的話" }), `${SAID}{Enter}`);
    const ask = await screen.findByText(/要記住這個習慣嗎？/);
    expect(ask.closest("form")).toHaveClass("prep-bottom");
    expect(screen.getByText("選擇題使用 3 個選項。")).toBeInTheDocument();
    expect(screen.queryByText("別的學習單的習慣。")).not.toBeInTheDocument();
  });
});
