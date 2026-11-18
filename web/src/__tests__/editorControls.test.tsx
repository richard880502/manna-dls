// The worksheet editor's controls each change what the sheet shows (and prints), and every change
// is saved and survives a reload. Two reports started this:
// 1. 「選了新題但沒出現」: choosing an answer format (表格整理, 選擇題, 是非題 …) changed only the
//    select — no table, no choices, the same answer lines.
// 2. 「這三個支持，不管選哪個，都沒變」: choosing another support form for a version of a
//    差異化學習單 changed only the select — the same lines stayed under the question.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  SaveWorksheetRequest,
  WorksheetContent,
  WorksheetState,
  WsBlock,
  WsItem,
  WsItemTier,
  WsLevel,
} from "../api/types";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { autosaveConfig } from "../lib/useAutosave";
import {
  answerLines,
  buildSupport,
  columnsFromPrompt,
  emptyBrief,
  emptySettings,
  emptyWorksheetContent,
  newItem,
  tableOf,
  withAnswerFormat,
} from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

const WS = "/api/worksheets/5";

beforeEach(() => {
  signIn();
  autosaveConfig.delayMs = 20;
  worksheetConfig.pollMs = 20;
});
afterEach(() => {
  autosaveConfig.delayMs = 1500;
  worksheetConfig.pollMs = 2000;
});

// ---------------------------------------------------------------- fixtures

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
  bloom: null,
  hint: "",
  ...extra,
});

const block = (id: string, title: string, items: WsItem[], extra: Partial<WsBlock> = {}): WsBlock => ({
  id,
  kind: "activity",
  enabled: true,
  title,
  instructions: "",
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
  ...extra,
});

