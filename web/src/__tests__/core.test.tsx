import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { CoreAssistRequest, CoreDraftRequest } from "../api/types";
import {
  baseRoutes,
  coreVersion,
  focus,
  mockApi,
  renderApp,
  signIn,
  type MockCall,
} from "../test/mockApi";

const CORE = "/api/units/1/core";
const DRAFT = "/api/units/1/core/draft";

function coreRoutes(extra: Record<string, unknown> = {}, cfg = {}) {
  const version = coreVersion([focus(11, 1, "倒裝句的語意功能")]);
  return {
    ...baseRoutes(cfg),
    [`GET ${CORE}`]: version,
    [`PUT ${DRAFT}`]: (call: MockCall) => {
      const body = call.body as CoreDraftRequest;
      return coreVersion(
        body.focuses.map((f, i) => focus(f.id ?? 900 + i, f.position, f.big_idea)),
      );
    },
    ...extra,
  };
}

async function cell(name: RegExp) {
  return within(await screen.findByRole("group", { name }));
}

describe("CoRe page", () => {
  it("shows the pre-question and short prompts only (help text hidden until 需要協助)", async () => {
    signIn();
    mockApi(coreRoutes());
    renderApp("/units/1/core");
    expect(await screen.findByText(/你最希望學生真正理解、能運用或能表達的 2–3 個重要學習重點/)).toBeInTheDocument();
    expect(await screen.findByRole("group", { name: /你希望學生學會什麼？/ })).toBeInTheDocument();
    expect(screen.queryByText("學生最後要真正學會、能運用或能表達什麼")).not.toBeInTheDocument();
  });

  it("「目前沒有想法」 clears the answer, sends status no_idea, and restores the text when cancelled", async () => {
    signIn();
    const api = mockApi(coreRoutes());
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const q1 = await cell(/你希望學生學會什麼？/);
    const textarea = q1.getByRole("textbox");
    await user.type(textarea, "Students can explain inversion.");
    await user.click(q1.getByRole("checkbox", { name: "目前沒有想法" }));

    expect(textarea).toHaveValue("");
    expect(textarea).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => expect(api.callsTo("PUT", DRAFT).length).toBeGreaterThan(0));
    let body = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
    expect(body.focuses).toHaveLength(1);
    expect(body.focuses[0]?.id).toBe(11);
    expect(body.focuses[0]?.responses.Q1).toEqual({ answer: "", status: "no_idea", content_source: "self" });
    expect(JSON.stringify(body)).not.toContain("Students can explain inversion.");

    // cancel: the text comes back and is sent as filled
    await user.click(q1.getByRole("checkbox", { name: "目前沒有想法" }));
    expect(textarea).toHaveValue("Students can explain inversion.");
    expect(textarea).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => {
      body = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
      expect(body.focuses[0]?.responses.Q1).toEqual({
        answer: "Students can explain inversion.",
        status: "filled",
        content_source: "self",
      });
    });
    expect(body.focuses[0]?.responses.Q2).toEqual({ answer: "", status: "blank", content_source: "self" });
  });

  it("layered help: level 1 first, level 2 only when the teacher continues; AI text never enters the answer", async () => {
    signIn();
    const api = mockApi(
      coreRoutes({
        "POST /api/core/assist": (call: MockCall) => {
          const req = call.body as CoreAssistRequest;
          return {
            interaction_id: 100 + req.level,
            status: "shown",
            message: null,
            output: { level: req.level, text: `AI 提示第${req.level}層：想想學生的真實溝通情境` },
            sources: [],
          };
        },
        "POST /api/ai/101/decision": { ok: true },
        "POST /api/ai/102/decision": { ok: true },
      }),
    );
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const q2 = await cell(/為什麼要學這個重點？/);
    const textarea = q2.getByRole("textbox");
    await user.type(textarea, "我自己的想法");
    expect(q2.queryByRole("combobox", { name: "內容來源" })).not.toBeInTheDocument();

    await user.click(q2.getByRole("button", { name: "需要協助" }));
    const panel = await screen.findByRole("complementary", { name: "需要協助" });
    expect(await within(panel).findByText("AI 提示第1層：想想學生的真實溝通情境")).toBeInTheDocument();
    // help text (第一層題意說明) appears only now
    expect(within(panel).getByText("與本單元、後續學習或真實溝通有什麼關係")).toBeInTheDocument();

    expect(api.callsTo("POST", "/api/core/assist").map((c) => c.body)).toEqual([
      { unit_id: 1, focus_id: 11, question_id: "Q2", level: 1 },
    ]);
    expect(textarea).toHaveValue("我自己的想法");
    for (const box of screen.getAllByRole("textbox")) {
      expect((box as HTMLTextAreaElement).value).not.toContain("AI 提示");
    }

    // rating + 回答 → decision endpoint
    await user.click(within(panel).getByRole("button", { name: "有幫助" }));
    await user.click(within(panel).getByRole("button", { name: "回答" }));
    await waitFor(() =>
      expect(api.callsTo("POST", "/api/ai/101/decision").map((c) => c.body)).toEqual([
        { decision: "answer", thumbs: "up" },
      ]),
    );
    // content source selector appears once AI help was seen
    expect(q2.getByRole("combobox", { name: "內容來源" })).toHaveValue("self");

    // still no level 2 until the teacher asks
    expect(api.callsTo("POST", "/api/core/assist")).toHaveLength(1);

    await user.click(within(panel).getByRole("button", { name: "繼續：提供思考面向" }));
    expect(await within(panel).findByText("AI 提示第2層：想想學生的真實溝通情境")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/core/assist").map((c) => (c.body as CoreAssistRequest).level)).toEqual([1, 2]);
    expect(textarea).toHaveValue("我自己的想法");

    await user.click(within(panel).getAllByRole("button", { name: "略過" })[1]!);
    await waitFor(() =>
      expect(api.callsTo("POST", "/api/ai/102/decision").map((c) => c.body)).toEqual([{ decision: "skip" }]),
    );

    // the teacher records where the content came from
    await user.selectOptions(q2.getByRole("combobox", { name: "內容來源" }), "self_after_ai");
    await user.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => {
      const body = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
      expect(body.focuses[0]?.responses.Q2).toEqual({
        answer: "我自己的想法",
        status: "filled",
        content_source: "self_after_ai",
      });
    });

    const events = api.callsTo("POST", "/api/scaffold-events").map((c) => c.body as { scaffold_code: string; action: string });
    expect(events).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ scaffold_code: "SC-CORE-HELP", action: "expand" }),
        expect.objectContaining({ scaffold_code: "SC-CORE-HELP", action: "show" }),
        expect.objectContaining({ scaffold_code: "SC-CORE-HELP", action: "use" }),
        expect.objectContaining({ scaffold_code: "SC-CORE-HELP", action: "skip" }),
      ]),
    );
  });

  it("「完成單元想法」 works with every answer blank", async () => {
    signIn();
    const api = mockApi(
      coreRoutes({
        "POST /api/units/1/core/complete": coreVersion([focus(11, 1, "")], {
          completed_at: "2026-11-12T09:30:00Z",
        }),
      }),
    );
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const button = await screen.findByRole("button", { name: "完成單元想法" });
    expect(button).toBeEnabled();
    await user.click(button);

    expect(await screen.findByText(/已完成單元想法/)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/units/1/core/complete")).toHaveLength(1);
    const body = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
    expect(Object.values(body.focuses[0]!.responses).every((r) => r?.status === "blank")).toBe(true);
  });

  it("renders the message for blocked AI results and offers no answer/skip on them", async () => {
    signIn();
    mockApi(
      coreRoutes({
        "POST /api/core/assist": {
          interaction_id: 9,
          status: "blocked",
          message: "目前暫無建議，你可以繼續手動完成。",
          output: null,
          sources: [],
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const q4 = await cell(/這個重點可能哪裡難教？/);
    await user.click(q4.getByRole("button", { name: "需要協助" }));
    const panel = await screen.findByRole("complementary", { name: "需要協助" });
    expect(await within(panel).findByText("目前暫無建議，你可以繼續手動完成。")).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "回答" })).not.toBeInTheDocument();
    expect(q4.queryByRole("combobox", { name: "內容來源" })).not.toBeInTheDocument();
  });

  it("renders a friendly message for a disabled AI result even without server text", async () => {
    signIn();
    mockApi(
      coreRoutes({
        "POST /api/core/assist": { interaction_id: null, status: "disabled", message: null, output: null, sources: [] },
      }),
    );
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const q1 = await cell(/你希望學生學會什麼？/);
    await user.click(q1.getByRole("button", { name: "需要協助" }));
    expect(await screen.findByText("AI 功能目前未開放，你可以繼續手動完成。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /繼續：/ })).not.toBeInTheDocument();
  });

  it("when ai_enabled is false, 需要協助 shows the question help without calling the AI", async () => {
    signIn();
    const api = mockApi(coreRoutes({}, { ai_enabled: false }));
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const q1 = await cell(/你希望學生學會什麼？/);
    await waitFor(() => expect(api.callsTo("GET", "/api/config")).toHaveLength(1));
    await user.click(q1.getByRole("button", { name: "需要協助" }));
    const panel = await screen.findByRole("complementary", { name: "需要協助" });
    expect(within(panel).getByText("學生最後要真正學會、能運用或能表達什麼")).toBeInTheDocument();
    expect(within(panel).getByText(/AI 功能目前未開放/)).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: /請 Manna/ })).not.toBeInTheDocument();
    expect(api.callsTo("POST", "/api/core/assist")).toHaveLength(0);
  });

  it("adds and removes focuses (max 3); a removed focus is left out of the draft", async () => {
    signIn();
    const api = mockApi(coreRoutes());
    const user = userEvent.setup();
    renderApp("/units/1/core");

    const add = await screen.findByRole("button", { name: "＋ 新增學習重點" });
    await user.click(add);
    await user.click(add);
    expect(screen.getAllByRole("tab")).toHaveLength(3);
    expect(add).toBeDisabled();

    await user.type(screen.getByLabelText("學習重點 3"), "第三個重點");
    await user.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => {
      const body = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
      expect(body.focuses.map((f) => [f.id, f.position, f.big_idea])).toEqual([
        [11, 1, "倒裝句的語意功能"],
        [undefined, 2, ""],
        [undefined, 3, "第三個重點"],
      ]);
    });

    await user.click(screen.getByRole("tab", { name: /學習重點 1/ }));
    await user.click(screen.getByRole("button", { name: "移除此重點" }));
    await user.click(screen.getByRole("button", { name: "確定移除" }));
    await user.click(screen.getByRole("button", { name: "儲存草稿" }));
    await waitFor(() => {
      const body = api.lastBody("PUT", DRAFT) as CoreDraftRequest;
      // ids assigned by the previous save are reused; focus 11 is dropped
      expect(body.focuses.map((f) => [f.id, f.position, f.big_idea])).toEqual([
        [901, 1, ""],
        [902, 2, "第三個重點"],
      ]);
    });
  });

  it("scaffold logging failures never break the page", async () => {
    signIn();
    const api = mockApi(
      coreRoutes({
        "POST /api/scaffold-events": () => Promise.reject(new Error("offline")),
        "POST /api/core/assist": {
          interaction_id: 3,
          status: "shown",
          message: null,
          output: { level: 1, text: "換個角度想想" },
          sources: [],
        },
      }),
    );
    const user = userEvent.setup();
    renderApp("/units/1/core");
    const q1 = await cell(/你希望學生學會什麼？/);
    await user.click(q1.getByRole("button", { name: "需要協助" }));
    expect(await screen.findByText("換個角度想想")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/scaffold-events").length).toBeGreaterThan(0);
  });
});
