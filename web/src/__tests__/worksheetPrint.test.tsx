// What the printed sheet says, the way the exports print it (api/src/worksheet/export.rs): the
// objectives under 「Today I can …」 without their own "I can", the choices lettered (A)–(D) once,
// and a 選擇題 answer key with the letter of its choice. The case tables are the server's
// (api/src/objectives/mod.rs, api/src/worksheet/assemble.rs), so the preview and the DOCX/PDF agree.
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { WsSheet } from "../api/types";
import { SheetPreview, sheetBlockText } from "../components/prep/SheetPreview";
import { printedObjective } from "../lib/objectives";
import { optionLabel, printedAnswer, printedOption } from "../lib/worksheet";

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

describe("printedOption", () => {
  it.each([
    ["Paris", 0, "(A) Paris"],
    ["London", 1, "(B) London"],
    ["A. Paris", 0, "(A) Paris"],
    ["(A) Paris", 0, "(A) Paris"],
    ["（B）London", 1, "(B) London"],
    ["( c ) Rome", 2, "(C) Rome"],
    ["b) London", 1, "(B) London"],
    ["D: Tokyo", 3, "(D) Tokyo"],
    ["C．Rome", 2, "(C) Rome"],
    ["C、Rome", 2, "(C) Rome"],
    // Not its own position, no space after the dot, or nothing after the label: kept.
    ["B. London", 0, "(A) B. London"],
    ["A.D. 1066", 0, "(A) A.D. 1066"],
    ["A lot of rain", 0, "(A) A lot of rain"],
    ["A.", 0, "(A) A."],
    ["(A)", 0, "(A) (A)"],
    ["True", 0, "(A) True"],
  ] as const)("%j at %i prints as %j", (text, index, printed) => {
    expect(printedOption(text, index)).toBe(printed);
  });

  it("labels choices (A), (B) …", () => {
    expect(optionLabel(0)).toBe("(A)");
    expect(optionLabel(11)).toBe("(L)");
  });
});

describe("printedAnswer", () => {
  const options = ["Paris", "London", "Rome", "Tokyo"];
  const lettered = ["A. Paris", "B. London", "C. Rome", "D. Tokyo"];
  it.each([
    ["選擇題", "London", options, "(B) London"],
    ["選擇題", " london ", options, "(B) London"],
    ["選擇題", "B. London", options, "(B) London"],
    ["選擇題", "(D) Tokyo", options, "(D) Tokyo"],
    ["選擇題", "Rome", lettered, "(C) Rome"],
    // Not one of the choices (or only a letter): as written.
    ["選擇題", "Berlin", options, "Berlin"],
    ["選擇題", "B", options, "B"],
    // Other formats keep their answer.
    ["是非題", "True", options, "True"],
    ["簡答題", "London", options, "London"],
    [null, "London", options, "London"],
  ] as const)("%s %j prints as %j", (format, answer, opts, printed) => {
    expect(printedAnswer(format, answer, opts)).toBe(printed);
  });
});

describe("the prep preview prints the sheet as the exports do", () => {
  const sheet = (audience: "student" | "teacher"): WsSheet => ({
    title: "Narrative Writing",
    level: "standard",
    audience,
    version_code: null,
    objectives: ["I can identify the key elements of the story.", "I can predict the main idea of a paragraph."],
    blocks: [
      {
        id: "b1",
        kind: "activity",
        label: "A",
        title: "Find the Facts",
        instructions: "",
        steps: [],
        vocabulary: [],
        supports: [],
        items: [
          {
            no: 1,
            answer_format: "選擇題",
            prompt: "Where did the story begin?",
            options: ["A. Paris", "London", "Rome", "Tokyo"],
            lines: 0,
            ...(audience === "teacher" ? { answer: "London", evidence: "" } : {}),
          },
        ],
      },
    ],
  });

  it("drops the objectives' own I can and letters the choices once", () => {
    render(<SheetPreview sheet={sheet("student")} highlight={new Set()} interactionFor={() => null} />);
    const goals = screen.getByRole("region", { name: "學習目標" });
    expect(within(goals).getByRole("heading", { name: "Today I can …" })).toBeInTheDocument();
    expect(within(goals).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "identify the key elements of the story.",
      "predict the main idea of a paragraph.",
    ]);
    const block = screen.getByRole("region", { name: "區塊 A 活動" });
    for (const choice of ["(A) Paris", "(B) London", "(C) Rome", "(D) Tokyo"])
      expect(within(block).getByText(choice)).toBeInTheDocument();
    expect(block.textContent).not.toContain("(A) A. Paris");
    expect(block.textContent).not.toContain("參考答案");
  });

  it("names the letter of a choice answer in the teacher version and in 複製", () => {
    render(<SheetPreview sheet={sheet("teacher")} highlight={new Set()} interactionFor={() => null} />);
    const block = screen.getByRole("region", { name: "區塊 A 活動" });
    expect(within(block).getByText("(B) London", { selector: ".ws-answer-row span" })).toBeInTheDocument();
    const text = sheetBlockText(sheet("teacher").blocks[0]!);
    expect(text).toContain("   (A) Paris\n   (B) London\n   (C) Rome\n   (D) Tokyo");
    expect(text).toContain("參考答案：(B) London");
  });
});
