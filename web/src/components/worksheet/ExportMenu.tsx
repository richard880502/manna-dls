import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { WsExportPart, WsLevel } from "../../api/types";
import { saveBlob } from "../../lib/download";
import { Dialog, DialogCancel } from "../Dialog";

const ITEMS = [
  { id: "student-docx", label: "學生版 Word" },
  { id: "student-pdf", label: "學生版 PDF" },
  { id: "teacher-docx", label: "教師版 Word" },
  { id: "teacher-pdf", label: "教師版 PDF" },
] as const;

export function ExportMenu({
  worksheetId,
  level,
  levelLabel = null,
  hasPrepSession = false,
  disabled = false,
  beforeExport,
  onError,
  footnote,
}: {
  worksheetId: number;
  level: WsLevel;
  levelLabel?: string | null;
  hasPrepSession?: boolean;
  disabled?: boolean;
  beforeExport?: () => Promise<string | null | undefined | void>;
  onError: (message: string | null) => void;
  footnote?: string;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<WsExportPart[]>([]);
  const [activity, setActivity] = useState(false);
  const [activityError, setActivityError] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const returnFocus = useCallback(() => trigger.current, []);

  useEffect(() => {
    if (!open || !hasPrepSession) return undefined;
    let active = true;
    api.worksheetExportOptions(worksheetId).then(
      (options) => { if (active) setActivity(options.activity); },
      () => { if (active) setActivityError(true); },
    );
    return () => { active = false; };
  }, [open, hasPrepSession, worksheetId]);

  function toggle(part: WsExportPart) {
    setSelected((parts) => parts.includes(part) ? parts.filter((p) => p !== part) : [...parts, part]);
  }

  async function run() {
    if (selected.length === 0 || busy) return;
    setOpen(false);
    setBusy(true);
    onError(null);
    try {
      const failed = await beforeExport?.();
      if (failed) throw new Error(failed);
      const single = ITEMS.find(({ id }) => selected.length === 1 && selected[0] === id);
      const { blob, filename } = single
        ? await api.exportWorksheet(worksheetId, {
            format: single.id.endsWith("-pdf") ? "pdf" : "docx",
            level,
            audience: single.id.startsWith("student-") ? "student" : "teacher",
          })
        : await api.exportWorksheetBundle(worksheetId, level, selected);
      saveBlob(blob, filename);
    } catch (err) {
      onError(err instanceof Error && !("status" in err) ? `沒有匯出：${err.message}` : errorMessage(err));
    } finally {
      setBusy(false);
      trigger.current?.focus();
    }
  }

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className="secondary-button small-button"
        aria-haspopup="dialog"
        disabled={disabled || busy}
        onClick={() => {
          setSelected([]);
          setActivity(false);
          setActivityError(false);
          setOpen(true);
        }}
      >
        {busy ? "匯出中…" : "匯出"}
      </button>
      {open && (
        <Dialog
          title={levelLabel ? `匯出：${levelLabel}` : "匯出學習單"}
          initialFocus="input[type=checkbox]"
          onClose={() => setOpen(false)}
          returnFocus={returnFocus}
        >
          <fieldset className="ws-export-choices">
            <legend>要匯出的項目</legend>
            {ITEMS.map(({ id, label }) => (
              <label key={id}>
                <input type="checkbox" checked={selected.includes(id)} onChange={() => toggle(id)} />
                {label}
              </label>
            ))}
            {activity && (
              <label>
                <input type="checkbox" checked={selected.includes("activity")} onChange={() => toggle("activity")} />
                課堂活動 Word
              </label>
            )}
          </fieldset>
          {activityError && <p role="status">無法確認是否有可匯出的課堂活動。</p>}
          {footnote && <p className="quiet-note">{footnote}</p>}
          <div className="button-row">
            <DialogCancel />
            <button type="button" className="primary-button" disabled={selected.length === 0} onClick={() => void run()}>
              {selected.length > 1 ? "下載 ZIP" : "下載"}
            </button>
          </div>
        </Dialog>
      )}
    </>
  );
}
