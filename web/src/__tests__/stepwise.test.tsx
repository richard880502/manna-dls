// 我想逐步設計 (the home page's third card): the card and its words, the unit dialog (an existing
// unit, a new unit, the attached article going in as step 1's material, cancel and the dirty guard),
// the log's entry, and the lessons listed in 我的作品.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, describe, expect, it } from "vitest";
import type { Lesson, StepwiseLesson } from "../api/types";
import { StepwiseDialog } from "../components/home/StepwiseDialog";
import { pickedOf } from "../components/library/LibraryPicker";
import type { MaterialChoice } from "../components/material/MaterialChooser";
import { stepwiseProgress } from "../lib/sop";
import { baseRoutes, mockApi, renderApp, respond, signIn, unit, unitWith } from "../test/mockApi";

const HOME = "今天想準備什麼？";
const PREP = "/api/prep-sessions";

beforeEach(() => signIn());

const lesson = (overrides: Partial<Lesson> = {}): Lesson => ({
  id: 31,
  unit_id: 1,
  title: "第 2 課",
  lesson_date: null,
  class_description: "",
  created_at: "2026-11-15T08:00:00Z",
  my_example: "",
  ...overrides,
});

const row = (overrides: Partial<StepwiseLesson> = {}): StepwiseLesson => ({
  id: 31,
  title: "The Umbrella Stand",
  unit_id: 1,
  unit_title: "倒裝句單元",
  updated_at: "2026-11-15T09:00:00Z",
  steps_done: 3,
  next_step: 4,
  ...overrides,
});

const tiles = () => within(screen.getByRole("radiogroup", { name: "要做什麼" }));
const box = () => screen.getByRole("textbox", { name: "給 Manna 的要求" });
const dialog = () => within(screen.getByRole("dialog", { name: "逐步設計：放進哪個單元？" }));

