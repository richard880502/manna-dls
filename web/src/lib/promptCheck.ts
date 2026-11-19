import type { PromptInfo, PromptVar } from "../api/types";

/** The distinct `{{names}}` of a text, in order (the same reading as the server's `prompts::placeholders`). */
export function placeholders(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(/\{\{([^{}]*?)\}\}/g)) {
    const name = (m[1] ?? "").trim();
    if (name && !out.includes(name)) out.push(name);
  }
  return out;
}

export interface VarCheck {
  /** The code always fills these and the prompt cannot do without them. Saving is refused. */
  missing: PromptVar[];
  /** `{{names}}` the code never fills. Saving is allowed. */
  unknown: string[];
}

/** The same check the server runs on save (a key that is not registered has nothing to check). */
export function checkVariables(info: PromptInfo | undefined, systemText: string, userTemplate: string): VarCheck {
  if (!info) return { missing: [], unknown: [] };
  const used = placeholders(`${systemText}\n${userTemplate}`);
  return {
    missing: info.vars.filter((v) => v.required && !used.includes(v.name)),
    unknown: used.filter((u) => !info.vars.some((v) => v.name === u)),
  };
}

export interface DiffLine {
  kind: "same" | "add" | "del";
  text: string;
}

/** A line diff from `from` to `to` (longest common subsequence): "del" lines only in `from`, "add" only in `to`. */
export function lineDiff(from: string, to: string): DiffLine[] {
  const a = from.split("\n");
  const b = to.split("\n");
  const n = a.length;
  const m = b.length;
  // lcs[i][j]: length of the common subsequence of a[i..] and b[j..]
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i]![j] = a[i] === b[j] ? lcs[i + 1]![j + 1]! + 1 : Math.max(lcs[i + 1]![j]!, lcs[i]![j + 1]!);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ kind: "same", text: a[i]! });
      i++;
      j++;
    } else if (lcs[i + 1]![j]! >= lcs[i]![j + 1]!) {
      out.push({ kind: "del", text: a[i]! });
      i++;
    } else {
      out.push({ kind: "add", text: b[j]! });
      j++;
    }
  }
  while (i < n) out.push({ kind: "del", text: a[i++]! });
  while (j < m) out.push({ kind: "add", text: b[j++]! });
  return out;
}
