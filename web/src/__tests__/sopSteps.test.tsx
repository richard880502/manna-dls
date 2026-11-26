// 逐步設計 steps 3–7 with Manna (docs/API.md「逐步設計：每一步的 AI 協助」): 換一個, 幫我修英文,
// per-paragraph goals, tasks with the generator's labels, warm-up, exit ticket, and 產生學習單
// from the choices made on the way.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import type {
  AiResult,
  SopAnalysisSuggestion,
  SopContent,
  SopExitSuggestion,
  SopGoalsSuggestion,
  SopJob,
  SopJobKind,
  SopMaterial,
  SopObjectivesSuggestion,
  SopState,
  SopTasksSuggestion,
  SopWarmupSuggestion,
  WorksheetSettings,
} from "../api/types";
import { emptyContent } from "../lib/sop";
import { sopConfig } from "../components/sop/useSop";
import { baseRoutes, lessonDetail, mockApi, renderApp, settleAutosave, signIn, type MockCall } from "../test/mockApi";

const SOP = "/api/lessons/7/sop";
const P1 = "Mrs. Lin put an umbrella stand outside her shop. A sign said, Take one if you need it.";
const P2 = "Her neighbors laughed. They were sure every umbrella would be gone within a week.";

const material: SopMaterial = {
  id: 1,
  version_no: 1,
  title: "The Umbrella Stand",
  source_note: "",
  text: `${P1}\n\n${P2}`,
  paragraphs: [
    { id: "p1", no: 1, text: P1 },
    { id: "p2", no: 2, text: P2 },
  ],
  word_count: 30,
  created_at: "2026-11-14T08:00:00Z",
};

const objectives: SopContent["objectives"] = [
  {
    id: "o-1",
    type: "content",
    text: "學生能說出原因",
    student_facing_en: "I can say why.",
    reason: "",
    paragraph_ids: [],
    links: [],
    link_status: "unlinked",
    origin: "teacher",
  },
  {
    id: "o-2",
    type: "language",
    text: "學生能用 because 說明原因",
    student_facing_en: "I can use because.",
    reason: "",
    paragraph_ids: [],
    links: [],
    link_status: "unlinked",
    origin: "teacher",
  },
];

const settings = (patch: Partial<WorksheetSettings> = {}): WorksheetSettings => ({
  material_types: ["閱讀文本"],
  student_tasks: [],
  answer_formats: [],
  generation: ["加入暖身活動"],
  levels: ["standard"],
  minutes: null,
  output: "worksheet",
  skip_exit_ticket: false,
  ...patch,
});

function sopState(content: SopContent = { ...emptyContent(), objectives }, jobs: Partial<SopState["jobs"]> = {}): SopState {
  return {
    lesson_id: 7,
    version_no: 1,
    versions_count: 1,
    saved_at: "2026-11-14T08:00:00Z",
    material,
    content,
    today_objective: "",
    worksheet_settings: settings(),
    worksheet_suggested: settings(),
    jobs: { analysis: null, objectives: null, goals: null, tasks: null, warmup: null, exit_ticket: null, redo: [], ...jobs },
  };
}

let nextId = 40;
function job<T>(kind: SopJobKind, output: T | null, extra: Partial<SopJob<T>> = {}, interaction = 91): SopJob<T> {
  const result: AiResult<T> | null =
    output === null
      ? null
      : { interaction_id: interaction, status: "shown", message: null, output, sources: [], context: [] };
  nextId += 1;
  return {
    id: nextId,
    lesson_id: 7,
    kind,
    status: result ? "done" : "running",
    phase: result ? "" : "正在處理",
    started_at: new Date().toISOString(),
    finished_at: result ? new Date().toISOString() : null,
    sop_version_no: 1,
    result,
    decided: [],
    slot: "",
    parent_job_id: null,
    ...extra,
  };
}

const savedSop = (call: MockCall) => ({
  version_no: 2,
  versions_count: 2,
  saved_at: "2026-11-14T09:30:00Z",
  content: (call.body as { content: SopContent }).content,
  worksheet_settings: settings(),
  worksheet_suggested: settings(),
});

function routes(sop: SopState, extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/lessons/7": lessonDetail({ latest_version: null, versions_count: 0 }),
    "GET /api/units/1/core": { id: 1, unit_id: 1, focuses: [] },
    "GET /api/siop": [],
    "GET /api/teacher-cases": [],
    "GET /api/lessons/7/runs": [],
    "GET /api/worksheets": [],
    [`GET ${SOP}`]: sop,
    [`PUT ${SOP}`]: savedSop,
    "GET /api/materials/1/annotations": [],
    ...extra,
  };
}

const rail = () => within(screen.getByRole("navigation", { name: "備課步驟" }));
const goToStep = (user: ReturnType<typeof userEvent.setup>, name: RegExp) => user.click(rail().getByRole("button", { name }));
const lastSaved = (api: ReturnType<typeof mockApi>) => (api.lastBody("PUT", SOP) as { content: SopContent }).content;

async function open() {
  const rendered = renderApp("/lessons/7");
  await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 });
  return rendered;
}

beforeEach(() => {
  sopConfig.pollMs = 20;
});

// ---------------------------------------------------------------- step 3

