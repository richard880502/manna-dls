// 版本紀錄：列出整份學習單的每一版、預覽任一版、還原成那一版（寫成新的一版）。
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PrepSession, PrepTurn, SaveWorksheetRequest, WorksheetState, WsAudience, WsBlock, WsItem, WsSheet, WsVersionRow } from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";
import { openBarPopover } from "../test/menus";

const S7 = "/api/prep-sessions/7";
const WS = "/api/worksheets/11";
const MODE_KEY = "manna.prepSheetMode";

beforeEach(() => {
  signIn();
  window.localStorage.removeItem(MODE_KEY);
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  window.localStorage.removeItem(MODE_KEY);
  vi.unstubAllGlobals();
});

const item = (id: string, prompt: string): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: "",
  evidence: "",
  purpose: null,
  levels: ["standard"],
  origin: "ai",
});

const block = (prompt: string): WsBlock => ({
  id: "b2",
  kind: "activity",
  enabled: true,
  title: "Paragraph 1",
  instructions: "Read it.",
  paragraph_ids: [],
  info_function: null,
  student_tasks: [],
  objective_ids: [],
  vocabulary: [],
  items: [item("b2-1", prompt)],
  scaffolds: [],
  teacher_notes: "",
  origin: "ai",
  message: null,
  sop_block_id: null,
});

const worksheet = (prompt: string, version = 3, running = false): WorksheetState => ({
  id: 11,
  title: "學習單一",
  filename: "學習單一",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: version,
  created_at: "2026-11-25T08:00:00Z",
  updated_at: "2026-11-25T08:00:00Z",
  material: {
    title: "學習單一",
    paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
    source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 },
  },
  settings: emptySettings(),
  suggested: emptySettings(),
  brief: emptyBrief(),
  versions_count: version,
  saved_at: "2026-11-25T08:00:00Z",
  content: { ...emptyWorksheetContent(), levels: ["standard"], blocks: [block(prompt)] },
  jobs: [],
  running,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
});

const sheet = (audience: WsAudience): WsSheet => ({
  title: "學習單一",
  level: "standard",
  audience,
  objectives: [],
  blocks: [
    {
      id: "b2",
      kind: "activity",
      label: "C",
      title: "Paragraph 1",
      instructions: "Read it.",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ id: "b2-1", no: 1, answer_format: "簡答題", prompt: "Why?", options: [], lines: 2 }],
    },
  ],
});

const chatTurn = (status: PrepTurn["status"]): PrepTurn => ({
  id: 202,
  seq: 2,
  kind: "chat",
  request: "改一下",
  status,
  error: null,
  worksheet: null,
  result: null,
  interaction_id: null,
  created_at: "2026-11-25T08:00:00Z",
  finished_at: null,
});

const session = (turns: PrepTurn[]): PrepSession => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: turns.length + 1,
  last_kind: "worksheet",
  updated_at: "2026-11-25T09:00:00Z",
  created_at: "2026-11-25T08:00:00Z",
  turns: [
    {
      id: 101,
      seq: 1,
      kind: "worksheet",
      request: "做學習單",
      status: "done",
      error: null,
      worksheet: { id: 11, title: "學習單一", version_no: 3, running: false },
      result: null,
      interaction_id: null,
      created_at: "2026-11-25T08:00:00Z",
      finished_at: "2026-11-25T08:01:00Z",
    },
    ...turns,
  ],
  running: turns.some((t) => t.status === "running"),
  next: [],
});

const saved = (call: MockCall) => ({
  ...worksheet("x"),
  ...(call.body as SaveWorksheetRequest),
  version_no: 3,
  saved_at: "2026-11-25T09:30:00Z",
});

const routes = (extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  [`GET ${WS}/comments`]: [],
  [`GET ${WS}/proposals`]: [],
  [`GET ${WS}`]: worksheet("Why did she start it?"),
  [`PUT ${WS}`]: saved,
  [`GET ${WS}/sheet`]: (c: MockCall) =>
    sheet((new URL(c.url, "http://x").searchParams.get("audience") ?? "student") as WsAudience),
  [`GET ${S7}`]: session([]),
  [`POST ${S7}/turns`]: session([]),
  ...extra,
});


const PROMPT = "Why did she start it?";

