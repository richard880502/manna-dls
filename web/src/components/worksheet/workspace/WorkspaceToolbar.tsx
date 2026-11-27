import { useId, useRef, useState } from "react";
import { Link } from "react-router";
import type { WsAudience, WsLevel } from "../../../api/types";
import { AUDIENCE_LABEL, LEVEL_LABEL } from "../../../lib/worksheet";
import { ErrorText } from "../../common";
import { ExportMenu } from "../ExportMenu";
import type { WorksheetController } from "../useWorksheet";
import { DisplayPopover } from "./DisplayPopover";
import { InfoPopover } from "./InfoPopover";
import { SaveStatus } from "./SaveStatus";
import { ToolbarPopover } from "./ToolbarPopover";
import { VersionMenu } from "./VersionMenu";
import type { VocabFill } from "./useVocabFill";

const AUDIENCES: WsAudience[] = ["student", "teacher"];

/** The dialogs the toolbar asks the workspace for, with the button focus returns to. */
export type WorkspaceDialog = "print" | "vocab";

/**
 * The row of tools above the sheet, in two groups. Start: how the sheet is looked at and worked on
 * (學生版｜教師版, 顯示, 詞彙檢查, ⓘ, 更多…). End: what is kept and handed out (the saved time,
 * 「第 N 版 ▾」 with 為這一版命名… and 版本紀錄, 預覽, 匯出): 預覽 shows what 匯出 will print, so the two
 * sit together. Anything bigger than a switch opens a popover or a dialog, so the sheet stays the
 * thing on screen. On a narrow pane the end group wraps under the start.
 */
export function WorkspaceToolbar({
  ws,
  level,
  audience,
  onAudience,
  locked,
  blockedReason,
  fill,
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
  /** Why the versions (名稱, 版本紀錄) cannot be touched right now; null: they can. */
  blockedReason: string | null;
  fill: VocabFill;
  onOpenDialog: (dialog: WorkspaceDialog, opener: HTMLElement | null) => void;
  /** The prep page: 在完整編輯頁開啟. */
  openFullHref?: string;
  /** The full page: 調整設定. */
  onAdjust?: () => void;
}) {
  const ids = useId();
  const state = ws.state;
  const [exportError, setExportError] = useState<string | null>(null);
  const [named, setNamed] = useState<{ versionNo: number; name: string } | null>(null);
  const vocabRef = useRef<HTMLButtonElement>(null);
  const previewRef = useRef<HTMLButtonElement>(null);
  if (!state) return null;
  const levels = state.content.levels;
  // 「已把第 N 版命名為…」 stays until the sheet moves on to another version.
  const namedNow = named && named.versionNo === state.version_no ? named : null;

  return (
    <div className="ws-bar-wrap">
      <div className="ws-bar" role="group" aria-label="學習單工具列" lang="zh-Hant">
        <div className="ws-bar-start">
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
          <DisplayPopover ws={ws} fill={fill} disabled={locked} />
          <button
            ref={vocabRef}
            type="button"
            className="ws-bar-tool"
            onClick={() => onOpenDialog("vocab", vocabRef.current)}
          >
            詞彙檢查
          </button>
          <InfoPopover content={state.content} jobs={state.jobs} teacherView={audience === "teacher"} />
          {(openFullHref || onAdjust) && (
            <ToolbarPopover label="更多…" panelLabel="更多工具" panelClassName="ws-more-panel">
              {(close) => (
                <>
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
          )}
        </div>
        <div className="ws-bar-end">
          <SaveStatus ws={ws} className="ws-bar-saved" />
          <VersionMenu
            ws={ws}
            levels={levels}
            blockedReason={blockedReason}
            onNamed={(n) => setNamed({ versionNo: n.version_no, name: n.name })}
          />
          <button
            ref={previewRef}
            type="button"
            className="ws-bar-tool"
            onClick={() => onOpenDialog("print", previewRef.current)}
          >
            預覽
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
        </div>
      </div>
      <ErrorText message={exportError} />
      <p className="quiet-note ws-bar-note" role="status">
        {namedNow ? `已把第 ${namedNow.versionNo} 版命名為「${namedNow.name}」，在版本紀錄裡找得到。` : ""}
      </p>
    </div>
  );
}
