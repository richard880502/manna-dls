// 長課文與「回上一步」: the long-material notice (修改課文 / 只選要用的段落 / 照樣產生), the way back
// from a warning or a failed turn with nothing lost (the home box, browser Back), editing the text
// and choosing paragraphs for one conversation or worksheet.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type {
  Material,
  MaterialDetail,
  PrepKind,
  PrepSession,
  PrepTurn,
  WorksheetState,
} from "../api/types";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import {
  baseRoutes,
  cardOf,
  emptyLibrary,
  materialBrowse,
  mockApi,
  renderApp,
  respond,
  signIn,
} from "../test/mockApi";

const HOME = "今天想準備什麼？";
const SESSIONS = "/api/prep-sessions";
const S7 = `${SESSIONS}/7`;
const LONG_MESSAGE = "Manna 沒有做完：輸出太長被截斷了。";

const material = (overrides: Partial<Material> = {}): Material => ({
  id: 5,
  kind: "document",
  origin: "upload",
  title: "The Umbrella Stand",
  source_note: "",
  url: null,
  filename: "umbrella.docx",
  content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  size_bytes: 2048,
  sha256: "ab".repeat(32),
  has_file: true,
  text_status: "ready",
  text_truncated: false,
  page_count: 1,
  paragraph_count: 52,
  word_count: 3000,
  revision: 1,
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  lessons: [],
  worksheet_count: 0,
  annotation_count: 0,
  ...overrides,
});

const detail = (n: number, overrides: Partial<MaterialDetail> = {}): MaterialDetail => ({
  ...material({ paragraph_count: n, origin: "paste", kind: "text", filename: null, has_file: false }),
  text: Array.from({ length: n }, (_, i) => `Paragraph ${i + 1} is about umbrellas.`).join("\n\n"),
  paragraphs: Array.from({ length: n }, (_, i) => ({ id: `p${i + 1}`, no: i + 1, text: `Paragraph ${i + 1} is about umbrellas.` })),
  ...overrides,
});

let turnId = 300;
const turn = (kind: PrepKind, overrides: Partial<PrepTurn> = {}): PrepTurn => ({
  id: ++turnId,
  seq: 1,
  kind,
  request: "B1 程度的閱讀理解",
  status: "failed",
  error: LONG_MESSAGE,
  worksheet: null,
  result: null,
  interaction_id: null,
  created_at: "2026-11-15T08:00:00Z",
  finished_at: "2026-11-15T08:01:00Z",
  ...overrides,
});

const session = (turns: PrepTurn[], overrides: Partial<PrepSession> = {}): PrepSession => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: turns.length,
  last_kind: turns[turns.length - 1]?.kind ?? null,
  updated_at: "2026-11-15T09:00:00Z",
  created_at: "2026-11-15T08:00:00Z",
  turns: turns.map((t, i) => ({ ...t, seq: i + 1 })),
  running: false,
  next: [],
  material_paragraphs: 52,
  material_selected: null,
  ...overrides,
});

const library = (m: Material = material()) => ({ ...emptyLibrary(), materials: [m] });

const box = () => screen.getByRole("textbox", { name: "給 Manna 的要求" });

async function attachFromLibrary(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "附上文章" }));
  await user.click(within(screen.getByRole("group", { name: "附上文章" })).getByRole("button", { name: "從教材挑選" }));
  const dialog = within(await screen.findByRole("dialog", { name: "從教材挑選" }));
  await user.click(await dialog.findByRole("button", { name: "選用「The Umbrella Stand」" }));
}

function homeRoutes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/materials": library(),
    "GET /api/browse/materials": materialBrowse([cardOf(material())]),
    ...extra,
  };
}

