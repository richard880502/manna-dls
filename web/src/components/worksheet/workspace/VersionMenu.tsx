import { useCallback, useId, useRef, useState } from "react";
import { api } from "../../../api/client";
import type { WsLevel, WsSnapshot } from "../../../api/types";
import { menuButtonOf } from "../../prep/SectionTools";
import { VersionHistoryDialog } from "../VersionHistory";
import type { WorksheetController } from "../useWorksheet";
import { NameVersionDialog } from "./NameVersionDialog";
import { ToolbarPopover } from "./ToolbarPopover";

/**
 * 「第 N 版 ▾」: everything about versions in one place. A named version is a version too, so naming
 * the current one and browsing all of them sit in the same menu (the Google Docs model: version
 * history, plus a name on the versions that matter). The dialogs live outside the popover, which is
 * gone by the time they open, and hand focus back to this button.
 */
export function VersionMenu({
  ws,
  levels,
  blockedReason,
  onNamed,
}: {
  ws: WorksheetController;
  levels: WsLevel[];
  /** Why the versions can't be touched right now (Manna or a chat turn is working); null: they can. */
  blockedReason: string | null;
  /** The current version got a name (the toolbar says so). */
  onNamed: (named: WsSnapshot) => void;
}) {
  const hint = useId();
  const [dialog, setDialog] = useState<"name" | "history" | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const returnFocus = useCallback(() => (opener.current?.isConnected ? opener.current : null), []);
  const state = ws.state;
  if (!state) return null;

  const open = (kind: "name" | "history", item: HTMLButtonElement, close: () => void) => {
    if (blockedReason) return;
    opener.current = menuButtonOf(item);
    close();
    setDialog(kind);
  };

  return (
    <>
      <ToolbarPopover
        label={
          <>
            第 {state.version_no} 版
            <span className="export-caret" aria-hidden="true">
              ▾
            </span>
          </>
        }
        ariaLabel={`版本：第 ${state.version_no} 版`}
        panelLabel="版本"
        className="ws-bar-tool"
        panelClassName="ws-version-panel"
      >
        {(close) => (
          <>
            {blockedReason && (
              <p className="quiet-note ws-version-blocked" id={hint}>
                {blockedReason}
              </p>
            )}
            <button
              type="button"
              className="popover-item"
              aria-disabled={blockedReason ? true : undefined}
              aria-describedby={blockedReason ? hint : undefined}
              onClick={(e) => open("name", e.currentTarget, close)}
            >
              為這一版命名…
            </button>
            <button
              type="button"
              className="popover-item"
              aria-disabled={blockedReason ? true : undefined}
              aria-describedby={blockedReason ? hint : undefined}
              onClick={(e) => open("history", e.currentTarget, close)}
            >
              版本紀錄
            </button>
          </>
        )}
      </ToolbarPopover>
      {dialog === "name" && (
        <NameVersionDialog
          worksheetId={state.id}
          baseVersionNo={state.version_no}
          returnFocus={returnFocus}
          onClose={() => setDialog(null)}
          flush={async () => {
            const failed = await ws.flush();
            if (failed) return { ok: false, error: failed };
            const versions = await api.worksheetVersions(state.id);
            return { ok: true, versionNo: versions[0]?.version_no };
          }}
          onSaved={(named) => {
            setDialog(null);
            onNamed(named);
          }}
        />
      )}
      {dialog === "history" && (
        <VersionHistoryDialog
          worksheetId={state.id}
          beforeOpen={ws.flush}
          levels={levels}
          returnFocus={returnFocus}
          onClose={() => setDialog(null)}
          onRestored={(done) => {
            setDialog(null);
            ws.adopt(done.worksheet);
          }}
        />
      )}
    </>
  );
}
