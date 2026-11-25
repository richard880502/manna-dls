// 產生進度 (docs/API.md「產生進度」): the 「進度」 panel lists the server's steps, marks the current one,
// says changes in a polite live region, hides on ×, stays to show a failed step with its error and
// folds away a moment after everything is done; the 備課對話 shows it by the bottom bar while a turn
// runs, and a turn without progress (an older server or turn) keeps the working line.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { PrepSession, PrepTurn, ProgressStep, RunProgress, WorksheetState } from "../api/types";
import { RunProgressPanel, RunProgressSlot } from "../components/RunProgress";
import { worksheetConfig } from "../components/worksheet/useWorksheet";
import { prepConfig } from "../lib/prep";
import { announcement, stepText } from "../lib/progress";
import { emptyBrief, emptySettings, emptyWorksheetContent } from "../lib/worksheet";
import { baseRoutes, mockApi, renderApp, signIn } from "../test/mockApi";

beforeEach(() => {
  prepConfig.pollMs = 20;
  worksheetConfig.pollMs = 20;
});

afterEach(() => {
  prepConfig.pollMs = 2000;
  worksheetConfig.pollMs = 2000;
});

const step = (key: string, label: string, status: ProgressStep["status"], extra: Partial<ProgressStep> = {}): ProgressStep => ({
  key,
  label,
  status,
  ...extra,
});

const running: RunProgress = {
  steps: [
    step("read", "讀教材", "done"),
    step("tasks", "寫逐段任務", "running", { note: "重試中" }),
    step("warmup", "寫暖身", "pending"),
    step("pictures", "配圖", "pending"),
  ],
  current: 1,
  status: "running",
};

const failed: RunProgress = {
  steps: [
    step("read", "讀教材", "done"),
    step("tasks", "寫逐段任務", "failed", { message: "AI 目前沒有回應，這一步沒有產生成功；你可以繼續手動完成。" }),
    step("warmup", "寫暖身", "done"),
  ],
  current: null,
  status: "failed",
};

const done: RunProgress = {
  steps: running.steps.map((s) => ({ ...s, status: "done", note: undefined })),
  current: null,
  status: "done",
};

const panel = () => within(screen.getByRole("region", { name: "進度" }));

describe("the 進度 panel", () => {
  it("lists the steps one per line and marks the current one", () => {
    render(<RunProgressPanel progress={running} onClose={() => undefined} />);
    const items = panel().getAllByRole("listitem");
    expect(items.map((li) => li.querySelector(".run-progress-label")?.firstChild?.textContent)).toEqual([
      "讀教材",
      "寫逐段任務",
      "寫暖身",
      "配圖",
    ]);
    expect(items[1]).toHaveAttribute("aria-current", "step");
    expect(items.filter((li) => li.hasAttribute("aria-current"))).toHaveLength(1);
    expect(items[0]).toHaveClass("is-done");
    expect(items[1]).toHaveClass("is-running");
    expect(items[2]).toHaveClass("is-pending");
    // Each mark is said in words, and the retry beside the running step.
    expect(within(items[0]!).getByText("（完成）")).toBeInTheDocument();
    expect(within(items[1]!).getByText("重試中")).toBeInTheDocument();
    // The polite live region says where the run is.
    const live = panel().getByRole("status");
    expect(live).toHaveAttribute("aria-live", "polite");
    expect(live).toHaveTextContent("進行中：寫逐段任務，重試中（第 2 步，共 4 步）");
  });

  it("says counts, the end and the failed step", () => {
    expect(stepText(step("pictures", "配圖中", "running", { count: { done: 3, total: 6 } }))).toBe("配圖中（3／6）");
    expect(announcement(done)).toBe("全部完成");
    expect(announcement(failed)).toBe("沒有成功：寫逐段任務");
    render(<RunProgressPanel progress={failed} error="不會用到" onClose={() => undefined} />);
    const item = panel().getAllByRole("listitem")[1]!;
    expect(item).toHaveClass("is-failed");
    expect(item).toHaveTextContent("AI 目前沒有回應，這一步沒有產生成功；你可以繼續手動完成。");
    expect(panel().queryByText("不會用到")).toBeNull();
  });

  it("shows the run's own error under a failed step that has none", () => {
    const bare: RunProgress = { ...failed, steps: [step("plan", "規劃教學活動", "failed")] };
    render(<RunProgressPanel progress={bare} error="AI 目前沒有回應，這次沒有完成。" onClose={() => undefined} />);
    expect(panel().getByText("AI 目前沒有回應，這次沒有完成。")).toBeInTheDocument();
  });
});

