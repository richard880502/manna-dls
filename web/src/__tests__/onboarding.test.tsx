import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { SavePreferencesRequest } from "../api/types";
import { baseRoutes, mockApi, preferences, renderApp, respond, signIn, type MockCall } from "../test/mockApi";

const PREFS = "/api/me/preferences";

const echoPreferences =
  (extra: Parameters<typeof preferences>[1] = {}) =>
  (call: MockCall) => {
    const body = call.body as SavePreferencesRequest;
    return preferences(body.answers, { skipped: body.skipped, source: body.source, ...extra });
  };

describe("first-use survey (初次使用設定)", () => {
  it("shows the wizard when preferences are null and saves every answer with source onboarding", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes({}, null),
      [`PUT ${PREFS}`]: echoPreferences({ version_no: 1 }),
    });
    const user = userEvent.setup();
    renderApp("/");

    expect(await screen.findByRole("heading", { name: "先認識你的備課習慣" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "初次使用設定", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "教師工作檯" })).not.toBeInTheDocument();
    expect(screen.getByText(/第 1 \/ 6 題/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "上一步" })).toBeDisabled();

    await user.click(screen.getByRole("radio", { name: "3–9 年" }));
    await user.click(screen.getByRole("button", { name: "下一步" }));
    await user.click(screen.getByRole("radio", { name: "課文或教材" }));
    await user.click(screen.getByRole("button", { name: "下一步" }));
    await user.click(screen.getByRole("checkbox", { name: "差異化與時間安排" }));
    await user.click(screen.getByRole("checkbox", { name: "跟 AI 溝通" }));
    await user.click(screen.getByRole("button", { name: "下一步" }));
    await user.click(screen.getByRole("radio", { name: "偶爾" }));
    await user.click(screen.getByRole("button", { name: "下一步" }));
    await user.click(screen.getByRole("radio", { name: "精簡" }));
    // going back keeps the answer
    await user.click(screen.getByRole("button", { name: "上一步" }));
    expect(screen.getByRole("radio", { name: "偶爾" })).toBeChecked();
    await user.click(screen.getByRole("button", { name: "下一步" }));
    await user.click(screen.getByRole("button", { name: "下一步" }));

    // P6: free text; the privacy hint comes with the server prompt and is not repeated
    expect(screen.getAllByText(/不要寫學生姓名或個人資料/)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "略過" })).toBeInTheDocument();
    await user.type(screen.getByLabelText(/我的班級特性/), "高一普通班，口說較害羞");
    await user.click(screen.getByRole("button", { name: "完成設定" }));

    await waitFor(() => expect(api.callsTo("PUT", PREFS)).toHaveLength(1));
    expect(api.lastBody("PUT", PREFS)).toEqual({
      answers: {
        experience: "3-9",
        start_from: "material",
        hardest: ["differentiation", "ai_communication"],
        ai_experience: "sometimes",
        detail: "concise",
        class_profile: "高一普通班，口說較害羞",
      },
      skipped: false,
      source: "onboarding",
    });
    expect(await screen.findByRole("heading", { name: "教師工作檯" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "先認識你的備課習慣" })).not.toBeInTheDocument();
  });

  it("「略過」 sends skipped: true (with whatever was answered) and opens the platform", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes({}, null),
      [`PUT ${PREFS}`]: echoPreferences(),
    });
    const user = userEvent.setup();
    renderApp("/");

    await screen.findByRole("heading", { name: "先認識你的備課習慣" });
    await user.click(screen.getByRole("radio", { name: "0–2 年" }));
    await user.click(screen.getByRole("button", { name: "下一步" }));
    await user.click(screen.getByRole("button", { name: "略過" }));

    await waitFor(() => expect(api.callsTo("PUT", PREFS)).toHaveLength(1));
    expect(api.lastBody("PUT", PREFS)).toEqual({
      answers: {
        experience: "0-2",
        start_from: null,
        hardest: [],
        ai_experience: null,
        detail: null,
        class_profile: "",
      },
      skipped: true,
      source: "onboarding",
    });
    expect(await screen.findByRole("heading", { name: "教師工作檯" })).toBeInTheDocument();
  });

  it("uses the built-in questions (with the P6 privacy hint) when the question endpoint fails", async () => {
    signIn();
    mockApi({
      ...baseRoutes({}, null),
      "GET /api/preferences/questions": respond(500, { error: { code: "internal", message: "x" } }),
    });
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: "先認識你的備課習慣" });
    for (let i = 0; i < 5; i += 1) await user.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByLabelText(/我的班級特性/)).toBeInTheDocument();
    expect(screen.getByText(/^不要寫學生姓名或個人資料。/)).toBeInTheDocument();
  });

  it("never traps the teacher: a failed skip still opens the platform", async () => {
    signIn();
    mockApi({ ...baseRoutes({}, null) }); // PUT is not defined → 404
    const user = userEvent.setup();
    renderApp("/");
    await screen.findByRole("heading", { name: "先認識你的備課習慣" });
    await user.click(screen.getByRole("button", { name: "略過" }));
    expect(await screen.findByRole("heading", { name: "教師工作檯" })).toBeInTheDocument();
  });

  it("does not show the wizard when preferences already exist (including a skipped version)", async () => {
    signIn();
    mockApi(baseRoutes({}, preferences({}, { skipped: true })));
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "教師工作檯" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "先認識你的備課習慣" })).not.toBeInTheDocument();
  });
});

describe("我的備課偏好 page", () => {
  it("shows the current version, edits the answers and saves a new version with source settings", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(
        {},
        preferences(
          { experience: "10+", hardest: ["material"], detail: "medium", class_profile: "高二" },
          { version_no: 3, source: "settings" },
        ),
      ),
      [`PUT ${PREFS}`]: echoPreferences({ version_no: 4 }),
    });
    const user = userEvent.setup();
    renderApp("/preferences");

    expect(await screen.findByText(/目前第 3 版/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /我的備課偏好/ })).toBeInTheDocument();
    expect(await screen.findByRole("radio", { name: "10 年以上" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "找合適素材" })).toBeChecked();
    expect(screen.getByLabelText(/我的班級特性/)).toHaveValue("高二");

    await user.click(screen.getByRole("radio", { name: "詳細" }));
    await user.click(screen.getByRole("checkbox", { name: "差異化與時間安排" }));
    await user.click(screen.getByRole("radio", { name: "學習目標" }));
    await user.click(screen.getByRole("button", { name: "儲存偏好（產生新版本）" }));

    await waitFor(() => expect(api.callsTo("PUT", PREFS)).toHaveLength(1));
    expect(api.lastBody("PUT", PREFS)).toEqual({
      answers: {
        experience: "10+",
        start_from: "goal",
        hardest: ["material", "differentiation"],
        ai_experience: null,
        detail: "detailed",
        class_profile: "高二",
      },
      skipped: false,
      source: "settings",
    });
    expect(await screen.findByText(/已儲存為第 4 版/)).toBeInTheDocument();
    expect(screen.getByText(/目前第 4 版/)).toBeInTheDocument();
  });
});
