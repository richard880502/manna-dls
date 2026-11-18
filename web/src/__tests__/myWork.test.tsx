// 我的作品 (/prep): prep sessions and standalone worksheets as cards (or the list), filter chips
// kept in the URL, the picture or text preview of each card, the 卡片／清單 switch and its memory,
// delete of a standalone worksheet in both views, the empty state, and /worksheets leading to it.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { PrepSessionSummary, WorksheetSummary } from "../api/types";
import { baseRoutes, mockApi, renderApp, respond, signIn } from "../test/mockApi";

const SESSIONS = "GET /api/prep-sessions";
const WORKSHEETS = "GET /api/worksheets";

const session = (overrides: Partial<PrepSessionSummary> = {}): PrepSessionSummary => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: 1,
  last_kind: "worksheet",
  preview_worksheet: null,
  preview_text: null,
  updated_at: "2026-11-15T09:00:00Z",
  created_at: "2026-11-15T08:00:00Z",
  ...overrides,
});

const sheet = (overrides: Partial<WorksheetSummary> = {}): WorksheetSummary => ({
  id: 21,
  title: "獨立的學習單",
  filename: "x",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: 1,
  created_at: "2026-11-10T08:00:00Z",
  updated_at: "2026-11-10T09:00:00Z",
  prep_session_id: null,
  ...overrides,
});

const sessions = [
  session({
    id: 7,
    class: { id: 3, name: "高一 3 班" },
    preview_worksheet: { id: 22, version_no: 2 },
    preview_text: "The Umbrella Stand",
    updated_at: "2026-11-15T09:00:00Z",
  }),
  // The material is not the title: it is said.
  session({
    id: 8,
    title: "暖身遊戲",
    last_kind: "activity",
    preview_text: "暖身遊戲\n1. 看圖猜字\n2. 兩人一組",
    updated_at: "2026-11-12T09:00:00Z",
  }),
  session({ id: 9, title: "環境保護", material: null, topic: "環境保護", last_kind: "ask", updated_at: "2026-11-01T09:00:00Z" }),
];
const worksheets = [
  sheet(),
  // Made in session 7: its row is the session, not a second one.
  sheet({ id: 22, title: "屬於備課 7 的學習單", prep_session_id: 7, updated_at: "2026-11-16T09:00:00Z" }),
  sheet({ id: 23, title: "最新的獨立學習單", updated_at: "2026-11-17T09:00:00Z", generated: false, levels: [] }),
];

const routes = (extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  [SESSIONS]: { sessions },
  [WORKSHEETS]: worksheets,
  ...extra,
});

/**
 * The preview pictures: the worksheets in `ok` answer a small JPEG (`blob:thumb-<id>`), any other
 * is still being made (409). Each request is still recorded by the mock API.
 */
function stubThumbnails(ok: number[]) {
  const mocked = globalThis.fetch;
  const urls = new Map<Blob, string>();
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const id = /\/api\/worksheets\/(\d+)\/thumbnail$/.exec(String(input))?.[1];
    if (id === undefined) return mocked(input, init);
    await mocked(input, init);
    if (!ok.includes(Number(id))) {
      return {
        ok: false,
        status: 409,
        text: async () => JSON.stringify({ error: { code: "conflict", message: "這份學習單還在產生中。" } }),
      } as unknown as Response;
    }
    const blob = new Blob(["jpeg"], { type: "image/jpeg" });
    urls.set(blob, `blob:thumb-${id}`);
    return { ok: true, status: 200, blob: async () => blob, headers: { get: () => null } } as unknown as Response;
  });
  vi.stubGlobal(
    "URL",
    Object.assign(URL, { createObjectURL: (b: Blob) => urls.get(b) ?? "blob:unknown", revokeObjectURL: vi.fn() }),
  );
}

const list = () => within(screen.getByRole("list", { name: "我的作品" }));
const card = (title: string) => list().getByRole("link", { name: title }).closest("li") as HTMLElement;
const showList = (user: ReturnType<typeof userEvent.setup>) =>
  user.click(within(screen.getByRole("group", { name: "檢視方式" })).getByRole("button", { name: "清單" }));

const titles = () =>
  within(screen.getByRole("list", { name: "我的作品" }))
    .getAllByRole("heading", { level: 3 })
    .map((h) => h.textContent);

