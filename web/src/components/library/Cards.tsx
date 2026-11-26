import { useId, type ReactNode } from "react";
import { Link } from "react-router";
import type { MaterialCard as MaterialCardData, TemplateCard as TemplateCardData } from "../../api/types";
import { TOOLS, TOOL_NAME, toolMaterialPath } from "../../lib/create";
import { ocrBadge, ocrOpen } from "../../lib/ocr";
import { Icon, type IconName } from "../Icon";
import { usePopover } from "../Popover";
import {
  TEMPLATE_AREA_LABEL,
  USE_MATERIAL,
  cardTitle,
  fileTypeLabel,
  materialCardMeta,
  templateCardMeta,
  templateCardTitle,
} from "./labels";
import { MetaList } from "../MetaList";

const TAGS_SHOWN = 3;

/** Up to three tags, then 「+n」. */
export function TagList({ tags }: { tags: string[] }) {
  if (tags.length === 0) return null;
  const rest = tags.length - TAGS_SHOWN;
  return (
    <ul className="lib-tags" aria-label="標籤">
      {tags.slice(0, TAGS_SHOWN).map((t) => (
        <li key={t} className="lib-tag">
          {t}
        </li>
      ))}
      {rest > 0 && (
        <li className="lib-tag is-more" title={tags.slice(TAGS_SHOWN).join("、")}>
          +{rest}
        </li>
      )}
    </ul>
  );
}

const KIND_ICON: Record<string, IconName> = {
  image: "file",
  audio: "file",
  video: "file",
  link: "paperclip",
};

/** The top of a material card: its first lines set as print, fading out at the bottom. */
function MaterialPaper({ card }: { card: MaterialCardData }) {
  return (
    <div className="lib-paper" aria-hidden="true">
      {card.excerpt ? (
        <p className="lib-paper-text" lang="en">
          {card.excerpt}
        </p>
      ) : (
        <span className="lib-paper-empty">
          <Icon name={KIND_ICON[card.kind] ?? "file"} />
          {ocrOpen(card.ocr) ? "正在辨識文字" : card.text_status === "ready" ? "" : "沒有可預覽的文字"}
        </span>
      )}
    </div>
  );
}

/** The top of a template card: its sections, numbered, on a sheet with a name line. */
function TemplatePaper({ card }: { card: TemplateCardData }) {
  return (
    <div className="lib-paper is-template" aria-hidden="true">
      <span className="lib-paper-header" />
      {card.status === "ready" && card.section_titles.length > 0 ? (
        <ol className="lib-paper-sections" lang="en">
          {card.section_titles.map((t, i) => (
            // oxlint-disable-next-line react/no-array-index-key -- titles may repeat; the order is the identity
            <li key={i}>{t.trim() || "（沒有標題）"}</li>
          ))}
        </ol>
      ) : (
        <span className={`lib-paper-lines${card.status === "extracting" ? " is-working" : ""}`} />
      )}
    </div>
  );
}

/**
 * A material as a card: a paper preview of its first lines with the file type (and 學習單？ when
 * Manna thinks it is one) in the corners, the title, a meta line, tags, whose it is, and actions.
 */
export function MaterialCard({ card, actions }: { card: MaterialCardData; actions?: ReactNode }) {
  const ids = useId();
  const ocr = ocrBadge(card.ocr);
  return (
    <li className="lib-card">
      <article className="lib-card-inner" aria-labelledby={`${ids}-title`}>
        <div className="lib-thumb">
          <MaterialPaper card={card} />
          {card.looks_like_worksheet && <span className="lib-badge is-sheet">學習單？</span>}
          <span className="lib-badge is-type">{fileTypeLabel(card.file_type)}</span>
          {ocr && <span className={`lib-badge is-ocr ${ocrOpen(card.ocr) ? "is-extracting" : "is-failed"}`}>{ocr}</span>}
        </div>
        <div className="lib-card-body">
          <h3 className="lib-card-title" id={`${ids}-title`}>
            {cardTitle(card)}
          </h3>
          <MetaList className="lib-card-meta" items={materialCardMeta(card)} />
          <TagList tags={card.tags} />
          <p className="lib-card-foot">
            <span className={`lib-owner${card.scope === "platform" ? " is-platform" : ""}`}>{card.owner_label}</span>
            {card.category && <span className="lib-card-category">{card.category}</span>}
          </p>
        </div>
        <div className="lib-card-actions">{actions}</div>
      </article>
    </li>
  );
}

