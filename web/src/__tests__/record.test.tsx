import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LessonRun, RecordOp, ReviewRequest, RunDetail, SyncResponse } from "../api/types";
import { NOTE_IDLE_SECONDS } from "../lib/recordQueue";
import { fakeRecordServer, recordEvent } from "../test/fakeRecordServer";
import {
  baseRoutes,
  lessonDetail,
  mockApi,
  renderApp,
  respond,
  schoolClass,
  signIn,
  step,
  type MockCall,
} from "../test/mockApi";

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const SYNC = "/api/runs/9/events/sync";
const QUEUE_KEY = "manna_dls_record_queue:run:9";

const run = (overrides: Partial<LessonRun> = {}): LessonRun => ({
  id: 9,
  lesson_id: 7,
  lesson_title: "第 1 課：Not only… but also",
  unit_id: 1,
  unit_title: "倒裝句單元",
  started_at: "2026-11-12T01:00:00Z",
  ended_at: null,
  review_completed_at: null,
  good_count: 0,
  stuck_count: 0,
  class_id: null,
  class_name: null,
  ...overrides,
});

const runDetail = (overrides: Partial<RunDetail> = {}): RunDetail => ({
  ...run(),
  events: [],
  plan: {
    lesson_version_id: 70,
    version_no: 2,
    objective: "學生能說明倒裝的語意功能",
    steps: [
      { id: 301, ...step({ position: 1, activity_type: "暖身", content: "比較兩組句子" }) },
      { id: 302, ...step({ position: 2, activity_type: "兩兩討論", content: "改寫倒裝句" }) },
    ],
  },
  remotes: [],
  ...overrides,
});

/** Sync route backed by an in-memory server with the API's idempotency rules. */
function syncRoute(fail: (n: number) => "network" | "server" | null = () => null) {
  const server = fakeRecordServer();
  let calls = 0;
  const handler = (call: MockCall) => {
    calls += 1;
    const mode = fail(calls);
    if (mode === "network") throw new TypeError("Failed to fetch");
    const res: SyncResponse = server.apply((call.body as { ops: RecordOp[] }).ops);
    if (mode === "server") return respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } });
    return res;
  };
  return { server, handler };
}

function opsSent(api: ReturnType<typeof mockApi>, path = SYNC): RecordOp[] {
  return api.callsTo("POST", path).flatMap((c) => (c.body as { ops: RecordOp[] }).ops);
}

function scaffolds(api: ReturnType<typeof mockApi>) {
  return api.callsTo("POST", "/api/scaffold-events").map((c) => c.body as Record<string, unknown>);
}

async function openConsole() {
  const rendered = renderApp("/runs/9");
  expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
  return rendered;
}

afterEach(() => {
  vi.useRealTimers();
  delete (window as Window & { documentPictureInPicture?: unknown }).documentPictureInPicture;
});

