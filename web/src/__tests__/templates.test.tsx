// 我的學習單版型 (docs/WORKSHEET_TEMPLATES_CONTRACT.md, "Frontend"; now 學習單範本's 我的範本 tab, library
// contract): upload a worksheet, the cards with their state, polling while Manna reads), the template page
// (edit, reorder, toggles, save as a new version, delete, failed and retry), 直接做一份 with a
// template, and the 照我的版型 select on the worksheet settings view.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  Material,
  MaterialDetail,
  MaterialLibrary,
  UpdateWsTemplateRequest,
  WorksheetState,
  WsBlock,
  WsTemplate,
  WsTemplateDetail,
  WsTemplateSection,
  WsTemplateSummary,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { UPLOAD_WARNING } from "../lib/materials";
import { templateConfig } from "../lib/templates";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import {
  baseRoutes,
  mockApi,
  renderApp,
  respond,
  signIn,
  templateBrowse,
  templateCard,
  type MockCall,
} from "../test/mockApi";

const TEMPLATES = "/api/worksheet-templates";
const T9 = `${TEMPLATES}/9`;
const EXTRACTING = "Manna 正在讀這份學習單的設計…（約 20 秒）";

// ---------------------------------------------------------------- fixtures

const material = (overrides: Partial<Material> = {}): Material => ({
  id: 5,
  kind: "document",
  origin: "upload",
  title: "L2 學習單",
  source_note: "",
  url: null,
  filename: "l2.docx",
  content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  size_bytes: 2048,
  sha256: "ab".repeat(32),
  has_file: true,
  text_status: "ready",
  text_truncated: false,
  page_count: 2,
  paragraph_count: 12,
  word_count: 400,
  revision: 1,
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  lessons: [],
  worksheet_count: 0,
  annotation_count: 0,
  ...overrides,
});

const materialDetail = (overrides: Partial<MaterialDetail> = {}): MaterialDetail => ({
  ...material(),
  text: "Name ___",
  paragraphs: [{ id: "p1", no: 1, text: "Name ___" }],
  ...overrides,
});

const library = (materials: Material[] = [material()]): MaterialLibrary => ({
  materials,
  usage: { used_bytes: 1024 * 1024, quota_bytes: 1024 * 1024 * 1024, count: materials.length, max_count: 300 },
  limits: {
    files: [
      { ext: "docx", kind: "document", max_bytes: 20 * 1024 * 1024 },
      { ext: "pdf", kind: "document", max_bytes: 20 * 1024 * 1024 },
    ],
    max_pasted_chars: 50000,
    max_note_chars: 2000,
  },
  storage: "fs",
  uploads_enabled: true,
});

const section = (id: string, title: string, extra: Partial<WsTemplateSection> = {}): WsTemplateSection => ({
  id,
  title_pattern: title,
  purpose: `${title} 的用途`,
  block_kind: "activity",
  per_paragraph: false,
  student_tasks: ["理解內容"],
  answer_formats: ["簡答題"],
  item_count: 3,
  text_scope: "全文",
  scaffolds: [],
  instruction_style: "English imperative",
  item_patterns: ["What does [主題] mean?"],
  needs_external_material: false,
  ...extra,
});

const content = (overrides: Partial<WsTemplate> = {}): WsTemplate => ({
  worksheet_kind: "課文深度閱讀",
  instruction_language: "英文",
  header_fields: ["Name", "No", "Class"],
  shows_objectives: true,
  includes_passage_text: false,
  sections: [
    section("s1", "Warm-up: [主題]", { block_kind: "warmup", item_count: 2 }),
    section("s2", "Paragraph [n]: Find", { per_paragraph: true, answer_formats: ["填空題"] }),
    section("s3", "Video talk", { needs_external_material: true, item_count: 4 }),
  ],
  generation_options: ["加入暖身活動"],
  style_notes: ["每題留兩行作答空間"],
  ...overrides,
});

