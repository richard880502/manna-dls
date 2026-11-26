import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, errorMessage } from "../../api/client";
import type { WsBlock, WsBlockAdopted, WsBlockHistory, WsHistoryPicture, WsHistoryStep } from "../../api/types";
import { formatDateTime } from "../../lib/labels";
import { printedAnswer, printedOption } from "../../lib/worksheet";
import { Dialog } from "../Dialog";
import { ErrorText } from "../common";
import { useWorksheetImage } from "./SlotPicture";
import { Icon } from "../Icon";

/**
 * 修改歷程 (docs/API.md「修改歷程與採用過往的版本」): every earlier state of one block, newest
 * first, each with a preview; ‹ › step through the previews and 「採用這個版本」 puts any of them
 * back as a new version (the later ones stay in the list, so they can be switched back to).
 */

export const WARN_OLDER =
  "這個版本是根據比較早的內容改的，採用後會取代這一區之後的修改（之後的修改仍保留在歷程裡，可以再換回來）。";

const SLOT_STATUS: Record<string, string> = {
  queued: "排隊畫圖中",
  running: "畫圖中",
  failed: "沒畫成",
  skipped: "沒有畫",
  removed: "已拿掉",
  none: "沒有圖",
};

// ---------------------------------------------------------------- the button

export function BlockHistoryButton({
  worksheetId,
  blockId,
  label,
  disabled,
  className,
  beforeOpen,
  focusAfter,
  onAdopted,
}: {
  worksheetId: number;
  blockId: string;
  /**
   * The button sits in a menu that closes (and hides its contents) while the dialog is open: the
   * dialog is drawn on the page itself, and focus goes to the element given when it closes.
   */
  focusAfter?: (button: HTMLButtonElement | null) => HTMLElement | null;
  /** For the screen reader: which block (「修改歷程（區塊 B）」). */
  label: string;
  disabled?: boolean;
  className: string;
  /** Runs before the list is loaded (the editor saves first); a message stops it. */
  beforeOpen?: () => Promise<string | null>;
  /** The step was adopted: the worksheet as it is now. */
  onAdopted: (adopted: WsBlockAdopted) => void;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const back = useCallback(() => focusAfter?.(button.current) ?? button.current, [focusAfter]);
  const dialog = (
    <BlockHistoryDialog
      worksheetId={worksheetId}
      blockId={blockId}
      returnFocus={back}
      beforeOpen={beforeOpen}
      onClose={() => setOpen(false)}
      onAdopted={(done) => {
        setOpen(false);
        onAdopted(done);
      }}
    />
  );
  return (
    <>
      <button
        ref={button}
        type="button"
        className={className}
        aria-label={label}
        disabled={disabled}
        onClick={() => setOpen(true)}
      >
        修改歷程
      </button>
      {open && (focusAfter ? createPortal(dialog, document.body) : dialog)}
    </>
  );
}

// ---------------------------------------------------------------- the dialog

function BlockHistoryDialog({
  worksheetId,
  blockId,
  returnFocus,
  beforeOpen,
  onClose,
  onAdopted,
}: {
  worksheetId: number;
  blockId: string;
  returnFocus: () => HTMLElement | null;
  beforeOpen?: () => Promise<string | null>;
  onClose: () => void;
  onAdopted: (adopted: WsBlockAdopted) => void;
}) {
  const [history, setHistory] = useState<WsBlockHistory | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [at, setAt] = useState(0);
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const beforeRef = useRef(beforeOpen);
  useEffect(() => {
    beforeRef.current = beforeOpen;
  });

  useEffect(() => {
    let alive = true;
    (async () => {
      const failed = (await beforeRef.current?.()) ?? null;
      if (failed) throw new Error(`沒有儲存成功，所以還看不到最新的歷程：${failed}`);
      const next = await api.worksheetBlockHistory(worksheetId, blockId);
      if (alive) setHistory(next);
    })().catch((err: unknown) => {
      if (alive) setError(errorMessage(err));
    });
    return () => {
      alive = false;
    };
  }, [worksheetId, blockId]);

  const steps = history?.steps ?? [];
  const position = Math.min(at, Math.max(steps.length - 1, 0));
  const step: WsHistoryStep | undefined = steps[position];
  const go = (to: number) => {
    setAt(Math.min(Math.max(to, 0), steps.length - 1));
    setAsking(false);
  };

  async function adopt() {
    if (!step) return;
    setBusy(true);
    setError(null);
    try {
      const done = await api.adoptWorksheetBlockVersion(
        worksheetId,
        blockId,
        step.version_no === null && step.proposal_id !== null
          ? { proposal_id: step.proposal_id }
          : { version_no: step.version_no! },
      );
      onAdopted(done);
    } catch (err) {
      setError(errorMessage(err));
      setAsking(false);
    } finally {
      setBusy(false);
    }
  }

  const needsAsk = step ? step.adopt.warn_older || step.adopt.draws_pictures : false;
  const canAdopt = step ? step.adopt.possible && !history?.busy : false;

  return (
    <Dialog title="修改歷程" size="large" returnFocus={returnFocus} onClose={onClose} busy={busy}>
      <div className="ws-history" lang="zh-Hant">
        {!history && !error && <p className="quiet-note">載入中…</p>}
        <ErrorText message={error} />
        {history && steps.length === 0 && <p className="quiet-note">這一區還沒有可以看的歷程。</p>}
        {history && step && (
          <>
            <p className="quiet-note">
              這一區每一次的樣子都留著，新的在前。選一步看內容，按「採用這個版本」就會換成那個樣子；現在這一版和之後的修改都還在歷程裡，隨時可以換回來。
            </p>
            <div className="ws-history-body">
              <ol className="ws-history-list" aria-label="這一區的修改歷程">
                {steps.map((s, i) => (
                  <li key={`${s.version_no ?? "p"}-${s.proposal_id ?? ""}-${s.at}`}>
                    <button
                      type="button"
                      className={`ws-history-step${i === position ? " is-selected" : ""}`}
                      aria-current={i === position ? "step" : undefined}
                      onClick={() => go(i)}
                    >
                      <span className="ws-history-label">{s.label}</span>
                      <span className="ws-history-time">{formatDateTime(s.at)}</span>
                      {s.current && <span className="ws-history-now">現在這一版</span>}
                    </button>
                  </li>
                ))}
              </ol>
              <section className="ws-history-preview" aria-label="這一步的內容">
                <div className="ws-history-nav">
                  <button
                    type="button"
                    className="secondary-button small-button"
                    aria-label="較新的一步"
                    disabled={position <= 0}
                    onClick={() => go(position - 1)}
                  >
                    <Icon name="collapse" className="is-glyph" />
                  </button>
                  <span aria-live="polite">
                    第 {position + 1} 步（共 {steps.length} 步）
                  </span>
                  <button
                    type="button"
                    className="secondary-button small-button"
                    aria-label="較舊的一步"
                    disabled={position >= steps.length - 1}
                    onClick={() => go(position + 1)}
                  >
                    <Icon name="next" className="is-glyph" />
                  </button>
                </div>
                <StepPreview worksheetId={worksheetId} step={step} />
                {asking ? (
                  <div className="ws-history-ask" role="alert">
                    {step.adopt.warn_older && <p>{WARN_OLDER}</p>}
                    {step.adopt.draws_pictures && (
                      <p>
                        這個提案當時沒有採用，所以它的圖片動作還沒做過。採用後會照提案畫圖（會用到 AI 產圖）：
                        {step.picture_actions.map((a) => a.text).join("、")}。
                      </p>
                    )}
                    <div className="button-row">
                      <button type="button" className="secondary-button" disabled={busy} onClick={() => void adopt()}>
                        仍要採用
                      </button>
                      <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setAsking(false)}>
                        先不要
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="button-row ws-history-actions">
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={!canAdopt || busy}
                      onClick={() => (needsAsk ? setAsking(true) : void adopt())}
                    >
                      採用這個版本
                    </button>
                    {step.current && <span className="quiet-note">這一區現在就是這個樣子。</span>}
                    {!step.current && history.busy && <span className="quiet-note">Manna 正在處理，好了之後再採用。</span>}
                    {!step.current && !history.busy && !step.adopt.possible && step.adopt.reason && (
                      <span className="quiet-note">{step.adopt.reason}</span>
                    )}
                  </div>
                )}
              </section>
            </div>
          </>
        )}
      </div>
    </Dialog>
  );
}

