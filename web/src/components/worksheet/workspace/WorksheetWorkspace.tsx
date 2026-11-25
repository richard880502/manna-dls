import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Link } from "react-router";
import type { WsAudience, WsBlock, WsLevel } from "../../../api/types";
import { interactionFor } from "../../../lib/blockRating";
import { projectSheet } from "../../../lib/worksheet";
import { templateTitle } from "../../../lib/templates";
import { ErrorText } from "../../common";
import { ResultView, type BlockTarget } from "../ResultView";
import type { WorksheetController } from "../useWorksheet";
import { api } from "../../../api/client";
import { PrintPreviewDialog } from "./PrintPreviewDialog";
import { SnapshotButton } from "./SnapshotButton";
import { VocabCheckDialog } from "./VocabCheckDialog";
import { WorkspaceToolbar, type WorkspaceDialog } from "./WorkspaceToolbar";
import { useVocabFill } from "./useVocabFill";

/** What the prep conversation shares with the worksheet beside it. */
export interface WorkspaceChat {
  /** A chat turn of this conversation is running (or being sent): the sheet is read-only meanwhile. */
  busy: boolean;
  /** Blocks the last chat turn changed. */
  highlight: ReadonlySet<string>;
  /** The conversation turn's own AI call: what a block without a step of its own is rated on. */
  interactionId: number | null;
  /** Reports which letter each block prints with, for the replies that name blocks. */
  onLabels: (worksheetId: number, labels: Record<string, string>) => void;
  /** The block the bottom bar will revise (「針對 B 區」) and how 「改這一區」 points it at one. */
  blockTarget: BlockTarget;
  /** Changes when the bottom bar started a block revision here. */
  proposalsKey?: number;
  /** The teacher's own worksheet this one follows (照「…」的樣式); the turn knows it before the worksheet does. */
  template?: { id: number; title: string } | null;
}

/**
 * The worksheet, always editable: a toolbar, the editor (`ResultView`) and the dialogs the toolbar
 * opens (列印樣式預覽, 詞彙檢查; 版本紀錄 is the toolbar's own). One component for the prep page
 * (`chat` given: beside the conversation) and the full editor page (`onAdjust`: 調整設定).
 */
export function WorksheetWorkspace({
  ws,
  chat,
  onAdjust,
  snapshotSlot,
}: {
  ws: WorksheetController;
  chat?: WorkspaceChat;
  /** The full page: 調整設定 goes back to the options. */
  onAdjust?: () => void;
  /** Where 「存成快照」 goes in the toolbar. */
  snapshotSlot?: ReactNode;
}) {
  const state = ws.state;
  const [levelPick, setLevel] = useState<WsLevel | null>(null);
  const [audience, setAudience] = useState<WsAudience>("student");
  const [dialog, setDialog] = useState<{ kind: WorkspaceDialog; opener: HTMLElement | null } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const fill = useVocabFill(ws);
  const levels = state?.content.levels ?? [];
  const level: WsLevel = levelPick && levels.includes(levelPick) ? levelPick : (levels[0] ?? "standard");

  const jobs = state?.jobs;
  const fallback = chat?.interactionId ?? null;
  const interactionOf = useCallback(
    (block: WsBlock) => interactionFor(jobs ?? [], block, fallback),
    [jobs, fallback],
  );

  // The letters the blocks print with, for the conversation's replies (「B 區」).
  const onLabels = chat?.onLabels;
  const content = state?.content;
  const worksheetId = state?.id;
  useEffect(() => {
    if (!onLabels || !content || worksheetId === undefined) return;
    const sheet = projectSheet(content, level, "student");
    onLabels(worksheetId, Object.fromEntries(sheet.blocks.map((b) => [b.block.id, b.label])));
  }, [onLabels, content, worksheetId, level]);

  // Dialogs hand focus back to the button that asked (a menu item's own button is hidden by then).
  const opener = useRef<HTMLElement | null>(null);
  const returnFocus = useCallback(() => (opener.current?.isConnected ? opener.current : null), []);
  async function open(kind: WorkspaceDialog, from: HTMLElement | null) {
    // What the dialog shows is what is saved: pending edits go first.
    const failed = await ws.flush();
    if (failed) {
      setNotice(`沒有儲存成功，所以還不能打開：${failed}`);
      return;
    }
    setNotice(null);
    opener.current = from;
    setDialog({ kind, opener: from });
  }

  if (!state) return null;
  const template = chat?.template ?? state.template ?? null;
  const frozen = (chat?.busy ?? false) || state.running;
  const locked = frozen || ws.busy;
  const blockedReason = locked
    ? chat
      ? "備課對話正在處理，等這一輪結束後再看版本紀錄。"
      : "Manna 正在處理這份學習單，好了之後再看版本紀錄。"
    : null;

  // 存成快照: pending edits are saved first, then the version that is now the latest is labelled.
  const snapshot = snapshotSlot ?? (
    <SnapshotButton
      worksheetId={state.id}
      baseVersionNo={state.version_no}
      className="decision-button"
      disabled={locked}
      disabledReason={
        locked
          ? chat
            ? "備課對話正在處理，等這一輪結束後再存快照。"
            : "Manna 正在處理這份學習單，好了之後再存快照。"
          : null
      }
      flush={async () => {
        const failed = await ws.flush();
        if (failed) return { ok: false, error: failed };
        const versions = await api.worksheetVersions(state.id);
        return { ok: true, versionNo: versions[0]?.version_no };
      }}
    />
  );

  return (
    <div className="ws-workspace">
      {chat && template && (
        <p className="prep-ws-template">
          照「<Link to={`/templates/${template.id}`}>{templateTitle(template)}</Link>」的樣式
        </p>
      )}
      <WorkspaceToolbar
        ws={ws}
        level={level}
        audience={audience}
        onAudience={setAudience}
        locked={locked}
        blockedReason={blockedReason}
        fill={fill}
        snapshotSlot={snapshot}
        onOpenDialog={(kind, from) => void open(kind, from)}
        openFullHref={chat ? `/worksheets/${state.id}` : undefined}
        onAdjust={chat ? undefined : onAdjust}
      />
      <ErrorText message={notice} />
      {fill.notice && (
        <p className="field-hint ws-fill-notice" role="status" lang="zh-Hant">
          {fill.notice}
        </p>
      )}
      {chat && frozen && (
        <p className="prep-sheet-frozen" role="status">
          Manna 正在改這份學習單，完成後就能繼續編輯。
        </p>
      )}
      <ResultView
        ws={ws}
        level={level}
        onLevel={setLevel}
        audience={audience}
        embedded={chat !== undefined}
        frozen={frozen}
        interactionOf={interactionOf}
        highlight={chat?.highlight}
        blockTarget={chat?.blockTarget}
        reviseReloadKey={chat?.proposalsKey}
      />
      {dialog?.kind === "print" && (
        <PrintPreviewDialog
          worksheetId={state.id}
          levels={levels}
          level={level}
          audience={audience}
          returnFocus={returnFocus}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "vocab" && (
        <VocabCheckDialog
          worksheetId={state.id}
          returnFocus={returnFocus}
          onClose={(changed) => {
            setDialog(null);
            if (changed) void ws.refresh();
          }}
        />
      )}
    </div>
  );
}
