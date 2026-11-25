import { useId, useRef, useState } from "react";
import { api, errorMessage } from "../../../api/client";
import type { WsSnapshot } from "../../../api/types";
import { Dialog, DialogCancel, useDialogGuard } from "../../Dialog";
import { ErrorText } from "../../common";

/** 快照 names are at most this long (the server answers 400 beyond it). */
export const SNAPSHOT_NAME_MAX = 60;

/** What saving the pending edits came to: the latest version afterwards, or why it failed. */
export type FlushResult = { ok: true; versionNo?: number } | { ok: false; error: string };

/** 「快照 11/25 17:30」: the name the server fills in when none is typed (shown as the placeholder). */
export function defaultSnapshotName(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `快照 ${pad(now.getMonth() + 1)}/${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;
}

/**
 * 存成快照 (docs/WORKSHEET_WORKSPACE.md §2): a button and a small dialog that names the current
 * version. Pending edits are saved first (`flush`, which says the version they made), then the
 * latest version is labelled. Independent of the workspace internals so any toolbar can hold it.
 */
export function SnapshotButton({
  worksheetId,
  baseVersionNo,
  flush,
  disabled = false,
  disabledReason = null,
  onSaved,
  className = "secondary-button",
}: {
  worksheetId: number;
  /** The latest version the page knows (before `flush`). */
  baseVersionNo: number;
  /** Saves pending edits; resolves to the version number that is the latest afterwards. */
  flush?: () => Promise<FlushResult | void>;
  /** Manna or a chat turn is working: the button stays but says why it cannot be used. */
  disabled?: boolean;
  disabledReason?: string | null;
  onSaved?: (snapshot: WsSnapshot) => void;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const hint = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={className}
        aria-disabled={disabled || undefined}
        aria-describedby={disabled && disabledReason ? hint : undefined}
        title={disabled ? (disabledReason ?? undefined) : undefined}
        onClick={() => {
          if (disabled) return;
          setDone(null);
          setOpen(true);
        }}
      >
        存成快照
      </button>
      {disabled && disabledReason && (
        <span id={hint} className="sr-only">
          {disabledReason}
        </span>
      )}
      <span className="quiet-note" role="status" aria-live="polite">
        {done}
      </span>
      {open && (
        <Dialog title="存成快照" returnFocus={() => trigger.current} onClose={() => setOpen(false)}>
          <SnapshotForm
            worksheetId={worksheetId}
            baseVersionNo={baseVersionNo}
            flush={flush}
            onSaved={(snap) => {
              setOpen(false);
              setDone(`已存成快照「${snap.name}」`);
              onSaved?.(snap);
            }}
          />
        </Dialog>
      )}
    </>
  );
}

function SnapshotForm({
  worksheetId,
  baseVersionNo,
  flush,
  onSaved,
}: {
  worksheetId: number;
  baseVersionNo: number;
  flush?: () => Promise<FlushResult | void>;
  onSaved: (snapshot: WsSnapshot) => void;
}) {
  const id = useId();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [placeholder] = useState(() => defaultSnapshotName());
  useDialogGuard({ busy, dirty: false });

  async function save() {
    setBusy(true);
    setError(null);
    try {
      let base = baseVersionNo;
      const flushed = await flush?.();
      if (flushed && !flushed.ok) {
        setError(`沒有儲存成功，所以還不能存成快照：${flushed.error}`);
        return;
      }
      if (flushed?.versionNo !== undefined) base = flushed.versionNo;
      const trimmed = name.trim();
      onSaved(await api.createWorksheetSnapshot(worksheetId, { ...(trimmed ? { name: trimmed } : {}), base_version_no: base }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      lang="zh-Hant"
      onSubmit={(e) => {
        e.preventDefault();
        if (!busy) void save();
      }}
    >
      <p className="quiet-note">把現在這一版標上名字，之後可以在版本紀錄找到它、回到這個樣子。</p>
      <label htmlFor={`${id}-name`}>名稱（選填）</label>
      <input
        id={`${id}-name`}
        type="text"
        value={name}
        maxLength={SNAPSHOT_NAME_MAX}
        placeholder={placeholder}
        autoComplete="off"
        onChange={(e) => setName(e.target.value)}
      />
      <ErrorText message={error} />
      <div className="button-row">
        <button type="submit" className="primary-button" aria-disabled={busy || undefined}>
          {busy ? "儲存中…" : "存成快照"}
        </button>
        <DialogCancel disabled={busy} />
      </div>
    </form>
  );
}
