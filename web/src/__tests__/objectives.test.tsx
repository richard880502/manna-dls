// 內容目標與語言目標 (SIOP G1–G3; docs/API.md「SIOP 目標」): the two fields on /create, the drafts
// Manna makes when both are empty (editable lines under the fields: left, edited or cleared) and
// the decisions they record, 請 Manna 擬一組, the home quick box doing the same for a tool, and the
// lib that turns the lines into fields and decisions.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ObjectiveDraft, ObjectiveDraftResult, PrepSession } from "../api/types";
import { EXEMPLAR_TEXT } from "../lib/create";
import { checkedSetNote, choiceFor, decisionsFor, fieldsFrom, mergeRedraw, objectiveLines, objectiveOptions } from "../lib/objectives";
import { baseRoutes, mockApi, renderApp, respond, signIn } from "../test/mockApi";

const SESSIONS = "/api/prep-sessions";
const DRAFTS = "/api/objective-drafts";
const DECISION = "/api/ai/41/decision";

const created: PrepSession = {
  id: 7,
  title: "Bees",
  material: null,
  topic: null,
  class: null,
  template: null,
  turn_count: 0,
  last_kind: null,
  updated_at: "2026-11-16T08:00:00Z",
  created_at: "2026-11-16T08:00:00Z",
  turns: [],
  running: false,
  next: [],
};

const content: ObjectiveDraft = {
  index: 0,
  kind: "content",
  code: "C1",
  text: "I can explain why bees matter with two examples.",
  zh: "用兩個例子說明蜜蜂為什麼重要",
  check_zh: "Exit ticket：寫出兩個例子。",
};

const language: ObjectiveDraft = {
  index: 2,
  kind: "language",
  code: "L1",
  text: "I can underline signal words like for example.",
  zh: "畫出 for example 等提示詞",
  category: "strategy",
  supports: "C1",
  relation_zh: "學生要用畫出提示詞的策略找出例子，才能說明蜜蜂為什麼重要。",
  check_zh: "檢查學生畫出的提示詞。",
};

const shown = (objectives: ObjectiveDraft[] = [content, language]): ObjectiveDraftResult => ({
  interaction_id: 41,
  status: "shown",
  message: null,
  objectives,
  hidden: 1,
  context: [],
});

function routes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    [`POST ${SESSIONS}`]: created,
    [`GET ${SESSIONS}/7`]: created,
    [`POST ${DECISION}`]: { ok: true },
    ...extra,
  };
}

const generate = () => screen.getByRole("button", { name: "產生" });
const PANEL = "Manna 先幫你草擬目標";
const panel = () => within(screen.getByRole("region", { name: PANEL }));
const contentBox = () => screen.getByRole("textbox", { name: "內容目標 C1" });
const languageBox = () => screen.getByRole("textbox", { name: /^語言目標 L1/ });

async function openWithText(path = "/create/comprehension") {
  const rendered = renderApp(path);
  await waitFor(() => expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(/^來做一份/));
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "用範例試試" }));
  return { ...rendered, user };
}

describe("the lib: fields and decisions", () => {
  it("a field is one objective per line; empty ones are not sent", () => {
    expect(objectiveLines(" a \n\n b ")).toEqual(["a", "b"]);
    expect(objectiveOptions(" A ", "  ")).toEqual({ content_objective: "A" });
    expect(objectiveOptions("", "")).toEqual({});
  });

  it("a line left, edited or cleared becomes adopt / revise / reject, and the fields", () => {
    const second: ObjectiveDraft = { ...content, index: 1, code: "C2", text: "I can compare two farms." };
    const drafts = [content, second, language];
    const choices = {
      0: choiceFor(content, "  I can explain why bees matter.  "),
      1: choiceFor(second, "   "),
      2: choiceFor(language, language.text),
    };
    expect(choices[2]).toEqual({ decision: "adopt" });
    expect(fieldsFrom(drafts, choices)).toEqual({
      content: "I can explain why bees matter.",
      language: language.text,
    });
    expect(decisionsFor(drafts, choices, false)).toEqual([
      { item_index: 0, decision: "revise", revised_content: "I can explain why bees matter." },
      // Cleared: the draft is turned down, nothing takes its place.
      { item_index: 1, decision: "reject" },
      { item_index: 2, decision: "adopt" },
    ]);
    expect(decisionsFor(drafts, choices, true).map((d) => d.decision)).toEqual(["skip", "skip", "skip"]);
  });
});

