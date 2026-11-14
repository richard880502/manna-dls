import { useEffect, useId, useRef, useState } from "react";
import type { LessonRun, UnitClassRef } from "../../api/types";
import { formatShortDate } from "../../lib/labels";

interface StartClassDialogProps {
  /** The classes that use this lesson's unit (two or more). */
  classes: UnitClassRef[];
  /** This lesson's runs, newest first. */
  runs: LessonRun[];
  starting: boolean;
  onStart: (classId: number) => void;
  onCancel: () => void;
}

/**
 * 「這節課在哪一班上？」 (docs/design/overhaul/start-class.html): the one place a class is asked
 * for. It only opens when a lesson is started from the library and several classes use its unit;
 * from a class the lesson starts straight away. The class most likely meant is already chosen:
 * the one where this lesson is open, otherwise the first one that has not had it yet.
 */
export function StartClassDialog({ classes, runs, starting, onStart, onCancel }: StartClassDialogProps) {
  const ids = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const openIn = (classId: number) => runs.find((r) => r.ended_at === null && r.class_id === classId);
  const lastIn = (classId: number) => runs.find((r) => r.ended_at !== null && r.class_id === classId);
  const [picked, setPicked] = useState<number>(
    () => (classes.find((c) => openIn(c.id)) ?? classes.find((c) => !lastIn(c.id)) ?? classes[0])?.id ?? 0,
  );

  // Focus starts on the chosen class and goes back to where it was when the dialog closes.
  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.querySelector<HTMLInputElement>("input:checked")?.focus();
    return () => before?.focus();
  }, []);

  // Escape cancels; Tab stays inside the dialog while it is open.
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        if (!starting) onCancel();
        return;
      }
      if (event.key !== "Tab") return;
      const stops = [
        ...(dialogRef.current?.querySelectorAll<HTMLElement>("input:checked, button:not(:disabled)") ?? []),
      ];
      const first = stops[0];
      const last = stops[stops.length - 1];
      if (!first || !last) return;
      const inside = dialogRef.current?.contains(document.activeElement) ?? false;
      if (event.shiftKey && (document.activeElement === first || !inside)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !inside)) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [starting, onCancel]);

  return (
    <div className="scrim">
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${ids}-title`}
        aria-describedby={`${ids}-why`}
        ref={dialogRef}
      >
        <h2 id={`${ids}-title`}>這節課在哪一班上？</h2>
        <p className="quiet-note" id={`${ids}-why`}>
          你是從備課庫進來的，所以問一次。從班級進來會直接開始。
        </p>
        <div className="radio-rows" role="radiogroup" aria-labelledby={`${ids}-title`}>
          {classes.map((c) => {
            const open = openIn(c.id);
            const last = lastIn(c.id);
            return (
              <label key={c.id}>
                <input
                  type="radio"
                  name={`${ids}-class`}
                  checked={picked === c.id}
                  disabled={starting}
                  onChange={() => setPicked(c.id)}
                />
                <span>
                  {c.name}
                  <span className="meta">
                    {open
                      ? "這節課正在上，會回到課堂"
                      : last
                        ? `${formatShortDate(last.started_at)} 上過這一節`
                        : "還沒上過這一節"}
                  </span>
                </span>
              </label>
            );
          })}
        </div>
        <div className="actions">
          <button type="button" className="primary-button" disabled={starting} onClick={() => onStart(picked)}>
            {starting ? "開始中…" : "開始上課"}
          </button>
          <button type="button" className="text-link is-plain" disabled={starting} onClick={onCancel}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}
