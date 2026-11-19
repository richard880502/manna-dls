import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type {
  PrepTurn,
  RunProgress,
  WorksheetImages,
  WorksheetState,
  WsAudience,
  WsJob,
  WsLevel,
  WsSheet,
  WsSheetBlock,
} from "../../api/types";
import { tally } from "../../lib/bloom";
import { NOTES_HINT, holdsBlock, placeOfSelection, selectionOnSheet, type SheetSelection } from "../../lib/blockRevise";
import { clipQuote } from "../../lib/materials";
import { ocrPrepLine } from "../../lib/ocr";
import { READING_SAMPLE_LINE, prepConfig } from "../../lib/prep";
import { templateTitle } from "../../lib/templates";
import { LEVEL_LABEL, projectSheet } from "../../lib/worksheet";
import { ErrorText, Loading } from "../common";
import { RunProgressSlot } from "../RunProgress";
import { OcrProgress } from "../material/Ocr";
import { BloomDistribution } from "../worksheet/Bloom";
import { ExportMenu } from "../worksheet/ExportMenu";
import { BlockReviseArea, LostNotes, ReviseByNotesButton, useBlockRevise } from "../worksheet/BlockRevise";
import { ProgressPanel, RunOutcome } from "../worksheet/OptionsView";
import { anyOpen } from "../worksheet/SlotPicture";
import { SheetPreview, sheetBlockName } from "./SheetPreview";

/** How long a block stays highlighted after 採用. */
const FLASH_MS = 4000;

const AUDIENCES: Array<[WsAudience, string]> = [
  ["student", "學生版"],
  ["teacher", "教師版"],
];

/** The step whose AI call produced a block: its own job first, then the step for its kind. */
function interactionFor(jobs: WsJob[], block: WsSheetBlock, fallback: number | null): number | null {
  const own = jobs.find((j) => j.block_id === block.id && j.interaction_id !== null);
  if (own) return own.interaction_id;
  const step = block.kind === "warmup" ? "warmup" : block.kind === "exit_ticket" ? "exit_ticket" : "tasks";
  const byStep = jobs.find((j) => j.block_id === null && j.step === step && j.interaction_id !== null);
  if (byStep) return byStep.interaction_id;
  const template = jobs.find((j) => j.step === "template" && j.interaction_id !== null);
  return template?.interaction_id ?? fallback;
}

/**
 * A worksheet turn (學習單, 閱讀理解題, 分層練習, 評量, or the worksheet of a 教學套件): the job
 * progress while Manna works, then the student version as a read-only preview with the tools that
 * lead on (editor, checks, export), the version tabs when there are several (分層練習 always has
 * three), and the Bloom distribution of the version shown (read-only).
 *
 * A worksheet made after the teacher's own worksheet (a template) first says that Manna is reading
 * that worksheet (a scanned one is recognised by OCR before that, page by page), and once done
 * says whose style it follows, with a link to the template.
 */
