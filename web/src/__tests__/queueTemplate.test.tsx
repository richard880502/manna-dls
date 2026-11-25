// 排隊顯示 for a template being read (docs/API.md「其他功能的 `queue`」): the template page shows 「排隊中，前面還有 N 個」
// in place of the reading line, and 取消排隊 loads the template again, which shows how it ended.
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WsTemplateDetail } from "../api/types";
import { baseRoutes, mockApi, renderApp, respond, signIn } from "../test/mockApi";

beforeEach(() => signIn());
afterEach(() => vi.unstubAllGlobals());

const queue = { job_id: 21, ahead: 2, eta_secs: 61 };
const LINE = "排隊中，前面還有 2 個（約 2 分鐘）";
const DELETE = "DELETE /api/ai-jobs/21";

async function cancelQueued(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "取消排隊" }));
  await user.click(screen.getByRole("button", { name: "確定取消" }));
}

describe("template being read", () => {
  const extracting = (overrides: Partial<WsTemplateDetail> = {}): WsTemplateDetail => ({
    id: 9,
    title: "L2 學習單版型",
    status: "extracting",
    section_count: 0,
    source_material: { id: 5, title: "L2 學習單" },
    version_no: null,
    updated_at: "2026-11-15T08:00:00Z",
    error: null,
    content: null,
    deletes_source_material: true,
    queue,
    ...overrides,
  });

  it("shows the queue line in place of the reading line; cancelling loads the template again", async () => {
    let state = extracting();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/worksheet-templates/9": () => state,
      [DELETE]: () => {
        state = extracting({ status: "failed", queue: undefined, error: "已取消排隊。" });
        return respond(204, undefined);
      },
    });
    const user = userEvent.setup();
    renderApp("/templates/9");
    expect(await screen.findByText(LINE)).toBeInTheDocument();
    expect(screen.queryByText("Manna 正在讀這份學習單的設計…（約 20 秒）")).not.toBeInTheDocument();
    await cancelQueued(user);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("已取消排隊。"));
    expect(api.callsTo("DELETE", "/api/ai-jobs/21")).toHaveLength(1);
  });
});
