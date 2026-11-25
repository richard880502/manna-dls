// 掃描檔與 OCR (docs/API.md「Phase 2.10」): Manna reading a scan page by page in the viewer and after
// an upload (asked for again every few seconds, then the text), 用 OCR 重新讀取 with the server's
// 409 and 503 words, the 辨識中／辨識失敗 badges and 選用 held back on cards, the admin rows, a prep turn
// waiting for its sample's OCR, and the create page's 409 said beside 產生.
// Synthetic materials only: made-up titles, a sentence of text.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  AdminLibraryMaterial,
  AdminLibraryMaterialRow,
  MaterialCard,
  MaterialDetail,
  MaterialOcr,
  PrepSession,
  PrepTurn,
} from "../api/types";
import { adminLibraryConfig } from "../components/adminLibrary/prefill";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { ocrConfig } from "../lib/ocr";
import { prepConfig } from "../lib/prep";
import {
  baseRoutes,
  emptyLibrary,
  materialBrowse,
  materialCard,
  mockApi,
  renderApp,
  respond,
  signIn,
  teacher,
  type MockCall,
} from "../test/mockApi";

const TEXT = "Bees carry pollen from flower to flower.";
const BUSY = "這份檔案正在辨識文字（第 2／5 頁），完成後再試一次。";
const OFF = "這個環境沒有開啟文字辨識（OCR）。";

beforeEach(() => {
  ocrConfig.pollMs = 20;
  adminLibraryConfig.pollMs = 20;
  prepConfig.pollMs = 20;
});

afterEach(() => {
  ocrConfig.pollMs = 3000;
  adminLibraryConfig.pollMs = 3000;
  prepConfig.pollMs = 2000;
});

// ---------------------------------------------------------------- fixtures

const ocr = (status: MaterialOcr["status"], done_pages = 0, total_pages = 0, error: string | null = null): MaterialOcr => ({
  status,
  done_pages,
  total_pages,
  error,
});

/** A scanned PDF as the server returns it (no text yet unless given). */
const scan = (overrides: Partial<MaterialDetail> = {}): MaterialDetail => ({
  id: 9,
  kind: "document",
  origin: "upload",
  title: "Bees scan",
  source_note: "",
  url: null,
  filename: "bees-scan.pdf",
  content_type: "application/pdf",
  size_bytes: 4096,
  sha256: "cd".repeat(32),
  has_file: true,
  text_status: "no_text",
  text_truncated: false,
  page_count: 4,
  paragraph_count: 0,
  word_count: 0,
  revision: 1,
  created_at: "2026-11-16T08:00:00Z",
  updated_at: "2026-11-16T08:00:00Z",
  lessons: [],
  worksheet_count: 0,
  annotation_count: 0,
  scope: "personal",
  purpose: "material",
  category: "",
  tags: [],
  ocr: ocr("none"),
  text_source: "file",
  text: "",
  paragraphs: [],
  ...overrides,
});

/** The same scan once OCR read it. */
const read = (overrides: Partial<MaterialDetail> = {}): MaterialDetail =>
  scan({
    text_status: "ready",
    text_source: "ocr",
    paragraph_count: 1,
    word_count: 7,
    revision: 2,
    text: TEXT,
    paragraphs: [{ id: "p1", no: 1, text: TEXT, page: 1 }],
    ocr: ocr("done", 4, 4),
    ...overrides,
  });

/** Routes for one material in the viewer, its file link and its notes. */
function viewerRoutes(id: number, material: () => MaterialDetail, extra: Record<string, unknown> = {}) {
  return {
    [`GET /api/materials/${id}`]: material,
    [`GET /api/materials/${id}/annotations`]: [],
    [`POST /api/materials/${id}/file-link`]: { url: `/api/materials/${id}/file?t=1.1.x`, expires_at: "2026-11-16T10:00:00Z" },
    ...extra,
  };
}

const pause = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)));

// ---------------------------------------------------------------- the viewer

