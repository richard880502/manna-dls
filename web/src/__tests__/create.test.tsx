// 建立 (/create/:tool), the generator from Bloom contract Addendum 1 (like Eduaide's): the tool
// switcher in the heading, the 內容 card (objective, article, file or library instead, the exemplar),
// the 設定 card per tool (班級, 題數, Bloom presets, 題型, 版型), the sticky summary and the POST body.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { Material, PrepSession, WsTemplateSummary } from "../api/types";
import {
  BLOOM_LEVELS,
  BLOOM_PRESETS,
  countsTotal,
  presetCounts,
  type BloomCounts,
} from "../components/bloom/presets";
import { EXEMPLAR_TEXT } from "../lib/create";
import { baseRoutes, emptyLibrary, mockApi, renderApp, schoolClass, signIn } from "../test/mockApi";

const SESSIONS = "/api/prep-sessions";

const counts = (...n: number[]): BloomCounts => {
  const out = {} as BloomCounts;
  BLOOM_LEVELS.forEach((l, i) => {
    out[l] = n[i] ?? 0;
  });
  return out;
};

const pct = (id: string) => BLOOM_PRESETS.find((p) => p.id === id)!.pct;

const material: Material = {
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
};

const template: WsTemplateSummary = {
  id: 9,
  title: "L2 學習單版型",
  status: "ready",
  section_count: 3,
  source_material: { id: 4, title: "L2 學習單" },
  version_no: 1,
  updated_at: "2026-11-14T08:00:00Z",
};

const created: PrepSession = {
  id: 7,
  title: "Bees",
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
    "GET /api/materials": { ...emptyLibrary(), materials: [material] },
    [`POST ${SESSIONS}`]: created,
    [`GET ${SESSIONS}/7`]: created,
    ...extra,
  };
}

const heading = () => screen.getByRole("heading", { level: 1 });
const article = () => screen.getByRole("textbox", { name: "文章或主題" });
const summaryText = () => (document.querySelector(".create-summary") as HTMLElement).textContent;
const generate = () => screen.getByRole("button", { name: "產生" });

async function open(path: string) {
  const rendered = renderApp(path);
  await waitFor(() => expect(heading()).toHaveTextContent(/^來做一份/));
  return rendered;
}

// ---------------------------------------------------------------- presets

describe("Bloom presets → per-level counts (largest remainder, at least one per level that has a share)", () => {
  it.each([
    ["balanced", 8, counts(1, 2, 2, 1, 1, 1)],
    ["balanced", 12, counts(2, 3, 3, 2, 1, 1)],
    ["balanced", 30, counts(5, 7, 6, 6, 3, 3)],
    ["foundation", 8, counts(3, 3, 2, 0, 0, 0)],
    ["challenge", 8, counts(0, 1, 2, 2, 2, 1)],
    ["higher", 3, counts(0, 0, 0, 1, 1, 1)],
    ["higher", 8, counts(0, 0, 0, 2, 3, 3)],
    // Fewer questions than levels: the largest shares get one each.
    ["balanced", 3, counts(0, 1, 1, 1, 0, 0)],
  ])("%s × %i", (id, total, expected) => {
    expect(presetCounts(pct(id), total)).toEqual(expected);
  });

  it("always adds up to the total, and gives every level with a share at least one", () => {
    for (const p of BLOOM_PRESETS) {
      const levels = p.pct.filter((x) => x > 0).length;
      for (let total = 3; total <= 30; total += 1) {
        const c = presetCounts(p.pct, total);
        expect(countsTotal(c)).toBe(total);
        BLOOM_LEVELS.forEach((l, i) => {
          if (p.pct[i] === 0) expect(c[l]).toBe(0);
          else if (total >= levels) expect(c[l]).toBeGreaterThanOrEqual(1);
        });
      }
    }
  });
});

// ---------------------------------------------------------------- the generator

