// 備課庫：單元與課例改名、刪除；教材平台：管理員刪除 (#132, docs/API.md「備課庫：單元與課例改名、
// 刪除」). Renaming opens a dialog with the current name; deleting first says what goes with the
// unit or lesson (read from the server), and nothing is sent until it is confirmed. On 平台教材 and
// 平台範本 only accounts that manage the knowledge base see 刪除. Every title here is made up.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import type { Lesson, LessonDeletePreview, UnitDeletePreview } from "../api/types";
import {
  baseRoutes,
  classRef,
  coreVersion,
  materialBrowse,
  materialCard,
  mockApi,
  renderApp,
  respond,
  schoolClass,
  signIn,
  teacher,
  templateBrowse,
  templateCard,
  unitWith,
} from "../test/mockApi";

const class3 = schoolClass({ id: 3, name: "高一 3 班" });
const class7 = schoolClass({ id: 7, name: "高一 7 班", position: 2 });
const shared = unitWith({ id: 1, title: "L3 倒裝句", lesson_count: 3, classes: [classRef(class3), classRef(class7)] });
const empty = unitWith({ id: 2, title: "L4 假設語氣", lesson_count: 0, core_status: "none" });

const lesson = (id: number, title: string): Lesson => ({
  id,
  unit_id: 1,
  title,
  lesson_date: null,
  class_description: "",
  created_at: "2026-11-10T08:00:00Z",
  my_example: "",
});

const unitPreview: UnitDeletePreview = { lessons: 3, worksheets: 5, runs: 2 };
const row = (title: string) =>
  within(screen.getByRole("link", { name: title }).closest("li") as HTMLElement);

beforeEach(() => signIn());

describe("備課庫: a unit's ⋯", () => {
  it("改名 opens with the current name, refuses a blank one, and saves the new name in the row", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [shared, empty],
      "PUT /api/units/1": unitWith({ ...shared, title: "L3 Not only… but also" }),
    });
    const user = userEvent.setup();
    renderApp("/units");
    const more = await screen.findByRole("button", { name: "更多動作「L3 倒裝句」" });
    await user.click(more);
    await user.click(row("L3 倒裝句").getByRole("button", { name: "改名" }));

    const dialog = within(await screen.findByRole("dialog", { name: "改單元名稱" }));
    const field = dialog.getByLabelText("單元名稱");
    expect(field).toHaveValue("L3 倒裝句");
    expect(field).toHaveFocus();
    await user.clear(field);
    await user.click(dialog.getByRole("button", { name: "儲存" }));
    expect(dialog.getByText("請輸入單元名稱。")).toBeInTheDocument();
    expect(api.callsTo("PUT", "/api/units/1")).toHaveLength(0);

    await user.type(field, "  L3 Not only… but also ");
    await user.click(dialog.getByRole("button", { name: "儲存" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/units/1")).toEqual({ title: "L3 Not only… but also" }));
    expect(await screen.findByRole("link", { name: "L3 Not only… but also" })).toHaveAttribute("href", "/units/1");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(more).toHaveFocus();
  });

  it("刪除 says what goes with the unit first; 取消 sends nothing, 刪除單元 takes the row away", async () => {
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units": [shared, empty],
      "GET /api/units/1/delete-preview": unitPreview,
      "DELETE /api/units/1": { ok: true, lessons_deleted: 3, worksheets_deleted: 5 },
    });
    const user = userEvent.setup();
    renderApp("/units");
    const more = await screen.findByRole("button", { name: "更多動作「L3 倒裝句」" });
    await user.click(more);
    await user.click(row("L3 倒裝句").getByRole("button", { name: "刪除" }));

    const confirm = within(await screen.findByRole("alertdialog", { name: "刪除單元「L3 倒裝句」？" }));
    expect(await confirm.findByText("會一起刪除：")).toBeInTheDocument();
    expect(confirm.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "3 節課例",
      "5 份學習單",
      "單元想法",
      "2 次上課紀錄",
    ]);
    expect(confirm.getByText("也會從高一 3 班、高一 7 班移除。")).toBeInTheDocument();
    expect(confirm.getByText("刪除後無法復原。")).toBeInTheDocument();

    await user.click(confirm.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(api.callsTo("DELETE", "/api/units/1")).toHaveLength(0);
    expect(more).toHaveFocus();

    await user.click(more);
    await user.click(row("L3 倒裝句").getByRole("button", { name: "刪除" }));
    const again = within(await screen.findByRole("alertdialog"));
    await user.click(await again.findByRole("button", { name: "刪除單元" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/units/1")).toHaveLength(1));
    expect(await screen.findByText("已刪除單元「L3 倒裝句」。")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "L3 倒裝句" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "L4 假設語氣" })).toBeInTheDocument();
    // Its row is gone: focus lands on the page heading.
    expect(screen.getByRole("heading", { name: "備課庫", level: 1 })).toHaveFocus();
  });

  it("a unit with nothing under it says so, and a refused delete keeps the row and says why", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/units": [empty],
      "GET /api/units/2/delete-preview": { lessons: 0, worksheets: 0, runs: 0 } satisfies UnitDeletePreview,
      "DELETE /api/units/2": respond(500, { error: { code: "internal", message: "伺服器暫時無法處理，請稍後再試。" } }),
    });
    const user = userEvent.setup();
    renderApp("/units");
    await user.click(await screen.findByRole("button", { name: "更多動作「L4 假設語氣」" }));
    await user.click(screen.getByRole("button", { name: "刪除" }));
    const confirm = within(await screen.findByRole("alertdialog"));
    expect(await confirm.findByText("這個單元還沒有課例、學習單或單元想法。")).toBeInTheDocument();
    expect(confirm.queryByText(/移除。$/)).not.toBeInTheDocument();
    await user.click(confirm.getByRole("button", { name: "刪除單元" }));
    expect(await confirm.findByText("伺服器暫時無法處理，請稍後再試。")).toBeInTheDocument();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "L4 假設語氣" })).toBeInTheDocument();
  });
});

