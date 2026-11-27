import type { WorksheetController } from "../useWorksheet";
import type { VocabFill } from "./useVocabFill";
import { ToolbarPopover } from "./ToolbarPopover";

/**
 * 顯示: what the sheet shows (學習目標, where the Key Vocabulary tables go, the language of their
 * meanings). The two Key Vocabulary switches say so by name: a block's Word bank (its 字詞表
 * support) is another thing, which they do not move or translate. A sheet without Key Vocabulary
 * has them off, with a line that says why.
 */
export function DisplayPopover({
  ws,
  fill,
  disabled,
}: {
  ws: WorksheetController;
  fill: VocabFill;
  disabled: boolean;
}) {
  const content = ws.state?.content;
  if (!content) return null;
  const hasTable = content.blocks.some((b) => b.enabled && b.vocabulary.length > 0);
  const tableOff = disabled || !hasTable;
  return (
    <ToolbarPopover label="顯示" panelLabel="顯示設定" panelClassName="ws-display-panel">
      <label className="check-row ws-show-objectives">
        <input
          type="checkbox"
          checked={content.show_objectives}
          disabled={disabled}
          onChange={(e) => ws.setContent((c) => ({ ...c, show_objectives: e.target.checked }))}
        />
        <span>顯示學習目標</span>
      </label>
      <label className="check-row ws-vocab-at-end">
        <input
          type="checkbox"
          checked={content.vocabulary_at_end === true}
          disabled={tableOff}
          onChange={(e) => ws.setContent((c) => ({ ...c, vocabulary_at_end: e.target.checked }))}
        />
        <span>Key Vocabulary 放在學習單最後</span>
      </label>
      <label className="check-row ws-vocab-zh">
        <input
          type="checkbox"
          checked={content.vocabulary_language === "zh"}
          disabled={tableOff}
          onChange={(e) => {
            const zhOn = e.target.checked;
            ws.setContent((c) => ({ ...c, vocabulary_language: zhOn ? "zh" : "en" }));
            if (zhOn && fill.missingZh) void fill.fill({ tables: true });
          }}
        />
        <span>Key Vocabulary 的字義用中文</span>
      </label>
      {!hasTable && (
        <p className="quiet-note ws-display-note">
          這份學習單沒有 Key Vocabulary（單字和字義的表），所以上面兩項用不到。題目旁的 Word bank 不受影響。
        </p>
      )}
      {hasTable && content.vocabulary_language === "zh" && fill.missingZh && !fill.filling && (
        <button
          type="button"
          className="text-link is-plain"
          disabled={disabled}
          onClick={() => void fill.fill({ tables: true })}
        >
          補上中文字義
        </button>
      )}
    </ToolbarPopover>
  );
}