const ROWS: WsVersionRow[] = [
  { version_no: 3, cause: "edit", source: { cause: "edit" }, created_at: "2026-11-25T09:00:00Z" },
  { version_no: 2, cause: "regenerate", source: { cause: "regenerate" }, created_at: "2026-11-25T08:30:00Z" },
  { version_no: 1, cause: "generate", source: null, created_at: "2026-11-25T08:00:00Z" },
];

const versionSheet = (no: number): WsSheet => {
  const s = sheet("student");
  return { ...s, title: `第${no}版的標題` };
};

const versionRoutes = (extra: Record<string, unknown> = {}) =>
  routes({
    [`GET ${WS}/versions`]: ROWS,
    [`GET ${WS}/versions/3/sheet`]: versionSheet(3),
    [`GET ${WS}/versions/2/sheet`]: versionSheet(2),
    [`GET ${WS}/versions/1/sheet`]: versionSheet(1),
    ...extra,
  });

/** 版本紀錄 is in the toolbar's 「第 N 版 ▾」 menu. */
const versionMenu = async (user: ReturnType<typeof userEvent.setup>) => {
  await screen.findByRole("button", { name: /^版本：第 \d+ 版$/ });
  return openBarPopover(user, /^版本：第 \d+ 版$/);
};

const openDialog = async (user: ReturnType<typeof userEvent.setup>) => {
  const menu = await versionMenu(user);
  await user.click(menu.getByRole("button", { name: "版本紀錄" }));
  return screen.findByRole("dialog", { name: "版本紀錄" });
};

