// 詞彙級別檢查：處理超出級別的字 — each word above the level gets 提供字義／換字／不處理 (one by one
// or all at once), Manna's meanings and replacements are asked for once per action, the choices are
// applied as one new version and the page shows the new analysis; the Glossary shows in the editor.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  VocabularyAnalysis,
  WorksheetContent,
  WorksheetState,
  WsBlock,
  WsItem,
  WsOverLevelWord,
  WsVocabApplied,
  WsVocabCheck,
  WsVocabPlace,
  WsVocabSuggestions,
} from "../api/types";
import { autosaveConfig } from "../lib/useAutosave";
import { emptyBrief, emptySettings } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

beforeEach(() => {
  signIn();
  autosaveConfig.delayMs = 20;
});
afterEach(() => {
  autosaveConfig.delayMs = 1500;
});

const WS = "/api/worksheets/11";

const item = (id: string, prompt: string): WsItem => ({
  id,
  answer_format: "簡答題",
  prompt,
  options: [],
  answer: "",
  evidence: "",
  purpose: null,
  levels: ["basic", "standard"],
  origin: "ai",
});

const block = (id: string, extra: Partial<WsBlock>): WsBlock => ({
  id,
  kind: "activity",
  enabled: true,
  title: "Part A",
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

const content = (extra: Partial<WsBlock> = {}): WorksheetContent => ({
  objectives: [],
  show_objectives: false,
  levels: ["basic", "standard"],
  chosen: null,
  notices: [],
  blocks: [block("b1", { items: [item("i1", "Why did the family abandon their enormous house?")], ...extra })],
});

const worksheet = (overrides: Partial<WorksheetState> = {}): WorksheetState =>
  ({
    id: 11,
    title: "The Farm",
    filename: "The Farm_學習單",
    lesson: null,
    class_id: null,
    generated: true,
    levels: ["basic", "standard"],
    version_no: 3,
    created_at: "2026-11-15T08:00:00Z",
    updated_at: "2026-11-15T08:00:00Z",
    material: {
      title: "The Farm",
      paragraphs: [{ id: "p1", text: "The family decided to abandon the old farm." }],
      source: { kind: "pasted", lesson_id: null, material_version_no: null, note: "" },
    },
    settings: emptySettings(),
    suggested: emptySettings(),
    brief: emptyBrief(),
    versions_count: 3,
    saved_at: "2026-11-15T08:00:00Z",
    content: content(),
    jobs: [],
    running: false,
    undoable: [],
    sop_sync: { linked: false, changes: 0 },
    ...overrides,
  }) as WorksheetState;

const word = (base: string, extra: Partial<WsOverLevelWord> = {}): WsOverLevelWord => ({
  base,
  level: 5,
  pos: "v.",
  count: 1,
  forms: [base],
  replaceable: 1,
  fixed: [],
  in_material: false,
  key_word: false,
  glossed: null,
  kept: false,
  suggested: "replace",
  reason: "只出現在題目或指示裡，可以換成較簡單的字。",
  contexts: [],
  ...extra,
});

const analysis = (bases: string[]): VocabularyAnalysis => ({
  wordlist_version: "TEST",
  target_level: 4,
  basis: {
    counted_tokens: 10,
    counted_types: 8,
    denominator_note: "分母說明",
    excluded_tokens: { proper_nouns: 0, unknown: 0, multiword: 0, appendix: 0 },
    match_types: {},
  },
  level_distribution: [1, 2, 3, 4, 5, 6].map((level) => ({ level, tokens: 1, token_ratio: 0.1, types: 1, type_ratio: 0.1 })),
  above_target: bases.map((base) => ({
    base,
    level: 5,
    count: 1,
    positions: [{ form: base, line: 3, col: 5, context: `… ${base} …`, match: "exact" }],
  })),
  ambiguous_forms: [],
  derived_matches: [],
  multi_level_words: [],
  proper_nouns: [],
  unknown: [],
  multiword: [],
  appendix_words: [],
  limits: [],
  disclaimer: "本分析為備課輔助。",
});

const check = (words: WsOverLevelWord[], extra: Partial<WsVocabCheck> = {}): WsVocabCheck => ({
  id: 41,
  level: "standard",
  audience: "student",
  target_level: 4,
  version_no: 3,
  checked_at: "2026-11-18T08:00:00Z",
  stale: false,
  analysis: analysis(words.map((w) => w.base)),
  lines: ["標題", "A 活動 · 標題", "A 活動 · 第 1 題"],
  words,
  ...extra,
});

const WORDS = [
  word("abandon", {
    forms: ["abandon", "abandoned"],
    in_material: true,
    suggested: "gloss",
    reason: "課文裡也有這個字；課文不會改，附上字義學生才對得起來。",
  }),
  word("enormous", { pos: "adj." }),
  word("resilient", {
    level: 6,
    replaceable: 0,
    key_word: true,
    fixed: ["A 活動 · 字詞表：字詞表（這節課要教的字）"],
    suggested: "leave",
  }),
];

/** The row of a word in the table of words above the level. */
const row = (base: string) => {
  const found = screen
    .getAllByRole("row")
    .find((r) => r.querySelector("td.en")?.firstChild?.textContent === base);
  if (!found) throw new Error(`no row for ${base}`);
  return found;
};

describe("詞彙級別檢查：處理超出級別的字", () => {
  it("chooses per word and in bulk, shows each rewritten sentence, and applies exactly what is ticked", async () => {
    const user = userEvent.setup();
    const field = (index: number): WsVocabPlace["field"] => ({ block: "b1", part: "prompt", id: `i${index}` });
    const PLACES: WsVocabPlace[] = [
      {
        id: "p1",
        field: field(1),
        place: "A 活動 · 第 1 題",
        start: 0,
        end: 48,
        original: "Why did the family abandon their enormous house?",
        bases: ["enormous"],
        rewritten: "Why did the family abandon their huge house?",
        interaction_id: 72,
        problem: null,
      },
      {
        id: "p2",
        field: { block: "b1", part: "instructions" },
        place: "A 活動 · 作答指示",
        start: 0,
        end: 30,
        original: "Talk about an enormous change.",
        bases: ["enormous"],
        rewritten: "Talk about a huge change.",
        interaction_id: 72,
        problem: null,
      },
      {
        id: "p3",
        field: field(2),
        place: "A 活動 · 第 2 題",
        start: 0,
        end: 17,
        original: "An enormous tree.",
        bases: ["enormous"],
        rewritten: null,
        interaction_id: null,
        problem: "用了「gigantic」（第 6 級），超出第 4 級。",
      },
    ];
    const suggestions = (call: MockCall): WsVocabSuggestions => {
      const body = call.body as { action: "gloss" | "replace"; words: string[] };
      return body.action === "gloss"
        ? {
            action: "gloss",
            status: "shown",
            message: null,
            interaction_id: 71,
            places: [],
            words: body.words.map((base) => ({
              base,
              interaction_id: 71,
              gloss: { pos: "v.", meaning_zh: "拋棄；放棄", definition_en: "to leave and not come back" },
              replacement: null,
              places: [],
              notes: [],
            })),
          }
        : {
            action: "replace",
            status: "shown",
            message: "有 1 句這次沒有可用的改寫（用了超出級別的字、改動太大或沒有寫出），這些地方不會改。",
            interaction_id: 72,
            places: PLACES,
            words: body.words.map((base) => ({
              base,
              interaction_id: 72,
              gloss: null,
              replacement: "huge",
              places: ["p1", "p2", "p3"],
              notes: [],
            })),
          };
    };
    const after = check([WORDS[0]!, WORDS[1]!, WORDS[2]!].map((w) => ({ ...w })), { id: 42, version_no: 4 });
    after.words![0]!.glossed = "拋棄；放棄";
    after.words![2]!.kept = true;
    const applied = (call: MockCall): WsVocabApplied => {
      void call;
      return {
        version_no: 4,
        glossed: ["abandon"],
        replaced: [{ base: "enormous", sentences: 1, left: 2 }],
        left: ["resilient"],
        check: after,
        worksheet: worksheet({ version_no: 4 }),
      };
    };
    const { calls } = mockApi({
      ...baseRoutes(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/checks`]: [check(WORDS)],
      [`POST ${WS}/checks/41/suggest`]: suggestions,
      [`POST ${WS}/checks/41/apply`]: applied,
    });
    renderApp("/worksheets/11/check");
    await screen.findByRole("heading", { name: "超出目標級別的詞" });

    // A key word cannot be replaced; the suggestion says why.
    expect(within(row("resilient")).getByRole("radio", { name: "resilient：換字" })).toBeDisabled();
    expect(within(row("abandon")).getByText(/建議：提供字義/)).toBeInTheDocument();
    expect(within(row("abandon")).getByText("課文裡也有這個字（課文不會改）。")).toBeInTheDocument();

    // 照建議選: gloss, replace, leave.
    await user.click(screen.getByRole("button", { name: "照建議選" }));
    expect(screen.getByRole("radio", { name: "abandon：提供字義" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "enormous：換字" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "resilient：不處理" })).toBeChecked();
    // Nothing to apply before the sentences are rewritten and the meaning written.
    expect(screen.getByRole("button", { name: /套用到學習單/ })).toBeDisabled();
    // 全部提供字義 then back to the suggestion for enormous.
    await user.click(screen.getByRole("button", { name: "全部提供字義" }));
    expect(screen.getByRole("radio", { name: "enormous：提供字義" })).toBeChecked();
    await user.click(screen.getByRole("radio", { name: "enormous：換字" }));
    await user.click(screen.getByRole("radio", { name: "resilient：不處理" }));

    await user.click(screen.getByRole("button", { name: "請 Manna 建議（2 個字）" }));
    await waitFor(() => expect(screen.getByLabelText("abandon 的中文意思")).toHaveValue("拋棄；放棄"));
    const asked = calls.filter((c) => c.path.endsWith("/suggest")).map((c) => c.body);
    expect(asked).toEqual([
      { action: "gloss", words: ["abandon"] },
      { action: "replace", words: ["enormous"] },
    ]);
    // Every sentence before → after; the one without a usable rewrite says why and cannot be ticked.
    const enormous = row("enormous");
    expect(within(enormous).getByText("Why did the family abandon their huge house?")).toBeInTheDocument();
    expect(within(enormous).getByText("Talk about an enormous change.")).toBeInTheDocument();
    expect(within(enormous).getByText(/主要換成 huge/)).toBeInTheDocument();
    expect(within(enormous).getByRole("checkbox", { name: "enormous：A 活動 · 第 1 題" })).toBeChecked();
    expect(within(enormous).getByRole("checkbox", { name: "enormous：A 活動 · 第 2 題" })).toBeDisabled();
    expect(within(enormous).getByText(/第 6 級/)).toBeInTheDocument();
    // The teacher unticks one sentence.
    await user.click(within(enormous).getByRole("checkbox", { name: "enormous：A 活動 · 作答指示" }));
    // The teacher edits the meaning; the gloss goes to every version.
    await user.clear(screen.getByLabelText("abandon 的中文意思"));
    await user.type(screen.getByLabelText("abandon 的中文意思"), "放棄");
    await user.click(screen.getByRole("radio", { name: "所有印出這個字的版本" }));

    await user.click(screen.getByRole("button", { name: "套用到學習單（3 個字）" }));
    await screen.findByText(/已寫成第 4 版：提供字義 1 個、換字 1 個（改寫 1 句）、不處理 1 個/);
    const sent = calls.find((c) => c.path.endsWith("/apply"))?.body;
    expect(sent).toEqual({
      base_version_no: 3,
      gloss_scope: "all",
      words: [
        {
          base: "abandon",
          action: "gloss",
          gloss: { pos: "v.", meaning_zh: "放棄", definition_en: "to leave and not come back" },
          interaction_id: 71,
        },
        { base: "enormous", action: "replace", interaction_id: 72, offered: 2 },
        { base: "resilient", action: "leave", interaction_id: null },
      ],
      // Exactly what was shown and left ticked.
      rewrites: [
        {
          field: { block: "b1", part: "prompt", id: "i1" },
          start: 0,
          end: 48,
          original: "Why did the family abandon their enormous house?",
          rewritten: "Why did the family abandon their huge house?",
          bases: ["enormous"],
        },
      ],
    });
    // The new analysis says what happened to each word.
    await waitFor(() => expect(within(row("abandon")).getByText("已提供字義：拋棄；放棄")).toBeInTheDocument());
    expect(within(row("resilient")).getByText("已標記不處理")).toBeInTheDocument();
  });

  it("does not apply 換字 until a usable sentence is ticked", async () => {
    const user = userEvent.setup();
    mockApi({
      ...baseRoutes(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/checks`]: [check(WORDS)],
      [`POST ${WS}/checks/41/suggest`]: {
        action: "replace",
        status: "blocked",
        message: "這次沒有可用的建議，你可以再試一次，或直接在學習單上修改。",
        interaction_id: 80,
        places: [
          {
            id: "p1",
            field: { block: "b1", part: "prompt", id: "i1" },
            place: "A 活動 · 第 1 題",
            start: 0,
            end: 48,
            original: "Why did the family abandon their enormous house?",
            bases: ["enormous"],
            rewritten: null,
            interaction_id: null,
            problem: "改動太多：只能改這個字和它旁邊的幾個字，句子其他部分要保持原樣。",
          },
        ],
        words: [
          {
            base: "enormous",
            interaction_id: null,
            gloss: null,
            replacement: null,
            places: ["p1"],
            notes: ["這個字的句子都沒有可用的改寫，可以改用提供字義。"],
          },
        ],
      } satisfies WsVocabSuggestions,
    });
    renderApp("/worksheets/11/check");
    await screen.findByRole("heading", { name: "超出目標級別的詞" });
    await user.click(screen.getByRole("radio", { name: "enormous：換字" }));
    await user.click(screen.getByRole("button", { name: "請 Manna 建議（1 個字）" }));
    expect(await within(row("enormous")).findByText(/改動太多/)).toBeInTheDocument();
    expect(within(row("enormous")).getByText(/可以改用提供字義/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "套用到學習單（1 個字）" })).toBeDisabled();
    expect(screen.getByText(/還差：enormous/)).toBeInTheDocument();
  });

  it("says when the worksheet changed after the analysis", async () => {
    const user = userEvent.setup();
    mockApi({
      ...baseRoutes(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/checks`]: [check(WORDS)],
      [`POST ${WS}/checks/41/apply`]: () =>
        respond(409, { error: { code: "conflict", message: "學習單在這次分析之後改過了，請重新分析，再處理超出級別的字。" } }),
    });
    renderApp("/worksheets/11/check");
    await screen.findByRole("heading", { name: "超出目標級別的詞" });
    await user.click(screen.getByRole("button", { name: "全部不處理" }));
    await user.click(screen.getByRole("button", { name: "套用到學習單（3 個字）" }));
    expect(await screen.findByText(/學習單在這次分析之後改過了/)).toBeInTheDocument();
  });

  it("shows only the old list for a stale result", async () => {
    mockApi({
      ...baseRoutes(),
      [`GET ${WS}`]: worksheet(),
      [`GET ${WS}/checks`]: [check(WORDS, { stale: true })],
    });
    renderApp("/worksheets/11/check");
    await screen.findByRole("heading", { name: "超出目標級別的詞" });
    expect(screen.queryByRole("button", { name: "全部提供字義" })).not.toBeInTheDocument();
    expect(screen.getByText(/學習單之後改過，請重新分析/)).toBeInTheDocument();
  });

  it("shows the Glossary in the editor and takes an entry off the version shown", async () => {
    const user = userEvent.setup();
    const ws = worksheet({
      content: content({
        glosses: [
          {
            word: "abandon",
            pos: "v.",
            meaning_zh: "拋棄",
            definition_en: "",
            levels: ["basic", "standard"],
            origin: "ai",
          },
        ],
      }),
    });
    const { calls } = mockApi({
      ...baseRoutes(),
      [`GET ${WS}`]: ws,
      [`GET ${WS}/jobs`]: { running: false, jobs: [], version_no: 3 },
      [`GET ${WS}/comments`]: [],
      [`GET ${WS}/proposals`]: [],
      [`GET ${WS}/images`]: { enabled: false, slots: [] },
      [`PUT ${WS}`]: (call: MockCall) => ({ ...ws, content: (call.body as { content: WorksheetContent }).content }),
    });
    renderApp("/worksheets/11");
    const box = await screen.findByLabelText(/^Glossary/);
    expect(within(box).getByText("(v.) 拋棄")).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "移除字義 abandon" }));
    await waitFor(() => expect(screen.queryByLabelText(/^Glossary/)).not.toBeInTheDocument());
    await waitFor(() => expect(calls.some((c) => c.method === "PUT" && c.path === WS)).toBe(true));
    const saved = calls.filter((c) => c.method === "PUT" && c.path === WS).at(-1)?.body as { content: WorksheetContent };
    // Off the version shown only (the editor opens on 基礎版, the first version).
    expect(saved.content.blocks[0]?.glosses).toEqual([
      expect.objectContaining({ word: "abandon", levels: ["standard"] }),
    ]);
  });
});
