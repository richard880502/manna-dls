// Key Vocabulary (the word table): Content and Academic side by side with the part of speech, the
// meanings in English or Chinese (one switch per worksheet, both kept), and the word banks'
// parts of speech with the meanings the teacher ticks (api/src/worksheet/key_vocab.rs).
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { WorksheetContent, WorksheetState, WsBlock, WsScaffold, WsSheet, WsSheetWord, WsVocab } from "../api/types";
import { SheetPreview } from "../components/prep/SheetPreview";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { autosaveConfig } from "../lib/useAutosave";
import { bankWithText, emptyBrief, emptySettings, parseWordList, vocabMeaning } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn, type MockCall } from "../test/mockApi";
import { openBarPopover } from "../test/menus";

const NO_PICTURES = { worksheetId: 1, load: () => Promise.reject(new Error("no pictures")) };
beforeEach(() => {
  signIn();
  worksheetConfig.pollMs = 20;
  autosaveConfig.delayMs = 20;
});
afterEach(() => {
  worksheetConfig.pollMs = 2000;
  autosaveConfig.delayMs = 1500;
});

// ---------------------------------------------------------------- the printed sheet

const word = (w: string, extra: Partial<WsSheetWord> = {}): WsSheetWord => ({
  word: w,
  level: null,
  pos: "n.",
  definition: "a thing that keeps rain off you",
  ...extra,
});

const sheetWith = (vocabulary: WsSheetWord[], end_words?: WsSheetWord[]): WsSheet => ({
  title: "The Umbrella Stand",
  level: "standard",
  audience: "student",
  objectives: [],
  blocks: [
    {
      id: "w",
      kind: "warmup",
      label: "A",
      title: "Warm-up",
      instructions: "",
      steps: [],
      vocabulary,
      supports: [],
      items: [],
    },
  ],
  ...(end_words ? { end_words } : {}),
});

const preview = (sheet: WsSheet) =>
  render(<SheetPreview sheet={sheet} pictures={NO_PICTURES} />);