describe("step 3: 換一個 and 幫我修英文", () => {
  const output: SopObjectivesSuggestion = {
    curriculum_available: 0,
    objectives: [
      {
        type: "content",
        text: "學生能比較預期和結果",
        student_facing_en: "I can compare.",
        reason: "第 2 段有對比",
        paragraph_ids: [],
        link_status: "unlinked",
        links: [],
      },
      {
        type: "language",
        text: "學生能用 however 說明轉折",
        student_facing_en: "I can use however.",
        reason: "需要轉折語",
        paragraph_ids: [],
        link_status: "unlinked",
        links: [],
      },
    ],
  };

  it("換一個 sets one suggestion aside, asks for just that one, and shows the new one in its place", async () => {
    signIn();
    const whole = job("objectives", output, {}, 91);
    const replacement: SopObjectivesSuggestion = {
      curriculum_available: 0,
      objectives: [{ ...output.objectives[1]!, text: "學生能用 so 和 therefore 說明結果", student_facing_en: "I can use so." }],
    };
    const redo = job("objectives", null, { slot: `${whole.id}:1`, parent_job_id: whole.id });
    const done = { ...redo, status: "done" as const, phase: "", result: job("objectives", replacement, {}, 92).result };
    const api = mockApi(
      routes(sopState({ ...emptyContent(), objectives: [] }, { objectives: whole }), {
        [`POST ${SOP}/objectives`]: redo,
        [`GET /api/sop-jobs/${redo.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /內容目標與語言目標/);

    const language = within(screen.getByRole("group", { name: "Manna 建議：語言目標" }));
    expect(language.getByText("學生能用 however 說明轉折")).toBeInTheDocument();
    await user.click(language.getByRole("button", { name: "換一個" }));
    // Only that one is asked for again; the other suggestion stays.
    expect(api.lastBody("POST", `${SOP}/objectives`)).toEqual({ parent_job_id: whole.id, item_index: 1 });
    expect(screen.getByText("學生能比較預期和結果")).toBeInTheDocument();
    // The one set aside is logged as 不用.
    await waitFor(() =>
      expect(api.callsTo("POST", "/api/ai/91/decision").map((c) => c.body)).toEqual([{ item_index: 1, decision: "reject" }]),
    );

    // The replacement takes its place and is adopted with its own interaction's decision.
    const again = await screen.findByRole("group", { name: "Manna 建議：語言目標" });
    expect(within(again).getByText("學生能用 so 和 therefore 說明結果")).toBeInTheDocument();
    expect(screen.queryByText("學生能用 however 說明轉折")).not.toBeInTheDocument();
    await user.click(within(again).getByRole("button", { name: "採用" }));
    await settleAutosave();
    expect(api.callsTo("POST", "/api/ai/92/decision").map((c) => c.body)).toEqual([{ item_index: 0, decision: "adopt" }]);
    const saved = lastSaved(api).objectives;
    expect(saved.map((o) => [o.text, o.origin])).toEqual([["學生能用 so 和 therefore 說明結果", "ai_adopted"]]);
    // The content suggestion was never touched.
    expect(screen.getByText("學生能比較預期和結果")).toBeInTheDocument();
  });

  async function openForm(api: ReturnType<typeof mockApi>, user: ReturnType<typeof userEvent.setup>) {
    void api;
    await open();
    await goToStep(user, /內容目標與語言目標/);
    await user.click(screen.getByRole("button", { name: "自己新增一項目標" }));
  }

  it("幫我修英文 shows Manna's version beside the teacher's; 用這個 takes it, 保留我的 never replaces", async () => {
    signIn();
    const english = (id: number, text: string, changed = true) => ({
      interaction_id: id,
      status: "shown",
      message: null,
      output: { english: text, changed, note: "修了大小寫與句點。" },
      sources: [],
      context: [],
    });
    let calls = 0;
    const api = mockApi(
      routes(sopState({ ...emptyContent(), objectives: [] }), {
        [`POST ${SOP}/objective-english`]: () => {
          calls += 1;
          return english(70 + calls, "I can say why it happened.");
        },
        "POST /api/ai/71/decision": { ok: true },
        "POST /api/ai/72/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await openForm(api, user);
    // Nothing to ask about yet: the Chinese objective comes first.
    expect(screen.getByRole("button", { name: "依中文目標寫英文版" })).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "目標" }), "學生能說出原因");
    await user.type(screen.getByRole("textbox", { name: "給學生看的英文版本" }), "i can say why it happen");
    await user.click(screen.getByRole("button", { name: "幫我修英文" }));
    expect(api.lastBody("POST", `${SOP}/objective-english`)).toEqual({
      type: "content",
      text: "學生能說出原因",
      student_facing_en: "i can say why it happen",
    });
    const proposal = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    expect(proposal.getByText("i can say why it happen")).toBeInTheDocument();
    expect(proposal.getByText("I can say why it happened.")).toBeInTheDocument();
    // 保留我的: the teacher's English stays.
    await user.click(proposal.getByRole("button", { name: "保留我的" }));
    expect(screen.getByRole("textbox", { name: "給學生看的英文版本" })).toHaveValue("i can say why it happen");
    expect(screen.queryByRole("group", { name: "Manna 建議的英文" })).not.toBeInTheDocument();
    // Ask again and take it.
    await user.click(screen.getByRole("button", { name: "幫我修英文" }));
    const second = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    await user.click(second.getByRole("button", { name: "用這個" }));
    expect(screen.getByRole("textbox", { name: "給學生看的英文版本" })).toHaveValue("I can say why it happened.");
    await user.click(screen.getByRole("button", { name: "加入目標" }));
    await settleAutosave();
    expect(api.callsTo("POST", "/api/ai/71/decision").map((c) => c.body)).toEqual([{ item_index: 0, decision: "reject" }]);
    expect(api.callsTo("POST", "/api/ai/72/decision").map((c) => c.body)).toEqual([{ item_index: 0, decision: "adopt" }]);
    expect(lastSaved(api).objectives.map((o) => [o.student_facing_en, o.origin])).toEqual([
      ["I can say why it happened.", "ai_edited"],
    ]);
  });

  it("with no English yet the same button writes one from the Chinese objective; a sentence that is fine is left alone", async () => {
    signIn();
    const reply = (text: string, changed: boolean) => ({
      interaction_id: 73,
      status: "shown",
      message: null,
      output: { english: text, changed, note: changed ? "依中文目標寫的英文版本。" : "已經很好。" },
      sources: [],
      context: [],
    });
    let n = 0;
    const api = mockApi(
      routes(sopState({ ...emptyContent(), objectives: [] }), {
        [`POST ${SOP}/objective-english`]: () => {
          n += 1;
          return n === 1 ? reply("I can explain why it happened.", true) : reply("I can explain why it happened.", false);
        },
        "POST /api/ai/73/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await openForm(api, user);
    await user.type(screen.getByRole("textbox", { name: "目標" }), "學生能說明原因");
    await user.click(screen.getByRole("button", { name: "依中文目標寫英文版" }));
    const proposal = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    await user.click(proposal.getByRole("button", { name: "用這個" }));
    expect(screen.getByRole("textbox", { name: "給學生看的英文版本" })).toHaveValue("I can explain why it happened.");
    // Now the button says 幫我修英文; a sentence that needs no change has no 用這個.
    await user.click(screen.getByRole("button", { name: "幫我修英文" }));
    const fine = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    expect(fine.queryByRole("button", { name: "用這個" })).not.toBeInTheDocument();
    expect(fine.getByText("已經很好。")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- step 4

describe("step 4: 逐段整合目標", () => {
  const output: SopGoalsSuggestion = {
    paragraphs: [
      { paragraph_id: "p1", paragraph_no: 1, objective_ids: ["o-1"], note: "先找出告示，再說明規則。" },
      { paragraph_id: "p2", paragraph_no: 2, objective_ids: [], note: "這一段只是過場，不特別對應目標。" },
    ],
    missing_paragraph_ids: [],
  };

  it("asks, then shows a suggestion under each paragraph: adopt keeps what the teacher wrote, dismiss and edit are logged", async () => {
    signIn();
    let polls = 0;
    const running = job<SopGoalsSuggestion>("goals", null);
    const api = mockApi(
      routes(
        sopState({
          ...emptyContent(),
          objectives,
          paragraph_goals: [{ paragraph_id: "p1", objective_ids: ["o-2"], note: "我寫的筆記", origin: "teacher" }],
        }),
        {
          [`POST ${SOP}/goals`]: running,
          [`GET /api/sop-jobs/${running.id}`]: () => {
            polls += 1;
            return polls < 2 ? running : { ...running, ...job("goals", output), id: running.id };
          },
          "POST /api/ai/91/decision": { ok: true },
        },
      ),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段整合目標/);
    await user.click(screen.getByRole("button", { name: "請 Manna 逐段建議" }));
    expect(api.lastBody("POST", `${SOP}/goals`)).toEqual({});

    const p1 = within(await screen.findByRole("region", { name: "第 1 段" }));
    const well = within(await p1.findByRole("group", { name: "Manna 建議：第 1 段" }));
    expect(well.getByText("先找出告示，再說明規則。")).toBeInTheDocument();
    await user.click(well.getByRole("button", { name: "採用" }));
    // Her objective and her note stay; the suggested objective is added.
    expect(p1.getByDisplayValue("我寫的筆記")).toBeInTheDocument();
    expect(p1.getAllByRole("checkbox").filter((c) => (c as HTMLInputElement).checked)).toHaveLength(2);

    const p2 = within(screen.getByRole("region", { name: "第 2 段" }));
    const second = within(p2.getByRole("group", { name: "Manna 建議：第 2 段" }));
    expect(second.getByText("Manna 認為這一段不特別對應目標。")).toBeInTheDocument();
    await user.click(second.getByRole("button", { name: "修改後採用" }));
    expect(p2.getByRole("textbox", { name: "這一段要怎麼帶" })).toHaveValue("這一段只是過場，不特別對應目標。");

    await settleAutosave();
    const goals = lastSaved(api).paragraph_goals;
    expect(goals.find((g) => g.paragraph_id === "p1")).toEqual({
      paragraph_id: "p1",
      objective_ids: ["o-2", "o-1"],
      note: "我寫的筆記",
      origin: "ai_edited",
    });
    expect(goals.find((g) => g.paragraph_id === "p2")?.origin).toBe("ai_edited");
    expect(api.callsTo("POST", "/api/ai/91/decision").map((c) => c.body)).toEqual([
      { item_index: 1, decision: "adopt" },
      { item_index: 2, decision: "revise", revised_content: "這一段只是過場，不特別對應目標。" },
    ]);
  });

  it("這一段再建議一次 asks for that paragraph only; an undecided suggestion is set aside", async () => {
    signIn();
    const whole = job("goals", output, {}, 91);
    const redo = job<SopGoalsSuggestion>("goals", null, { slot: "p:p1", parent_job_id: whole.id });
    const redone: SopGoalsSuggestion = {
      paragraphs: [{ paragraph_id: "p1", paragraph_no: 1, objective_ids: ["o-2"], note: "換個角度：先圈出 because。" }],
      missing_paragraph_ids: [],
    };
    const api = mockApi(
      routes(sopState({ ...emptyContent(), objectives }, { goals: whole }), {
        [`POST ${SOP}/goals`]: redo,
        [`GET /api/sop-jobs/${redo.id}`]: { ...redo, ...job("goals", redone, {}, 95), id: redo.id, slot: "p:p1", parent_job_id: whole.id },
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/95/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段整合目標/);
    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    await user.click(p1.getByRole("button", { name: "這一段再建議一次" }));
    expect(api.lastBody("POST", `${SOP}/goals`)).toEqual({ paragraph_id: "p1", parent_job_id: whole.id });
    expect(await p1.findByText("換個角度：先圈出 because。")).toBeInTheDocument();
    expect(p1.queryByText("先找出告示，再說明規則。")).not.toBeInTheDocument();
    // The other paragraph's suggestion is still there.
    expect(screen.getByText("這一段只是過場，不特別對應目標。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/ai/91/decision").map((c) => c.body)).toEqual([{ item_index: 1, decision: "reject" }]);
    await user.click(within(p1.getByRole("group", { name: "Manna 建議：第 1 段" })).getByRole("button", { name: "採用" }));
    expect(api.callsTo("POST", "/api/ai/95/decision").map((c) => c.body)).toEqual([{ item_index: 1, decision: "adopt" }]);
  });

  it("says why it cannot start without objectives", async () => {
    signIn();
    mockApi(
      routes(sopState({ ...emptyContent(), objectives: [] }), {
        [`POST ${SOP}/goals`]: {
          status: 400,
          body: { error: { code: "bad_request", message: "請先在第 3 步寫（或採用）至少一項目標，Manna 才知道要對應哪些目標。" } },
        },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段整合目標/);
    expect(screen.getByText(/還沒有目標可以對應/)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- step 5

describe("step 5: 逐段問題或任務", () => {
  const output: SopTasksSuggestion = {
    scaffolds_ok: true,
    missing_paragraph_ids: [],
    blocks: [
      {
        paragraph_ids: ["p1"],
        paragraph_nos: [1],
        title: "Why the stand?",
        student_tasks: ["理解內容", "推論／預測"],
        answer_formats: ["選擇題"],
        objective_ids: ["o-1"],
        reason: "先確認學生讀懂規則",
        tasks: [
          {
            function: "discover",
            text: "Why did Mrs. Lin put the stand outside?",
            answer_format: "選擇題",
            options: ["To help people", "To sell", "To hide", "To play"],
            answer: "To help people",
            evidence: "A sign said, Take one if you need it.",
            scaffolds: { verbal: "句型框架：She did it because ____.", instructional: "", procedural: "1. Read.\n2. Answer." },
          },
        ],
      },
    ],
  };

  it("a suggested block comes with the generator's labels starred; adopting adds to what the teacher wrote and the labels can be changed", async () => {
    signIn();
    const whole = job("tasks", output, {}, 91);
    const api = mockApi(
      routes(
        sopState(
          {
            ...emptyContent(),
            objectives,
            task_blocks: [
              {
                id: "mine",
                paragraph_ids: ["p1"],
                student_tasks: ["討論／分享"],
                answer_formats: [],
                ai_student_tasks: [],
                ai_answer_formats: [],
                tasks: [
                  {
                    id: "t-mine",
                    function: "discover",
                    text: "What did the sign say?",
                    scaffolds: { verbal: "", instructional: "", procedural: "" },
                    origin: "teacher",
                    answer_format: "",
                    options: [],
                    answer: "",
                    evidence: "",
                  },
                ],
              },
            ],
          },
          { tasks: whole },
        ),
        { "POST /api/ai/91/decision": { ok: true } },
      ),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段問題或任務/);

    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    // Her own task first, then Manna's, with the options and the answer for her.
    expect(p1.getByText("What did the sign say?")).toBeInTheDocument();
    const well = within(p1.getByRole("group", { name: "Manna 建議：第 1 段" }));
    expect(well.getByText("Why did Mrs. Lin put the stand outside?")).toBeInTheDocument();
    expect(well.getByText("To help people", { selector: "li" })).toBeInTheDocument();
    expect(well.getByText(/Manna 先選了學生任務：理解內容、推論／預測/)).toBeInTheDocument();
    await user.click(well.getByRole("button", { name: "採用" }));

    await settleAutosave();
    const saved = lastSaved(api).task_blocks;
    expect(saved).toHaveLength(1);
    const block = saved[0]!;
    expect(block.tasks.map((t) => [t.text, t.origin])).toEqual([
      ["What did the sign say?", "teacher"],
      ["Why did Mrs. Lin put the stand outside?", "ai_adopted"],
    ]);
    expect(block.tasks[1]).toMatchObject({ answer_format: "選擇題", answer: "To help people", evidence: "A sign said, Take one if you need it." });
    // Her label stays, Manna's are added and remembered as hers (the star).
    expect(block.student_tasks).toEqual(["討論／分享", "理解內容", "推論／預測"]);
    expect(block.answer_formats).toEqual(["選擇題"]);
    expect(block.ai_student_tasks).toEqual(["理解內容", "推論／預測"]);
    expect(api.callsTo("POST", "/api/ai/91/decision").map((c) => c.body)).toEqual([{ item_index: 0, decision: "adopt" }]);

    // The labels are the generator's own chips, changed here, per block.
    await user.click(p1.getByText(/學生任務與作答方式/));
    const starred = p1.getByRole("checkbox", { name: /理解內容/ });
    expect(starred).toBeChecked();
    expect(starred.closest("label")).toHaveClass("is-suggested");
    await user.click(starred);
    await user.click(p1.getByRole("checkbox", { name: /是非題/ }));
    await settleAutosave();
    const after = lastSaved(api).task_blocks[0]!;
    expect(after.student_tasks).toEqual(["推論／預測", "討論／分享"]);
    expect(after.answer_formats).toEqual(["選擇題", "是非題"]);
    expect(after.ai_student_tasks).toEqual(["理解內容", "推論／預測"]);
  });

  it("產生程序鷹架 fills an empty field and offers, never forces, steps over what the teacher wrote", async () => {
    signIn();
    const api = mockApi(
      routes(sopState(), {
        [`POST ${SOP}/procedural-steps`]: { steps: ["Read.", "Answer."], text: "1. Read.\n2. Answer." },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段問題或任務/);
    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    await user.click(p1.getByRole("button", { name: "自己寫第 1 段的問題或任務" }));
    await user.type(screen.getByRole("textbox", { name: "問題或任務" }), "Why?");
    await user.click(screen.getByRole("button", { name: "產生程序鷹架" }));
    expect(api.lastBody("POST", `${SOP}/procedural-steps`)).toEqual({
      paragraph_ids: ["p1"],
      student_tasks: [],
      answer_formats: [],
    });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "程序鷹架" })).toHaveValue("1. Read.\n2. Answer."));
    // With her own steps there, the program's steps are only offered.
    const field = screen.getByRole("textbox", { name: "程序鷹架" });
    await user.clear(field);
    await user.type(field, "自己的步驟");
    await user.click(screen.getByRole("button", { name: "產生程序鷹架" }));
    const offer = within(await screen.findByRole("group", { name: "系統產生的程序鷹架" }));
    expect(field).toHaveValue("自己的步驟");
    await user.click(offer.getByRole("button", { name: "保留我的" }));
    expect(field).toHaveValue("自己的步驟");
    await user.click(screen.getByRole("button", { name: "產生程序鷹架" }));
    await user.click(within(await screen.findByRole("group", { name: "系統產生的程序鷹架" })).getByRole("button", { name: "用這個" }));
    expect(field).toHaveValue("1. Read.\n2. Answer.");
  });
});

// ---------------------------------------------------------------- step 6

describe("step 6: 暖身", () => {
  const output: SopWarmupSuggestion = {
    candidates_available: 5,
    levels_available: true,
    background: [
      {
        text: "Look at the title.\n- What is an umbrella stand for?",
        scaffolds: { verbal: "", instructional: "", procedural: "1. Think.\n2. Share." },
        purpose: "喚醒傘的經驗",
      },
    ],
    vocabulary: [
      { word: "umbrella", level: 2, paragraph_id: "p1", evidence: "Mrs. Lin put an umbrella stand outside her shop.", definition_en: "a thing for rain" },
      { word: "neighbors", level: null, paragraph_id: "p2", evidence: "Her neighbors laughed.", definition_en: "people near you" },
    ],
  };

  it("activities and key words come with levels from the word list; adopting adds only what is missing", async () => {
    signIn();
    const whole = job("warmup", output, {}, 91);
    const api = mockApi(
      routes(
        sopState(
          {
            ...emptyContent(),
            warmup: { background: [], vocabulary: [{ id: "v0", word: "Umbrella", note: "我的", origin: "teacher" }] },
          },
          { warmup: whole },
        ),
        { "POST /api/ai/91/decision": { ok: true } },
      ),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /暖身/);
    const activity = within(screen.getByRole("group", { name: "Manna 建議：暖身活動 1" }));
    await user.click(activity.getByRole("button", { name: "採用" }));
    const words = within(screen.getByRole("group", { name: "Manna 建議：關鍵字彙" }));
    // The level is the word list's: a word it does not have says so.
    expect(words.getByText("大考中心詞彙表第 2 級")).toBeInTheDocument();
    expect(words.getByText("詞彙表裡沒有這個字")).toBeInTheDocument();
    await user.click(words.getByRole("button", { name: "採用" }));
    await settleAutosave();
    const saved = lastSaved(api).warmup;
    expect(saved.background.map((a) => [a.text.split("\n")[0], a.origin, a.scaffolds.procedural])).toEqual([
      ["Look at the title.", "ai_adopted", "1. Think.\n2. Share."],
    ]);
    // "Umbrella" was hers already: only the other word was added.
    expect(saved.vocabulary.map((v) => [v.word, v.origin])).toEqual([
      ["Umbrella", "teacher"],
      ["neighbors", "ai_adopted"],
    ]);
    expect(api.callsTo("POST", "/api/ai/91/decision").map((c) => c.body)).toEqual([
      { item_index: 0, decision: "adopt" },
      { item_index: 100, decision: "adopt" },
    ]);
  });
});

// ---------------------------------------------------------------- step 7 and the worksheet

describe("step 7: Exit ticket and 產生學習單", () => {
  const output: SopExitSuggestion = {
    uncovered_objective_ids: ["o-2"],
    items: [
      { kind: "formative", text: "Why did Mrs. Lin put the stand outside?", objective_id: "o-1", answer: "To help people." },
      { kind: "reflection", text: "What was hardest today?", objective_id: "", answer: "" },
    ],
  };

  it("items link to a real objective with a reference answer; 修改後採用 opens the field; the choices are the worksheet's", async () => {
    signIn();
    const whole = job("exit_ticket", output, {}, 91);
    const api = mockApi(routes(sopState(undefined, { exit_ticket: whole }), { "POST /api/ai/91/decision": { ok: true } }));
    const user = userEvent.setup();
    await open();
    await goToStep(user, /Exit ticket/);
    expect(screen.getByText(/這次沒有檢核到：語言目標 1/)).toBeInTheDocument();
    const first = within(screen.getByRole("group", { name: "Manna 建議：形成性評量" }));
    expect(first.getByText("To help people.")).toBeInTheDocument();
    await user.click(first.getByRole("button", { name: "採用" }));
    await user.click(within(screen.getByRole("group", { name: "Manna 建議：學生反思" })).getByRole("button", { name: "不用" }));
    await settleAutosave();
    expect(lastSaved(api).exit_ticket.items).toEqual([
      {
        id: expect.any(String),
        kind: "formative",
        text: "Why did Mrs. Lin put the stand outside?",
        origin: "ai_adopted",
        objective_id: "o-1",
        answer: "To help people.",
      },
    ]);
    expect(api.callsTo("POST", "/api/ai/91/decision").map((c) => c.body)).toEqual([
      { item_index: 0, decision: "adopt" },
      { item_index: 1, decision: "reject" },
    ]);

    // 產生設定: the generator's options, pre-filled; turning the exit ticket off is saved in the SOP.
    const panel = within(screen.getByRole("region", { name: "產生設定" }));
    await user.click(panel.getByRole("checkbox", { name: "學習單最後放 Exit ticket" }));
    await user.click(panel.getByRole("checkbox", { name: /顯示學習目標/ }));
    await settleAutosave();
    const options = lastSaved(api).options;
    expect(options).toMatchObject({ exit_ticket: false, generation: ["加入暖身活動", "顯示學習目標"] });
  });

  it("產生學習單 shows what it will use; 產生 makes and starts the worksheet without asking again", async () => {
    signIn();
    const api = mockApi(
      routes(
        {
          ...sopState(),
          worksheet_settings: settings({
            student_tasks: ["理解內容"],
            answer_formats: ["簡答題"],
            skip_exit_ticket: true,
          }),
        },
        {
          "POST /api/worksheets": { id: 12 },
          "POST /api/worksheets/12/generate": { id: 12 },
          "GET /api/worksheets/12": { status: 404, body: { error: { code: "not_found", message: "測試不載入" } } },
        },
      ),
    );
    const user = userEvent.setup();
    const { router } = await open();
    await goToStep(user, /Exit ticket/);
    await user.click(within(document.querySelector(".stage-nav") as HTMLElement).getByRole("button", { name: "產生學習單" }));
    const dialog = within(await screen.findByRole("dialog", { name: "用這些設定產生學習單" }));
    expect(dialog.getByText("理解內容")).toBeInTheDocument();
    expect(dialog.getByText("簡答題")).toBeInTheDocument();
    expect(dialog.getByText("不放")).toBeInTheDocument();
    await user.click(dialog.getByRole("button", { name: "產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/worksheets/12"));
    expect(api.lastBody("POST", "/api/worksheets")).toEqual({ lesson_id: 7 });
    expect(api.callsTo("POST", "/api/worksheets/12/generate")).toHaveLength(1);
  });

  it("step 1 has the material types, step 3 顯示學習目標, step 5 the scaffolds' options, step 6 暖身: all saved in the SOP", async () => {
    signIn();
    const api = mockApi(routes(sopState()));
    const user = userEvent.setup();
    await open();
    await user.click(screen.getByRole("checkbox", { name: /資料／數據/ }));
    await goToStep(user, /內容目標與語言目標/);
    await user.click(screen.getByRole("checkbox", { name: /顯示學習目標/ }));
    await goToStep(user, /逐段問題或任務/);
    await user.click(screen.getByRole("checkbox", { name: /提供句型框架/ }));
    await user.click(screen.getByRole("checkbox", { name: "進階版" }));
    await goToStep(user, /暖身/);
    await user.click(screen.getByRole("checkbox", { name: /加入暖身活動/ }));
    await settleAutosave();
    const o = lastSaved(api).options;
    expect(o?.material_types).toEqual(["閱讀文本", "資料／數據"]);
    expect(o?.levels).toEqual(["standard", "advanced"]);
    expect(o?.generation).toEqual(["顯示學習目標", "提供句型框架"]);
  });
});

// ---------------------------------------------------------------- 換一個 on every suggestion (steps 2, 5, 6, 7)

/** A 換一個 job as the server starts it (running) and as it ends (done, with `output` or a status). */
function redoPair<T>(
  kind: SopJobKind,
  parent: SopJob,
  slot: string,
  outcome: T | { status: "blocked"; message: string },
  interaction: number,
) {
  const started = job<T>(kind, null, { slot, parent_job_id: parent.id });
  const result: AiResult<T> =
    outcome !== null && typeof outcome === "object" && "status" in outcome
      ? { interaction_id: interaction, status: "blocked", message: outcome.message, output: null, sources: [], context: [] }
      : { interaction_id: interaction, status: "shown", message: null, output: outcome as T, sources: [], context: [] };
  const done: SopJob<T> = { ...started, status: "done", phase: "", finished_at: new Date().toISOString(), result };
  return { started, done };
}

const decisions = (api: ReturnType<typeof mockApi>, interaction: number) =>
  api.callsTo("POST", `/api/ai/${interaction}/decision`).map((c) => c.body);

describe("step 2: 這一段再分析一次 and 換一個 on the whole text", () => {
  const analysis = (mainIdea: string, p1Function: string): SopAnalysisSuggestion => ({
    text_type: "記敘文",
    main_idea: mainIdea,
    structure: "開場 → 轉折",
    language_focus: [],
    paragraphs: [
      { paragraph_id: "p1", paragraph_no: 1, function: p1Function, summary: "店門口的傘架", features: [] },
      { paragraph_id: "p2", paragraph_no: 2, function: "預測", summary: "鄰居不看好", features: [] },
    ],
    missing_paragraph_ids: [],
    dropped_evidence: 0,
  });

  it("這一段再分析一次 asks for that paragraph, replaces its suggestion only, and the new one is adopted with its own interaction", async () => {
    signIn();
    const whole = job("analysis", analysis("善意會被珍惜", "介紹"), {}, 91);
    const redone: SopAnalysisSuggestion = {
      ...analysis("不會被採用的主旨", "重新介紹"),
      paragraphs: [{ paragraph_id: "p1", paragraph_no: 1, function: "重新介紹", summary: "傘架的由來", features: [] }],
    };
    const { started, done } = redoPair("analysis", whole, "p:p1", redone, 92);
    const api = mockApi(
      routes(sopState(undefined, { analysis: whole }), {
        [`POST ${SOP}/analysis`]: started,
        [`GET /api/sop-jobs/${started.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /分析寫作特色/);
    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    expect(p1.getByText("介紹")).toBeInTheDocument();
    await user.click(p1.getByRole("button", { name: "這一段再分析一次" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/analysis`).map((c) => c.body)).toEqual([
        { paragraph_id: "p1", parent_job_id: whole.id },
      ]),
    );
    await waitFor(() => expect(p1.getByText("重新介紹")).toBeInTheDocument());
    expect(p1.queryByText("介紹")).not.toBeInTheDocument();
    // The second paragraph and the whole-text suggestion stay as they were.
    expect(screen.getByText("預測")).toBeInTheDocument();
    expect(screen.getByText("善意會被珍惜")).toBeInTheDocument();
    expect(screen.queryByText("不會被採用的主旨")).not.toBeInTheDocument();
    await waitFor(() => expect(decisions(api, 91)).toEqual([{ item_index: 1, decision: "reject" }]));

    await user.click(within(p1.getByRole("group", { name: "Manna 建議：第 1 段" })).getByRole("button", { name: "採用" }));
    await waitFor(() => expect(decisions(api, 92)).toEqual([{ item_index: 1, decision: "adopt" }]));
    await settleAutosave();
    expect(lastSaved(api).analysis.paragraphs.map((a) => [a.paragraph_id, a.function, a.origin])).toEqual([
      ["p1", "重新介紹", "ai_adopted"],
    ]);
  });

  it("換一個 on the whole text asks for a new overall analysis and ignores its paragraphs", async () => {
    signIn();
    const whole = job("analysis", analysis("善意會被珍惜", "介紹"), {}, 91);
    const redone: SopAnalysisSuggestion = {
      ...analysis("信任讓社區變好", "不會出現的段落功能"),
      text_type: "說明文",
    };
    const { started, done } = redoPair("analysis", whole, `${whole.id}:0`, redone, 92);
    const api = mockApi(
      routes(sopState(undefined, { analysis: whole }), {
        [`POST ${SOP}/analysis`]: started,
        [`GET /api/sop-jobs/${started.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /分析寫作特色/);
    const well = within(screen.getByRole("group", { name: "Manna 建議：全文" }));
    expect(well.getByText("善意會被珍惜")).toBeInTheDocument();
    await user.click(well.getByRole("button", { name: "換一個" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/analysis`).map((c) => c.body)).toEqual([
        { parent_job_id: whole.id, item_index: 0 },
      ]),
    );
    const again = within(await screen.findByRole("group", { name: "Manna 建議：全文" }));
    await waitFor(() => expect(again.getByText("信任讓社區變好")).toBeInTheDocument());
    expect(screen.queryByText("善意會被珍惜")).not.toBeInTheDocument();
    // The new analysis's paragraphs are not used: paragraph 1 still shows the whole run's.
    expect(screen.queryByText("不會出現的段落功能")).not.toBeInTheDocument();
    expect(screen.getByText("介紹")).toBeInTheDocument();
    await waitFor(() => expect(decisions(api, 91)).toEqual([{ item_index: 0, decision: "reject" }]));

    await user.click(again.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(decisions(api, 92)).toEqual([{ item_index: 0, decision: "adopt" }]));
    await settleAutosave();
    expect(lastSaved(api).analysis).toMatchObject({ text_type: "說明文", main_idea: "信任讓社區變好" });
  });
});

describe("step 5: 換一個 on a block", () => {
  const block = (text: string) => ({
    paragraph_ids: ["p1"],
    paragraph_nos: [1],
    title: "Why the stand?",
    student_tasks: ["理解內容"],
    answer_formats: ["簡答題"],
    objective_ids: ["o-1"],
    reason: "先確認學生讀懂規則",
    tasks: [
      {
        function: "discover" as const,
        text,
        answer_format: "簡答題",
        options: [],
        answer: "To help people",
        evidence: "",
        scaffolds: { verbal: "", instructional: "", procedural: "" },
      },
    ],
  });
  const suggestion = (text: string): SopTasksSuggestion => ({
    scaffolds_ok: true,
    missing_paragraph_ids: [],
    blocks: [block(text)],
  });

  it("asks for that block with the paragraphs it covers, shows the new one in place, and a second 換一個 chains from the redo job", async () => {
    signIn();
    const whole = job("tasks", suggestion("Why did Mrs. Lin put the stand outside?"), {}, 91);
    const first = redoPair("tasks", whole, `${whole.id}:0`, suggestion("Who could take an umbrella?"), 92);
    const second = redoPair("tasks", first.done, `${first.done.id}:0`, suggestion("What did the sign say?"), 93);
    let posts = 0;
    const api = mockApi(
      routes(sopState(undefined, { tasks: whole }), {
        [`POST ${SOP}/tasks`]: () => {
          posts += 1;
          return posts === 1 ? first.started : second.started;
        },
        [`GET /api/sop-jobs/${first.started.id}`]: first.done,
        [`GET /api/sop-jobs/${second.started.id}`]: second.done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
        "POST /api/ai/93/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段問題或任務/);
    const p1 = within(screen.getByRole("region", { name: "第 1 段" }));
    await user.click(within(p1.getByRole("group", { name: "Manna 建議：第 1 段" })).getByRole("button", { name: "換一個" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/tasks`).map((c) => c.body)).toEqual([{ parent_job_id: whole.id, item_index: 0 }]),
    );
    await waitFor(() => expect(p1.getByText("Who could take an umbrella?")).toBeInTheDocument());
    expect(p1.queryByText("Why did Mrs. Lin put the stand outside?")).not.toBeInTheDocument();
    await waitFor(() => expect(decisions(api, 91)).toEqual([{ item_index: 0, decision: "reject" }]));

    // Again: the redo job is the parent now.
    await user.click(within(p1.getByRole("group", { name: "Manna 建議：第 1 段" })).getByRole("button", { name: "換一個" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/tasks`).map((c) => c.body)).toEqual([
        { parent_job_id: whole.id, item_index: 0 },
        { parent_job_id: first.done.id, item_index: 0 },
      ]),
    );
    await waitFor(() => expect(p1.getByText("What did the sign say?")).toBeInTheDocument());
    await waitFor(() => expect(decisions(api, 92)).toEqual([{ item_index: 0, decision: "reject" }]));

    await user.click(within(p1.getByRole("group", { name: "Manna 建議：第 1 段" })).getByRole("button", { name: "採用" }));
    await waitFor(() => expect(decisions(api, 93)).toEqual([{ item_index: 0, decision: "adopt" }]));
    await settleAutosave();
    expect(lastSaved(api).task_blocks.flatMap((b) => b.tasks.map((t) => [t.text, t.origin]))).toEqual([
      ["What did the sign say?", "ai_adopted"],
    ]);
  });
});

describe("step 6: 換一個 on an activity and on the word group", () => {
  const empty = { text: "", scaffolds: { verbal: "", instructional: "", procedural: "" }, purpose: "" };
  const whole = (): SopWarmupSuggestion => ({
    candidates_available: 5,
    levels_available: true,
    background: [{ ...empty, text: "Look at the title.", purpose: "喚醒傘的經驗" }],
    vocabulary: [
      { word: "umbrella", level: 2, paragraph_id: "p1", evidence: "an umbrella stand", definition_en: "a thing for rain" },
    ],
  });

  it("an activity: replaced in place, the word group untouched, adopted with the redo job's interaction", async () => {
    signIn();
    const parent = job("warmup", whole(), {}, 91);
    const redone: SopWarmupSuggestion = {
      ...whole(),
      background: [{ ...empty, text: "Share a time you borrowed an umbrella.", purpose: "換個角度" }],
      vocabulary: [],
    };
    const { started, done } = redoPair("warmup", parent, `${parent.id}:0`, redone, 92);
    const api = mockApi(
      routes(sopState(undefined, { warmup: parent }), {
        [`POST ${SOP}/warmup`]: started,
        [`GET /api/sop-jobs/${started.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /暖身/);
    await user.click(within(screen.getByRole("group", { name: "Manna 建議：暖身活動 1" })).getByRole("button", { name: "換一個" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/warmup`).map((c) => c.body)).toEqual([{ parent_job_id: parent.id, item_index: 0 }]),
    );
    await waitFor(() => expect(screen.getByText("Share a time you borrowed an umbrella.")).toBeInTheDocument());
    expect(screen.queryByText("Look at the title.")).not.toBeInTheDocument();
    // The redo job has no words: the whole run's group is still there.
    expect(screen.getByRole("group", { name: "Manna 建議：關鍵字彙" })).toBeInTheDocument();
    await waitFor(() => expect(decisions(api, 91)).toEqual([{ item_index: 0, decision: "reject" }]));

    await user.click(within(screen.getByRole("group", { name: "Manna 建議：暖身活動 1" })).getByRole("button", { name: "採用" }));
    await waitFor(() => expect(decisions(api, 92)).toEqual([{ item_index: 0, decision: "adopt" }]));
    await settleAutosave();
    expect(lastSaved(api).warmup.background.map((a) => [a.text, a.origin])).toEqual([
      ["Share a time you borrowed an umbrella.", "ai_adopted"],
    ]);
  });

  it("the word group: asked with item_index 100, replaced in place, adopted as 100 of the redo job", async () => {
    signIn();
    const parent = job("warmup", whole(), {}, 91);
    const redone: SopWarmupSuggestion = {
      ...whole(),
      background: [],
      vocabulary: [{ word: "stand", level: 1, paragraph_id: "p1", evidence: "an umbrella stand", definition_en: "a small table" }],
    };
    const { started, done } = redoPair("warmup", parent, `${parent.id}:100`, redone, 92);
    const api = mockApi(
      routes(sopState(undefined, { warmup: parent }), {
        [`POST ${SOP}/warmup`]: started,
        [`GET /api/sop-jobs/${started.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /暖身/);
    const words = within(screen.getByRole("group", { name: "Manna 建議：關鍵字彙" }));
    expect(words.getByText("umbrella")).toBeInTheDocument();
    await user.click(words.getByRole("button", { name: "換一個" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/warmup`).map((c) => c.body)).toEqual([{ parent_job_id: parent.id, item_index: 100 }]),
    );
    await waitFor(() => expect(screen.getByText("a small table")).toBeInTheDocument());
    expect(screen.queryByText("a thing for rain")).not.toBeInTheDocument();
    // The activity suggestion stays.
    expect(screen.getByText("Look at the title.")).toBeInTheDocument();
    await waitFor(() => expect(decisions(api, 91)).toEqual([{ item_index: 100, decision: "reject" }]));

    await user.click(within(screen.getByRole("group", { name: "Manna 建議：關鍵字彙" })).getByRole("button", { name: "採用" }));
    await waitFor(() => expect(decisions(api, 92)).toEqual([{ item_index: 100, decision: "adopt" }]));
    await settleAutosave();
    expect(lastSaved(api).warmup.vocabulary.map((v) => [v.word, v.origin])).toEqual([["stand", "ai_adopted"]]);
  });

  it("when the material has no other words, the redo ends blocked: Manna's message is shown with 再試一次", async () => {
    signIn();
    const parent = job("warmup", whole(), {}, 91);
    const { started, done } = redoPair(
      "warmup",
      parent,
      `${parent.id}:100`,
      { status: "blocked", message: "教材裡沒有其他適合的候選字了。" },
      92,
    );
    const api = mockApi(
      routes(sopState(undefined, { warmup: parent }), {
        [`POST ${SOP}/warmup`]: started,
        [`GET /api/sop-jobs/${started.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /暖身/);
    await user.click(within(screen.getByRole("group", { name: "Manna 建議：關鍵字彙" })).getByRole("button", { name: "換一個" }));
    expect(await screen.findByText("教材裡沒有其他適合的候選字了。")).toBeInTheDocument();
    expect(screen.queryByText("a thing for rain")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再試一次" })).toBeInTheDocument();
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/warmup`).map((c) => c.body)).toEqual([{ parent_job_id: parent.id, item_index: 100 }]),
    );
  });

  it("a group that took every candidate word offers no 換一個 (the activity still does)", async () => {
    signIn();
    const parent = job("warmup", { ...whole(), more_words: false }, {}, 91);
    mockApi(routes(sopState(undefined, { warmup: parent })));
    const user = userEvent.setup();
    await open();
    await goToStep(user, /暖身/);
    const words = within(await screen.findByRole("group", { name: "Manna 建議：關鍵字彙" }));
    expect(words.getByRole("button", { name: "採用" })).toBeInTheDocument();
    expect(words.queryByRole("button", { name: "換一個" })).not.toBeInTheDocument();
    const activity = within(screen.getByRole("group", { name: "Manna 建議：暖身活動 1" }));
    expect(activity.getByRole("button", { name: "換一個" })).toBeInTheDocument();
  });
});

describe("step 7: 換一個 on an item", () => {
  const output: SopExitSuggestion = {
    uncovered_objective_ids: [],
    items: [
      { kind: "formative", text: "Why did Mrs. Lin put the stand outside?", objective_id: "o-1", answer: "To help people." },
      { kind: "reflection", text: "What was hardest today?", objective_id: "", answer: "" },
    ],
  };

  it("asks for that item only, shows the new one in place, adopts it with the redo job's interaction", async () => {
    signIn();
    const whole = job("exit_ticket", output, {}, 91);
    const redone: SopExitSuggestion = {
      uncovered_objective_ids: [],
      items: [{ kind: "formative", text: "What did the sign say?", objective_id: "o-1", answer: "Take one." }],
    };
    const { started, done } = redoPair("exit_ticket", whole, `${whole.id}:0`, redone, 92);
    const api = mockApi(
      routes(sopState(undefined, { exit_ticket: whole }), {
        [`POST ${SOP}/exit-ticket`]: started,
        [`GET /api/sop-jobs/${started.id}`]: done,
        "POST /api/ai/91/decision": { ok: true },
        "POST /api/ai/92/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /Exit ticket/);
    await user.click(within(screen.getByRole("group", { name: "Manna 建議：形成性評量" })).getByRole("button", { name: "換一個" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/exit-ticket`).map((c) => c.body)).toEqual([
        { parent_job_id: whole.id, item_index: 0 },
      ]),
    );
    await waitFor(() => expect(screen.getByText("What did the sign say?")).toBeInTheDocument());
    expect(screen.queryByText("Why did Mrs. Lin put the stand outside?")).not.toBeInTheDocument();
    // The other item is where it was.
    expect(screen.getByText("What was hardest today?")).toBeInTheDocument();
    await waitFor(() => expect(decisions(api, 91)).toEqual([{ item_index: 0, decision: "reject" }]));

    await user.click(within(screen.getByRole("group", { name: "Manna 建議：形成性評量" })).getByRole("button", { name: "採用" }));
    await waitFor(() => expect(decisions(api, 92)).toEqual([{ item_index: 0, decision: "adopt" }]));
    await settleAutosave();
    expect(lastSaved(api).exit_ticket.items.map((i) => [i.text, i.origin, i.answer])).toEqual([
      ["What did the sign say?", "ai_adopted", "Take one."],
    ]);
  });
});

// ---------------------------------------------------------------- 幫我修英文 in steps 5, 6, 7

describe("幫我修英文 in steps 5, 6, 7", () => {
  const reply = (id: number, english: string, changed = true) => ({
    interaction_id: id,
    status: "shown",
    message: null,
    output: { english, changed, note: "改了動詞。" },
    sources: [],
    context: [],
  });
  const noScaffolds = { verbal: "", instructional: "", procedural: "" };

  /** Two asks: the first is kept (保留我的), the second is taken (用這個). */
  function textEnglishRoutes(first: string, second: string) {
    let calls = 0;
    return {
      [`POST ${SOP}/text-english`]: () => {
        calls += 1;
        return calls === 1 ? reply(71, first) : reply(72, second);
      },
      "POST /api/ai/71/decision": { ok: true },
      "POST /api/ai/72/decision": { ok: true },
    };
  }

  it("step 5: a task, with its paragraphs; 保留我的 keeps the text, 用這個 replaces it", async () => {
    signIn();
    const content: SopContent = {
      ...emptyContent(),
      objectives,
      task_blocks: [
        {
          id: "b1",
          paragraph_ids: ["p1"],
          student_tasks: [],
          answer_formats: [],
          ai_student_tasks: [],
          ai_answer_formats: [],
          tasks: [
            {
              id: "t1",
              function: "discover",
              text: "Why Mrs. Lin put stand?",
              scaffolds: noScaffolds,
              origin: "teacher",
              answer_format: "",
              options: [],
              answer: "",
              evidence: "",
            },
          ],
        },
      ],
    };
    const api = mockApi(
      routes(sopState(content), textEnglishRoutes("Why did Mrs. Lin put the stand?", "Why did Mrs. Lin put the stand outside?\nFind the sentence.")),
    );
    const user = userEvent.setup();
    await open();
    await goToStep(user, /逐段問題或任務/);
    await user.click(screen.getByRole("button", { name: "修改「Why Mrs. Lin put stand?」" }));
    const editor = within(screen.getByRole("group", { name: "問題或任務" }));
    await user.click(editor.getByRole("button", { name: "幫我修英文" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/text-english`).map((c) => c.body)).toEqual([
        { field: "task", text: "Why Mrs. Lin put stand?", paragraph_ids: ["p1"] },
      ]),
    );
    const proposal = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    expect(proposal.getByText("Why Mrs. Lin put stand?")).toBeInTheDocument();
    expect(proposal.getByText("Why did Mrs. Lin put the stand?")).toBeInTheDocument();
    await user.click(proposal.getByRole("button", { name: "保留我的" }));
    expect(editor.getByRole("textbox", { name: "問題或任務" })).toHaveValue("Why Mrs. Lin put stand?");
    expect(screen.queryByRole("group", { name: "Manna 建議的英文" })).not.toBeInTheDocument();

    await user.click(editor.getByRole("button", { name: "幫我修英文" }));
    const second = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    await user.click(second.getByRole("button", { name: "用這個" }));
    expect(editor.getByRole("textbox", { name: "問題或任務" })).toHaveValue(
      "Why did Mrs. Lin put the stand outside?\nFind the sentence.",
    );
    await settleAutosave();
    expect(lastSaved(api).task_blocks[0]!.tasks[0]).toMatchObject({
      text: "Why did Mrs. Lin put the stand outside?\nFind the sentence.",
      origin: "ai_edited",
    });
    await waitFor(() => expect(decisions(api, 71)).toEqual([{ item_index: 0, decision: "reject" }]));
    await waitFor(() => expect(decisions(api, 72)).toEqual([{ item_index: 0, decision: "adopt" }]));
  });

  it("step 6: a warm-up activity; the button waits for some text", async () => {
    signIn();
    const content: SopContent = {
      ...emptyContent(),
      objectives,
      warmup: { background: [{ id: "w1", text: "", scaffolds: noScaffolds, origin: "teacher" }], vocabulary: [] },
    };
    const api = mockApi(routes(sopState(content), textEnglishRoutes("Look at the title.", "Look at the title and guess.")));
    const user = userEvent.setup();
    await open();
    await goToStep(user, /暖身/);
    const activity = within(screen.getByRole("group", { name: "暖身活動 1" }));
    expect(activity.getByRole("button", { name: "幫我修英文" })).toBeDisabled();
    await user.type(activity.getByRole("textbox", { name: "暖身活動 1" }), "look at title");
    await user.click(activity.getByRole("button", { name: "幫我修英文" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/text-english`).map((c) => c.body)).toEqual([{ field: "warmup", text: "look at title" }]),
    );
    const proposal = within(await screen.findByRole("group", { name: "Manna 建議的英文" }));
    await user.click(proposal.getByRole("button", { name: "保留我的" }));
    expect(activity.getByRole("textbox", { name: "暖身活動 1" })).toHaveValue("look at title");

    await user.click(activity.getByRole("button", { name: "幫我修英文" }));
    await user.click(within(await screen.findByRole("group", { name: "Manna 建議的英文" })).getByRole("button", { name: "用這個" }));
    expect(activity.getByRole("textbox", { name: "暖身活動 1" })).toHaveValue("Look at the title and guess.");
    await settleAutosave();
    expect(lastSaved(api).warmup.background.map((a) => [a.text, a.origin])).toEqual([
      ["Look at the title and guess.", "ai_edited"],
    ]);
    await waitFor(() => expect(decisions(api, 71)).toEqual([{ item_index: 0, decision: "reject" }]));
    await waitFor(() => expect(decisions(api, 72)).toEqual([{ item_index: 0, decision: "adopt" }]));
  });

  it("step 7: an exit-ticket item", async () => {
    signIn();
    const content: SopContent = {
      ...emptyContent(),
      objectives,
      exit_ticket: {
        items: [{ id: "e1", kind: "reflection", text: "what hardest today", origin: "teacher", objective_id: "", answer: "" }],
      },
    };
    const api = mockApi(routes(sopState(content), textEnglishRoutes("What was hardest today?", "What was the hardest part today?")));
    const user = userEvent.setup();
    await open();
    await goToStep(user, /Exit ticket/);
    const item = within(screen.getByRole("group", { name: "Exit ticket 第 1 題" }));
    await user.click(item.getByRole("button", { name: "幫我修英文" }));
    await waitFor(() =>
      expect(api.callsTo("POST", `${SOP}/text-english`).map((c) => c.body)).toEqual([{ field: "exit", text: "what hardest today" }]),
    );
    await user.click(within(await screen.findByRole("group", { name: "Manna 建議的英文" })).getByRole("button", { name: "保留我的" }));
    expect(item.getByRole("textbox", { name: "題目" })).toHaveValue("what hardest today");

    await user.click(item.getByRole("button", { name: "幫我修英文" }));
    await user.click(within(await screen.findByRole("group", { name: "Manna 建議的英文" })).getByRole("button", { name: "用這個" }));
    expect(item.getByRole("textbox", { name: "題目" })).toHaveValue("What was the hardest part today?");
    await settleAutosave();
    expect(lastSaved(api).exit_ticket.items.map((i) => [i.text, i.origin])).toEqual([
      ["What was the hardest part today?", "ai_edited"],
    ]);
    await waitFor(() => expect(decisions(api, 71)).toEqual([{ item_index: 0, decision: "reject" }]));
    await waitFor(() => expect(decisions(api, 72)).toEqual([{ item_index: 0, decision: "adopt" }]));
  });
});