describe("Teach & Record console", () => {
  it("tap → written to the device → synced (device id, device time, type)", async () => {
    signIn();
    const { server, handler } = syncRoute();
    let storedAtSend: RecordOp[] = [];
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": runDetail(),
      [`POST ${SYNC}`]: (call: MockCall) => {
        storedAtSend = JSON.parse(window.localStorage.getItem(QUEUE_KEY) ?? "[]") as RecordOp[];
        return handler(call);
      },
    });
    const user = userEvent.setup();
    await openConsole();
    // Fixed SVG icons, not emoji.
    const good = screen.getByRole("button", { name: "順利" });
    expect(good.querySelector("svg")).not.toBeNull();
    expect(good).toHaveAccessibleName("順利");

    const before = Date.now();
    await user.click(good);
    await waitFor(() => expect(api.callsTo("POST", SYNC)).toHaveLength(1));
    const [op] = opsSent(api);
    expect(op).toMatchObject({ kind: "create", polarity: "good", device_type: "desktop" });
    if (op?.kind !== "create") throw new Error("expected a create");
    expect(op.device_event_id).toMatch(UUID_V4);
    expect(op.op_id).toMatch(UUID_V4);
    expect(Date.parse(op.device_ts)).toBeGreaterThanOrEqual(before - 1);
    // The op was on the device before it went to the server.
    expect(storedAtSend.map((o) => o.op_id)).toEqual([op.op_id]);

    expect(await screen.findByText("全部已同步")).toBeInTheDocument();
    expect(window.localStorage.getItem(QUEUE_KEY)).toBeNull();
    expect(server.events.size).toBe(1);
    expect(screen.getByText(/^已記下：順利/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "這節課記了 1 次" })).toBeInTheDocument();

    // Scaffold: console shown, first tap used (once).
    await user.click(screen.getByRole("button", { name: "卡住" }));
    await waitFor(() => expect(server.events.size).toBe(2));
    expect(scaffolds(api)).toEqual([
      { scaffold_code: "SC-RECORD", action: "show" },
      { scaffold_code: "SC-RECORD", action: "use", trigger: "console" },
    ]);
  });

  it("offline taps stay on the device, show the pending count and sync once on reconnect", async () => {
    signIn();
    let offline = true;
    const { server, handler } = syncRoute(() => (offline ? "network" : null));
    const api = mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail(), [`POST ${SYNC}`]: handler });
    const user = userEvent.setup();
    await openConsole();

    await user.click(screen.getByRole("button", { name: "順利" }));
    await user.click(screen.getByRole("button", { name: "卡住" }));
    expect(await screen.findByText(/^2 筆待同步：目前離線/)).toBeInTheDocument();
    expect(screen.getAllByText("待同步")).toHaveLength(2);
    expect(JSON.parse(window.localStorage.getItem(QUEUE_KEY) ?? "[]")).toHaveLength(2);

    offline = false;
    act(() => {
      window.dispatchEvent(new Event("online"));
    });
    expect(await screen.findByText("全部已同步")).toBeInTheDocument();
    expect(server.events.size).toBe(2);
    expect(screen.queryAllByText("待同步")).toHaveLength(0);
    // Every create sent so far carries one of the same two device ids: retries never mint new ones.
    const ids = new Set(opsSent(api).map((o) => o.device_event_id));
    expect(ids.size).toBe(2);
    expect(screen.getByRole("heading", { name: "這節課記了 2 次" })).toBeInTheDocument();
  });

  it("a response lost after the server stored the tap does not duplicate it", async () => {
    signIn();
    const { server, handler } = syncRoute((n) => (n === 1 ? "server" : null));
    const api = mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail(), [`POST ${SYNC}`]: handler });
    const user = userEvent.setup();
    await openConsole();
    await user.click(screen.getByRole("button", { name: "順利" }));
    await user.click(await screen.findByRole("button", { name: "立即重試" }));
    expect(await screen.findByText("全部已同步")).toBeInTheDocument();
    expect(api.callsTo("POST", SYNC)).toHaveLength(2);
    expect(server.events.size).toBe(1);
    expect(screen.getAllByRole("listitem", { name: /順利$/ })).toHaveLength(1);
  });

  it("undo removes the last tap and switching / deleting append ops", async () => {
    signIn();
    const { server, handler } = syncRoute();
    const api = mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail(), [`POST ${SYNC}`]: handler });
    const user = userEvent.setup();
    await openConsole();
    expect(screen.getByRole("button", { name: "復原上一筆" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "順利" }));
    await user.click(screen.getByRole("button", { name: "卡住" }));
    await waitFor(() => expect(server.events.size).toBe(2));
    const [first, second] = opsSent(api);

    await user.click(screen.getByRole("button", { name: "復原上一筆" }));
    await waitFor(() => expect(server.events.size).toBe(1));
    expect(opsSent(api).at(-1)).toMatchObject({ kind: "undo", device_event_id: second?.device_event_id });
    expect(screen.queryAllByRole("listitem", { name: /卡住$/ })).toHaveLength(0);
    expect(screen.getByText(/^已記下：順利/)).toBeInTheDocument();

    // Switch 順利 → 卡住, then delete it (with confirmation).
    const row = screen.getByRole("listitem", { name: /順利$/ });
    await user.click(within(row).getByRole("button", { name: "改成卡住" }));
    await waitFor(() =>
      expect(opsSent(api).at(-1)).toMatchObject({
        kind: "edit",
        device_event_id: first?.device_event_id,
        polarity: "stuck",
      }),
    );
    const stuckRow = await screen.findByRole("listitem", { name: /卡住$/ });
    await user.click(within(stuckRow).getByRole("button", { name: "刪除" }));
    await user.click(within(stuckRow).getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(server.events.size).toBe(0));
    expect(opsSent(api).at(-1)).toMatchObject({ kind: "delete", device_event_id: first?.device_event_id });
    expect(screen.getByText("還沒有書籤。沒有書籤也沒關係。")).toBeInTheDocument();
  });

  it("the sentence field collapses by itself and never blocks the next tap", async () => {
    signIn();
    const { server, handler } = syncRoute();
    const api = mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail(), [`POST ${SYNC}`]: handler });
    await openConsole();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

    await user.click(screen.getByRole("button", { name: "順利" }));
    const note = screen.getByLabelText("加一句話（可略過）");
    expect(screen.getByText(/秒沒有動作會自動收起/)).toHaveTextContent(
      `不要寫學生姓名。${NOTE_IDLE_SECONDS} 秒沒有動作會自動收起`,
    );
    await user.type(note, "例句太難");
    // Tap again right away: not blocked; the sentence goes to the first bookmark.
    const stuck = screen.getByRole("button", { name: "卡住" });
    expect(stuck).toBeEnabled();
    await user.click(stuck);
    await waitFor(() => expect(server.events.size).toBe(2));
    const creates = opsSent(api).filter((o) => o.kind === "create");
    expect(creates.map((o) => (o.kind === "create" ? o.polarity : ""))).toEqual(["good", "stuck"]);
    await waitFor(() =>
      expect(opsSent(api).find((o) => o.kind === "edit")).toMatchObject({
        device_event_id: creates[0]?.device_event_id,
        note: "例句太難",
      }),
    );
    // A fresh, empty field for the new tap.
    expect(screen.getByLabelText("加一句話（可略過）")).toHaveValue("");

    // Idle for NOTE_IDLE_SECONDS → collapses; typed text is kept.
    await user.type(screen.getByLabelText("加一句話（可略過）"), "改用生活情境");
    act(() => {
      vi.advanceTimersByTime(NOTE_IDLE_SECONDS * 1000 - 500);
    });
    expect(screen.getByLabelText("加一句話（可略過）")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(screen.queryByLabelText("加一句話（可略過）")).not.toBeInTheDocument();
    expect(
      within(screen.getByRole("group", { name: "這次書籤的一句話" })).getByRole("button", { name: "改這一句" }),
    ).toBeInTheDocument();
    expect(screen.getByText("「改用生活情境」")).toBeInTheDocument();
    await waitFor(() =>
      expect(opsSent(api).filter((o) => o.kind === "edit").map((o) => (o.kind === "edit" ? o.note : ""))).toEqual([
        "例句太難",
        "改用生活情境",
      ]),
    );

    // Untouched field just collapses, nothing is sent.
    await user.click(screen.getByRole("button", { name: "順利" }));
    act(() => {
      vi.advanceTimersByTime(NOTE_IDLE_SECONDS * 1000 + 100);
    });
    expect(screen.queryByLabelText("加一句話（可略過）")).not.toBeInTheDocument();
    const group = within(screen.getByRole("group", { name: "這次書籤的一句話" }));
    expect(group.getByRole("button", { name: "加一句話" })).toBeInTheDocument();
    await waitFor(() => expect(server.events.size).toBe(3));
    expect(opsSent(api).filter((o) => o.kind === "edit")).toHaveLength(2);
  });

  it("ends the class after syncing and goes to the review", async () => {
    signIn();
    const { handler } = syncRoute();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": runDetail(),
      [`POST ${SYNC}`]: handler,
      "POST /api/runs/9/end": run({ ended_at: "2026-11-12T01:50:00Z" }),
    });
    const user = userEvent.setup();
    const { router } = await openConsole();
    await user.click(screen.getByRole("button", { name: "結束本節課" }));
    expect(api.callsTo("POST", "/api/runs/9/end")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "確定結束並回顧" }));
    // The console has a heading with the same title, so wait for the review page itself:
    // finding that heading could pass before the class was ended.
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9/review"));
    expect(api.callsTo("POST", "/api/runs/9/end")).toHaveLength(1);
    expect(await screen.findByRole("heading", { name: "教案步驟" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
  });

  it("QR pairing shows a single-use code as a QR image and can be revoked", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": runDetail({ remotes: [{ id: 1, device_type: "phone", paired_at: "2026-11-12T01:05:00Z", last_seen_at: null }] }),
      "POST /api/runs/9/pairing": { pairing_id: 3, code: "c0ffee", expires_at: new Date(Date.now() + 600_000).toISOString() },
      "DELETE /api/runs/9/pairing": { ok: true, revoked_pairings: 1, revoked_remotes: 1 },
    });
    const user = userEvent.setup();
    await openConsole();
    await user.click(screen.getByRole("button", { name: "用手機當遙控器" }));
    expect(screen.getByText(/QR Code 不要投影給學生看/)).toBeInTheDocument();
    expect(screen.getByText("已連線 1 支")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "產生 QR Code" }));
    const qr = await screen.findByRole("img", { name: "手機遙控器配對 QR Code" });
    expect(qr.querySelector("path")?.getAttribute("d")).toMatch(/^M\d+,\d+h1v1h-1z/);
    expect(screen.getByText(/剩下 (9|10):\d\d（單次使用）/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "撤銷配對" }));
    expect(await screen.findByText("已撤銷：1 個 QR Code、1 支遙控器。")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "手機遙控器配對 QR Code" })).not.toBeInTheDocument();
    expect(scaffolds(api)).toContainEqual({ scaffold_code: "SC-RECORD", action: "expand", trigger: "qr" });
  });
});

