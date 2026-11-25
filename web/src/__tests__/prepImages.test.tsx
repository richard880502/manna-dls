// 備課對話：附上圖片（composer）與圖片動作（提案、回覆）。
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PictureOutcome,
  PrepImage,
  PrepSession,
  PrepTurn,
  WorksheetContent,
  WorksheetState,
  WsAudience,
  WsBlock,
  WsItem,
  WsPictureAction,
  WsProposal,
  WsSheet,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { blockReviseConfig } from "../lib/blockRevise";
import { prepConfig } from "../lib/prep";
import { IMAGE_PRIVACY_NOTE } from "../lib/prepImages";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

beforeEach(() => {
  signIn();
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
  blockReviseConfig.pollMs = 20;
  autosaveConfig.delayMs = 20;
});
afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
  blockReviseConfig.pollMs = 2000;
  autosaveConfig.delayMs = 1500;
});

// ---------------------------------------------------------------- fixtures

const WS = "/api/worksheets/11";

const item = (id: string, prompt: string, extra: Partial<WsItem> = {}): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: `Answer to ${id}`,
  evidence: "",
  purpose: null,
  levels: ["standard"],
  origin: "ai",
  ...extra,
});

const block = (id: string, extra: Partial<WsBlock>): WsBlock => ({
  id,
  kind: "activity",
  enabled: true,
  title: "",
  instructions: "",
  paragraph_ids: [],
  info_function: null,
  student_tasks: [],
  objective_ids: [],
  vocabulary: [],
  items: [],
  scaffolds: [],
  teacher_notes: "",
  origin: "ai",
  message: null,
  sop_block_id: null,
  ...extra,
});

const B2_ITEMS = [
  item("b2-1", "Why did Mrs. Lin start the stand?"),
  item("b2-2", "What did the neighbors expect would happen?"),
];

const content = (b2Items: WsItem[] = B2_ITEMS): WorksheetContent => ({
  objectives: [],
  show_objectives: false,
  levels: ["standard"],
  chosen: null,
  notices: [],
  blocks: [
    block("w", { kind: "warmup", title: "Warm-up", items: [item("w-1", "Have you ever borrowed an umbrella?")] }),
    block("b2", { title: "Paragraph 1: Find", instructions: "Read paragraph 1.", items: b2Items }),
  ],
});

const worksheet = (overrides: Partial<WorksheetState> = {}): WorksheetState => ({
  id: 11,
  title: "The Umbrella Stand",
  filename: "The Umbrella Stand_學習單",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: 3,
  created_at: "2026-11-15T08:00:00Z",
  updated_at: "2026-11-15T08:00:00Z",
  material: {
    title: "The Umbrella Stand",
    paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
    source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 },
  },
  settings: emptySettings(),
  suggested: emptySettings(),
  brief: emptyBrief(),
  versions_count: 3,
  saved_at: "2026-11-15T08:00:00Z",
  content: content(),
  jobs: [],
  running: false,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
  ...overrides,
});

const sheet = (audience: WsAudience, b2Items: WsItem[] = B2_ITEMS): WsSheet => ({
  title: "The Umbrella Stand",
  level: "standard",
  audience,
  objectives: [],
  blocks: [
    {
      id: "w",
      kind: "warmup",
      label: "A",
      title: "Warm-up",
      instructions: "",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ id: "w-1", no: 1, answer_format: "簡答題", prompt: "Have you ever borrowed an umbrella?", options: [], lines: 2 }],
    },
    {
      id: "b2",
      kind: "activity",
      label: "B",
      title: "Paragraph 1: Find",
      instructions: "Read paragraph 1.",
      steps: [],
      vocabulary: [],
      supports: [],
      items: b2Items.map((it, k) => ({
        id: it.id,
        no: k + 2,
        answer_format: it.answer_format,
        prompt: it.prompt,
        options: [],
        lines: 3,
      })),
    },
  ],
});

const REVISED = [B2_ITEMS[0]!, item("b2-2", "Did the neighbors think the umbrellas would stay?", { origin: "ai" })];

