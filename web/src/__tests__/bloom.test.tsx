// Bloom 認知層次 (contract: "Bloom UI and session cards", Addendum 1): the preset arithmetic, the
// editor's chip and select, the distribution bar and its filter, the hint field, the settings view
// (要做什麼, presets, 題數, 自訂, 學習目標, 給 Manna 的要求), the new job steps, and the session
// cards for 閱讀理解題, 差異化學習單 (formerly 分層練習) and 教學套件 (with its download).
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ActivityPlan,
  Bloom,
  PrepKind,
  PrepSession,
  PrepTurn,
  SaveWorksheetRequest,
  WorksheetContent,
  WorksheetSettings,
  WorksheetState,
  WsAudience,
  WsBlock,
  WsItem,
  WsJob,
  WsLevel,
  WsSheet,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import {
  BLOOM_ORDER,
  BLOOM_PRESETS,
  TOTAL_MAX,
  countsFor,
  countsTotal,
  fillCounts,
  matchPreset,
  planFromSettings,
  planSummary,
  presetMinTotal,
  type FixedPresetId,
} from "../lib/bloom";
import { prepConfig } from "../lib/prep";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn, type MockCall } from "../test/mockApi";

const WS = "/api/worksheets/5";

beforeEach(() => {
  signIn();
  autosaveConfig.delayMs = 20;
  worksheetConfig.pollMs = 20;
  prepConfig.pollMs = 20;
});
afterEach(() => {
  autosaveConfig.delayMs = 1500;
  worksheetConfig.pollMs = 2000;
  prepConfig.pollMs = 2000;
});

// ---------------------------------------------------------------- fixtures

const item = (id: string, prompt: string, bloom: Bloom | null, extra: Partial<WsItem> = {}): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: `Answer to ${id}`,
  evidence: "",
  purpose: null,
  levels: ["standard"],
  origin: "ai",
  bloom,
  hint: "",
  ...extra,
});

const block = (id: string, title: string, items: WsItem[]): WsBlock => ({
  id,
  kind: "activity",
  enabled: true,
  title,
  instructions: "Answer the questions.",
  paragraph_ids: [],
  info_function: null,
  student_tasks: [],
  objective_ids: [],
  vocabulary: [],
  items,
  scaffolds: [],
  teacher_notes: "",
  origin: "ai",
  message: null,
  sop_block_id: null,
});

function bloomContent(): WorksheetContent {
  return {
    ...emptyWorksheetContent(),
    show_objectives: false,
    levels: ["standard"],
    blocks: [
      block("b1", "Part A · Find the Facts", [
        item("i1", "Who put the umbrella stand outside?", "remember"),
        item("i2", "What is the main idea of paragraph 1?", "understand"),
      ]),
      block("b2", "Look Closer", [item("i3", "Why did the neighbors laugh?", "analyze")]),
      block("b3", "Old block", [item("i4", "An old question without a level.", null)]),
    ],
  };
}

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
    created_at: "2026-11-16T08:00:00Z",
    updated_at: "2026-11-16T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-16T08:00:00Z",
    content: emptyWorksheetContent(),
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    request: "",
    ...overrides,
  };
}

const generated = () => wsState({ generated: true, levels: ["standard"], version_no: 2, content: bloomContent() });

const saved = (base: WorksheetState) => (call: MockCall) => ({
  ...base,
  ...(call.body as SaveWorksheetRequest),
  saved_at: "2026-11-16T09:30:00Z",
});

const routes = (state: WorksheetState, extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  "GET /api/worksheets": [],
  [`GET ${WS}`]: state,
  [`PUT ${WS}`]: saved(state),
  ...extra,
});

async function settle() {
  window.dispatchEvent(new Event("blur"));
  await screen.findByText(/^已自動儲存 \d\d:\d\d/);
}

const lastSave = (api: ReturnType<typeof mockApi>) => api.lastBody("PUT", WS) as SaveWorksheetRequest;

const job = (id: number, step: WsJob["step"], extra: Partial<WsJob> = {}): WsJob => ({
  id,
  worksheet_id: 5,
  run_no: 1,
  step,
  block_id: null,
  status: "queued",
  outcome: null,
  phase: "",
  message: null,
  interaction_id: null,
  context: [],
  started_at: null,
  finished_at: null,
  ...extra,
});

