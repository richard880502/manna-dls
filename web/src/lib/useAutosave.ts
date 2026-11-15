import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../api/client";

/** How long after the last edit a save starts. Tests shorten it. */
export const autosaveConfig = { delayMs: 1500 };

export interface Autosave {
  /** There are edits the server does not have yet (also while a save is running or after one failed). */
  dirty: boolean;
  saving: boolean;
  /** Why the last save failed; cleared by the next edit or a successful save. */
  error: string | null;
  /** When the last successful save of this visit happened (as the server reported it). */
  savedAt: string | null;
  /** Call on every edit: schedules a save. */
  touch: () => void;
  /** Saves now if anything is pending. Resolves to an error message, or null when all is saved. */
  flush: () => Promise<string | null>;
}

/**
 * Quiet saving (docs/DESIGN_OVERHAUL.md: no save button). `save` is called a moment after the
 * last edit and must read the current value itself (from a ref); it resolves to the time the
 * server stored it. Edits made while a save is running are saved right after it, and switching
 * to another window or tab saves at once. Nothing is dropped silently: a failed save leaves
 * `dirty` set, so the page's leave guard still asks.
 */
export function useAutosave(save: () => Promise<string | null>): Autosave {
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<string | null>(null);

  const saveRef = useRef(save);
  useEffect(() => {
    saveRef.current = save;
  }, [save]);
  /** Bumped on every edit; a save only clears `dirty` when nothing changed while it ran. */
  const editSeq = useRef(0);
  const savedSeq = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const inFlight = useRef<Promise<string | null> | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  const runOnce = useCallback((): Promise<string | null> => {
    if (inFlight.current) return inFlight.current;
    const seq = editSeq.current;
    setSaving(true);
    const run = (async () => {
      try {
        const at = await saveRef.current();
        savedSeq.current = seq;
        if (alive.current) {
          if (at) setSavedAt(at);
          setError(null);
          if (editSeq.current === seq) setDirty(false);
        }
        return null;
      } catch (err) {
        const message = errorMessage(err);
        if (alive.current) setError(message);
        return message;
      } finally {
        inFlight.current = null;
        if (alive.current) setSaving(false);
      }
    })();
    inFlight.current = run;
    return run;
  }, []);

  const flush = useCallback(async (): Promise<string | null> => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    // A save that is already running may predate the latest edit: wait for it, then save again.
    while (inFlight.current || savedSeq.current !== editSeq.current) {
      const failed = await runOnce();
      if (failed) return failed;
    }
    return null;
  }, [runOnce]);

  // Looking away from the page (another tab, another window) is a good moment to save.
  useEffect(() => {
    const onAway = () => {
      if (savedSeq.current !== editSeq.current) void flush();
    };
    window.addEventListener("blur", onAway);
    document.addEventListener("visibilitychange", onAway);
    return () => {
      window.removeEventListener("blur", onAway);
      document.removeEventListener("visibilitychange", onAway);
    };
  }, [flush]);

  const touch = useCallback(() => {
    editSeq.current += 1;
    setDirty(true);
    setError(null);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      timer.current = null;
      void flush();
    }, autosaveConfig.delayMs);
  }, [flush]);

  return { dirty, saving, error, savedAt, touch, flush };
}
