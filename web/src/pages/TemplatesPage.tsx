import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { adminLibraryApi, api, errorMessage } from "../api/client";
import type {
  Material,
  MaterialDetail,
  TemplateCard as TemplateCardData,
  WsTemplateDetail,
} from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { Dialog, useDialogGuard } from "../components/Dialog";
import { TemplateBrowser } from "../components/library/BrowsePanel";
import { MoreMenu } from "../components/library/Cards";
import {
  TEMPLATE_AREA_LABEL,
  UPLOAD_SAMPLE,
  USE_TEMPLATE,
  VIEW_SOURCE,
  VIEW_TEMPLATE,
  templateCardTitle,
  type TemplateArea,
} from "../components/library/labels";
import { PLATFORM_TEMPLATE_DELETE_NOTE, PlatformDeleteDialog } from "../components/library/PlatformDeleteDialog";
import { Tabs } from "../components/library/Tabs";
import { DeleteMaterialDialog } from "../components/material/DeleteMaterialDialog";
import { MaterialAddForm, titleOf, useMaterialLibrary } from "../components/material/MaterialLibrary";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { OcrFollow } from "../components/material/Ocr";
import { OncePerPage } from "../components/OncePerPage";
import { useApp } from "../context/AppContext";
import { mimicTemplatePath } from "../lib/create";
import { can } from "../lib/permissions";
import { formatDate } from "../lib/labels";
import { ocrFailed, ocrFailedLine, ocrOpen } from "../lib/ocr";
import { NO_TEXT_MESSAGE, TEMPLATES_INTRO, templateDeleteConsequences } from "../lib/templates";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { MetaList } from "../components/MetaList";

const TABS: ReadonlyArray<readonly [TemplateArea, string]> = [
  ["platform", TEMPLATE_AREA_LABEL.platform],
  ["mine", TEMPLATE_AREA_LABEL.mine],
];

/**
 * 上傳我的學習單: the file goes in as a worksheet sample (never into 我的教材), then Manna starts
 * reading its design into a template (a scanned worksheet's text is read by OCR first).
 */
function UploadSample({ onStarted, onCancel }: { onStarted: (title: string) => void; onCancel: () => void }) {
  const { library, error, reload } = useMaterialLibrary();
  const [busy, setBusy] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const [reading, setReading] = useState<MaterialDetail | null>(null);
  // Starting the template is part of the upload: the dialog stays until it is sent.
  const requestClose = useDialogGuard({ busy });
  const cancel = requestClose ?? onCancel;

  async function start(m: MaterialDetail) {
    setStartError(null);
    if (ocrOpen(m.ocr)) {
      setReading(m);
      return;
    }
    if (m.text_status !== "ready") {
      setStartError(ocrFailed(m.ocr) && m.ocr ? ocrFailedLine(m.ocr) : NO_TEXT_MESSAGE);
      return;
    }
    setBusy(true);
    try {
      const t = await api.createWorksheetTemplate({ material_id: m.id });
      onStarted(t.title.trim() || m.title);
    } catch (err) {
      setStartError(errorMessage(err));
      setBusy(false);
    }
  }

  if (error) {
    return (
      <div>
        <ErrorText message={error} />
        <button type="button" className="text-link is-plain" onClick={reload}>
          再試一次
        </button>
      </div>
    );
  }
  if (!library) return <Loading />;
  if (reading) {
    return (
      <div className="template-add">
        <OcrFollow
          material={reading}
          onDone={(m) => {
            setReading(null);
            void start(m);
          }}
        />
        <ErrorText message={startError} />
        <button type="button" className="text-link is-plain" onClick={cancel}>
          取消
        </button>
      </div>
    );
  }
  return (
    <div className="template-add">
      <MaterialAddForm
        way="upload"
        library={library}
        purpose="worksheet_sample"
        submitLabel="上傳，讓 Manna 讀設計"
        onCancel={onCancel}
        onAdded={(m) => void start(m)}
      />
      {busy && <Loading label="送出中…" />}
      <ErrorText message={startError} />
    </div>
  );
}

