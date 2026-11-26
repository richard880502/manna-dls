// 社群 (docs/COMMUNITY.md): the nav entry only while the PI's switch is on, the three tabs in
// ?tab=, the explore tiles with search and sort, save and remix, the read-only preview overlay,
// 我的分享 with its 不公開／已分享 switch and the sharing dialog, the admin card, and 「改編自」.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type {
  CommunityCard,
  CommunityMine,
  CommunityPreview,
  CommunityShareState,
  WorksheetState,
  WsSheet,
} from "../api/types";
import { emptyBrief, emptySettings } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, teacher } from "../test/mockApi";

const nav = () => within(screen.getByRole("navigation", { name: "主要導覽" }));

const sheet = (title = "The Umbrella Stand"): WsSheet => ({
  title,
  level: "standard",
  audience: "student",
  version_code: null,
  objectives: [],
  blocks: [
    {
      id: "b1",
      kind: "activity",
      label: "A",
      title: "Paragraph 1: Find",
      instructions: "Read paragraph 1.",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ no: 1, answer_format: "簡答題", prompt: "Who put the stand outside?", options: [], lines: 2 }],
    },
  ],
});

const card = (overrides: Partial<CommunityCard> = {}): CommunityCard => ({
  id: 11,
  title: "雨傘架的故事",
  owner_name: "T-2026-007",
  shared_at: "2026-11-20T08:00:00Z",
  saves: 2,
  remixes: 1,
  is_mine: false,
  saved: false,
  allow_remix: true,
  output: "worksheet",
  levels: ["standard"],
  cover: sheet(),
  ...overrides,
});

const page = (items: CommunityCard[]) => ({ items, total: items.length, limit: 24, offset: 0 });

const mine = (overrides: Partial<CommunityMine> = {}): CommunityMine => ({
  id: 31,
  title: "我的學習單",
  updated_at: "2026-11-21T08:00:00Z",
  shared: false,
  shared_at: null,
  name: null,
  allow_remix: true,
  saves: 0,
  remixes: 0,
  output: "worksheet",
  levels: ["standard"],
  cover: sheet("我的學習單"),
  ...overrides,
});

const preview = (c: CommunityCard): CommunityPreview => {
  const { cover: _cover, saved_at: _saved, ...meta } = c;
  return { ...meta, level: "standard", sheet: sheet(c.title) };
};

const on = (extra: Record<string, unknown> = {}) => ({
  ...baseRoutes({ community_enabled: true }),
  "GET /api/community/worksheets": page([card()]),
  "GET /api/community/saves": page([]),
  "GET /api/community/mine": { items: [mine()], total: 1, limit: 24, offset: 0 },
  ...extra,
});

describe("社群: the PI's switch", () => {
  it("shows no nav entry and sends /community home while the switch is off", async () => {
    signIn();
    const api = mockApi({ ...baseRoutes(), "GET /api/community/worksheets": page([card()]) });
    const { router } = renderApp("/community");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    expect(router.state.location.pathname).toBe("/");
    expect(nav().queryByRole("link", { name: "社群" })).toBeNull();
    expect(screen.queryByText(/社群/)).toBeNull();
    // Nothing about the community was even asked for.
    expect(api.calls.filter((c) => c.path.startsWith("/api/community"))).toEqual([]);
  });

  it("shows 社群 in the sidebar while the switch is on", async () => {
    signIn();
    mockApi(on());
    renderApp("/");
    await screen.findByRole("navigation", { name: "主要導覽" });
    expect(await nav().findByRole("link", { name: "社群" })).toHaveAttribute("href", "/community");
  });
});