const summary = (overrides: Partial<WsTemplateSummary> = {}): WsTemplateSummary => ({
  id: 9,
  title: "L2 學習單版型",
  status: "ready",
  section_count: 3,
  source_material: { id: 5, title: "L2 學習單" },
  version_no: 1,
  updated_at: "2026-11-15T08:00:00Z",
  ...overrides,
});

const detail = (overrides: Partial<WsTemplateDetail> = {}): WsTemplateDetail => ({
  ...summary(),
  error: null,
  content: content(),
  ...overrides,
});

const extracting = (overrides: Partial<WsTemplateDetail> = {}) =>
  detail({ status: "extracting", section_count: 0, version_no: null, content: null, ...overrides });

beforeEach(() => {
  signIn();
  templateConfig.pollMs = 20;
  autosaveConfig.delayMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  templateConfig.pollMs = 3000;
  autosaveConfig.delayMs = 1500;
  worksheetConfig.pollMs = 2000;
});

const BROWSE = "/api/browse/templates";
const MINE = "/templates?tab=mine";
const tcard = (title: string) => within(screen.getByRole("article", { name: title }));

// ---------------------------------------------------------------- 學習單範本 → 我的範本

describe("學習單範本: 我的範本", () => {
  it("shows my templates as cards with their state and asks again every few seconds while one is being read", async () => {
    let polls = 0;
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${BROWSE}`]: () => {
        polls += 1;
        return templateBrowse([
          polls < 3
            ? templateCard({ id: 10, title: "新的版型", status: "extracting", section_count: 0, section_titles: [] })
            : templateCard({ id: 10, title: "新的版型" }),
          templateCard({ id: 8, title: "舊的版型", status: "failed", section_count: 0, section_titles: [] }),
          templateCard(),
        ]);
      },
    });
    renderApp(MINE);
    expect(await screen.findByRole("tab", { name: "我的範本" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("上傳一份你做過的學習單，Manna 學它的設計，之後新課可以照這個版型產生。")).toBeInTheDocument();
    await screen.findByRole("article", { name: "新的版型" });
    expect(tcard("新的版型").getByText("讀取中", { selector: ".lib-badge" })).toBeInTheDocument();
    expect(tcard("新的版型").getByRole("link", { name: "看版型「新的版型」" })).toHaveAttribute("href", "/templates/10");
    expect(tcard("新的版型").getByRole("button", { name: "照這個範本做「新的版型」" })).toBeDisabled();
    expect(tcard("舊的版型").getByText("沒有讀成功")).toBeInTheDocument();
    expect(tcard("舊的版型").getByText("失敗")).toBeInTheDocument();
    expect(tcard("L2 學習單版型").getByText("3 區 · 英文指示 · 閱讀理解")).toBeInTheDocument();
    expect(tcard("L2 學習單版型").getByRole("link", { name: "照這個範本做「L2 學習單版型」" })).toHaveAttribute(
      "href",
      "/create/mimic?template=9",
    );

    // Asked again until nothing is being read any more, then left alone.
    await waitFor(() => expect(tcard("新的版型").queryByText("讀取中")).not.toBeInTheDocument());
    expect(tcard("新的版型").getByRole("link", { name: "照這個範本做「新的版型」" })).toBeInTheDocument();
    const settled = api.callsTo("GET", BROWSE).length;
    expect(settled).toBe(3);
    expect(api.calls.filter((c) => c.path === BROWSE).every((c) => c.url.includes("area=mine"))).toBe(true);
    await new Promise((r) => setTimeout(r, 80));
    expect(api.callsTo("GET", BROWSE).length).toBe(settled);
  });

  it("re-reads a failed one from its card", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${BROWSE}`]: templateBrowse([templateCard({ status: "failed", section_count: 0, section_titles: [] })]),
      [`POST ${T9}/retry`]: extracting(),
    });
    const user = userEvent.setup();
    renderApp(MINE);
    await user.click(await screen.findByRole("button", { name: "再讀一次「L2 學習單版型」" }));
    expect(await tcard("L2 學習單版型").findByText("讀取中", { selector: ".lib-badge" })).toBeInTheDocument();
    expect(api.callsTo("POST", `${T9}/retry`)).toHaveLength(1);
  });

  it("上傳我的學習單 sends the file as a worksheet sample, starts reading it, and says the privacy reminder once", async () => {
    const uploaded = materialDetail({ id: 6, title: "期中學習單" });
    let started = false;
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      [`GET ${BROWSE}`]: () =>
        templateBrowse(
          started
            ? [templateCard({ id: 11, title: "期中學習單版型", status: "extracting", section_count: 0, section_titles: [] })]
            : [],
        ),
      "POST /api/materials/upload": uploaded,
      [`POST ${TEMPLATES}`]: () => {
        started = true;
        return respond(202, extracting({ id: 11, title: "期中學習單版型", source_material: { id: 6, title: "期中學習單" } }));
      },
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp(MINE);
    expect(await screen.findByText(/^還沒有我的範本。/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "上傳我的學習單" }));
    const form = within(await screen.findByRole("form", { name: "上傳檔案" }));
    expect(screen.getAllByText(UPLOAD_WARNING)).toHaveLength(1);
    await user.upload(form.getByLabelText("選擇檔案"), new File(["PK"], "midterm.docx"));
    await user.click(form.getByRole("button", { name: "上傳，讓 Manna 讀設計" }));

    expect(await screen.findByText("Manna 正在讀「期中學習單版型」的設計，讀好了就能用。")).toBeInTheDocument();
    await screen.findByRole("article", { name: "期中學習單版型" });
    expect(tcard("期中學習單版型").getByText("讀取中", { selector: ".lib-badge" })).toBeInTheDocument();
    // The file went in as a worksheet sample (it stays out of 我的教材), then the template from it.
    const call = api.fetchMock.mock.calls.find(([url]) => String(url) === "/api/materials/upload");
    const sent = call?.[1]?.body as FormData;
    expect(sent.get("purpose")).toBe("worksheet_sample");
    expect(api.lastBody("POST", TEMPLATES)).toEqual({ material_id: 6 });
  });

  it("refuses a scan without text before asking the server", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "POST /api/materials/upload": materialDetail({ id: 6, text_status: "no_text", paragraphs: [] }),
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp(MINE);
    await user.click(await screen.findByRole("button", { name: "上傳我的學習單" }));
    const form = within(await screen.findByRole("form", { name: "上傳檔案" }));
    await user.upload(form.getByLabelText("選擇檔案"), new File(["%PDF"], "scan.pdf"));
    await user.click(form.getByRole("button", { name: "上傳，讓 Manna 讀設計" }));
    expect(await screen.findByText(/這份檔案讀不到文字（可能是掃描檔）/)).toBeInTheDocument();
    expect(api.callsTo("POST", TEMPLATES)).toHaveLength(0);
  });
});