describe("Picture-in-Picture", () => {
  it("is hidden when the browser has no Document PiP", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail() });
    await openConsole();
    expect(screen.queryByRole("button", { name: "子母畫面小視窗" })).not.toBeInTheDocument();
  });

  it("warns about mirrored projection before opening, then renders the buttons in the small window", async () => {
    signIn();
    const { server, handler } = syncRoute();
    const api = mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail(), [`POST ${SYNC}`]: handler });
    // A same-origin iframe stands in for the PiP window (a real Window with its own document).
    const frame = document.createElement("iframe");
    document.body.appendChild(frame);
    const pipWindow = frame.contentWindow as Window;
    const pipDoc = pipWindow.document;
    const close = vi.spyOn(pipWindow, "close").mockImplementation(() => undefined);
    const requestWindow = vi.fn(async () => pipWindow);
    (window as Window & { documentPictureInPicture?: unknown }).documentPictureInPicture = { requestWindow };
    const user = userEvent.setup();
    await openConsole();

    await user.click(screen.getByRole("button", { name: "子母畫面小視窗" }));
    expect(screen.getByText("鏡像投影會被學生看到")).toBeInTheDocument();
    expect(requestWindow).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "仍要開啟" }));
    expect(requestWindow).toHaveBeenCalledTimes(1);

    const pip = within(pipDoc.body);
    expect(requestWindow).toHaveBeenCalledWith({ width: 360, height: 300 });
    fireEvent.click(await pip.findByRole("button", { name: "卡住" }));
    await waitFor(() => expect(server.events.size).toBe(1));
    expect(opsSent(api)[0]).toMatchObject({ kind: "create", polarity: "stuck", via: "pip" });
    expect(scaffolds(api)).toContainEqual({ scaffold_code: "SC-RECORD", action: "expand", trigger: "pip" });
    // The main console shows the bookmark from the small window too.
    expect(await screen.findByRole("listitem", { name: /卡住$/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "關閉子母畫面小視窗" }));
    expect(close).toHaveBeenCalled();
    frame.remove();
  });

  it("can be cancelled at the warning", async () => {
    signIn();
    const requestWindow = vi.fn();
    (window as Window & { documentPictureInPicture?: unknown }).documentPictureInPicture = { requestWindow };
    mockApi({ ...baseRoutes(), "GET /api/runs/9": runDetail() });
    const user = userEvent.setup();
    await openConsole();
    await user.click(screen.getByRole("button", { name: "子母畫面小視窗" }));
    await user.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByText("鏡像投影會被學生看到")).not.toBeInTheDocument();
    expect(requestWindow).not.toHaveBeenCalled();
  });
});

