import { describe, expect, it } from "vitest";
import { ownNumber, printedNumbers, renumberRefs, withoutOwnNumber } from "../lib/worksheet";

// Mirrors api/src/worksheet/assemble.rs (renumber_refs): the sheet numbers questions through the
// whole worksheet, the model names them by their number in the block.
describe("renumberRefs", () => {
  const printed = new Map([
    [1, 7],
    [2, 8],
  ]);
  it.each([
    ["Questions 1–2, then paragraph 2 and step 1.", "Questions 7–8, then paragraph 2 and step 1."],
    ["items 1, 2 and 3", "items 7, 8 and 3"],
    ["Ask an inquiry 2 times.", "Ask an inquiry 2 times."],
    ["Line 12 of item 1", "Line 12 of item 7"],
    ["Check Q2 against paragraph 2.", "Check Q8 against paragraph 2."],
  ])("%j prints as %j", (text, out) => {
    expect(renumberRefs(text, printed)).toBe(out);
  });

  it("leaves a block whose numbers are already the printed ones", () => {
    expect(renumberRefs("Answer item 2.", new Map([[2, 2]]))).toBe("Answer item 2.");
  });

  it("reads the block's own number from the question's id", () => {
    expect(ownNumber("b2-i3")).toBe(3);
    expect(ownNumber("warmup-i12")).toBe(12);
    expect(ownNumber("b2-1")).toBeNull();
    expect(ownNumber("b2-ix")).toBeNull();
    expect(printedNumbers([{ item: { id: "b2-i1" }, no: 4 }, { item: { id: "new" }, no: 5 }])).toEqual(new Map([[1, 4]]));
  });
});

// Mirrors assemble::without_own_number: template sheets came with 「1. ______」 and printed 「1. 1.」.
describe("withoutOwnNumber", () => {
  it.each([
    ["1. ______(n.) 峰會；高峰會議", "______(n.) 峰會；高峰會議"],
    ["2.______ (n.) 同盟", "______ (n.) 同盟"],
    ["(3) Why did they meet?", "Why did they meet?"],
    ["（4）為什麼？", "為什麼？"],
    ["5) Explain.", "Explain."],
    ["6、說明原因。", "說明原因。"],
    ["  7 . Name the leaders.", "Name the leaders."],
    ["3.5 million people watched.", "3.5 million people watched."],
    ["1.", "1."],
    ["1945. The war ended.", "1945. The war ended."],
    ["Why did they meet?", "Why did they meet?"],
  ])("%j prints as %j", (text, out) => {
    expect(withoutOwnNumber(text)).toBe(out);
  });
});
