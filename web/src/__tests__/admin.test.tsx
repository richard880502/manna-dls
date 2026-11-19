import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { api as client } from "../api/client";
import { logScaffold } from "../lib/scaffold";
import type { Account } from "../api/types";
import { baseRoutes, mockApi, renderApp, respond, signIn, teacher, type MockCall } from "../test/mockApi";

const pi = { ...teacher, research_id: "PI-01", role: "pi" as const };

const accounts: Account[] = [
  { id: 1, research_id: "PI-01", school_track: "general", role: "pi", created_at: "2026-11-01T08:00:00Z", is_self: true },
  {
    id: 7,
    research_id: "T-2026-007",
    school_track: "vocational",
    role: "teacher",
    created_at: "2026-11-12T08:00:00Z",
    is_self: false,
  },
];

describe("admin page (pi)", () => {
  it("creates invite codes, saves a new prompt version and uploads CSV as text/csv", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      "GET /api/admin/settings": { route_choice_enabled: true },
      "GET /api/admin/accounts": accounts,
      "GET /api/admin/prompts": {
        groups: [],
        registry: [],
        try_keys: [],
        prompts: [
          {
            id: 1,
            prompt_key: "core_assist",
            version_no: 2,
            system_text: "你是 Manna",
            user_template: "{{question}}",
            created_at: "2026-11-11T08:00:00Z",
          },
        ],
      },
      "POST /api/admin/prompts": (call: MockCall) => ({
        id: 2,
        version_no: 3,
        created_at: "2026-11-12T08:00:00Z",
        ...(call.body as object),
      }),
      "POST /api/admin/invites": { code: "ABCD-EFGH" },
      "POST /api/admin/teacher-cases/import": { imported: 2, sha256: "deadbeef" },
    });
    const user = userEvent.setup();
    renderApp("/admin");

    expect(await screen.findByRole("heading", { name: "後台管理", level: 1 })).toBeInTheDocument();
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

    await user.click(await screen.findByText("core_assist", { selector: "summary code" }));
    const system = await screen.findByLabelText(/系統提示詞/);
    await user.type(system, "，不提供答案");
    await user.click(screen.getByRole("button", { name: "儲存為新版本" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/admin/prompts")).toEqual({
        prompt_key: "core_assist",
        system_text: "你是 Manna，不提供答案",
        user_template: "{{question}}",
      }),
    );
    expect(await screen.findByText(/已儲存為 v3/)).toBeInTheDocument();

    const csv = "case_code,school_track,grade,topic,summary,details,approved\nC-1,both,高二,閱讀,摘要,細節,true\n";
    const cases = within(screen.getByRole("region", { name: "匯入教師備課經驗" }));
    await user.upload(cases.getByLabelText("選擇 CSV 檔案"), new File([csv], "cases.csv", { type: "text/csv" }));
    await user.click(cases.getByRole("button", { name: "匯入" }));
    expect(await cases.findByText(/已匯入 2 筆/)).toBeInTheDocument();
    const upload = api.callsTo("POST", "/api/admin/teacher-cases/import")[0];
    expect(upload?.headers["Content-Type"]).toBe("text/csv");
    expect(upload?.rawBody).toBe(csv);

    // the knowledge base has its own page; the PI reaches it from the bar and from here
    expect(screen.queryByRole("region", { name: "匯入知識庫條目" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("link", { name: "知識庫" }).length).toBeGreaterThan(0);
    expect(screen.getAllByRole("link", { name: "知識庫" }).map((a) => a.getAttribute("href"))).toContain(
      "/knowledge?tab=curriculum",
    );
  });

  it("lets the PI invite a 知識庫管理員 and a comprehensive-school teacher", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      "GET /api/admin/settings": { route_choice_enabled: true },
      "GET /api/admin/accounts": accounts,
      "GET /api/admin/prompts": { prompts: [], groups: [], registry: [], try_keys: [] },
      "POST /api/admin/invites": { code: "KBKB-1234" },
    });
    const user = userEvent.setup();
    renderApp("/admin");
    await user.type(await screen.findByLabelText("研究編號"), "KB-2026-001");
    await user.selectOptions(screen.getByLabelText("學校類型"), "綜高");
    await user.selectOptions(screen.getByLabelText("角色"), "知識庫管理員");
    await user.click(screen.getByRole("button", { name: "建立邀請碼" }));
    expect(await screen.findByText("KBKB-1234")).toBeInTheDocument();
    expect(api.lastBody("POST", "/api/admin/invites")).toEqual({
      research_id: "KB-2026-001",
      school_track: "comprehensive",
      role: "kb_manager",
    });
  });

  it("toggles route_choice_enabled (自選備課菜單) for the whole platform", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      "GET /api/admin/settings": { route_choice_enabled: true },
      "GET /api/admin/accounts": accounts,
      "PUT /api/admin/settings": (call: MockCall) => call.body,
      "GET /api/admin/prompts": { prompts: [], groups: [], registry: [], try_keys: [] },
    });
    const user = userEvent.setup();
    renderApp("/admin");

    const toggle = await screen.findByRole("checkbox", { name: /允許教師自選備課菜單/ });
    expect(toggle).toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(api.lastBody("PUT", "/api/admin/settings")).toEqual({ route_choice_enabled: false }));
    expect(await screen.findByText(/已關閉：所有單元改用固定的「目標優先」順序/)).toBeInTheDocument();
    expect(toggle).not.toBeChecked();
  });

  it("hides the admin page from teachers", async () => {
    signIn();
    const api = mockApi(baseRoutes());
    renderApp("/admin");
    expect(await screen.findByText("這個頁面只開放給計畫主持人。")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /後台管理/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "帳號與角色" })).not.toBeInTheDocument();
    expect(api.callsTo("GET", "/api/admin/accounts")).toHaveLength(0);
  });

  it("hides 帳號與角色 from a 知識庫管理員", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": { ...teacher, role: "kb_manager", capabilities: ["manage_knowledge", "use_materials"] },
    });
    renderApp("/admin");
    expect(await screen.findByText("這個頁面只開放給計畫主持人。")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "帳號與角色" })).not.toBeInTheDocument();
    expect(api.callsTo("GET", "/api/admin/accounts")).toHaveLength(0);
  });
});

