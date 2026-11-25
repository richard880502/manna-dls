import { formatClock } from "../../../lib/worksheet";
import type { WorksheetController } from "../useWorksheet";

/** 已自動儲存 / 儲存中… / 沒有存到，with the way on when it did not work out. */
export function SaveStatus({ ws, className = "" }: { ws: WorksheetController; className?: string }) {
  const state = ws.state;
  if (!state) return null;
  const line = ws.saveError
    ? `沒有存到：${ws.saveError}`
    : ws.saving
      ? "儲存中…"
      : ws.dirty
        ? "尚未儲存"
        : `已自動儲存 ${formatClock(ws.savedAt ?? state.saved_at)}`;
  return (
    <p className={`saved${className ? ` ${className}` : ""}${ws.saveError ? " is-failed" : ""}`} role="status">
      {line}
      {ws.conflict ? (
        <>
          {" "}
          <button type="button" className="text-link is-plain" onClick={ws.reload}>
            重新載入
          </button>
        </>
      ) : (
        ws.saveError && (
          <>
            {" "}
            <button type="button" className="text-link is-plain" onClick={() => void ws.flush()}>
              再試一次
            </button>
          </>
        )
      )}
    </p>
  );
}