describe("課後補寫", () => {
  const ended = runDetail({
    ended_at: "2026-11-12T01:50:00Z",
    good_count: 1,
    stuck_count: 1,
    events: [
      recordEvent("ev-2", "2026-11-12T01:20:00Z", { id: 12, polarity: "good" }),
      recordEvent("ev-1", "2026-11-12T01:10:00Z", { id: 11, polarity: "stuck", note: "例句太難" }),
    ],
  });

  it("lists bookmarks in time order next to the plan and sends optional review payloads", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": ended,
      "PUT /api/runs/9/events/ev-1/review": (call: MockCall) => ({
        ...recordEvent("ev-1", "2026-11-12T01:10:00Z", { id: 11, polarity: "stuck", note: "例句太難" }),
        ...(call.body as ReviewRequest),
        followup_at: "2026-11-12T02:00:00Z",
        revision_count: 1,
      }),
      "PUT /api/runs/9/events/ev-2/review": (call: MockCall) => ({
        ...recordEvent("ev-2", "2026-11-12T01:20:00Z", { id: 12 }),
        ...(call.body as ReviewRequest),
      }),
      "POST /api/runs/9/review/complete": run({
        ended_at: "2026-11-12T01:50:00Z",
        review_completed_at: "2026-11-12T02:05:00Z",
      }),
    });
    const user = userEvent.setup();
    renderApp("/runs/9/review");
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();

    // Plan steps alongside; bookmarks in device-time order.
    const plan = within(screen.getByRole("complementary", { name: "教案步驟" }));
    expect(plan.getByText("暖身")).toBeInTheDocument();
    expect(plan.getByText("改寫倒裝句")).toBeInTheDocument();
    const cards = screen.getAllByRole("article");
    expect(cards.map((c) => c.getAttribute("aria-label"))).toEqual([
      expect.stringMatching(/卡住$/),
      expect.stringMatching(/順利$/),
    ]);
    const first = within(cards[0] as HTMLElement);
    expect(first.getByText("「例句太難」")).toBeInTheDocument();
    expect(first.getByText(/不要寫學生姓名/)).toBeInTheDocument();
    // The privacy reminder is said once on the page, under the first card's field.
    expect(within(cards[1] as HTMLElement).queryByText(/不要寫學生姓名/)).not.toBeInTheDocument();
    expect(first.getByRole("button", { name: "儲存這張書籤" })).toBeDisabled();

    await user.selectOptions(first.getByLabelText("活動歸屬（選填）"), "步驟 2：兩兩討論");
    await user.click(first.getByRole("checkbox", { name: "討論" }));
    await user.click(first.getByRole("checkbox", { name: "聆聽" }));
    await user.click(first.getByRole("checkbox", { name: "其他" }));
    await user.click(first.getByRole("checkbox", { name: "其他" }));
    await user.click(first.getByRole("radio", { name: "補充例子" }));
    await user.type(first.getByLabelText("補寫（選填）"), "  多給兩個生活例句後就能改寫 ");
    await user.click(first.getByRole("button", { name: "儲存這張書籤" }));
    await waitFor(() => expect(api.callsTo("PUT", "/api/runs/9/events/ev-1/review")).toHaveLength(1));
    expect(api.lastBody("PUT", "/api/runs/9/events/ev-1/review")).toEqual({
      activity_step_id: 302,
      behaviors: ["listening", "discussing"],
      handling: "add_example",
      followup_text: "多給兩個生活例句後就能改寫",
    });
    expect(await first.findByText("已儲存")).toBeInTheDocument();

    // Everything left blank is a valid save too.
    const second = within(cards[1] as HTMLElement);
    await user.click(second.getByRole("radio", { name: "稍後處理" }));
    await user.click(second.getByRole("radio", { name: "未選" }));
    await user.click(second.getByRole("button", { name: "儲存這張書籤" }));
    await waitFor(() =>
      expect(api.lastBody("PUT", "/api/runs/9/events/ev-2/review")).toEqual({
        activity_step_id: null,
        behaviors: [],
        handling: null,
        followup_text: "",
      }),
    );

    // Completing never requires anything.
    await user.click(screen.getByRole("button", { name: "完成課後補寫" }));
    expect(await screen.findByText(/完成課後補寫。之後仍可回來修改/)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/runs/9/review/complete")).toHaveLength(1);
    expect(scaffolds(api)).toEqual([
      { scaffold_code: "SC-RECORD", action: "show", trigger: "review" },
      { scaffold_code: "SC-RECORD", action: "use", trigger: "review" },
      { scaffold_code: "SC-RECORD", action: "use", trigger: "review" },
    ]);
  });

  it("can be completed with no bookmarks at all", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": runDetail({ ended_at: "2026-11-12T01:50:00Z", plan: null }),
      "POST /api/runs/9/review/complete": run({
        ended_at: "2026-11-12T01:50:00Z",
        review_completed_at: "2026-11-12T02:05:00Z",
      }),
    });
    const user = userEvent.setup();
    renderApp("/runs/9/review");
    expect(await screen.findByText("這節課沒有書籤。沒有書籤也沒關係，可以直接完成。")).toBeInTheDocument();
    expect(screen.getByText("這節課還沒有教案步驟，活動歸屬可以留白。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "完成課後補寫" }));
    await waitFor(() => expect(api.callsTo("POST", "/api/runs/9/review/complete")).toHaveLength(1));
  });

  it("corrections on the review page go through the same queue", async () => {
    signIn();
    const { server, handler } = syncRoute();
    // Seed the fake server with what the run already has.
    server.apply([
      { op_id: "c1", kind: "create", device_event_id: "ev-1", device_ts: "2026-11-12T01:10:00Z", polarity: "stuck", note: "例句太難", device_type: "desktop" },
    ]);
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": runDetail({
        ended_at: "2026-11-12T01:50:00Z",
        events: [recordEvent("ev-1", "2026-11-12T01:10:00Z", { polarity: "stuck", note: "例句太難" })],
      }),
      [`POST ${SYNC}`]: handler,
    });
    const user = userEvent.setup();
    renderApp("/runs/9/review");
    const card = within(await screen.findByRole("article", { name: /卡住$/ }));
    await user.click(card.getByRole("button", { name: "改一句話" }));
    const input = card.getByLabelText("一句話");
    await user.clear(input);
    await user.type(input, "例句太長");
    await user.click(card.getByRole("button", { name: "儲存" }));
    await waitFor(() => expect(opsSent(api).at(-1)).toMatchObject({ kind: "edit", device_event_id: "ev-1", note: "例句太長" }));
    expect(await card.findByText("「例句太長」")).toBeInTheDocument();
  });
});

