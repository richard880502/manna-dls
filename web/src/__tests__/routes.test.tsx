import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type {
  AiResult,
  CoreDraftRequest,
  CreateMaterialRequest,
  MaterialFocusesOutput,
  UpdateUnitRequest,
} from "../api/types";
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
  unitWith,
  type MockCall,
} from "../test/mockApi";

const DRAFT = "/api/units/1/core/draft";

function scaffoldEvents(api: ReturnType<typeof mockApi>) {
  return api.callsTo("POST", "/api/scaffold-events").map((c) => c.body as Record<string, unknown>);
}

describe("prep route (備課菜單) on unit creation and the unit page", () => {
  it("defaults the route from 我的備課偏好, sends it on create and opens the unit on that route", async () => {
    signIn();
    const created = unitWith({ id: 2, title: "新單元", prep_route: "material", core_status: "none", lesson_count: 0 });
    const api = mockApi({
      ...baseRoutes({}, preferences({ start_from: "material" })),
      "POST /api/units": created,
      "GET /api/units/2": created,
      "GET /api/units/2/lessons": [],
      "GET /api/units/2/materials": [],
    });
    const user = userEvent.setup();
    renderApp("/plan");

    expect(await screen.findByRole("radio", { name: /教材優先/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /目標優先/ })).not.toBeChecked();
    await user.type(screen.getByLabelText("建立新單元"), "新單元");
    await user.click(screen.getByRole("button", { name: "建立單元" }));

    await waitFor(() => expect(api.lastBody("POST", "/api/units")).toEqual({ title: "新單元", prep_route: "material" }));
    expect(await screen.findByRole("heading", { name: "新單元", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "備課菜單：教材優先" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "下一步：貼上課文或教材" })).toBeInTheDocument();
    // material-first: the material area is open on the first screen
    expect(screen.getByLabelText("教材文字")).toBeInTheDocument();
  });

  it("lets the teacher pick another route than the default", async () => {
    signIn();
    const created = unitWith({ id: 2, title: "活動單元", prep_route: "activity" });
    const api = mockApi({
      ...baseRoutes({}, preferences({ start_from: "goal" })),
      "POST /api/units": created,
      "GET /api/units/2": created,
      "GET /api/units/2/lessons": [],
      "GET /api/units/2/materials": [],
    });
    const user = userEvent.setup();
    renderApp("/plan");

    expect(await screen.findByRole("radio", { name: /目標優先/ })).toBeChecked();
    await user.click(screen.getByRole("radio", { name: /活動優先/ }));
    await user.type(screen.getByLabelText("建立新單元"), "活動單元");
    await user.click(screen.getByRole("button", { name: "建立單元" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/units")).toEqual({ title: "活動單元", prep_route: "activity" }));
    expect(await screen.findByRole("button", { name: "下一步：建立課例，先寫活動點子" })).toBeInTheDocument();
  });

  it("switches the route on the unit page; a 409 reverts it with a friendly message", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [],
      "PUT /api/units/1": (call: MockCall) => {
        const body = call.body as UpdateUnitRequest;
        if (body.prep_route === "activity") {
          return respond(409, { error: { code: "conflict", message: "route choice disabled" } });
        }
        return unitWith(body);
      },
    });
    const user = userEvent.setup();
    renderApp("/units/1");

    expect(await screen.findByRole("heading", { name: "備課菜單：目標優先" })).toBeInTheDocument();
    // CoRe already has a draft, so the next step is the first lesson
    expect(screen.getByRole("button", { name: "下一步：建立第一個課例" })).toBeInTheDocument();
    await user.click(screen.getByText("換一種備課菜單"));
    await user.click(screen.getByRole("radio", { name: /學生優先/ }));

    await waitFor(() => expect(api.lastBody("PUT", "/api/units/1")).toEqual({ prep_route: "student" }));
    expect(await screen.findByRole("heading", { name: "備課菜單：學生優先" })).toBeInTheDocument();
    expect(screen.getByText("已切換為「學生優先」。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下一步：寫班級特性與學生卡點" })).toBeInTheDocument();
    expect(screen.getByLabelText("班級特性、學生最近卡住的地方")).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /活動優先/ }));
    expect(await screen.findByText(/主持人目前把備課順序固定為「目標優先」/)).toBeInTheDocument();
    expect(api.callsTo("PUT", "/api/units/1")).toHaveLength(2);
    expect(screen.getByRole("heading", { name: "備課順序" })).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: /活動優先/ })).not.toBeInTheDocument();
  });

  it("hides route choice when route_choice_enabled is false and uses the fixed goal-first order", async () => {
    signIn();
    const created = unitWith({ id: 3, title: "固定順序單元", prep_route: "material", core_status: "none" });
    const api = mockApi({
      ...baseRoutes({ route_choice_enabled: false }, preferences({ start_from: "material" })),
      "POST /api/units": created,
      "GET /api/units/3": created,
      "GET /api/units/3/lessons": [],
      "GET /api/units/3/materials": [],
    });
    const user = userEvent.setup();
    renderApp("/plan");

    await screen.findByLabelText("建立新單元");
    await waitFor(() => expect(api.callsTo("GET", "/api/config")).toHaveLength(1));
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("建立新單元"), "固定順序單元");
    await user.click(screen.getByRole("button", { name: "建立單元" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/units")).toEqual({ title: "固定順序單元" }));

    expect(await screen.findByRole("heading", { name: "備課順序" })).toBeInTheDocument();
    expect(screen.queryByText("換一種備課菜單")).not.toBeInTheDocument();
    expect(screen.queryByText(/備課菜單：/)).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "下一步：寫單元前 CoRe（先寫 Q1–Q3）" })).toHaveAttribute(
      "href",
      "/units/3/core",
    );
    // every screen stays reachable: the material area is collapsed, not removed
    expect(screen.getByRole("button", { name: "展開教材區" })).toBeInTheDocument();
  });

  it("edits the unit class profile", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [],
      "PUT /api/units/1": (call: MockCall) => unitWith(call.body as UpdateUnitRequest),
    });
    const user = userEvent.setup();
    renderApp("/units/1");

    const profile = await screen.findByLabelText("這個單元的班級特性");
    await user.type(profile, "高一普通班，程度落差大");
    await user.click(screen.getByRole("button", { name: "儲存班級特性" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/units/1")).toEqual({ class_profile: "高一普通班，程度落差大" }));
    expect(await screen.findByText("已儲存班級特性。")).toBeInTheDocument();
  });

  it("highlights Q1–Q3 for goal-first and Q5/Q6 for student-first on the CoRe page", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/units/1/core": coreVersion([focus(11, 1, "重點")]) });
    renderApp("/units/1/core");
    const q1 = within(await screen.findByRole("group", { name: /你希望學生學會什麼？/ }));
    expect(q1.getByText("建議先寫")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: /這個重點可能哪裡難教？/ })).queryByText("建議先寫")).toBeNull();
  });

  it("student-first highlights Q5 and Q6", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ prep_route: "student" }),
      "GET /api/units/1/core": coreVersion([focus(11, 1, "重點")]),
    });
    renderApp("/units/1/core");
    const q5 = within(await screen.findByRole("group", { name: /學生可能怎麼想或誤解？/ }));
    expect(await q5.findByText("建議先寫")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: /還有哪些因素會影響教學？/ })).getByText("建議先寫")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: /你希望學生學會什麼？/ })).queryByText("建議先寫")).toBeNull();
  });

  it("activity-first puts the lesson plan before the CoRe links and points back to CoRe", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ prep_route: "activity" }),
      "GET /api/lessons/7": lessonDetail(),
      "GET /api/units/1/core": coreVersion([]),
      "GET /api/siop": [],
      "GET /api/teacher-cases": [],
    });
    const user = userEvent.setup();
    renderApp("/lessons/7");
    expect(await screen.findByText(/活動優先：先寫活動點子與教學步驟/)).toBeInTheDocument();
    const plan = sectionToggle("教學步驟");
    const links = sectionToggle("連結的 CoRe 重點");
    expect(plan.compareDocumentPosition(links) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await openSection(user, "連結的 CoRe 重點");
    expect(await screen.findByRole("link", { name: "回頭補寫 CoRe →" })).toHaveAttribute("href", "/units/1/core");
  });
});

