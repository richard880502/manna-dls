// 差異化學習單 (docs/SIOP_DESIGN_RULES.md 三, formerly 分層練習): the student sheets carry a neutral
// code (Version A／B／C, never 基礎／一般／進階, T9), every question shows the support of the version
// shown (T3), and the teacher sees each version's answer and scoring note (T6).
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  PrepTurn,
  SaveWorksheetRequest,
  WorksheetContent,
  WorksheetState,
  WsAudience,
  WsItem,
  WsItemTier,
  WsLevel,
  WsSheet,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings, emptyWorksheetContent, versionCode } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn, type MockCall } from "../test/mockApi";
import { openBarPopover } from "../test/menus";
const WS = "/api/worksheets/5";
const LEVEL_NAMES = ["基礎", "一般版", "進階"];

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

const tier = (level: WsLevel, extra: Partial<WsItemTier>): WsItemTier => ({
  level,
  support_kind: "none",
  model_sentence: "",
  support: [],
  word_bank: [],
  answer: "",
  scoring: "",
  ...extra,
});

const question: WsItem = {
  id: "q1",
  answer_format: "簡答題",
  prompt: "What did the neighbors think would happen?",
  options: [],
  answer: "They were sure every umbrella would be gone.",
  evidence: "",
  purpose: null,
  levels: ["basic", "standard", "advanced"],
  origin: "ai",
  bloom: "understand",
  hint: "",
  tiers: [
    tier("basic", {
      support_kind: "sentence_frame",
      model_sentence: "The neighbors thought the umbrellas would be gone.",
      support: ["The neighbors thought the ___ would be ___."],
      word_bank: ["umbrellas", "gone"],
      answer: "umbrellas, gone",
      scoring: "片語即可給分",
    }),
    tier("standard", {
      support_kind: "sentence_starter",
      support: ["The neighbors thought that …"],
      answer: "They thought the umbrellas would be gone.",
      scoring: "一個完整句",
    }),
    tier("advanced", { answer: "They were sure every umbrella would be gone.", scoring: "1–2 個完整句" }),
  ],
};

function content(): WorksheetContent {
  return {
    ...emptyWorksheetContent(),
    show_objectives: true,
    objectives: [
      {
        id: "o1",
        type: "content",
        text: "說明鄰居的想法",
        student_facing_en: "I can explain what the neighbors thought.",
        origin: "teacher",
        sop_id: null,
      },
    ],
    levels: ["basic", "standard", "advanced"],
    blocks: [
      {
        id: "b1",
        kind: "activity",
        enabled: true,
        title: "Understand the Text",
        instructions: "",
        paragraph_ids: [],
        info_function: null,
        student_tasks: [],
        objective_ids: [],
        vocabulary: [],
        items: [question],
        scaffolds: [],
        teacher_notes: "",
        origin: "ai",
        message: null,
        sop_block_id: null,
      },
    ],
  };
}

function wsState(): WorksheetState {
  return {
    id: 5,
    title: "The Umbrella Stand",
    filename: "The Umbrella Stand_學習單",
    lesson: null,
    class_id: null,
    generated: true,
    levels: ["basic", "standard", "advanced"],
    version_no: 2,
    created_at: "2026-11-16T08:00:00Z",
    updated_at: "2026-11-16T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: { ...emptySettings(), output: "tiered" },
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-16T08:00:00Z",
    content: content(),
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    request: "",
  } as WorksheetState;
}

