import { useState, type DragEvent, type ReactNode } from "react";
import { Icon } from "../Icon";
import { dropEntries, filesFromDrop, fromFileList, type PickedFile } from "./files";
import { AUTO_CATEGORY_LABEL } from "./prefill";
import type { QueueRow, UploadQueue } from "./useUploadQueue";

const STATUS_LABEL = {
  ready: "待上傳",
  queued: "排隊中",
  uploading: "上傳中…",
  failed: "上傳失敗",
} as const;

/** 「略過 2 個不支援的檔案：a.doc、b.pptx」, the first few named. */
function unsupportedNote(paths: string[]): string | null {
  if (paths.length === 0) return null;
  const named = paths.slice(0, 3).join("、");
  return `略過 ${paths.length} 個不支援的檔案：${named}${paths.length > 3 ? "…" : ""}`;
}

/**
 * The bulk upload of one tab: a drop zone (files or whole folders), a file and a folder picker,
 * one row per file with its title, category and tags already filled in from the name and folders,
 * and 全部上傳. Rows that are done say what became of them (`renderDone`).
 */
export function BulkUpload<R>({
  queue,
  label,
  categories,
  categoryRequired,
  extensions,
  hint,
  renderDone,
}: {
  queue: UploadQueue<R>;
  /** 知識庫教材 or 平台範本: names the controls. */
  label: string;
  categories: readonly string[];
  /** Materials need one; a template may leave it to 自動判斷. */
  categoryRequired: boolean;
  extensions: readonly string[];
  hint: ReactNode;
  renderDone: (row: QueueRow<R>) => ReactNode;
}) {
  const [dragging, setDragging] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const { rows } = queue;

  function take(picked: PickedFile[]) {
    const { added, unsupported } = queue.add(picked);
    const skipped = unsupportedNote(unsupported);
    setNote(skipped ?? (added === 0 && picked.length > 0 ? "這些檔案已經在清單上了。" : null));
  }

  function onDrop(e: DragEvent<HTMLDivElement>) {
    e.preventDefault();
    setDragging(false);
    // The entries must be read while the drop is being handled; the walk can finish later.
    const entries = dropEntries(e.dataTransfer);
    const files = e.dataTransfer.files;
    void filesFromDrop(entries, files).then(take, () => take(fromFileList(files)));
  }

  function onDragOver(e: DragEvent<HTMLDivElement>) {
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    if (!dragging) setDragging(true);
  }

  function onDragLeave(e: DragEvent<HTMLDivElement>) {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false);
  }

  const accept = extensions.map((x) => `.${x}`).join(",");
  const count = (s: QueueRow<R>["status"]) => rows.filter((r) => r.status === s).length;
  const done = count("done");
  const failed = count("failed");
  const moving = count("queued") + count("uploading");
  const waiting = count("ready") + failed;
  const started = done + failed + moving > 0;
  const missingCategory = categoryRequired
    ? rows.filter((r) => (r.status === "ready" || r.status === "failed") && r.category === "").length
    : 0;

  let summary = started ? `已上傳 ${done}／${rows.length} 個檔案` : `${rows.length} 個檔案待上傳`;
  if (moving > 0) summary += `，${moving} 個上傳中或排隊中`;
  if (failed > 0) summary += `，${failed} 個失敗`;
  if (missingCategory > 0) summary += `，${missingCategory} 個還沒選類別`;

  return (
    <div className="adlib-upload">
      <div
        className={`adlib-drop${dragging ? " is-dragging" : ""}`}
        onDragEnter={onDragOver}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
      >
        <Icon name="upload" className="adlib-drop-icon" />
        <p className="adlib-drop-title">把檔案或整個資料夾拖到這裡</p>
        <div className="adlib-drop-actions">
          <label className="secondary-button small-button adlib-pick">
            選擇檔案
            <input
              type="file"
              className="sr-only"
              multiple
              accept={accept}
              onChange={(e) => {
                take(fromFileList(e.target.files));
                e.target.value = "";
              }}
            />
          </label>
          <label className="ghost-button small-button adlib-pick">
            選擇資料夾
            <input
              type="file"
              className="sr-only"
              multiple
              ref={(el) => {
                // Not in React's attribute list; the browser then gives each file its relative path.
                el?.setAttribute("webkitdirectory", "");
              }}
              onChange={(e) => {
                take(fromFileList(e.target.files));
                e.target.value = "";
              }}
            />
          </label>
        </div>
        <p className="quiet-note">{hint}</p>
      </div>
      {note && (
        <p className="quiet-note" role="status">
          {note}
        </p>
      )}

      {rows.length > 0 && (
        <>
          <div className="adlib-queue-bar">
            <p className="adlib-summary" role="status">
              {summary}
            </p>
            {started && (
              <progress className="adlib-progress" max={rows.length} value={done} aria-label="上傳進度" />
            )}
            <div className="adlib-queue-actions">
              <button
                type="button"
                className="primary-button"
                disabled={waiting === 0}
                onClick={() => {
                  setNote(null);
                  queue.uploadAll();
                }}
              >
                全部上傳
              </button>
              {done > 0 && (
                <button type="button" className="text-link is-plain" onClick={() => queue.clear("done")}>
                  清除已上傳的
                </button>
              )}
              {rows.length > moving && (
                <button type="button" className="text-link is-plain" onClick={() => queue.clear("all")}>
                  清空清單
                </button>
              )}
            </div>
          </div>

          <div className="adlib-queue-head" aria-hidden="true">
            <span>標題</span>
            <span>類別</span>
            <span>標籤</span>
            <span>狀態</span>
          </div>
          <ul className="adlib-queue" aria-label={`待上傳的${label}`}>
            {rows.map((row) => {
              const editable = row.status === "ready" || row.status === "failed";
              return (
                <li key={row.key} className={`adlib-file is-${row.status}`}>
                  <div className="adlib-file-fields" role="group" aria-label={row.path}>
                    <div className="adlib-file-title">
                      <label className="adlib-field">
                        <span className="adlib-field-name">標題</span>
                        <input
                          type="text"
                          value={row.title}
                          disabled={!editable}
                          onChange={(e) => queue.edit(row.key, { title: e.target.value })}
                        />
                      </label>
                      <p className="adlib-path" title={row.path}>
                        {row.path}
                      </p>
                    </div>
                    <label className="adlib-field">
                      <span className="adlib-field-name">類別</span>
                      <select
                        value={row.category}
                        disabled={!editable}
                        aria-invalid={categoryRequired && row.category === "" && row.error !== null ? true : undefined}
                        onChange={(e) => queue.edit(row.key, { category: e.target.value })}
                      >
                        <option value="">{categoryRequired ? "請選類別" : AUTO_CATEGORY_LABEL}</option>
                        {categories.map((c) => (
                          <option key={c} value={c}>
                            {c}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="adlib-field">
                      <span className="adlib-field-name">標籤</span>
                      <input
                        type="text"
                        placeholder="以頓號分開"
                        value={row.tags}
                        disabled={!editable}
                        onChange={(e) => queue.edit(row.key, { tags: e.target.value })}
                      />
                    </label>
                    <div className="adlib-file-state">
                      {row.status === "done" ? (
                        renderDone(row)
                      ) : (
                        <span className={`adlib-state is-${row.status}`}>{STATUS_LABEL[row.status]}</span>
                      )}
                      {row.status === "failed" && (
                        <button
                          type="button"
                          className="text-link"
                          aria-label={`重試「${row.title || row.path}」`}
                          onClick={() => queue.retry(row.key)}
                        >
                          重試
                        </button>
                      )}
                      {editable && (
                        <button
                          type="button"
                          className="text-link is-plain"
                          aria-label={`移除「${row.title || row.path}」`}
                          onClick={() => queue.remove(row.key)}
                        >
                          移除
                        </button>
                      )}
                    </div>
                    {row.error && <p className="error-text adlib-file-error">{row.error}</p>}
                  </div>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}