/**
 * 刪除 one of my templates (its card's ⋯), as on its page: what goes with it is said first, the
 * uploaded file too when no other template uses it.
 */
function DeleteTemplateDialog({
  card,
  returnFocus,
  onDone,
  onClose,
}: {
  card: Pick<TemplateCardData, "id" | "title">;
  returnFocus: () => HTMLElement | null;
  onDone: () => void;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<WsTemplateDetail | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .getWorksheetTemplate(card.id)
      .then((t) => alive && setDetail(t))
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
      await api.deleteWorksheetTemplate(card.id);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Dialog
      title={`永久刪除「${templateCardTitle(card)}」？`}
      role="alertdialog"
      returnFocus={returnFocus}
      onClose={onClose}
    >
      <div className="material-delete">
        {detail ? (
          <p>{templateDeleteConsequences(detail)}</p>
        ) : loadError ? (
          <p>
            刪除後無法復原：這個版型的所有版本都會刪除；原本上傳的檔案沒有別的範本在用時，也會一起永久刪除。用它做過的學習單會留著，只是不再連到版型。
          </p>
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

/**
 * 沒有範本在用的原檔 (我的範本): worksheets the teacher uploaded that no template uses any more, left
 * behind by a template deleted before its file went with it, or by an upload that never became a
 * template. Each can be looked at and deleted for good; the section is not shown when there are
 * none (nor when they could not be read: it is only a tidy-up).
 */
function OrphanSamples({
  refreshSignal,
  viewing,
  onView,
  onDeleted,
}: {
  refreshSignal: number;
  viewing: number | null;
  onView: (id: number) => void;
  onDeleted: (sample: Material) => void;
}) {
  const ids = useId();
  const [samples, setSamples] = useState<Material[] | null>(null);
  const [deleting, setDeleting] = useState<Material | null>(null);
  const [reloads, setReloads] = useState(0);
  /** The 刪除 button the dialog was opened from: focus goes back there. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);

  useEffect(() => {
    let alive = true;
    api
      .listOrphanedSamples()
      .then((list) => alive && setSamples(list))
      .catch(() => alive && setSamples(null));
    return () => {
      alive = false;
    };
  }, [refreshSignal, reloads]);

  if (!samples || samples.length === 0) return null;
  return (
    <section className="orphan-samples" aria-labelledby={`${ids}-heading`}>
      <h2 id={`${ids}-heading`}>沒有範本在用的原檔</h2>
      <p className="quiet-note">
        你上傳過、但現在沒有範本在用的學習單檔案（範本已經刪除，或沒有做成範本）。只有你看得到；不需要的話可以刪除。
      </p>
      <ul className="orphan-list">
        {samples.map((m) => {
          const title = titleOf(m);
          const added = `${m.origin === "upload" ? "上傳" : "加入"}於 ${formatDate(m.created_at)}`;
          const meta = m.filename && m.filename !== m.title.trim() ? [m.filename, added] : [added];
          return (
            <li key={m.id} className="orphan-row" aria-label={title}>
              <div className="orphan-name">
                <strong>{title}</strong>
                <MetaList className="meta" items={meta} />
              </div>
              {(m.has_file || m.text_status === "ready") && (
                <button
                  type="button"
                  className="ghost-button small-button"
                  aria-label={`查看「${title}」`}
                  aria-expanded={viewing === m.id}
                  aria-controls="material-viewer"
                  onClick={() => onView(m.id)}
                >
                  查看
                </button>
              )}
              <button
                type="button"
                className="text-link is-plain"
                aria-label={`刪除「${title}」`}
                aria-haspopup="dialog"
                onClick={(event) => {
                  opener.current = event.currentTarget;
                  setDeleting(m);
                }}
              >
                刪除
              </button>
            </li>
          );
        })}
      </ul>
      {deleting && (
        <DeleteMaterialDialog
          card={deleting}
          returnFocus={backToOpener}
          onClose={() => setDeleting(null)}
          onDone={() => {
            setDeleting(null);
            onDeleted(deleting);
            setReloads((n) => n + 1);
          }}
        />
      )}
    </section>
  );
}

/**
 * 學習單範本 (/templates): 平台範本 | 我的範本 (?tab=mine). A card shows its sections; 照這個範本做
 * opens 照我的學習單做 with it. 我的範本 also takes a new worksheet (上傳我的學習單), shows the
 * ones Manna is still reading, deletes one of mine behind its ⋯ (更多動作 → 刪除), and lists the
 * uploaded worksheets no template uses any more. A `manage_knowledge` account also deletes a
 * platform template from its card's ⋯ on 平台範本, as in 教材與範本管理.
 */
function Templates() {
  const ids = useId();
  const { teacher } = useApp();
  const manages = can(teacher, "manage_knowledge");
  const [params, setParams] = useSearchParams();
  const tab: TemplateArea = params.get("tab") === "mine" ? "mine" : "platform";
  const [viewing, setViewing] = useState<number | null>(null);
  const [uploading, setUploading] = useState(false);
  const [changed, setChanged] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [rowError, setRowError] = useState<{ id: number; message: string } | null>(null);
  const [retrying, setRetrying] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<TemplateCardData | null>(null);
  const [deletingPlatform, setDeletingPlatform] = useState<TemplateCardData | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);
  /** 上傳我的學習單 or a card's ⋯: focus goes back there when its dialog closes. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);

  /** Something in 我的範本 changed: say what, and read the cards and the left-over files again. */
  const listChanged = (message: string) => {
    setNotice(message);
    setChanged((n) => n + 1);
  };

  const choose = (next: TemplateArea) => {
    setUploading(false);
    setNotice(null);
    setParams(next === "mine" ? { tab: "mine" } : {}, { replace: true });
  };

  async function retry(card: TemplateCardData, update: (next: TemplateCardData) => void) {
    setRetrying(card.id);
    setRowError(null);
    try {
      const t = await api.retryWorksheetTemplate(card.id);
      update({ ...card, status: t.status });
    } catch (err) {
      setRowError({ id: card.id, message: errorMessage(err) });
    } finally {
      setRetrying(null);
    }
  }

  return (
    <section className="library-page" aria-labelledby={`${ids}-heading`}>
      <div className="page-head">
        <h1 id={`${ids}-heading`}>學習單範本</h1>
        <p className="lead">別的老師的學習單做法與你自己的。選一份，Manna 照它的區塊和題型為新課文出題。</p>
      </div>

      <Tabs label="範本從哪裡來" tabs={TABS} current={tab} panelId={`${ids}-panel`} onChange={choose} />
      <div id={`${ids}-panel`} role="tabpanel" className="library-tabpanel">
        {tab === "mine" && (
          <div className="library-add">
            <p className="quiet-note">{TEMPLATES_INTRO}</p>
            <div className="button-row materials-add-row">
              <button
                type="button"
                className="secondary-button"
                aria-haspopup="dialog"
                onClick={(event) => {
                  opener.current = event.currentTarget;
                  setNotice(null);
                  setUploading(true);
                }}
              >
                {UPLOAD_SAMPLE}
              </button>
            </div>
            {uploading && (
              <Dialog title={UPLOAD_SAMPLE} returnFocus={backToOpener} onClose={() => setUploading(false)}>
                <UploadSample
                  onCancel={() => setUploading(false)}
                  onStarted={(title) => {
                    setUploading(false);
                    listChanged(`Manna 正在讀「${title}」的設計，讀好了就能用。`);
                  }}
                />
              </Dialog>
            )}
            {notice && (
              <p className="quiet-note" role="status">
                {notice}
              </p>
            )}
          </div>
        )}

        {tab === "platform" && notice && (
          <p className="quiet-note" role="status">
            {notice}
          </p>
        )}

        <TemplateBrowser
          key={tab}
          area={tab}
          refreshSignal={changed}
          empty={
            <p className="empty-line">
              {tab === "platform" ? "還沒有平台範本。" : "還沒有我的範本。上傳一份你做過的學習單，Manna 會學它的設計。"}
            </p>
          }
          renderActions={(card, update) => {
            const title = templateCardTitle(card);
            const source = card.source_material;
            return (
              <>
                <Link className="text-link lib-action-link" to={`/templates/${card.id}`} aria-label={`${VIEW_TEMPLATE}「${title}」`}>
                  {VIEW_TEMPLATE}
                </Link>
                {source && (
                  <button
                    type="button"
                    className="ghost-button small-button"
                    aria-label={`${VIEW_SOURCE}「${title}」`}
                    aria-expanded={viewing === source.id}
                    aria-controls="material-viewer"
                    onClick={() => setViewing((cur) => (cur === source.id ? null : source.id))}
                  >
                    {VIEW_SOURCE}
                  </button>
                )}
                {card.status === "failed" && card.scope === "personal" ? (
                  <button
                    type="button"
                    className="secondary-button small-button"
                    disabled={retrying === card.id}
                    aria-label={`再讀一次「${title}」`}
                    onClick={() => void retry(card, update)}
                  >
                    {retrying === card.id ? "送出中…" : "再讀一次"}
                  </button>
                ) : card.status === "ready" ? (
                  <Link
                    className="primary-button small-button"
                    to={mimicTemplatePath(card.id)}
                    aria-label={`${USE_TEMPLATE}「${title}」`}
                  >
                    {USE_TEMPLATE}
                  </Link>
                ) : (
                  <button type="button" className="primary-button small-button" disabled aria-label={`${USE_TEMPLATE}「${title}」`}>
                    {USE_TEMPLATE}
                  </button>
                )}
                {/* Platform templates: only for those who manage them (as in 教材與範本管理). */}
                {tab === "platform" && card.scope === "platform" && manages && (
                  <MoreMenu
                    label={`更多動作「${title}」`}
                    items={[
                      {
                        label: "刪除",
                        onSelect: (from) => {
                          opener.current = from;
                          setDeletingPlatform(card);
                        },
                      },
                    ]}
                  />
                )}
                {tab === "mine" && card.scope === "personal" && (
                  <MoreMenu
                    label={`更多動作「${title}」`}
                    items={[
                      {
                        label: "刪除",
                        onSelect: (from) => {
                          opener.current = from;
                          setDeleting(card);
                        },
                      },
                    ]}
                  />
                )}
                {rowError?.id === card.id && <ErrorText message={rowError.message} />}
              </>
            );
          }}
        />

        {tab === "mine" && (
          <OrphanSamples
            refreshSignal={changed}
            viewing={viewing}
            onView={(id) => setViewing((cur) => (cur === id ? null : id))}
            onDeleted={(sample) => {
              if (viewing === sample.id) setViewing(null);
              setNotice(`已永久刪除「${titleOf(sample)}」。`);
            }}
          />
        )}
      </div>

      <MaterialViewer mode="panel" materialId={viewing} open={viewing !== null} onClose={closeViewer} />

      {deletingPlatform && (
        <PlatformDeleteDialog
          title={templateCardTitle(deletingPlatform)}
          note={PLATFORM_TEMPLATE_DELETE_NOTE}
          remove={() => adminLibraryApi.deleteTemplate(deletingPlatform.id)}
          returnFocus={backToOpener}
          onClose={() => setDeletingPlatform(null)}
          onDone={() => {
            if (deletingPlatform.source_material && viewing === deletingPlatform.source_material.id) setViewing(null);
            setDeletingPlatform(null);
            listChanged(`已永久刪除「${templateCardTitle(deletingPlatform)}」。`);
          }}
        />
      )}
      {deleting && (
        <DeleteTemplateDialog
          card={deleting}
          returnFocus={backToOpener}
          onClose={() => setDeleting(null)}
          onDone={() => {
            if (deleting.source_material && viewing === deleting.source_material.id) setViewing(null);
            setDeleting(null);
            listChanged(`已永久刪除「${templateCardTitle(deleting)}」。`);
          }}
        />
      )}
    </section>
  );
}

export function TemplatesPage() {
  useDocumentTitle("學習單範本");
  return (
    <OncePerPage>
      <Templates />
    </OncePerPage>
  );
}
