// 教材與範本管理 (docs/API.md「Phase 2.9」, "Frontend: admin side"): the role gate, what a bulk
// upload fills in from file and folder names, 全部上傳 two at a time with each row's progress,
// error and retry, edit and delete in the admin list, and polling while templates are being read.
// Synthetic files only: names shaped like the real folders, a few bytes of content.
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  AdminLibraryMaterial,
  AdminLibraryMaterialRow,
  AdminLibraryTemplate,
  AdminLibraryTemplateRow,
} from "../api/types";
import {
  adminLibraryConfig,
  guessMaterialCategory,
  guessTags,
  isSystemFile,
  materialMeta,
  parseTags,
  tagsProblem,
  templateMeta,
  titleFromPath,
} from "../components/adminLibrary/prefill";
import { baseRoutes, mockApi, renderApp, respond, signIn, teacher, type MockCall } from "../test/mockApi";
import { getMeta, metaItems } from "../test/metaList";

const LIB = "/api/admin/library";
const TPL = "/api/admin/library/templates";

const pi = { ...teacher, research_id: "PI-01", role: "pi" as const };
const manager = { ...teacher, research_id: "KB-01", role: "kb_manager" as const };

// ---------------------------------------------------------------- fixtures

const materialCard = (overrides: Partial<AdminLibraryMaterial> = {}): AdminLibraryMaterial => ({
  id: 101,
  title: "114學測英文_試題",
  scope: "platform",
  category: "學測英文",
  tags: ["114", "試題"],
  kind: "document",
  file_type: "docx",
  page_count: null,
  paragraph_count: 40,
  word_count: 2912,
  excerpt: "Read the passage and answer the questions.",
  text_status: "ready",
  looks_like_worksheet: false,
  created_at: "2026-11-16T02:00:00Z",
  owner_label: "Manna 知識庫",
  ...overrides,
});

const materialRow = (overrides: Partial<AdminLibraryMaterialRow> = {}): AdminLibraryMaterialRow => ({
  ...materialCard(),
  uploaded_by: "PI-01",
  size_bytes: 1536 * 1024,
  ...overrides,
});

const templateCard = (overrides: Partial<AdminLibraryTemplate> = {}): AdminLibraryTemplate => ({
  id: 201,
  title: "0001_03_三民-B1L5-自學講義",
  scope: "platform",
  status: "extracting",
  category: "",
  tags: ["三民", "B1L5"],
  worksheet_kind: "",
  instruction_language: "",
  section_count: 0,
  section_titles: [],
  source_material: { id: 301, title: "0001_03_三民-B1L5-自學講義", file_type: "pdf", page_count: 4 },
  updated_at: "2026-11-16T02:00:00Z",
  owner_label: "Manna 範本",
  ...overrides,
});

const templateRow = (overrides: Partial<AdminLibraryTemplateRow> = {}): AdminLibraryTemplateRow => ({
  ...templateCard(),
  uploaded_by: "KB-01",
  size_bytes: 4096,
  ...overrides,
});

const ready = (overrides: Partial<AdminLibraryTemplateRow> = {}) =>
  templateRow({
    status: "ready",
    category: "自學講義",
    instruction_language: "英文",
    section_count: 4,
    section_titles: ["Warm-up", "Vocabulary", "Reading", "Reflection"],
    updated_at: "2026-11-16T02:01:00Z",
    ...overrides,
  });

/** A file whose browser-given path is `path` (as webkitdirectory gives it). */
function fileAt(path: string, content = "x"): File {
  const name = path.split("/").pop() ?? path;
  const file = new File([content], name);
  if (path !== name) Object.defineProperty(file, "webkitRelativePath", { value: path });
  return file;
}

const isTemplates = (call: MockCall) => call.url.includes("kind=templates");

function libraryRoutes(
  me: typeof pi | typeof manager,
  lists: { materials?: () => AdminLibraryMaterialRow[]; templates?: () => AdminLibraryTemplateRow[] } = {},
) {
  return {
    ...baseRoutes(),
    "GET /api/me": me,
    [`GET ${LIB}`]: (call: MockCall) => ({
      items: isTemplates(call) ? (lists.templates?.() ?? []) : (lists.materials?.() ?? []),
      next_cursor: null,
    }),
  };
}

