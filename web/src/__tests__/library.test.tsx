// 教材參考、我的教材、學習單範本 (library contract, "Frontend: teacher side"): the sidebar's 資源 and the
// old address, each page's browse calls (area, q, category, tags, 只看 filters, sort, cursor), the
// cards (badges, meta, tags), the empty state, 用這篇出題, the picker (從教材挑選 / 從範本挑選: focus,
// tabs, preview, Escape), ?material= and ?template= on the create pages, and a 平台範本's page
// (read only, 另存成我的範本). Every title, text and file here is made up.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MaterialDetail, PrepSession, WsTemplate, WsTemplateDetail } from "../api/types";
import { libraryConfig } from "../components/library/useBrowse";
import { NO_TEXT_LINE } from "../components/material/MaterialChooser";
import {
  baseRoutes,
  materialBrowse,
  materialCard,
  mockApi,
  queryOf,
  renderApp,
  respond,
  signIn,
  teacher,
  templateBrowse,
  templateCard,
  type MockCall,
} from "../test/mockApi";

const MATERIALS = "/api/browse/materials";
const TEMPLATES = "/api/browse/templates";
const SESSIONS = "/api/prep-sessions";

beforeEach(() => {
  signIn();
  libraryConfig.searchDelayMs = 10;
});
afterEach(() => {
  libraryConfig.searchDelayMs = 300;
});

// ---------------------------------------------------------------- fixtures (all made up)

const paper = materialCard({
  id: 31,
  title: "114 學測英文試題（示範）",
  scope: "platform",
  category: "學測英文",
  tags: ["114", "試題", "學測", "閱讀", "示範"],
  kind: "document",
  file_type: "pdf",
  page_count: 7,
  paragraph_count: 38,
  word_count: 2873,
  excerpt: "Part One. Read the passage and answer the questions that follow. Bees do a much bigger job than most people think.",
  looks_like_worksheet: true,
  owner_label: "Manna 知識庫",
});

const syllabus = materialCard({
  id: 32,
  title: "英語文課綱摘要（示範）",
  scope: "platform",
  category: "課綱",
  tags: ["課綱"],
  kind: "text",
  file_type: "text",
  page_count: null,
  paragraph_count: 6,
  word_count: 0,
  excerpt: "核心素養：語言知識、溝通功能、學習策略。",
  owner_label: "Manna 知識庫",
});

const scan = materialCard({
  id: 33,
  title: "掃描的講義",
  scope: "platform",
  category: "其他",
  kind: "document",
  file_type: "pdf",
  page_count: 2,
  paragraph_count: 0,
  word_count: 0,
  excerpt: "",
  text_status: "no_text",
  owner_label: "Manna 知識庫",
});

const platformTemplate = templateCard({
  id: 21,
  title: "三民 L3 閱讀學習單（示範）",
  scope: "platform",
  category: "閱讀理解",
  tags: ["三民", "113"],
  section_count: 4,
  section_titles: ["Warm-up: [主題]", "Paragraph [n]: Find", "Main idea", "Exit ticket"],
  owner_label: "Manna 範本",
});

const detailOf = (id: number, title: string, overrides: Partial<MaterialDetail> = {}): MaterialDetail => ({
  id,
  kind: "document",
  origin: "upload",
  title,
  source_note: "",
  url: null,
  filename: "paper.pdf",
  content_type: "application/pdf",
  size_bytes: 4096,
  sha256: null,
  has_file: true,
  text_status: "ready",
  text_truncated: false,
  page_count: 7,
  paragraph_count: 2,
  word_count: 40,
  revision: 1,
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  lessons: [],
  worksheet_count: 0,
  annotation_count: 0,
  scope: "platform",
  text: "Part One.\n\nBees do a much bigger job than most people think.",
  paragraphs: [
    { id: "p1", no: 1, text: "Part One." },
    { id: "p2", no: 2, text: "Bees do a much bigger job than most people think." },
  ],
  ...overrides,
});