/**
 * A template as a card: its sections on paper with 平台範本／我的 (and 讀取中／失敗) in the
 * corners, the title, 「4 區」「英文指示」「閱讀理解」, tags, and actions.
 */
export function TemplateCard({ card, actions }: { card: TemplateCardData; actions: ReactNode }) {
  const ids = useId();
  const status = card.status === "extracting" ? "讀取中" : card.status === "failed" ? "失敗" : null;
  return (
    <li className="lib-card">
      <article className="lib-card-inner" aria-labelledby={`${ids}-title`}>
        <div className="lib-thumb">
          <TemplatePaper card={card} />
          <span className={`lib-badge is-scope${card.scope === "platform" ? " is-platform" : ""}`}>
            {card.scope === "platform" ? TEMPLATE_AREA_LABEL.platform : "我的"}
          </span>
          {status && <span className={`lib-badge is-status is-${card.status}`}>{status}</span>}
        </div>
        <div className="lib-card-body">
          <h3 className="lib-card-title" id={`${ids}-title`}>
            {templateCardTitle(card)}
          </h3>
          <MetaList className="lib-card-meta" items={templateCardMeta(card)} />
          <TagList tags={card.tags} />
          <p className="lib-card-foot">
            <span className={`lib-owner${card.scope === "platform" ? " is-platform" : ""}`}>{card.owner_label}</span>
            {card.worksheet_kind.trim() && <span className="lib-card-category">{card.worksheet_kind.trim()}</span>}
          </p>
        </div>
        <div className="lib-card-actions">{actions}</div>
      </article>
    </li>
  );
}

/** A grey card while the first page loads. */
export function SkeletonCards({ count = 6 }: { count?: number }) {
  return (
    <ul className="lib-grid is-skeleton" aria-hidden="true">
      {Array.from({ length: count }, (_, i) => (
        <li key={i} className="lib-card">
          <div className="lib-card-inner">
            <div className="lib-thumb" />
            <div className="lib-card-body">
              <span className="lib-skeleton-line" />
              <span className="lib-skeleton-line is-short" />
            </div>
          </div>
        </li>
      ))}
    </ul>
  );
}

/** 「用這篇出題 ▾」: the tools, each opening its generator with this material in the article slot. */
export function UseMaterialMenu({
  card,
  disabled,
  title,
}: {
  card: Pick<MaterialCardData, "id" | "title">;
  disabled?: boolean;
  /** Why it is off (a tooltip). */
  title?: string;
}) {
  const ids = useId();
  const { open, toggle, close, rootRef, triggerRef, onBlur } = usePopover();
  const name = cardTitle(card);
  return (
    <div className="popover-root lib-use" ref={rootRef} onBlur={onBlur}>
      <button
        ref={triggerRef}
        type="button"
        className="primary-button small-button"
        aria-expanded={open}
        aria-controls={`${ids}-tools`}
        aria-label={`${USE_MATERIAL}「${name}」`}
        title={disabled ? title : undefined}
        disabled={disabled}
        onClick={toggle}
      >
        {USE_MATERIAL}
        <Icon name="chevron" />
      </button>
      {open && (
        <div className="popover is-tools lib-use-menu" id={`${ids}-tools`} role="group" aria-label={USE_MATERIAL}>
          {TOOLS.map((t) => (
            <Link key={t} className="popover-item" to={toolMaterialPath(t, card.id)} onClick={close}>
              <Icon name={t} />
              {TOOL_NAME[t]}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

/** A small popover of quiet actions behind 「⋯」 (改名, 刪除). Each gets the ⋯ button, to give focus back to. */
export function MoreMenu({
  label,
  items,
}: {
  label: string;
  items: Array<{ label: string; onSelect: (trigger: HTMLButtonElement | null) => void }>;
}) {
  const ids = useId();
  const { open, toggle, close, rootRef, triggerRef, onBlur } = usePopover();
  return (
    <div className="popover-root lib-more-menu" ref={rootRef} onBlur={onBlur}>
      <button
        ref={triggerRef}
        type="button"
        className="lib-icon-button"
        aria-expanded={open}
        aria-controls={`${ids}-more`}
        aria-label={label}
        title="更多"
        onClick={toggle}
      >
        <Icon name="more" />
      </button>
      {open && (
        <div className="popover lib-more-popover" id={`${ids}-more`} role="group" aria-label={label}>
          {items.map((item) => (
            <button
              key={item.label}
              type="button"
              className="popover-item"
              onClick={() => {
                close();
                item.onSelect(triggerRef.current);
              }}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