describe("建立: 內容目標與語言目標", () => {
  it("every tool has the two fields; written objectives go straight to the generation", async () => {
    signIn();
    for (const tool of ["comprehension", "tiered", "activity", "kit", "worksheet", "mimic"]) {
      mockApi(routes());
      const { unmount } = renderApp(`/create/${tool}`);
      expect(await screen.findByRole("textbox", { name: "內容目標（選填）" })).toBeInTheDocument();
      expect(screen.getByRole("textbox", { name: "語言目標（選填）" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "請 Manna 擬內容目標" })).toBeInTheDocument();
      expect(screen.queryByRole("textbox", { name: "學習目標（選填）" })).not.toBeInTheDocument();
      unmount();
    }
    const api = mockApi(routes());
    const { user, router } = await openWithText("/create/activity");
    await user.type(screen.getByRole("textbox", { name: "語言目標（選填）" }), "I can use because.");
    await user.click(generate());
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DRAFTS)).toHaveLength(0);
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      entry: "create",
      kind: "activity",
      request: "課堂活動",
      material: { title: "", text: EXEMPLAR_TEXT },
      options: { language_objective: "I can use because." },
    });
  });

  it("both empty: the drafts open under the fields with focus; one primary, and it adopts all", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText();
    await user.click(generate());

    const drafts = panel();
    // The page's own key for asking how far the drafting is (docs/API.md「產生進度」).
    expect(api.lastBody("POST", DRAFTS)).toEqual({
      kind: "comprehension",
      material: { title: "", text: EXEMPLAR_TEXT },
      stage: "content",
      progress_id: expect.stringMatching(/^[A-Za-z0-9_-]{8,64}$/),
    });
    await waitFor(() => expect(contentBox()).toHaveValue(content.text));
    expect(languageBox()).toHaveValue(language.text);
    expect(drafts.getByText(content.zh)).toBeInTheDocument();
    expect(drafts.getByText(`怎麼檢核：${content.check_zh}`)).toBeInTheDocument();
    // The 語言目標: its kind (strategy first) and how it serves the 內容目標.
    const l1 = within(drafts.getByRole("group", { name: "語言目標 L1" }));
    expect(l1.getByText(/語言目標 L1 · 學習策略/)).toBeInTheDocument();
    expect(l1.getByText(language.relation_zh!)).toBeInTheDocument();
    expect(drafts.queryByRole("button", { name: /^(採用|修改|改寫)/ })).not.toBeInTheDocument();

    // Under the fields, inside the 內容 card, and it took focus; Manna's line is said politely.
    const card = screen.getByRole("region", { name: "內容" });
    expect(card).toContainElement(screen.getByRole("region", { name: PANEL }));
    expect(screen.getByRole("heading", { name: PANEL })).toHaveFocus();
    expect(drafts.getByRole("status")).toHaveTextContent("Manna 擬了一組目標，看一下再產生。");
    await user.tab();
    expect(drafts.getByRole("button", { name: "收起" })).toHaveFocus();
    await user.tab();
    expect(use("內容目標 C1")).toHaveFocus();
    await user.tab();
    expect(contentBox()).toHaveFocus();

    // One primary on the page: the bar keeps its summary but not its own 產生.
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "產生" })).not.toBeInTheDocument();
    expect(screen.queryByText(/請先在上方選好目標/)).not.toBeInTheDocument();
    expect(screen.getByText(/8 題/)).toBeInTheDocument();
    // Nothing is made yet.
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);

    await user.click(drafts.getByRole("button", { name: "用勾選的 2 個目標產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "adopt" },
      { item_index: 2, decision: "adopt" },
    ]);
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({
      kind: "comprehension",
      request: "閱讀理解題",
      options: { content_objective: content.text, language_objective: language.text },
    });
  });

  it("an edited line is revised, a cleared one rejected; the generation uses what is left", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user } = await openWithText("/create/worksheet");
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toHaveValue(content.text));

    await user.clear(contentBox());
    await user.type(contentBox(), "I can explain why bees matter.");
    await user.clear(languageBox());
    await user.click(panel().getByRole("button", { name: "用勾選的 1 個目標產生" }));

    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "revise", revised_content: "I can explain why bees matter." },
      { item_index: 2, decision: "reject" },
    ]);
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({ options: { content_objective: "I can explain why bees matter." } });
    expect(api.lastBody("POST", SESSIONS)).not.toHaveProperty("options.language_objective");
  });

  it("換一組 asks again without the lines shown; 收起 goes back to the form with nothing recorded", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());

    await user.click(panel().getByRole("button", { name: "換一組" }));
    await waitFor(() => expect(api.callsTo("POST", DRAFTS)).toHaveLength(2));
    expect(api.lastBody("POST", DRAFTS)).toMatchObject({ avoid: [content.text, language.text] });

    await user.click(await panel().findByRole("button", { name: "收起" }));
    expect(screen.queryByRole("region", { name: PANEL })).not.toBeInTheDocument();
    expect(generate()).toBeEnabled();
    expect(api.callsTo("POST", DECISION)).toHaveLength(0);
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
  });

  it("不用目標，直接產生 records a skip for every draft and sends no objectives", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText("/create/kit");
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());
    await user.click(panel().getByRole("button", { name: "不用目標，直接產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "skip" },
      { item_index: 2, decision: "skip" },
    ]);
    expect(api.lastBody("POST", SESSIONS)).not.toHaveProperty("options.content_objective");
  });

  it("the 內容目標 come first: only they are drafted, and the 語言目標 wait for the ones she decides on", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown([content]) }));
    const { user, router } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toHaveValue(content.text));
    expect(api.lastBody("POST", DRAFTS)).toMatchObject({ stage: "content" });
    expect(api.lastBody("POST", DRAFTS)).not.toHaveProperty("content");
    expect(screen.queryByRole("textbox", { name: /^語言目標 L1/ })).not.toBeInTheDocument();
    expect(panel().getByText(/決定好內容目標之後，Manna 才會依它擬語言目標/)).toBeInTheDocument();
    expect(panel().getByRole("status")).toHaveTextContent("Manna 擬了內容目標，看一下，決定後再擬語言目標。");
    // Nothing is generated by the primary of this step; the quiet way out is the 內容目標 alone.
    expect(panel().queryByRole("button", { name: /^用勾選的 1 個目標/ })).not.toBeInTheDocument();
    expect(panel().getByRole("button", { name: "只用勾選的 1 個內容目標產生" })).toBeEnabled();
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
    await user.click(panel().getByRole("button", { name: "只用勾選的 1 個內容目標產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DRAFTS)).toHaveLength(1);
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([{ item_index: 0, decision: "adopt" }]);
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({ options: { content_objective: content.text } });
    expect(api.lastBody("POST", SESSIONS)).not.toHaveProperty("options.language_objective");
  });

  it("then 請 Manna 擬語言目標 sends the 內容目標 she kept (as edited); the 語言目標 are derived from them", async () => {
    signIn();
    const lang0: ObjectiveDraft = { ...language, index: 0 };
    let calls = 0;
    const api = mockApi(
      routes({
        [`POST ${DRAFTS}`]: () => (++calls === 1 ? shown([content]) : { ...shown([lang0]), interaction_id: 42 }),
        "POST /api/ai/42/decision": { ok: true },
      }),
    );
    const { user, router } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toHaveValue(content.text));
    await user.clear(contentBox());
    await user.type(contentBox(), "I can explain why bees matter.");
    await user.click(panel().getByRole("button", { name: "用勾選的 1 個內容目標，請 Manna 擬語言目標" }));

    await waitFor(() => expect(languageBox()).toHaveValue(language.text));
    expect(api.lastBody("POST", DRAFTS)).toMatchObject({
      stage: "language",
      content: ["I can explain why bees matter."],
    });
    // The 內容目標 she decided are said above the 語言目標, and are not editable here.
    const decided = within(panel().getByRole("group", { name: "你決定的內容目標" }));
    expect(decided.getByText("I can explain why bees matter.")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "內容目標 C1" })).not.toBeInTheDocument();
    expect(panel().getByText(language.relation_zh!)).toBeInTheDocument();
    // Her decisions on the 內容目標 draft are recorded once the 語言目標 are in.
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(1));
    expect(api.callsTo("POST", DECISION)[0]?.body).toEqual({
      item_index: 0,
      decision: "revise",
      revised_content: "I can explain why bees matter.",
    });

    await user.click(panel().getByRole("button", { name: "用勾選的 1 個目標產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    await waitFor(() => expect(api.callsTo("POST", "/api/ai/42/decision")).toHaveLength(1));
    expect(api.callsTo("POST", "/api/ai/42/decision")[0]?.body).toEqual({ item_index: 0, decision: "adopt" });
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({
      options: { content_objective: "I can explain why bees matter.", language_objective: language.text },
    });
  });

  it("when the 語言目標 do not come, the 內容目標 step stays as she left it, with a note", async () => {
    signIn();
    let calls = 0;
    const api = mockApi(
      routes({
        [`POST ${DRAFTS}`]: () =>
          ++calls === 1 ? shown([content]) : respond(500, { error: { code: "internal", message: "伺服器發生錯誤" } }),
      }),
    );
    const { user } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());
    await user.type(contentBox(), " Really.");
    await user.click(panel().getByRole("button", { name: "用勾選的 1 個內容目標，請 Manna 擬語言目標" }));
    expect(await panel().findByText(/伺服器發生錯誤/)).toBeInTheDocument();
    expect(contentBox()).toHaveValue(`${content.text} Really.`);
    expect(panel().getByRole("button", { name: "用勾選的 1 個內容目標，請 Manna 擬語言目標" })).toBeEnabled();
    expect(api.callsTo("POST", DECISION)).toHaveLength(0);
  });

  it("nothing passed the checks, or the call failed: said plainly, and 不用目標 goes on", async () => {
    signIn();
    const blocked: ObjectiveDraftResult = {
      ...shown([]),
      status: "blocked",
      message: "Manna 這次草擬的目標沒有通過檢查，所以沒有列出。你可以自己寫，或直接產生。",
    };
    let failing = false;
    const api = mockApi(
      routes({
        [`POST ${DRAFTS}`]: () =>
          failing ? respond(500, { error: { code: "internal", message: "伺服器發生錯誤" } }) : blocked,
      }),
    );
    const { user, router } = await openWithText();
    await user.click(generate());
    expect(await panel().findByText(blocked.message!)).toBeInTheDocument();
    // Writing her own objective closes the panel.
    await user.type(screen.getByRole("textbox", { name: "內容目標（選填）" }), "I");
    expect(screen.queryByRole("region", { name: PANEL })).not.toBeInTheDocument();
    await user.clear(screen.getByRole("textbox", { name: "內容目標（選填）" }));

    failing = true;
    await user.click(generate());
    expect(await panel().findByText(/伺服器發生錯誤/)).toBeInTheDocument();
    await user.click(panel().getByRole("button", { name: "不用目標，直接產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DECISION)).toHaveLength(0);
  });
});

describe("建立: 請 Manna 擬內容目標", () => {
  it("opens the drafts without 產生; 用這組目標 fills the fields, records, and 產生 then works as usual", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText();
    await user.click(screen.getByRole("button", { name: "請 Manna 擬內容目標" }));
    await waitFor(() => expect(contentBox()).toHaveValue(content.text));
    expect(panel().getByRole("status")).toHaveTextContent("Manna 擬了一組目標，看一下再填入。");
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);

    await user.clear(languageBox());
    await user.type(languageBox(), "I can use for example.");
    await user.click(panel().getByRole("button", { name: "用勾選的 2 個目標" }));

    // Nothing was generated; the fields hold the (edited) lines and the decisions are in.
    expect(screen.queryByRole("region", { name: PANEL })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "內容目標（選填）" })).toHaveValue(content.text);
    expect(screen.getByRole("textbox", { name: "語言目標（選填）" })).toHaveValue("I can use for example.");
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "adopt" },
      { item_index: 2, decision: "revise", revised_content: "I can use for example." },
    ]);

    await user.click(generate());
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DRAFTS)).toHaveLength(1);
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({
      options: { content_objective: content.text, language_objective: "I can use for example." },
    });
  });

  it("請 Manna 依內容目標擬語言目標 waits for a 內容目標, then drafts the 語言目標 from it and fills only that field", async () => {
    signIn();
    const lang0: ObjectiveDraft = { ...language, index: 0 };
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown([lang0]) }));
    const { user } = await openWithText();
    const ask = screen.getByRole("button", { name: "請 Manna 依內容目標擬語言目標" });
    expect(ask).toBeDisabled();
    expect(screen.getByText("先決定內容目標，才能請 Manna 依它擬語言目標。")).toBeInTheDocument();

    await user.type(screen.getByRole("textbox", { name: "內容目標（選填）" }), "I can explain why bees matter.");
    expect(ask).toBeEnabled();
    await user.click(ask);
    await waitFor(() => expect(languageBox()).toHaveValue(language.text));
    expect(api.lastBody("POST", DRAFTS)).toMatchObject({ stage: "language", content: ["I can explain why bees matter."] });
    expect(panel().getByRole("status")).toHaveTextContent("Manna 擬了一組目標，看一下再填入。");
    await user.click(panel().getByRole("button", { name: "用勾選的 1 個目標" }));
    expect(screen.getByRole("textbox", { name: "內容目標（選填）" })).toHaveValue("I can explain why bees matter.");
    expect(screen.getByRole("textbox", { name: "語言目標（選填）" })).toHaveValue(language.text);
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(1));
  });

  it("with no material it says why instead of drafting", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    renderApp("/create/comprehension");
    const ask = await screen.findByRole("button", { name: "請 Manna 擬內容目標" });
    await userEvent.setup().click(ask);
    expect(await screen.findByRole("alert")).toHaveTextContent(/文章/);
    expect(api.callsTo("POST", DRAFTS)).toHaveLength(0);
  });
});

