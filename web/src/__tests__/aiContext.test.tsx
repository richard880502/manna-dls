// 「Manna 已帶入的資訊」 (AiResult.context) under a suggestion in the lesson workspace: open by default
// for teachers who said talking to AI is hard (P3), collapsed for everyone else.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { AiResult, Hardest, SopAnalysisSuggestion, SopJob, SopState } from "../api/types";
import { emptyContent } from "../lib/sop";
import { baseRoutes, lessonDetail, mockApi, preferences, renderApp, signIn } from "../test/mockApi";

const P1 = "Mrs. Lin put an umbrella stand outside her shop.";

const output: SopAnalysisSuggestion = {
  text_type: "記敘文",
  main_idea: "信任會帶來信任",
  structure: "開場",
  language_focus: [],
  paragraphs: [],
  missing_paragraph_ids: [],
  dropped_evidence: 0,
};

function sopWithAnalysis(context: AiResult<SopAnalysisSuggestion>["context"]): SopState {
  const result: AiResult<SopAnalysisSuggestion> = {
    interaction_id: 91,
    status: "shown",
    message: null,
    output,
    sources: [],
    context,
  };
  const job: SopJob<SopAnalysisSuggestion> = {
    id: 5,
    lesson_id: 7,
    kind: "analysis",
    status: "done",
    phase: "正在讀教材，逐段分析寫作特色",
    started_at: "2026-11-14T08:00:00Z",
    finished_at: "2026-11-14T08:01:00Z",
    sop_version_no: 1,
    result,
    decided: [],
  };
  return {
    lesson_id: 7,
    version_no: 1,
    versions_count: 1,
    saved_at: "2026-11-14T08:00:00Z",
    material: {
      id: 1,
      version_no: 1,
      title: "The Umbrella Stand",
      source_note: "",
      text: P1,
      paragraphs: [{ id: "p1", no: 1, text: P1 }],
      word_count: 9,
      created_at: "2026-11-14T08:00:00Z",
    },
    content: emptyContent(),
    today_objective: "",
    jobs: { analysis: job, objectives: null },
  };
}

const context = [
  { label: "班級特性（高一 3 班）", preview: "高一普通班，程度落差大" },
  { label: "教材", preview: "The Umbrella Stand（1 段，9 字）" },
];

function routesFor(hardest: Hardest[], sop: SopState) {
  return {
    ...baseRoutes({}, preferences({ hardest })),
    "GET /api/lessons/7": lessonDetail(),
    "GET /api/lessons/7/runs": [],
    "GET /api/lessons/7/sop": sop,
    "GET /api/materials/1/annotations": [],
  };
}

function events(api: ReturnType<typeof mockApi>) {
  return api.callsTo("POST", "/api/scaffold-events").map((c) => c.body as Record<string, unknown>);
}

async function openAnalysisStep(user: ReturnType<typeof userEvent.setup>) {
  renderApp("/lessons/7");
  expect(await screen.findByRole("heading", { name: "第 1 課：Not only… but also", level: 1 })).toBeInTheDocument();
  await user.click(within(screen.getByRole("navigation", { name: "備課步驟" })).getByRole("button", { name: /分析寫作特色/ }));
}

describe("「Manna 已帶入的資訊」 (AiResult.context)", () => {
  it("is open by default for 跟 AI 溝通 and logs SC-PREF", async () => {
    signIn();
    const api = mockApi(routesFor(["ai_communication"], sopWithAnalysis(context)));
    const user = userEvent.setup();
    await openAnalysisStep(user);

    const panel = await screen.findByRole("region", { name: "Manna 已帶入的資訊" });
    expect(within(panel).getByText("班級特性（高一 3 班）")).toBeInTheDocument();
    expect(within(panel).getByText("高一普通班，程度落差大")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Manna 已帶入的資訊/ })).toHaveAttribute("aria-expanded", "true");
    await waitFor(() =>
      expect(events(api)).toContainEqual({
        scaffold_code: "SC-PREF",
        action: "show",
        trigger: "preference:hardest=ai_communication",
      }),
    );
  });

  it("is collapsed for others, opens on click and logs SC-CONTEXT expand", async () => {
    signIn();
    const api = mockApi(routesFor([], sopWithAnalysis(context)));
    const user = userEvent.setup();
    await openAnalysisStep(user);

    const toggle = await screen.findByRole("button", { name: /Manna 已帶入的資訊/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("高一普通班，程度落差大")).not.toBeInTheDocument();
    await user.click(toggle);
    expect(screen.getByText("高一普通班，程度落差大")).toBeInTheDocument();
    await waitFor(() =>
      expect(events(api)).toContainEqual(expect.objectContaining({ scaffold_code: "SC-CONTEXT", action: "expand" })),
    );
    expect(events(api).some((e) => e.scaffold_code === "SC-PREF" && e.action === "show")).toBe(false);
  });

  it("renders nothing when the result carries no context", async () => {
    signIn();
    mockApi(routesFor(["ai_communication"], sopWithAnalysis([])));
    const user = userEvent.setup();
    await openAnalysisStep(user);
    expect(await screen.findByRole("button", { name: "請 Manna 再分析一次" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Manna 已帶入的資訊/ })).not.toBeInTheDocument();
  });
});
