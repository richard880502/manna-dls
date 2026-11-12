import { useId, useState } from "react";
import type { Polarity } from "../../api/types";
import { POLARITY_LABELS } from "../../lib/labels";
import type { SyncState, ViewEvent } from "../../lib/recordQueue";

/** Sync status line with the pending count (書籤先存裝置端). */
export function SyncStatus({ state, onRetry }: { state: SyncState; onRetry: () => void }) {
  const { pending, syncing, offline, stopped, error } = state;
  let text: string;
  let tone: "ok" | "busy" | "warn";
  if (pending === 0 && !stopped) {
    text = "全部已同步";
    tone = "ok";
  } else if (stopped) {
    text = `${pending} 筆待同步：已暫停同步`;
    tone = "warn";
  } else if (offline) {
    text = `${pending} 筆待同步：目前離線，書籤先存在這台裝置，恢復連線會自動同步`;
    tone = "warn";
  } else {
    text = syncing ? `${pending} 筆待同步：同步中…` : `${pending} 筆待同步`;
    tone = "busy";
  }
  return (
    <div className={`sync-status sync-${tone}`} role="status">
      <span className="sync-dot" aria-hidden="true" />
      <span>{text}</span>
      {(offline || stopped) && pending > 0 && (
        <button type="button" className="ghost-button small-button" onClick={onRetry}>
          立即重試
        </button>
      )}
      {error && <span className="sync-error">{error}</span>}
    </div>
  );
}

interface EventControlsProps {
  event: ViewEvent;
  onEdit: (change: { polarity?: Polarity; note?: string }) => void;
  onDelete: () => void;
}

/** 改 順利／卡住、改一句話、刪除誤按. Every change is kept as a revision on the server. */
export function EventControls({ event, onEdit, onDelete }: EventControlsProps) {
  const noteId = useId();
  const [editing, setEditing] = useState(false);
  const [note, setNote] = useState(event.note);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const other: Polarity = event.polarity === "good" ? "stuck" : "good";

  return (
    <div className="event-controls">
      <div className="event-control-buttons">
        <button type="button" className="ghost-button small-button" onClick={() => onEdit({ polarity: other })}>
          改成{POLARITY_LABELS[other]}
        </button>
        <button
          type="button"
          className="ghost-button small-button"
          onClick={() => {
            setNote(event.note);
            setEditing((v) => !v);
          }}
          aria-expanded={editing}
        >
          {event.note ? "改一句話" : "加一句話"}
        </button>
        {confirmDelete ? (
          <span className="confirm-row">
            確定刪除這個書籤？
            <button
              type="button"
              className="ghost-button small-button"
              onClick={() => {
                setConfirmDelete(false);
                onDelete();
              }}
            >
              確定刪除
            </button>
            <button type="button" className="ghost-button small-button" onClick={() => setConfirmDelete(false)}>
              取消
            </button>
          </span>
        ) : (
          <button type="button" className="ghost-button small-button" onClick={() => setConfirmDelete(true)}>
            刪除
          </button>
        )}
      </div>
      {editing && (
        <div className="event-note-edit">
          <label className="field-label" htmlFor={noteId}>
            一句話
          </label>
          <div className="live-note-row">
            <input id={noteId} value={note} maxLength={300} onChange={(e) => setNote(e.target.value)} />
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                onEdit({ note: note.trim() });
                setEditing(false);
              }}
            >
              儲存
            </button>
          </div>
          <p className="field-hint">不要寫學生姓名。</p>
        </div>
      )}
    </div>
  );
}
