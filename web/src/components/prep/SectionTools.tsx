import { createContext, useCallback, useContext, useEffect, useId, useState, type ReactNode } from "react";
import { api, errorMessage } from "../../api/client";
import type { Thumbs } from "../../api/types";
import { copyText } from "../../lib/download";
import { ErrorText } from "../common";
import { usePopover } from "../Popover";

/**
 * The tools under one section of a result: 複製, and 有幫助／不合用 (recorded on the AI call that
 * produced it, as a skip decision with a rating). Without an AI call there is nothing to rate.
 */
export function SectionTools({
  name,
  text,
  interactionId,
  children,
  more,
  ratingTarget,
}: {
  /** What the section is called, for the buttons' accessible names. */
  name: string;
  text: string;
  interactionId: number | null;
  /** More tools of the section (a worksheet block: 改這一區、加註記…). */
  children?: ReactNode;
  /**
   * Set: the section shows only `children` and a 「更多…」 menu holding 複製, the rating and `more`
   * (the secondary tools), so a worksheet block does not carry a row of buttons. It draws the
   * items: `MenuItem` buttons close the menu and put focus back on 「更多…」 (copy and the rating
   * keep it open, so 已複製 and the pressed rating stay in view). An item that opens a dialog is a
   * plain `popover-item` button: focus leaving the menu closes it.
   */
  more?: ReactNode;
  /**
   * Which part of a shared AI call the rating is about. Sections of one worksheet step all rate the
   * same call, so `item_index` carries the section's 1-based position on the sheet when it was rated.
   * (`reason_note` stays empty, so it is not mistaken for something the teacher wrote.)
   */
  ratingTarget?: { itemIndex: number };
}) {
  const ids = useId();
  const menu = usePopover();
  const [copied, setCopied] = useState<boolean | null>(null);
  const [thumbs, setThumbs] = useState<Thumbs | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (copied === null) return undefined;
    const timer = setTimeout(() => setCopied(null), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  async function rate(next: Thumbs) {
    if (interactionId === null) return;
    const before = thumbs;
    setThumbs(next);
    setError(null);
    try {
      await api.sendDecision(interactionId, {
        decision: "skip",
        thumbs: next,
        ...(ratingTarget ? { item_index: ratingTarget.itemIndex } : {}),
      });
    } catch (err) {
      setThumbs(before);
      setError(errorMessage(err));
    }
  }

  const inMenu = more !== undefined;
  const itemClass = inMenu ? "popover-item" : "decision-button";
  const { open, toggle, close, rootRef, triggerRef, onBlur } = menu;
  const closeAndFocus = useCallback(() => {
    close();
    triggerRef.current?.focus();
  }, [close, triggerRef]);

  const copy = (
      <button
        type="button"
        className={itemClass}
        aria-label={`複製${name}`}
        onClick={() => void copyText(text).then(setCopied)}
      >
        {copied === true ? "已複製" : copied === false ? "沒有複製成功" : "複製"}
      </button>
  );
  const rating = interactionId !== null && (
    <>
      <button
        type="button"
        className={itemClass}
        aria-label={`${name}有幫助`}
        aria-pressed={thumbs === "up"}
        onClick={() => void rate("up")}
      >
        有幫助
      </button>
      <button
        type="button"
        className={itemClass}
        aria-label={`${name}不合用`}
        aria-pressed={thumbs === "down"}
        onClick={() => void rate("down")}
      >
        不合用
      </button>
    </>
  );

  if (inMenu) {
    return (
      <div className="prep-tools">
        {children}
        <div className="popover-root" ref={rootRef} onBlur={onBlur}>
          <button
            ref={triggerRef}
            type="button"
            className="decision-button"
            aria-expanded={open}
            aria-controls={`${ids}-more`}
            aria-label={`更多（${name}）`}
            onClick={toggle}
          >
            更多…
          </button>
          {/* Always mounted (hidden): an item may own a dialog that outlives the menu. */}
          <div
            className="popover prep-more-popover"
            id={`${ids}-more`}
            role="group"
            aria-label={`${name}的更多工具`}
            hidden={!open}
          >
            <MenuClose.Provider value={closeAndFocus}>
              {copy}
              {rating}
              {more}
            </MenuClose.Provider>
          </div>
        </div>
        <ErrorText message={error} />
      </div>
    );
  }

  return (
    <div className="prep-tools">
      {copy}
      {rating}
      {children}
      <ErrorText message={error} />
    </div>
  );
}

const MenuClose = createContext<() => void>(() => undefined);

/** A button of 「更多…」 that does its job and then closes the menu. */
export function MenuItem({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  const close = useContext(MenuClose);
  return (
    <button
      type="button"
      className="popover-item"
      onClick={() => {
        close();
        onClick();
      }}
    >
      {children}
    </button>
  );
}
