// 學習單的圖片 (docs/API.md「學習單的圖片」): pictures after a block's instructions in the editor
// (/worksheets/:id) and in the 備課對話 preview, empty boxes with what they are for when there is
// no picture, 「AI 生成」 in the teacher version, and the teacher's 重畫 (with a note), 換成我的圖
// and 拿掉, polling while a picture is being made.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PrepSession,
  PrepTurn,
  WorksheetContent,
  WorksheetImages,
  WorksheetState,
  WsAudience,
  WsBlock,
  WsSheet,
  WsSlotImage,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

const WS = "/api/worksheets/5";

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
  items: [
    {
      id: `${id}-i1`,
      answer_format: "簡答題",
      prompt: `Question of ${id}`,
      options: [],
      answer: "",
      evidence: "",
      purpose: null,
      levels: ["standard"],
      origin: "ai",
    },
  ],
  scaffolds: [],
  teacher_notes: "",
  origin: "ai",
  message: null,
  sop_block_id: null,
  ...extra,
});

function content(): WorksheetContent {
  return {
    objectives: [],
    show_objectives: false,
    levels: ["standard"],
    chosen: null,
    notices: [],
    blocks: [
      block("b1", {
        title: "Paragraph 1: Find",
        instructions: "Read paragraph 1.",
        image_slots: [
          {
            id: "b1-p1",
            kind: "scene",
            purpose_zh: "幫助學生想像下雨的店門口",
            prompt_en: "A girl with an umbrella outside a small shop.",
            alt_en: "A girl with an umbrella outside a shop",
            caption: "Look at the umbrella stand.",
          },
        ],
      }),
      block("b2", {
        title: "Paragraph 2: Where",
        image_slots: [
          {
            id: "b2-p1",
            kind: "photo_needed",
            purpose_zh: "讓學生看出故事發生的城市",
            prompt_en: "A map of the town.",
            alt_en: "A map of the town",
            caption: "",
          },
          {
            id: "b2-p2",
            kind: "object",
            purpose_zh: "一把傘",
            prompt_en: "An umbrella.",
            alt_en: "An umbrella",
            caption: "",
          },
        ],
      }),
    ],
  };
}

const slotImage = (slot_id: string, block_id: string, extra: Partial<WsSlotImage>): WsSlotImage => ({
  slot_id,
  block_id,
  status: "done",
  source: "generated",
  image_id: null,
  width: 48,
  height: 32,
  message: null,
  updated_at: "2026-11-17T08:00:00Z",
  ...extra,
});

const images = (slots: WsSlotImage[], extra: Partial<WorksheetImages> = {}): WorksheetImages => ({
  enabled: true,
  regenerations_left: 3,
  slots,
  ...extra,
});

const initialImages = () =>
  images([
    slotImage("b1-p1", "b1", { image_id: 31 }),
    slotImage("b2-p1", "b2", {
      status: "skipped",
      source: "none",
      message: "這裡需要真實、準確的照片（地圖、國旗、真實人物或地標、歷史照片），Manna 不會自己畫；請上傳一張。",
    }),
    slotImage("b2-p2", "b2", { status: "failed", message: "產圖服務暫時無法使用（HTTP 503）；可以稍後按「重畫」，或上傳自己的圖。" }),
  ]);

function wsState(overrides: Partial<WorksheetState> = {}): WorksheetState {
  return {
    id: 5,
    title: "The Umbrella Stand",
    filename: "The Umbrella Stand_學習單",
    lesson: null,
    class_id: null,
    generated: true,
    levels: ["standard"],
    version_no: 2,
    created_at: "2026-11-17T08:00:00Z",
    updated_at: "2026-11-17T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 2,
    saved_at: "2026-11-17T08:00:00Z",
    content: content(),
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    images: initialImages(),
    ...overrides,
  };
}

/** Stored pictures answer a small JPEG, shown as `blob:img-<id>`. */
function stubPictures() {
  const mocked = globalThis.fetch;
  const urls = new Map<Blob, string>();
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const id = /\/api\/worksheets\/\d+\/images\/(\d+)$/.exec(String(input))?.[1];
    if (id === undefined) return mocked(input, init);
    await mocked(input, init);
    const blob = new Blob(["jpeg"], { type: "image/jpeg" });
    urls.set(blob, `blob:img-${id}`);
    return { ok: true, status: 200, blob: async () => blob, headers: { get: () => null } } as unknown as Response;
  });
  vi.stubGlobal(
    "URL",
    Object.assign(URL, { createObjectURL: (b: Blob) => urls.get(b) ?? "blob:unknown", revokeObjectURL: vi.fn() }),
  );
}

const region = (name: string) => within(screen.getByRole("region", { name: `區塊 ${name}` }));