describe("課後補寫 never drops an unsaved bookmark choice (R2c, regression for docs/UX_REVIEW.md F2)", () => {
  const REVIEW_1 = "/api/runs/9/events/ev-1/review";
  const COMPLETE = "/api/runs/9/review/complete";
  const ended = runDetail({
    ended_at: "2026-11-12T01:50:00Z",
    events: [
      recordEvent("ev-1", "2026-11-12T01:10:00Z", { id: 11, polarity: "stuck", note: "例句太難" }),
      recordEvent("ev-2", "2026-11-12T01:20:00Z", { id: 12, polarity: "good" }),
    ],
  });
  const reviewed = (call: MockCall) => ({
    ...recordEvent("ev-1", "2026-11-12T01:10:00Z", { id: 11, polarity: "stuck", note: "例句太難" }),
    ...(call.body as ReviewRequest),
  });
  const completed = run({ ended_at: "2026-11-12T01:50:00Z", review_completed_at: "2026-11-12T02:05:00Z" });
  const index = (api: ReturnType<typeof mockApi>, method: string, path: string) =>
    api.calls.findIndex((c) => c.method === method && c.path === path);

  async function openReview() {
    const rendered = renderApp("/runs/9/review");
    const card = within(await screen.findByRole("article", { name: /卡住$/ }));
    return { ...rendered, card };
  }

  it("「完成課後補寫」 first saves a card the teacher did not save", async () => {
    signIn();
    const api = mockApi({ ...baseRoutes(), "GET /api/runs/9": ended, [`PUT ${REVIEW_1}`]: reviewed, [`POST ${COMPLETE}`]: completed });
    const user = userEvent.setup();
    const { card } = await openReview();

    // The reported case: pick 處理方式 重新解釋, do not press 儲存這張書籤, press 完成課後補寫.
    await user.click(card.getByRole("radio", { name: "重新解釋" }));
    expect(card.getByText("尚未儲存")).toBeInTheDocument();
    expect(screen.getByText("有 1 張書籤的補寫還沒儲存；按「完成課後補寫」會先把它們存起來。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "完成課後補寫" }));

    expect(await screen.findByText(/完成課後補寫。之後仍可回來修改/)).toBeInTheDocument();
    expect(api.lastBody("PUT", REVIEW_1)).toEqual({
      activity_step_id: null,
      behaviors: [],
      handling: "re_explain",
      followup_text: "",
    });
    expect(index(api, "PUT", REVIEW_1)).toBeLessThan(index(api, "POST", COMPLETE));
    // Only the edited card is saved; the untouched one is not re-sent.
    expect(api.callsTo("PUT", "/api/runs/9/events/ev-2/review")).toHaveLength(0);
    expect(await card.findByText("已儲存")).toBeInTheDocument();
  });

  it("does not complete when a pending card cannot be saved, and keeps the choice", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": ended,
      [`PUT ${REVIEW_1}`]: respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } }),
      [`POST ${COMPLETE}`]: completed,
    });
    const user = userEvent.setup();
    const { card } = await openReview();
    await user.click(card.getByRole("radio", { name: "重新解釋" }));
    await user.click(screen.getByRole("button", { name: "完成課後補寫" }));

    expect(
      await screen.findByText("1 張書籤的補寫沒有存成功（伺服器暫時發生問題），所以還沒完成課後補寫。請看該張書籤的訊息後再試一次。"),
    ).toBeInTheDocument();
    expect(card.getByText("伺服器暫時發生問題")).toBeInTheDocument();
    expect(api.callsTo("POST", COMPLETE)).toHaveLength(0);
    expect(card.getByRole("radio", { name: "重新解釋" })).toBeChecked();
  });

  it("asks before leaving with an unsaved card; 「儲存後離開」 saves it", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/runs/9": ended,
      [`PUT ${REVIEW_1}`]: reviewed,
      "GET /api/lessons/7": lessonDetail(),
      "GET /api/units/1/core": { id: 1, unit_id: 1, phase: "pre", version_no: 1, completed_at: null, saw_ai_assist: false, focuses: [] },
      "GET /api/siop": [],
      "GET /api/teacher-cases": [],
      "GET /api/lessons/7/runs": [],
    });
    const user = userEvent.setup();
    const { card, router } = await openReview();
    await user.type(card.getByLabelText("補寫（選填）"), "多給例句");

    const unload = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(unload);
    expect(unload.defaultPrevented).toBe(true);

    await user.click(screen.getByRole("link", { name: "第 1 課：Not only… but also" }));
    const dialog = within(await screen.findByRole("alertdialog", { name: "還有沒儲存的內容" }));
    expect(dialog.getByText("離開這頁，還沒儲存的書籤補寫就會遺失。")).toBeInTheDocument();
    await user.click(dialog.getByRole("button", { name: "留在這頁" }));
    expect(router.state.location.pathname).toBe("/runs/9/review");
    expect(card.getByLabelText("補寫（選填）")).toHaveValue("多給例句");

    await user.click(screen.getByRole("link", { name: "第 1 課：Not only… but also" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "儲存後離開" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/lessons/7"));
    expect(api.lastBody("PUT", REVIEW_1)).toMatchObject({ followup_text: "多給例句" });
  });
});

