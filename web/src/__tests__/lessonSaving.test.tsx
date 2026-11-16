// The lesson page never loses unsaved work (docs/UX_REVIEW.md R2): 開始上課 saves the SOP first,
// leaving saves first and only asks when that fails, and the head says how saving went.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { LessonRun, RunDetail, SopContent, SopState } from "../api/types";
import { emptyContent } from "../lib/sop";
import { autosaveConfig } from "../lib/useAutosave";
import { baseRoutes, lessonDetail, mockApi, renderApp, respond, settleAutosave, signIn, type MockCall } from "../test/mockApi";

const SOP = "/api/lessons/7/sop";
const START = "/api/lessons/7/runs";

const run: LessonRun = {
  id: 9,
  lesson_id: 7,
  lesson_title: "第 1 課：Not only… but also",
  unit_id: 1,
  unit_title: "倒裝句單元",
  started_at: "2026-11-12T01:00:00Z",
  ended_at: null,
  review_completed_at: null,
  good_count: 0,
  stuck_count: 0,
  class_id: null,
  class_name: null,
};
const runDetail: RunDetail = { ...run, events: [], plan: null, remotes: [] };

const blankSop: SopState = {
  lesson_id: 7,
  version_no: null,
  versions_count: 0,
  saved_at: null,
  material: null,
  content: emptyContent(),
  today_objective: "",
  jobs: { analysis: null, objectives: null },
};

const savedSop = (call: MockCall) => ({
  version_no: 2,
  versions_count: 2,
  saved_at: "2026-11-14T09:30:00Z",
  content: (call.body as { content: SopContent }).content,
});
const failing = respond(502, { error: { code: "internal", message: "伺服器暫時發生問題" } });

function routes(extra: Record<string, unknown> = {}) {
  return {
    ...baseRoutes(),
    "GET /api/lessons/7": lessonDetail({ latest_version: null, versions_count: 0 }),
    "GET /api/lessons/7/runs": [],
    [`POST ${START}`]: run,
    "GET /api/runs/9": runDetail,
    [`GET ${SOP}`]: blankSop,
    [`PUT ${SOP}`]: savedSop,
    ...extra,
  };
}

async function openLesson() {
  const rendered = renderApp("/lessons/7");
  expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
  return rendered;
}

/** Writes one exit-ticket question (step 7): a small, ordinary SOP edit. */
async function writeExitTicket(user: ReturnType<typeof userEvent.setup>, text: string) {
  await user.click(within(screen.getByRole("navigation", { name: "備課步驟" })).getByRole("button", { name: /Exit ticket/ }));
  await user.click(await screen.findByRole("button", { name: "＋ 學生反思" }));
  await user.type(screen.getByRole("textbox", { name: "題目" }), text);
}

const startButton = () => within(document.querySelector(".lesson-head") as HTMLElement).getByRole("button", { name: "開始上課" });
const navLink = (name: string) => within(screen.getByRole("navigation", { name: "主要導覽" })).getByRole("link", { name });
const savedText = (api: ReturnType<typeof mockApi>) =>
  (api.lastBody("PUT", SOP) as { content: SopContent } | undefined)?.content.exit_ticket.items[0]?.text;
const callIndex = (api: ReturnType<typeof mockApi>, method: string, path: string) =>
  api.calls.findIndex((c) => c.method === method && c.path === path);

