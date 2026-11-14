import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { api as client } from "../api/client";
import { logScaffold } from "../lib/scaffold";
import { baseRoutes, mockApi, renderApp, signIn, teacher, type MockCall } from "../test/mockApi";

const pi = { ...teacher, research_id: "PI-01", role: "pi" as const };

describe("admin page (pi)", () => {
  it("creates invite codes, saves a new prompt version and uploads CSV as text/csv", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      "GET /api/admin/settings": { route_choice_enabled: true },
      "GET /api/admin/prompts": [
        {
          id: 1,
          prompt_key: "core_assist",
          version_no: 2,
          system_text: "你是 Manna",
          user_template: "{{question}}",
          created_at: "2026-11-11T08:00:00Z",
        },
      ],
      "POST /api/admin/prompts": (call: MockCall) => ({
        id: 2,
        version_no: 3,
        created_at: "2026-11-12T08:00:00Z",
        ...(call.body as object),
      }),
      "POST /api/admin/invites": { code: "ABCD-EFGH" },
      "POST /api/admin/knowledge/import": { imported: 2, sha256: "deadbeef" },
    });
    const user = userEvent.setup();
    renderApp("/admin");

    expect(await screen.findByRole("heading", { name: "後台管理", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /後台管理/ })).toBeInTheDocument();

    await user.type(screen.getByLabelText("研究編號"), "T-2026-009");
    await user.selectOptions(screen.getByLabelText("學校類型"), "vocational");
    await user.click(screen.getByRole("button", { name: "建立邀請碼" }));
    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    expect(api.lastBody("POST", "/api/admin/invites")).toEqual({
      research_id: "T-2026-009",
      school_track: "vocational",
      role: "teacher",
    });

    const system = await screen.findByLabelText("system_text");
    await user.type(system, "，不提供答案");
    await user.click(screen.getByRole("button", { name: "儲存為新版本" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/admin/prompts")).toEqual({
        prompt_key: "core_assist",
        system_text: "你是 Manna，不提供答案",
        user_template: "{{question}}",
      }),
    );
    expect(await screen.findByText(/v3/)).toBeInTheDocument();

    const csv = "item_code,category,school_track,title,content,source_doc,source_version,source_page\nA,b,general,t,c,d,v,1\n";
    const knowledge = within(screen.getByRole("region", { name: "匯入知識庫條目" }));
    await user.upload(knowledge.getByLabelText("選擇 CSV 檔案"), new File([csv], "kb.csv", { type: "text/csv" }));
    await user.click(knowledge.getByRole("button", { name: "匯入" }));
    expect(await knowledge.findByText(/已匯入 2 筆/)).toBeInTheDocument();
    const upload = api.callsTo("POST", "/api/admin/knowledge/import")[0];
    expect(upload?.headers["Content-Type"]).toBe("text/csv");
    expect(upload?.rawBody).toBe(csv);
  });

  it("toggles route_choice_enabled (自選備課菜單) for the whole platform", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      "GET /api/admin/settings": { route_choice_enabled: true },
      "PUT /api/admin/settings": (call: MockCall) => call.body,
      "GET /api/admin/prompts": [],
    });
    const user = userEvent.setup();
    renderApp("/admin");

    const toggle = await screen.findByRole("checkbox", { name: /允許教師自選備課菜單/ });
    expect(toggle).toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(api.lastBody("PUT", "/api/admin/settings")).toEqual({ route_choice_enabled: false }));
    expect(await screen.findByText(/已關閉：所有單元改用固定的「目標優先」順序/)).toBeInTheDocument();
    expect(toggle).not.toBeChecked();

    // the rest of the app follows the new setting without a reload
    await user.click(screen.getByRole("link", { name: "備課庫" }));
    await screen.findByLabelText("建立新單元");
    expect(screen.queryByRole("radio", { name: /教材優先/ })).not.toBeInTheDocument();
  });

  it("hides the admin page from teachers", async () => {
    signIn();
    mockApi(baseRoutes());
    renderApp("/admin");
    expect(await screen.findByText("這個頁面只開放給計畫主持人。")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /後台管理/ })).not.toBeInTheDocument();
  });
});

describe("api client", () => {
  it("logScaffold never throws, even when fetch rejects", async () => {
    const fetchMock = vi.fn(() => Promise.reject(new Error("offline")));
    vi.stubGlobal("fetch", fetchMock);
    expect(() => logScaffold("SC-RAG", "show")).not.toThrow();
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
  });

  it("turns network failures into a friendly Chinese message", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("offline"))));
    await expect(client.listUnits()).rejects.toThrow("無法連線到伺服器，請確認網路後再試一次。");
  });
});
