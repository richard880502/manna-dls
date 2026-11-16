import { useId } from "react";
import { KEEP_AS_TEXT, LOOKS_LIKE_WORKSHEET, USE_AS_SAMPLE } from "../../lib/create";

/**
 * Under the article slot, when the material put there looks like a worksheet (the server's
 * `looks_like_worksheet`): offer to use it as the sample for 照我的學習單做 instead. It never stops
 * the teacher from generating with it as the article.
 */
export function WorksheetNotice({
  useLabel = USE_AS_SAMPLE,
  disabled,
  onUse,
  onKeep,
}: {
  /** The primary button: 照這份的樣式做 (opens 照我的學習單做), or 把它當範本 on the home quick box. */
  useLabel?: string;
  disabled?: boolean;
  onUse: () => void;
  /** 不用，當成課文 */
  onKeep: () => void;
}) {
  const ids = useId();
  return (
    <div className="notice notice-info worksheet-notice" role="group" aria-labelledby={`${ids}-q`}>
      <p id={`${ids}-q`}>{LOOKS_LIKE_WORKSHEET}</p>
      <div className="button-row">
        <button type="button" className="primary-button small-button" disabled={disabled} onClick={onUse}>
          {useLabel}
        </button>
        <button type="button" className="text-link is-plain" disabled={disabled} onClick={onKeep}>
          {KEEP_AS_TEXT}
        </button>
      </div>
    </div>
  );
}

/** Router state for the mimic page: the sample's id and title, so it needs no extra request. */
export interface MimicState {
  sample?: { id: number; title: string };
}