const content = (): WsTemplate => ({
  worksheet_kind: "課文深度閱讀",
  instruction_language: "英文",
  header_fields: ["Name", "Class"],
  shows_objectives: true,
  includes_passage_text: false,
  sections: [
    {
      id: "s1",
      title_pattern: "Warm-up: [主題]",
      purpose: "引起動機",
      block_kind: "warmup",
      per_paragraph: false,
      student_tasks: [],
      answer_formats: ["簡答題"],
      item_count: 2,
      text_scope: "",
      scaffolds: [],
      instruction_style: "",
      item_patterns: [],
      needs_external_material: false,
    },
    {
      id: "s2",
      title_pattern: "Paragraph [n]: Find",
      purpose: "逐段找細節",
      block_kind: "activity",
      per_paragraph: true,
      student_tasks: [],
      answer_formats: ["填空題"],
      item_count: 3,
      text_scope: "",
      scaffolds: [],
      instruction_style: "",
      item_patterns: [],
      needs_external_material: false,
    },
  ],
  generation_options: [],
  style_notes: ["每題留兩行作答空間"],
});

const templateDetail = (overrides: Partial<WsTemplateDetail> = {}): WsTemplateDetail => ({
  id: 21,
  title: "三民 L3 閱讀學習單（示範）",
  status: "ready",
  section_count: 2,
  source_material: { id: 41, title: "三民 L3 學習單原檔" },
  version_no: 1,
  updated_at: "2026-11-15T08:00:00Z",
  scope: "platform",
  category: "閱讀理解",
  tags: ["三民"],
  error: null,
  content: content(),
  ...overrides,
});

