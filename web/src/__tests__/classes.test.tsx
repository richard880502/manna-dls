// 班級與備課庫 (docs/API.md Phase 1.4, docs/DESIGN_OVERHAUL.md step 1): the class-centred home,
// the class page, the library, the class as context on unit and lesson pages (never a field), and
// the one place a class is asked for: starting a lesson from the library.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ClassDetail, CreateClassRequest, LessonRun, RunDetail, SchoolClass } from "../api/types";
import { emptyContent } from "../lib/sop";
import { CURRENT_CLASS_STORAGE_KEY } from "../lib/storage";
import {
  baseRoutes,
  classDetail,
  classRef,
  classUnit,
  lessonDetail,
  mockApi,
  preferences,
  renderApp,
  respond,
  schoolClass,
  signIn,
  unitWith,
  type MockCall,
} from "../test/mockApi";

const class3 = schoolClass({ id: 3, name: "高一 3 班", class_profile: "三班：不太敢開口" });
const class7 = schoolClass({ id: 7, name: "高一 7 班", class_profile: "七班：活潑、愛發言", position: 2 });

/** A unit used by both classes; class 7 keeps a note on it. */
const shared = unitWith({ id: 1, title: "L3 倒裝句", classes: [classRef(class3), classRef(class7, "這班用基礎版")] });
const onlyClass3 = unitWith({ id: 1, title: "L3 倒裝句", classes: [classRef(class3)] });
const inLibraryOnly = unitWith({ id: 2, title: "L4 假設語氣", lesson_count: 0, core_status: "none" });

const run = (overrides: Partial<LessonRun> = {}): LessonRun => ({
  id: 9,
  lesson_id: 7,
  lesson_title: "第 1 課：Not only… but also",
  unit_id: 1,
  unit_title: "L3 倒裝句",
  started_at: "2026-11-12T01:00:00Z",
  ended_at: null,
  review_completed_at: null,
  good_count: 0,
  stuck_count: 0,
  class_id: null,
  class_name: null,
  ...overrides,
});
const runDetail = (r: LessonRun): RunDetail => ({ ...r, events: [], plan: null, remotes: [] });
const ended = { ended_at: "2026-11-12T02:00:00Z" };

/** Everything the lesson page of lesson 7 (unit 1) loads. */
function lessonRoutes(unit = shared, extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/units/1": unit,
    "GET /api/lessons/7": lessonDetail({ class_description: "", effective_class_profile: "" }),
    "GET /api/lessons/7/runs": [],
    "POST /api/lessons/7/runs": (call: MockCall) => {
      const classId = (call.body as { class_id?: number }).class_id ?? null;
      return run({ class_id: classId, class_name: classId === 7 ? "高一 7 班" : classId === 3 ? "高一 3 班" : null });
    },
    "GET /api/runs/9": runDetail(run({ class_id: 7, class_name: "高一 7 班" })),
    ...extra,
  };
}

/** The class is context: no page offers a field to pick it. */
const expectNoClassField = () => {
  expect(screen.queryByRole("combobox", { name: /班級/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "目前班級" })).not.toBeInTheDocument();
};

