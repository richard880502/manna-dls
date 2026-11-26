// 教材與註記 (docs/API.md Phase 2.2): 我的教材 (/materials; cards since the library contract: upload
// with the privacy warning, rename and delete with a confirmation), the material viewer with teacher
// annotations, and the viewer as a resizable, collapsible pane for the worksheet generator.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { Material, MaterialAnnotation, MaterialDetail, MaterialLibrary } from "../api/types";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { segmentsFor, snapToWords } from "../lib/materials";
import {
  baseRoutes,
  cardOf,
  materialBrowse,
  mockApi,
  renderApp,
  respond,
  signIn,
  type MockCall,
} from "../test/mockApi";
import { getMeta } from "../test/metaList";

const P1 = "Mrs. Lin put an umbrella stand outside her shop.";
const P2 = "In the first month, twelve umbrellas did disappear. However, something unexpected followed.";

const summary = (overrides: Partial<Material> = {}): Material => ({
  id: 5,
  kind: "text",
  origin: "paste",
  title: "The Umbrella Stand",
  source_note: "課本第三課",
  url: null,
  filename: null,
  content_type: null,
  size_bytes: 140,
  sha256: "ab".repeat(32),
  has_file: false,
  text_status: "ready",
  text_truncated: false,
  page_count: null,
  paragraph_count: 2,
  word_count: 24,
  revision: 1,
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  lessons: [{ lesson_id: 7, lesson_title: "第 2 節 精讀", unit_title: "L3" }],
  worksheet_count: 0,
  annotation_count: 1,
  ...overrides,
});

const detail = (overrides: Partial<MaterialDetail> = {}): MaterialDetail => ({
  ...summary(),
  text: `${P1}\n\n${P2}`,
  paragraphs: [
    { id: "p1", no: 1, text: P1 },
    { id: "p2", no: 2, text: P2 },
  ],
  ...overrides,
});

const library = (materials: Material[] = [summary()], overrides: Partial<MaterialLibrary> = {}): MaterialLibrary => ({
  materials,
  usage: { used_bytes: 3 * 1024 * 1024, quota_bytes: 1024 * 1024 * 1024, count: materials.length, max_count: 300 },
  limits: {
    files: [
      { ext: "txt", kind: "document", max_bytes: 20 * 1024 * 1024 },
      { ext: "docx", kind: "document", max_bytes: 20 * 1024 * 1024 },
      { ext: "pdf", kind: "document", max_bytes: 20 * 1024 * 1024 },
      { ext: "png", kind: "image", max_bytes: 10 * 1024 * 1024 },
      { ext: "mp4", kind: "video", max_bytes: 100 * 1024 * 1024 },
    ],
    max_pasted_chars: 50000,
    max_note_chars: 2000,
  },
  storage: "fs",
  uploads_enabled: true,
  ...overrides,
});

const annotation = (overrides: Partial<MaterialAnnotation> = {}): MaterialAnnotation => ({
  id: 31,
  material_id: 5,
  quote: "umbrella stand",
  note: "先確認學生知道這是什麼",
  share_with_ai: false,
  hidden: false,
  anchor: { status: "ok", paragraph_id: "p1", paragraph_no: 1, start: 16, end: 30 },
  created_at: "2026-11-14T08:00:00Z",
  updated_at: "2026-11-14T08:00:00Z",
  ...overrides,
});

async function openLibrary() {
  renderApp("/materials");
  return within(await screen.findByRole("region", { name: "我的教材" }));
}

/** 我的教材's cards for these materials. */
const browse = (materials: Material[] = [summary()]) => materialBrowse(materials.map((m) => cardOf(m)));