describe("starting a run", () => {
  it("開始上課 in the lesson page head opens the console", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/lessons/7": lessonDetail(),
      "GET /api/lessons/7/runs": [run({ id: 8, ended_at: "2026-11-11T02:00:00Z", good_count: 2, stuck_count: 1 })],
      "POST /api/lessons/7/runs": run(),
      "GET /api/runs/9": runDetail(),
    });
    const user = userEvent.setup();
    renderApp("/lessons/7");
    const start = await screen.findByRole("button", { name: "開始上課" });
    expect(start).toHaveClass("secondary-button");
    await user.click(start);
    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(1);
  });

  it("開始上課 on the home page starts in that class, and resumes on 409 when the lesson is already open", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/classes": [
        schoolClass({
          id: 3,
          next_action: { kind: "start_lesson", lesson_id: 7, lesson_title: "第 1 課", unit_id: 1, unit_title: "倒裝句單元" },
        }),
      ],
      "GET /api/runs": [run({ id: 5, lesson_title: "第 0 課", ended_at: "2026-11-10T02:00:00Z", review_completed_at: "2026-11-10T03:00:00Z" })],
      "POST /api/lessons/7/runs": respond(409, { error: { code: "conflict", message: "這節課已經開始了" } }),
      "GET /api/lessons/7/runs": [run({ class_id: 3, class_name: "高一 3 班" })],
      "GET /api/runs/9": runDetail(),
    });
    const user = userEvent.setup();
    renderApp("/classes");
    await user.click(await screen.findByRole("button", { name: "開始上課" }));
    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 3 });
    expect(api.callsTo("GET", "/api/lessons/7/runs")).toHaveLength(1);
  });

  it("the records page lists past lessons and offers no second way to start one", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/runs": [run({ id: 5, lesson_title: "第 0 課", ended_at: "2026-11-10T02:00:00Z", review_completed_at: "2026-11-10T03:00:00Z" })],
    });
    renderApp("/record");
    expect(await screen.findByRole("heading", { name: "上課紀錄", level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/已補寫/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /^回顧/ })).toHaveAttribute("href", "/runs/5/review");
    expect(screen.queryByRole("button", { name: /開始上課/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });
});