const created: PrepSession = {
  id: 7,
  title: "示範",
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

const browseCalls = (api: ReturnType<typeof mockApi>, path: string) =>
  api.calls.filter((c) => c.path === path).map((c) => queryOf(c));
const lastBrowse = (api: ReturnType<typeof mockApi>, path: string) => browseCalls(api, path).at(-1);
const card = (title: string) => within(screen.getByRole("article", { name: title }));

// ---------------------------------------------------------------- the sidebar and old addresses

describe("資源 in the sidebar", () => {
  it("lists 教材參考, 我的教材 and 學習單範本 under 資源 instead of 教材庫 and 我的版型", async () => {
    mockApi(baseRoutes());
    renderApp("/");
    const nav = within(await screen.findByRole("navigation", { name: "主要導覽" }));
    const resources = within(nav.getByRole("list", { name: "資源" }));
    expect(resources.getAllByRole("link").map((a) => [a.textContent, a.getAttribute("href")])).toEqual([
      ["教材參考", "/reference"],
      ["我的教材", "/materials"],
      ["學習單範本", "/templates"],
    ]);
    expect(nav.queryByRole("link", { name: "教材庫" })).not.toBeInTheDocument();
    expect(nav.queryByRole("link", { name: "我的版型" })).not.toBeInTheDocument();
  });

  it("/materials#templates goes on to /templates?tab=mine; a template page sits inside 學習單範本", async () => {
    mockApi({ ...baseRoutes(), "GET /api/worksheet-templates/21": templateDetail() });
    const { router } = renderApp("/materials#templates");
    await waitFor(() => expect(router.state.location.pathname).toBe("/templates"));
    expect(router.state.location.search).toBe("?tab=mine");
    await router.navigate("/templates/21");
    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    await waitFor(() => expect(nav.getByRole("link", { name: "學習單範本" })).toHaveAttribute("aria-current", "true"));
  });
});

// ---------------------------------------------------------------- 教材參考

describe("教材參考 (/reference)", () => {
  it("asks for area=reference, then follows the search, a category, tags, 只看 chips and the sort", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${MATERIALS}`]: materialBrowse([paper, syllabus], {
        facets: {
          categories: [
            { key: "學測英文", count: 12 },
            { key: "課綱", count: 1 },
          ],
          tags: [
            { key: "114", count: 6 },
            { key: "試題", count: 5 },
          ],
          file_types: [{ key: "pdf", count: 12 }],
        },
        total: 13,
      }),
    });
    const user = userEvent.setup();
    renderApp("/reference");
    expect(await screen.findByRole("heading", { name: "教材參考", level: 1 })).toBeInTheDocument();
    await screen.findByRole("article", { name: paper.title });
    expect(browseCalls(api, MATERIALS)).toEqual([{ area: "reference" }]);
    expect(screen.getByText("共 13 份")).toBeInTheDocument();

    // Category chips: 全部 first with the sum, each with its count, in the fixed list's order.
    const chips = within(screen.getByRole("group", { name: "分類" }));
    expect(chips.getAllByRole("button").map((b) => b.textContent)).toEqual(["全部 13", "學測英文 12", "課綱 1"]);
    expect(chips.getByRole("button", { name: "全部 13" })).toHaveAttribute("aria-pressed", "true");

    await user.type(screen.getByRole("searchbox", { name: "搜尋教材參考" }), "bees");
    await waitFor(() => expect(lastBrowse(api, MATERIALS)).toEqual({ area: "reference", q: "bees" }));

    await user.click(chips.getByRole("button", { name: "學測英文 12" }));
    expect(chips.getByRole("button", { name: "學測英文 12" })).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => expect(lastBrowse(api, MATERIALS)).toEqual({ area: "reference", q: "bees", category: "學測英文" }));

    const tags = within(screen.getByRole("group", { name: "標籤：" }));
    await user.click(tags.getByRole("button", { name: "114" }));
    await user.click(tags.getByRole("button", { name: "試題" }));
    await waitFor(() =>
      expect(lastBrowse(api, MATERIALS)).toEqual({
        area: "reference",
        q: "bees",
        category: "學測英文",
        tag: ["114", "試題"],
      }),
    );

    // 只看：PDF, Word and 文字 take one file type; 可直接出題 goes with any.
    const only = within(screen.getByRole("group", { name: "只看：" }));
    expect(only.getAllByRole("button").map((b) => b.textContent)).toEqual(["PDF", "Word", "文字", "可直接出題（≤40 段）"]);
    await user.click(only.getByRole("button", { name: "PDF" }));
    await user.click(only.getByRole("button", { name: "Word" }));
    expect(only.getByRole("button", { name: "PDF" })).toHaveAttribute("aria-pressed", "false");
    await user.click(only.getByRole("button", { name: "可直接出題（≤40 段）" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "排序" }), "標題");
    await waitFor(() =>
      expect(lastBrowse(api, MATERIALS)).toEqual({
        area: "reference",
        q: "bees",
        category: "學測英文",
        tag: ["114", "試題"],
        file_type: "docx",
        fits_lesson: "true",
        sort: "title",
      }),
    );
  });

  it("載入更多 asks with the cursor and adds the next cards", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${MATERIALS}`]: (call: MockCall) =>
        queryOf(call).cursor === "c2"
          ? materialBrowse([syllabus], { total: 2 })
          : materialBrowse([paper], { next_cursor: "c2", total: 2 }),
    });
    const user = userEvent.setup();
    renderApp("/reference");
    await screen.findByRole("article", { name: paper.title });
    await user.click(screen.getByRole("button", { name: "載入更多" }));
    expect(await screen.findByRole("article", { name: syllabus.title })).toBeInTheDocument();
    expect(screen.getByRole("article", { name: paper.title })).toBeInTheDocument();
    expect(lastBrowse(api, MATERIALS)).toEqual({ area: "reference", cursor: "c2" });
    expect(screen.queryByRole("button", { name: "載入更多" })).not.toBeInTheDocument();
  });

  it("draws each card: badges, the meta line, three tags then +n, whose it is and its category", async () => {
    mockApi({ ...baseRoutes(), [`GET ${MATERIALS}`]: materialBrowse([paper, syllabus, scan]) });
    renderApp("/reference");
    await screen.findByRole("article", { name: paper.title });

    const exam = card(paper.title);
    expect(exam.getByText("PDF", { selector: ".lib-badge" })).toBeInTheDocument();
    expect(exam.getByText("學習單？", { selector: ".lib-badge" })).toBeInTheDocument();
    expect(exam.getByText("PDF · 7 頁 · 約 2,900 字")).toBeInTheDocument();
    expect(within(exam.getByRole("list", { name: "標籤" })).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "114",
      "試題",
      "學測",
      "+2",
    ]);
    expect(exam.getByText("Manna 知識庫")).toBeInTheDocument();
    expect(exam.getByText("學測英文", { selector: ".lib-card-category" })).toBeInTheDocument();
    // The paper preview is the excerpt, set as print.
    expect(exam.getByText(/^Part One\. Read the passage/)).toHaveClass("lib-paper-text");

    const pasted = card(syllabus.title);
    expect(pasted.getByText("文字", { selector: ".lib-badge" })).toBeInTheDocument();
    expect(pasted.getByText("貼上的文字 · 6 段")).toBeInTheDocument();
    expect(pasted.queryByText("學習單？")).not.toBeInTheDocument();

    // Without text it cannot be used for questions.
    const scanned = card(scan.title);
    expect(scanned.getByText("PDF · 2 頁 · 沒有可選取的文字")).toBeInTheDocument();
    expect(scanned.getByRole("button", { name: `用這篇出題「${scan.title}」` })).toBeDisabled();
  });

  it("says 沒有符合的教材 with 清除條件, which starts over", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${MATERIALS}`]: (call: MockCall) => materialBrowse(queryOf(call).q ? [] : [paper]),
    });
    const user = userEvent.setup();
    renderApp("/reference");
    await screen.findByRole("article", { name: paper.title });
    await user.type(screen.getByRole("searchbox", { name: "搜尋教材參考" }), "zzz{Enter}");
    expect(await screen.findByText(/^沒有符合的教材/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "清除條件" }));
    expect(await screen.findByRole("article", { name: paper.title })).toBeInTheDocument();
    expect(screen.getByRole("searchbox", { name: "搜尋教材參考" })).toHaveValue("");
    expect(lastBrowse(api, MATERIALS)).toEqual({ area: "reference" });
  });

  it("預覽 opens the viewer (with the teacher's own notes); 用這篇出題 lists the tools and fills the article slot (?material=)", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${MATERIALS}`]: materialBrowse([paper]),
      "GET /api/materials/31": detailOf(31, paper.title, { looks_like_worksheet: false }),
      "GET /api/materials/31/annotations": [],
      [`POST ${SESSIONS}`]: created,
      [`GET ${SESSIONS}/7`]: created,
    });
    const user = userEvent.setup();
    const { router } = renderApp("/reference");
    await screen.findByRole("article", { name: paper.title });

    await user.click(card(paper.title).getByRole("button", { name: `預覽「${paper.title}」` }));
    const viewer = within(await screen.findByRole("complementary", { name: "教材" }));
    expect(await viewer.findByText("Bees do a much bigger job than most people think.")).toBeInTheDocument();
    // The material is the platform's; the notes are this teacher's own, as on her materials.
    await waitFor(() => expect(api.callsTo("GET", "/api/materials/31/annotations").length).toBeGreaterThan(0));
    await user.click(viewer.getByRole("button", { name: "收起教材" }));

    await user.click(card(paper.title).getByRole("button", { name: `用這篇出題「${paper.title}」` }));
    const tools = within(screen.getByRole("group", { name: "用這篇出題" }));
    expect(tools.getAllByRole("link").map((a) => [a.textContent, a.getAttribute("href")])).toEqual([
      ["照我的學習單做", "/create/mimic?material=31"],
      ["閱讀理解題", "/create/comprehension?material=31"],
      ["差異化學習單", "/create/tiered?material=31"],
      ["課堂活動", "/create/activity?material=31"],
      ["教學套件", "/create/kit?material=31"],
      ["完整學習單", "/create/worksheet?material=31"],
    ]);
    await user.click(tools.getByRole("link", { name: "閱讀理解題" }));

    // The generator reads it by id, shows it as the chip, and the parameter leaves the address.
    expect(await screen.findByRole("button", { name: `移除「${paper.title}」` })).toBeInTheDocument();
    await waitFor(() => expect(router.state.location.search).toBe(""));
    expect(router.state.location.pathname).toBe("/create/comprehension");
    await user.click(screen.getByRole("button", { name: "產生" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({ kind: "comprehension", request: "閱讀理解題", material_id: 31 });
  });
});

