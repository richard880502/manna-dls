import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
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
  initialVersionNo,
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
  initialVersionNo?: number;
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
          initialVersionNo={initialVersionNo}
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

/** One line of the list: a version, or several consecutive 手動編輯 versions folded together. */
type Entry = { kind: "row"; row: WsVersionRow } | { kind: "group"; key: number; rows: WsVersionRow[] };

const clock = (iso: string) => formatDateTime(iso).slice(-5);

/**
 * Consecutive manual edits (`cause = "edit"`, no snapshot) become one group, so autosave does not
 * bury the versions that matter. The newest version always stays a row of its own (it says 目前),
 * and a lone edit stays a row. `rows` are newest first.
 */
export function groupVersions(rows: WsVersionRow[]): Entry[] {
  const plain = (r: WsVersionRow, i: number) => i > 0 && r.cause === "edit" && !r.snapshot;
  const out: Entry[] = [];
  let run: WsVersionRow[] = [];
  const flushRun = () => {
    if (run.length === 1) out.push({ kind: "row", row: run[0]! });
    else if (run.length > 1) out.push({ kind: "group", key: run[0]!.version_no, rows: run });
    run = [];
  };
  rows.forEach((r, i) => {
    if (plain(r, i)) run.push(r);
    else {
      flushRun();
      out.push({ kind: "row", row: r });
    }
  });
  flushRun();
  return out;
}

