import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Navigate, useLocation } from "react-router";
import { api, errorMessage } from "../api/client";
import type { MaterialCard as MaterialCardData, MaterialDetail } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { MaterialBrowser } from "../components/library/BrowsePanel";
import { MoreMenu, UseMaterialMenu } from "../components/library/Cards";
import { PREVIEW, cardTitle, unusableReason, usableMaterial } from "../components/library/labels";
import { Dialog } from "../components/Dialog";
import {
  ADD_WAY_LABEL,
  MaterialAddForm,
  deleteConsequences,
  titleOf,
  useMaterialLibrary,
  type AddWay,
} from "../components/material/MaterialLibrary";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { OcrFollow } from "../components/material/Ocr";
import { OncePerPage } from "../components/OncePerPage";
import { useApp } from "../context/AppContext";
import { formatBytes } from "../lib/materials";
import { ocrOpen } from "../lib/ocr";
import { can } from "../lib/permissions";

/** 改名: the title in a small dialog. */
function RenameDialog({
  card,
  returnFocus,
  onDone,
  onClose,
}: {
  card: Pick<MaterialCardData, "id" | "title">;
  returnFocus: () => HTMLElement | null;
  onDone: (title: string) => void;
  onClose: () => void;
}) {
  const ids = useId();
  const [title, setTitle] = useState(card.title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const saved = await api.updateLibraryMaterial(card.id, { title: title.trim() });
      onDone(saved.title);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Dialog title="教材改名" returnFocus={returnFocus} onClose={onClose}>
      <form className="material-rename" onSubmit={(e) => void submit(e)} aria-label="教材改名">
        <label className="field-label" htmlFor={`${ids}-title`}>
          教材標題
        </label>
        <input
          id={`${ids}-title`}
          type="text"
          maxLength={200}
          value={title}
          disabled={busy}
          onChange={(e) => setTitle(e.target.value)}
        />
        <ErrorText message={error} />
        <div className="button-row">
          <button type="submit" className="primary-button small-button" disabled={busy}>
            儲存標題
          </button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={onClose}>
            取消
          </button>
        </div>
      </form>
    </Dialog>
  );
}