// ---------------------------------------------------------------- ?material= and ?template=

describe("pre-selection from an address", () => {
  it("?material=<id> without text says so and leaves the article box", async () => {
    mockApi({ ...baseRoutes(), "GET /api/materials/33": detailOf(33, scan.title, { text_status: "no_text" }) });
    const { router } = renderApp("/create/activity?material=33");
    expect(await screen.findByText(NO_TEXT_LINE)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "文章或主題" })).toBeInTheDocument();
    await waitFor(() => expect(router.state.location.search).toBe(""));
  });

  it("?material=<id> on 照我的學習單做 fills 這節課的課文", async () => {
    mockApi({ ...baseRoutes(), "GET /api/materials/31": detailOf(31, paper.title) });
    renderApp("/create/mimic?material=31");
    const lesson = within(await screen.findByRole("region", { name: "這節課的課文" }));
    expect(await lesson.findByRole("button", { name: `移除「${paper.title}」` })).toBeInTheDocument();
  });

  it("?template=<id> on 照我的學習單做 makes that template the sample; one still being read says so", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheet-templates/21": templateDetail(),
      "GET /api/worksheet-templates/22": templateDetail({ id: 22, status: "extracting", content: null }),
      [`POST ${SESSIONS}`]: created,
      [`GET ${SESSIONS}/7`]: created,
    });
    const user = userEvent.setup();
    const { router } = renderApp("/create/mimic?template=21");
    const sample = within(await screen.findByRole("region", { name: "你的學習單範本" }));
    expect(await sample.findByRole("status")).toHaveTextContent(`${platformTemplate.title}· 2 區`);
    expect(sample.getByRole("link", { name: "看版型內容" })).toHaveAttribute("href", "/templates/21");
    await waitFor(() => expect(router.state.location.search).toBe(""));

    await user.click(screen.getByRole("button", { name: "用範例試試" }));
    await user.click(screen.getByRole("button", { name: "產生" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({ kind: "worksheet", template_id: 21 });

    await router.navigate("/create/mimic?template=22");
    expect(await screen.findByText(/這個範本還沒準備好/)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the picker

describe("從教材挑選 (the picker)", () => {
  it("opens with the search focused on 教材參考; 我的教材 asks area=mine; Escape closes and gives focus back", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${MATERIALS}`]: (call: MockCall) =>
        materialBrowse(queryOf(call).area === "mine" ? [materialCard({ id: 5, title: "我的講義" })] : [paper]),
    });
    const user = userEvent.setup();
    renderApp("/create/comprehension");
    const open = await screen.findByRole("button", { name: "從教材挑選" });
    await user.click(open);
    const dialog = within(await screen.findByRole("dialog", { name: "從教材挑選" }));
    expect(dialog.getByRole("searchbox", { name: "搜尋教材參考" })).toHaveFocus();
    expect(dialog.getByRole("tab", { name: "教材參考" })).toHaveAttribute("aria-selected", "true");
    await dialog.findByRole("article", { name: paper.title });

    await user.click(dialog.getByRole("tab", { name: "我的教材" }));
    expect(await dialog.findByRole("article", { name: "我的講義" })).toBeInTheDocument();
    expect(browseCalls(api, MATERIALS)).toEqual([{ area: "reference" }, { area: "mine" }]);

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(open).toHaveFocus();
  });

  it("預覽 reads the material inside the dialog; Escape goes back to the list; 選用 from the preview fills the slot", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${MATERIALS}`]: materialBrowse([paper]),
      "GET /api/materials/31": detailOf(31, paper.title),
    });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(within(screen.getByRole("group", { name: "附上文章" })).getByRole("button", { name: "從教材挑選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從教材挑選" }));
    await user.click(await dialog.findByRole("button", { name: `預覽「${paper.title}」` }));
    const preview = within(dialog.getByRole("region", { name: `預覽「${paper.title}」` }));
    expect(await preview.findByText("Bees do a much bigger job than most people think.")).toBeInTheDocument();
    expect(dialog.queryByRole("article", { name: paper.title })).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "從教材挑選" })).toBeInTheDocument();
    expect(await dialog.findByRole("article", { name: paper.title })).toBeVisible();
    await waitFor(() => expect(dialog.getByRole("button", { name: `預覽「${paper.title}」` })).toHaveFocus());

    await user.click(dialog.getByRole("button", { name: `預覽「${paper.title}」` }));
    await user.click(within(dialog.getByRole("region", { name: `預覽「${paper.title}」` })).getByRole("button", { name: "選用" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: `移除「${paper.title}」` })).toBeInTheDocument();
    // A worksheet-looking material still asks whether it is a sample.
    expect(screen.getByRole("group", { name: "這份看起來是一份學習單。要照它的樣式為新課文出題嗎？" })).toBeInTheDocument();
  });

  it("從範本挑選 on the mimic page: 平台範本 cards show their sections, 看版型 opens the page, 選用 fills the sample", async () => {
    const api = mockApi({ ...baseRoutes(), [`GET ${TEMPLATES}`]: templateBrowse([platformTemplate]) });
    const user = userEvent.setup();
    renderApp("/create/mimic");
    const sample = within(await screen.findByRole("region", { name: "你的學習單範本" }));
    await user.click(sample.getByRole("button", { name: "從範本挑選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從範本挑選" }));
    const tcard = within(await dialog.findByRole("article", { name: platformTemplate.title }));
    expect(tcard.getByText("平台範本", { selector: ".lib-badge" })).toBeInTheDocument();
    expect(tcard.getByText("4 區 · 英文指示 · 閱讀理解")).toBeInTheDocument();
    expect(tcard.getByText("Manna 範本")).toBeInTheDocument();
    expect(Array.from(tcard.getByText("Main idea").closest("ol")!.children).map((li) => li.textContent)).toEqual([
      "Warm-up: [主題]",
      "Paragraph [n]: Find",
      "Main idea",
      "Exit ticket",
    ]);
    expect(tcard.getByRole("link", { name: `看版型「${platformTemplate.title}」` })).toHaveAttribute("href", "/templates/21");

    // 只看 英文指示 asks with lang.
    await user.click(within(dialog.getByRole("group", { name: "只看：" })).getByRole("button", { name: "英文指示" }));
    await waitFor(() => expect(lastBrowse(api, TEMPLATES)).toEqual({ area: "platform", lang: "英文" }));

    await user.click(tcard.getByRole("button", { name: `選用「${platformTemplate.title}」` }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(sample.getByRole("status")).toHaveTextContent(`${platformTemplate.title}· 4 區`);
  });
});

