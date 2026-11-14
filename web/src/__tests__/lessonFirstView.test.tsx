// Lesson page first view (docs/UX_REVIEW.md R1) and never losing unsaved work there (R2).
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { LessonRun, RunDetail, SaveLessonVersionRequest, UpdateLessonRequest } from "../api/types";
import {
  baseRoutes,
  coreVersion,
  focus,
  lessonDetail,
  mockApi,
  openSection,
  preferences,
  renderApp,
  respond,
  sectionToggle,
  signIn,
  step,
  todayCard,
  type MockCall,
} from "../test/mockApi";

const VERSIONS = "/api/lessons/7/versions";
const START = "/api/lessons/7/runs";

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

/** A lesson that was created but never saved: no plan version, no steps. */
const blankLesson = lessonDetail({ latest_version: null, versions_count: 0, class_description: "", effective_class_profile: "" });

function routes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/lessons/7": blankLesson,
    "GET /api/units/1/core": coreVersion([focus(11, 1, "倒裝句的語意功能")]),
    "GET /api/siop": [{ id: 1, internal_id: "SIOP-01", component: "Lesson Preparation", summary: "清楚寫出語言目標" }],
    "GET /api/teacher-cases": [],
    "GET /api/lessons/7/runs": [],
    [`POST ${START}`]: run,
    "GET /api/runs/9": runDetail,
    [`POST ${VERSIONS}`]: (call: MockCall) => ({
      id: 71,
      version_no: 1,
      objective: (call.body as SaveLessonVersionRequest).objective,
      steps: (call.body as SaveLessonVersionRequest).steps,
      created_at: "2026-11-13T08:00:00Z",
    }),
    ...extra,
  };
}

async function openLesson() {
  const rendered = renderApp("/lessons/7");
  expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
  return rendered;
}

const objectiveField = () => screen.getByRole("textbox", { name: "今天要教什麼？" });
const startButton = () => todayCard().getByRole("button", { name: "開始上課" });
const callIndex = (api: ReturnType<typeof mockApi>, method: string, path: string) =>
  api.calls.findIndex((c) => c.method === method && c.path === path);

