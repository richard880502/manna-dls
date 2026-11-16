// 掃描檔與 OCR (docs/API.md「Phase 2.10」): what the interface says while Manna reads a scanned
// file, and the small rules it checks before offering anything. The server decides; these only
// mirror its words (the 409 line, 「第 n／m 頁」).
import { ApiError } from "../api/client";
import type { MaterialKind, MaterialOcr } from "../api/types";

/** How often a material being read is asked for again. Tests shorten it. */
export const ocrConfig = { pollMs: 3000 };

export const OCR_LABEL = "由 OCR 辨識";
export const OCR_RERUN = "用 OCR 重新讀取";
export const OCR_RERUN_AGAIN = "重新辨識";
export const OCR_RETRY = "再試一次";
export const OCR_NOT_YET = "辨識完成後才能選用";
export const OCR_FAILED_BADGE = "辨識失敗";

type Progress = Pick<MaterialOcr, "done_pages" | "total_pages">;

/** Queued or running: the material cannot be used yet. A server without OCR sends no `ocr`. */
export const ocrOpen = (ocr: Pick<MaterialOcr, "status"> | null | undefined): boolean =>
  ocr?.status === "queued" || ocr?.status === "running";

export const ocrFailed = (ocr: Pick<MaterialOcr, "status"> | null | undefined): boolean => ocr?.status === "failed";

/** The page being read now: one past the pages done, never past the last (the server's 「第 n 頁」). */
export const ocrPage = (p: Progress): number => Math.max(1, Math.min(p.done_pages + 1, p.total_pages));

/** 「Manna 正在辨識掃描檔（第 2／8 頁）…」; without the pages while their number is not known. */
export function ocrProgressLine(p: Progress): string {
  return p.total_pages > 0
    ? `Manna 正在辨識掃描檔（第 ${ocrPage(p)}／${p.total_pages} 頁）…`
    : "Manna 正在辨識掃描檔…";
}

/** A 照我的學習單做 turn waiting for its sample: 「…（第 2／8 頁），接著讀學習單設計…」. */
export function ocrPrepLine(p: Progress): string {
  return p.total_pages > 0
    ? `Manna 正在辨識掃描檔（第 ${ocrPage(p)}／${p.total_pages} 頁），接著讀學習單設計…`
    : "Manna 正在辨識掃描檔，接著讀學習單設計…";
}

/** 「辨識中 2／8」 on a card or an admin row; 「辨識失敗」; null otherwise. */
export function ocrBadge(ocr: MaterialOcr | null | undefined): string | null {
  if (!ocr) return null;
  if (ocrOpen(ocr)) return ocr.total_pages > 0 ? `辨識中 ${ocrPage(ocr)}／${ocr.total_pages}` : "辨識中";
  if (ocrFailed(ocr)) return OCR_FAILED_BADGE;
  return null;
}

/** 「辨識沒有成功：…」 */
export const ocrFailedLine = (ocr: Pick<MaterialOcr, "error">): string =>
  `辨識沒有成功：${ocr.error?.trim() || "原因不明。"}`;

/** What the server answers (409) when the material is used while it is read; said here before asking. */
export function ocrBusyLine(p: Progress): string {
  return p.total_pages > 0
    ? `這份檔案正在辨識文字（第 ${ocrPage(p)}／${p.total_pages} 頁），完成後再試一次。`
    : "這份檔案正在辨識文字，完成後再試一次。";
}

/** The server's message when starting something failed because the material is still being read (409). */
export function ocrBusyError(err: unknown): string | null {
  return err instanceof ApiError && err.status === 409 && err.message.includes("正在辨識文字") ? err.message : null;
}

/** An uploaded PDF or image (with its file): the only materials OCR reads. */
export function canRunOcr(m: {
  kind?: MaterialKind | string;
  has_file?: boolean;
  content_type?: string | null;
  filename?: string | null;
}): boolean {
  if (!m.has_file) return false;
  if (m.kind === "image") return true;
  return m.content_type === "application/pdf" || (m.filename ?? "").toLowerCase().endsWith(".pdf");
}
