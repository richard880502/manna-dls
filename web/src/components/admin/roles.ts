import type { Role } from "../../api/types";

export const ROLES: Role[] = ["teacher", "pi", "kb_manager", "ra", "rater", "engineer"];

/** What a 知識庫管理員 may and may not do (docs/API.md「角色與權限」), said wherever the role is given. */
export const KB_MANAGER_NOTE =
  "「知識庫管理員」可以管理知識庫的教材、學習單範本、課綱條目與詞彙，但不能建立邀請碼或變更平台設定，也看不到其他教師的資料。";

/** One line on what the new role means for the account, shown before the change is confirmed. */
export function roleChangeNote(from: Role, to: Role): string {
  if (to === "pi") return "主持人可以使用全部後台功能，包括建立邀請碼與變更其他帳號的角色。";
  if (from === "pi") return "對方將不能再使用後台。";
  if (to === "kb_manager") return KB_MANAGER_NOTE;
  if (from === "kb_manager") return "對方將不能再管理知識庫。";
  return "對方的備課資料不受影響。";
}

