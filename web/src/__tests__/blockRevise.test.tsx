// 學習單：逐區修改與註記 (docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §4–§5): in the 備課對話 preview a
// block can be targeted from the bottom bar (「針對 B 區」), text can be selected and noted, notes
// are listed under their block, and Manna's proposal waits for 採用／放棄／再改一次; in the editor
// the same with 改這一區, 加註記 and 註記 on a question, the block held while a proposal waits, and
// a save over a newer version offering 重新載入.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  PrepSession,
  PrepTurn,
  WorksheetComment,
  WorksheetContent,
  WorksheetState,
  WsAudience,
  WsBlock,
  WsItem,
  WsProposal,
  WsSheet,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { blockReviseConfig } from "../lib/blockRevise";
import { prepConfig } from "../lib/prep";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

beforeEach(() => {
  signIn();
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
  blockReviseConfig.pollMs = 20;
  autosaveConfig.delayMs = 20;
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  blockReviseConfig.pollMs = 2000;
  autosaveConfig.delayMs = 1500;
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

/** Selects `quote` inside the element `holder` as a teacher would with the mouse. */
function selectText(holder: Element, quote: string) {
  const walker = document.createTreeWalker(holder, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  while (node && !(node.textContent ?? "").includes(quote)) node = walker.nextNode();
  if (!node) throw new Error(`${quote} not found`);
  const at = (node.textContent ?? "").indexOf(quote);
  act(() => {
    window.getSelection()?.setBaseAndExtent(node, at, node, at + quote.length);
    document.dispatchEvent(new Event("selectionchange"));
  });
}

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
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "改這一區（B 活動）" }));
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
    expect(prepBlock().getByRole("button", { name: "復原上一版" })).toBeInTheDocument();
  });

  it("selected text becomes a note, and 儲存並請 Manna 改 sends it to Manna", async () => {
    const created = note({ id: 42 });
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/sheet`]: sheetRoute(),
      [`GET ${WS}/comments`]: [],
      [`GET ${WS}/proposals`]: [],
      [`POST ${WS}/comments`]: created,
      [`POST ${WS}/blocks/b2/revise`]: proposal("running", { comment_ids: [42] }),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const prompt = await screen.findByText("What did the neighbors expect would happen?");
    selectText(prompt, "neighb");
    expect(await screen.findByText(/已選取第 3 題的「/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "加註記" }));
    const draft = within(screen.getByRole("form", { name: "新增註記" }));
    expect(draft.getByText("第 3 題")).toBeInTheDocument();
    expect(draft.getByRole("checkbox", { name: "提供給 AI" })).toBeChecked();
    await user.type(draft.getByRole("textbox", { name: "註記內容" }), "這題太難，改簡單一點");
    await user.click(draft.getByRole("button", { name: "儲存並請 Manna 改" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/revise`)).toHaveLength(1));
    // The selection snapped to the whole word.
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
    expect(await prepBlock().findByRole("group", { name: "我的註記：第 3 題" })).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "新增註記" })).not.toBeInTheDocument();
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
    expect(editorBlock().getByRole("button", { name: "復原上一版" })).toBeInTheDocument();
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
