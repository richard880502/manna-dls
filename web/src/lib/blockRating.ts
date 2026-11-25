import type { WsBlockKind, WsJob } from "../api/types";

/**
 * The step whose AI call produced a block: its own job first, then the step for its kind, then the
 * template read, then `fallback` (the conversation turn's own call). 有幫助／不合用 is recorded on it.
 */
export function interactionFor(
  jobs: readonly WsJob[],
  block: { id: string; kind: WsBlockKind },
  fallback: number | null,
): number | null {
  const own = jobs.find((j) => j.block_id === block.id && j.interaction_id !== null);
  if (own) return own.interaction_id;
  const step = block.kind === "warmup" ? "warmup" : block.kind === "exit_ticket" ? "exit_ticket" : "tasks";
  const byStep = jobs.find((j) => j.block_id === null && j.step === step && j.interaction_id !== null);
  if (byStep) return byStep.interaction_id;
  const template = jobs.find((j) => j.step === "template" && j.interaction_id !== null);
  return template?.interaction_id ?? fallback;
}
