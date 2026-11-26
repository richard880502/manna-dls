import { useEffect, useLayoutEffect, useRef, type MutableRefObject } from "react";
import { ErrorText, Loading } from "../common";
import { WorksheetWorkspace, type WorkspaceChat } from "../worksheet/workspace/WorksheetWorkspace";
import { useFlushOnUnmount, useWorksheet } from "../worksheet/useWorksheet";

/** Saves what is pending and answers why it could not (null: all saved). Registered by the pane. */
export type SheetFlush = () => Promise<string | null>;

/**
 * The worksheet beside the prep conversation: the workspace (always editable), with the rules that
 * keep it and the chat from stepping on each other. While a chat turn runs it is read-only; when
 * that ends (or a new version arrives) the worksheet is read again so the new version shows;
 * pending edits are saved when the page asks (`flushRef`, before a message is sent) and when the
 * pane goes away.
 */
export function PrepSheetPane({
  worksheetId,
  chat,
  versionKey,
  flushRef,
  waiting = false,
  onReady,
}: {
  worksheetId: number;
  chat: WorkspaceChat;
  /** Changes when the conversation learned of a new version of this worksheet. */
  versionKey: string;
  flushRef: MutableRefObject<SheetFlush | null>;
  /** The turn's 「進度」 panel is still on screen above: no loading line of our own. */
  waiting?: boolean;
  /** Called once the worksheet is on screen (or could not be loaded), in the same frame. */
  onReady?: () => void;
}) {
  const ws = useWorksheet(worksheetId);
  const flush = ws.flush;
  useEffect(() => {
    flushRef.current = flush;
    return () => {
      if (flushRef.current === flush) flushRef.current = null;
    };
  }, [flush, flushRef]);
  useFlushOnUnmount(flush);

  // A turn ended, or a new version arrived: take it (not the first time: the load just did).
  const seen = useRef({ busy: chat.busy, versionKey });
  const { conflict, refresh, reload } = ws;
  useEffect(() => {
    const before = seen.current;
    seen.current = { busy: chat.busy, versionKey };
    if (chat.busy || (before.busy === chat.busy && before.versionKey === versionKey)) return;
    if (conflict) reload();
    else void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when the turn or the version changes
  }, [chat.busy, versionKey]);

  // Before paint, so the page swaps its panel in the very frame the worksheet appears.
  const ready = ws.status !== "loading";
  const onReadyRef = useRef(onReady);
  useLayoutEffect(() => {
    onReadyRef.current = onReady;
  });
  useLayoutEffect(() => {
    if (ready) onReadyRef.current?.();
  }, [ready]);

  if (ws.status === "loading" && !ws.state) return waiting ? null : <Loading label="正在載入學習單…" />;
  if (ws.status === "error" || !ws.state) {
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
  if (!ws.state.generated && !ws.state.running) {
    return <p className="quiet-note">這份學習單還沒做好，做好之後就能在這裡編輯。</p>;
  }
  return (
    <div className="prep-sheet-wrap is-arriving">
      <WorksheetWorkspace ws={ws} chat={chat} />
    </div>
  );
}
