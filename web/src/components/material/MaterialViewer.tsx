import { useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { api, errorMessage } from "../../api/client";
import type { MaterialAnnotation, MaterialDetail, SopMaterial } from "../../api/types";
import {
  KIND_LABEL,
  clipQuote,
  segmentsFor,
  selectionInParagraph,
  spansIn,
  toViewMaterial,
  type TextSelection,
  type ViewMaterial,
} from "../../lib/materials";
import { useSignedInTeacher } from "../../context/AppContext";
import { OCR_LABEL, OCR_RERUN, OCR_RERUN_AGAIN, canRunOcr, ocrFailed, ocrOpen } from "../../lib/ocr";
import { can } from "../../lib/permissions";
import { ErrorText, Loading } from "../common";
import { OcrFailed, OcrProgress, useOcrPoll, useOcrRerun } from "./Ocr";

const NOTE_MAX = 2000;

// ---------------------------------------------------------------- the original file

/**
 * A short-lived link to the original file for <img>, <audio>, <video> and downloads: those
 * cannot send the bearer header, so the API hands out a link to itself that expires.
 */
function useFileLink(materialId: number, wanted: boolean) {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seq, setSeq] = useState(0);
  useEffect(() => {
    if (!wanted) return;
    let alive = true;
    api
      .materialFileLink(materialId)
      .then((link) => {
        if (!alive) return;
        setUrl(link.url);
        setError(null);
      })
      .catch((err: unknown) => {
        if (alive) setError(errorMessage(err));
      });
    return () => {
      alive = false;
    };
  }, [materialId, wanted, seq]);
  /** Asks for a fresh link (the old one expired while the page stayed open). */
  const renew = useCallback(() => setSeq((n) => n + 1), []);
  return { url, error, renew };
}

function MediaPreview({ material }: { material: ViewMaterial }) {
  const { url, error, renew } = useFileLink(material.id, material.has_file);
  const renewed = useRef(false);
  const onMediaError = () => {
    // Once: an expired link is replaced; a file that will not play is left with its message.
    if (renewed.current) return;
    renewed.current = true;
    renew();
  };
  if (!material.has_file) return null;
  if (error) return <ErrorText message={`原檔沒有載入成功：${error}`} />;
  if (!url) return <Loading label="載入原檔…" />;
  const name = material.filename ?? material.title;
  return (
    <div className="material-media">
      {material.kind === "image" && <img src={url} alt={material.title || name} onError={onMediaError} />}
      {material.kind === "audio" && (
        // A teacher's own recording has no caption file; the viewer cannot invent one.
        // eslint-disable-next-line jsx-a11y/media-has-caption
        <audio controls preload="metadata" src={url} onError={onMediaError} aria-label={material.title || name} />
      )}
      {material.kind === "video" && (
        // eslint-disable-next-line jsx-a11y/media-has-caption
        <video controls preload="metadata" src={url} onError={onMediaError} aria-label={material.title || name} />
      )}
      <p className="material-file">
        <a href={`${url}&download=1`} download={name}>
          下載原檔
        </a>
        <span>{name}</span>
      </p>
    </div>
  );
}

// ---------------------------------------------------------------- one note

interface NoteProps {
  annotation: MaterialAnnotation;
  busy: boolean;
  onChange: (patch: { note?: string; share_with_ai?: boolean; hidden?: boolean }) => void;
  onDelete: () => void;
  /** Lost notes: start choosing a new place for it. */
  onPlace?: () => void;
  placing?: boolean;
  focused?: boolean;
}

/**
 * A teacher note, in ink blue under the paragraph it belongs to. It is never part of the
 * material: editing or deleting it leaves the text alone.
 */
