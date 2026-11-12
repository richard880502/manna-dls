import { useCallback, useMemo, useState } from "react";
import type { DeviceType, Polarity } from "../api/types";
import { createOp, editOp, removeOp, type RecordSync, type ViewEvent } from "./recordQueue";

export interface Tapper {
  /** Records a bookmark on this device (queued, then synced). Returns its device_event_id. */
  tap: (polarity: Polarity, via?: "console" | "pip") => string;
  /** Undoes the latest bookmark tapped here that is still there. */
  undo: () => void;
  /** Bookmark that 「復原」 would undo. */
  lastEvent: ViewEvent | null;
  /** The newest tap, while it still exists: 「加一句話」 attaches to it. */
  noteTarget: string | null;
  /** Saves a sentence on a bookmark (skipped if it was undone meanwhile or nothing changed). */
  commitNote: (deviceEventId: string, note: string) => void;
}

/** Tap / undo / note for one input surface (console, PiP window or phone remote). */
export function useTapper(sync: RecordSync, events: ViewEvent[], deviceType: DeviceType): Tapper {
  const [taps, setTaps] = useState<string[]>([]);
  const live = useMemo(() => new Set(events.map((e) => e.device_event_id)), [events]);

  const tap = useCallback(
    (polarity: Polarity, via?: "console" | "pip") => {
      const op = createOp(polarity, deviceType, via);
      sync.enqueue(op);
      setTaps((prev) => [...prev, op.device_event_id]);
      return op.device_event_id;
    },
    [sync, deviceType],
  );

  const liveTaps = taps.filter((id) => live.has(id));
  const undoTarget = liveTaps[liveTaps.length - 1] ?? null;
  const newest = taps[taps.length - 1];
  const noteTarget = newest !== undefined && live.has(newest) ? newest : null;
  const lastEvent = undoTarget ? (events.find((e) => e.device_event_id === undoTarget) ?? null) : null;

  const undo = useCallback(() => {
    if (undoTarget) sync.enqueue(removeOp("undo", undoTarget));
  }, [sync, undoTarget]);

  const commitNote = useCallback(
    (deviceEventId: string, note: string) => {
      const current = sync.getSnapshot().events.find((e) => e.device_event_id === deviceEventId);
      if (!current || current.note === note) return;
      sync.enqueue(editOp(deviceEventId, { note }));
    },
    [sync],
  );

  return { tap, undo, lastEvent, noteTarget, commitNote };
}
