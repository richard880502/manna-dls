// The app shell (docs/DESIGN_OVERHAUL.md step 1): three destinations (班級｜學習單｜教材), nothing unbuilt, all in Chinese.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import {
  baseRoutes,
  classRef,
  lessonDetail,
  mockApi,
  renderApp,
  schoolClass,
  signIn,
  teacher,
  unitWith,
} from "../test/mockApi";

describe("app shell", () => {
  it("has exactly three destinations, plus preferences and sign-out", async () => {
    signIn();
    mockApi(baseRoutes());
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "班級", level: 1 })).toBeInTheDocument();

    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    const links = nav.getAllByRole("link");
    expect(links.map((a) => a.textContent)).toEqual(["班級", "學習單", "教材"]);
    expect(links.map((a) => a.getAttribute("href"))).toEqual(["/", "/worksheets", "/materials"]);
    expect(nav.getByRole("link", { name: "班級" })).toHaveAttribute("aria-current", "page");

    expect(screen.getByRole("link", { name: /T-2026-001.*備課偏好/ })).toHaveAttribute("href", "/preferences");
    expect(screen.getByRole("button", { name: "登出" })).toBeInTheDocument();
    // A teacher does not see the admin entry.
    expect(screen.queryByRole("link", { name: "後台管理" })).not.toBeInTheDocument();

    // Nothing unbuilt is offered, and nothing is addressed to developers.
    for (const gone of [/Reflect/, /Share/, /履歷/, /社群動能地圖/, /Teach & Record/, /Phase/, /本輪平台範圍/]) {
      expect(screen.queryByText(gone)).not.toBeInTheDocument();
    }
  });

  it("gives the project lead a way to the admin page", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/me": { ...teacher, role: "pi" } });
    renderApp("/");
    expect(await screen.findByRole("link", { name: "後台管理" })).toHaveAttribute("href", "/admin");
  });

  it.each(["/reflect", "/share", "/profile", "/community", "/no-such-page"])(
    "%s is gone: it leads home",
    async (path) => {
      signIn();
      mockApi(baseRoutes());
      const { router } = renderApp(path);
      await waitFor(() => expect(router.state.location.pathname).toBe("/"));
      expect(await screen.findByRole("heading", { name: "班級", level: 1 })).toBeInTheDocument();
    },
  );

  it("學習單 opens the generator: paste material to make one, and an honest empty list", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/worksheets": [] });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("link", { name: "學習單" }));

    expect(await screen.findByRole("heading", { name: "學習單", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "貼上教材文字" })).toBeInTheDocument();
    expect(await screen.findByText("還沒有學習單。做好的會列在這裡。")).toBeInTheDocument();
    expect(within(screen.getByRole("navigation", { name: "主要導覽" })).getByRole("link", { name: "學習單" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  /** Which top-bar destination is marked as the current page. */
  const currentNav = () =>
    within(screen.getByRole("navigation", { name: "主要導覽" }))
      .getAllByRole("link")
      .filter((a) => a.getAttribute("aria-current") === "page")
      .map((a) => a.textContent);

  it.each([
    ["/units/1", "倒裝句單元"],
    ["/units", "備課庫"],
  ])("%s is inside 班級: units are reached from their class", async (path, heading) => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/units/1/lessons": [] });
    renderApp(path);
    expect(await screen.findByRole("heading", { name: heading, level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["班級"]);
  });

  it("a lesson reached from a class marks 班級, not another destination", async () => {
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
    expect(currentNav()).toEqual(["班級"]);
  });

  it("教材 is its own destination: the material library", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/materials": {
        materials: [],
        usage: { used_bytes: 0, quota_bytes: 1024, count: 0, max_count: 300 },
        limits: { files: [], max_pasted_chars: 50000, max_note_chars: 2000 },
        storage: "fs",
        uploads_enabled: true,
      },
    });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("link", { name: "教材" }));
    expect(await screen.findByRole("heading", { name: "教材", level: 1 })).toBeInTheDocument();
    expect(currentNav()).toEqual(["教材"]);
  });

  it.each(["/library", "/plan"])("the old %s address leads to 備課庫, the list of every unit", async (path) => {
    signIn();
    mockApi(baseRoutes());
    const { router } = renderApp(path);
    await waitFor(() => expect(router.state.location.pathname).toBe("/units"));
    expect(await screen.findByRole("heading", { name: "備課庫", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "倒裝句單元" })).toHaveAttribute("href", "/units/1");
    expect(currentNav()).toEqual(["班級"]);
  });

  it("does not stop a teacher without preferences with a survey", async () => {
    signIn();
    mockApi(baseRoutes({}, null));
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "班級", level: 1 })).toBeInTheDocument();
    expect(screen.queryByText(/初次使用設定/)).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: /備課偏好/ })).toHaveAttribute("href", "/preferences");
  });
});
