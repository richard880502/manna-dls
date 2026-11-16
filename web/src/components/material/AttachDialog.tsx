import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { UPLOAD_WARNING } from "../../lib/materials";
import { Icon, type IconName } from "../Icon";
import { useFirstOnPage } from "../OncePerPage";
import { LibraryStep, type MaterialChoice, type PickedMaterial } from "./MaterialChooser";

export type AttachWay = "upload" | "library" | "paste";

export const ATTACH_LABEL: Record<AttachWay, string> = {
  upload: "上傳檔案",
  library: "從教材庫選",
  paste: "貼上文字",
};

/** The privacy reminder, said once on the page (an upload form in a dialog may say it instead). */
export function PrivacyReminder() {
  const show = useFirstOnPage();
  if (!show) return null;
  return (
    <p className="material-warning attach-privacy" role="note">
      {UPLOAD_WARNING}
    </p>
  );
}

/** What a choice shows on its chip. */
export function choiceTitle(choice: MaterialChoice): string {
  if (choice.way === "paste") return choice.title.trim() || "貼上的文字";
  return choice.picked?.title.trim() || "未命名教材";
}

/**
 * The attached material inside a prompt card: which slot it fills when there are two (e.g.
 * 「範本：」), its title (and a short fact, e.g. 「3 區」) and ×.
 */
export function MaterialChip({
  label,
  title,
  meta,
  icon = "file",
  disabled,
  onRemove,
}: {
  label?: string;
  title: string;
  meta?: string;
  icon?: IconName;
  disabled?: boolean;
  onRemove: () => void;
}) {
  return (
    <span className="attach-chip" role="status">
      <Icon name={icon} />
      {label && <span className="attach-chip-label">{label}：</span>}
      <span className="attach-chip-title">{title}</span>
      {meta && <span className="attach-chip-meta">· {meta}</span>}
      <button
        type="button"
        className="attach-chip-remove"
        aria-label={`移除${label ?? ""}「${title}」`}
        disabled={disabled}
        onClick={onRemove}
      >
        <Icon name="close" />
      </button>
    </span>
  );
}

/**
 * A modal dialog over the page: Escape or a click on the scrim closes it, focus starts on the
 * first field and goes back where it was afterwards, and Tab stays inside while it is open.
 */
export function Dialog({
  title,
  returnFocus,
  onClose,
  children,
}: {
  title: string;
  /** Where focus goes when the dialog closes (by default, wherever it was before). Keep it stable. */
  returnFocus?: () => HTMLElement | null;
  onClose: () => void;
  children: ReactNode;
}) {
  const ids = useId();
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first = ref.current?.querySelector<HTMLElement>("input, textarea, select, button:not(.dialog-close)");
    (first ?? ref.current)?.focus();
    return () => {
      const back = returnFocus?.() ?? before;
      if (back?.isConnected) back.focus();
    };
  }, [returnFocus]);

  useEffect(() => {
    // A press outside the dialog (on the scrim) closes it.
    const onPointer = (event: Event) => {
      if (event.target instanceof Node && ref.current && !ref.current.contains(event.target)) closeRef.current();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== "Tab" || !ref.current) return;
      const stops = [
        ...ref.current.querySelectorAll<HTMLElement>(
          "a[href], button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled)",
        ),
      ];
      const first = stops[0];
      const last = stops[stops.length - 1];
      if (!first || !last) return;
      const inside = ref.current.contains(document.activeElement);
      if (event.shiftKey && (document.activeElement === first || !inside)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !inside)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("mousedown", onPointer);
    };
  }, []);

  return (
    <div className="scrim">
      <div className="dialog attach-dialog" role="dialog" aria-modal="true" aria-labelledby={`${ids}-title`} ref={ref} tabIndex={-1}>
        <div className="attach-dialog-head">
          <h2 id={`${ids}-title`}>{title}</h2>
          <button type="button" className="dialog-close" aria-label="關閉" onClick={onClose}>
            <Icon name="close" />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

/** 貼上文字: a title (optional) and the text, attached when the teacher says so. */
function PasteForm({
  initial,
  onDone,
  onCancel,
}: {
  initial: { title: string; text: string };
  onDone: (title: string, text: string) => void;
  onCancel: () => void;
}) {
  const ids = useId();
  const [title, setTitle] = useState(initial.title);
  const [text, setText] = useState(initial.text);
  return (
    <form
      className="attach-paste"
      onSubmit={(event) => {
        event.preventDefault();
        if (text.trim()) onDone(title, text);
      }}
    >
      <PrivacyReminder />
      <label className="field-label" htmlFor={`${ids}-title`}>
        教材標題 <span className="optional-label">可以不填</span>
      </label>
      <input id={`${ids}-title`} type="text" maxLength={200} value={title} onChange={(e) => setTitle(e.target.value)} />
      <label className="field-label" htmlFor={`${ids}-text`}>
        貼上教材文字
      </label>
      <textarea
        id={`${ids}-text`}
        className="print-input"
        rows={10}
        lang="en"
        maxLength={50000}
        value={text}
        aria-describedby={`${ids}-hint`}
        onChange={(e) => setText(e.target.value)}
      />
      <p className="field-hint" id={`${ids}-hint`}>
        段落之間空一行。請不要貼上學生姓名或其他個人資料。
      </p>
      <div className="button-row">
        <button type="submit" className="primary-button" disabled={text.trim() === ""}>
          附上
        </button>
        <button type="button" className="text-link is-plain" onClick={onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/**
 * 上傳檔案, 從教材庫選 or 貼上文字, in a dialog: MaterialChooser's steps without its big
 * always-open form. Ends with a material choice, or nothing when closed.
 */
export function AttachDialog({
  way,
  pasted = { title: "", text: "" },
  returnFocus,
  onChoice,
  onClose,
}: {
  way: AttachWay;
  /** What was pasted before, to edit it. */
  pasted?: { title: string; text: string };
  returnFocus?: () => HTMLElement | null;
  onChoice: (choice: MaterialChoice) => void;
  onClose: () => void;
}) {
  const take = (m: PickedMaterial) => onChoice({ way, picked: m, title: "", text: "" });
  return (
    <Dialog title={ATTACH_LABEL[way]} returnFocus={returnFocus} onClose={onClose}>
      {way === "paste" ? (
        <PasteForm
          initial={pasted}
          onCancel={onClose}
          onDone={(title, text) => onChoice({ way: "paste", picked: null, title, text })}
        />
      ) : (
        <div className="attach-step">
          <LibraryStep way={way} onPicked={take} />
        </div>
      )}
    </Dialog>
  );
}
