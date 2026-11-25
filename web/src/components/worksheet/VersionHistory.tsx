import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { WsAudience, WsLevel, WsSheet, WsVersionRestored, WsVersionRow } from "../../api/types";
import { formatDateTime } from "../../lib/labels";
import { LEVEL_LABEL } from "../../lib/worksheet";
import { Dialog } from "../Dialog";
import { ErrorText } from "../common";
import { SheetPreview } from "../prep/SheetPreview";

/**
 * 版本紀錄 (docs/API.md「版本紀錄與還原整份學習單」): every version of the whole worksheet, newest
 * first, each readable as a sheet; 「還原成這一版」 writes that version's content as a NEW version, so
 * nothing is lost (the versions after it stay in the list and can be switched back to).
 */

const NO_HIGHLIGHT: ReadonlySet<string> = new Set();
const NO_INTERACTION = () => null;

/** What produced a version, in one short phrase (`oldest`: the first version of the worksheet). */
export function versionLabel(row: Pick<WsVersionRow, "cause" | "source" | "version_no">, oldest: boolean): string {
  const src = row.source ?? null;
  const cause = typeof src?.cause === "string" ? src.cause : row.cause;
  switch (cause) {
    case "created":
      return "建立";
    case "generate":
      return oldest ? "Manna 產生" : "整份重新產生";
    case "regenerate":
      return "重新產生一區";
    case "block_proposal":
    case "block_proposal_pictures":
      return "Manna 修改一區";
    case "prep_turn":
    case "revise":
      return "對話修改";
    case "edit":
      return "手動編輯";
    case "undo":
      return "復原";
    case "vocabulary":
      return "字詞";
    case "pictures":
      return "圖片";
    case "restore":
      return src?.adopted_kind === "worksheet" && typeof src.adopted_version_no === "number"
        ? `還原成第 ${src.adopted_version_no} 版`
        : "採用一區的過往版本";
    default:
      return oldest ? "Manna 產生" : "修改";
  }
}

const AUDIENCES: Array<[WsAudience, string]> = [
  ["student", "學生版"],
  ["teacher", "教師版"],
];