describe("phone remote", () => {
  afterEach(() => {
    window.history.replaceState(null, "", "/");
  });

  it("pairs from the QR link, then taps with the run-scoped token only", async () => {
    signIn("teacher-session-token");
    window.history.replaceState(null, "", "/remote#pair=c0ffee");
    const { server, handler } = syncRoute();
    const api = mockApi({
      ...baseRoutes(),
      "POST /api/remote/pair": {
        token: "remote-token",
        run: { run_id: 9, lesson_title: "第 1 課：Not only… but also", started_at: "2026-11-12T01:00:00Z", ended_at: null, events: [] },
      },
      "GET /api/remote/run": { run_id: 9, lesson_title: "第 1 課：Not only… but also", started_at: "2026-11-12T01:00:00Z", ended_at: null, events: [] },
      "POST /api/remote/events/sync": handler,
    });
    const user = userEvent.setup();
    renderApp("/remote");
    const good = await screen.findByRole("button", { name: "順利" });
    const pair = api.callsTo("POST", "/api/remote/pair")[0];
    expect(pair?.body).toEqual({ code: "c0ffee", device_type: "desktop" });
    expect(pair?.headers.Authorization).toBeUndefined();
    // The one-time code is gone from the address bar.
    expect(window.location.hash).toBe("");

    await user.click(good);
    await waitFor(() => expect(server.events.size).toBe(1));
    const call = api.callsTo("POST", "/api/remote/events/sync")[0];
    expect(call?.headers.Authorization).toBe("Bearer remote-token");
    expect(opsSent(api, "/api/remote/events/sync")[0]).not.toHaveProperty("via");
    expect(await screen.findByText(/已記錄 1 筆/)).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/remote/run")[0]?.headers.Authorization).toBe("Bearer remote-token");

    await user.click(screen.getByRole("button", { name: "復原上一筆" }));
    await waitFor(() => expect(server.events.size).toBe(0));
    expect(screen.queryByRole("button", { name: /改成/ })).not.toBeInTheDocument();
  });

  it("explains an expired QR code", async () => {
    window.history.replaceState(null, "", "/remote#pair=old");
    mockApi({
      ...baseRoutes(),
      "POST /api/remote/pair": respond(401, {
        error: { code: "unauthorized", message: "這個 QR Code 已過期（10 分鐘內有效），請在電腦上重新產生。" },
      }),
    });
    renderApp("/remote");
    expect(await screen.findByText(/這個 QR Code 已過期/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "順利" })).not.toBeInTheDocument();
  });
});
