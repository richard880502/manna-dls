import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { KnowledgeImportResult, KnowledgeItemRow, KnowledgeSummary, VocabularyRow } from "../api/types";
import { can } from "../lib/permissions";
import { baseRoutes, mockApi, renderApp, respond, signIn, teacher, type MockCall } from "../test/mockApi";

const manager = { ...teacher, research_id: "KB-01", role: "kb_manager" as const };
const pi = { ...teacher, research_id: "PI-01", role: "pi" as const };

// Synthetic fixtures: not real curriculum text or word-list data.
const item = (overrides: Partial<KnowledgeItemRow> = {}): KnowledgeItemRow => ({
  item_code: "T-G-3-V-2",
  version: 1,
  category: "curriculum",
  school_track: "general",
  title: "◎3-Ⅴ-2 學習表現｜測試閱讀",
  content: "能讀懂測試用的短文並找出主旨。",
  official_code: "◎3-Ⅴ-2",
  item_kind: "learning_performance",
  strand_name: "測試閱讀",
  stage: "V",
  subject: "英語文",
  advanced: false,
  recurring: true,
  source_doc: "測試領綱.pdf",
  source_version: "測試版",
  source_page: "12",
  source_sha256: "a".repeat(64),
  import_batch_id: 1,
  created_at: "2026-11-15T02:00:00Z",
  retired_at: null,
  ...overrides,
});

const word = (overrides: Partial<VocabularyRow> = {}): VocabularyRow => ({
  word: "conserve",
  headword: "conserve",
  level: 6,
  pos: "v.",
  kind: "base",
  appendix_category: "",
  list_version: "TEST LIST v1",
  source_doc: "測試詞彙表.pdf",
  source_page: "5",
  source_sha256: "b".repeat(64),
  version: 1,
  import_batch_id: 2,
  retired_at: null,
  ...overrides,
});

const summary = (overrides: Partial<KnowledgeSummary> = {}): KnowledgeSummary => ({
  knowledge: {
    active: 22,
    real_active: 6,
    demo_active: 16,
    groups: [
      { school_track: "general", category: "curriculum", item_kind: "learning_performance", demo: false, count: 4 },
      { school_track: "comprehensive", category: "curriculum", item_kind: "learning_performance", demo: false, count: 2 },
      { school_track: "both", category: "grammar", item_kind: "", demo: true, count: 16 },
    ],
  },
  vocabulary: {
    active_forms: 3,
    groups: [
      { list_version: "TEST LIST v1", level: 1, forms: 2, entries: 1 },
      { list_version: "TEST LIST v1", level: null, forms: 1, entries: 1 },
    ],
  },
  recent_batches: [
    {
      id: 1,
      kind: "knowledge",
      file_sha256: "c".repeat(64),
      row_count: 6,
      inserted: 6,
      updated: 0,
      unchanged: 0,
      imported_by: "KB-01",
      created_at: "2026-11-15T02:00:00Z",
    },
  ],
  ...overrides,
});

const importResult = (overrides: Partial<KnowledgeImportResult> = {}): KnowledgeImportResult => ({
  dry_run: true,
  imported: 0,
  rows: 3,
  inserted: 2,
  updated: 1,
  unchanged: 0,
  by_track: { general: 2, comprehensive: 1 },
  sha256: "d".repeat(64),
  batch_id: null,
  ...overrides,
});

const knowledgeRoutes = (me: typeof manager | typeof pi = manager) => ({
  ...baseRoutes(),
  "GET /api/me": me,
  "GET /api/admin/knowledge/summary": summary(),
  "GET /api/admin/knowledge/items": { total: 1, items: [item()] },
  "GET /api/admin/vocabulary/items": { total: 1, items: [word()] },
});

describe("permissions", () => {
  it("maps roles to capabilities and prefers what the server reported", () => {
    expect(can(pi, "administer")).toBe(true);
    expect(can(pi, "manage_knowledge")).toBe(true);
    expect(can(manager, "manage_knowledge")).toBe(true);
    expect(can(manager, "administer")).toBe(false);
    expect(can(teacher, "manage_knowledge")).toBe(false);
    expect(can(null, "manage_knowledge")).toBe(false);
    expect(can({ ...teacher, capabilities: ["manage_knowledge"] }, "manage_knowledge")).toBe(true);
    expect(can({ ...pi, capabilities: [] }, "administer")).toBe(false);
  });
});

