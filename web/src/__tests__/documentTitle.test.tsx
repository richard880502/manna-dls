// The tab title: 「{page}｜Manna DLS」 while a page is open, back to what it was when the page is left.
import { renderHook, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { pageTitle, useDocumentTitle } from "../lib/useDocumentTitle";
import { baseRoutes, mockApi, renderApp, signIn } from "../test/mockApi";

describe("useDocumentTitle", () => {
  it("names the tab after the page and restores the old title on leaving", () => {
    document.title = "Manna DLS";
    const { rerender, unmount } = renderHook(({ page }) => useDocumentTitle(page), { initialProps: { page: "我的作品" as string | null } });
    expect(document.title).toBe("我的作品｜Manna DLS");
    rerender({ page: "The Umbrella Stand" });
    expect(document.title).toBe("The Umbrella Stand｜Manna DLS");
    unmount();
    expect(document.title).toBe("Manna DLS");
  });

  it("is just the site name while the page has no name yet", () => {
    expect(pageTitle(null)).toBe("Manna DLS");
    expect(pageTitle("  ")).toBe("Manna DLS");
  });
});

describe("a routed page", () => {
  it("sets the tab title from its heading", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/worksheets": [] });
    renderApp("/prep");
    await screen.findByRole("heading", { level: 1, name: "我的作品" });
    // The title is set by an effect, which may run a moment after the heading is painted.
    await waitFor(() => expect(document.title).toBe("我的作品｜Manna DLS"));
  });
});
