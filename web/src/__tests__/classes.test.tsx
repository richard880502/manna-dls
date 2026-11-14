// 班級與備課庫 (docs/API.md Phase 1.4): class-centred home, class page, the design library,
// the class context (目前班級) on unit and lesson pages, and which class a run belongs to.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ClassDetail, CreateClassRequest, LessonRun, RunDetail, SchoolClass } from "../api/types";
import { CURRENT_CLASS_STORAGE_KEY } from "../lib/storage";
import {
  baseRoutes,
  classDetail,
  classRef,
  classUnit,
  coreVersion,
  lessonDetail,
  mockApi,
  preferences,
  renderApp,
  respond,
  schoolClass,
  signIn,
  todayCard,
  unitWith,
  type MockCall,
} from "../test/mockApi";

const class3 = schoolClass({ id: 3, name: "高一 3 班", class_profile: "三班：不太敢開口" });
const class7 = schoolClass({ id: 7, name: "高一 7 班", class_profile: "七班：活潑、愛發言", position: 2 });

/** A unit design used by both classes; class 7 keeps a note on it. */
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

/** Everything the lesson page of lesson 7 (unit 1) loads. */
function lessonRoutes(unit = shared, extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/units/1": unit,
    "GET /api/lessons/7": lessonDetail({ class_description: "", effective_class_profile: "" }),
    "GET /api/units/1/core": coreVersion([]),
    "GET /api/siop": [],
    "GET /api/teacher-cases": [],
    "GET /api/lessons/7/runs": [],
    "POST /api/lessons/7/runs": (call: MockCall) => {
      const classId = (call.body as { class_id?: number }).class_id ?? null;
      return run({ class_id: classId, class_name: classId === 7 ? "高一 7 班" : classId === 3 ? "高一 3 班" : null });
    },
    "GET /api/runs/9": runDetail(run({ class_id: 7, class_name: "高一 7 班" })),
    ...extra,
  };
}

const classGroup = () => within(screen.getByRole("group", { name: "目前班級" }));