// ---------------------------------------------------------------- one step

function Thumb({ worksheetId, picture }: { worksheetId: number; picture: WsHistoryPicture }) {
  const { url, failed } = useWorksheetImage(worksheetId, picture.image_id);
  return (
    <figure className="ws-history-pic">
      {picture.image_id !== null && url ? (
        <img src={url} alt={picture.purpose_zh || "這一步的圖"} />
      ) : (
        <div className="ws-history-pic-empty">
          {picture.image_id !== null && !failed ? "載入中…" : (SLOT_STATUS[picture.status] ?? "沒有圖")}
        </div>
      )}
      {picture.purpose_zh && <figcaption>{picture.purpose_zh}</figcaption>}
    </figure>
  );
}

function BlockLines({ block }: { block: WsBlock }) {
  return (
    <div className="ws-history-block">
      {block.title && <h4 lang="en">{block.title}</h4>}
      {block.instructions && (
        <p className="ws-history-instructions" lang="en">
          {block.instructions}
        </p>
      )}
      {block.items.length > 0 && (
        <ol className="ws-history-items">
          {block.items.map((item) => (
            <li key={item.id}>
              <span lang="en">{item.prompt}</span>
              {item.options.length > 0 && (
                <ul className="prep-sheet-options" lang="en">
                  {item.options.map((o, i) => (
                    <li key={o}>{printedOption(o, i)}</li>
                  ))}
                </ul>
              )}
              {item.answer.trim() && (
                <p className="ws-proposal-answer">
                  參考答案：<span lang="en">{printedAnswer(item.answer_format, item.answer, item.options)}</span>
                </p>
              )}
            </li>
          ))}
        </ol>
      )}
      {!block.title && !block.instructions && block.items.length === 0 && (
        <p className="quiet-note">這一步的區塊沒有文字內容。</p>
      )}
    </div>
  );
}