describe("the material viewer while Manna reads a scan", () => {
  it("shows the page being read with a bar, asks again every few seconds, then shows the text and stops", async () => {
    signIn();
    let server = scan({ ocr: ocr("running", 1, 4) });
    const api = mockApi(viewerRoutes(9, () => server));
    const gets = () => api.callsTo("GET", "/api/materials/9").length;
    render(<MaterialViewer mode="pane" materialId={9} />);

    expect(await screen.findByText("Manna 正在辨識掃描檔（第 2／4 頁）…")).toBeInTheDocument();
    const bar = screen.getByRole("progressbar", { name: "辨識進度" });
    expect(bar).toHaveAttribute("max", "4");
    expect(bar).toHaveAttribute("value", "1");
    // While it is being read, 「沒有可以選取的文字」 would soon be wrong, and there is nothing to start.
    expect(screen.queryByText(/沒有可以選取的文字/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "用 OCR 重新讀取" })).not.toBeInTheDocument();

    await waitFor(() => expect(gets()).toBeGreaterThanOrEqual(3));
    server = scan({ ocr: ocr("running", 3, 4) });
    expect(await screen.findByText("Manna 正在辨識掃描檔（第 4／4 頁）…")).toBeInTheDocument();

    server = read();
    expect(await screen.findByText(TEXT)).toBeInTheDocument();
    expect(screen.getByText("由 OCR 辨識")).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新辨識" })).toBeInTheDocument();
    // Done: no more asking.
    const settled = gets();
    await pause(120);
    expect(gets()).toBe(settled);
    // The notes are placed against the new text.
    expect(api.callsTo("GET", "/api/materials/9/annotations").length).toBeGreaterThanOrEqual(2);
  });

  it("before the number of pages is known, says so without pages and with a moving bar", async () => {
    signIn();
    mockApi(viewerRoutes(9, () => scan({ ocr: ocr("queued", 0, 0) })));
    render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByText("Manna 正在辨識掃描檔…")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "辨識進度" })).not.toHaveAttribute("value");
  });

  it("says a partial run's note quietly", async () => {
    signIn();
    mockApi(viewerRoutes(9, () => read({ ocr: ocr("done", 4, 4, "第 2 頁沒有辨識成功。") })));
    render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByText("第 2 頁沒有辨識成功。")).toHaveClass("quiet-note");
    expect(screen.getByText("由 OCR 辨識")).toBeInTheDocument();
  });

  it("says why a run failed and starts it again with 再試一次", async () => {
    signIn();
    let server = scan({ ocr: ocr("failed", 4, 4, "文字辨識中斷了。") });
    const api = mockApi(
      viewerRoutes(9, () => server, {
        "POST /api/materials/9/ocr": () => {
          server = scan({ ocr: ocr("queued", 0, 4) });
          return respond(202, server);
        },
      }),
    );
    const user = userEvent.setup();
    render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByText("辨識沒有成功：文字辨識中斷了。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "用 OCR 重新讀取" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "再試一次" }));
    expect(await screen.findByText("Manna 正在辨識掃描檔（第 1／4 頁）…")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/materials/9/ocr")).toHaveLength(1);
    expect(screen.queryByText(/辨識沒有成功/)).not.toBeInTheDocument();
  });
});