const proposal = (status: WsProposal["status"], overrides: Partial<WsProposal> = {}): WsProposal => {
  const before = content().blocks[1]!;
  const proposed = status === "running" || status === "failed" ? null : { ...before, items: REVISED };
  return {
    id: 90,
    worksheet_id: 11,
    block_id: "b2",
    status,
    instruction: "",
    comment_ids: [41],
    whole_block: false,
    round: 1,
    reply: status === "proposed" ? "已把第 3 題改簡單。" : "",
    error: status === "failed" ? "Manna 沒有改動這一區：你想改成哪一種題型？" : null,
    base_version_no: 3,
    before,
    block: proposed,
    changes: proposed
      ? {
          title: false,
          instructions: false,
          items: [
            { id: "b2-1", change: "kept" },
            { id: "b2-2", change: "modified" },
          ],
        }
      : null,
    interaction_id: 300,
    accepted_version_no: null,
    created_at: "2026-11-15T09:00:00Z",
    finished_at: null,
    decided_at: null,
    ...overrides,
  };
};

const wsTurn = (): PrepTurn => ({
  id: 101,
  seq: 1,
  kind: "worksheet",
  request: "做學習單",
  status: "done",
  error: null,
  worksheet: { id: 11, title: "The Umbrella Stand", version_no: 3, running: false },
  result: null,
  interaction_id: null,
  created_at: "2026-11-15T08:00:00Z",
  finished_at: "2026-11-15T08:01:00Z",
});

const session = (): PrepSession => ({
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
  turns: [wsTurn()],
  running: false,
  next: [],
});

const sheetRoute = (items?: WsItem[]) => (call: MockCall) =>
  sheet(new URL(call.url, "http://x").searchParams.get("audience") as WsAudience, items);

const IMG = (id: number): PrepImage => ({ id, session_id: 7, width: 640, height: 480, bytes: 2000 });
const png = (name = "bee.png", type = "image/png") => new File(["x"], name, { type });

/** Stored attached images answer a small JPEG, shown as `blob:prep-<id>`. */
function stubImages() {
  const mocked = globalThis.fetch;
  const urls = new Map<Blob, string>();
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const id = /\/api\/prep-sessions\/\d+\/images\/(\d+)$/.exec(String(input))?.[1];
    if (id === undefined || (init?.method ?? "GET") !== "GET") return mocked(input, init);
    const blob = new Blob(["jpeg"], { type: "image/jpeg" });
    urls.set(blob, `blob:prep-${id}`);
    return { ok: true, status: 200, blob: async () => blob, headers: { get: () => null } } as unknown as Response;
  });
  vi.stubGlobal(
    "URL",
    Object.assign(URL, {
      createObjectURL: (b: Blob) => urls.get(b) ?? "blob:local",
      revokeObjectURL: vi.fn(),
    }),
  );
}

afterEach(() => vi.unstubAllGlobals());

let nextImage = 100;
const pageRoutes = (extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  "GET /api/prep-sessions/7": session(),
  [`GET ${WS}`]: worksheet(),
  [`GET ${WS}/sheet`]: sheetRoute(),
  [`GET ${WS}/comments`]: [],
  [`GET ${WS}/proposals`]: [],
  "POST /api/prep-sessions/7/images": () => IMG(nextImage++),
  "DELETE /api/prep-sessions/7/images/100": { ok: true },
  ...extra,
});

const bar = () => screen.findByRole("textbox", { name: "給 Manna 的話" });
const fileInput = () => screen.getByTestId("prep-attach-input");

