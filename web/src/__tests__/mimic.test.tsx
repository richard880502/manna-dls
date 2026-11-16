// 照我的學習單做 (mimic-contract, "Frontend"): the tool at /create/mimic (first in the sidebar and on
// the home page) with its sample card (上傳學習單, 從我的版型選, 從教材庫選), the lesson text, the
// settings and the sticky bar; the notice when a worksheet lands in the article slot (on any create
// page and on the home quick box); the 完整學習單 link; and the session page while Manna reads the
// teacher's worksheet, once it is done, and when reading failed.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  Material,
  MaterialDetail,
  MaterialLibrary,
  PrepSession,
  PrepTurn,
  WorksheetState,
  WsSheet,
  WsTemplateSummary,
} from "../api/types";
import { NO_TEXT_LINE } from "../components/material/MaterialChooser";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { EXEMPLAR_TEXT } from "../lib/create";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, emptyLibrary, mockApi, renderApp, schoolClass, signIn } from "../test/mockApi";

const SESSIONS = "/api/prep-sessions";
const S7 = `${SESSIONS}/7`;
const NOTICE = "這份看起來是一份學習單。要照它的樣式為新課文出題嗎？";
const SAMPLE_LINE = "Manna 會讀這份學習單的題型、順序和作答方式，新的學習單照同樣的樣式出題。第一次約需 20 秒。";
const READING = "Manna 正在讀你的學習單設計…（約 20 秒）";

beforeEach(() => {
  signIn();
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});

afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
});

// ---------------------------------------------------------------- fixtures (all made up)

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
  paragraph_count: 4,
  word_count: 300,
  revision: 1,
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  lessons: [],
  worksheet_count: 0,
  annotation_count: 0,
  looks_like_worksheet: false,
  ...overrides,
});

/** A worksheet sample: lots of numbered lines and blanks. */
const sampleMaterial = (overrides: Partial<Material> = {}) =>
  material({ id: 4, title: "L2 學習單", filename: "l2.docx", paragraph_count: 147, looks_like_worksheet: true, ...overrides });

const detail = (m: Material): MaterialDetail => ({
  ...m,
  text: "Name ____ Class ____\n1. ____",
  paragraphs: [{ id: "p1", no: 1, text: "Name ____ Class ____" }],
});

const library = (materials: Material[]): MaterialLibrary => ({ ...emptyLibrary(), materials });

const template = (overrides: Partial<WsTemplateSummary> = {}): WsTemplateSummary => ({
  id: 9,
  title: "L2 學習單版型",
  status: "ready",
  section_count: 3,
  source_material: { id: 4, title: "L2 學習單" },
  version_no: 1,
  updated_at: "2026-11-14T08:00:00Z",
  ...overrides,
});

const created: PrepSession = {
  id: 7,
  title: "Why Bees Matter",
  material: null,
  topic: null,
  class: null,
  template: null,
  turn_count: 0,
  last_kind: null,
  updated_at: "2026-11-16T08:00:00Z",
  created_at: "2026-11-16T08:00:00Z",
  turns: [],
  running: false,
  next: [],
};

function routes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/classes": [schoolClass({ id: 3, name: "高一 3 班" })],
    "GET /api/materials": library([sampleMaterial(), material(), material({ id: 8, title: "掃描檔", text_status: "no_text" })]),
    "GET /api/worksheet-templates": {
      templates: [template(), template({ id: 10, title: "還在讀的版型", status: "extracting", section_count: 0 })],
    },
    [`POST ${SESSIONS}`]: created,
    [`GET ${S7}`]: created,
    ...extra,
  };
}

const heading = () => screen.getByRole("heading", { level: 1 });
const summaryText = () => (document.querySelector(".create-summary") as HTMLElement).textContent;
const generate = () => screen.getByRole("button", { name: "產生" });
const sampleCard = () => within(screen.getByRole("region", { name: "你的學習單範本" }));
const lessonCard = () => within(screen.getByRole("region", { name: "這節課的課文" }));
const ways = () => within(screen.getByRole("group", { name: "範本從哪裡來" }));
const notice = () => screen.queryByRole("group", { name: NOTICE });

async function openMimic(path = "/create/mimic") {
  const rendered = renderApp(path);
  await waitFor(() => expect(heading()).toHaveTextContent("照我的學習單做"));
  return rendered;
}

