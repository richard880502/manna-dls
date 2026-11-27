import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import { saveBlob } from "../../lib/download";
import { ErrorText } from "../common";

export const KIT_DOWNLOAD_LABEL = "下載教學套件 (DOCX)";
export const ACTIVITY_DOWNLOAD_LABEL = "下載課堂活動 (DOCX)";

/** A one-line note, a download button, and while it is not ready, why. */
function DocxDownload({
  label,
  note,
  ready,
  waiting,
  fetchFile,
}: {
  label: string;
  note: string;
  ready: boolean;
  /** Said under the button while it is not ready. */
  waiting?: string;
  fetchFile: () => Promise<{ blob: Blob; filename: string }>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function download() {
    setBusy(true);
    setError(null);
    try {
      const { blob, filename } = await fetchFile();
      saveBlob(blob, filename);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="prep-kit-head">
      <p>{note}</p>
      <button type="button" className="secondary-button" disabled={!ready || busy} onClick={() => void download()}>
        {busy ? "下載中…" : label}
      </button>
      {!ready && waiting && <p className="quiet-note">{waiting}</p>}
      <ErrorText message={error} />
    </div>
  );
}

/**
 * 教學套件: one DOCX with 教學流程, the student version of each level and 教師版 with answers and
 * Bloom tags. It can be downloaded once both halves are done (the server answers 409 before).
 */
export function KitDownload({ sessionId, turnId, ready }: { sessionId: number; turnId: number; ready: boolean }) {
  return (
    <DocxDownload
      label={KIT_DOWNLOAD_LABEL}
      note="活動流程＋差異化學習單＋教師版答案，一次下載"
      ready={ready}
      waiting="兩個部分都做好之後就可以下載。"
      fetchFile={() => api.exportPrepKit(sessionId, turnId)}
    />
  );
}

/**
 * 課堂活動: one DOCX with the whole plan, the same 教學流程 a 教學套件 starts with. Shown once the plan
 * is there (a plan is only stored when the turn is done).
 */
export function ActivityDownload({ sessionId, turnId }: { sessionId: number; turnId: number }) {
  return (
    <DocxDownload
      label={ACTIVITY_DOWNLOAD_LABEL}
      note="學習目標、課前準備、每一步和複習與評量，一份 Word 檔"
      ready
      fetchFile={() => api.exportPrepActivity(sessionId, turnId)}
    />
  );
}
