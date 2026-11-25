import type { WorksheetController } from "../useWorksheet";
import type { VocabFill } from "./useVocabFill";
import { ToolbarPopover } from "./ToolbarPopover";

/** 顯示: what the sheet shows (學習目標, where the word tables go, the language of their meanings). */
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
          disabled={disabled}
          onChange={(e) => ws.setContent((c) => ({ ...c, vocabulary_at_end: e.target.checked }))}
        />
        <span>字詞表放在學習單最後</span>
      </label>
      <label className="check-row ws-vocab-zh">
        <input
          type="checkbox"
          checked={content.vocabulary_language === "zh"}
          disabled={disabled}
          onChange={(e) => {
            const zhOn = e.target.checked;
            ws.setContent((c) => ({ ...c, vocabulary_language: zhOn ? "zh" : "en" }));
            if (zhOn && fill.missingZh) void fill.fill({ tables: true });
          }}
        />
        <span>字詞表的字義用中文</span>
      </label>
      {content.vocabulary_language === "zh" && fill.missingZh && !fill.filling && (
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
