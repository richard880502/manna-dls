// 備課對話 (prep sessions, the contract's "Frontend"): the home page's input bar and its chips, the
// 直接問 AI tab, the conversation page with a card per turn kind, the bottom bar, polling, 我的備課,
// and the navigation (備課｜我的備課｜教材｜班級).
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  ActivityPlan,
  Material,
  MaterialLibrary,
  PrepKind,
  PrepSession,
  PrepSessionSummary,
  PrepTurn,
  WorksheetState,
  WsAudience,
  WsJob,
  WsSheet,
  WsTemplateSummary,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, emptyLibrary, mockApi, renderApp, schoolClass, signIn, type MockCall } from "../test/mockApi";

const HOME = "老師，今天要教哪一課？";
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
  goals: [{ text: "學生能說出林太太設置傘架的原因", curriculum_codes: ["5-IV-2"] }],
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

const sheetRoute = (call: MockCall) => sheet(new URL(call.url, "http://x").searchParams.get("audience") as WsAudience);

// ---------------------------------------------------------------- home

describe("備課 (home)", () => {
  /** Picks the library material on the home page, so the action chips open up. */
  async function pickMaterial(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByRole("button", { name: "從教材庫選" }));
    await user.click(await screen.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    expect(screen.getByRole("status")).toHaveTextContent("教材：「The Umbrella Stand」");
  }

  it.each([
    ["做學習單", "worksheet"],
    ["規劃教學活動", "activity"],
    ["設計評量", "assessment"],
    ["找教學資源", "resources"],
  ] as const)("「%s」 creates a %s session from the chosen material and opens it", async (label, kind) => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      [`POST ${SESSIONS}`]: session([turn(kind, { request: label, status: "running" })]),
      [`GET ${S7}`]: session([turn(kind, { request: label, status: "running" })]),
      "GET /api/worksheets/11/jobs": { running: true, jobs: [], version_no: 1 },
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    expect(await screen.findByRole("heading", { name: HOME, level: 1 })).toBeInTheDocument();

    const chips = within(screen.getByRole("group", { name: "要 Manna 做什麼" }));
    expect(chips.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "做學習單",
      "規劃教學活動",
      "設計評量",
      "找教學資源",
      "我想做別的…",
    ]);
    // Nothing to work from yet: the chips wait for a material.
    expect(chips.getByRole("button", { name: label })).toBeDisabled();

    await pickMaterial(user);
    await user.click(chips.getByRole("button", { name: label }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ kind, request: label, material_id: 5 });
  });

  it("sends the chosen class and template, and 我想做別的… sends what the teacher typed as an ask", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/classes": [schoolClass({ id: 3, name: "高一 3 班" })],
      "GET /api/worksheet-templates": { templates: [template()] },
      [`POST ${SESSIONS}`]: session([turn("ask", { status: "running" })]),
      [`GET ${S7}`]: session([turn("ask", { status: "running" })]),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await pickMaterial(user);

    const context = within(screen.getByRole("group", { name: "備課情境（可以不選）" }));
    const classSelect = context.getByRole("combobox", { name: "班級" });
    expect(classSelect).toHaveDisplayValue("不指定");
    await user.selectOptions(classSelect, "高一 3 班");
    const templateSelect = context.getByRole("combobox", { name: /照我的版型/ });
    expect(templateSelect).toHaveDisplayValue("不使用版型");
    await user.selectOptions(templateSelect, "9");

    await user.click(screen.getByRole("button", { name: "我想做別的…" }));
    await user.type(screen.getByRole("textbox", { name: "你想做什麼？" }), "幫我想三個暖身活動");
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    // An ask does not use a template.
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "ask",
      request: "幫我想三個暖身活動",
      material_id: 5,
      class_id: 3,
    });
  });

  it("pasted text goes along as material; with only a topic, 找教學資源 is the one chip that works", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      [`POST ${SESSIONS}`]: session([turn("resources", { status: "running" })]),
      [`GET ${S7}`]: session([turn("resources", { status: "running" })]),
    });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });

    await user.type(screen.getByRole("textbox", { name: /或只輸入主題/ }), "環境保護");
    const chips = within(screen.getByRole("group", { name: "要 Manna 做什麼" }));
    expect(chips.getByRole("button", { name: "做學習單" })).toBeDisabled();
    expect(chips.getByRole("button", { name: "找教學資源" })).toBeEnabled();
    await user.click(chips.getByRole("button", { name: "找教學資源" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ kind: "resources", request: "找教學資源", topic: "環境保護" });
  });

  it("pasted text is sent as material", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      [`POST ${SESSIONS}`]: session([turn("activity", { status: "running" })]),
      [`GET ${S7}`]: session([turn("activity", { status: "running" })]),
    });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("button", { name: "貼上文字" }));
    await user.type(screen.getByRole("textbox", { name: /教材標題/ }), "Umbrella");
    await user.type(screen.getByRole("textbox", { name: "貼上教材文字" }), "Mrs. Lin put a stand outside.");
    await user.click(screen.getByRole("button", { name: "規劃教學活動" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "activity",
      request: "規劃教學活動",
      material: { title: "Umbrella", text: "Mrs. Lin put a stand outside." },
    });
  });

  it("「直接問 AI」 sends the question as an ask session, with the class but without material unless attached", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/classes": [schoolClass({ id: 3, name: "高一 3 班" })],
      [`POST ${SESSIONS}`]: session([turn("ask", { status: "running" })]),
      [`GET ${S7}`]: session([turn("ask", { status: "running" })]),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await user.click(await screen.findByRole("tab", { name: "直接問 AI" }));
    expect(screen.getByRole("tab", { name: "直接問 AI" })).toHaveAttribute("aria-selected", "true");

    const box = screen.getByRole("textbox", { name: "直接問 AI" });
    expect(box).toHaveAttribute("placeholder", "說說你想做什麼，例如：幫我想三個這一課的暖身活動");
    expect(screen.getByRole("button", { name: "送出" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "附上教材（可以不附）" })).toBeInTheDocument();
    // No template chip here: only the class.
    expect(screen.queryByRole("combobox", { name: /照我的版型/ })).not.toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "班級" }), "高一 3 班");

    await user.type(box, "怎麼教倒裝句比較不無聊？");
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({ kind: "ask", request: "怎麼教倒裝句比較不無聊？", class_id: 3 });
  });

  it("shows the privacy reminder once, the latest sessions, and the way to the full SOP flow", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      [`GET ${SESSIONS}`]: {
        sessions: Array.from({ length: 7 }, (_, i) => summary({ id: i + 1, title: `備課 ${i + 1}` })),
      },
    });
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    // The upload form is open too, but the reminder is said once.
    expect(await screen.findByRole("form", { name: "上傳檔案" })).toBeInTheDocument();
    expect(screen.getAllByText(/請不要上傳學生作業/)).toHaveLength(1);

    const recent = within(await screen.findByRole("region", { name: "最近的備課" }));
    expect(recent.getAllByRole("link").filter((a) => a.textContent?.startsWith("備課"))).toHaveLength(5);
    expect(recent.getByRole("link", { name: "備課 1" })).toHaveAttribute("href", "/prep/1");
    expect(screen.getByRole("link", { name: "完整備課流程（班級、單元、課例 SOP）" })).toHaveAttribute("href", "/classes");
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
    expect(screen.getByRole("link", { name: "開啟編輯器" })).toHaveAttribute("href", "/worksheets/11");
    expect(screen.getByRole("link", { name: "詞彙檢查" })).toHaveAttribute("href", "/worksheets/11/check");
    expect(screen.getByRole("button", { name: "匯出 DOCX" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "匯出 PDF" })).toBeEnabled();
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
    expect(screen.getByText("學生能說出林太太設置傘架的原因")).toBeInTheDocument();
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
      "分層練習",
      "課堂活動",
      "教學套件",
      "學習單",
    ]);
    await user.click(next.getByRole("button", { name: "學習單" }));
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(2));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "worksheet", request: "學習單" });
  });

  it("the bottom bar sends a turn without kind on Enter; Shift+Enter is a new line", async () => {
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
    // There is a worksheet: the bar offers to revise it.
    expect(bar).toHaveAttribute("placeholder", "針對上面產出的內容，你有什麼想修改的嗎？");
    expect(screen.getByText("AI 產生的內容可能有錯，請核對重要資訊。")).toBeInTheDocument();

    await user.type(bar, "第一行{Shift>}{Enter}{/Shift}第二行");
    expect(bar).toHaveValue("第一行\n第二行");
    expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(0);

    await user.clear(bar);
    await user.type(bar, "B 區改簡單一點{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ request: "B 區改簡單一點" });
    await waitFor(() => expect(bar).toHaveValue(""));
  });

  it("with a worksheet, 問 Manna switches the bar from revising to asking", async () => {
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
    await user.click(screen.getByRole("radio", { name: "問 Manna" }));
    expect(bar).toHaveAttribute("placeholder", "還想問什麼？");
    await user.type(bar, "這課要上幾節？{Enter}");
    await waitFor(() => expect(api.callsTo("POST", `${S7}/turns`)).toHaveLength(1));
    expect(api.lastBody("POST", `${S7}/turns`)).toEqual({ kind: "ask", request: "這課要上幾節？" });
  });

  it("without a worksheet the bar asks 還想問什麼？, and it is disabled while a turn runs", async () => {
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
    expect(within(screen.getByRole("group", { name: "接下來可以" })).getByRole("button", { name: "學習單" })).toBeDisabled();
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
    expect(bar).toHaveAttribute("placeholder", "還想問什麼？");
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
    expect(context.getByText("刪除這次備課對話？學習單會保留。")).toBeInTheDocument();
    expect(api.callsTo("DELETE", S7)).toHaveLength(0);
    await user.click(context.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep"));
  });
});

