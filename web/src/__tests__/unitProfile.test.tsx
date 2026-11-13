// 建立新單元: the class profile carries over from the previous unit (docs/UX_REVIEW.md R3, decision D2).
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { ClassProfileDefault, CreateUnitRequest } from "../api/types";
import { baseRoutes, mockApi, preferences, renderApp, respond, signIn, unitWith, type MockCall } from "../test/mockApi";

const DEFAULT = "/api/units/class-profile-default";
const PREVIOUS = "高二 207 普通班 34 人：程度落差大，用計時與角色卡比較順。";

const fromPrevious: ClassProfileDefault = {
  class_profile: PREVIOUS,
  source: "previous_unit",
  source_unit: { id: 1, title: "L3 倒裝句" },
};

function routes(prefill: ClassProfileDefault) {
  const created = unitWith({ id: 2, title: "L4 假設語氣", core_status: "none", lesson_count: 0 });
  return {
    ...baseRoutes({}, preferences({ start_from: "goal", class_profile: "問卷時寫的舊描述" })),
    [`GET ${DEFAULT}`]: prefill,
    "POST /api/units": (call: MockCall) => ({ ...created, ...(call.body as CreateUnitRequest) }),
    "GET /api/units/2": created,
    "GET /api/units/2/lessons": [],
    "GET /api/units/2/materials": [],
  };
}

async function openForm() {
  renderApp("/plan");
  return within(await screen.findByRole("group", { name: "班級特性" }));
}

async function createUnit(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText("建立新單元"), "L4 假設語氣");
  await user.click(screen.getByRole("button", { name: "建立單元" }));
  expect(await screen.findByRole("heading", { name: "L4 假設語氣", level: 2 })).toBeInTheDocument();
}

describe("new unit: class profile prefill (R3)", () => {
  it("shows the previous unit's profile with its source, and leaves it to the server when untouched", async () => {
    signIn();
    const api = mockApi(routes(fromPrevious));
    const user = userEvent.setup();
    const profile = await openForm();

    expect(profile.getByText("沿用上一個單元『L3 倒裝句』的班級特性，可修改")).toBeInTheDocument();
    expect(profile.getByText(PREVIOUS)).toBeInTheDocument();
    // Not the stale survey answer.
    expect(screen.queryByText("問卷時寫的舊描述")).not.toBeInTheDocument();

    await createUnit(user);
    // Untouched: no class_profile, so the server applies the default and records its source.
    expect(api.lastBody("POST", "/api/units")).toEqual({ title: "L4 假設語氣", prep_route: "goal" });
  });

  it("can be edited before creating; the edited text is sent", async () => {
    signIn();
    const api = mockApi(routes(fromPrevious));
    const user = userEvent.setup();
    const profile = await openForm();

    await user.click(profile.getByRole("button", { name: "修改" }));
    const field = screen.getByRole("textbox", { name: "班級特性（選填）" });
    expect(field).toHaveValue(PREVIOUS);
    expect(screen.getByText("沿用上一個單元『L3 倒裝句』的班級特性，可修改")).toBeInTheDocument();
    expect(screen.getByText("請不要輸入學生姓名或其他個人資料。")).toBeInTheDocument();
    await user.clear(field);
    await user.type(field, "  這學期換成 208 班，口說較弱  ");

    await createUnit(user);
    expect(api.lastBody("POST", "/api/units")).toEqual({
      title: "L4 假設語氣",
      prep_route: "goal",
      class_profile: "這學期換成 208 班，口說較弱",
    });
  });

  it("clearing the prefill on purpose sends an empty profile", async () => {
    signIn();
    const api = mockApi(routes(fromPrevious));
    const user = userEvent.setup();
    const profile = await openForm();
    await user.click(profile.getByRole("button", { name: "修改" }));
    await user.clear(screen.getByRole("textbox", { name: "班級特性（選填）" }));
    await createUnit(user);
    expect(api.lastBody("POST", "/api/units")).toEqual({ title: "L4 假設語氣", prep_route: "goal", class_profile: "" });
  });

  it("「還原沿用的內容」 (or the same text) goes back to the default and sends nothing", async () => {
    signIn();
    const api = mockApi(routes(fromPrevious));
    const user = userEvent.setup();
    const profile = await openForm();
    await user.click(profile.getByRole("button", { name: "修改" }));
    await user.type(screen.getByRole("textbox", { name: "班級特性（選填）" }), "加一句");
    await user.click(screen.getByRole("button", { name: "還原沿用的內容" }));
    expect(screen.queryByRole("textbox", { name: "班級特性（選填）" })).not.toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: "班級特性" })).getByText(PREVIOUS)).toBeInTheDocument();

    await createUnit(user);
    expect(api.lastBody("POST", "/api/units")).not.toHaveProperty("class_profile");
  });

  it("the first unit says the profile comes from the preference survey", async () => {
    signIn();
    mockApi(routes({ class_profile: "問卷時寫的舊描述", source: "preference", source_unit: null }));
    const profile = await openForm();
    expect(profile.getByText("來自你的備課偏好，可修改")).toBeInTheDocument();
    expect(profile.getByText("問卷時寫的舊描述")).toBeInTheDocument();
  });

  it("with nothing to carry over, the profile is optional and can be filled in", async () => {
    signIn();
    const api = mockApi(routes({ class_profile: "", source: "none", source_unit: null }));
    const user = userEvent.setup();
    const profile = await openForm();
    expect(profile.getByText("還沒有可以沿用的班級特性，可以留白")).toBeInTheDocument();
    await user.click(profile.getByRole("button", { name: "填寫" }));
    await user.type(screen.getByRole("textbox", { name: "班級特性（選填）" }), "高一普通班");
    expect(screen.queryByRole("button", { name: "還原沿用的內容" })).not.toBeInTheDocument();
    await createUnit(user);
    expect(api.lastBody("POST", "/api/units")).toEqual({
      title: "L4 假設語氣",
      prep_route: "goal",
      class_profile: "高一普通班",
    });
  });

  it("still creates the unit when the prefill cannot be loaded (the server applies the same rule)", async () => {
    signIn();
    const api = mockApi({
      ...routes(fromPrevious),
      [`GET ${DEFAULT}`]: respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } }),
    });
    const user = userEvent.setup();
    renderApp("/plan");
    await screen.findByLabelText("建立新單元");
    await waitFor(() => expect(api.callsTo("GET", DEFAULT)).toHaveLength(1));
    expect(screen.queryByRole("group", { name: "班級特性" })).not.toBeInTheDocument();
    await createUnit(user);
    expect(api.lastBody("POST", "/api/units")).toEqual({ title: "L4 假設語氣", prep_route: "goal" });
  });
});