describe("Key Vocabulary on the sheet", () => {
  const both = [
    word("umbrella", { pos: "n." }),
    word("trust", { pos: "v.", definition: "to believe someone is honest" }),
    word("compare", { pos: "v.", task_word: true, definition: "to look at how things are the same and different" }),
  ];

  it("has the title, a Content and an Academic column, each with Word, POS and Meaning", () => {
    const { container } = preview(sheetWith(both));
    expect(screen.getByRole("heading", { name: "Key Vocabulary" })).toBeInTheDocument();
    expect(container.querySelector(".ws-keyvocab-cols")).toHaveClass("is-two");
    const content = screen.getByRole("table", { name: "字詞表 A 暖身（Content）" });
    const academic = screen.getByRole("table", { name: "字詞表 A 暖身（Academic）" });
    for (const table of [content, academic]) {
      expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Word", "POS", "Meaning"]);
    }
    expect(within(content).getAllByRole("row")).toHaveLength(3);
    expect(within(content).getByRole("cell", { name: "umbrella" })).toBeInTheDocument();
    expect(within(content).getAllByRole("cell", { name: "v." })).toHaveLength(1);
    expect(within(academic).getByRole("cell", { name: "compare" })).toBeInTheDocument();
    expect(within(academic).getByRole("cell", { name: "to look at how things are the same and different" })).toBeInTheDocument();
    expect(screen.getByText("Content")).toBeInTheDocument();
    expect(screen.getByText("Academic")).toBeInTheDocument();
  });

  it("is one column when the other group is empty", () => {
    const { container, unmount } = preview(sheetWith([word("umbrella")]));
    expect(container.querySelector(".ws-keyvocab-cols")).not.toHaveClass("is-two");
    expect(screen.getByText("Content")).toBeInTheDocument();
    expect(screen.queryByText("Academic")).toBeNull();
    expect(screen.getByRole("table", { name: "字詞表 A 暖身" })).toBeInTheDocument();
    unmount();
    preview(sheetWith([word("compare", { task_word: true })]));
    expect(screen.getByText("Academic")).toBeInTheDocument();
    expect(screen.queryByText("Content")).toBeNull();
  });

  it("shows a row without a part of speech (an older worksheet) with an empty cell", () => {
    preview(sheetWith([word("umbrella", { pos: undefined })]));
    const table = screen.getByRole("table", { name: "字詞表 A 暖身" });
    expect(within(table).getAllByRole("row")).toHaveLength(2);
    expect(within(table).getByRole("cell", { name: "umbrella" })).toBeInTheDocument();
  });

  it("is printed once after the last block with the same two columns", () => {
    preview(sheetWith([], both));
    const end = screen.getByRole("region", { name: "字詞表（學習單最後）" });
    expect(within(end).getByRole("heading", { name: "Key Vocabulary" })).toBeInTheDocument();
    expect(within(end).getByRole("table", { name: "字詞表（Content）" })).toBeInTheDocument();
    expect(within(end).getByRole("table", { name: "字詞表（Academic）" })).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the editor

const WS = "/api/worksheets/11";

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

const umbrella: WsVocab = {
  word: "umbrella",
  level: 2,
  pos: "n.",
  definition: "a thing that keeps rain off you",
  meaning_zh: "雨傘",
  context: "",
  paragraph_id: null,
};
const trust: WsVocab = { word: "trust", level: 3, pos: "v.", definition: "to believe someone is honest", context: "", paragraph_id: null };

const bankScaffold = (extra: Partial<WsScaffold> = {}): WsScaffold => ({
  id: "sc-bank",
  type: "verbal",
  kind: "字詞表",
  content: "plain, profound",
  steps: [],
  levels: ["standard"],
  auto: false,
  origin: "ai",
  bank: { words: [{ word: "plain", pos: "adj.", definition: "easy to see", meaning_zh: "明顯的" }, { word: "profound", pos: "adj." , definition: "very deep", meaning_zh: "深刻的" }] },
  ...extra,
});

const contentOf = (b: WsBlock, extra: Partial<WorksheetContent> = {}): WorksheetContent => ({
  objectives: [],
  show_objectives: false,
  levels: ["standard"],
  chosen: null,
  notices: [],
  blocks: [b],
  ...extra,
});

const worksheet = (content: WorksheetContent, version_no = 3): WorksheetState => ({
  id: 11,
  title: "The Umbrella Stand",
  filename: "The Umbrella Stand_學習單",
  lesson: null,
  class_id: null,
  generated: true,
  levels: ["standard"],
  version_no,
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
  content,
  jobs: [],
  running: false,
  undoable: [],
  sop_sync: { linked: false, changes: 0 },
  prep_session_id: 7,
});

const routes = (state: WorksheetState, extra: Record<string, unknown> = {}) => ({
  ...baseRoutes(),
  "GET /api/worksheets": [],
  [`GET ${WS}`]: state,
  [`GET ${WS}/comments`]: [],
  [`GET ${WS}/proposals`]: [],
  [`PUT ${WS}`]: (call: MockCall) =>
    worksheet((call.body as { content: WorksheetContent }).content, state.version_no + 1),
  ...extra,
});

const savedContent = (api: ReturnType<typeof mockApi>) =>
  (api.lastBody("PUT", WS) as { content: WorksheetContent }).content;

describe("Key Vocabulary in the editor: the language switch", () => {
  const withWords = (rows: WsVocab[], extra: Partial<WorksheetContent> = {}) =>
    worksheet(contentOf(block("b1", { title: "Warm-up", kind: "warmup", vocabulary: rows }), extra));

  it("switches between English and Chinese at once, keeps both and saves the choice", async () => {
    const api = mockApi(routes(withWords([umbrella])));
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    const english = await screen.findByRole("textbox", { name: "umbrella 的解釋" });
    expect(english).toHaveValue("a thing that keeps rain off you");
    expect(screen.getByRole("heading", { name: "Key Vocabulary" })).toBeInTheDocument();

    await user.click((await openBarPopover(user, "顯示")).getByRole("checkbox", { name: "字詞表的字義用中文" }));
    expect(await screen.findByRole("textbox", { name: "umbrella 的中文解釋" })).toHaveValue("雨傘");
    await waitFor(() => expect(api.callsTo("PUT", WS).length).toBeGreaterThan(0));
    expect(savedContent(api).vocabulary_language).toBe("zh");
    // Nothing was missing: Manna was not asked.
    expect(api.callsTo("POST", `${WS}/vocabulary-meanings`)).toHaveLength(0);

    await user.click((await openBarPopover(user, "顯示")).getByRole("checkbox", { name: "字詞表的字義用中文" }));
    expect(await screen.findByRole("textbox", { name: "umbrella 的解釋" })).toHaveValue("a thing that keeps rain off you");
    await waitFor(() => expect(savedContent(api).vocabulary_language).toBe("en"));
    const row = savedContent(api).blocks[0]!.vocabulary[0]!;
    expect(row.definition).toBe("a thing that keeps rain off you");
    expect(row.meaning_zh).toBe("雨傘");
  });

  it("asks Manna once for the Chinese meanings that are missing and says plainly when it could not", async () => {
    const api = mockApi(
      routes(withWords([umbrella, trust]), {
        [`POST ${WS}/vocabulary-meanings`]: () => ({
          ...withWords([umbrella, trust], { vocabulary_language: "zh" }),
          notice: "Manna 這次沒有寫出字義，1 個字沒有中文字義。沒有中文字義的字會先印英文字義；你也可以直接在字詞表上自己填。",
          filled: 0,
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    await screen.findByRole("textbox", { name: "umbrella 的解釋" });
    await user.click((await openBarPopover(user, "顯示")).getByRole("checkbox", { name: "字詞表的字義用中文" }));
    await waitFor(() => expect(api.callsTo("POST", `${WS}/vocabulary-meanings`)).toHaveLength(1));
    // The switch was saved first, and the request asks for the tables.
    expect(savedContent(api).vocabulary_language).toBe("zh");
    expect(api.lastBody("POST", `${WS}/vocabulary-meanings`)).toEqual({ tables: true });
    expect(await screen.findByText(/沒有中文字義的字會先印英文字義/)).toBeInTheDocument();
    // The word without Chinese keeps its English meaning in the field's hint; the switch stays on.
    expect(screen.getByRole("textbox", { name: "trust 的中文解釋" })).toHaveAttribute("placeholder", expect.stringContaining("to believe someone is honest"));
    const display = await openBarPopover(user, "顯示");
    expect(display.getByRole("checkbox", { name: "字詞表的字義用中文" })).toBeChecked();
    expect(display.getByRole("button", { name: "補上中文字義" })).toBeInTheDocument();
  });

  it("shows Content and Academic side by side with an editable part of speech", async () => {
    const compare: WsVocab = {
      word: "compare",
      level: null,
      pos: "v.",
      definition: "to look at how things are the same and different",
      meaning_zh: "比較",
      context: "",
      paragraph_id: null,
      task_word: true,
    };
    const api = mockApi(routes(withWords([umbrella, compare])));
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    const content = await screen.findByRole("table", { name: /^字詞表.*（Content）$/ });
    expect(screen.getByRole("table", { name: /^字詞表.*（Academic）$/ })).toBeInTheDocument();
    expect(within(content).getByRole("combobox", { name: "umbrella 的詞性" })).toHaveValue("n.");
    await user.selectOptions(within(content).getByRole("combobox", { name: "umbrella 的詞性" }), "v.");
    await waitFor(() => expect(savedContent(api).blocks[0]!.vocabulary[0]!.pos).toBe("v."));
  });
});

describe("字詞表鷹架 (Word bank) in the editor", () => {
  const withBank = (scaffold: WsScaffold) => worksheet(contentOf(block("b1", { title: "Paragraph 1", scaffolds: [scaffold] })));

  it("lists the words with their part of speech; a tick shows the meaning in the chosen language", async () => {
    const api = mockApi(routes(withBank(bankScaffold())));
    const user = userEvent.setup();
    renderApp("/worksheets/11");
    const plain = await screen.findByRole("combobox", { name: "plain 的詞性（字詞表）" });
    expect(plain).toHaveValue("adj.");
    // Nothing ticked: no meaning is shown, and nothing was asked of Manna.
    expect(screen.queryByText("easy to see")).toBeNull();
    expect(api.callsTo("POST", `${WS}/vocabulary-meanings`)).toHaveLength(0);

    await user.click(screen.getByRole("checkbox", { name: "plain 印出字義" }));
    expect(await screen.findByText("easy to see")).toBeInTheDocument();
    expect(screen.queryByText("very deep")).toBeNull();
    await user.click(screen.getByRole("radio", { name: "中文" }));
    expect(await screen.findByText("明顯的")).toBeInTheDocument();
    expect(screen.queryByText("easy to see")).toBeNull();
    await waitFor(() => expect(savedContent(api).blocks[0]!.scaffolds[0]!.bank?.lang).toBe("zh"));
    const bank = savedContent(api).blocks[0]!.scaffolds[0]!.bank!;
    expect(bank.words.map((w) => [w.word, w.show_meaning === true])).toEqual([["plain", true], ["profound", false]]);
    expect(api.callsTo("POST", `${WS}/vocabulary-meanings`)).toHaveLength(0);
  });

  it("an old worksheet's words without a part of speech are filled once when it is opened", async () => {
    const old = bankScaffold({ bank: { words: [{ word: "plain" }, { word: "profound" }] } });
    let filled = false;
    const api = mockApi(
      routes(withBank(old), {
        [`GET ${WS}`]: () => withBank(filled ? bankScaffold() : old),
        [`POST ${WS}/vocabulary-meanings`]: () => {
          filled = true;
          return { ...withBank(bankScaffold()), notice: null, filled: 2 };
        },
      }),
    );
    renderApp("/worksheets/11");
    await waitFor(() => expect(screen.getByRole("combobox", { name: "plain 的詞性（字詞表）" })).toHaveValue("adj."));
    expect(api.callsTo("POST", `${WS}/vocabulary-meanings`)).toHaveLength(1);
    expect(api.lastBody("POST", `${WS}/vocabulary-meanings`)).toEqual({ banks: true });
  });

  it("says so when the parts of speech could not be written, and prints the words as they are", async () => {
    const old = bankScaffold({ bank: { words: [{ word: "plain" }] }, content: "plain" });
    mockApi(
      routes(withBank(old), {
        [`POST ${WS}/vocabulary-meanings`]: () => ({
          ...withBank(old),
          notice: "目前沒有開啟 AI，字詞表鷹架有 1 個字沒有詞性或字義。沒有詞性的字會先只印字；你也可以自己在字詞表上填。",
          filled: 0,
        }),
      }),
    );
    renderApp("/worksheets/11");
    expect(await screen.findByText(/沒有詞性的字會先只印字/)).toBeInTheDocument();
    // Never blocks: the word is still there and its part of speech can be chosen by hand.
    expect(screen.getByRole("combobox", { name: "plain 的詞性（字詞表）" })).toHaveValue("");
  });
});

describe("the word list helpers agree with the server", () => {
  it("reads a plain list of words and nothing else", () => {
    expect(parseWordList("Word bank: plain, profound, in the end")).toEqual(["plain", "profound", "in the end"]);
    expect(parseWordList("umbrella")).toEqual(["umbrella"]);
    expect(parseWordList("Linking words for contrast: however, but")).toBeNull();
    expect(parseWordList("Write what you think.")).toBeNull();
    expect(parseWordList("")).toBeNull();
  });

  it("keeps what is known about the words that stay when the list is edited", () => {
    const bank = { lang: "zh" as const, words: [{ word: "plain", pos: "adj.", show_meaning: true }, { word: "fork", pos: "n." }] };
    const next = bankWithText(bank, "Plain, fork, hesitate")!;
    expect(next.lang).toBe("zh");
    expect(next.words[0]).toMatchObject({ word: "Plain", pos: "adj.", show_meaning: true });
    expect(next.words[2]).toEqual({ word: "hesitate" });
    expect(bankWithText(bank, "Not: a list")).toBeUndefined();
  });

  it("falls back to the other language when a meaning is missing", () => {
    expect(vocabMeaning({ definition: "easy to see", meaning_zh: "明顯的" }, "zh")).toBe("明顯的");
    expect(vocabMeaning({ definition: "easy to see" }, "zh")).toBe("easy to see");
    expect(vocabMeaning({ definition: "easy to see", meaning_zh: "明顯的" }, undefined)).toBe("easy to see");
  });
});