function AnnotationNote({ annotation, busy, onChange, onDelete, onPlace, placing, focused }: NoteProps) {
  const ids = useId();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(annotation.note);
  const [confirming, setConfirming] = useState(false);
  const lost = annotation.anchor.status === "lost";
  const ref = useRef<HTMLDivElement>(null);
  const editRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView?.({ block: "nearest" });
  }, [focused]);
  useEffect(() => {
    if (editing) editRef.current?.focus();
  }, [editing]);

  return (
    <div
      ref={ref}
      className={`anno-note${lost ? " is-lost" : ""}${annotation.hidden ? " is-hidden" : ""}${focused ? " is-focused" : ""}`}
      role="group"
      aria-label={`我的註記：${clipQuote(annotation.quote, 30)}`}
    >
      <p className="anno-head">
        <b>我的註記</b>
        {lost && <span className="anno-flag">需要重新確認位置</span>}
        {annotation.hidden && <span className="anno-flag is-quiet">已隱藏</span>}
        <span className="anno-quote" lang="en">
          「{clipQuote(annotation.quote)}」
        </span>
      </p>
      {lost && (
        <p className="anno-why">
          教材改過之後，找不到這段原本標的文字。註記還在；請重新選它要標的位置，或刪除它。
        </p>
      )}
      {editing ? (
        <form
          className="anno-form"
          onSubmit={(e) => {
            e.preventDefault();
            onChange({ note: text.trim() });
            setEditing(false);
          }}
        >
          <label className="sr-only" htmlFor={`${ids}-note`}>
            註記內容
          </label>
          <textarea
            id={`${ids}-note`}
            ref={editRef}
            rows={3}
            maxLength={NOTE_MAX}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
          <div className="anno-actions">
            <button type="submit" className="primary-button small-button" disabled={busy}>
              儲存註記
            </button>
            <button
              type="button"
              className="text-link is-plain"
              onClick={() => {
                setText(annotation.note);
                setEditing(false);
              }}
            >
              取消
            </button>
          </div>
        </form>
      ) : (
        <p className="anno-text">{annotation.note || "（只標記，沒有寫說明）"}</p>
      )}
      {!editing && (
        <div className="anno-actions">
          <label className="anno-share">
            <input
              type="checkbox"
              checked={annotation.share_with_ai}
              disabled={busy}
              onChange={(e) => onChange({ share_with_ai: e.target.checked })}
            />
            提供給 AI
          </label>
          {onPlace && (
            <button type="button" className="text-link is-plain" aria-pressed={placing} onClick={onPlace}>
              {placing ? "取消重新選位置" : "重新選位置"}
            </button>
          )}
          <button type="button" className="text-link is-plain" onClick={() => setEditing(true)}>
            修改
          </button>
          <button
            type="button"
            className="text-link is-plain"
            disabled={busy}
            onClick={() => onChange({ hidden: !annotation.hidden })}
          >
            {annotation.hidden ? "取消隱藏" : "隱藏"}
          </button>
          {confirming ? (
            <span className="confirm-row" role="group" aria-label="確認刪除註記">
              刪除這則註記？教材不會變動。
              <button type="button" className="ghost-button small-button" disabled={busy} onClick={onDelete}>
                確定刪除
              </button>
              <button type="button" className="ghost-button small-button" onClick={() => setConfirming(false)}>
                取消
              </button>
            </span>
          ) : (
            <button type="button" className="text-link is-plain" onClick={() => setConfirming(true)}>
              刪除
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- the material with its notes

export interface MaterialContentProps {
  /** The material to show. Pass `material` when the page already has it; otherwise it is loaded by id. */
  materialId: number;
  material?: MaterialDetail | SopMaterial | null;
  /** Without annotations the text is still shown and selectable (e.g. a read-only preview). */
  annotate?: boolean;
}

/**
 * A material as a printed sheet with the teacher's notes: paragraph numbers in the margin,
 * annotated spans underlined in ink blue, notes under their paragraph. Selecting text offers
 * 加註記. Images, audio and video are previewed with the browser's own players; they have no
 * text to annotate (規格 §4.4(10)).
 */
export function MaterialContent({ materialId, material: given, annotate = true }: MaterialContentProps) {
  const ids = useId();
  const [loaded, setLoaded] = useState<MaterialDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [annotations, setAnnotations] = useState<MaterialAnnotation[] | null>(null);
  const [annotationsError, setAnnotationsError] = useState(false);
  const [reload, setReload] = useState(0);
  const [selection, setSelection] = useState<TextSelection | "multi" | null>(null);
  const [draft, setDraft] = useState<{ position: TextSelection; note: string; share: boolean } | null>(null);
  const [placing, setPlacing] = useState<number | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sheetRef = useRef<HTMLDivElement>(null);
  const draftRef = useRef<HTMLTextAreaElement>(null);
  const drafting = draft !== null;
  useEffect(() => {
    if (drafting) draftRef.current?.focus();
  }, [drafting]);

  const base = given ?? (loaded && loaded.id === materialId ? loaded : null);
  // A scanned file being read: the material as the last poll (or 用 OCR 重新讀取) reported it,
  // for as long as the page still shows the material it was read over.
  const [fresh, setFresh] = useState<{ over: typeof base; material: MaterialDetail } | null>(null);
  const source = fresh && fresh.over === base && fresh.material.id === materialId ? fresh.material : base;
  const material = useMemo(() => (source ? toViewMaterial(source) : null), [source]);
  const revision = material?.revision ?? 0;
  const canAnnotate = annotate && material?.text_status === "ready";

  const baseRef = useRef(base);
  useEffect(() => {
    baseRef.current = base;
  });
  const takeFresh = useCallback((m: MaterialDetail) => setFresh({ over: baseRef.current, material: m }), []);
  // A lesson SOP's copy of a material says nothing of OCR; a library material does.
  const ocr = source && "ocr" in source ? source.ocr : undefined;
  const textSource = source && "text_source" in source ? source.text_source : undefined;
  const rerun = useOcrRerun(materialId, takeFresh);
  const { clear: clearRerun } = rerun;
  const onPoll = useCallback(
    (m: MaterialDetail) => {
      takeFresh(m);
      // A 409 said a run was going; once it has ended the words are out of date.
      if (!ocrOpen(m.ocr)) clearRerun();
    },
    [takeFresh, clearRerun],
  );
  useOcrPoll(source, onPoll);
  const teacher = useSignedInTeacher();
  // Her own material, or a platform one for pi and kb_manager.
  const writable = source && "scope" in source && source.scope === "platform" ? can(teacher, "manage_knowledge") : true;
  const offerOcr = ocr !== undefined && source !== null && "revision" in source && canRunOcr(source) && writable;

  useEffect(() => {
    if (given) return;
    let alive = true;
    api
      .getLibraryMaterial(materialId)
      .then((m) => {
        if (!alive) return;
        setLoaded(m);
        setLoadError(null);
      })
      .catch((err: unknown) => {
        if (alive) setLoadError(errorMessage(err));
      });
    return () => {
      alive = false;
    };
  }, [materialId, given, reload]);

  // Notes are placed against the text as it is now: load them again when it changes.
  useEffect(() => {
    if (!annotate) return;
    let alive = true;
    api
      .listAnnotations(materialId)
      .then((list) => {
        if (!alive) return;
        setAnnotations(list);
        setAnnotationsError(false);
      })
      .catch(() => {
        if (alive) setAnnotationsError(true);
      });
    return () => {
      alive = false;
    };
  }, [materialId, revision, annotate, reload]);

  const replace = (next: MaterialAnnotation) =>
    setAnnotations((prev) => (prev ?? []).map((a) => (a.id === next.id ? next : a)));

  const run = useCallback(async (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const placingRef = useRef(placing);
  useEffect(() => {
    placingRef.current = placing;
  }, [placing]);

  // Follow the browser selection: one paragraph's text can be annotated at a time.
  useEffect(() => {
    if (!canAnnotate) return;
    const onSelection = () => {
      const current = selectionInParagraph(sheetRef.current);
      const target = placingRef.current;
      if (target !== null && current && current !== "multi") {
        // The teacher is showing where a lost note belongs.
        setPlacing(null);
        window.getSelection()?.removeAllRanges();
        void run(async () => {
          const updated = await api.updateAnnotation(target, {
            anchor: { paragraph_id: current.paragraph_id, start: current.start, end: current.end },
          });
          setAnnotations((prev) => (prev ?? []).map((a) => (a.id === updated.id ? updated : a)));
        });
        return;
      }
      setSelection(current);
    };
    document.addEventListener("selectionchange", onSelection);
    return () => document.removeEventListener("selectionchange", onSelection);
  }, [canAnnotate, run]);

  if (loadError) {
    return (
      <div>
        <ErrorText message={loadError} />
        <button type="button" className="text-link is-plain" onClick={() => setReload((n) => n + 1)}>
          再試一次
        </button>
      </div>
    );
  }
  if (!material) return <Loading label="載入教材…" />;

  const list = annotations ?? [];
  const lost = list.filter((a) => a.anchor.status === "lost");
  const hiddenCount = list.filter((a) => a.hidden && a.anchor.status !== "lost").length;
  const noteProps = (a: MaterialAnnotation) => ({
    annotation: a,
    busy,
    onChange: (patch: { note?: string; share_with_ai?: boolean; hidden?: boolean }) =>
      void run(async () => replace(await api.updateAnnotation(a.id, patch))),
    onDelete: () =>
      void run(async () => {
        await api.deleteAnnotation(a.id);
        setAnnotations((prev) => (prev ?? []).filter((x) => x.id !== a.id));
      }),
  });

  const saveDraft = () => {
    if (!draft) return;
    void run(async () => {
      const created = await api.createAnnotation(material.id, {
        paragraph_id: draft.position.paragraph_id,
        start: draft.position.start,
        end: draft.position.end,
        note: draft.note.trim(),
        share_with_ai: draft.share,
      });
      setAnnotations((prev) => [...(prev ?? []), created]);
      setDraft(null);
      setSelection(null);
      window.getSelection()?.removeAllRanges();
    });
  };

  const picked = selection && selection !== "multi" ? selection : null;
  const reading = ocrOpen(ocr);
  /** 用 OCR 重新讀取 (重新辨識 once the text came from OCR); a failed run offers 再試一次 instead. */
  const showRerun = offerOcr && !reading && !ocrFailed(ocr);
  const showLabel = textSource === "ocr" && material.paragraphs.length > 0;
  /** Paragraphs that start a new page of the source PDF get a 「第 n 頁」 line above them. */
  const pageStarts = new Set(
    material.paragraphs
      .filter((p, i) => p.page !== undefined && p.page !== material.paragraphs[i - 1]?.page)
      .map((p) => p.id),
  );

  return (
    <div className="material-content">
      {lost.length > 0 && (
        <section className="anno-lost" aria-label="需要重新確認位置的註記">
          {placing !== null && (
            <p className="anno-placing" role="status">
              請在下面的教材裡選取這則註記要標的文字。
            </p>
          )}
          {lost.map((a) => (
            <AnnotationNote
              key={a.id}
              {...noteProps(a)}
              placing={placing === a.id}
              onPlace={() => setPlacing((cur) => (cur === a.id ? null : a.id))}
            />
          ))}
        </section>
      )}

      {material.kind === "link" && material.url && (
        <p className="material-link">
          <a href={material.url} target="_blank" rel="noopener noreferrer">
            {material.title || material.url}
          </a>
          <span className="field-hint">外部網址，會在新分頁開啟。Manna 只記下連結，沒有擷取網頁內容。</span>
        </p>
      )}
      {material.paragraphs.length === 0 && material.kind !== "link" && material.title && (
        <p className="material-media-title">{material.title}</p>
      )}
      <MediaPreview material={material} />

      {reading && ocr && <OcrProgress ocr={ocr} />}
      {ocr && ocrFailed(ocr) && <OcrFailed ocr={ocr} retry={offerOcr ? rerun : undefined} />}
      {ocr?.status === "done" && ocr.error && <p className="quiet-note ocr-note">{ocr.error}</p>}

      {material.text_status === "no_text" && material.kind === "document" && !reading && (
        <p className="material-state">
          這份文件沒有可以選取的文字（可能是掃描檔），所以還不能在上面加註記，也不能請 Manna 分析。
          {showRerun
            ? `可以按「${OCR_RERUN}」讓 Manna 辨識文字，或下載原檔對照、另外貼上文字。`
            : "你可以下載原檔對照，或另外貼上文字。"}
        </p>
      )}
      {material.text_status === "failed" && (
        <p className="material-state">這份文件的文字讀不出來（檔案可能加密或損壞）。原檔仍然保存著，可以下載。</p>
      )}
      {material.text_truncated && (
        <p className="material-state">這份文件很長，只擷取了前面的文字；原檔完整保存。</p>
      )}

      {(showLabel || showRerun) && (
        <div className="ocr-tools">
          {showLabel && <span className="ocr-label">{OCR_LABEL}</span>}
          {showRerun && (
            <button type="button" className="text-link is-plain" disabled={rerun.busy} onClick={() => void rerun.run()}>
              {rerun.busy ? "送出中…" : textSource === "ocr" ? OCR_RERUN_AGAIN : OCR_RERUN}
            </button>
          )}
        </div>
      )}
      {!(ocr && ocrFailed(ocr)) && <ErrorText message={rerun.error} />}

      {material.paragraphs.length > 0 && (
        <div className="material-sheet-wrap">
          <div className="material-sheet" lang="en" ref={sheetRef}>
            {material.title && <p className="material-title">{material.title}</p>}
            {material.paragraphs.map((p) => {
              const segments = segmentsFor(p.text, spansIn(list, p.id, showHidden));
              const here = list.filter(
                (a) => a.anchor.status !== "lost" && a.anchor.paragraph_id === p.id && (showHidden || !a.hidden),
              );
              const newPage = pageStarts.has(p.id);
              return (
                <div key={p.id} className="material-para">
                  {newPage && (
                    <p className="material-page" lang="zh-Hant">
                      第 {p.page} 頁
                    </p>
                  )}
                  <p data-n={p.no}>
                    <span className="sr-only">第 {p.no} 段：</span>
                    <span data-pid={p.id}>
                      {segments.map((seg) =>
                        seg.ids.length > 0 ? (
                          <mark key={seg.start} className="anno">
                            {seg.text}
                          </mark>
                        ) : (
                          <span key={seg.start}>{seg.text}</span>
                        ),
                      )}
                    </span>
                  </p>
                  {draft && draft.position.paragraph_id === p.id && (
                    <form
                      className="anno-note is-draft"
                      aria-label="新增註記"
                      lang="zh-Hant"
                      onSubmit={(e) => {
                        e.preventDefault();
                        saveDraft();
                      }}
                    >
                      <p className="anno-head">
                        <b>新增註記</b>
                        <span className="anno-quote" lang="en">
                          「{clipQuote(draft.position.text)}」
                        </span>
                      </p>
                      <label className="sr-only" htmlFor={`${ids}-draft`}>
                        註記內容
                      </label>
                      <textarea
                        id={`${ids}-draft`}
                        ref={draftRef}
                        rows={3}
                        maxLength={NOTE_MAX}
                        value={draft.note}
                        placeholder="例如：學生常把 refuse 和 reject 搞混，想多一題辨析。"
                        onChange={(e) => setDraft({ ...draft, note: e.target.value })}
                      />
                      <div className="anno-actions">
                        <label className="anno-share">
                          <input
                            type="checkbox"
                            checked={draft.share}
                            onChange={(e) => setDraft({ ...draft, share: e.target.checked })}
                          />
                          提供給 AI
                        </label>
                        <button type="submit" className="primary-button small-button" disabled={busy}>
                          儲存註記
                        </button>
                        <button type="button" className="text-link is-plain" onClick={() => setDraft(null)}>
                          取消
                        </button>
                      </div>
                    </form>
                  )}
                  {here.map((a) => (
                    <div key={a.id} lang="zh-Hant">
                      <AnnotationNote {...noteProps(a)} />
                    </div>
                  ))}
                </div>
              );
            })}
            {material.source_note && (
              <p className="material-source" lang="zh-Hant">
                來源：{material.source_note}
              </p>
            )}
          </div>

          {canAnnotate && !draft && placing === null && selection && (
            <div className="anno-bar" role="status">
              {picked ? (
                <>
                  <span lang="en">已選取「{clipQuote(picked.text, 40)}」</span>
                  <button
                    type="button"
                    className="primary-button small-button"
                    // Keep the selection while the button takes the click.
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => setDraft({ position: picked, note: "", share: false })}
                  >
                    加註記
                  </button>
                </>
              ) : (
                <span>一則註記標在同一段裡的文字。請只選一段裡的文字。</span>
              )}
            </div>
          )}
        </div>
      )}

      <ErrorText message={error} />
      {annotationsError && (
        <p className="quiet-note">
          註記沒有載入成功。
          <button type="button" className="text-link is-plain" onClick={() => setReload((n) => n + 1)}>
            再試一次
          </button>
        </p>
      )}
      {hiddenCount > 0 && (
        <p className="quiet-note">
          有 {hiddenCount} 則隱藏的註記。
          <button
            type="button"
            className="text-link is-plain"
            aria-pressed={showHidden}
            onClick={() => setShowHidden(!showHidden)}
          >
            {showHidden ? "收起隱藏的註記" : "顯示隱藏的註記"}
          </button>
        </p>
      )}
      {canAnnotate && (
        <p className="field-hint material-hint">
          選取文字就能加註記。註記不會改動教材，也不會出現在學生版；勾了「提供給 AI」的才會被參考。
        </p>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- the viewer

const PANE_WIDTH_KEY = "manna_dls_material_pane_width";
const PANE_MIN = 320;
const PANE_MAX = 760;
const PANE_DEFAULT = 448;

function loadPaneWidth(): number {
  try {
    const stored = Number(window.localStorage.getItem(PANE_WIDTH_KEY));
    if (Number.isFinite(stored) && stored >= PANE_MIN && stored <= PANE_MAX) return stored;
  } catch {
    // storage blocked: the default width is used
  }
  return PANE_DEFAULT;
}

export interface MaterialViewerProps {
  /** The material to show; null shows `empty`. */
  materialId: number | null;
  /**
   * `panel`: slides in from the left over the page's side rail (the lesson SOP's 對照教材).
   * `pane`: an in-flow column beside the work area that can be resized, collapsed and taken
   * full screen (the worksheet generator). At phone width both are a full-screen sheet.
   */
  mode: "panel" | "pane";
  /** Panel mode: whether it is showing. A pane decides for itself (collapse). */
  open?: boolean;
  /** Panel mode: 收起教材 and Escape. */
  onClose?: () => void;
  /** The material when the page already has it (skips one request). */
  material?: MaterialDetail | SopMaterial | null;
  /** What to say when there is no material. */
  empty?: ReactNode;
  /** Pane mode: start collapsed. */
  defaultCollapsed?: boolean;
  /** Notes on the text (the teacher's own materials). A platform material is read only. */
  annotate?: boolean;
}

/** Where the worksheet stacks its columns (worksheet.css, with the sidebar taking 248px) the pane must not be open on arrival. */
function paneOverlaysPage(): boolean {
  try {
    return typeof window.matchMedia === "function" && window.matchMedia("(max-width: 75rem)").matches;
  } catch {
    return false;
  }
}

/**
 * 教材檢視: the material, readable while working on something else, with the teacher's notes.
 * One component for the lesson SOP and the worksheet generator (docs/DESIGN_OVERHAUL.md).
 *
 * ```tsx
 * <div className="material-split">
 *   <MaterialViewer mode="pane" materialId={materialId} />
 *   <div className="material-split-main">…settings, result or editor…</div>
 * </div>
 * ```
 */
export function MaterialViewer({
  materialId,
  mode,
  open = true,
  onClose,
  material,
  empty,
  defaultCollapsed = false,
  annotate = true,
}: MaterialViewerProps) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const [collapsed, setCollapsed] = useState(() => defaultCollapsed || (mode === "pane" && paneOverlaysPage()));
  const [full, setFull] = useState(false);
  const [width, setWidth] = useState(loadPaneWidth);
  const showing = mode === "panel" ? open : !collapsed;

  useEffect(() => {
    if (mode !== "panel" || !open) return;
    closeRef.current?.focus();
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onClose?.();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [mode, open, onClose]);

  const resize = useCallback((next: number) => {
    const clamped = Math.min(PANE_MAX, Math.max(PANE_MIN, Math.round(next)));
    setWidth(clamped);
    try {
      window.localStorage.setItem(PANE_WIDTH_KEY, String(clamped));
    } catch {
      // storage blocked: the width lasts for this visit only
    }
  }, []);

  const startDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = width;
    const onMove = (e: PointerEvent) => resize(startWidth + (e.clientX - startX));
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const onSeparatorKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") resize(width - 24);
    else if (event.key === "ArrowRight") resize(width + 24);
    else if (event.key === "Home") resize(PANE_MIN);
    else if (event.key === "End") resize(PANE_MAX);
    else return;
    event.preventDefault();
  };

  const body =
    materialId === null ? (
      (empty ?? <p className="quiet-note">還沒有教材。</p>)
    ) : (
      <MaterialContent key={materialId} materialId={materialId} material={material} annotate={annotate} />
    );

  if (mode === "panel") {
    if (!open) return null;
    return (
      <aside id="material-viewer" className="material-viewer" aria-label="教材">
        <div className="material-viewer-head">
          <h2>教材</h2>
          <button type="button" className="text-link is-plain" ref={closeRef} onClick={onClose}>
            收起教材
          </button>
        </div>
        {body}
      </aside>
    );
  }

  if (!showing) {
    return (
      <aside className="material-pane is-collapsed" aria-label="教材">
        <button type="button" className="material-pane-open" aria-expanded={false} onClick={() => setCollapsed(false)}>
          對照教材
        </button>
      </aside>
    );
  }
  return (
    <aside
      className={`material-pane${full ? " is-full" : ""}`}
      aria-label="教材"
      style={full ? undefined : { width: `${width}px` }}
    >
      <div className="material-viewer-head">
        <h2>教材{material?.kind && material.kind !== "text" ? ` · ${KIND_LABEL[material.kind]}` : ""}</h2>
        <div className="material-pane-tools">
          <button type="button" className="text-link is-plain" aria-pressed={full} onClick={() => setFull(!full)}>
            {full ? "離開全螢幕" : "全螢幕"}
          </button>
          <button
            type="button"
            className="text-link is-plain"
            aria-expanded
            onClick={() => {
              setFull(false);
              setCollapsed(true);
            }}
          >
            收起教材
          </button>
        </div>
      </div>
      <div className="material-pane-body">{body}</div>
      {!full && (
        <div
          className="material-pane-resizer"
          role="separator"
          aria-orientation="vertical"
          aria-label="調整教材欄的寬度"
          aria-valuemin={PANE_MIN}
          aria-valuemax={PANE_MAX}
          aria-valuenow={width}
          tabIndex={0}
          onPointerDown={startDrag}
          onKeyDown={onSeparatorKey}
        />
      )}
    </aside>
  );
}
