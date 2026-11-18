// 內容目標與語言目標 (SIOP G1–G3; docs/API.md「SIOP 目標」): the two fields on /create, the drafts
// Manna makes when both are empty (採用／修改／改寫, all at once, or none) and the decisions they
// record, the home quick box doing the same for a tool, and the lib that turns choices into
// fields and decisions.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ObjectiveDraft, ObjectiveDraftResult, PrepSession } from "../api/types";
import { EXEMPLAR_TEXT } from "../lib/create";
import { decisionsFor, fieldsFrom, objectiveLines, objectiveOptions } from "../lib/objectives";
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
const panel = () => within(screen.getByRole("region", { name: "Manna 先幫你草擬目標" }));

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

  it("採用、修改、改寫 become the fields and adopt / revise / reject decisions", () => {
    const second: ObjectiveDraft = { ...content, index: 1, code: "C2", text: "I can compare two farms." };
    const choices = {
      0: { decision: "revise" as const, text: "I can explain why bees matter." },
      1: { decision: "rewrite" as const, text: "  " },
      2: { decision: "rewrite" as const, text: "I can reread hard sentences." },
    };
    const drafts = [content, second, language];
    expect(fieldsFrom(drafts, choices)).toEqual({
      content: "I can explain why bees matter.",
      language: "I can reread hard sentences.",
    });
    expect(decisionsFor(drafts, choices, false)).toEqual([
      { item_index: 0, decision: "revise", revised_content: "I can explain why bees matter." },
      // Rewritten as nothing: the draft is turned down, nothing takes its place.
      { item_index: 1, decision: "reject" },
      { item_index: 2, decision: "reject", reason: "better_idea", revised_content: "I can reread hard sentences." },
    ]);
    // Undecided counts as adopted; an edit that changed nothing is an adoption.
    expect(decisionsFor(drafts, { 0: { decision: "revise", text: content.text } }, false).map((d) => d.decision)).toEqual([
      "adopt",
      "adopt",
      "adopt",
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
      kind: "activity",
      request: "課堂活動",
      material: { title: "", text: EXEMPLAR_TEXT },
      options: { language_objective: "I can use because." },
    });
  });

  it("both empty: Manna drafts them; 全部採用並產生 adopts all in one click and records it", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText();
    await user.click(generate());

    const drafts = panel();
    // The page's own key for asking how far the drafting is (docs/API.md「產生進度」).
    expect(api.lastBody("POST", DRAFTS)).toEqual({
      kind: "comprehension",
      material: { title: "", text: EXEMPLAR_TEXT },
      progress_id: expect.stringMatching(/^[A-Za-z0-9_-]{8,64}$/),
    });
    expect(await drafts.findByText(content.text)).toBeInTheDocument();
    expect(drafts.getByText(content.zh)).toBeInTheDocument();
    expect(drafts.getByText(`怎麼檢核：${content.check_zh}`)).toBeInTheDocument();
    // The 語言目標: its kind (strategy first) and how it serves the 內容目標.
    const l1 = within(drafts.getByRole("group", { name: "語言目標 L1" }));
    expect(l1.getByText(/語言目標 · 學習策略/)).toBeInTheDocument();
    expect(l1.getByText(language.relation_zh!)).toBeInTheDocument();
    // Nothing is made yet; 產生 waits for the panel.
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
    expect(generate()).toBeDisabled();
    // and says why, instead of leaving a silent grey button.
    expect(generate()).toHaveAccessibleDescription(/請先在上方選好目標/);

    await user.click(drafts.getByRole("button", { name: "全部採用並產生" }));
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

  it("修改 and 改寫 one by one, then 用這些目標產生; the fields show what was chosen", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user } = await openWithText("/create/worksheet");
    await user.click(generate());
    const drafts = panel();
    await drafts.findByText(content.text);

    await user.click(drafts.getByRole("button", { name: "修改內容目標 C1" }));
    const edit = drafts.getByRole("textbox", { name: "修改內容目標 C1" });
    expect(edit).toHaveValue(content.text);
    await user.clear(edit);
    await user.type(edit, "I can explain why bees matter.");
    await user.click(drafts.getByRole("button", { name: "確定" }));
    expect(within(drafts.getByRole("group", { name: "內容目標 C1" })).getByText("已修改")).toBeInTheDocument();

    await user.click(drafts.getByRole("button", { name: "改寫語言目標 L1" }));
    const own = drafts.getByRole("textbox", { name: "改寫語言目標 L1" });
    expect(own).toHaveValue("");
    await user.type(own, "I can reread hard sentences.");
    await user.click(drafts.getByRole("button", { name: "確定" }));
    expect(drafts.getByText("I can reread hard sentences.")).toBeInTheDocument();

    await user.click(drafts.getByRole("button", { name: "用這些目標產生" }));
    await waitFor(() => expect(api.callsTo("POST", SESSIONS)).toHaveLength(1));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "revise", revised_content: "I can explain why bees matter." },
      { item_index: 2, decision: "reject", reason: "better_idea", revised_content: "I can reread hard sentences." },
    ]);
    expect(api.lastBody("POST", SESSIONS)).toMatchObject({
      options: {
        content_objective: "I can explain why bees matter.",
        language_objective: "I can reread hard sentences.",
      },
    });
  });

  it("重選 undoes a choice; 換一組 asks again without the lines shown; 收起 goes back to the form", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user } = await openWithText();
    await user.click(generate());
    const drafts = panel();
    await drafts.findByText(content.text);
    await user.click(drafts.getByRole("button", { name: "採用內容目標 C1" }));
    expect(drafts.getByRole("button", { name: "用這些目標產生" })).toBeInTheDocument();
    await user.click(drafts.getByRole("button", { name: "重選" }));
    expect(drafts.getByRole("button", { name: "全部採用並產生" })).toBeInTheDocument();

    await user.click(drafts.getByRole("button", { name: "換一組" }));
    await waitFor(() => expect(api.callsTo("POST", DRAFTS)).toHaveLength(2));
    expect(api.lastBody("POST", DRAFTS)).toMatchObject({ avoid: [content.text, language.text] });

    await user.click(await panel().findByRole("button", { name: "收起" }));
    expect(screen.queryByRole("region", { name: "Manna 先幫你草擬目標" })).not.toBeInTheDocument();
    expect(generate()).toBeEnabled();
    expect(api.callsTo("POST", DECISION)).toHaveLength(0);
    expect(api.callsTo("POST", SESSIONS)).toHaveLength(0);
  });

  it("不用目標，直接產生 records a skip for every draft and sends no objectives", async () => {
    signIn();
    const api = mockApi(routes({ [`POST ${DRAFTS}`]: shown() }));
    const { user, router } = await openWithText("/create/kit");
    await user.click(generate());
    await panel().findByText(content.text);
    await user.click(panel().getByRole("button", { name: "不用目標，直接產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION).map((c) => c.body)).toEqual([
      { item_index: 0, decision: "skip" },
      { item_index: 2, decision: "skip" },
    ]);
    expect(api.lastBody("POST", SESSIONS)).not.toHaveProperty("options.content_objective");
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
    expect(screen.queryByRole("region", { name: "Manna 先幫你草擬目標" })).not.toBeInTheDocument();
    await user.clear(screen.getByRole("textbox", { name: "內容目標（選填）" }));

    failing = true;
    await user.click(generate());
    expect(await panel().findByText(/伺服器發生錯誤/)).toBeInTheDocument();
    await user.click(panel().getByRole("button", { name: "不用目標，直接產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.callsTo("POST", DECISION)).toHaveLength(0);
  });
});

describe("首頁: a tool from the quick box drafts objectives first", () => {
  it("the typed request goes with the draft; adopting all makes the session with them", async () => {
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
    await user.click(within(screen.getByRole("group", { name: "要做什麼" })).getByRole("button", { name: "課堂活動" }));
    await user.type(screen.getByRole("textbox", { name: "給 Manna 的要求" }), "多一點討論");
    await user.click(screen.getByRole("button", { name: "送出" }));

    await panel().findByText(content.text);
    expect(api.lastBody("POST", DRAFTS)).toEqual({
      kind: "activity",
      material: { title: "", text: "Bees help plants." },
      request: "多一點討論",
      progress_id: expect.any(String),
    });
    await user.click(panel().getByRole("button", { name: "全部採用並產生" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/prep/7"));
    expect(api.lastBody("POST", SESSIONS)).toEqual({
      kind: "activity",
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
