import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type {
  AiResult,
  Hardest,
  LessonDetail,
  PlanSuggestOutput,
  SaveLessonVersionRequest,
  UpdateLessonRequest,
} from "../api/types";
import {
  baseRoutes,
  coreVersion,
  lessonDetail,
  mockApi,
  openSection,
  preferences,
  renderApp,
  savePlanButton,
  sectionToggle,
  signIn,
  step,
  type MockCall,
} from "../test/mockApi";

const SUGGEST = "/api/lessons/7/suggestions";

function routesFor(hardest: Hardest[], extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes({}, preferences({ hardest })),
    "GET /api/lessons/7": lessonDetail(),
    "GET /api/units/1/core": coreVersion([]),
    "GET /api/siop": [],
    "GET /api/teacher-cases": [],
    "POST /api/ai/60/decision": { ok: true },
    ...extra,
  };
}

const suggestionResult = (context: AiResult<PlanSuggestOutput>["context"] = []): AiResult<PlanSuggestOutput> => ({
  interaction_id: 60,
  status: "shown",
  message: null,
  output: {
    kb_empty: false,
    suggestions: [
      {
        title: "分層改寫",
        body: "學生依程度改寫倒裝句。",
        activity_type: "個人練習",
        icap_level: "constructive",
        citations: ["GRM-076"],
        duration_min: 12,
        support_note: "提供句型框架",
        extension_note: "改寫成兩種倒裝並比較語氣",
      },
    ],
  },
  sources: [{ item_code: "GRM-076", title: "倒裝句", category: "grammar", source_doc: "文法 76 項", source_page: "45" }],
  context,
});

function events(api: ReturnType<typeof mockApi>) {
  return api.callsTo("POST", "/api/scaffold-events").map((c) => c.body as Record<string, unknown>);
}

async function openLesson() {
  renderApp("/lessons/7");
  expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also" })).toBeInTheDocument();
}

