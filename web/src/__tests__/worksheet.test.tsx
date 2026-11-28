// 學習單產生器 (docs/API.md Phase 2.3; spec §4.1, §4.2, §4.6, §5, §9, §10): the four option
// blocks with the spec's exact labels, both entries, the result editor (versions, student or
// teacher view, regenerate and undo, reorder, toggles), sync-back only on confirmation, export,
// and the vocabulary-level check.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SaveWorksheetRequest,
  SopSyncPreview,
  VocabularyAnalysis,
  WorksheetContent,
  WorksheetState,
  WsBlock,
  WsItem,
  WsJob,
  WsScaffold,
  WsVocabCheck,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { newOrganizer } from "../lib/organizer";
import { emptyContent } from "../lib/sop";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings, emptyWorksheetContent, projectSheet } from "../lib/worksheet";
import { baseRoutes, lessonDetail, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";
import { clickBlockMore, openBarPopover, openBlockMore } from "../test/menus";

const WS = "/api/worksheets/5";
const P1 = "Mrs. Lin put an umbrella stand outside her shop. A sign said, Take one if you need it.";
const P2 = "Her neighbors laughed. They were sure every umbrella would be gone within a week.";

const item = (id: string, prompt: string, extra: Partial<WsItem> = {}): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: `Answer to ${id}`,
  evidence: "",
  purpose: null,
  levels: ["basic", "standard"],
  origin: "ai",
  ...extra,
});

const scaffold = (id: string, extra: Partial<WsScaffold>): WsScaffold => ({
  id,
  type: "verbal",
  kind: "句型框架",
  content: "",
  steps: [],
  levels: ["basic", "standard"],
  auto: false,
  origin: "ai",
  ...extra,
});

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

function generatedContent(): WorksheetContent {
  return {
    objectives: [
      { id: "O1", type: "content", text: "學生能說明原因", student_facing_en: "I can explain why Mrs. Lin started the stand.", origin: "ai", sop_id: "O1" },
    ],
    show_objectives: true,
    levels: ["basic", "standard"],
    chosen: {
      settings: { ...emptySettings(), material_types: ["閱讀文本"], levels: ["basic", "standard"] },
      auto: ["material_types", "levels"],
      item_budget: 6,
      reasons: [],
    },
    notices: [],
    blocks: [
      block("warmup", {
        kind: "warmup",
        title: "Warm-up",
        vocabulary: [
          { word: "neighbor", level: 2, definition: "a person who lives near you", context: "", paragraph_id: "p2" },
          { word: "umbrella", level: null, definition: "a thing for the rain", context: "", paragraph_id: "p1" },
        ],
        items: [item("w1", "Have you ever borrowed something from a stranger?")],
        scaffolds: [
          scaffold("ws1", {
            type: "procedural",
            kind: "清楚的分步指示及完成順序",
            steps: ["Think alone for one minute.", "Tell your partner."],
            auto: true,
            levels: ["basic", "standard"],
          }),
        ],
      }),
      block("b1", {
        title: "Paragraph 1: Find",
        info_function: "discover",
        paragraph_ids: ["p1"],
        teacher_notes: "先確認大意。",
        items: [
          item("b1-1", "Why did Mrs. Lin start the umbrella stand?", { answer: "A stranger once helped her.", evidence: "A sign said" }),
          item("b1-2", "Circle the word that means the same as shop.", { levels: ["basic"], answer: "store" }),
        ],
        scaffolds: [
          scaffold("b1s1", { kind: "句型框架", content: "Mrs. Lin started the stand because ___.", levels: ["basic"] }),
        ],
      }),
      block("b2", {
        title: "Paragraph 2: Compare",
        info_function: "compare",
        paragraph_ids: ["p2"],
        items: [item("b2-1", "What did the neighbors expect?", { answer: "That every umbrella would be gone." })],
      }),
      block("exit", {
        kind: "exit_ticket",
        title: "Exit ticket",
        items: [item("e1", "One question I still have:", { purpose: "questions", answer: "" })],
      }),
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
    created_at: "2026-11-15T08:00:00Z",
    updated_at: "2026-11-15T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [
        { id: "p1", text: P1 },
        { id: "p2", text: P2 },
      ],
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
    ...overrides,
  };
}

const generated = (overrides: Partial<WorksheetState> = {}) =>
  wsState({ generated: true, levels: ["basic", "standard"], content: generatedContent(), version_no: 2, ...overrides });

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

/** The server's answer to a save: what it was sent, on top of `base`. */
const saved = (base: WorksheetState) => (call: MockCall) => {
  const body = call.body as SaveWorksheetRequest;
  return { ...base, ...body, version_no: base.version_no + 1, saved_at: "2026-11-15T09:30:00Z" };
};

function routes(state: WorksheetState, extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/worksheets": [],
    [`GET ${WS}`]: state,
    [`PUT ${WS}`]: saved(state),
    ...extra,
  };
}