describe("我的作品", () => {
  it("shows sessions and standalone worksheets together as cards, newest first, without repeating session worksheets", async () => {
    signIn();
    mockApi(routes());
    renderApp("/prep");
    expect(await screen.findByRole("heading", { name: "我的作品", level: 1 })).toBeInTheDocument();
    await screen.findByRole("list", { name: "我的作品" });
    expect(titles()).toEqual(["最新的獨立學習單", "The Umbrella Stand", "暖身遊戲", "獨立的學習單", "環境保護"]);
    // Each card is one link, named by the title alone.
    expect(list().getAllByRole("link")).toHaveLength(5);
    expect(list().getByRole("link", { name: "The Umbrella Stand" })).toHaveAttribute("href", "/prep/7");
    expect(list().getByRole("link", { name: "獨立的學習單" })).toHaveAttribute("href", "/worksheets/21");
    expect(list().getByRole("link", { name: "最新的獨立學習單" })).toHaveAttribute("href", "/worksheets/23");
    expect(list().queryByText("屬於備課 7 的學習單")).not.toBeInTheDocument();
    // The material is said only when it is not the title again; the class and the date are.
    const first = within(card("The Umbrella Stand"));
    expect(first.getByText(/^\d+\/15 \d+:\d+ · 高一 3 班$/)).toBeInTheDocument();
    expect(first.queryByText(/教材：/)).not.toBeInTheDocument();
    expect(first.getByText("完整學習單")).toBeInTheDocument();
    expect(within(card("暖身遊戲")).getByText(/· 教材：The Umbrella Stand$/)).toBeInTheDocument();
    expect(within(card("環境保護")).queryByText(/主題：/)).not.toBeInTheDocument();
    expect(within(card("環境保護")).getByText("問 Manna")).toBeInTheDocument();
    expect(within(card("暖身遊戲")).getByText("課堂活動")).toBeInTheDocument();
    expect(within(card("最新的獨立學習單")).getByText(/還沒產生/)).toBeInTheDocument();
    // One primary action.
    expect(screen.getByRole("link", { name: "新的備課" })).toHaveAttribute("href", "/");
  });

  it("gives a card a page picture when it has a worksheet, and its first lines when it has none", async () => {
    signIn();
    const api = mockApi(routes());
    stubThumbnails([22, 21]);
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });

    // A session with a worksheet: the student version's first page, asked for with the token.
    const img = await within(card("The Umbrella Stand")).findByRole("img", { name: "The Umbrella Stand 學生版第一頁" });
    await waitFor(() => expect(img).toHaveAttribute("src", "blob:thumb-22"));
    expect(img).toHaveAttribute("loading", "lazy");
    expect(img).toHaveAttribute("decoding", "async");
    expect(img).toHaveAttribute("width");
    expect(img).toHaveAttribute("height");
    expect(api.callsTo("GET", "/api/worksheets/22/thumbnail")[0]?.headers.Authorization).toBe("Bearer test-token");
    // A standalone worksheet that was generated: its own first page.
    const own = await within(card("獨立的學習單")).findByRole("img", { name: "獨立的學習單 學生版第一頁" });
    await waitFor(() => expect(own).toHaveAttribute("src", "blob:thumb-21"));
    // An activity: its plan's first lines, no picture, nothing asked for.
    const activity = within(card("暖身遊戲"));
    expect(activity.getByText(/1\. 看圖猜字/)).toBeInTheDocument();
    expect(activity.queryByRole("img")).not.toBeInTheDocument();
    // A worksheet that was never generated has no page to ask for.
    expect(api.callsTo("GET", "/api/worksheets/23/thumbnail")).toHaveLength(0);
    expect(within(card("最新的獨立學習單")).queryByRole("img")).not.toBeInTheDocument();
    // The text is decoration: the link is still named by the title alone.
    expect(list().getByRole("link", { name: "暖身遊戲" })).toBeInTheDocument();
  });

  it("falls back to the text when the picture is still being made, cannot be had, or fails to load", async () => {
    signIn();
    const api = mockApi(routes());
    stubThumbnails([21]);
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    // Session 7's worksheet answers 409 (still being made): its first lines instead, asked once.
    await waitFor(() => expect(api.callsTo("GET", "/api/worksheets/22/thumbnail")).toHaveLength(1));
    await waitFor(() =>
      expect(within(card("The Umbrella Stand")).getByText("The Umbrella Stand", { selector: "p" })).toBeInTheDocument(),
    );
    expect(within(card("The Umbrella Stand")).queryByRole("img")).not.toBeInTheDocument();
    // A picture that loads but cannot be shown: the title as the first line.
    const img = await within(card("獨立的學習單")).findByRole("img");
    fireEvent.error(img);
    await waitFor(() => expect(within(card("獨立的學習單")).queryByRole("img")).not.toBeInTheDocument());
    expect(within(card("獨立的學習單")).getByText("獨立的學習單", { selector: "p" })).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/worksheets/22/thumbnail")).toHaveLength(1);
  });

  it("switches between cards and the list, keeps the choice, and keeps the list as it was", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const first = renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    const switcher = within(screen.getByRole("group", { name: "檢視方式" }));
    expect(switcher.getAllByRole("button").map((b) => b.textContent)).toEqual(["卡片", "清單"]);
    expect(switcher.getByRole("button", { name: "卡片" })).toHaveAttribute("aria-pressed", "true");
    expect(switcher.getByRole("button", { name: "清單" })).toHaveAttribute("aria-pressed", "false");
    expect(document.querySelector(".work-grid")).not.toBeNull();

    await showList(user);
    expect(switcher.getByRole("button", { name: "清單" })).toHaveAttribute("aria-pressed", "true");
    expect(document.querySelector(".work-grid")).toBeNull();
    expect(document.querySelector("ul.rows")).not.toBeNull();
    expect(titles()).toEqual(["最新的獨立學習單", "The Umbrella Stand", "暖身遊戲", "獨立的學習單", "環境保護"]);
    // The list says the material only when it is not the title; the date closes the line.
    expect(list().getByText(/^高一 3 班 · \d+\/15 \d+:\d+$/)).toBeInTheDocument();
    expect(list().getByText(/^教材：The Umbrella Stand · \d+\/12 /)).toBeInTheDocument();
    expect(list().queryByText(/教材：The Umbrella Stand · 高一/)).not.toBeInTheDocument();
    expect(list().getByText(/還沒產生/)).toBeInTheDocument();
    expect(window.localStorage.getItem("manna.mywork.view")).toBe("list");

    // Remembered on the next visit.
    first.unmount();
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    expect(document.querySelector("ul.rows")).not.toBeNull();
    await user.click(within(screen.getByRole("group", { name: "檢視方式" })).getByRole("button", { name: "卡片" }));
    expect(document.querySelector(".work-grid")).not.toBeNull();
    expect(window.localStorage.getItem("manna.mywork.view")).toBe("cards");
  });

  it("still works when the browser will not keep the choice", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const get = Storage.prototype.getItem;
    const put = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(function (this: Storage, key: string) {
      if (key === "manna.mywork.view") throw new Error("blocked");
      return get.call(this, key);
    });
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (this: Storage, key: string, value: string) {
      if (key === "manna.mywork.view") throw new Error("blocked");
      put.call(this, key, value);
    });
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    expect(document.querySelector(".work-grid")).not.toBeNull();
    await showList(user);
    expect(set).toHaveBeenCalled();
    expect(document.querySelector("ul.rows")).not.toBeNull();
    vi.restoreAllMocks();
  });

  it("filters with chips, keeps the choice in the URL, and follows back and forward", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    const chips = within(screen.getByRole("group", { name: "作品類型" }));
    expect(chips.getAllByRole("button").map((b) => b.textContent)).toEqual(["全部 5", "學習單 3", "課堂活動 1", "問 Manna 1"]);
    expect(chips.getByRole("button", { name: /^全部/ })).toHaveAttribute("aria-pressed", "true");

    await user.click(chips.getByRole("button", { name: /^學習單/ }));
    expect(router.state.location.search).toBe("?type=worksheet");
    expect(titles()).toEqual(["最新的獨立學習單", "The Umbrella Stand", "獨立的學習單"]);
    await user.click(chips.getByRole("button", { name: /^問 Manna/ }));
    expect(titles()).toEqual(["環境保護"]);

    // Outside act, React commits the re-render after router.state changes: wrap it so the list is read after.
    await act(() => router.navigate(-1));
    expect(router.state.location.search).toBe("?type=worksheet");
    expect(titles()).toHaveLength(3);
    expect(chips.getByRole("button", { name: /^學習單/ })).toHaveAttribute("aria-pressed", "true");

    await user.click(chips.getByRole("button", { name: /^全部/ }));
    expect(router.state.location.search).toBe("");
    expect(titles()).toHaveLength(5);
  });

  it("filters the list view the same way", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    window.localStorage.setItem("manna.mywork.view", "list");
    const { router } = renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    expect(document.querySelector("ul.rows")).not.toBeNull();
    await user.click(within(screen.getByRole("group", { name: "作品類型" })).getByRole("button", { name: /^課堂活動/ }));
    expect(router.state.location.search).toBe("?type=activity");
    expect(titles()).toEqual(["暖身遊戲"]);
    expect(document.querySelector("ul.rows")).not.toBeNull();
  });

  it("opens straight on the filter in the URL", async () => {
    signIn();
    mockApi(routes());
    renderApp("/prep?type=activity");
    await screen.findByRole("list", { name: "我的作品" });
    expect(titles()).toEqual(["暖身遊戲"]);
  });

  it("shows 刪除 on a standalone worksheet's card (outside its link) and on no session's", async () => {
    signIn();
    mockApi(routes());
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    const own = card("獨立的學習單");
    const del = within(own).getByRole("button", { name: "刪除 獨立的學習單" });
    expect(del).toBeVisible();
    expect(own.querySelector("a")?.contains(del)).toBe(false);
    expect(within(card("The Umbrella Stand")).queryByRole("button")).not.toBeInTheDocument();
  });

  it("deletes a standalone worksheet after a confirmation, and offers no delete on sessions", async () => {
    signIn();
    const api = mockApi(routes({ "DELETE /api/worksheets/21": { deleted: true } }));
    const user = userEvent.setup();
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    expect(screen.getAllByRole("button", { name: /^刪除/ })).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "刪除 獨立的學習單" }));
    expect(api.callsTo("DELETE", "/api/worksheets/21")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "不刪" }));
    expect(screen.getByRole("button", { name: "刪除 獨立的學習單" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "刪除 獨立的學習單" }));
    await user.click(screen.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(screen.queryByText("獨立的學習單")).not.toBeInTheDocument());
    expect(api.callsTo("DELETE", "/api/worksheets/21")).toHaveLength(1);
    expect(titles()).toHaveLength(4);
  });

  it("deletes from the list view too, and says a failure in the row", async () => {
    signIn();
    const api = mockApi(
      routes({
        "DELETE /api/worksheets/21": respond(500, { error: { message: "刪不掉" } }),
        "DELETE /api/worksheets/23": { deleted: true },
      }),
    );
    const user = userEvent.setup();
    window.localStorage.setItem("manna.mywork.view", "list");
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    expect(document.querySelector("ul.rows")).not.toBeNull();
    expect(screen.getAllByRole("button", { name: /^刪除/ })).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "刪除 獨立的學習單" }));
    await user.click(screen.getByRole("button", { name: "確定刪除" }));
    const row = screen.getByRole("link", { name: "獨立的學習單" }).closest("li") as HTMLElement;
    expect(await within(row).findByRole("alert")).toHaveTextContent("刪不掉");
    await user.click(within(row).getByRole("button", { name: "不刪" }));

    await user.click(screen.getByRole("button", { name: "刪除 最新的獨立學習單" }));
    await user.click(screen.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(screen.queryByText("最新的獨立學習單")).not.toBeInTheDocument());
    expect(api.callsTo("DELETE", "/api/worksheets/23")).toHaveLength(1);
    expect(titles()).toHaveLength(4);
  });

  it("moves focus through the delete confirmation and says a failed delete in the row", async () => {
    signIn();
    mockApi(routes({ "DELETE /api/worksheets/21": respond(500, { error: { message: "刪不掉" } }) }));
    const user = userEvent.setup();
    renderApp("/prep");
    await screen.findByRole("list", { name: "我的作品" });
    await user.click(screen.getByRole("button", { name: "刪除 獨立的學習單" }));
    expect(screen.getByRole("button", { name: "不刪" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "不刪" }));
    expect(screen.getByRole("button", { name: "刪除 獨立的學習單" })).toHaveFocus();

    await user.click(screen.getByRole("button", { name: "刪除 獨立的學習單" }));
    await user.click(screen.getByRole("button", { name: "確定刪除" }));
    const row = card("獨立的學習單");
    expect(await within(row).findByRole("alert")).toHaveTextContent("刪不掉");
    expect(titles()).toHaveLength(5);
  });

  it("says so when there is nothing yet, with a way to the home page", async () => {
    signIn();
    mockApi({ ...baseRoutes(), [SESSIONS]: { sessions: [] }, [WORKSHEETS]: [] });
    renderApp("/prep");
    expect(await screen.findByText(/還沒有作品。/)).toBeInTheDocument();
    expect(within(screen.getByRole("main")).getByRole("link", { name: "首頁" })).toHaveAttribute("href", "/");
    expect(screen.queryByRole("group", { name: "作品類型" })).not.toBeInTheDocument();
  });

  it("shows the server's error", async () => {
    signIn();
    mockApi({ ...baseRoutes(), [SESSIONS]: respond(500, { error: { message: "伺服器出錯了" } }), [WORKSHEETS]: [] });
    renderApp("/prep");
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  it("/worksheets leads here", async () => {
    signIn();
    mockApi(routes());
    const { router } = renderApp("/worksheets");
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep"));
    expect(await screen.findByRole("heading", { name: "我的作品", level: 1 })).toBeInTheDocument();
  });
});
