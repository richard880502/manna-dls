// 對話中偵測到的習慣建議：「要記住這個習慣嗎？」的記住／不用／修改後記住，與我的出題習慣頁在
// 很多習慣時的分組、搜尋與「N / 100」。
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { MemorySuggestion, PrepSession, PrepTurn, TeacherMemory } from "../api/types";
import { MemorySuggestions } from "../components/memory/MemorySuggestions";
import { TeacherMemories } from "../components/prefs/TeacherMemories";
import { baseRoutes, mockApi, renderApp, signIn } from "../test/mockApi";

const suggestion = (overrides: Partial<MemorySuggestion> = {}): MemorySuggestion => ({
  id: 31,
  kind: "custom",
  scope: "content",
  statement: "每份學習單最後附一題反思問題。",
  replaces_id: null,
  replaces_statement: null,
  source: "prep_turn",
  prep_session_id: 7,
  prep_turn_id: 101,
  worksheet_id: null,
  proposal_id: null,
  created_at: "2026-11-23T08:00:00Z",
  expires_at: "2026-11-23T08:15:00Z",
  intro: false,
  ...overrides,
});

const LIST = "/api/me/memories/candidates";

describe("habit suggestions found in what the teacher typed", () => {
  it("asks first, logs that it was shown, and remembers only after 記住", async () => {
    let pending = [suggestion({ intro: true })];
    const api = mockApi({
      [`GET ${LIST}`]: () => ({ candidates: pending }),
      "POST /api/me/memories/candidates/31/shown": { ok: true },
      "POST /api/me/memories": () => {
        pending = [];
        return { id: 9 };
      },
    });
    const user = userEvent.setup();
    render(<MemorySuggestions filter={{ worksheetId: 5 }} watch="a" />);
    expect(await screen.findByText(/要記住這個習慣嗎？/)).toBeInTheDocument();
    expect(screen.getByText("每份學習單最後附一題反思問題。")).toBeInTheDocument();
    // The one-time explanation comes with the first suggestion.
    expect(screen.getByText("Manna 只會在你按「記住」後才記，可隨時停止使用。")).toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("POST", "/api/me/memories/candidates/31/shown")).toHaveLength(1));
    // Nothing is saved before the teacher decides.
    expect(api.callsTo("POST", "/api/me/memories")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "記住" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/me/memories")).toEqual({ candidate_id: 31, confirmed: true }));
    expect(await screen.findByText(/已記住這項習慣/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "記住" })).not.toBeInTheDocument();
  });

  it("says what it would replace, and 不用 is sent to the server", async () => {
    const api = mockApi({
      [`GET ${LIST}`]: {
        candidates: [
          suggestion({ id: 32, replaces_id: 4, replaces_statement: "題目前面先放一小段短文。", intro: false }),
        ],
      },
      "POST /api/me/memories/candidates/32/shown": { ok: true },
      "POST /api/me/memories": { ok: true, cancelled: true },
    });
    const user = userEvent.setup();
    render(<MemorySuggestions filter={{ prepSessionId: 7 }} watch="a" />);
    expect(await screen.findByText(/會取代：『題目前面先放一小段短文。』/)).toBeInTheDocument();
    expect(screen.queryByText(/只會在你按「記住」後才記/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "不用" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/me/memories")).toEqual({ candidate_id: 32, confirmed: false }));
    await waitFor(() => expect(screen.queryByText(/要記住這個習慣嗎？/)).not.toBeInTheDocument());
  });

  it("修改後記住 opens the habit dialog with the sentence, and saves only what is confirmed there", async () => {
    const api = mockApi({
      [`GET ${LIST}`]: { candidates: [suggestion({ id: 33 })] },
      "POST /api/me/memories/candidates/33/shown": { ok: true },
      "POST /api/me/memories/preview": { id: 90, statement: "每份學習單最後附兩題反思問題。", replaces_id: null, replaces_statement: null },
      "POST /api/me/memories": { id: 12 },
    });
    const user = userEvent.setup();
    render(<MemorySuggestions filter={{ prepSessionId: 7 }} watch="a" />);
    await user.click(await screen.findByRole("button", { name: "修改後記住" }));
    // The suggestion is closed as an edit (a revise decision, not a rejection).
    await waitFor(() => expect(api.callsTo("POST", "/api/me/memories")[0]?.body).toEqual({ candidate_id: 33, confirmed: false, edit: true }));
    const dialog = await screen.findByRole("dialog");
    const draft = within(dialog).getByLabelText("想記住的長期習慣");
    expect(draft).toHaveValue("之後每份學習單最後附一題反思問題。");
    await user.clear(draft);
    await user.type(draft, "之後每份學習單最後附兩題反思問題");
    await user.click(within(dialog).getByRole("button", { name: "整理成習慣（尚未記住）" }));
    expect(await within(dialog).findByText("每份學習單最後附兩題反思問題。")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "確認並記住" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/me/memories")).toEqual({ candidate_id: 90, confirmed: true }));
    expect(await screen.findByText("已記住這項習慣。")).toBeInTheDocument();
  });

  it("shows nothing when there is no suggestion or the list cannot be read", async () => {
    mockApi({ [`GET ${LIST}`]: { candidates: [] } });
    const { container } = render(<MemorySuggestions filter={{ worksheetId: 5 }} watch="a" />);
    await waitFor(() => expect(container).toBeEmptyDOMElement());
  });

  it("sits next to the message it came from in a 備課對話", async () => {
    signIn();
    const turn = (id: number, request: string): PrepTurn => ({
      id,
      seq: id - 100,
      kind: "ask",
      request,
      status: "done",
      error: null,
      worksheet: null,
      result: { answer: "好的。", follow_ups: [] },
      interaction_id: null,
      created_at: "2026-11-23T08:00:00Z",
      finished_at: "2026-11-23T08:01:00Z",
    });
    const session: PrepSession = {
      id: 7,
      title: "備課",
      material: null,
      topic: null,
      class: null,
      template: null,
      turn_count: 2,
      last_kind: "ask",
      updated_at: "2026-11-23T08:01:00Z",
      created_at: "2026-11-23T08:00:00Z",
      turns: [turn(101, "之後題目都附反思問題"), turn(102, "謝謝")],
      running: false,
      next: [],
    };
    mockApi({
      ...baseRoutes(),
      "GET /api/prep-sessions/7": session,
      [`GET ${LIST}`]: { candidates: [suggestion({ prep_turn_id: 101 })] },
      "POST /api/me/memories/candidates/31/shown": { ok: true },
    });
    renderApp("/prep/7");
    const first = (await screen.findByText("之後題目都附反思問題")).closest("li")!;
    expect(await within(first).findByText(/要記住這個習慣嗎？/)).toBeInTheDocument();
    const second = screen.getByText("謝謝").closest("li")!;
    expect(within(second).queryByText(/要記住這個習慣嗎？/)).not.toBeInTheDocument();
  });
});

describe("我的出題習慣 with many habits", () => {
  const memory = (id: number, statement: string, overrides: Partial<TeacherMemory> = {}): TeacherMemory => ({
    id,
    kind: "custom",
    value: statement,
    statement,
    scope: "content",
    valid_at: "2026-11-19T08:00:00Z",
    invalid_at: null,
    superseded_by: null,
    invalidation_reason: null,
    interaction_id: 4,
    ...overrides,
  });

  it("groups by where a habit applies, searches, and counts against 100", async () => {
    const many: TeacherMemory[] = [
      memory(1, "選擇題使用 3 個選項。", { kind: "choice_options", value: "3", scope: null }),
      ...Array.from({ length: 60 }, (_, i) => memory(10 + i, `內容習慣第${i}條。`, { scope: "content" })),
      ...Array.from({ length: 30 }, (_, i) => memory(100 + i, `版面習慣第${i}條。`, { scope: "layout" })),
      memory(200, "作答說明用中文。", { scope: "language" }),
    ];
    mockApi({
      "GET /api/me/memories": { memories: many, enabled: true, graph_enabled: false, index: null, max_active: 100 },
    });
    const user = userEvent.setup();
    render(<TeacherMemories />);
    expect(await screen.findByText("92 / 100 項有效習慣")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /題目內容（60 項）/ })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /版面與格式（30 項）/ })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /語言與用字（1 項）/ })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /固定類型/ })).toBeInTheDocument();
    // Searching keeps only the matching habits (and the groups that still have one).
    await user.type(screen.getByLabelText("搜尋習慣"), "第7條");
    expect(await screen.findByText("找到 2 項。")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: /語言與用字/ })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: /題目內容（1 項）/ })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /版面與格式（1 項）/ })).toBeInTheDocument();
    // The same 修改 and 停止使用 on a free-form habit.
    expect(screen.getAllByRole("button", { name: /^修改：/ })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: /^停止使用：/ })).toHaveLength(2);
  });

  it("lists the suggestions still waiting for an answer", async () => {
    mockApi({
      "GET /api/me/memories": {
        memories: [],
        enabled: true,
        graph_enabled: false,
        index: null,
        max_active: 100,
        pending: [suggestion({ id: 41 })],
      },
      "POST /api/me/memories/candidates/41/shown": { ok: true },
    });
    render(<TeacherMemories />);
    expect(await screen.findByText("等你回答的建議")).toBeInTheDocument();
    expect(screen.getByText("0 / 100 項有效習慣")).toBeInTheDocument();
    expect(screen.getByText(/要記住這個習慣嗎？/)).toBeInTheDocument();
  });
});
