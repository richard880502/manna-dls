import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { baseRoutes, mockApi, renderApp, respond, signIn, teacher } from "../test/mockApi";

const envValue = { provider: "openai_compatible", model: "gemma-4-31B-it", effort: "medium", timeout_secs: 120 };
const aiModel = {
  active: envValue,
  source: "env",
  changed_by: null,
  changed_at: null,
  env_default: envValue,
  providers: [{ id: "openai_compatible", label: "國網中心（NCHC）", available: true, models: ["gemma-4-31B-it"], uses_effort: false }],
  efforts: ["low", "medium"],
  timeout_range: { min: 30, max: 900 },
};
const traffic = (now: object = {}, hour: object = {}) => ({
  now: { max_running: 6, running: 4, queued: 7, oldest_wait_secs: 38, queue_enabled: true, quota_blocked_secs: null, ...now },
  last_hour: {
    jobs: { done: 52, failed: 1, cancelled: 0 },
    wait_secs: { p50: 4, p95: 41 },
    run_secs: {},
    calls: 488,
    errors: 0,
    rate_limited: 0,
    quota: 0,
    overloaded: 0,
    tokens: { prompt: 0, cached: 0, completion: 0, reasoning: 0 },
    latency_ms: { p50: 0, p95: 0 },
    ...hour,
  },
  last_7_days: [],
});
const prompt = (key: string, version_no = 1) => ({
  id: 1,
  prompt_key: key,
  version_no,
  system_text: "s",
  user_template: "u",
  created_at: "2026-11-11T08:00:00Z",
});
const info = (key: string, group: string, name: string) => ({
  key,
  name_zh: name,
  group,
  description: `${name}的說明`,
  vars: [],
  uses_user_template: true,
});

function routes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/me": { ...teacher, role: "pi" },
    "GET /api/admin/ai-model": aiModel,
    "GET /api/admin/ai-traffic": traffic(),
    "GET /api/admin/memory-settings": { enabled: true, graph_enabled: true },
    "GET /api/admin/memory-index": { pending: 2, failed: 0, compatible: true, providers_ready: true },
    "GET /api/admin/settings": { route_choice_enabled: false },
    "GET /api/admin/accounts": [
      { id: 1, research_id: "PI-01", school_track: "general", role: "pi", created_at: "2026-11-01T00:00:00Z", is_self: true },
    ],
    "GET /api/admin/prompts": {
      groups: ["學習單產生", "單元與課例（舊流程）"],
      registry: [info("a", "學習單產生", "暖身"), info("b", "單元與課例（舊流程）", "舊單元")],
      try_keys: [],
      prompts: [prompt("a"), prompt("b")],
    },
    ...extra,
  };
}

describe("後台管理 tabs", () => {
  it("opens on 總覽 with one status row per item", async () => {
    signIn();
    mockApi(routes());
    renderApp("/admin");
    const list = within(await screen.findByRole("list", { name: "目前設定" }));
    expect(await list.findByText("國網中心（NCHC），gemma-4-31B-it")).toBeInTheDocument();
    expect(await list.findByText("開啟，索引待同步 2")).toBeInTheDocument();
    expect(await list.findByText("固定順序")).toBeInTheDocument();
    expect(await list.findByText("1 個帳號")).toBeInTheDocument();
    expect(await list.findByText("2 個提示詞")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "總覽" })).toHaveAttribute("aria-selected", "true");
  });

  it("keeps one row's failure from breaking the others", async () => {
    signIn();
    mockApi(routes({ "GET /api/admin/ai-model": respond(500, { error: { code: "x", message: "模型設定讀不到" } }) }));
    renderApp("/admin");
    const list = within(await screen.findByRole("list", { name: "目前設定" }));
    expect(await list.findByText(/模型設定讀不到/)).toBeInTheDocument();
    expect(await list.findByText("1 個帳號")).toBeInTheDocument();
  });

  it("shows the AI request row plainly when traffic is normal", async () => {
    signIn();
    mockApi(routes());
    renderApp("/admin");
    const list = within(await screen.findByRole("list", { name: "目前設定" }));
    const text = await list.findByText("進行中 4／排隊 7（最久 38 秒），近一小時 呼叫 488 次、太頻繁 0 次");
    expect(text).not.toHaveClass("is-warn");
    expect(text).not.toHaveClass("is-error");
  });

  it("warns when jobs wait long or a call was rate limited", async () => {
    signIn();
    mockApi(routes({ "GET /api/admin/ai-traffic": traffic({ oldest_wait_secs: 150 }) }));
    const { unmount } = renderApp("/admin");
    expect(await screen.findByText(/最久 150 秒/)).toHaveClass("is-warn");
    unmount();
    mockApi(routes({ "GET /api/admin/ai-traffic": traffic({}, { rate_limited: 3 }) }));
    renderApp("/admin");
    const row = await screen.findByText(/太頻繁 3 次/);
    expect(row).toHaveClass("is-warn");
  });

  it("shows the quota message in error style while the cooldown runs", async () => {
    signIn();
    mockApi(routes({ "GET /api/admin/ai-traffic": traffic({ quota_blocked_secs: 420 }) }));
    renderApp("/admin");
    const row = await screen.findByText(/額度已用完（約 7 分鐘後再試），請到 OpenAI 帳戶提高上限或加值/);
    expect(row).toHaveClass("is-error");
  });

  it("keeps a failing AI traffic endpoint from breaking the other rows", async () => {
    signIn();
    mockApi(routes({ "GET /api/admin/ai-traffic": respond(500, { error: { code: "x", message: "流量讀不到" } }) }));
    renderApp("/admin");
    const list = within(await screen.findByRole("list", { name: "目前設定" }));
    expect(await list.findByText(/流量讀不到/)).toBeInTheDocument();
    expect(await list.findByText("1 個帳號")).toBeInTheDocument();
    expect(await list.findByText("2 個提示詞")).toBeInTheDocument();
  });

  it("selects the tab from ?tab= and updates it when a tab is clicked", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/admin?tab=settings");
    expect(await screen.findByRole("region", { name: "文字模型" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "模型與研究設定" })).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("tab", { name: "匯入" }));
    expect(router.state.location.search).toBe("?tab=import");
    expect(await screen.findByRole("region", { name: "匯入教師備課經驗" })).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "總覽" }));
    expect(router.state.location.search).toBe("");
  });

  it("switches tab from an overview link", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = renderApp("/admin");
    await user.click(await screen.findByRole("link", { name: "管理（帳號）" }));
    expect(router.state.location.search).toBe("?tab=accounts");
    expect(await screen.findByRole("region", { name: "帳號與角色" })).toBeInTheDocument();
    expect(screen.getByText("各角色能做什麼")).toBeInTheDocument();
  });

  it("shows group chips on the prompts tab and starts the old-flow group collapsed", async () => {
    signIn();
    mockApi(routes());
    renderApp("/admin?tab=prompts");
    const chips = within(await screen.findByRole("list", { name: "跳到分組" }));
    expect(chips.getByRole("button", { name: /學習單產生/ })).toBeInTheDocument();
    expect(chips.getByRole("button", { name: /單元與課例（舊流程）/ })).toBeInTheDocument();
    const legacy = screen.getByRole("region", { name: "單元與課例（舊流程）" });
    expect(legacy.querySelector("details")).not.toHaveAttribute("open");
    expect(screen.getByRole("region", { name: "學習單產生" }).querySelector("details")).toHaveAttribute("open");
  });
});