const FORBIDDEN = ["課綱議題", "SDGs", "SEL", "探究", "高層次思考", "專題任務", "合作活動"];
const MATERIAL_TYPES = ["閱讀文本", "聽力素材", "影片素材", "圖像素材", "資料／數據"];
const TASK_GROUPS: Record<string, string[]> = {
  語言與內容學習: ["理解內容", "字彙練習", "文法／句型練習"],
  思考與資訊處理: ["比較／分類", "推論／預測", "評估／提出理由", "蒐集資料", "整理／分析資料"],
  互動與合作: ["討論／分享", "小組共同完成"],
  表達與成果: ["寫作", "製作成果", "成果發表"],
  反思與評量: ["學習反思", "自我評量", "同儕互評"],
};
const ANSWER_FORMATS = ["填空題", "選擇題", "是非題", "配對題", "簡答題", "表格整理", "圖像組織", "段落寫作"];
const GENERATION = ["加入暖身活動", "顯示學習目標", "提供引導問題", "顯示作答示例", "提供字詞表", "提供句型框架"];

const optionBlock = (name: string) => within(screen.getByRole("region", { name }));
const blockRegion = (name: string) => within(screen.getByRole("region", { name: `區塊 ${name}` }));

async function settle() {
  window.dispatchEvent(new Event("blur"));
  await screen.findByText(/^已自動儲存 \d\d:\d\d/);
}

beforeEach(() => {
  signIn();
  autosaveConfig.delayMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  autosaveConfig.delayMs = 1500;
  worksheetConfig.pollMs = 2000;
});

