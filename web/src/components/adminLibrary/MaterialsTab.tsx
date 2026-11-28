import { useCallback, useEffect, useState } from "react";
import { adminLibraryApi } from "../../api/client";
import type { AdminLibraryMaterial, AdminLibraryMaterialRow } from "../../api/types";
import { formatDate } from "../../lib/labels";
import { formatBytes } from "../../lib/materials";
import { ocrBadge, ocrFailed, ocrFailedLine, ocrOpen } from "../../lib/ocr";
import { BulkUpload } from "./BulkUpload";
import { PLATFORM_MATERIAL_DELETE_NOTE } from "../library/PlatformDeleteDialog";
import { LibraryList, useAdminList, type ListFilters } from "./LibraryList";
import {
  MATERIAL_CATEGORIES,
  MATERIAL_EXTENSIONS,
  guessMaterialCategory,
  guessTags,
  materialMeta,
  parseTags,
  tagsProblem,
  tagsText,
  titleFromPath,
  titleProblem,
} from "./prefill";
import { useListPoll } from "./useListPoll";
import { useUploadQueue, type QueueRow } from "./useUploadQueue";
import { MetaList } from "../MetaList";

const prefill = (path: string) => ({
  title: titleFromPath(path),
  category: guessMaterialCategory(path),
  tags: tagsText(guessTags(path)),
});

function validate(row: QueueRow<AdminLibraryMaterial>): string | null {
  return (
    titleProblem(row.title) ?? (row.category === "" ? "請選類別。" : null) ?? tagsProblem(parseTags(row.tags))
  );
}

const upload = (row: QueueRow<AdminLibraryMaterial>) =>
  adminLibraryApi.uploadMaterial(row.file, {
    title: row.title.trim(),
    category: row.category,
    tags: parseTags(row.tags),
  });

const ocrChanged = (next: AdminLibraryMaterial, cur: AdminLibraryMaterial) =>
  next.ocr !== undefined &&
  (next.ocr.status !== cur.ocr?.status ||
    next.ocr.done_pages !== cur.ocr.done_pages ||
    next.ocr.total_pages !== cur.ocr.total_pages);

/** 「辨識中 2／8」 while Manna reads a scan; 「辨識失敗」 with the reason. */
function OcrState({ material, prefix = "" }: { material: AdminLibraryMaterial; prefix?: string }) {
  const { ocr } = material;
  if (ocrOpen(ocr)) {
    return (
      <span className="adlib-state is-working">
        <span className="star is-working" aria-hidden="true" />
        {prefix}
        {ocrBadge(ocr)}
      </span>
    );
  }
  if (ocr && ocrFailed(ocr)) {
    return (
      <span className="adlib-state is-failed" title={ocr.error ?? undefined}>
        {prefix}
        {ocrFailedLine(ocr)}
      </span>
    );
  }
  return null;
}

/** What an uploaded row says: added, and whether its text could be read (or is being read). */
function Uploaded({ material }: { material: AdminLibraryMaterial }) {
  if (ocrOpen(material.ocr) || ocrFailed(material.ocr)) return <OcrState material={material} prefix="已加入：" />;
  const unreadable = material.text_status === "no_text" || material.text_status === "failed";
  return (
    <span className={`adlib-state ${unreadable ? "is-warn" : "is-done"}`}>
      已加入：{materialMeta(material).join("，")}
    </span>
  );
}

/** 知識庫教材: bulk upload, then the platform materials with edit and delete. */
export function MaterialsTab({ onBusy }: { onBusy?: (busy: boolean) => void }) {
  const [revision, setRevision] = useState(0);
  const [filters, setFilters] = useState<ListFilters>({ q: "", category: "" });
  const list = useAdminList<AdminLibraryMaterialRow>(adminLibraryApi.listMaterials, filters, revision);
  const onDrained = useCallback((uploaded: number) => {
    if (uploaded > 0) setRevision((r) => r + 1);
  }, []);
  const queue = useUploadQueue<AdminLibraryMaterial>({
    extensions: MATERIAL_EXTENSIONS,
    prefill,
    validate,
    upload,
    onDrained,
  });
  const { busy } = queue;
  useEffect(() => onBusy?.(busy), [onBusy, busy]);
  const { patch } = list;
  const { patchResults } = queue;

  /** Materials the server reported again: the upload rows and the list take their OCR progress. */
  const apply = useCallback(
    (items: AdminLibraryMaterial[]) => {
      const byId = new Map(items.map((m) => [m.id, m]));
      patch((x) => {
        const n = byId.get(x.id);
        return n && ocrChanged(n, x) ? { ...x, ...n } : x;
      });
      patchResults((r) => {
        const n = byId.get(r.id);
        return n && ocrChanged(n, r) ? { ...r, ...n } : r;
      });
    },
    [patch, patchResults],
  );
  // While Manna reads a scanned upload, the list is asked for again every few seconds.
  const reading =
    (list.items ?? []).some((m) => ocrOpen(m.ocr)) || queue.rows.some((r) => ocrOpen(r.result?.ocr));
  useListPoll(reading, adminLibraryApi.listMaterials, apply);

  return (
    <>
      <BulkUpload
        queue={queue}
        label="知識庫教材"
        categories={MATERIAL_CATEGORIES}
        categoryRequired
        extensions={MATERIAL_EXTENSIONS}
        hint="文件（.txt、.docx、.pdf）、圖片與影音。標題、類別與標籤會依檔名和資料夾名稱先填好（例如「學測英文/114/…試題」→ 學測英文、114、試題），上傳前都可以改。"
        renderDone={(row) => row.result && <Uploaded material={row.result} />}
      />
      <LibraryList<AdminLibraryMaterialRow>
        label="知識庫教材"
        categories={MATERIAL_CATEGORIES}
        allowAuto={false}
        list={list}
        filters={filters}
        onFilters={setFilters}
        renderMeta={(m) => (
          <MetaList
            className="meta"
            items={[
              (ocrOpen(m.ocr) || ocrFailed(m.ocr)) && <OcrState key="ocr" material={m} />,
              m.category || "未分類",
              ...materialMeta(m),
              m.size_bytes > 0 ? formatBytes(m.size_bytes) : null,
              m.uploaded_by ? `${m.uploaded_by} 上傳` : null,
              formatDate(m.created_at),
            ]}
          />
        )}
        save={adminLibraryApi.updateMaterial}
        remove={adminLibraryApi.deleteMaterial}
        deleteNote={PLATFORM_MATERIAL_DELETE_NOTE}
      />
    </>
  );
}
