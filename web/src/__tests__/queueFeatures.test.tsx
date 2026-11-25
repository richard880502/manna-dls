// 排隊顯示 for the other queued features (docs/API.md「其他功能的 `queue`」): a block revision (改這一區), an SOP
// step, the SIOP check and a template being read (queueTemplate.test.tsx) show 「排隊中，前面還有 N 個」 in place of their working line;
// 取消排隊 calls DELETE /api/ai-jobs/{id} and then asks for that item again, which shows how it ended.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PrepTurn, SiopCheck, SopJob, WsBlock, WsProposal } from "../api/types";
import { SiopCheckPanel } from "../components/prep/SiopCheckPanel";
import { AskManna, RedoPending } from "../components/sop/parts";
import type { SopController } from "../components/sop/useSop";
import { ProposalPanel, type BlockRevise } from "../components/worksheet/BlockRevise";
import { mockApi, respond, signIn } from "../test/mockApi";

vi.mock("../context/AppContext", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../context/AppContext")>();
  return { ...actual, useApp: () => ({ config: { ai_enabled: true } }) as unknown as ReturnType<typeof actual.useApp> };
});

beforeEach(() => signIn());
afterEach(() => vi.unstubAllGlobals());

const queue = { job_id: 21, ahead: 2, eta_secs: 61 };
const LINE = "排隊中，前面還有 2 個（約 2 分鐘）";
const DELETE = "DELETE /api/ai-jobs/21";

async function cancelQueued(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "取消排隊" }));
  await user.click(screen.getByRole("button", { name: "確定取消" }));
}

describe("改這一區 (block revise)", () => {
  const before = { id: "b1", title: "A", items: [] } as unknown as WsBlock;
  const proposal = (overrides: Partial<WsProposal> = {}): WsProposal =>
    ({ id: 5, worksheet_id: 11, block_id: "b1", status: "running", before, queue, ...overrides }) as WsProposal;
  const ctl = (reload = vi.fn()) => ({ busy: false, running: true, reload }) as unknown as BlockRevise;

  it("shows the queue line instead of the working line, and cancelling reloads the proposals", async () => {
    const api = mockApi({ [DELETE]: respond(204, undefined) });
    const reload = vi.fn();
    const user = userEvent.setup();
    render(<ProposalPanel proposal={proposal()} ctl={ctl(reload)} />);
    expect(screen.getByText(LINE)).toBeInTheDocument();
    expect(screen.queryByText("Manna 正在改這一區…", { exact: false })).not.toBeInTheDocument();
    await cancelQueued(user);
    expect(api.callsTo("DELETE", "/api/ai-jobs/21")).toHaveLength(1);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("without queue the working line shows as before; a cancelled one shows the server's message", () => {
    const { rerender } = render(<ProposalPanel proposal={proposal({ queue: undefined })} ctl={ctl()} />);
    expect(screen.getByText("Manna 正在改這一區…", { exact: false })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "取消排隊" })).not.toBeInTheDocument();
    rerender(<ProposalPanel proposal={proposal({ status: "failed", queue: undefined, error: "已取消排隊。" })} ctl={ctl()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("已取消排隊。");
    expect(screen.getByRole("button", { name: "再改一次" })).toBeInTheDocument();
  });
});

describe("SOP step", () => {
  const job = (overrides: Partial<SopJob> = {}): SopJob => ({
    id: 8,
    lesson_id: 1,
    kind: "analysis",
    status: "running",
    phase: "排隊中",
    started_at: new Date().toISOString(),
    finished_at: null,
    sop_version_no: null,
    result: null,
    decided: [],
    queue,
    ...overrides,
  });

  it("AskManna shows the queue line and cancelling asks for the job again", async () => {
    const api = mockApi({ [DELETE]: respond(204, undefined) });
    const refreshJob = vi.fn().mockResolvedValue(undefined);
    const sop = {
      jobs: { analysis: job(), redo: [] },
      jobError: {},
      material: { paragraphs: [{ id: 1 }] },
      refreshJob,
    } as unknown as SopController;
    const user = userEvent.setup();
    render(<AskManna sop={sop} kind="analysis" label="請 Manna 分析" againLabel="再分析一次" />);
    expect(screen.getByText(LINE)).toBeInTheDocument();
    expect(screen.queryByText(/已經 \d+ 秒/)).not.toBeInTheDocument();
    await cancelQueued(user);
    expect(api.callsTo("DELETE", "/api/ai-jobs/21")).toHaveLength(1);
    expect(refreshJob).toHaveBeenCalledWith(8);
  });

  it("a suggestion asked for again shows the queue line too; cancelled, it says so and offers 再試一次", async () => {
    mockApi({ [DELETE]: respond(204, undefined) });
    const onCancelled = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <RedoPending label="第 1 段" job={job({ slot: "1" })} onAgain={() => undefined} onCancelled={onCancelled} />,
    );
    expect(screen.getByText(LINE)).toBeInTheDocument();
    await cancelQueued(user);
    expect(onCancelled).toHaveBeenCalledTimes(1);
    rerender(
      <RedoPending
        label="第 1 段"
        job={job({ status: "done", phase: "", queue: undefined, result: { status: "error", message: "已取消排隊。" } as SopJob["result"] })}
        onAgain={() => undefined}
      />,
    );
    expect(screen.getByText("已取消排隊。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再試一次" })).toBeInTheDocument();
  });
});

describe("SIOP check", () => {
  const check = (overrides: Partial<SiopCheck> = {}): SiopCheck => ({
    id: 4,
    run_no: 1,
    status: "running",
    error: null,
    worksheet_version_no: 1,
    items: [],
    interaction_id: null,
    created_at: "2026-11-25T08:00:00Z",
    finished_at: null,
    queue,
    ...overrides,
  });
  const turn = { id: 3, status: "done", kind: "worksheet", siop_check: check() } as unknown as PrepTurn;
  const CHECK = "GET /api/prep-sessions/7/turns/3/siop-check";

  it("shows the queue line; cancelling reads the check again and shows how it ended", async () => {
    const api = mockApi({
      [DELETE]: respond(204, undefined),
      [CHECK]: check({ status: "failed", queue: undefined, error: "已取消排隊。" }),
    });
    const user = userEvent.setup();
    render(<SiopCheckPanel sessionId={7} turn={turn} />);
    expect(screen.getByText(LINE)).toBeInTheDocument();
    expect(screen.queryByText("Manna 正在做 SIOP 自我檢核…")).not.toBeInTheDocument();
    await cancelQueued(user);
    expect(api.callsTo("DELETE", "/api/ai-jobs/21")).toHaveLength(1);
    expect(await screen.findByText("已取消排隊。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "再試一次" })).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/prep-sessions/7/turns/3/siop-check").length).toBeGreaterThanOrEqual(1);
  });
});
