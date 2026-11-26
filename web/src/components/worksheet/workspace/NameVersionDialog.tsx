import { useId, useState } from "react";
import { api, errorMessage } from "../../../api/client";
import type { WsSnapshot } from "../../../api/types";
import { Dialog, DialogCancel, useDialogGuard } from "../../Dialog";
import { ErrorText } from "../../common";

/** Version names are at most this long (the server answers 400 beyond it). */
export const VERSION_NAME_MAX = 60;

/** What saving the pending edits came to: the latest version afterwards, or why it failed. */
export type FlushResult = { ok: true; versionNo?: number } | { ok: false; error: string };

/** 「版本 11/25 17:30」: the name the server gives when none is typed (shown as the placeholder). */
export function defaultVersionName(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `版本 ${pad(now.getMonth() + 1)}/${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}`;
}

/**
 * 為這一版命名 (docs/WORKSHEET_WORKSPACE.md §2): a named version is still just a version — the name
 * marks it in 版本紀錄 so it is easy to find among the autosaved ones. Pending edits are saved first
 * (`flush`, which says the version they made), then the latest version gets the name. The API and the
 * research events still call it a snapshot (`worksheet_snapshots`, `worksheet_snapshot_saved`).
 */
export function NameVersionDialog({
  worksheetId,
  baseVersionNo,
  flush,
  returnFocus,
  onClose,
  onSaved,
}: {
  worksheetId: number;
  /** The latest version the page knows (before `flush`). */
  baseVersionNo: number;
  /** Saves pending edits; resolves to the version number that is the latest afterwards. */
  flush?: () => Promise<FlushResult | void>;
  returnFocus?: () => HTMLElement | null;
  onClose: () => void;
  onSaved: (named: WsSnapshot) => void;
}) {
  return (
    <Dialog title="為這一版命名" returnFocus={returnFocus} onClose={onClose}>
      <NameVersionForm worksheetId={worksheetId} baseVersionNo={baseVersionNo} flush={flush} onSaved={onSaved} />
    </Dialog>
  );
}

function NameVersionForm({
  worksheetId,
  baseVersionNo,
  flush,
  onSaved,
}: {
  worksheetId: number;
  baseVersionNo: number;
  flush?: () => Promise<FlushResult | void>;
  onSaved: (named: WsSnapshot) => void;
}) {
  const id = useId();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [placeholder] = useState(() => defaultVersionName());
  useDialogGuard({ busy, dirty: false });

  async function save() {
    setBusy(true);
    setError(null);
    try {
      let base = baseVersionNo;
      const flushed = await flush?.();
      if (flushed && !flushed.ok) {
        setError(`沒有儲存成功，所以還不能命名：${flushed.error}`);
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
      <p className="quiet-note">
        幫現在這一版取個名字（例如「上課用定稿」），之後在版本紀錄裡很快就能找到它、回到這個樣子。
      </p>
      <label htmlFor={`${id}-name`}>名稱（選填）</label>
      <input
        id={`${id}-name`}
        type="text"
        value={name}
        maxLength={VERSION_NAME_MAX}
        placeholder={placeholder}
        autoComplete="off"
        onChange={(e) => setName(e.target.value)}
      />
      <ErrorText message={error} />
      <div className="button-row">
        <button type="submit" className="primary-button" aria-disabled={busy || undefined}>
          {busy ? "儲存中…" : "儲存名稱"}
        </button>
        <DialogCancel disabled={busy} />
      </div>
    </form>
  );
}
