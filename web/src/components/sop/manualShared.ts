import type { SopController } from "./useSop";

export interface StageProps {
  sop: SopController;
  onGoToStage: (step: 1 | 3) => void;
}

export const blank = (s: string | null | undefined) => !s || !s.trim();
export const union = (a: readonly string[] | undefined, b: readonly string[]) => [...(a ?? []), ...b.filter((x) => !(a ?? []).includes(x))];