// ---------------------------------------------------------------- where it is

describe("照我的學習單做: where it is", () => {
  it("is the first tool in the sidebar and the first, widest card on the home page", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    const create = nav.getAllByRole("link").filter((a) => a.getAttribute("href")?.startsWith("/create/"));
    expect(create[0]).toHaveTextContent("照我的學習單做");
    expect(create[0]).toHaveAttribute("href", "/create/mimic");

    const tools = within(screen.getByRole("region", { name: "Manna 可以幫你做" }));
    const first = tools.getAllByRole("link")[0]!;
    expect(first).toHaveAttribute("href", "/create/mimic");
    expect(first).toHaveTextContent("照我的學習單做");
    expect(first).toHaveTextContent("上傳你以前的學習單，Manna 照它的樣式為新課文出題");
    expect(first.closest("li")).toHaveClass("is-featured");
    // The quick box's chips stay the five kinds.
    expect(
      within(screen.getByRole("group", { name: "要做什麼" }))
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["閱讀理解題", "分層練習", "課堂活動", "教學套件", "學習單"]);

    await user.click(first);
    await waitFor(() => expect(router.state.location.pathname).toBe("/create/mimic"));
    expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("照我的學習單做");
    expect(nav.getByRole("link", { name: "照我的學習單做" })).toHaveAttribute("aria-current", "page");
  });

  it("has three cards: the sample, the lesson text, and 設定 with only 班級／程度 and 學習目標", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    await openMimic();
    // No 「來做一份」 before this tool's name; the switcher lists it first.
    expect(heading().textContent).toBe("照我的學習單做");
    expect(screen.getByText("上傳你以前的學習單，Manna 照它的樣式為新課文出題。")).toBeInTheDocument();
    expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual([
      "你的學習單範本",
      "這節課的課文",
      "設定",
    ]);
    expect(sampleCard().getByText(SAMPLE_LINE)).toBeInTheDocument();
    // The privacy reminder is said once, with the sample (where the upload form is).
    expect(await sampleCard().findByRole("form", { name: "上傳檔案" })).toBeInTheDocument();
    expect(screen.getAllByText(/請不要上傳學生作業/)).toHaveLength(1);
    expect(sampleCard().getByText(/請不要上傳學生作業/)).toBeInTheDocument();
    expect(lessonCard().getByRole("button", { name: "用範例試試" })).toBeInTheDocument();
    expect(lessonCard().getByRole("button", { name: "上傳檔案" })).toBeInTheDocument();
    expect(lessonCard().getByRole("button", { name: "從教材庫選" })).toBeInTheDocument();

    const settings = within(screen.getByRole("region", { name: "設定" }));
    expect(settings.getByRole("combobox", { name: "班級／程度" })).toBeInTheDocument();
    expect(settings.getByRole("textbox", { name: "學習目標（選填）" })).toBeInTheDocument();
    for (const gone of ["認知層次（Bloom）", "題數", "題型"]) {
      expect(screen.queryByRole("group", { name: gone })).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("combobox", { name: "版型" })).not.toBeInTheDocument();

    await user.click(within(heading()).getByRole("button", { name: "照我的學習單做" }));
    expect(
      within(screen.getByRole("group", { name: "換一種" }))
        .getAllByRole("link")
        .map((a) => a.textContent),
    ).toEqual(["照我的學習單做", "閱讀理解題", "分層練習", "課堂活動", "教學套件", "完整學習單"]);
  });

  it("完整學習單 keeps its 版型 select and links to 照我的學習單做, keeping what was filled in", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/create/worksheet");
    await waitFor(() => expect(heading()).toHaveTextContent("來做一份 完整學習單"));
    expect(await screen.findByRole("combobox", { name: "版型" })).toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "文章或主題" }), "Bees are busy.");
    const link = screen.getByRole("link", { name: "或上傳你的學習單範本 →" });
    expect(link).toHaveAttribute("href", "/create/mimic");
    await user.click(link);
    await waitFor(() => expect(router.state.location.pathname).toBe("/create/mimic"));
    expect(lessonCard().getByRole("textbox", { name: "這節課的課文" })).toHaveValue("Bees are busy.");
  });

  it("完整學習單 without templates still links to 照我的學習單做", async () => {
    mockApi(routes({ "GET /api/worksheet-templates": { templates: [] } }));
    renderApp("/create/worksheet");
    expect(await screen.findByRole("link", { name: "或上傳你的學習單範本 →" })).toHaveAttribute("href", "/create/mimic");
  });
});