describe("工作檯: classes", () => {
  it("lists the classes as cards with their next step, and a clear entry to the library", async () => {
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
    });
    renderApp("/");

    const list = within(await screen.findByRole("region", { name: "我的班級" }));
    expect(await list.findByText("3 個班級")).toBeInTheDocument();
    const cards = list.getAllByRole("listitem");
    expect(cards).toHaveLength(3);

    const first = within(cards[0] as HTMLElement);
    expect(first.getByRole("heading", { name: "高一 3 班" })).toBeInTheDocument();
    expect(first.getByText("高一 · 英文閱讀選修")).toBeInTheDocument();
    expect(first.getByText("2 個單元")).toBeInTheDocument();
    expect(first.getByText("繼續：L3 倒裝句 第 2 節")).toBeInTheDocument();
    expect(first.getByRole("link", { name: "開始本節課" })).toHaveAttribute("href", "/lessons/8");
    expect(first.getByRole("link", { name: "班級頁" })).toHaveAttribute("href", "/classes/3");

    const second = within(cards[1] as HTMLElement);
    expect(second.getByText("上課中：L3 倒裝句 第 1 節")).toBeInTheDocument();
    expect(second.getByRole("link", { name: "回到課堂紀錄" })).toHaveAttribute("href", "/runs/9");

    const third = within(cards[2] as HTMLElement);
    expect(third.getByText("還沒有單元")).toBeInTheDocument();
    expect(third.getByRole("link", { name: "從備課庫加入單元" })).toHaveAttribute("href", "/classes/8");

    expect(list.getByRole("button", { name: "新增班級" })).toBeInTheDocument();
    const library = within(screen.getByRole("region", { name: "備課庫" }));
    expect(library.getByText(/目前有 1 個單元設計/)).toBeInTheDocument();
    expect(library.getByRole("link", { name: "進入備課庫" })).toHaveAttribute("href", "/library");
    // The sidebar leads to both.
    expect(screen.getByRole("link", { name: "備課庫" })).toHaveAttribute("href", "/library");
  });

  it("following a class's next step makes that class the current class", async () => {
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
    await user.click(await screen.findByRole("link", { name: "開始本節課" }));
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 2 })).toBeInTheDocument();
    await waitFor(() => expect(classGroup().getByLabelText("目前班級")).toHaveValue("7"));
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

    const card = within((await screen.findByRole("heading", { name: "我的班級", level: 4 })).closest("li") as HTMLElement);
    expect(card.getByText("2 個單元")).toBeInTheDocument();
    await user.click(card.getByRole("link", { name: "我的班級" }));

    expect(await screen.findByRole("heading", { name: "我的班級", level: 2 })).toBeInTheDocument();
    const units = within(screen.getByRole("region", { name: "這個班級的單元" }));
    const rows = units.getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByRole("link", { name: "L1 舊單元" })).toHaveAttribute("href", "/units/1");
    expect(within(rows[0] as HTMLElement).getByText(/這班已上 1 \/ 2 個課例 · 最近一次上課 2026\/11\/12/)).toBeInTheDocument();
    expect(within(rows[0] as HTMLElement).getByRole("link", { name: "下一節：第 2 節" })).toHaveAttribute("href", "/lessons/8");
    expect(within(rows[1] as HTMLElement).getByText("還沒有課例")).toBeInTheDocument();
    // The profile carried over from the legacy unit is the class's, and editable.
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
    expect(invite.getByText("可略過")).toBeInTheDocument();
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
    expect(await screen.findByRole("heading", { name: "高一 3 班", level: 2 })).toBeInTheDocument();
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
    expect(screen.getByText("還沒有班級。可以直接到備課庫準備單元，之後再建立班級。")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "進入備課庫" })).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/classes")).toHaveLength(0);
    // …and the invitation comes back on request.
    await user.click(screen.getByRole("button", { name: "新增班級" }));
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

  const unitsCard = () => within(screen.getByRole("region", { name: "這個班級的單元" }));

  async function openClass() {
    const rendered = renderApp("/classes/3");
    expect(await screen.findByRole("heading", { name: "高一 3 班", level: 2 })).toBeInTheDocument();
    return rendered;
  }

  it("assigns a unit from the library with the picker", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openClass();
    expect(unitsCard().getAllByRole("listitem")).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "從備課庫加入單元" }));
    const picker = within(await screen.findByRole("region", { name: "從備課庫加入單元" }));
    // Only the designs this class does not use yet.
    expect(await picker.findByText("L4 假設語氣")).toBeInTheDocument();
    expect(picker.queryByText("L3 倒裝句")).not.toBeInTheDocument();
    expect(picker.getByText(/還沒有班級使用/)).toBeInTheDocument();

    await user.click(picker.getByRole("button", { name: "把「L4 假設語氣」加入這個班級" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/classes/3/units")).toEqual({ unit_id: 2 }));
    expect(await screen.findByText("已加入「L4 假設語氣」。")).toBeInTheDocument();
    expect(unitsCard().getByRole("link", { name: "L4 假設語氣" })).toHaveAttribute("href", "/units/2");
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
    // The new unit opens in this class's context.
    expect(await screen.findByRole("heading", { name: "L5 分詞構句", level: 2 })).toBeInTheDocument();
    expect(classGroup().getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
  });

  it("keeps a per-class note, reorders, and removes a unit without deleting the design", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openClass();

    await user.click(unitsCard().getByRole("button", { name: "加上這班的備註" }));
    await user.type(screen.getByRole("textbox", { name: "這班的備註" }), "這班用基礎版");
    await user.click(screen.getByRole("button", { name: "儲存備註" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/classes/3/units/1")).toEqual({ note: "這班用基礎版" }));
    expect(await unitsCard().findByText("這班用基礎版")).toBeInTheDocument();
    expect(unitsCard().getByRole("button", { name: "修改備註" })).toBeInTheDocument();

    // Add a second unit, then move it first.
    await user.click(screen.getByRole("button", { name: "從備課庫加入單元" }));
    await user.click(await screen.findByRole("button", { name: "把「L4 假設語氣」加入這個班級" }));
    await waitFor(() => expect(unitsCard().getAllByRole("listitem")).toHaveLength(2));
    await user.click(screen.getByRole("button", { name: "把「L4 假設語氣」往前移" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/classes/3/units/order")).toEqual({ unit_ids: [2, 1] }));
    await waitFor(() =>
      expect(within(unitsCard().getAllByRole("listitem")[0] as HTMLElement).getByRole("link", { name: "L4 假設語氣" })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "把「L3 倒裝句」移出這個班級" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/classes/3/units/1")).toHaveLength(1));
    expect(await screen.findByText(/單元設計還在備課庫，這班上過的紀錄也都保留。/)).toBeInTheDocument();
    expect(unitsCard().queryByRole("link", { name: "L3 倒裝句" })).not.toBeInTheDocument();
  });

  it("edits the class profile, and archives the class only after a confirm in the page", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openClass();

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
  it("lists every unit design with the classes using it, and assigns to classes", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [shared, inLibraryOnly],
      "GET /api/classes": [class3, class7],
      "POST /api/classes/7/units": classDetail(class7, []),
      "DELETE /api/classes/3/units/1": classDetail(class3, []),
    });
    const user = userEvent.setup();
    renderApp("/library");

    expect(await screen.findByRole("heading", { name: "備課庫：你已經準備好的單元設計" })).toBeInTheDocument();
    const rows = await within(screen.getByRole("region", { name: "我的單元設計" })).findAllByRole("listitem");
    expect(rows).toHaveLength(2);
    const first = within(rows[0] as HTMLElement);
    expect(first.getByText("使用中的班級")).toBeInTheDocument();
    expect(first.getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
    expect(first.getByRole("link", { name: "高一 7 班" })).toHaveAttribute("href", "/classes/7");
    const second = within(rows[1] as HTMLElement);
    expect(second.getByText("還沒有班級使用")).toBeInTheDocument();

    // Assign the unused design to class 7.
    await user.click(second.getByRole("button", { name: "把「L4 假設語氣」加入班級" }));
    const assign = within(second.getByRole("group", { name: "「L4 假設語氣」給哪些班級用？" }));
    expect(assign.getByRole("checkbox", { name: "高一 3 班" })).not.toBeChecked();
    await user.click(assign.getByRole("checkbox", { name: "高一 7 班" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/classes/7/units")).toEqual({ unit_id: 2 }));
    expect(await second.findByRole("link", { name: "高一 7 班" })).toBeInTheDocument();
    expect(second.queryByText("還沒有班級使用")).not.toBeInTheDocument();

    // Take the shared design away from class 3: the other class keeps it.
    await user.click(first.getByRole("button", { name: "把「L3 倒裝句」加入班級" }));
    await user.click(within(first.getByRole("group")).getByRole("checkbox", { name: "高一 3 班" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/classes/3/units/1")).toHaveLength(1));
    await waitFor(() => expect(first.queryByRole("link", { name: "高一 3 班" })).not.toBeInTheDocument());
    expect(first.getByRole("link", { name: "高一 7 班" })).toBeInTheDocument();
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
    renderApp("/library");
    const pick = await screen.findByRole("combobox", { name: "建立後給哪個班級用？（選填）" });
    expect(pick).toHaveValue("");
    // The class profile now lives on the class: the form no longer asks for one.
    expect(screen.queryByRole("group", { name: "班級特性" })).not.toBeInTheDocument();
    await user.selectOptions(pick, "高一 7 班");
    await user.type(screen.getByLabelText("建立新單元"), "L5 分詞構句");
    await user.click(screen.getByRole("button", { name: "建立單元" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/units")).toEqual({ title: "L5 分詞構句", class_id: 7 }));
    expect(await screen.findByRole("heading", { name: "L5 分詞構句", level: 2 })).toBeInTheDocument();
  });

  it("leaves the class context: a unit opened from the library has no current class when several use it", async () => {
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
    renderApp("/library");
    await user.click(await screen.findByRole("link", { name: "課例備課" }));
    expect(await screen.findByRole("heading", { name: "L3 倒裝句", level: 2 })).toBeInTheDocument();
    expect(classGroup().getByLabelText("目前班級")).toHaveValue("");
    expect(classGroup().getByRole("option", { name: "尚未選擇（2 個班級在用）" })).toBeInTheDocument();
    expect(within(screen.getByText(/^備課庫/, { selector: ".breadcrumb a" }).closest("p") as HTMLElement).getByText(/L3 倒裝句/)).toBeInTheDocument();
  });
});

describe("class context (目前班級)", () => {
  it("is shown on the unit page when entered from a class, and can be switched", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/classes/3": classDetail(class3, [classUnit(shared)]),
      "GET /api/units/1": shared,
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [
        { id: 31, filename: "L3 課文", char_count: 120, created_at: "2026-11-10T08:00:00Z" },
      ],
      "POST /api/units/1/material-focuses": {
        interaction_id: 1,
        status: "blocked",
        message: "目前暫無建議，你可以繼續手動完成。",
        output: null,
        sources: [],
        context: [],
      },
    });
    const user = userEvent.setup();
    renderApp("/classes/3");
    await user.click(await screen.findByRole("link", { name: "打開單元" }));

    expect(await screen.findByRole("heading", { name: "L3 倒裝句", level: 2 })).toBeInTheDocument();
    const picker = classGroup().getByLabelText("目前班級");
    expect(picker).toHaveValue("3");
    expect(within(picker).getAllByRole("option").map((o) => o.textContent)).toEqual(["高一 3 班", "高一 7 班"]);
    // The breadcrumb leads back to the class, and the profile shown is that class's.
    expect(screen.getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
    const profile = within(screen.getByRole("region", { name: "班級特性" }));
    expect(profile.getByText("高一 3 班的班級特性")).toBeInTheDocument();
    expect(profile.getByText("三班：不太敢開口")).toBeInTheDocument();
    expect(profile.getByRole("link", { name: "到班級頁修改" })).toHaveAttribute("href", "/classes/3");

    // Switch to the other class that uses this unit: same design, the other class's profile and note.
    await user.selectOptions(picker, "高一 7 班");
    expect(picker).toHaveValue("7");
    expect(profile.getByText("七班：活潑、愛發言")).toBeInTheDocument();
    expect(classGroup().getByText("這班的備註：這班用基礎版")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "高一 7 班" })).toHaveAttribute("href", "/classes/7");
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBe("7");

    // Manna's context follows the current class.
    await user.click(screen.getByRole("button", { name: /教材/ }));
    await user.click(await screen.findByRole("button", { name: /候選/ }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/units/1/material-focuses")).toEqual({ material_id: 31, class_id: 7 }),
    );
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
    expect(await screen.findByRole("heading", { name: "L3 倒裝句", level: 2 })).toBeInTheDocument();
    expect(classGroup().getByText(/目前班級：/)).toBeInTheDocument();
    expect(classGroup().getByRole("link", { name: "高一 3 班" })).toHaveAttribute("href", "/classes/3");
    expect(screen.queryByRole("combobox", { name: "目前班級" })).not.toBeInTheDocument();
  });

  it("a unit no class uses keeps its own (legacy) class profile and points to the library", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ class_profile: "單元舊描述" }),
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [],
    });
    renderApp("/units/1");
    expect(await screen.findByText(/還沒有班級取用這個單元。/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "到備課庫加入班級" })).toHaveAttribute("href", "/library");
    expect(screen.getByRole("textbox", { name: "這個單元的班級特性" })).toHaveValue("單元舊描述");
  });

  it("the lesson page shows the class, its profile, and sends it with suggestion requests", async () => {
    signIn();
    window.sessionStorage.setItem(CURRENT_CLASS_STORAGE_KEY, "7");
    const api = mockApi(
      lessonRoutes(shared, {
        "GET /api/lessons/7": (call: MockCall) =>
          call.url.includes("class_id=7")
            ? lessonDetail({ class_description: "", effective_class_profile: "七班：活潑、愛發言", class_profile_source: "class" })
            : lessonDetail({ class_description: "", effective_class_profile: "" }),
        "POST /api/lessons/7/suggestions": {
          interaction_id: 1,
          status: "blocked",
          message: "目前暫無建議，你可以繼續手動完成。",
          output: null,
          sources: [],
          context: [],
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/lessons/7");
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 2 })).toBeInTheDocument();
    await waitFor(() => expect(classGroup().getByLabelText("目前班級")).toHaveValue("7"));
    expect(await screen.findByText(/班級特性（高一 7 班的班級特性）：七班：活潑、愛發言/)).toBeInTheDocument();
    expect(todayCard().getByText(/上課紀錄會記在「高一 7 班」。/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/suggestions")).toEqual({ class_id: 7 }));
  });
});

describe("開始本節課: which class", () => {
  const startButton = () => todayCard().getByRole("button", { name: "開始本節課" });

  async function openLesson() {
    const rendered = renderApp("/lessons/7");
    expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 2 })).toBeInTheDocument();
    return rendered;
  }

  it("asks in the page when several classes use the unit and none is current", async () => {
    signIn();
    const api = mockApi(
      lessonRoutes(shared, {
        // The lesson is already open in class 3: class 7 still gets its own run.
        "GET /api/lessons/7/runs": [run({ id: 4, class_id: 3, class_name: "高一 3 班" })],
      }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    await waitFor(() => expect(classGroup().getByLabelText("目前班級")).toHaveValue(""));

    await user.click(startButton());
    const chooser = within(await screen.findByRole("group", { name: "這節課是哪一班？" }));
    expect(chooser.getByText("這個單元有 2 個班級在用，上課紀錄會記在你選的班級。")).toBeInTheDocument();
    expect(chooser.getByRole("button", { name: "高一 3 班（上課中）" })).toBeInTheDocument();
    // Nothing started yet.
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(0);

    // Cancel is possible; asking again shows the same choice.
    await user.click(chooser.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("group", { name: "這節課是哪一班？" })).not.toBeInTheDocument();
    await user.click(startButton());
    await user.click(within(screen.getByRole("group", { name: "這節課是哪一班？" })).getByRole("button", { name: "高一 7 班" }));

    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 7 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    // The record console says which class it is.
    expect(await screen.findByText(/高一 7 班 · L3 倒裝句/)).toBeInTheDocument();
    expect(window.sessionStorage.getItem(CURRENT_CLASS_STORAGE_KEY)).toBe("7");
  });

  it("uses the current class without asking, and resumes that class's open run", async () => {
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
    await waitFor(() => expect(classGroup().getByLabelText("目前班級")).toHaveValue("3"));
    // Class 7's open run is not class 3's: the button still starts a lesson.
    await user.click(startButton());
    expect(screen.queryByRole("group", { name: "這節課是哪一班？" })).not.toBeInTheDocument();
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 3 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
  });

  it("goes back to the open run of the current class instead of starting another", async () => {
    signIn();
    window.sessionStorage.setItem(CURRENT_CLASS_STORAGE_KEY, "7");
    const api = mockApi(
      lessonRoutes(shared, { "GET /api/lessons/7/runs": [run({ id: 9, class_id: 7, class_name: "高一 7 班" })] }),
    );
    const user = userEvent.setup();
    const { router } = await openLesson();
    await user.click(await todayCard().findByRole("button", { name: "回到課堂紀錄" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    expect(api.callsTo("POST", "/api/lessons/7/runs")).toHaveLength(0);
  });

  it("starts in the unit's only class, and without a class when no class uses the unit", async () => {
    signIn();
    const one = mockApi(lessonRoutes(onlyClass3));
    const user = userEvent.setup();
    const first = await openLesson();
    await waitFor(() => expect(classGroup().getByRole("link", { name: "高一 3 班" })).toBeInTheDocument());
    await user.click(startButton());
    await waitFor(() => expect(one.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 3 }));
    first.unmount();

    const none = mockApi(lessonRoutes(unitWith({ id: 1, title: "L3 倒裝句" })));
    await openLesson();
    await screen.findByText(/還沒有班級取用這個單元。/);
    await user.click(startButton());
    // Never blocked: a run without a class.
    await waitFor(() => expect(none.lastBody("POST", "/api/lessons/7/runs")).toEqual({}));
  });
});

describe("Teach & Record index", () => {
  it("groups past runs by class and starts a lesson in the chosen class", async () => {
    signIn();
    const ended = { ended_at: "2026-11-10T02:00:00Z" };
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [shared, inLibraryOnly],
      "GET /api/classes": [class3, class7],
      "GET /api/runs": [
        run({ id: 21, lesson_title: "第 2 節", class_id: 7, class_name: "高一 7 班", ...ended }),
        run({ id: 22, lesson_title: "第 1 節", class_id: 3, class_name: "高一 3 班", ...ended }),
        run({ id: 23, lesson_title: "第 0 節", ...ended }),
        run({ id: 24, lesson_title: "第 1 節", class_id: 7, class_name: "高一 7 班", ...ended }),
      ],
      "GET /api/units/1/lessons": [
        { id: 7, unit_id: 1, title: "第 1 課：Not only… but also", lesson_date: null, class_description: "", created_at: "", my_example: "" },
      ],
      "POST /api/lessons/7/runs": run({ class_id: 7, class_name: "高一 7 班" }),
      "GET /api/runs/9": runDetail(run({ class_id: 7, class_name: "高一 7 班" })),
    });
    const user = userEvent.setup();
    const { router } = renderApp("/record");

    // Groups follow the teacher's class order; runs without a class come last.
    const past = within(await screen.findByRole("region", { name: "最近的課" }));
    await waitFor(() =>
      expect(past.getAllByRole("heading", { level: 4 }).map((h) => h.textContent)).toEqual([
        "高一 3 班1 節",
        "高一 7 班2 節",
        "未指定班級1 節",
      ]),
    );
    expect(within(past.getByRole("region", { name: /高一 7 班/ })).getAllByRole("listitem")).toHaveLength(2);

    // Without a class, a unit several classes use cannot be started: the page says why.
    await user.selectOptions(await screen.findByLabelText("單元"), "L3 倒裝句");
    await user.selectOptions(await screen.findByLabelText("課例"), "第 1 課：Not only… but also");
    expect(screen.getByRole("button", { name: "開始本節課" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("這個單元有 2 個班級在用，請先在上面選擇班級。");

    // Choosing a class narrows the units to that class's.
    await user.selectOptions(screen.getByLabelText("班級"), "高一 7 班");
    expect(within(screen.getByLabelText("單元")).getAllByRole("option").map((o) => o.textContent)).toEqual(["請選擇單元", "L3 倒裝句"]);
    await user.selectOptions(screen.getByLabelText("單元"), "L3 倒裝句");
    await user.selectOptions(await screen.findByLabelText("課例"), "第 1 課：Not only… but also");
    await user.click(screen.getByRole("button", { name: "開始本節課" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/lessons/7/runs")).toEqual({ class_id: 7 }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
  });
});