describe("the 我想逐步設計 card", () => {
  it("is the third card, with a blurb that says the teacher decides at each step", async () => {
    mockApi(baseRoutes());
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    const radios = tiles().getAllByRole("radio");
    expect(radios.map((r) => r.querySelector(".tool-card-title")?.textContent)).toEqual([
      "AI 幫我設計",
      "照我的範本設計",
      "我想逐步設計",
    ]);
    expect(radios[2]).toHaveTextContent("跟著七個步驟一步步設計，每一步由你決定，Manna 從旁建議");
  });

  it("sends to the unit dialog, not to the generator: no session is made and 更多設定 is hidden", async () => {
    const api = mockApi(baseRoutes());
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(tiles().getByRole("radio", { name: "我想逐步設計" }));
    expect(screen.queryByRole("link", { name: /更多設定/ })).not.toBeInTheDocument();
    // Nothing needs to be typed or attached.
    expect(screen.getByRole("button", { name: "送出" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "送出" }));
    expect(await screen.findByRole("dialog", { name: "逐步設計：放進哪個單元？" })).toBeInTheDocument();
    expect(api.callsTo("POST", PREP)).toHaveLength(0);
  });

  it("an existing unit: the lesson is made there (entry home) and opens, with no material put in", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [unitWith({ id: 4, title: "新的單元", lesson_count: 0 }), unit],
      "POST /api/units/1/lessons": lesson({ id: 31, title: "第 2 課" }),
      "GET /api/lessons/31": respond(404, { error: { code: "not_found", message: "x" } }),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(tiles().getByRole("radio", { name: "我想逐步設計" }));
    await user.click(screen.getByRole("button", { name: "送出" }));
    const d = dialog();
    await screen.findByRole("option", { name: "倒裝句單元" });
    await user.selectOptions(d.getByRole("combobox", { name: "單元" }), "1");
    // The default name continues the unit's count (the unit has one lesson).
    expect(d.getByRole("textbox", { name: "課例名稱" })).toHaveValue("第 2 課");
    await user.click(d.getByRole("button", { name: "開始逐步設計" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/lessons/31"));
    expect(api.lastBody("POST", "/api/units/1/lessons")).toEqual({ title: "第 2 課", entry: "home" });
    expect(api.callsTo("POST", "/api/units")).toHaveLength(0);
    expect(api.callsTo("PUT", "/api/lessons/31/sop/material")).toHaveLength(0);
  });

  it("a new unit with a pasted article: the unit takes the article's title and the text goes in as step 1", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [unit],
      "POST /api/units": unitWith({ id: 9, title: "Umbrella" }),
      "POST /api/units/9/lessons": lesson({ id: 40, unit_id: 9, title: "Umbrella" }),
      "PUT /api/lessons/40/sop/material": {},
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(screen.getByRole("button", { name: "附上文章" }));
    await user.click(screen.getByRole("button", { name: "貼上文字" }));
    const paste = within(screen.getByRole("dialog", { name: "貼上文字" }));
    await user.type(paste.getByRole("textbox", { name: /教材標題/ }), "Umbrella");
    await user.type(paste.getByRole("textbox", { name: "貼上教材文字" }), "Mrs. Lin put a stand outside.");
    await user.click(paste.getByRole("button", { name: "附上" }));
    await user.click(tiles().getByRole("radio", { name: "我想逐步設計" }));
    await user.click(screen.getByRole("button", { name: "送出" }));
    const d = dialog();
    await user.click(await d.findByRole("radio", { name: "建立新單元" }));
    expect(d.getByRole("textbox", { name: "新單元的名稱" })).toHaveValue("Umbrella");
    expect(d.getByRole("textbox", { name: "課例名稱" })).toHaveValue("Umbrella");
    await user.click(d.getByRole("button", { name: "開始逐步設計" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/lessons/40"));
    expect(api.lastBody("POST", "/api/units")).toEqual({ title: "Umbrella" });
    expect(api.lastBody("POST", "/api/units/9/lessons")).toEqual({ title: "Umbrella", entry: "home" });
    expect(api.lastBody("PUT", "/api/lessons/40/sop/material")).toEqual({
      title: "Umbrella",
      text: "Mrs. Lin put a stand outside.",
    });
  });

  it("the typed words name the lesson, and a new unit needs a name when nothing is attached", async () => {
    const api = mockApi({ ...baseRoutes(), "GET /api/units": [] });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(tiles().getByRole("radio", { name: "我想逐步設計" }));
    await user.type(box(), "第三課 讀報");
    await user.click(screen.getByRole("button", { name: "送出" }));
    const d = dialog();
    // No units yet: only the new-unit choice is offered.
    expect(await d.findByRole("textbox", { name: "新單元的名稱" })).toHaveValue("第三課 讀報");
    await user.clear(d.getByRole("textbox", { name: "新單元的名稱" }));
    await user.click(d.getByRole("button", { name: "開始逐步設計" }));
    expect(await d.findByText("請輸入單元名稱。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/units")).toHaveLength(0);
  });

  it("Escape closes a clean dialog; after typing it asks before throwing the input away", async () => {
    mockApi({ ...baseRoutes(), "GET /api/units": [unit] });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: HOME, level: 1 });
    await user.click(tiles().getByRole("radio", { name: "我想逐步設計" }));
    await user.click(screen.getByRole("button", { name: "送出" }));
    await screen.findByRole("dialog", { name: "逐步設計：放進哪個單元？" });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "送出" }));
    const d = dialog();
    await user.type(await d.findByRole("textbox", { name: "課例名稱" }), "加字");
    await user.keyboard("{Escape}");
    expect(screen.getByText("要放棄剛才輸入的內容嗎？")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "繼續編輯" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.click(d.getByRole("button", { name: "取消" }));
    await user.click(screen.getByRole("button", { name: "放棄並關閉" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("the unit dialog with a library article", () => {
  it("puts the library material in as step 1 by its id; a failed material step does not make a second lesson", async () => {
    let failMaterial = true;
    const api = mockApi({
      "GET /api/units": [unit],
      "POST /api/units/1/lessons": lesson({ id: 50, title: "The Umbrella Stand" }),
      "PUT /api/lessons/50/sop/material": () => {
        if (failMaterial) {
          failMaterial = false;
          return respond(500, { error: { code: "internal", message: "教材放不進去。" } });
        }
        return {};
      },
    });
    const choice: MaterialChoice = {
      way: "library",
      picked: pickedOf({ id: 5, title: "The Umbrella Stand" }),
      title: "",
      text: "",
    };
    const user = userEvent.setup();
    render(
      <MemoryRouter initialEntries={["/"]}>
        <Routes>
          <Route path="/" element={<StepwiseDialog choice={choice} typed="" onClose={() => undefined} />} />
          <Route path="/lessons/:id" element={<p>課例頁</p>} />
        </Routes>
      </MemoryRouter>,
    );
    await user.click(await screen.findByRole("button", { name: "開始逐步設計" }));
    expect(await screen.findByText("教材放不進去。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "開始逐步設計" }));
    expect(await screen.findByText("課例頁")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/units/1/lessons")).toHaveLength(1);
    expect(api.lastBody("PUT", "/api/lessons/50/sop/material")).toEqual({ material_id: 5 });
  });
});

describe("我的作品 with 逐步設計 lessons", () => {
  const sessions = {
    sessions: [
      {
        id: 7,
        title: "暖身遊戲",
        material: null,
        topic: "暖身遊戲",
        class: null,
        template: null,
        turn_count: 1,
        last_kind: "activity",
        preview_worksheet: null,
        preview_text: null,
        updated_at: "2026-11-12T09:00:00Z",
        created_at: "2026-11-12T08:00:00Z",
      },
    ],
  };

  it("mixes lessons in by last update, with the tag, the unit and where the lesson stands, and no 刪除", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions": sessions,
      "GET /api/worksheets": [],
      "GET /api/stepwise-lessons": [
        row({ id: 31, title: "The Umbrella Stand", updated_at: "2026-11-15T09:00:00Z" }),
        row({ id: 32, title: "做完的一課", next_step: null, steps_done: 7, updated_at: "2026-11-10T09:00:00Z" }),
      ],
    });
    const user = userEvent.setup();
    renderApp("/prep");
    const list = await screen.findByRole("list", { name: "我的作品" });
    const names = within(list)
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(names).toEqual(["The Umbrella Stand", "暖身遊戲", "做完的一課"]);
    const first = within(list).getByRole("link", { name: "The Umbrella Stand" });
    expect(first).toHaveAttribute("href", "/lessons/31");
    expect(first).toHaveAccessibleDescription(/逐步設計.*倒裝句單元 · 第 4 步：逐段整合目標/);
    expect(within(list).getByRole("link", { name: "做完的一課" })).toHaveAccessibleDescription(/已完成/);
    // Sessions keep their 刪除; the lessons have none.
    expect(within(list).getAllByRole("button", { name: /^刪除/ })).toHaveLength(1);

    // The 逐步設計 chip shows only the lessons.
    await user.click(screen.getByRole("button", { name: /^逐步設計/ }));
    expect(
      within(screen.getByRole("list", { name: "我的作品" }))
        .getAllByRole("heading", { level: 3 })
        .map((h) => h.textContent),
    ).toEqual(["The Umbrella Stand", "做完的一課"]);
  });

  it("says where a lesson stands in words", () => {
    expect(stepwiseProgress(1)).toBe("第 1 步：閱讀教材");
    expect(stepwiseProgress(7)).toBe("第 7 步：Exit ticket");
    expect(stepwiseProgress(null)).toBe("已完成");
  });
});