describe("lesson page first view (R1)", () => {
  it("asks only 今天要教什麼？ with 開始本節課 right below; everything else is collapsed with counts", async () => {
    signIn();
    mockApi(routes());
    await openLesson();

    const today = todayCard();
    const objective = objectiveField();
    const start = startButton();
    expect(today.getAllByRole("textbox")).toEqual([objective]);
    expect(objective.compareDocumentPosition(start) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(today.getByText("開始上課時會先幫你儲存。上課中不會呼叫 AI。")).toBeInTheDocument();

    // CoRe links, steps and the other aids: collapsed, with counts, after the start button.
    for (const [title, label] of [
      ["單元想法的重點", "單元想法的重點（0）"],
      ["教學步驟", "教學步驟（0）"],
      ["本課背景", "本課背景：班級特性與我的範例"],
    ] as const) {
      const toggle = sectionToggle(title);
      expect(toggle).toHaveAccessibleName(label);
      expect(toggle).toHaveAttribute("aria-expanded", "false");
      expect(start.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    expect(await screen.findByRole("button", { name: "SIOP 提示（1）" })).toHaveAttribute("aria-expanded", "false");
    // No approved teacher cases: no empty section.
    await waitFor(() => expect(screen.queryByRole("button", { name: /^其他老師怎麼教/ })).not.toBeInTheDocument());

    // Hidden, not removed: nothing to fill in until a section is opened.
    expect(screen.queryByRole("button", { name: "＋ 新增步驟" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "本課班級描述（覆寫單元設定）" })).not.toBeInTheDocument();
  });

  it("keeps every tool reachable: opening 教學步驟 shows the editor and the count follows the steps", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    await openLesson();

    await openSection(user, "教學步驟");
    expect(sectionToggle("教學步驟")).toHaveAttribute("aria-expanded", "true");
    await user.click(screen.getByRole("button", { name: "＋ 新增步驟" }));
    await user.click(screen.getByRole("button", { name: "＋ 新增步驟" }));
    expect(sectionToggle("教學步驟")).toHaveAccessibleName("教學步驟（2）");
    expect(screen.getByRole("button", { name: "顯示時間與差異化欄位" })).toBeInTheDocument();

    await user.click(sectionToggle("教學步驟"));
    expect(sectionToggle("教學步驟")).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: "＋ 新增步驟" })).not.toBeInTheDocument();
    // Collapsing does not throw the steps away.
    await user.click(sectionToggle("教學步驟"));
    expect(screen.getByLabelText("步驟 2")).toBeInTheDocument();
  });

  it("opens 教學步驟 by default when the lesson already has steps (still below 開始本節課)", async () => {
    signIn();
    mockApi(
      routes({
        "GET /api/lessons/7": lessonDetail({
          latest_version: {
            id: 70,
            version_no: 2,
            objective: "學生能說明倒裝的語意功能",
            steps: [step({ position: 1, content: "比較兩組句子" }), step({ position: 2, content: "兩兩改寫" })],
            created_at: "2026-11-11T08:00:00Z",
          },
          core_focus_ids: [11],
        }),
      }),
    );
    await openLesson();
    const steps = sectionToggle("教學步驟");
    expect(steps).toHaveAccessibleName("教學步驟（2）");
    expect(steps).toHaveAttribute("aria-expanded", "true");
    expect(startButton().compareDocumentPosition(steps) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByLabelText("步驟 2")).toBeInTheDocument();
    expect(objectiveField()).toHaveValue("學生能說明倒裝的語意功能");
    // Linked focuses are counted before the section is opened.
    expect(sectionToggle("單元想法的重點")).toHaveAccessibleName("單元想法的重點（1）");
    expect(sectionToggle("單元想法的重點")).toHaveAttribute("aria-expanded", "false");
  });

  it("past runs sit in a collapsed 上課紀錄與課後補寫 section; with a run in progress the button resumes it", async () => {
    signIn();
    const api = mockApi(
      routes({
        "GET /api/lessons/7/runs": [run, { ...run, id: 8, ended_at: "2026-11-11T02:00:00Z", good_count: 2, stuck_count: 1 }],
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    const resume = await todayCard().findByRole("button", { name: "回到課堂" });
    expect(sectionToggle("上課紀錄與課後補寫")).toHaveAccessibleName("上課紀錄與課後補寫（1）");
    await openSection(user, "上課紀錄與課後補寫");
    expect(screen.getByRole("link", { name: /順利 2 · 卡住 1 · 課後補寫/ })).toHaveAttribute("href", "/runs/8/review");

    await user.click(resume);
    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.callsTo("POST", START)).toHaveLength(0);
  });
});

describe("開始本節課 never drops unsaved changes (R2a, regression for docs/UX_REVIEW.md F2)", () => {
  it("saves the typed objective as a new plan version first, then starts the run", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();

    // The reported case: type the objective, do not press 儲存教案, press 開始本節課.
    await user.type(objectiveField(), "學生能用 Never 開頭寫倒裝句");
    expect(screen.getByText("尚未儲存")).toBeInTheDocument();
    await user.click(startButton());

    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.callsTo("POST", VERSIONS)).toHaveLength(1);
    expect(api.lastBody("POST", VERSIONS)).toEqual({ objective: "學生能用 Never 開頭寫倒裝句", steps: [] });
    expect(api.callsTo("POST", START)).toHaveLength(1);
    // The version exists before the run, so the run records it.
    expect(callIndex(api, "POST", VERSIONS)).toBeLessThan(callIndex(api, "POST", START));
    // Nothing was left unsaved, so leaving did not ask.
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("does not start when saving fails, and says why", async () => {
    signIn();
    const api = mockApi(
      routes({ [`POST ${VERSIONS}`]: respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } }) }),
    );
    const user = userEvent.setup();
    await openLesson();
    await user.type(objectiveField(), "目標");
    await user.click(startButton());

    expect(await todayCard().findByText("教案沒有儲存成功，所以還沒開始上課：伺服器暫時發生問題")).toBeInTheDocument();
    expect(api.callsTo("POST", START)).toHaveLength(0);
    expect(screen.getByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
    expect(objectiveField()).toHaveValue("目標");
    expect(screen.getByText("尚未儲存")).toBeInTheDocument();
  });

  it("starts without a new version when nothing changed", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    await user.click(startButton());
    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.callsTo("POST", VERSIONS)).toHaveLength(0);
    expect(api.callsTo("POST", START)).toHaveLength(1);
  });

  it("also saves unsaved 本課背景 edits before starting", async () => {
    signIn();
    const api = mockApi(
      routes({
        "PUT /api/lessons/7": (call: MockCall) =>
          lessonDetail({ ...blankLesson, class_description: (call.body as UpdateLessonRequest).class_description ?? "" }),
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    await openSection(user, "本課背景");
    await user.type(screen.getByLabelText("本課班級描述（覆寫單元設定）"), "今天只有半班");
    await user.click(startButton());

    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.lastBody("PUT", "/api/lessons/7")).toEqual({ class_description: "今天只有半班", my_example: "" });
    expect(callIndex(api, "PUT", "/api/lessons/7")).toBeLessThan(callIndex(api, "POST", START));
    expect(api.callsTo("POST", VERSIONS)).toHaveLength(0);
  });
});

