import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Icon } from "./Icon";

interface Guard {
  /** A save or upload is running: the dialog cannot be closed until it ends. */
  busy?: boolean;
  /** The teacher typed something: closing asks first. */
  dirty?: boolean;
}

interface DialogControl {
  setGuard: (key: string, guard: Guard | null) => void;
  /** Close the way 取消 / Esc do: refused while busy, asks first when dirty. */
  requestClose: () => void;
}

const DialogContext = createContext<DialogControl | null>(null);

/**
 * For a form inside a <Dialog>: says whether it is busy (saving) or dirty (has typed input), so
 * the dialog does not close by accident. Returns `requestClose` for the form's own 取消 button
 * (null outside a dialog).
 */
export function useDialogGuard(guard: Guard): (() => void) | null {
  const control = useContext(DialogContext);
  const key = useId();
  const setGuard = control?.setGuard;
  const { busy, dirty } = guard;
  useEffect(() => {
    setGuard?.(key, { busy, dirty });
  }, [setGuard, key, busy, dirty]);
  useEffect(() => () => setGuard?.(key, null), [setGuard, key]);
  return control?.requestClose ?? null;
}

/** 取消 for a dialog whose typed state lives in its opener: closes the way Esc does (asks first when dirty). */
export function DialogCancel({ label = "取消", disabled }: { label?: string; disabled?: boolean }) {
  const control = useContext(DialogContext);
  return (
    <button type="button" className="text-link is-plain" disabled={disabled} onClick={control?.requestClose}>
      {label}
    </button>
  );
}

/**
 * The one modal dialog of the app (docs/DESIGN.md: adding, uploading and creating open here).
 * Focus starts on the first field and returns to the opener; Tab stays inside; Escape, × and a
 * click on the scrim close it. While a form inside reports `busy` (useDialogGuard) none of those
 * work; while it reports `dirty` they first ask 「要放棄剛才輸入的內容嗎？」 inside the dialog, so
 * typed input is never lost silently.
 */
export function Dialog({
  title,
  returnFocus,
  onClose,
  onEscape,
  size = "normal",
  initialFocus,
  role = "dialog",
  busy: busyProp = false,
  dirty: dirtyProp = false,
  children,
}: {
  title: string;
  /** Where focus goes when the dialog closes (by default, wherever it was before). Keep it stable. */
  returnFocus?: () => HTMLElement | null;
  onClose: () => void;
  /** Escape, when it should do something else first (e.g. leave a preview inside the dialog). */
  onEscape?: () => void;
  /** `large`: the library picker (up to 1100 px wide). */
  size?: "normal" | "large";
  /** What takes focus when the dialog opens (a selector); by default the first field or button. */
  initialFocus?: string;
  /** `alertdialog` for a confirmation that cannot be undone. */
  role?: "dialog" | "alertdialog";
  /** Same as a form's useDialogGuard, for a dialog whose state lives in its opener. */
  busy?: boolean;
  dirty?: boolean;
  children: ReactNode;
}) {
  const ids = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [guards, setGuards] = useState<Record<string, Guard>>({});
  const setGuard = useCallback((key: string, guard: Guard | null) => {
    setGuards((prev) => {
      const { [key]: _gone, ...rest } = prev;
      return guard ? { ...rest, [key]: guard } : rest;
    });
  }, []);
  const [confirming, setConfirming] = useState(false);
  const reported = Object.values(guards);
  const busy = busyProp || reported.some((g) => g.busy);
  const dirty = dirtyProp || reported.some((g) => g.dirty);

  const live = useRef({ onClose, onEscape, busy, dirty, confirming });
  useEffect(() => {
    live.current = { onClose, onEscape, busy, dirty, confirming };
  }, [onClose, onEscape, busy, dirty, confirming]);
  const focusFirst = useRef(initialFocus);
  const beforeConfirm = useRef<HTMLElement | null>(null);

  const requestClose = useCallback(() => {
    const cur = live.current;
    if (cur.busy || cur.confirming) return;
    if (cur.dirty) {
      beforeConfirm.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      live.current.confirming = true;
      setConfirming(true);
    } else cur.onClose();
  }, []);
  const keepEditing = useCallback(() => {
    live.current.confirming = false;
    setConfirming(false);
    if (beforeConfirm.current?.isConnected) beforeConfirm.current.focus();
    else ref.current?.focus();
  }, []);
  const control = useMemo<DialogControl>(() => ({ setGuard, requestClose }), [setGuard, requestClose]);

  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first =
      (focusFirst.current ? ref.current?.querySelector<HTMLElement>(focusFirst.current) : null) ??
      ref.current?.querySelector<HTMLElement>("input, textarea, select, button:not(.dialog-close)");
    (first ?? ref.current)?.focus();
    return () => {
      const back = returnFocus?.() ?? before;
      if (back?.isConnected) back.focus();
    };
  }, [returnFocus]);

  // The confirm shows: focus goes to the safe choice.
  useEffect(() => {
    if (confirming) ref.current?.querySelector<HTMLElement>(".dialog-confirm [data-keep]")?.focus();
  }, [confirming]);

  useEffect(() => {
    // A press outside the dialog (on the scrim) asks to close it.
    const onPointer = (event: Event) => {
      if (event.target instanceof Node && ref.current && !ref.current.contains(event.target)) requestClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        const cur = live.current;
        if (cur.busy) return;
        if (cur.confirming) keepEditing();
        else if (cur.onEscape) cur.onEscape();
        else requestClose();
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
  }, [requestClose, keepEditing]);

  const large = size === "large";
  return (
    <div className={`scrim${large ? " has-large-dialog" : ""}`}>
      <div
        className={`dialog attach-dialog${large ? " is-large" : ""}`}
        role={role}
        aria-modal="true"
        aria-labelledby={`${ids}-title`}
        aria-busy={busy || undefined}
        ref={ref}
        tabIndex={-1}
      >
        <div className="attach-dialog-head">
          <h2 id={`${ids}-title`}>{title}</h2>
          <button type="button" className="dialog-close" aria-label="關閉" disabled={busy} onClick={requestClose}>
            <Icon name="close" />
          </button>
        </div>
        <DialogContext.Provider value={control}>{children}</DialogContext.Provider>
        {confirming && (
          <div className="dialog-confirm" role="alert">
            <p>要放棄剛才輸入的內容嗎？</p>
            <div className="button-row">
              <button type="button" className="secondary-button" data-keep onClick={keepEditing}>
                繼續編輯
              </button>
              <button type="button" className="text-link is-plain" onClick={() => live.current.onClose()}>
                放棄並關閉
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