/** Selects `quote` inside paragraph `pid` as a teacher would with the mouse. */
function selectText(pid: string, quote: string) {
  const holder = document.querySelector(`[data-pid="${pid}"]`) as HTMLElement;
  const walker = document.createTreeWalker(holder, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  while (node && !(node.textContent ?? "").includes(quote)) node = walker.nextNode();
  if (!node) throw new Error(`${quote} not found in ${pid}`);
  const at = (node.textContent ?? "").indexOf(quote);
  act(() => {
    window.getSelection()?.setBaseAndExtent(node, at, node, at + quote.length);
    document.dispatchEvent(new Event("selectionchange"));
  });
}

describe("我的教材 (the material library page)", () => {
  it("shows the teacher's materials as cards (area=mine) and how much space is left", async () => {
    signIn();
    const api = mockApi({ ...baseRoutes(), "GET /api/materials": library(), "GET /api/browse/materials": browse() });
    const section = await openLibrary();
    const card = within(await section.findByRole("article", { name: "The Umbrella Stand" }));
    getMeta(["貼上的文字", "2 段"]);
    expect(card.getByText("文字")).toBeInTheDocument();
    expect(card.getByText("我")).toBeInTheDocument();
    expect(await section.findByText("1 份，檔案已使用 3 MB／1 GB")).toBeInTheDocument();
    expect(api.callsTo("GET", "/api/browse/materials").map((c) => c.url)).toEqual(["/api/browse/materials?area=mine"]);
  });

  it("uploads a file after the privacy warning; refuses what the server would refuse before sending", async () => {
    signIn();
    const uploaded = detail({ id: 6, kind: "document", origin: "upload", title: "handout", has_file: true });
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": browse(),
      "POST /api/materials/upload": uploaded,
    });
    const user = userEvent.setup({ applyAccept: false });
    const section = await openLibrary();
    const opener = await section.findByRole("button", { name: "上傳檔案" });
    await user.click(opener);
    // The form is in a dialog over the page, not unrolled under the buttons.
    const dialog = await screen.findByRole("dialog", { name: "上傳檔案" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    const form = within(within(dialog).getByRole("form", { name: "上傳檔案" }));
    // The warning comes with every way of adding a material.
    expect(form.getByRole("note")).toHaveTextContent("請不要上傳學生作業、學生姓名、成績或照片");
    expect(form.getByText(/文件（\.txt、\.docx、\.pdf）每個最大 20 MB/)).toBeInTheDocument();
    const input = form.getByLabelText("選擇檔案");
    expect(input).toHaveAttribute("accept", ".txt,.docx,.pdf,.png,.mp4");

    // An executable is refused in the page.
    await user.upload(input, new File(["MZ"], "tool.exe"));
    expect(form.getByRole("alert")).toHaveTextContent("不支援這種檔案");
    expect(form.getByRole("button", { name: "上傳" })).toBeDisabled();
    // So is an image over 10 MB.
    const big = new File(["x"], "board.png");
    Object.defineProperty(big, "size", { value: 11 * 1024 * 1024 });
    await user.upload(input, big);
    expect(form.getByRole("alert")).toHaveTextContent("超過這類檔案的上限 10 MB");

    await user.upload(input, new File(["%PDF-1.4"], "handout.pdf", { type: "application/pdf" }));
    await user.type(form.getByRole("textbox", { name: "來源說明" }), "自編講義");
    await user.click(form.getByRole("button", { name: "上傳" }));
    expect(await screen.findByText("已加入「handout」。")).toBeInTheDocument();
    // Done: the dialog is gone and focus is back on the button that opened it.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
    // Sent as multipart, with the file and the fields (a material: no purpose).
    const call = api.fetchMock.mock.calls.find(([url]) => String(url) === "/api/materials/upload");
    const body = call?.[1]?.body as FormData;
    expect(body).toBeInstanceOf(FormData);
    expect((body.get("file") as File).name).toBe("handout.pdf");
    expect(body.get("source_note")).toBe("自編講義");
    expect(body.get("purpose")).toBeNull();
    // The cards and the usage are read again.
    await waitFor(() => expect(api.callsTo("GET", "/api/browse/materials").length).toBe(2));
    expect(api.callsTo("GET", "/api/materials").length).toBe(2);
  });

  it("says plainly when uploads are not set up, and pasting still works", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library([], { uploads_enabled: false, storage: "none" }),
      "POST /api/materials": detail({ id: 8, title: "Pasted" }),
    });
    const user = userEvent.setup();
    const section = await openLibrary();
    expect(await section.findByText(/^還沒有教材。/)).toBeInTheDocument();
    await user.click(await section.findByRole("button", { name: "上傳檔案" }));
    const upload = within(await screen.findByRole("dialog", { name: "上傳檔案" }));
    expect(upload.getByText(/還沒有設定檔案儲存空間/)).toBeInTheDocument();
    await user.click(upload.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(section.getByRole("button", { name: "貼上文字" }));
    const form = within(within(await screen.findByRole("dialog", { name: "貼上文字" })).getByRole("form", { name: "貼上文字" }));
    await user.type(form.getByRole("textbox", { name: "教材標題" }), "Pasted");
    await user.click(form.getByRole("textbox", { name: "教材文字" }));
    await user.paste(P1);
    await user.click(form.getByRole("button", { name: "加入教材庫" }));
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/materials")).toEqual({ kind: "text", title: "Pasted", source_note: "", text: P1 }),
    );
  });

  it("加入網址 is a dialog too; Esc and 取消 close it, and typed input is asked about first", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/materials": library(), "GET /api/browse/materials": browse() });
    const user = userEvent.setup();
    const section = await openLibrary();
    const opener = await section.findByRole("button", { name: "加入網址" });
    await user.click(opener);
    const dialog = within(await screen.findByRole("dialog", { name: "加入網址" }));
    expect(dialog.getByRole("textbox", { name: "網址" })).toHaveFocus();
    expect(dialog.getByRole("note")).toHaveTextContent("請不要上傳學生作業");

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();

    await user.click(opener);
    await user.click(await screen.findByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();

    // Typed input is not thrown away silently: Esc, 取消 and a click on the backdrop all ask.
    await user.click(opener);
    await user.type(await screen.findByRole("textbox", { name: "網址" }), "https://example.com/a");
    await user.keyboard("{Escape}");
    expect(screen.getByText("要放棄剛才輸入的內容嗎？")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "繼續編輯" }));
    expect(screen.getByRole("textbox", { name: "網址" })).toHaveValue("https://example.com/a");
    await user.click(document.querySelector(".scrim") as HTMLElement);
    expect(screen.getByText("要放棄剛才輸入的內容嗎？")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "放棄並關閉" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("while a paste is being saved the dialog cannot be closed; a failure is shown inside it", async () => {
    signIn();
    const failer: { run: (() => void) | null } = { run: null };
    mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": browse(),
      "POST /api/materials": () =>
        new Promise((resolve) => {
          failer.run = () => resolve(respond(500, { error: { code: "internal", message: "存不進去" } }));
        }),
    });
    const user = userEvent.setup();
    const section = await openLibrary();
    await user.click(await section.findByRole("button", { name: "貼上文字" }));
    const dialog = within(await screen.findByRole("dialog", { name: "貼上文字" }));
    await user.click(dialog.getByRole("textbox", { name: "教材文字" }));
    await user.paste(P1);
    await user.click(dialog.getByRole("button", { name: "加入教材庫" }));
    await waitFor(() => expect(dialog.getByRole("button", { name: "儲存中…" })).toBeDisabled());
    expect(dialog.getByRole("button", { name: "關閉" })).toBeDisabled();
    expect(dialog.getByRole("button", { name: "取消" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "貼上文字" })).toBeInTheDocument();

    await waitFor(() => expect(failer.run).not.toBeNull());
    failer.run?.();
    expect(await dialog.findByRole("alert")).toHaveTextContent("存不進去");
    expect(screen.getByRole("dialog", { name: "貼上文字" })).toBeInTheDocument();
    expect(dialog.getByRole("textbox", { name: "教材文字" })).toHaveValue(P1);
  });

  it("renames from the card's ⋯ menu", async () => {
    signIn();
    let title = "The Umbrella Stand";
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": () => browse([summary({ title })]),
      "PUT /api/materials/5": (call: MockCall) => {
        title = (call.body as { title: string }).title;
        return detail({ title });
      },
    });
    const user = userEvent.setup();
    const section = await openLibrary();
    await user.click(await section.findByRole("button", { name: "更多動作「The Umbrella Stand」" }));
    await user.click(screen.getByRole("button", { name: "改名" }));
    const dialog = within(screen.getByRole("dialog", { name: "教材改名" }));
    const input = dialog.getByRole("textbox", { name: "教材標題" });
    expect(input).toHaveFocus();
    await user.clear(input);
    await user.type(input, "L3 The Umbrella Stand");
    await user.click(dialog.getByRole("button", { name: "儲存標題" }));
    expect(await section.findByText("已改名為「L3 The Umbrella Stand」。")).toBeInTheDocument();
    expect(api.lastBody("PUT", "/api/materials/5")).toEqual({ title: "L3 The Umbrella Stand" });
    expect(await section.findByRole("article", { name: "L3 The Umbrella Stand" })).toBeInTheDocument();
  });

  it("deletes only after a confirmation that says the deletion is permanent and what goes with it", async () => {
    signIn();
    let deleted = false;
    const withFile = summary({ has_file: true, kind: "document", size_bytes: 2048, filename: "umbrella.pdf" });
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/materials": () => library(deleted ? [] : [withFile]),
      "GET /api/browse/materials": () => browse(deleted ? [] : [withFile]),
      "GET /api/materials/5": detail({ ...withFile }),
      "DELETE /api/materials/5": () => {
        deleted = true;
        return { deleted: true, annotations: 1, lessons: [7] };
      },
    });
    const user = userEvent.setup();
    const section = await openLibrary();
    const more = await section.findByRole("button", { name: "更多動作「The Umbrella Stand」" });
    await user.click(more);
    await user.click(screen.getByRole("button", { name: "刪除" }));
    const confirm = within(screen.getByRole("alertdialog", { name: "永久刪除「The Umbrella Stand」？" }));
    expect(await confirm.findByText(/刪除後無法復原：原檔、擷取的文字與段落、1 則註記都會永久刪除/)).toBeInTheDocument();
    expect(confirm.getByText(/用到它的課例（「第 2 節 精讀」）會顯示「教材已刪除」/)).toBeInTheDocument();
    // Cancelling sends nothing.
    await user.click(confirm.getByRole("button", { name: "取消" }));
    expect(api.callsTo("DELETE", "/api/materials/5")).toHaveLength(0);
    expect(more).toHaveFocus();

    await user.click(more);
    await user.click(screen.getByRole("button", { name: "刪除" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "永久刪除" }));
    expect(await section.findByText("已永久刪除「The Umbrella Stand」。")).toBeInTheDocument();
    expect(api.callsTo("DELETE", "/api/materials/5")).toHaveLength(1);
    expect(await section.findByText(/^還沒有教材。/)).toBeInTheDocument();
  });

  it("a failed delete keeps the material and says why", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": browse(),
      "GET /api/materials/5": detail(),
      "DELETE /api/materials/5": respond(503, {
        error: { code: "unavailable", message: "原檔沒有刪除成功（儲存空間暫時無法使用），所以這份教材還沒有刪除。請稍後再試一次。" },
      }),
    });
    const user = userEvent.setup();
    const section = await openLibrary();
    await user.click(await section.findByRole("button", { name: "更多動作「The Umbrella Stand」" }));
    await user.click(screen.getByRole("button", { name: "刪除" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "永久刪除" }));
    expect(await screen.findByText(/所以這份教材還沒有刪除/)).toBeInTheDocument();
    expect(section.getByRole("article", { name: "The Umbrella Stand" })).toBeInTheDocument();
  });
});

