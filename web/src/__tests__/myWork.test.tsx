// 我的作品 (/prep): one list of prep sessions and standalone worksheets, filter chips kept in the
// URL, delete of a standalone worksheet, the empty state, and /worksheets leading to it.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
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
  session({ id: 7, class: { id: 3, name: "高一 3 班" }, updated_at: "2026-11-15T09:00:00Z" }),
  session({ id: 8, title: "暖身遊戲", last_kind: "activity", updated_at: "2026-11-12T09:00:00Z" }),
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

const titles = () =>
  within(screen.getByRole("list", { name: "我的作品" }))
    .getAllByRole("heading", { level: 3 })
    .map((h) => h.textContent);

describe("我的作品", () => {
  it("lists sessions and standalone worksheets together, newest first, without repeating session worksheets", async () => {
    signIn();
    mockApi(routes());
    renderApp("/prep");
    expect(await screen.findByRole("heading", { name: "我的作品", level: 1 })).toBeInTheDocument();
    await screen.findByRole("list", { name: "我的作品" });
    expect(titles()).toEqual(["最新的獨立學習單", "The Umbrella Stand", "暖身遊戲", "獨立的學習單", "環境保護"]);
    const list = within(screen.getByRole("list", { name: "我的作品" }));
    expect(list.getByRole("link", { name: "The Umbrella Stand" })).toHaveAttribute("href", "/prep/7");
    expect(list.getByRole("link", { name: "獨立的學習單" })).toHaveAttribute("href", "/worksheets/21");
    expect(list.queryByText("屬於備課 7 的學習單")).not.toBeInTheDocument();
    expect(list.getByText(/教材：The Umbrella Stand · 高一 3 班 · 11\/15/)).toBeInTheDocument();
    expect(list.getByText("問 Manna")).toBeInTheDocument();
    expect(list.getByText("課堂活動")).toBeInTheDocument();
    expect(list.getByText(/還沒產生/)).toBeInTheDocument();
    // One primary action.
    expect(screen.getByRole("link", { name: "新的備課" })).toHaveAttribute("href", "/");
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

  it("opens straight on the filter in the URL", async () => {
    signIn();
    mockApi(routes());
    renderApp("/prep?type=activity");
    await screen.findByRole("list", { name: "我的作品" });
    expect(titles()).toEqual(["暖身遊戲"]);
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
    const row = screen.getByRole("link", { name: "獨立的學習單" }).closest("li") as HTMLElement;
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
