import { useEffect, useId, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type {
  PrepTurn,
  WorksheetState,
  WsAudience,
  WsExportFormat,
  WsJob,
  WsLevel,
  WsSheet,
  WsSheetBlock,
} from "../../api/types";
import { tally } from "../../lib/bloom";
import { saveBlob } from "../../lib/download";
import { prepConfig } from "../../lib/prep";
import { LEVEL_LABEL, projectSheet } from "../../lib/worksheet";
import { ErrorText, Loading } from "../common";
import { BloomDistribution } from "../worksheet/Bloom";
import { ProgressPanel, RunOutcome } from "../worksheet/OptionsView";
import { SheetPreview } from "./SheetPreview";

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
 */
export function WorksheetCard({
  turn,
  refreshKey,
  highlight,
  onLabels,
}: {
  turn: PrepTurn;
  /** Changes when the worksheet got a new version (a revise turn finished). */
  refreshKey: string;
  highlight: ReadonlySet<string>;
  /** Reports which letter each block printed with, for the revise replies. */
  onLabels: (worksheetId: number, labels: Record<string, string>) => void;
}) {
  const ids = useId();
  const ref = turn.worksheet;
  const wsId = ref?.id ?? null;
  const running = turn.status === "running" || (ref?.running ?? false);
  const [jobs, setJobs] = useState<WsJob[]>([]);
  const [state, setState] = useState<WorksheetState | null>(null);
  const [sheet, setSheet] = useState<WsSheet | null>(null);
  const [level, setLevel] = useState<WsLevel | null>(null);
  const [audience, setAudience] = useState<WsAudience>("student");
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState<WsExportFormat | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  // While the worksheet's jobs run: their progress, asked for as often as the session.
  useEffect(() => {
    if (!running || wsId === null) return undefined;
    let alive = true;
    const load = () =>
      api
        .worksheetJobs(wsId)
        .then((res) => alive && setJobs(res.jobs))
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
      .then((ws) => alive && setState(ws))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [running, wsId, refreshKey]);

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
  }, [running, wsId, refreshKey, level, audience, onLabels]);

  if (wsId === null) {
    return turn.status === "running" ? (
      <p className="manna-label prep-working" role="status">
        <span className="star is-working" aria-hidden="true" />
        Manna 正在建立學習單…
      </p>
    ) : null;
  }

  if (running) return <ProgressPanel jobs={jobs} />;

  async function exportAs(format: WsExportFormat) {
    if (wsId === null || !sheet) return;
    setExporting(format);
    setExportError(null);
    try {
      const { blob, filename } = await api.exportWorksheet(wsId, { format, level: sheet.level, audience });
      saveBlob(blob, filename);
    } catch (err) {
      setExportError(errorMessage(err));
    } finally {
      setExporting(null);
    }
  }

  const levels = state?.content.levels ?? [];
  const current = sheet?.level ?? level;
  const shownLevel = current ?? levels[0] ?? "standard";
  const bloomTally = state
    ? tally(projectSheet(state.content, shownLevel, "teacher").blocks.flatMap((b) => b.items.map(({ item }) => item)))
    : null;

  return (
    <div className="prep-worksheet">
      <div className="prep-ws-toolbar">
        <div className="ws-seg" role="radiogroup" aria-label="學生版或教師版">
          {AUDIENCES.map(([a, label]) => (
            <label key={a}>
              <input type="radio" name={`${ids}-aud`} checked={audience === a} onChange={() => setAudience(a)} />
              <span>{label}</span>
            </label>
          ))}
        </div>
        <Link className="text-link" to={`/worksheets/${wsId}`}>
          開啟編輯器
        </Link>
        <Link className="text-link is-plain" to={`/worksheets/${wsId}/check`}>
          詞彙檢查
        </Link>
        <span className="prep-export">
          <button
            type="button"
            className="text-link is-plain"
            disabled={exporting !== null || !sheet}
            onClick={() => void exportAs("docx")}
          >
            {exporting === "docx" ? "匯出中…" : "匯出 DOCX"}
          </button>
          <button
            type="button"
            className="text-link is-plain"
            disabled={exporting !== null || !sheet}
            onClick={() => void exportAs("pdf")}
          >
            {exporting === "pdf" ? "匯出中…" : "匯出 PDF"}
          </button>
        </span>
        {state && (
          <span className="prep-version" aria-label="版本">
            第 {state.version_no} 版，共 {Math.max(state.versions_count, state.version_no)} 版
          </span>
        )}
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
      {sheet && (
        <SheetPreview
          sheet={sheet}
          highlight={highlight}
          interactionFor={(b) => interactionFor(state?.jobs ?? [], b, turn.interaction_id)}
        />
      )}
    </div>
  );
}