beforeEach(() => {
  signIn();
  worksheetConfig.pollMs = 20;
  prepConfig.pollMs = 20;
  autosaveConfig.delayMs = 20;
});
afterEach(() => {
  worksheetConfig.pollMs = 2000;
  prepConfig.pollMs = 2000;
  autosaveConfig.delayMs = 1500;
  vi.unstubAllGlobals();
});

describe("the editor", () => {
  it("shows the picture after the instructions, and boxes with what they are for", async () => {
    const api = mockApi({ ...baseRoutes(), "GET /api/worksheets": [], [`GET ${WS}`]: wsState() });
    stubPictures();
    renderApp("/worksheets/5");
    const first = await screen.findByRole("figure", { name: "圖片 A" });
    await waitFor(() =>
      expect(within(first).getByRole("img", { name: "A girl with an umbrella outside a shop" })).toHaveAttribute(
        "src",
        "blob:img-31",
      ),
    );
    expect(api.callsTo("GET", `${WS}/images/31`)[0]?.headers.Authorization).toBe("Bearer test-token");
    expect(within(first).getByText("Look at the umbrella stand.")).toBeInTheDocument();
    expect(within(first).getByText("AI 生成")).toBeInTheDocument();
    expect(within(first).getByText("還能重畫 3 次")).toBeInTheDocument();
    // The picture comes after the instructions, before the questions.
    const a = region("A");
    const order = Array.from(a.getByRole("figure").parentElement?.children ?? []);
    expect(order.indexOf(first)).toBeGreaterThan(order.indexOf(a.getByRole("textbox", { name: /作答指示/ })));

    // The two of block B: a slot an older worksheet left for a real photo (skipped, with its
    // reason; it can be drawn now), and one that failed.
    const b = region("B");
    const [photo, failed] = b.getAllByRole("figure");
    expect(within(photo!).getByText("請放一張圖：讓學生看出故事發生的城市")).toBeInTheDocument();
    expect(within(photo!).getByText(/需要真實、準確的照片/)).toBeInTheDocument();
    expect(within(photo!).getByRole("button", { name: "重畫" })).toBeEnabled();
    expect(within(failed!).getByText(/HTTP 503/)).toBeInTheDocument();
    expect(within(failed!).getByRole("button", { name: "重畫" })).toBeEnabled();
    expect(within(failed!).queryByText("AI 生成")).not.toBeInTheDocument();
  });

  it("重畫 with a note: sends it, shows Manna drawing, and polls until the picture is there", async () => {
    let polls = 0;
    const queued = images(
      initialImages().slots.map((s) => (s.slot_id === "b2-p2" ? { ...s, status: "queued", message: null } : s)),
      { regenerations_left: 2 },
    );
    const done = images(
      initialImages().slots.map((s) =>
        s.slot_id === "b2-p2" ? { ...s, status: "done", image_id: 44, message: null } : s,
      ),
      { regenerations_left: 2 },
    );
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      [`GET ${WS}`]: wsState(),
      [`POST ${WS}/image-slots/b2-p2/regenerate`]: queued,
      [`GET ${WS}/images`]: () => (++polls < 2 ? queued : done),
    });
    stubPictures();
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const failed = within((await screen.findAllByRole("figure", { name: "圖片 B" }))[1]!);
    await user.click(failed.getByRole("button", { name: "重畫" }));
    await user.type(failed.getByRole("textbox", { name: "想怎麼改（可以不寫）" }), "畫成晴天");
    await user.click(failed.getByRole("button", { name: "送出重畫" }));
    await waitFor(() =>
      expect(api.lastBody("POST", `${WS}/image-slots/b2-p2/regenerate`)).toEqual({ hint: "畫成晴天" }),
    );
    expect(await screen.findByText("Manna 正在畫這張圖…")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("img", { name: "An umbrella" })).toHaveAttribute("src", "blob:img-44"));
    expect(screen.getAllByText("還能重畫 2 次").length).toBeGreaterThan(0);
    expect(api.callsTo("GET", `${WS}/images`).length).toBeGreaterThanOrEqual(2);
  });

  it("換成我的圖 uploads the file, 拿掉 removes the picture, and a refusal is shown", async () => {
    const uploaded = images(
      initialImages().slots.map((s) =>
        s.slot_id === "b2-p1" ? { ...s, status: "done", source: "upload", image_id: 50, message: null } : s,
      ),
    );
    const removed = images(
      uploaded.slots.map((s) => (s.slot_id === "b1-p1" ? { ...s, status: "removed", source: "none", image_id: null } : s)),
    );
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      [`GET ${WS}`]: wsState(),
      [`POST ${WS}/image-slots/b2-p1/upload`]: uploaded,
      [`DELETE ${WS}/image-slots/b1-p1`]: removed,
      [`POST ${WS}/image-slots/b1-p1/regenerate`]: respond(409, {
        error: { code: "conflict", message: "這份學習單的圖已經重畫 10 次，到上限了；可以上傳自己的圖。" },
      }),
    });
    stubPictures();
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await screen.findAllByRole("figure", { name: "圖片 B" });

    const file = new File(["png"], "map.png", { type: "image/png" });
    const photo = within(screen.getAllByRole("figure", { name: "圖片 B" })[0]!);
    await user.upload(photo.getByLabelText("換成我的圖 B"), file);
    await waitFor(() => expect(api.callsTo("POST", `${WS}/image-slots/b2-p1/upload`)).toHaveLength(1));
    const sent = api.fetchMock.mock.calls.find(([url]) => String(url).endsWith("/b2-p1/upload"))?.[1]?.body;
    expect(sent).toBeInstanceOf(FormData);
    expect((sent as FormData).get("file")).toBe(file);
    await waitFor(() =>
      expect(screen.getByRole("img", { name: "A map of the town" })).toHaveAttribute("src", "blob:img-50"),
    );
    // The teacher's own picture is not labelled as AI-made.
    const mine = screen.getByRole("img", { name: "A map of the town" });
    expect(within(mine.closest("figure")!).queryByText("AI 生成")).not.toBeInTheDocument();

    // A refusal from the server is said next to the picture.
    const first = within(screen.getByRole("figure", { name: "圖片 A" }));
    await user.click(first.getByRole("button", { name: "重畫" }));
    await user.click(first.getByRole("button", { name: "送出重畫" }));
    expect(await first.findByRole("alert")).toHaveTextContent("到上限了");
    expect(api.lastBody("POST", `${WS}/image-slots/b1-p1/regenerate`)).toEqual({});
    await user.click(first.getByRole("button", { name: "取消" }));

    await user.click(first.getByRole("button", { name: "拿掉" }));
    await waitFor(() => expect(api.callsTo("DELETE", `${WS}/image-slots/b1-p1`)).toHaveLength(1));
    expect(await screen.findByText("這一區的圖已經拿掉了（不會印出）。")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "A girl with an umbrella outside a shop" })).not.toBeInTheDocument();
    // Picture actions never save the worksheet.
    expect(api.callsTo("PUT", WS)).toHaveLength(0);
  });

  it("keeps the slots when the teacher's edits are saved", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      [`GET ${WS}`]: wsState(),
      [`PUT ${WS}`]: (call: MockCall) => ({ ...wsState(), ...(call.body as object) }),
    });
    stubPictures();
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const title = await screen.findByRole("textbox", { name: "區塊標題 A" });
    await user.type(title, "!");
    await waitFor(() => expect(api.callsTo("PUT", WS).length).toBeGreaterThan(0), { timeout: 4000 });
    const body = api.lastBody("PUT", WS) as { content: WorksheetContent };
    expect(body.content.blocks[0]?.image_slots?.[0]?.id).toBe("b1-p1");
    expect(body.content.blocks[1]?.image_slots).toHaveLength(2);
  });

  it("with pictures off, a slot offers an upload but no 重畫", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      [`GET ${WS}`]: wsState({ images: images(initialImages().slots, { enabled: false }) }),
    });
    stubPictures();
    renderApp("/worksheets/5");
    const failed = within((await screen.findAllByRole("figure", { name: "圖片 B" }))[1]!);
    expect(failed.queryByRole("button", { name: "重畫" })).not.toBeInTheDocument();
    expect(failed.getByText("這個環境沒有開啟 AI 產圖，可以上傳自己的圖。")).toBeInTheDocument();
    expect(failed.getByLabelText("換成我的圖 B")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- 備課對話

const S7 = "/api/prep-sessions/7";

const prepTurn: PrepTurn = {
  id: 301,
  seq: 1,
  kind: "worksheet",
  request: "做學習單",
  status: "done",
  error: null,
  worksheet: { id: 11, title: "U", version_no: 2, running: false },
  result: null,
  interaction_id: null,
  created_at: "2026-11-17T08:00:00Z",
  finished_at: "2026-11-17T08:01:00Z",
};

const prepSession: PrepSession = {
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: 1,
  last_kind: "worksheet",
  updated_at: "2026-11-17T09:00:00Z",
  created_at: "2026-11-17T08:00:00Z",
  turns: [prepTurn],
  running: false,
  next: [],
};

function prepSheet(audience: WsAudience, slotStatus: "done" | "queued" = "done"): WsSheet {
  const teacher = audience === "teacher";
  return {
    title: "The Umbrella Stand",
    level: "standard",
    audience,
    objectives: [],
    blocks: [
      {
        id: "b1",
        kind: "activity",
        label: "A",
        title: "Paragraph 1: Find",
        instructions: "Read paragraph 1.",
        steps: [],
        vocabulary: [],
        supports: [],
        items: [{ no: 1, answer_format: "簡答題", prompt: "Why?", options: [], lines: 2 }],
        images: [
          {
            slot_id: "b1-p1",
            status: slotStatus,
            image_id: slotStatus === "done" ? 31 : null,
            width: 48,
            height: 32,
            alt: "A girl with an umbrella",
            caption: "Look at the umbrella stand.",
            ai_generated: slotStatus === "done",
            ...(teacher ? { purpose: "幫助學生想像下雨的店門口", message: null } : {}),
          },
          {
            slot_id: "b1-p2",
            status: "failed",
            image_id: null,
            width: null,
            height: null,
            alt: "A map",
            caption: "",
            ai_generated: false,
            ...(teacher ? { purpose: "讓學生看出故事發生的城市", message: "產圖逾時" } : {}),
          },
        ],
      },
    ],
  };
}

const prepWorksheet = (pictures: WorksheetImages): WorksheetState => ({
  ...wsState({ id: 11, images: pictures }),
});

describe("the 備課對話 preview", () => {
  it("prints the pictures; the boxes say what they are for only in the teacher version", async () => {
    const pictures = images([
      slotImage("b1-p1", "b1", { image_id: 31 }),
      slotImage("b1-p2", "b1", { status: "failed", message: "產圖逾時" }),
    ]);
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: prepSession,
      "GET /api/worksheets/11": prepWorksheet(pictures),
      "GET /api/worksheets/11/sheet": (call: MockCall) =>
        prepSheet(new URL(call.url, "http://x").searchParams.get("audience") as WsAudience),
    });
    stubPictures();
    const user = userEvent.setup();
    renderApp("/prep/7");
    const preview = await screen.findByRole("article", { name: "一般版 學生版" });
    await waitFor(() =>
      expect(within(preview).getByRole("img", { name: "A girl with an umbrella" })).toHaveAttribute(
        "src",
        "blob:img-31",
      ),
    );
    expect(api.callsTo("GET", "/api/worksheets/11/images/31")).not.toHaveLength(0);
    expect(within(preview).getByText("Look at the umbrella stand.")).toBeInTheDocument();
    // Student version: an empty box, no purpose, no 「AI 生成」; the teacher looking at it still
    // sees why the box is empty (the printed student sheet never does).
    expect(within(preview).getByRole("img", { name: "A map" })).toBeInTheDocument();
    expect(within(preview).queryByText(/請放一張圖/)).not.toBeInTheDocument();
    expect(within(preview).getByText("產圖逾時")).toBeInTheDocument();
    expect(within(preview).queryByText("AI 生成")).not.toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: "教師版" }));
    const teacher = await screen.findByRole("article", { name: "一般版 教師版" });
    expect(await within(teacher).findByText("請放一張圖：讓學生看出故事發生的城市")).toBeInTheDocument();
    expect(within(teacher).getByText("產圖逾時")).toBeInTheDocument();
    expect(within(teacher).getByText("AI 生成")).toBeInTheDocument();
  });

  it("asks for pictures still being made, then shows them; 重畫 works here too", async () => {
    let polls = 0;
    const queued = images([slotImage("b1-p1", "b1", { status: "queued" }), slotImage("b1-p2", "b1", { status: "failed" })]);
    const done = images([slotImage("b1-p1", "b1", { image_id: 31 }), slotImage("b1-p2", "b1", { status: "failed" })]);
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: prepSession,
      "GET /api/worksheets/11": prepWorksheet(queued),
      "GET /api/worksheets/11/sheet": () => prepSheet("student", polls < 2 ? "queued" : "done"),
      "GET /api/worksheets/11/images": () => (++polls < 2 ? queued : done),
      "POST /api/worksheets/11/image-slots/b1-p2/regenerate": images([
        slotImage("b1-p1", "b1", { image_id: 31 }),
        slotImage("b1-p2", "b1", { status: "queued" }),
      ]),
    });
    stubPictures();
    const user = userEvent.setup();
    renderApp("/prep/7");
    const preview = await screen.findByRole("article", { name: "一般版 學生版" });
    expect(await within(preview).findByText("Manna 正在畫這張圖…")).toBeInTheDocument();
    await waitFor(() =>
      expect(within(preview).getByRole("img", { name: "A girl with an umbrella" })).toHaveAttribute(
        "src",
        "blob:img-31",
      ),
    );
    // The sheet is read again once the pictures are done.
    await waitFor(() => expect(api.callsTo("GET", "/api/worksheets/11/sheet").length).toBeGreaterThanOrEqual(2));

    const box = within(within(preview).getAllByRole("figure")[1]!);
    await user.click(box.getByRole("button", { name: "重畫" }));
    await user.click(box.getByRole("button", { name: "送出重畫" }));
    await waitFor(() =>
      expect(api.callsTo("POST", "/api/worksheets/11/image-slots/b1-p2/regenerate")).toHaveLength(1),
    );
  });
});