export function VersionHistoryButton({
  worksheetId,
  blockedReason = null,
  beforeOpen,
  onRestored,
  levels = [],
  className = "text-link is-plain",
}: {
  worksheetId: number;
  /** Why 版本紀錄 cannot be used right now (Manna or a chat turn is working); null: it can. */
  blockedReason?: string | null;
  /** Runs before the list is loaded (the editor saves first); a message stops it. */
  beforeOpen?: () => Promise<string | null>;
  /** The worksheet as it is after 還原成這一版. */
  onRestored: (done: WsVersionRestored) => void;
  /** The levels of the worksheet (the preview can switch between them when there are several). */
  levels?: WsLevel[];
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const hint = useId();
  return (
    <>
      <button
        type="button"
        className={className}
        aria-disabled={blockedReason ? true : undefined}
        title={blockedReason ?? undefined}
        aria-describedby={blockedReason ? hint : undefined}
        onClick={() => !blockedReason && setOpen(true)}
      >
        版本紀錄
      </button>
      {blockedReason && (
        <span id={hint} className="sr-only">
          {blockedReason}
        </span>
      )}
      {open && (
        <VersionHistoryDialog
          worksheetId={worksheetId}
          beforeOpen={beforeOpen}
          levels={levels}
          onClose={() => setOpen(false)}
          onRestored={(done) => {
            setOpen(false);
            onRestored(done);
          }}
        />
      )}
    </>
  );
}

function VersionHistoryDialog({
  worksheetId,
  beforeOpen,
  levels,
  onClose,
  onRestored,
}: {
  worksheetId: number;
  beforeOpen?: () => Promise<string | null>;
  levels: WsLevel[];
  onClose: () => void;
  onRestored: (done: WsVersionRestored) => void;
}) {
  const ids = useId();
  const [rows, setRows] = useState<WsVersionRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<number | null>(null);
  const [audience, setAudience] = useState<WsAudience>("student");
  const [level, setLevel] = useState<WsLevel | undefined>(undefined);
  const [sheet, setSheet] = useState<{ no: number; sheet: WsSheet } | null>(null);
  const [sheetError, setSheetError] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const safe = useRef<HTMLButtonElement>(null);
  const beforeRef = useRef(beforeOpen);
  useEffect(() => {
    beforeRef.current = beforeOpen;
  });

  useEffect(() => {
    let alive = true;
    (async () => {
      const failed = (await beforeRef.current?.()) ?? null;
      if (failed) throw new Error(`沒有儲存成功，所以還看不到最新的版本紀錄：${failed}`);
      const list = await api.worksheetVersions(worksheetId);
      if (!alive) return;
      setRows(list);
      setPicked(list[1]?.version_no ?? list[0]?.version_no ?? null);
    })().catch((err: unknown) => {
      if (alive) setError(errorMessage(err));
    });
    return () => {
      alive = false;
    };
  }, [worksheetId]);

  useEffect(() => {
    if (picked === null) return undefined;
    let alive = true;
    api
      .worksheetVersionSheet(worksheetId, picked, audience, level)
      .then((s) => {
        if (!alive) return;
        setSheet({ no: picked, sheet: s });
        setSheetError(null);
      })
      .catch((err: unknown) => {
        if (alive) {
          setSheet(null);
          setSheetError(errorMessage(err));
        }
      });
    return () => {
      alive = false;
    };
  }, [worksheetId, picked, audience, level]);

  useEffect(() => {
    if (asking) safe.current?.focus();
  }, [asking]);

  // Stable: the preview reads the pictures of the version through it.
  const loadImage = useCallback((imageId: number) => api.worksheetImage(worksheetId, imageId), [worksheetId]);

  const latest = rows?.[0]?.version_no ?? null;
  const oldest = rows ? rows[rows.length - 1]?.version_no : null;
  const row = rows?.find((r) => r.version_no === picked) ?? null;
  const isLatest = picked !== null && picked === latest;

  async function restore() {
    if (picked === null || latest === null) return;
    setBusy(true);
    setError(null);
    try {
      onRestored(await api.restoreWorksheetVersion(worksheetId, picked, latest));
    } catch (err) {
      setError(errorMessage(err));
      setAsking(false);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog title="版本紀錄" size="large" onClose={onClose} busy={busy}>
      <div className="ws-history" lang="zh-Hant">
        {!rows && !error && <p className="quiet-note">載入中…</p>}
        <ErrorText message={error} />
        {rows && row && (
          <>
            <p className="quiet-note">
              整份學習單每一次的樣子都留著，新的在前。選一版看內容，按「還原成這一版」會把學習單換成那個樣子，並存成新的一版；現在這一版和其他版本都還在，隨時可以換回來。
            </p>
            <div className="ws-history-body">
              <ol className="ws-history-list" aria-label="學習單的版本">
                {rows.map((r) => (
                  <li key={r.version_no}>
                    <button
                      type="button"
                      className={`ws-history-step${r.version_no === picked ? " is-selected" : ""}`}
                      aria-current={r.version_no === picked ? "true" : undefined}
                      onClick={() => {
                        setPicked(r.version_no);
                        setAsking(false);
                      }}
                    >
                      <span className="ws-history-label">
                        第 {r.version_no} 版・{versionLabel(r, r.version_no === oldest)}
                      </span>
                      <span className="ws-history-time">{formatDateTime(r.created_at)}</span>
                      {r.version_no === latest && <span className="ws-history-now">目前</span>}
                    </button>
                  </li>
                ))}
              </ol>
              <section className="ws-history-preview" aria-label={`第 ${row.version_no} 版的內容`}>
                <div className="ws-tool-row">
                  <div className="ws-seg" role="radiogroup" aria-label="學生版或教師版">
                    {AUDIENCES.map(([a, label]) => (
                      <label key={a}>
                        <input
                          type="radio"
                          name={`${ids}-aud`}
                          checked={audience === a}
                          onChange={() => setAudience(a)}
                        />
                        <span>{label}</span>
                      </label>
                    ))}
                  </div>
                  {levels.length > 1 && (
                    <div className="ws-tabs" role="tablist" aria-label="版本程度">
                      {levels.map((l) => (
                        <button
                          key={l}
                          type="button"
                          role="tab"
                          aria-selected={l === (level ?? sheet?.sheet.level)}
                          className={l === (level ?? sheet?.sheet.level) ? "is-current" : ""}
                          onClick={() => setLevel(l)}
                        >
                          {LEVEL_LABEL[l]}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
                <ErrorText message={sheetError} />
                {!sheet && !sheetError && <p className="quiet-note">載入這一版…</p>}
                {sheet && sheet.no === picked && (
                  <SheetPreview
                    sheet={sheet.sheet}
                    highlight={NO_HIGHLIGHT}
                    interactionFor={NO_INTERACTION}
                    readOnly={{ worksheetId, load: loadImage }}
                  />
                )}
                {asking ? (
                  <div className="ws-history-ask" role="alert">
                    <p>
                      要把整份學習單換成第 {row.version_no} 版的樣子嗎？會存成新的第 {(latest ?? 0) + 1} 版，現在這一版仍留在版本紀錄裡。
                    </p>
                    <div className="button-row">
                      <button ref={safe} type="button" className="secondary-button" disabled={busy} onClick={() => setAsking(false)}>
                        不要還原
                      </button>
                      <button type="button" className="text-link is-plain" disabled={busy} onClick={() => void restore()}>
                        還原
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="button-row ws-history-actions">
                    <button type="button" className="secondary-button" disabled={isLatest || busy} onClick={() => setAsking(true)}>
                      還原成這一版
                    </button>
                    {isLatest && <span className="quiet-note">這就是現在的版本。</span>}
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
