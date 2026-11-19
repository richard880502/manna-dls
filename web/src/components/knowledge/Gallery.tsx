import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api } from "../../api/client";
import type { MaterialCard as MaterialCardData } from "../../api/types";
import { ocrBadge, ocrOpen } from "../../lib/ocr";
import { Icon } from "../Icon";
import { MaterialBrowser } from "../library/BrowsePanel";
import { TagList } from "../library/Cards";
import { cardTitle, fileTypeLabel, materialCardMeta } from "../library/labels";
import { Dialog } from "../Dialog";
import { MaterialContent } from "../material/MaterialViewer";
import "./knowledge.css";

/** Uploaded PDFs and images have a preview image (docs/API.md「知識庫：教材牆與預覽圖」). */
export const hasThumbnail = (card: Pick<MaterialCardData, "file_type">) =>
  card.file_type === "pdf" || card.file_type === "image";

/** A card this far below the screen already asks for its preview. */
const NEAR_SCREEN = "600px";

/**
 * A card's preview image as an object URL, asked for once the card comes near the screen (at
 * once where IntersectionObserver is missing). `failed`: there is none to show (no PDF renderer
 * on the server, a damaged file…), and the card shows its first lines instead.
 */
function useThumbnail(id: number, wanted: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(() => typeof IntersectionObserver === "undefined");
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!wanted || near || !el) return undefined;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) setNear(true);
      },
      { rootMargin: NEAR_SCREEN },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [wanted, near]);

  useEffect(() => {
    if (!wanted || !near) return undefined;
    let alive = true;
    let made: string | null = null;
    api
      .materialThumbnail(id)
      .then((blob) => {
        if (!alive) return;
        made = URL.createObjectURL(blob);
        setUrl(made);
      })
      .catch(() => {
        if (alive) setFailed(true);
      });
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [id, wanted, near]);

  return { ref, url, failed };
}

/** The top of a text card: its first lines (up to ~280 characters), faded where the window ends. */
function Excerpt({ card }: { card: MaterialCardData }) {
  if (card.excerpt) return <p className="kb-excerpt">{card.excerpt}</p>;
  return (
    <span className="kb-excerpt-empty">
      <Icon name="file" />
      {ocrOpen(card.ocr) ? "正在辨識文字" : "沒有可預覽的文字"}
    </span>
  );
}

interface Opened {
  card: MaterialCardData;
  /** The card's preview image, when it has one. */
  thumb: string | null;
  /** The card's button: focus goes back there. */
  from: HTMLElement | null;
}

/**
 * One material on the wall, the same size as every other: the top of its first page (or image),
 * or its first lines; the file type and an OCR state in the corners; the title in two lines at
 * most (the button that opens the preview: the whole card is its target), category and meta on
 * one line, one row of tags.
 */
function GalleryCard({ card, onOpen }: { card: MaterialCardData; onOpen: (opened: Opened) => void }) {
  const ids = useId();
  const wanted = hasThumbnail(card);
  const { ref, url, failed } = useThumbnail(card.id, wanted);
  const image = wanted && !failed;
  const ocr = ocrBadge(card.ocr);
  let art;
  if (!image) art = <Excerpt card={card} />;
  else if (url) art = <img src={url} alt="" />;
  else art = <span className="kb-thumb-wait" />;
  return (
    <li className="kb-card">
      <article className="kb-card-inner" aria-labelledby={`${ids}-title`}>
        <div className={`kb-thumb${image ? ` is-image is-${card.file_type}` : " is-text"}`} ref={ref}>
          <div className="kb-thumb-art" aria-hidden="true">
            {art}
          </div>
          <span className="lib-badge is-type">{fileTypeLabel(card.file_type)}</span>
          {ocr && <span className={`lib-badge is-ocr ${ocrOpen(card.ocr) ? "is-extracting" : "is-failed"}`}>{ocr}</span>}
        </div>
        <div className="kb-card-body">
          <h3 className="kb-card-title" id={`${ids}-title`}>
            <button
              type="button"
              className="kb-card-open"
              aria-haspopup="dialog"
              onClick={(e) => onOpen({ card, thumb: url, from: e.currentTarget })}
            >
              <span className="kb-card-title-text">{cardTitle(card)}</span>
            </button>
          </h3>
          <p className="lib-card-meta">{[card.category, materialCardMeta(card)].filter(Boolean).join(" · ")}</p>
          <div className="kb-card-tags">
            <TagList tags={card.tags} />
          </div>
        </div>
      </article>
    </li>
  );
}

/**
 * The preview: the first page beside the text Manna read from the file (the viewer's sheet,
 * without notes; images, audio and video play there, and the original can be downloaded).
 */
function PreviewDialog({ opened, onClose }: { opened: Opened; onClose: () => void }) {
  const { card, thumb, from } = opened;
  const returnFocus = useCallback(() => from, [from]);
  const title = cardTitle(card);
  const ocr = ocrBadge(card.ocr);
  // An image is shown whole by the viewer; a PDF's first page goes beside its text.
  const page = card.file_type === "pdf" ? thumb : null;
  return (
    <Dialog title={title} size="large" returnFocus={returnFocus} onClose={onClose}>
      <p className="kb-preview-meta">{[card.category, materialCardMeta(card), ocr].filter(Boolean).join(" · ")}</p>
      {card.tags.length > 0 && (
        <ul className="lib-tags" aria-label="標籤">
          {card.tags.map((t) => (
            <li key={t} className="lib-tag">
              {t}
            </li>
          ))}
        </ul>
      )}
      <div className={`kb-preview${page ? " has-page" : ""}`}>
        {page && (
          <figure className="kb-preview-page">
            <img src={page} alt={`「${title}」的第 1 頁`} />
            <figcaption>第 1 頁{card.page_count ? `（共 ${card.page_count} 頁）` : ""}</figcaption>
          </figure>
        )}
        <div className="kb-preview-text">
          <MaterialContent materialId={card.id} annotate={false} />
        </div>
      </div>
    </Dialog>
  );
}

/**
 * 知識庫's wall of materials (/knowledge): every platform material (平台教材) as a card of its own
 * height in columns, with the browse search, category chips, filters and tags. A card opens its
 * preview.
 */
export function KnowledgeGallery() {
  const [opened, setOpened] = useState<Opened | null>(null);
  const close = useCallback(() => setOpened(null), []);
  return (
    <>
      <MaterialBrowser
        area="reference"
        layout="wall"
        labels={{
          search: "搜尋知識庫",
          placeholder: "搜尋標題、內容或標籤，例如：114 學測、參考答案",
          list: "知識庫教材",
        }}
        empty={<p className="empty-line">知識庫還沒有教材。按「上傳教材」放進第一份。</p>}
        renderCard={(card) => <GalleryCard key={card.id} card={card} onOpen={setOpened} />}
      />
      {opened && <PreviewDialog opened={opened} onClose={close} />}
    </>
  );
}
