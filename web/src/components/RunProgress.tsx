import { useId, useState, type ReactNode } from "react";
import type { QueueInfo, RunProgress } from "../api/types";
import { api, errorMessage } from "../api/client";
import { PROGRESS_WORDS, QUEUE_WORDS, announcement, queueText, stepText } from "../lib/progress";
import { useConfirmFocus } from "../lib/useConfirmFocus";
import { ErrorText } from "./common";

/** 「排隊中…」 with 取消排隊 (an in-page confirm); `onCancelled` refreshes the page after the job is cancelled. */
export function QueueLine({ queue, onCancelled }: { queue: QueueInfo; onCancelled?: () => void }) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { triggerRef, safeRef } = useConfirmFocus(asking);
  async function cancel() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.cancelAiJob(queue.job_id);
      setAsking(false);
      onCancelled?.();
    } catch (err) {
      setError(errorMessage(err));
      setAsking(false);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="run-progress-queue">
      <p role="status" aria-live="polite">
        {queueText(queue)}
      </p>
      {asking ? (
        <span role="group" aria-label={QUEUE_WORDS.cancel}>
          {QUEUE_WORDS.confirm}
          <button
            type="button"
            className="text-link is-danger"
            aria-disabled={busy || undefined}
            onClick={() => void cancel()}
          >
            {QUEUE_WORDS.yes}
          </button>
          <button type="button" className="text-link is-plain" ref={safeRef} onClick={() => setAsking(false)}>
            {QUEUE_WORDS.no}
          </button>
        </span>
      ) : (
        <button type="button" className="text-link is-plain" ref={triggerRef} onClick={() => setAsking(true)}>
          {QUEUE_WORDS.cancel}
        </button>
      )}
      <ErrorText message={error} />
    </div>
  );
}

/**
 * Whether the progress panel of run `runKey` shows: while the run goes on, and once it is all
 * done (the panel then folds into one fixed line and stays, so nothing below it moves later), or,
 * when a step failed, until the teacher closes it. A run that was already over when the page
 * first saw it shows nothing: the page says how it ended. Closing (×) hides that run only; the
 * next run shows again.
 */
export function useRunPanel(progress: RunProgress | null | undefined, runKey: string | null) {
  const [closed, setClosed] = useState<string | null>(null);
  const [watched, setWatched] = useState<string | null>(null);
  const status = progress?.status ?? null;
  // A run seen running shows how it ends, too (state adjusted while rendering).
  if (status === "running" && runKey !== null && watched !== runKey) setWatched(runKey);
  const seen = runKey !== null && watched === runKey;

  const open = runKey !== null && closed !== runKey;
  const visible = !!progress && open && (status === "running" || seen);
  return {
    visible,
    /** The teacher closed this run's panel. */
    closed: runKey !== null && closed === runKey,
    close: () => setClosed(runKey),
  };
}

interface RunProgressPanelProps {
  progress: RunProgress;
  /** Said under a failed step that carries no message of its own (e.g. the turn's error). */
  error?: string | null;
  onClose: () => void;
  /** Called after a queued job was cancelled, so the page can refresh at once. */
  onCancelled?: () => void;
  className?: string;
  /**
   * Fold into the one 「全部完成」 line once every step is done (default). A page that still waits
   * for what the run makes (the worksheet being loaded) passes false until it is there.
   */
  collapseDone?: boolean;
}

/**
 * 「進度」: the steps of a run, one line each, in the order the server runs them. Finished steps
 * are greyed, the step being worked on has an orange dot (and `aria-current="step"`), the ones
 * to come are plain; a failed step says why. A polite live region says when the step changes,
 * when everything is done, or which step failed. × hides the panel; the run goes on.
 */
export function RunProgressPanel({ progress, error, onClose, onCancelled, className, collapseDone = true }: RunProgressPanelProps) {
  const ids = useId();
  const folded = progress.status === "done" && collapseDone;
  return (
    <section
      className={`run-progress is-${progress.status}${folded ? " is-folded" : ""}${className ? ` ${className}` : ""}`}
      aria-labelledby={`${ids}-name`}
    >
      <div className="run-progress-head">
        <p className="run-progress-title" id={`${ids}-title`}>
          <span id={`${ids}-name`}>{PROGRESS_WORDS.title}</span>
          {folded && <span className="run-progress-summary">：{PROGRESS_WORDS.doneLine(progress.steps.length)}</span>}
        </p>
        <button type="button" className="run-progress-close" aria-label={PROGRESS_WORDS.close} onClick={onClose}>
          <span aria-hidden="true">×</span>
        </button>
      </div>
      {progress.status === "running" && progress.queue && <QueueLine queue={progress.queue} onCancelled={onCancelled} />}
      {!folded && !(progress.status === "running" && progress.queue) && (
      <ol className="run-progress-steps">
        {progress.steps.map((step, i) => (
          <li
            key={step.key}
            className={`run-progress-step is-${step.status}`}
            aria-current={i === progress.current ? "step" : undefined}
          >
            <span className="run-progress-mark" aria-hidden="true" />
            <span className="run-progress-label">
              {stepText(step)}
              <span className="sr-only">（{PROGRESS_WORDS.status[step.status]}）</span>
            </span>
            {step.note && step.status === "running" && <span className="run-progress-note">{step.note}</span>}
            {step.status === "failed" && (
              <span className="run-progress-message">{step.message || error || PROGRESS_WORDS.failedFallback}</span>
            )}
          </li>
        ))}
      </ol>
      )}
      <p className="sr-only" role="status" aria-live="polite">
        {announcement(progress)}
      </p>
    </section>
  );
}

/**
 * Where a page shows a run's progress: the panel while [`useRunPanel`] says so, otherwise
 * `fallback` (the page's own working line, for a run without steps or a closed panel).
 */
export function RunProgressSlot({
  progress,
  runKey,
  error,
  fallback = null,
  className,
  onCancelled,
}: {
  onCancelled?: () => void;
  progress: RunProgress | null | undefined;
  runKey: string | null;
  error?: string | null;
  fallback?: ReactNode;
  className?: string;
}) {
  const panel = useRunPanel(progress, runKey);
  if (panel.visible && progress) {
    return <RunProgressPanel progress={progress} error={error} onClose={panel.close} onCancelled={onCancelled} className={className} />;
  }
  return <>{fallback}</>;
}
