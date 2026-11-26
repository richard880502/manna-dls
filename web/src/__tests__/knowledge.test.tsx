import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type {
  EmbeddingStatus,
  KnowledgeImportResult,
  KnowledgeItemRow,
  KnowledgeSummary,
  MaterialDetail,
  VocabularyRow,
} from "../api/types";
import { can } from "../lib/permissions";
import {
  baseRoutes,
  materialBrowse,
  materialCard,
  mockApi,
  renderApp,
  respond,
  signIn,
  teacher,
  type MockCall,
} from "../test/mockApi";
import { getMeta, itemTexts } from "../test/metaList";

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

const embeddings = (overrides: Partial<EmbeddingStatus> = {}): EmbeddingStatus => ({
  enabled: true,
  provider: "openai_compatible",
  model: "test-embedding-model",
  dims: 768,
  active_items: 1352,
  embedded: 1352,
  other_model: 0,
  running: false,
  job: {
    id: 1,
    trigger: "import",
    model: "test-embedding-model",
    dims: 768,
    status: "done",
    total: 1352,
    embedded: 1352,
    error: null,
    created_at: "2026-11-15T02:00:00Z",
    finished_at: "2026-11-15T02:01:00Z",
  },
  ...overrides,
});

const knowledgeRoutes = (me: typeof manager | typeof pi = manager) => ({
  ...baseRoutes(),
  "GET /api/me": me,
  "GET /api/admin/knowledge/summary": summary(),
  "GET /api/admin/knowledge/embeddings": embeddings(),
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

describe("知識庫：課綱條目與詞彙", () => {
  it("is offered to a 知識庫管理員, who does not get 後台管理", async () => {
    signIn();
    mockApi(knowledgeRoutes());
    renderApp("/");
    expect(await screen.findByRole("link", { name: "知識庫" })).toHaveAttribute("href", "/knowledge");
    expect(screen.queryByRole("link", { name: "後台管理" })).not.toBeInTheDocument();
  });

  it("is hidden from teachers; the direct URL explains instead of loading anything", async () => {
    signIn();
    const api = mockApi(baseRoutes());
    renderApp("/knowledge");
    expect(await screen.findByRole("heading", { name: "知識庫", level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/只開放給計畫主持人和負責知識庫的帳號/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "到平台教材" })).toHaveAttribute("href", "/reference");
    expect(screen.queryByRole("link", { name: "知識庫" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("選擇 CSV 檔案")).not.toBeInTheDocument();
    expect(api.calls.some((c) => c.path.startsWith("/api/admin/"))).toBe(false);
    expect(api.callsTo("GET", "/api/browse/materials")).toHaveLength(0);
  });

  it("shows what is in the knowledge base and lists items and words read-only", async () => {
    signIn();
    const api = mockApi(knowledgeRoutes(pi));
    const user = userEvent.setup();
    const { router } = renderApp("/knowledge");

    // The curriculum tools are the second tab; the address remembers it.
    await user.click(await screen.findByRole("tab", { name: "課綱條目與詞彙" }));
    expect(router.state.location.search).toBe("?tab=curriculum");
    const status = within(await screen.findByRole("list", { name: "目前內容" }));
    getMeta(["普高 4 條", "綜高 2 條"], document.body);
    expect(status.getByText("6 條")).toBeInTheDocument();
    getMeta(["TEST LIST v1：第 1 級 1 詞條", "附錄 1 詞"]);
    expect(status.getByText("16 條使用中")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "最近的匯入" })).getByText(/新增 6、更新 0、不變 0/)).toBeInTheDocument();

    const items = within(await screen.findByRole("list", { name: "條目" }));
    expect(items.getByRole("heading", { name: "◎3-Ⅴ-2 學習表現｜測試閱讀" })).toBeInTheDocument();
    expect(items.getByText("能讀懂測試用的短文並找出主旨。")).toBeInTheDocument();
    expect(items.getByText("測試領綱.pdf（測試版） 第 12 頁")).toBeInTheDocument();
    expect(items.getByText("第 1 版")).toBeInTheDocument();
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
    renderApp("/knowledge?tab=curriculum");

    const section = within(await screen.findByRole("region", { name: "匯入課綱條目" }));
    // no file yet
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByText("請先選擇 CSV 檔案。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/knowledge/import")).toHaveLength(0);

    const csv = "item_code,category,school_track,title,content,source_doc,source_version,source_page\nA,curriculum,general,t,c,d,v,1\n";
    await user.upload(section.getByLabelText("選擇 CSV 檔案"), new File([csv], "kb.csv", { type: "text/csv" }));
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByText(/預覽（尚未寫入）：檔案共 3 列（普高 2、綜高 1）。新增 2、更新 1、不變 0。/)).toBeInTheDocument();
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
    renderApp("/knowledge?tab=curriculum");
    const section = within(await screen.findByRole("region", { name: "匯入詞彙表" }));
    const file = new File(["word\n"], "v.csv", { type: "text/csv" });
    await user.upload(section.getByLabelText("選擇 CSV 檔案"), file);
    await user.click(section.getByRole("button", { name: "預覽" }));
    expect(await section.findByText(/第 1 級 13、附錄 1/)).toBeInTheDocument();
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
    renderApp("/knowledge?tab=curriculum");
    await user.click(await screen.findByRole("button", { name: "停用示範條目" }));
    expect(await screen.findByText(/將停用 16 條示範條目（文法 7、SIOP 2）/)).toBeInTheDocument();
    expect(screen.getByText(/文法、SIOP類別將沒有任何條目/)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/knowledge/retire-demo")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "確認停用" }));
    expect(await screen.findByText("已停用 16 條示範條目。")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/admin/knowledge/retire-demo")[1]?.url).toContain("dry_run=false");
  });

  it("shows the semantic index coverage", async () => {
    signIn();
    mockApi(knowledgeRoutes());
    renderApp("/knowledge?tab=curriculum");
    const index = within(await screen.findByRole("list", { name: "語意索引" }));
    expect(index.getByRole("heading", { name: "1,352 條中 1,352 條已建立語意索引" })).toBeInTheDocument();
    expect(index.getByText(/test-embedding-model（768 維）/)).toBeInTheDocument();
    // fully indexed: only a quiet re-check
    expect(index.getByRole("button", { name: "重新檢查" })).toBeInTheDocument();
  });

  it("builds the missing part of the index and follows its progress", async () => {
    signIn();
    let state = embeddings({
      embedded: 1000,
      job: { ...embeddings().job!, status: "failed", total: 1352, embedded: 1000, error: "embedding request timed out" },
    });
    const api = mockApi({
      ...knowledgeRoutes(pi),
      "GET /api/admin/knowledge/embeddings": () => state,
      "POST /api/admin/knowledge/embeddings/rebuild": () => {
        state = embeddings({
          embedded: 1000,
          running: true,
          job: { ...embeddings().job!, trigger: "manual", status: "running", total: 352, embedded: 32, finished_at: null },
        });
        return { ...state, started: true };
      },
    });
    const user = userEvent.setup();
    renderApp("/knowledge?tab=curriculum");
    const region = within(await screen.findByRole("region", { name: "語意索引" }));
    expect(await region.findByText(/1,352 條中 1,000 條已建立語意索引/)).toBeInTheDocument();
    expect(region.getByText(/上次建立在 1,000 \/ 1,352 時中斷（embedding request timed out）/)).toBeInTheDocument();

    await user.click(region.getByRole("button", { name: "建立語意索引" }));
    expect(api.callsTo("POST", "/api/admin/knowledge/embeddings/rebuild")[0]?.body).toEqual({ force: false });
    expect(await region.findByText("建立中：32 / 352")).toBeInTheDocument();
    expect(region.getByRole("button", { name: "建立中…" })).toBeDisabled();

    // the page polls until the run is over
    state = embeddings();
    expect(await region.findByText(/1,352 條中 1,352 條已建立語意索引/, {}, { timeout: 4000 })).toBeInTheDocument();
    expect(region.getByRole("button", { name: "重新檢查" })).toBeEnabled();
  });

  it("says retrieval is keyword-only when no embedding model is set", async () => {
    signIn();
    mockApi({
      ...knowledgeRoutes(),
      "GET /api/admin/knowledge/embeddings": embeddings({
        enabled: false,
        provider: "none",
        model: null,
        dims: null,
        embedded: 0,
        job: null,
      }),
    });
    renderApp("/knowledge?tab=curriculum");
    const region = within(await screen.findByRole("region", { name: "語意索引" }));
    expect(await region.findByText("尚未設定語意索引模型，目前只用關鍵字檢索。")).toBeInTheDocument();
    expect(region.queryByRole("button")).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- 知識庫：教材牆

// Synthetic cards and text: not real exam papers.
const paperPdf = materialCard({
  id: 7,
  title: "114 學測英文（測試）",
  scope: "platform",
  category: "學測英文",
  tags: ["114", "試題", "測試", "閱讀"],
  kind: "document",
  file_type: "pdf",
  page_count: 8,
  paragraph_count: 30,
  word_count: 2900,
  excerpt: "Part One. Bees do a much bigger job than most people think.",
  owner_label: "Manna 知識庫",
});
const answerDocx = materialCard({
  id: 8,
  title: "114 參考答案（測試）",
  scope: "platform",
  category: "統測英文",
  tags: ["參考答案"],
  file_type: "docx",
  excerpt: "1. B 2. C 3. A — 測試用的參考答案。",
  owner_label: "Manna 知識庫",
});
const photo = materialCard({
  id: 9,
  title: "黑板照片（測試）",
  scope: "platform",
  category: "其他",
  kind: "image",
  file_type: "image",
  paragraph_count: 0,
  word_count: 0,
  excerpt: "",
  text_status: "not_applicable",
  owner_label: "Manna 知識庫",
});
const scanning = materialCard({
  id: 10,
  title: "掃描的講義（測試）",
  scope: "platform",
  category: "其他",
  file_type: "pdf",
  page_count: 8,
  paragraph_count: 0,
  word_count: 0,
  excerpt: "",
  text_status: "no_text",
  ocr: { status: "running", done_pages: 1, total_pages: 8, error: null },
  owner_label: "Manna 知識庫",
});

const paperDetail = (): MaterialDetail => ({
  id: 7,
  kind: "document",
  origin: "upload",
  title: paperPdf.title,
  source_note: "",
  url: null,
  filename: "paper.pdf",
  content_type: "application/pdf",
  size_bytes: 4096,
  sha256: null,
  has_file: true,
  text_status: "ready",
  text_truncated: false,
  page_count: 8,
  paragraph_count: 2,
  word_count: 40,
  revision: 1,
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  lessons: [],
  worksheet_count: 0,
  annotation_count: 0,
  scope: "platform",
  text: "Part One.\n\nBees do a much bigger job than most people think.",
  paragraphs: [
    { id: "p1", no: 1, text: "Part One." },
    { id: "p2", no: 2, text: "Bees do a much bigger job than most people think." },
  ],
});

const BROWSE = "/api/browse/materials";

/**
 * The preview images: the ids in `ok` answer a small JPEG (shown as `blob:thumb-<id>`), the
 * others 404 like a material without one. Each request is still recorded by the mock API.
 */
function stubThumbnails(ok: number[]) {
  const mocked = globalThis.fetch;
  const urls = new Map<Blob, string>();
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const id = /\/api\/materials\/(\d+)\/thumbnail$/.exec(String(input))?.[1];
    if (id === undefined) return mocked(input, init);
    await mocked(input, init);
    if (!ok.includes(Number(id))) {
      return {
        ok: false,
        status: 404,
        text: async () => JSON.stringify({ error: { code: "not_found", message: "這份教材沒有預覽圖。" } }),
      } as unknown as Response;
    }
    const blob = new Blob(["jpeg"], { type: "image/jpeg" });
    urls.set(blob, `blob:thumb-${id}`);
    return { ok: true, status: 200, blob: async () => blob, headers: { get: () => null } } as unknown as Response;
  });
  vi.stubGlobal(
    "URL",
    Object.assign(URL, { createObjectURL: (b: Blob) => urls.get(b) ?? "blob:unknown", revokeObjectURL: vi.fn() }),
  );
}

const galleryCard = (title: string) => screen.getByRole("article", { name: title });
const thumbSrc = (title: string) => galleryCard(title).querySelector("img")?.getAttribute("src");

describe("知識庫：教材牆", () => {
  it("shows the platform's materials as cards with previews, and nothing on the page says 管理", async () => {
    signIn();
    const api = mockApi({
      ...knowledgeRoutes(manager),
      [`GET ${BROWSE}`]: materialBrowse([paperPdf, answerDocx, photo, scanning]),
    });
    stubThumbnails([7, 10]);
    const user = userEvent.setup();
    renderApp("/knowledge");

    const page = await screen.findByRole("region", { name: "知識庫" });
    expect(screen.getByRole("heading", { name: "知識庫", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "教材" })).toHaveAttribute("aria-selected", "true");
    expect(within(page).getByRole("link", { name: "上傳教材" })).toHaveAttribute("href", "/admin/library");
    const wall = within(await screen.findByRole("list", { name: "知識庫教材" }));
    expect(wall.getAllByRole("article")).toHaveLength(4);
    expect(page.textContent).not.toMatch(/管理/);
    expect(new URLSearchParams(api.callsTo("GET", BROWSE)[0]?.url.split("?")[1]).get("area")).toBe("reference");

    // A PDF shows its first page, asked for with the teacher's token.
    await waitFor(() => expect(thumbSrc(paperPdf.title)).toBe("blob:thumb-7"));
    expect(api.callsTo("GET", "/api/materials/7/thumbnail")[0]?.headers.Authorization).toBe("Bearer test-token");
    const pdf = within(galleryCard(paperPdf.title));
    expect(pdf.getByText("約 2,900 字")).toBeInTheDocument();
    getMeta(["學測英文", "PDF", "8 頁", "約 2,900 字"]);
    expect(pdf.getByText("+1")).toBeInTheDocument();
    // A Word file has no preview image: its first lines are the preview.
    expect(within(galleryCard(answerDocx.title)).getByText(answerDocx.excerpt)).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/materials/8/thumbnail")).toHaveLength(0);
    // A preview the server cannot make (404): the card falls back to its text, here none.
    expect(await within(galleryCard(photo.title)).findByText("沒有可預覽的文字")).toBeInTheDocument();
    expect(galleryCard(photo.title).querySelector("img")).toBeNull();
    // A scan being read says how far it got.
    expect(within(galleryCard(scanning.title)).getByText("辨識中 2／8")).toBeInTheDocument();
    await waitFor(() => expect(thumbSrc(scanning.title)).toBe("blob:thumb-10"));

    // Category chips and the search go to the server.
    await user.click(within(page).getByRole("button", { name: /^統測英文/ }));
    await waitFor(() =>
      expect(new URLSearchParams(api.callsTo("GET", BROWSE).at(-1)?.url.split("?")[1]).get("category")).toBe(
        "統測英文",
      ),
    );
    await user.type(within(page).getByRole("searchbox", { name: "搜尋知識庫" }), "bees{Enter}");
    await waitFor(() =>
      expect(new URLSearchParams(api.callsTo("GET", BROWSE).at(-1)?.url.split("?")[1]).get("q")).toBe("bees"),
    );
  });

  it("opens a card's preview: the first page beside the text read from it, read only", async () => {
    signIn();
    const api = mockApi({
      ...knowledgeRoutes(manager),
      [`GET ${BROWSE}`]: materialBrowse([paperPdf]),
      "GET /api/materials/7": paperDetail(),
      "POST /api/materials/7/file-link": { url: "/api/materials/7/file?t=abc", expires_at: "2026-11-16T10:00:00Z" },
    });
    stubThumbnails([7]);
    const user = userEvent.setup();
    renderApp("/knowledge");

    const open = await screen.findByRole("button", { name: paperPdf.title });
    await waitFor(() => expect(thumbSrc(paperPdf.title)).toBe("blob:thumb-7"));
    await user.click(open);

    const dialog = within(await screen.findByRole("dialog", { name: paperPdf.title }));
    expect(dialog.getByRole("img", { name: `「${paperPdf.title}」的第 1 頁` })).toHaveAttribute("src", "blob:thumb-7");
    expect(dialog.getByText("第 1 頁（共 8 頁）")).toBeInTheDocument();
    expect(itemTexts(document.querySelector(".kb-preview-meta")!)).toEqual(["學測英文", "PDF", "8 頁", "約 2,900 字"]);
    expect(dialog.getAllByRole("listitem").map((li) => li.textContent)).toEqual(expect.arrayContaining(["114", "閱讀"]));
    expect(await dialog.findByText("Bees do a much bigger job than most people think.")).toBeInTheDocument();
    expect(await dialog.findByRole("link", { name: "下載原檔" })).toHaveAttribute(
      "href",
      "/api/materials/7/file?t=abc&download=1",
    );
    // A preview, not a place for notes.
    expect(api.callsTo("GET", "/api/materials/7/annotations")).toHaveLength(0);

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(open).toHaveFocus();
  });

  it("says where to start when the knowledge base is empty", async () => {
    signIn();
    mockApi(knowledgeRoutes(pi));
    renderApp("/knowledge");
    expect(await screen.findByText("知識庫還沒有教材。按「上傳教材」放進第一份。")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "上傳教材" })).toHaveAttribute("href", "/admin/library");
  });
});