export function WorksheetCard({
  turn,
  refreshKey,
  highlight,
  onLabels,
  target = null,
  onTarget,
  proposalsKey,
  stepsShown = false,
}: {
  turn: PrepTurn;
  /** Changes when the worksheet got a new version (a revise turn finished). */
  refreshKey: string;
  highlight: ReadonlySet<string>;
  /** Reports which letter each block printed with, for the revise replies. */
  onLabels: (worksheetId: number, labels: Record<string, string>) => void;
  /** The block of this worksheet the bottom bar will revise (「針對 B 區」). */
  target?: string | null;
  /** 「改這一區」: the bottom bar targets this block. */
  onTarget?: (worksheetId: number, blockId: string, name: string) => void;
  /** Changes when the bottom bar started a block revision here. */
  proposalsKey?: number;
  /** The page shows this turn's 「進度」 panel above the card: no working line of our own. */
  stepsShown?: boolean;
}) {
  const ids = useId();
  const ref = turn.worksheet;
  const wsId = ref?.id ?? null;
  const running = turn.status === "running" || (ref?.running ?? false);
  const [jobs, setJobs] = useState<WsJob[]>([]);
  // The worksheet's own steps (a regeneration started in the editor is not a turn of its own).
  const [wsProgress, setWsProgress] = useState<RunProgress | null>(null);
  const [state, setState] = useState<WorksheetState | null>(null);
  const [sheet, setSheet] = useState<WsSheet | null>(null);
  const [level, setLevel] = useState<WsLevel | null>(null);
  const [audience, setAudience] = useState<WsAudience>("student");
  const [error, setError] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  // 逐區修改與註記: a new version after 採用 or 復原 is loaded again here.
  const [localKey, setLocalKey] = useState(0);
  const [flash, setFlash] = useState<ReadonlySet<string>>(EMPTY);
  const [selection, setSelection] = useState<SheetSelection | "multi" | null>(null);
  const [draft, setDraft] = useState<SheetSelection | null>(null);
  const [undoError, setUndoError] = useState<string | null>(null);
  const sheetRef = useRef<HTMLDivElement>(null);
  const revise = useBlockRevise(running ? null : wsId, {
    reloadKey: proposalsKey,
    onApplied: (done) => {
      setState(done.worksheet);
      setImages(done.worksheet.images ?? null);
      setLocalKey((n) => n + 1);
      setFlash(new Set([done.proposal.block_id]));
    },
  });

  useEffect(() => {
    if (flash.size === 0) return undefined;
    const timer = setTimeout(() => setFlash(EMPTY), FLASH_MS);
    return () => clearTimeout(timer);
  }, [flash]);

  // Follow the browser selection on the sheet: a piece of it can get a note. The offer floats just
  // under the selection (the page's bottom bar would cover a bar at the bottom of the sheet).
  const [barAt, setBarAt] = useState<{ top: number; left: number } | null>(null);
  const follow = useCallback(() => {
    const wrap = sheetRef.current;
    const found = selectionOnSheet(wrap);
    setSelection(found);
    const range = found ? window.getSelection()?.getRangeAt(0) : undefined;
    // (A test DOM has no layout for ranges.)
    if (!wrap || !range || typeof range.getBoundingClientRect !== "function") {
      setBarAt(null);
      return;
    }
    const at = range.getBoundingClientRect();
    const box = wrap.getBoundingClientRect();
    setBarAt({
      top: Math.max(0, at.bottom - box.top + 6),
      left: Math.max(0, Math.min(at.left - box.left, box.width - 320)),
    });
  }, []);
  useEffect(() => {
    document.addEventListener("selectionchange", follow);
    return () => document.removeEventListener("selectionchange", follow);
  }, [follow]);
  // The pictures of the worksheet (newer than the sheet while they are being made), and a
  // counter that reloads the sheet once they change.
  const [images, setImages] = useState<WorksheetImages | null>(null);
  const [sheetKey, setSheetKey] = useState(0);
  const picturesOpen = !running && anyOpen(images);

  // While the worksheet's jobs run: their progress, asked for as often as the session.
  useEffect(() => {
    if (!running || wsId === null) return undefined;
    let alive = true;
    const load = () =>
      api
        .worksheetJobs(wsId)
        .then((res) => {
          if (!alive) return;
          setJobs(res.jobs);
          setWsProgress(res.progress ?? null);
        })
        .catch(() => undefined);
    void load();
    const timer = setInterval(() => void load(), prepConfig.pollMs);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [running, wsId]);

  // Done: the worksheet (its versions and jobs) and the sheet for the chosen version and audience.
  useEffect(() => {
    if (running || wsId === null) return undefined;
    let alive = true;
    api
      .getWorksheet(wsId)
      .then((ws) => {
        if (!alive) return;
        setState(ws);
        setImages(ws.images ?? null);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [running, wsId, refreshKey, localKey]);

  useEffect(() => {
    if (running || wsId === null) return undefined;
    let alive = true;
    api
      .worksheetSheet(wsId, audience, level ?? undefined)
      .then((s) => {
        if (!alive) return;
        setSheet(s);
        setError(null);
        onLabels(wsId, Object.fromEntries(s.blocks.map((b) => [b.id, b.label])));
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [running, wsId, refreshKey, localKey, level, audience, onLabels, sheetKey]);

  // While pictures are being made: ask for them; when the last one is done, reload the sheet.
  useEffect(() => {
    if (!picturesOpen || wsId === null) return undefined;
    let alive = true;
    const timer = setInterval(() => {
      api
        .worksheetImages(wsId)
        .then((next) => {
          if (!alive) return;
          setImages(next);
          if (!anyOpen(next)) setSheetKey((k) => k + 1);
        })
        .catch(() => undefined);
    }, prepConfig.pollMs);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [picturesOpen, wsId]);

  if (wsId === null) {
    if (turn.status === "running" && stepsShown) return null;
    if (turn.status === "running" && turn.phase === "ocr" && turn.ocr) {
      return (
        <div className="prep-working">
          <OcrProgress ocr={turn.ocr} line={ocrPrepLine(turn.ocr)} />
        </div>
      );
    }
    return turn.status === "running" ? (
      <p className="manna-label prep-working" role="status">
        <span className="star is-working" aria-hidden="true" />
        {turn.template?.status === "extracting" ? READING_SAMPLE_LINE : "Manna 正在建立學習單…"}
      </p>
    ) : null;
  }

  // The steps are listed in the turn's 「進度」 panel just above; when that panel is closed (×) the
  // working line stands in. A turn without steps (an older server) shows the job rows as before.
  if (running && stepsShown) return null;
  // Not the turn's own run (e.g. regenerated from the editor): the worksheet's steps, here in the
  // conversation, with the working line once the panel is closed.
  if (running && !turn.progress && wsProgress) {
    return (
      <RunProgressSlot
        progress={wsProgress}
        runKey={`ws-${wsId}-${jobs[0]?.run_no ?? 0}`}
        className="prep-turn-progress"
        fallback={
          <p className="manna-label prep-working" role="status">
            <span className="star is-working" aria-hidden="true" />
            Manna 正在產生這份學習單…
          </p>
        }
      />
    );
  }
  if (running && turn.progress) {
    return (
      <p className="manna-label prep-working" role="status">
        <span className="star is-working" aria-hidden="true" />
        Manna 正在產生這份學習單…
      </p>
    );
  }
  if (running) return <ProgressPanel jobs={jobs} />;

  const levels = state?.content.levels ?? [];
  const current = sheet?.level ?? level;
  const shownLevel = current ?? levels[0] ?? "standard";
  const bloomTally = state
    ? tally(projectSheet(state.content, shownLevel, "teacher").blocks.flatMap((b) => b.items.map(({ item }) => item)))
    : null;

  const template = turn.template ?? null;
  const numbers = new Map((sheet?.blocks ?? []).flatMap((b) => b.items.map((it) => [it.id ?? "", it.no] as const)));
  const numberOf = (id: string) => numbers.get(id);
  const picked = selection && selection !== "multi" ? selection : null;

  async function undo(blockId: string) {
    if (wsId === null) return;
    setUndoError(null);
    try {
      setState(await api.undoWorksheetBlock(wsId, blockId));
      setLocalKey((n) => n + 1);
    } catch (err) {
      setUndoError(errorMessage(err));
    }
  }

  const extras = (b: WsSheetBlock) => {
    const proposal = revise.proposalOf(b.id);
    const held = holdsBlock(proposal);
    const name = sheetBlockName(b);
    // How the bar names it: 「B 區」 (the exit ticket has no letter).
    const region = b.label ? `${b.label} 區` : name;
    return {
      targeted: target === b.id,
      tools: (
        <>
          {onTarget && (
            <button
              type="button"
              className="decision-button"
              aria-label={`改這一區（${name}）`}
              aria-pressed={target === b.id}
              disabled={held || revise.running}
              onClick={() => wsId !== null && onTarget(wsId, b.id, region)}
            >
              改這一區
            </button>
          )}
          <button
            type="button"
            className="decision-button"
            aria-label={`加註記（${name}）`}
            onClick={() => setDraft({ block_id: b.id, field: "block", quote: "" })}
          >
            加註記
          </button>
          <ReviseByNotesButton blockId={b.id} ctl={revise} disabled={held} />
          {state?.undoable.includes(b.id) && !held && (
            <button type="button" className="decision-button" onClick={() => void undo(b.id)}>
              復原上一版
            </button>
          )}
        </>
      ),
      below: (
        <BlockReviseArea
          blockId={b.id}
          ctl={revise}
          numberOf={numberOf}
          draft={draft}
          onDraftDone={() => {
            setDraft(null);
            window.getSelection()?.removeAllRanges();
          }}
          teacher={audience === "teacher"}
        />
      ),
    };
  };

  return (
    <div className="prep-worksheet">
      {template && (
        <p className="prep-ws-template">
          照「<Link to={`/templates/${template.id}`}>{templateTitle(template)}</Link>」的樣式
        </p>
      )}
      <div className="prep-ws-toolbar">
        <div className="ws-seg" role="radiogroup" aria-label="學生版或教師版">
          {AUDIENCES.map(([a, label]) => (
            <label key={a}>
              <input type="radio" name={`${ids}-aud`} checked={audience === a} onChange={() => setAudience(a)} />
              <span>{label}</span>
            </label>
          ))}
        </div>
        {state && (
          <span className="prep-version" aria-label="版本">
            第 {state.version_no} 版，共 {Math.max(state.versions_count, state.version_no)} 版
          </span>
        )}
        <div className="prep-ws-actions">
          <Link className="text-link is-plain" to={`/worksheets/${wsId}`}>
            直接編輯
          </Link>
          <Link className="text-link is-plain" to={`/worksheets/${wsId}/check`}>
            詞彙檢查
          </Link>
          <ExportMenu
            worksheetId={wsId}
            level={shownLevel}
            levelLabel={levels.length > 1 ? LEVEL_LABEL[shownLevel] : null}
            disabled={!sheet}
            onError={setExportError}
          />
        </div>
      </div>

      {levels.length > 1 && (
        <div className="ws-tabs" role="tablist" aria-label="版本程度">
          {levels.map((l) => (
            <button
              key={l}
              type="button"
              role="tab"
              aria-selected={l === current}
              className={l === current ? "is-current" : ""}
              onClick={() => setLevel(l)}
            >
              {LEVEL_LABEL[l]}
            </button>
          ))}
        </div>
      )}

      {bloomTally && bloomTally.total > bloomTally.none && (
        <BloomDistribution tally={bloomTally} label={`Bloom 層次分布（${LEVEL_LABEL[shownLevel]}）`} />
      )}

      <ErrorText message={exportError} />
      <ErrorText message={error} />
      {state && <RunOutcome jobs={state.jobs} />}
      {!sheet && !error && <Loading label="正在載入學習單…" />}
      <ErrorText message={undoError} />
      <LostNotes ctl={revise} />
      {sheet && (
        <div ref={sheetRef} className="prep-sheet-wrap">
          <SheetPreview
            sheet={sheet}
            highlight={flash.size > 0 ? new Set([...highlight, ...flash]) : highlight}
            interactionFor={(b) => interactionFor(state?.jobs ?? [], b, turn.interaction_id)}
            extras={extras}
            pictures={{
              worksheetId: wsId,
              images,
              onImages: (next) => {
                setImages(next);
                setSheetKey((k) => k + 1);
              },
            }}
          />
          {selection && !draft && (
            <div
              className="anno-bar is-floating"
              role="status"
              lang="zh-Hant"
              style={barAt ? { top: `${barAt.top}px`, left: `${barAt.left}px` } : undefined}
            >
              {picked ? (
                <>
                  <span>
                    已選取{placeOfSelection(picked, numberOf)}的「<span lang="en">{clipQuote(picked.quote, 40)}</span>」
                  </span>
                  <button
                    type="button"
                    className="primary-button small-button"
                    // Keep the selection while the button takes the click.
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => setDraft(picked)}
                  >
                    加註記
                  </button>
                </>
              ) : (
                <span>一則註記標在同一區裡的文字。請只選一區裡的文字。</span>
              )}
            </div>
          )}
          <p className="field-hint prep-sheet-hint">選取學習單上的文字就能加註記。{NOTES_HINT}</p>
        </div>
      )}
    </div>
  );
}

const EMPTY: ReadonlySet<string> = new Set();