function StepPreview({ worksheetId, step }: { worksheetId: number; step: WsHistoryStep }) {
  return (
    <div className="ws-history-step-view">
      <h3 className="ws-history-step-title">
        {step.label}
        <span className="ws-history-time">{formatDateTime(step.at)}</span>
      </h3>
      {step.detail && <p className="ws-history-detail">{step.detail}</p>}
      {step.undone && <p className="quiet-note">這次採用後來按了「復原」。</p>}
      {step.adopted_later_version_no !== null && (
        <p className="quiet-note">這個提案後來從歷程採用過（第 {step.adopted_later_version_no} 版）。</p>
      )}
      {step.merged_version_nos.length > 0 && (
        <p className="quiet-note">這一步合併了連續的 {step.merged_version_nos.length + 1} 次手動修改，顯示的是最後的樣子。</p>
      )}
      <BlockLines block={step.block} />
      {step.pictures !== null && step.pictures.length > 0 && (
        <div className="ws-history-pics" role="group" aria-label="這一步的圖片">
          {step.pictures.map((p) => (
            <Thumb key={p.slot_id} worksheetId={worksheetId} picture={p} />
          ))}
        </div>
      )}
      {step.pictures === null && step.version_no !== null && (
        <p className="quiet-note">這一步沒有留下圖片的紀錄，採用後圖片維持現在的樣子。</p>
      )}
      {step.picture_actions.length > 0 && (
        <p className="quiet-note">提案裡的圖片動作（還沒做）：{step.picture_actions.map((a) => a.text).join("、")}</p>
      )}
    </div>
  );
}