describe("長課文的提醒（首頁）", () => {
  it("says it before generating, offers three ways on, and 照樣產生 sends as it is", async () => {
    signIn();
    const api = mockApi(
      homeRoutes({
        [`POST ${SESSIONS}`]: session([turn("worksheet", { status: "running", error: null })]),
        [`GET ${S7}`]: session([turn("worksheet", { status: "running", error: null })]),
      }),
    );
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    const notice = within(await screen.findByRole("group", { name: /這份課文有 52 段/ }));
    expect(
      screen.getByText("這份課文有 52 段，比一般一節課長；產生會比較久，後面的段落也比較可能寫得簡略。"),
    ).toBeInTheDocument();
    expect(notice.getByRole("button", { name: "修改課文" })).toBeInTheDocument();
    expect(notice.getByRole("button", { name: "只選要用的段落" })).toBeInTheDocument();
    // It never blocks: nothing is sent until the teacher says so.
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
    await user.type(box(), "B1 程度");
    await user.click(notice.getByRole("button", { name: "照樣產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      entry: "home",
      kind: "worksheet",
      request: "B1 程度",
      material_id: 5,
    });
  });

  it("只選要用的段落 takes a range, shows how much is used, and sends only those paragraphs", async () => {
    signIn();
    const api = mockApi(
      homeRoutes({
        "GET /api/materials/5": detail(52),
        [`POST ${SESSIONS}`]: session([turn("worksheet", { status: "running", error: null })]),
        [`GET ${S7}`]: session([turn("worksheet", { status: "running", error: null })]),
      }),
    );
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    await user.click(await screen.findByRole("button", { name: "只選要用的段落" }));
    const dialog = within(await screen.findByRole("dialog", { name: "只選要用的段落" }));
    expect(await dialog.findByText("已選 52 段（共 52 段）")).toBeInTheDocument();
    await user.clear(dialog.getByRole("textbox", { name: "從第" }));
    await user.type(dialog.getByRole("textbox", { name: "從第" }), "3");
    await user.type(dialog.getByRole("textbox", { name: "段到第" }), "12");
    await user.click(dialog.getByRole("button", { name: "選這一段範圍" }));
    expect(dialog.getByText("已選 10 段（共 52 段）")).toBeInTheDocument();
    await user.click(dialog.getByRole("checkbox", { name: /第 3 段/ }));
    expect(dialog.getByText("已選 9 段（共 52 段）")).toBeInTheDocument();
    await user.click(dialog.getByRole("button", { name: "使用選的 9 段" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // The notice gives way to the line that says how much is used.
    expect(screen.queryByRole("group", { name: /這份課文有 52 段/ })).not.toBeInTheDocument();
    expect(screen.getByText(/只用其中 9 段（共 52 段）/)).toBeInTheDocument();

    await user.type(box(), "B1 程度{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    const sent = api.lastBody("POST", SESSIONS) as { options: { paragraph_ids: string[] } };
    expect(sent.options.paragraph_ids).toEqual(["p4", "p5", "p6", "p7", "p8", "p9", "p10", "p11", "p12"]);
    // The objectives draft reads the same paragraphs.
    expect((api.lastBody("POST", "/api/objective-drafts") as { paragraph_ids: string[] }).paragraph_ids).toHaveLength(9);
  });

  it("pasted text: the server counts the paragraphs; 只選要用的段落 stores it in the library first", async () => {
    signIn();
    const api = mockApi(
      homeRoutes({
        "POST /api/materials/split-preview": { paragraph_count: 52 },
        "POST /api/materials": detail(52, { id: 50, title: "Umbrella" }),
        "GET /api/materials/50": detail(52, { id: 50, title: "Umbrella" }),
        [`POST ${SESSIONS}`]: session([turn("worksheet", { status: "running", error: null })]),
        [`GET ${S7}`]: session([turn("worksheet", { status: "running", error: null })]),
      }),
    );
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(screen.getByRole("button", { name: "貼上文字" }));
    const paste = within(screen.getByRole("dialog", { name: "貼上文字" }));
    await user.type(paste.getByRole("textbox", { name: /教材標題/ }), "Umbrella");
    await user.type(paste.getByRole("textbox", { name: "貼上教材文字" }), "Mrs. Lin put a stand outside.");
    await user.click(paste.getByRole("button", { name: "附上" }));
    expect(await screen.findByText(/這份課文有 52 段/)).toBeInTheDocument();
    expect(api.lastBody("POST", "/api/materials/split-preview")).toEqual({ text: "Mrs. Lin put a stand outside." });

    // 修改課文 reopens the pasted text with what was typed.
    await user.click(screen.getByRole("button", { name: "修改課文" }));
    const again = within(screen.getByRole("dialog", { name: "貼上文字" }));
    expect(again.getByRole("textbox", { name: "貼上教材文字" })).toHaveValue("Mrs. Lin put a stand outside.");
    await user.click(again.getByRole("button", { name: "取消" }));

    await user.click(screen.getByRole("button", { name: "只選要用的段落" }));
    expect(api.lastBody("POST", "/api/materials")).toEqual({
      kind: "text",
      title: "Umbrella",
      text: "Mrs. Lin put a stand outside.",
    });
    const dialog = within(await screen.findByRole("dialog", { name: "只選要用的段落" }));
    await dialog.findByText("已選 52 段（共 52 段）");
    await user.click(dialog.getByRole("button", { name: "全部取消" }));
    await user.click(dialog.getByRole("checkbox", { name: /第 1 段/ }));
    await user.click(dialog.getByRole("button", { name: "使用選的 1 段" }));
    expect(await screen.findByText(/只用其中 1 段（共 52 段）/)).toBeInTheDocument();
    // The chip now names the stored material, so sending uses its id.
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({ material_id: 50, options: { paragraph_ids: ["p1"] } });
  });

  it("a material of 40 paragraphs or fewer is not announced", async () => {
    signIn();
    mockApi(homeRoutes({ "GET /api/materials": library(material({ paragraph_count: 40 })), "GET /api/browse/materials": materialBrowse([cardOf(material({ paragraph_count: 40 }))]) }));
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    expect(screen.getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
    expect(screen.queryByText(/比一般一節課長/)).not.toBeInTheDocument();
  });
});

describe("回上一步：什麼都不會不見", () => {
  const failedSession = () => session([turn("worksheet")], { running: false });

  it("a failed turn offers 回上一步, 修改課文, 只選要用的段落 and 再試一次; 回上一步 returns to the filled-in home box", async () => {
    signIn();
    mockApi(
      homeRoutes({
        [`POST ${SESSIONS}`]: failedSession(),
        [`GET ${S7}`]: failedSession(),
      }),
    );
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    await user.type(box(), "B1 程度的閱讀理解");
    await user.click(await screen.findByRole("button", { name: "照樣產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));

    const alert = within(await screen.findByRole("alert"));
    expect(alert.getByText(LONG_MESSAGE)).toBeInTheDocument();
    for (const name of ["回上一步", "修改課文", "只選要用的段落", "再試一次"]) {
      expect(alert.getByRole("button", { name })).toBeInTheDocument();
    }
    await user.click(alert.getByRole("button", { name: "回上一步" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    await screen.findByRole("heading", { name: HOME, level: 1 });
    // Both the typed request and the attached article are still there.
    expect(box()).toHaveValue("B1 程度的閱讀理解");
    expect(screen.getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
  });

  it("the browser's Back button finds the home box as it was left", async () => {
    signIn();
    mockApi(
      homeRoutes({
        [`POST ${SESSIONS}`]: failedSession(),
        [`GET ${S7}`]: failedSession(),
      }),
    );
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    await user.type(box(), "B1 程度的閱讀理解");
    await user.click(await screen.findByRole("button", { name: "照樣產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    await screen.findByRole("alert");

    await router.navigate(-1);
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    await screen.findByRole("heading", { name: HOME, level: 1 });
    expect(box()).toHaveValue("B1 程度的閱讀理解");
    expect(screen.getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
  });

  it("without the box's own draft (another browser), 回上一步 rebuilds it from the conversation", async () => {
    signIn();
    mockApi(homeRoutes({ [`GET ${S7}`]: failedSession() }));
    const user = userEvent.setup();
    const { router } = renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "回上一步" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    await screen.findByRole("heading", { name: HOME, level: 1 });
    expect(box()).toHaveValue("B1 程度的閱讀理解");
    expect(screen.getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
  });

  it("a failure at home (before any conversation) keeps the box and offers 修改課文, 換一份教材 and 再試一次", async () => {
    signIn();
    const api = mockApi(
      homeRoutes({
        "GET /api/materials/5": detail(52),
        [`POST ${SESSIONS}`]: respond(400, { error: { code: "bad_request", message: "這份教材沒有可以讀的文字。" } }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    await user.type(box(), "B1 程度");
    await user.click(await screen.findByRole("button", { name: "照樣產生" }));
    expect(await screen.findByText("這份教材沒有可以讀的文字。")).toBeInTheDocument();
    expect(screen.getByText("你輸入的課文和要求都還在。")).toBeInTheDocument();
    expect(box()).toHaveValue("B1 程度");
    expect(screen.getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
    for (const name of ["修改課文", "換一份教材", "再試一次"]) {
      expect(screen.getAllByRole("button", { name }).length).toBeGreaterThan(0);
    }
    await user.click(screen.getByRole("button", { name: "再試一次" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(2));
  });
});

describe("備課對話：修改課文與只選要用的段落", () => {
  it("修改課文 saves the pasted text in place; then 重新產生 runs again with the new text", async () => {
    signIn();
    prepConfig.pollMs = 20;
    let current = session([turn("worksheet")]);
    const api = mockApi({
      ...homeRoutes(),
      [`GET ${S7}`]: () => current,
      "GET /api/materials/5": detail(52),
      "PUT /api/materials/5": () => detail(30, { id: 5 }),
      [`POST ${S7}/turns`]: () => {
        current = session([...current.turns, turn("worksheet", { status: "running", error: null })]);
        return current;
      },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const alert = within(await screen.findByRole("alert"));
    await user.click(alert.getByRole("button", { name: "修改課文" }));
    const dialog = within(await screen.findByRole("dialog", { name: "修改課文" }));
    const text = await dialog.findByRole("textbox", { name: "課文" });
    expect(text).toHaveValue(detail(52).text);
    await user.clear(text);
    await user.type(text, "A shorter text.");
    await user.click(dialog.getByRole("button", { name: "儲存課文" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(api.lastBody("PUT", "/api/materials/5")).toEqual({
      title: "The Umbrella Stand",
      text: "A shorter text.",
      edit_from: "prep_session",
    });
    const again = within(await screen.findByRole("alert"));
    expect(again.getByText("課文已更新，按「重新產生」就會用新的課文。")).toBeInTheDocument();
    expect(again.queryByRole("button", { name: "再試一次" })).not.toBeInTheDocument();
    await user.click(again.getByRole("button", { name: "重新產生" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "worksheet", request: "B1 程度的閱讀理解" });
    prepConfig.pollMs = 2000;
  });

  it("an uploaded file's text cannot be edited: the dialog says so and offers 回上一步", async () => {
    signIn();
    mockApi({
      ...homeRoutes(),
      [`GET ${S7}`]: session([turn("worksheet")]),
      "GET /api/materials/5": { ...detail(52), origin: "upload", kind: "document" },
    });
    const user = userEvent.setup();
    const { router } = renderApp("/prep/7");
    await user.click(within(await screen.findByRole("alert")).getByRole("button", { name: "修改課文" }));
    const dialog = within(await screen.findByRole("dialog", { name: "修改課文" }));
    expect(await dialog.findByText(/上傳的檔案，讀出來的文字不能直接修改/)).toBeInTheDocument();
    await user.click(dialog.getByRole("button", { name: "回上一步，換一份教材" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  });

  it("只選要用的段落 stores the choice on the conversation; the material is not touched", async () => {
    signIn();
    let current = session([turn("worksheet")]);
    const api = mockApi({
      ...homeRoutes(),
      [`GET ${S7}`]: () => current,
      "GET /api/materials/5": detail(52),
      [`PUT ${S7}/material-selection`]: (call: { body: unknown }) => {
        const ids = (call.body as { paragraph_ids: string[] | null }).paragraph_ids;
        current = { ...current, options: { paragraph_ids: ids ?? [] }, material_selected: ids ? ids.length : null };
        return current;
      },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    expect(await screen.findByText(/這份課文有 52 段/)).toBeInTheDocument();
    await user.click(within(await screen.findByRole("alert")).getByRole("button", { name: "只選要用的段落" }));
    const dialog = within(await screen.findByRole("dialog", { name: "只選要用的段落" }));
    await dialog.findByText("已選 52 段（共 52 段）");
    await user.type(dialog.getByRole("textbox", { name: "從第" }), "1");
    await user.type(dialog.getByRole("textbox", { name: "段到第" }), "20");
    await user.click(dialog.getByRole("button", { name: "選這一段範圍" }));
    await user.click(dialog.getByRole("button", { name: "使用選的 20 段" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect((api.lastBody("PUT", `${S7}/material-selection`) as { paragraph_ids: string[] }).paragraph_ids).toHaveLength(20);
    expect(api.callsTo("PUT", "/api/materials/5")).toHaveLength(0);
    expect(await screen.findByText(/只用其中 20 段（共 52 段）/)).toBeInTheDocument();
    // The notice is gone now that part of it is chosen, and 重新產生 is offered.
    expect(screen.queryByText(/比一般一節課長/)).not.toBeInTheDocument();
    expect(within(screen.getByRole("alert")).getByRole("button", { name: "重新產生" })).toBeInTheDocument();
  });

  it("a failed turn without a material still offers 回上一步 and 再試一次", async () => {
    signIn();
    mockApi({
      ...homeRoutes(),
      [`GET ${S7}`]: session([turn("ask", { request: "什麼是 SIOP？", error: "這一步沒有成功。" })], {
        material: null,
        material_paragraphs: null,
      }),
    });
    renderApp("/prep/7");
    const alert = within(await screen.findByRole("alert"));
    expect(alert.getByRole("button", { name: "回上一步" })).toBeInTheDocument();
    expect(alert.getByRole("button", { name: "再試一次" })).toBeInTheDocument();
    expect(alert.queryByRole("button", { name: "修改課文" })).not.toBeInTheDocument();
  });
});

describe("學習單頁", () => {
  const P = (n: number) => Array.from({ length: n }, (_, i) => ({ id: `p${i + 1}`, text: `Paragraph ${i + 1} is about umbrellas.` }));
  const wsState = (overrides: Partial<WorksheetState> = {}): WorksheetState => ({
    id: 5,
    title: "The Umbrella Stand",
    filename: "The Umbrella Stand_學習單",
    lesson: null,
    class_id: null,
    generated: false,
    levels: [],
    version_no: 1,
    created_at: "2026-11-15T08:00:00Z",
    updated_at: "2026-11-15T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: P(52),
      source: { kind: "library", lesson_id: null, material_version_no: 1, note: "", material_id: 9 },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-15T08:00:00Z",
    content: emptyWorksheetContent(),
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    ...overrides,
  });
  const WS = "/api/worksheets/5";
  const routes = (state: WorksheetState, extra: Record<string, unknown> = {}) => ({
    ...baseRoutes(),
    "GET /api/worksheets": [],
    [`GET ${WS}`]: state,
    "GET /api/materials/9": detail(52, { id: 9 }),
    "GET /api/materials/9/annotations": [],
    ...extra,
  });

  it("says a long material is long before generating; 只選要用的段落 re-takes the snapshot; the library stays", async () => {
    signIn();
    let state = wsState();
    const api = mockApi(
      routes(state, {
        [`GET ${WS}`]: () => state,
        [`PUT ${WS}/material-selection`]: (call: { body: unknown }) => {
          const ids = (call.body as { paragraph_ids: string[] | null }).paragraph_ids ?? [];
          state = wsState({
            material: {
              title: "The Umbrella Stand",
              paragraphs: P(52).filter((p) => ids.includes(p.id)),
              source: {
                kind: "library",
                lesson_id: null,
                material_version_no: 1,
                note: "",
                material_id: 9,
                selected_ids: ids,
                total_paragraphs: 52,
              },
            },
          });
          return state;
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const notice = within(await screen.findByRole("group", { name: /這份課文有 52 段/ }));
    expect(notice.getByRole("button", { name: "照樣產生" })).toBeInTheDocument();
    await user.click(notice.getByRole("button", { name: "只選要用的段落" }));
    const dialog = within(await screen.findByRole("dialog", { name: "只選要用的段落" }));
    await dialog.findByText("已選 52 段（共 52 段）");
    await user.type(dialog.getByRole("textbox", { name: "從第" }), "5");
    await user.type(dialog.getByRole("textbox", { name: "段到第" }), "10");
    await user.click(dialog.getByRole("button", { name: "選這一段範圍" }));
    await user.click(dialog.getByRole("button", { name: "使用選的 6 段" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(api.lastBody("PUT", `${WS}/material-selection`)).toEqual({
      paragraph_ids: ["p5", "p6", "p7", "p8", "p9", "p10"],
    });
    expect(await screen.findByText(/只用其中 6 段（共 52 段）/)).toBeInTheDocument();
    expect(screen.queryByText(/比一般一節課長/)).not.toBeInTheDocument();
    expect(api.callsTo("PUT", "/api/materials/9")).toHaveLength(0);
  });

  it("照樣產生 starts the generation as it is", async () => {
    signIn();
    const state = wsState();
    const api = mockApi(
      routes(state, { [`POST ${WS}/generate`]: { ...state, running: true } }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(await screen.findByRole("button", { name: "照樣產生" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/generate`)).toHaveLength(1));
  });

  it("a generation that fails offers 修改課文, 只選要用的段落 and 再試一次 beside the message; the settings stay", async () => {
    signIn();
    const state = wsState({ material: { ...wsState().material, paragraphs: P(5) } });
    const api = mockApi(
      routes(state, {
        [`POST ${WS}/generate`]: respond(400, { error: { code: "bad_request", message: "Manna 這次沒有讀出教材。" } }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(await screen.findByRole("button", { name: "產生學習單" }));
    const message = await screen.findByText("Manna 這次沒有讀出教材。");
    const row = message.parentElement!;
    for (const name of ["修改課文", "只選要用的段落", "再試一次"]) {
      expect(within(row).getAllByRole("button", { name }).length).toBeGreaterThan(0);
    }
    // 修改課文 opens the text; the options page is still where it was.
    await user.click(within(row).getByRole("button", { name: "修改課文" }));
    expect(await screen.findByRole("dialog", { name: "修改課文" })).toBeInTheDocument();
    expect(api.callsTo("POST", `${WS}/generate`)).toHaveLength(1);
  });
});
