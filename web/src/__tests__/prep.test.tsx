// 備課對話 (prep sessions): the home page's quick box (Bloom contract, Addendum 1) with its keyword
// chips, 附上文章 and its dialogs, the conversation page with a card per turn kind, the bottom bar,
// polling, and where a session sits in the sidebar.
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  ActivityGoal,
  ActivityPlan,
  Material,
  MaterialLibrary,
  PrepKind,
  PrepSession,
  PrepSessionSummary,
  PrepTurn,
  SiopCheck,
  SiopCheckItem,
  WorksheetState,
  WsAudience,
  WsJob,
  WsSheet,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { goalGroups, goalsText, hasPrepare, prepConfig, prepareText, reviewText, stepText } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import {
  baseRoutes,
  cardOf,
  emptyLibrary,
  materialBrowse,
  mockApi,
  renderApp,
  schoolClass,
  signIn,
  type MockCall,
} from "../test/mockApi";

const HOME = "今天想準備什麼？";
const SESSIONS = "/api/prep-sessions";
const S7 = `${SESSIONS}/7`;

beforeEach(() => {
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});

afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
});

// ---------------------------------------------------------------- fixtures

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
  ...overrides,
});

const library = (materials: Material[] = [material()]): MaterialLibrary => ({ ...emptyLibrary(), materials });
/** The library's materials, as 從教材挑選 lists them. */
const browse = (materials: Material[] = [material()]) => materialBrowse(materials.map((m) => cardOf(m)));

const summary = (overrides: Partial<PrepSessionSummary> = {}): PrepSessionSummary => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: 1,
  last_kind: "activity",
  updated_at: "2026-11-15T09:00:00Z",
  created_at: "2026-11-15T08:00:00Z",
  ...overrides,
});

let turnId = 100;
const turn = (kind: PrepKind, overrides: Partial<PrepTurn> = {}): PrepTurn => ({
  id: ++turnId,
  seq: 1,
  kind,
  request: kind === "activity" ? "規劃教學活動" : "做學習單",
  status: "done",
  error: null,
  worksheet: null,
  result: null,
  interaction_id: null,
  created_at: "2026-11-15T08:00:00Z",
  finished_at: "2026-11-15T08:01:00Z",
  ...overrides,
});

const session = (turns: PrepTurn[], overrides: Partial<PrepSession> = {}): PrepSession => ({
  ...summary({ turn_count: turns.length }),
  turns: turns.map((t, i) => ({ ...t, seq: i + 1 })),
  running: turns.some((t) => t.status === "running"),
  next: [],
  ...overrides,
});

const plan: ActivityPlan = {
  title: "The Umbrella Stand：一節課的活動",
  goals: [
    {
      kind: "content",
      zh: "學生能說出林太太設置傘架的原因",
      en: "Students will be able to explain why Mrs. Lin set up the umbrella stand.",
      curriculum_codes: ["5-IV-2"],
    },
    {
      kind: "language",
      zh: "學生能用 because 說明原因",
      en: "Students will be able to use because to give a reason.",
      curriculum_codes: [],
    },
  ],
  steps: [
    {
      phase: "引起動機",
      title: "雨天經驗",
      minutes: 5,
      idea: "從學生自己的雨天經驗帶入。",
      options: [
        { title: "看圖猜", idea: "展示一張傘架照片。", script: ["What do you see in this picture?"], minutes: 3 },
        { title: "兩人分享", idea: "兩人說一次借東西的經驗。", script: [], minutes: 4 },
      ],
      script: ["Have you ever forgotten your umbrella?"],
      stuck_tip: "學生不開口時，先示範一個自己的例子。",
    },
    {
      phase: "發展活動",
      title: "逐段找原因",
      minutes: 25,
      idea: "每段找一個原因。",
      options: [],
      script: [],
      stuck_tip: "",
    },
  ],
  curriculum: [{ code: "5-IV-2", text: "能讀懂簡易故事的主要內容" }],
};

/** What goalGroups adds to a goal that has none of version 4's fields. */
const OLD_GOAL = { en: "", curriculum_codes: [], category: null, supports: null, relation: "", fromTeacher: false };

/** A plan of prep_activity version 4 (SIOP): goal relations, A6 fields, groupings and skills. */
const planV4: ActivityPlan = {
  ...plan,
  goals: [
    { ...(plan.goals[0] as Extract<ActivityGoal, { zh: string }>), from_teacher: true },
    {
      kind: "language",
      zh: "學生能在讀不懂時重讀並畫出關鍵句",
      en: "Students will be able to reread and underline the key sentence.",
      curriculum_codes: [],
      category: "strategy",
      supports: 1,
      relation: "學生要用重讀並畫出關鍵句的策略，才能說出林太太設置傘架的原因",
    },
  ],
  key_vocabulary: [
    { word: "trust", zh: "信任" },
    { word: "neighbor", zh: "鄰居" },
  ],
  materials: ["課文第 1–4 段", "老師準備的傘架照片"],
  higher_order_questions: [{ question: "Why do you think the plan worked?", bloom: "evaluate" }],
  background: "請學生回想借東西給別人的經驗。",
  steps: [
    {
      ...plan.steps[0]!,
      grouping: "兩人一組",
      roles: [
        { role: "夥伴 A", task: "說一次借東西的經驗" },
        { role: "夥伴 B", task: "聽完後問一個問題" },
      ],
      monitoring: "",
      skills: ["聽", "說"],
      scaffolding: "",
    },
    {
      ...plan.steps[1]!,
      grouping: "小組",
      roles: [
        { role: "主持", task: "帶討論" },
        { role: "記錄", task: "寫下原因" },
        { role: "報告", task: "向全班說明" },
      ],
      monitoring: "巡視各組，先聽記錄員的原因。",
      skills: ["說", "讀"],
      scaffolding: "教師先示範第 1 段。",
    },
  ],
  review: "用字卡複習 trust 與 neighbor。",
  assessment: "exit ticket 寫出一個原因。",
};

const SIOP_ITEMS: Array<Pick<SiopCheckItem, "id" | "name" | "scope" | "note">> = [
  { id: "1-2", name: "內容目標與語言目標", scope: "page", note: "" },
  { id: "3", name: "概念適切", scope: "page", note: "" },
  { id: "10", name: "教師語速與用語", scope: "classroom", note: "要在課堂上聽教師說話才能判斷" },
  { id: "12", name: "讓概念清楚的技巧", scope: "partial", note: "只看得到書面的視覺輔助" },
  { id: "17", name: "分組", scope: "page", note: "" },
  { id: "18", name: "等待時間", scope: "classroom", note: "要在課堂上看教師是否給學生足夠的時間回答" },
  { id: "25", name: "學生投入", scope: "classroom", note: "要在課堂上看學生參與的情形" },
  { id: "26", name: "節奏", scope: "classroom", note: "要在課堂上看教學節奏是否適合學生" },
  { id: "30", name: "評量", scope: "page", note: "" },
];
const SIOP_RATINGS: Record<string, [SiopCheckItem["rating"], string]> = {
  "1-2": ["met", "兩種目標都寫了，語言目標寫出了和內容目標的關係。"],
  "3": ["partial", "內容適合高一，但第 3 段的概念沒有用到。"],
  "12": ["met", "有傘架照片。"],
  "17": ["not_seen", "這份產出沒有寫分組。"],
};

