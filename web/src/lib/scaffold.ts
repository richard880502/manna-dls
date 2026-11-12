import { api } from "../api/client";
import type { ScaffoldAction, ScaffoldCode } from "../api/types";

/**
 * Fire-and-forget scaffold event. Never throws, never rejects, never blocks the UI.
 */
export function logScaffold(
  scaffold_code: ScaffoldCode,
  action: ScaffoldAction,
  extra: { trigger?: string; dwell_ms?: number } = {},
): void {
  try {
    const body = { scaffold_code, action, ...extra };
    void api.scaffoldEvent(body).catch(() => undefined);
  } catch {
    // swallow: research logging must not break the teacher's work
  }
}