describe("差異化學習單: neutral codes and per-version support", () => {
  it("the code follows the worksheet's versions", () => {
    expect(versionCode(["basic", "standard", "advanced"], "basic")).toBe("A");
    expect(versionCode(["basic", "standard", "advanced"], "advanced")).toBe("C");
    expect(versionCode(["standard", "advanced"], "standard")).toBe("A");
    expect(versionCode(["standard"], "standard")).toBeNull();
    // A stored students' order (random for a 差異化學習單) decides the letters …
    const order = ["advanced", "basic", "standard"] as const;
    expect(versionCode(["basic", "standard", "advanced"], "basic", order)).toBe("B");
    expect(versionCode(["basic", "standard", "advanced"], "advanced", order)).toBe("A");
    // … unless it doesn't list exactly the worksheet's versions.
    expect(versionCode(["basic", "standard", "advanced"], "basic", ["advanced", "basic"])).toBe("A");
  });

  it("the editor names a student sheet by its code and edits the version's support and scoring", async () => {
    const state = wsState();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheets": [],
      [`GET ${WS}`]: state,
      [`PUT ${WS}`]: (call: MockCall) => ({ ...state, ...(call.body as SaveWorksheetRequest) }),
    });
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    // 基礎版 tab, student view: printed as Version A, with its sentence frame.
    const sheetA = await screen.findByRole("article", { name: "A 版 學生版" });
    expect(within(sheetA).getByText("Version A")).toBeInTheDocument();
    const support = within(sheetA).getByRole("group", { name: "基礎版的支持 1" });
    expect(within(support).getByRole("combobox", { name: "支持形式 1" })).toHaveValue("sentence_frame");
    expect(within(support).getByRole("textbox", { name: "支持 1（每行一項）" })).toHaveValue(
      "The neighbors thought the ___ would be ___.",
    );
    expect(within(support).getByRole("textbox", { name: "字庫 1（用逗號分開）" })).toHaveValue("umbrellas, gone");
    // No hint field for a question with versions of its own.
    expect(within(sheetA).queryByText("提示（只印在基礎版）")).not.toBeInTheDocument();

    // 進階版: Version C, no support.
    await user.click(within(screen.getByRole("tablist", { name: "版本" })).getByRole("tab", { name: "進階版" }));
    const sheetC = await screen.findByRole("article", { name: "C 版 學生版" });
    expect(within(sheetC).getByText("Version C")).toBeInTheDocument();
    expect(within(sheetC).getByRole("combobox", { name: "支持形式 1" })).toHaveValue("none");
    expect(within(sheetC).queryByRole("textbox", { name: "支持 1（每行一項）" })).not.toBeInTheDocument();
    expect(screen.getByText(/學生版印成「Version C」，不印程度名稱。/)).toBeInTheDocument();

    // The teacher view: this version's answer and scoring note, edited in its own part.
    await user.click(screen.getByRole("radio", { name: "教師版" }));
    await screen.findByRole("article", { name: "進階版 教師版" });
    expect(screen.getByRole("textbox", { name: "參考答案 1" })).toHaveValue("They were sure every umbrella would be gone.");
    const scoring = screen.getByRole("textbox", { name: "評分說明 1" });
    expect(scoring).toHaveValue("1–2 個完整句");
    await user.type(scoring, "，用自己的話");
    await waitFor(() => {
      const body = api.lastBody("PUT", WS) as SaveWorksheetRequest | undefined;
      const tiers = body?.content?.blocks[0]?.items[0]?.tiers ?? [];
      expect(tiers.find((t) => t.level === "advanced")?.scoring).toBe("1–2 個完整句，用自己的話");
      expect(tiers.find((t) => t.level === "basic")?.scoring).toBe("片語即可給分");
    });
  });

  it("the print preview of the session page prints Version A／B／C and never a level name", async () => {
    const t: PrepTurn = {
      id: 401,
      seq: 1,
      kind: "tiered",
      request: "差異化學習單",
      status: "done",
      error: null,
      worksheet: { id: 11, title: "U", version_no: 2, running: false },
      result: null,
      interaction_id: null,
      created_at: "2026-11-16T08:00:00Z",
      finished_at: "2026-11-16T08:01:00Z",
    };
    const sheet = (call: MockCall): WsSheet => {
      const params = new URL(call.url, "http://x").searchParams;
      const audience = (params.get("audience") ?? "student") as WsAudience;
      const level = (params.get("level") ?? "basic") as WsLevel;
      const teacher = audience === "teacher";
      const v = question.tiers!.find((x) => x.level === level)!;
      return {
        title: "The Umbrella Stand",
        level,
        audience,
        version_code: versionCode(["basic", "standard", "advanced"], level),
        ...(teacher ? { tiering_note: "差異化學習單：三個版本的學習目標、題目與 Bloom 分布相同" } : {}),
        objectives: ["I can explain what the neighbors thought."],
        blocks: [
          {
            id: "b1",
            kind: "activity",
            label: "A",
            title: "Understand the Text",
            instructions: "",
            steps: ["Read the question first.", "Write your answer."],
            vocabulary: [],
            supports: [],
            items: [
              {
                no: 1,
                answer_format: "簡答題",
                prompt: question.prompt,
                options: [],
                lines: 2,
                ...(v.support_kind === "none"
                  ? {}
                  : {
                      support: {
                        kind: v.support_kind,
                        heading: v.support_kind === "sentence_frame" ? "Sentence frame" : "Start like this",
                        lines: v.support,
                        word_bank: [...v.word_bank].sort(),
                      },
                    }),
                ...(teacher ? { answer: v.answer, evidence: "", bloom: "understand" as const, scoring: v.scoring } : {}),
              },
            ],
          },
        ],
      };
    };
    const prepState = { ...wsState(), id: 11, prep_session_id: 7 } as WorksheetState;
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": {
        id: 7,
        title: "The Umbrella Stand",
        material: { id: 5, title: "The Umbrella Stand" },
        topic: null,
        class: null,
        template: null,
        turn_count: 1,
        last_kind: "tiered",
        updated_at: "2026-11-16T09:00:00Z",
        created_at: "2026-11-16T08:00:00Z",
        turns: [t],
        running: false,
        next: [],
      },
      "GET /api/worksheets/11": prepState,
      "GET /api/worksheets/11/sheet": sheet,
    });
    const user = userEvent.setup();
    renderApp("/prep/7");
    const result = within(await screen.findByRole("complementary", { name: "目前的學習單" }));
    // The editor beside the conversation names the version by its neutral code too.
    await result.findByRole("article", { name: "A 版 學生版" });
    // The print preview is the server's projection: what the exports print.
    await user.click((await openBarPopover(user, "更多…")).getByRole("button", { name: "列印樣式預覽" }));
    const dialog = within(await screen.findByRole("dialog", { name: "列印樣式預覽" }));
    const a = await dialog.findByRole("article", { name: "A 版 學生版" });
    expect(within(a).getByText("Version A")).toBeInTheDocument();
    expect(within(a).getByText("Sentence frame:")).toBeInTheDocument();
    expect(within(a).getByText("gone / umbrellas")).toBeInTheDocument();
    for (const name of LEVEL_NAMES) expect(a.textContent).not.toContain(name);
    expect(a.textContent).not.toContain("片語即可給分");

    const tabs = within(dialog.getByRole("tablist", { name: "版本程度" }));
    await user.click(tabs.getByRole("tab", { name: "一般版" }));
    const b = await dialog.findByRole("article", { name: "B 版 學生版" });
    expect(within(b).getByText("Version B")).toBeInTheDocument();
    expect(within(b).getByText("The neighbors thought that …")).toBeInTheDocument();
    for (const name of LEVEL_NAMES) expect(b.textContent).not.toContain(name);

    // The teacher keeps the level name, this version's answer and scoring note, and the 分層說明.
    await user.click(dialog.getByRole("radio", { name: "教師版" }));
    const teacher = await dialog.findByRole("article", { name: "一般版 教師版" });
    expect(within(teacher).getByText("一個完整句")).toBeInTheDocument();
    expect(within(teacher).getByText(/^差異化學習單：三個版本/)).toBeInTheDocument();
  });
});