/** A turn's SIOP check (a short item list stands in for the 26). */
const siopCheck = (status: SiopCheck["status"], overrides: Partial<SiopCheck> = {}): SiopCheck => ({
  id: 1,
  run_no: 1,
  status,
  error: null,
  worksheet_version_no: null,
  items: SIOP_ITEMS.map((i) => {
    const [rating, reason] = status === "done" ? (SIOP_RATINGS[i.id] ?? [null, ""]) : [null, ""];
    return { ...i, question: i.scope === "classroom" ? "" : "？", rating, reason };
  }),
  interaction_id: status === "done" ? 90 : null,
  created_at: "2026-11-17T08:00:00Z",
  finished_at: status === "running" ? null : "2026-11-17T08:01:00Z",
  ...overrides,
});

const job = (id: number, step: WsJob["step"], extra: Partial<WsJob> = {}): WsJob => ({
  id,
  worksheet_id: 11,
  run_no: 1,
  step,
  block_id: null,
  status: "done",
  outcome: "shown",
  phase: "",
  message: null,
  interaction_id: null,
  context: [],
  started_at: "2026-11-15T08:00:00Z",
  finished_at: null,
  ...extra,
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
  content: { ...emptyWorksheetContent(), levels: ["standard"] },
  jobs: [job(1, "warmup", { interaction_id: 61 }), job(2, "tasks", { interaction_id: 62 })],
  running: false,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
  ...overrides,
});

const sheet = (audience: WsAudience): WsSheet => {
  const teacher = audience === "teacher";
  return {
    title: "The Umbrella Stand",
    level: "standard",
    audience,
    objectives: ["I can explain why Mrs. Lin started the stand."],
    blocks: [
      {
        id: "w",
        kind: "warmup",
        label: "A",
        title: "Warm-up",
        instructions: "Talk with a partner.",
        steps: [],
        vocabulary: [{ word: "neighbor", level: 2, definition: "a person who lives near you" }],
        supports: [],
        items: [{ no: 1, answer_format: "簡答題", prompt: "Have you ever borrowed an umbrella?", options: [], lines: 2 }],
      },
      {
        id: "b2",
        kind: "activity",
        label: "B",
        title: "Paragraph 1: Find",
        instructions: "",
        steps: ["Read paragraph 1.", "Underline the reason."],
        vocabulary: [],
        supports: [],
        items: [
          {
            no: 2,
            answer_format: "簡答題",
            prompt: "Why did Mrs. Lin start the stand?",
            options: [],
            lines: 3,
            ...(teacher ? { answer: "A stranger once helped her.", evidence: "A stranger" } : {}),
          },
        ],
        ...(teacher ? { teacher_notes: "先確認大意。" } : {}),
      },
    ],
  };
};

/** The activity card's 學習目標 (the worksheet preview has a 學習目標 region of its own). */
function activityGoals() {
  const region = screen.getAllByRole("region", { name: "學習目標" }).find((r) => r.querySelector("table.prep-goals"));
  if (!region) throw new Error("the activity card has no 學習目標");
  return within(region);
}

const sheetRoute = (call: MockCall) => sheet(new URL(call.url, "http://x").searchParams.get("audience") as WsAudience);

// ---------------------------------------------------------------- home