// ---------------------------------------------------------------- the three ways to a sample

describe("照我的學習單做: the sample and the request", () => {
  it("上傳學習單 is the default; 產生 waits for a sample, then for the text; the upload goes as template_material_id", async () => {
    const api = mockApi(
      routes({ "POST /api/materials/upload": detail(sampleMaterial({ id: 6, title: "期中學習單" })) }),
    );
    const user = userEvent.setup({ applyAccept: false });
    const { router } = await openMimic();

    expect(ways().getAllByRole("button").map((b) => [b.textContent, b.getAttribute("aria-pressed")])).toEqual([
      ["上傳學習單", "true"],
      ["從我的版型選", "false"],
      ["從教材庫選", "false"],
    ]);
    expect(generate()).toBeDisabled();
    expect(summaryText()).toBe("先放你的學習單範本");
    expect(generate()).toHaveAccessibleDescription("先放你的學習單範本");

    // The upload form is right here.
    const form = within(await sampleCard().findByRole("form", { name: "上傳檔案" }));
    await user.upload(form.getByLabelText("選擇檔案"), new File(["PK"], "midterm.docx"));
    await user.click(form.getByRole("button", { name: "上傳" }));
    expect(await sampleCard().findByRole("button", { name: "移除「期中學習單」" })).toBeInTheDocument();
    expect(sampleCard().queryByRole("group", { name: "範本從哪裡來" })).not.toBeInTheDocument();
    expect(generate()).toBeDisabled();
    expect(summaryText()).toBe("照「期中學習單」的樣式 · 先放這節課的課文");

    await user.click(lessonCard().getByRole("button", { name: "用範例試試" }));
    expect(lessonCard().getByRole("textbox", { name: "這節課的課文" })).toHaveValue(EXEMPLAR_TEXT);
    expect(summaryText()).toBe("照「期中學習單」的樣式");
    expect(generate()).toBeEnabled();

    await user.selectOptions(screen.getByRole("combobox", { name: "班級／程度" }), "高一 3 班");
    await user.type(screen.getByRole("textbox", { name: "學習目標（選填）" }), "學生能說出蜜蜂的重要");
    await user.click(generate());
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "worksheet",
      request: "學生能說出蜜蜂的重要",
      material: { title: "", text: EXEMPLAR_TEXT },
      class_id: 3,
      template_material_id: 6,
      options: { objective: "學生能說出蜜蜂的重要" },
    });
  });

  it("從我的版型選 lists the ready templates; the chip shows the title, the sections and 看版型內容; it goes as template_id", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openMimic();
    await user.click(ways().getByRole("button", { name: "從我的版型選" }));
    const step = within(sampleCard().getByRole("region", { name: "從我的版型選" }));
    expect(step.getByText("3 區 · 第 1 版 · 來自「L2 學習單」")).toBeInTheDocument();
    // Only ready templates can be used.
    expect(step.queryByRole("button", { name: "選用「還在讀的版型」" })).not.toBeInTheDocument();
    await user.click(step.getByRole("button", { name: "選用「L2 學習單版型」" }));

    const chip = sampleCard().getByRole("status");
    expect(chip).toHaveTextContent("L2 學習單版型· 3 區");
    const view = sampleCard().getByRole("link", { name: "看版型內容" });
    expect(view).toHaveAttribute("href", "/templates/9");
    expect(view).toHaveAttribute("target", "_blank");
    expect(summaryText()).toBe("照「L2 學習單版型」的樣式 · 先放這節課的課文");

    await user.type(lessonCard().getByRole("textbox", { name: "這節課的課文" }), "Bees are busy.");
    expect(summaryText()).toBe("照「L2 學習單版型」的樣式");
    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "worksheet",
      request: "學習單",
      material: { title: "", text: "Bees are busy." },
      template_id: 9,
    });
  });

  it("removing the sample brings the three ways back", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    await openMimic();
    await user.click(ways().getByRole("button", { name: "從我的版型選" }));
    await user.click(await sampleCard().findByRole("button", { name: "選用「L2 學習單版型」" }));
    await user.click(sampleCard().getByRole("button", { name: "移除「L2 學習單版型」" }));
    expect(ways().getByRole("button", { name: "從我的版型選" })).toHaveAttribute("aria-pressed", "true");
    expect(generate()).toBeDisabled();
    expect(summaryText()).toBe("先放你的學習單範本");
  });

  it("從教材庫選 offers materials with text; the library sample goes as template_material_id beside the article's material_id", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openMimic();
    await user.click(ways().getByRole("button", { name: "從教材庫選" }));
    const step = within(sampleCard().getByRole("region", { name: "從教材庫選" }));
    expect(await step.findByRole("button", { name: "選用「L2 學習單」" })).toBeInTheDocument();
    expect(step.queryByRole("button", { name: "選用「掃描檔」" })).not.toBeInTheDocument();
    await user.click(step.getByRole("button", { name: "選用「L2 學習單」" }));
    expect(sampleCard().getByRole("button", { name: "移除「L2 學習單」" })).toBeInTheDocument();
    expect(sampleCard().queryByRole("link", { name: "看版型內容" })).not.toBeInTheDocument();

    // The lesson text from the library too.
    await user.click(lessonCard().getByRole("button", { name: "從教材庫選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從教材庫選" }));
    await user.click(await dialog.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    expect(lessonCard().getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
    expect(summaryText()).toBe("照「L2 學習單」的樣式");

    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "worksheet",
      request: "學習單",
      material_id: 5,
      template_material_id: 4,
    });
  });

  it("?sample=<id> reads that material and makes it the sample; one without text says so", async () => {
    const api = mockApi(
      routes({
        "GET /api/materials/4": detail(sampleMaterial()),
        "GET /api/materials/8": detail(material({ id: 8, title: "掃描檔", text_status: "no_text" })),
      }),
    );
    const { router } = await openMimic("/create/mimic?sample=4");
    expect(await sampleCard().findByRole("button", { name: "移除「L2 學習單」" })).toBeInTheDocument();
    await waitFor(() => expect(router.state.location.search).toBe(""));
    expect(router.state.location.pathname).toBe("/create/mimic");
    expect(api.callsTo("GET", "/api/materials/4")).toHaveLength(1);

    await router.navigate("/create/mimic?sample=8");
    expect(await sampleCard().findByText(NO_TEXT_LINE)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the notice

describe("a worksheet in the article slot", () => {
  it("on a create page: 照這份的樣式做 opens 照我的學習單做 with it as the sample, keeping the objective and class", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/create/comprehension");
    await waitFor(() => expect(heading()).toHaveTextContent("來做一份 閱讀理解題"));
    await user.type(screen.getByRole("textbox", { name: "學習目標（選填）" }), "學生能分析作者的論點");
    await user.selectOptions(screen.getByRole("combobox", { name: "班級／程度" }), "高一 3 班");

    // A material without the mark shows no notice.
    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    await user.click(await screen.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    expect(notice()).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "移除「The Umbrella Stand」" }));

    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    await user.click(await screen.findByRole("button", { name: "選用「L2 學習單」" }));
    const box = within(notice()!);
    // It never blocks generating with it as the article.
    expect(generate()).toBeEnabled();
    await user.click(box.getByRole("button", { name: "照這份的樣式做" }));

    await waitFor(() => expect(router.state.location.pathname).toBe("/create/mimic"));
    await waitFor(() => expect(router.state.location.search).toBe(""));
    expect(heading()).toHaveTextContent("照我的學習單做");
    expect(sampleCard().getByRole("button", { name: "移除「L2 學習單」" })).toBeInTheDocument();
    // It left the article slot; the objective and the class came along.
    expect(lessonCard().getByRole("textbox", { name: "這節課的課文" })).toHaveValue("");
    expect(lessonCard().queryByRole("button", { name: /^移除/ })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "學習目標（選填）" })).toHaveValue("學生能分析作者的論點");
    expect(screen.getByRole("combobox", { name: "班級／程度" })).toHaveDisplayValue("高一 3 班");
    expect(notice()).not.toBeInTheDocument();
    // The title came along: nothing more was asked for.
    expect(api.callsTo("GET", "/api/materials/4")).toHaveLength(0);

    await user.click(lessonCard().getByRole("button", { name: "用範例試試" }));
    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "worksheet",
      request: "學生能分析作者的論點",
      material: { title: "", text: EXEMPLAR_TEXT },
      class_id: 3,
      template_material_id: 4,
      options: { objective: "學生能分析作者的論點" },
    });
  });

  it("不用，當成課文 hides the notice for that material, and it goes as the article", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/create/activity");
    await waitFor(() => expect(heading()).toHaveTextContent("來做一份 課堂活動"));
    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    await user.click(await screen.findByRole("button", { name: "選用「L2 學習單」" }));
    await user.click(within(notice()!).getByRole("button", { name: "不用，當成課文" }));
    expect(notice()).not.toBeInTheDocument();

    // Taken away and chosen again: still no notice for it.
    await user.click(screen.getByRole("button", { name: "移除「L2 學習單」" }));
    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    await user.click(await screen.findByRole("button", { name: "選用「L2 學習單」" }));
    expect(notice()).not.toBeInTheDocument();

    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ kind: "activity", request: "課堂活動", material_id: 4 });
  });

  it("on the home quick box: 不用，當成課文 hides the notice and keeps the material attached", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    await attachFromLibrary(user, "L2 學習單");
    expect(notice()).toBeInTheDocument();
    await user.click(within(notice()!).getByRole("button", { name: "不用，當成課文" }));
    expect(notice()).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "移除「L2 學習單」" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送出" })).toBeEnabled();
  });

  it("on the home quick box: 照這份的樣式做 opens 照我的學習單做 with it as the sample", async () => {
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    await attachFromLibrary(user, "The Umbrella Stand");
    expect(notice()).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "移除「The Umbrella Stand」" }));
    await attachFromLibrary(user, "L2 學習單");
    await user.click(within(notice()!).getByRole("button", { name: "照這份的樣式做" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/create/mimic"));
    expect(await sampleCard().findByRole("button", { name: "移除「L2 學習單」" })).toBeInTheDocument();
    await waitFor(() => expect(router.state.location.search).toBe(""));
    expect(generate()).toBeDisabled();
    expect(summaryText()).toBe("照「L2 學習單」的樣式 · 先放這節課的課文");
  });
});

