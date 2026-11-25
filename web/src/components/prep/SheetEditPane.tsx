import { useEffect, useId, useRef, type MutableRefObject } from "react";
import { ErrorText, Loading } from "../common";
import { ResultView } from "../worksheet/ResultView";
import { useWorksheet } from "../worksheet/useWorksheet";
import { formatClock } from "../../lib/worksheet";

/** 預覽 (the read-only print preview) or 編輯 (the real editor) in the prep page's worksheet pane. */
export type SheetMode = "preview" | "edit";

const MODE_KEY = "manna.prepSheetMode";

/** The chosen mode is a per-browser convenience (localStorage may throw: then it lasts this visit). */
export function loadSheetMode(): SheetMode {
  try {
    return window.localStorage.getItem(MODE_KEY) === "edit" ? "edit" : "preview";
  } catch {
    return "preview";
  }
}

export function saveSheetMode(mode: SheetMode): void {
  try {
    window.localStorage.setItem(MODE_KEY, mode);
  } catch {
    // ignore: it only lasts this visit
  }
}

/** Saves what is pending and answers why it could not (null: all saved). Registered by the editor. */
export type SheetFlush = () => Promise<string | null>;

/** The two-state 「預覽｜編輯」 control: a radio group, so the arrow keys move between the two. */
export function SheetModeToggle({ mode, onChange }: { mode: SheetMode; onChange: (mode: SheetMode) => void }) {
  const name = useId();
  return (
    <div className="ws-seg prep-sheet-mode" role="radiogroup" aria-label="學習單的顯示方式">
      {(
        [
          ["preview", "預覽"],
          ["edit", "編輯"],
        ] as const
      ).map(([m, label]) => (
        <label key={m}>
          <input type="radio" name={name} checked={mode === m} onChange={() => onChange(m)} />
          <span>{label}</span>
        </label>
      ))}
    </div>
  );
}

/**
 * The editor of the full editor page (`ResultView`), inside the pane beside the conversation.
 *
 * While a chat turn runs (or the worksheet is being generated) it is read-only with a one-line
 * notice; when that ends, the worksheet is read again so the new version shows. Pending edits are
 * saved when the page asks (`flushRef`, before a message is sent), and when the editor goes away.
 */
export function SheetEditor({
  worksheetId,
  chatBusy,
  versionKey,
  flushRef,
}: {
  worksheetId: number;
  /** A chat turn is running (or being sent) in this conversation. */
  chatBusy: boolean;
  /** Changes when the conversation learned of a new version of this worksheet. */
  versionKey: string;
  flushRef: MutableRefObject<SheetFlush | null>;
}) {
  const ws = useWorksheet(worksheetId);
  const flush = ws.flush;
  useEffect(() => {
    flushRef.current = flush;
    return () => {
      if (flushRef.current === flush) flushRef.current = null;
    };
  }, [flush, flushRef]);
  // Whatever is still pending is saved when the editor goes (back to 預覽, another worksheet).
  const last = useRef(flush);
  useEffect(() => {
    last.current = flush;
  }, [flush]);
  useEffect(() => () => void last.current(), []);

  // A turn ended, or a new version arrived: take it (not the first time: the load just did).
  const seen = useRef({ chatBusy, versionKey });
  const { conflict, refresh, reload } = ws;
  useEffect(() => {
    const before = seen.current;
    seen.current = { chatBusy, versionKey };
    if (chatBusy || (before.chatBusy === chatBusy && before.versionKey === versionKey)) return;
    if (conflict) reload();
    else void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when the turn or the version changes
  }, [chatBusy, versionKey]);

  if (ws.status === "loading" && !ws.state) return <Loading />;
  const state = ws.state;
  if (ws.status === "error" || !state) {
    return (
      <>
        <ErrorText message={ws.loadError ?? "找不到這份學習單。"} />
        <p className="after-rows">
          <button type="button" className="text-link is-plain" onClick={ws.reload}>
            再試一次
          </button>
        </p>
      </>
    );
  }
  if (!state.generated && !state.running) {
    return <p className="quiet-note">這份學習單還沒做好，做好之後就能在這裡編輯。</p>;
  }

  const frozen = chatBusy || state.running;
  const savedLine = ws.saveError
    ? `沒有存到：${ws.saveError}`
    : ws.saving
      ? "儲存中…"
      : ws.dirty
        ? "尚未儲存"
        : `已自動儲存 ${formatClock(ws.savedAt ?? state.saved_at)}`;

  return (
    <div className="prep-sheet-editor">
      <p className={`saved prep-sheet-saved${ws.saveError ? " is-failed" : ""}`} role="status">
        {savedLine}
        {ws.conflict ? (
          <>
            {" "}
            <button type="button" className="text-link is-plain" onClick={ws.reload}>
              重新載入
            </button>
          </>
        ) : (
          ws.saveError && (
            <>
              {" "}
              <button type="button" className="text-link is-plain" onClick={() => void ws.flush()}>
                再試一次
              </button>
            </>
          )
        )}
      </p>
      {frozen && (
        <p className="prep-sheet-frozen" role="status">
          Manna 正在改這份學習單，完成後就能繼續編輯。
        </p>
      )}
      <ResultView ws={ws} embedded frozen={frozen} />
    </div>
  );
}
