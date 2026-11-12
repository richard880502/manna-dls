import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { AiResult, PlanSuggestOutput, SaveLessonVersionRequest } from "../api/types";
import {
  baseRoutes,
  coreVersion,
  focus,
  lessonDetail,
  mockApi,
  renderApp,
  signIn,
  type MockCall,
} from "../test/mockApi";

const SUGGEST = "/api/lessons/7/suggestions";
const DECISION = "/api/ai/55/decision";

const shownResult: AiResult<PlanSuggestOutput> = {
  interaction_id: 55,
  status: "shown",
  message: null,
  output: {
    kb_empty: false,
    suggestions: [
      {
        title: "語意對比暖身",
        body: "先比較一般語序與倒裝句兩組句子，請學生說出共同語意。",
        activity_type: "暖身",
        icap_level: "constructive",
        citations: ["CUR-E-5-01"],
        duration_min: null,
        support_note: null,
        extension_note: null,
      },
      {
        title: "兩兩改寫",
        body: "學生兩兩把一般句改寫成 Not only 倒裝句並互相檢查。",
        activity_type: "兩兩討論",
        icap_level: "interactive",
        citations: ["GRM-076", "VOC-2000-12"],
        duration_min: null,
        support_note: null,
        extension_note: null,
      },
      {
        title: "出場券",
        body: "每人寫一句倒裝句並說明語意。",
        activity_type: null,
        icap_level: null,
        citations: ["CUR-E-5-01"],
        duration_min: null,
        support_note: null,
        extension_note: null,
      },
    ],
  },
  sources: [
    {
      item_code: "CUR-E-5-01",
      title: "語言知識：句型與語意",
      category: "curriculum",
      source_doc: "十二年國教課綱英語文",
      source_page: "12",
    },
    { item_code: "GRM-076", title: "倒裝句", category: "grammar", source_doc: "文法 76 項", source_page: "45" },
    { item_code: "VOC-2000-12", title: "only", category: "vocabulary", source_doc: "國教院 2000 字", source_page: "88" },
  ],
  context: [],
};

function lessonRoutes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/lessons/7": lessonDetail(),
    "GET /api/units/1/core": coreVersion([
      focus(11, 1, "倒裝句的語意功能", { Q1: { answer: "能說明倒裝的語意", status: "filled" } }),
      focus(12, 2, "口語表達", { Q1: { answer: "", status: "no_idea" } }),
    ]),
    "GET /api/siop": [],
    "GET /api/teacher-cases": [],
    [`POST ${DECISION}`]: { ok: true },
    ...extra,
  };
}

async function openLesson() {
  renderApp("/lessons/7");
  expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also" })).toBeInTheDocument();
}

