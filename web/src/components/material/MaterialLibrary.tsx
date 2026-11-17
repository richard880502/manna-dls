import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { api, errorMessage } from "../../api/client";
import type { Material, MaterialDetail, MaterialLibrary, MaterialPurpose } from "../../api/types";
import {
  UPLOAD_WARNING,
  acceptAttribute,
  describeLimits,
  formatBytes,
  materialMeta,
  ruleForFile,
} from "../../lib/materials";
import { OCR_NOT_YET, ocrOpen } from "../../lib/ocr";
import { ErrorText } from "../common";
import { useFirstOnPage } from "../OncePerPage";

/** The teacher's library as the server reports it: materials, usage, limits, whether uploads work. */
export function useMaterialLibrary() {
  const [library, setLibrary] = useState<MaterialLibrary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [seq, setSeq] = useState(0);
  useEffect(() => {
    let alive = true;
    api
      .listLibraryMaterials()
      .then((lib) => {
        if (!alive) return;
        setLibrary(lib);
        setError(null);
      })
      .catch((err: unknown) => {
        if (alive) setError(errorMessage(err));
      });
    return () => {
      alive = false;
    };
  }, [seq]);
  const reload = useCallback(() => setSeq((n) => n + 1), []);
  return { library, error, reload };
}

export type AddWay = "upload" | "paste" | "link";

export const ADD_WAY_LABEL: Record<AddWay, string> = { upload: "上傳檔案", paste: "貼上文字", link: "加入網址" };

interface AddFormProps {
  way: AddWay;
  library: MaterialLibrary;
  /** Link the new material to this lesson. */
  lessonId?: number;
  onAdded: (material: MaterialDetail) => void;
  onCancel?: () => void;
  /** The submit button's words when the upload leads somewhere else (e.g. 上傳並讀取設計). */
  submitLabel?: string;
  /** `worksheet_sample` for a worksheet uploaded as a sample: it stays out of 我的教材. */
  purpose?: MaterialPurpose;
}

/**
 * One way of adding a material. Every way carries the same warning: no student work, names,
 * grades or photos; the material stays the teacher's own.
 */
