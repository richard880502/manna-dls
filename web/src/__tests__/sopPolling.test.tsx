import { act, renderHook } from "@testing-library/react";
import { StrictMode, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/client";
import type { SopAnalysisSuggestion, SopJob, SopState } from "../api/types";
import { sopConfig, useSop } from "../components/sop/useSop";
import { emptyContent } from "../lib/sop";

const running = (id: number, kind: SopJob["kind"] = "analysis"): SopJob => ({
  id, kind, lesson_id: 7, status: "running", phase: "", started_at: "", finished_at: null,
  sop_version_no: null, result: null, decided: [],
});
const shown = (job: SopJob): SopJob => ({
  ...job, status: "done", result: { status: "shown", interaction_id: null, message: null, output: null, sources: [], context: [] },
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const state = (job: SopJob): SopState => ({
  lesson_id: 7, version_no: null, versions_count: 0, saved_at: null, material: null,
  content: emptyContent(), today_objective: "", jobs: { analysis: job as SopJob<SopAnalysisSuggestion>, objectives: null },
});
const wrapper = ({ children }: { children: ReactNode }) => <StrictMode>{children}</StrictMode>;
beforeEach(() => { vi.useFakeTimers(); sopConfig.pollMs = 20; });
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

describe("SOP job polling", () => {
  it("keeps one request per job across effect changes and logs the shown transition once", async () => {
    const events = vi.spyOn(api, "scaffoldEvent").mockResolvedValue({} as Awaited<ReturnType<typeof api.scaffoldEvent>>);
    const first = running(1);
    const pending = deferred<SopJob>();
    vi.spyOn(api, "getSop").mockResolvedValue(state(first));
    const poll = vi.spyOn(api, "getSopJob").mockImplementation((id) => id === 1 ? pending.promise : Promise.resolve(running(id, "objectives")));
    vi.spyOn(api, "startSopJob").mockResolvedValue(running(2, "objectives"));
    const { result } = renderHook(() => useSop(7, null), { wrapper });
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(20); });
    await act(async () => { await result.current.startJob("objectives"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    expect(poll.mock.calls.filter(([id]) => id === 1)).toHaveLength(1);
    await act(async () => { pending.resolve(shown(first)); await pending.promise; });
    await act(async () => { await vi.advanceTimersByTimeAsync(60); });
    expect(result.current.jobs.analysis?.status).toBe("done");
    expect(events.mock.calls.filter(([event]) => event.action === "show")).toEqual([
      [{ scaffold_code: "SC-SOP-ANALYSIS", action: "show", trigger: "suggestions" }],
    ]);
  });
  it("does not log a late response after unmount", async () => {
    const events = vi.spyOn(api, "scaffoldEvent").mockResolvedValue({} as Awaited<ReturnType<typeof api.scaffoldEvent>>);
    const first = running(1);
    const pending = deferred<SopJob>();
    vi.spyOn(api, "getSop").mockResolvedValue(state(first));
    const poll = vi.spyOn(api, "getSopJob").mockReturnValue(pending.promise);
    const { unmount } = renderHook(() => useSop(7, null), { wrapper });
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(20); });
    expect(poll).toHaveBeenCalledTimes(1);
    unmount();
    await act(async () => { pending.resolve(shown(first)); await pending.promise; });
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    expect(poll).toHaveBeenCalledTimes(1);
    expect(events.mock.calls.filter(([event]) => event.action === "show")).toHaveLength(0);
  });
  it("ignores an old response when a running job is replaced", async () => {
    const events = vi.spyOn(api, "scaffoldEvent").mockResolvedValue({} as Awaited<ReturnType<typeof api.scaffoldEvent>>);
    const first = running(1);
    const pending = deferred<SopJob>();
    vi.spyOn(api, "getSop").mockResolvedValue(state(first));
    vi.spyOn(api, "getSopJob").mockReturnValue(pending.promise);
    vi.spyOn(api, "startSopJob").mockResolvedValue(running(2));
    const { result } = renderHook(() => useSop(7, null), { wrapper });
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(20); });
    await act(async () => { await result.current.startJob("analysis"); });
    await act(async () => { pending.resolve(shown(first)); await pending.promise; });
    expect(result.current.jobs.analysis?.id).toBe(2);
    expect(result.current.jobs.analysis?.status).toBe("running");
    expect(events.mock.calls.filter(([event]) => event.action === "show")).toHaveLength(0);
  });
});