/** A page that feeds the slot one progress after another. */
function Harness({ initial, runKey = "r1" }: { initial: RunProgress | null; runKey?: string }) {
  const [progress, setProgress] = useState<RunProgress | null>(initial);
  const [key, setKey] = useState(runKey);
  return (
    <>
      <button type="button" onClick={() => setProgress(failed)}>
        fail
      </button>
      <button type="button" onClick={() => setProgress(done)}>
        finish
      </button>
      <button
        type="button"
        onClick={() => {
          setKey("r2");
          setProgress(running);
        }}
      >
        next run
      </button>
      <RunProgressSlot progress={progress} runKey={key} fallback={<p>還在做</p>} />
    </>
  );
}

describe("when the panel shows", () => {
  it("× hides it for this run only; the next run shows again", async () => {
    const user = userEvent.setup();
    render(<Harness initial={running} />);
    await user.click(panel().getByRole("button", { name: "關閉進度" }));
    expect(screen.queryByRole("region", { name: "進度" })).toBeNull();
    expect(screen.getByText("還在做")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "next run" }));
    expect(screen.getByRole("region", { name: "進度" })).toBeInTheDocument();
  });

  it("a failed run stays, saying which step failed", async () => {
    const user = userEvent.setup();
    render(<Harness initial={running} />);
    await user.click(screen.getByRole("button", { name: "fail" }));
    expect(panel().getByRole("status")).toHaveTextContent("沒有成功：寫逐段任務");
    await new Promise((r) => setTimeout(r, 120));
    expect(screen.getByRole("region", { name: "進度" })).toBeInTheDocument();
  });

  it("a finished run folds into one line at once, says so, and stays", async () => {
    const user = userEvent.setup();
    render(<Harness initial={running} />);
    await user.click(screen.getByRole("button", { name: "finish" }));
    expect(panel().getByRole("status")).toHaveTextContent("全部完成");
    expect(panel().getByText(`：全部完成（${done.steps.length} 步）`)).toBeInTheDocument();
    expect(panel().queryByRole("list")).toBeNull();
    await new Promise((r) => setTimeout(r, 120));
    expect(screen.getByRole("region", { name: "進度" })).toBeInTheDocument();
    // × still hides it.
    await user.click(panel().getByRole("button", { name: "關閉進度" }));
    expect(screen.queryByRole("region", { name: "進度" })).toBeNull();
  });

  it("collapseDone={false} keeps the steps listed, all done", () => {
    render(<RunProgressPanel progress={done} onClose={() => undefined} collapseDone={false} />);
    expect(panel().getByRole("list")).toBeInTheDocument();
    expect(panel().getByRole("status")).toHaveTextContent("全部完成");
  });

  it("a run already over when the page saw it shows nothing", () => {
    render(<Harness initial={done} />);
    expect(screen.queryByRole("region", { name: "進度" })).toBeNull();
    expect(screen.getByText("還在做")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the 備課對話

const S7 = "/api/prep-sessions/7";

const turn = (overrides: Partial<PrepTurn> = {}): PrepTurn => ({
  id: 301,
  seq: 1,
  kind: "activity",
  request: "規劃教學活動",
  status: "running",
  error: null,
  worksheet: null,
  result: null,
  interaction_id: null,
  created_at: "2026-11-18T08:00:00Z",
  finished_at: null,
  ...overrides,
});

const session = (t: PrepTurn): PrepSession => ({
  id: 7,
  title: "The Umbrella Stand",
  material: { id: 5, title: "The Umbrella Stand" },
  topic: null,
  class: null,
  template: null,
  turn_count: 1,
  last_kind: t.kind,
  updated_at: "2026-11-18T08:00:00Z",
  created_at: "2026-11-18T08:00:00Z",
  turns: [t],
  running: t.status === "running",
  next: [],
});

const activitySteps = (statuses: ProgressStep["status"][], current: number | null, status: RunProgress["status"]): RunProgress => ({
  steps: [
    step("read", "讀教材", statuses[0]!),
    step("curriculum", "找課綱條目", statuses[1]!),
    step("plan", "規劃教學活動", statuses[2]!),
  ],
  current,
  status,
});

describe("the 備課對話", () => {
  it("a worksheet regenerated after its turn shows the worksheet's steps in its card", async () => {
    signIn();
    // The turn is long over (no progress of its own); the worksheet runs again (from the editor).
    const wsTurn = turn({
      kind: "worksheet",
      request: "學習單",
      status: "done",
      finished_at: "2026-11-18T06:00:00Z",
      progress: null,
      worksheet: { id: 11, title: "U", version_no: 2, running: true },
    });
    const progress: RunProgress = {
      steps: [step("read", "讀教材", "done"), step("tasks", "寫逐段任務", "running"), step("pictures", "配圖", "pending")],
      current: 1,
      status: "running",
    };
    const regenerating: WorksheetState = {
      id: 11,
      title: "U",
      filename: "U",
      lesson: null,
      class_id: null,
      generated: true,
      levels: ["standard"],
      version_no: 2,
      created_at: "2026-11-18T05:00:00Z",
      updated_at: "2026-11-18T05:00:00Z",
      material: { title: "U", paragraphs: [], source: { kind: "library", lesson_id: null, material_version_no: null, note: "", material_id: 5 } },
      settings: emptySettings(),
      suggested: emptySettings(),
      brief: emptyBrief(),
      versions_count: 2,
      saved_at: "2026-11-18T05:00:00Z",
      content: { ...emptyWorksheetContent(), levels: ["standard"] },
      jobs: [
        {
          id: 1,
          worksheet_id: 11,
          run_no: 2,
          step: "tasks",
          block_id: null,
          status: "running",
          outcome: null,
          phase: "",
          message: null,
          interaction_id: null,
          context: [],
          started_at: "2026-11-18T06:00:00Z",
          finished_at: null,
        },
      ],
      running: true,
      progress,
      undoable: [],
      sop_sync: { linked: false, changes: 0 },
      prep_session_id: 7,
    };
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: { ...session(wsTurn), running: false },
      "GET /api/worksheets/11": regenerating,
      "GET /api/worksheets/11/comments": [],
      "GET /api/worksheets/11/proposals": [],
      "GET /api/worksheets/11/jobs": { running: true, version_no: 2, jobs: [], progress },
    });
    renderApp("/prep/7");
    const region = await screen.findByRole("region", { name: "進度" });
    // The worksheet's steps stand in the pane beside the conversation (the sheet is there).
    expect(region.closest(".prep-split-sheet")).not.toBeNull();
    expect(within(region).getAllByRole("listitem")[1]).toHaveAttribute("aria-current", "step");
  });

  it("shows the turn's steps in the conversation, in the turn's place, while it runs, then its failure", async () => {
    signIn();
    const polls: PrepSession[] = [
      session(turn({ progress: activitySteps(["done", "running", "pending"], 1, "running") })),
      session(turn({ progress: activitySteps(["done", "done", "running"], 2, "running") })),
      session(
        turn({
          status: "failed",
          error: "AI 目前沒有回應，這次沒有完成。請稍後按「再試一次」。",
          finished_at: "2026-11-18T08:01:00Z",
          progress: activitySteps(["done", "done", "failed"], null, "failed"),
        }),
      ),
    ];
    let n = 0;
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: () => polls[Math.min(n++, polls.length - 1)] });
    renderApp("/prep/7");
    const region = await screen.findByRole("region", { name: "進度" });
    expect(within(region).getAllByRole("listitem")).toHaveLength(3);
    // In the conversation, where the turn's result will be, not by the bottom bar.
    expect(region.closest(".prep-result")).not.toBeNull();
    expect(region.closest("form")).toBeNull();
    // The panel stands in for the working line.
    expect(document.querySelector(".prep-working")).toBeNull();
    await waitFor(() => expect(within(region).getByRole("status")).toHaveTextContent("規劃教學活動"));
    await waitFor(() => expect(within(region).getByRole("status")).toHaveTextContent("沒有成功：規劃教學活動"));
    const failedItem = within(region).getAllByRole("listitem")[2]!;
    expect(failedItem).toHaveClass("is-failed");
    expect(failedItem).toHaveTextContent("AI 目前沒有回應，這次沒有完成。");
    expect(within(region).queryByRole("listitem", { current: "step" })).toBeNull();
  });

  it("× leaves the working line; a turn without progress keeps it as before", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      [`GET ${S7}`]: session(turn({ progress: activitySteps(["running", "pending", "pending"], 0, "running") })),
    });
    const user = userEvent.setup();
    const { unmount } = renderApp("/prep/7");
    const region = await screen.findByRole("region", { name: "進度" });
    expect(within(region).getAllByRole("listitem")[0]).toHaveAttribute("aria-current", "step");
    await user.click(within(region).getByRole("button", { name: "關閉進度" }));
    expect(screen.queryByRole("region", { name: "進度" })).toBeNull();
    expect(screen.getAllByText(/Manna 正在/).length).toBeGreaterThan(0);
    unmount();

    // An older server: no `progress` at all.
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session(turn()) });
    renderApp("/prep/7");
    await waitFor(() => expect(document.querySelector(".prep-working")).not.toBeNull());
    expect(screen.queryByRole("region", { name: "進度" })).toBeNull();
  });

  it("keeps following a done turn while its SIOP 自我檢核 runs, then folds the panel into one line", async () => {
    signIn();
    const siopRunning: RunProgress = {
      steps: [step("read", "讀教材", "done"), step("plan", "規劃教學活動", "done"), step("siop", "SIOP 自我檢核", "running")],
      current: 2,
      status: "running",
    };
    const over: RunProgress = { ...siopRunning, steps: siopRunning.steps.map((s) => ({ ...s, status: "done" })), current: null, status: "done" };
    const doneTurn = { status: "done" as const, finished_at: "2026-11-18T08:01:00Z" };
    const polls = [session(turn({ ...doneTurn, progress: siopRunning })), session(turn({ ...doneTurn, progress: over }))];
    let n = 0;
    const api = mockApi({ ...baseRoutes(), [`GET ${S7}`]: () => polls[Math.min(n++, polls.length - 1)] });
    renderApp("/prep/7");
    const region = await screen.findByRole("region", { name: "進度" });
    await waitFor(() => expect(within(region).getByRole("status")).toHaveTextContent("全部完成"));
    expect(api.calls.filter((c) => c.path === S7).length).toBeGreaterThanOrEqual(2);
    await waitFor(() => expect(within(region).queryByRole("list")).toBeNull());
    expect(within(region).getByText(/全部完成（3 步）/)).toBeInTheDocument();
    await act(async () => new Promise((r) => setTimeout(r, 80)));
    expect(screen.getByRole("region", { name: "進度" })).toBeInTheDocument();
  });

  it("a turn long over comes without progress: no panel, and the turn shows as before", async () => {
    signIn();
    // The server only reads progress for turns still running or just over; older turns carry null.
    const over = turn({ status: "done", finished_at: "2026-11-18T06:00:00Z", progress: null });
    const polls = [
      session(turn({ progress: activitySteps(["done", "done", "running"], 2, "running") })),
      session(over),
    ];
    let n = 0;
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: () => polls[Math.min(n++, polls.length - 1)] });
    renderApp("/prep/7");
    await screen.findByRole("region", { name: "進度" });
    await waitFor(() => expect(screen.queryByRole("region", { name: "進度" })).toBeNull());
    expect(document.querySelector(".prep-working")).toBeNull();

    // Loading a session whose turns are all over: nothing to show.
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: session(over) });
    renderApp("/prep/7");
    await waitFor(() => expect(screen.getAllByRole("list", { name: "備課對話" }).length).toBeGreaterThan(0));
    expect(screen.queryByRole("region", { name: "進度" })).toBeNull();
  });
});