describe("班級 (home)", () => {
  it("leads with one next lesson, lists the classes as rows, and shows what is not written up yet", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/classes": [
        schoolClass({
          ...class3,
          course_label: "英文閱讀選修",
          unit_count: 2,
          next_action: { kind: "start_lesson", lesson_id: 8, lesson_title: "第 2 節", unit_id: 1, unit_title: "L3 倒裝句" },
        }),
        schoolClass({
          ...class7,
          next_action: { kind: "resume_run", run_id: 9, lesson_id: 7, lesson_title: "第 1 節", unit_id: 1, unit_title: "L3 倒裝句" },
        }),
        schoolClass({ id: 8, name: "高二 1 班", unit_count: 0, next_action: { kind: "add_unit" } }),
      ],
      "GET /api/runs": [
        run({ id: 9, class_id: 7, class_name: "高一 7 班" }),
        run({ id: 22, lesson_title: "第 1 節", class_id: 3, class_name: "高一 3 班", good_count: 4, stuck_count: 2, ...ended }),
        run({ id: 23, class_id: 3, class_name: "高一 3 班", review_completed_at: "2026-11-12T03:00:00Z", ...ended }),
      ],
    });
    renderApp("/");

    // The one focal block: the class that is in a lesson right now, and its one filled action.
    const next = within(await screen.findByRole("region", { name: /L3 倒裝句\s+第 1 節/ }));
    expect(next.getByText("上課中 · 高一 7 班")).toBeInTheDocument();
    expect(next.getByRole("heading", { level: 1 })).toHaveTextContent(/L3 倒裝句\s+第 1 節/);
    expect(next.getByRole("link", { name: "回到課堂" })).toHaveAttribute("href", "/runs/9");
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);

    const list = within(screen.getByRole("region", { name: "我的班級" }));
    const rows = list.getAllByRole("listitem");
    expect(rows).toHaveLength(3);

    const first = within(rows[0] as HTMLElement);
    expect(first.getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
    expect(first.getByText("高一 · 普高 · 英文閱讀選修 · 2 個單元")).toBeInTheDocument();
    expect(first.getByText("下一節：第 2 節")).toBeInTheDocument();
    expect(first.getByRole("button", { name: "高一 3 班：開始上課" })).toBeInTheDocument();

    // The class the page leads with does not repeat its action in its row.
    const second = within(rows[1] as HTMLElement);
    expect(second.getByText("上課中：第 1 節")).toBeInTheDocument();
    expect(second.queryByRole("link", { name: "回到課堂" })).not.toBeInTheDocument();

    const third = within(rows[2] as HTMLElement);
    expect(third.getByText(/還沒有單元/)).toBeInTheDocument();
    expect(third.getByRole("link", { name: "從備課庫加入單元" })).toHaveAttribute("href", "/classes/8");

    expect(list.getByRole("button", { name: "新增班級" })).toBeInTheDocument();

    // Taught, not written up yet: only the run without a completed review.
    const todo = within(screen.getByRole("region", { name: "上完還沒補寫" }));
    expect(todo.getAllByRole("listitem")).toHaveLength(1);
    expect(todo.getByText(/順利 4 次 · 卡住 2 次/)).toBeInTheDocument();
    expect(todo.getByRole("link", { name: /^補寫/ })).toHaveAttribute("href", "/runs/22/review");
    expect(todo.getByRole("link", { name: "全部上課紀錄" })).toHaveAttribute("href", "/record");
  });

  it("with no lesson in progress, leads with the class taught most recently", async () => {
    signIn();
    const next = (lessonId: number, title: string) =>
      ({ kind: "start_lesson", lesson_id: lessonId, lesson_title: title, unit_id: 1, unit_title: "L3 倒裝句" }) as const;
    mockApi({
      ...baseRoutes(),
      "GET /api/classes": [
        schoolClass({ ...class3, next_action: next(8, "第 2 節") }),
        schoolClass({ ...class7, next_action: next(9, "第 3 節") }),
      ],
      "GET /api/runs": [run({ id: 21, class_id: 7, class_name: "高一 7 班", ...ended })],
    });
    renderApp("/");
    const block = within(await screen.findByRole("region", { name: /第 3 節/ }));
    expect(block.getByText("接下來 · 高一 7 班")).toBeInTheDocument();
    expect(block.getByText("上一節 11/12 上完。")).toBeInTheDocument();
    expect(block.getByRole("button", { name: "開始上課" })).toBeInTheDocument();
    expect(block.getByRole("link", { name: "繼續備課" })).toHaveAttribute("href", "/lessons/9");
  });

  it("開始上課 starts in that class straight away: nothing is asked", async () => {
    signIn();
    const api = mockApi({
      ...lessonRoutes(),
      "GET /api/classes": [
        schoolClass({
          ...class7,
          next_action: { kind: "start_lesson", lesson_id: 7, lesson_title: "第 1 課", unit_id: 1, unit_title: "L3 倒裝句" },
        }),
      ],
    });
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await user.click(await screen.findByRole("button", { name: "開始上課" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 7 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBe("7");
  });

  it("繼續備課 opens the lesson in that class", async () => {
    signIn();
    mockApi({
      ...lessonRoutes(),
      "GET /api/classes": [
        schoolClass({
          ...class7,
          next_action: { kind: "start_lesson", lesson_id: 7, lesson_title: "第 1 課", unit_id: 1, unit_title: "L3 倒裝句" },
        }),
      ],
    });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("link", { name: "繼續備課" }));
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
    // The class appears once, in the breadcrumb.
    expect(await screen.findByRole("link", { name: "高一 7 班" })).toHaveAttribute("href", "/classes/7");
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBe("7");
    expectNoClassField();
  });

  it("shows legacy units under 「我的班級」 after the migration", async () => {
    signIn();
    const legacy = schoolClass({
      id: 1,
      name: "我的班級",
      grade: "",
      class_profile: "高二 207：最近的描述",
      class_profile_source: "migration",
      unit_count: 2,
    });
    const u1 = unitWith({ id: 1, title: "L1 舊單元", lesson_count: 2, classes: [classRef(legacy)] });
    const u2 = unitWith({ id: 2, title: "L2 較新的單元", lesson_count: 0, classes: [classRef(legacy)] });
    mockApi({
      ...baseRoutes(),
      "GET /api/classes": [legacy],
      "GET /api/classes/1": classDetail(legacy, [
        classUnit(u1, { lessons_taught: 1, last_run_at: "2026-11-12T01:00:00Z", next_lesson: { id: 8, title: "第 2 節" } }),
        classUnit(u2, { position: 2 }),
      ]),
    });
    const user = userEvent.setup();
    renderApp("/");

    const row = within((await screen.findByRole("heading", { name: "我的班級", level: 3 })).closest("li") as HTMLElement);
    expect(row.getByText(/2 個單元/)).toBeInTheDocument();
    await user.click(row.getByRole("link", { name: "我的班級" }));

    expect(await screen.findByRole("heading", { name: "我的班級", level: 1 })).toBeInTheDocument();
    const units = within(screen.getByRole("region", { name: "這班的單元" }));
    const rows = units.getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByRole("link", { name: "L1 舊單元" })).toHaveAttribute("href", "/units/1");
    expect(within(rows[0] as HTMLElement).getByText(/已上 1 節，共 2 節 · 最近 11\/12/)).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText("還沒有課例")).toBeInTheDocument();
    // The profile carried over from the legacy unit is the class's, and editable.
    const profile = within(screen.getByRole("region", { name: "班級特性" }));
    expect(profile.getByText("高二 207：最近的描述")).toBeInTheDocument();
    await user.click(profile.getByRole("button", { name: "修改" }));
    expect(screen.getByRole("textbox", { name: "這個班級的特性" })).toHaveValue("高二 207：最近的描述");
  });
});