describe("首頁 (the quick box, the tools, the latest sessions)", () => {
  const box = () => screen.getByRole("textbox", { name: "給 Manna 的要求" });
  const chips = () => within(screen.getByRole("radiogroup", { name: "要做什麼" }));
  const pressed = () =>
    chips()
      .getAllByRole("radio")
      .filter((b) => b.getAttribute("aria-checked") === "true")
      .map((b) => b.querySelector(".tool-card-title")?.textContent);

  /** 附上文章 → 從教材挑選 → 選用, so the material shows as a chip in the box. */
  async function attachFromLibrary(user: ReturnType<typeof userEvent.setup>) {
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(within(screen.getByRole("group", { name: "附上文章" })).getByRole("button", { name: "從教材挑選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從教材挑選" }));
    await user.click(await dialog.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
  }

  it("typed text with an attached material makes a 完整學習單 by default; Enter sends it and opens the session", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": browse(),
      [`POST ${SESSIONS}`]: session([turn("ask", { status: "running" })]),
      [`GET ${S7}`]: session([turn("ask", { status: "running" })]),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    expect(await screen.findByRole("heading", { name: HOME, level: 1 })).toBeInTheDocument();
    expect(screen.getByText("附上一篇文章或教材，選要做什麼，Manna 幫你做出來。")).toBeInTheDocument();
    expect(box()).toHaveAttribute("placeholder", "例如：B1 程度、8 題閱讀理解，記憶到評鑑都要有");
    expect(box()).toHaveAttribute("rows", "2");

    await attachFromLibrary(user);
    // Focus went back to 附上文章 when the dialog closed.
    expect(screen.getByRole("button", { name: "附上文章" })).toHaveFocus();
    await user.type(box(), "B1 程度，記憶到評鑑都要有{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      entry: "home",
      kind: "worksheet",
      request: "B1 程度，記憶到評鑑都要有",
      material_id: 5,
    });
  });

  it("text without a material asks Manna; Shift+Enter is a new line and composing text never sends", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      [`POST ${SESSIONS}`]: session([turn("ask", { status: "running" })]),
      [`GET ${S7}`]: session([turn("ask", { status: "running" })]),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    expect(screen.getByRole("button", { name: "送出" })).toBeDisabled();

    await user.type(box(), "第一行{Shift>}{Enter}{/Shift}第二行");
    expect(box()).toHaveValue("第一行\n第二行");
    // Enter that confirms an input-method candidate is not a send.
    fireEvent.keyDown(box(), { key: "Enter", isComposing: true });
    fireEvent.keyDown(box(), { key: "Enter", keyCode: 229 });
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);

    await user.clear(box());
    await user.type(box(), "怎麼教倒裝句比較不無聊？");
    expect(pressed()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ entry: "home", kind: "ask", request: "怎麼教倒裝句比較不無聊？" });
  });

  it("typed words pick a chip until the teacher picks one herself; the chips are single-select", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    expect(chips().getAllByRole("radio").map((b) => b.textContent)).toEqual([
      "AI 幫我設計由 Manna 設計版面：暖身、逐段任務，到 Exit ticket",
      "照我的範本設計上傳你以前用過的學習單，Manna 照它的版面和樣式，為新課文做一份",
      "我想逐步設計跟著七個步驟一步步設計，每一步由你決定，Manna 從旁建議",
    ]);
    // The other four tools are named in one quiet line, not as tiles.
    expect(screen.getByText(/^閱讀理解題、差異化學習單、課堂活動、教學套件：/)).toBeInTheDocument();
    // One selector, no second list of the tools on the page.
    expect(screen.queryByRole("region", { name: "Manna 可以幫你做" })).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "要做什麼" })).not.toBeInTheDocument();
    expect(chips().getByRole("radio", { name: "照我的範本設計" })).toHaveClass("tool-tile");
    expect(pressed()).toEqual([]);

    for (const [words, chip] of [
      ["一份學習單", "AI 幫我設計"],
      // 照我的學習單 is not just 學習單.
      ["照我的學習單出一份", "照我的範本設計"],
      ["照這份出題", "照我的範本設計"],
      ["用我的範本", "照我的範本設計"],
      ["一樣的樣式", "照我的範本設計"],
      ["模仿這份學習單", "照我的範本設計"],
    ] as const) {
      await user.clear(box());
      await user.type(box(), words);
      expect(pressed()).toEqual([chip]);
    }

    // The four tools without a tile are still picked by their words (no tile is filled).
    for (const words of ["做差異化的題目", "要分層的題目", "一節課的教案", "給我一個套件", "幫我出題"]) {
      await user.clear(box());
      await user.type(box(), words);
      expect(pressed()).toEqual([]);
      // With no tile chosen the first one stays the way into the radiogroup.
      expect(chips().getAllByRole("radio").map((r) => r.getAttribute("tabindex"))).toEqual(["0", "-1", "-1"]);
    }

    // A click wins over typed words, and only one chip is filled.
    await user.click(chips().getByRole("radio", { name: "AI 幫我設計" }));
    expect(pressed()).toEqual(["AI 幫我設計"]);
    await user.type(box(), "，要分層");
    expect(pressed()).toEqual(["AI 幫我設計"]);
    await user.click(chips().getByRole("radio", { name: "照我的範本設計" }));
    expect(pressed()).toEqual(["照我的範本設計"]);
    // A tile stays chosen when clicked again (a radio does not clear); typed words no longer choose.
    await user.click(chips().getByRole("radio", { name: "照我的範本設計" }));
    expect(pressed()).toEqual(["照我的範本設計"]);
  });

  it("the tiles are one radiogroup: Tab enters on the first, the arrow keys move and choose", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    const radios = chips().getAllByRole("radio");
    // Nothing chosen: only the first tile is in the tab order.
    expect(radios.map((r) => r.getAttribute("tabindex"))).toEqual(["0", "-1", "-1"]);
    radios[0]!.focus();
    await user.keyboard("{ArrowRight}");
    expect(radios[1]).toHaveFocus();
    expect(pressed()).toEqual(["照我的範本設計"]);
    expect(radios.map((r) => r.getAttribute("tabindex"))).toEqual(["-1", "0", "-1"]);
    await user.keyboard("{ArrowRight}");
    // The third card, 我想逐步設計, is part of the cycle.
    expect(radios[2]).toHaveFocus();
    expect(radios[2]).toHaveAttribute("aria-checked", "true");
    expect(pressed()).toEqual(["我想逐步設計"]);
    await user.keyboard("{ArrowRight}");
    // It wraps round to the first tile.
    expect(radios[0]).toHaveFocus();
    expect(radios[0]).toHaveAttribute("aria-checked", "true");
    expect(radios[2]).toHaveAttribute("aria-checked", "false");

  });

  it("更多設定 goes to the full form for the chosen tool, with the attached material", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/materials": library(), "GET /api/browse/materials": browse() });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    // Without a tool there is nothing to go to.
    expect(screen.queryByRole("link", { name: /更多設定/ })).not.toBeInTheDocument();
    await user.click(chips().getByRole("radio", { name: "AI 幫我設計" }));
    expect(screen.getByRole("link", { name: "更多設定（班級、題數、Bloom、題型）" })).toHaveAttribute("href", "/create/worksheet");
    await attachFromLibrary(user);
    await user.click(chips().getByRole("radio", { name: "照我的範本設計" }));
    expect(screen.getByRole("link", { name: /更多設定/ })).toHaveAttribute("href", "/create/mimic");
    // What is attached here goes along: the form opens with the lesson text already in place.
    await user.click(screen.getByRole("link", { name: /更多設定/ }));
    expect(await screen.findByRole("button", { name: "移除「The Umbrella Stand」" })).toBeInTheDocument();
    expect(screen.queryByText("先放這節課的課文")).not.toBeInTheDocument();
  });

  it("更多設定 carries pasted text too", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/materials": library(), "GET /api/browse/materials": browse() });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(chips().getByRole("radio", { name: "AI 幫我設計" }));
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(within(screen.getByRole("group", { name: "附上文章" })).getByRole("button", { name: "貼上文字" }));
    const dialog = within(await screen.findByRole("dialog"));
    await user.type(dialog.getByRole("textbox", { name: /內容|文字|課文/ }), "Bees help plants grow.");
    await user.click(dialog.getByRole("button", { name: /^(加入|附上|用這段|確定)/ }));
    await user.click(screen.getByRole("link", { name: /更多設定/ }));
    expect(await screen.findByDisplayValue("Bees help plants grow.")).toBeInTheDocument();
  });

  it("an output without a material says 先附上文章或教材 next to 附上文章 and sends nothing", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": browse(),
      [`POST ${SESSIONS}`]: session([turn("worksheet", { status: "running" })]),
      [`GET ${S7}`]: session([turn("worksheet", { status: "running" })]),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });

    await user.click(chips().getByRole("radio", { name: "AI 幫我設計" }));
    await user.click(screen.getByRole("button", { name: "送出" }));
    const toolbar = screen.getByRole("button", { name: "附上文章" }).closest(".prompt-toolbar") as HTMLElement;
    expect(within(toolbar).getByRole("alert")).toHaveTextContent("先附上文章或教材");
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);

    await attachFromLibrary(user);
    expect(screen.queryByText("先附上文章或教材")).not.toBeInTheDocument();
    // Nothing typed: the chip's label is the request.
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ entry: "home", kind: "worksheet", request: "學習單", material_id: 5 });
  });

  it("貼上文字 opens a dialog with a title and the text; the pasted material shows as a chip and goes along", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      [`POST ${SESSIONS}`]: session([turn("activity", { status: "running" })]),
      [`GET ${S7}`]: session([turn("activity", { status: "running" })]),
    });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(screen.getByRole("button", { name: "貼上文字" }));
    const dialog = within(screen.getByRole("dialog", { name: "貼上文字" }));
    expect(dialog.getByRole("textbox", { name: /教材標題/ })).toHaveFocus();
    expect(dialog.getByRole("button", { name: "附上" })).toBeDisabled();
    await user.type(dialog.getByRole("textbox", { name: /教材標題/ }), "Umbrella");
    await user.type(dialog.getByRole("textbox", { name: "貼上教材文字" }), "Mrs. Lin put a stand outside.");
    await user.click(dialog.getByRole("button", { name: "附上" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(chips().getByRole("radio", { name: "AI 幫我設計" }));
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      entry: "home",
      kind: "worksheet",
      request: "學習單",
      material: { title: "Umbrella", text: "Mrs. Lin put a stand outside." },
    });
  });

  it("removing the material chip takes it back out", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/materials": library(), "GET /api/browse/materials": browse() });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await attachFromLibrary(user);
    await user.click(screen.getByRole("button", { name: "移除「The Umbrella Stand」" }));
    expect(screen.queryByRole("button", { name: /移除/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送出" })).toBeDisabled();
  });

  it("the 附上文章 popover and its dialogs close on Escape and on a click outside", async () => {
    signIn();
    mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    const heading = await screen.findByRole("heading", { name: HOME, level: 1 });
    const attach = screen.getByRole("button", { name: "附上文章" });

    await user.click(attach);
    expect(attach).toHaveAttribute("aria-expanded", "true");
    const menu = within(screen.getByRole("group", { name: "附上文章" }));
    expect(menu.getAllByRole("button").map((b) => b.textContent)).toEqual(["上傳檔案", "從教材挑選", "貼上文字"]);
    await user.keyboard("{Escape}");
    expect(attach).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("group", { name: "附上文章" })).not.toBeInTheDocument();
    expect(attach).toHaveFocus();

    await user.click(attach);
    await user.click(heading);
    expect(screen.queryByRole("group", { name: "附上文章" })).not.toBeInTheDocument();

    await user.click(attach);
    await user.click(screen.getByRole("button", { name: "上傳檔案" }));
    expect(await screen.findByRole("dialog", { name: "上傳檔案" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(attach).toHaveFocus();

    await user.click(attach);
    await user.click(screen.getByRole("button", { name: "貼上文字" }));
    await user.click(screen.getByRole("dialog").parentElement as HTMLElement);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("says the privacy reminder once (in the upload dialog while it is open), lists the tools (照我的學習單做 first) and the latest sessions", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      [`GET ${SESSIONS}`]: {
        sessions: Array.from({ length: 7 }, (_, i) => summary({ id: i + 1, title: `備課 ${i + 1}` })),
      },
    });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    expect(screen.getAllByText(/請不要上傳學生作業/)).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(screen.getByRole("button", { name: "上傳檔案" }));
    const dialog = within(await screen.findByRole("dialog", { name: "上傳檔案" }));
    expect(await dialog.findByRole("form", { name: "上傳檔案" })).toBeInTheDocument();
    expect(screen.getAllByText(/請不要上傳學生作業/)).toHaveLength(1);
    expect(dialog.getByText(/請不要上傳學生作業/)).toBeInTheDocument();
    await user.click(dialog.getByRole("button", { name: "關閉" }));

    const recent = within(await screen.findByRole("region", { name: "最近的備課" }));
    expect(recent.getAllByRole("link").filter((a) => a.textContent?.startsWith("備課"))).toHaveLength(5);
    expect(recent.getByRole("link", { name: "備課 1" })).toHaveAttribute("href", "/prep/1");
    expect(recent.getByRole("link", { name: "全部" })).toHaveAttribute("href", "/prep");
  });
});

// ---------------------------------------------------------------- session page

describe("備課對話 (session page)", () => {
  it("a worksheet turn shows the job progress while it runs", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([
        turn("worksheet", { status: "running", worksheet: { id: 11, title: "U", version_no: 1, running: true } }),
      ]),
      "GET /api/worksheets/11/jobs": {
        running: true,
        version_no: 1,
        jobs: [job(1, "tasks", { status: "running", outcome: null, phase: "正在讀第 2 段" }), job(2, "warmup", { status: "queued", outcome: null })],
      },
    });
    renderApp("/prep/7");
    const progress = await screen.findByRole("status", { name: "產生進度" });
    expect(within(progress).getByText("Manna 正在產生")).toBeInTheDocument();
    expect(await within(progress).findByText("逐段的問題與任務")).toBeInTheDocument();
    expect(within(progress).getByText("進行中：正在讀第 2 段")).toBeInTheDocument();
    expect(within(progress).getByText("排隊中")).toBeInTheDocument();
  });

  it("renders each kind of turn: worksheet, revise, activity, resources, ask and a failed one", async () => {
    signIn();
    const wsTurn = turn("worksheet", { worksheet: { id: 11, title: "U", version_no: 3, running: false } });
    const reviseTurn = turn("revise", {
      request: "B 區改簡單一點",
      result: { reply: "已把 B 區的題目改短。", changed_block_ids: ["b2"], version_no: 3 },
      interaction_id: 80,
    });
    const activityTurn = turn("activity", { result: plan, interaction_id: 77 });
    const resourcesTurn = turn("resources", {
      request: "找教學資源",
      result: {
        curriculum: [{ code: "5-IV-2", text: "能讀懂簡易故事的主要內容", school_track: null, similarity: 0.8 }],
        materials: [{ id: 6, title: "Rainy Day Reader" }],
        note: "外部資源（影片、網站）搜尋還沒有開放。",
      },
    });
    const askTurn = turn("ask", {
      request: "怎麼開場？",
      result: { answer: "可以先問學生上次忘記帶傘的經驗。", follow_ups: ["給我一段英文開場白"] },
      interaction_id: 81,
    });
    const failedTurn = turn("activity", { status: "failed", error: "Manna 這次沒有回應，請再試一次。" });
    const all = session([wsTurn, reviseTurn, activityTurn, resourcesTurn, askTurn, failedTurn]);
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: all,
      "GET /api/worksheets/11": worksheet(),
      "GET /api/worksheets/11/sheet": sheetRoute,
      "POST /api/ai/77/decision": { ok: true },
      "POST /api/ai/62/decision": { ok: true },
      [`POST ${S7}/turns`]: { ...all, running: true, turns: [...all.turns, turn("ask", { seq: 7, status: "running" })] },
    });
    const user = userEvent.setup();
    renderApp("/prep/7");

    // The teacher's words are bubbles.
    expect(await screen.findByText("B 區改簡單一點")).toHaveClass("prep-bubble");

    // worksheet: the student version, read-only, a section per block with copy and ratings.
    const preview = await screen.findByRole("article", { name: "一般版 學生版" });
    const blockB = within(preview).getByRole("region", { name: "區塊 B 活動" });
    expect(within(blockB).getByText("Why did Mrs. Lin start the stand?")).toBeInTheDocument();
    expect(within(blockB).queryByText("A stranger once helped her.")).not.toBeInTheDocument();
    expect(within(blockB).getByRole("button", { name: "複製區塊 B 活動" })).toBeInTheDocument();
    await user.click(within(blockB).getByRole("button", { name: "區塊 B 活動有幫助" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/ai/62/decision")).toEqual({ decision: "skip", thumbs: "up" }));
    expect(screen.getByRole("link", { name: "直接編輯" })).toHaveAttribute("href", "/worksheets/11");
    expect(screen.queryByRole("link", { name: "開啟編輯器" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "詞彙檢查" })).toHaveAttribute("href", "/worksheets/11/check");
    // One 匯出 menu, not two look-alike links; the file is named in the menu, whichever version is shown.
    expect(screen.queryByRole("button", { name: "匯出 DOCX" })).not.toBeInTheDocument();
    const exportButton = screen.getByRole("button", { name: /匯出/ });
    expect(exportButton).toBeEnabled();
    expect(exportButton).toHaveAttribute("aria-haspopup", "menu");
    await user.click(exportButton);
    expect(screen.getAllByRole("menuitem").map((i) => i.textContent)).toEqual([
      "學生版 Word",
      "學生版 PDF",
      "教師版 Word",
      "教師版 PDF",
    ]);
    // The student version is shown, and 教師版 PDF still asks for the teacher file of this level.
    await user.click(screen.getByRole("menuitem", { name: "教師版 PDF" }));
    await waitFor(() => expect(api.callsTo("GET", "/api/worksheets/11/export")).toHaveLength(1));
    const exportUrl = api.callsTo("GET", "/api/worksheets/11/export")[0]!.url;
    expect(new URLSearchParams(exportUrl.split("?")[1]).toString()).toBe("format=pdf&level=standard&audience=teacher");
    // (The mock has no file to give: the failure is said.)
    expect(await screen.findByText("測試未定義：GET /api/worksheets/11/export")).toBeInTheDocument();
    expect(screen.getByLabelText("版本")).toHaveTextContent("第 3 版，共 3 版");
    // 教師版 asks for the teacher projection.
    await user.click(screen.getByRole("radio", { name: "教師版" }));
    expect(await screen.findByText("A stranger once helped her.")).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/worksheets/11/sheet").some((c) => c.url.includes("audience=teacher"))).toBe(true);

    // revise: the reply and which blocks changed, by their printed letters.
    expect(screen.getByText("已把 B 區的題目改短。")).toBeInTheDocument();
    expect(await screen.findByText("已修改：B 區（第 3 版）")).toBeInTheDocument();

    // activity: goals table with code chips, step cards, options, the two collapsibles, ratings.
    expect(screen.getByRole("columnheader", { name: "目標" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "對應課綱" })).toBeInTheDocument();
    // The goals in two labelled groups, each goal in Chinese with its English under it.
    const goals = activityGoals();
    const content = within(goals.getByRole("rowgroup", { name: "內容目標" }));
    expect(content.getByText("學生要理解或完成什麼")).toBeInTheDocument();
    expect(content.getByText("學生能說出林太太設置傘架的原因")).toBeInTheDocument();
    expect(content.getByText("Students will be able to explain why Mrs. Lin set up the umbrella stand.")).toHaveAttribute(
      "lang",
      "en",
    );
    expect(content.getByText("5-IV-2")).toHaveAttribute("title", "能讀懂簡易故事的主要內容");
    const language = within(goals.getByRole("rowgroup", { name: "語言目標" }));
    expect(language.getByText("為了做到，需要用什麼語言")).toBeInTheDocument();
    expect(language.getByText("學生能用 because 說明原因")).toBeInTheDocument();
    expect(language.getByText("Students will be able to use because to give a reason.")).toBeInTheDocument();
    expect(language.queryByText("5-IV-2")).toBeNull();
    // 複製 copies both groups in both languages.
    const copy = goals.getByRole("button", { name: "複製學習目標" });
    await user.click(copy);
    expect(await navigator.clipboard.readText()).toBe(goalsText(plan));
    expect(copy).toHaveTextContent("已複製");
    const warm = within(screen.getByRole("region", { name: "引起動機：雨天經驗" }));
    expect(warm.getByText("5 分鐘")).toBeInTheDocument();
    expect(warm.getByText("教學思路")).toBeInTheDocument();
    expect(warm.getByText("選項一")).toBeInTheDocument();
    expect(warm.getByText("選項二")).toBeInTheDocument();
    expect(warm.getAllByText("展開查看建議腳本").length).toBeGreaterThan(0);
    expect(warm.getByText("卡住了怎麼辦")).toBeInTheDocument();
    await user.click(warm.getByRole("button", { name: "「雨天經驗」不合用" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/ai/77/decision")).toEqual({ decision: "skip", thumbs: "down" }));
    expect(warm.getByRole("button", { name: "「雨天經驗」不合用" })).toHaveAttribute("aria-pressed", "true");
    // A step without a tip has no 卡住了怎麼辦.
    expect(within(screen.getByRole("region", { name: "發展活動：逐段找原因" })).queryByText("卡住了怎麼辦")).toBeNull();

    // resources: curriculum rows, the teacher's materials, the note.
    const curriculum = within(screen.getByRole("region", { name: "課綱條目" }));
    expect(curriculum.getByText("5-IV-2")).toBeInTheDocument();
    expect(curriculum.getByText("能讀懂簡易故事的主要內容")).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "你的教材" })).getByRole("button", { name: "Rainy Day Reader" })).toBeInTheDocument();
    expect(screen.getByText("外部資源（影片、網站）搜尋還沒有開放。")).toBeInTheDocument();

    // ask: the answer and its follow-ups as chips that ask again.
    expect(screen.getByText("可以先問學生上次忘記帶傘的經驗。")).toBeInTheDocument();
    const followUps = within(screen.getByRole("group", { name: "接著問" }));
    expect(followUps.getByRole("button", { name: "給我一段英文開場白" })).toBeInTheDocument();

    // failed: the error and 再試一次, which sends the same kind and request again.
    expect(screen.getByText("Manna 這次沒有回應，請再試一次。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "再試一次" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "activity", request: "規劃教學活動" });
  });

  it("an activity plan made before the goal groups shows its goals as one list, as before", async () => {
    signIn();
    // As stored by version 1 or 2 of prep_activity: one Chinese line per goal, no kind.
    const old = {
      ...plan,
      goals: [
        { text: "學生能說出林太太設置傘架的原因", curriculum_codes: ["5-IV-2"] },
        { text: "學生能用課文句型寫出兩句話", curriculum_codes: [] },
      ],
    } satisfies ActivityPlan;
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session([turn("activity", { result: old, interaction_id: 77 })]) });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await screen.findByText("學生能用課文句型寫出兩句話");
    const goals = activityGoals();
    expect(goals.getByRole("columnheader", { name: "目標" })).toBeInTheDocument();
    expect(goals.getByText("學生能說出林太太設置傘架的原因")).toBeInTheDocument();
    expect(goals.getByText("學生能用課文句型寫出兩句話")).toBeInTheDocument();
    expect(goals.getByText("5-IV-2")).toBeInTheDocument();
    expect(goals.queryByText("內容目標")).toBeNull();
    expect(goals.queryByText("語言目標")).toBeNull();
    expect(goals.queryByRole("rowgroup", { name: "內容目標" })).toBeNull();
    await user.click(goals.getByRole("button", { name: "複製學習目標" }));
    expect(await navigator.clipboard.readText()).toBe("1. 學生能說出林太太設置傘架的原因（5-IV-2）\n2. 學生能用課文句型寫出兩句話");
  });

  it("a follow-up chip asks again, and the next-step chips send their kind", async () => {
    signIn();
    const askTurn = turn("ask", {
      request: "怎麼開場？",
      result: { answer: "先問經驗。", follow_ups: ["給我一段英文開場白"] },
    });
    const base = session([askTurn], { next: ["comprehension", "tiered", "activity", "kit", "worksheet"] });
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: base,
      [`POST ${S7}/turns`]: base,
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    await user.click(await screen.findByRole("button", { name: "給我一段英文開場白" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "ask", request: "給我一段英文開場白" });

    expect(screen.getByText("接下來可以：")).toBeInTheDocument();
    const next = within(screen.getByRole("group", { name: "接下來可以" }));
    expect(next.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "閱讀理解題",
      "差異化學習單",
      "課堂活動",
      "教學套件",
      "完整學習單",
    ]);
    // The chip reads the tool's name; the request keeps the label the server knows.
    await user.click(next.getByRole("button", { name: "完整學習單" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(2));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "worksheet", request: "學習單" });
  });

  it("the bottom bar sends a chat turn on Enter; Shift+Enter is a new line", async () => {
    signIn();
    const wsTurn = turn("worksheet", { worksheet: { id: 11, title: "U", version_no: 3, running: false } });
    const done = session([wsTurn]);
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: done,
      "GET /api/worksheets/11": worksheet(),
      "GET /api/worksheets/11/sheet": sheetRoute,
      [`POST ${S7}/turns`]: done,
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const bar = await screen.findByRole("textbox", { name: "給 Manna 的話" });
    // One box for questions and changes, and the worksheet it works on.
    expect(bar).toHaveAttribute("placeholder", "問問題，或說要怎麼改學習單（例如：把 C 區第 2 題改成比較表）");
    expect(screen.getByText("目前的學習單：U")).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "問 Manna" })).not.toBeInTheDocument();
    expect(screen.getByText("AI 產生的內容可能有錯，請核對重要資訊。")).toBeInTheDocument();

    await user.type(bar, "第一行{Shift>}{Enter}{/Shift}第二行");
    expect(bar).toHaveValue("第一行\n第二行");
    expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(0);

    await user.clear(bar);
    await user.type(bar, "B 區改簡單一點{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "chat", request: "B 區改簡單一點" });
    await waitFor(() => expect(bar).toHaveValue(""));
  });

  it("the bar is disabled while a turn runs", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([turn("activity", { status: "running" })], { next: ["worksheet"] }),
    });
    renderApp("/prep/7");
    const bar = await screen.findByRole("textbox", { name: "給 Manna 的話" });
    expect(bar).toBeDisabled();
    expect(bar).toHaveAttribute("placeholder", "Manna 正在處理…");
    expect(screen.getAllByText("Manna 正在處理…").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "送出" })).toBeDisabled();
    expect(within(screen.getByRole("group", { name: "接下來可以" })).getByRole("button", { name: "完整學習單" })).toBeDisabled();
  });

  it("polls while a turn runs and stops when running is false", async () => {
    signIn();
    let reads = 0;
    const running = session([turn("activity", { status: "running" })]);
    const done = session([turn("activity", { result: plan, interaction_id: 77 })]);
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: () => {
        reads += 1;
        return reads < 3 ? running : done;
      },
    });
    renderApp("/prep/7");
    expect(await screen.findByText("雨天經驗")).toBeInTheDocument();
    const bar = screen.getByRole("textbox", { name: "給 Manna 的話" });
    expect(bar).toBeEnabled();
    expect(bar).toHaveAttribute("placeholder", "問問題，或說要怎麼改學習單（例如：把 C 區第 2 題改成比較表）");
    const count = api.callsTo("GET", S7).length;
    expect(count).toBe(3);
    await new Promise((r) => setTimeout(r, 150));
    expect(api.callsTo("GET", S7)).toHaveLength(count);
  });

  it("changes the class through PATCH and deletes only after the in-page confirm", async () => {
    signIn();
    const base = session([turn("activity", { result: plan })]);
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/classes": [schoolClass({ id: 3, name: "高一 3 班" })],
      [`GET ${S7}`]: base,
      [`PATCH ${S7}`]: (call: MockCall) => ({ ...base, class: { id: 3, name: "高一 3 班" }, ...(call.body as object) }),
      [`DELETE ${S7}`]: { ok: true },
      [`GET ${SESSIONS}`]: { sessions: [] },
    });
    const user = userEvent.setup();
    const { router } = renderApp("/prep/7");
    expect(await screen.findByRole("heading", { name: "The Umbrella Stand", level: 1 })).toBeInTheDocument();
    const context = within(screen.getByRole("group", { name: "備課情境" }));
    expect(context.getByRole("button", { name: "The Umbrella Stand" })).toBeInTheDocument();
    await user.selectOptions(await context.findByRole("combobox", { name: "班級" }), "高一 3 班");
    await waitFor(() => expect(api.lastBody("PATCH", S7)).toEqual({ class_id: 3 }));

    await user.click(context.getByRole("button", { name: "刪除" }));
    expect(context.getByText("刪除這份備課？裡面的學習單也會一起刪除。")).toBeInTheDocument();
    expect(api.callsTo("DELETE", S7)).toHaveLength(0);
    await user.click(context.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep"));
  });
});

