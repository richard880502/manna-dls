import { useState, type ChangeEvent } from "react";
import { errorMessage } from "../../api/client";
import type { ImportResult } from "../../api/types";
import { ErrorText, Notice } from "../common";

export function CsvImport({
  title,
  columns,
  inputId,
  upload,
}: {
  title: string;
  columns: string;
  inputId: string;
  upload: (csv: string) => Promise<ImportResult>;
}) {
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  function onFile(e: ChangeEvent<HTMLInputElement>) {
    setFile(e.target.files?.[0] ?? null);
    setResult(null);
    setError(null);
  }

  async function onUpload() {
    if (!file) {
      setError("請先選擇 CSV 檔案。");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const text = await file.text();
      setResult(await upload(text));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel-card" aria-labelledby={`${inputId}-heading`}>
      <h2 id={`${inputId}-heading`}>{title}</h2>
      <p className="field-hint">
        CSV（UTF-8），欄位：<code>{columns}</code>
      </p>
      <label className="field-label" htmlFor={inputId}>
        選擇 CSV 檔案
      </label>
      <input id={inputId} type="file" accept=".csv,text/csv" onChange={onFile} />
      <div className="button-row admin-actions">
        <button type="button" className="secondary-button" onClick={() => void onUpload()} disabled={busy}>
          {busy ? "匯入中…" : "匯入"}
        </button>
      </div>
      {result && (
        <Notice tone="success">
          已匯入 {result.imported} 筆。來源 SHA-256：<code className="sha">{result.sha256}</code>
        </Notice>
      )}
      <ErrorText message={error} />
    </section>
  );
}