/** 刪除: said in full (what goes with it) before anything is sent. */
function DeleteDialog({
  card,
  returnFocus,
  onDone,
  onClose,
}: {
  card: Pick<MaterialCardData, "id" | "title">;
  returnFocus: () => HTMLElement | null;
  onDone: () => void;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<MaterialDetail | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .getLibraryMaterial(card.id)
      .then((m) => alive && setDetail(m))
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
      await api.deleteLibraryMaterial(card.id);
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <Dialog title={`永久刪除「${titleOf(card)}」？`} role="alertdialog" returnFocus={returnFocus} onClose={onClose}>
      <div className="material-delete">
        {detail ? (
          <p>{deleteConsequences(detail)}</p>
        ) : loadError ? (
          <p>刪除後無法復原：原檔、擷取的文字與段落和你的註記都會永久刪除。</p>
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
 * 我的教材 (/materials): the teacher's own materials as cards, with 上傳檔案, 貼上文字 and 加入網址
 * at the top. A card previews in the viewer (with the teacher's notes), opens a tool with it,
 * and behind ⋯ is renamed or deleted for good (after a confirmation that says what goes with it).
 */
function MyMaterials() {
  const ids = useId();
  const { library, error, reload } = useMaterialLibrary();
  const [adding, setAdding] = useState<AddWay | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [changed, setChanged] = useState(0);
  const [viewing, setViewing] = useState<number | null>(null);
  const [renaming, setRenaming] = useState<MaterialCardData | null>(null);
  const [deleting, setDeleting] = useState<MaterialCardData | null>(null);
  /** A scanned upload Manna is still reading (OCR): its progress shows under the notice. */
  const [reading, setReading] = useState<MaterialDetail | null>(null);
  const closeViewer = useCallback(() => setViewing(null), []);
  /** The button (⋯ or 上傳檔案…) a dialog was opened from: focus goes back there. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);

  const listChanged = (message: string) => {
    setNotice(message);
    setChanged((n) => n + 1);
    reload();
  };

  return (
    <section className="library-page" aria-labelledby={`${ids}-heading`}>
      <div className="head-row page-head">
        <div>
          <h1 id={`${ids}-heading`}>我的教材</h1>
          <p className="lead">你的課文、講義、圖片與影音。只有你看得到；一份教材可以用在好幾個課例和學習單。</p>
        </div>
        {library && (
          <p className="materials-usage">
            {library.usage.count} 份 · 檔案已使用 {formatBytes(library.usage.used_bytes)}／
            {formatBytes(library.usage.quota_bytes)}
          </p>
        )}
      </div>

      <ErrorText message={error} />
      <div className="button-row materials-add-row" role="group" aria-label="新增教材">
        {(["upload", "paste", "link"] as const).map((way) => (
          <button
            key={way}
            type="button"
            className="secondary-button"
            aria-haspopup="dialog"
            disabled={!library}
            onClick={(event) => {
              opener.current = event.currentTarget;
              setNotice(null);
              setAdding(way);
            }}
          >
            {ADD_WAY_LABEL[way]}
          </button>
        ))}
      </div>
      {adding && library && (
        <Dialog title={ADD_WAY_LABEL[adding]} returnFocus={backToOpener} onClose={() => setAdding(null)}>
          <MaterialAddForm
            key={adding}
            way={adding}
            library={library}
            onCancel={() => setAdding(null)}
            onAdded={(m) => {
              setAdding(null);
              setReading(ocrOpen(m.ocr) ? m : null);
              listChanged(`已加入「${titleOf(m)}」。`);
            }}
          />
        </Dialog>
      )}
      {notice && (
        <p className="quiet-note" role="status">
          {notice}
        </p>
      )}
      {reading && (
        <OcrFollow
          key={reading.id}
          material={reading}
          onDone={(m) => {
            setReading(null);
            listChanged(`「${titleOf(m)}」的文字辨識好了。${m.ocr?.error ? m.ocr.error : ""}`);
          }}
        />
      )}

      <MaterialBrowser
        area="mine"
        refreshSignal={changed}
        empty={<p className="empty-line">還沒有教材。上傳檔案、貼上文字或加入網址，就能在出題和學習單裡取用。</p>}
        renderActions={(card) => {
          const title = cardTitle(card);
          return (
            <>
              <button
                type="button"
                className="ghost-button small-button"
                aria-label={`${PREVIEW}「${title}」`}
                aria-expanded={viewing === card.id}
                aria-controls="material-viewer"
                onClick={() => setViewing((cur) => (cur === card.id ? null : card.id))}
              >
                {PREVIEW}
              </button>
              <UseMaterialMenu card={card} disabled={!usableMaterial(card)} title={unusableReason(card)} />
              <MoreMenu
                label={`更多動作「${title}」`}
                items={[
                  {
                    label: "改名",
                    onSelect: (from) => {
                      opener.current = from;
                      setRenaming(card);
                    },
                  },
                  {
                    label: "刪除",
                    onSelect: (from) => {
                      opener.current = from;
                      setDeleting(card);
                    },
                  },
                ]}
              />
            </>
          );
        }}
      />

      <MaterialViewer mode="panel" materialId={viewing} open={viewing !== null} onClose={closeViewer} />

      {renaming && (
        <RenameDialog
          card={renaming}
          returnFocus={backToOpener}
          onClose={() => setRenaming(null)}
          onDone={(title) => {
            setRenaming(null);
            listChanged(`已改名為「${titleOf({ title })}」。`);
          }}
        />
      )}
      {deleting && (
        <DeleteDialog
          card={deleting}
          returnFocus={backToOpener}
          onClose={() => setDeleting(null)}
          onDone={() => {
            if (viewing === deleting.id) setViewing(null);
            setDeleting(null);
            listChanged(`已永久刪除「${titleOf(deleting)}」。`);
          }}
        />
      )}
    </section>
  );
}

/** 我的教材. The old 我的版型 anchor (/materials#templates) now lives at 學習單範本's 我的範本. */
export function MaterialsPage() {
  const { teacher } = useApp();
  const { hash } = useLocation();
  if (hash === "#templates") return <Navigate to="/templates?tab=mine" replace />;
  if (!can(teacher, "use_materials")) {
    return (
      <div className="page-head">
        <h1>我的教材</h1>
        <p className="lead">你的帳號目前沒有使用教材庫的權限。</p>
      </div>
    );
  }
  return (
    <OncePerPage>
      <MyMaterials />
    </OncePerPage>
  );
}
