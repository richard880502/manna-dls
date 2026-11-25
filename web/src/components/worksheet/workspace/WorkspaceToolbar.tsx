import { useId, useRef, useState, type ReactNode } from "react";
import { Link } from "react-router";
import type { WsAudience, WsLevel } from "../../../api/types";
import { AUDIENCE_LABEL, LEVEL_LABEL } from "../../../lib/worksheet";
import { ErrorText } from "../../common";
import { menuButtonOf } from "../../prep/SectionTools";
import { ExportMenu } from "../ExportMenu";
import { VersionHistoryButton } from "../VersionHistory";
import type { WorksheetController } from "../useWorksheet";
import { DisplayPopover } from "./DisplayPopover";
import { InfoPopover } from "./InfoPopover";
import { SaveStatus } from "./SaveStatus";
import { ToolbarPopover } from "./ToolbarPopover";
import type { VocabFill } from "./useVocabFill";

const AUDIENCES: WsAudience[] = ["student", "teacher"];

/** The dialogs the toolbar asks the workspace for, with the button focus returns to. */
export type WorkspaceDialog = "print" | "vocab";

/**
 * The one row of tools above the sheet: 學生版｜教師版, the version (版本紀錄), a slot for 存成快照,
 * 顯示, 詞彙檢查, 匯出, ⓘ and 更多…, with the saved time at the end. Anything bigger than a switch
 * opens a popover or a dialog, so the sheet stays the thing on screen.
 */
export function WorkspaceToolbar({
  ws,
  level,
  audience,
  onAudience,
  locked,
  blockedReason,
  fill,
  snapshotSlot,
  onOpenDialog,
  openFullHref,
  onAdjust,
}: {
  ws: WorksheetController;
  level: WsLevel;
  audience: WsAudience;
  onAudience: (audience: WsAudience) => void;
  /** Manna or the page is changing the worksheet: switches that edit it are off. */
  locked: boolean;
  /** Why 版本紀錄 cannot be used right now; null: it can. */
  blockedReason: string | null;
  fill: VocabFill;
  /** Where 「存成快照」 goes: right after the version button. */
  snapshotSlot?: ReactNode;
  onOpenDialog: (dialog: WorkspaceDialog, opener: HTMLElement | null) => void;
  /** The prep page: 在完整編輯頁開啟. */
  openFullHref?: string;
  /** The full page: 調整設定. */
  onAdjust?: () => void;
}) {
  const ids = useId();
  const state = ws.state;
  const [exportError, setExportError] = useState<string | null>(null);
  const vocabRef = useRef<HTMLButtonElement>(null);
  if (!state) return null;
  const levels = state.content.levels;

  return (
    <div className="ws-bar-wrap">
      <div className="ws-bar" role="group" aria-label="學習單工具列" lang="zh-Hant">
        <div className="ws-seg" role="radiogroup" aria-label="顯示版本">
          {AUDIENCES.map((a) => (
            <label key={a}>
              <input
                type="radio"
                name={`${ids}-aud`}
                checked={audience === a}
                onChange={() => onAudience(a)}
              />
              <span>{AUDIENCE_LABEL[a]}</span>
            </label>
          ))}
        </div>
        <span className="ws-bar-version">
          <span className="prep-version" aria-label="版本">
            第 {state.version_no} 版
          </span>
          <VersionHistoryButton
            worksheetId={state.id}
            levels={levels}
            className="decision-button"
            blockedReason={blockedReason}
            beforeOpen={ws.flush}
            onRestored={(done) => ws.adopt(done.worksheet)}
          />
        </span>
        {snapshotSlot}
        <DisplayPopover ws={ws} fill={fill} disabled={locked} />
        <button
          ref={vocabRef}
          type="button"
          className="decision-button"
          onClick={() => onOpenDialog("vocab", vocabRef.current)}
        >
          詞彙檢查
        </button>
        <ExportMenu
          worksheetId={state.id}
          level={level}
          levelLabel={levels.length > 1 ? LEVEL_LABEL[level] : null}
          disabled={state.running}
          beforeExport={ws.flush}
          onError={setExportError}
          footnote="Word 可再編輯；參考答案只在教師版。"
        />
        <InfoPopover content={state.content} jobs={state.jobs} teacherView={audience === "teacher"} />
        <ToolbarPopover
          label="更多…"
          panelLabel="更多工具"
          panelClassName="ws-more-panel"
        >
          {(close) => (
            <>
              <button
                type="button"
                className="popover-item"
                onClick={(e) => {
                  // The menu is gone when the dialog closes: focus returns to its button.
                  const opener = menuButtonOf(e.currentTarget);
                  close();
                  onOpenDialog("print", opener);
                }}
              >
                列印樣式預覽
              </button>
              {openFullHref && (
                <Link className="popover-item" to={openFullHref}>
                  在完整編輯頁開啟
                </Link>
              )}
              {onAdjust && (
                <button
                  type="button"
                  className="popover-item"
                  onClick={() => {
                    close();
                    onAdjust();
                  }}
                >
                  調整設定
                </button>
              )}
            </>
          )}
        </ToolbarPopover>
        <SaveStatus ws={ws} className="ws-bar-saved" />
      </div>
      <ErrorText message={exportError} />
    </div>
  );
}