describe("the composer: attaching pictures", () => {
  beforeEach(() => {
    nextImage = 100;
  });

  it("has 附上圖片 when there is a worksheet, and no mode toggle", async () => {
    mockApi(pageRoutes());
    renderApp("/prep/7");
    await bar();
    expect(screen.getByRole("button", { name: "附上圖片" })).toBeInTheDocument();
    expect(fileInput()).toHaveAttribute("accept", "image/png,image/jpeg,image/webp");
    expect(fileInput()).toHaveAttribute("multiple");
    expect(screen.queryByRole("radio", { name: "問 Manna" })).not.toBeInTheDocument();
  });

  it("a chosen file uploads and shows a thumbnail; the send carries image_ids and clears it", async () => {
    const api = mockApi(pageRoutes({ "POST /api/prep-sessions/7/turns": { ...session(), running: true } }));
    const user = userEvent.setup();
    renderApp("/prep/7");
    const box = await bar();
    await user.upload(fileInput(), png());
    expect(await screen.findByRole("img", { name: "bee.png" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("上傳中")).not.toBeInTheDocument());
    expect(api.callsTo("POST", "/api/prep-sessions/7/images")).toHaveLength(1);
    const send = screen.getByRole("button", { name: "送出" });
    expect(send).toBeDisabled();
    await user.type(box, "照這張圖加一張圖");
    expect(send).toBeEnabled();
    await user.click(send);
    await waitFor(() => expect(api.callsTo("POST", "/api/prep-sessions/7/turns")).toHaveLength(1));
    expect(api.lastBody("POST", "/api/prep-sessions/7/turns")).toEqual({
      kind: "chat",
      request: "照這張圖加一張圖",
      image_ids: [100],
    });
    await waitFor(() => expect(screen.queryByRole("img", { name: "bee.png" })).not.toBeInTheDocument());
  });

  it("shows the privacy note only once a picture is attached", async () => {
    mockApi(pageRoutes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await bar();
    expect(screen.queryByText(IMAGE_PRIVACY_NOTE)).not.toBeInTheDocument();
    // It is still reachable before that, on the attach button.
    expect(screen.getByRole("button", { name: "附上圖片" })).toHaveAttribute("title", expect.stringContaining(IMAGE_PRIVACY_NOTE));
    await user.upload(fileInput(), png());
    expect(await screen.findByText(IMAGE_PRIVACY_NOTE)).toBeInTheDocument();
  });

  it("disables send while an upload is in flight", async () => {
    let finish: (v: unknown) => void = () => undefined;
    mockApi(pageRoutes({ "POST /api/prep-sessions/7/images": () => new Promise((r) => (finish = r)) }));
    const user = userEvent.setup();
    renderApp("/prep/7");
    const box = await bar();
    await user.type(box, "加圖");
    await user.upload(fileInput(), png());
    expect(await screen.findByText("上傳中")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送出" })).toBeDisabled();
    finish(IMG(100));
    await waitFor(() => expect(screen.getByRole("button", { name: "送出" })).toBeEnabled());
  });

  it("accepts a pasted picture and a dropped one", async () => {
    const api = mockApi(pageRoutes());
    renderApp("/prep/7");
    const box = await bar();
    fireEvent.paste(box, { clipboardData: { files: [png("pasted.png")], items: [], types: ["Files"] } });
    expect(await screen.findByRole("img", { name: "pasted.png" })).toBeInTheDocument();

    const composer = box.closest(".prep-composer") as HTMLElement;
    // Dragging over must be allowed (preventDefault) and lights the box.
    const notCancelled = fireEvent.dragOver(composer, { dataTransfer: { types: ["Files"], files: [] } });
    expect(notCancelled).toBe(false);
    expect(composer).toHaveAttribute("data-dragging");
    fireEvent.drop(composer, { dataTransfer: { types: ["Files"], files: [png("dropped.png")] } });
    expect(await screen.findByRole("img", { name: "dropped.png" })).toBeInTheDocument();
    expect(composer).not.toHaveAttribute("data-dragging");
    expect(api.callsTo("POST", "/api/prep-sessions/7/images")).toHaveLength(2);
  });

  it("the remove button deletes the uploaded picture and drops the thumbnail", async () => {
    const api = mockApi(pageRoutes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await bar();
    await user.upload(fileInput(), png());
    await waitFor(() => expect(screen.queryByText("上傳中")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "移除圖片 bee.png" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/prep-sessions/7/images/100")).toHaveLength(1));
    expect(screen.queryByRole("img", { name: "bee.png" })).not.toBeInTheDocument();
  });

  it("says why a file is refused, before uploading", async () => {
    const api = mockApi(pageRoutes());
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/prep/7");
    await bar();
    await user.upload(fileInput(), new File(["x"], "notes.pdf", { type: "application/pdf" }));
    expect(await screen.findByText("只能附 PNG、JPEG 或 WebP 圖片。")).toBeInTheDocument();
    const big = new File([new Uint8Array(5 * 1024 * 1024 + 1)], "huge.png", { type: "image/png" });
    await user.upload(fileInput(), big);
    expect(await screen.findByText("圖片最大 5 MB，「huge.png」超過了。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/prep-sessions/7/images")).toHaveLength(0);
  });

  it("allows two pictures at most", async () => {
    const api = mockApi(pageRoutes());
    const user = userEvent.setup();
    renderApp("/prep/7");
    await bar();
    await user.upload(fileInput(), [png("a.png"), png("b.png")]);
    expect(await screen.findByRole("img", { name: "b.png" })).toBeInTheDocument();
    await user.upload(fileInput(), png("c.png"));
    expect(await screen.findByText("一則訊息最多附 2 張圖片。")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "c.png" })).not.toBeInTheDocument();
    expect(api.callsTo("POST", "/api/prep-sessions/7/images")).toHaveLength(2);
  });

  it("shows the server's message when an upload fails, and drops the thumbnail", async () => {
    mockApi(
      pageRoutes({ "POST /api/prep-sessions/7/images": respond(400, { error: { code: "bad", message: "這張圖打不開。" } }) }),
    );
    const user = userEvent.setup();
    renderApp("/prep/7");
    await bar();
    await user.upload(fileInput(), png());
    expect(await screen.findByText("這張圖打不開。")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "bee.png" })).not.toBeInTheDocument();
  });

  it("sends image_ids with 改這一區 too", async () => {
    const api = mockApi(pageRoutes({ [`POST ${WS}/blocks/b2/revise`]: proposal("running", { comment_ids: [] }) }));
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "改這一區（區塊 B）" }));
    await user.upload(fileInput(), png());
    await waitFor(() => expect(screen.queryByText("上傳中")).not.toBeInTheDocument());
    await user.type(await bar(), "照圖畫{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${WS}/blocks/b2/revise`)).toHaveLength(1));
    expect(api.lastBody("POST", `${WS}/blocks/b2/revise`)).toEqual({ instruction: "照圖畫", image_ids: [100] });
  });
});

