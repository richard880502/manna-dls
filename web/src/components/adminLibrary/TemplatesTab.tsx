import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router";
import { adminLibraryApi, errorMessage } from "../../api/client";
import type { AdminLibraryTemplate, AdminLibraryTemplateRow } from "../../api/types";
import { formatDate } from "../../lib/labels";
import { BulkUpload } from "./BulkUpload";
import { LibraryList, useAdminList, type ListFilters } from "./LibraryList";
import {
  SAMPLE_CATEGORIES,
  TEMPLATE_EXTENSIONS,
  guessTags,
  parseTags,
  tagsProblem,
  tagsText,
  templateMeta,
  titleFromPath,
  titleProblem,
} from "./prefill";
import { useListPoll } from "./useListPoll";
import { useUploadQueue, type QueueRow } from "./useUploadQueue";

// 自動判斷 by default: the server suggests a category from what it read.
const prefill = (path: string) => ({ title: titleFromPath(path), category: "", tags: tagsText(guessTags(path)) });

const validate = (row: QueueRow<AdminLibraryTemplate>): string | null =>
  titleProblem(row.title) ?? tagsProblem(parseTags(row.tags));

const upload = (row: QueueRow<AdminLibraryTemplate>) =>
  adminLibraryApi.uploadTemplate(row.file, {
    title: row.title.trim(),
    category: row.category || undefined,
    tags: parseTags(row.tags),
  });

const isNewer = (next: AdminLibraryTemplate, cur: AdminLibraryTemplate) =>
  next.status !== cur.status || next.updated_at !== cur.updated_at || next.category !== cur.category;

function ExtractionState({
  template,
  retrying,
  onRetry,
}: {
  template: AdminLibraryTemplate;
  retrying: boolean;
  onRetry: () => void;
}) {
  if (template.status === "extracting") {
    return (
      <span className="adlib-state is-working">
        <span className="star is-working" aria-hidden="true" />
        已上傳 · 讀取中…
      </span>
    );
  }
  if (template.status === "failed") {
    return (
      <>
        <span className="adlib-state is-failed">已上傳 · 沒有讀成功</span>
        <button
          type="button"
          className="text-link"
          disabled={retrying}
          aria-label={`再讀一次「${template.title}」`}
          onClick={onRetry}
        >
          {retrying ? "送出中…" : "再讀一次"}
        </button>
      </>
    );
  }
  return <span className="adlib-state is-done">已上傳 · {templateMeta(template)}</span>;
}

/** 平台範本: bulk upload of example worksheets, their reading status, then the platform templates. */
export function TemplatesTab({ onBusy }: { onBusy?: (busy: boolean) => void }) {
  const [revision, setRevision] = useState(0);
  const [filters, setFilters] = useState<ListFilters>({ q: "", category: "" });
  const [retrying, setRetrying] = useState<number | null>(null);
  const [retryError, setRetryError] = useState<{ id: number; message: string } | null>(null);
  const list = useAdminList<AdminLibraryTemplateRow>(adminLibraryApi.listTemplates, filters, revision);
  const onDrained = useCallback((uploaded: number) => {
    if (uploaded > 0) setRevision((r) => r + 1);
  }, []);
  const queue = useUploadQueue<AdminLibraryTemplate>({
    extensions: TEMPLATE_EXTENSIONS,
    prefill,
    validate,
    upload,
    onDrained,
  });
  const { busy } = queue;
  useEffect(() => onBusy?.(busy), [onBusy, busy]);
  const { patch } = list;
  const { patchResults } = queue;

  /** Templates the server reported: the upload rows and the list take their news. */
  const apply = useCallback(
    (items: AdminLibraryTemplate[]) => {
      const byId = new Map(items.map((t) => [t.id, t]));
      patch((x) => {
        const n = byId.get(x.id);
        return n && isNewer(n, x) ? { ...x, ...n } : x;
      });
      patchResults((r) => {
        const n = byId.get(r.id);
        return n && isNewer(n, r) ? { ...r, ...n } : r;
      });
    },
    [patch, patchResults],
  );

  const extracting =
    (list.items ?? []).some((t) => t.status === "extracting") ||
    queue.rows.some((r) => r.result?.status === "extracting");
  // While any template is being read, the list is asked for again every few seconds.
  useListPoll(extracting, adminLibraryApi.listTemplates, apply);

  async function retry(t: AdminLibraryTemplate) {
    setRetrying(t.id);
    setRetryError(null);
    try {
      apply([await adminLibraryApi.retryTemplate(t.id)]);
    } catch (err) {
      setRetryError({ id: t.id, message: errorMessage(err) });
    } finally {
      setRetrying(null);
    }
  }

  return (
    <>
      <BulkUpload
        queue={queue}
        label="平台範本"
        categories={SAMPLE_CATEGORIES}
        categoryRequired={false}
        extensions={TEMPLATE_EXTENSIONS}
        hint="有文字的學習單（.docx、.pdf、.txt）。上傳後 Manna 會讀每一份的設計（約 20 秒）；類別預設自動判斷，出版社（三民、龍騰…）與冊次（B1L5）會從檔名帶進標籤。"
        renderDone={(row) =>
          row.result && (
            <>
              <ExtractionState
                template={row.result}
                retrying={retrying === row.result.id}
                onRetry={() => row.result && void retry(row.result)}
              />
              {retryError?.id === row.result.id && <span className="error-text">{retryError.message}</span>}
            </>
          )
        }
      />
      <LibraryList<AdminLibraryTemplateRow>
        label="平台範本"
        categories={SAMPLE_CATEGORIES}
        allowAuto
        list={list}
        filters={filters}
        onFilters={setFilters}
        renderMeta={(t) => (
          <p className="meta">
            {t.status === "extracting" && (
              <span className="adlib-state is-working">
                <span className="star is-working" aria-hidden="true" />
                讀取中…
              </span>
            )}
            {t.status === "failed" && <span className="adlib-state is-failed">沒有讀成功</span>}
            {[
              t.status === "ready" ? templateMeta(t) : null,
              t.uploaded_by ? `${t.uploaded_by} 上傳` : null,
              formatDate(t.updated_at),
            ]
              .filter(Boolean)
              .join(" · ")}
            {retryError?.id === t.id && <span className="error-text"> {retryError.message}</span>}
          </p>
        )}
        renderActions={(t) =>
          t.status === "failed" ? (
            <button
              type="button"
              className="text-link"
              disabled={retrying === t.id}
              aria-label={`再讀一次「${t.title}」`}
              onClick={() => void retry(t)}
            >
              {retrying === t.id ? "送出中…" : "再讀一次"}
            </button>
          ) : t.status === "ready" ? (
            <Link className="text-link" to={`/templates/${t.id}`}>
              看版型
            </Link>
          ) : null
        }
        save={adminLibraryApi.updateTemplate}
        remove={adminLibraryApi.deleteTemplate}
        deleteNote="這個平台範本會永久刪除，老師就選不到它了。老師已經照它做好的學習單不受影響。"
      />
    </>
  );
}