describe("Study & Plan lesson page", () => {
  it("renders suggestion citations and sends adopt / revise / reject decisions per API.md", async () => {
    signIn();
    const api = mockApi(
      lessonRoutes({
        [`POST ${SUGGEST}`]: shownResult,
        "POST /api/lessons/7/versions": (call: MockCall) => ({
          id: 71,
          version_no: 3,
          objective: (call.body as SaveLessonVersionRequest).objective,
          steps: (call.body as SaveLessonVersionRequest).steps,
          created_at: "2026-11-12T10:00:00Z",
        }),
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    expect(screen.getByText(/目前第 2 版（共 2 版）/)).toBeInTheDocument();

    await user.type(screen.getByLabelText("想請 Manna 特別注意什麼？（選填）"), "多一點口說");
    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));

    const first = within(await screen.findByRole("article", { name: "語意對比暖身" }));
    expect(api.callsTo("POST", SUGGEST)[0]?.body).toEqual({ request_note: "多一點口說" });
    expect(first.getByText("CUR-E-5-01")).toBeInTheDocument();
    expect(first.getByText("語言知識：句型與語意")).toBeInTheDocument();
    expect(first.getByText(/十二年國教課綱英語文，p\. 12/)).toBeInTheDocument();

    const second = within(screen.getByRole("article", { name: "兩兩改寫" }));
    expect(second.getByText("GRM-076")).toBeInTheDocument();
    expect(second.getByText(/文法 76 項，p\. 45/)).toBeInTheDocument();
    expect(second.getByText("VOC-2000-12")).toBeInTheDocument();
    expect(second.getByText(/國教院 2000 字，p\. 88/)).toBeInTheDocument();

    // adopt → appended as a new step (still unsaved)
    await user.click(first.getByRole("button", { name: "採用" }));
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(1));
    expect(api.callsTo("POST", DECISION)[0]?.body).toEqual({ item_index: 0, decision: "adopt" });
    expect(await first.findByText("已採用")).toBeInTheDocument();
    expect(screen.getByLabelText("步驟 2").querySelector("textarea")).toHaveValue(
      "語意對比暖身：先比較一般語序與倒裝句兩組句子，請學生說出共同語意。",
    );
    expect(screen.getByText("尚未儲存")).toBeInTheDocument();

    // revise with edited text and a reason
    await user.click(second.getByRole("button", { name: "修訂" }));
    const revise = second.getByLabelText("修訂後的內容");
    expect(revise).toHaveValue("學生兩兩把一般句改寫成 Not only 倒裝句並互相檢查。");
    await user.clear(revise);
    await user.type(revise, "改成三人小組輪流改寫");
    await user.selectOptions(second.getByLabelText("理由（選填）"), "better_idea");
    await user.click(second.getByRole("button", { name: "送出修訂" }));
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(2));
    expect(api.callsTo("POST", DECISION)[1]?.body).toEqual({
      item_index: 1,
      decision: "revise",
      revised_content: "改成三人小組輪流改寫",
      reason: "better_idea",
    });

    // reject with reason "other" + note
    const third = within(screen.getByRole("article", { name: "出場券" }));
    await user.selectOptions(third.getByLabelText("理由（選填）"), "other");
    await user.type(third.getByLabelText("其他理由說明（選填）"), "時間不夠");
    await user.click(third.getByRole("button", { name: "拒絕" }));
    await waitFor(() => expect(api.callsTo("POST", DECISION)).toHaveLength(3));
    expect(api.callsTo("POST", DECISION)[2]?.body).toEqual({
      item_index: 2,
      decision: "reject",
      reason: "other",
      reason_note: "時間不夠",
    });
    expect(await third.findByText("已拒絕")).toBeInTheDocument();

    // teacher saves: a new version with ordered positions
    await user.click(screen.getByRole("button", { name: "儲存教案（產生新版本）" }));
    expect(await screen.findByText(/已儲存為第 3 版/)).toBeInTheDocument();
    const saved = api.lastBody("POST", "/api/lessons/7/versions") as SaveLessonVersionRequest;
    expect(saved.objective).toBe("學生能說明倒裝的語意功能");
    expect(saved.steps.map((s) => [s.position, s.activity_type, s.icap_level])).toEqual([
      [1, "暖身", "active"],
      [2, "暖身", "constructive"],
      [3, "兩兩討論", "interactive"],
    ]);
    expect(saved.steps[2]?.content).toBe("改成三人小組輪流改寫");

    const rag = api
      .callsTo("POST", "/api/scaffold-events")
      .map((c) => c.body as { scaffold_code: string; action: string })
      .filter((e) => e.scaffold_code === "SC-RAG")
      .map((e) => e.action);
    expect(rag).toEqual(expect.arrayContaining(["expand", "show", "use", "skip"]));
  });

  it("shows 「知識庫查無相關條目」 when kb_empty", async () => {
    signIn();
    mockApi(
      lessonRoutes({
        [`POST ${SUGGEST}`]: {
          interaction_id: 56,
          status: "shown",
          message: null,
          output: { kb_empty: true, suggestions: [] },
          sources: [],
        },
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));
    expect(await screen.findByText(/知識庫查無相關條目/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "採用" })).not.toBeInTheDocument();
  });

  it.each([
    ["blocked", "目前暫無建議，你可以繼續手動完成。", "目前暫無建議，你可以繼續手動完成。"],
    ["timeout", null, "AI 回應逾時，你可以繼續手動完成。"],
    ["disabled", "AI 已由主持人暫停。", "AI 已由主持人暫停。"],
  ])("shows the message for a %s result", async (status, message, expected) => {
    signIn();
    mockApi(
      lessonRoutes({
        [`POST ${SUGGEST}`]: { interaction_id: null, status, message, output: null, sources: [] },
      }),
    );
    const user = userEvent.setup();
    await openLesson();
    await user.click(screen.getByRole("button", { name: "請 Manna 提供建議" }));
    expect(await screen.findByText(expected)).toBeInTheDocument();
  });

  it("disables the suggestion button when ai_enabled is false; the editor still saves", async () => {
    signIn();
    const api = mockApi({
      ...lessonRoutes(),
      "GET /api/config": { demo_mode: false, ai_enabled: false, ai_provider: "", ai_model: "" },
      "POST /api/lessons/7/versions": {
        id: 72,
        version_no: 3,
        objective: "",
        steps: [],
        created_at: "2026-11-12T10:00:00Z",
      },
    });
    const user = userEvent.setup();
    await openLesson();
    expect(await screen.findByRole("button", { name: "AI 目前未開放" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "儲存教案（產生新版本）" }));
    expect(await screen.findByText(/已儲存為第 3 版/)).toBeInTheDocument();
    expect(api.callsTo("POST", SUGGEST)).toHaveLength(0);
  });

  it("links CoRe focuses (shows related answers) and shows the previous lesson reminder", async () => {
    signIn();
    const api = mockApi({
      ...lessonRoutes(),
      "GET /api/lessons/7": lessonDetail({ previous_next_change: "下次先給例句再請學生歸納" }),
      "PUT /api/lessons/7/core-links": (call: MockCall) => call.body,
    });
    const user = userEvent.setup();
    await openLesson();
    expect(screen.getByText("下次先給例句再請學生歸納")).toBeInTheDocument();

    await user.click(await screen.findByRole("checkbox", { name: /學習重點 1：倒裝句的語意功能/ }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/lessons/7/core-links")).toEqual({ focus_ids: [11] }));
    expect(await screen.findByText("能說明倒裝的語意")).toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: /學習重點 2：口語表達/ }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/lessons/7/core-links")).toEqual({ focus_ids: [11, 12] }));
    expect(screen.getAllByText("目前沒有想法").length).toBeGreaterThan(0);
  });

  it("reorders and removes steps before saving", async () => {
    signIn();
    const api = mockApi({
      ...lessonRoutes(),
      "POST /api/lessons/7/versions": (call: MockCall) => ({
        id: 73,
        version_no: 3,
        objective: "",
        steps: (call.body as SaveLessonVersionRequest).steps,
        created_at: "2026-11-12T10:00:00Z",
      }),
    });
    const user = userEvent.setup();
    await openLesson();

    await user.click(screen.getByRole("button", { name: "＋ 新增步驟" }));
    const step2 = within(screen.getByLabelText("步驟 2"));
    await user.type(step2.getByLabelText("活動型態"), "小組討論");
    await user.selectOptions(step2.getByLabelText("ICAP 層次"), "interactive");
    await user.type(step2.getByLabelText("步驟內容"), "小組改寫");
    await user.click(screen.getByRole("button", { name: "步驟 2 上移" }));
    await user.click(screen.getByRole("button", { name: "＋ 新增步驟" }));
    await user.click(screen.getByRole("button", { name: "刪除步驟 3" }));
    await user.click(screen.getByRole("button", { name: "儲存教案（產生新版本）" }));

    await screen.findByText(/已儲存為第 3 版/);
    const saved = api.lastBody("POST", "/api/lessons/7/versions") as SaveLessonVersionRequest;
    const blankDiff = { duration_min: null, support_note: "", extension_note: "" };
    expect(saved.steps).toEqual([
      { position: 1, activity_type: "小組討論", icap_level: "interactive", content: "小組改寫", ...blankDiff },
      { position: 2, activity_type: "暖身", icap_level: "active", content: "比較兩組句子", ...blankDiff },
    ]);
  });

  it("SIOP hints are optional, selectable and saved; teacher cases can be searched", async () => {
    signIn();
    const api = mockApi({
      ...lessonRoutes(),
      "GET /api/siop": [
        { id: 1, internal_id: "SIOP-01", component: "Lesson Preparation", summary: "清楚寫出語言目標" },
        { id: 2, internal_id: "SIOP-02", component: "Building Background", summary: "連結學生生活經驗" },
      ],
      "PUT /api/lessons/7/siop": (call: MockCall) => call.body,
      "GET /api/teacher-cases": (call: MockCall) =>
        call.url.includes("q=")
          ? [
              {
                case_code: "TC-2",
                school_track: "both",
                grade: "高二",
                topic: "口說暖身",
                summary: "用圖片引導口說",
                details: "詳細流程",
              },
            ]
          : [
              {
                case_code: "TC-1",
                school_track: "general",
                grade: "高一",
                topic: "倒裝句",
                summary: "用語意卡片比較句型",
                details: "先發卡片，再兩兩比較",
              },
            ],
    });
    const user = userEvent.setup();
    await openLesson();

    await user.click(await screen.findByRole("checkbox", { name: /清楚寫出語言目標/ }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/lessons/7/siop")).toEqual({ siop_feature_ids: [1] }));

    expect(await screen.findByText(/用語意卡片比較句型/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "閱讀" }));
    expect(screen.getByText("先發卡片，再兩兩比較")).toBeInTheDocument();

    await user.type(screen.getByRole("searchbox", { name: "搜尋教師經驗" }), "口說");
    await user.click(screen.getByRole("button", { name: "搜尋" }));
    expect(await screen.findByText("用圖片引導口說")).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/teacher-cases").some((c) => c.url.endsWith("?q=%E5%8F%A3%E8%AA%AA"))).toBe(true);
  });
});