/** 附上文章 → 從教材庫選 → 選用 on the home page. */
async function attachFromLibrary(user: ReturnType<typeof userEvent.setup>, title: string) {
  await user.click(screen.getByRole("button", { name: "附上文章" }));
  await user.click(within(screen.getByRole("group", { name: "附上文章" })).getByRole("button", { name: "從教材庫選" }));
  await user.click(await screen.findByRole("button", { name: `選用「${title}」` }));
}

// ---------------------------------------------------------------- the session page

let turnId = 300;
const wsTurn = (overrides: Partial<PrepTurn> = {}): PrepTurn => ({
  id: ++turnId,
  seq: 1,
  kind: "worksheet",
  request: "學習單",
  status: "running",
  error: null,
  worksheet: null,
  template: { id: 12, title: "期中學習單", status: "extracting" },
  result: null,
  interaction_id: null,
  created_at: "2026-11-16T08:00:00Z",
  finished_at: null,
  ...overrides,
});

const session = (turn: PrepTurn): PrepSession => ({
  ...created,
  material: { id: 5, title: "The Umbrella Stand" },
  template: turn.template ? { id: turn.template.id, title: turn.template.title } : null,
  turn_count: 1,
  last_kind: "worksheet",
  turns: [turn],
  running: turn.status === "running",
});

