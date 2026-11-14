import type { Polarity } from "../../api/types";
import { POLARITY_LABELS, formatClock } from "../../lib/labels";
import type { ViewEvent } from "../../lib/recordQueue";
import { GoodIcon, StuckIcon } from "./RecordIcons";

interface TapPadProps {
  onTap: (polarity: Polarity) => void;
  onUndo: () => void;
  lastEvent: ViewEvent | null;
  disabled?: boolean;
  compact?: boolean;
}

/** The two big bookmark buttons (順利／卡住) and 「復原上一筆」. Never blocked by anything else. */
export function TapPad({ onTap, onUndo, lastEvent, disabled = false, compact = false }: TapPadProps) {
  const iconSize = compact ? 40 : 48;
  return (
    <div className={`tap-area${compact ? " is-compact" : ""}`}>
      <div className="tap-pad">
        <button type="button" className="tap-button tap-good" onClick={() => onTap("good")} disabled={disabled}>
          <GoodIcon size={iconSize} />
          <span className="tap-caption">{POLARITY_LABELS.good}</span>
          {!compact && <small aria-hidden="true">這裡進行得不錯</small>}
        </button>
        <button type="button" className="tap-button tap-stuck" onClick={() => onTap("stuck")} disabled={disabled}>
          <StuckIcon size={iconSize} />
          <span className="tap-caption">{POLARITY_LABELS.stuck}</span>
          {!compact && <small aria-hidden="true">這裡學生有困難</small>}
        </button>
      </div>
      <div className="tap-status">
        <span aria-live="polite">
          {lastEvent
            ? `已記下：${POLARITY_LABELS[lastEvent.polarity]} ${formatClock(lastEvent.device_ts)}`
            : "按一下就記下時間"}
        </span>
        <button type="button" className="ghost-button small-button" onClick={onUndo} disabled={!lastEvent}>
          復原上一筆
        </button>
      </div>
    </div>
  );
}
