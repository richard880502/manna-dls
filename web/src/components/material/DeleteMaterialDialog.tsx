import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { MaterialDetail } from "../../api/types";
import { ErrorText, Loading } from "../common";
import { Dialog } from "../Dialog";
import { deleteConsequences, titleOf } from "./MaterialLibrary";

/**
 * 永久刪除 one of the teacher's materials (我的教材, and 我的範本's 沒有範本在用的原檔): said in full
 * (what goes with it) before anything is sent.
 */
export function DeleteMaterialDialog({
  card,
  returnFocus,
  onDone,
  onClose,
}: {
  card: { id: number; title: string };
  returnFocus: () => HTMLElement | null;
  onDone: () => void;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<MaterialDetail | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .getLibraryMaterial(card.id)
      .then((m) => alive && setDetail(m))
      // Without the details the general warning is still said.
      .catch(() => alive && setLoadError(true));
    return () => {
      alive = false;
    };
  }, [card.id]);

  async function remove() {
    setBusy(true);
    setError(null);
    try {
      await api.deleteLibraryMaterial(card.id);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Dialog title={`永久刪除「${titleOf(card)}」？`} role="alertdialog" returnFocus={returnFocus} onClose={onClose}>
      <div className="material-delete">
        {detail ? (
          <p>{deleteConsequences(detail)}</p>
        ) : loadError ? (
          <p>刪除後無法復原：原檔、擷取的文字與段落和你的註記都會永久刪除。</p>
        ) : (
          <Loading />
        )}
        <div className="button-row">
          <button type="button" className="danger-button" disabled={busy} onClick={() => void remove()}>
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
