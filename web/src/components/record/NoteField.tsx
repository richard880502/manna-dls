import { useCallback, useEffect, useId, useRef, useState } from "react";
import { NOTE_IDLE_SECONDS } from "../../lib/recordQueue";

interface NoteFieldProps {
  /** The bookmark the sentence belongs to (the newest tap); null hides the field. */
  target: string | null;
  onCommit: (deviceEventId: string, note: string) => void;
  idleSeconds?: number;
}

/**
 * 「加一句話」(optional). Opens after each tap and collapses by itself after `idleSeconds`
 * without typing; whatever was typed is kept. It never blocks the next tap: a new tap saves the
 * sentence on the previous bookmark and starts fresh for the new one.
 */
export function NoteField({ target, onCommit, idleSeconds = NOTE_IDLE_SECONDS }: NoteFieldProps) {
  const inputId = useId();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [activity, setActivity] = useState(0);
  const [saved, setSaved] = useState("");
  const onCommitRef = useRef(onCommit);
  const targetRef = useRef<string | null>(null);
  const textRef = useRef("");
  const savedRef = useRef("");

  useEffect(() => {
    onCommitRef.current = onCommit;
  }, [onCommit]);

  const commit = useCallback(() => {
    const t = targetRef.current;
    const value = textRef.current.trim();
    if (t && value !== savedRef.current) {
      savedRef.current = value;
      setSaved(value);
      onCommitRef.current(t, value);
    }
  }, []);

  // A new tap: keep what was typed for the previous bookmark, then start fresh.
  useEffect(() => {
    if (target === targetRef.current) return;
    commit();
    targetRef.current = target;
    textRef.current = "";
    savedRef.current = "";
    setSaved("");
    setText("");
    setOpen(target !== null);
    setActivity((n) => n + 1);
  }, [target, commit]);

  // Auto-collapse after a few idle seconds.
  useEffect(() => {
    if (!open) return;
    const id = setTimeout(() => {
      commit();
      setOpen(false);
    }, idleSeconds * 1000);
    return () => clearTimeout(id);
  }, [open, activity, idleSeconds, commit]);

  // Leaving the page keeps the sentence too.
  useEffect(() => commit, [commit]);

  if (!target) return null;

  if (!open) {
    return (
      <div className="note-field is-collapsed" role="group" aria-label="這次書籤的一句話">
        <button
          type="button"
          className="ghost-button small-button"
          onClick={() => {
            textRef.current = saved;
            setText(saved);
            setOpen(true);
            setActivity((n) => n + 1);
          }}
        >
          {saved ? "改這一句" : "加一句話"}
        </button>
        {saved && <span className="note-saved">「{saved}」</span>}
      </div>
    );
  }

  return (
    <div className="note-field" role="group" aria-label="這次書籤的一句話">
      <label className="field-label" htmlFor={inputId}>
        加一句話（可略過）
      </label>
      <div className="live-note-row">
        <input
          id={inputId}
          value={text}
          maxLength={300}
          placeholder="例如：例句太難，改用生活情境"
          onChange={(e) => {
            textRef.current = e.target.value;
            setText(e.target.value);
            setActivity((n) => n + 1);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
              setOpen(false);
            }
          }}
        />
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            commit();
            setOpen(false);
          }}
        >
          收起
        </button>
      </div>
      <p className="field-hint">不要寫學生姓名。{idleSeconds} 秒沒有動作會自動收起，打好的字會保留。</p>
    </div>
  );
}
