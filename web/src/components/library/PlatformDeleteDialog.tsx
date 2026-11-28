import { useState } from "react";
import { errorMessage } from "../../api/client";
import { ErrorText } from "../common";
import { Dialog } from "../Dialog";

/** What deleting a platform material does (教材與範本管理 and the 平台教材 cards say the same). */
export const PLATFORM_MATERIAL_DELETE_NOTE =
  "原檔、擷取的文字與段落都會永久刪除，老師就選不到它了。老師已經用它做好的學習單不受影響。";
/** What deleting a platform template does (教材與範本管理 and the 平台範本 cards say the same). */
export const PLATFORM_TEMPLATE_DELETE_NOTE =
  "這個平台範本會永久刪除，老師就選不到它了。老師已經照它做好的學習單不受影響。";

/**
 * 刪除 on a 平台教材 or 平台範本 card, for `manage_knowledge` accounts only (the caller checks):
 * the same delete as 教材與範本管理 (`DELETE /api/admin/library/…`), said first in a confirmation.
 */
export function PlatformDeleteDialog({
  title,
  note,
  remove,
  returnFocus,
  onDone,
  onClose,
}: {
  /** The card's title as shown. */
  title: string;
  note: string;
  remove: () => Promise<unknown>;
  returnFocus: () => HTMLElement | null;
  onDone: () => void;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onConfirm() {
    setBusy(true);
    setError(null);
    try {
      await remove();
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Dialog title={`永久刪除「${title}」？`} role="alertdialog" returnFocus={returnFocus} onClose={onClose} busy={busy}>
      <div className="material-delete">
        <p>{note}</p>
        <p>刪除後無法復原。</p>
        <div className="button-row">
          <button type="button" className="danger-button" disabled={busy} onClick={() => void onConfirm()}>
            {busy ? "刪除中…" : "永久刪除"}
          </button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={onClose}>
            取消
          </button>
        </div>
        <ErrorText message={error} />
      </div>
    </Dialog>
  );
}
