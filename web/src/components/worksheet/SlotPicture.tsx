import { useEffect, useId, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { WorksheetImages, WsImageSlotKind, WsImageStatus, WsSheetImage, WsSlotImage } from "../../api/types";

/**
 * 學習單的圖片 (docs/API.md「學習單的圖片」): one picture slot of a block, drawn after the block's
 * instructions. A picture that is there; otherwise a dashed box (with what it is for, for the
 * teacher). The teacher can 重畫 (with a note), 換成我的圖 or 拿掉 it.
 */

/** What one slot shows, from the editor's content or from the server's sheet. */
export interface PictureView {
  slotId: string;
  status: WsImageStatus;
  imageId: number | null;
  alt: string;
  caption: string;
  aiGenerated: boolean;
  /** For the teacher: what the picture is for (null on the student sheet). */
  purpose: string | null;
  /** Why there is no picture (teacher only). */
  message: string | null;
  kind: WsImageSlotKind | null;
}

/** Statuses while the picture is being made. */
export const isOpenStatus = (s: WsImageStatus) => s === "queued" || s === "running";

export const anyOpen = (images: WorksheetImages | null | undefined) =>
  (images?.slots ?? []).some((s) => isOpenStatus(s.status));

/** A slot of the sheet with the latest state of its picture on top (the sheet may be older). */
export function viewOfSheetImage(img: WsSheetImage, live: WsSlotImage | undefined): PictureView {
  return {
    slotId: img.slot_id,
    status: live?.status ?? img.status,
    imageId: live ? live.image_id : img.image_id,
    alt: img.alt,
    caption: img.caption,
    aiGenerated: live ? live.status === "done" && live.source === "generated" : img.ai_generated,
    purpose: img.purpose ?? null,
    message: live ? live.message : (img.message ?? null),
    kind: null,
  };
}

/** A picture of the teacher's worksheet as an object URL (fetched with her token). */
export function useWorksheetImage(worksheetId: number, imageId: number | null) {
  // What was loaded, for which picture (a different picture shows nothing until it loads).
  const [loaded, setLoaded] = useState<{ id: number; url: string | null; failed: boolean } | null>(null);
  useEffect(() => {
    if (imageId === null) return undefined;
    let alive = true;
    let made: string | null = null;
    api
      .worksheetImage(worksheetId, imageId)
      .then((blob) => {
        if (!alive) return;
        made = URL.createObjectURL(blob);
        setLoaded({ id: imageId, url: made, failed: false });
      })
      .catch(() => {
        if (alive) setLoaded({ id: imageId, url: null, failed: true });
      });
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [worksheetId, imageId]);
  const current = loaded !== null && loaded.id === imageId ? loaded : null;
  return { url: current?.url ?? null, failed: current?.failed ?? false };
}

/** What the teacher can do with a slot; null where pictures are only shown. */
export interface PictureActions {
  worksheetId: number;
  images: WorksheetImages | null;
  /** The pictures after an action (the server's answer). */
  onImages: (next: WorksheetImages) => void;
  disabled?: boolean;
}

const ACCEPT = "image/png,image/jpeg,image/webp,image/gif";

function Tools({ view, actions, name }: { view: PictureView; actions: PictureActions; name: string }) {
  const ids = useId();
  const [hinting, setHinting] = useState(false);
  const [hint, setHint] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const enabled = actions.images?.enabled ?? false;
  const left = actions.images?.regenerations_left ?? 0;
  const photo = view.kind === "photo_needed";
  const open = isOpenStatus(view.status);
  const off = busy || actions.disabled === true;

  async function run(call: () => Promise<WorksheetImages>) {
    setBusy(true);
    setError(null);
    try {
      actions.onImages(await call());
      return true;
    } catch (err) {
      setError(errorMessage(err));
      return false;
    } finally {
      setBusy(false);
    }
  }

  const redraw = async () => {
    const ok = await run(() => api.regenerateWorksheetImage(actions.worksheetId, view.slotId, hint));
    if (ok) {
      setHinting(false);
      setHint("");
    }
  };

  // Why 重畫 is not offered, if it is not.
  const noRedraw = !enabled
    ? "這個環境沒有開啟 AI 產圖，可以上傳自己的圖。"
    : photo
      ? null
      : left <= 0
        ? "這份學習單的重畫次數用完了，可以上傳自己的圖。"
        : null;

  return (
    <div className="ws-picture-tools" lang="zh-Hant" role="group" aria-label={`圖片工具 ${name}`}>
      {enabled && !photo && !hinting && (
        <button
          type="button"
          className="text-link is-plain is-gold"
          disabled={off || open || left <= 0}
          onClick={() => setHinting(true)}
        >
          重畫
        </button>
      )}
      {hinting && (
        <span className="ws-picture-hint">
          <label htmlFor={`${ids}-hint`} className="sr-only">
            想怎麼改（可以不寫）
          </label>
          <input
            id={`${ids}-hint`}
            type="text"
            maxLength={200}
            value={hint}
            placeholder="想怎麼改（可以不寫），例如：改成晴天"
            onChange={(e) => setHint(e.target.value)}
          />
          <button type="button" className="text-link is-gold" disabled={off} onClick={() => void redraw()}>
            {busy ? "送出中…" : "送出重畫"}
          </button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setHinting(false)}>
            取消
          </button>
        </span>
      )}
      <label className={`text-link is-plain ws-picture-upload${off ? " is-disabled" : ""}`}>
        換成我的圖
        <input
          type="file"
          accept={ACCEPT}
          className="sr-only"
          aria-label={`換成我的圖 ${name}`}
          disabled={off}
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) void run(() => api.uploadWorksheetImage(actions.worksheetId, view.slotId, file));
          }}
        />
      </label>
      {view.status !== "removed" && (
        <button
          type="button"
          className="text-link is-plain"
          disabled={off}
          onClick={() => void run(() => api.removeWorksheetImage(actions.worksheetId, view.slotId))}
        >
          拿掉
        </button>
      )}
      {enabled && !photo && <span className="ws-picture-left">還能重畫 {left} 次</span>}
      {noRedraw && <span className="ws-picture-left">{noRedraw}</span>}
      {error && (
        <span className="ws-picture-error" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

/**
 * One slot: the picture or an empty box, the caption, 「AI 生成」 under a generated picture
 * (teacher only), and the tools when `actions` is given. A removed slot prints nothing; with
 * tools it shows one line to bring a picture back.
 */
export function SlotPicture({
  worksheetId,
  view,
  teacher,
  actions,
  name,
}: {
  worksheetId: number;
  view: PictureView;
  teacher: boolean;
  actions: PictureActions | null;
  /** The block's name, for labels. */
  name: string;
}) {
  const done = view.status === "done" && view.imageId !== null;
  const { url, failed } = useWorksheetImage(worksheetId, done ? view.imageId : null);
  if (view.status === "removed") {
    if (!actions) return null;
    return (
      <div className="ws-picture is-removed" lang="zh-Hant">
        <p className="quiet-note">這一區的圖已經拿掉了（不會印出）。</p>
        <Tools view={view} actions={actions} name={name} />
      </div>
    );
  }
  const open = isOpenStatus(view.status);
  return (
    <figure className={`ws-picture is-${view.status}`} aria-label={`圖片 ${name}`}>
      {done && url ? (
        <img src={url} alt={view.alt} />
      ) : (
        <div className="ws-picture-box" lang="zh-Hant" role="img" aria-label={view.alt || "圖片位置"}>
          {open && (
            <p className="manna-label" role="status">
              <span className="star is-working" aria-hidden="true" />
              Manna 正在畫這張圖…
            </p>
          )}
          {done && !url && !failed && <p>載入圖片中…</p>}
          {done && failed && <p>這張圖暫時讀不到。</p>}
          {!done && !open && teacher && view.purpose && <p className="ws-picture-purpose">請放一張圖：{view.purpose}</p>}
          {!done && !open && teacher && view.message && <p className="ws-picture-message">{view.message}</p>}
        </div>
      )}
      {view.caption.trim() && <figcaption>{view.caption}</figcaption>}
      {teacher && done && view.aiGenerated && (
        <p className="ws-picture-ai" lang="zh-Hant">
          AI 生成
        </p>
      )}
      {actions && <Tools view={view} actions={actions} name={name} />}
    </figure>
  );
}