// ---------------------------------------------------------------- lib/bloom

describe("lib/bloom: presets and counts", () => {
  const fixed = BLOOM_PRESETS.filter((p) => p.percent !== null).map((p) => p.id as FixedPresetId);

  it("has the contract's presets in order, each summing to 100%", () => {
    expect(BLOOM_PRESETS.map((p) => [p.id, p.label])).toEqual([
      ["foundation", "基礎鞏固"],
      ["balanced", "均衡（預設）"],
      ["challenge", "思考挑戰"],
      ["higher", "深度思考"],
      ["custom", "自訂"],
    ]);
    for (const p of BLOOM_PRESETS) {
      if (p.percent) expect(countsTotal(p.percent)).toBe(100);
    }
    expect(BLOOM_PRESETS.find((p) => p.id === "balanced")?.percent).toEqual({
      remember: 15,
      understand: 25,
      apply: 20,
      analyze: 20,
      evaluate: 10,
      create: 10,
    });
  });

  it("countsFor: largest remainder, at least one for every level with a share, sums to the total", () => {
    expect(countsFor("balanced", 8)).toEqual({ remember: 1, understand: 2, apply: 2, analyze: 1, evaluate: 1, create: 1 });
    expect(countsFor("foundation", 8)).toEqual({ remember: 3, understand: 3, apply: 2, analyze: 0, evaluate: 0, create: 0 });
    expect(countsFor("higher", 9)).toEqual({ remember: 0, understand: 0, apply: 0, analyze: 3, evaluate: 3, create: 3 });
    expect(countsFor("balanced", 20)).toEqual({ remember: 3, understand: 5, apply: 4, analyze: 4, evaluate: 2, create: 2 });

    for (const id of fixed) {
      const percent = BLOOM_PRESETS.find((p) => p.id === id)!.percent!;
      for (let total = 1; total <= TOTAL_MAX; total += 1) {
        const counts = countsFor(id, total);
        expect(countsTotal(counts)).toBe(total);
        const used = BLOOM_ORDER.filter((l) => percent[l] > 0).length;
        for (const l of BLOOM_ORDER) {
          if (percent[l] > 0 && total >= used) expect(counts[l]).toBeGreaterThanOrEqual(1);
          if (percent[l] === 0) expect(counts[l]).toBe(0);
          expect(counts[l]).toBeLessThanOrEqual(12);
        }
      }
    }
  });

  it("fewer questions than levels: the levels with the largest share get one each, and the total is kept", () => {
    expect(presetMinTotal("balanced")).toBe(3);
    expect(presetMinTotal("challenge")).toBe(3);
    expect(countsFor("balanced", 3)).toEqual({ remember: 0, understand: 1, apply: 1, analyze: 1, evaluate: 0, create: 0 });
    expect(countsFor("challenge", 4)).toEqual({ remember: 0, understand: 1, apply: 1, analyze: 1, evaluate: 1, create: 0 });
  });

  it("matchPreset recognises every preset's own counts, anything else is 自訂", () => {
    for (const id of fixed) {
      for (let total = presetMinTotal(id); total <= TOTAL_MAX; total += 1) {
        expect(matchPreset(countsFor(id, total))).toBe(id);
      }
    }
    expect(matchPreset({ remember: 5, create: 1 })).toBe("custom");
  });

  it("fills chosen levels the way the server does: one each, then from the lowest level up", () => {
    expect(fillCounts(BLOOM_ORDER, 8)).toEqual({ remember: 2, understand: 2, apply: 1, analyze: 1, evaluate: 1, create: 1 });
    expect(fillCounts(["evaluate", "understand", "analyze"], 8)).toEqual({
      remember: 0,
      understand: 3,
      apply: 0,
      analyze: 3,
      evaluate: 2,
      create: 0,
    });
  });

  it("reads a plan from the settings and says it in one line", () => {
    const settings = (extra: Partial<WorksheetSettings>): WorksheetSettings => ({ ...emptySettings(), ...extra });
    expect(planFromSettings(settings({ output: "comprehension" }))).toMatchObject({ preset: "balanced", total: 8 });
    expect(planFromSettings(settings({ output: "tiered" }))).toMatchObject({ preset: "balanced", total: 6 });
    expect(planFromSettings(settings({ output: "comprehension", bloom_counts: { analyze: 2, create: 3 } }))).toMatchObject({
      preset: "custom",
      total: 5,
    });
    expect(
      planFromSettings(settings({ output: "comprehension", bloom: ["understand", "analyze", "evaluate"], question_count: 6 })),
    ).toMatchObject({ preset: "custom", counts: { understand: 2, analyze: 2, evaluate: 2 }, total: 6 });
    expect(planSummary(countsFor("balanced", 8))).toBe("8 題 · 記憶 1・理解 2・應用 2・分析 1・評鑑 1・創造 1 · 約 20 分鐘");
  });
});