export function MaterialAddForm({ way, library, lessonId, onAdded, onCancel, submitLabel, purpose }: AddFormProps) {
  const ids = useId();
  const [title, setTitle] = useState("");
  const [sourceNote, setSourceNote] = useState("");
  const [text, setText] = useState("");
  const [url, setUrl] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const rules = library.limits.files;
  // The privacy reminder is said once per page (another add form may already show it).
  const showWarning = useFirstOnPage();

  const fileProblem = (() => {
    if (!file) return null;
    const rule = ruleForFile(file.name, rules);
    if (!rule) return `不支援這種檔案。可以上傳：${rules.map((r) => `.${r.ext}`).join("、")}。`;
    if (file.size > rule.max_bytes) {
      return `這個檔案是 ${formatBytes(file.size)}，超過這類檔案的上限 ${formatBytes(rule.max_bytes)}。`;
    }
    if (library.usage.used_bytes + file.size > library.usage.quota_bytes) {
      return `教材庫的空間不夠（上限 ${formatBytes(library.usage.quota_bytes)}，已使用 ${formatBytes(library.usage.used_bytes)}）。請先刪除不再使用的教材。`;
    }
    return null;
  })();

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    let work: Promise<MaterialDetail>;
    if (way === "upload") {
      if (!file) {
        setError("請選擇要上傳的檔案。");
        return;
      }
      if (fileProblem) return;
      work = api.uploadLibraryMaterial(file, {
        title: title.trim(),
        source_note: sourceNote.trim(),
        lesson_id: lessonId,
        ...(purpose ? { purpose } : {}),
      });
    } else if (way === "paste") {
      if (!text.trim()) {
        setError("請貼上教材文字。");
        return;
      }
      work = api.createLibraryMaterial({
        kind: "text",
        title: title.trim(),
        source_note: sourceNote.trim(),
        text,
        lesson_id: lessonId,
        ...(purpose ? { purpose } : {}),
      });
    } else {
      if (!url.trim()) {
        setError("請輸入網址。");
        return;
      }
      work = api.createLibraryMaterial({
        kind: "link",
        title: title.trim(),
        source_note: sourceNote.trim(),
        url: url.trim(),
        lesson_id: lessonId,
      });
    }
    setBusy(true);
    try {
      onAdded(await work);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  if (way === "upload" && !library.uploads_enabled) {
    return (
      <div className="material-add" role="group" aria-label={ADD_WAY_LABEL[way]}>
        <p className="material-state">
          這個環境還沒有設定檔案儲存空間，所以現在不能上傳檔案。你仍然可以貼上文字或加入網址。
        </p>
        {onCancel && (
          <button type="button" className="text-link is-plain" onClick={onCancel}>
            取消
          </button>
        )}
      </div>
    );
  }

  return (
    <form className="material-add" onSubmit={onSubmit} aria-label={ADD_WAY_LABEL[way]}>
      {showWarning && (
        <p className="material-warning" role="note">
          {UPLOAD_WARNING}
        </p>
      )}
      {way === "upload" && (
        <>
          <label className="field-label" htmlFor={`${ids}-file`}>
            選擇檔案
          </label>
          <input
            id={`${ids}-file`}
            ref={fileRef}
            type="file"
            accept={acceptAttribute(rules)}
            aria-describedby={`${ids}-limits`}
            disabled={busy}
            onChange={(e) => {
              setFile(e.target.files?.[0] ?? null);
              setError(null);
            }}
          />
          <p className="field-hint" id={`${ids}-limits`}>
            {describeLimits(rules)}。文件會擷取文字供對照與註記；掃描的 PDF 與課本照片會在上傳後試著辨識文字（OCR）；影音只保存和預覽。
            已使用 {formatBytes(library.usage.used_bytes)}／{formatBytes(library.usage.quota_bytes)}。
          </p>
          <ErrorText message={fileProblem} />
        </>
      )}
      {way === "link" && (
        <>
          <label className="field-label" htmlFor={`${ids}-url`}>
            網址
          </label>
          <input
            id={`${ids}-url`}
            type="url"
            inputMode="url"
            value={url}
            placeholder="https://"
            disabled={busy}
            onChange={(e) => setUrl(e.target.value)}
          />
          <p className="field-hint">只記下連結、標題與來源說明，不會擷取或保存網頁內容。</p>
        </>
      )}
      <label className="field-label" htmlFor={`${ids}-title`}>
        教材標題
      </label>
      <input
        id={`${ids}-title`}
        type="text"
        value={title}
        placeholder={way === "upload" ? "不填就用檔名" : "例如：L3 The Umbrella Stand"}
        disabled={busy}
        onChange={(e) => setTitle(e.target.value)}
      />
      {way === "paste" && (
        <>
          <label className="field-label" htmlFor={`${ids}-text`}>
            教材文字
          </label>
          <textarea
            id={`${ids}-text`}
            className="material-input"
            rows={10}
            lang="en"
            value={text}
            placeholder="貼上課文或文章。段落之間空一行。"
            disabled={busy}
            onChange={(e) => setText(e.target.value)}
          />
        </>
      )}
      <label className="field-label" htmlFor={`${ids}-source`}>
        來源說明
      </label>
      <input
        id={`${ids}-source`}
        type="text"
        value={sourceNote}
        placeholder="例如：龍騰版第二冊第三課；或文章的出處"
        disabled={busy}
        onChange={(e) => setSourceNote(e.target.value)}
      />
      <ErrorText message={error} />
      <div className="button-row">
        <button type="submit" className="primary-button" disabled={busy || (way === "upload" && fileProblem !== null)}>
          {busy
            ? way === "upload"
              ? "上傳中…"
              : "儲存中…"
            : (submitLabel ?? (way === "upload" ? "上傳" : "加入教材庫"))}
        </button>
        {onCancel && (
          <button type="button" className="text-link is-plain" disabled={busy} onClick={onCancel}>
            取消
          </button>
        )}
      </div>
    </form>
  );
}

export const titleOf = (m: { title: string }) => m.title.trim() || "未命名教材";

/** 從教材庫選: the teacher's materials, one 選用 each. */
export function MaterialPicker({
  library,
  currentId,
  busy,
  onPick,
}: {
  library: MaterialLibrary;
  currentId: number | null;
  busy: boolean;
  onPick: (material: Material) => void;
}) {
  if (library.materials.length === 0) {
    return <p className="empty-line">教材庫還沒有教材。可以貼上文字，或上傳檔案。</p>;
  }
  return (
    <ul className="rows material-rows" aria-label="教材庫">
      {library.materials.map((m) => (
        <li key={m.id} className="row">
          <div>
            <h3>{titleOf(m)}</h3>
            <p className="meta">{materialMeta(m)}</p>
          </div>
          <p className="side">
            {m.id === currentId ? (
              <span>目前使用中</span>
            ) : (
              <button
                type="button"
                className="secondary-button small-button"
                disabled={busy || ocrOpen(m.ocr)}
                title={ocrOpen(m.ocr) ? OCR_NOT_YET : undefined}
                aria-label={`選用「${titleOf(m)}」`}
                onClick={() => onPick(m)}
              >
                選用
              </button>
            )}
          </p>
        </li>
      ))}
    </ul>
  );
}

/** What deleting this material takes with it, said before the teacher confirms. */
export function deleteConsequences(m: Material): string {
  const parts = [m.has_file ? "原檔、擷取的文字與段落" : "文字與段落"];
  if (m.annotation_count > 0) parts.push(`${m.annotation_count} 則註記`);
  let text = `刪除後無法復原：${parts.join("、")}都會永久刪除。`;
  if (m.lessons.length > 0) {
    const names = m.lessons.map((l) => `「${l.lesson_title}」`).join("、");
    text += `用到它的課例（${names}）會顯示「教材已刪除」，課例裡引用的原文片段也會移除；你寫的分析、目標與任務會保留。`;
  }
  return text;
}