// ---------------------------------------------------------------- the template page

const card = (n: number) => within(screen.getByRole("listitem", { name: `第 ${n} 區` }));
const titlesInOrder = () =>
  screen.getAllByRole("textbox", { name: "標題寫法" }).map((input) => (input as HTMLInputElement).value);

describe("/templates/:id", () => {
  it("shows the header facts, the sections in order, and the source material", async () => {
    mockApi({ ...baseRoutes(), [`GET ${T9}`]: detail() });
    renderApp("/templates/9");
    expect(await screen.findByRole("heading", { name: "L2 學習單版型", level: 1 })).toBeInTheDocument();
    expect(screen.getByText("課文深度閱讀 · 指示語：英文 · 抬頭：Name、No、Class · 列出學習目標 · 不附課文")).toBeInTheDocument();
    expect(titlesInOrder()).toEqual(["Warm-up: [主題]", "Paragraph [n]: Find", "Video talk"]);
    expect(card(2).getByRole("checkbox", { name: "逐段（每段各做一次）" })).toBeChecked();
    expect(card(3).getByRole("checkbox", { name: "需要我提供素材" })).toBeChecked();
    expect(card(1).getByText("Warm-up: [主題] 的用途", { exact: false })).toBeInTheDocument();
    // Item patterns are read-only, behind an expand.
    expect(screen.queryByText("What does [主題] mean?")).not.toBeInTheDocument();
    await userEvent.setup().click(card(1).getByRole("button", { name: "看 Manna 讀到的細節" }));
    expect(card(1).getByText("What does [主題] mean?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "看原本的學習單" })).toBeInTheDocument();
    // The answer-format chips are the fixed labels.
    const chips = within(card(1).getByRole("group", { name: "作答方式（第 1 區）" }))
      .getAllByRole("checkbox")
      .map((c) => (c as HTMLInputElement).labels?.[0]?.textContent);
    expect(chips).toEqual(["填空題", "選擇題", "是非題", "配對題", "簡答題", "表格整理", "圖像組織", "段落寫作"]);
  });

  it("edits, reorders and toggles 逐段, then saves a new version", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${T9}`]: detail(),
      [`PUT ${T9}`]: (call: MockCall) => {
        const body = call.body as UpdateWsTemplateRequest;
        return detail({ version_no: 2, content: body.content ?? content(), title: body.title ?? "L2 學習單版型" });
      },
    });
    const user = userEvent.setup();
    renderApp("/templates/9");
    await screen.findByRole("heading", { name: "L2 學習單版型", level: 1 });
    const save = screen.getByRole("button", { name: "儲存版型" });
    expect(save).toBeDisabled();

    // Reorder: the third section moves up, the first one down.
    await user.click(screen.getByRole("button", { name: "上移第 3 區" }));
    expect(titlesInOrder()).toEqual(["Warm-up: [主題]", "Video talk", "Paragraph [n]: Find"]);
    await user.click(screen.getByRole("button", { name: "下移第 1 區" }));
    expect(titlesInOrder()).toEqual(["Video talk", "Warm-up: [主題]", "Paragraph [n]: Find"]);
    expect(screen.getByRole("button", { name: "上移第 1 區" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下移第 3 區" })).toBeDisabled();

    // 逐段 on the (now) second, off on the third; 需要我提供素材 off on the first.
    await user.click(card(2).getByRole("checkbox", { name: "逐段（每段各做一次）" }));
    await user.click(card(3).getByRole("checkbox", { name: "逐段（每段各做一次）" }));
    await user.click(card(1).getByRole("checkbox", { name: "需要我提供素材" }));
    // Chips, item count, title pattern, instruction style.
    await user.click(card(2).getByRole("checkbox", { name: "選擇題" }));
    await user.click(card(2).getByRole("checkbox", { name: "簡答題" }));
    await user.selectOptions(card(2).getByRole("combobox", { name: /題數/ }), "12");
    const title = card(1).getByRole("textbox", { name: "標題寫法" });
    await user.clear(title);
    await user.type(title, "Watch and talk");
    const style = card(1).getByRole("textbox", { name: "指示語寫法" });
    await user.clear(style);
    await user.type(style, "中英並用");
    // Style notes as lines; a blank line is dropped.
    const notes = screen.getByRole("textbox", { name: "風格備註" });
    await user.type(notes, "{Enter}{Enter}指示一律用英文");

    expect(save).toBeEnabled();
    await user.click(save);
    expect(await screen.findByText("已存成第 2 版")).toBeInTheDocument();
    const body = api.lastBody("PUT", T9) as UpdateWsTemplateRequest;
    expect(body.title).toBeUndefined();
    const sent = body.content as WsTemplate;
    expect(sent.sections.map((s) => s.id)).toEqual(["s3", "s1", "s2"]);
    expect(sent.sections.map((s) => s.per_paragraph)).toEqual([false, true, false]);
    expect(sent.sections[0]).toMatchObject({
      title_pattern: "Watch and talk",
      instruction_style: "中英並用",
      needs_external_material: false,
    });
    expect(sent.sections[1]).toMatchObject({ answer_formats: ["選擇題"], item_count: 12 });
    expect(sent.style_notes).toEqual(["每題留兩行作答空間", "指示一律用英文"]);
    // The header facts are sent back unchanged.
    expect(sent).toMatchObject({ worksheet_kind: "課文深度閱讀", header_fields: ["Name", "No", "Class"] });
    expect(save).toBeDisabled();
  });

  it("deletes a section (never the last one) and renames without making a new version", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${T9}`]: detail({ content: content() }),
      [`PUT ${T9}`]: (call: MockCall) => {
        const body = call.body as UpdateWsTemplateRequest;
        return detail({ title: body.title ?? "L2 學習單版型", content: body.content ?? content() });
      },
    });
    const user = userEvent.setup();
    renderApp("/templates/9");
    await screen.findByRole("heading", { name: "L2 學習單版型", level: 1 });
    await user.click(screen.getByRole("button", { name: "刪除第 2 區" }));
    await user.click(screen.getByRole("button", { name: "刪除第 1 區" }));
    expect(titlesInOrder()).toEqual(["Video talk"]);
    expect(screen.getByRole("button", { name: "刪除第 1 區" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "放棄修改" }));
    expect(titlesInOrder()).toHaveLength(3);

    const name = screen.getByRole("textbox", { name: "版型名稱" });
    await user.clear(name);
    await user.type(name, "我的閱讀學習單");
    await user.click(screen.getByRole("button", { name: "儲存版型" }));
    expect(await screen.findByText("已儲存名稱")).toBeInTheDocument();
    expect(api.lastBody("PUT", T9)).toEqual({ title: "我的閱讀學習單" });
    expect(screen.getByRole("heading", { name: "我的閱讀學習單", level: 1 })).toBeInTheDocument();
  });

  it("refuses more than 8 style notes before sending", async () => {
    const api = mockApi({ ...baseRoutes(), [`GET ${T9}`]: detail() });
    const user = userEvent.setup();
    renderApp("/templates/9");
    const notes = await screen.findByRole("textbox", { name: "風格備註" });
    await user.clear(notes);
    await user.type(notes, Array.from({ length: 9 }, (_, i) => `note ${i + 1}`).join("{Enter}"));
    await user.click(screen.getByRole("button", { name: "儲存版型" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("風格備註最多 8 則，現在有 9 則。");
    expect(api.callsTo("PUT", T9)).toHaveLength(0);
  });

  it("shows why reading failed, retries, waits with the reading line, then opens the editor", async () => {
    let state: WsTemplateDetail = detail({ status: "failed", error: "AI 回應不完整", content: null, version_no: null, section_count: 0 });
    let polls = 0;
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${T9}`]: () => {
        if (state.status === "extracting") {
          polls += 1;
          if (polls >= 2) state = detail();
        }
        return state;
      },
      [`POST ${T9}/retry`]: () => {
        state = extracting();
        return respond(202, state);
      },
    });
    const user = userEvent.setup();
    renderApp("/templates/9");
    expect(await screen.findByRole("alert")).toHaveTextContent("Manna 沒有讀成功這份學習單的設計：AI 回應不完整");
    expect(screen.queryByRole("form", { name: "編修版型" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "再讀一次" }));
    expect(await screen.findByText(EXTRACTING)).toBeInTheDocument();
    expect(api.callsTo("POST", `${T9}/retry`)).toHaveLength(1);
    expect(await screen.findByRole("form", { name: "編修版型" })).toBeInTheDocument();
    expect(screen.queryByText(EXTRACTING)).not.toBeInTheDocument();
    expect(titlesInOrder()).toHaveLength(3);
  });

  it("says when retrying is refused", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${T9}`]: detail({ status: "failed", error: null, content: null, version_no: null }),
      [`POST ${T9}/retry`]: respond(400, { error: { code: "bad_request", message: "這份檔案讀不到文字（可能是掃描檔）" } }),
    });
    const user = userEvent.setup();
    renderApp("/templates/9");
    await user.click(await screen.findByRole("button", { name: "再讀一次" }));
    expect(await screen.findByText("這份檔案讀不到文字（可能是掃描檔）")).toBeInTheDocument();
  });

  it("deletes the template only after confirming, then goes back to 我的範本", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${T9}`]: detail(),
      [`DELETE ${T9}`]: { ok: true },
      "GET /api/materials": library(),
    });
    const user = userEvent.setup();
    renderApp("/templates/9");
    await user.click(await screen.findByRole("button", { name: "刪除這個版型" }));
    const dialog = within(screen.getByRole("alertdialog", { name: "確認刪除版型" }));
    await user.click(dialog.getByRole("button", { name: "取消" }));
    expect(api.callsTo("DELETE", T9)).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "刪除這個版型" }));
    await user.click(within(screen.getByRole("alertdialog", { name: "確認刪除版型" })).getByRole("button", { name: "永久刪除" }));
    expect(await screen.findByRole("heading", { name: "學習單範本", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "我的範本" })).toHaveAttribute("aria-selected", "true");
    expect(api.callsTo("DELETE", T9)).toHaveLength(1);
  });
});

// ---------------------------------------------------------------- 直接做一份

const P1 = "Mrs. Lin put an umbrella stand outside her shop.";

function wsState(overrides: Partial<WorksheetState> = {}): WorksheetState {
  return {
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
      paragraphs: [{ id: "p1", text: P1 }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
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
    template: null,
    ...overrides,
  };
}

const readyAndNot = {
  templates: [
    summary(),
    summary({ id: 12, title: "還在讀的", status: "extracting", version_no: null }),
    summary({ id: 13, title: "沒讀成的", status: "failed", version_no: null }),
  ],
};

describe("學習單: 直接做一份", () => {
  it("uploads a file by default, follows a template, and creates with material_id and template_id", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      "GET /api/materials": library(),
      [`GET ${TEMPLATES}`]: readyAndNot,
      "POST /api/materials/upload": materialDetail({ id: 6, title: "The Umbrella Stand" }),
      "POST /api/worksheets": wsState(),
      "GET /api/worksheets/5": wsState(),
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/worksheets");
    const upload = within(await screen.findByRole("region", { name: "上傳檔案" }));
    expect(screen.getByRole("button", { name: "上傳檔案" })).toHaveAttribute("aria-pressed", "true");
    // Only ready templates are offered; none is the default.
    // The select appears once the template list has loaded.
    const select = await screen.findByRole("combobox", { name: /照我的版型/ });
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["不使用版型", "L2 學習單版型（第 1 版）"]);
    expect(select).toHaveValue("");

    await user.upload(await upload.findByLabelText("選擇檔案"), new File(["PK"], "umbrella.docx"));
    await user.click(upload.getByRole("button", { name: "上傳" }));
    expect(await screen.findByText(/教材：「The Umbrella Stand」/)).toBeInTheDocument();
    await user.selectOptions(select, "9");
    await user.click(screen.getByRole("button", { name: "下一步：設定學習單" }));
    expect(await screen.findByRole("heading", { name: "做一份學習單", level: 1 })).toBeInTheDocument();
    expect(api.lastBody("POST", "/api/worksheets")).toEqual({ material_id: 6, template_id: 9 });
  });

  it("picks from the library without a template", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      "GET /api/materials": library([material({ title: "Umbrella text" }), material({ id: 7, title: "圖片", kind: "image", text_status: "not_applicable" })]),
      [`GET ${TEMPLATES}`]: readyAndNot,
      "POST /api/worksheets": wsState(),
      "GET /api/worksheets/5": wsState(),
    });
    const user = userEvent.setup();
    renderApp("/worksheets");
    await user.click(await screen.findByRole("button", { name: "從教材庫選" }));
    const picker = within(await screen.findByRole("region", { name: "從教材庫選" }));
    expect(picker.queryByRole("button", { name: "選用「圖片」" })).not.toBeInTheDocument();
    await user.click(await picker.findByRole("button", { name: "選用「Umbrella text」" }));
    await user.click(screen.getByRole("button", { name: "下一步：設定學習單" }));
    await screen.findByRole("heading", { name: "做一份學習單", level: 1 });
    expect(api.lastBody("POST", "/api/worksheets")).toEqual({ material_id: 5 });
  });

  it("pastes text with a template; asks for a material before creating from upload", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      "GET /api/materials": library(),
      [`GET ${TEMPLATES}`]: readyAndNot,
      "POST /api/worksheets": wsState(),
      "GET /api/worksheets/5": wsState(),
    });
    const user = userEvent.setup();
    renderApp("/worksheets");
    await screen.findByRole("region", { name: "上傳檔案" });
    await user.click(screen.getByRole("button", { name: "下一步：設定學習單" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("請先上傳教材檔案。");
    expect(api.callsTo("POST", "/api/worksheets")).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "貼上文字" }));
    await user.type(screen.getByRole("textbox", { name: "貼上教材文字" }), P1);
    await user.selectOptions(screen.getByRole("combobox", { name: /照我的版型/ }), "9");
    await user.click(screen.getByRole("button", { name: "下一步：設定學習單" }));
    await screen.findByRole("heading", { name: "做一份學習單", level: 1 });
    expect(api.lastBody("POST", "/api/worksheets")).toEqual({ material: { title: "", text: P1 }, template_id: 9 });
  });

  it("without templates there is no select, only a pointer to 學習單範本's 我的範本", async () => {
    mockApi({ ...baseRoutes(), "GET /api/worksheets": [], "GET /api/materials": library() });
    renderApp("/worksheets");
    const pointer = await screen.findByText(
      (_, el) => el?.tagName === "P" && (el.textContent ?? "").includes("到學習單範本上傳一份做過的學習單，就能建立版型"),
    );
    expect(within(pointer).getByRole("link", { name: "學習單範本" })).toHaveAttribute("href", "/templates?tab=mine");
    expect(screen.queryByRole("combobox", { name: /照我的版型/ })).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the worksheet settings view

describe("學習單設定: 照我的版型", () => {
  const WS = "/api/worksheets/5";
  const fromLesson = wsState({
    lesson: { id: 7, title: "第 2 節 精讀", unit_id: 1, unit_title: "L3" },
    material: { ...wsState().material, source: { kind: "lesson_sop", lesson_id: 7, material_version_no: 1, note: "" } },
  });
  const withTemplate = (base: WorksheetState, version_no = 1) =>
    wsState({
      ...base,
      template: { id: 9, title: "L2 學習單版型", version_no },
      settings: { ...emptySettings(), student_tasks: ["理解內容"], answer_formats: ["填空題", "簡答題"], generation: ["加入暖身活動"] },
    });

  it("sets a template on a worksheet made from a lesson, shows its skeleton, and removes it again", async () => {
    let current = fromLesson;
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${TEMPLATES}`]: readyAndNot,
      [`GET ${T9}`]: detail(),
      [`GET ${WS}`]: () => current,
      [`PUT ${WS}`]: (call: MockCall) => {
        const body = call.body as { template_id?: number | null };
        if ("template_id" in body) {
          current = body.template_id === null ? { ...current, template: null } : withTemplate(current);
        }
        return current;
      },
    });
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const select = await screen.findByRole("combobox", { name: "照我的版型" });
    expect(select).toHaveValue("");
    expect(screen.queryByText("會依這個版型產生：")).not.toBeInTheDocument();

    await user.selectOptions(select, "9");
    await waitFor(() => expect(api.lastBody("PUT", WS)).toEqual({ template_id: 9 }));
    expect(await screen.findByText("會依這個版型產生：")).toBeInTheDocument();
    const skeleton = within(screen.getByRole("list", { name: "版型的區塊" }));
    expect(skeleton.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Warm-up: [主題]（2 題）",
      "Paragraph [n]: Find（逐段，每段 3 題）",
      "Video talk（4 題，需要你提供素材）",
    ]);
    // The settings the server derived from the template are what the chips show.
    expect(screen.getByRole("checkbox", { name: /填空題/ })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /加入暖身活動/ })).toBeChecked();
    expect(screen.getByText("依版型：", { exact: false })).toHaveTextContent("依版型：L2 學習單版型 第 1 版");
    expect(screen.getByRole("link", { name: "L2 學習單版型" })).toHaveAttribute("href", "/templates/9");

    await user.selectOptions(select, "");
    await waitFor(() => expect(api.lastBody("PUT", WS)).toEqual({ template_id: null }));
    await waitFor(() => expect(screen.queryByText("會依這個版型產生：")).not.toBeInTheDocument());
    // Removing the template leaves the settings as they were.
    expect(screen.getByRole("checkbox", { name: /填空題/ })).toBeChecked();
  });

  it("offers the template's newer version when it was edited after being chosen", async () => {
    let current = withTemplate(wsState(), 1);
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${TEMPLATES}`]: { templates: [summary({ version_no: 2 })] },
      [`GET ${T9}`]: detail({ version_no: 2 }),
      [`GET ${WS}`]: () => current,
      [`PUT ${WS}`]: () => {
        current = withTemplate(wsState(), 2);
        return current;
      },
    });
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    expect(await screen.findByText(/這份學習單用的是第 1 版；版型後來改成第 2 版。/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "改用第 2 版" }));
    await waitFor(() => expect(api.lastBody("PUT", WS)).toEqual({ template_id: 9 }));
    expect(await screen.findByText("會依這個版型產生：")).toBeInTheDocument();
  });

  it("says why when the template cannot be set right now", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${TEMPLATES}`]: readyAndNot,
      [`GET ${WS}`]: wsState(),
      [`PUT ${WS}`]: respond(409, { error: { code: "conflict", message: "正在產生學習單，等產生完再換版型。" } }),
    });
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.selectOptions(await screen.findByRole("combobox", { name: "照我的版型" }), "9");
    expect(await screen.findByRole("alert")).toHaveTextContent("正在產生學習單，等產生完再換版型。");
  });

  it("a block that needs the teacher's own material says so plainly", async () => {
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
    const generated = withTemplate(
      wsState({
        generated: true,
        levels: ["standard"],
        content: {
          ...emptyWorksheetContent(),
          levels: ["standard"],
          blocks: [
            block("b1", { title: "Paragraph 1: Find", instructions: "Answer." }),
            block("b2", {
              title: "Video talk",
              instructions: "Watch the video.",
              message: "這一區需要你提供素材（例如影片或另一篇文章），Manna 不會自己編造。",
            }),
          ],
        },
      }),
    );
    mockApi({ ...baseRoutes(), [`GET ${TEMPLATES}`]: readyAndNot, [`GET ${WS}`]: generated });
    renderApp("/worksheets/5");
    const note = await screen.findByRole("note", { name: "需要你提供素材" });
    expect(note).toHaveTextContent("需要你提供素材");
    expect(note).toHaveTextContent("Manna 不會自己編造");
    expect(screen.getByText(/有 1 個區塊需要你提供素材/)).toBeInTheDocument();
    expect(screen.getByText("依版型：", { exact: false })).toHaveTextContent("依版型：L2 學習單版型 第 1 版");
  });
});