describe("建立: the generator", () => {
  it("the tool name in the heading switches tools and keeps what was filled in", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = await open("/create/comprehension");
    expect(heading()).toHaveTextContent("來做一份 閱讀理解題");
    expect(screen.getByText(/Manna 依 Bloom 認知層次出閱讀理解題/)).toBeInTheDocument();
    // The sidebar marks the tool.
    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    expect(nav.getByRole("link", { name: "閱讀理解題" })).toHaveAttribute("aria-current", "page");

    await user.type(screen.getByRole("textbox", { name: "學習目標（選填）" }), "學生能說出蜜蜂的重要");
    await user.type(article(), "Bees are busy.");
    await user.click(screen.getByRole("button", { name: "題數：多一題" }));
    expect(within(screen.getByRole("group", { name: "題數" })).getByText("9")).toBeInTheDocument();

    const switcher = within(heading()).getByRole("button", { name: "閱讀理解題" });
    await user.click(switcher);
    expect(switcher).toHaveAttribute("aria-expanded", "true");
    const tools = within(screen.getByRole("group", { name: "換一種" }));
    expect(tools.getAllByRole("link").map((a) => a.textContent)).toEqual([
      "照我的學習單做",
      "閱讀理解題",
      "分層練習",
      "課堂活動",
      "教學套件",
      "完整學習單",
    ]);
    // Escape closes the list and gives focus back.
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "換一種" })).not.toBeInTheDocument();
    expect(switcher).toHaveFocus();

    await user.click(switcher);
    await user.click(within(screen.getByRole("group", { name: "換一種" })).getByRole("link", { name: "分層練習" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/create/tiered"));
    expect(heading()).toHaveTextContent("來做一份 分層練習");
    expect(screen.queryByRole("group", { name: "換一種" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "學習目標（選填）" })).toHaveValue("學生能說出蜜蜂的重要");
    expect(article()).toHaveValue("Bees are busy.");
    expect(within(screen.getByRole("group", { name: "題數" })).getByText("9")).toBeInTheDocument();
  });

  it("題數 starts at 12 for 分層練習 (8 elsewhere)", async () => {
    signIn();
    mockApi(routes());
    await open("/create/tiered");
    expect(within(screen.getByRole("group", { name: "題數" })).getByText("12")).toBeInTheDocument();
    expect(summaryText()).toBe("12 題 · 記憶 2・理解 3・應用 3・分析 2・評鑑 1・創造 1 · 約 30 分鐘");
  });

  it.each(["/create", "/create/nonsense"])("%s opens 閱讀理解題", async (path) => {
    signIn();
    mockApi(routes());
    const { router } = renderApp(path);
    await waitFor(() => expect(router.state.location.pathname).toBe("/create/comprehension"));
    expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("來做一份 閱讀理解題");
  });

  it("presets show as stacked bars with a legend; the summary follows the preset and the count", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    await open("/create/comprehension");
    const bloom = within(screen.getByRole("group", { name: "認知層次（Bloom）" }));
    expect(bloom.getAllByRole("radio").map((r) => r.closest("label")?.querySelector(".bloom-preset-name")?.textContent)).toEqual([
      "基礎鞏固",
      "均衡（預設）",
      "思考挑戰",
      "深度思考",
      "自訂",
    ]);
    expect(bloom.getByRole("radio", { name: "均衡（預設）" })).toBeChecked();
    expect(bloom.getByRole("radio", { name: "基礎鞏固" })).toHaveAccessibleDescription("記憶 40%、理解 40%、應用 20%");
    expect(within(bloom.getByRole("list", { name: "顏色說明" })).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "記憶",
      "理解",
      "應用",
      "分析",
      "評鑑",
      "創造",
    ]);
    expect(summaryText()).toBe("8 題 · 記憶 1・理解 2・應用 2・分析 1・評鑑 1・創造 1 · 約 20 分鐘");
    // A label on the spec's forbidden list (§5) never shows.
    expect(screen.queryByText(/高層次思考/)).not.toBeInTheDocument();

    await user.click(bloom.getByRole("radio", { name: "深度思考" }));
    expect(summaryText()).toBe("8 題 · 分析 2・評鑑 3・創造 3 · 約 20 分鐘");
    await user.click(screen.getByRole("button", { name: "題數：少一題" }));
    expect(summaryText()).toBe("7 題 · 分析 2・評鑑 3・創造 2 · 約 18 分鐘");
  });

  it("自訂 starts from the chosen preset, has a stepper per level, and holds 產生 back outside 3–30", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await open("/create/comprehension");
    const bloom = within(screen.getByRole("group", { name: "認知層次（Bloom）" }));
    await user.click(bloom.getByRole("radio", { name: "基礎鞏固" }));
    await user.click(bloom.getByRole("radio", { name: "自訂" }));
    const custom = within(screen.getByRole("group", { name: "每個層次的題數" }));
    expect(within(custom.getByRole("group", { name: "記憶" })).getByText("3")).toBeInTheDocument();
    expect(within(custom.getByRole("group", { name: "應用" })).getByText("2")).toBeInTheDocument();
    // The total comes from the levels now.
    expect(screen.queryByRole("group", { name: "題數" })).not.toBeInTheDocument();

    await user.click(custom.getByRole("button", { name: "創造：多一題" }));
    expect(summaryText()).toBe("9 題 · 記憶 3・理解 3・應用 2・創造 1 · 約 23 分鐘");

    for (const level of ["記憶", "記憶", "記憶", "理解", "理解", "理解", "應用", "應用"]) {
      await user.click(custom.getByRole("button", { name: `${level}：少一題` }));
    }
    expect(custom.getByRole("alert")).toHaveTextContent("合計 1 題；要在 3 到 30 題之間。");
    expect(generate()).toBeDisabled();

    await user.click(custom.getByRole("button", { name: "分析：多一題" }));
    await user.click(custom.getByRole("button", { name: "評鑑：多一題" }));
    expect(generate()).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "用範例試試" }));
    await user.click(generate());
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({
      options: { bloom_counts: { analyze: 1, evaluate: 1, create: 1 }, question_count: 3 },
    });
  });

  it("用範例試試 fills the article box with an original passage; the counter follows", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    await open("/create/comprehension");
    expect(screen.getByText("0/30000")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "用範例試試" }));
    expect(article()).toHaveValue(EXEMPLAR_TEXT);
    expect(EXEMPLAR_TEXT.split("\n\n")).toHaveLength(6);
    expect(EXEMPLAR_TEXT.split("\n\n")[0]).toBe("Why Bees Matter");
    expect(screen.getByText(`${EXEMPLAR_TEXT.length}/30000`)).toBeInTheDocument();
  });

  it("without an article or a material, 產生 says 先附上文章或教材 and sends nothing", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await open("/create/comprehension");
    await user.type(screen.getByRole("textbox", { name: "學習目標（選填）" }), "目標");
    await user.click(generate());
    expect(screen.getByRole("alert")).toHaveTextContent("先附上文章或教材");
    expect(article()).toHaveFocus();
    expect(article()).toHaveAttribute("aria-invalid", "true");
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);

    await user.type(article(), "Bees are busy.");
    expect(screen.queryByText("先附上文章或教材")).not.toBeInTheDocument();
    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
  });

  it("a library material shows as a chip instead of the article box, and goes as material_id", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await open("/create/activity");
    await user.type(article(), "this text gives way");
    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從教材庫選" }));
    await user.click(await dialog.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    expect(screen.queryByRole("textbox", { name: "文章或主題" })).not.toBeInTheDocument();
    expect(screen.queryByText(/\/30000$/)).not.toBeInTheDocument();
    // Taking the chip away brings the pasted text back.
    await user.click(screen.getByRole("button", { name: "移除「The Umbrella Stand」" }));
    expect(article()).toHaveValue("this text gives way");

    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    await user.click(await screen.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ kind: "activity", request: "課堂活動", material_id: 5 });
  });

  it("閱讀理解題: objective, class, Bloom counts, 題型 and the pasted text make the POST body", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await open("/create/comprehension");
    const objective = screen.getByRole("textbox", { name: "學習目標（選填）" });
    expect(objective).toHaveAttribute("placeholder", "例如：學生能分析作者如何用例子支持論點");
    await user.type(objective, "學生能分析作者的論點");
    await user.type(article(), "Bees are busy.");

    const klass = screen.getByRole("combobox", { name: "班級／程度" });
    expect(klass).toHaveDisplayValue("不指定");
    await user.selectOptions(klass, "高一 3 班");

    const formats = within(screen.getByRole("group", { name: "題型" }));
    expect(formats.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "選擇題",
      "是非題",
      "配對題",
      "填空題",
      "簡答題",
      "表格整理",
      "段落寫作",
    ]);
    expect(screen.getByText("沒有選：自動（依每個層次適合的題型）")).toBeInTheDocument();
    await user.click(formats.getByRole("button", { name: "簡答題" }));
    await user.click(formats.getByRole("button", { name: "選擇題" }));
    expect(formats.getByRole("button", { name: "選擇題" })).toHaveAttribute("aria-pressed", "true");
    // No 版型 outside 完整學習單.
    expect(screen.queryByRole("combobox", { name: "版型" })).not.toBeInTheDocument();

    await user.click(generate());
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "comprehension",
      request: "學生能分析作者的論點",
      material: { title: "", text: "Bees are busy." },
      class_id: 3,
      options: {
        bloom_counts: { remember: 1, understand: 2, apply: 2, analyze: 1, evaluate: 1, create: 1 },
        question_count: 8,
        answer_formats: ["選擇題", "簡答題"],
        objective: "學生能分析作者的論點",
      },
    });
  });

  it.each([
    [
      "tiered",
      { bloom: true, formats: true, template: false },
      {
        kind: "tiered",
        request: "分層練習",
        options: { bloom_counts: { remember: 2, understand: 3, apply: 3, analyze: 2, evaluate: 1, create: 1 }, question_count: 12 },
      },
    ],
    ["activity", { bloom: false, formats: false, template: false }, { kind: "activity", request: "課堂活動" }],
    [
      "kit",
      { bloom: true, formats: false, template: false },
      {
        kind: "kit",
        request: "教學套件",
        options: { bloom_counts: { remember: 1, understand: 2, apply: 2, analyze: 1, evaluate: 1, create: 1 }, question_count: 8 },
      },
    ],
    [
      "worksheet",
      { bloom: true, formats: true, template: true },
      {
        kind: "worksheet",
        request: "學習單",
        template_id: 9,
        options: { bloom_counts: { remember: 1, understand: 2, apply: 2, analyze: 1, evaluate: 1, create: 1 }, question_count: 8 },
      },
    ],
  ] as const)("%s shows only its settings and sends its own body", async (tool, shows, body) => {
    signIn();
    const api = mockApi(routes({ "GET /api/worksheet-templates": { templates: [template] } }));
    const user = userEvent.setup();
    await open(`/create/${tool}`);
    expect(screen.getByRole("combobox", { name: "班級／程度" })).toBeInTheDocument();
    expect(!!screen.queryByRole("group", { name: "認知層次（Bloom）" })).toBe(shows.bloom);
    expect(!!screen.queryByRole("group", { name: "題數" })).toBe(shows.bloom);
    expect(!!screen.queryByRole("group", { name: "題型" })).toBe(shows.formats);
    if (shows.template) {
      const select = await screen.findByRole("combobox", { name: "版型" });
      expect(select).toHaveDisplayValue("不使用版型");
      await user.selectOptions(select, "9");
    } else {
      expect(screen.queryByRole("combobox", { name: "版型" })).not.toBeInTheDocument();
    }
    if (!shows.bloom) expect(summaryText()).toBe("課堂活動 · 引起動機到統整的一節課流程");

    await user.click(screen.getByRole("button", { name: "用範例試試" }));
    await user.click(generate());
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ ...body, material: { title: "", text: EXEMPLAR_TEXT } });
  });

  it("完整學習單 without templates points to 我的版型", async () => {
    signIn();
    mockApi(routes());
    await open("/create/worksheet");
    expect(heading()).toHaveTextContent("來做一份 完整學習單");
    const main = within(screen.getByRole("main"));
    expect(await main.findByRole("link", { name: "我的版型" })).toHaveAttribute("href", "/materials#templates");
  });
});
