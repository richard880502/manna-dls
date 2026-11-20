import { useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { SopSyncPreview } from "../../api/types";
import { ErrorText } from "../common";
import type { WorksheetController } from "./useWorksheet";

const ACTION_LABEL = { add: "新增到課例", update: "更新課例", remove: "從課例拿掉" } as const;

/**
 * Spec §9.5: what is changed in the worksheet never reaches the lesson by itself. One line says
 * the two differ; 「同步回課例」 shows exactly what would change, and only 「確認同步到課例」
 * writes it (as a new version of the lesson SOP).
 */
export function SopSyncNotice({ ws }: { ws: WorksheetController }) {
  const state = ws.state;
  const [dismissed, setDismissed] = useState(false);
  const [preview, setPreview] = useState<SopSyncPreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  if (!state) return null;
  const differs = state.sop_sync.linked && state.sop_sync.changes > 0;

  async function open() {
    setError(null);
    setLoading(true);
    try {
      const failed = await ws.flush();
      if (failed) throw new Error(failed);
      setPreview(await api.worksheetSopSyncPreview(state!.id));
    } catch (err) {
      setError(err instanceof Error && !("status" in err) ? `沒有儲存成功：${err.message}` : errorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      {done && (
        <p className="notice notice-success" role="status">
          已同步，課例多了一個版本。
        </p>
      )}
      {differs && !dismissed && !done && (
        <p className="ws-sync-line" role="status">
          <span>學習單的目標或任務和課例不同，課例還是原本的。</span>
          <button type="button" className="text-link" disabled={loading} onClick={() => void open()}>
            {loading ? "讀取中…" : "同步回課例"}
          </button>
          <button type="button" className="text-link is-plain" onClick={() => setDismissed(true)}>
            不用
          </button>
        </p>
      )}
      <ErrorText message={error} />
      {preview && (
        <SyncDialog
          worksheetId={state.id}
          preview={preview}
          onCancel={() => setPreview(null)}
          onDone={() => {
            setPreview(null);
            setDone(true);
            void ws.refresh();
          }}
        />
      )}
    </>
  );
}

function SyncDialog({
  worksheetId,
  preview,
  onCancel,
  onDone,
}: {
  worksheetId: number;
  preview: SopSyncPreview;
  onCancel: () => void;
  onDone: () => void;
}) {
  const ids = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(preview.changes.filter((c) => c.action !== "remove").map((c) => c.key)),
  );
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    dialogRef.current?.querySelector<HTMLElement>("input, button")?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !sending) onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [sending, onCancel]);

  async function confirm() {
    setSending(true);
    setError(null);
    try {
      await api.applyWorksheetSopSync(worksheetId, [...picked], preview);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setSending(false);
    }
  }

  return (
    <div className="scrim">
      <div
        className="dialog ws-sync-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${ids}-title`}
        aria-describedby={`${ids}-why`}
        ref={dialogRef}
      >
        <h2 id={`${ids}-title`}>同步回課例</h2>
        <p className="quiet-note" id={`${ids}-why`}>
          勾選的項目會寫進課例，成為課例的新版本；原本的版本還在。沒有按「確認同步到課例」之前，課例不會改變。
        </p>
        {preview.changes.length === 0 ? (
          <p className="empty-line">目前沒有和課例不同的地方。</p>
        ) : (
          <ul className="ws-sync-list">
            {preview.changes.map((c) => (
              <li key={c.key}>
                <label className="check-row">
                  <input
                    type="checkbox"
                    checked={picked.has(c.key)}
                    disabled={sending}
                    onChange={() =>
                      setPicked((prev) => {
                        const next = new Set(prev);
                        if (next.has(c.key)) next.delete(c.key);
                        else next.add(c.key);
                        return next;
                      })
                    }
                  />
                  <span>
                    {ACTION_LABEL[c.action]}：{c.label}
                  </span>
                </label>
                <dl className="ws-diff">
                  <div>
                    <dt>原本</dt>
                    <dd>{c.before || "（沒有）"}</dd>
                  </div>
                  <div>
                    <dt>改成</dt>
                    <dd>{c.action === "remove" ? "（拿掉）" : c.after || "（空白）"}</dd>
                  </div>
                </dl>
              </li>
            ))}
          </ul>
        )}
        <ErrorText message={error} />
        <div className="actions">
          <button
            type="button"
            className="primary-button"
            disabled={sending || picked.size === 0}
            onClick={() => void confirm()}
          >
            {sending ? "同步中…" : "確認同步到課例"}
          </button>
          <button type="button" className="text-link is-plain" disabled={sending} onClick={onCancel}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}
