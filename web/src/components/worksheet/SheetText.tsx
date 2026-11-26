import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { QUOTE_MAX, type SheetSelection } from "../../lib/blockRevise";
import { snapToWords } from "../../lib/materials";

/** Where a text lives on the sheet, so a selection in it can become a note. */
export interface NoteTarget {
  block_id: string;
  field: SheetSelection["field"];
  item_id?: string;
  /** False for texts whose offsets the note cannot keep (options, answers…): the note is on the question with the quote. */
  offsets?: boolean;
  onNote: (selection: SheetSelection) => void;
}

interface Offer {
  start: number;
  end: number;
  x: number;
  y: number;
  /** Hangs below the pointer (a mouse selection) rather than above the field (a keyboard one). */
  pointer: boolean;
}

/** A text field that reads as text on the sheet and grows with what is typed. */
export function SheetText({
  label,
  value,
  onChange,
  disabled,
  placeholder,
  className = "",
  lang = "en",
  note,
}: {
  label: string;
  value: string;
  onChange: (next: string) => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
  lang?: string;
  note?: NoteTarget;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const pressed = useRef(false);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const [offer, setOffer] = useState<Offer | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !el.scrollHeight) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);

  const hide = () => setOffer(null);
  const active = note && !disabled;

  // The selection now in the field: offered while it is a real one, and the field is editable.
  const look = () => {
    const el = ref.current;
    if (!el || !active || pressed.current) return;
    const from = el.selectionStart ?? 0;
    const to = el.selectionEnd ?? 0;
    if (from === to) return hide();
    const snapped = note.offsets === false ? { start: from, end: to } : snapToWords(el.value, { start: from, end: to });
    if (el.value.slice(snapped.start, snapped.end).trim() === "") return hide();
    const rect = el.getBoundingClientRect();
    const at = pointer.current ?? { x: rect.right - 8, y: rect.top };
    setOffer({ start: snapped.start, end: snapped.end, x: at.x, y: at.y, pointer: pointer.current !== null });
  };

  // A scroll of any pane moves the field away from where the button hangs.
  useEffect(() => {
    if (!offer) return;
    window.addEventListener("scroll", hide, true);
    return () => window.removeEventListener("scroll", hide, true);
  }, [offer]);

  const offerNote = () => {
    if (!offer || !note) return;
    const el = ref.current;
    const text = (el?.value ?? value).slice(offer.start, offer.end).slice(0, QUOTE_MAX);
    const withOffsets = note.offsets !== false && (note.field === "title" || note.field === "instructions" || note.field === "item");
    note.onNote({
      block_id: note.block_id,
      field: note.field,
      ...(note.item_id ? { item_id: note.item_id } : {}),
      ...(withOffsets ? { start: offer.start, end: offer.start + text.length } : {}),
      quote: text,
    });
    hide();
  };

  const shown = active ? offer : null;
  const left = offer ? Math.max(8, Math.min(offer.x, window.innerWidth - 120)) : 0;
  const top = offer ? Math.max(8, Math.min(offer.y + (offer.pointer ? 14 : -34), window.innerHeight - 40)) : 0;
  return (
    <Fragment>
      <textarea
        ref={ref}
        className={`ws-text ${className}`}
        rows={1}
        lang={lang}
        aria-label={label}
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        onMouseDown={
          active
            ? () => {
                pressed.current = true;
                pointer.current = null;
                hide();
                const done = (e: MouseEvent) => {
                  window.removeEventListener("mouseup", done, true);
                  pressed.current = false;
                  pointer.current = { x: e.clientX, y: e.clientY };
                  window.setTimeout(look, 0);
                };
                window.addEventListener("mouseup", done, true);
              }
            : undefined
        }
        onSelect={active ? look : undefined}
        onKeyDown={
          active
            ? (e) => {
                if (e.key === "Escape") hide();
                else if (e.key !== "Tab") pointer.current = null;
              }
            : undefined
        }
        onBlur={(e) => {
          if (e.relatedTarget !== button.current) hide();
        }}
      />
      {shown && (
        <button
          ref={button}
          type="button"
          className="ws-note-pop"
          lang="zh-Hant"
          style={{ left, top }}
          aria-label="為選取的文字加註記"
          // The field keeps its focus and selection while the button is pressed.
          onMouseDown={(e) => e.preventDefault()}
          onClick={offerNote}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              hide();
              ref.current?.focus();
            }
          }}
          onBlur={(e) => {
            if (e.relatedTarget !== ref.current) hide();
          }}
        >
          加註記
        </button>
      )}
    </Fragment>
  );
}