describe("版本紀錄", () => {
  it("names the current version from the 「第 N 版」 menu, says so, and gives focus back to the menu", async () => {
    const api = mockApi(
      versionRoutes({
        [`POST ${WS}/snapshots`]: { id: 5, version_no: 3, name: "上課用定稿", created_at: "2026-11-26T08:00:00Z" },
      }),
    );
    const user = userEvent.setup();
    renderApp("/prep/7");
    const menu = await versionMenu(user);
    await user.click(menu.getByRole("button", { name: "為這一版命名…" }));
    const dialog = await screen.findByRole("dialog", { name: "為這一版命名" });
    await user.type(within(dialog).getByRole("textbox", { name: "名稱（選填）" }), "上課用定稿");
    await user.click(within(dialog).getByRole("button", { name: "儲存名稱" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    // Nothing was pending: the name goes on the version the page shows.
    expect(api.lastBody("POST", `${WS}/snapshots`)).toEqual({ name: "上課用定稿", base_version_no: 3 });
    expect(screen.getByText(/^已把第 3 版命名為「上課用定稿」/)).toHaveAttribute("role", "status");
    await waitFor(() => expect(screen.getByRole("button", { name: "版本：第 3 版" })).toHaveFocus());
  });

  it("lists every version newest first with what made it, and previews the selected one", async () => {
    const api = mockApi(versionRoutes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    const dialog = await openDialog(user);
    const list = await within(dialog).findByRole("list", { name: "學習單的版本" });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent("第 3 版・手動編輯");
    expect(items[0]).toHaveTextContent("目前");
    expect(items[1]).toHaveTextContent("第 2 版・重新產生一區");
    expect(items[2]).toHaveTextContent("第 1 版・Manna 產生");
    // The version before the current one is shown first; its sheet is the one asked for.
    await waitFor(() => expect(api.callsTo("GET", `${WS}/versions/2/sheet`)).toHaveLength(1));
    expect(await within(dialog).findByRole("article", { name: /學生版/ })).toBeInTheDocument();
    await user.click(within(list).getByRole("button", { name: /第 1 版/ }));
    await waitFor(() => expect(api.callsTo("GET", `${WS}/versions/1/sheet`)).toHaveLength(1));
    expect(new URL(api.callsTo("GET", `${WS}/versions/1/sheet`)[0]!.url, "http://x").searchParams.get("audience")).toBe(
      "student",
    );
    await user.click(within(dialog).getByRole("radio", { name: "教師版" }));
    await waitFor(() => expect(api.callsTo("GET", `${WS}/versions/1/sheet`)).toHaveLength(2));
    // The current version cannot be restored.
    await user.click(within(list).getByRole("button", { name: /第 3 版/ }));
    expect(within(dialog).getByRole("button", { name: "還原成這一版" })).toBeDisabled();
  });

  it("asks first (the safe choice has focus), then restores and shows the new version", async () => {
    let restored = false;
    const api = mockApi(
      versionRoutes({
        [`GET ${WS}`]: () => (restored ? worksheet("Back to the old one", 4) : worksheet(PROMPT)),
        [`POST ${WS}/versions/2/restore`]: () => {
          restored = true;
          return { version_no: 4, from_version_no: 2, worksheet: worksheet("Back to the old one", 4) };
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/prep/7");
    const dialog = await openDialog(user);
    await user.click(await within(dialog).findByRole("button", { name: "還原成這一版" }));
    const safe = within(dialog).getByRole("button", { name: "不要還原" });
    await waitFor(() => expect(safe).toHaveFocus());
    expect(api.callsTo("POST", `${WS}/versions/2/restore`)).toHaveLength(0);
    // 不要還原 goes back without calling the server.
    await user.click(safe);
    expect(api.callsTo("POST", `${WS}/versions/2/restore`)).toHaveLength(0);

    await user.click(within(dialog).getByRole("button", { name: "還原成這一版" }));
    await user.click(within(dialog).getByRole("button", { name: "還原" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/versions/2/restore`)).toHaveLength(1));
    expect(api.lastBody("POST", `${WS}/versions/2/restore`)).toEqual({ base_version_no: 3 });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "版本紀錄" })).not.toBeInTheDocument());
    // The editor beside the conversation shows the restored version.
    expect(await screen.findByDisplayValue("Back to the old one")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "版本：第 4 版" })).toBeInTheDocument();
  });

  it("shows the reason of a 409 inside the dialog and keeps it open", async () => {
    const WHY = "備課對話正在修改這份學習單，等這一輪結束後再還原。";
    mockApi(
      versionRoutes({
        [`POST ${WS}/versions/2/restore`]: respond(409, { error: { code: "conflict", message: WHY } }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/prep/7");
    const dialog = await openDialog(user);
    await user.click(await within(dialog).findByRole("button", { name: "還原成這一版" }));
    await user.click(within(dialog).getByRole("button", { name: "還原" }));
    expect(await within(dialog).findByText(WHY)).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "版本紀錄" })).toBeInTheDocument();
  });

  it("is disabled with a reason while a chat turn runs", async () => {
    const api = mockApi(versionRoutes({ [`GET ${S7}`]: session([chatTurn("running")]) }));
    const user = userEvent.setup();
    renderApp("/prep/7");
    const menu = await versionMenu(user);
    const button = menu.getByRole("button", { name: "版本紀錄" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAccessibleDescription(/備課對話正在處理/);
    expect(menu.getByRole("button", { name: "為這一版命名…" })).toHaveAttribute("aria-disabled", "true");
    await user.click(button);
    expect(screen.queryByRole("dialog", { name: "版本紀錄" })).not.toBeInTheDocument();
    expect(api.callsTo("GET", `${WS}/versions`)).toHaveLength(0);
  });

  it("in the editor: saves pending edits first, then shows the restored version", async () => {
    window.localStorage.setItem(MODE_KEY, "edit");
    const api = mockApi(
      versionRoutes({
        [`POST ${WS}/versions/2/restore`]: { version_no: 4, from_version_no: 2, worksheet: worksheet("Back to the old one", 4) },
      }),
    );
    const user = userEvent.setup();
    renderApp("/prep/7");
    fireEvent.change(await screen.findByRole("textbox", { name: "題目 1" }), { target: { value: "Typed, not saved yet" } });
    expect(api.callsTo("PUT", WS)).toHaveLength(0);
    const dialog = await openDialog(user);
    // The pending edit became a version before the list was read.
    const order = api.calls.map((c) => `${c.method} ${c.path}`);
    expect(order.indexOf(`PUT ${WS}`)).toBeGreaterThan(-1);
    expect(order.indexOf(`PUT ${WS}`)).toBeLessThan(order.indexOf(`GET ${WS}/versions`));
    await user.click(await within(dialog).findByRole("button", { name: "還原成這一版" }));
    await user.click(within(dialog).getByRole("button", { name: "還原" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "題目 1" })).toHaveValue("Back to the old one"));
  });
});
