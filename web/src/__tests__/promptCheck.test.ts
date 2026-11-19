import { describe, expect, it } from "vitest";
import type { PromptInfo } from "../api/types";
import { checkVariables, lineDiff, placeholders } from "../lib/promptCheck";

const info: PromptInfo = {
  key: "k",
  name_zh: "測試",
  group: "g",
  description: "d",
  uses_user_template: true,
  vars: [
    { name: "passage_numbered", label: "教材", required: true, auto: false },
    { name: "class_profile", label: "班級", required: false, auto: true },
    { name: "note", label: "備註", required: false, auto: false },
  ],
};

describe("placeholders", () => {
  it("lists distinct names in order, allowing spaces inside the braces", () => {
    expect(placeholders("{{ a }} x {{b}} {{a}} {{}} {{ c")).toEqual(["a", "b"]);
  });
});

describe("checkVariables", () => {
  it("reports a missing required variable and names the code never fills", () => {
    const r = checkVariables(info, "沒有變數 {{oops}}", "{{class_profile}}");
    expect(r.missing.map((v) => v.name)).toEqual(["passage_numbered"]);
    expect(r.unknown).toEqual(["oops"]);
  });

  it("accepts the variable in either text and never blocks on auto variables", () => {
    expect(checkVariables(info, "", "{{passage_numbered}}")).toEqual({ missing: [], unknown: [] });
    expect(checkVariables(info, "{{ passage_numbered }}", "")).toEqual({ missing: [], unknown: [] });
  });

  it("checks nothing for a key that is not registered", () => {
    expect(checkVariables(undefined, "{{x}}", "")).toEqual({ missing: [], unknown: [] });
  });
});

describe("lineDiff", () => {
  it("marks lines only in the first text as del and only in the second as add", () => {
    expect(lineDiff("a\nb\nc", "a\nc\nd")).toEqual([
      { kind: "same", text: "a" },
      { kind: "del", text: "b" },
      { kind: "same", text: "c" },
      { kind: "add", text: "d" },
    ]);
  });

  it("is all same for equal texts", () => {
    expect(lineDiff("x\ny", "x\ny").every((l) => l.kind === "same")).toBe(true);
  });
});
