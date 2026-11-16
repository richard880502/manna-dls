// The seven-step lesson workspace (docs/DESIGN_OVERHAUL.md step 2, docs/API.md Phase 2.1):
// step navigation, quiet saving, Manna's suggestions (adopt / edit / dismiss), nothing required,
// starting the lesson from any step, and the material viewer.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type {
  AiResult,
  LessonRun,
  RunDetail,
  SopAnalysisSuggestion,
  SopContent,
  SopCurriculumLink,
  SopJob,
  SopMaterial,
  SopObjectivesSuggestion,
  SopState,
} from "../api/types";
import { emptyContent } from "../lib/sop";
import { autosaveConfig } from "../lib/useAutosave";
import { sopConfig } from "../components/sop/useSop";
import {
  baseRoutes,
  config,
  lessonDetail,
  mockApi,
  renderApp,
  respond,
  settleAutosave,
  signIn,
  type MockCall,
} from "../test/mockApi";

const SOP = "/api/lessons/7/sop";
const P1 = "Mrs. Lin put an umbrella stand outside her shop. A sign said, Take one if you need it.";
const P2 = "Her neighbors laughed. They were sure every umbrella would be gone within a week.";
const P3 = "In the first month, twelve umbrellas did disappear. However, something unexpected followed.";

const material: SopMaterial = {
  id: 1,
  version_no: 1,
  title: "The Umbrella Stand",
  source_note: "課本第三課",
  text: `${P1}\n\n${P2}\n\n${P3}`,
  paragraphs: [
    { id: "p1", no: 1, text: P1 },
    { id: "p2", no: 2, text: P2 },
    { id: "p3", no: 3, text: P3 },
  ],
  word_count: 46,
  created_at: "2026-11-14T08:00:00Z",
};

function sopState(overrides: Partial<SopState> = {}): SopState {
  return {
    lesson_id: 7,
    version_no: 1,
    versions_count: 1,
    saved_at: "2026-11-14T08:00:00Z",
    material,
    content: emptyContent(),
    today_objective: "",
    jobs: { analysis: null, objectives: null },
    ...overrides,
  };
}

const blankSop = sopState({ version_no: null, versions_count: 0, saved_at: null, material: null });

const run: LessonRun = {
  id: 9,
  lesson_id: 7,
  lesson_title: "第 1 課：Not only… but also",
  unit_id: 1,
  unit_title: "倒裝句單元",
  started_at: "2026-11-12T01:00:00Z",
  ended_at: null,
  review_completed_at: null,
  good_count: 0,
  stuck_count: 0,
  class_id: null,
  class_name: null,
};
const runDetail: RunDetail = { ...run, events: [], plan: null, remotes: [] };

/** What the server answers to a save: the content it was sent, as a new version. */
const savedSop = (call: MockCall) => ({
  version_no: 2,
  versions_count: 2,
  saved_at: "2026-11-14T09:30:00Z",
  content: (call.body as { content: SopContent }).content,
});

function routes(sop: SopState = sopState(), extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/lessons/7": lessonDetail({ latest_version: null, versions_count: 0 }),
    "GET /api/units/1/core": { id: 1, unit_id: 1, focuses: [] },
    "GET /api/siop": [],
    "GET /api/teacher-cases": [],
    "GET /api/lessons/7/runs": [],
    "POST /api/lessons/7/runs": run,
    "GET /api/runs/9": runDetail,
    [`GET ${SOP}`]: sop,
    [`PUT ${SOP}`]: savedSop,
    "GET /api/materials/1/annotations": [],
    ...extra,
  };
}

async function openLesson() {
  const rendered = renderApp("/lessons/7");
  expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
  return rendered;
}

const rail = () => within(screen.getByRole("navigation", { name: "備課步驟" }));
/** The title of the step on screen. */
const stageTitle = () => document.querySelector(".stage-head .stage-title") as HTMLElement;
const goToStep = async (user: ReturnType<typeof userEvent.setup>, name: RegExp) =>
  user.click(rail().getByRole("button", { name }));
const lastSaved = (api: ReturnType<typeof mockApi>) => (api.lastBody("PUT", SOP) as { content: SopContent }).content;

function job<T>(kind: "analysis" | "objectives", result: AiResult<T> | null, overrides: Partial<SopJob<T>> = {}): SopJob<T> {
  return {
    id: kind === "analysis" ? 5 : 6,
    lesson_id: 7,
    kind,
    status: result ? "done" : "running",
    phase: kind === "analysis" ? "正在讀教材，逐段分析寫作特色" : "正在依教材與班級特性想目標，並對照課綱",
    started_at: new Date().toISOString(),
    finished_at: result ? new Date().toISOString() : null,
    sop_version_no: 1,
    result,
    decided: [],
    ...overrides,
  };
}

function shown<T>(output: T, interaction_id = 91): AiResult<T> {
  return {
    interaction_id,
    status: "shown",
    message: null,
    output,
    sources: [],
    context: [{ label: "教材", preview: "The Umbrella Stand（3 段，46 字）" }],
  };
}

