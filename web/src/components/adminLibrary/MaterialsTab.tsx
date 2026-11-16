import { useCallback, useEffect, useState } from "react";
import { adminLibraryApi } from "../../api/client";
import type { AdminLibraryMaterial, AdminLibraryMaterialRow } from "../../api/types";
import { formatDate } from "../../lib/labels";
import { formatBytes } from "../../lib/materials";
import { BulkUpload } from "./BulkUpload";
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
import { useUploadQueue, type QueueRow } from "./useUploadQueue";

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

/** What an uploaded row says: added, and whether its text could be read. */
function Uploaded({ material }: { material: AdminLibraryMaterial }) {
  const unreadable = material.text_status === "no_text" || material.text_status === "failed";
  return (
    <span className={`adlib-state ${unreadable ? "is-warn" : "is-done"}`}>
      已加入 · {materialMeta(material)}
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
          <p className="meta">
            {[
              m.category || "未分類",
              materialMeta(m),
              m.size_bytes > 0 ? formatBytes(m.size_bytes) : null,
              m.uploaded_by ? `${m.uploaded_by} 上傳` : null,
              formatDate(m.created_at),
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
        )}
        save={adminLibraryApi.updateMaterial}
        remove={adminLibraryApi.deleteMaterial}
        deleteNote="原檔、擷取的文字與段落都會永久刪除，老師就選不到它了。老師已經用它做好的學習單不受影響。"
      />
    </>
  );
}