describe("單元頁: the unit's and each lesson's ⋯", () => {
  const routes = (extra: Record<string, unknown> = {}) => ({
    ...baseRoutes(),
    "GET /api/units/1": shared,
    "GET /api/units/1/lessons": [lesson(7, "第 1 節 讀懂故事"), lesson(8, "第 2 節 精讀")],
    "GET /api/units/1/core": coreVersion([]),
    "GET /api/runs": [],
    ...extra,
  });

  it("改名 renames a lesson in its row", async () => {
    const api = mockApi(
      routes({ "PUT /api/lessons/8": { ...lesson(8, "第 2 節 句型"), latest_version: null } }),
    );
    const user = userEvent.setup();
    renderApp("/units/1");
    await user.click(await screen.findByRole("button", { name: "更多動作「第 2 節 精讀」" }));
    await user.click(row("第 2 節 精讀").getByRole("button", { name: "改名" }));
    const dialog = within(await screen.findByRole("dialog", { name: "改課例名稱" }));
    const field = dialog.getByLabelText("課例名稱");
    expect(field).toHaveValue("第 2 節 精讀");
    await user.clear(field);
    await user.type(field, "第 2 節 句型");
    await user.click(dialog.getByRole("button", { name: "儲存" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/lessons/8")).toEqual({ title: "第 2 節 句型" }));
    expect(await screen.findByRole("link", { name: "第 2 節 句型" })).toHaveAttribute("href", "/lessons/8");
  });

  it("the same name closes 改名 without a request; typed changes are asked about before closing", async () => {
    const api = mockApi(routes());
    const user = userEvent.setup();
    renderApp("/units/1");
    await user.click(await screen.findByRole("button", { name: "更多動作「第 1 節 讀懂故事」" }));
    await user.click(row("第 1 節 讀懂故事").getByRole("button", { name: "改名" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "儲存" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(api.callsTo("PUT", "/api/lessons/7")).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "更多動作「第 1 節 讀懂故事」" }));
    await user.click(row("第 1 節 讀懂故事").getByRole("button", { name: "改名" }));
    await user.type(await screen.findByLabelText("課例名稱"), "！");
    await user.keyboard("{Escape}");
    expect(screen.getByText("要放棄剛才輸入的內容嗎？")).toBeInTheDocument();
  });

  it("刪除 a lesson says its worksheets and 上課紀錄 go too, then takes it off the list", async () => {
    const api = mockApi(
      routes({
        "GET /api/lessons/7/delete-preview": { worksheets: 2, runs: 1 } satisfies LessonDeletePreview,
        "DELETE /api/lessons/7": { ok: true, worksheets_deleted: 2 },
      }),
    );
    const user = userEvent.setup();
    renderApp("/units/1");
    await user.click(await screen.findByRole("button", { name: "更多動作「第 1 節 讀懂故事」" }));
    await user.click(row("第 1 節 讀懂故事").getByRole("button", { name: "刪除" }));
    const confirm = within(await screen.findByRole("alertdialog", { name: "刪除課例「第 1 節 讀懂故事」？" }));
    await confirm.findByText("會一起刪除：");
    expect(confirm.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["2 份學習單", "1 次上課紀錄"]);
    await user.click(confirm.getByRole("button", { name: "刪除課例" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/lessons/7")).toHaveLength(1));
    expect(await screen.findByText("已刪除課例「第 1 節 讀懂故事」。")).toBeInTheDocument();
    const rows = within(screen.getByRole("region", { name: "課例" })).getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(screen.queryByRole("link", { name: "第 1 節 讀懂故事" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "課例", level: 2 })).toHaveFocus();
  });

  it("a lesson with nothing under it says so; an unreadable preview still says what goes in general", async () => {
    mockApi(routes({ "GET /api/lessons/8/delete-preview": { worksheets: 0, runs: 0 } satisfies LessonDeletePreview }));
    const user = userEvent.setup();
    renderApp("/units/1");
    await user.click(await screen.findByRole("button", { name: "更多動作「第 2 節 精讀」" }));
    await user.click(row("第 2 節 精讀").getByRole("button", { name: "刪除" }));
    expect(await screen.findByText("這節課例還沒有學習單或上課紀錄。")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "取消" }));

    // Lesson 7's preview is not routed (404): the general line, and the delete is still offered.
    await user.click(screen.getByRole("button", { name: "更多動作「第 1 節 讀懂故事」" }));
    await user.click(row("第 1 節 讀懂故事").getByRole("button", { name: "刪除" }));
    expect(await screen.findByText("連到這節課例的學習單會一起刪除。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刪除課例" })).toBeEnabled();
  });

  it("刪除 the unit from its page goes back to 備課庫 and says it once", async () => {
    let deleted = false;
    const api = mockApi(
      routes({
        "GET /api/units": () => (deleted ? [empty] : [shared, empty]),
        "GET /api/units/1/delete-preview": unitPreview,
        "DELETE /api/units/1": () => {
          deleted = true;
          return { ok: true, lessons_deleted: 3, worksheets_deleted: 5 };
        },
      }),
    );
    const user = userEvent.setup();
    const { router } = renderApp("/units/1");
    await screen.findByRole("heading", { name: "L3 倒裝句", level: 1 });
    await user.click(screen.getByRole("button", { name: "更多動作「L3 倒裝句」" }));
    await user.click(screen.getByRole("button", { name: "刪除" }));
    const confirm = within(await screen.findByRole("alertdialog", { name: "刪除單元「L3 倒裝句」？" }));
    await user.click(await confirm.findByRole("button", { name: "刪除單元" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/units/1")).toHaveLength(1));
    expect(await screen.findByRole("heading", { name: "備課庫", level: 1 })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/units");
    expect(screen.getByText("已刪除單元「L3 倒裝句」。")).toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "L4 假設語氣" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "L3 倒裝句" })).not.toBeInTheDocument();
    // Said once: the address no longer carries it.
    await waitFor(() => expect(router.state.location.state).toBeNull());
  });

  it("改名 the unit renames its heading", async () => {
    const api = mockApi(routes({ "PUT /api/units/1": unitWith({ ...shared, title: "L3 倒裝句（新版）" }) }));
    const user = userEvent.setup();
    renderApp("/units/1");
    await screen.findByRole("heading", { name: "L3 倒裝句", level: 1 });
    await user.click(screen.getByRole("button", { name: "更多動作「L3 倒裝句」" }));
    await user.click(screen.getByRole("button", { name: "改名" }));
    const field = await screen.findByLabelText("單元名稱");
    await user.clear(field);
    await user.type(field, "L3 倒裝句（新版）");
    await user.click(screen.getByRole("button", { name: "儲存" }));
    await waitFor(() => expect(api.lastBody("PUT", "/api/units/1")).toEqual({ title: "L3 倒裝句（新版）" }));
    expect(await screen.findByRole("heading", { name: "L3 倒裝句（新版）", level: 1 })).toBeInTheDocument();
  });
});

describe("教材平台: 刪除 for those who manage the knowledge base", () => {
  const paper = materialCard({ id: 31, title: "114 學測英文試題（示範）", scope: "platform", owner_label: "Manna 知識庫" });
  const platformTemplate = templateCard({ id: 21, title: "閱讀理解範本（示範）", scope: "platform", owner_label: "Manna 範本" });
  const card = (title: string) => within(screen.getByRole("article", { name: title }));

  it("a teacher sees no ⋯ on 平台教材 or 平台範本", async () => {
    mockApi({
      ...baseRoutes(),
      "GET /api/browse/materials": materialBrowse([paper]),
      "GET /api/browse/templates": templateBrowse([platformTemplate]),
    });
    renderApp("/reference");
    await screen.findByRole("article", { name: paper.title });
    expect(screen.queryByRole("button", { name: /^更多動作/ })).not.toBeInTheDocument();
  });

  it("a pi deletes a 平台教材 from its card after a confirmation, and the list is read again", async () => {
    let deleted = false;
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": { ...teacher, role: "pi" },
      "GET /api/browse/materials": () => materialBrowse(deleted ? [] : [paper]),
      "DELETE /api/admin/library/31": () => {
        deleted = true;
        return { deleted: true };
      },
    });
    const user = userEvent.setup();
    renderApp("/reference");
    await screen.findByRole("article", { name: paper.title });
    const more = card(paper.title).getByRole("button", { name: `更多動作「${paper.title}」` });
    await user.click(more);
    await user.click(card(paper.title).getByRole("button", { name: "刪除" }));
    const confirm = within(screen.getByRole("alertdialog", { name: `永久刪除「${paper.title}」？` }));
    expect(
      confirm.getByText("原檔、擷取的文字與段落都會永久刪除，老師就選不到它了。老師已經用它做好的學習單不受影響。"),
    ).toBeInTheDocument();
    await user.click(confirm.getByRole("button", { name: "取消" }));
    expect(api.callsTo("DELETE", "/api/admin/library/31")).toHaveLength(0);
    expect(more).toHaveFocus();

    await user.click(more);
    await user.click(card(paper.title).getByRole("button", { name: "刪除" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "永久刪除" }));
    expect(await screen.findByText(`已永久刪除「${paper.title}」。`)).toBeInTheDocument();
    expect(api.callsTo("DELETE", "/api/admin/library/31")).toHaveLength(1);
    expect(await screen.findByText("知識庫還沒有教材。")).toBeInTheDocument();
  });

  it("a kb_manager deletes a 平台範本 from its card; 我的範本 cards are unchanged", async () => {
    let deleted = false;
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/me": { ...teacher, role: "kb_manager", capabilities: ["manage_knowledge", "use_materials"] },
      "GET /api/browse/templates": () => templateBrowse(deleted ? [] : [platformTemplate]),
      "DELETE /api/admin/library/templates/21": () => {
        deleted = true;
        return { ok: true };
      },
    });
    const user = userEvent.setup();
    renderApp("/templates");
    await screen.findByRole("article", { name: platformTemplate.title });
    await user.click(card(platformTemplate.title).getByRole("button", { name: `更多動作「${platformTemplate.title}」` }));
    await user.click(card(platformTemplate.title).getByRole("button", { name: "刪除" }));
    const confirm = within(screen.getByRole("alertdialog", { name: `永久刪除「${platformTemplate.title}」？` }));
    expect(confirm.getByText("這個平台範本會永久刪除，老師就選不到它了。老師已經照它做好的學習單不受影響。")).toBeInTheDocument();
    await user.click(confirm.getByRole("button", { name: "永久刪除" }));
    expect(await screen.findByText(`已永久刪除「${platformTemplate.title}」。`)).toBeInTheDocument();
    expect(api.callsTo("DELETE", "/api/admin/library/templates/21")).toHaveLength(1);
    expect(await screen.findByText("還沒有平台範本。")).toBeInTheDocument();
  });
});
