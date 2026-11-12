import { useEffect, useRef } from "react";
import { api } from "../api/client";
import type { Hardest, ScaffoldAction, ScaffoldCode } from "../api/types";
import { preferenceTrigger } from "./preferences";

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

/**
 * Logs `SC-PREF show` once per mounted component when an aid is opened by default
 * because of the teacher's preference (e.g. trigger "preference:hardest=differentiation").
 */
export function usePreferenceScaffold(active: boolean, value: Hardest): void {
  const logged = useRef(false);
  useEffect(() => {
    if (!active || logged.current) return;
    logged.current = true;
    logScaffold("SC-PREF", "show", { trigger: preferenceTrigger(value) });
  }, [active, value]);
}
