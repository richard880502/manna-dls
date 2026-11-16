import { useCallback, useId, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { MaterialDetail, TemplateCard as TemplateCardData } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { TemplateBrowser } from "../components/library/BrowsePanel";
import {
  TEMPLATE_AREA_LABEL,
  UPLOAD_SAMPLE,
  USE_TEMPLATE,
  VIEW_SOURCE,
  VIEW_TEMPLATE,
  templateCardTitle,
  type TemplateArea,
} from "../components/library/labels";
import { Tabs } from "../components/library/Tabs";
import { MaterialAddForm, useMaterialLibrary } from "../components/material/MaterialLibrary";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { OcrFollow } from "../components/material/Ocr";
import { OncePerPage } from "../components/OncePerPage";
import { mimicTemplatePath } from "../lib/create";
import { ocrFailed, ocrFailedLine, ocrOpen } from "../lib/ocr";
import { NO_TEXT_MESSAGE, TEMPLATES_INTRO } from "../lib/templates";

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
        <button type="button" className="text-link is-plain" onClick={onCancel}>
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
 * 學習單範本 (/templates): 平台範本 | 我的範本 (?tab=mine). A card shows its sections; 照這個範本做
 * opens 照我的學習單做 with it. 我的範本 also takes a new worksheet (上傳我的學習單) and shows the
 * ones Manna is still reading.
 */
function Templates() {
  const ids = useId();
  const [params, setParams] = useSearchParams();
  const tab: TemplateArea = params.get("tab") === "mine" ? "mine" : "platform";
  const [viewing, setViewing] = useState<number | null>(null);
  const [uploading, setUploading] = useState(false);
  const [changed, setChanged] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const [rowError, setRowError] = useState<{ id: number; message: string } | null>(null);
  const [retrying, setRetrying] = useState<number | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);

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
                aria-pressed={uploading}
                onClick={() => {
                  setNotice(null);
                  setUploading((u) => !u);
                }}
              >
                {UPLOAD_SAMPLE}
              </button>
            </div>
            {uploading && (
              <UploadSample
                onCancel={() => setUploading(false)}
                onStarted={(title) => {
                  setUploading(false);
                  setNotice(`Manna 正在讀「${title}」的設計，讀好了就能用。`);
                  setChanged((n) => n + 1);
                }}
              />
            )}
            {notice && (
              <p className="quiet-note" role="status">
                {notice}
              </p>
            )}
          </div>
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
                {rowError?.id === card.id && <ErrorText message={rowError.message} />}
              </>
            );
          }}
        />
      </div>

      <MaterialViewer mode="panel" materialId={viewing} open={viewing !== null} onClose={closeViewer} />
    </section>
  );
}

export function TemplatesPage() {
  return (
    <OncePerPage>
      <Templates />
    </OncePerPage>
  );
}
