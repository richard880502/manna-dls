// 產生進度 (docs/API.md「產生進度」): the words of the progress panel. The steps themselves come from
// the server (what the run really does, in its order); this file only says them.
import type { ProgressStep, RunProgress } from "../api/types";

export const PROGRESS_WORDS = {
  title: "進度",
  close: "關閉進度",
  allDone: "全部完成",
  /** For a screen reader: what each step's mark means. */
  status: {
    pending: "還沒開始",
    running: "進行中",
    done: "完成",
    failed: "沒有成功",
  } satisfies Record<ProgressStep["status"], string>,
  /** Said where the panel was, once the teacher closed it while the run goes on. */
  stillWorking: "Manna 正在產生…",
  failedFallback: "這一步沒有成功。",
};

export const QUEUE_WORDS = {
  next: "排隊中，下一個就輪到你",
  cancel: "取消排隊",
  confirm: "要取消這個排隊中的工作嗎？",
  yes: "確定取消",
  no: "不取消",
};

/** 「排隊中，前面還有 3 個（約 2 分鐘）」: minutes are rounded up; under a minute is 「不到 1 分鐘」. */
export function queueText(queue: NonNullable<RunProgress["queue"]>): string {
  if (queue.ahead <= 0) return QUEUE_WORDS.next;
  const eta =
    queue.eta_secs === null || queue.eta_secs === undefined
      ? ""
      : queue.eta_secs < 60
        ? "（不到 1 分鐘）"
        : `（約 ${Math.ceil(queue.eta_secs / 60)} 分鐘）`;
  return `排隊中，前面還有 ${queue.ahead} 個${eta}`;
}

/** How long the panel stays, all done, before it folds away. Tests shorten it. */
export const progressConfig = { doneMs: 4000 };

/** One line of the list: 「配圖中（3／6）」. */
export function stepText(step: ProgressStep): string {
  const count = step.count && step.count.total > 0 ? `（${step.count.done}／${step.count.total}）` : "";
  return `${step.label}${count}`;
}

/** The step the run is on (the first one, when two run side by side). */
export function currentStep(progress: RunProgress): ProgressStep | null {
  return progress.current === null ? null : (progress.steps[progress.current] ?? null);
}

/** The first step that failed. */
export function failedStep(progress: RunProgress): ProgressStep | null {
  return progress.steps.find((s) => s.status === "failed") ?? null;
}

/**
 * What the polite live region says: the step being worked on (with its place in the list), that
 * everything is done, or which step failed. It only changes when one of those changes.
 */
export function announcement(progress: RunProgress): string {
  if (progress.status === "done") return PROGRESS_WORDS.allDone;
  if (progress.status === "failed") {
    const failed = failedStep(progress);
    return failed ? `${PROGRESS_WORDS.status.failed}：${failed.label}` : PROGRESS_WORDS.status.failed;
  }
  const step = currentStep(progress);
  if (!step || progress.current === null) return "";
  const note = step.note ? `，${step.note}` : "";
  return `${PROGRESS_WORDS.status.running}：${stepText(step)}${note}（第 ${progress.current + 1} 步，共 ${progress.steps.length} 步）`;
}
