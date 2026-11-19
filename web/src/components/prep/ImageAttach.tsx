import { useCallback, useEffect, useRef, useState, type ClipboardEvent, type DragEvent } from "react";
import { api, errorMessage } from "../../api/client";
import type { PictureOutcome, PrepImage } from "../../api/types";
import {
  IMAGE_ACCEPT,
  IMAGE_COUNT_MESSAGE,
  IMAGE_MAX_COUNT,
  checkImageFile,
  imageFilesOf,
  outcomeNote,
} from "../../lib/prepImages";

/**
 * Pictures a teacher attaches to a revision (docs/API.md「附上圖片」): chosen, pasted or dropped,
 * uploaded at once, shown as small thumbnails above the text, sent with the message.
 */

export interface AttachedImage {
  key: number;
  name: string;
  /** A local preview (null where the browser cannot make one). */
  preview: string | null;
  /** The server's id once uploaded. */
  imageId: number | null;
}

const previewOf = (file: File): string | null => {
  try {
    return typeof URL.createObjectURL === "function" ? URL.createObjectURL(file) : null;
  } catch {
    return null;
  }
};
const release = (url: string | null) => {
  if (!url) return;
  try {
    URL.revokeObjectURL?.(url);
  } catch {
    // nothing to release
  }
};

export interface ImageAttach {
  items: AttachedImage[];
  error: string | null;
  /** An upload is still going. */
  uploading: boolean;
  /** Ids of the uploaded images, in the order attached. */
  imageIds: number[];
  add: (files: File[]) => void;
  remove: (item: AttachedImage) => void;
  /** Forget the thumbnails (after the message was sent: the images now belong to it). */
  clear: () => void;
}

export function useImageAttach(sessionId: number): ImageAttach {
  const [items, setItems] = useState<AttachedImage[]>([]);
  const [error, setError] = useState<string | null>(null);
  const itemsRef = useRef<AttachedImage[]>([]);
  const cancelled = useRef(new Set<number>());
  const nextKey = useRef(1);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      itemsRef.current.forEach((i) => release(i.preview));
    };
  }, []);

  const put = useCallback((next: AttachedImage[]) => {
    itemsRef.current = next;
    setItems(next);
  }, []);

  const add = useCallback(
    (files: File[]) => {
      let message: string | null = null;
      for (const file of files) {
        const bad = checkImageFile(file);
        if (bad) {
          message = bad;
          continue;
        }
        if (itemsRef.current.length >= IMAGE_MAX_COUNT) {
          message = IMAGE_COUNT_MESSAGE;
          continue;
        }
        const item: AttachedImage = { key: nextKey.current++, name: file.name, preview: previewOf(file), imageId: null };
        put([...itemsRef.current, item]);
        api
          .uploadPrepImage(sessionId, file)
          .then((image: PrepImage) => {
            if (cancelled.current.delete(item.key)) {
              void api.deletePrepImage(sessionId, image.id).catch(() => undefined);
              return;
            }
            if (!alive.current) return;
            put(itemsRef.current.map((i) => (i.key === item.key ? { ...i, imageId: image.id } : i)));
          })
          .catch((err: unknown) => {
            if (cancelled.current.delete(item.key) || !alive.current) return;
            release(item.preview);
            put(itemsRef.current.filter((i) => i.key !== item.key));
            setError(errorMessage(err));
          });
      }
      setError(message);
    },
    [sessionId, put],
  );

  const remove = useCallback(
    (item: AttachedImage) => {
      if (item.imageId === null) cancelled.current.add(item.key);
      else void api.deletePrepImage(sessionId, item.imageId).catch(() => undefined);
      release(item.preview);
      put(itemsRef.current.filter((i) => i.key !== item.key));
      setError(null);
    },
    [sessionId, put],
  );

  const clear = useCallback(() => {
    itemsRef.current.forEach((i) => release(i.preview));
    put([]);
    setError(null);
  }, [put]);

  return {
    items,
    error,
    uploading: items.some((i) => i.imageId === null),
    imageIds: items.flatMap((i) => (i.imageId === null ? [] : [i.imageId])),
    add,
    remove,
    clear,
  };
}

/** Drop highlight and paste/drop handlers for the composer box. */
export function useImageDrop(enabled: boolean, add: (files: File[]) => void) {
  const [dragging, setDragging] = useState(false);
  const hasFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes("Files");
  return {
    dragging,
    onDragEnter: (e: DragEvent) => {
      if (enabled && hasFiles(e)) setDragging(true);
    },
    onDragOver: (e: DragEvent) => {
      if (!enabled) return;
      e.preventDefault();
      if (hasFiles(e)) setDragging(true);
    },
    onDragLeave: (e: DragEvent) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false);
    },
    onDrop: (e: DragEvent) => {
      if (!enabled) return;
      e.preventDefault();
      setDragging(false);
      const files = imageFilesOf(e.dataTransfer?.files);
      if (files.length > 0) add(files);
    },
    onPaste: (e: ClipboardEvent) => {
      if (!enabled) return;
      const fromItems = Array.from(e.clipboardData?.items ?? []).flatMap((i) => {
        const f = i.kind === "file" ? i.getAsFile() : null;
        return f ? [f] : [];
      });
      const files = imageFilesOf(e.clipboardData?.files?.length ? e.clipboardData.files : fromItems);
      if (files.length === 0) return;
      // Pasted text still pastes; a pasted picture is attached instead.
      e.preventDefault();
      add(files);
    },
  };
}