/** The multipart bodies sent to `path`, in order. */
function formsTo(api: ReturnType<typeof mockApi>, path: string): FormData[] {
  return api.fetchMock.mock.calls
    .filter(([url, init]) => String(url) === path && init?.method === "POST")
    .map(([, init]) => init?.body as FormData);
}

const panel = (name: string) => within(screen.getByRole("tabpanel", { name }));
const fileRow = (path: string) => within(screen.getByRole("group", { name: path }));

beforeEach(() => {
  adminLibraryConfig.pollMs = 20;
});

afterEach(() => {
  adminLibraryConfig.pollMs = 3000;
});

// ---------------------------------------------------------------- the role gate

describe("教材與範本管理: who gets it", () => {
  it("sends a teacher home without asking the admin APIs, and shows no link", async () => {
    signIn();
    const api = mockApi({ ...baseRoutes() });
    const { router } = renderApp("/admin/library");
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(screen.queryByRole("heading", { name: "教材與範本管理" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "教材與範本管理" })).not.toBeInTheDocument();
    expect(api.calls.some((c) => c.path.startsWith(LIB))).toBe(false);
  });

  it("is in the admin links for a 知識庫管理員 and for the PI, next to 知識庫", async () => {
    signIn();
    mockApi(libraryRoutes(manager));
    const user = userEvent.setup();
    renderApp("/");
    const link = await screen.findByRole("link", { name: "教材與範本管理" });
    expect(screen.getByRole("link", { name: "知識庫" })).toBeInTheDocument();
    await user.click(link);
    expect(await screen.findByRole("heading", { name: "教材與範本管理", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "知識庫教材" })).toHaveAttribute("aria-selected", "true");
    expect(link).toHaveAttribute("aria-current", "page");
  });

  it("opens 平台範本 from its tab or from ?tab=templates", async () => {
    signIn();
    const api = mockApi(libraryRoutes(pi, { templates: () => [ready()] }));
    const user = userEvent.setup();
    const { router } = renderApp("/admin/library?tab=templates");
    expect(await screen.findByRole("tab", { name: "平台範本" })).toHaveAttribute("aria-selected", "true");
    expect(await panel("平台範本").findByRole("heading", { name: "0001_03_三民-B1L5-自學講義" })).toBeInTheDocument();
    // The materials tab is only asked for once it is opened.
    expect(api.calls.filter((c) => c.path === LIB && !isTemplates(c))).toHaveLength(0);
    await user.click(screen.getByRole("tab", { name: "知識庫教材" }));
    expect(screen.getByRole("tab", { name: "知識庫教材" })).toHaveAttribute("aria-selected", "true");
    expect(router.state.location.pathname).toBe("/admin/library");
    await waitFor(() => expect(api.calls.filter((c) => c.path === LIB && !isTemplates(c))).toHaveLength(1));
  });
});

// ---------------------------------------------------------------- what names fill in