describe("用 OCR 重新讀取", () => {
  it("is offered on a PDF with text; 503 and 409 say the server's words, and 409 shows the run already going", async () => {
    signIn();
    let server = read({ text_source: "file", ocr: ocr("none") });
    let answer: unknown = respond(503, { error: { code: "unavailable", message: OFF } });
    const api = mockApi(viewerRoutes(9, () => server, { "POST /api/materials/9/ocr": () => answer }));
    const user = userEvent.setup();
    render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByText(TEXT)).toBeInTheDocument();
    expect(screen.queryByText("由 OCR 辨識")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "用 OCR 重新讀取" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(OFF);
    // Still offered: nothing started.
    expect(screen.getByRole("button", { name: "用 OCR 重新讀取" })).toBeEnabled();

    answer = respond(409, { error: { code: "conflict", message: BUSY } });
    server = read({ text_source: "file", ocr: ocr("running", 1, 5) });
    await user.click(screen.getByRole("button", { name: "用 OCR 重新讀取" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(BUSY);
    expect(await screen.findByText("Manna 正在辨識掃描檔（第 2／5 頁）…")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/materials/9/ocr")).toHaveLength(2);
    // Once that run is done, its 409 words go too.
    server = read({ ocr: ocr("done", 5, 5) });
    expect(await screen.findByText("由 OCR 辨識")).toBeInTheDocument();
    expect(screen.queryByText(BUSY)).not.toBeInTheDocument();
  });

  it("is 重新辨識 once the text came from OCR, and starts a run", async () => {
    signIn();
    let server = read();
    const api = mockApi(
      viewerRoutes(9, () => server, {
        "POST /api/materials/9/ocr": () => {
          server = read({ ocr: ocr("queued", 0, 4) });
          return respond(202, server);
        },
      }),
    );
    const user = userEvent.setup();
    render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByText("由 OCR 辨識")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "用 OCR 重新讀取" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "重新辨識" }));
    expect(await screen.findByText("Manna 正在辨識掃描檔（第 1／4 頁）…")).toBeInTheDocument();
    // The old text stays readable meanwhile.
    expect(screen.getByText(TEXT)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/materials/9/ocr")).toHaveLength(1);
  });

  it("is offered on an image, and on a scan without text says it can be used", async () => {
    signIn();
    mockApi({
      ...viewerRoutes(9, () => scan()),
      ...viewerRoutes(10, () =>
        scan({ id: 10, kind: "image", filename: "page.jpg", content_type: "image/jpeg", text_status: "not_applicable" }),
      ),
    });
    const first = render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByText(/沒有可以選取的文字（可能是掃描檔）.*可以按「用 OCR 重新讀取」讓 Manna 辨識文字/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "用 OCR 重新讀取" })).toBeInTheDocument();
    first.unmount();
    render(<MaterialViewer mode="pane" materialId={10} />);
    expect(await screen.findByRole("button", { name: "用 OCR 重新讀取" })).toBeInTheDocument();
  });

  it("is not offered on a Word file, nor on a platform material to a teacher", async () => {
    signIn();
    mockApi({
      ...viewerRoutes(11, () =>
        read({ id: 11, filename: "notes.docx", content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", text_source: "file" }),
      ),
      ...viewerRoutes(12, () => read({ id: 12, scope: "platform", text_source: "file" })),
    });
    const first = render(<MaterialViewer mode="pane" materialId={11} />);
    expect(await screen.findByText(TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "用 OCR 重新讀取" })).not.toBeInTheDocument();
    first.unmount();
    render(<MaterialViewer mode="pane" materialId={12} />);
    expect(await screen.findByText(TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "用 OCR 重新讀取" })).not.toBeInTheDocument();
  });

  it("is offered to a kb_manager on 平台教材's platform material", async () => {
    signIn();
    const platform = read({ id: 31, title: "114 學測英文（掃描）", scope: "platform", text_source: "file" });
    mockApi({
      ...baseRoutes(),
      "GET /api/me": { ...teacher, role: "kb_manager" },
      "GET /api/browse/materials": materialBrowse([
        materialCard({ id: 31, title: platform.title, scope: "platform", file_type: "pdf", owner_label: "Manna 知識庫" }),
      ]),
      ...viewerRoutes(31, () => platform),
    });
    const user = userEvent.setup();
    renderApp("/reference");
    const card = within(await screen.findByRole("article", { name: platform.title }));
    await user.click(card.getByRole("button", { name: `預覽「${platform.title}」` }));
    const viewer = within(await screen.findByRole("complementary", { name: "教材" }));
    expect(await viewer.findByRole("button", { name: "用 OCR 重新讀取" })).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- uploads

describe("an upload whose text Manna is reading", () => {
  it("上傳檔案 shows the progress, asks again, and attaches the material once its text is there", async () => {
    signIn();
    let server = scan({ id: 6, ocr: ocr("queued", 0, 2) });
    const api = mockApi({
      ...baseRoutes(),
      "POST /api/materials/upload": () => server,
      "GET /api/materials/6": () => server,
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/create/comprehension");
    await user.click(await screen.findByRole("button", { name: "上傳檔案" }));
    const dialog = within(await screen.findByRole("dialog", { name: "上傳檔案" }));
    await user.upload(await dialog.findByLabelText("選擇檔案"), new File(["%PDF-1.4"], "bees-scan.pdf", { type: "application/pdf" }));
    await user.click(dialog.getByRole("button", { name: "上傳" }));

    expect(await dialog.findByText("Manna 正在辨識掃描檔（第 1／2 頁）…")).toBeInTheDocument();
    expect(dialog.getByRole("progressbar", { name: "辨識進度" })).toBeInTheDocument();
    // Not taken (nor refused as unreadable) while it is being read.
    expect(dialog.queryByText(/讀不到文字/)).not.toBeInTheDocument();
    await waitFor(() => expect(api.callsTo("GET", "/api/materials/6").length).toBeGreaterThanOrEqual(2));

    server = read({ id: 6 });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "移除「Bees scan」" })).toBeInTheDocument();
    const settled = api.callsTo("GET", "/api/materials/6").length;
    await pause(120);
    expect(api.callsTo("GET", "/api/materials/6").length).toBe(settled);
  });

  it("上傳檔案 says when the reading failed, and 再試一次 starts it again", async () => {
    signIn();
    let server = scan({ id: 6, ocr: ocr("queued", 0, 2) });
    const api = mockApi({
      ...baseRoutes(),
      "POST /api/materials/upload": () => server,
      "GET /api/materials/6": () => server,
      "POST /api/materials/6/ocr": () => {
        server = scan({ id: 6, ocr: ocr("queued", 0, 2) });
        return respond(202, server);
      },
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/create/comprehension");
    await user.click(await screen.findByRole("button", { name: "上傳檔案" }));
    const dialog = within(await screen.findByRole("dialog", { name: "上傳檔案" }));
    await user.upload(await dialog.findByLabelText("選擇檔案"), new File(["%PDF-1.4"], "bees-scan.pdf", { type: "application/pdf" }));
    await user.click(dialog.getByRole("button", { name: "上傳" }));
    expect(await dialog.findByText("Manna 正在辨識掃描檔（第 1／2 頁）…")).toBeInTheDocument();
    server = scan({ id: 6, ocr: ocr("failed", 2, 2, "兩頁都沒有辨識成功。") });
    expect(await dialog.findByText("辨識沒有成功：兩頁都沒有辨識成功。")).toBeInTheDocument();
    await user.click(dialog.getByRole("button", { name: "再試一次" }));
    expect(await dialog.findByText("Manna 正在辨識掃描檔（第 1／2 頁）…")).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/materials/6/ocr")).toHaveLength(1);
  });

  it("a worksheet sample (照我的學習單做) is taken at once: the server waits for its OCR", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "POST /api/materials/upload": scan({ id: 6, title: "期中學習單", purpose: "worksheet_sample", ocr: ocr("queued", 0, 3) }),
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/create/mimic");
    const region = within(await screen.findByRole("region", { name: "上傳學習單" }));
    await user.upload(await region.findByLabelText("選擇檔案"), new File(["%PDF-1.4"], "midterm.pdf", { type: "application/pdf" }));
    await user.click(region.getByRole("button", { name: "上傳" }));
    expect(await screen.findByRole("button", { name: "移除「期中學習單」" })).toBeInTheDocument();
  });

  it("我的教材 shows the progress under the notice, then says the text is ready", async () => {
    signIn();
    let server = scan({ id: 6, ocr: ocr("running", 0, 3) });
    mockApi({
      ...baseRoutes(),
      "POST /api/materials/upload": () => server,
      "GET /api/materials/6": () => server,
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/materials");
    const section = within(await screen.findByRole("region", { name: "我的教材" }));
    // Enabled once the library (limits, usage) is in.
    await waitFor(() => expect(section.getByRole("button", { name: "上傳檔案" })).toBeEnabled());
    await user.click(section.getByRole("button", { name: "上傳檔案" }));
    // The dialog is drawn on <body> (a portal), outside the section.
    const form = within(await screen.findByRole("form", { name: "上傳檔案" }));
    await user.upload(form.getByLabelText("選擇檔案"), new File(["%PDF-1.4"], "bees-scan.pdf", { type: "application/pdf" }));
    await user.click(form.getByRole("button", { name: "上傳" }));
    expect(await section.findByText("已加入「Bees scan」。")).toBeInTheDocument();
    expect(section.getByText("Manna 正在辨識掃描檔（第 1／3 頁）…")).toBeInTheDocument();
    server = read({ id: 6, ocr: ocr("done", 3, 3, "第 3 頁沒有辨識成功。") });
    expect(await section.findByText("「Bees scan」的文字辨識好了。第 3 頁沒有辨識成功。")).toBeInTheDocument();
    expect(section.queryByRole("progressbar")).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- cards

describe("cards while OCR reads them", () => {
  const reading = (c: Partial<MaterialCard> = {}) =>
    materialCard({
      id: 21,
      title: "Scanned reading",
      file_type: "pdf",
      kind: "document",
      excerpt: "",
      text_status: "no_text",
      paragraph_count: 0,
      word_count: 0,
      page_count: 3,
      ocr: ocr("running", 1, 3),
      ...c,
    });

  it("say 辨識中 n／m and hold 選用 back until the text is there; a failed one says 辨識失敗", async () => {
    signIn();
    let cards = [reading(), reading({ id: 22, title: "Blurry photo", file_type: "image", kind: "image", text_status: "not_applicable", ocr: ocr("failed", 1, 1, "沒有找到文字。") })];
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/browse/materials": () => materialBrowse(cards),
    });
    const user = userEvent.setup();
    renderApp("/create/comprehension");
    await user.click(await screen.findByRole("button", { name: "從教材挑選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從教材挑選" }));
    const card = (title: string) => within(dialog.getByRole("article", { name: title }));

    expect(await card("Scanned reading").findByText("辨識中 2／3")).toBeInTheDocument();
    // The meta line does not call it unreadable while it is being read.
    expect(card("Scanned reading").getByText("PDF · 3 頁")).toBeInTheDocument();
    const choose = card("Scanned reading").getByRole("button", { name: "選用「Scanned reading」" });
    expect(choose).toBeDisabled();
    expect(choose).toHaveAttribute("title", "辨識完成後才能選用");
    expect(card("Blurry photo").getByText("辨識失敗")).toBeInTheDocument();
    expect(card("Blurry photo").getByRole("button", { name: "選用「Blurry photo」" })).toHaveAttribute(
      "title",
      "讀不到文字，不能出題",
    );

    // Asked for again while one is being read; done, the badge goes and 選用 is on.
    await waitFor(() => expect(api.callsTo("GET", "/api/browse/materials").length).toBeGreaterThanOrEqual(3));
    cards = [
      reading({ text_status: "ready", text_source: "ocr", paragraph_count: 3, word_count: 120, excerpt: TEXT, ocr: ocr("done", 3, 3) }),
      cards[1] as MaterialCard,
    ];
    await waitFor(() => expect(card("Scanned reading").getByRole("button", { name: "選用「Scanned reading」" })).toBeEnabled());
    expect(card("Scanned reading").queryByText(/辨識中/)).not.toBeInTheDocument();
    const settled = api.callsTo("GET", "/api/browse/materials").length;
    await pause(120);
    expect(api.callsTo("GET", "/api/browse/materials").length).toBe(settled);
  });

  it("用這篇出題 is off on 我的教材 while a card is being read", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/browse/materials": materialBrowse([reading()]) });
    renderApp("/materials");
    const card = within(await screen.findByRole("article", { name: "Scanned reading" }));
    const use = card.getByRole("button", { name: "用這篇出題「Scanned reading」" });
    expect(use).toBeDisabled();
    expect(use).toHaveAttribute("title", "辨識完成後才能選用");
  });
});

// ---------------------------------------------------------------- admin rows

describe("教材與範本管理: knowledge-base uploads being read", () => {
  const LIB = "/api/admin/library";
  const pi = { ...teacher, research_id: "PI-01", role: "pi" as const };
  const adminCard = (overrides: Partial<AdminLibraryMaterial> = {}): AdminLibraryMaterial => ({
    id: 101,
    title: "114學測英文_試題",
    scope: "platform",
    category: "學測英文",
    tags: ["114"],
    kind: "document",
    file_type: "pdf",
    page_count: 3,
    paragraph_count: 0,
    word_count: 0,
    excerpt: "",
    text_status: "no_text",
    looks_like_worksheet: false,
    created_at: "2026-11-16T02:00:00Z",
    owner_label: "Manna 知識庫",
    ocr: ocr("queued", 0, 3),
    text_source: "file",
    ...overrides,
  });
  const row = (overrides: Partial<AdminLibraryMaterial> = {}): AdminLibraryMaterialRow => ({
    ...adminCard(overrides),
    uploaded_by: "PI-01",
    size_bytes: 2048,
  });
  const isMaterials = (call: MockCall) => !call.url.includes("kind=templates");

  it("upload rows and list rows say 辨識中 n／m, asked for again until done", async () => {
    signIn();
    let server: AdminLibraryMaterialRow[] = [];
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      [`GET ${LIB}`]: (call: MockCall) => ({ items: isMaterials(call) ? server : [], next_cursor: null }),
      [`POST ${LIB}`]: () => {
        server = [row()];
        return adminCard();
      },
    });
    const gets = () => api.callsTo("GET", LIB).filter(isMaterials).length;
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/admin/library");
    const tab = within(await screen.findByRole("tabpanel", { name: "知識庫教材" }));
    await user.upload(tab.getByLabelText("選擇檔案"), [new File(["%PDF-1.4"], "114學測英文_試題.pdf")]);
    await user.click(tab.getByRole("button", { name: "全部上傳" }));
    const upload = within(await screen.findByRole("group", { name: "114學測英文_試題.pdf" }));
    expect(await upload.findByText("已加入 · 辨識中 1／3")).toBeInTheDocument();

    const list = within(await screen.findByRole("list", { name: "知識庫教材" }));
    expect(await list.findByText("辨識中 1／3")).toBeInTheDocument();
    // Not 「讀不到文字」 while it is being read.
    expect(list.queryByText(/讀不到文字/)).not.toBeInTheDocument();
    await waitFor(() => expect(gets()).toBeGreaterThanOrEqual(3));

    server = [row({ ocr: ocr("running", 2, 3) })];
    expect(await list.findByText("辨識中 3／3")).toBeInTheDocument();
    expect(await upload.findByText("已加入 · 辨識中 3／3")).toBeInTheDocument();

    server = [row({ text_status: "ready", text_source: "ocr", paragraph_count: 12, word_count: 900, ocr: ocr("done", 3, 3) })];
    expect(await upload.findByText("已加入 · PDF · 3 頁 · 約 900 字")).toBeInTheDocument();
    await waitFor(() => expect(list.queryByText(/辨識中/)).not.toBeInTheDocument());
    const settled = gets();
    await pause(120);
    expect(gets()).toBe(settled);
  });

  it("a failed reading says so on its row", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/me": pi,
      [`GET ${LIB}`]: (call: MockCall) => ({
        items: isMaterials(call) ? [row({ ocr: ocr("failed", 3, 3, "三頁都沒有辨識成功。") })] : [],
        next_cursor: null,
      }),
    });
    renderApp("/admin/library");
    const list = within(await screen.findByRole("list", { name: "知識庫教材" }));
    expect(await list.findByText("辨識沒有成功：三頁都沒有辨識成功。")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- prep turn

describe("a 照我的學習單做 turn waiting for its sample's OCR", () => {
  const S7 = "/api/prep-sessions/7";
  const wsTurn = (overrides: Partial<PrepTurn> = {}): PrepTurn => ({
    id: 301,
    seq: 1,
    kind: "worksheet",
    request: "學習單",
    status: "running",
    error: null,
    worksheet: null,
    template: { id: 12, title: "期中學習單", status: "extracting" },
    result: null,
    interaction_id: null,
    created_at: "2026-11-16T08:00:00Z",
    finished_at: null,
    ...overrides,
  });
  const session = (turn: PrepTurn): PrepSession => ({
    id: 7,
    title: "Bees",
    material: { id: 5, title: "Bees" },
    topic: null,
    class: null,
    template: { id: 12, title: "期中學習單" },
    turn_count: 1,
    last_kind: "worksheet",
    updated_at: "2026-11-16T08:00:00Z",
    created_at: "2026-11-16T08:00:00Z",
    turns: [turn],
    running: turn.status === "running",
    next: [],
  });

  it("says which page is being read first, then that Manna reads the design", async () => {
    signIn();
    let current = session(wsTurn({ phase: "ocr", ocr: { done_pages: 2, total_pages: 5 } }));
    mockApi({ ...baseRoutes(), [`GET ${S7}`]: () => current });
    renderApp("/prep/7");
    expect(await screen.findByText("Manna 正在辨識掃描檔（第 3／5 頁），接著讀學習單設計…")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "辨識進度" })).toHaveAttribute("value", "2");
    expect(screen.queryByText("Manna 正在讀你的學習單設計…（約 20 秒）")).not.toBeInTheDocument();

    current = session(wsTurn());
    expect(await screen.findByText("Manna 正在讀你的學習單設計…（約 20 秒）")).toBeInTheDocument();
    expect(screen.queryByText(/正在辨識掃描檔/)).not.toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- create page 409

describe("產生 while the chosen material is still being read", () => {
  it("says the server's 409 beside 產生, not as a general error, until the material changes", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": emptyLibrary(),
      "GET /api/browse/materials": materialBrowse([materialCard({ id: 5, title: "The Umbrella Stand" })]),
      "POST /api/prep-sessions": respond(409, { error: { code: "conflict", message: BUSY } }),
    });
    const user = userEvent.setup();
    renderApp("/create/comprehension");
    await user.click(await screen.findByRole("button", { name: "從教材挑選" }));
    const dialog = within(await screen.findByRole("dialog", { name: "從教材挑選" }));
    await user.click(await dialog.findByRole("button", { name: "選用「The Umbrella Stand」" }));
    await user.click(screen.getByRole("button", { name: "產生" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(BUSY);
    expect(alert.closest(".create-bar")).not.toBeNull();
    expect(screen.getAllByText(BUSY)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "產生" })).toHaveAccessibleDescription(BUSY);
    expect(api.callsTo("POST", "/api/prep-sessions")).toHaveLength(1);

    // Another material: the line goes.
    await user.click(screen.getByRole("button", { name: "移除「The Umbrella Stand」" }));
    expect(screen.queryByText(BUSY)).not.toBeInTheDocument();
  });

  it("用這篇出題 on a material still being read says so instead of filling the slot", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/materials/9": scan({ ocr: ocr("running", 1, 5) }),
    });
    renderApp("/create/comprehension?material=9");
    expect(await screen.findByText(BUSY)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "移除「Bees scan」" })).not.toBeInTheDocument();
  });
});
