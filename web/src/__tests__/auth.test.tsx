import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TOKEN_STORAGE_KEY } from "../lib/storage";
import { baseRoutes, config, mockApi, renderApp, respond, signIn, teacher } from "../test/mockApi";

describe("login", () => {
  it("logs in with an invite code, stores the token and uses it for later requests", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "POST /api/auth/login": { token: "tok-123", teacher },
      "POST /api/auth/logout": { ok: true },
    });
    const user = userEvent.setup();
    renderApp("/login");

    await user.type(screen.getByLabelText("邀請碼"), "  INVITE-42 ");
    await user.click(screen.getByRole("button", { name: "登入" }));

    expect(await screen.findByRole("heading", { name: "教師工作檯" })).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/auth/login")[0]?.body).toEqual({ code: "INVITE-42" });
    expect(window.localStorage.getItem(TOKEN_STORAGE_KEY)).toBe("tok-123");

    await waitFor(() => expect(api.callsTo("GET", "/api/units").length).toBeGreaterThan(0));
    expect(api.callsTo("GET", "/api/units")[0]?.headers.Authorization).toBe("Bearer tok-123");
    expect(await screen.findByText(/目前有 1 個單元設計/)).toBeInTheDocument();

    // logout clears the token and returns to the login page
    await user.click(screen.getByRole("button", { name: "登出" }));
    expect(await screen.findByLabelText("邀請碼")).toBeInTheDocument();
    expect(window.localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
    expect(api.callsTo("POST", "/api/auth/logout")).toHaveLength(1);
  });

  it("shows the server's Chinese error for a bad code and stores nothing", async () => {
    mockApi({
      "GET /api/config": config(),
      "POST /api/auth/login": respond(401, { error: { code: "unauthorized", message: "邀請碼無效或已停用" } }),
    });
    const user = userEvent.setup();
    renderApp("/login");

    await user.type(screen.getByLabelText("邀請碼"), "WRONG");
    await user.click(screen.getByRole("button", { name: "登入" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("邀請碼無效或已停用");
    expect(window.localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
  });

  it("redirects to login without a token, and drops an expired token", async () => {
    mockApi({ "GET /api/config": config(), "GET /api/me": respond(401, { error: { code: "unauthorized", message: "請重新登入" } }) });
    renderApp("/");
    expect(await screen.findByLabelText("邀請碼")).toBeInTheDocument();

    signIn("expired");
    renderApp("/");
    expect(await screen.findByText("登入已過期，請重新登入。")).toBeInTheDocument();
    expect(window.localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
  });
});

describe("demo banner", () => {
  it("shows the demo-environment warning when demo_mode is true", async () => {
    mockApi({ "GET /api/config": config({ demo_mode: true }) });
    renderApp("/login");
    expect(await screen.findByText("示範環境：請勿輸入真實教師或學生資料")).toBeInTheDocument();
  });

  it("stays on every page after login", async () => {
    signIn();
    mockApi(baseRoutes({ demo_mode: true }));
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "教師工作檯" })).toBeInTheDocument();
    expect(await screen.findByText("示範環境：請勿輸入真實教師或學生資料")).toBeInTheDocument();
  });

  it("is hidden when demo_mode is false", async () => {
    const api = mockApi({ "GET /api/config": config({ demo_mode: false }) });
    renderApp("/login");
    await waitFor(() => expect(api.callsTo("GET", "/api/config")).toHaveLength(1));
    await screen.findByLabelText("邀請碼");
    expect(screen.queryByText("示範環境：請勿輸入真實教師或學生資料")).not.toBeInTheDocument();
  });
});