describe("首頁: a tool from the quick box drafts objectives first", () => {
  it("the typed request goes with the draft; using the set makes the session with it", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(screen.getByRole("button", { name: "貼上文字" }));
    const dialog = within(screen.getByRole("dialog", { name: "貼上文字" }));
    await user.type(dialog.getByRole("textbox", { name: "貼上教材文字" }), "Bees help plants.");
    await user.click(dialog.getByRole("button", { name: "附上" }));
    await user.click(within(screen.getByRole("radiogroup", { name: "要做什麼" })).getByRole("radio", { name: "AI 幫我設計" }));
    await user.type(screen.getByRole("textbox", { name: "給 Manna 的要求" }), "多一點討論");
    await user.click(screen.getByRole("button", { name: "送出" }));

    await waitFor(() => expect(contentBox()).toHaveValue(content.text));
    expect(api.lastBody("POST", DRAFTS)).toEqual({
      kind: "worksheet",
      material: { title: "", text: "Bees help plants." },
      request: "多一點討論",
      stage: "content",
      progress_id: expect.any(String),
    });
    await user.click(panel().getByRole("button", { name: "用勾選的 2 個目標產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      entry: "home",
      kind: "worksheet",
      request: "多一點討論",
      material: { title: "", text: "Bees help plants." },
      options: { content_objective: content.text, language_objective: language.text },
    });
    expect(api.callsTo("POST", DECISION)).toHaveLength(2);
  });

  it("a question (ask) is no tool: no draft", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: "今天想準備什麼？", level: 1 });
    await user.type(screen.getByRole("textbox", { name: "給 Manna 的要求" }), "怎麼教倒裝句？");
    await user.click(screen.getByRole("button", { name: "送出" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.callsTo("POST", DRAFTS)).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------------------------
// 每一項各自勾選（docs/API.md「SIOP 目標」）
// ---------------------------------------------------------------------------------------------

const content2: ObjectiveDraft = {
  index: 1,
  kind: "content",
  code: "C2",
  text: "I can compare two farms.",
  zh: "比較兩個農場",
  check_zh: "兩欄比較表。",
};

const language2: ObjectiveDraft = {
  index: 3,
  kind: "language",
  code: "L2",
  text: "I can use both and but to compare.",
  zh: "用 both … but … 比較",
  category: "function",
  supports: "C2",
  relation_zh: "學生要用 both … but …，才能比較兩個農場。",
  check_zh: "寫一個比較句。",
};

const four = () => shown([content, content2, language, language2]);
const useButton = (n: number, fill = false) =>
  panel().getByRole("button", { name: `用勾選的 ${n} 個目標${fill ? "" : "產生"}` });
const use = (name: string) => screen.getByRole("checkbox", { name: `使用 ${name}` });

describe("lib: the checked set", () => {
  it("an unchecked draft is left out of the fields and recorded as skipped, whatever its box says", () => {
    const drafts = [content, language];
    const choices = { 0: choiceFor(content, content.text), 2: choiceFor(language, "I can do more.") };
    const off = new Set([2]);
    expect(fieldsFrom(drafts, choices, off)).toEqual({ content: content.text, language: "" });
    expect(decisionsFor(drafts, choices, false, off)).toEqual([
      { item_index: 0, decision: "adopt" },
      { item_index: 2, decision: "skip" },
    ]);
  });

  it("a redraw puts the new drafts into the places of the unchecked ones and keeps the rest", () => {
    const fresh: ObjectiveDraft[] = [
      { ...content, index: 0, text: "I can summarize the text." },
      { ...language, index: 2, text: "I can predict from the title." },
      { ...language2, index: 3, text: "I can retell in order." },
    ];
    const merged = mergeRedraw([content, content2, language, language2], new Set([1, 3]), fresh);
    expect(merged.drafts.map((d) => [d.code, d.text])).toEqual([
      ["C1", content.text],
      ["C2", "I can summarize the text."],
      ["L1", language.text],
      ["L2", "I can retell in order."],
    ]);
    expect(merged.fresh).toEqual({ 4: 0, 5: 3 });
    // A kind with no new draft left: that place is dropped.
    const lean = mergeRedraw([content, content2, language], new Set([1]), [{ ...language, index: 2 }]);
    expect(lean.drafts.map((d) => d.code)).toEqual(["C1", "L1"]);
  });

  it("only one kind checked is said, not blocked", () => {
    const all = [content, language];
    expect(checkedSetNote(all, [language])).toMatch(/只勾了語言目標/);
    expect(checkedSetNote(all, [content])).toMatch(/只勾了內容目標/);
    expect(checkedSetNote(all, all)).toBeNull();
    expect(checkedSetNote(all, [])).toBeNull();
    // Nothing to be missing when the set has only one kind.
    expect(checkedSetNote([content], [content])).toBeNull();
  });
});

describe("建立: 每一項各自勾選", () => {
  it("every draft has a checkbox, checked; unchecking dims it but it stays editable; the primary counts", async () => {
    signIn();
    mockApi(routes({ [`POST ${DRAFTS}`]: four() }));
    const { user } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());

    for (const name of ["內容目標 C1", "內容目標 C2", "語言目標 L1", "語言目標 L2"]) expect(use(name)).toBeChecked();
    // The old instruction is gone.
    expect(panel().queryByText(/清空那一項/)).not.toBeInTheDocument();
    expect(panel().getByText(/勾選要用的目標/)).toBeInTheDocument();
    expect(useButton(4)).toBeEnabled();
    expect(panel().getByRole("button", { name: "換一組" })).toBeInTheDocument();

    await user.click(use("內容目標 C2"));
    expect(use("內容目標 C2")).not.toBeChecked();
    expect(screen.getByRole("group", { name: "內容目標 C2" })).toHaveClass("is-off");
    expect(screen.getByRole("group", { name: "內容目標 C1" })).not.toHaveClass("is-off");
    const c2 = screen.getByRole("textbox", { name: "內容目標 C2" });
    expect(c2).toBeEnabled();
    await user.type(c2, " Really.");
    expect(c2).toHaveValue("I can compare two farms. Really.");
    expect(useButton(3)).toBeEnabled();
    expect(panel().getByRole("button", { name: "換掉沒勾的" })).toBeInTheDocument();
    expect(panel().queryByRole("button", { name: "換一組" })).not.toBeInTheDocument();

    // Back on: it counts again, and the edit is still there.
    await user.click(use("內容目標 C2"));
    expect(useButton(4)).toBeInTheDocument();
    expect(c2).toHaveValue("I can compare two farms. Really.");
  });

  it("only the checked ones are used; the unchecked one is recorded as skipped", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText("/create/tiered");
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());

    await user.click(use("語言目標 L1"));
    await user.clear(contentBox());
    await user.type(contentBox(), "I can explain why bees matter.");
    // Only 內容目標 is checked: said, and the primary still works.
    expect(panel().getByText(/只勾了內容目標/)).toBeInTheDocument();
    await user.click(useButton(1));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "revise", revised_content: "I can explain why bees matter." },
      { item_index: 2, decision: "skip" },
    ]);
    const body = api.lastBody("POST", SESSIONS);
    expect(body).toMatchObject({ options: { content_objective: "I can explain why bees matter." } });
    expect(body).not.toHaveProperty("options.language_objective");
  });

  it("nothing checked: the primary is off, says why, and 不用目標 is the way", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());
    await user.click(use("內容目標 C1"));
    await user.click(use("語言目標 L1"));

    const primary = panel().getByRole("button", { name: "用勾選的 0 個目標產生" });
    expect(primary).toBeDisabled();
    expect(primary).toHaveAccessibleDescription(/還沒勾選任何目標/);
    // None checked: 換一組 redraws everything.
    expect(panel().getByRole("button", { name: "換一組" })).toBeInTheDocument();
    await user.click(panel().getByRole("button", { name: "不用目標，直接產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "skip" },
      { item_index: 2, decision: "skip" },
    ]);
  });

  it("the same in the 請 Manna 擬一組 flow: the primary only fills the fields with the checked ones", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user } = await openWithText();
    await user.click(screen.getByRole("button", { name: "請 Manna 擬內容目標" }));
    await waitFor(() => expect(contentBox()).toBeInTheDocument());
    await user.click(use("內容目標 C1"));
    expect(useButton(1, true)).toBeEnabled();
    expect(panel().getByText(/只勾了語言目標/)).toBeInTheDocument();
    await user.click(useButton(1, true));
    expect(screen.getByRole("textbox", { name: "內容目標（選填）" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "語言目標（選填）" })).toHaveValue(language.text);
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "skip" },
      { item_index: 2, decision: "adopt" },
    ]);
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
  });

  it("all checked: 換一組 still redraws everything", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: four() }));
    const { user } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());
    await user.click(panel().getByRole("button", { name: "換一組" }));
    await waitFor(() => expect(api.callsTo("POST", DRAFTS)).toHaveLength(2));
    const body = api.lastBody("POST", DRAFTS);
    expect(body).toMatchObject({ avoid: [content.text, content2.text, language.text, language2.text] });
    expect(body).not.toHaveProperty("keep");
  });

  it("換掉沒勾的 redraws only the unchecked ones, keeps the others as edited, and records who was kept", async () => {
    signIn();
    const fresh: ObjectiveDraftResult = {
      ...shown([
        { ...content, index: 0, text: "I can summarize the text in three sentences.", zh: "三句話摘要" },
        { ...content2, index: 1, text: "I can retell the text in order.", zh: "依序重述" },
        { ...language, index: 2, text: "I can predict from the title.", zh: "看標題預測" },
        { ...language2, index: 3, text: "I can use first and then to retell.", zh: "用 first、then 重述" },
      ]),
      interaction_id: 42,
    };
    let calls = 0;
    const api = mockApi(
      routes({
        [`POST ${DRAFTS}`]: () => (++calls === 1 ? four() : fresh),
        "POST /api/ai/42/decision": { ok: true },
      }),
    );
    const { user, router } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());

    // Keep C1 (edited) and L1; redraw C2 and L2.
    await user.clear(contentBox());
    await user.type(contentBox(), "I can explain why bees matter.");
    await user.click(use("內容目標 C2"));
    await user.click(use("語言目標 L2"));
    await user.click(panel().getByRole("button", { name: "換掉沒勾的" }));

    await waitFor(() => expect(api.callsTo("POST", DRAFTS)).toHaveLength(2));
    const body = api.lastBody("POST", DRAFTS);
    expect(body).toMatchObject({
      avoid: [content2.text, language2.text],
      keep: [
        { code: "C1", text: "I can explain why bees matter." },
        { code: "L1", text: language.text },
      ],
    });
    // The redrawn ones are recorded as skipped on the set they came from.
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 1, decision: "skip" },
      { item_index: 3, decision: "skip" },
    ]);

    // The panel: C1 and L1 as she left them, C2 and L2 new; all checked again.
    await waitFor(() => expect(screen.getByRole("textbox", { name: "內容目標 C2" })).toHaveValue("I can retell the text in order."));
    expect(contentBox()).toHaveValue("I can explain why bees matter.");
    expect(languageBox()).toHaveValue(language.text);
    expect(screen.getByRole("textbox", { name: /^語言目標 L2/ })).toHaveValue("I can use first and then to retell.");
    expect(screen.getByText("依序重述")).toBeInTheDocument();
    expect(screen.queryByText("三句話摘要")).not.toBeInTheDocument();
    expect(useButton(4)).toBeEnabled();

    // Using them: the kept ones are decided on the first set, the new ones on the second.
    await user.click(useButton(4));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    await waitFor(() => expect(api.callsTo("POST", "/api/ai/42/decision")).toHaveLength(2));
    expect(api.callsTo("POST", "/api/ai/42/decision").map((c) => c.body)).toEqual([
      { item_index: 1, decision: "adopt" },
      { item_index: 3, decision: "adopt" },
    ]);
    expect(api.callsTo("POST", DECISION).slice(2).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "revise", revised_content: "I can explain why bees matter." },
      { item_index: 2, decision: "adopt" },
    ]);
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({
      options: {
        content_objective: "I can explain why bees matter.\nI can retell the text in order.",
        language_objective: `${language.text}\nI can use first and then to retell.`,
      },
    });
  });

  it("when the redraw does not work the set stays as it was, with the edits and the unchecked boxes", async () => {
    signIn();
    let calls = 0;
    const api = mockApi(
      routes({
        [`POST ${DRAFTS}`]: () =>
          ++calls === 1 ? four() : respond(500, { error: { code: "internal", message: "伺服器發生錯誤" } }),
      }),
    );
    const { user } = await openWithText();
    await user.click(generate());
    await waitFor(() => expect(contentBox()).toBeInTheDocument());
    await user.type(contentBox(), " Really.");
    await user.click(use("語言目標 L2"));
    await user.click(panel().getByRole("button", { name: "換掉沒勾的" }));

    expect(await panel().findByText(/伺服器發生錯誤/)).toBeInTheDocument();
    expect(contentBox()).toHaveValue(`${content.text} Really.`);
    expect(use("語言目標 L2")).not.toBeChecked();
    expect(use("內容目標 C1")).toBeChecked();
    // Nothing was recorded for a redraw that did not happen.
    expect(api.callsTo("POST", DECISION)).toHaveLength(0);
  });
});