describe("preference: 差異化與時間安排 (differentiation)", () => {
  it("shows time / support / extension fields by default, totals the minutes and logs SC-PREF", async () => {
    signIn();
    const api = mockApi(
      routesFor(["differentiation"], {
        "GET /api/lessons/7": lessonDetail({
          latest_version: {
            id: 70,
            version_no: 2,
            objective: "目標",
            steps: [
              step({ position: 1, activity_type: "暖身", content: "比較", duration_min: 10 }),
              step({ position: 2, activity_type: "練習", content: "改寫", duration_min: 15, support_note: "句型框架" }),
            ],
            created_at: "2026-11-11T08:00:00Z",
          },
        }),
        "POST /api/lessons/7/versions": (call: MockCall) => ({
          id: 71,
          version_no: 3,
          objective: "目標",
          steps: (call.body as SaveLessonVersionRequest).steps,
          created_at: "2026-11-12T10:00:00Z",
        }),
      }),
    );
    const user = userEvent.setup();
    await openLesson();

    const s1 = within(screen.getByLabelText("步驟 1"));
    expect(s1.getByLabelText("預估時間（分鐘）")).toHaveValue(10);
    expect(s1.getByLabelText("程度較弱的支援")).toBeInTheDocument();
    expect(s1.getByLabelText("程度較好的延伸")).toBeInTheDocument();
    expect(screen.getByText("依你的備課偏好預設顯示")).toBeInTheDocument();
    expect(screen.getByText("預估總時間：25 分鐘")).toBeInTheDocument();

    await user.clear(s1.getByLabelText("預估時間（分鐘）"));
    await user.type(s1.getByLabelText("預估時間（分鐘）"), "20");
    expect(screen.getByText("預估總時間：35 分鐘")).toBeInTheDocument();

    await waitFor(() =>
      expect(events(api)).toContainEqual({
        scaffold_code: "SC-PREF",
        action: "show",
        trigger: "preference:hardest=differentiation",
      }),
    );

    await user.click(savePlanButton());
    await screen.findByText(/已儲存為第 3 版/);
    const saved = api.lastBody("POST", "/api/lessons/7/versions") as SaveLessonVersionRequest;
    expect(saved.steps.map((s) => [s.duration_min, s.support_note, s.extension_note])).toEqual([
      [20, "", ""],
      [15, "句型框架", ""],
    ]);
  });

  it("without the preference the fields sit behind 「顯示時間與差異化欄位」 and no SC-PREF default is logged", async () => {
    signIn();
    const api = mockApi(routesFor([]));
    const user = userEvent.setup();
    await openLesson();

    expect(screen.queryByLabelText("預估時間（分鐘）")).not.toBeInTheDocument();
    expect(screen.queryByText(/預估總時間/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "顯示時間與差異化欄位" }));
    expect(within(screen.getByLabelText("步驟 1")).getByLabelText("預估時間（分鐘）")).toBeInTheDocument();
    expect(screen.getByText("預估總時間：0 分鐘")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "隱藏時間與差異化欄位" })).toBeInTheDocument();
    expect(screen.queryByText("依你的備課偏好預設顯示")).not.toBeInTheDocument();

    await waitFor(() => expect(events(api).some((e) => e.trigger === "manual:differentiation")).toBe(true));
    expect(events(api).some((e) => String(e.trigger ?? "").startsWith("preference:"))).toBe(false);
  });

  it("suggestion cards show the three fields (time labelled as an AI estimate) and adopting copies them", async () => {
    signIn();
    const api = mockApi(routesFor(["differentiation"], { [`POST ${SUGGEST}`]: suggestionResult() }));
    const user = userEvent.setup();
    await openLesson();

    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));
    const card = within(await screen.findByRole("article", { name: "分層改寫" }));
    expect(card.getByText("12 分鐘")).toBeInTheDocument();
    expect(card.getByText("AI 估計，請依班級調整")).toBeInTheDocument();
    expect(card.getByText("提供句型框架")).toBeInTheDocument();
    expect(card.getByText("改寫成兩種倒裝並比較語氣")).toBeInTheDocument();

    await user.click(card.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", "/api/ai/60/decision")).toHaveLength(1));
    const s2 = within(await screen.findByLabelText("步驟 2"));
    expect(s2.getByLabelText("預估時間（分鐘）")).toHaveValue(12);
    expect(s2.getByLabelText("程度較弱的支援")).toHaveValue("提供句型框架");
    expect(s2.getByLabelText("程度較好的延伸")).toHaveValue("改寫成兩種倒裝並比較語氣");
    expect(screen.getByText("預估總時間：12 分鐘")).toBeInTheDocument();
  });
});

