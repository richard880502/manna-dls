import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, errorMessage } from "../../api/client";
import type { MemoryCandidate, MemorySuggestion } from "../../api/types";
import { ErrorText, Notice } from "../common";
import { Dialog } from "../Dialog";

/** Manna detects in the background; ask again a few times after something was sent (tests shorten it). */
export const memorySuggestionsConfig = { pollAtMs: [2500, 7000, 18000, 40000] };

type Filter = { prepSessionId?: number; worksheetId?: number };

const SENT_EVENT = "manna:habit-words-sent";

/**
 * The habit suggestions waiting for an answer for one 備課對話 or worksheet. They appear a few
 * seconds after the teacher sent something (the detection runs in the background), so the list
 * is read again at a few moments after `watch` changes or [`habitWordsSent`] is called. A failed
 * read shows nothing.
 */
export function useMemorySuggestions(filter: Filter, watch: string) {
  const [items, setItems] = useState<MemorySuggestion[]>([]);
  const [again, setAgain] = useState(0);
  const { prepSessionId, worksheetId } = filter;

  useEffect(() => {
    let alive = true;
    const load = () =>
      api
        .memorySuggestions({
          ...(prepSessionId ? { prep_session_id: prepSessionId } : {}),
          ...(worksheetId ? { worksheet_id: worksheetId } : {}),
        })
        .then(({ candidates }) => alive && setItems(candidates))
        // a suggestion is optional: no message when it cannot be read
        .catch(() => undefined);
    void load();
    const timers = memorySuggestionsConfig.pollAtMs.map((ms) => setTimeout(() => void load(), ms));
    return () => {
      alive = false;
      timers.forEach(clearTimeout);
    };
  }, [prepSessionId, worksheetId, watch, again]);

  // 「改這一區」 was sent somewhere on the page: its words may hold a habit, ask again.
  useEffect(() => {
    const askAgain = () => setAgain((n) => n + 1);
    window.addEventListener(SENT_EVENT, askAgain);
    return () => window.removeEventListener(SENT_EVENT, askAgain);
  }, []);

  const drop = useCallback((id: number) => setItems((prev) => prev.filter((c) => c.id !== id)), []);
  const reload = useCallback(() => setAgain((n) => n + 1), []);
  return { items, drop, reload };
}

/**
 * Words the teacher typed were just sent where Manna looks for habits without changing what the
 * page watches (a 「改這一區」 request only adds a proposal): the suggestion lists ask again.
 */
export function habitWordsSent() {
  window.dispatchEvent(new Event(SENT_EVENT));
}

type Props = {
  items: MemorySuggestion[];
  onDone: (id: number) => void;
  /** After 記住 (so a page can refresh what it shows). */
  onRemembered?: () => void;
  /**
   * Stays in view while the page scrolls (the worksheet page, where 「改這一區」 is far below the
   * top); the 「已記住」 line then goes away by itself.
   */
  floating?: boolean;
};

const INTRO = "Manna 只會在你按「記住」後才記，可隨時停止使用。";
/** How long 「已記住…」 stays in a floating list. */
export const FLOATING_NOTE_MS = 8000;

