import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { AiModelSettings } from "../components/admin/AiModelSettings";
import { baseRoutes, mockApi, renderApp, respond, signIn, teacher } from "../test/mockApi";
import type { AiModelSettings as Settings } from "../api/types";

const envValue = { provider: "openai_compatible", model: "gemma-4-31B-it", effort: "medium", timeout_secs: 120 };

const base: Settings = {
  active: envValue,
  source: "env",
  changed_by: null,
  changed_at: null,
  env_default: envValue,
  providers: [
    { id: "openai", label: "OpenAI（海外）", available: false, models: ["gpt-5.6-sol", "gpt-5.6-terra"], uses_effort: true },
    { id: "openai_compatible", label: "國網中心（NCHC）", available: true, models: ["gemma-4-31B-it"], uses_effort: false },
  ],
  efforts: ["low", "medium", "high", "xhigh", "max"],
  timeout_range: { min: 30, max: 900 },
};

describe("文字模型 settings", () => {
  it("shows the environment default and disables a provider that has no key", async () => {
    mockApi({ "GET /api/admin/ai-model": base });
    render(<AiModelSettings />);
    expect(await screen.findByText("環境變數預設")).toBeInTheDocument();
    expect(screen.getByText(/國網中心（NCHC）・gemma-4-31B-it・等待上限 120 秒/)).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /OpenAI（海外）/ })).toBeDisabled();
    expect(screen.getByRole("option", { name: "國網中心（NCHC）" })).toBeEnabled();
    expect(screen.getByText(/已經在跑的工作（例如正在產生的學習單）會用開始時的模型跑完/)).toBeInTheDocument();
    // No effort for the NCHC model; no reset while the default is in use.
    expect(screen.queryByLabelText("推理強度")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "恢復環境變數預設" })).not.toBeInTheDocument();
  });

  it("tests and applies a typed model name, then shows it as a 後台設定", async () => {
    const saved: Settings = {
      ...base,
      active: { provider: "openai_compatible", model: "my-model", effort: "medium", timeout_secs: 240 },
      source: "admin",
      changed_by: "PI-1",
      changed_at: "2026-11-20T08:30:00Z",
      test_latency_ms: 2400,
    };
    const api = mockApi({
      "GET /api/admin/ai-model": base,
      "PUT /api/admin/ai-model": saved,
    });
    const user = userEvent.setup();
    render(<AiModelSettings />);
    await screen.findByText("環境變數預設");
    await user.selectOptions(screen.getByLabelText("模型"), "其他（自行輸入）");
    await user.type(screen.getByLabelText("模型名稱"), "my-model");
    await user.clear(screen.getByLabelText("等待上限（秒）"));
    await user.type(screen.getByLabelText("等待上限（秒）"), "240");
    await user.click(screen.getByRole("button", { name: "測試並套用" }));
    await waitFor(() =>
      expect(api.lastBody("PUT", "/api/admin/ai-model")).toEqual({
        provider: "openai_compatible",
        model: "my-model",
        timeout_secs: 240,
      }),
    );
    expect(await screen.findByText(/測試成功（約 2.4 秒），已套用/)).toBeInTheDocument();
    expect(screen.getByText("後台設定")).toBeInTheDocument();
    expect(screen.getByText(/由 PI-1 於 .* 設定/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "恢復環境變數預設" })).toBeInTheDocument();
  });

  it("shows the server's reason when the test call fails and keeps the old model", async () => {
    mockApi({
      "GET /api/admin/ai-model": base,
      "PUT /api/admin/ai-model": respond(400, {
        error: { code: "bad_request", message: "測試呼叫沒有通過，設定沒有儲存。原因：請檢查模型名稱有沒有打錯。" },
      }),
    });
    const user = userEvent.setup();
    render(<AiModelSettings />);
    await screen.findByText("環境變數預設");
    await user.click(screen.getByRole("button", { name: "測試並套用" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("請檢查模型名稱有沒有打錯");
    expect(screen.getByText("環境變數預設")).toBeInTheDocument();
    expect(screen.queryByText(/已套用/)).not.toBeInTheDocument();
  });

  it("sends the effort for OpenAI and resets to the default after a confirmation", async () => {
    const overridden: Settings = {
      ...base,
      providers: base.providers.map((p) => ({ ...p, available: true })),
      active: { provider: "openai", model: "gpt-5.6-sol", effort: "high", timeout_secs: 300 },
      source: "admin",
      changed_by: "PI-1",
      changed_at: "2026-11-20T08:30:00Z",
    };
    const api = mockApi({
      "GET /api/admin/ai-model": overridden,
      "PUT /api/admin/ai-model": overridden,
      "DELETE /api/admin/ai-model": { ...overridden, active: envValue, source: "env", changed_by: null, changed_at: null },
    });
    const user = userEvent.setup();
    render(<AiModelSettings />);
    expect(await screen.findByText("後台設定")).toBeInTheDocument();
    expect(screen.getByLabelText("推理強度")).toHaveValue("high");
    await user.selectOptions(screen.getByLabelText("推理強度"), "low");
    await user.click(screen.getByRole("button", { name: "測試並套用" }));
    await waitFor(() =>
      expect(api.lastBody("PUT", "/api/admin/ai-model")).toEqual({
        provider: "openai",
        model: "gpt-5.6-sol",
        effort: "low",
        timeout_secs: 300,
      }),
    );
    await user.click(screen.getByRole("button", { name: "恢復環境變數預設" }));
    expect(api.callsTo("DELETE", "/api/admin/ai-model")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "確定恢復" }));
    expect(await screen.findByText("已恢復成環境變數的預設模型。")).toBeInTheDocument();
    expect(screen.getByText("環境變數預設")).toBeInTheDocument();
  });

  it("will not send a timeout outside the allowed range", async () => {
    const api = mockApi({ "GET /api/admin/ai-model": base });
    const user = userEvent.setup();
    render(<AiModelSettings />);
    await screen.findByText("環境變數預設");
    await user.clear(screen.getByLabelText("等待上限（秒）"));
    await user.type(screen.getByLabelText("等待上限（秒）"), "5");
    expect(screen.getByRole("button", { name: "測試並套用" })).toBeDisabled();
    expect(api.callsTo("PUT", "/api/admin/ai-model")).toHaveLength(0);
  });

  it("refreshes memory readiness after applying and resetting the model on the admin page", async () => {
    signIn();
    let ready = false;
    const saved: Settings = { ...base, source: "admin" };
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": { ...teacher, role: "pi" },
      "GET /api/admin/ai-model": base,
      "PUT /api/admin/ai-model": () => { ready = true; return saved; },
      "DELETE /api/admin/ai-model": () => { ready = false; return base; },
      "GET /api/admin/memory-settings": { enabled: true, graph_enabled: true },
      "GET /api/admin/memory-index": () => ({ pending: 1, failed: 0, providers_ready: ready, compatible: true }),
    });
    const user = userEvent.setup();
    renderApp("/admin");
    await screen.findByText(/模型設定可用後會自動接續同步/);
    await user.click(screen.getByRole("button", { name: "測試並套用" }));
    await waitFor(() => expect(api.callsTo("GET", "/api/admin/memory-index")).toHaveLength(2));
    await waitFor(() => expect(screen.queryByText(/模型設定可用後會自動接續同步/)).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "恢復環境變數預設" }));
    await user.click(screen.getByRole("button", { name: "確定恢復" }));
    expect(await screen.findByText(/模型設定可用後會自動接續同步/)).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/admin/memory-index")).toHaveLength(3);
  });

});