describe("material viewer and annotations", () => {
  function viewerRoutes(extra: Record<string, unknown> = {}) {
    return {
      ...baseRoutes(),
      "GET /api/materials": library(),
      "GET /api/browse/materials": browse(),
      "GET /api/materials/5": detail(),
      "GET /api/materials/5/annotations": [
        annotation(),
        annotation({
          id: 32,
          quote: "However",
          note: "學生常忽略這個轉折",
          anchor: { status: "lost", paragraph_id: null, paragraph_no: null, start: null, end: null },
        }),
      ],
      ...extra,
    };
  }

  async function openViewer() {
    const user = userEvent.setup();
    const section = await openLibrary();
    await user.click(await section.findByRole("button", { name: "預覽「The Umbrella Stand」" }));
    const viewer = within(await screen.findByRole("complementary", { name: "教材" }));
    await viewer.findByText("先確認學生知道這是什麼");
    return { user, viewer };
  }

  it("shows the text with paragraph numbers, notes in place, and lost notes asking to be placed again", async () => {
    signIn();
    mockApi(viewerRoutes());
    const { viewer } = await openViewer();
    // The annotated span is marked; the material text itself is unchanged around it.
    const mark = document.querySelector("mark.anno") as HTMLElement;
    expect(mark).toHaveTextContent("umbrella stand");
    expect(document.querySelector('[data-pid="p1"]')).toHaveTextContent(P1);
    expect(viewer.getByText("第 1 段：")).toBeInTheDocument();
    expect(viewer.getByText("來源：課本第三課")).toBeInTheDocument();
    // The lost note is kept and flagged.
    const lost = within(viewer.getByRole("region", { name: "需要重新確認位置的註記" }));
    expect(lost.getByText("需要重新確認位置")).toBeInTheDocument();
    expect(lost.getByText("學生常忽略這個轉折")).toBeInTheDocument();
    // AI sharing is off unless the teacher turns it on.
    const note = within(viewer.getByRole("group", { name: "我的註記：umbrella stand" }));
    expect(note.getByRole("checkbox", { name: "提供給 AI" })).not.toBeChecked();
  });

  it("selecting text and 加註記 stores a note anchored to the paragraph; it can be shared with AI, hidden and deleted", async () => {
    signIn();
    const created = annotation({
      id: 40,
      quote: "twelve umbrellas",
      note: "數字題",
      anchor: { status: "ok", paragraph_id: "p2", paragraph_no: 2, start: 20, end: 36 },
    });
    const api = mockApi(
      viewerRoutes({
        "POST /api/materials/5/annotations": created,
        "PUT /api/annotations/40": (call: MockCall) => ({ ...created, ...(call.body as object) }),
        "DELETE /api/annotations/40": { deleted: true },
      }),
    );
    const { user, viewer } = await openViewer();

    selectText("p2", "twelve umbrellas");
    await user.click(await viewer.findByRole("button", { name: "加註記" }));
    const draft = within(viewer.getByRole("form", { name: "新增註記" }));
    expect(draft.getByText("「twelve umbrellas」")).toBeInTheDocument();
    expect(draft.getByRole("checkbox", { name: "提供給 AI" })).not.toBeChecked();
    await user.type(draft.getByRole("textbox", { name: "註記內容" }), "數字題");
    await user.click(draft.getByRole("button", { name: "儲存註記" }));
    // Where, not what: the server takes the quote from the paragraph.
    await waitFor(() =>
      expect(api.lastBody("POST", "/api/materials/5/annotations")).toEqual({
        paragraph_id: "p2",
        start: 20,
        end: 36,
        note: "數字題",
        share_with_ai: false,
      }),
    );
    const note = within(await viewer.findByRole("group", { name: "我的註記：twelve umbrellas" }));
    expect(document.querySelectorAll("mark.anno")).toHaveLength(2);

    await user.click(note.getByRole("checkbox", { name: "提供給 AI" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/annotations/40")).toEqual({ share_with_ai: true }));
    expect(note.getByRole("checkbox", { name: "提供給 AI" })).toBeChecked();

    await user.click(note.getByRole("button", { name: "修改" }));
    const edit = note.getByRole("textbox", { name: "註記內容" });
    await user.clear(edit);
    await user.type(edit, "數字與比例");
    await user.click(note.getByRole("button", { name: "儲存註記" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/annotations/40")).toEqual({ note: "數字與比例" }));

    await user.click(note.getByRole("button", { name: "隱藏" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/annotations/40")).toEqual({ hidden: true }));
    expect(await viewer.findByText("有 1 則隱藏的註記。")).toBeInTheDocument();
    expect(viewer.queryByRole("group", { name: "我的註記：twelve umbrellas" })).not.toBeInTheDocument();
    await user.click(viewer.getByRole("button", { name: "顯示隱藏的註記" }));
    const shown = within(viewer.getByRole("group", { name: "我的註記：twelve umbrellas" }));
    expect(shown.getByText("已隱藏")).toBeInTheDocument();

    await user.click(shown.getByRole("button", { name: "刪除" }));
    expect(shown.getByRole("group", { name: "確認刪除註記" })).toHaveTextContent("教材不會變動");
    await user.click(shown.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/annotations/40")).toHaveLength(1));
    // The material was never sent anywhere.
    expect(api.callsTo("PUT", "/api/materials/5")).toHaveLength(0);
  });

  it("a selection across paragraphs is not one note; a lost note is placed again by selecting", async () => {
    signIn();
    const api = mockApi(
      viewerRoutes({
        "PUT /api/annotations/32": annotation({
          id: 32,
          quote: "something unexpected",
          note: "學生常忽略這個轉折",
          anchor: { status: "ok", paragraph_id: "p2", paragraph_no: 2, start: 61, end: 81 },
        }),
      }),
    );
    const { user, viewer } = await openViewer();
    const p1 = document.querySelector('[data-pid="p1"]')?.lastChild?.firstChild as Node;
    const p2 = document.querySelector('[data-pid="p2"]')?.firstChild?.firstChild as Node;
    act(() => {
      window.getSelection()?.setBaseAndExtent(p1, 2, p2, 5);
      document.dispatchEvent(new Event("selectionchange"));
    });
    expect(await viewer.findByText("一則註記標在同一段裡的文字。請只選一段裡的文字。")).toBeInTheDocument();

    const lost = within(viewer.getByRole("region", { name: "需要重新確認位置的註記" }));
    await user.click(lost.getByRole("button", { name: "重新選位置" }));
    expect(viewer.getByText("請在下面的教材裡選取這則註記要標的文字。")).toBeInTheDocument();
    selectText("p2", "something unexpected");
    await waitFor(() =>
      expect(api.lastBody("PUT", "/api/annotations/32")).toEqual({
        anchor: { paragraph_id: "p2", start: 61, end: 81 },
      }),
    );
    await waitFor(() => expect(viewer.queryByText("需要重新確認位置")).not.toBeInTheDocument());
  });

  it("previews an image through a short-lived API link and says when a PDF has no text", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/materials/9": detail({ id: 9, kind: "image", origin: "upload", has_file: true, filename: "board.png", title: "Board", text_status: "not_applicable", paragraphs: [], text: "" }),
      "GET /api/materials/9/annotations": [],
      "POST /api/materials/9/file-link": { url: "/api/materials/9/file?t=123.1.abc", expires_at: "2026-11-15T10:00:00Z" },
      "GET /api/materials/10": detail({ id: 10, kind: "document", origin: "upload", has_file: true, filename: "scan.pdf", title: "Scan", text_status: "no_text", page_count: 2, paragraphs: [], text: "" }),
      "GET /api/materials/10/annotations": [],
      "POST /api/materials/10/file-link": { url: "/api/materials/10/file?t=123.1.def", expires_at: "2026-11-15T10:00:00Z" },
    });
    const first = render(<MaterialViewer mode="pane" materialId={9} />);
    expect(await screen.findByRole("img", { name: "Board" })).toHaveAttribute("src", "/api/materials/9/file?t=123.1.abc");
    expect(screen.getByRole("link", { name: "下載原檔" })).toHaveAttribute("href", "/api/materials/9/file?t=123.1.abc&download=1");
    first.unmount();
    render(<MaterialViewer mode="pane" materialId={10} />);
    // A server without OCR (no `ocr` on the material): nothing to start, so no 用 OCR 重新讀取.
    expect(await screen.findByText(/沒有可以選取的文字（可能是掃描檔），所以還不能在上面加註記.*你可以下載原檔對照/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "用 OCR 重新讀取" })).not.toBeInTheDocument();
    expect(screen.queryByText(/選取文字就能加註記/)).not.toBeInTheDocument();
  });
});

describe("MaterialViewer as a side-by-side pane", () => {
  it("collapses, opens again, goes full screen and resizes from the keyboard", async () => {
    signIn();
    mockApi({ "GET /api/materials/5": detail(), "GET /api/materials/5/annotations": [] });
    const user = userEvent.setup();
    render(<MaterialViewer mode="pane" materialId={5} />);
    const pane = await screen.findByRole("complementary", { name: "教材" });
    expect(await within(pane).findByText(P2)).toBeInTheDocument();
    const separator = within(pane).getByRole("separator", { name: "調整教材欄的寬度" });
    expect(separator).toHaveAttribute("aria-valuenow", "448");
    separator.focus();
    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(separator).toHaveAttribute("aria-valuenow", "496");
    expect(pane).toHaveStyle({ width: "496px" });
    await user.keyboard("{Home}");
    expect(separator).toHaveAttribute("aria-valuenow", "320");

    await user.click(within(pane).getByRole("button", { name: "全螢幕" }));
    expect(screen.getByRole("complementary", { name: "教材" })).toHaveClass("is-full");
    await user.click(screen.getByRole("button", { name: "離開全螢幕" }));

    await user.click(screen.getByRole("button", { name: "收起教材" }));
    expect(screen.queryByText(P2)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "對照教材" }));
    expect(await screen.findByText(P2)).toBeInTheDocument();
    // The width the teacher chose is remembered.
    expect(screen.getByRole("separator")).toHaveAttribute("aria-valuenow", "320");
  });

  it("splits a paragraph at overlapping annotations", () => {
    expect(
      segmentsFor("abcdefgh", [
        { id: 1, start: 1, end: 5 },
        { id: 2, start: 3, end: 7 },
      ]).map((s) => [s.text, s.ids]),
    ).toEqual([
      ["a", []],
      ["bc", [1]],
      ["de", [1, 2]],
      ["fg", [2]],
      ["h", []],
    ]);
    expect(segmentsFor("abc", [{ id: 1, start: 2, end: 99 }]).map((s) => s.text)).toEqual(["ab", "c"]);
  });

  it("a drag that lands inside a word takes the whole word", () => {
    const text = "Mrs. Lin put an umbrella stand outside her shop.";
    const snap = (a: string, b: string) => {
      const r = snapToWords(text, { start: text.indexOf(a), end: text.indexOf(b) + b.length });
      return text.slice(r.start, r.end);
    };
    expect(snap("rella", "stand ou")).toBe("umbrella stand outside");
    expect(snap(" umbrella", "stand ")).toBe("umbrella stand");
    expect(snap("Lin", "Lin")).toBe("Lin");
  });
});