function state(content: WorksheetContent, extra: Partial<WorksheetState> = {}): WorksheetState {
  return {
    id: 5,
    title: "The Umbrella Stand",
    filename: "The Umbrella Stand_學習單",
    lesson: null,
    class_id: null,
    generated: true,
    levels: content.levels,
    version_no: 2,
    created_at: "2026-11-18T08:00:00Z",
    updated_at: "2026-11-18T08:00:00Z",
    material: {
      title: "The Umbrella Stand",
      paragraphs: [{ id: "p1", text: "Mrs. Lin put an umbrella stand outside her shop." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 1,
    saved_at: "2026-11-18T08:00:00Z",
    content,
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    request: "",
    ...extra,
  };
}

const plain = (): WorksheetContent => ({
  ...emptyWorksheetContent(),
  show_objectives: false,
  levels: ["basic", "standard"],
  blocks: [
    block("b1", "Find the Facts", [
      item("q1", "Who put the umbrella stand outside?", { bloom: "remember" }),
      item("q2", "Which sentence comes from paragraph 1?", {
        answer_format: "選擇題",
        options: ["Mrs. Lin put a stand outside.", "Nobody laughed.", "It never rained.", "The shop closed."],
        bloom: "remember",
      }),
    ]),
    block("b2", "Look Closer", [item("q3", "Why did the neighbors laugh?", { bloom: "analyze" })]),
    block("b3", "Think Further", [item("q4", "What would you do?", { bloom: "analyze" })]),
  ],
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

/** The question of the second report: 基礎版 has a frame cut from its model sentence, 一般版 a
 * starter written without one. */
const tieredContent = (): WorksheetContent => ({
  ...emptyWorksheetContent(),
  levels: ["basic", "standard", "advanced"],
  blocks: [
    block("t1", "Understand the Text", [
      item("q1", "What did the neighbors think would happen?", {
        levels: ["basic", "standard", "advanced"],
        bloom: "understand",
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
          }),
          tier("advanced", { answer: "They were sure every umbrella would be gone." }),
        ],
      }),
    ]),
  ],
});

/**
 * A server that keeps what it is sent: a save is checked against the version it was edited from
 * (409 when another version came in between, like the real one) and a reload gets the latest.
 */
function server(initial: WorksheetState) {
  let current = initial;
  const saves: SaveWorksheetRequest[] = [];
  const api = mockApi({
    ...baseRoutes(),
    "GET /api/worksheets": [],
    [`GET ${WS}`]: () => current,
    [`PUT ${WS}`]: (call: MockCall) => {
      const body = call.body as SaveWorksheetRequest;
      saves.push(body);
      if (body.content && body.base_version_no !== current.version_no) {
        return respond(409, { error: { code: "conflict", message: "請重新載入" } });
      }
      const { base_version_no: _base, ...rest } = body;
      current = {
        ...current,
        ...(rest as Partial<WorksheetState>),
        content: body.content ?? current.content,
        version_no: body.content ? current.version_no + 1 : current.version_no,
        saved_at: "2026-11-18T09:30:00Z",
      };
      return current;
    },
  });
  return { api, saves, now: () => current };
}

async function settle() {
  window.dispatchEvent(new Event("blur"));
  await screen.findByText(/^已自動儲存 \d\d:\d\d/);
}

const question = (no: number) => {
  const prompt = screen.getByRole("textbox", { name: `題目 ${no}` });
  return within(prompt.closest("li") as HTMLElement);
};
const linesOf = (no: number) => question(no).queryByRole("img", { name: new RegExp(`^作答線 ${no}：`) });

// ---------------------------------------------------------------- the shared rules

describe("answer formats: the rules the editor shares with the server", () => {
  it("reads a table's columns from the prompt the way the server does", () => {
    // The same cases as api/src/worksheet/mod.rs a_table_takes_its_columns_from_the_prompt.
    expect(columnsFromPrompt("Complete the table for P1: Who? / What happened? / Why? (Question 2)")).toEqual([
      "Who?",
      "What happened?",
      "Why?",
    ]);
    expect(columnsFromPrompt("Fill in the chart： Cause | Effect.")).toEqual(["Cause", "Effect"]);
    for (const none of [
      "Complete the table about his/her day.",
      "Complete the table: Cause",
      "Compare them: what the neighbors expected before the stand opened / what really happened after a month",
      "Complete the table: a / b / c / d / e / f / g",
      "Complete the table: Who? / / Why?",
      "",
    ]) {
      expect(columnsFromPrompt(none)).toBeNull();
    }
    expect(tableOf({ prompt: "Complete the table.", table: undefined })).toEqual({ columns: ["", ""], rows: 3 });
  });

  it("answer lines follow the server: none for a table, a box or the closed formats", () => {
    const lines = (f: string | null) => answerLines({ answer_format: f }, "standard");
    expect([lines("簡答題"), lines(null), lines("段落寫作")]).toEqual([3, 3, 5]);
    for (const f of ["表格整理", "圖像組織", "選擇題", "是非題", "配對題", "填空題"]) expect(lines(f)).toBe(0);
  });

  it("switching formats keeps what fits and sets aside what does not, so switching back restores it", () => {
    const choice = item("x", "Which one?", { answer_format: "選擇題", options: ["A1", "B1", "C1", "D1"] });
    const tf = withAnswerFormat(choice, "是非題");
    expect(tf.options).toEqual(["True", "False"]);
    expect(tf.kept_options).toEqual(["A1", "B1", "C1", "D1"]);
    const back = withAnswerFormat(tf, "選擇題");
    expect(back.options).toEqual(["A1", "B1", "C1", "D1"]);
    expect(back.kept_options).toEqual([]);
    // An open format keeps the choices (unprinted); a new 選擇題 gets four to fill in.
    expect(withAnswerFormat(choice, "簡答題").options).toEqual(choice.options);
    expect(withAnswerFormat(item("y", "Q"), "選擇題").options).toEqual(["", "", "", ""]);
    expect(withAnswerFormat(item("y", "Q"), "配對題").options).toHaveLength(3);
    // Nothing else is touched.
    const table = withAnswerFormat({ ...choice, hint: "h", table: { columns: ["A"], rows: 2 } }, "表格整理");
    expect(table).toMatchObject({ prompt: "Which one?", answer: "Answer to x", hint: "h", table: { columns: ["A"], rows: 2 } });
  });

  it("builds each support form from the model sentence and word bank (T3, T4)", () => {
    const model = "The neighbors thought the umbrellas would be gone.";
    const bank = ["umbrellas", "gone"];
    expect(buildSupport("cloze", model, bank)).toEqual({
      ok: true,
      support: ["The neighbors thought the ___ would be ___."],
      word_bank: bank,
    });
    expect(buildSupport("sentence_frame", model, bank)).toEqual({
      ok: true,
      support: ["___ thought ___ would be ___."],
      word_bank: bank,
    });
    expect(buildSupport("word_bank", model, bank)).toEqual({ ok: true, support: [], word_bank: bank });
    expect(buildSupport("sentence_starter", model, bank)).toEqual({
      ok: true,
      support: ["The neighbors thought …"],
      word_bank: bank,
    });
    expect(buildSupport("none", model, bank)).toBeNull();
    // Deterministic, and phrases in the bank are one blank.
    expect(buildSupport("cloze", "They were sure every umbrella would be gone.", ["every umbrella"])).toEqual({
      ok: true,
      support: ["They were sure ___ would be gone."],
      word_bank: ["every umbrella"],
    });
    // A frame without a bank: what it took out becomes the bank (T4).
    expect(buildSupport("sentence_frame", "I felt nervous when I gave my speech.", [])).toEqual({
      ok: true,
      support: ["I felt ___ when I ___ my ___."],
      word_bank: ["nervous", "gave", "speech"],
    });
    // A cloze whose bank names none of the sentence's words blanks its key words.
    expect(buildSupport("cloze", model, [])).toEqual({
      ok: true,
      support: ["The ___ thought the umbrellas would be gone."],
      word_bank: ["neighbors"],
    });
    // What cannot be made says why.
    expect(buildSupport("sentence_frame", "", bank)).toEqual({ ok: false, missing: "model_sentence" });
    expect(buildSupport("cloze", "  ", [])).toEqual({ ok: false, missing: "model_sentence" });
    expect(buildSupport("word_bank", "", [])).toEqual({ ok: false, missing: "words" });
  });

  it("a question added to a 差異化學習單 has its part in every version", () => {
    const added = newItem(["basic", "standard", "advanced"], true);
    expect(added.tiers?.map((t) => [t.level, t.support_kind])).toEqual([
      ["basic", "sentence_frame"],
      ["standard", "sentence_starter"],
      ["advanced", "none"],
    ]);
    expect(newItem(["standard"]).tiers).toBeUndefined();
  });
});

// ---------------------------------------------------------------- report 1

describe("report 1: choosing an answer format changes the sheet", { timeout: 20_000 }, () => {
  it("a new question turned into 表格整理 shows a table, its headings edit and save, and it survives a reload", async () => {
    const srv = server(state(plain()));
    const user = userEvent.setup();
    const view = renderApp("/worksheets/5");
    const blockA = within(await screen.findByRole("region", { name: "區塊 A" }));
    await user.click(blockA.getByRole("button", { name: "加一題" }));
    // The new question 3: a plain short answer with its lines.
    const select = screen.getByRole("combobox", { name: "作答方式 3" });
    expect(select).toHaveValue("簡答題");
    expect(linesOf(3)).toBeInTheDocument();

    await user.selectOptions(select, "表格整理");
    // A table (two columns, three rows, no headings yet) instead of the lines, and the prompt
    // field now says what a table question asks for.
    const table = question(3).getByRole("table", { name: "作答表格 3" });
    expect(within(table).getAllByRole("columnheader")).toHaveLength(2);
    expect(within(table).getAllByRole("row")).toHaveLength(4);
    expect(linesOf(3)).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "題目 3" })).toHaveAttribute(
      "placeholder",
      "Complete the table: Column 1 / Column 2",
    );

    // The prompt names columns: the table follows it until the teacher edits a heading.
    await user.type(screen.getByRole("textbox", { name: "題目 3" }), "Complete the table: Problem / Solution / Result");
    expect(within(table).getAllByRole("columnheader")).toHaveLength(3);
    expect(screen.getByRole("textbox", { name: "欄名 2（題目 3）" })).toHaveValue("Solution");
    await user.clear(screen.getByRole("textbox", { name: "欄名 3（題目 3）" }));
    await user.type(screen.getByRole("textbox", { name: "欄名 3（題目 3）" }), "What happened next?");
    await user.click(screen.getByRole("button", { name: "加一列（題目 3）" }));
    await settle();
    const saved = srv.saves[srv.saves.length - 1];
    expect(saved?.base_version_no).toBeTypeOf("number");
    expect(saved?.content?.blocks[0]?.items[2]).toMatchObject({
      answer_format: "表格整理",
      table: { columns: ["Problem", "Solution", "What happened next?"], rows: 4 },
      origin: "teacher",
    });

    // A reload shows the same table.
    view.unmount();
    renderApp("/worksheets/5");
    const again = await screen.findByRole("table", { name: "作答表格 3" });
    expect(within(again).getAllByRole("row")).toHaveLength(5);
    expect(screen.getByRole("textbox", { name: "欄名 3（題目 3）" })).toHaveValue("What happened next?");
  });

  it("選擇題 shows editable choices, 是非題 True／False, 簡答／段落寫作 lines — and switching back brings the choices back", async () => {
    const srv = server(state(plain()));
    const user = userEvent.setup();
    const view = renderApp("/worksheets/5");
    const select = await screen.findByRole("combobox", { name: "作答方式 1" });
    expect(question(1).queryByRole("group", { name: "選項 1" })).not.toBeInTheDocument();

    // 選擇題: four choices to fill in, lettered, one by one; the lines go.
    await user.selectOptions(select, "選擇題");
    const choices = question(1).getByRole("group", { name: "選項 1" });
    expect(within(choices).getAllByRole("textbox")).toHaveLength(4);
    expect(linesOf(1)).not.toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "選項 A（題目 1）" }), "Mrs. Lin");
    await user.type(screen.getByRole("textbox", { name: "選項 B（題目 1）" }), "Her neighbors");
    await user.click(screen.getByRole("button", { name: "刪除選項 D（題目 1）" }));
    expect(within(choices).getAllByRole("textbox")).toHaveLength(3);

    // 是非題: True／False; the choices are set aside and said to be.
    await user.selectOptions(select, "是非題");
    expect(screen.getByRole("textbox", { name: "選項 A（題目 1）" })).toHaveValue("True");
    expect(screen.getByRole("textbox", { name: "選項 B（題目 1）" })).toHaveValue("False");
    expect(question(1).getByText(/改成是非題前的 3 個選項先收起來了/)).toBeInTheDocument();

    // 段落寫作: lines, more than a short answer; the choices are not shown (nor printed).
    await user.selectOptions(select, "段落寫作");
    expect(linesOf(1)).toHaveAccessibleName("作答線 1：4 行");
    expect(question(1).queryByRole("group", { name: "選項 1" })).not.toBeInTheDocument();
    await user.selectOptions(select, "簡答題");
    expect(linesOf(1)).toHaveAccessibleName("作答線 1：2 行");

    // Back to 選擇題: the teacher's choices again.
    await user.selectOptions(select, "選擇題");
    expect(screen.getByRole("textbox", { name: "選項 A（題目 1）" })).toHaveValue("Mrs. Lin");
    expect(screen.getByRole("textbox", { name: "選項 B（題目 1）" })).toHaveValue("Her neighbors");
    await settle();
    view.unmount();
    renderApp("/worksheets/5");
    expect(await screen.findByRole("combobox", { name: "作答方式 1" })).toHaveValue("選擇題");
    expect(screen.getByRole("textbox", { name: "選項 B（題目 1）" })).toHaveValue("Her neighbors");
    expect(srv.now().content.blocks[0]?.items[0]).toMatchObject({
      options: ["Mrs. Lin", "Her neighbors", ""],
      kept_options: [],
    });
  });

  it("a format that does not use choices keeps them, says so, and the teacher can drop them; 圖像組織 draws a chart", async () => {
    server(state(plain()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const select = await screen.findByRole("combobox", { name: "作答方式 2" });
    expect(within(question(2).getByRole("group", { name: "選項 2" })).getAllByRole("textbox")).toHaveLength(4);
    await user.selectOptions(select, "圖像組織");
    // Switched to 圖像組織, the question gets a T-chart to fill in (no empty box).
    expect(question(2).getByRole("group", { name: "圖像組織 2" })).toBeInTheDocument();
    expect(question(2).getByRole("combobox", { name: "圖表類型 2" })).toHaveValue("t_chart");
    expect(question(2).queryByRole("img", { name: "圖像組織作答框 2" })).not.toBeInTheDocument();
    expect(question(2).queryByRole("group", { name: "選項 2" })).not.toBeInTheDocument();
    expect(question(2).getByText(/這題原本的 4 個選項還留著，圖像組織不會印出/)).toBeInTheDocument();
    await user.click(question(2).getByRole("button", { name: "刪掉這些選項" }));
    expect(question(2).queryByText(/選項還留著/)).not.toBeInTheDocument();
    // Lines show in the teacher view too, as on the printed teacher sheet.
    await user.click(screen.getByRole("radio", { name: "教師參考版" }));
    expect(linesOf(1)).toHaveAccessibleName("作答線 1：2 行");
  });
});

