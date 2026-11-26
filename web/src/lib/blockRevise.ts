import type {
  WorksheetComment,
  WsCommentField,
  WsCommentStatus,
  WsProposal,
  WsProposalStatus,
} from "../api/types";

/** How often a running block revision is asked for. Tests shorten it. */
export const blockReviseConfig = { pollMs: 2000 };

export const NOTE_MAX = 2000;
export const QUOTE_MAX = 500;
export const INSTRUCTION_MAX = 2000;

export const COMMENT_STATUS_LABEL: Record<WsCommentStatus, string> = {
  open: "待處理",
  applied: "已依此修改",
  dismissed: "已略過",
};

export const PROPOSAL_STATUS_LABEL: Record<WsProposalStatus, string> = {
  running: "Manna 正在改這一區…",
  proposed: "Manna 的修改建議",
  accepted: "已採用",
  rejected: "已放棄",
  stale: "這個修改沒有套用",
  failed: "這次沒有改成",
};

export const REVISE_PLACEHOLDER = "想怎麼改這一區？例如：第 2 題太難，改簡單一點";
export const NOTE_PLACEHOLDER = "例如：這題太難，改簡單一點";
/** Said under every place notes are made. */
export const NOTES_HINT = "註記不會印在學習單上，也不會放進匯出的檔案；勾了「提供給 AI」的，Manna 改這一區時會照著改。";

/** The printed number of a question on the sheet shown (by id), when it is there. */
export type NumberOf = (itemId: string) => number | undefined;

/** Where a note is, in the teacher's words: 「第 6 題」, 「標題」, 「作答指示」, 「這一區」. */
export function placeOf(c: Pick<WorksheetComment, "field" | "anchor">, numberOf?: NumberOf): string {
  switch (c.field) {
    case "title":
      return "標題";
    case "instructions":
      return "作答指示";
    case "item": {
      if (c.anchor.status === "lost") return "原本的那一題";
      const no = c.anchor.item_id ? numberOf?.(c.anchor.item_id) : undefined;
      return no !== undefined ? `第 ${no} 題` : "一題";
    }
    default:
      return "這一區";
  }
}

/** The block a note belongs to now (or where it was made, when that block is gone). */
export const blockOfNote = (c: WorksheetComment) => c.anchor.block_id ?? c.block_id;

/** The open notes of a block that Manna may read (「依註記修改」). */
export const openAiNotes = (notes: WorksheetComment[]) =>
  notes.filter((c) => c.status === "open" && c.share_with_ai);

/** A proposal that keeps its block from being edited until it is decided. */
export const holdsBlock = (p: WsProposal | undefined) => p?.status === "running" || p?.status === "proposed";

// ---------------------------------------------------------------- selection → note position

/** What the teacher selected on a printed worksheet, as the API takes it. */
export interface SheetSelection {
  block_id: string;
  field: WsCommentField;
  item_id?: string;
  start?: number;
  end?: number;
  /** The selected text (always set). */
  quote: string;
}

/** 「第 6 題」「標題」… for a selection, before it is saved. */
export function placeOfSelection(s: SheetSelection, numberOf?: NumberOf): string {
  return placeOf(
    {
      field: s.field,
      anchor: { status: "ok", block_id: s.block_id, item_id: s.item_id ?? null, start: null, end: null },
    },
    numberOf,
  );
}
