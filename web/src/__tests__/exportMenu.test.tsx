// 匯出 menu: one button, four explicit files (學生版／教師版 × Word／PDF), the same component on the
// prep session's worksheet card and on the editor page.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import { ExportMenu } from "../components/worksheet/ExportMenu";
import * as download from "../lib/download";

const ITEMS = ["學生版 Word", "學生版 PDF", "教師版 Word", "教師版 PDF"];

let exportSpy: ReturnType<typeof vi.spyOn>;
let saveSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  exportSpy = vi
    .spyOn(api, "exportWorksheet")
    .mockResolvedValue({ blob: new Blob(["x"]), filename: "worksheet.docx" });
  saveSpy = vi.spyOn(download, "saveBlob").mockImplementation(() => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

function setup(props: Partial<React.ComponentProps<typeof ExportMenu>> = {}) {
  const onError = vi.fn();
  render(
    <div>
      <ExportMenu worksheetId={9} level="standard" onError={onError} {...props} />
      <button type="button">別處</button>
    </div>,
  );
  return { onError, user: userEvent.setup() };
}

describe("ExportMenu", () => {
  it("is a closed menu button until opened, then lists the four files", async () => {
    const { user } = setup();
    const button = screen.getByRole("button", { name: /匯出/ });
    expect(button).toHaveAttribute("aria-haspopup", "menu");
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await user.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(screen.getAllByRole("menuitem").map((i) => i.textContent)).toEqual(ITEMS);
  });

  it("names the level on the first line only when there are several", async () => {
    const { user } = setup({ levelLabel: "基礎版" });
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    expect(screen.getByRole("menu")).toHaveTextContent("匯出：基礎版");
  });

  it("has no level line for a single level", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    expect(screen.getByRole("menu")).not.toHaveTextContent("匯出：");
  });

  it("each item exports with its own format and audience for the level shown", async () => {
    const { user, onError } = setup({ level: "advanced" });
    const expected = [
      { format: "docx", audience: "student" },
      { format: "pdf", audience: "student" },
      { format: "docx", audience: "teacher" },
      { format: "pdf", audience: "teacher" },
    ];
    for (const [i, name] of ITEMS.entries()) {
      await user.click(screen.getByRole("button", { name: /匯出/ }));
      await user.click(screen.getByRole("menuitem", { name }));
      await waitFor(() => expect(exportSpy).toHaveBeenCalledTimes(i + 1));
      expect(exportSpy).toHaveBeenLastCalledWith(9, { ...expected[i], level: "advanced" });
      await waitFor(() => expect(screen.getByRole("button", { name: /匯出/ })).toBeEnabled());
    }
    expect(saveSpy).toHaveBeenCalledTimes(4);
    expect(onError).not.toHaveBeenCalledWith(expect.stringMatching(/./));
  });

  it("opens with the arrow keys, moves, wraps, and activates with Enter", async () => {
    const { user } = setup();
    screen.getByRole("button", { name: /匯出/ }).focus();
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(screen.getByRole("menuitem", { name: "學生版 Word" })).toHaveFocus());
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "教師版 Word" })).toHaveFocus();
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "學生版 Word" })).toHaveFocus();
    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("menuitem", { name: "教師版 PDF" })).toHaveFocus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(exportSpy).toHaveBeenCalledWith(9, { format: "pdf", level: "standard", audience: "teacher" }));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("activates an item with Space", async () => {
    const { user } = setup();
    screen.getByRole("button", { name: /匯出/ }).focus();
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(screen.getByRole("menuitem", { name: "學生版 Word" })).toHaveFocus());
    await user.keyboard(" ");
    await waitFor(() => expect(exportSpy).toHaveBeenCalledWith(9, { format: "docx", level: "standard", audience: "student" }));
  });

  it("Esc closes the menu and returns focus to the button", async () => {
    const { user } = setup();
    const button = screen.getByRole("button", { name: /匯出/ });
    await user.click(button);
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(screen.getByRole("menuitem", { name: "學生版 Word" })).toHaveFocus());
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(button).toHaveFocus();
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(exportSpy).not.toHaveBeenCalled();
  });

  it("a click outside closes it", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "別處" }));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(exportSpy).not.toHaveBeenCalled();
  });

  it("while exporting the button says 匯出中… and is disabled", async () => {
    let finish: (v: { blob: Blob; filename: string }) => void = () => undefined;
    exportSpy.mockReturnValue(new Promise((resolve) => (finish = resolve)));
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    await user.click(screen.getByRole("menuitem", { name: "學生版 PDF" }));
    const busy = await screen.findByRole("button", { name: /匯出中…/ });
    expect(busy).toBeDisabled();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    finish({ blob: new Blob(["x"]), filename: "w.pdf" });
    await waitFor(() => expect(screen.getByRole("button", { name: /^匯出/ })).toBeEnabled());
    expect(saveSpy).toHaveBeenCalledTimes(1);
  });

  it("is disabled when told so", () => {
    setup({ disabled: true });
    expect(screen.getByRole("button", { name: /匯出/ })).toBeDisabled();
  });

  it("reports a failure and clears it on the next try", async () => {
    exportSpy.mockRejectedValueOnce(new Error("網路不通"));
    const { user, onError } = setup();
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    await user.click(screen.getByRole("menuitem", { name: "教師版 Word" }));
    await waitFor(() => expect(onError).toHaveBeenCalledWith("沒有匯出：網路不通"));
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    await user.click(screen.getByRole("menuitem", { name: "教師版 Word" }));
    await waitFor(() => expect(saveSpy).toHaveBeenCalledTimes(1));
    expect(onError).toHaveBeenLastCalledWith(null);
  });

  it("does not export when beforeExport says why not", async () => {
    const beforeExport = vi.fn().mockResolvedValue("還沒存好");
    const { user, onError } = setup({ beforeExport });
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    await user.click(screen.getByRole("menuitem", { name: "學生版 Word" }));
    await waitFor(() => expect(onError).toHaveBeenCalledWith("沒有匯出：還沒存好"));
    expect(exportSpy).not.toHaveBeenCalled();
  });

  it("shows the footnote inside the menu", async () => {
    const { user } = setup({ footnote: "Word 可再編輯" });
    await user.click(screen.getByRole("button", { name: /匯出/ }));
    expect(screen.getByRole("menu")).toHaveTextContent("Word 可再編輯");
  });
});