// ---------------------------------------------------------------- report 2

describe("report 2: choosing a version's support form changes the support", { timeout: 20_000 }, () => {
  it("makes the support of each form from the model sentence, brings the earlier lines back, saves and survives a reload", async () => {
    const srv = server(state(tieredContent(), { settings: { ...emptySettings(), output: "tiered" } }));
    const user = userEvent.setup();
    const view = renderApp("/worksheets/5");
    const group = within(await screen.findByRole("group", { name: "基礎版的支持 1" }));
    const kind = group.getByRole("combobox", { name: "支持形式 1" });
    const lines = () => group.getByRole("textbox", { name: "支持 1（每行一項）" });
    const bank = () => group.queryByRole("textbox", { name: "字庫 1（用逗號分開）" });
    expect(lines()).toHaveValue("The neighbors thought the ___ would be ___.");

    await user.selectOptions(kind, "cloze");
    expect(lines()).toHaveValue("The neighbors thought the ___ would be ___.");
    await user.selectOptions(kind, "word_bank");
    expect(lines()).toHaveValue("");
    expect(bank()).toHaveValue("umbrellas, gone");

    // Back to 句型框架: the frame it had (the model's), not a new one.
    await user.selectOptions(kind, "sentence_frame");
    expect(lines()).toHaveValue("The neighbors thought the ___ would be ___.");
    // 依範句重做 makes it from the model sentence.
    await user.click(group.getByRole("button", { name: "依範句重做支持 1" }));
    expect(lines()).toHaveValue("___ thought ___ would be ___.");
    // Still editable by hand.
    await user.type(lines(), " Why?");
    await settle();
    const basic = (s: SaveWorksheetRequest | undefined) =>
      s?.content?.blocks[0]?.items[0]?.tiers?.find((t) => t.level === "basic");
    expect(basic(srv.saves[srv.saves.length - 1])).toMatchObject({
      support_kind: "sentence_frame",
      support: ["___ thought ___ would be ___. Why?"],
      word_bank: ["umbrellas", "gone"],
    });
    expect(srv.saves.every((s) => s.base_version_no !== undefined)).toBe(true);

    view.unmount();
    renderApp("/worksheets/5");
    const reloaded = within(await screen.findByRole("group", { name: "基礎版的支持 1" }));
    expect(reloaded.getByRole("combobox", { name: "支持形式 1" })).toHaveValue("sentence_frame");
    expect(reloaded.getByRole("textbox", { name: "支持 1（每行一項）" })).toHaveValue("___ thought ___ would be ___. Why?");
  });

  it("a version without a model sentence says so and keeps its lines; once one is written, the form is made from it", async () => {
    server(state(tieredContent(), { settings: { ...emptySettings(), output: "tiered" } }));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(within(await screen.findByRole("tablist", { name: "版本" })).getByRole("tab", { name: "一般版" }));
    const group = within(screen.getByRole("group", { name: "一般版的支持 1" }));
    await user.selectOptions(group.getByRole("combobox", { name: "支持形式 1" }), "sentence_frame");
    expect(group.getByRole("status")).toHaveTextContent("這個版本沒有範句，沒辦法自動做成「句型框架」");
    expect(group.getByRole("textbox", { name: "支持 1（每行一項）" })).toHaveValue("The neighbors thought that …");

    await user.type(group.getByRole("textbox", { name: "範句 1（不印出）" }), "They thought every umbrella would be gone.");
    await user.click(group.getByRole("button", { name: "依範句重做支持 1" }));
    expect(group.queryByRole("status")).not.toBeInTheDocument();
    expect(group.getByRole("textbox", { name: "支持 1（每行一項）" })).toHaveValue("They thought ___ would be ___.");
    expect(group.getByRole("textbox", { name: "字庫 1（用逗號分開）" })).toHaveValue("every umbrella, gone");

    // A starter keeps the bank but does not print it (and says so).
    await user.selectOptions(group.getByRole("combobox", { name: "支持形式 1" }), "sentence_starter");
    expect(group.getByRole("textbox", { name: "支持 1（每行一項）" })).toHaveValue("The neighbors thought that …");
    expect(group.queryByRole("textbox", { name: "字庫 1（用逗號分開）" })).not.toBeInTheDocument();
    expect(group.getByText(/字庫還留著，句首不印字庫/)).toBeInTheDocument();
  });

  it("a question added to a 差異化學習單 gets a support for each version and only the open formats", async () => {
    server(state(tieredContent(), { settings: { ...emptySettings(), output: "tiered" } }));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    await user.click(await screen.findByRole("button", { name: "加一題" }));
    const group = within(screen.getByRole("group", { name: "基礎版的支持 2" }));
    expect(group.getByRole("combobox", { name: "支持形式 2" })).toHaveValue("sentence_frame");
    expect(screen.queryByText("提示（只印在基礎版）")).not.toBeInTheDocument();
    const formats = within(screen.getByRole("combobox", { name: "作答方式 2" }))
      .getAllByRole("option")
      .map((o) => o.textContent);
    expect(formats).toEqual(["簡答題", "表格整理", "圖像組織", "段落寫作"]);
  });
});

