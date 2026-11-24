import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { PromptInfo, PromptVersion } from "../api/types";
import { baseRoutes, mockApi, renderApp, signIn, teacher, type MockCall } from "../test/mockApi";

const pi = { ...teacher, research_id: "PI-01", role: "pi" as const };

const registry: PromptInfo[] = [
  {
    key: "worksheet_warmup",
    name_zh: "暖身活動（一般學習單）",
    group: "學習單產生",
    description: "一般學習單產生的第 2 步：設計暖身。「照我的學習單做」不會用這一個。",
    uses_user_template: true,
    vars: [
      { name: "passage_numbered", label: "教材（每段有編號）", required: true, auto: false },
      { name: "class_profile", label: "班級特性", required: false, auto: true },
    ],
  },
  {
    key: "worksheet_template_section",
    name_zh: "照我的學習單做：每一區的題目",
    group: "照我的學習單做",
    description: "用範本為新教材產生學習單時，每一區各呼叫一次。",
    uses_user_template: true,
    vars: [{ name: "passage_numbered", label: "教材", required: true, auto: false }],
  },
];

function version(key: string, no: number, extra: Partial<PromptVersion> = {}): PromptVersion {
  return {
    id: no,
    prompt_key: key,
    version_no: no,
    system_text: `系統 v${no}`,
    user_template: "教材：{{passage_numbered}}",
    created_at: "2026-11-10T08:00:00Z",
    note: null,
    restored_from: null,
    created_by_research_id: null,
    by_me: false,
    ...extra,
  };
}

const list = {
  groups: ["學習單產生", "照我的學習單做"],
  registry,
  try_keys: ["worksheet_warmup"],
  prompts: [
    version("worksheet_template_section", 4),
    version("worksheet_warmup", 3, { note: "加一句", created_by_research_id: "PI-01", by_me: true }),
    version("legacy_thing", 1),
  ],
};

const aiValue = { provider: "fake", model: "fake-model", effort: "medium", timeout_secs: 60 };
const aiModel = {
  active: aiValue,
  source: "env",
  changed_by: null,
  changed_at: null,
  env_default: aiValue,
  providers: [],
  efforts: ["low", "medium", "high"],
  timeout_range: { min: 30, max: 900 },
};

function routes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/me": pi,
    "GET /api/admin/settings": { route_choice_enabled: true },
    "GET /api/admin/accounts": [],
    "GET /api/admin/ai-model": aiModel,
    "GET /api/admin/prompts": list,
    ...extra,
  };
}