const worksheet = (): WorksheetState => ({
  id: 11,
  title: "The Umbrella Stand",
  filename: "The Umbrella Stand_學習單",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no: 1,
  created_at: "2026-11-16T08:00:00Z",
  updated_at: "2026-11-16T08:00:00Z",
  material: {
    title: "The Umbrella Stand",
    paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
    source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 },
  },
  settings: emptySettings(),
  suggested: emptySettings(),
  brief: emptyBrief(),
  versions_count: 1,
  saved_at: "2026-11-16T08:00:00Z",
  content: { ...emptyWorksheetContent(), levels: ["standard"] },
  jobs: [],
  running: false,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
});

const sheet: WsSheet = {
  title: "The Umbrella Stand",
  level: "standard",
  audience: "student",
  objectives: [],
  blocks: [
    {
      id: "b1",
      kind: "activity",
      label: "A",
      title: "Paragraph 1: Find",
      instructions: "",
      steps: [],
      vocabulary: [],
      supports: [],
      items: [{ no: 1, answer_format: "簡答題", prompt: "Why did Mrs. Lin start the stand?", options: [], lines: 2 }],
    },
  ],
};

describe("the session page with the teacher's worksheet", () => {
  it("says Manna is reading the worksheet, then shows the progress, then whose style the worksheet follows", async () => {
    const steps = [
      session(wsTurn()),
      session(wsTurn({ id: turnId, template: { id: 12, title: "期中學習單", status: "ready" }, worksheet: { id: 11, title: "U", version_no: 1, running: true } })),
      session(
        wsTurn({
          id: turnId,
          status: "done",
          template: { id: 12, title: "期中學習單", status: "ready" },
          worksheet: { id: 11, title: "U", version_no: 1, running: false },
          finished_at: "2026-11-16T08:01:00Z",
        }),
      ),
    ];
    let reads = 0;
    let holdProgress = true;
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: () => {
        // The reading line, then the progress (held until the test has seen it), then done.
        const at = reads === 0 ? 0 : holdProgress ? 1 : 2;
        reads += 1;
        return steps[at];
      },
      "GET /api/worksheets/11/jobs": {
        running: true,
        version_no: 1,
        jobs: [
          {
            id: 1,
            worksheet_id: 11,
            run_no: 1,
            step: "tasks",
            block_id: null,
            status: "running",
            outcome: null,
            phase: "正在讀第 1 段",
            message: null,
            interaction_id: null,
            context: [],
            started_at: "2026-11-16T08:00:00Z",
            finished_at: null,
          },
        ],
      },
      "GET /api/worksheets/11": worksheet(),
      "GET /api/worksheets/11/sheet": sheet,
    });
    renderApp("/prep/7");
    expect(await screen.findByText(READING)).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "產生進度" })).not.toBeInTheDocument();

    expect(await screen.findByRole("status", { name: "產生進度" })).toBeInTheDocument();
    expect(screen.queryByText(READING)).not.toBeInTheDocument();
    holdProgress = false;

    const line = await screen.findByText((_, el) => el?.classList.contains("prep-ws-template") ?? false);
    expect(line).toHaveTextContent("照「期中學習單」的樣式");
    expect(within(line).getByRole("link", { name: "期中學習單" })).toHaveAttribute("href", "/templates/12");
    expect(await screen.findByText("Why did Mrs. Lin start the stand?")).toBeInTheDocument();
  });

  it("a worksheet without a template says nothing about a style", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session(
        wsTurn({ status: "done", template: null, worksheet: { id: 11, title: "U", version_no: 1, running: false } }),
      ),
      "GET /api/worksheets/11": worksheet(),
      "GET /api/worksheets/11/sheet": sheet,
    });
    renderApp("/prep/7");
    expect(await screen.findByText("Why did Mrs. Lin start the stand?")).toBeInTheDocument();
    expect(screen.queryByText(/的樣式$/)).not.toBeInTheDocument();
    expect(document.querySelector(".prep-ws-template")).toBeNull();
  });

  it("when reading failed, the turn shows the template's error and 再試一次", async () => {
    const failed = session(
      wsTurn({
        status: "failed",
        error: "Manna 讀不出這份學習單的設計，請到版型頁再讀一次。",
        template: { id: 12, title: "期中學習單", status: "failed" },
        finished_at: "2026-11-16T08:01:00Z",
      }),
    );
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: failed,
      [`POST ${S7}/turns`]: failed,
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    expect(await screen.findByText("Manna 讀不出這份學習單的設計，請到版型頁再讀一次。")).toBeInTheDocument();
    expect(screen.queryByText(READING)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "再試一次" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "worksheet", request: "學習單" });
  });

  it("the session's template made after the page loaded still shows in the 照我的版型 chip", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/worksheet-templates": { templates: [template()] },
      [`GET ${S7}`]: session(
        wsTurn({ status: "done", template: { id: 12, title: "期中學習單", status: "ready" }, worksheet: null }),
      ),
    });
    renderApp("/prep/7");
    const select = await screen.findByRole("combobox", { name: /照我的版型/ });
    expect(select).toHaveDisplayValue("期中學習單");
  });
});