describe("開始上課 never drops unsaved changes (R2a, regression for docs/UX_REVIEW.md F2)", () => {
  it("is a secondary button in the head; it saves what is pending first, then starts the run", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    expect(startButton()).toHaveClass("secondary-button");

    await writeExitTicket(user, "What was hard today?");
    await user.click(startButton());

    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(savedText(api)).toBe("What was hard today?");
    expect(callIndex(api, "PUT", SOP)).toBeLessThan(callIndex(api, "POST", START));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("does not start when saving fails, and says why", async () => {
    signIn();
    const api = mockApi(routes({ [`PUT ${SOP}`]: failing }));
    const user = userEvent.setup();
    await openLesson();
    await writeExitTicket(user, "Why?");
    await user.click(startButton());

    expect(await screen.findByText("沒有儲存成功，所以還沒開始上課：伺服器暫時發生問題")).toBeInTheDocument();
    expect(api.callsTo("POST", START)).toHaveLength(0);
    expect(screen.getByRole("textbox", { name: "題目" })).toHaveValue("Why?");
    // The head says so too, with a way to try again.
    expect(screen.getByText(/^沒有存到：伺服器暫時發生問題/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再試一次" })).toBeInTheDocument();
  });

  it("starts without saving when nothing changed", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    await user.click(startButton());
    expect(await screen.findByRole("button", { name: "順利" })).toBeInTheDocument();
    expect(api.callsTo("PUT", SOP)).toHaveLength(0);
    expect(api.callsTo("POST", START)).toHaveLength(1);
  });

  it("with a run in progress in the class, the head button goes back to it", async () => {
    signIn();
    const api = mockApi(routes({ "GET /api/lessons/7/runs": [run] }));
    const user = userEvent.setup();
    const { router } = await openLesson();
    await user.click(await within(document.querySelector(".lesson-head") as HTMLElement).findByRole("button", { name: "回到課堂" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/runs/9"));
    expect(api.callsTo("POST", START)).toHaveLength(0);
  });
});

describe("leaving the lesson page with unsaved changes (R2b)", () => {
  it("saves what is pending first and goes on without asking", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openLesson();
    await writeExitTicket(user, "要存的題目");

    await user.click(navLink("班級"));
    await waitFor(() => expect(router.state.location.pathname).toBe("/classes"));
    expect(savedText(api)).toBe("要存的題目");
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("asks in the page when that save fails: stay keeps the edits, leave drops them", async () => {
    signIn();
    mockApi(routes({ [`PUT ${SOP}`]: failing }));
    const user = userEvent.setup();
    const { router } = await openLesson();
    await writeExitTicket(user, "還沒存的題目");

    await user.click(navLink("班級"));
    const dialog = within(await screen.findByRole("alertdialog", { name: "還有沒儲存的內容" }));
    expect(dialog.getByText("有內容沒有存到。現在離開，這些內容就會遺失。")).toBeInTheDocument();
    expect(dialog.getByRole("button", { name: "留在這頁" })).toHaveFocus();
    await user.click(dialog.getByRole("button", { name: "留在這頁" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/lessons/7");
    expect(screen.getByRole("textbox", { name: "題目" })).toHaveValue("還沒存的題目");

    // Escape stays too.
    await user.click(navLink("我的備課"));
    await screen.findByRole("alertdialog");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    await user.click(navLink("班級"));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "不儲存，直接離開" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/classes"));
  });

  it("「儲存後離開」 tries again, then goes on; it stays when it still cannot save", async () => {
    signIn();
    let serverUp = false;
    const api = mockApi(routes({ [`PUT ${SOP}`]: (call: MockCall) => (serverUp ? savedSop(call) : failing) }));
    const user = userEvent.setup();
    const { router } = await openLesson();
    await writeExitTicket(user, "要存的題目");
    await user.click(navLink("班級"));
    const dialog = within(await screen.findByRole("alertdialog"));
    await user.click(dialog.getByRole("button", { name: "儲存後離開" }));
    expect(await dialog.findByText("沒有儲存成功，所以還沒離開：伺服器暫時發生問題")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/lessons/7");

    serverUp = true;
    await user.click(dialog.getByRole("button", { name: "儲存後離開" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/classes"));
    expect(savedText(api)).toBe("要存的題目");
  });

  it("does not ask once the changes are saved, and warns on tab close only while something is unsaved", async () => {
    signIn();
    mockApi(routes());
    const user = userEvent.setup();
    const { router } = await openLesson();
    const unload = () => {
      const event = new Event("beforeunload", { cancelable: true });
      act(() => {
        window.dispatchEvent(event);
      });
      return event.defaultPrevented;
    };
    expect(unload()).toBe(false);
    await writeExitTicket(user, "Why?");
    expect(unload()).toBe(true);

    await settleAutosave(user);
    expect(unload()).toBe(false);
    await user.click(navLink("班級"));
    await waitFor(() => expect(router.state.location.pathname).toBe("/classes"));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
});

describe("the lesson saves itself (no save button)", () => {
  it("saves a moment after the last edit and says so in the head", async () => {
    signIn();
    const api = mockApi(routes());
    const user = userEvent.setup();
    await openLesson();
    expect(screen.getByText("會自動儲存")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /儲存/ })).not.toBeInTheDocument();

    autosaveConfig.delayMs = 20;
    await writeExitTicket(user, "What did you learn?");
    // No blur, no button: the timer saves.
    expect(await screen.findByText(/^已自動儲存 \d\d:\d\d$/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "題目" })).toHaveFocus();
    expect(savedText(api)).toBe("What did you learn?");
  });
});