describe("帳號與角色 (pi)", () => {
  const routes = (patch: (call: MockCall) => unknown) => ({
    ...baseRoutes(),
    "GET /api/me": pi,
    "GET /api/admin/settings": { route_choice_enabled: true },
    "GET /api/admin/prompts": { prompts: [], groups: [], registry: [], try_keys: [] },
    "GET /api/admin/accounts": accounts,
    "PATCH /api/admin/accounts/7": patch,
  });

  it("lists accounts, keeps the PI's own row read-only and explains 知識庫管理員", async () => {
    signIn();
    mockApi(routes(() => respond(500, null)));
    renderApp("/admin");
    const section = within(await screen.findByRole("region", { name: "帳號與角色" }));
    expect(await section.findByText("T-2026-007")).toBeInTheDocument();
    expect(section.getByText("2 個帳號")).toBeInTheDocument();
    expect(
      section.getByText(
        "「知識庫管理員」可以管理知識庫的教材、學習單範本、課綱條目與詞彙，但不能建立邀請碼或變更平台設定，也看不到其他教師的資料。",
      ),
    ).toBeInTheDocument();

    const rows = section.getAllByRole("listitem");
    const own = within(rows[0]!);
    expect(own.getByRole("heading", { name: /PI-01/ })).toBeInTheDocument();
    expect(own.getByText("你")).toBeInTheDocument();
    expect(own.getByText("主持人")).toBeInTheDocument();
    expect(own.queryByRole("combobox")).not.toBeInTheDocument();
    expect(own.getByText(/普高 · 2026\/11\/01 加入/)).toBeInTheDocument();

    const other = within(rows[1]!);
    expect(other.queryByText("你")).not.toBeInTheDocument();
    expect(other.getByText(/技高 · 2026\/11\/12 加入/)).toBeInTheDocument();
    const select = other.getByRole("combobox", { name: "T-2026-007 的角色" });
    expect(select).toHaveValue("teacher");
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "教師",
      "主持人",
      "知識庫管理員",
      "研究助理",
      "評分者",
      "工程師",
    ]);
  });

  it("asks before changing a role, then saves it", async () => {
    signIn();
    const api = mockApi(
      routes((call) => ({ ...accounts[1], ...(call.body as { role: string }) })),
    );
    const user = userEvent.setup();
    renderApp("/admin");
    const section = within(await screen.findByRole("region", { name: "帳號與角色" }));
    const select = await section.findByRole("combobox", { name: "T-2026-007 的角色" });

    // Choosing a role only asks; cancelling puts the old role back and sends nothing.
    await user.selectOptions(select, "知識庫管理員");
    let dialog = section.getByRole("alertdialog", { name: "確認變更 T-2026-007 的角色" });
    expect(dialog).toHaveTextContent("把 T-2026-007 的角色從「教師」改為「知識庫管理員」？");
    expect(dialog).toHaveTextContent("可以管理知識庫的教材、學習單範本、課綱條目與詞彙");
    expect(select).toHaveValue("kb_manager");
    await user.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(section.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(select).toHaveValue("teacher");
    expect(api.callsTo("PATCH", "/api/admin/accounts/7")).toHaveLength(0);

    await user.selectOptions(select, "知識庫管理員");
    dialog = section.getByRole("alertdialog", { name: "確認變更 T-2026-007 的角色" });
    await user.click(within(dialog).getByRole("button", { name: "確定變更" }));
    expect(
      await section.findByText("已將 T-2026-007 的角色改為「知識庫管理員」。對方不必重新登入，重新整理網頁後畫面就會更新。"),
    ).toBeInTheDocument();
    expect(api.callsTo("PATCH", "/api/admin/accounts/7")).toHaveLength(1);
    expect(api.lastBody("PATCH", "/api/admin/accounts/7")).toEqual({ role: "kb_manager" });
    expect(section.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(select).toHaveValue("kb_manager");
  });

  it("shows the server's refusal and keeps the old role", async () => {
    signIn();
    // e.g. another PI took this account's PI role away a moment ago
    mockApi(routes(() => respond(403, { error: { code: "forbidden", message: "只有計畫主持人可以使用這個功能。" } })));
    const user = userEvent.setup();
    renderApp("/admin");
    const section = within(await screen.findByRole("region", { name: "帳號與角色" }));
    const select = await section.findByRole("combobox", { name: "T-2026-007 的角色" });
    await user.selectOptions(select, "主持人");
    const dialog = section.getByRole("alertdialog");
    expect(dialog).toHaveTextContent("主持人可以使用全部後台功能");
    await user.click(within(dialog).getByRole("button", { name: "確定變更" }));
    expect(await section.findByRole("alert")).toHaveTextContent("只有計畫主持人可以使用這個功能。");
    expect(select).toHaveValue("teacher");
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
