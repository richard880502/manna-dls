import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TeacherMemories } from "../components/prefs/TeacherMemories";
import { MemorySettings } from "../components/admin/MemorySettings";
import { mockApi } from "../test/mockApi";
import type { TeacherMemory } from "../api/types";

const row: TeacherMemory = { id: 1, kind: "choice_options", value: "3", statement: "選擇題使用 3 個選項。", valid_at: "2026-11-19T08:00:00Z", invalid_at: null, superseded_by: null, invalidation_reason: null, interaction_id: 4 };

describe("confirmed teaching habits", () => {
  it("previews a replacement without saving, confirms explicitly, and shows history", async () => {
    let memories = [row];
    const api = mockApi({
      "GET /api/me/memories": () => ({ memories, enabled: false, graph_enabled: false, index: null }),
      "POST /api/me/memories/preview": { id: 7, statement: "選擇題使用 4 個選項。", replaces_id: 1 },
      "POST /api/me/memories": () => {
        memories = [{ ...row, invalid_at: "2026-11-19T09:00:00Z", superseded_by: 2, invalidation_reason: "replaced" }, { ...row, id: 2, value: "4", statement: "選擇題使用 4 個選項。" }];
        return { id: 2 };
      },
    });
    const user = userEvent.setup(); render(<TeacherMemories />);
    expect(await screen.findByText(/目前尚未開放/)).toBeInTheDocument();
    await user.type(screen.getByLabelText("想記住的長期習慣"), "之後選擇題用四個選項");
    await user.click(screen.getByRole("button", { name: "整理成習慣（尚未記住）" }));
    expect(await screen.findByText(/將取代/)).toBeInTheDocument();
    expect(api.callsTo("POST", "/api/me/memories")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "確認並記住" }));
    await waitFor(() => expect(api.lastBody("POST", "/api/me/memories")).toEqual({ candidate_id: 7, confirmed: true }));
    expect(await screen.findByText(/已記住這項習慣/)).toBeInTheDocument();
    await user.click(screen.getByText("查看歷程（1 項）"));
    expect(screen.getByText(/已被新習慣取代/)).toBeInTheDocument();
  });

  it("requires an explicit stop confirmation and keeps errors visible", async () => {
    const api = mockApi({
      "GET /api/me/memories": { memories: [row], enabled: true, graph_enabled: false, index: null },
      "DELETE /api/me/memories/1": { ok: true },
    });
    const user = userEvent.setup(); render(<TeacherMemories />);
    await user.click(await screen.findByRole("button", { name: /停止使用：/ }));
    expect(api.callsTo("DELETE", "/api/me/memories/1")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "確認停止使用" }));
    await waitFor(() => expect(api.callsTo("DELETE", "/api/me/memories/1")).toHaveLength(1));
    expect(await screen.findByText(/已停止使用這項習慣/)).toBeInTheDocument();
  });

  it("shows that incompatible services use Postgres without blocking settings", async () => {
    mockApi({
      "GET /api/admin/memory-settings": { enabled: true, graph_enabled: true },
      "GET /api/admin/memory-index": { pending: 1, failed: 0, compatible: false, sidecar: null },
    });
    render(<MemorySettings />);
    expect(await screen.findByText(/目前只使用 Postgres/)).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /讓已確認的出題習慣/ })).toBeChecked();
  });

  it("lets the PI enable memory and request a rebuild", async () => {
    const api = mockApi({
      "GET /api/admin/memory-settings": { enabled: false, graph_enabled: true },
      "GET /api/admin/memory-index": { pending: 2, failed: 0 },
      "PUT /api/admin/memory-settings": { enabled: true, graph_enabled: true },
      "POST /api/admin/memory-index/rebuild": { ok: true },
    });
    const user = userEvent.setup(); render(<MemorySettings />);
    const toggle = await screen.findByRole("checkbox", { name: /讓已確認的出題習慣/ });
    expect(toggle).not.toBeChecked(); await user.click(toggle);
    expect(await screen.findByText("已開放讓出題習慣影響產生結果。")).toBeInTheDocument();
    expect(api.lastBody("PUT", "/api/admin/memory-settings")).toEqual({ enabled: true });
    await user.click(screen.getByRole("button", { name: "從正本重建記憶索引" }));
    await waitFor(() => expect(api.callsTo("POST", "/api/admin/memory-index/rebuild")).toHaveLength(1));
  });
});