describe("material-first: candidate focuses from pasted material", () => {
  const TEXT = "Not only does Amy sing well, but she also dances beautifully.";
  const candidates: AiResult<MaterialFocusesOutput> = {
    interaction_id: 77,
    status: "shown",
    message: null,
    output: {
      candidates: [
        {
          big_idea: "用倒裝句強調人物的兩項優點",
          rationale: "課文用 Not only 倒裝描述人物。",
          excerpt: "Not only does Amy sing well",
        },
        { big_idea: "描述人物特質的副詞", rationale: "課文用副詞修飾動作。", excerpt: "dances beautifully" },
      ],
    },
    sources: [],
    context: [{ label: "班級特性", preview: "高一普通班" }],
  };

  it("paste → candidates with excerpt → 放進我的學習重點 sends the draft PUT and an adopt decision; nothing is inserted without the click", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ prep_route: "material", core_status: "none", lesson_count: 0 }),
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [],
      "POST /api/units/1/materials": (call: MockCall) => ({
        id: 31,
        filename: (call.body as CreateMaterialRequest).filename ?? "",
        char_count: (call.body as CreateMaterialRequest).text.length,
        created_at: "2026-11-12T08:00:00Z",
      }),
      "POST /api/units/1/material-focuses": candidates,
      "GET /api/units/1/core": coreVersion([focus(11, 1, "既有重點")]),
      [`PUT ${DRAFT}`]: (call: MockCall) =>
        coreVersion(
          (call.body as CoreDraftRequest).focuses.map((f, i) => focus(f.id ?? 900 + i, f.position, f.big_idea)),
        ),
      "POST /api/ai/77/decision": { ok: true },
      "DELETE /api/units/1/materials/31": { ok: true },
    });
    const user = userEvent.setup();
    renderApp("/units/1");

    expect(await screen.findByRole("button", { name: "下一步：貼上課文或教材" })).toBeInTheDocument();
    await user.type(screen.getByLabelText("教材名稱（選填）"), "B1 L3 課文");
    await user.type(screen.getByLabelText("教材文字"), TEXT);
    await user.click(screen.getByRole("button", { name: "儲存教材" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/units/1/materials")).toEqual({ text: TEXT, filename: "B1 L3 課文" }),
    );

    const item = within(await screen.findByRole("listitem", { name: "B1 L3 課文" }));
    expect(screen.getByRole("button", { name: "下一步：請 Manna 從教材提出候選學習重點" })).toBeInTheDocument();
    await user.click(item.getByRole("button", { name: "請 Manna 從教材提出候選學習重點" }));
    expect(api.lastBody("POST", "/api/units/1/material-focuses")).toEqual({ material_id: 31 });

    const first = within(await screen.findByRole("article", { name: "用倒裝句強調人物的兩項優點" }));
    expect(first.getByText("Not only does Amy sing well")).toBeInTheDocument();
    expect(first.getByText("課文用 Not only 倒裝描述人物。")).toBeInTheDocument();
    expect(first.getByText("教材原文")).toBeInTheDocument();
    // AI never inserts on its own
    expect(api.callsTo("PUT", DRAFT)).toHaveLength(0);
    expect(api.callsTo("POST", "/api/ai/77/decision")).toHaveLength(0);

    await user.click(first.getByRole("button", { name: "放進我的學習重點" }));
    await waitFor(() => expect(api.callsTo("PUT", DRAFT)).toHaveLength(1));
    const draft = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
    expect(draft.focuses.map((f) => [f.id, f.position, f.big_idea])).toEqual([
      [11, 1, "既有重點"],
      [undefined, 2, "用倒裝句強調人物的兩項優點"],
    ]);
    await waitFor(() =>
      expect(api.callsTo("POST", "/api/ai/77/decision").map((c) => c.body)).toEqual([
        { item_index: 0, decision: "adopt" },
      ]),
    );
    expect(await first.findByText("已放進學習重點 2")).toBeInTheDocument();

    const second = within(screen.getByRole("article", { name: "描述人物特質的副詞" }));
    await user.click(second.getByRole("button", { name: "略過" }));
    await waitFor(() =>
      expect(api.callsTo("POST", "/api/ai/77/decision").map((c) => c.body)).toEqual([
        { item_index: 0, decision: "adopt" },
        { item_index: 1, decision: "skip" },
      ]),
    );
    expect(api.callsTo("PUT", DRAFT)).toHaveLength(1);

    const material = scaffoldEvents(api).filter((e) => e.scaffold_code === "SC-MATERIAL").map((e) => e.action);
    expect(material).toEqual(expect.arrayContaining(["expand", "show", "use", "skip"]));

    // hard delete only after the in-page confirmation
    await user.click(item.getByRole("button", { name: "刪除教材 B1 L3 課文" }));
    expect(api.callsTo("DELETE", "/api/units/1/materials/31")).toHaveLength(0);
    expect(item.getByText(/無法復原/)).toBeInTheDocument();
    await user.click(item.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/units/1/materials/31")).toHaveLength(1));
    await waitFor(() => expect(screen.queryByRole("listitem", { name: "B1 L3 課文" })).not.toBeInTheDocument());
  });

  it("refuses to add a fourth CoRe focus", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ prep_route: "material" }),
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/materials": [{ id: 31, filename: "課文", char_count: 60, created_at: "2026-11-12T08:00:00Z" }],
      "POST /api/units/1/material-focuses": candidates,
      "GET /api/units/1/core": coreVersion([focus(11, 1, "一"), focus(12, 2, "二"), focus(13, 3, "三")]),
    });
    const user = userEvent.setup();
    renderApp("/units/1");
    const item = within(await screen.findByRole("listitem", { name: "課文" }));
    await user.click(item.getByRole("button", { name: "請 Manna 從教材提出候選學習重點" }));
    const first = within(await screen.findByRole("article", { name: "用倒裝句強調人物的兩項優點" }));
    await user.click(first.getByRole("button", { name: "放進我的學習重點" }));
    expect(await first.findByText(/CoRe 已有 3 個學習重點/)).toBeInTheDocument();
    expect(api.callsTo("PUT", DRAFT)).toHaveLength(0);
    expect(api.callsTo("POST", "/api/ai/77/decision")).toHaveLength(0);
  });
});