describe("知識庫管理", () => {
  it("is offered to a 知識庫管理員, who does not get 後台管理", async () => {
    signIn();
    mockApi(knowledgeRoutes());
    renderApp("/");
    expect(await screen.findByRole("link", { name: "知識庫管理" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "後台管理" })).not.toBeInTheDocument();
  });

  it("is hidden from teachers; the direct URL explains instead of loading anything", async () => {
    signIn();
    const api = mockApi(baseRoutes());
    renderApp("/knowledge");
    expect(await screen.findByRole("heading", { name: "知識庫管理", level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/只開放給計畫主持人與知識庫管理員/)).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "知識庫管理" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("選擇 CSV 檔案")).not.toBeInTheDocument();
    expect(api.calls.some((c) => c.path.startsWith("/api/admin/"))).toBe(false);
  });

  it("shows what is in the knowledge base and lists items and words read-only", async () => {
    signIn();
    const api = mockApi(knowledgeRoutes(pi));
    const user = userEvent.setup();
    renderApp("/knowledge");

    const status = within(await screen.findByRole("list", { name: "目前內容" }));
    expect(status.getByText("普高 4 條 · 綜高 2 條")).toBeInTheDocument();
    expect(status.getByText("6 條")).toBeInTheDocument();
    expect(status.getByText(/TEST LIST v1：第 1 級 1 詞條 · 附錄 1 詞/)).toBeInTheDocument();
    expect(status.getByText("16 條使用中")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "最近的匯入" })).getByText(/新增 6、更新 0、不變 0/)).toBeInTheDocument();

    const items = within(await screen.findByRole("list", { name: "條目" }));
    expect(items.getByRole("heading", { name: "◎3-Ⅴ-2 學習表現｜測試閱讀" })).toBeInTheDocument();
    expect(items.getByText("能讀懂測試用的短文並找出主旨。")).toBeInTheDocument();
    expect(items.getByText(/測試領綱\.pdf（測試版） 第 12 頁 · 第 1 版/)).toBeInTheDocument();
    expect(screen.getByText("顯示 1／1 條")).toBeInTheDocument();
    const words = within(await screen.findByRole("list", { name: "詞彙" }));
    expect(words.getByRole("heading", { name: "conserve" })).toBeInTheDocument();
    expect(words.getByText("第 6 級")).toBeInTheDocument();

    // filters go to the server
    const search = within(screen.getByRole("search", { name: "搜尋條目" }));
    await user.type(search.getByLabelText("代碼或文字"), "3-V-2");
    await user.selectOptions(search.getByLabelText("學校類型"), "綜高");
    await user.selectOptions(search.getByLabelText("狀態"), "已停用");
    await user.click(search.getByRole("button", { name: "搜尋" }));
    await waitFor(() => {
      const last = api.callsTo("GET", "/api/admin/knowledge/items").at(-1);
      const params = new URLSearchParams(last?.url.split("?")[1]);
      expect(Object.fromEntries(params)).toEqual({
        q: "3-V-2",
        track: "comprehensive",
        status: "retired",
        limit: "20",
        offset: "0",
      });
    });
    const vocab = within(screen.getByRole("search", { name: "搜尋詞彙表" }));
    await user.selectOptions(vocab.getByLabelText("級別"), "附錄（未分級）");
    await waitFor(() =>
      expect(api.callsTo("GET", "/api/admin/vocabulary/items").at(-1)?.url).toContain("level=appendix"),
    );
    // nothing on the page edits an item
    expect(screen.queryByRole("button", { name: /修改|刪除/ })).not.toBeInTheDocument();
  });

  it("previews an import before committing it", async () => {
    signIn();
    const api = mockApi({
      ...knowledgeRoutes(),
      "POST /api/admin/knowledge/import": (call: MockCall) =>
        call.url.includes("dry_run=true")
          ? importResult()
          : importResult({ dry_run: false, imported: 3, batch_id: 7 }),
    });
    const user = userEvent.setup();
    renderApp("/knowledge");

    const section = within(await screen.findByRole("region", { name: "匯入課綱條目" }));
    // no file yet
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByText("請先選擇 CSV 檔案。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/knowledge/import")).toHaveLength(0);

    const csv = "item_code,category,school_track,title,content,source_doc,source_version,source_page\nA,curriculum,general,t,c,d,v,1\n";
    await user.upload(section.getByLabelText("選擇 CSV 檔案"), new File([csv], "kb.csv", { type: "text/csv" }));
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByText(/預覽（尚未寫入）：檔案共 3 列（普高 2 · 綜高 1）。新增 2、更新 1、不變 0。/)).toBeInTheDocument();
    expect(section.getByText("d".repeat(64))).toBeInTheDocument();
    const dry = api.callsTo("POST", "/api/admin/knowledge/import")[0];
    expect(dry?.url).toContain("dry_run=true");
    expect(dry?.headers["Content-Type"]).toBe("text/csv");
    expect(dry?.rawBody).toBe(csv);

    const before = api.callsTo("GET", "/api/admin/knowledge/summary").length;
    await user.click(section.getByRole("button", { name: "確認匯入" }));
    expect(await section.findByText(/已匯入：新增 2、更新 1、不變 0。/)).toBeInTheDocument();
    const commit = api.callsTo("POST", "/api/admin/knowledge/import")[1];
    expect(commit?.url).not.toContain("dry_run");
    expect(commit?.rawBody).toBe(csv);
    // the status and the lists reload
    await waitFor(() => expect(api.callsTo("GET", "/api/admin/knowledge/summary").length).toBeGreaterThan(before));
    expect(section.queryByRole("button", { name: "確認匯入" })).not.toBeInTheDocument();
  });

  it("offers nothing to confirm when the file changes nothing, and shows server errors", async () => {
    signIn();
    let fail = false;
    mockApi({
      ...knowledgeRoutes(),
      "POST /api/admin/vocabulary/import": () =>
        fail
          ? respond(400, { error: { code: "bad_request", message: "第 3 列的 level 需為 1–6（kind 為 appendix 時留空）。" } })
          : importResult({ rows: 14, inserted: 0, updated: 0, unchanged: 14, by_track: undefined, by_level: { "1": 13, appendix: 1 } }),
    });
    const user = userEvent.setup();
    renderApp("/knowledge");
    const section = within(await screen.findByRole("region", { name: "匯入詞彙表" }));
    const file = new File(["word\n"], "v.csv", { type: "text/csv" });
    await user.upload(section.getByLabelText("選擇 CSV 檔案"), file);
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByText(/第 1 級 13 · 附錄 1/)).toBeInTheDocument();
    expect(section.getByText(/檔案內容和目前資料相同，不需要匯入。/)).toBeInTheDocument();
    expect(section.queryByRole("button", { name: "確認匯入" })).not.toBeInTheDocument();
    await user.click(section.getByRole("button", { name: "關閉預覽" }));

    fail = true;
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByRole("alert")).toHaveTextContent("第 3 列的 level 需為 1–6");
  });

  it("retires the demo items only after showing what that leaves", async () => {
    signIn();
    const api = mockApi({
      ...knowledgeRoutes(pi),
      "POST /api/admin/knowledge/retire-demo": (call: MockCall) => ({
        dry_run: call.url.includes("dry_run=true"),
        demo_active: 16,
        demo_by_category: { grammar: 7, siop: 2 },
        real_active: 6,
        categories_left_empty: ["grammar", "siop"],
        retired: call.url.includes("dry_run=true") ? 0 : 16,
      }),
    });
    const user = userEvent.setup();
    renderApp("/knowledge");
    await user.click(await screen.findByRole("button", { name: "停用示範條目" }));
    expect(await screen.findByText(/將停用 16 條示範條目（文法 7、SIOP 2）/)).toBeInTheDocument();
    expect(screen.getByText(/文法、SIOP類別將沒有任何條目/)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/knowledge/retire-demo")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "確認停用" }));
    expect(await screen.findByText("已停用 16 條示範條目。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/knowledge/retire-demo")[1]?.url).toContain("dry_run=false");
  });
});