describe("the conversation shows attached pictures", () => {
  it("small thumbnails under the bubble; a click opens a larger view that Escape closes", async () => {
    const turn: PrepTurn = {
      ...wsTurn(),
      id: 102,
      seq: 2,
      kind: "revise",
      request: "照這張圖加一張",
      result: { reply: "好。", changed_block_ids: [], version_no: null },
      images: [IMG(5), IMG(6)],
    };
    mockApi(pageRoutes({ "GET /api/prep-sessions/7": { ...session(), turns: [wsTurn(), turn] } }));
    stubImages();
    const user = userEvent.setup();
    renderApp("/prep/7");
    const first = await screen.findByRole("img", { name: "附上的圖片 1" });
    expect(first).toHaveAttribute("src", "blob:prep-5");
    expect(screen.getByRole("img", { name: "附上的圖片 2" })).toHaveAttribute("src", "blob:prep-6");
    await user.click(first);
    const dialog = screen.getByRole("dialog", { name: "附上的圖片" });
    expect(within(dialog).getByRole("button", { name: "關閉" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(first);
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "關閉" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("a revise reply lists the picture outcomes", async () => {
    const pictures: PictureOutcome[] = [
      { action: "add", text: "加一張圖：蜜蜂停在花上", status: "queued", message: null },
      { action: "remove", text: "移除圖：舊的雨傘", status: "removed", message: null },
      { action: "redraw", text: "重畫第 2 張圖：不要有文字", status: "skipped", message: "這一區沒有第 2 張圖。" },
    ];
    const turn: PrepTurn = {
      ...wsTurn(),
      id: 102,
      seq: 2,
      kind: "revise",
      request: "加圖",
      result: { reply: "已處理圖片。", changed_block_ids: [], version_no: null, pictures },
    };
    mockApi(pageRoutes({ "GET /api/prep-sessions/7": { ...session(), turns: [wsTurn(), turn] } }));
    renderApp("/prep/7");
    const list = within(await screen.findByRole("list", { name: "圖片" }));
    expect(list.getByText("加一張圖：蜜蜂停在花上")).toBeInTheDocument();
    expect(list.getByText("：已排入繪圖，畫好會自動出現")).toBeInTheDocument();
    expect(list.getByText("移除圖：舊的雨傘")).toBeInTheDocument();
    expect(list.getByText("重畫第 2 張圖：不要有文字")).toBeInTheDocument();
    expect(list.getByText("：這一區沒有第 2 張圖。")).toBeInTheDocument();
  });
});

describe("picture actions in a block proposal", () => {
  const actions: WsPictureAction[] = [
    { action: "add", block_id: "b2", slot_id: "s1", text: "加一張圖：蜜蜂停在花上" },
    { action: "redraw", block_id: "b2", slot_id: "s2", text: "重畫第 2 張圖：不要有文字" },
    { action: "remove", block_id: "b2", slot_id: "s3", text: "移除圖：舊的雨傘" },
  ];

  it("lists them under 圖片, and a proposal of only pictures can be adopted; outcomes show after", async () => {
    let accepted = false;
    const only = proposal("proposed", {
      block: null,
      changes: null,
      reply: "我加了圖片。",
      picture_actions: actions,
      images: [IMG(8)],
    });
    const api = mockApi(
      pageRoutes({
        [`GET ${WS}/proposals`]: () => (accepted ? [] : [only]),
        [`POST ${WS}/proposals/90/accept`]: () => {
          accepted = true;
          return {
            proposal: proposal("accepted"),
            worksheet: worksheet({ version_no: 4 }),
            pictures: [
              { action: "add", text: actions[0]!.text, status: "queued", message: null },
              { action: "redraw", text: actions[1]!.text, status: "skipped", message: "這張圖已經在畫了。" },
              { action: "remove", text: actions[2]!.text, status: "removed", message: null },
            ] satisfies PictureOutcome[],
          };
        },
      }),
    );
    stubImages();
    const user = userEvent.setup();
    renderApp("/prep/7");
    const panel = within(await screen.findByRole("region", { name: "Manna 的修改建議" }));
    const group = within(panel.getByRole("group", { name: "圖片" }));
    expect(group.getByText("圖片")).toBeInTheDocument();
    for (const a of actions) expect(group.getByText(a.text)).toBeInTheDocument();
    expect(group.getByText("按「採用」後才會畫圖或移除圖片。")).toBeInTheDocument();
    expect(await panel.findByRole("img", { name: "附上的圖片 1" })).toHaveAttribute("src", "blob:prep-8");
    await user.click(panel.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/proposals/90/accept`)).toHaveLength(1));
    const status = within(await screen.findByRole("status", { name: "圖片的結果" }));
    expect(status.getByText("加一張圖：蜜蜂停在花上")).toBeInTheDocument();
    expect(status.getByText("：已排入繪圖，畫好會自動出現")).toBeInTheDocument();
    expect(status.getByText("：這張圖已經在畫了。")).toBeInTheDocument();
    expect(status.getByText("：已移除")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Manna 的修改建議" })).not.toBeInTheDocument();
  });

  it("a proposal with text changes and pictures shows both", async () => {
    mockApi(pageRoutes({ [`GET ${WS}/proposals`]: [proposal("proposed", { picture_actions: actions })] }));
    renderApp("/prep/7");
    const panel = within(await screen.findByRole("region", { name: "Manna 的修改建議" }));
    expect(panel.getByText("Did the neighbors think the umbrellas would stay?")).toBeInTheDocument();
    expect(panel.getByRole("group", { name: "圖片" })).toBeInTheDocument();
  });

  it("an old proposal without picture_actions renders as before", async () => {
    mockApi(pageRoutes({ [`GET ${WS}/proposals`]: [proposal("proposed")] }));
    renderApp("/prep/7");
    const panel = within(await screen.findByRole("region", { name: "Manna 的修改建議" }));
    expect(panel.queryByRole("group", { name: "圖片" })).not.toBeInTheDocument();
  });
});