describe("社群: tabs and explore", () => {
  it("opens on 探索 and keeps the tab in ?tab=", async () => {
    signIn();
    mockApi(on());
    const user = userEvent.setup();
    const { router } = renderApp("/community");
    expect(await screen.findByRole("heading", { name: "社群", level: 1 })).toBeInTheDocument();
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["探索", "我的收藏", "我的分享"]);
    expect(screen.getByRole("tab", { name: "探索" })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("tab", { name: "我的收藏" }));
    expect(router.state.location.search).toBe("?tab=saved");
    await user.click(screen.getByRole("tab", { name: "我的分享" }));
    expect(router.state.location.search).toBe("?tab=mine");
    expect(await screen.findByRole("list", { name: "我的學習單" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "探索" }));
    expect(router.state.location.search).toBe("");
  });

  it("draws a tile with title, owner and counts, and searches and sorts by asking the server", async () => {
    signIn();
    const api = mockApi(on());
    const user = userEvent.setup();
    renderApp("/community");
    const grid = within(await screen.findByRole("list", { name: "分享的學習單" }));
    expect(grid.getByRole("heading", { name: "雨傘架的故事" })).toBeInTheDocument();
    expect(grid.getByText("T-2026-007")).toBeInTheDocument();
    expect(grid.getByLabelText("2 次收藏")).toBeInTheDocument();
    expect(grid.getByLabelText("1 次改編")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("排序"), "remixed");
    await waitFor(() => expect(api.calls.at(-1)?.url).toContain("sort=remixed"));
    await user.type(screen.getByRole("searchbox"), "雨傘{Enter}");
    await waitFor(() => expect(api.calls.at(-1)?.url).toContain("q=%E9%9B%A8%E5%82%98"));
    await user.selectOptions(screen.getByLabelText("類型"), "tiered");
    await waitFor(() => expect(api.calls.at(-1)?.url).toContain("output=tiered"));
  });

  it("says so when nothing matches", async () => {
    signIn();
    mockApi(on({ "GET /api/community/worksheets": page([]) }));
    renderApp("/community");
    expect(await screen.findByText(/還沒有人分享學習單/)).toBeInTheDocument();
  });

  it("toggles 收藏 on a tile and tells the server", async () => {
    signIn();
    const api = mockApi(
      on({
        "POST /api/community/worksheets/11/save": { id: 11, saved: true },
        "DELETE /api/community/worksheets/11/save": { id: 11, saved: false },
      }),
    );
    const user = userEvent.setup();
    renderApp("/community");
    const grid = within(await screen.findByRole("list", { name: "分享的學習單" }));
    await user.click(grid.getByRole("button", { name: "收藏" }));
    const saved = await grid.findByRole("button", { name: "收藏", pressed: true });
    expect(grid.getByLabelText("3 次收藏")).toBeInTheDocument();
    await user.click(saved);
    expect(await grid.findByRole("button", { name: "收藏", pressed: false })).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/community/worksheets/11/save")).toHaveLength(1);
    expect(api.callsTo("DELETE", "/api/community/worksheets/11/save")).toHaveLength(1);
  });

  it("shows 收藏 at once and takes it back, with the error, when the server refuses", async () => {
    signIn();
    mockApi(
      on({
        "POST /api/community/worksheets/11/save": respond(500, { error: { code: "boom", message: "收藏失敗了。" } }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/community");
    const grid = within(await screen.findByRole("list", { name: "分享的學習單" }));
    await user.click(grid.getByRole("button", { name: "收藏" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("收藏失敗了。");
    expect(grid.getByRole("button", { name: "收藏", pressed: false })).toBeInTheDocument();
    expect(grid.getByLabelText("2 次收藏")).toBeInTheDocument();
  });

  it("remixes into a private copy and goes to it", async () => {
    signIn();
    const api = mockApi(on({ "POST /api/community/worksheets/11/remix": { worksheet_id: 90 } }));
    const user = userEvent.setup();
    const { router } = renderApp("/community");
    const grid = within(await screen.findByRole("list", { name: "分享的學習單" }));
    await user.click(grid.getByRole("button", { name: "拿來改" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/worksheets/90"));
    expect(api.callsTo("POST", "/api/community/worksheets/11/remix")).toHaveLength(1);
  });

  it("offers no 拿來改 on a worksheet that is not open to remixing, nor on one's own", async () => {
    signIn();
    mockApi(
      on({
        "GET /api/community/worksheets": page([
          card({ id: 12, title: "不開放改編", allow_remix: false }),
          card({ id: 13, title: "我自己的", is_mine: true }),
        ]),
      }),
    );
    renderApp("/community");
    const grid = within(await screen.findByRole("list", { name: "分享的學習單" }));
    expect(await grid.findByRole("heading", { name: "不開放改編" })).toBeInTheDocument();
    expect(grid.queryByRole("button", { name: "拿來改" })).toBeNull();
    expect(grid.queryByRole("button", { name: "收藏" })).not.toBeNull();
    expect(grid.getAllByRole("button", { name: "收藏" })).toHaveLength(1);
  });

  it("lists what was saved, and says how many are no longer available", async () => {
    signIn();
    mockApi(on({ "GET /api/community/saves": { ...page([card({ saved: true })]), unavailable: 2 } }));
    renderApp("/community?tab=saved");
    expect(await screen.findByRole("heading", { name: "雨傘架的故事" })).toBeInTheDocument();
    expect(screen.getByText(/有 2 份收藏的學習單已經取消分享/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "收藏", pressed: true })).toBeInTheDocument();
  });
});

describe("社群: the preview", () => {
  it("opens the read-only sheet, keeps focus inside and closes with Esc and ×", async () => {
    signIn();
    const c = card();
    const api = mockApi(on({ "GET /api/community/worksheets/11": preview(c) }));
    const user = userEvent.setup();
    renderApp("/community");
    const opener = await screen.findByRole("button", { name: "預覽：雨傘架的故事" });
    await user.click(opener);
    const dialog = await screen.findByRole("dialog", { name: "雨傘架的故事" });
    expect(await within(dialog).findByText("Who put the stand outside?")).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/community/worksheets/11")).toHaveLength(1);
    // Read-only: none of the editing or copying tools of a block.
    expect(within(dialog).queryByRole("button", { name: /複製/ })).toBeNull();
    expect(within(dialog).getByText("T-2026-007")).toBeInTheDocument();
    // The focus trap: Tab stays in the dialog.
    for (let i = 0; i < 8; i++) await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();

    await user.click(opener);
    await user.click(await screen.findByRole("button", { name: "關閉" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("saves and remixes from the preview, and says when the owner did not allow remixing", async () => {
    signIn();
    const c = card({ allow_remix: false });
    mockApi(
      on({
        "GET /api/community/worksheets": page([c]),
        "GET /api/community/worksheets/11": preview(c),
        "POST /api/community/worksheets/11/save": { id: 11, saved: true },
      }),
    );
    const user = userEvent.setup();
    renderApp("/community");
    await user.click(await screen.findByRole("button", { name: "預覽：雨傘架的故事" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("這份學習單的老師沒有開放改編。")).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "拿來改" })).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "收藏" }));
    expect(await within(dialog).findByRole("button", { name: "收藏", pressed: true })).toBeInTheDocument();
  });

  it("shows the error when the worksheet was withdrawn meanwhile", async () => {
    signIn();
    mockApi(
      on({
        "GET /api/community/worksheets/11": respond(404, {
          error: { code: "not_found", message: "找不到這份分享的學習單，它可能已經取消分享。" },
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/community");
    await user.click(await screen.findByRole("button", { name: "預覽：雨傘架的故事" }));
    expect(await screen.findByText(/可能已經取消分享/)).toBeInTheDocument();
  });
});

describe("社群: 我的分享", () => {
  const shareState = (over: Partial<CommunityShareState> = {}): CommunityShareState => ({
    id: 31,
    shared: true,
    shared_at: "2026-11-24T08:00:00Z",
    name: "anonymous",
    allow_remix: true,
    ...over,
  });

  it("starts private; the switch opens a dialog that says what is shown before anything is shared", async () => {
    signIn();
    const api = mockApi(on({ "PUT /api/worksheets/31/share": shareState() }));
    const user = userEvent.setup();
    renderApp("/community?tab=mine");
    const toggle = await screen.findByRole("switch", { name: "分享：我的學習單" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(within(toggle).getByText("不公開")).toBeInTheDocument();

    await user.click(toggle);
    const dialog = await screen.findByRole("dialog", { name: "分享這份學習單" });
    expect(api.callsTo("PUT", "/api/worksheets/31/share")).toHaveLength(0);
    expect(within(dialog).getByText("其他老師看得到")).toBeInTheDocument();
    expect(within(dialog).getByText("其他老師看不到")).toBeInTheDocument();
    expect(within(dialog).getByText(/隨時都可以撤回/)).toBeInTheDocument();
    expect(within(dialog).getByRole("radio", { name: /代號：顯示你的研究代號（T-2026-001）/ })).toBeInTheDocument();
    expect(within(dialog).getByRole("radio", { name: /匿名：顯示「一位老師」/ })).toBeChecked();
    expect(within(dialog).getByRole("checkbox", { name: "允許其他老師改編" })).toBeChecked();

    await user.click(within(dialog).getByRole("radio", { name: /代號/ }));
    await user.click(within(dialog).getByRole("checkbox", { name: "允許其他老師改編" }));
    await user.click(within(dialog).getByRole("button", { name: "分享" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(api.lastBody("PUT", "/api/worksheets/31/share")).toEqual({
      shared: true,
      name: "code",
      allow_remix: false,
    });
    expect(screen.getByRole("switch", { name: "分享：我的學習單" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("已分享")).toBeInTheDocument();
  });

  it("cancelling the dialog shares nothing", async () => {
    signIn();
    const api = mockApi(on({ "PUT /api/worksheets/31/share": shareState() }));
    const user = userEvent.setup();
    renderApp("/community?tab=mine");
    await user.click(await screen.findByRole("switch", { name: "分享：我的學習單" }));
    await user.click(await screen.findByRole("button", { name: "取消" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(api.callsTo("PUT", "/api/worksheets/31/share")).toHaveLength(0);
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  });

  it("withdraws at once with the switch, without a confirmation", async () => {
    signIn();
    const api = mockApi(
      on({
        "GET /api/community/mine": {
          items: [mine({ shared: true, name: "code", saves: 3, remixes: 2 })],
          total: 1,
          limit: 24,
          offset: 0,
        },
        "PUT /api/worksheets/31/share": shareState({ shared: false, shared_at: null, name: null }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/community?tab=mine");
    const toggle = await screen.findByRole("switch", { name: "分享：我的學習單" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(screen.getByLabelText("3 次收藏")).toBeInTheDocument();
    await user.click(toggle);
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(api.lastBody("PUT", "/api/worksheets/31/share")).toEqual({ shared: false });
  });

  it("changes the options of a shared worksheet in the same dialog", async () => {
    signIn();
    const api = mockApi(
      on({
        "GET /api/community/mine": {
          items: [mine({ shared: true, name: "code", allow_remix: true })],
          total: 1,
          limit: 24,
          offset: 0,
        },
        "PUT /api/worksheets/31/share": shareState({ name: "anonymous" }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/community?tab=mine");
    await user.click(await screen.findByRole("button", { name: "分享設定" }));
    const dialog = await screen.findByRole("dialog", { name: "分享設定" });
    expect(within(dialog).getByRole("radio", { name: /代號/ })).toBeChecked();
    await user.click(within(dialog).getByRole("radio", { name: /匿名/ }));
    await user.click(within(dialog).getByRole("button", { name: "儲存設定" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(api.lastBody("PUT", "/api/worksheets/31/share")).toEqual({
      shared: true,
      name: "anonymous",
      allow_remix: true,
    });
  });
});

describe("社群: the admin switch", () => {
  const pi = { ...teacher, role: "pi" as const };

  it("has a card in 模型與研究設定 and a row in 總覽", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      "GET /api/admin/community-settings": { enabled: false },
      "PUT /api/admin/community-settings": (call: { body: unknown }) => ({
        enabled: (call.body as { enabled: boolean }).enabled,
      }),
      "GET /api/admin/ai-model": {
        active: { provider: "fake", model: "fake", effort: "low", timeout_secs: 60 },
        source: "env",
        changed_by: null,
        changed_at: null,
        env_default: { provider: "fake", model: "fake", effort: "low", timeout_secs: 60 },
        providers: [],
        efforts: ["low"],
        timeout_range: { min: 30, max: 900 },
      },
      "GET /api/admin/settings": { route_choice_enabled: true },
      "GET /api/admin/accounts": [],
      "GET /api/admin/prompts": { groups: [], registry: [], try_keys: [], prompts: [] },
    });
    const user = userEvent.setup();
    renderApp("/admin");
    const list = within(await screen.findByRole("list", { name: "目前設定" }));
    const row = (await list.findByRole("heading", { name: "社群" })).closest("li") as HTMLElement;
    expect(await within(row).findByText("關閉")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "模型與研究設定" }));
    const card = await screen.findByRole("region", { name: "社群" });
    const box = within(card).getByRole("checkbox", { name: /開放老師在社群分享/ });
    expect(box).not.toBeChecked();
    await user.click(box);
    await waitFor(() => expect(within(card).getByRole("checkbox")).toBeChecked());
    expect(api.lastBody("PUT", "/api/admin/community-settings")).toEqual({ enabled: true });
    expect(within(card).getByText("開啟")).toBeInTheDocument();
  });
});

describe("社群: 改編自", () => {
  const wsState = (remixed: WorksheetState["remixed_from"]): WorksheetState => ({
    id: 90,
    title: "改過的學習單",
    filename: "x",
    lesson: null,
    class_id: null,
    generated: true,
    levels: ["standard"],
    version_no: 1,
    created_at: "2026-11-24T08:00:00Z",
    updated_at: "2026-11-24T08:00:00Z",
    material: {
      title: "t",
      paragraphs: [{ id: "p1", text: "x" }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-24T08:00:00Z",
    content: { objectives: [], show_objectives: false, levels: ["standard"], chosen: null, notices: [], blocks: [] },
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    remixed_from: remixed,
  });

  it("names the original and how its owner chose to be shown, and only while it is available", async () => {
    signIn();
    mockApi({
      ...baseRoutes({ community_enabled: true }),
      "GET /api/worksheets/90": wsState({ id: 11, title: "雨傘架的故事", owner_name: "一位老師" }),
      "GET /api/worksheets/91": wsState(null),
    });
    const first = renderApp("/worksheets/90");
    expect(await screen.findByText("改編自「雨傘架的故事」（一位老師）")).toBeInTheDocument();
    first.unmount();
    renderApp("/worksheets/91");
    await screen.findByRole("heading", { name: "改過的學習單", level: 1 });
    expect(screen.queryByText(/改編自/)).toBeNull();
  });
});
