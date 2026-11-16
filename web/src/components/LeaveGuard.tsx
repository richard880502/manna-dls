import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useBlocker, type Blocker, type BlockerFunction } from "react-router";
import { ErrorText } from "./common";

export interface LeaveGuard {
  blocker: Blocker;
  /** Lets the next navigation through; call it right after the changes were saved. */
  release: () => void;
}

/**
 * Warns before a page with unsaved changes is left (docs/UX_REVIEW.md R2): an in-app navigation
 * is held by react-router's blocker and confirmed in the page (<LeaveConfirm>); closing or
 * reloading the tab gets the browser's own beforeunload prompt.
 */
export function useLeaveGuard(dirty: boolean): LeaveGuard {
  // Read at navigation time: a save that just finished must not have to wait for a re-render.
  // Updated as the change is committed (a layout effect), not some time after the page already
  // shows it: otherwise, for a moment, 「已自動儲存」 is on screen while closing the tab still
  // warns, or an edit is on screen while leaving is not held yet.
  const dirtyRef = useRef(dirty);
  useLayoutEffect(() => {
    dirtyRef.current = dirty;
  }, [dirty]);

  const shouldBlock = useCallback<BlockerFunction>(
    ({ currentLocation, nextLocation }) =>
      dirtyRef.current &&
      (currentLocation.pathname !== nextLocation.pathname || currentLocation.search !== nextLocation.search),
    [],
  );
  const blocker = useBlocker(shouldBlock);

  useEffect(() => {
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      if (!dirtyRef.current) return;
      event.preventDefault();
      // Older Safari and Chrome only prompt when returnValue is set.
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, []);

  const release = useCallback(() => {
    dirtyRef.current = false;
  }, []);

  return { blocker, release };
}

interface LeaveConfirmProps {
  guard: LeaveGuard;
  /** What would be lost, in one sentence. */
  message: string;
  /** Saves everything pending; resolves to an error message, or null when all was saved. */
  onSave?: () => Promise<string | null>;
}

/** The in-page confirm shown while a navigation is held. Nothing is lost unless the teacher says so. */
export function LeaveConfirm({ guard, message, onSave }: LeaveConfirmProps) {
  const ids = useId();
  const { blocker, release } = guard;
  const stayRef = useRef<HTMLButtonElement>(null);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const blocked = blocker.state === "blocked";

  // Focus the safe choice; Escape also stays.
  useEffect(() => {
    if (!blocked) return;
    stayRef.current?.focus();
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && !savingRef.current) {
        setError(null);
        blocker.reset?.();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [blocked, blocker]);

  if (!blocked) return null;

  const stay = () => {
    setError(null);
    blocker.reset?.();
  };

  async function saveAndLeave() {
    if (!onSave) return;
    setSaving(true);
    savingRef.current = true;
    setError(null);
    const failed = await onSave();
    setSaving(false);
    savingRef.current = false;
    if (failed) {
      setError(`沒有儲存成功，所以還沒離開：${failed}`);
      return;
    }
    release();
    blocker.proceed?.();
  }

  return (
    <div className="leave-confirm-layer">
      <section
        className="panel-card leave-confirm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={`${ids}-title`}
        aria-describedby={`${ids}-message`}
      >
        <h3 id={`${ids}-title`}>還有沒儲存的內容</h3>
        <p id={`${ids}-message`}>{message}</p>
        <div className="button-row">
          {onSave && (
            <button type="button" className="primary-button" onClick={() => void saveAndLeave()} disabled={saving}>
              {saving ? "儲存中…" : "儲存後離開"}
            </button>
          )}
          <button type="button" className="secondary-button" ref={stayRef} onClick={stay} disabled={saving}>
            留在這頁
          </button>
          <button type="button" className="ghost-button" onClick={() => blocker.proceed?.()} disabled={saving}>
            不儲存，直接離開
          </button>
        </div>
        <ErrorText message={error} />
      </section>
    </div>
  );
}
