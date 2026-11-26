// The app shell (Bloom contract, "Shell and home" and Addendum 1): a left sidebar like Eduaide's —
// 首頁, 我的作品, 最近 (three), 教材與範本 (我的教材, 平台教材, 學習單範本), a folded 更多 — that folds to
// icons on a wide screen and is a drawer behind ☰ on a phone. Every page renders inside it, and
// old addresses keep working.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PrepSessionSummary } from "../api/types";
import {
  baseRoutes,
  classRef,
  emptyLibrary,
  lessonDetail,
  mockApi,
  renderApp,
  schoolClass,
  signIn,
  teacher,
  unitWith,
} from "../test/mockApi";

const HOME = "今天想準備什麼？";

const nav = () => within(screen.getByRole("navigation", { name: "主要導覽" }));

/** The sidebar links marked as where the teacher is ("page", or "true" for a page inside it). */
const currentNav = () =>
  nav()
    .getAllByRole("link")
    .filter((a) => a.hasAttribute("aria-current"))
    .map((a) => a.textContent);

const summary = (id: number, title: string): PrepSessionSummary => ({
  id,
  title,
  material: null,
  topic: null,
  class: null,
  template: null,
  turn_count: 1,
  last_kind: "ask",
  updated_at: "2026-11-15T09:00:00Z",
  created_at: "2026-11-15T08:00:00Z",
});

