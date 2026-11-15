// The app shell (docs/DESIGN_OVERHAUL.md step 1): three destinations, nothing unbuilt, all in Chinese.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { baseRoutes, mockApi, renderApp, signIn, teacher } from "../test/mockApi";

describe("app shell", () => {
  it("has exactly three destinations, plus preferences and sign-out", async () => {
    signIn();
    mockApi(baseRoutes());
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "班級", level: 1 })).toBeInTheDocument();

    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    const links = nav.getAllByRole("link");
    expect(links.map((a) => a.textContent)).toEqual(["班級", "備課庫", "學習單"]);
    expect(links.map((a) => a.getAttribute("href"))).toEqual(["/", "/library", "/worksheets"]);
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

  it("marks 備課庫 as the place for units and lessons", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/units/1/lessons": [], "GET /api/units/1/materials": [] });
    renderApp("/units/1");
    expect(await screen.findByRole("heading", { name: "倒裝句單元", level: 1 })).toBeInTheDocument();
    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    expect(nav.getByRole("link", { name: "備課庫" })).toHaveAttribute("aria-current", "page");
    expect(nav.getByRole("link", { name: "班級" })).not.toHaveAttribute("aria-current");
  });
});