describe("defaults from file and folder names", () => {
  it("guesses the 教材參考 category from the path, the most specific word first", () => {
    expect(guessMaterialCategory("學測英文/114/114學測英文_試題.docx")).toBe("學測英文");
    expect(guessMaterialCategory("統測英文/113/113統測_英文_標準答案.pdf")).toBe("統測英文");
    expect(guessMaterialCategory("國民中小學暨普通型高中_語文領域-英語文課程綱要(發布版).pdf")).toBe("課綱");
    expect(guessMaterialCategory("108課綱英文.pdf")).toBe("課綱");
    expect(guessMaterialCategory("學測英文/高中英文參考詞彙表.pdf")).toBe("詞彙表");
    expect(guessMaterialCategory("114學年度學測英文考試說明.pdf")).toBe("考試說明");
    expect(guessMaterialCategory("講義.pdf")).toBe("");
  });

  it("takes the year, 試題／答案／參考答案, publishers and book-lesson codes as tags", () => {
    expect(guessTags("學測英文/114/114學測英文_試題.docx")).toEqual(["114", "試題"]);
    expect(guessTags("統測英文/113/113統測_英文_標準答案.pdf")).toEqual(["113", "答案"]);
    expect(guessTags("學測英文/112/參考答案.pdf")).toEqual(["112", "參考答案"]);
    expect(guessTags("0001_03_三民-B1L5-自學講義.pdf")).toEqual(["三民", "B1L5"]);
    expect(guessTags("0013_01_龍騰 B3L2 課文講義.docx")).toEqual(["龍騰", "B3L2"]);
    expect(guessTags("國民中小學暨普通型高中_語文領域-英語文課程綱要(發布版).pdf")).toEqual([]);
  });

  it("titles a row with the file name, skips system files, and checks tags", () => {
    expect(titleFromPath("學測英文/114/114學測英文_試題.docx")).toBe("114學測英文_試題");
    expect(titleFromPath("0013_01_龍騰 B3L2 課文講義.docx")).toBe("0013_01_龍騰 B3L2 課文講義");
    expect(isSystemFile("學測英文/.DS_Store")).toBe(true);
    expect(isSystemFile("學測英文/~$114學測英文_試題.docx")).toBe(true);
    expect(isSystemFile("__MACOSX/學測英文/a.pdf")).toBe(true);
    expect(isSystemFile("學測英文/114/114學測英文_試題.docx")).toBe(false);
    expect(parseTags("114、試題, 試題，聽力 ")).toEqual(["114", "試題", "聽力"]);
    expect(tagsProblem(Array.from({ length: 13 }, (_, i) => `t${i}`))).toBe("標籤最多 12 個。");
    expect(tagsProblem(["一二三四五六七八九十一二三四五六七八九十一"])).toMatch(/超過 20 字/);
  });

  it("says what a row is", () => {
    expect(materialMeta(materialCard())).toEqual(["Word", "約 2,900 字"]);
    expect(materialMeta(materialCard({ file_type: "pdf", page_count: 7, word_count: 0, paragraph_count: 6 }))).toEqual(["PDF", "7 頁", "6 段"]);
    expect(materialMeta(materialCard({ file_type: "pdf", text_status: "no_text" }))).toEqual(["PDF", "讀不到文字（可能是掃描檔）"]);
    expect(templateMeta(ready())).toEqual(["4 區", "英文指示", "自學講義"]);
  });

  it("fills each row from a picked folder's paths, and leaves out what cannot be uploaded", async () => {
    signIn();
    mockApi(libraryRoutes(pi));
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/admin/library");
    const tab = await waitFor(() => panel("知識庫教材"));
    await user.upload(tab.getByLabelText("選擇資料夾"), [
      fileAt("大考英文/學測英文/114/114學測英文_試題.docx"),
      fileAt("大考英文/統測英文/113/113統測_英文_標準答案.pdf"),
      fileAt("大考英文/國民中小學暨普通型高中_語文領域-英語文課程綱要(發布版).pdf"),
      fileAt("大考英文/補充/閱讀講義.pdf"),
      fileAt("大考英文/.DS_Store"),
      fileAt("大考英文/舊檔/old.doc"),
    ]);

    const gsat = fileRow("大考英文/學測英文/114/114學測英文_試題.docx");
    expect(gsat.getByRole("textbox", { name: "標題" })).toHaveValue("114學測英文_試題");
    expect(gsat.getByRole("combobox", { name: "類別" })).toHaveDisplayValue("學測英文");
    expect(gsat.getByRole("textbox", { name: "標籤" })).toHaveValue("114、試題");
    const tve = fileRow("大考英文/統測英文/113/113統測_英文_標準答案.pdf");
    expect(tve.getByRole("combobox", { name: "類別" })).toHaveDisplayValue("統測英文");
    expect(tve.getByRole("textbox", { name: "標籤" })).toHaveValue("113、答案");
    const curriculum = fileRow("大考英文/國民中小學暨普通型高中_語文領域-英語文課程綱要(發布版).pdf");
    expect(curriculum.getByRole("combobox", { name: "類別" })).toHaveDisplayValue("課綱");
    // No keyword: the admin chooses.
    expect(fileRow("大考英文/補充/閱讀講義.pdf").getByRole("combobox", { name: "類別" })).toHaveDisplayValue(
      "請選類別",
    );
    expect(screen.queryByRole("group", { name: "大考英文/.DS_Store" })).not.toBeInTheDocument();
    expect(screen.getByText("略過 1 個不支援的檔案：大考英文/舊檔/old.doc")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "待上傳的知識庫教材" })).getAllByRole("listitem")).toHaveLength(4);
    expect(screen.getByText("4 個檔案待上傳，1 個還沒選類別")).toBeInTheDocument();
  });

  it("walks a dropped folder and uses its folder names", async () => {
    signIn();
    mockApi(libraryRoutes(pi));
    renderApp("/admin/library");
    const tab = await waitFor(() => panel("知識庫教材"));

    // What a browser's drop gives: entries to walk (DataTransferItem.webkitGetAsEntry).
    const fileEntry = (path: string) => ({
      isFile: true,
      isDirectory: false,
      name: path.split("/").pop(),
      fullPath: `/${path}`,
      file: (ok: (f: File) => void) => ok(new File(["x"], path.split("/").pop() ?? path)),
    });
    const dirEntry = (path: string, children: unknown[]) => ({
      isFile: false,
      isDirectory: true,
      name: path.split("/").pop(),
      fullPath: `/${path}`,
      createReader: () => {
        let given = false;
        return {
          readEntries: (ok: (e: unknown[]) => void) => {
            ok(given ? [] : children);
            given = true;
          },
        };
      },
    });
    const folder = dirEntry("學測英文", [
      dirEntry("學測英文/114", [fileEntry("學測英文/114/試題.pdf"), fileEntry("學測英文/114/參考答案.pdf")]),
    ]);
    const zone = tab.getByText("把檔案或整個資料夾拖到這裡").closest(".adlib-drop");
    expect(zone).not.toBeNull();
    fireEvent.drop(zone as Element, {
      dataTransfer: { files: [], items: [{ kind: "file", webkitGetAsEntry: () => folder }], types: ["Files"] },
    });

    const paper = await waitFor(() => fileRow("學測英文/114/試題.pdf"));
    expect(paper.getByRole("textbox", { name: "標題" })).toHaveValue("試題");
    expect(paper.getByRole("combobox", { name: "類別" })).toHaveDisplayValue("學測英文");
    expect(paper.getByRole("textbox", { name: "標籤" })).toHaveValue("114、試題");
    expect(fileRow("學測英文/114/參考答案.pdf").getByRole("textbox", { name: "標籤" })).toHaveValue("114、參考答案");
  });

  it("picks plain files too, with publisher tags and 自動判斷 for templates", async () => {
    signIn();
    mockApi(libraryRoutes(manager));
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/admin/library?tab=templates");
    const tab = await waitFor(() => panel("平台範本"));
    await user.upload(tab.getByLabelText("選擇檔案"), [
      fileAt("0001_03_三民-B1L5-自學講義.pdf"),
      fileAt("0013_01_龍騰 B3L2 課文講義.docx"),
      fileAt("課堂照片.png"),
    ]);
    const sanmin = fileRow("0001_03_三民-B1L5-自學講義.pdf");
    expect(sanmin.getByRole("textbox", { name: "標題" })).toHaveValue("0001_03_三民-B1L5-自學講義");
    expect(sanmin.getByRole("combobox", { name: "類別" })).toHaveDisplayValue("自動判斷");
    expect(sanmin.getByRole("textbox", { name: "標籤" })).toHaveValue("三民、B1L5");
    expect(fileRow("0013_01_龍騰 B3L2 課文講義.docx").getByRole("textbox", { name: "標籤" })).toHaveValue("龍騰、B3L2");
    // A template is read from text: an image is left out.
    expect(screen.getByText("略過 1 個不支援的檔案：課堂照片.png")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- 全部上傳

describe("全部上傳", () => {
  it("sends two at a time, with each row's progress and error, and retries a row", async () => {
    signIn();
    const pending: Array<(value: unknown) => void> = [];
    let inFlight = 0;
    let maxInFlight = 0;
    let nextId = 500;
    const api = mockApi({
      ...libraryRoutes(pi),
      [`POST ${LIB}`]: () =>
        new Promise((resolve) => {
          inFlight += 1;
          maxInFlight = Math.max(maxInFlight, inFlight);
          pending.push((value) => {
            inFlight -= 1;
            resolve(value);
          });
        }),
    });
    /** Answers the oldest request still waiting. */
    const answer = (value: unknown) =>
      act(async () => {
        pending.shift()?.(value);
      });
    const added = (title: string) => materialCard({ id: nextId++, title });

    const user = userEvent.setup({ applyAccept: false });
    renderApp("/admin/library");
    const tab = await waitFor(() => panel("知識庫教材"));
    await user.upload(tab.getByLabelText("選擇檔案"), [
      fileAt("112學測英文_試題.pdf"),
      fileAt("113統測_英文_標準答案.pdf"),
      fileAt("114學測英文_試題.docx"),
      fileAt("閱讀講義.pdf"),
    ]);
    // Titles can be changed before sending.
    const first = fileRow("112學測英文_試題.pdf");
    await user.clear(first.getByRole("textbox", { name: "標題" }));
    await user.type(first.getByRole("textbox", { name: "標題" }), "112 學測英文 試題");

    await user.click(tab.getByRole("button", { name: "全部上傳" }));
    // The one with no category stays, and says why.
    expect(fileRow("閱讀講義.pdf").getByText("請選類別。")).toBeInTheDocument();
    expect(fileRow("閱讀講義.pdf").getByText("待上傳")).toBeInTheDocument();

    await waitFor(() => expect(pending).toHaveLength(2));
    expect(first.getByText("上傳中…")).toBeInTheDocument();
    expect(fileRow("113統測_英文_標準答案.pdf").getByText("上傳中…")).toBeInTheDocument();
    expect(fileRow("114學測英文_試題.docx").getByText("排隊中")).toBeInTheDocument();
    expect(first.getByRole("textbox", { name: "標題" })).toBeDisabled();
    expect(screen.getByRole("progressbar", { name: "上傳進度" })).toHaveAttribute("value", "0");

    // The fields go with the file.
    const [form] = formsTo(api, LIB);
    expect((form?.get("file") as File | undefined)?.name).toBe("112學測英文_試題.pdf");
    expect(form?.get("title")).toBe("112 學測英文 試題");
    expect(form?.get("category")).toBe("學測英文");
    expect(form?.get("tags")).toBe("112,試題");

    await answer(added("112 學測英文 試題"));
    // A slot is free: the third goes; never more than two at once.
    await waitFor(() => expect(formsTo(api, LIB)).toHaveLength(3));
    expect(first.getByText("已加入：Word，約 2,900 字")).toBeInTheDocument();
    await answer(respond(413, { error: { code: "platform_quota", message: "平台空間已滿，請先刪除不用的檔案。" } }));
    const tve = fileRow("113統測_英文_標準答案.pdf");
    expect(await tve.findByText("上傳失敗")).toBeInTheDocument();
    expect(screen.getByText("平台空間已滿，請先刪除不用的檔案。")).toBeInTheDocument();
    await answer(added("114學測英文_試題"));
    expect(await screen.findByText("已上傳 2／4 個檔案，1 個失敗，1 個還沒選類別")).toBeInTheDocument();
    expect(maxInFlight).toBe(2);
    // Once the run is over, the list below is read again.
    await waitFor(() => expect(api.callsTo("GET", LIB)).toHaveLength(2));

    // 重試 sends just that row again.
    await user.click(tve.getByRole("button", { name: "重試「113統測_英文_標準答案」" }));
    await waitFor(() => expect(formsTo(api, LIB)).toHaveLength(4));
    expect(formsTo(api, LIB)[3]?.get("title")).toBe("113統測_英文_標準答案");
    await answer(added("113統測_英文_標準答案"));
    expect(await tve.findByText(/^已加入/)).toBeInTheDocument();

    // The last one, once it has a category.
    await user.selectOptions(fileRow("閱讀講義.pdf").getByRole("combobox", { name: "類別" }), "其他");
    await user.click(tab.getByRole("button", { name: "全部上傳" }));
    await waitFor(() => expect(formsTo(api, LIB)).toHaveLength(5));
    expect(formsTo(api, LIB)[4]?.get("category")).toBe("其他");
    expect(formsTo(api, LIB)[4]?.get("tags")).toBeNull();
    await answer(added("閱讀講義"));
    expect(await screen.findByText("已上傳 4／4 個檔案")).toBeInTheDocument();
    expect(tab.getByRole("button", { name: "全部上傳" })).toBeDisabled();
    expect(maxInFlight).toBe(2);

    await user.click(tab.getByRole("button", { name: "清除已上傳的" }));
    expect(screen.queryByRole("list", { name: "待上傳的知識庫教材" })).not.toBeInTheDocument();
  });

  it("asks before leaving while files are on their way", async () => {
    signIn();
    const pending: Array<(value: unknown) => void> = [];
    mockApi({
      ...libraryRoutes(pi),
      [`POST ${LIB}`]: () => new Promise((resolve) => pending.push(resolve)),
    });
    const user = userEvent.setup({ applyAccept: false });
    const { router } = renderApp("/admin/library");
    const tab = await waitFor(() => panel("知識庫教材"));
    await user.upload(tab.getByLabelText("選擇檔案"), [fileAt("114學測英文_試題.pdf")]);
    await user.click(tab.getByRole("button", { name: "全部上傳" }));
    await waitFor(() => expect(pending).toHaveLength(1));
    // Switching tabs is not leaving.
    await user.click(screen.getByRole("tab", { name: "平台範本" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "知識庫教材" }));

    await user.click(screen.getByRole("link", { name: "首頁" }));
    const confirm = within(await screen.findByRole("alertdialog"));
    expect(confirm.getByText(/排隊中的檔案就不會上傳/)).toBeInTheDocument();
    await user.click(confirm.getByRole("button", { name: "留在這頁" }));
    expect(router.state.location.pathname).toBe("/admin/library");

    await act(async () => {
      pending.shift()?.(materialCard());
    });
    expect(await fileRow("114學測英文_試題.pdf").findByText(/^已加入/)).toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "首頁" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  });

  it("warns on a row whose text could not be read", async () => {
    signIn();
    mockApi({
      ...libraryRoutes(pi),
      [`POST ${LIB}`]: materialCard({ file_type: "pdf", page_count: 3, text_status: "no_text", word_count: 0 }),
    });
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/admin/library");
    const tab = await waitFor(() => panel("知識庫教材"));
    await user.upload(tab.getByLabelText("選擇檔案"), [fileAt("114學測英文_試題.pdf")]);
    await user.click(tab.getByRole("button", { name: "全部上傳" }));
    expect(
      await fileRow("114學測英文_試題.pdf").findByText("已加入：PDF，3 頁，讀不到文字（可能是掃描檔）"),
    ).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- the admin list

describe("the admin list", () => {
  it("searches, edits title, category and tags, and deletes after confirming", async () => {
    signIn();
    let rows = [
      materialRow(),
      materialRow({ id: 102, title: "113統測_英文_標準答案", category: "統測英文", tags: [], uploaded_by: "KB-01" }),
    ];
    let saved = materialCard();
    const api = mockApi({
      ...libraryRoutes(manager, { materials: () => rows }),
      [`PATCH ${LIB}/101`]: (call: MockCall) => {
        saved = { ...saved, ...(call.body as Partial<AdminLibraryMaterial>) };
        return saved;
      },
      [`DELETE ${LIB}/102`]: () => {
        rows = rows.filter((r) => r.id !== 102);
        return { deleted: true };
      },
    });
    const user = userEvent.setup();
    renderApp("/admin/library");

    const list = within(await screen.findByRole("list", { name: "知識庫教材" }));
    expect(list.getByRole("heading", { name: "114學測英文_試題" })).toBeInTheDocument();
    // (The date is local time, so only its shape is checked.)
    const facts = metaItems(await screen.findByRole("list", { name: "知識庫教材" }))[0]!;
    expect(facts.slice(0, 5)).toEqual(["學測英文", "Word", "約 2,900 字", "1.5 MB", "PI-01 上傳"]);
    expect(facts[5]).toMatch(/^\d{4}\/\d{2}\/\d{2}$/);
    expect(list.getByText("試題")).toHaveClass("soft-tag");

    // Search and the category filter go to the server.
    const search = within(screen.getByRole("search", { name: "搜尋知識庫教材" }));
    await user.type(search.getByLabelText("標題、標籤或文字"), "試題");
    await user.click(search.getByRole("button", { name: "搜尋" }));
    await user.selectOptions(search.getByLabelText("類別"), "學測英文");
    await waitFor(() => {
      const last = api.callsTo("GET", LIB).at(-1);
      const query = new URL(last?.url ?? "", "http://x").searchParams;
      expect(query.get("kind")).toBe("materials");
      expect(query.get("q")).toBe("試題");
      expect(query.get("category")).toBe("學測英文");
    });

    // Edit: only what changed is sent.
    await user.click(await screen.findByRole("button", { name: "編輯「114學測英文_試題」" }));
    const form = within(screen.getByRole("form", { name: "編輯「114學測英文_試題」" }));
    await user.clear(form.getByLabelText("標題"));
    await user.type(form.getByLabelText("標題"), "114 學測英文試題");
    await user.clear(form.getByLabelText("標籤"));
    await user.type(form.getByLabelText("標籤"), "114、試題、閱讀");
    await user.click(form.getByRole("button", { name: "儲存" }));
    await waitFor(() =>
      expect(api.lastBody("PATCH", `${LIB}/101`)).toEqual({ title: "114 學測英文試題", tags: ["114", "試題", "閱讀"] }),
    );
    expect(await screen.findByText("已儲存「114 學測英文試題」。")).toBeInTheDocument();
    expect(list.getByRole("heading", { name: "114 學測英文試題" })).toBeInTheDocument();
    expect(list.getByText("閱讀")).toBeInTheDocument();
    // The admin fields stay.
    getMeta(["學測英文", "Word", "約 2,900 字", "1.5 MB", "PI-01 上傳", facts[5]!]);

    // Change the category.
    await user.click(list.getByRole("button", { name: "編輯「114 學測英文試題」" }));
    const again = within(screen.getByRole("form", { name: "編輯「114 學測英文試題」" }));
    await user.selectOptions(again.getByLabelText("類別"), "考試說明");
    await user.click(again.getByRole("button", { name: "儲存" }));
    await waitFor(() => expect(api.lastBody("PATCH", `${LIB}/101`)).toEqual({ category: "考試說明" }));

    // A tag over 20 characters is refused before sending.
    await user.click(await list.findByRole("button", { name: "編輯「114 學測英文試題」" }));
    const bad = within(screen.getByRole("form", { name: "編輯「114 學測英文試題」" }));
    await user.type(bad.getByLabelText("標籤"), "、一二三四五六七八九十一二三四五六七八九十一");
    await user.click(bad.getByRole("button", { name: "儲存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("超過 20 字");
    expect(api.callsTo("PATCH", `${LIB}/101`)).toHaveLength(2);
    await user.click(bad.getByRole("button", { name: "取消" }));

    // Delete asks first.
    await user.click(list.getByRole("button", { name: "刪除「113統測_英文_標準答案」" }));
    const confirm = within(screen.getByRole("alertdialog", { name: "確認刪除「113統測_英文_標準答案」" }));
    expect(confirm.getByText(/永久刪除/)).toBeInTheDocument();
    await user.click(confirm.getByRole("button", { name: "取消" }));
    expect(api.callsTo("DELETE", `${LIB}/102`)).toHaveLength(0);
    await user.click(list.getByRole("button", { name: "刪除「113統測_英文_標準答案」" }));
    await user.click(screen.getByRole("button", { name: "確定刪除" }));
    expect(await screen.findByText("已刪除「113統測_英文_標準答案」。")).toBeInTheDocument();
    expect(api.callsTo("DELETE", `${LIB}/102`)).toHaveLength(1);
    expect(list.queryByRole("heading", { name: "113統測_英文_標準答案" })).not.toBeInTheDocument();
  });

  it("edits a platform template with the sample categories, and links to it", async () => {
    signIn();
    const api = mockApi({
      ...libraryRoutes(pi, { templates: () => [ready()] }),
      [`PATCH ${TPL}/201`]: (call: MockCall) => ready({ ...(call.body as object) }),
      [`DELETE ${TPL}/201`]: { deleted: true },
    });
    const user = userEvent.setup();
    renderApp("/admin/library?tab=templates");
    const list = within(await screen.findByRole("list", { name: "平台範本" }));
    expect(metaItems(await screen.findByRole("list", { name: "平台範本" }))[0]!.slice(0, 4)).toEqual(["4 區", "英文指示", "自學講義", "KB-01 上傳"]);
    expect(list.getByRole("link", { name: "看版型" })).toHaveAttribute("href", "/templates/201");

    await user.click(list.getByRole("button", { name: "編輯「0001_03_三民-B1L5-自學講義」" }));
    const form = within(screen.getByRole("form", { name: "編輯「0001_03_三民-B1L5-自學講義」" }));
    const options = within(form.getByLabelText("類別")).getAllByRole("option").map((o) => o.textContent);
    expect(options).toEqual(["字彙", "文法句型", "閱讀理解", "課文講義", "自學講義", "寫作", "聽說", "學習反思", "其他"]);
    await user.selectOptions(form.getByLabelText("類別"), "課文講義");
    await user.click(form.getByRole("button", { name: "儲存" }));
    await waitFor(() => expect(api.lastBody("PATCH", `${TPL}/201`)).toEqual({ category: "課文講義" }));
    await waitFor(() => expect(metaItems(screen.getByRole("list", { name: "平台範本" }))[0]!.slice(0, 3)).toEqual(["4 區", "英文指示", "課文講義"]));

    await user.click(list.getByRole("button", { name: "刪除「0001_03_三民-B1L5-自學講義」" }));
    await user.click(screen.getByRole("button", { name: "確定刪除" }));
    await waitFor(() => expect(api.callsTo("DELETE", `${TPL}/201`)).toHaveLength(1));
    expect(await screen.findByText("還沒有平台範本。用上面的上傳區加入。")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------- extraction

describe("平台範本 being read", () => {
  it("uploads a sample, polls every few seconds while it is being read, then stops", async () => {
    signIn();
    let server: AdminLibraryTemplateRow[] = [];
    const api = mockApi({
      ...libraryRoutes(manager, { templates: () => server }),
      [`POST ${TPL}`]: () => {
        server = [templateRow()];
        return templateCard();
      },
    });
    const templateGets = () => api.callsTo("GET", LIB).filter(isTemplates).length;
    const user = userEvent.setup({ applyAccept: false });
    renderApp("/admin/library?tab=templates");
    const tab = await waitFor(() => panel("平台範本"));
    expect(await tab.findByText("還沒有平台範本。用上面的上傳區加入。")).toBeInTheDocument();
    expect(templateGets()).toBe(1);

    await user.upload(tab.getByLabelText("選擇檔案"), [fileAt("0001_03_三民-B1L5-自學講義.pdf")]);
    await user.click(tab.getByRole("button", { name: "全部上傳" }));
    const row = fileRow("0001_03_三民-B1L5-自學講義.pdf");
    expect(await row.findByText("已上傳：讀取中…")).toBeInTheDocument();
    const [form] = formsTo(api, TPL);
    // 自動判斷 sends no category.
    expect(form?.get("category")).toBeNull();
    expect(form?.get("tags")).toBe("三民,B1L5");
    expect(form?.get("title")).toBe("0001_03_三民-B1L5-自學講義");

    // The list shows it too, and both are asked about again while it is being read.
    const list = within(await screen.findByRole("list", { name: "平台範本" }));
    expect(list.getByText("讀取中…")).toBeInTheDocument();
    await waitFor(() => expect(templateGets()).toBeGreaterThanOrEqual(4));
    expect(row.getByText("已上傳：讀取中…")).toBeInTheDocument();

    server = [ready()];
    expect(await row.findByText("已上傳：4 區，英文指示，自學講義")).toBeInTheDocument();
    expect(await list.findByRole("link", { name: "看版型" })).toBeInTheDocument();
    const settled = templateGets();
    await act(() => new Promise((r) => setTimeout(r, 120)));
    expect(templateGets()).toBe(settled);
  });

  it("retries a template that could not be read, and polls again", async () => {
    signIn();
    let server = [templateRow({ status: "failed" })];
    const api = mockApi({
      ...libraryRoutes(pi, { templates: () => server }),
      [`POST ${TPL}/201/retry`]: () => {
        server = [templateRow()];
        return templateCard();
      },
    });
    const user = userEvent.setup();
    renderApp("/admin/library?tab=templates");
    const list = within(await screen.findByRole("list", { name: "平台範本" }));
    expect(list.getByText("沒有讀成功")).toBeInTheDocument();
    await user.click(list.getByRole("button", { name: "再讀一次「0001_03_三民-B1L5-自學講義」" }));
    expect(await list.findByText("讀取中…")).toBeInTheDocument();
    expect(api.callsTo("POST", `${TPL}/201/retry`)).toHaveLength(1);
    server = [ready()];
    await waitFor(() => expect(metaItems(document.body).flat()).toEqual(expect.arrayContaining(["4 區", "英文指示", "自學講義"])));
  });
});