describe("options: the four blocks of spec §5", () => {
  it("shows exactly the four blocks with every label verbatim, the five 學生任務 sub-groups, and none of the forbidden labels", async () => {
    mockApi(routes(wsState()));
    renderApp("/worksheets/5");
    expect(await screen.findByRole("heading", { name: "做一份學習單", level: 1 })).toBeInTheDocument();

    const names = (scope: { getAllByRole: (role: "checkbox") => HTMLElement[] }) =>
      scope.getAllByRole("checkbox").map((c: HTMLElement) => (c as HTMLInputElement).labels?.[0]?.textContent);

    expect(names(optionBlock("素材類型"))).toEqual(MATERIAL_TYPES);
    expect(names(optionBlock("作答方式"))).toEqual(ANSWER_FORMATS);
    const tasks = optionBlock("學生任務");
    expect(tasks.getAllByRole("group").map((g) => g.getAttribute("aria-label"))).toEqual(Object.keys(TASK_GROUPS));
    for (const [group, labels] of Object.entries(TASK_GROUPS)) {
      expect(names(within(tasks.getByRole("group", { name: group })))).toEqual(labels);
    }
    const generation = optionBlock("產生設定");
    expect(names(generation)).toEqual([...GENERATION, "基礎版", "一般版", "進階版"]);
    expect(names(within(generation.getByRole("group", { name: "產生基礎版／一般版／進階版" })))).toEqual([
      "基礎版",
      "一般版",
      "進階版",
    ]);

    // Four option blocks, no fifth.
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings.filter((h) => ["素材類型", "學生任務", "作答方式", "產生設定"].includes(h ?? ""))).toHaveLength(4);
    const page = document.body.textContent ?? "";
    for (const label of FORBIDDEN) expect(page).not.toContain(label);
  });

  it("is multi-select and fully optional: nothing selected still generates, and selections are saved", async () => {
    const running = wsState({ running: true, jobs: [job(1, "tasks", { status: "running", phase: "正在設計題目", started_at: new Date().toISOString() }), job(2, "warmup"), job(3, "scaffolds"), job(4, "exit_ticket")] });
    const api = mockApi(
      routes(wsState(), {
        [`POST ${WS}/generate`]: running,
        [`GET ${WS}/jobs`]: { running: true, jobs: running.jobs, version_no: 1 },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const generate = await screen.findByRole("button", { name: "產生學習單" });
    expect(generate).toBeEnabled();
    expect(screen.getAllByRole("checkbox").every((c) => !(c as HTMLInputElement).checked)).toBe(true);

    // Several in one block, across sub-groups.
    await user.click(screen.getByRole("checkbox", { name: "理解內容" }));
    await user.click(screen.getByRole("checkbox", { name: "寫作" }));
    await user.click(screen.getByRole("checkbox", { name: "簡答題" }));
    await user.click(screen.getByRole("checkbox", { name: "基礎版" }));
    await user.click(screen.getByRole("checkbox", { name: "進階版" }));
    await user.type(screen.getByRole("textbox", { name: /預計完成時間（分鐘）/ }), "25");
    await settle();
    const body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.settings).toMatchObject({
      student_tasks: ["理解內容", "寫作"],
      answer_formats: ["簡答題"],
      levels: ["basic", "advanced"],
      minutes: 25,
    });

    // Take everything off again: generating is still possible.
    for (const name of ["理解內容", "寫作", "簡答題", "基礎版", "進階版"]) {
      await user.click(screen.getByRole("checkbox", { name }));
    }
    await user.clear(screen.getByRole("textbox", { name: /預計完成時間（分鐘）/ }));
    await user.click(screen.getByRole("button", { name: "產生學習單" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/generate`)).toHaveLength(1));
    const last = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(last.settings).toEqual(emptySettings());

    // While Manna works the four steps are listed with their state.
    const panel = await screen.findByRole("status", { name: "產生進度" });
    // 進度 sits at the top of the settings column, before the first settings section.
    const firstSection = screen.getByRole("heading", { name: "要做什麼" });
    expect(panel.compareDocumentPosition(firstSection) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const progress = within(panel);
    expect(progress.getByText("逐段的問題與任務")).toBeInTheDocument();
    expect(progress.getByText("進行中：正在設計題目")).toBeInTheDocument();
    expect(progress.getAllByText("排隊中")).toHaveLength(3);
  });

  it("stars what Manna suggested and lets the teacher take it all off", async () => {
    const suggested = { ...emptySettings(), material_types: ["閱讀文本"], generation: ["加入暖身活動"], levels: ["standard" as const] };
    const api = mockApi(routes(wsState({ settings: suggested, suggested })));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const reading = await screen.findByRole("checkbox", { name: /閱讀文本/ });
    expect(reading).toBeChecked();
    expect(reading.closest("label")).toHaveClass("is-suggested");
    expect(screen.getByText(/都可以改；全部不選也能產生，Manna 會依教材判斷。/)).toBeInTheDocument();
    await user.click(reading);
    await user.click(screen.getByRole("checkbox", { name: /加入暖身活動/ }));
    await user.click(screen.getByRole("checkbox", { name: /一般版/ }));
    await settle();
    expect((api.lastBody("PUT", WS) as SaveWorksheetRequest).settings).toEqual(emptySettings());
  });

  it("shows the server's reason when generating is not possible yet (no material)", async () => {
    const empty = wsState({ material: { ...wsState().material, title: "", paragraphs: [] } });
    mockApi(routes(empty, { [`POST ${WS}/generate`]: respond(400, { error: { code: "bad_request", message: "請先貼上教材。" } }) }));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    expect(await screen.findByText(/還沒有教材。/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "產生學習單" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("請先貼上教材。");
  });
});

describe("entries", () => {
  it("from a lesson: the lesson page creates a linked worksheet; what was carried over can be edited and removed", async () => {
    const fromLesson = wsState({
      lesson: { id: 7, title: "第 1 課：Not only… but also", unit_id: 1, unit_title: "倒裝句單元" },
      material: { ...wsState().material, source: { kind: "lesson_sop", lesson_id: 7, material_version_no: 1, note: "課本第三課" } },
      settings: {
        ...emptySettings(),
        material_types: ["閱讀文本"],
        student_tasks: ["理解內容"],
        language_objective: "I can use because.",
      },
      suggested: { ...emptySettings(), material_types: ["閱讀文本"], student_tasks: ["理解內容"] },
      content: {
        ...emptyWorksheetContent(),
        objectives: [
          { id: "O1", type: "content", text: "學生能說明原因", student_facing_en: "I can explain why.", origin: "teacher", sop_id: "O1" },
          // From the 語言目標 field: edited there, not among what the lesson carried over.
          { id: "teacher-l1", type: "language", text: "I can use because.", student_facing_en: "I can use because.", origin: "teacher", sop_id: null },
        ],
      },
      brief: {
        tasks: [{ id: "t1", paragraph_ids: ["p1"], function: "discover", text: "Why did she start it?", sop_block_id: "tb1", sop_task_id: "t1" }],
        warmup: ["Talk about rainy days"],
        vocabulary: ["umbrella"],
        exit_ticket: [{ kind: "reflection", text: "One thing I learned" }],
      },
    });
    const api = mockApi({
      ...routes(fromLesson),
      "GET /api/lessons/7": lessonDetail({ latest_version: null, versions_count: 0 }),
      "GET /api/lessons/7/runs": [],
      "GET /api/lessons/7/sop": {
        lesson_id: 7,
        version_no: null,
        versions_count: 0,
        saved_at: null,
        material: null,
        content: emptyContent(),
        today_objective: "",
        jobs: { analysis: null, objectives: null },
      },
      "POST /api/worksheets": fromLesson,
    });
    const user = userEvent.setup();
    renderApp("/lessons/7");
    // The rail ends with 產生學習單, so it can be made from any step.
    await user.click(
      await within(await screen.findByRole("navigation", { name: "備課步驟" })).findByRole("button", { name: /產生學習單/ }),
    );
    // The summary of the lesson's choices; 調整設定 opens the generator's form.
    await user.click(await screen.findByRole("button", { name: "調整設定" }));

    expect(await screen.findByRole("heading", { name: "做一份學習單", level: 1 })).toBeInTheDocument();
    expect(api.lastBody("POST", "/api/worksheets")).toEqual({ lesson_id: 7 });

    const carried = within(screen.getByRole("region", { name: "從課例帶入的內容" }));
    expect(carried.getByRole("textbox", { name: "學習目標 1" })).toHaveValue("I can explain why.");
    expect(carried.queryByRole("textbox", { name: "學習目標 2" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "語言目標（選填）" })).toHaveValue("I can use because.");
    expect(carried.getByRole("textbox", { name: "帶入的任務 1" })).toHaveValue("Why did she start it?");
    expect(carried.getByRole("textbox", { name: "帶入的暖身想法 1" })).toHaveValue("Talk about rainy days");
    expect(carried.getByRole("textbox", { name: "帶入的關鍵字 1" })).toHaveValue("umbrella");
    expect(carried.getByRole("textbox", { name: "帶入的 Exit ticket 1" })).toHaveValue("One thing I learned");
    // Suggested from the lesson, starred, and removable.
    expect(screen.getByRole("checkbox", { name: /理解內容/ })).toBeChecked();
    expect(screen.getByText(/Manna 依課例先幫你選的/)).toBeInTheDocument();

    await user.click(carried.getByRole("button", { name: "拿掉任務 1" }));
    await user.click(carried.getByRole("button", { name: "拿掉學習目標 1" }));
    await user.click(screen.getByRole("checkbox", { name: /理解內容/ }));
    await settle();
    const body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.brief?.tasks).toEqual([]);
    expect(body.content?.objectives.map((o) => o.id)).toEqual(["teacher-l1"]);
    expect(body.settings?.student_tasks).toEqual([]);

    await user.click(carried.getByRole("button", { name: "全部不帶入" }));
    await waitFor(() => expect(screen.queryByRole("region", { name: "從課例帶入的內容" })).not.toBeInTheDocument());
    // Nothing was written to the lesson.
    expect(api.callsTo("PUT", "/api/lessons/7/sop")).toHaveLength(0);
  });
});

describe("result editor", () => {
  it("version tabs are only the selected levels; switching changes the items and scaffolds on the sheet", async () => {
    mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const tabs = within(await screen.findByRole("tablist", { name: "版本" }));
    expect(tabs.getAllByRole("tab").map((t) => t.textContent)).toEqual(["基礎版", "一般版"]);
    expect(tabs.getByRole("tab", { name: "基礎版" })).toHaveAttribute("aria-selected", "true");

    // 基礎版: the support item and the sentence frame are there.
    expect(screen.getByDisplayValue("Circle the word that means the same as shop.")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Mrs. Lin started the stand because ___.")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "題目 4" })).toHaveValue("What did the neighbors expect?");

    await user.click(tabs.getByRole("tab", { name: "一般版" }));
    expect(screen.queryByDisplayValue("Circle the word that means the same as shop.")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("Mrs. Lin started the stand because ___.")).not.toBeInTheDocument();
    // Core items stay, renumbered.
    expect(screen.getByRole("textbox", { name: "題目 2" })).toHaveValue("Why did Mrs. Lin start the umbrella stand?");
    expect(screen.getByRole("textbox", { name: "題目 3" })).toHaveValue("What did the neighbors expect?");

    // Warm-up first, exit ticket last, procedural scaffolds marked as automatic, levels from data.
    const sheet = projectSheet(generatedContent(), "standard", "student");
    expect(sheet.blocks.map((b) => b.label)).toEqual(["A", "B", "C", ""]);
    expect(blockRegion("A").getByText("程序鷹架（自動產生）")).toBeInTheDocument();
    expect(blockRegion("A").getByText("第 2 級")).toBeInTheDocument();
    expect(blockRegion("A").getByText("級別未知")).toBeInTheDocument();
    const info = await openBarPopover(userEvent.setup(), "關於這份學習單");
    expect(info.getByText(/這次由 Manna 決定：素材類型：閱讀文本；版本：基礎版、一般版。/)).toBeInTheDocument();
  });

  it("a block a step left empty is shown in the editor but not lettered or printed", () => {
    const content = generatedContent();
    content.blocks[0] = block("warmup", {
      kind: "warmup",
      title: "Warm-up",
      message: "暖身沒有產生成功（AI 目前沒有回應）。你可以自己寫，或按「重新產生」。",
    });
    for (const audience of ["student", "teacher"] as const) {
      const sheet = projectSheet(content, "standard", audience);
      expect(sheet.blocks.map((b) => [b.block.id, b.label, b.printed])).toEqual([
        ["warmup", "", false],
        ["b1", "A", true],
        ["b2", "B", true],
        ["exit", "", true],
      ]);
    }
    // Instructions alone are worth printing.
    content.blocks[0] = { ...content.blocks[0], instructions: "Talk with a partner." };
    expect(projectSheet(content, "standard", "student").blocks[0]?.label).toBe("A");
  });

  it("answers, evidence and teacher notes exist only in 教師版", async () => {
    const api = mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const toggle = within(await screen.findByRole("radiogroup", { name: "顯示版本" }));
    expect(toggle.getByRole("radio", { name: "學生版" })).toBeChecked();
    expect(screen.queryByDisplayValue("A stranger once helped her.")).not.toBeInTheDocument();
    expect(screen.queryByText("參考答案")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("先確認大意。")).not.toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain("A stranger once helped her.");

    await user.click(toggle.getByRole("radio", { name: "教師版" }));
    const answer = screen.getByRole("textbox", { name: "參考答案 2" });
    expect(answer).toHaveValue("A stranger once helped her.");
    expect(screen.getByRole("textbox", { name: "依據 2" })).toHaveValue("A sign said");
    expect(screen.getByRole("textbox", { name: "教師備註 B" })).toHaveValue("先確認大意。");

    // The reference answer can be edited; an edited AI item becomes ai_edited.
    await user.clear(answer);
    await user.type(answer, "Because a stranger helped her.");
    await settle();
    const body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    const edited = body.content?.blocks[1]?.items[0];
    expect(edited).toMatchObject({ answer: "Because a stranger helped her.", origin: "ai_edited" });
    expect(body.content?.blocks[1]?.origin).toBe("ai_edited");
  });

  it("看答案 reveals one block's answers in the student version, and 收起答案 hides them again", async () => {
    const org = newOrganizer("t_chart");
    const content = generatedContent();
    content.blocks = content.blocks.map((b) =>
      b.id === "b2"
        ? {
            ...b,
            items: [
              ...b.items,
              item("b2-2", "Sort the ideas.", {
                answer_format: "圖像組織",
                answer: "",
                organizer: {
                  ...org,
                  sections: org.sections.map((sec, i) => ({
                    ...sec,
                    cells: sec.cells.map((c, j) => ({ ...c, given: "", prefilled: [], answer: `Key ${i}.${j}` })),
                  })),
                },
              }),
            ],
          }
        : b,
    );
    const api = mockApi(routes(generated({ content })));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await screen.findByRole("radiogroup", { name: "顯示版本" });

    // The exit ticket's question has no answer: no button there. Blocks A and B have answers.
    expect(blockRegion("A").getByRole("button", { name: /^看答案/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getAllByRole("button", { name: /^看答案/ })).toHaveLength(3);
    expect(screen.queryByRole("textbox", { name: /參考答案/ })).not.toBeInTheDocument();

    const open = blockRegion("C").getByRole("button", { name: /^看答案/ });
    await user.click(open);
    // C's answers show (reference answer and the organizer's), the sheet is still the student version.
    expect(open).toHaveTextContent("收起答案");
    expect(open).toHaveAttribute("aria-expanded", "true");
    expect(document.getElementById(open.getAttribute("aria-controls") ?? "")).toBeInTheDocument();
    expect(blockRegion("C").getByText("參考答案（只有你看得到）：印出與匯出的學生版不含這些答案。")).toBeInTheDocument();
    expect(screen.getByDisplayValue("That every umbrella would be gone.")).toBeInTheDocument();
    expect(screen.getAllByRole("textbox", { name: /^參考答案 .*Key|^參考答案 / }).length).toBeGreaterThan(1);
    expect(screen.getByDisplayValue("Key 0.0")).toBeInTheDocument();
    // Block B's answer stays hidden, and so does B's teacher notes.
    expect(screen.queryByDisplayValue("A stranger once helped her.")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("先確認大意。")).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "學生版" })).toBeChecked();

    await user.click(open);
    expect(open).toHaveTextContent("看答案");
    expect(screen.queryByDisplayValue("That every umbrella would be gone.")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("Key 0.0")).not.toBeInTheDocument();

    // Keyboard: the button is a real button.
    open.focus();
    await user.keyboard("{Enter}");
    expect(screen.getByDisplayValue("That every umbrella would be gone.")).toBeInTheDocument();

    // 教師版 shows everything and has no 看答案 buttons; the exports were never asked for.
    await user.click(screen.getByRole("radio", { name: "教師版" }));
    expect(screen.getByDisplayValue("A stranger once helped her.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^看答案|^收起答案/ })).not.toBeInTheDocument();
    expect(api.callsTo("GET", `${WS}/export`)).toHaveLength(0);
  });

  it("edits text in place and reorders blocks with 上移／下移", async () => {
    const api = mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const title = await screen.findByRole("textbox", { name: "區塊標題 B" });
    await user.clear(title);
    await user.type(title, "Read and find");
    expect((await openBlockMore(user, blockRegion("A"))).getByRole("button", { name: "上移" })).toBeDisabled();

    await clickBlockMore(user, blockRegion("C"), "上移");
    await settle();
    const body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.content?.blocks.map((b) => b.id)).toEqual(["warmup", "b2", "b1", "exit"]);
    expect(body.content?.blocks[2]?.title).toBe("Read and find");
    // The sheet follows: what was C is now B.
    expect(screen.getByRole("textbox", { name: "區塊標題 B" })).toHaveValue("Paragraph 2: Compare");
  });

  it("上移 slides the blocks that changed place for a pointer, and not for the keyboard", async () => {
    mockApi(routes(generated()));
    const user = userEvent.setup();
    const animate = vi.fn(() => ({ cancel: () => undefined }) as unknown as Animation);
    // jsdom has no layout: a block's top is its place among the blocks.
    const rect = vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const all = Array.from(document.querySelectorAll("[data-block]"));
      return { top: all.indexOf(this) * 100 } as DOMRect;
    });
    Object.defineProperty(HTMLElement.prototype, "animate", { value: animate, configurable: true, writable: true });
    try {
      renderApp("/worksheets/5");
      await screen.findByRole("textbox", { name: "區塊標題 B" });

      await clickBlockMore(user, blockRegion("C"), "上移");
      expect(animate).toHaveBeenCalledTimes(2);

      animate.mockClear();
      const menu = await openBlockMore(user, blockRegion("C"));
      menu.getByRole("button", { name: "上移" }).focus();
      await user.keyboard("{Enter}");
      await settle();
      expect(animate).not.toHaveBeenCalled();
    } finally {
      rect.mockRestore();
      Reflect.deleteProperty(HTMLElement.prototype, "animate");
    }
  });

  it("調整設定 can be left without generating: 回到學習單 at the top and beside the generate button", async () => {
    const api = mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await screen.findByRole("textbox", { name: "區塊標題 B" });
    await user.click((await openBarPopover(user, "更多…")).getByRole("button", { name: "調整設定" }));
    // The form, with a way back at the top…
    await user.click(await screen.findByRole("button", { name: "回到學習單" }));
    expect(await screen.findByRole("textbox", { name: "區塊標題 B" })).toBeInTheDocument();
    // …and beside the generate button.
    await user.click((await openBarPopover(user, "更多…")).getByRole("button", { name: "調整設定" }));
    await user.click(await screen.findByRole("button", { name: "不改了，回到學習單" }));
    expect(await screen.findByRole("textbox", { name: "區塊標題 B" })).toBeInTheDocument();
    // Nothing was generated on the way.
    expect(api.callsTo("POST", `${WS}/generate`)).toHaveLength(0);
  });

  it("regenerates one block and can undo it", async () => {
    const regenerated = generated({
      version_no: 3,
      undoable: ["b1"],
      content: {
        ...generatedContent(),
        blocks: generatedContent().blocks.map((b) =>
          b.id === "b1" ? { ...b, title: "Paragraph 1: Find (new)", items: [item("b1-9", "Who is Mrs. Lin?")] } : b,
        ),
      },
    });
    const api = mockApi(
      routes(generated(), {
        [`POST ${WS}/blocks/b1/regenerate`]: regenerated,
        [`POST ${WS}/blocks/b1/undo`]: generated({ version_no: 4 }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await screen.findByRole("textbox", { name: "區塊標題 B" });
    expect((await openBlockMore(user, blockRegion("B"))).queryByRole("button", { name: "復原上一版" })).not.toBeInTheDocument();

    await clickBlockMore(user, blockRegion("B"), "重新產生");
    expect(await screen.findByDisplayValue("Paragraph 1: Find (new)")).toBeInTheDocument();
    expect(api.callsTo("POST", `${WS}/blocks/b1/regenerate`)).toHaveLength(1);

    await clickBlockMore(user, blockRegion("B"), "復原上一版");
    expect(await screen.findByDisplayValue("Paragraph 1: Find")).toBeInTheDocument();
    expect(api.callsTo("POST", `${WS}/blocks/b1/undo`)).toHaveLength(1);
    expect((await openBlockMore(user, blockRegion("B"))).queryByRole("button", { name: "復原上一版" })).not.toBeInTheDocument();
  });

  it("turns the warm-up and the exit ticket off and on, and says so honestly when a block failed", async () => {
    const withMessage = generated();
    withMessage.content.blocks[0] = { ...withMessage.content.blocks[0]!, message: "暖身沒有產生成功，你可以自己寫，或按重新產生。" };
    const api = mockApi(routes(withMessage));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    expect(await screen.findByText("暖身沒有產生成功，你可以自己寫，或按重新產生。")).toBeInTheDocument();
    // A failed block can still be edited.
    expect(blockRegion("A").getByRole("textbox", { name: "區塊標題 A" })).toBeEnabled();

    await clickBlockMore(user, blockRegion("Exit ticket"), "關掉 Exit ticket");
    await clickBlockMore(user, blockRegion("A"), "關掉暖身");
    await settle();
    let body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.content?.blocks.map((b) => b.enabled)).toEqual([false, true, true, false]);
    // The first activity block is now A; the switched-off blocks keep their content and can come back.
    expect(screen.getByRole("textbox", { name: "區塊標題 A" })).toHaveValue("Paragraph 1: Find");
    expect(screen.queryByDisplayValue("One question I still have:")).not.toBeInTheDocument();

    await user.click(within(screen.getByRole("region", { name: "區塊 Exit ticket（已關掉）" })).getByRole("button", { name: "加回 Exit ticket" }));
    await settle();
    body = api.lastBody("PUT", WS) as SaveWorksheetRequest;
    expect(body.content?.blocks.map((b) => b.enabled)).toEqual([false, true, true, true]);
    expect(screen.getByDisplayValue("One question I still have:")).toBeInTheDocument();
  });

  it("exports the current tab from one 匯出 menu: student or teacher version, Word or PDF", async () => {
    const api = mockApi(routes(generated()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const button = await screen.findByRole("button", { name: /匯出/ });
    expect(button).toHaveAttribute("aria-haspopup", "dialog");
    expect(screen.queryByRole("button", { name: "學生版 DOCX" })).not.toBeInTheDocument();
    await user.click(within(screen.getByRole("tablist", { name: "版本" })).getByRole("tab", { name: "一般版" }));
    await user.click(button);
    const dialog = within(screen.getByRole("dialog", { name: "匯出：一般版" }));
    expect(dialog.getAllByRole("checkbox").map((b) => b.closest("label")?.textContent)).toEqual([
      "學生版 Word",
      "學生版 PDF",
      "教師版 Word",
      "教師版 PDF",
    ]);
    await user.click(dialog.getByRole("checkbox", { name: "教師版 PDF" }));
    await user.click(dialog.getByRole("button", { name: "下載" }));
    await waitFor(() => expect(api.callsTo("GET", `${WS}/export`)).toHaveLength(1));
    const url = api.callsTo("GET", `${WS}/export`)[0]!.url;
    expect(new URLSearchParams(url.split("?")[1]).toString()).toBe("format=pdf&level=standard&audience=teacher");
    // The mock has no file to give: the failure is said, not swallowed.
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  it("never writes to the lesson by itself: sync-back needs the preview and the confirm button", async () => {
    const linked = generated({
      lesson: { id: 7, title: "第 1 課", unit_id: 1, unit_title: "倒裝句單元" },
      sop_sync: { linked: true, changes: 2 },
    });
    const preview: SopSyncPreview = {
      lesson_id: 7,
      sop_version_no: 3,
      worksheet_version_no: 8,
      worksheet_updated_at: "2026-11-20T12:00:00Z",
      core_suggestions: [],
      changes: [
        { key: "objective:O1", target: "objective", action: "update", label: "內容目標", before: "I can explain.", after: "I can explain why Mrs. Lin started the stand." },
        { key: "block:b2", target: "task_block", action: "add", label: "第 2 段的任務", before: "", after: "What did the neighbors expect?" },
        { key: "block:tb9", target: "task_block", action: "remove", label: "第 4 段的任務", before: "Old task", after: "" },
      ],
    };
    const api = mockApi(
      routes(linked, {
        [`GET ${WS}/sop-sync`]: preview,
        [`POST ${WS}/sop-sync`]: { sop_version_no: 4, applied: 2 },
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    expect(await screen.findByText("學習單的目標或任務和課例不同，課例還是原本的。")).toBeInTheDocument();
    expect(api.callsTo("GET", `${WS}/sop-sync`)).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "同步回課例" }));
    const dialog = within(await screen.findByRole("dialog", { name: "同步回課例" }));
    expect(api.callsTo("POST", `${WS}/sop-sync`)).toHaveLength(0);
    expect(dialog.getByText("I can explain why Mrs. Lin started the stand.")).toBeInTheDocument();
    const boxes = dialog.getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes.map((b) => b.checked)).toEqual([true, true, false]);

    // Cancelling sends nothing.
    await user.click(dialog.getByRole("button", { name: "取消" }));
    expect(api.callsTo("POST", `${WS}/sop-sync`)).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "同步回課例" }));
    const again = within(await screen.findByRole("dialog", { name: "同步回課例" }));
    await user.click(again.getAllByRole("checkbox")[1]!);
    await user.click(again.getByRole("button", { name: "確認同步到課例" }));
    expect(await screen.findByText("已同步，課例多了一個版本。")).toBeInTheDocument();
    expect(api.lastBody("POST", `${WS}/sop-sync`)).toEqual({ confirm: true, keys: ["objective:O1"], sop_version_no: 3, worksheet_version_no: 8, worksheet_updated_at: "2026-11-20T12:00:00Z", lesson_id: 7 });
  });

  it("shows a conflict when the worksheet changes after the sync preview", async () => {
    const linked = generated({
      lesson: { id: 7, title: "第 1 課", unit_id: 1, unit_title: "倒裝句單元" },
      sop_sync: { linked: true, changes: 1 },
    });
    const preview: SopSyncPreview = {
      lesson_id: 7, sop_version_no: 3, worksheet_version_no: 8, core_suggestions: [],
      changes: [{ key: "objective:O1", target: "objective", action: "update", label: "內容目標", before: "old", after: "new" }],
    };
    mockApi(routes(linked, {
      [`GET ${WS}/sop-sync`]: preview,
      [`POST ${WS}/sop-sync`]: respond(409, { error: { code: "conflict", message: "學習單已更新，請重新預覽。" } }),
    }));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(await screen.findByRole("button", { name: "同步回課例" }));
    const dialog = within(await screen.findByRole("dialog", { name: "同步回課例" }));
    await user.click(dialog.getByRole("button", { name: "確認同步到課例" }));
    expect(await dialog.findByRole("alert")).toHaveTextContent("學習單已更新，請重新預覽。");
    expect(screen.queryByText("已同步，課例多了一個版本。")).not.toBeInTheDocument();
  });

  it("「不用」 dismisses the sync line without calling anything", async () => {
    const linked = generated({
      lesson: { id: 7, title: "第 1 課", unit_id: 1, unit_title: "倒裝句單元" },
      sop_sync: { linked: true, changes: 1 },
    });
    const api = mockApi(routes(linked));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(await screen.findByRole("button", { name: "不用" }));
    expect(screen.queryByText(/課例還是原本的/)).not.toBeInTheDocument();
    expect(api.calls.filter((c) => c.path.includes("sop-sync"))).toHaveLength(0);
  });
});

describe("詞彙級別檢查", () => {
  const analysis: VocabularyAnalysis = {
    wordlist_version: "CEEC-111",
    target_level: 4,
    basis: {
      counted_tokens: 120,
      counted_types: 80,
      denominator_note: "比例的分母＝可對應到詞彙表的詞。",
      excluded_tokens: { proper_nouns: 3, unknown: 2, multiword: 1, appendix: 4 },
      match_types: { exact: 100, inflection: 20 },
    },
    level_distribution: [
      { level: 1, tokens: 90, token_ratio: 0.75, types: 56, type_ratio: 0.7 },
      { level: 5, tokens: 2, token_ratio: 0.0167, types: 2, type_ratio: 0.025 },
    ],
    above_target: [
      {
        base: "conserve",
        level: 5,
        count: 1,
        positions: [{ form: "conserve", line: 2, col: 4, context: "to conserve water", match: "exact" }],
      },
    ],
    ambiguous_forms: [],
    derived_matches: [],
    multi_level_words: [],
    proper_nouns: [{ word: "Lin", count: 3, occurrences: [] }],
    unknown: [{ word: "gyre", count: 1, occurrences: [] }],
    multiword: [{ word: "flood-resistant", count: 1, occurrences: [] }],
    appendix_words: [{ word: "Monday", count: 4, occurrences: [] }],
    limits: ["不判斷詞性與詞義。"],
    disclaimer: "本分析為備課輔助，不是大考中心的官方評定、命題預測或品質保證。",
  };
  const check = (extra: Partial<WsVocabCheck> = {}): WsVocabCheck => ({
    id: 1,
    level: "basic",
    audience: "student",
    target_level: 4,
    version_no: 2,
    checked_at: "2026-11-15T09:00:00Z",
    stale: false,
    analysis,
    lines: ["A 暖身 · 字詞表", "B · 第 2 題"],
    ...extra,
  });

  it("shows the distribution with its basis, words above the target with positions, the separate lists, limits, disclaimer and the stale note", async () => {
    const api = mockApi(
      routes(generated(), {
        [`GET ${WS}/checks`]: [check({ stale: true })],
        [`POST ${WS}/checks/vocabulary`]: check({ id: 2, version_no: 3 }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5/check");
    expect(await screen.findByRole("heading", { name: "詞彙級別檢查", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("note")).toHaveTextContent("不是大考中心的官方評定、命題預測或品質保證");
    expect(screen.getByRole("note")).toHaveTextContent("分析的是「基礎版」學生版第 2 版");
    expect(screen.getByText("這是第 2 版的結果，學習單之後改過，請重新分析。")).toBeInTheDocument();

    const dist = within(screen.getByRole("region", { name: "詞彙級別分布" }));
    expect(dist.getByText("比例的分母＝可對應到詞彙表的詞。")).toBeInTheDocument();
    expect(dist.getByText(/詞彙表版本：CEEC-111/)).toHaveTextContent("計入 120 個詞次、80 個不同的詞");
    expect(dist.getByText(/未計入的詞次：專有名詞 3、未收錄 2/)).toBeInTheDocument();
    expect(dist.getByRole("row", { name: /第 1 級/ })).toHaveTextContent("56 個（70%）");
    expect(dist.getByRole("row", { name: /第 5 級（超出）/ })).toHaveTextContent("2 個（2.5%）");

    const above = within(screen.getByRole("region", { name: "超出目標級別的詞" }));
    const row = within(above.getByRole("row", { name: /conserve/ }));
    expect(row.getByText("B · 第 2 題")).toBeInTheDocument();
    expect(row.getByText("to conserve water")).toBeInTheDocument();

    const other = within(screen.getByRole("region", { name: "另外列出、不歸入任何一級" }));
    expect(within(other.getByRole("region", { name: "專有名詞（1）" })).getByText("Lin ×3")).toBeInTheDocument();
    expect(within(other.getByRole("region", { name: "未收錄（1）" })).getByText("gyre")).toBeInTheDocument();
    expect(within(other.getByRole("region", { name: "連字號與複合詞、片語（1）" })).getByText("flood-resistant")).toBeInTheDocument();
    expect(within(other.getByRole("region", { name: "附錄詞彙（1）" })).getByText("Monday ×4")).toBeInTheDocument();
    expect(screen.getByText("不判斷詞性與詞義。")).toBeInTheDocument();

    // Only this worksheet's levels can be analysed; analysing again replaces the stale result.
    expect(within(screen.getByRole("combobox", { name: "版本" })).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "基礎版",
      "一般版",
    ]);
    await user.click(screen.getByRole("button", { name: "重新分析" }));
    await waitFor(() => expect(screen.queryByText(/學習單之後改過/)).not.toBeInTheDocument());
    expect(api.lastBody("POST", `${WS}/checks/vocabulary`)).toEqual({ level: "basic", target_level: 4 });
    expect(screen.getByRole("link", { name: "回學習單修改" })).toHaveAttribute("href", "/worksheets/5");
  });

  it("says so when no word list has been imported", async () => {
    mockApi(
      routes(generated(), {
        [`GET ${WS}/checks`]: [],
        [`POST ${WS}/checks/vocabulary`]: respond(409, {
          error: { code: "conflict", message: "系統尚未匯入大考中心參考詞彙表，無法分析詞彙級別。" },
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/5/check");
    await user.click(await screen.findByRole("button", { name: "分析" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("系統尚未匯入大考中心參考詞彙表，無法分析詞彙級別。");
    expect(screen.queryByRole("region", { name: "詞彙級別分布" })).not.toBeInTheDocument();
  });
});