export function VersionHistoryDialog({
  worksheetId,
  beforeOpen,
  levels = [],
  initialVersionNo,
  onClose,
  onRestored,
}: {
  worksheetId: number;
  beforeOpen?: () => Promise<string | null>;
  levels?: WsLevel[];
  /** Open with this version selected (a chat card's 「產生了第 N 版」); ignored when it does not exist. */
  initialVersionNo?: number;
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
  const [onlySnapshots, setOnlySnapshots] = useState(false);
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const safe = useRef<HTMLButtonElement>(null);
  const renameInput = useRef<HTMLInputElement>(null);
  const initialRef = useRef(initialVersionNo);
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
      const want = initialRef.current;
      const start =
        want !== undefined && list.some((r) => r.version_no === want)
          ? want
          : (list[1]?.version_no ?? list[0]?.version_no ?? null);
      setPicked(start);
      // The folded group the picked version is in starts open.
      const holder = groupVersions(list).find((e) => e.kind === "group" && e.rows.some((r) => r.version_no === start));
      if (holder?.kind === "group") setOpen(new Set([holder.key]));
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
  const renamingNow = renaming !== null;
  useEffect(() => {
    if (renamingNow) renameInput.current?.focus();
  }, [renamingNow]);

  // Stable: the preview reads the pictures of the version through it.
  const loadImage = useCallback((imageId: number) => api.worksheetImage(worksheetId, imageId), [worksheetId]);

  const latest = rows?.[0]?.version_no ?? null;
  const oldest = rows ? rows[rows.length - 1]?.version_no : null;
  const row = rows?.find((r) => r.version_no === picked) ?? null;
  const isLatest = picked !== null && picked === latest;
  const entries = useMemo<Entry[]>(
    () =>
      !rows
        ? []
        : onlySnapshots
          ? rows.filter((r) => r.snapshot).map((r) => ({ kind: "row", row: r }))
          : groupVersions(rows),
    [rows, onlySnapshots],
  );

  function changeSnapshot(versionNo: number, snapshot: WsVersionRow["snapshot"]) {
    setRows((prev) => prev?.map((r) => (r.version_no === versionNo ? { ...r, snapshot } : r)) ?? prev);
  }

  async function renameSnapshot() {
    const snap = row?.snapshot;
    if (!snap || renaming === null) return;
    setBusy(true);
    setError(null);
    try {
      const done = await api.renameWorksheetSnapshot(worksheetId, snap.id, renaming);
      changeSnapshot(done.version_no, { id: done.id, name: done.name });
      setRenaming(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function removeSnapshot() {
    const snap = row?.snapshot;
    if (!snap || !row) return;
    setBusy(true);
    setError(null);
    try {
      await api.removeWorksheetSnapshot(worksheetId, snap.id);
      changeSnapshot(row.version_no, null);
      setRemoving(false);
    } catch (err) {
      setError(errorMessage(err));
      setRemoving(false);
    } finally {
      setBusy(false);
    }
  }

  function pick(no: number) {
    setPicked(no);
    setAsking(false);
    setRenaming(null);
    setRemoving(false);
  }

  function renderRow(r: WsVersionRow) {
    return (
      <li key={r.version_no}>
        <button
          type="button"
          className={`ws-history-step${r.version_no === picked ? " is-selected" : ""}`}
          aria-current={r.version_no === picked ? "true" : undefined}
          onClick={() => pick(r.version_no)}
        >
          <span className="ws-history-label">
            第 {r.version_no} 版・{versionLabel(r, r.version_no === oldest)}
          </span>
          {r.snapshot && <span className="ws-history-snap">快照：{r.snapshot.name}</span>}
          <span className="ws-history-time">{formatDateTime(r.created_at)}</span>
          {r.version_no === latest && <span className="ws-history-now">目前</span>}
        </button>
      </li>
    );
  }

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
              <div className="ws-history-side">
                <label className="ws-history-filter">
                  <input type="checkbox" checked={onlySnapshots} onChange={(e) => setOnlySnapshots(e.target.checked)} />
                  <span>只看快照</span>
                </label>
                {onlySnapshots && entries.length === 0 && (
                  <p className="quiet-note">還沒有快照。按工具列的「存成快照」，把現在這一版標上名字。</p>
                )}
                <ol className="ws-history-list" aria-label="學習單的版本">
                  {entries.map((e) => {
                    if (e.kind === "row") return renderRow(e.row);
                    const expanded = open.has(e.key);
                    const first = e.rows[e.rows.length - 1]!;
                    return (
                      <li key={`g${e.key}`}>
                        <button
                          type="button"
                          className="ws-history-group"
                          aria-expanded={expanded}
                          onClick={() =>
                            setOpen((prev) => {
                              const next = new Set(prev);
                              if (!next.delete(e.key)) next.add(e.key);
                              return next;
                            })
                          }
                        >
                          手動編輯 ×{e.rows.length}・{clock(first.created_at)}–{clock(e.rows[0]!.created_at)}
                        </button>
                        {expanded && <ol className="ws-history-sublist">{e.rows.map(renderRow)}</ol>}
                      </li>
                    );
                  })}
                </ol>
              </div>
              <section className="ws-history-preview" aria-label={`第 ${row.version_no} 版的內容`}>
                {row.snapshot && (
                  <div className="ws-history-snapshot">
                    {renaming !== null ? (
                      <form
                        className="button-row"
                        onSubmit={(e) => {
                          e.preventDefault();
                          if (!busy) void renameSnapshot();
                        }}
                      >
                        <label className="sr-only" htmlFor={`${ids}-snap`}>
                          快照名稱
                        </label>
                        <input
                          id={`${ids}-snap`}
                          type="text"
                          value={renaming}
                          maxLength={60}
                          ref={renameInput}
                          onChange={(e) => setRenaming(e.target.value)}
                        />
                        <button type="submit" className="secondary-button" disabled={busy}>
                          儲存名稱
                        </button>
                        <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setRenaming(null)}>
                          取消
                        </button>
                      </form>
                    ) : removing ? (
                      <div className="button-row" role="group" aria-label="確認移除快照">
                        <span>只拿掉「{row.snapshot.name}」這個名稱，這一版仍留在版本紀錄裡。</span>
                        <button type="button" className="text-link is-danger" disabled={busy} onClick={() => void removeSnapshot()}>
                          確定移除
                        </button>
                        <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setRemoving(false)}>
                          不移除
                        </button>
                      </div>
                    ) : (
                      <div className="button-row">
                        <strong>快照：{row.snapshot.name}</strong>
                        <button type="button" className="text-link is-plain" onClick={() => setRenaming(row.snapshot!.name)}>
                          改名
                        </button>
                        <button type="button" className="text-link is-plain" onClick={() => setRemoving(true)}>
                          移除快照
                        </button>
                      </div>
                    )}
                  </div>
                )}
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
                    pictures={{ worksheetId, load: loadImage }}
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