describe("leaving the lesson page with unsaved changes (R2b)", () => {
  it("asks in the page before an in-app navigation: stay keeps the edits, leave drops them", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openLesson();
    await user.type(objectiveField(), "還沒存的目標");

    await user.click(screen.getByRole("link", { name: "班級" }));
    const dialog = within(await screen.findByRole("alertdialog", { name: "還有沒儲存的內容" }));
    expect(dialog.getByText("離開這頁，尚未儲存的教案或本課背景就會遺失。")).toBeInTheDocument();
    expect(dialog.getByRole("button", { name: "留在這頁" })).toHaveFocus();
    await user.click(dialog.getByRole("button", { name: "留在這頁" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/lessons/7");
    expect(objectiveField()).toHaveValue("還沒存的目標");

    // Escape stays too.
    await user.click(screen.getAllByRole("link", { name: "備課庫" })[0] as HTMLElement);
    await screen.findByRole("alertdialog");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    await user.click(screen.getByRole("link", { name: "班級" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "不儲存，直接離開" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(api.callsTo("POST", VERSIONS)).toHaveLength(0);
  });

  it("「儲存後離開」 saves a new version, then goes on", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openLesson();
    await user.type(objectiveField(), "要存的目標");
    await user.click(screen.getByRole("link", { name: "班級" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "儲存後離開" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(api.lastBody("POST", VERSIONS)).toEqual({ objective: "要存的目標", steps: [] });
  });

  it("stays on the page when 「儲存後離開」 cannot save", async () => {
    signIn();
    mockApi(routes({ [`POST ${VERSIONS}`]: respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } }) }));
    const user = userEvent.setup();
    const { router } = await openLesson();
    await user.type(objectiveField(), "目標");
    await user.click(screen.getByRole("link", { name: "班級" }));
    const dialog = within(await screen.findByRole("alertdialog"));
    await user.click(dialog.getByRole("button", { name: "儲存後離開" }));
    expect(await dialog.findByText("沒有儲存成功，所以還沒離開：伺服器暫時發生問題")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/lessons/7");
  });

  it("does not ask once the changes are saved, and warns on tab close only while something is unsaved", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openLesson();
    const unload = () => {
      const event = new Event("beforeunload", { cancelable: true });
      act(() => {
        window.dispatchEvent(event);
      });
      return event.defaultPrevented;
    };
    expect(unload()).toBe(false);
    await user.type(objectiveField(), "目標");
    expect(unload()).toBe(true);

    await user.click(todayCard().getByRole("button", { name: "儲存教案" }));
    expect(await screen.findByText(/已儲存.*第 1 版/)).toBeInTheDocument();
    expect(unload()).toBe(false);
    await user.click(screen.getByRole("link", { name: "班級" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
});

describe("preference defaults inside the collapsed sections", () => {
  it("差異化 opens the time fields once the steps are opened, and logs SC-PREF show only then", async () => {
    signIn();
    const api = mockApi({ ...routes(), "GET /api/me/preferences": preferences({ hardest: ["differentiation"] }) });
    const user = userEvent.setup();
    await openLesson();
    const prefShows = () =>
      api
        .callsTo("POST", "/api/scaffold-events")
        .map((c) => c.body as { scaffold_code: string; action: string; trigger?: string })
        .filter((e) => e.scaffold_code === "SC-PREF" && e.action === "show");

    // Give the lesson page's effects time to run: still collapsed, so nothing shown yet.
    await screen.findByRole("button", { name: "SIOP 提示（1）" });
    expect(prefShows()).toEqual([]);

    await openSection(user, "教學步驟");
    await user.click(screen.getByRole("button", { name: "＋ 新增步驟" }));
    expect(within(screen.getByLabelText("步驟 1")).getByLabelText("預估時間（分鐘）")).toBeInTheDocument();
    expect(screen.getByText("依你的備課偏好預設顯示")).toBeInTheDocument();
    await waitFor(() =>
      expect(prefShows()).toEqual([
        { scaffold_code: "SC-PREF", action: "show", trigger: "preference:hardest=differentiation" },
      ]),
    );
  });
});