const analysisOutput: SopAnalysisSuggestion = {
  text_type: "記敘文",
  main_idea: "信任會帶來信任",
  structure: "開場 → 鄰居的預測 → 轉折",
  language_focus: [{ kind: "篇章標記", item: "However", paragraph_id: "p3", evidence: "However, something unexpected followed.", note: "轉折" }],
  paragraphs: [
    {
      paragraph_id: "p1",
      paragraph_no: 1,
      function: "以故事開場",
      summary: "林太太在店外放了傘架",
      features: [{ feature: "引用告示", evidence: "A sign said, Take one if you need it.", explanation: "直接交代規則" }],
    },
    {
      paragraph_id: "p2",
      paragraph_no: 2,
      function: "鄰居的預測",
      summary: "鄰居不看好",
      features: [{ feature: "對比的伏筆", evidence: "They were sure every umbrella would be gone within a week.", explanation: "為轉折鋪路" }],
    },
    {
      paragraph_id: "p3",
      paragraph_no: 3,
      function: "轉折",
      summary: "事情和預期不同",
      features: [{ feature: "轉折語", evidence: "However, something unexpected followed.", explanation: "帶出意外" }],
    },
  ],
  missing_paragraph_ids: [],
  dropped_evidence: 0,
};

const link: SopCurriculumLink = {
  item_code: "108-G-3-V-11",
  why: "這個目標要學生推論",
  official_code: "3-Ⅴ-11",
  title: "學習表現｜閱讀",
  text: "能閱讀不同體裁、不同主題的文章。",
  category: "curriculum",
  source_doc: "十二年國教課綱語文領域－英語文",
  source_version: "107 年發布版",
  source_page: "17",
};

const objectivesOutput: SopObjectivesSuggestion = {
  curriculum_available: 147,
  objectives: [
    {
      type: "content",
      text: "學生能比較鄰居的預期和實際發生的事",
      student_facing_en: "I can compare what people expected with what happened.",
      reason: "第 3 段用 However 轉折，前後對比清楚",
      paragraph_ids: ["p3"],
      link_status: "linked",
      links: [link],
    },
    {
      type: "language",
      text: "學生能用 because 和 however 說明預期與結果的差別",
      student_facing_en: "I can use because and however.",
      reason: "內容目標要學生比較，他們需要表達轉折的語言",
      paragraph_ids: [],
      link_status: "unlinked",
      links: [],
    },
  ],
};