describe("creating a class", () => {
  function routes() {
    const created = schoolClass({ id: 5, name: "高一 3 班", unit_count: 0, next_action: { kind: "add_unit" } });
    return {
      ...baseRoutes({}, preferences({ class_profile: "問卷寫的：高一普通班，程度落差大" })),
      "GET /api/units": [],
      "POST /api/classes": (call: MockCall) => ({ ...created, ...(call.body as CreateClassRequest) }),
      "GET /api/classes/5": classDetail(created, []),
    };
  }

  it("invites a teacher with no classes to create the first one, prefilled from P6", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/");

    const invite = within(await screen.findByRole("region", { name: "先建立你的第一個班級" }));
    expect(invite.getByRole("button", { name: "先略過" })).toBeInTheDocument();
    expect(invite.getByRole("textbox", { name: "班級特性（選填）" })).toHaveValue("問卷寫的：高一普通班，程度落差大");
    expect(invite.getByText(/已帶入你在備課偏好寫的班級特性，可修改。/)).toBeInTheDocument();
    expect(invite.getByText("請不要輸入學生姓名或其他個人資料。")).toBeInTheDocument();

    // The name is the only thing asked for.
    await user.click(invite.getByRole("button", { name: "建立班級" }));
    expect(invite.getByRole("alert")).toHaveTextContent("請輸入班級名稱。");
    expect(api.callsTo("POST", "/api/classes")).toHaveLength(0);

    await user.type(invite.getByRole("textbox", { name: "班級名稱" }), " 高一 3 班 ");
    await user.type(invite.getByRole("textbox", { name: "年級（選填）" }), "高一");
    await user.click(invite.getByRole("button", { name: "建立班級" }));

    // The untouched prefill is left to the server (which copies P6 and records where it came from).
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/classes")).toEqual({
        name: "高一 3 班",
        grade: "高一",
        school_track: "general",
        course_label: "",
        school_year: "",
        term: "",
      }),
    );
    // The new class opens, ready for its first unit.
    expect(await screen.findByRole("heading", { name: "高一 3 班", level: 1 })).toBeInTheDocument();
    expect(screen.getByText("這個班級還沒有單元。從備課庫加入，或新增一個單元。")).toBeInTheDocument();
  });

  it("sends an edited profile and the optional fields", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/");
    const invite = within(await screen.findByRole("region", { name: "先建立你的第一個班級" }));
    await user.type(invite.getByRole("textbox", { name: "班級名稱" }), "閱讀選修 A");
    const profile = invite.getByRole("textbox", { name: "班級特性（選填）" });
    await user.clear(profile);
    await user.type(profile, "跨班選修，24 人");
    await user.click(invite.getByText("課程名稱、學年度等（選填）"));
    await user.type(invite.getByRole("textbox", { name: "課程名稱" }), "英文閱讀選修");
    await user.selectOptions(invite.getByRole("combobox", { name: "學校類型" }), "技高");
    await user.type(invite.getByRole("textbox", { name: "學年度" }), "115");
    await user.type(invite.getByRole("textbox", { name: "學期" }), "上學期");
    await user.click(invite.getByRole("button", { name: "建立班級" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/classes")).toEqual({
        name: "閱讀選修 A",
        grade: "",
        school_track: "vocational",
        course_label: "英文閱讀選修",
        class_profile: "跨班選修，24 人",
        school_year: "115",
        term: "上學期",
      }),
    );
  });

  it("can be skipped: nothing is created and the library is still one click away", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("button", { name: "先略過" }));
    expect(screen.queryByRole("region", { name: "先建立你的第一個班級" })).not.toBeInTheDocument();
    const empty = within(screen.getByText(/還沒有班級。/));
    expect(empty.getByRole("link", { name: "備課庫" })).toHaveAttribute("href", "/units");
    expect(api.callsTo("POST", "/api/classes")).toHaveLength(0);
    // …and the invitation comes back on request.
    await user.click(empty.getByRole("button", { name: "新增班級" }));
    expect(screen.getByRole("region", { name: "先建立你的第一個班級" })).toBeInTheDocument();
  });

  it("a later class starts with an empty profile (P6 is only the first class's default)", async () => {
    signIn();
    const api = mockApi({ ...routes(), "GET /api/classes": [class3] });
    const user = userEvent.setup();
    renderApp("/");
    await user.click(await screen.findByRole("button", { name: "新增班級" }));
    const form = within(screen.getByRole("region", { name: "新增班級" }));
    expect(form.getByRole("textbox", { name: "班級特性（選填）" })).toHaveValue("");
    await user.type(form.getByRole("textbox", { name: "班級名稱" }), "高一 7 班");
    await user.click(form.getByRole("button", { name: "建立班級" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/classes")).toMatchObject({ name: "高一 7 班", class_profile: "" }));
  });
});

describe("class page", () => {
  function routes() {
    let detail: ClassDetail = classDetail(class3, [classUnit(onlyClass3)]);
    const set = (next: ClassDetail) => {
      detail = next;
      return detail;
    };
    return {
      ...baseRoutes(),
      "GET /api/units": [onlyClass3, inLibraryOnly],
      "GET /api/classes": [class3, class7],
      "GET /api/classes/3": () => detail,
      "POST /api/classes/3/units": (call: MockCall) => {
        const { unit_id } = call.body as { unit_id: number };
        if (unit_id !== 2) return respond(404, { error: { code: "not_found", message: "找不到這個單元。" } });
        return set(
          classDetail(class3, [
            ...detail.units,
            classUnit({ ...inLibraryOnly, classes: [classRef(class3)] }, { position: 2 }),
          ]),
        );
      },
      "PUT /api/classes/3/units/1": (call: MockCall) =>
        set({ ...detail, units: detail.units.map((u) => (u.unit.id === 1 ? { ...u, note: (call.body as { note: string }).note } : u)) }),
      "PUT /api/classes/3/units/order": (call: MockCall) => {
        const order = (call.body as { unit_ids: number[] }).unit_ids;
        return set({ ...detail, units: [...detail.units].sort((a, b) => order.indexOf(a.unit.id) - order.indexOf(b.unit.id)) });
      },
      "DELETE /api/classes/3/units/1": () => set(classDetail(class3, detail.units.filter((u) => u.unit.id !== 1))),
      "PUT /api/classes/3": (call: MockCall) => ({ ...class3, ...(call.body as Partial<SchoolClass>) }),
      "POST /api/classes/3/archive": { ...class3, archived_at: "2026-11-14T00:00:00Z" },
      "POST /api/units": (call: MockCall) => unitWith({ id: 4, ...(call.body as { title: string }), classes: [classRef(class3)] }),
      "GET /api/units/4": unitWith({ id: 4, title: "L5 分詞構句", lesson_count: 0, classes: [classRef(class3)] }),
      "GET /api/units/4/lessons": [],
      "GET /api/units/4/materials": [],
    };
  }

  const unitsSection = () => within(screen.getByRole("region", { name: "這班的單元" }));
  const unitRows = () => within(unitsSection().getByRole("list")).getAllByRole("listitem");

  async function openClass() {
    const rendered = renderApp("/classes/3");
    expect(await screen.findByRole("heading", { name: "高一 3 班", level: 1 })).toBeInTheDocument();
    return rendered;
  }

  it("leads with the class's next lesson and lists its lesson records", async () => {
    signIn();
    const api = mockApi({
      ...routes(),
      "GET /api/classes/3": classDetail(
        schoolClass({
          ...class3,
          next_action: { kind: "start_lesson", lesson_id: 7, lesson_title: "第 2 節", unit_id: 1, unit_title: "L3 倒裝句" },
        }),
        [classUnit(onlyClass3)],
      ),
      "GET /api/runs": (call: MockCall) =>
        call.url.includes("class_id=3")
          ? [
              run({ id: 31, lesson_title: "第 1 節", class_id: 3, class_name: "高一 3 班", good_count: 5, stuck_count: 1, ...ended }),
              run({ id: 30, lesson_title: "第 0 節", class_id: 3, class_name: "高一 3 班", review_completed_at: "2026-11-12T03:00:00Z", ...ended }),
            ]
          : [],
      "POST /api/lessons/7/runs": run({ class_id: 3, class_name: "高一 3 班" }),
      "GET /api/runs/9": runDetail(run({ class_id: 3, class_name: "高一 3 班" })),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/classes/3");

    const next = within(await screen.findByRole("region", { name: /L3 倒裝句\s+第 2 節/ }));
    expect(next.getByText("高一 3 班 · 下一節")).toBeInTheDocument();
    expect(next.getByRole("link", { name: "繼續備課" })).toHaveAttribute("href", "/lessons/7");
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);

    const records = within(await screen.findByRole("region", { name: "上課紀錄" }));
    const rows = records.getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByText(/11\/12 · 順利 5 次 · 卡住 1 次 · 還沒補寫/)).toBeInTheDocument();
    expect(within(rows[0] as HTMLElement).getByRole("link", { name: /^補寫/ })).toHaveAttribute("href", "/runs/31/review");
    expect(within(rows[1] as HTMLElement).getByRole("link", { name: /^回顧/ })).toHaveAttribute("href", "/runs/30/review");

    // Starting from the class page: the class is known.
    await user.click(next.getByRole("button", { name: "開始上課" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 3 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
  });

  it("assigns a unit from the library with the picker", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openClass();
    expect(unitRows()).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "從備課庫加入單元" }));
    const picker = within(await screen.findByRole("region", { name: "從備課庫加入單元" }));
    // Only the units this class does not use yet.
    expect(await picker.findByText("L4 假設語氣")).toBeInTheDocument();
    expect(picker.queryByText("L3 倒裝句")).not.toBeInTheDocument();
    expect(picker.getByText(/還沒有班級在用/)).toBeInTheDocument();

    await user.click(picker.getByRole("button", { name: "把「L4 假設語氣」加入這個班級" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/classes/3/units")).toEqual({ unit_id: 2 }));
    expect(await screen.findByText("已加入「L4 假設語氣」。")).toBeInTheDocument();
    expect(unitsSection().getByRole("link", { name: "L4 假設語氣" })).toHaveAttribute("href", "/units/2");
    expect(screen.getByText("備課庫的單元都已經加入這個班級。")).toBeInTheDocument();
  });

  it("creates a new unit in the library and assigns it to the class in one step", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openClass();
    await user.click(screen.getByRole("button", { name: "新增單元" }));
    await user.type(screen.getByRole("textbox", { name: "新單元名稱" }), "L5 分詞構句");
    await user.click(screen.getByRole("button", { name: "建立並加入這個班級" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/units")).toEqual({ title: "L5 分詞構句", class_id: 3 }));
    // The new unit opens in this class: the breadcrumb leads back to it.
    expect(await screen.findByRole("heading", { name: "L5 分詞構句", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
  });

  it("keeps a per-class note, reorders, and removes a unit without deleting it from the library", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openClass();

    await user.click(unitsSection().getByRole("button", { name: "加「L3 倒裝句」的備註" }));
    await user.type(screen.getByRole("textbox", { name: "這班的備註" }), "這班用基礎版");
    await user.click(screen.getByRole("button", { name: "儲存備註" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/classes/3/units/1")).toEqual({ note: "這班用基礎版" }));
    expect(await unitsSection().findByText(/這班的備註：這班用基礎版/)).toBeInTheDocument();
    expect(unitsSection().getByRole("button", { name: "改「L3 倒裝句」的備註" })).toBeInTheDocument();

    // Add a second unit, then move it first. Order and removal sit behind 「更多」.
    await user.click(screen.getByRole("button", { name: "從備課庫加入單元" }));
    await user.click(await screen.findByRole("button", { name: "把「L4 假設語氣」加入這個班級" }));
    await waitFor(() => expect(unitRows()).toHaveLength(2));
    expect(screen.queryByRole("button", { name: "把「L4 假設語氣」往前移" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "「L4 假設語氣」的順序與移出" }));
    await user.click(screen.getByRole("button", { name: "把「L4 假設語氣」往前移" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/classes/3/units/order")).toEqual({ unit_ids: [2, 1] }));
    await waitFor(() =>
      expect(within(unitRows()[0] as HTMLElement).getByRole("link", { name: "L4 假設語氣" })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "「L3 倒裝句」的順序與移出" }));
    await user.click(screen.getByRole("button", { name: "把「L3 倒裝句」移出這個班級" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/classes/3/units/1")).toHaveLength(1));
    expect(await screen.findByText(/單元還在備課庫，這班上過的紀錄也都保留。/)).toBeInTheDocument();
    expect(unitsSection().queryByRole("link", { name: "L3 倒裝句" })).not.toBeInTheDocument();
  });

  it("edits the class profile, and archives the class only after a confirm in the page", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openClass();

    // The profile reads as text until the teacher asks to change it.
    const section = within(screen.getByRole("region", { name: "班級特性" }));
    expect(section.getByText("三班：不太敢開口")).toBeInTheDocument();
    await user.click(section.getByRole("button", { name: "修改" }));
    const profile = screen.getByRole("textbox", { name: "這個班級的特性" });
    expect(profile).toHaveValue("三班：不太敢開口");
    await user.type(profile, "；期中後好一些");
    await user.click(screen.getByRole("button", { name: "儲存班級特性" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/classes/3")).toEqual({ class_profile: "三班：不太敢開口；期中後好一些" }));
    expect(await screen.findByText("已儲存班級特性。")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "封存這個班級" }));
    expect(api.callsTo("POST", "/api/classes/3/archive")).toHaveLength(0);
    const confirm = within(screen.getByRole("group", { name: "封存這個班級" }));
    expect(confirm.getByText(/這班的上課紀錄與補寫都保留，之後可以取消封存。/)).toBeInTheDocument();
    await user.click(confirm.getByRole("button", { name: "確定封存" }));
    await waitFor(() => expect(api.callsTo("POST", "/api/classes/3/archive")).toHaveLength(1));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBeNull();
  });
});

describe("備課庫", () => {
  it("lists every unit as a row with the classes using it, and assigns to classes", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [shared, inLibraryOnly],
      "GET /api/classes": [class3, class7],
      "POST /api/classes/7/units": classDetail(class7, []),
      "DELETE /api/classes/3/units/1": classDetail(class3, []),
    });
    const user = userEvent.setup();
    renderApp("/units");

    expect(await screen.findByRole("heading", { name: "備課庫", level: 1 })).toBeInTheDocument();
    const rows = await within(await screen.findByRole("list", { name: "單元" })).findAllByRole("listitem");
    expect(rows).toHaveLength(2);
    const first = within(rows[0] as HTMLElement);
    expect(first.getByRole("link", { name: "L3 倒裝句" })).toHaveAttribute("href", "/units/1");
    expect(first.getByText("1 節課例 · 單元想法寫了一些")).toBeInTheDocument();
    expect(first.getByText("高一 3 班、高一 7 班在用")).toBeInTheDocument();
    const second = within(rows[1] as HTMLElement);
    expect(second.getByText("還沒有課例 · 單元想法還沒寫")).toBeInTheDocument();
    expect(second.getByText("還沒有班級在用")).toBeInTheDocument();
    // One filled action on the page.
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);

    // Assign the unused unit to class 7.
    await user.click(second.getByRole("button", { name: "把「L4 假設語氣」加到班級" }));
    const assign = within(second.getByRole("group", { name: "「L4 假設語氣」給哪些班級用？" }));
    expect(assign.getByRole("checkbox", { name: "高一 3 班" })).not.toBeChecked();
    await user.click(assign.getByRole("checkbox", { name: "高一 7 班" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/classes/7/units")).toEqual({ unit_id: 2 }));
    expect(await second.findByText("高一 7 班在用")).toBeInTheDocument();

    // Take the shared unit away from class 3: the other class keeps it.
    await user.click(first.getByRole("button", { name: "把「L3 倒裝句」加到班級" }));
    await user.click(within(first.getByRole("group")).getByRole("checkbox", { name: "高一 3 班" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/classes/3/units/1")).toHaveLength(1));
    expect(await first.findByText("高一 7 班在用")).toBeInTheDocument();
  });

  it("creates a unit straight into a class, or just into the library", async () => {
    signIn();
    const created = unitWith({ id: 4, title: "L5 分詞構句", lesson_count: 0 });
    const api = mockApi({
      ...baseRoutes({ route_choice_enabled: false }),
      "GET /api/classes": [class3, class7],
      "POST /api/units": created,
      "GET /api/units/4": created,
      "GET /api/units/4/lessons": [],
      "GET /api/units/4/materials": [],
    });
    const user = userEvent.setup();
    renderApp("/units");
    // The form opens on request; the page's one filled action then is 「建立單元」.
    await user.click(await screen.findByRole("button", { name: "新增單元" }));
    const pick = await screen.findByRole("combobox", { name: "給哪個班級用？" });
    expect(pick).toHaveValue("");
    // The class profile lives on the class: the form does not ask for one.
    expect(screen.queryByRole("group", { name: "班級特性" })).not.toBeInTheDocument();
    await user.selectOptions(pick, "高一 7 班");
    await user.type(screen.getByLabelText("單元名稱"), "L5 分詞構句");
    await user.click(screen.getByRole("button", { name: "建立單元" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/units")).toEqual({ title: "L5 分詞構句", class_id: 7 }));
    expect(await screen.findByRole("heading", { name: "L5 分詞構句", level: 1 })).toBeInTheDocument();
  });

  it("leaves the class context: a unit opened from the library has no class in play when several use it", async () => {
    signIn();
    window.sessionStorage.setItem(CURRENT_CLASS_STORAGE_KEY, "3");
    mockApi({
      ...baseRoutes(),
      "GET /api/units": [shared],
      "GET /api/units/1": shared,
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [],
      "GET /api/classes": [class3, class7],
    });
    const user = userEvent.setup();
    renderApp("/units");
    await user.click(await screen.findByRole("link", { name: "L3 倒裝句" }));
    expect(await screen.findByRole("heading", { name: "L3 倒裝句", level: 1 })).toBeInTheDocument();
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBeNull();
    // The breadcrumb leads back to the library; the classes are named once, as a fact.
    expect(within(document.querySelector(".crumbs") as HTMLElement).getByRole("link", { name: "備課庫" })).toHaveAttribute(
      "href",
      "/units",
    );
    expect(screen.getByText("高一 3 班、高一 7 班在用")).toBeInTheDocument();
    expectNoClassField();
  });
});

describe("the class is context, never a field", () => {
  it("a unit entered from a class shows that class in the breadcrumb and keeps it as the context", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/classes/3": classDetail(class3, [classUnit(shared)]),
      "GET /api/units/1": shared,
      "GET /api/units/1/lessons": [],
    });
    const user = userEvent.setup();
    renderApp("/classes/3");
    await user.click(await screen.findByRole("link", { name: "L3 倒裝句" }));

    expect(await screen.findByRole("heading", { name: "L3 倒裝句", level: 1 })).toBeInTheDocument();
    expectNoClassField();
    // The breadcrumb leads back to the class; the class profile is not repeated on the unit page.
    expect(screen.getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
    expect(screen.queryByRole("region", { name: "班級特性" })).not.toBeInTheDocument();
    expect(screen.queryByText("三班：不太敢開口")).not.toBeInTheDocument();
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBe("3");
  });

  it("with exactly one class, that class is the context without choosing", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": onlyClass3,
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [],
    });
    renderApp("/units/1");
    expect(await screen.findByRole("heading", { name: "L3 倒裝句", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
    expect(screen.getByText("高一 3 班在用")).toBeInTheDocument();
    expectNoClassField();
  });

  it("a unit no class uses points to the library to add it to a class", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ class_profile: "單元舊描述" }),
      "GET /api/units/1/lessons": [],
    });
    renderApp("/units/1");
    expect(await screen.findByText(/還沒有班級在用/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "到備課庫加到班級" })).toHaveAttribute("href", "/units");
    // The unit's legacy profile is kept on the server but no longer edited here.
    expect(screen.queryByRole("textbox", { name: /班級特性/ })).not.toBeInTheDocument();
  });

  it("the lesson page shows the class once and sends it when Manna is asked", async () => {
    signIn();
    window.sessionStorage.setItem(CURRENT_CLASS_STORAGE_KEY, "7");
    const api = mockApi(
      lessonRoutes(shared, {
        "GET /api/lessons/7/sop": {
          lesson_id: 7,
          version_no: 1,
          versions_count: 1,
          saved_at: "2026-11-14T08:00:00Z",
          material: {
            id: 1,
            version_no: 1,
            title: "L3",
            source_note: "",
            text: "Not only does Amy sing well.",
            paragraphs: [{ id: "p1", no: 1, text: "Not only does Amy sing well." }],
            word_count: 6,
            created_at: "2026-11-14T08:00:00Z",
          },
          content: emptyContent(),
          today_objective: "",
          jobs: { analysis: null, objectives: null },
        },
        "GET /api/materials/1/annotations": [],
        "POST /api/lessons/7/sop/analysis": respond(503, { error: { code: "ai_unavailable", message: "暫時不能用" } }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/lessons/7");
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "高一 7 班" })).toHaveAttribute("href", "/classes/7");
    expectNoClassField();
    // The class's profile is not shown or overridden on the lesson page: it is edited on the class page.
    expect(screen.queryByText(/本課背景/)).not.toBeInTheDocument();
    expect(screen.queryByText(/這節課的班級描述/)).not.toBeInTheDocument();

    await user.click(within(screen.getByRole("navigation", { name: "備課步驟" })).getByRole("button", { name: /分析寫作特色/ }));
    await user.click(await screen.findByRole("button", { name: /請 Manna/ }));
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/sop/analysis")).toEqual({ class_id: 7 }));
  });
});

describe("開始上課: the class is asked once, and only when it is not known", () => {
  /** 「開始上課」 is a secondary button in the lesson page head. */
  const startButton = () =>
    within(document.querySelector(".lesson-head") as HTMLElement).getByRole("button", { name: "開始上課" });
  const dialog = () => screen.queryByRole("dialog", { name: "這節課在哪一班上？" });

  async function openLesson() {
    const rendered = renderApp("/lessons/7");
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
    return rendered;
  }

  it("from the library, with several classes using the unit, a dialog asks which class", async () => {
    signIn();
    const api = mockApi(
      lessonRoutes(shared, {
        "GET /api/lessons/7/runs": [
          // The lesson is open in class 3 and was taught in class 7 before.
          run({ id: 4, class_id: 3, class_name: "高一 3 班" }),
          run({ id: 5, class_id: 7, class_name: "高一 7 班", ...ended }),
        ],
      }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    // The unit (and so its classes) has loaded: the breadcrumb names it.
    await screen.findByRole("link", { name: "L3 倒裝句" });
    expectNoClassField();

    await user.click(startButton());
    const asked = within(await screen.findByRole("dialog", { name: "這節課在哪一班上？" }));
    expect(asked.getByText("你是從備課庫進來的，所以問一次。從班級進來會直接開始。")).toBeInTheDocument();
    // The class most likely meant is already chosen and holds the focus.
    const three = asked.getByRole("radio", { name: /高一 3 班.*這節課正在上，會回到課堂/ });
    const seven = asked.getByRole("radio", { name: /高一 7 班.*11\/12 上過這一節/ });
    expect(three).toBeChecked();
    expect(three).toHaveFocus();
    // Nothing started yet.
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(0);

    // Cancel (or Escape) is possible; asking again shows the same choice.
    await user.click(asked.getByRole("button", { name: "取消" }));
    expect(dialog()).not.toBeInTheDocument();
    await user.click(startButton());
    await user.keyboard("{Escape}");
    expect(dialog()).not.toBeInTheDocument();

    await user.click(startButton());
    const again = within(dialog() as HTMLElement);
    await user.click(again.getByRole("radio", { name: /高一 7 班/ }));
    expect(seven).not.toBeInTheDocument();
    await user.click(again.getByRole("button", { name: "開始上課" }));

    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 7 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    // The console says which class it is.
    expect(await screen.findByText(/高一 7 班 · L3 倒裝句/)).toBeInTheDocument();
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBe("7");
  });

  it("from a class, starts in that class without asking, and leaves another class's open run alone", async () => {
    signIn();
    window.sessionStorage.setItem(CURRENT_CLASS_STORAGE_KEY, "3");
    const api = mockApi(
      lessonRoutes(shared, {
        "GET /api/lessons/7/runs": [run({ id: 4, class_id: 7, class_name: "高一 7 班" })],
        "GET /api/runs/9": runDetail(run({ class_id: 3, class_name: "高一 3 班" })),
      }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    await screen.findByRole("link", { name: "高一 3 班" });
    // Class 7's open run is not class 3's: the button still starts a lesson.
    await user.click(startButton());
    expect(dialog()).not.toBeInTheDocument();
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 3 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
  });

  it("goes back to the open run of the class in play instead of starting another", async () => {
    signIn();
    window.sessionStorage.setItem(CURRENT_CLASS_STORAGE_KEY, "7");
    const api = mockApi(
      lessonRoutes(shared, { "GET /api/lessons/7/runs": [run({ id: 9, class_id: 7, class_name: "高一 7 班" })] }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    await user.click(
      await within(document.querySelector(".lesson-head") as HTMLElement).findByRole("button", { name: "回到課堂" }),
    );
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(0);
  });

  it("starts in the unit's only class, and without a class when no class uses the unit", async () => {
    signIn();
    const one = mockApi(lessonRoutes(onlyClass3));
    const user = userEvent.setup();
    const first = await openLesson();
    await screen.findByRole("link", { name: "高一 3 班" });
    await user.click(startButton());
    expect(dialog()).not.toBeInTheDocument();
    await waitFor(() => expect(one.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 3 }));
    first.unmount();

    const none = mockApi(lessonRoutes(unitWith({ id: 1, title: "L3 倒裝句" })));
    await openLesson();
    await screen.findByRole("link", { name: "L3 倒裝句" });
    await user.click(startButton());
    // Never blocked: a run without a class.
    expect(dialog()).not.toBeInTheDocument();
    await waitFor(() => expect(none.lastBody("POST", "/api/lessons/7/runs")).toEqual({}));
  });
});

describe("全部上課紀錄", () => {
  it("groups past runs by class; lessons are not started from here", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/classes": [class3, class7],
      "GET /api/runs": [
        run({ id: 20, lesson_title: "第 3 節", class_id: 3, class_name: "高一 3 班" }),
        run({ id: 21, lesson_title: "第 2 節", class_id: 7, class_name: "高一 7 班", ...ended }),
        run({ id: 22, lesson_title: "第 1 節", class_id: 3, class_name: "高一 3 班", ...ended }),
        run({ id: 23, lesson_title: "第 0 節", ...ended }),
        run({ id: 24, lesson_title: "第 1 節", class_id: 7, class_name: "高一 7 班", review_completed_at: "2026-11-12T03:00:00Z", ...ended }),
      ],
    });
    renderApp("/record");

    expect(await screen.findByRole("heading", { name: "上課紀錄", level: 1 })).toBeInTheDocument();
    // The lesson in progress first; then groups in the teacher's class order; runs without a class last.
    await waitFor(() =>
      expect(screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent)).toEqual([
        "上課中",
        "高一 3 班",
        "高一 7 班",
        "沒有記在班級的課",
      ]),
    );
    expect(within(screen.getByRole("region", { name: "上課中" })).getByRole("link", { name: /^回到課堂/ })).toHaveAttribute(
      "href",
      "/runs/20",
    );
    const seven = within(screen.getByRole("region", { name: "高一 7 班" }));
    expect(seven.getAllByRole("listitem")).toHaveLength(2);
    expect(seven.getByRole("link", { name: /^補寫/ })).toHaveAttribute("href", "/runs/21/review");
    expect(seven.getByRole("link", { name: /^回顧/ })).toHaveAttribute("href", "/runs/24/review");

    // No second way to start a lesson, and no class to pick.
    expect(screen.queryByRole("button", { name: /開始上課/ })).not.toBeInTheDocument();
    expectNoClassField();
  });
});
