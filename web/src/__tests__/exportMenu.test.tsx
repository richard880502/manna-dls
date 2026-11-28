import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import { ExportMenu } from "../components/worksheet/ExportMenu";
import * as download from "../lib/download";

afterEach(() => {
  vi.restoreAllMocks();
});

function setup(props: Partial<React.ComponentProps<typeof ExportMenu>> = {}) {
  const onError = vi.fn();
  const save = vi.spyOn(download, "saveBlob").mockImplementation(() => undefined);
  render(<ExportMenu worksheetId={9} level="standard" onError={onError} {...props} />);
  return { onError, save, user: userEvent.setup() };
}

describe("worksheet export", () => {
  it("opens the accessible chooser by keyboard and returns focus on Escape", async () => {
    const { user } = setup({ levelLabel: "基礎版" });
    const trigger = screen.getByRole("button", { name: "匯出" });
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    trigger.focus();
    await user.keyboard("{Enter}");
    const dialog = within(screen.getByRole("dialog", { name: "匯出：基礎版" }));
    expect(dialog.getAllByRole("checkbox")).toHaveLength(4);
    expect(dialog.getByRole("button", { name: "下載" })).toBeDisabled();
    expect(dialog.getByRole("checkbox", { name: "學生版 Word" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("exports one worksheet file in the selected format and audience", async () => {
    const file = { blob: new Blob(["x"]), filename: "worksheet.pdf" };
    const single = vi.spyOn(api, "exportWorksheet").mockResolvedValue(file);
    const { user, save } = setup({ level: "advanced" });
    await user.click(screen.getByRole("button", { name: "匯出" }));
    await user.click(screen.getByRole("checkbox", { name: "教師版 PDF" }));
    await user.click(screen.getByRole("button", { name: "下載" }));
    expect(single).toHaveBeenCalledWith(9, { format: "pdf", audience: "teacher", level: "advanced" });
    expect(save).toHaveBeenCalledWith(file.blob, file.filename);
  });

  it("downloads selected versions and an available activity together exactly once", async () => {
    const file = { blob: new Blob(["x"]), filename: "worksheet.zip" };
    const bundle = vi.spyOn(api, "exportWorksheetBundle").mockResolvedValue(file);
    const options = vi.spyOn(api, "worksheetExportOptions").mockResolvedValue({ activity: true });
    const beforeExport = vi.fn().mockResolvedValue(null);
    const { user, save } = setup({ hasPrepSession: true, beforeExport });
    await user.click(screen.getByRole("button", { name: "匯出" }));
    await user.click(await screen.findByRole("checkbox", { name: "課堂活動 Word" }));
    await user.click(screen.getByRole("checkbox", { name: "學生版 Word" }));
    await user.click(screen.getByRole("checkbox", { name: "教師版 PDF" }));
    await user.click(screen.getByRole("button", { name: "下載 ZIP" }));
    expect(options).toHaveBeenCalledWith(9);
    expect(beforeExport).toHaveBeenCalledTimes(1);
    expect(bundle).toHaveBeenCalledExactlyOnceWith(9, "standard", ["activity", "student-docx", "teacher-pdf"]);
    expect(save).toHaveBeenCalledExactlyOnceWith(file.blob, file.filename);
  });

  it("waits for edits to save before downloading", async () => {
    const beforeExport = vi.fn().mockResolvedValue("還沒存好");
    const single = vi.spyOn(api, "exportWorksheet");
    const { user, onError } = setup({ beforeExport });
    await user.click(screen.getByRole("button", { name: "匯出" }));
    await user.click(screen.getByRole("checkbox", { name: "學生版 Word" }));
    await user.click(screen.getByRole("button", { name: "下載" }));
    expect(onError).toHaveBeenCalledWith("沒有匯出：還沒存好");
    expect(single).not.toHaveBeenCalled();
  });
});
