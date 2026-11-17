import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import { saveBlob } from "../../lib/download";
import { ErrorText } from "../common";

export const KIT_DOWNLOAD_LABEL = "下載教學套件 (DOCX)";

/**
 * 教學套件: one DOCX with 教學流程, the student version of each level and 教師版 with answers and
 * Bloom tags. It can be downloaded once both halves are done (the server answers 409 before).
 */
export function KitDownload({ sessionId, turnId, ready }: { sessionId: number; turnId: number; ready: boolean }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function download() {
    setBusy(true);
    setError(null);
    try {
      const { blob, filename } = await api.exportPrepKit(sessionId, turnId);
      saveBlob(blob, filename);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="prep-kit-head">
      <p>活動流程＋差異化學習單＋教師版答案，一次下載</p>
      <button type="button" className="secondary-button" disabled={!ready || busy} onClick={() => void download()}>
        {busy ? "下載中…" : KIT_DOWNLOAD_LABEL}
      </button>
      {!ready && <p className="quiet-note">兩個部分都做好之後就可以下載。</p>}
      <ErrorText message={error} />
    </div>
  );
}