// ---------------------------------------------------------------- 我的備課 and the shell

describe("我的備課", () => {
  it("lists the sessions with material, class, last step and date, and links to all worksheets", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      [`GET ${SESSIONS}`]: {
        sessions: [
          summary({ id: 7, class: { id: 3, name: "高一 3 班" }, last_kind: "worksheet" }),
          summary({ id: 8, title: "環境保護", material: null, topic: "環境保護", last_kind: "resources" }),
        ],
      },
    });
    renderApp("/prep");
    expect(await screen.findByRole("heading", { name: "我的備課", level: 1 })).toBeInTheDocument();
    const rows = within(await screen.findByRole("list", { name: "備課對話" }));
    expect(rows.getByRole("link", { name: "The Umbrella Stand" })).toHaveAttribute("href", "/prep/7");
    expect(rows.getByText(/教材：The Umbrella Stand · 高一 3 班 · 做學習單 · 11\/15/)).toBeInTheDocument();
    expect(rows.getByRole("link", { name: "環境保護" })).toHaveAttribute("href", "/prep/8");
    expect(rows.getByText(/主題：環境保護 · 找教學資源/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "所有學習單" })).toHaveAttribute("href", "/worksheets");
  });

  it("the top bar is 備課｜我的備課｜教材｜班級, and a session counts as 我的備課", async () => {
    signIn();
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session([turn("activity", { result: plan })]) });
    renderApp("/prep/7");
    await screen.findByRole("heading", { name: "The Umbrella Stand", level: 1 });
    const nav = within(screen.getByRole("navigation", { name: "主要導覽" }));
    expect(nav.getAllByRole("link").map((a) => a.textContent)).toEqual(["備課", "我的備課", "教材", "班級"]);
    expect(nav.getByRole("link", { name: "我的備課" })).toHaveAttribute("aria-current", "page");
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
