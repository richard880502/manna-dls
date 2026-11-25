import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { PrepTurn, RunProgress, WsJob } from "../../api/types";
import { ocrPrepLine } from "../../lib/ocr";
import { READING_SAMPLE_LINE, prepConfig } from "../../lib/prep";
import { RunProgressSlot } from "../RunProgress";
import { OcrProgress } from "../material/Ocr";
import { ProgressPanel } from "../worksheet/OptionsView";

/**
 * A worksheet turn (學習單, 閱讀理解題, 分層練習, 評量, or the worksheet of a 教學套件) while Manna
 * works on it: the job progress (or the working line, once the page's 「進度」 panel stands in for
 * it). When the work is done the page swaps this for the workspace, in the same frame.
 *
 * A worksheet made after the teacher's own worksheet (a template) first says that Manna is reading
 * that worksheet (a scanned one is recognised by OCR before that, page by page).
 */
export function WorksheetProgress({ turn, stepsShown = false }: { turn: PrepTurn; stepsShown?: boolean }) {
  const ref = turn.worksheet;
  const wsId = ref?.id ?? null;
  const running = turn.status === "running" || (ref?.running ?? false);
  const [jobs, setJobs] = useState<WsJob[]>([]);
  // The worksheet's own steps (a regeneration started in the editor is not a turn of its own).
  const [wsProgress, setWsProgress] = useState<RunProgress | null>(null);

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
  return null;
}