describe("lesson workspace: steps", () => {
  it("a lesson opens on step 1; the rail and 下一步 walk the seven steps in any order and end at 產生學習單", async () => {
    signIn();
    mockApi(routes(blankSop));
    const user = userEvent.setup();
    await openLesson();

    // The seven steps are the page: no first view in front of them.
    expect(stageTitle()).toHaveTextContent("閱讀教材");
    expect(rail().getByText("第 1 步，共 7 步")).toBeInTheDocument();
    expect(rail().getByRole("button", { name: /閱讀教材/ })).toHaveAttribute("aria-current", "step");
    expect(rail().getAllByRole("listitem")).toHaveLength(7);
    expect(rail().queryByRole("button", { name: /本節概要/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "今天要教什麼？" })).not.toBeInTheDocument();
    // The rail ends where the steps lead.
    expect(rail().getByRole("button", { name: /產生學習單/ })).toBeInTheDocument();
    // 開始上課 is a secondary button in the head; the step's own action is the one filled button.
    expect(screen.getByRole("button", { name: "開始上課" })).toHaveClass("secondary-button");
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "上一步" })).not.toBeInTheDocument();

    for (const name of ["分析寫作特色", "內容目標與語言目標", "逐段整合目標", "逐段問題或任務", "暖身", "Exit ticket"]) {
      await user.click(screen.getByRole("button", { name: `下一步：${name}` }));
      expect(stageTitle()).toHaveTextContent(name);
      expect(stageTitle()).toHaveFocus();
    }
    expect(rail().getByText("第 7 步，共 7 步")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "先略過這一步" })).not.toBeInTheDocument();
    // The end of the steps: a prominent 產生學習單.
    const make = within(document.querySelector(".stage-nav") as HTMLElement).getByRole("button", { name: "產生學習單" });
    expect(make).toHaveClass("primary-button");

    await user.click(screen.getByRole("button", { name: "上一步" }));
    expect(stageTitle()).toHaveTextContent("暖身");
    // Any step can be opened directly, and skipped.
    await goToStep(user, /內容目標與語言目標/);
    expect(stageTitle()).toHaveTextContent("內容目標與語言目標");
    await user.click(screen.getByRole("button", { name: "先略過這一步" }));
    expect(stageTitle()).toHaveTextContent("逐段整合目標");
  });

  it("shows no second way to prepare: the old suggestions, steps, SIOP and lesson background are gone", async () => {
    signIn();
    const api = mockApi(routes(sopState(), { "GET /api/lessons/7": lessonDetail() }));
    await openLesson();
    for (const gone of [/Manna 備課建議/, /請 Manna 提供建議/, /教學步驟/, /SIOP/, /本課背景/, /單元想法的重點/, /想多準備一點/]) {
      expect(screen.queryByText(gone)).not.toBeInTheDocument();
    }
    expect(api.callsTo("GET", "/api/siop")).toHaveLength(0);
    expect(api.callsTo("POST", "/api/lessons/7/suggestions")).toHaveLength(0);
    // The lesson's earlier 今天要教什麼？ line is still offered where objectives are written.
    const user = userEvent.setup();
    await goToStep(user, /內容目標與語言目標/);
    expect(screen.getByText("學生能說明倒裝的語意功能")).toBeInTheDocument();
  });

  it("產生學習單 at the end of the steps saves what is pending, then opens the new worksheet", async () => {
    signIn();
    const api = mockApi(
      routes(sopState(), {
        "GET /api/worksheets": [{ id: 3, title: "上次的學習單" }],
        "POST /api/worksheets": { id: 12 },
        "GET /api/worksheets/12": respond(404, { error: { code: "not_found", message: "測試不載入" } }),
      }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    await goToStep(user, /Exit ticket/);
    expect(await screen.findByRole("link", { name: "上次的學習單" })).toHaveAttribute("href", "/worksheets/3");
    await user.click(screen.getByRole("button", { name: "＋ 學生反思" }));
    await user.type(screen.getByRole("textbox", { name: "題目" }), "Why?");
    await user.click(within(document.querySelector(".stage-nav") as HTMLElement).getByRole("button", { name: "產生學習單" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/worksheets/12"));
    expect(api.lastBody("POST", "/api/worksheets")).toEqual({ lesson_id: 7 });
    expect(lastSaved(api).exit_ticket.items[0]?.text).toBe("Why?");
    const putAt = api.calls.findIndex((c) => c.method === "PUT" && c.path === SOP);
    const postAt = api.calls.findIndex((c) => c.method === "POST" && c.path === "/api/worksheets");
    expect(putAt).toBeLessThan(postAt);
  });

  it("a lesson with SOP content opens on its steps, shows what each step holds, and remembers the step", async () => {
    signIn();
    const content: SopContent = {
      ...emptyContent(),
      objectives: [
        { id: "o1", type: "content", text: "學生能說出設立傘架的原因", student_facing_en: "", reason: "", paragraph_ids: [], links: [], link_status: "unlinked", origin: "teacher" },
      ],
      exit_ticket: { items: [{ id: "e1", kind: "reflection", text: "What was hard today?", origin: "teacher" }] },
    };
    mockApi(routes(sopState({ content })));
    const user = userEvent.setup();
    const first = await openLesson();

    expect(stageTitle()).toHaveTextContent("閱讀教材");
    expect(rail().getByRole("button", { name: /閱讀教材/ })).toHaveTextContent("3 段 · 46 字");
    expect(rail().getByRole("button", { name: /內容目標與語言目標/ })).toHaveTextContent("1 項目標");
    expect(rail().getByRole("button", { name: /Exit ticket/ })).toHaveTextContent("1 題");
    // Empty steps carry no state and no warning: nothing is required.
    expect(rail().getByRole("button", { name: /暖身/ })).toHaveTextContent(/^6第 6 步：暖身$/);
    // Step 1 shows the material as it will be read: numbered paragraphs.
    expect(screen.getByText(P2)).toBeInTheDocument();
    expect(screen.getByText("共 3 段、46 字。段落編號在左邊。")).toBeInTheDocument();

    await goToStep(user, /Exit ticket/);
    expect(screen.getByRole("textbox", { name: "題目" })).toHaveValue("What was hard today?");
    first.unmount();
    await openLesson();
    expect(stageTitle()).toHaveTextContent("Exit ticket");
  });

  it("開始上課 is on every step and never looks at the SOP", async () => {
    signIn();
    const api = mockApi(routes(blankSop));
    const user = userEvent.setup();
    const { router } = await openLesson();
    for (const name of ["分析寫作特色", "內容目標與語言目標", "逐段整合目標", "逐段問題或任務"]) {
      expect(screen.getByRole("button", { name: "開始上課" })).toBeEnabled();
      await user.click(screen.getByRole("button", { name: `下一步：${name}` }));
    }
    // Step 5 of an empty SOP: start anyway.
    await user.click(screen.getByRole("button", { name: "開始上課" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(1);
    expect(api.callsTo("PUT", SOP)).toHaveLength(0);
  });

  it("開始上課 still works when the SOP cannot be loaded", async () => {
    signIn();
    const api = mockApi(routes(blankSop, { [`GET ${SOP}`]: respond(500, { error: { code: "internal", message: "伺服器暫時發生問題" } }) }));
    const user = userEvent.setup();
    const { router } = await openLesson();
    await goToStep(user, /分析寫作特色/);
    expect(await screen.findByText("伺服器暫時發生問題")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再試一次" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "開始上課" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(1);
  });
});

describe("lesson workspace: quiet saving", () => {
  it("saves a step a moment after the last edit, with no save button and nothing required", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /Exit ticket/);
    expect(screen.getByText("還沒有題目。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /儲存/ })).not.toBeInTheDocument();

    autosaveConfig.delayMs = 20;
    await user.click(screen.getByRole("button", { name: "＋ 學生反思" }));
    await user.type(screen.getByRole("textbox", { name: "題目" }), "What was hard today?");
    expect(await screen.findByText(/^已自動儲存 \d\d:\d\d$/)).toBeInTheDocument();
    await waitFor(() =>
      expect(lastSaved(api).exit_ticket.items).toEqual([
        { id: expect.any(String), kind: "reflection", text: "What was hard today?", origin: "teacher" },
      ]),
    );
    // Only the exit ticket was written; every other part of the document is still empty.
    expect(lastSaved(api).objectives).toEqual([]);
    expect(lastSaved(api).task_blocks).toEqual([]);
    expect(rail().getByRole("button", { name: /Exit ticket/ })).toHaveTextContent("1 題");
  });

  it("pasted material is split by the server; leaving the field saves it", async () => {
    signIn();
    const api = mockApi(
      routes(blankSop, {
        [`PUT ${SOP}/material`]: (call: MockCall) =>
          sopState({ material: { ...material, title: (call.body as { title: string }).title } }),
      }),
    );
    const user = userEvent.setup();
    await openLesson();

    await user.type(await screen.findByRole("textbox", { name: "教材標題" }), "The Umbrella Stand");
    await user.click(screen.getByRole("textbox", { name: "教材文字" }));
    await user.paste(material.text);
    expect(screen.getByText("尚未儲存")).toBeInTheDocument();
    await user.click(screen.getByRole("heading", { level: 1 }));
    expect(await screen.findByText(/^已自動儲存/)).toBeInTheDocument();
    expect(api.lastBody("PUT", `${SOP}/material`)).toEqual({ title: "The Umbrella Stand", source_note: "", text: material.text });
    // The teacher keeps typing in the same fields; the step says how the text was split.
    expect(screen.getByRole("textbox", { name: "教材文字" })).toHaveValue(material.text);
    expect(screen.getByText("目前分成 3 段。")).toBeInTheDocument();
    expect(rail().getByRole("button", { name: /閱讀教材/ })).toHaveTextContent("3 段 · 46 字");

    await user.click(screen.getByRole("button", { name: "改好了，看分段結果" }));
    expect(screen.getByText(P3)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "修改教材" })).toBeInTheDocument();
  });

  it("step 1 can pick a material from the library; the lesson then uses it", async () => {
    signIn();
    const libraryMaterial = {
      ...material,
      id: 12,
      title: "Handout from the library",
      kind: "document" as const,
      origin: "upload" as const,
      has_file: true,
      text_status: "ready" as const,
    };
    const movedContent: SopContent = {
      ...emptyContent(),
      analysis: {
        ...emptyContent().analysis,
        paragraphs: [{ paragraph_id: "m1-p1", function: "舊教材的開場", summary: "", features: [], origin: "teacher" }],
      },
    };
    const api = mockApi(
      routes(sopState(), {
        "GET /api/materials": {
          materials: [
            { id: 1, kind: "text", origin: "paste", title: "The Umbrella Stand", source_note: "", url: null, filename: null, content_type: null, size_bytes: 10, sha256: null, has_file: false, text_status: "ready", text_truncated: false, page_count: null, paragraph_count: 3, word_count: 46, revision: 1, created_at: "", updated_at: "", lessons: [], worksheet_count: 0, annotation_count: 0 },
            { id: 12, kind: "document", origin: "upload", title: "Handout from the library", source_note: "", url: null, filename: "h.pdf", content_type: "application/pdf", size_bytes: 2048, sha256: null, has_file: true, text_status: "ready", text_truncated: false, page_count: 1, paragraph_count: 3, word_count: 46, revision: 1, created_at: "", updated_at: "", lessons: [], worksheet_count: 0, annotation_count: 0 },
          ],
          usage: { used_bytes: 2048, quota_bytes: 1073741824, count: 2, max_count: 300 },
          limits: { files: [], max_pasted_chars: 50000, max_note_chars: 2000 },
          storage: "fs",
          uploads_enabled: true,
        },
        [`PUT ${SOP}/material`]: sopState({ version_no: 2, material: libraryMaterial, content: movedContent }),
        "GET /api/materials/12/annotations": [],
        "POST /api/materials/12/file-link": { url: "/api/materials/12/file?t=1.1.a", expires_at: "2026-11-15T10:00:00Z" },
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    expect(stageTitle()).toHaveTextContent("閱讀教材");
    await user.click(screen.getByRole("button", { name: "換一份教材" }));
    await user.click(screen.getByRole("button", { name: "從教材庫選" }));
    const picker = within(await screen.findByRole("list", { name: "教材庫" }));
    expect(picker.getByText("目前使用中")).toBeInTheDocument();
    await user.click(picker.getByRole("button", { name: "選用「Handout from the library」" }));
    await waitFor(() => expect(api.lastBody("PUT", `${SOP}/material`)).toEqual({ material_id: 12 }));
    await waitFor(() =>
      expect(document.querySelector(".material-sheet .material-title")).toHaveTextContent("Handout from the library"),
    );
    expect(screen.getByRole("link", { name: "下載原檔" })).toBeInTheDocument();
    // Picking is one request: the SOP content was not saved again from the page.
    expect(api.callsTo("PUT", SOP)).toHaveLength(0);
  });

  it("a lesson whose material was deleted says so in step 1 and in 對照教材", async () => {
    signIn();
    mockApi(routes(sopState({ material: null, material_deleted: true })));
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /閱讀教材/);
    expect(screen.getByText("教材已刪除。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "從教材庫選" })).toBeInTheDocument();
    await goToStep(user, /Exit ticket/);
    await user.click(screen.getByRole("button", { name: "對照教材" }));
    const viewer = within(screen.getByRole("complementary", { name: "教材" }));
    expect(viewer.getByRole("button", { name: "到第 1 步放進新的教材" })).toBeInTheDocument();
  });

  it("a failed save stays on screen and holds the page until it is saved", async () => {
    signIn();
    let serverUp = false;
    const api = mockApi(
      routes(sopState(), {
        [`PUT ${SOP}`]: (call: MockCall) =>
          serverUp ? savedSop(call) : respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } }),
      }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    await goToStep(user, /暖身/);
    await user.click(screen.getByRole("button", { name: "＋ 新增關鍵字彙" }));
    await user.type(screen.getByRole("textbox", { name: "字彙 1" }), "trust");
    await user.click(screen.getByRole("heading", { level: 1 }));
    expect(await screen.findByText(/^沒有存到：伺服器暫時發生問題/)).toBeInTheDocument();

    // Leaving now would lose it: the page asks.
    await user.click(within(screen.getByRole("navigation", { name: "主要導覽" })).getByRole("link", { name: "班級與課例" }));
    const dialog = within(await screen.findByRole("alertdialog", { name: "還有沒儲存的內容" }));
    serverUp = true;
    await user.click(dialog.getByRole("button", { name: "儲存後離開" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/classes"));
    expect(lastSaved(api).warmup.vocabulary[0]?.word).toBe("trust");
  });
});

describe("lesson workspace: Manna's suggestions", () => {
  function analysisRoutes(sop: SopState = sopState()) {
    let polls = 0;
    return routes(sop, {
      [`POST ${SOP}/analysis`]: job<SopAnalysisSuggestion>("analysis", null),
      "GET /api/sop-jobs/5": () => {
        polls += 1;
        return polls < 2 ? job<SopAnalysisSuggestion>("analysis", null) : job("analysis", shown(analysisOutput));
      },
      "POST /api/ai/91/decision": { ok: true },
    });
  }

  it("shows progress while Manna works, then suggestions under their paragraphs: adopt, edit, dismiss", async () => {
    signIn();
    const teacherWrote: SopContent = {
      ...emptyContent(),
      analysis: {
        ...emptyContent().analysis,
        main_idea: "我自己寫的主旨",
        paragraphs: [{ paragraph_id: "p1", function: "我寫的功能", summary: "", features: [], origin: "teacher" }],
      },
    };
    const api = mockApi(analysisRoutes(sopState({ content: teacherWrote })));
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /分析寫作特色/);

    sopConfig.pollMs = 120;
    await user.click(screen.getByRole("button", { name: "請 Manna 逐段分析" }));
    // A long call: the teacher is told what is happening and that she can go on.
    const progress = within((await screen.findByText("Manna 正在處理")).closest(".manna-well") as HTMLElement);
    expect(progress.getByText("正在讀教材，逐段分析寫作特色…")).toBeInTheDocument();
    expect(progress.getByText(/通常要 15 到 50 秒/)).toBeInTheDocument();
    expect(api.lastBody("POST", `${SOP}/analysis`)).toEqual({});

    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    const well1 = within(await p1.findByRole("group", { name: "Manna 建議 · 第 1 段" }));
    expect(screen.queryByText("Manna 正在處理")).not.toBeInTheDocument();
    expect(well1.getByText("A sign said, Take one if you need it.")).toBeInTheDocument();
    expect(screen.getByText("Manna 分析了 3 段，建議在各段下方。")).toBeInTheDocument();
    // Nothing was written into the lesson by the result itself.
    expect(api.callsTo("PUT", SOP)).toHaveLength(0);

    // 採用: the teacher's own words stay, the suggestion fills in the rest.
    await user.click(well1.getByRole("button", { name: "採用" }));
    expect(p1.queryByRole("group", { name: "Manna 建議 · 第 1 段" })).not.toBeInTheDocument();
    expect(p1.getByText("我寫的功能")).toBeInTheDocument();
    expect(p1.getByText("林太太在店外放了傘架")).toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("POST", "/api/ai/91/decision")).toHaveLength(1));
    expect(api.lastBody("POST", "/api/ai/91/decision")).toEqual({ item_index: 1, decision: "adopt" });

    // 修改後採用: the suggestion opens as a form; it is written in only when the teacher is done.
    const p2 = within(screen.getByRole("region", { name: "第 2 段" }));
    await user.click(within(p2.getByRole("group", { name: "Manna 建議 · 第 2 段" })).getByRole("button", { name: "修改後採用" }));
    const fn = p2.getByRole("textbox", { name: "這一段在全文中的功能" });
    expect(fn).toHaveValue("鄰居的預測");
    await user.clear(fn);
    await user.type(fn, "鄰居不看好，埋下對比");
    expect(api.callsTo("POST", "/api/ai/91/decision")).toHaveLength(1);
    await user.click(p2.getByRole("button", { name: "完成" }));
    await waitFor(() => expect(api.callsTo("POST", "/api/ai/91/decision")).toHaveLength(2));
    expect(api.lastBody("POST", "/api/ai/91/decision")).toEqual({
      item_index: 2,
      decision: "revise",
      revised_content: "鄰居不看好，埋下對比｜鄰居不看好",
    });

    // 不用: gone from the page, nothing written.
    const p3 = within(screen.getByRole("region", { name: "第 3 段" }));
    await user.click(within(p3.getByRole("group", { name: "Manna 建議 · 第 3 段" })).getByRole("button", { name: "不用" }));
    expect(p3.queryByRole("group")).not.toBeInTheDocument();
    expect(p3.getByText(/這一段還沒有分析。/)).toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("POST", "/api/ai/91/decision")).toHaveLength(3));
    expect(api.lastBody("POST", "/api/ai/91/decision")).toEqual({ item_index: 3, decision: "reject" });

    // The whole-text suggestion only fills what the teacher left empty.
    await user.click(within(screen.getByRole("group", { name: "Manna 建議 · 全文" })).getByRole("button", { name: "採用" }));
    expect(screen.getByRole("textbox", { name: "主旨" })).toHaveValue("我自己寫的主旨");
    expect(screen.getByRole("textbox", { name: "結構" })).toHaveValue("開場 → 鄰居的預測 → 轉折");

    await settleAutosave();
    const saved = lastSaved(api).analysis;
    expect(saved.main_idea).toBe("我自己寫的主旨");
    expect(saved.overall_origin).toBe("ai_edited");
    expect(saved.language_focus.map((f) => f.item)).toEqual(["However"]);
    const byId = new Map(saved.paragraphs.map((p) => [p.paragraph_id, p]));
    expect(byId.get("p1")).toMatchObject({ function: "我寫的功能", summary: "林太太在店外放了傘架", origin: "ai_edited" });
    expect(byId.get("p1")?.features.map((f) => f.evidence)).toEqual(["A sign said, Take one if you need it."]);
    expect(byId.get("p2")).toMatchObject({ function: "鄰居不看好，埋下對比", origin: "ai_edited" });
    expect(byId.has("p3")).toBe(false);
  });

  it("an adopted suggestion on an empty paragraph is marked as Manna's; decided ones do not come back after a reload", async () => {
    signIn();
    const done = job("analysis", shown(analysisOutput), { decided: [0, 2, 3] });
    const api = mockApi(routes(sopState({ jobs: { analysis: done, objectives: null } }), { "POST /api/ai/91/decision": { ok: true } }));
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /分析寫作特色/);
    expect(screen.getAllByRole("group", { name: /^Manna 建議/ })).toHaveLength(1);
    await user.click(within(screen.getByRole("group", { name: "Manna 建議 · 第 1 段" })).getByRole("button", { name: "採用" }));
    await settleAutosave();
    expect(lastSaved(api).analysis.paragraphs).toEqual([
      {
        paragraph_id: "p1",
        function: "以故事開場",
        summary: "林太太在店外放了傘架",
        features: [{ id: expect.any(String), feature: "引用告示", evidence: "A sign said, Take one if you need it.", explanation: "直接交代規則" }],
        origin: "ai_adopted",
      },
    ]);
  });

  it("objectives: type, reason, curriculum item with official text and source; unlinked ones are marked; the teacher's own stay", async () => {
    signIn();
    const mine: SopContent = {
      ...emptyContent(),
      objectives: [
        { id: "mine", type: "language", text: "學生能用 Not only … but also 倒裝句", student_facing_en: "", reason: "", paragraph_ids: [], links: [], link_status: "unlinked", origin: "teacher" },
      ],
    };
    const api = mockApi(
      routes(sopState({ content: mine, jobs: { analysis: null, objectives: job("objectives", shown(objectivesOutput, 92)) } }), {
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /內容目標與語言目標/);

    const content = within(screen.getByRole("region", { name: /^內容目標/ }));
    const language = within(screen.getByRole("region", { name: /^語言目標/ }));
    const linked = within(content.getByRole("group", { name: "Manna 建議 · 內容目標" }));
    expect(linked.getByText("學生能比較鄰居的預期和實際發生的事")).toBeInTheDocument();
    expect(linked.getByText("為什麼建議：第 3 段用 However 轉折，前後對比清楚")).toBeInTheDocument();
    // The code shown is the one printed in the curriculum, not the knowledge base's own.
    expect(linked.getByText("3-Ⅴ-11")).toBeInTheDocument();
    expect(linked.getByText("能閱讀不同體裁、不同主題的文章。")).toBeInTheDocument();
    expect(linked.getByText("來源：十二年國教課綱語文領域－英語文，107 年發布版，第 17 頁")).toBeInTheDocument();
    const unlinked = within(language.getByRole("group", { name: "Manna 建議 · 語言目標" }));
    expect(unlinked.getByText("尚未完成課綱連結")).toBeInTheDocument();
    expect(language.getByText("Manna 另外想到 1 項")).toBeInTheDocument();
    // The teacher's objective is listed first and untouched.
    expect(language.getByText("學生能用 Not only … but also 倒裝句")).toBeInTheDocument();

    await user.click(linked.getByRole("button", { name: "採用" }));
    expect(content.queryByRole("group", { name: "Manna 建議 · 內容目標" })).not.toBeInTheDocument();
    expect(content.getByText("已連結 108 課綱")).toBeInTheDocument();
    await user.click(content.getByRole("button", { name: "看原文" }));
    expect(content.getByText("能閱讀不同體裁、不同主題的文章。")).toBeInTheDocument();

    // 修改後採用 turns the suggestion into a form first.
    await user.click(unlinked.getByRole("button", { name: "修改後採用" }));
    const text = screen.getByRole("textbox", { name: "目標" });
    await user.clear(text);
    await user.type(text, "學生能用 however 說出轉折");
    await user.click(screen.getByRole("button", { name: "採用修改後的目標" }));
    expect(language.getByText("學生能用 however 說出轉折")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: /^Manna 建議/ })).not.toBeInTheDocument();

    await settleAutosave();
    const decisions = api.callsTo("POST", "/api/ai/92/decision").map((c) => c.body);
    expect(decisions).toEqual([
      { item_index: 0, decision: "adopt" },
      { item_index: 1, decision: "revise", revised_content: "學生能用 however 說出轉折" },
    ]);
    const saved = lastSaved(api).objectives;
    expect(saved.map((o) => [o.type, o.text, o.origin, o.link_status])).toEqual([
      ["language", "學生能用 Not only … but also 倒裝句", "teacher", "unlinked"],
      ["content", "學生能比較鄰居的預期和實際發生的事", "ai_adopted", "linked"],
      ["language", "學生能用 however 說出轉折", "ai_edited", "unlinked"],
    ]);
    expect(saved[1]?.links[0]?.item_code).toBe("108-G-3-V-11");
  });

  it("objectives by hand: add, carry over 今天要教什麼, link a curriculum item from the knowledge base, delete", async () => {
    signIn();
    const api = mockApi(
      routes(sopState(), {
        "GET /api/lessons/7": lessonDetail({ versions_count: 1 }),
        "GET /api/knowledge/curriculum": {
          track: "general",
          filter: { kind: null, stage: "V", subject: ["英語文"] },
          ranked: true,
          items: [
            {
              score: 2,
              title_score: 1,
              item: {
                item_code: link.item_code,
                version: 1,
                category: "curriculum",
                school_track: "general",
                title: link.title,
                content: link.text,
                official_code: link.official_code,
                item_kind: "learning_performance",
                strand_name: "閱讀",
                stage: "V",
                subject: "英語文",
                advanced: false,
                recurring: false,
                source_doc: link.source_doc,
                source_version: link.source_version,
                source_page: link.source_page,
              },
            },
          ],
        },
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /內容目標與語言目標/);

    // The old single objective is offered, not filed for the teacher.
    const carry = within(screen.getByRole("group", { name: "今天要教什麼裡寫的目標" }));
    expect(carry.getByText("學生能說明倒裝的語意功能")).toBeInTheDocument();
    await user.click(carry.getByRole("button", { name: "放進語言目標" }));
    expect(screen.queryByRole("group", { name: "今天要教什麼裡寫的目標" })).not.toBeInTheDocument();
    const language = within(screen.getByRole("region", { name: /^語言目標/ }));
    expect(language.getByText("學生能說明倒裝的語意功能")).toBeInTheDocument();
    expect(language.getByText("尚未完成課綱連結")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "自己新增一項目標" }));
    await user.type(screen.getByRole("textbox", { name: "目標" }), "學生能說出設立傘架的原因");
    await user.click(screen.getByRole("button", { name: "加入目標" }));
    const content = within(screen.getByRole("region", { name: /^內容目標/ }));
    expect(content.getByText("學生能說出設立傘架的原因")).toBeInTheDocument();

    await user.click(content.getByRole("button", { name: "為「學生能說出設立傘架的原因」連結課綱條目" }));
    await user.type(screen.getByRole("searchbox", { name: "找課綱條目" }), "閱讀");
    await user.click(screen.getByRole("button", { name: "搜尋" }));
    await user.click(await screen.findByRole("button", { name: "連結 3-Ⅴ-11" }));
    expect(api.callsTo("GET", "/api/knowledge/curriculum")[0]?.url).toContain("q=%E9%96%B1%E8%AE%80");
    expect(content.getByText("已連結 108 課綱")).toBeInTheDocument();

    await user.click(language.getByRole("button", { name: "刪除「學生能說明倒裝的語意功能」" }));
    await settleAutosave();
    const saved = lastSaved(api).objectives;
    expect(saved).toHaveLength(1);
    expect(saved[0]).toMatchObject({ type: "content", origin: "teacher", link_status: "linked" });
    expect(saved[0]?.links.map((l) => l.item_code)).toEqual(["108-G-3-V-11"]);
  });

  it("when Manna has nothing to show the step says so and stays usable by hand; AI off disables the button", async () => {
    signIn();
    const blocked: AiResult<SopAnalysisSuggestion> = {
      interaction_id: 93,
      status: "blocked",
      message: "目前暫無建議，你可以繼續手動完成。",
      output: null,
      sources: [],
      context: [],
    };
    mockApi(routes(sopState({ jobs: { analysis: job("analysis", blocked), objectives: null } })));
    const user = userEvent.setup();
    const first = await openLesson();
    await goToStep(user, /分析寫作特色/);
    expect(screen.getByText("目前暫無建議，你可以繼續手動完成。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "請 Manna 再分析一次" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "自己寫第 1 段的分析" }));
    expect(screen.getByRole("textbox", { name: "這一段在全文中的功能" })).toBeInTheDocument();
    first.unmount();

    mockApi(routes(sopState(), { "GET /api/config": config({ ai_enabled: false }) }));
    await openLesson();
    await goToStep(user, /內容目標與語言目標/);
    const ask = screen.getByRole("button", { name: "請 Manna 推薦目標" });
    expect(ask).toBeDisabled();
    expect(
      within(ask.closest(".ask-manna") as HTMLElement).getByText("AI 功能目前未開放，你可以繼續手動完成所有步驟。"),
    ).toBeInTheDocument();
  });

  it("asking Manna saves what is pending first, and says why when it cannot start", async () => {
    signIn();
    const api = mockApi(
      routes(sopState(), {
        [`POST ${SOP}/objectives`]: respond(400, { error: { code: "bad_request", message: "請先在第 1 步貼上教材，Manna 才有東西可以讀。" } }),
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /內容目標與語言目標/);
    await user.click(screen.getByRole("button", { name: "自己新增一項目標" }));
    await user.type(screen.getByRole("textbox", { name: "目標" }), "學生能說出原因");
    await user.click(screen.getByRole("button", { name: "加入目標" }));
    await user.click(screen.getByRole("button", { name: "請 Manna 推薦目標" }));
    expect(await screen.findByText("請先在第 1 步貼上教材，Manna 才有東西可以讀。")).toBeInTheDocument();
    const order = api.calls.map((c) => `${c.method} ${c.path}`);
    expect(order.indexOf(`PUT ${SOP}`)).toBeGreaterThan(-1);
    expect(order.indexOf(`PUT ${SOP}`)).toBeLessThan(order.indexOf(`POST ${SOP}/objectives`));
  });
});

describe("lesson workspace: tasks and the material viewer", () => {
  it("a task has an information function and three scaffolds; neighbouring paragraphs can be merged", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    await goToStep(user, /逐段問題或任務/);

    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    expect(p1.getByText(/這一段還沒有問題或任務。/)).toBeInTheDocument();
    await user.click(p1.getByRole("button", { name: "自己寫第 1 段的問題或任務" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "這一題要學生做什麼" }), "discover");
    await user.type(screen.getByRole("textbox", { name: "問題或任務" }), "Why did Mrs. Lin start the stand?");
    await user.type(screen.getByRole("textbox", { name: "語言鷹架" }), "Mrs. Lin started the stand because ____.");
    await user.type(screen.getByRole("textbox", { name: "程序鷹架" }), "自己讀 2 分鐘，再和旁邊同學對答案");
    await user.click(screen.getByRole("button", { name: "完成" }));
    expect(screen.getByText("發現資訊")).toBeInTheDocument();
    expect(screen.getByText("Why did Mrs. Lin start the stand?")).toBeInTheDocument();
    expect(screen.getByText("自己讀 2 分鐘，再和旁邊同學對答案")).toBeInTheDocument();

    // Paragraph 2 joins paragraph 1's block.
    await user.click(within(screen.getByRole("region", { name: "第 2 段" })).getByRole("button", { name: "和第 1 段合併" }));
    const merged = within(screen.getByRole("region", { name: "第 1、2 段" }));
    expect(merged.getByText("第 1、2 段合併設計")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "第 2 段" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "第 3 段" })).toBeInTheDocument();

    await settleAutosave();
    expect(lastSaved(api).task_blocks).toEqual([
      {
        id: expect.any(String),
        paragraph_ids: ["p1", "p2"],
        tasks: [
          {
            id: expect.any(String),
            function: "discover",
            text: "Why did Mrs. Lin start the stand?",
            scaffolds: { verbal: "Mrs. Lin started the stand because ____.", instructional: "", procedural: "自己讀 2 分鐘，再和旁邊同學對答案" },
            origin: "teacher",
          },
        ],
      },
    ]);
  });

  it("對照教材 slides the material in beside steps 2 to 7 and 收起教材 puts it away", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    // Step 1 is the material itself: no viewer there.
    expect(screen.queryByRole("button", { name: "對照教材" })).not.toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "教材" })).not.toBeInTheDocument();

    for (const name of [/分析寫作特色/, /內容目標與語言目標/, /逐段整合目標/, /逐段問題或任務/, /暖身/, /Exit ticket/]) {
      await goToStep(user, name);
      expect(screen.getByRole("button", { name: "對照教材" })).toHaveAttribute("aria-expanded", "false");
    }
    await user.click(screen.getByRole("button", { name: "對照教材" }));
    const viewer = within(screen.getByRole("complementary", { name: "教材" }));
    expect(viewer.getByText("The Umbrella Stand")).toBeInTheDocument();
    expect(viewer.getByText(P3)).toBeInTheDocument();
    expect(viewer.getByText("來源：課本第三課")).toBeInTheDocument();
    expect(viewer.getByRole("button", { name: "收起教材" })).toHaveFocus();
    // The step is still there to work on.
    expect(stageTitle()).toHaveTextContent("Exit ticket");
    expect(screen.getByRole("button", { name: "＋ 形成性評量" })).toBeInTheDocument();

    // It stays open from step to step, and closes on request or with Escape.
    await goToStep(user, /暖身/);
    expect(screen.getByRole("complementary", { name: "教材" })).toBeInTheDocument();
    await user.click(within(screen.getByRole("complementary", { name: "教材" })).getByRole("button", { name: "收起教材" }));
    expect(screen.queryByRole("complementary", { name: "教材" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "對照教材" }));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("complementary", { name: "教材" })).not.toBeInTheDocument();
  });
});