// ---------------------------------------------------------------- the shell (the list itself: myWork.test.tsx)

describe("the shell around a session", () => {
  it("a session counts as being inside 我的作品 in the sidebar", async () => {
    signIn();
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session([turn("activity", { result: plan })]) });
    renderApp("/prep/7");
    await screen.findByRole("heading", { name: "The Umbrella Stand", level: 1 });
    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    expect(nav.getByRole("link", { name: "我的作品" })).toHaveAttribute("aria-current", "true");
    expect(nav.getAllByRole("link").filter((a) => a.getAttribute("aria-current") === "page")).toHaveLength(0);
  });

  it("a worksheet made in a session links back to it", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/worksheets/11": worksheet(),
      "GET /api/worksheets": [],
      "GET /api/worksheets/11/checks": [],
      "GET /api/materials/5": { ...material(), text: "x", paragraphs: [] },
      "GET /api/materials/5/annotations": [],
    });
    renderApp("/worksheets/11");
    expect(await screen.findByRole("link", { name: "← 回到備課對話" })).toHaveAttribute("href", "/prep/7");
  });
});

// ---------------------------------------------------------------- 學習目標 (lib)

describe("學習目標：內容目標與語言目標", () => {
  it("groups the goals, 內容目標 first, and copies each group under its label in both languages", () => {
    const mixed: ActivityPlan = {
      ...plan,
      goals: [
        { kind: "language", zh: "學生能用 because 說明原因", en: "Students will be able to use because.", curriculum_codes: [] },
        { kind: "content", zh: "學生能說出主旨", en: "Students will be able to tell the main idea.", curriculum_codes: ["5-IV-2", "3-V-1"] },
        { kind: "content", zh: "學生能比較兩個角色", en: "Students will be able to compare two characters.", curriculum_codes: [] },
      ],
    };
    expect(goalGroups(mixed).map((g) => [g.label, g.goals.length])).toEqual([
      ["內容目標", 2],
      ["語言目標", 1],
    ]);
    expect(goalsText(mixed)).toBe(
      [
        "【內容目標】",
        "1. 學生能說出主旨（5-IV-2、3-V-1）",
        "   Students will be able to tell the main idea.",
        "2. 學生能比較兩個角色",
        "   Students will be able to compare two characters.",
        "【語言目標】",
        "1. 學生能用 because 說明原因",
        "   Students will be able to use because.",
      ].join("\n"),
    );
  });

  it("keeps the goals of a plan made before the groups as one list without a label", () => {
    const old: ActivityPlan = {
      ...plan,
      goals: [
        { text: "學生能說出主旨", curriculum_codes: ["5-IV-2"] },
        { text: "學生能寫出兩句", curriculum_codes: [] },
      ],
    };
    expect(goalGroups(old)).toEqual([
      {
        kind: null,
        label: null,
        hint: null,
        goals: [
          { ...OLD_GOAL, zh: "學生能說出主旨", curriculum_codes: ["5-IV-2"] },
          { ...OLD_GOAL, zh: "學生能寫出兩句" },
        ],
      },
    ]);
    expect(goalsText(old)).toBe("1. 學生能說出主旨（5-IV-2）\n2. 學生能寫出兩句");
    expect(goalGroups({ ...plan, goals: [] })).toEqual([]);
  });

  it("a 語言目標 of version 4 copies with its kind and how it serves its 內容目標; a teacher's goal is marked", () => {
    expect(goalsText(planV4)).toBe(
      [
        "【內容目標】",
        "1. 學生能說出林太太設置傘架的原因（教師提供）（5-IV-2）",
        "   Students will be able to explain why Mrs. Lin set up the umbrella stand.",
        "【語言目標】",
        "1. 〔學習策略〕學生能在讀不懂時重讀並畫出關鍵句",
        "   Students will be able to reread and underline the key sentence.",
        "   學生要用重讀並畫出關鍵句的策略，才能說出林太太設置傘架的原因",
      ].join("\n"),
    );
    const [, language] = goalGroups(planV4);
    expect(language!.goals[0]).toMatchObject({ category: "strategy", supports: 1, fromTeacher: false });
    // A teacher's English goal has no Chinese line: the English one leads.
    const english: ActivityPlan = {
      ...planV4,
      goals: [{ kind: "content", zh: "", en: "Students will be able to retell the story.", curriculum_codes: [], from_teacher: true }],
    };
    expect(goalsText(english)).toBe("【內容目標】\n1. Students will be able to retell the story.（教師提供）");
  });

  it("copies 課前準備, a step's grouping, roles and skills, and 複習與評量", () => {
    expect(prepareText(planV4)).toBe(
      [
        "【關鍵字彙】",
        "trust：信任",
        "neighbor：鄰居",
        "【教材與教具】",
        "課文第 1–4 段、老師準備的傘架照片",
        "【高層次問題】",
        "1. Why do you think the plan worked?（評鑑）",
        "【背景連結】",
        "請學生回想借東西給別人的經驗。",
      ].join("\n"),
    );
    expect(reviewText(planV4)).toBe("複習：用字卡複習 trust 與 neighbor。\n評量：exit ticket 寫出一個原因。");
    expect(stepText(planV4.steps[1]!)).toBe(
      [
        "【發展活動】逐段找原因（25 分鐘）",
        "分組：小組｜語言技能：說、讀",
        "每段找一個原因。",
        "分工：主持：帶討論；記錄：寫下原因；報告：向全班說明",
        "教師巡視：巡視各組，先聽記錄員的原因。",
        "鷹架：教師先示範第 1 段。",
      ].join("\n"),
    );
    // A plan made before has none of them.
    expect(hasPrepare(plan)).toBe(false);
    expect(prepareText(plan)).toBe("");
    expect(reviewText(plan)).toBe("");
    expect(stepText(plan.steps[1]!)).toBe("【發展活動】逐段找原因（25 分鐘）\n每段找一個原因。");
  });
});