/** 「要記住這個習慣嗎？」 with 記住／不用／修改後記住, next to the message it came from. */
export function MemorySuggestionCards({ items, onDone, onRemembered, floating = false }: Props) {
  const shown = useRef(new Set<number>());
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notes, setNotes] = useState<{ id: number; text: string }[]>([]);

  useEffect(() => {
    if (!floating || notes.length === 0) return undefined;
    const timer = setTimeout(() => setNotes([]), FLOATING_NOTE_MS);
    return () => clearTimeout(timer);
  }, [floating, notes]);
  const [editing, setEditing] = useState<MemorySuggestion | null>(null);

  // Logged once per suggestion, when it is on the page.
  useEffect(() => {
    for (const c of items) {
      if (shown.current.has(c.id)) continue;
      shown.current.add(c.id);
      api.memorySuggestionShown(c.id).catch(() => undefined);
    }
  }, [items]);

  async function act(c: MemorySuggestion, run: () => Promise<void>) {
    setBusy(c.id);
    setError(null);
    try {
      await run();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  const remember = (c: MemorySuggestion) =>
    act(c, async () => {
      const saved = await api.confirmMemory(c.id);
      onDone(c.id);
      setNotes((n) => [...n, { id: c.id, text: saved.unchanged ? "這項習慣已經記住了。" : "已記住這項習慣，可以在「備課偏好 → 我的出題習慣」修改或停止使用。" }]);
      onRemembered?.();
    });

  const dismiss = (c: MemorySuggestion) =>
    act(c, async () => {
      await api.cancelMemory(c.id);
      onDone(c.id);
    });

  const edit = (c: MemorySuggestion) =>
    act(c, async () => {
      await api.cancelMemory(c.id, true);
      onDone(c.id);
      setEditing(c);
    });

  const listed = items.length > 0 || notes.length > 0 || error !== null;
  if (!listed && !editing) return null;
  return (
    <>
      {listed && (
    <div className={`memory-suggestions${floating ? " is-floating" : ""}`} role="region" aria-label="Manna 的習慣建議">
      {items.map((c) => (
        <div key={c.id} className="memory-suggestion" role="group" aria-label="習慣建議">
          <p className="memory-suggestion-ask">
            要記住這個習慣嗎？『<strong>{c.statement}</strong>』
          </p>
          {c.replaces_id !== null && c.replaces_statement && (
            <p className="field-hint">會取代：『{c.replaces_statement}』，舊的會保留在歷程。</p>
          )}
          {c.intro && <p className="field-hint">{INTRO}</p>}
          <div className="memory-suggestion-actions">
            <button type="button" className="primary-button small-button" disabled={busy !== null} onClick={() => void remember(c)}>
              記住
            </button>
            <button type="button" className="secondary-button small-button" disabled={busy !== null} onClick={() => void dismiss(c)}>
              不用
            </button>
            <button type="button" className="text-link is-plain" disabled={busy !== null} onClick={() => void edit(c)}>
              修改後記住
            </button>
          </div>
        </div>
      ))}
      {notes.map((n) => (
        <Notice key={n.id} tone="success">
          {n.text}
        </Notice>
      ))}
      <ErrorText message={error} />
    </div>
      )}
      {/* Outside the list: a floating (sticky) list would hold the dialog under the page's bars. */}
      {editing &&
        createPortal(
        <HabitEditDialog
          start={editing}
          onClose={() => setEditing(null)}
          onSaved={(text) => {
            setEditing(null);
            setNotes((n) => [...n, { id: -Date.now(), text }]);
            onRemembered?.();
          }}
        />,
          document.body,
        )}
    </>
  );
}

/** The existing habit dialog: the suggestion's sentence is edited, previewed and confirmed again. */
function HabitEditDialog({
  start,
  onClose,
  onSaved,
}: {
  start: MemorySuggestion;
  onClose: () => void;
  onSaved: (message: string) => void;
}) {
  const [text, setText] = useState(`之後${start.statement}`);
  const [candidate, setCandidate] = useState<MemoryCandidate | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const close = () => {
    if (candidate) api.cancelMemory(candidate.id).catch(() => undefined);
    onClose();
  };

  return (
    <Dialog title="修改後記住" busy={busy} onClose={close}>
      <label className="field-label" htmlFor="memory-edit-draft">
        想記住的長期習慣
      </label>
      <textarea
        id="memory-edit-draft"
        value={text}
        maxLength={1000}
        rows={3}
        disabled={busy}
        onChange={(e) => {
          setText(e.target.value);
          if (candidate) {
            api.cancelMemory(candidate.id).catch(() => undefined);
            setCandidate(null);
          }
        }}
      />
      <button
        type="button"
        className="secondary-button"
        disabled={busy || !text.trim()}
        onClick={() => void run(async () => setCandidate(await api.previewMemory(text)))}
      >
        {busy ? "處理中…" : "整理成習慣（尚未記住）"}
      </button>
      {candidate && (
        <div className="memory-preview" role="region" aria-label="確認出題習慣">
          <h3>確認要記住的內容</h3>
          <p>{candidate.statement}</p>
          {candidate.replaces_statement && <p>將取代：{candidate.replaces_statement} 舊的內容會保留在歷程。</p>}
          <button
            type="button"
            className="primary-button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await api.confirmMemory(candidate.id);
                onSaved("已記住這項習慣。");
              })
            }
          >
            確認並記住
          </button>
        </div>
      )}
      <ErrorText message={error} />
      <button type="button" className="text-link is-plain" disabled={busy} onClick={close}>
        取消
      </button>
    </Dialog>
  );
}

/** A page-level list: all the pending suggestions of one worksheet (or 備課對話). */
export function MemorySuggestions({ filter, watch, floating = false }: { filter: Filter; watch: string; floating?: boolean }) {
  const { items, drop, reload } = useMemorySuggestions(filter, watch);
  return <MemorySuggestionCards items={items} onDone={drop} onRemembered={reload} floating={floating} />;
}
