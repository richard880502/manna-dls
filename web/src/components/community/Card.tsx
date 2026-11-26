import type { CommunityCard } from "../../api/types";
import { Counts } from "./Counts";
import { Cover } from "./Cover";
import { outputLabel } from "./labels";

/**
 * One shared worksheet as a tile: its cover (a button that opens the preview), the title on one
 * line, the owner (as they chose to be shown) and the counts. Save and remix show over the cover
 * on hover or focus, and always on a touch screen.
 */
export function Card({
  card,
  busy,
  onOpen,
  onToggleSave,
  onRemix,
}: {
  card: CommunityCard;
  busy: boolean;
  onOpen: (card: CommunityCard) => void;
  onToggleSave: (card: CommunityCard) => void;
  onRemix: (card: CommunityCard) => void;
}) {
  const kind = outputLabel(card.output);
  return (
    <li className="com-card">
      <div className="com-cover-wrap">
        <button type="button" className="com-cover-btn" aria-label={`預覽：${card.title}`} onClick={() => onOpen(card)}>
          <Cover sheet={card.cover} tint={card.id} />
        </button>
        <div className="com-hover-acts">
          {!card.is_mine && (
            <button
              type="button"
              className="secondary-button"
              aria-pressed={card.saved}
              aria-disabled={busy || undefined}
              onClick={() => {
                if (!busy) onToggleSave(card);
              }}
            >
              收藏
            </button>
          )}
          {!card.is_mine && card.allow_remix && (
            <button
              type="button"
              className="primary-button"
              aria-disabled={busy || undefined}
              onClick={() => {
                if (!busy) onRemix(card);
              }}
            >
              拿來改
            </button>
          )}
        </div>
      </div>
      <h3 className="com-title" title={card.title}>
        {card.title || "未命名的學習單"}
      </h3>
      <p className="com-by">
        <span className="com-owner">{card.is_mine ? `${card.owner_name}（你）` : card.owner_name}</span>
        {kind && <span className="sr-only">，{kind}</span>}
      </p>
      <Counts saves={card.saves} remixes={card.remixes} />
    </li>
  );
}
