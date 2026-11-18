import { useEffect, useId, useState, type ReactNode } from "react";
import type { RunProgress } from "../api/types";
import { PROGRESS_WORDS, announcement, progressConfig, stepText } from "../lib/progress";

/**
 * Whether the progress panel of run `runKey` shows: while the run goes on, then for a moment once
 * it is all done (it folds away), or, when a step failed, until the teacher closes it. A run that
 * was already over when the page first saw it shows nothing: the page says how it ended. Closing
 * (×) hides that run only; the next run shows again.
 */
export function useRunPanel(progress: RunProgress | null | undefined, runKey: string | null) {
  const [closed, setClosed] = useState<string | null>(null);
  const [watched, setWatched] = useState<string | null>(null);
  const [folded, setFolded] = useState<string | null>(null);
  const status = progress?.status ?? null;
  // A run seen running shows how it ends, too (state adjusted while rendering).
  if (status === "running" && runKey !== null && watched !== runKey) setWatched(runKey);
  const seen = runKey !== null && watched === runKey;

  useEffect(() => {
    if (status !== "done" || !seen || runKey === null) return undefined;
    const timer = setTimeout(() => setFolded(runKey), progressConfig.doneMs);
    return () => clearTimeout(timer);
  }, [status, seen, runKey]);

  const open = runKey !== null && closed !== runKey;
  const visible =
    !!progress && open && (status === "running" || (seen && (status === "failed" || folded !== runKey)));
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
  className?: string;
}

/**
 * 「進度」: the steps of a run, one line each, in the order the server runs them. Finished steps
 * are greyed, the step being worked on has an orange dot (and `aria-current="step"`), the ones
 * to come are plain; a failed step says why. A polite live region says when the step changes,
 * when everything is done, or which step failed. × hides the panel; the run goes on.
 */
export function RunProgressPanel({ progress, error, onClose, className }: RunProgressPanelProps) {
  const ids = useId();
  return (
    <section
      className={`run-progress is-${progress.status}${className ? ` ${className}` : ""}`}
      aria-labelledby={`${ids}-title`}
    >
      <div className="run-progress-head">
        <p className="run-progress-title" id={`${ids}-title`}>
          {PROGRESS_WORDS.title}
        </p>
        <button type="button" className="run-progress-close" aria-label={PROGRESS_WORDS.close} onClick={onClose}>
          <span aria-hidden="true">×</span>
        </button>
      </div>
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
}: {
  progress: RunProgress | null | undefined;
  runKey: string | null;
  error?: string | null;
  fallback?: ReactNode;
  className?: string;
}) {
  const panel = useRunPanel(progress, runKey);
  if (panel.visible && progress) {
    return <RunProgressPanel progress={progress} error={error} onClose={panel.close} className={className} />;
  }
  return <>{fallback}</>;
}
