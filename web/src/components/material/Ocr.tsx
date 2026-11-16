import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, api, errorMessage } from "../../api/client";
import type { MaterialDetail, MaterialOcr } from "../../api/types";
import { OCR_RETRY, ocrConfig, ocrFailed, ocrFailedLine, ocrOpen, ocrProgressLine } from "../../lib/ocr";
import { ErrorText } from "../common";

type Progress = Pick<MaterialOcr, "done_pages" | "total_pages">;

/**
 * 「Manna 正在辨識掃描檔（第 n／m 頁）…」 in Manna's gold with a small bar of the pages done
 * (a moving bar while the number of pages is not known yet).
 */
export function OcrProgress({ ocr, line }: { ocr: Progress; line?: string }) {
  return (
    <div className="ocr-progress" role="status">
      <p className="manna-label">
        <span className="star is-working" aria-hidden="true" />
        {line ?? ocrProgressLine(ocr)}
      </p>
      {ocr.total_pages > 0 ? (
        <progress className="ocr-bar" max={ocr.total_pages} value={ocr.done_pages} aria-label="辨識進度" />
      ) : (
        <progress className="ocr-bar" aria-label="辨識進度" />
      )}
    </div>
  );
}

/**
 * While the material's OCR is queued or running, asks for the material again every few seconds
 * (one request at a time) and hands each answer to `onUpdate`; stops once it is done or failed.
 */
export function useOcrPoll(
  material: { id: number; ocr?: MaterialOcr } | null,
  onUpdate: (next: MaterialDetail) => void,
) {
  const open = ocrOpen(material?.ocr);
  const id = material?.id ?? null;
  const onUpdateRef = useRef(onUpdate);
  useEffect(() => {
    onUpdateRef.current = onUpdate;
  });
  useEffect(() => {
    if (!open || id === null) return undefined;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const next = () => {
      timer = setTimeout(() => {
        api
          .getLibraryMaterial(id)
          .then((m) => {
            if (!alive) return;
            onUpdateRef.current(m);
            if (ocrOpen(m.ocr)) next();
          })
          .catch(() => {
            // A missed poll is not a failure; the next one tries again.
            if (alive) next();
          });
      }, ocrConfig.pollMs);
    };
    next();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [open, id]);
}

/**
 * 用 OCR 重新讀取 / 再試一次: POST /api/materials/:id/ocr. The material it answers with (queued)
 * goes to `onStarted`; 409 (already reading), 400 and 503 (OCR is off here) come back as `error`.
 */
export function useOcrRerun(materialId: number, onStarted: (next: MaterialDetail) => void) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onStartedRef = useRef(onStarted);
  useEffect(() => {
    onStartedRef.current = onStarted;
  });
  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      onStartedRef.current(await api.runMaterialOcr(materialId));
    } catch (err) {
      setError(errorMessage(err));
      // Already being read (started elsewhere): its progress shows from here on.
      if (err instanceof ApiError && err.status === 409) {
        api
          .getLibraryMaterial(materialId)
          .then((m) => onStartedRef.current(m))
          .catch(() => undefined);
      }
    } finally {
      setBusy(false);
    }
  }, [materialId]);
  /** Forgets the last refusal (the run it was about has ended). */
  const clear = useCallback(() => setError(null), []);
  return { run, busy, error, clear };
}

/** 「辨識沒有成功：…」 with 再試一次 (when the teacher may start it again). */
export function OcrFailed({
  ocr,
  retry,
}: {
  ocr: Pick<MaterialOcr, "error">;
  retry?: { run: () => Promise<void>; busy: boolean; error: string | null; label?: string };
}) {
  return (
    <div className="ocr-failed">
      <p className="ocr-failed-line">{ocrFailedLine(ocr)}</p>
      {retry && (
        <button
          type="button"
          className="secondary-button small-button"
          disabled={retry.busy}
          onClick={() => void retry.run()}
        >
          {retry.busy ? "送出中…" : (retry.label ?? OCR_RETRY)}
        </button>
      )}
      {retry && <ErrorText message={retry.error} />}
    </div>
  );
}

/**
 * An upload whose text Manna is reading: the progress line, asked for again every few seconds,
 * then `onDone` with the material as it is now (text and all). A failed run says why and offers
 * 再試一次 (the uploader may always start it again).
 */
export function OcrFollow({ material, onDone }: { material: MaterialDetail; onDone: (next: MaterialDetail) => void }) {
  const [current, setCurrent] = useState(material);
  const onDoneRef = useRef(onDone);
  useEffect(() => {
    onDoneRef.current = onDone;
  });
  const update = useCallback((next: MaterialDetail) => {
    setCurrent(next);
    if (!ocrOpen(next.ocr) && !ocrFailed(next.ocr)) onDoneRef.current(next);
  }, []);
  useOcrPoll(current, update);
  const retry = useOcrRerun(current.id, update);
  const ocr = current.ocr;
  if (!ocr) return null;
  if (ocrOpen(ocr)) return <OcrProgress ocr={ocr} />;
  if (ocrFailed(ocr)) return <OcrFailed ocr={ocr} retry={retry} />;
  return null;
}