/** The 「附上圖片」 button and the hidden file input behind it. */
export function AttachButton({ onFiles, disabled }: { onFiles: (files: File[]) => void; disabled?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <>
      <button
        type="button"
        className="prep-attach"
        aria-label="附上圖片"
        title="附上圖片"
        disabled={disabled}
        onClick={() => input.current?.click()}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <path
            d="M20 12.5 12.6 20a5 5 0 0 1-7-7l8-8a3.3 3.3 0 0 1 4.7 4.7l-8 8a1.7 1.7 0 0 1-2.4-2.4l7.3-7.3"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
      <input
        ref={input}
        type="file"
        className="sr-only"
        data-testid="prep-attach-input"
        accept={IMAGE_ACCEPT}
        multiple
        tabIndex={-1}
        aria-hidden="true"
        onChange={(e) => {
          onFiles(Array.from(e.target.files ?? []));
          e.target.value = "";
        }}
      />
    </>
  );
}

/** The thumbnails of the images about to be sent (above the text). */
export function AttachedThumbs({ attach }: { attach: ImageAttach }) {
  if (attach.items.length === 0) return null;
  return (
    <ul className="prep-attached" aria-label="附上的圖片">
      {attach.items.map((item) => (
        <li key={item.key} className="prep-attached-item" data-uploading={item.imageId === null || undefined}>
          {item.preview ? (
            <img src={item.preview} alt={item.name} />
          ) : (
            <span className="prep-attached-name">{item.name}</span>
          )}
          {item.imageId === null && (
            <span className="prep-attached-state" role="status">
              上傳中
            </span>
          )}
          <button
            type="button"
            className="prep-attached-remove"
            aria-label={`移除圖片 ${item.name}`}
            onClick={() => attach.remove(item)}
          >
            ✕
          </button>
        </li>
      ))}
    </ul>
  );
}

// ---------------------------------------------------------------- shown in the conversation

/** An attached picture as an object URL (fetched with the teacher's token). */
export function usePrepImage(sessionId: number, imageId: number) {
  const [loaded, setLoaded] = useState<{ id: number; url: string | null } | null>(null);
  useEffect(() => {
    let alive = true;
    let made: string | null = null;
    api
      .prepImage(sessionId, imageId)
      .then((blob) => {
        if (!alive) return;
        made = URL.createObjectURL(blob);
        setLoaded({ id: imageId, url: made });
      })
      .catch(() => alive && setLoaded({ id: imageId, url: null }));
    return () => {
      alive = false;
      release(made);
    };
  }, [sessionId, imageId]);
  return loaded !== null && loaded.id === imageId ? loaded.url : null;
}

/** A larger view of one picture. */
export function ImageDialog({ src, onClose }: { src: string; onClose: () => void }) {
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="prep-image-backdrop">
      <div
        className="prep-image-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="附上的圖片"
      >
        <img src={src} alt="附上的圖片（放大）" />
        <button ref={close} type="button" className="secondary-button small-button" onClick={onClose}>
          關閉
        </button>
      </div>
    </div>
  );
}

function PrepImageThumb({ sessionId, image, n }: { sessionId: number; image: PrepImage; n: number }) {
  const url = usePrepImage(sessionId, image.id);
  const [open, setOpen] = useState(false);
  const closeIt = useCallback(() => setOpen(false), []);
  return (
    <li>
      {url ? (
        <button type="button" className="prep-thumb" aria-label={`看大圖 ${n}`} onClick={() => setOpen(true)}>
          <img src={url} alt={`附上的圖片 ${n}`} />
        </button>
      ) : (
        <span className="prep-thumb is-loading" aria-hidden="true" />
      )}
      {open && url && <ImageDialog src={url} onClose={closeIt} />}
    </li>
  );
}

/** Small thumbnails of the pictures a message (or a proposal) came with. */
export function PrepImageThumbs({ sessionId, images }: { sessionId?: number; images: PrepImage[] | undefined }) {
  if (!images || images.length === 0) return null;
  return (
    <ul className="prep-thumbs" aria-label="附上的圖片">
      {images.map((image, i) => (
        <PrepImageThumb key={image.id} sessionId={sessionId ?? image.session_id} image={image} n={i + 1} />
      ))}
    </ul>
  );
}

/** What Manna did with its picture actions: the line, and how it went. */
export function PictureOutcomeList({ outcomes }: { outcomes: PictureOutcome[] | undefined }) {
  if (!outcomes || outcomes.length === 0) return null;
  return (
    <ul className="prep-picture-outcomes" aria-label="圖片">
      {outcomes.map((o, i) => (
        <li key={`${o.action}-${i}`} data-status={o.status}>
          <span>{o.text}</span>
          <span className="prep-picture-outcome-note">：{outcomeNote(o)}</span>
        </li>
      ))}
    </ul>
  );
}
