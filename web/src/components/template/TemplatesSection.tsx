import { useCallback, useEffect, useId, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { MaterialDetail, Material, WsTemplateSummary } from "../../api/types";
import {
  EXTRACTING_LINE,
  NO_TEXT_MESSAGE,
  TEMPLATES_INTRO,
  templateConfig,
  templateMeta,
  templateTitle,
  withTextOnly,
} from "../../lib/templates";
import { ErrorText, Loading } from "../common";
import { MaterialAddForm, MaterialPicker, useMaterialLibrary } from "../material/MaterialLibrary";

type Way = "upload" | "library";

const WAY_LABEL: Record<Way, string> = { upload: "上傳一份學習單", library: "從教材庫選" };

/** The teacher's templates, re-read every few seconds while one is still being read. */
export function useTemplateList() {
  const [templates, setTemplates] = useState<WsTemplateSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seq, setSeq] = useState(0);

  useEffect(() => {
    let alive = true;
    api
      .listWorksheetTemplates()
      .then((res) => {
        if (!alive) return;
        setTemplates(res.templates);
        setError(null);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [seq]);

  const extracting = templates?.some((t) => t.status === "extracting") ?? false;
  useEffect(() => {
    if (!extracting) return undefined;
    let stopped = false;
    const timer = setInterval(() => {
      api
        .listWorksheetTemplates()
        .then((res) => !stopped && setTemplates(res.templates))
        .catch(() => {
          // A missed poll is not a failure; the next one tries again.
        });
    }, templateConfig.pollMs);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [extracting]);

  const reload = useCallback(() => setSeq((n) => n + 1), []);
  /** Puts a template the server just returned at the top (or in its place). */
  const upsert = useCallback((t: WsTemplateSummary) => {
    setTemplates((list) => [t, ...(list ?? []).filter((x) => x.id !== t.id)]);
  }, []);
  return { templates, error, reload, upsert };
}

/** 上傳一份學習單 or 從教材庫選: both end with a template being read from a library material. */
function AddTemplate({
  way,
  onStarted,
  onMaterialAdded,
  onCancel,
}: {
  way: Way;
  onStarted: (t: WsTemplateSummary) => void;
  onMaterialAdded: () => void;
  onCancel: () => void;
}) {
  const { library, error, reload } = useMaterialLibrary();
  const [busy, setBusy] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);

  async function start(material: Pick<Material, "id" | "text_status">) {
    setStartError(null);
    if (material.text_status !== "ready") {
      setStartError(NO_TEXT_MESSAGE);
      return;
    }
    setBusy(true);
    try {
      onStarted(await api.createWorksheetTemplate({ material_id: material.id }));
    } catch (err) {
      setStartError(errorMessage(err));
    } finally {
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
  return (
    <div className="template-add">
      {way === "upload" ? (
        <MaterialAddForm
          way="upload"
          library={library}
          submitLabel="上傳，讓 Manna 讀設計"
          onCancel={onCancel}
          onAdded={(m: MaterialDetail) => {
            onMaterialAdded();
            void start(m);
          }}
        />
      ) : (
        <>
          <p className="quiet-note">只列出有文字的教材（掃描檔與圖片讀不到設計）。</p>
          <MaterialPicker library={withTextOnly(library)} currentId={null} busy={busy} onPick={(m) => void start(m)} />
          <p className="quiet-note">
            <button type="button" className="text-link is-plain" onClick={onCancel}>
              取消
            </button>
          </p>
        </>
      )}
      {busy && <Loading label="送出中…" />}
      <ErrorText message={startError} />
    </div>
  );
}

/** The line shown while Manna reads a worksheet's design. */
export function ExtractingLine() {
  return (
    <p className="manna-label template-extracting" role="status">
      <span className="star is-working" aria-hidden="true" />
      {EXTRACTING_LINE}
    </p>
  );
}

/**
 * 我的學習單版型 on the 教材 page: add a template from an uploaded worksheet or a library material,
 * and the templates so far with their state. Opening one leads to its own page.
 */
export function TemplatesSection({ onMaterialAdded }: { onMaterialAdded: () => void }) {
  const ids = useId();
  const { templates, error, reload, upsert } = useTemplateList();
  const [adding, setAdding] = useState<Way | null>(null);
  const [retrying, setRetrying] = useState<number | null>(null);
  const [rowError, setRowError] = useState<{ id: number; message: string } | null>(null);

  async function retry(t: WsTemplateSummary) {
    setRetrying(t.id);
    setRowError(null);
    try {
      upsert(await api.retryWorksheetTemplate(t.id));
    } catch (err) {
      setRowError({ id: t.id, message: errorMessage(err) });
    } finally {
      setRetrying(null);
    }
  }

  return (
    <section className="section templates-section" aria-labelledby={`${ids}-heading`}>
      <h2 id={`${ids}-heading`}>我的學習單版型</h2>
      <p className="lead">{TEMPLATES_INTRO}</p>

      <div className="button-row materials-add-row" role="group" aria-label="新增版型">
        {(["upload", "library"] as const).map((way) => (
          <button
            key={way}
            type="button"
            className="secondary-button"
            aria-pressed={adding === way}
            onClick={() => setAdding((cur) => (cur === way ? null : way))}
          >
            {WAY_LABEL[way]}
          </button>
        ))}
      </div>
      {adding && (
        <AddTemplate
          key={adding}
          way={adding}
          onCancel={() => setAdding(null)}
          onMaterialAdded={onMaterialAdded}
          onStarted={(t) => {
            setAdding(null);
            upsert(t);
          }}
        />
      )}

      {error && (
        <>
          <ErrorText message={error} />
          <button type="button" className="text-link is-plain" onClick={reload}>
            再試一次
          </button>
        </>
      )}
      {!templates && !error && <Loading />}
      {templates && templates.length === 0 && <p className="empty-line">還沒有版型。</p>}
      {templates && templates.length > 0 && (
        <ul className="rows" aria-label="版型">
          {templates.map((t) => (
            <li key={t.id} className="row">
              <div>
                <h3>
                  <Link to={`/templates/${t.id}`}>{templateTitle(t)}</Link>
                </h3>
                {t.status === "extracting" ? <ExtractingLine /> : <p className="meta">{templateMeta(t)}</p>}
              </div>
              {t.status === "failed" && (
                <p className="side">
                  <button
                    type="button"
                    className="text-link"
                    disabled={retrying === t.id}
                    aria-label={`再讀一次「${templateTitle(t)}」`}
                    onClick={() => void retry(t)}
                  >
                    {retrying === t.id ? "送出中…" : "再讀一次"}
                  </button>
                </p>
              )}
              {rowError?.id === t.id && (
                <div className="row-more">
                  <ErrorText message={rowError.message} />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
