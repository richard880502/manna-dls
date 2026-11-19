// What the printed sheet says, the way the exports print it (api/src/worksheet/export.rs): the
// objectives under 「Today I can …」 without their own "I can". The case table is the server's
// (api/src/objectives/mod.rs), so the preview and the DOCX/PDF agree.
import { describe, expect, it } from "vitest";
import { printedObjective } from "../lib/objectives";

describe("printedObjective", () => {
  it.each([
    ["I can identify the key elements of the story.", "identify the key elements of the story."],
    ["i CAN predict the main idea.", "predict the main idea."],
    ["I can: explain why.", "explain why."],
    ["I can : explain why.", "explain why."],
    ["I can:explain why.", "explain why."],
    ["  I   can   compare two texts.", "compare two texts."],
    ["I can：說明原因", "說明原因"],
    // Not "I can" + something to print: unchanged.
    ["Identify the key elements.", "Identify the key elements."],
    ["I can't stop reading.", "I can't stop reading."],
    ["I cannot wait.", "I cannot wait."],
    ["I canvas the room.", "I canvas the room."],
    ["I can", "I can"],
    ["I can:", "I can:"],
    ["I can  ", "I can  "],
    ["We can compare.", "We can compare."],
    ["", ""],
  ])("%j prints as %j", (line, printed) => {
    expect(printedObjective(line)).toBe(printed);
  });
});