// ---------------------------------------------------------------- 課堂活動（SIOP）與自我檢核

describe("課堂活動的 SIOP 欄位與 SIOP 自我檢核", () => {
  it("shows 課前準備, each step's grouping, skills, roles, monitoring and scaffolding, and 複習與評量", async () => {
    signIn();
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session([turn("activity", { result: planV4, interaction_id: 77 })]) });
    renderApp("/prep/7");
    const prepare = within(await screen.findByRole("region", { name: "課前準備" }));
    expect(prepare.getByText("背景連結")).toBeInTheDocument();
    expect(prepare.getByText("請學生回想借東西給別人的經驗。")).toBeInTheDocument();
    expect(prepare.getByText("trust")).toHaveAttribute("lang", "en");
    expect(prepare.getByText("信任")).toBeInTheDocument();
    expect(prepare.getByText("課文第 1–4 段、老師準備的傘架照片")).toBeInTheDocument();
    expect(prepare.getByText("Why do you think the plan worked?")).toBeInTheDocument();
    expect(prepare.getByText("評鑑")).toBeInTheDocument();
    expect(prepare.getByRole("button", { name: "複製課前準備" })).toBeInTheDocument();

    // The goals: a 語言目標's kind and relation, a teacher's goal marked.
    const goals = activityGoals();
    const content = within(goals.getByRole("rowgroup", { name: "內容目標" }));
    expect(content.getByText("教師提供")).toBeInTheDocument();
    const language = within(goals.getByRole("rowgroup", { name: "語言目標" }));
    expect(language.getByText("學習策略")).toBeInTheDocument();
    expect(language.getByText("支持內容目標 1：")).toBeInTheDocument();
    expect(language.getByText(/學生要用重讀並畫出關鍵句的策略，才能/)).toBeInTheDocument();

    const warm = within(screen.getByRole("region", { name: "引起動機：雨天經驗" }));
    expect(warm.getByText("兩人一組")).toBeInTheDocument();
    expect(warm.getByLabelText("語言技能：聽、說")).toBeInTheDocument();
    expect(warm.getByText("夥伴 A")).toBeInTheDocument();
    const develop = within(screen.getByRole("region", { name: "發展活動：逐段找原因" }));
    expect(develop.getByText("小組")).toBeInTheDocument();
    expect(develop.getByText("分工")).toBeInTheDocument();
    expect(develop.getByText("主持")).toBeInTheDocument();
    expect(develop.getByText("教師巡視")).toBeInTheDocument();
    expect(develop.getByText("巡視各組，先聽記錄員的原因。")).toBeInTheDocument();
    expect(develop.getByText("鷹架")).toBeInTheDocument();

    const review = within(screen.getByRole("region", { name: "複習與評量" }));
    expect(review.getByText("用字卡複習 trust 與 neighbor。")).toBeInTheDocument();
    expect(review.getByText("exit ticket 寫出一個原因。")).toBeInTheDocument();
  });

  it("a plan made before version 4 shows none of the SIOP fields, and an old server shows no self-check", async () => {
    signIn();
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session([turn("activity", { result: plan, interaction_id: 77 })]) });
    renderApp("/prep/7");
    await screen.findByRole("region", { name: "引起動機：雨天經驗" });
    expect(screen.queryByRole("region", { name: "課前準備" })).toBeNull();
    expect(screen.queryByRole("region", { name: "複習與評量" })).toBeNull();
    expect(screen.queryByText("分工")).toBeNull();
    expect(screen.queryByText("教師巡視")).toBeNull();
    // No `siop_check` on the turn: no panel.
    expect(screen.queryByRole("region", { name: "SIOP 自我檢核" })).toBeNull();
  });

  it("a finished check opens to every item with its rating and reason, says which items need a classroom, and has no score", async () => {
    signIn();
    const activity = turn("activity", { result: planV4, interaction_id: 77, siop_check: siopCheck("done") });
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session([activity]) });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const panel = within(await screen.findByRole("region", { name: "SIOP 自我檢核" }));
    // Collapsed until opened.
    const summary = panel.getByText("SIOP 自我檢核", { selector: "summary" });
    expect(summary.closest("details")).not.toHaveAttribute("open");
    await user.click(summary);
    expect(summary.closest("details")).toHaveAttribute("open");
    expect(panel.getByText(/這是給你的建議，不是評分/)).toBeInTheDocument();
    const goalsItem = within(panel.getByText("內容目標與語言目標").closest("li")!);
    expect(goalsItem.getByText("符合")).toHaveClass("siop-rating", "is-met");
    expect(goalsItem.getByText("兩種目標都寫了，語言目標寫出了和內容目標的關係。")).toBeInTheDocument();
    expect(within(panel.getByText("概念適切").closest("li")!).getByText("部分符合")).toHaveClass("is-partial");
    expect(within(panel.getByText("分組").closest("li")!).getByText("未見")).toBeInTheDocument();
    // Judged on the visible part only: the panel says so.
    const visual = within(panel.getByText("讓概念清楚的技巧").closest("li")!);
    expect(visual.getByText("只能部分判斷：只看得到書面的視覺輔助")).toBeInTheDocument();
    // An item this check did not judge.
    expect(within(panel.getByText("評量").closest("li")!).getByText("這次沒有判斷")).toBeInTheDocument();
    // The classroom items: named, never rated.
    expect(panel.getByText("這幾項要在課堂上看，AI 不評：")).toBeInTheDocument();
    expect(panel.getAllByText("需課堂觀察")).toHaveLength(4);
    expect(panel.getByText("等待時間")).toBeInTheDocument();
    expect(panel.queryByText(/總分|分數/)).toBeNull();
    expect(panel.queryByRole("button")).toBeNull();
  });

  it("a running check is polled until it is done; a failed one can be run again and the output stays", async () => {
    signIn();
    const activity = turn("activity", { result: planV4, interaction_id: 77, siop_check: siopCheck("running") });
    let polled = 0;
    let retried = false;
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([activity]),
      [`GET ${S7}/turns/${activity.id}/siop-check`]: () =>
        retried
          ? siopCheck("done", { id: 2, run_no: 2 })
          : ++polled < 2
            ? siopCheck("running")
            : siopCheck("failed", { error: "AI 目前沒有回應，這次沒有完成。請稍後按「再試一次」。" }),
      [`POST ${S7}/turns/${activity.id}/siop-check`]: siopCheck("running", { id: 2, run_no: 2 }),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const panel = within(await screen.findByRole("region", { name: "SIOP 自我檢核" }));
    expect(panel.getByRole("status")).toHaveTextContent("Manna 正在做 SIOP 自我檢核…");
    // Failed: said plainly, the output is untouched, 再試一次.
    expect(await panel.findByRole("alert")).toHaveTextContent("這次的 SIOP 自我檢核沒有完成，產出不受影響。");
    expect(panel.getByText(/AI 目前沒有回應/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "引起動機：雨天經驗" })).toBeInTheDocument();
    // The next poll after 再試一次 finds the new run done.
    retried = true;
    await user.click(panel.getByRole("button", { name: "再試一次" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns/${activity.id}/siop-check`)).toHaveLength(1));
    expect(await panel.findByText("SIOP 自我檢核", { selector: "summary" })).toBeInTheDocument();
  });

  it("a worksheet revised after its check offers 重新檢核; a turn made before the check offers 開始檢核", async () => {
    signIn();
    const ws = turn("comprehension", {
      worksheet: { id: 11, title: "U", version_no: 5, running: false },
      siop_check: siopCheck("done", { worksheet_version_no: 3 }),
    });
    const old = turn("activity", { result: plan, siop_check: null });
    const api = mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session([ws, old]),
      "GET /api/worksheets/11": worksheet(),
      "GET /api/worksheets/11/sheet": sheetRoute,
      [`POST ${S7}/turns/${ws.id}/siop-check`]: siopCheck("done", { id: 3, run_no: 2, worksheet_version_no: 5 }),
      [`POST ${S7}/turns/${old.id}/siop-check`]: siopCheck("done", { id: 4 }),
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const [wsPanel, oldPanel] = (await screen.findAllByRole("region", { name: "SIOP 自我檢核" })).map((p) => within(p));
    await user.click(wsPanel!.getByText("SIOP 自我檢核", { selector: "summary" }));
    expect(wsPanel!.getByText("這是第 3 版的檢核；學習單之後改過了。")).toBeInTheDocument();
    await user.click(wsPanel!.getByRole("button", { name: "重新檢核" }));
    await waitFor(() => expect(wsPanel!.queryByText(/學習單之後改過了/)).toBeNull());
    expect(api.callsTo("POST", `${S7}/turns/${ws.id}/siop-check`)).toHaveLength(1);
    // The new results show open, as the teacher left the panel.
    expect(wsPanel!.getByText("SIOP 自我檢核", { selector: "summary" }).closest("details")).toHaveAttribute("open");

    expect(oldPanel!.getByText("這份產出還沒有做 SIOP 自我檢核。")).toBeInTheDocument();
    await user.click(oldPanel!.getByRole("button", { name: "開始檢核" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns/${old.id}/siop-check`)).toHaveLength(1));
    expect(await oldPanel!.findByText("SIOP 自我檢核", { selector: "summary" })).toBeInTheDocument();
  });
});