describe("app shell: the sidebar", () => {
  it("starts with a skip link that moves focus to the page", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.tab();
    const skip = screen.getByRole("link", { name: "跳到主要內容" });
    expect(skip).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("main")).toHaveFocus();
  });

  it("lists 首頁, 我的作品, 教材與範本 and a folded 更多; no 建立 group or tool links; then preferences and sign-out", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    expect(await screen.findByRole("heading", { name: HOME, level: 1 })).toBeInTheDocument();

    const links = nav().getAllByRole("link");
    expect(links.map((a) => [a.textContent, a.getAttribute("href")])).toEqual([
      ["首頁", "/"],
      ["我的作品", "/prep"],
      ["我的教材", "/materials"],
      ["平台教材", "/reference"],
      ["學習單範本", "/templates"],
    ]);
    // The three under a small label 「教材與範本」.
    expect(
      within(nav().getByRole("list", { name: "教材與範本" }))
        .getAllByRole("link")
        .map((a) => a.textContent),
    ).toEqual(["我的教材", "平台教材", "學習單範本"]);
    expect(nav().getByRole("link", { name: "首頁" })).toHaveAttribute("aria-current", "page");
    // Creating starts on the home page: the sidebar has neither the group nor the tools.
    expect(nav().queryByRole("button", { name: "建立" })).not.toBeInTheDocument();
    for (const tool of ["照我的學習單做", "閱讀理解題", "差異化學習單", "課堂活動", "教學套件", "完整學習單", "工作區", "所有學習單", "教材參考"]) {
      expect(nav().queryByRole("link", { name: tool })).not.toBeInTheDocument();
    }

    // 更多 is folded; opening it shows 班級與課例 and 上課紀錄.
    const more = nav().getByRole("button", { name: "更多" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(nav().queryByRole("link", { name: "班級與課例" })).not.toBeInTheDocument();
    await user.click(more);
    expect(more).toHaveAttribute("aria-expanded", "true");
    expect(nav().getByRole("link", { name: "班級與課例" })).toHaveAttribute("href", "/classes");
    expect(nav().getByRole("link", { name: "上課紀錄" })).toHaveAttribute("href", "/record");

    expect(screen.getByRole("link", { name: "備課偏好" })).toHaveAttribute("href", "/preferences");
    expect(screen.getByText("T-2026-001")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "登出" })).toBeInTheDocument();
    // A teacher does not see the admin entries.
    expect(screen.queryByRole("link", { name: "後台管理" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "知識庫" })).not.toBeInTheDocument();

    // Nothing unbuilt is offered, and nothing is addressed to developers.
    for (const gone of [/Reflect/, /Share/, /履歷/, /社群動能地圖/, /Teach & Record/, /Phase/, /本輪平台範圍/]) {
      expect(screen.queryByText(gone)).not.toBeInTheDocument();
    }
  });

  it("gives the project lead the knowledge base and admin pages", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/me": { ...teacher, role: "pi" } });
    renderApp("/");
    expect(await screen.findByRole("link", { name: "後台管理" })).toHaveAttribute("href", "/admin");
    expect(screen.getByRole("link", { name: "知識庫" })).toHaveAttribute("href", "/knowledge");
  });

  it("最近 lists the three latest sessions by title and marks the open one", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions": { sessions: Array.from({ length: 9 }, (_, i) => summary(i + 1, `備課 ${i + 1}`)) },
      "GET /api/prep-sessions/2": { ...summary(2, "備課 2"), turns: [], running: false, next: [] },
    });
    renderApp("/prep/2");
    const recent = within(await screen.findByRole("region", { name: "最近" }));
    expect(recent.getAllByRole("link").map((a) => a.textContent)).toEqual([
      "備課 1",
      "備課 2",
      "備課 3",
    ]);
    expect(recent.getByRole("link", { name: "備課 2" })).toHaveAttribute("aria-current", "page");
    expect(recent.getByRole("link", { name: "備課 1" })).not.toHaveAttribute("aria-current");
    // The session is inside 我的作品.
    expect(nav().getByRole("link", { name: "我的作品" })).toHaveAttribute("aria-current", "true");
  });

  it.each([
    ["/prep", "page"],
    ["/prep/2", "true"],
    ["/worksheets", "true"],
    ["/worksheets/3", "true"],
  ] as const)("%s marks 我的作品 as the current item (%s)", async (path, current) => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      "GET /api/prep-sessions/2": { ...summary(2, "備課 2"), turns: [], running: false, next: [] },
    });
    renderApp(path);
    await screen.findByRole("link", { name: "我的作品" });
    await waitFor(() => expect(currentNav()).toContain("我的作品"));
    expect(nav().getByRole("link", { name: "我的作品" })).toHaveAttribute("aria-current", current);
  });

  it("while creating (/create/*), 首頁 is the current sidebar item", async () => {
    signIn();
    mockApi(baseRoutes());
    renderApp("/create/comprehension");
    await screen.findByRole("heading", { level: 1 });
    expect(currentNav()).toEqual(["首頁"]);
  });

  it("folds to icons with a toggle that says whether it is open, and remembers it", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    const { unmount } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    const toggle = screen.getByRole("button", { name: "側邊欄" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector(".shell")).toHaveClass("is-collapsed");
    unmount();

    mockApi(baseRoutes());
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    expect(screen.getByRole("button", { name: "側邊欄" })).toHaveAttribute("aria-expanded", "false");
  });

  it("on a phone, ☰ opens the drawer; Escape, a tap outside and any navigation close it", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/worksheets": [] });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    const menu = screen.getByRole("button", { name: "選單" });
    expect(menu).toHaveAttribute("aria-expanded", "false");
    expect(menu).toHaveAttribute("aria-controls", "shell-sidebar");

    await user.click(menu);
    expect(menu).toHaveAttribute("aria-expanded", "true");
    expect(document.querySelector(".shell")).toHaveClass("is-drawer-open");
    // Focus moves into the drawer, but Tab is not held there.
    expect(document.getElementById("shell-sidebar")).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    expect(menu).toHaveAttribute("aria-expanded", "false");
    expect(menu).toHaveFocus();

    await user.click(menu);
    await user.click(screen.getByRole("heading", { name: HOME, level: 1 }));
    expect(menu).toHaveAttribute("aria-expanded", "false");

    await user.click(menu);
    await user.click(screen.getByRole("button", { name: "關閉選單" }));
    expect(menu).toHaveAttribute("aria-expanded", "false");

    await user.click(menu);
    await user.click(nav().getByRole("link", { name: "我的作品" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep"));
    expect(menu).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector(".shell")).not.toHaveClass("is-drawer-open");

    // A navigation that does not come from the drawer closes it too.
    await user.click(menu);
    await act(() => router.navigate("/materials"));
    expect(menu).toHaveAttribute("aria-expanded", "false");
  });

  it.each(["/reflect", "/share", "/profile", "/community", "/no-such-page"])("%s is gone: it leads home", async (path) => {
    signIn();
    mockApi(baseRoutes());
    const { router } = renderApp(path);
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(await screen.findByRole("heading", { name: HOME, level: 1 })).toBeInTheDocument();
  });

  it.each([
    ["/units/1", "倒裝句單元"],
    ["/units", "備課庫"],
  ])("%s is inside 班級與課例 (under 更多, which opens)", async (path, heading) => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/units/1/lessons": [] });
    renderApp(path);
    expect(await screen.findByRole("heading", { name: heading, level: 1 })).toBeInTheDocument();
    expect(nav().getByRole("button", { name: "更多" })).toHaveAttribute("aria-expanded", "true");
    expect(currentNav()).toEqual(["班級與課例"]);
  });

  it("a lesson reached from a class marks 班級與課例, not another place", async () => {
    signIn();
    const c = schoolClass();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ classes: [classRef(c)] }),
      "GET /api/lessons/7": lessonDetail(),
      "GET /api/lessons/7/runs": [],
      "GET /api/lessons/7/sop": { material: null, content: null, updated_at: null },
      "GET /api/worksheets": [],
    });
    renderApp("/lessons/7");
    expect(await screen.findByRole("heading", { name: lessonDetail().title, level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["班級與課例"]);
  });

  it("教材與範本: 平台教材, 我的教材 and 學習單範本 each mark their page; a template page is inside 學習單範本", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/materials": emptyLibrary() });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("link", { name: "平台教材" }));
    expect(await screen.findByRole("heading", { name: "平台教材", level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["平台教材"]);

    await user.click(nav().getByRole("link", { name: "我的教材" }));
    expect(await screen.findByRole("heading", { name: "我的教材", level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["我的教材"]);

    await user.click(nav().getByRole("link", { name: "學習單範本" }));
    expect(await screen.findByRole("heading", { name: "學習單範本", level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["學習單範本"]);
  });

  it("the old 我的版型 anchor (/materials#templates) leads to 學習單範本's 我的範本", async () => {
    signIn();
    mockApi(baseRoutes());
    const { router } = renderApp("/materials#templates");
    await waitFor(() => expect(router.state.location.pathname).toBe("/templates"));
    expect(router.state.location.search).toBe("?tab=mine");
    expect(await screen.findByRole("tab", { name: "我的範本" })).toHaveAttribute("aria-selected", "true");
    expect(currentNav()).toEqual(["學習單範本"]);
  });

  it.each(["/library", "/plan"])("the old %s address leads to 備課庫, the list of every unit", async (path) => {
    signIn();
    mockApi(baseRoutes());
    const { router } = renderApp(path);
    await waitFor(() => expect(router.state.location.pathname).toBe("/units"));
    expect(await screen.findByRole("heading", { name: "備課庫", level: 1 })).toBeInTheDocument();
    // The heading is there before the list has loaded.
    expect(await screen.findByRole("link", { name: "倒裝句單元" })).toHaveAttribute("href", "/units/1");
    expect(currentNav()).toEqual(["班級與課例"]);
  });

  it("班級與課例 (under 更多) keeps the class home at /classes; 首頁 leads back", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(nav().getByRole("button", { name: "更多" }));
    await user.click(nav().getByRole("link", { name: "班級與課例" }));
    expect(await screen.findByRole("heading", { name: "班級", level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["班級與課例"]);
    await user.click(nav().getByRole("link", { name: "首頁" }));
    expect(await screen.findByRole("heading", { name: HOME, level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["首頁"]);
  });

  it("does not stop a teacher without preferences with a survey", async () => {
    signIn();
    mockApi(baseRoutes({}, null));
    renderApp("/");
    expect(await screen.findByRole("heading", { name: HOME, level: 1 })).toBeInTheDocument();
    expect(screen.queryByText(/初次使用設定/)).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "備課偏好" })).toHaveAttribute("href", "/preferences");
  });

  describe("the automatic fold on the prep page (laptop width)", () => {
    afterEach(() => vi.unstubAllGlobals());
    const stubWidth = (compact: boolean) =>
      vi.stubGlobal("matchMedia", (q: string) => ({
        matches: compact && q.includes("max-width: 79.9375rem"),
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
      }));
    // The session is not mocked: only the shell around the page is under test.
    const routes = () => baseRoutes();

    it("folds to icons without storing it as her choice", async () => {
      signIn();
      stubWidth(true);
      mockApi(routes());
      renderApp("/prep/7");
      const toggle = await screen.findByRole("button", { name: "側邊欄" });
      expect(toggle).toHaveAttribute("aria-expanded", "false");
      expect(document.querySelector(".shell")).toHaveClass("is-collapsed");
      expect(window.localStorage.getItem("manna.sidebarCollapsed")).toBeNull();
    });

    it("stays open on a wide window, and a stored 'open' beats the automatic fold", async () => {
      signIn();
      stubWidth(false);
      mockApi(routes());
      const { unmount } = renderApp("/prep/7");
      expect(await screen.findByRole("button", { name: "側邊欄" })).toHaveAttribute("aria-expanded", "true");
      unmount();

      stubWidth(true);
      window.localStorage.setItem("manna.sidebarCollapsed", "0");
      mockApi(routes());
      renderApp("/prep/7");
      expect(await screen.findByRole("button", { name: "側邊欄" })).toHaveAttribute("aria-expanded", "true");
    });

    it("other pages stay open on a narrow window, and her own toggle is what gets stored", async () => {
      signIn();
      stubWidth(true);
      mockApi(routes());
      const user = userEvent.setup();
      renderApp("/");
      const toggle = await screen.findByRole("button", { name: "側邊欄" });
      expect(toggle).toHaveAttribute("aria-expanded", "true");
      await user.click(toggle);
      expect(window.localStorage.getItem("manna.sidebarCollapsed")).toBe("1");
      await user.click(toggle);
      expect(window.localStorage.getItem("manna.sidebarCollapsed")).toBe("0");
    });
  });
});