// ---------------------------------------------------------------- 學習單範本 and a 平台範本

describe("學習單範本 (/templates)", () => {
  it("opens on 平台範本 (area=platform); 我的範本 is ?tab=mine (area=mine) with 上傳我的學習單", async () => {
    const api = mockApi({ ...baseRoutes(), [`GET ${TEMPLATES}`]: templateBrowse([platformTemplate]) });
    const user = userEvent.setup();
    const { router } = renderApp("/templates");
    expect(await screen.findByRole("heading", { name: "學習單範本", level: 1 })).toBeInTheDocument();
    await screen.findByRole("article", { name: platformTemplate.title });
    expect(screen.getByRole("tab", { name: "平台範本" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("button", { name: "上傳我的學習單" })).not.toBeInTheDocument();
    expect(card(platformTemplate.title).getByRole("link", { name: `照這個範本做「${platformTemplate.title}」` })).toHaveAttribute(
      "href",
      "/create/mimic?template=21",
    );

    await user.click(within(screen.getByRole("group", { name: "分類" })).getByRole("button", { name: "閱讀理解 1" }));
    await waitFor(() => expect(lastBrowse(api, TEMPLATES)).toEqual({ area: "platform", category: "閱讀理解" }));

    await user.click(screen.getByRole("tab", { name: "我的範本" }));
    await waitFor(() => expect(router.state.location.search).toBe("?tab=mine"));
    expect(screen.getByRole("button", { name: "上傳我的學習單" })).toBeInTheDocument();
    await waitFor(() => expect(lastBrowse(api, TEMPLATES)).toEqual({ area: "mine" }));
  });

  it("看原檔 opens the original file of a platform template in the viewer", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${TEMPLATES}`]: templateBrowse([platformTemplate]),
      "GET /api/materials/4": detailOf(4, "L2 學習單"),
      "GET /api/materials/4/annotations": [],
    });
    const user = userEvent.setup();
    renderApp("/templates");
    await screen.findByRole("article", { name: platformTemplate.title });
    await user.click(card(platformTemplate.title).getByRole("button", { name: `看原檔「${platformTemplate.title}」` }));
    const viewer = within(await screen.findByRole("complementary", { name: "教材" }));
    expect(await viewer.findByText("Bees do a much bigger job than most people think.")).toBeInTheDocument();
  });
});

describe("a 平台範本's page", () => {
  it("is read only, offers 照這個範本做 and 看原檔, and 另存成我的範本 makes a personal copy to edit", async () => {
    const copy = templateDetail({ id: 30, title: `${platformTemplate.title}（我的版本）`, scope: "personal" });
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheet-templates/21": templateDetail(),
      "POST /api/worksheet-templates/21/copy": copy,
      "GET /api/worksheet-templates/30": copy,
    });
    const user = userEvent.setup();
    const { router } = renderApp("/templates/21");
    expect(await screen.findByRole("heading", { name: platformTemplate.title, level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/這是平台範本，只能看、不能改/)).toBeInTheDocument();
    // What it asks for, section by section, without anything to edit.
    const sections = within(screen.getByRole("region", { name: "範本內容" }));
    expect(sections.getByRole("listitem", { name: "第 2 區" })).toHaveTextContent("Paragraph [n]: Find（逐段，每段 3 題）");
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "儲存版型" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "刪除這個版型" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "照這個範本做" })).toHaveAttribute("href", "/create/mimic?template=21");
    expect(screen.getByRole("button", { name: "看原檔" })).toHaveAttribute("aria-expanded", "false");

    await user.click(screen.getByRole("button", { name: "另存成我的範本" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/templates/30"));
    expect(api.callsTo("POST", "/api/worksheet-templates/21/copy")).toHaveLength(1);
    // The copy is mine: it can be edited.
    expect(await screen.findByRole("heading", { name: copy.title, level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("form", { name: "編修版型" })).toBeInTheDocument();
  });

  it("says why when the copy is refused (still being read)", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/worksheet-templates/21": templateDetail(),
      "POST /api/worksheet-templates/21/copy": respond(409, {
        error: { code: "conflict", message: "這個範本還在讀取中，讀好之後才能另存。" },
      }),
    });
    const user = userEvent.setup();
    renderApp("/templates/21");
    await user.click(await screen.findByRole("button", { name: "另存成我的範本" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("這個範本還在讀取中，讀好之後才能另存。");
    expect(screen.getByRole("button", { name: "另存成我的範本" })).toBeEnabled();
  });

  it("a knowledge-base manager (kb_manager) edits a platform template here, and is told every teacher sees it", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/me": { ...teacher, role: "kb_manager" },
      "GET /api/worksheet-templates/21": templateDetail(),
    });
    renderApp("/templates/21");
    expect(await screen.findByRole("form", { name: "編修版型" })).toBeInTheDocument();
    expect(screen.getByText("這是平台範本：你在這裡的修改，所有老師都會看到。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "另存成我的範本" })).not.toBeInTheDocument();
  });
});