describe("preference: 找合適素材 (material)", () => {
  it("moves the RAG suggestion panel to the top of the lesson page, expanded, right under the first view", async () => {
    signIn();
    const api = mockApi(routesFor(["material"]));
    await openLesson();
    const rag = screen.getByRole("region", { name: "備課建議" });
    expect(within(rag).getByText("依你的備課偏好置頂")).toBeInTheDocument();
    expect(within(rag).getByRole("button", { name: "請 Manna 提供建議" })).toBeInTheDocument();
    // 今天要教什麼？ stays first (R1); the pinned panel comes before every collapsed section.
    const today = screen.getByRole("region", { name: "今天要教什麼？" });
    expect(today.compareDocumentPosition(rag) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const links = sectionToggle("連結的 CoRe 重點");
    expect(rag.compareDocumentPosition(links) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await waitFor(() =>
      expect(events(api)).toContainEqual({ scaffold_code: "SC-PREF", action: "show", trigger: "preference:hardest=material" }),
    );
  });
});

describe("「Manna 已帶入的資訊」 (AiResult.context)", () => {
  const context = [
    { label: "班級特性", preview: "高一普通班，程度落差大" },
    { label: "CoRe 學習重點", preview: "倒裝句的語意功能" },
  ];

  it("is open by default for 跟 AI 溝通 and logs SC-PREF", async () => {
    signIn();
    const api = mockApi(routesFor(["ai_communication"], { [`POST ${SUGGEST}`]: suggestionResult(context) }));
    const user = userEvent.setup();
    await openLesson();
    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));

    const panel = await screen.findByRole("region", { name: "Manna 已帶入的資訊" });
    expect(within(panel).getByText("班級特性")).toBeInTheDocument();
    expect(within(panel).getByText("高一普通班，程度落差大")).toBeInTheDocument();
    expect(within(panel).getByText("倒裝句的語意功能")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Manna 已帶入的資訊/ })).toHaveAttribute("aria-expanded", "true");
    await waitFor(() =>
      expect(events(api)).toContainEqual({
        scaffold_code: "SC-PREF",
        action: "show",
        trigger: "preference:hardest=ai_communication",
      }),
    );
  });

  it("is collapsed for others, opens on click and logs SC-CONTEXT expand", async () => {
    signIn();
    const api = mockApi(routesFor([], { [`POST ${SUGGEST}`]: suggestionResult(context) }));
    const user = userEvent.setup();
    await openLesson();
    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));

    const toggle = await screen.findByRole("button", { name: /Manna 已帶入的資訊/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("高一普通班，程度落差大")).not.toBeInTheDocument();
    await user.click(toggle);
    expect(screen.getByText("高一普通班，程度落差大")).toBeInTheDocument();
    await waitFor(() =>
      expect(events(api)).toContainEqual(expect.objectContaining({ scaffold_code: "SC-CONTEXT", action: "expand" })),
    );
    expect(events(api).some((e) => e.scaffold_code === "SC-PREF" && e.action === "show")).toBe(false);
  });

  it("renders nothing when the result carries no context", async () => {
    signIn();
    mockApi(routesFor(["ai_communication"], { [`POST ${SUGGEST}`]: suggestionResult([]) }));
    const user = userEvent.setup();
    await openLesson();
    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));
    await screen.findByRole("article", { name: "分層改寫" });
    expect(screen.queryByRole("button", { name: /Manna 已帶入的資訊/ })).not.toBeInTheDocument();
  });
});

describe("lesson background: class description override and 我的範例", () => {
  it("shows the effective profile and saves both fields with PUT /api/lessons/:id", async () => {
    signIn();
    const api = mockApi(
      routesFor([], {
        "PUT /api/lessons/7": (call: MockCall): LessonDetail => {
          const body = call.body as UpdateLessonRequest;
          return lessonDetail({
            class_description: body.class_description ?? "",
            my_example: body.my_example ?? "",
            effective_class_profile: body.class_description || "單元：高一資優班",
          });
        },
      }),
    );
    const user = userEvent.setup();
    await openLesson();

    await openSection(user, "本課背景");
    const effective = screen.getByLabelText("目前生效的班級特性");
    expect(within(effective).getByText("目前生效的班級特性（本課覆寫）")).toBeInTheDocument();
    expect(within(effective).getByText("高一普通班")).toBeInTheDocument();
    expect(screen.getByText(/我的範例只存在這個課例，不會進入共用知識庫/)).toBeInTheDocument();

    await user.clear(screen.getByLabelText("本課班級描述（覆寫單元設定）"));
    await user.type(screen.getByLabelText("我的範例"), "Warm-up: picture talk");
    await user.click(screen.getByRole("button", { name: "儲存本課背景" }));

    await waitFor(() =>
      expect(api.lastBody("PUT", "/api/lessons/7")).toEqual({ class_description: "", my_example: "Warm-up: picture talk" }),
    );
    expect(await screen.findByText("已儲存本課背景。")).toBeInTheDocument();
    expect(within(effective).getByText("單元：高一資優班")).toBeInTheDocument();
    expect(within(effective).getByText("目前生效的班級特性（沿用單元設定）")).toBeInTheDocument();
  });
});
