// 排隊顯示 (docs/AI_REQUEST_CONTROL.md 步驟 5): a run whose job still waits shows 「排隊中，前面還有 N 個」
// (with a rounded-up estimate), 取消排隊 asks in the page first, calls DELETE /api/ai-jobs/{id} and then
// refreshes the page; a refusal (409) is shown; without `queue` the steps show as before.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RunProgress } from "../api/types";
import { RunProgressPanel } from "../components/RunProgress";
import { queueText } from "../lib/progress";
import { mockApi, respond, signIn } from "../test/mockApi";

beforeEach(() => signIn());
afterEach(() => vi.unstubAllGlobals());

const base: RunProgress = {
  steps: [
    { key: "read", label: "讀教材", status: "running" },
    { key: "tasks", label: "寫逐段任務", status: "pending" },
  ],
  current: 0,
  status: "running",
};
const queued = (ahead: number, eta_secs: number | null): RunProgress => ({
  ...base,
  queue: { job_id: 7, ahead, eta_secs },
});

describe("queue line", () => {
  it("says how many are ahead, with the estimate rounded up to whole minutes", () => {
    render(<RunProgressPanel progress={queued(3, 61)} onClose={() => undefined} />);
    expect(screen.getByText("排隊中，前面還有 3 個（約 2 分鐘）")).toBeInTheDocument();
    expect(screen.queryByText("讀教材")).not.toBeInTheDocument();
  });

  it("omits the estimate when unknown, and says under a minute", () => {
    expect(queueText({ job_id: 1, ahead: 2, eta_secs: null })).toBe("排隊中，前面還有 2 個");
    expect(queueText({ job_id: 1, ahead: 2, eta_secs: 20 })).toBe("排隊中，前面還有 2 個（不到 1 分鐘）");
    expect(queueText({ job_id: 1, ahead: 1, eta_secs: 120 })).toBe("排隊中，前面還有 1 個（約 2 分鐘）");
  });

  it("ahead 0: the next one is yours", () => {
    render(<RunProgressPanel progress={queued(0, 5)} onClose={() => undefined} />);
    expect(screen.getByText("排隊中，下一個就輪到你")).toBeInTheDocument();
  });

  it("without queue the steps show as before", () => {
    render(<RunProgressPanel progress={base} onClose={() => undefined} />);
    expect(screen.getByText("讀教材")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消排隊" })).not.toBeInTheDocument();
  });

  it("cancel: asks first, then DELETEs the job and refreshes", async () => {
    const api = mockApi({ "DELETE /api/ai-jobs/7": respond(204, undefined) });
    const onCancelled = vi.fn();
    const user = userEvent.setup();
    render(<RunProgressPanel progress={queued(2, null)} onClose={() => undefined} onCancelled={onCancelled} />);
    await user.click(screen.getByRole("button", { name: "取消排隊" }));
    expect(api.callsTo("DELETE", "/api/ai-jobs/7")).toHaveLength(0);
    // Opening the confirm focuses the safe choice; cancelling gives focus back to the trigger.
    expect(screen.getByRole("button", { name: "不取消" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "不取消" }));
    expect(screen.getByRole("button", { name: "取消排隊" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "取消排隊" }));
    await user.click(screen.getByRole("button", { name: "確定取消" }));
    expect(api.callsTo("DELETE", "/api/ai-jobs/7")).toHaveLength(1);
    expect(onCancelled).toHaveBeenCalledTimes(1);
  });

  it("shows the server's message when cancelling is refused with 409", async () => {
    mockApi({
      "DELETE /api/ai-jobs/7": () => respond(409, { error: { code: "conflict", message: "這個工作已經開始了。" } }),
    });
    const onCancelled = vi.fn();
    const user = userEvent.setup();
    render(<RunProgressPanel progress={queued(2, null)} onClose={() => undefined} onCancelled={onCancelled} />);
    await user.click(screen.getByRole("button", { name: "取消排隊" }));
    await user.click(screen.getByRole("button", { name: "確定取消" }));
    expect(await screen.findByText("這個工作已經開始了。")).toBeInTheDocument();
    expect(onCancelled).not.toHaveBeenCalled();
  });
});