// ---------------------------------------------------------------- the rest of the editor

describe("the other controls", { timeout: 20_000 }, () => {
  it("the last version of a question cannot be unticked (it would vanish from every tab)", async () => {
    const srv = server(state(plain()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const versions = within(await screen.findByRole("group", { name: "題目 1 出現的版本" }));
    await user.click(versions.getByRole("checkbox", { name: "題目 1 出現在一般版" }));
    expect(versions.getByRole("checkbox", { name: "題目 1 出現在基礎版" })).toBeDisabled();
    await settle();
    expect(srv.now().content.blocks[0]?.items[0]?.levels).toEqual(["basic"]);
  });

  it("while filtering by Bloom level, 上移 moves past the block shown and 加一個區塊 stays in view", async () => {
    const srv = server(state(plain()));
    const user = userEvent.setup();
    renderApp("/worksheets/5");
    const dist = within(await screen.findByRole("group", { name: "Bloom 層次分布" }));
    await user.click(dist.getByRole("button", { name: "只看分析（2 題）" }));
    // Blocks B and C are shown (A has no 分析 question): C's 上移 puts it before B.
    expect(screen.queryByRole("region", { name: "區塊 A" })).not.toBeInTheDocument();
    const c = within(screen.getByRole("region", { name: "區塊 C" }));
    await user.click(c.getByRole("button", { name: "上移" }));
    await settle();
    expect(srv.now().content.blocks.map((b) => b.id)).toEqual(["b1", "b3", "b2"]);
    expect(screen.getByRole("textbox", { name: "區塊標題 B" })).toHaveValue("Think Further");

    await user.click(screen.getByRole("button", { name: "加一個區塊" }));
    await waitFor(() => expect(screen.getByRole("region", { name: "區塊 D" })).toBeInTheDocument());
    await settle();
    expect(srv.now().content.blocks[3]?.items[0]?.bloom).toBe("analyze");
  });
});
