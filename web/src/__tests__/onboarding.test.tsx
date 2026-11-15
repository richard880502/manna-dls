import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { SavePreferencesRequest } from "../api/types";
import { baseRoutes, mockApi, preferences, renderApp, signIn, type MockCall } from "../test/mockApi";

const PREFS = "/api/me/preferences";

const echoPreferences =
  (extra: Parameters<typeof preferences>[1] = {}) =>
  (call: MockCall) => {
    const body = call.body as SavePreferencesRequest;
    return preferences(body.answers, { skipped: body.skipped, source: body.source, ...extra });
  };

describe("備課偏好 is optional", () => {
  it("a teacher who never answered it goes straight to 班級; the page is in the top bar and saves P1–P5", async () => {
    signIn();
    const api = mockApi({ ...baseRoutes({}, null), [`PUT ${PREFS}`]: echoPreferences() });
    const user = userEvent.setup();
    renderApp("/");
    expect(await screen.findByRole("heading", { name: "班級", level: 1 })).toBeInTheDocument();
    expect(screen.queryByText(/初次使用設定/)).not.toBeInTheDocument();
    expect(api.callsTo("PUT", PREFS)).toHaveLength(0);

    await user.click(screen.getByRole("link", { name: /備課偏好/ }));
    expect(await screen.findByText("尚未設定")).toBeInTheDocument();
    await user.click(await screen.findByRole("radio", { name: "3–9 年" }));
    await user.click(screen.getByRole("button", { name: "儲存偏好（產生新版本）" }));
    await waitFor(() => expect(api.callsTo("PUT", PREFS)).toHaveLength(1));
    expect(api.lastBody("PUT", PREFS)).toMatchObject({
      answers: { experience: "3-9", class_profile: "" },
      skipped: false,
      source: "settings",
    });
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
    expect(screen.getByRole("link", { name: /備課偏好/ })).toBeInTheDocument();
    expect(await screen.findByRole("radio", { name: "10 年以上" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "找合適素材" })).toBeChecked();
    // 班級特性 is written on the class page only: P6 is not asked here, and an earlier answer is kept.
    expect(screen.queryByLabelText(/我的班級特性/)).not.toBeInTheDocument();
    expect(screen.queryByText("P6")).not.toBeInTheDocument();

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
    expect(await screen.findByText(/已儲存.*第 4 版/)).toBeInTheDocument();
    expect(screen.getByText(/目前第 4 版/)).toBeInTheDocument();
  });
});