describe("提示詞版本 (pi)", () => {
  it("groups prompts under their Chinese names, collapsed, and searches them", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    renderApp("/admin?tab=prompts");

    const section = within(await screen.findByRole("region", { name: "提示詞版本" }));
    const warm = await section.findByRole("region", { name: "學習單產生" });
    expect(within(warm).getByText("暖身活動（一般學習單）")).toBeInTheDocument();
    expect(within(warm).getByText("worksheet_warmup")).toBeInTheDocument();
    expect(section.getByRole("region", { name: "照我的學習單做" })).toBeInTheDocument();
    // a key nobody registered still shows
    const unregistered = section.getByRole("region", { name: "未登記" });
    expect(within(unregistered).getByText("legacy_thing")).toBeInTheDocument();
    // the long text is not on the page until a card is opened
    expect(section.queryByLabelText(/系統提示詞/)).not.toBeInTheDocument();

    await user.type(section.getByLabelText("搜尋提示詞"), "暖身");
    expect(section.queryByRole("region", { name: "照我的學習單做" })).not.toBeInTheDocument();
    expect(section.queryByRole("region", { name: "未登記" })).not.toBeInTheDocument();
    expect(within(section.getByRole("region", { name: "學習單產生" })).getByText("暖身活動（一般學習單）")).toBeInTheDocument();
  });

  it("stops a save that drops a required variable, warns about an unknown one, and sends the note", async () => {
    signIn();
    const api = mockApi(
      routes({
        "POST /api/admin/prompts": (call: MockCall) => ({
          ...version("worksheet_warmup", 4),
          ...(call.body as object),
          unknown_variables: ["oops"],
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/admin?tab=prompts");
    await user.click(await screen.findByText("worksheet_warmup", { selector: "summary code" }));

    // what it is for, and its variables
    expect(screen.getByText(/「照我的學習單做」不會用這一個/, { selector: "p" })).toBeInTheDocument();
    await user.click(screen.getByText(/程式會填入的變數/));
    expect(screen.getByText("必要")).toBeInTheDocument();
    expect(screen.getByText("沒寫也會自動補上")).toBeInTheDocument();

    const userBox = screen.getByLabelText(/使用者範本/);
    await user.clear(userBox);
    await user.type(userBox, "只有文字");
    expect(screen.getByRole("alert")).toHaveTextContent("{{passage_numbered}}");
    expect(screen.getByRole("button", { name: "儲存為新版本" })).toBeDisabled();

    await user.clear(userBox);
    await user.type(userBox, "教材 {{{{passage_numbered}} 與 {{{{oops}}");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("{{oops}}");
    await user.type(screen.getByLabelText(/這次改了什麼/), "改成兩個問題");
    await user.click(screen.getByRole("button", { name: "儲存為新版本" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/admin/prompts")).toEqual({
        prompt_key: "worksheet_warmup",
        system_text: "系統 v3",
        user_template: "教材 {{passage_numbered}} 與 {{oops}}",
        note: "改成兩個問題",
      }),
    );
    expect(await screen.findByText(/已儲存為 v4/)).toBeInTheDocument();
  });

  it("shows the history, compares an old version and restores it as a new version", async () => {
    signIn();
    const api = mockApi(
      routes({
        "GET /api/admin/prompts/worksheet_warmup/versions": [
          version("worksheet_warmup", 3, { note: "加一句", created_by_research_id: "PI-01", by_me: true }),
          version("worksheet_warmup", 2, { created_by_research_id: "PI-02", system_text: "第一行\n舊的第二行" }),
          version("worksheet_warmup", 1, { system_text: "第一行\n舊的第二行\n只有舊版有" }),
        ],
        "POST /api/admin/prompts": (call: MockCall) => ({
          ...version("worksheet_warmup", 4),
          ...(call.body as object),
          unknown_variables: [],
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/admin?tab=prompts");
    await user.click(await screen.findByText("worksheet_warmup", { selector: "summary code" }));
    await user.click(screen.getByRole("button", { name: /版本紀錄（共 3 版）/ }));

    const history = within(await screen.findByRole("list", { name: "版本紀錄" }));
    const rows = history.getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(within(rows[0]!).getByText("使用中")).toBeInTheDocument();
    expect(within(rows[0]!).getByText(/你/)).toBeInTheDocument();
    expect(within(rows[0]!).getByText("加一句")).toBeInTheDocument();
    expect(within(rows[1]!).getByText(/PI-02/)).toBeInTheDocument();
    expect(within(rows[2]!).getByText(/初始版本/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "查看 v1 並和目前版本比較" }));
    const diff = within(screen.getByLabelText("系統提示詞的差異"));
    expect(diff.getByText("只有舊版有").closest(".diff-line")).toHaveClass("diff-add");
    expect(diff.getByText("系統 v3").closest(".diff-line")).toHaveClass("diff-del");

    await user.click(screen.getByRole("button", { name: "用這一版建立新版本" }));
    expect(screen.getByText(/會新增 v4/)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/prompts")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "確定建立 v4" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/admin/prompts")).toEqual({
        prompt_key: "worksheet_warmup",
        system_text: "第一行\n舊的第二行\n只有舊版有",
        user_template: "教材：{{passage_numbered}}",
        note: "還原自 v1",
        restored_from: 1,
      }),
    );
  });

  it("tries the unsaved draft on a sample, shows output and checks, and offers nothing for other prompts", async () => {
    signIn();
    const api = mockApi(
      routes({
        "POST /api/admin/prompts/worksheet_warmup/try": () => ({
          run_id: 5,
          preview: true,
          status: "ok",
          message: null,
          system: "SYSTEM-TEXT",
          user: "USER-TEXT",
          output: { background_activities: [] },
          checks: [
            { name: "schema", passed: true },
            { name: "english_only", passed: false, detail: "第 1 個活動含有中文" },
          ],
          notes: [],
          unknown_variables: [],
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/admin?tab=prompts");
    await user.click(await screen.findByText("worksheet_template_section", { selector: "summary code" }));
    expect(screen.queryByText(/試產生：先試試看/)).not.toBeInTheDocument();
    await user.click(screen.getByText("worksheet_template_section", { selector: "summary code" }));
    await user.click(screen.getByText("worksheet_warmup", { selector: "summary code" }));
    await user.click(screen.getByText(/試產生：先試試看/));
    expect(screen.getByText(/這是預覽/)).toHaveTextContent("不會出現在任何老師的畫面");

    const run = screen.getByRole("button", { name: "試產生" });
    expect(run).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "放入範例短文" }));
    // the edited, unsaved text is what is sent
    const system = screen.getByLabelText(/系統提示詞/);
    await user.type(system, "草稿");
    await user.click(run);
    const result = within(await screen.findByLabelText("試產生的結果"));
    const checks = result.getByRole("list", { name: "平台檢查" });
    expect(checks).toHaveTextContent("通過：schema");
    expect(checks).toHaveTextContent("沒通過：english_only");
    expect(checks).toHaveTextContent("第 1 個活動含有中文");
    expect(result.getByText(/background_activities/)).toBeInTheDocument();
    const body = api.lastBody("POST", "/api/admin/prompts/worksheet_warmup/try") as {
      system_text: string;
      sample: { passage: string };
    };
    expect(body.system_text).toBe("系統 v3草稿");
    expect(body.sample.passage).toContain("Honeybees");
    // nothing was saved
    expect(api.callsTo("POST", "/api/admin/prompts")).toHaveLength(0);

    // a draft that dropped a required variable cannot be tried
    const userBox = screen.getByLabelText(/使用者範本/);
    await user.clear(userBox);
    await user.type(userBox, "沒有變數");
    expect(screen.getByRole("button", { name: "試產生" })).toBeDisabled();
  });
});