// ---------------------------------------------------------------- editor

describe("worksheet editor: Bloom chip, distribution and hint", () => {
  it("each item has a Bloom chip that is a select (未標 = null); changing it saves and marks the item edited", async () => {
    const api = mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const first = (await screen.findByRole("combobox", { name: "Bloom 層次 1" })) as HTMLSelectElement;
    expect(first).toHaveValue("remember");
    expect(first).toHaveClass("bloom-chip", "is-remember");
    expect(first).toHaveAttribute("title", "Bloom：Remember");
    expect(within(first).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "未標",
      "記憶",
      "理解",
      "應用",
      "分析",
      "評鑑",
      "創造",
    ]);
    expect(screen.getByRole("combobox", { name: "Bloom 層次 4" })).toHaveValue("");

    await user.selectOptions(first, "evaluate");
    expect(first).toHaveClass("is-evaluate");
    await user.selectOptions(screen.getByRole("combobox", { name: "Bloom 層次 3" }), "");
    await settle();
    const body = lastSave(api);
    expect(body.content?.blocks[0]?.items[0]).toMatchObject({ bloom: "evaluate", origin: "ai_edited" });
    expect(body.content?.blocks[1]?.items[0]).toMatchObject({ bloom: null, origin: "ai_edited" });
  });

  it("the distribution bar counts the levels and filters the items; 全部 shows everything again", async () => {
    mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const dist = within(await screen.findByRole("group", { name: "Bloom 層次分布" }));
    expect(dist.getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual([
      "只看記憶（1 題）",
      "只看理解（1 題）",
      "只看分析（1 題）",
      "只看未標（1 題）",
      "全部",
    ]);
    expect(dist.getByRole("button", { name: "全部" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByRole("textbox", { name: /^題目 \d$/ })).toHaveLength(4);

    await user.click(dist.getByRole("button", { name: "只看分析（1 題）" }));
    expect(dist.getByRole("button", { name: "只看分析（1 題）" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByRole("textbox", { name: /^題目 \d$/ }).map((t) => (t as HTMLTextAreaElement).value)).toEqual([
      "Why did the neighbors laugh?",
    ]);
    // Blocks without an item at that level step aside; the numbers stay the sheet's.
    expect(screen.queryByRole("region", { name: "區塊 A" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "題目 3" })).toBeInTheDocument();
    expect(screen.getByText(/^只顯示「分析」的題目（1 題）/)).toHaveAttribute("role", "status");

    await user.click(dist.getByRole("button", { name: "只看未標（1 題）" }));
    expect(screen.getAllByRole("textbox", { name: /^題目 \d$/ }).map((t) => (t as HTMLTextAreaElement).value)).toEqual([
      "An old question without a level.",
    ]);

    await user.click(dist.getByRole("button", { name: "全部" }));
    expect(screen.getAllByRole("textbox", { name: /^題目 \d$/ })).toHaveLength(4);
  });

  it("no bar on a worksheet without any Bloom tags", async () => {
    const old = generated();
    old.content.blocks = old.content.blocks.map((b) => ({ ...b, items: b.items.map((it) => ({ ...it, bloom: null })) }));
    mockApi(routes(old));
    renderApp("/worksheets/5");
    await screen.findByRole("combobox", { name: "Bloom 層次 1" });
    expect(screen.queryByRole("group", { name: "Bloom 層次分布" })).not.toBeInTheDocument();
  });

  it("the hint is editable on each item, labelled as printed only in 基礎版", async () => {
    // A worksheet with 基礎版: every question in it offers its hint.
    const withBasic = generated();
    withBasic.levels = ["basic", "standard"];
    withBasic.content.levels = ["basic", "standard"];
    withBasic.content.blocks = withBasic.content.blocks.map((b) => ({
      ...b,
      items: b.items.map((it) => ({ ...it, levels: ["basic", "standard"] as WsItem["levels"] })),
    }));
    const api = mockApi(routes(withBasic));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const hint = await screen.findByRole("textbox", { name: "提示（只印在基礎版） 1" });
    expect(screen.getAllByText("提示（只印在基礎版）")).toHaveLength(4);
    await user.type(hint, "Look at the first sentence.");
    await settle();
    expect(lastSave(api).content?.blocks[0]?.items[0]).toMatchObject({
      hint: "Look at the first sentence.",
      origin: "ai_edited",
    });
  });

  it("a worksheet without 基礎版 offers no empty hint (it would never print) and says so of a written one", async () => {
    const only = generated();
    only.content.blocks[0] = { ...only.content.blocks[0]!, items: [{ ...only.content.blocks[0]!.items[0]!, hint: "Who did it?" }, only.content.blocks[0]!.items[1]!] };
    mockApi(routes(only));
    renderApp("/worksheets/5");
    await screen.findByRole("textbox", { name: "題目 1" });
    expect(screen.queryByText("提示（只印在基礎版）")).not.toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: "提示（只印在基礎版；這題不在基礎版，不會印出） 1" }),
    ).toHaveValue("Who did it?");
    expect(screen.getAllByRole("textbox", { name: /^提示/ })).toHaveLength(1);
  });

  it("the teacher view shows each block's Bloom line", async () => {
    mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(await screen.findByRole("radio", { name: "教師版" }));
    expect(within(screen.getByRole("region", { name: "區塊 A" })).getByText("Bloom：記憶 1、理解 1")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- settings view

describe("settings view: 要做什麼, Bloom presets, 題數, 學習目標, 給 Manna 的要求", () => {
  const whatToMake = () => within(screen.getByRole("radiogroup", { name: "要做什麼" }));

  it("學習單 keeps the four blocks; 閱讀理解題 swaps them for the Bloom picker, 作答方式 and 版本", async () => {
    const api = mockApi(routes(wsState()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const what = await screen.findByRole("radiogroup", { name: "要做什麼" });
    expect(within(what).getAllByRole("radio").map((r) => (r as HTMLInputElement).labels?.[0]?.textContent)).toEqual([
      "學習單",
      "閱讀理解題",
      "差異化學習單",
    ]);
    expect(whatToMake().getByRole("radio", { name: "學習單" })).toBeChecked();
    expect(screen.getByRole("region", { name: "素材類型" })).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "出題比例" })).not.toBeInTheDocument();

    await user.click(whatToMake().getByRole("radio", { name: "閱讀理解題" }));
    expect(screen.queryByRole("region", { name: "素材類型" })).not.toBeInTheDocument();
    const presets = within(screen.getByRole("radiogroup", { name: "出題比例" }));
    expect(presets.getAllByRole("radio").map((r) => (r as HTMLInputElement).labels?.[0]?.querySelector(".bloom-preset-name")?.textContent)).toEqual([
      "基礎鞏固",
      "均衡（預設）",
      "思考挑戰",
      "深度思考",
      "自訂",
    ]);
    expect(presets.getByRole("radio", { name: /均衡（預設）/ })).toBeChecked();
    expect(screen.getByRole("status", { name: "這次會出" })).toHaveTextContent(
      "8 題 · 記憶 1・理解 2・應用 2・分析 1・評鑑 1・創造 1 · 約 20 分鐘",
    );
    expect(within(screen.getByRole("region", { name: "作答方式" })).getAllByRole("checkbox")).toHaveLength(8);
    expect(within(screen.getByRole("region", { name: "版本" })).getAllByRole("checkbox")).toHaveLength(3);
    expect(screen.getByRole("button", { name: "產生閱讀理解題" })).toBeEnabled();
    await settle();
    expect(lastSave(api).settings?.output).toBe("comprehension");
  });

  it("a preset and the total stepper write bloom_counts (with bloom and question_count to match)", async () => {
    const api = mockApi(routes(wsState({ settings: { ...emptySettings(), output: "comprehension" } })));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const presets = within(await screen.findByRole("radiogroup", { name: "出題比例" }));
    await user.click(presets.getByRole("radio", { name: /思考挑戰/ }));
    const total = within(screen.getByRole("group", { name: "題數" }));
    await user.click(total.getByRole("button", { name: "題數多一題" }));
    await user.click(total.getByRole("button", { name: "題數多一題" }));
    expect(total.getByText("10")).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "這次會出" })).toHaveTextContent("10 題 · 理解 2・應用 2・分析 3・評鑑 2・創造 1 · 約 25 分鐘");
    await settle();
    expect(lastSave(api).settings).toMatchObject({
      output: "comprehension",
      bloom_counts: { understand: 2, apply: 2, analyze: 3, evaluate: 2, create: 1 },
      bloom: ["understand", "apply", "analyze", "evaluate", "create"],
      question_count: 10,
    });
    expect(presets.getByRole("radio", { name: /思考挑戰/ })).toBeChecked();
    // Every preset goes down to the global minimum of 3.
    for (let i = 0; i < 7; i += 1) await user.click(total.getByRole("button", { name: "題數少一題" }));
    expect(total.getByText("3")).toBeInTheDocument();
    expect(total.getByRole("button", { name: "題數少一題" })).toBeDisabled();
  });

  it("自訂 gives a stepper per level; the whole stays 3–30", async () => {
    const api = mockApi(
      routes(wsState({ settings: { ...emptySettings(), output: "comprehension", bloom_counts: { remember: 1, understand: 1, analyze: 1 } } })),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const presets = within(await screen.findByRole("radiogroup", { name: "出題比例" }));
    // Counts that are no preset's read as 自訂.
    expect(presets.getByRole("radio", { name: /自訂/ })).toBeChecked();
    const levels = within(screen.getByRole("list", { name: "每個層次的題數" }));
    // At 3 in all nothing can go down.
    expect(levels.getByRole("button", { name: "記憶少一題" })).toBeDisabled();
    await user.click(levels.getByRole("button", { name: "創造多一題" }));
    await user.click(levels.getByRole("button", { name: "創造多一題" }));
    await user.click(levels.getByRole("button", { name: "記憶少一題" }));
    await settle();
    expect(lastSave(api).settings).toMatchObject({
      bloom_counts: { understand: 1, analyze: 1, create: 2 },
      bloom: ["understand", "analyze", "create"],
      question_count: 4,
    });
    expect(screen.getByRole("status", { name: "這次會出" })).toHaveTextContent("4 題 · 理解 1・分析 1・創造 2 · 約 10 分鐘");

    // Back to a preset: its counts at (about) the same total.
    await user.click(presets.getByRole("radio", { name: /基礎鞏固/ }));
    await settle();
    expect(lastSave(api).settings?.bloom_counts).toEqual({ remember: 2, understand: 1, apply: 1 });
  });

  it("generating with the picker untouched sends what it shows (the default preset)", async () => {
    const running = wsState({
      settings: { ...emptySettings(), output: "tiered" },
      running: true,
      jobs: [
        job(1, "bloom", { status: "running", phase: "正在出『分析』層次的題目", started_at: new Date().toISOString() }),
        job(2, "bloom_check"),
      ],
    });
    const api = mockApi(
      routes(wsState({ settings: { ...emptySettings(), output: "tiered" } }), {
        [`POST ${WS}/generate`]: running,
        [`GET ${WS}/jobs`]: { running: true, jobs: running.jobs, version_no: 1 },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    expect(
      await screen.findByText(
        "差異化學習單固定做基礎版、一般版、進階版三個版本：三個版本目標與題目相同，差在提示與支持的多寡。學生版只印 A、B、C 版，不印程度名稱。",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "這次會出" })).toHaveTextContent(/^6 題 · /);
    // Only the open answer formats.
    expect(screen.queryByRole("checkbox", { name: "選擇題" })).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "簡答題" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "產生差異化學習單" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/generate`)).toHaveLength(1));
    expect(lastSave(api).settings).toMatchObject({
      output: "tiered",
      bloom_counts: countsFor("balanced", 6),
      question_count: 6,
    });

    // The new steps have their own names in the progress list.
    const progress = within(await screen.findByRole("status", { name: "產生進度" }));
    expect(progress.getByText("依 Bloom 層次出題")).toBeInTheDocument();
    expect(progress.getByText("進行中：正在出『分析』層次的題目")).toBeInTheDocument();
    expect(progress.getByText("檢查題目的 Bloom 層次")).toBeInTheDocument();
  });

  it("內容目標、語言目標 and 給 Manna 的要求 are saved with the worksheet", async () => {
    const api = mockApi(routes(wsState()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const goals = within(await screen.findByRole("region", { name: "學習目標" }));
    await user.type(goals.getByRole("textbox", { name: "內容目標（選填）" }), "I can explain why.");
    await user.type(goals.getByRole("textbox", { name: "語言目標（選填）" }), "I can use because.");
    const ask = screen.getByRole("textbox", { name: "給 Manna 的要求" });
    expect(ask).toHaveAttribute("maxLength", "2000");
    await user.type(ask, "B1 程度");
    expect(screen.getByText("選填 · 5/2000")).toBeInTheDocument();
    await settle();
    const body = lastSave(api);
    expect(body.settings?.content_objective).toBe("I can explain why.");
    expect(body.settings?.language_objective).toBe("I can use because.");
    expect(body.settings).not.toHaveProperty("objective");
    // The worksheet's objectives follow the fields (as the server does), and print at the top.
    expect(body.content?.objectives.map((o) => [o.id, o.type, o.student_facing_en])).toEqual([
      ["teacher-c1", "content", "I can explain why."],
      ["teacher-l1", "language", "I can use because."],
    ]);
    expect(body.settings?.generation).toContain("顯示學習目標");
    expect(body.request).toBe("B1 程度");
    // Emptied, an objective is null again.
    await user.clear(goals.getByRole("textbox", { name: "內容目標（選填）" }));
    await settle();
    expect(lastSave(api).settings?.content_objective).toBeNull();
    expect(lastSave(api).settings?.language_objective).toBe("I can use because.");
  });

  it("with a template, only the full worksheet can be chosen", async () => {
    mockApi(routes(wsState({ template: { id: 9, title: "L2 版型", version_no: 1 } }), { "GET /api/worksheet-templates/9": { id: 9, title: "L2 版型", status: "ready", section_count: 0, source_material: null, version_no: 1, updated_at: "", error: null, content: null } }));
    renderApp("/worksheets/5");
    const what = within(await screen.findByRole("radiogroup", { name: "要做什麼" }));
    expect(what.getByRole("radio", { name: "學習單" })).toBeEnabled();
    expect(what.getByRole("radio", { name: "閱讀理解題" })).toBeDisabled();
    expect(what.getByRole("radio", { name: "差異化學習單" })).toBeDisabled();
    expect(screen.getByText(/版型只適用於完整學習單/)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- session page

const S7 = "/api/prep-sessions/7";
let turnId = 300;

const turn = (kind: PrepKind, overrides: Partial<PrepTurn> = {}): PrepTurn => ({
  id: ++turnId,
  seq: 1,
  kind,
  request: kind,
  status: "done",
  error: null,
  worksheet: { id: 11, title: "U", version_no: 2, running: false },
  result: null,
  interaction_id: null,
  created_at: "2026-11-16T08:00:00Z",
  finished_at: "2026-11-16T08:01:00Z",
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
  updated_at: "2026-11-16T09:00:00Z",
  created_at: "2026-11-16T08:00:00Z",
  turns: turns.map((t, i) => ({ ...t, seq: i + 1 })),
  running: turns.some((t) => t.status === "running"),
  next: [],
  ...overrides,
});

const plan: ActivityPlan = {
  title: "一節課",
  goals: [],
  steps: [
    { phase: "引起動機", title: "雨天經驗", minutes: 5, idea: "從經驗帶入。", options: [], script: [], stuck_tip: "", bloom: ["understand", "remember"] },
    { phase: "發展活動", title: "逐段找原因", minutes: 25, idea: "找原因。", options: [], script: [], stuck_tip: "", bloom: ["analyze"] },
    { phase: "統整與評量", title: "說說看", minutes: 10, idea: "分享。", options: [], script: [], stuck_tip: "" },
  ],
  curriculum: [],
};

function prepWorksheet(levels: WsLevel[]): WorksheetState {
  const content = bloomContent();
  content.levels = levels;
  content.blocks = content.blocks.map((b) => ({ ...b, items: b.items.map((it) => ({ ...it, levels })) }));
  return wsState({ id: 11, generated: true, levels, version_no: 2, content, prep_session_id: 7 });
}

/** The server's projection (no level: the worksheet's first): Bloom tags only for the teacher, hints only in 基礎版. */
const sheetFor = (first: WsLevel) => (call: MockCall): WsSheet => {
  const params = new URL(call.url, "http://x").searchParams;
  const audience = (params.get("audience") ?? "student") as WsAudience;
  const level = (params.get("level") ?? first) as WsLevel;
  const teacher = audience === "teacher";
  const it = (no: number, prompt: string, bloom: Bloom | null) => ({
    no,
    answer_format: "簡答題",
    prompt,
    options: [],
    lines: 2,
    ...(teacher ? { answer: `Answer ${no}`, evidence: "", bloom } : {}),
    ...(level === "basic" ? { hint: `Hint for ${no}` } : {}),
  });
  return {
    title: "The Umbrella Stand",
    level,
    audience,
    objectives: [],
    blocks: [
      {
        id: "b1",
        kind: "activity",
        label: "A",
        title: "Part A · Find the Facts",
        instructions: "",
        steps: [],
        vocabulary: [],
        supports: [],
        items: [it(1, "Who put the umbrella stand outside?", "remember"), it(2, "What is the main idea?", "understand")],
      },
      {
        id: "b2",
        kind: "activity",
        label: "B",
        title: "Look Closer",
        instructions: "",
        steps: [],
        vocabulary: [],
        supports: [],
        items: [it(3, "Why did the neighbors laugh?", "analyze")],
      },
    ],
  };
};

const pane = async () => within(await screen.findByRole("complementary", { name: "目前的學習單" }));

describe("session page: 閱讀理解題, 差異化學習單, 教學套件", () => {
  it("a 閱讀理解題 turn is a worksheet card; the teacher version shows Bloom tags and every card a read-only bar", async () => {
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([turn("comprehension", { request: "閱讀理解題" })]),
      "GET /api/worksheets/11": prepWorksheet(["standard"]),
      "GET /api/worksheets/11/sheet": sheetFor("standard"),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const result = await pane();
    const preview = await result.findByRole("article", { name: /學生版/ });
    expect(within(preview).getByText("Why did the neighbors laugh?")).toBeInTheDocument();
    // The student version has no tags.
    expect(preview.querySelector(".bloom-chip")).toBeNull();
    expect(result.getByRole("link", { name: "直接編輯" })).toHaveAttribute("href", "/worksheets/11");

    // The read-only distribution of the version shown (no buttons).
    const bar = await result.findByRole("img", { name: /Bloom 層次分布（一般版）/ });
    expect(bar).toHaveAccessibleName("Bloom 層次分布（一般版）：記憶 1、理解 1、分析 1、未標 1");
    expect(within(result.getByRole("group", { name: "Bloom 層次分布（一般版）" })).queryByRole("button")).toBeNull();

    await user.click(result.getByRole("radio", { name: "教師版" }));
    const teacher = await result.findByRole("article", { name: /教師版/ });
    const blockB = within(teacher).getByRole("region", { name: "區塊 B 活動" });
    expect(within(blockB).getByText("分析")).toHaveAttribute("title", "Bloom：Analyze");
    expect(within(blockB).getByText("Bloom：分析 1")).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/worksheets/11/sheet").some((c) => c.url.includes("audience=teacher"))).toBe(true);
  });

  it("a tiered turn made before the redesign has the three version tabs, and 基礎版 prints the hints", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([turn("tiered", { request: "分層練習" })]),
      "GET /api/worksheets/11": prepWorksheet(["basic", "standard", "advanced"]),
      "GET /api/worksheets/11/sheet": sheetFor("basic"),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const result = await pane();
    const tabs = within(await result.findByRole("tablist", { name: "版本程度" }));
    expect(tabs.getAllByRole("tab").map((t) => t.textContent)).toEqual(["基礎版", "一般版", "進階版"]);
    expect(await result.findByText("Hint: Hint for 1")).toBeInTheDocument();
    await user.click(tabs.getByRole("tab", { name: "進階版" }));
    await result.findByRole("article", { name: "進階版 學生版" });
    expect(result.queryByText("Hint: Hint for 1")).not.toBeInTheDocument();
  });

  it("a 教學套件 turn: the plan with Bloom chips on its steps, the worksheet card, and one DOCX download", async () => {
    const kit = turn("kit", { request: "教學套件", result: plan, interaction_id: 90 });
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([kit]),
      "GET /api/worksheets/11": prepWorksheet(["basic", "standard", "advanced"]),
      "GET /api/worksheets/11/sheet": sheetFor("basic"),
    });
    // The kit is a real file: the stub hands back a blob with its name.
    const mocked = globalThis.fetch;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.includes("/kit")) {
        await mocked(input, init);
        return {
          ok: true,
          status: 200,
          blob: async () => new Blob(["docx"]),
          headers: { get: () => "attachment; filename*=UTF-8''%E6%95%99%E5%AD%B8%E5%A5%97%E4%BB%B6.docx" },
        } as unknown as Response;
      }
      return mocked(input, init);
    });
    const createUrl = vi.fn(() => "blob:kit");
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: createUrl, revokeObjectURL: vi.fn() }));
    const clicked: string[] = [];
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push(this.download);
    });

    const user = userEvent.setup();
    renderApp("/prep/7");
    const result = await screen.findByRole("region", { name: "教學套件（第 1 則）" });
    // The plan, as an activity card, with the levels each step works on.
    const warm = within(within(result).getByRole("region", { name: "引起動機：雨天經驗" }));
    expect(
      within(warm.getByRole("group", { name: "「雨天經驗」的 Bloom 層次" }))
        .getAllByText(/記憶|理解/)
        .map((c) => c.textContent),
    ).toEqual(["Bloom 層次：記憶", "Bloom 層次：理解"]);
    expect(within(result).queryByRole("group", { name: "「說說看」的 Bloom 層次" })).toBeNull();
    // The worksheet beside it.
    expect(await (await pane()).findByRole("tablist", { name: "版本程度" })).toBeInTheDocument();

    await user.click(within(result).getByRole("button", { name: "下載教學套件 (DOCX)" }));
    await waitFor(() => expect(clicked).toEqual(["教學套件.docx"]));
    const call = api.callsTo("GET", `${S7}/turns/${kit.id}/kit`)[0]!;
    expect(new URLSearchParams(call.url.split("?")[1]).toString()).toBe("format=docx");
    expect(createUrl).toHaveBeenCalled();
    click.mockRestore();
  });

  it("while a 教學套件 is still being made, the download waits", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([turn("kit", { request: "教學套件", status: "running", worksheet: null })]),
    });
    renderApp("/prep/7");
    const result = await screen.findByRole("region", { name: "教學套件（第 1 則）" });
    expect(within(result).getByRole("button", { name: "下載教學套件 (DOCX)" })).toBeDisabled();
    expect(within(result).getByText("兩個部分都做好之後就可以下載。")).toBeInTheDocument();
    expect(within(result).getByText("Manna 正在規劃教學活動…")).toBeInTheDocument();
    expect((await pane()).getByText("Manna 正在建立學習單…")).toBeInTheDocument();
  });
});
