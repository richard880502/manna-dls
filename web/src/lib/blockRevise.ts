import type {
  WorksheetComment,
  WsCommentField,
  WsCommentStatus,
  WsProposal,
  WsProposalStatus,
} from "../api/types";
import { snapToWords } from "./materials";

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

/** The part of `range` inside `el`, as offsets into `el`'s text. */
function clampToElement(range: Range, el: Element): { start: number; end: number } {
  const length = (el.textContent ?? "").length;
  const offsetOf = (node: Node, offset: number, fallback: number): number => {
    if (!el.contains(node)) return fallback;
    const probe = document.createRange();
    probe.selectNodeContents(el);
    probe.setEnd(node, offset);
    return probe.toString().length;
  };
  return {
    start: offsetOf(range.startContainer, range.startOffset, 0),
    end: offsetOf(range.endContainer, range.endOffset, length),
  };
}

const elementOf = (node: Node): Element | null => (node instanceof Element ? node : node.parentElement);

/**
 * The browser selection as a place on the sheet in `root`. Blocks carry `data-block`, a question
 * `data-item`, and the texts a selection is measured in (title, instructions, a question's prompt)
 * `data-field`. A selection inside one of those gives its offsets (snapped to whole English
 * words); inside a question elsewhere (options, answer) it is a note on the question; across
 * texts of one block it is a note on the block. Returns "multi" across blocks, null when
 * nothing of the sheet is selected.
 */
export function selectionOnSheet(root: HTMLElement | null): SheetSelection | "multi" | null {
  const selection = typeof window !== "undefined" ? window.getSelection() : null;
  if (!root || !selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  const start = elementOf(range.startContainer);
  const end = elementOf(range.endContainer);
  if (!start || !end || !root.contains(start) || !root.contains(end)) return null;
  const text = range.toString().replace(/\s+/g, " ").trim();
  if (!text) return null;
  const quote = text.slice(0, QUOTE_MAX);
  const block = start.closest<HTMLElement>("[data-block]");
  if (!block || block !== end.closest("[data-block]")) return block ? "multi" : null;
  const blockId = block.dataset.block ?? "";
  const holder = start.closest<HTMLElement>("[data-field]");
  const item = start.closest<HTMLElement>("[data-item]");
  if (holder && holder === end.closest("[data-field]")) {
    const full = holder.textContent ?? "";
    const snapped = snapToWords(full, clampToElement(range, holder));
    const field = (holder.dataset.field ?? "block") as WsCommentField;
    const itemId = field === "item" ? item?.dataset.item : undefined;
    if (field === "item" && !itemId) return { block_id: blockId, field: "block", quote };
    return {
      block_id: blockId,
      field,
      ...(itemId ? { item_id: itemId } : {}),
      start: snapped.start,
      end: snapped.end,
      quote: full.slice(snapped.start, snapped.end),
    };
  }
  if (item?.dataset.item && item === end.closest("[data-item]")) {
    return { block_id: blockId, field: "item", item_id: item.dataset.item, quote };
  }
  return { block_id: blockId, field: "block", quote };
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
