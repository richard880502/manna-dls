// 為這一版命名（先存好編輯、預設名稱、409）與版本紀錄裡有名字的版本：標示、篩選、收合、改名、移除名稱。
// API 與研究紀錄仍叫 snapshot（`/snapshots`、`worksheet_snapshot_saved`）。
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WsSheet, WsVersionRow } from "../api/types";
import { NameVersionDialog, type FlushResult } from "../components/worksheet/workspace/NameVersionDialog";
import { VersionHistoryDialog, groupVersions } from "../components/worksheet/VersionHistory";
import { mockApi, respond, signIn } from "../test/mockApi";

const WS = "/api/worksheets/11";

beforeEach(() => signIn());
afterEach(() => vi.unstubAllGlobals());

const field = (dialog: HTMLElement) => within(dialog).getByRole("textbox", { name: "名稱（選填）" });
const saveButton = (dialog: HTMLElement) => within(dialog).getByRole("button", { name: "儲存名稱" });

describe("為這一版命名", () => {
  it("saves pending edits first, then names the version that made", async () => {
    const order: string[] = [];
    const api = mockApi({
      [`POST ${WS}/snapshots`]: () => {
        order.push("name");
        return { id: 9, version_no: 6, name: "考前版", created_at: "2026-11-25T09:00:00Z" };
      },
    });
    const flush = vi.fn(async (): Promise<FlushResult> => {
      order.push("flush");
      return { ok: true, versionNo: 6 };
    });
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<NameVersionDialog worksheetId={11} baseVersionNo={5} flush={flush} onClose={() => {}} onSaved={onSaved} />);
    const dialog = await screen.findByRole("dialog", { name: "為這一版命名" });
    await user.type(field(dialog), "  考前版 ");
    await user.click(saveButton(dialog));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(order).toEqual(["flush", "name"]);
    // The base is the version the flush made, the name is trimmed.
    expect(api.lastBody("POST", `${WS}/snapshots`)).toEqual({ name: "考前版", base_version_no: 6 });
    expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ version_no: 6, name: "考前版" }));
  });

  it("sends no name when the field is empty and shows the default name as the placeholder", async () => {
    const api = mockApi({
      [`POST ${WS}/snapshots`]: { id: 1, version_no: 5, name: "版本 11/25 17:30", created_at: "2026-11-25T09:00:00Z" },
    });
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<NameVersionDialog worksheetId={11} baseVersionNo={5} onClose={() => {}} onSaved={onSaved} />);
    const dialog = await screen.findByRole("dialog", { name: "為這一版命名" });
    expect(field(dialog)).toHaveAttribute("placeholder", expect.stringMatching(/^版本 \d\d\/\d\d \d\d:\d\d$/));
    await user.click(saveButton(dialog));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/snapshots`)).toHaveLength(1));
    expect(api.lastBody("POST", `${WS}/snapshots`)).toEqual({ base_version_no: 5 });
    expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ name: "版本 11/25 17:30" }));
  });

  it("shows a 409 reason inside the dialog and stays open; a failed flush stops before the request", async () => {
    const WHY = "這一版已經有名字了，可以在版本紀錄裡改名。";
    const api = mockApi({ [`POST ${WS}/snapshots`]: respond(409, { error: { code: "conflict", message: WHY } }) });
    const user = userEvent.setup();
    const onSaved = vi.fn();
    const { rerender } = render(
      <NameVersionDialog worksheetId={11} baseVersionNo={5} onClose={() => {}} onSaved={onSaved} />,
    );
    let dialog = await screen.findByRole("dialog", { name: "為這一版命名" });
    await user.click(saveButton(dialog));
    expect(await within(dialog).findByText(WHY)).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "為這一版命名" })).toBeInTheDocument();

    rerender(
      <NameVersionDialog
        worksheetId={11}
        baseVersionNo={5}
        flush={async () => ({ ok: false, error: "網路不通" })}
        onClose={() => {}}
        onSaved={onSaved}
      />,
    );
    dialog = screen.getByRole("dialog", { name: "為這一版命名" });
    await user.click(saveButton(dialog));
    expect(await within(dialog).findByText(/沒有儲存成功，所以還不能命名：網路不通/)).toBeInTheDocument();
    expect(api.callsTo("POST", `${WS}/snapshots`)).toHaveLength(1);
    expect(onSaved).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------- 版本紀錄

const at = (hhmm: string) => `2026-11-25T${hhmm}:00`;
const edit = (no: number, hhmm: string, snapshot: WsVersionRow["snapshot"] = null): WsVersionRow => ({
  version_no: no,
  cause: "edit",
  source: { cause: "edit" },
  created_at: at(hhmm),
  snapshot,
});

const ROWS: WsVersionRow[] = [
  edit(8, "17:30"),
  edit(7, "17:20"),
  edit(6, "17:10", { id: 3, name: "考前版" }),
  edit(5, "17:05"),
  edit(4, "17:02"),
  { version_no: 3, cause: "prep_turn", source: { cause: "prep_turn" }, created_at: at("16:00"), snapshot: null },
  { version_no: 2, cause: "regenerate", source: { cause: "regenerate" }, created_at: at("15:30"), snapshot: { id: 2, name: "第一稿" } },
  { version_no: 1, cause: "generate", source: null, created_at: at("15:00"), snapshot: null },
];

const sheet: WsSheet = { title: "學習單一", level: "standard", audience: "student", objectives: [], blocks: [] };

const historyRoutes = (extra: Record<string, unknown> = {}) => {
  const routes: Record<string, unknown> = { [`GET ${WS}/versions`]: ROWS, ...extra };
  for (const r of ROWS) routes[`GET ${WS}/versions/${r.version_no}/sheet`] = sheet;
  return routes;
};

const open = async (initialVersionNo?: number) => {
  const onRestored = vi.fn();
  render(<VersionHistoryDialog worksheetId={11} initialVersionNo={initialVersionNo} onClose={() => {}} onRestored={onRestored} />);
  const dialog = await screen.findByRole("dialog", { name: "版本紀錄" });
  const list = await within(dialog).findByRole("list", { name: "學習單的版本" });
  return { dialog, list, onRestored };
};

describe("版本紀錄裡有名字的版本", () => {
  it("groups consecutive plain manual edits (the newest stays a row) and expands on demand", async () => {
    mockApi(historyRoutes());
    const user = userEvent.setup();
    const { list } = await open();
    // 8 stays a row; 7 is a lone edit between 8 and the snapshot 6, so it stays a row too; 5+4 fold.
    expect(within(list).getByRole("button", { name: /第 8 版/ })).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: /第 7 版/ })).toBeInTheDocument();
    const group = within(list).getByRole("button", { name: "手動編輯 ×2・17:02–17:05" });
    expect(group).toHaveAttribute("aria-expanded", "false");
    expect(within(list).queryByRole("button", { name: /第 5 版/ })).not.toBeInTheDocument();
    await user.click(group);
    expect(group).toHaveAttribute("aria-expanded", "true");
    expect(within(list).getByRole("button", { name: /第 5 版/ })).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: /第 4 版/ })).toBeInTheDocument();
    await user.click(group);
    expect(within(list).queryByRole("button", { name: /第 4 版/ })).not.toBeInTheDocument();
  });

  it("groupVersions folds only runs of two or more", () => {
    const entries = groupVersions([edit(5, "10:00"), edit(4, "09:50"), edit(3, "09:40", { id: 1, name: "x" }), edit(2, "09:30"), edit(1, "09:00")]);
    expect(entries.map((e) => (e.kind === "group" ? `g${e.rows.length}` : `r${e.row.version_no}`))).toEqual(["r5", "r4", "r3", "g2"]);
  });

  it("shows the names on their versions and filters with 只看有名字的版本", async () => {
    mockApi(historyRoutes());
    const user = userEvent.setup();
    const { dialog, list } = await open();
    expect(within(list).getByRole("button", { name: /第 6 版.*名稱：考前版/ })).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: /第 2 版.*名稱：第一稿/ })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("checkbox", { name: "只看有名字的版本" }));
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("第 6 版");
    expect(items[1]).toHaveTextContent("第 2 版");
    await user.click(within(dialog).getByRole("checkbox", { name: "只看有名字的版本" }));
    expect(within(list).getAllByRole("listitem").length).toBeGreaterThan(2);
  });

  it("opens with initialVersionNo selected, expanding the group that holds it", async () => {
    const api = mockApi(historyRoutes());
    const { list } = await open(4);
    expect(within(list).getByRole("button", { name: /第 4 版/ })).toHaveAttribute("aria-current", "true");
    await waitFor(() => expect(api.callsTo("GET", `${WS}/versions/4/sheet`)).toHaveLength(1));
    expect(api.callsTo("GET", `${WS}/versions/7/sheet`)).toHaveLength(0);
  });

  it("renames a snapshot from the dialog", async () => {
    const api = mockApi(
      historyRoutes({
        [`PATCH ${WS}/snapshots/2`]: { id: 2, version_no: 2, name: "定稿", created_at: at("15:31") },
      }),
    );
    const user = userEvent.setup();
    const { dialog, list } = await open();
    await user.click(within(list).getByRole("button", { name: /第 2 版/ }));
    await user.click(within(dialog).getByRole("button", { name: "改名" }));
    const input = within(dialog).getByRole("textbox", { name: "版本名稱" });
    await user.clear(input);
    await user.type(input, "定稿");
    await user.click(within(dialog).getByRole("button", { name: "儲存名稱" }));
    await waitFor(() => expect(api.lastBody("PATCH", `${WS}/snapshots/2`)).toEqual({ name: "定稿" }));
    expect(await within(list).findByRole("button", { name: /第 2 版.*名稱：定稿/ })).toBeInTheDocument();
  });

  it("removes a snapshot after asking; the version stays in the list", async () => {
    const api = mockApi(historyRoutes({ [`DELETE ${WS}/snapshots/2`]: { ok: true } }));
    const user = userEvent.setup();
    const { dialog, list } = await open();
    await user.click(within(list).getByRole("button", { name: /第 2 版/ }));
    await user.click(within(dialog).getByRole("button", { name: "移除名稱" }));
    expect(api.callsTo("DELETE", `${WS}/snapshots/2`)).toHaveLength(0);
    expect(within(dialog).getByText(/這一版仍留在版本紀錄裡/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "確定移除" }));
    await waitFor(() => expect(api.callsTo("DELETE", `${WS}/snapshots/2`)).toHaveLength(1));
    const row = await within(list).findByRole("button", { name: /第 2 版/ });
    expect(row).not.toHaveTextContent("名稱：");
    expect(within(dialog).queryByRole("button", { name: "改名" })).not.toBeInTheDocument();
  });
});
