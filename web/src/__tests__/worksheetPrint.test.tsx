// What the printed sheet says, the way the exports print it (api/src/worksheet/export.rs): the
// objectives under 「Today I can …」 without their own "I can", the choices lettered (A)–(D) once,
// and a 選擇題 answer key with the letter of its choice. The case tables are the server's
// (api/src/objectives/mod.rs, api/src/worksheet/assemble.rs), so the preview and the DOCX/PDF agree.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

  it("看答案 shows one block's answers on the student sheet only when the teacher projection is given", async () => {
    const two = (audience: "student" | "teacher"): WsSheet => {
      const base = sheet(audience);
      const [first] = base.blocks;
      const second = {
        ...first!,
        id: "b2",
        label: "B",
        title: "Compare",
        items: [
          {
            no: 2,
            answer_format: "簡答題",
            prompt: "What did the neighbors expect?",
            options: [],
            lines: 2,
            ...(audience === "teacher" ? { answer: "Every umbrella would be gone.", evidence: "laughed" } : {}),
          },
        ],
      };
      return { ...base, blocks: [first!, second] };
    };
    const user = userEvent.setup();
    const { rerender } = render(<SheetPreview sheet={two("student")} highlight={new Set()} interactionFor={() => null} />);
    // No teacher projection: no button, no answers.
    expect(screen.queryByRole("button", { name: /看答案/ })).toBeNull();

    rerender(
      <SheetPreview sheet={two("student")} answers={two("teacher")} highlight={new Set()} interactionFor={() => null} />,
    );
    const b = within(screen.getByRole("region", { name: "區塊 B 活動" }));
    expect(document.body.textContent).not.toContain("Every umbrella would be gone.");
    const open = b.getByRole("button", { name: /^看答案/ });
    expect(open).toHaveAttribute("aria-expanded", "false");
    // Block A has an answer too (London): its own button, still closed.
    expect(screen.getAllByRole("button", { name: /^看答案/ })).toHaveLength(2);
    await user.click(open);
    expect(open).toHaveAttribute("aria-expanded", "true");
    expect(open).toHaveTextContent("收起答案");
    expect(b.getByText("Every umbrella would be gone.")).toBeInTheDocument();
    expect(b.getByText("參考答案（只有你看得到）：印出與匯出的學生版不含這些答案。")).toBeInTheDocument();
    // Block A's answer stays hidden.
    expect(screen.queryByText("(B) London", { selector: ".ws-answer-row span" })).toBeNull();
    await user.click(open);
    expect(screen.queryByText("Every umbrella would be gone.")).toBeNull();
    // 複製 copies the student block, never the answers.
    expect(sheetBlockText(two("student").blocks[1]!)).not.toContain("參考答案");
  });

  it("prints the word table once after the last block when the sheet carries end_words", () => {
    const base = sheet("student");
    render(
      <SheetPreview
        sheet={{ ...base, end_words: [{ word: "umbrella", level: 2, definition: "雨傘" }] }}
        highlight={new Set()}
        interactionFor={() => null}
      />,
    );
    const block = screen.getByRole("region", { name: "區塊 A 活動" });
    expect(within(block).queryByRole("table")).toBeNull();
    const end = screen.getByRole("region", { name: "字詞表（學習單最後）" });
    expect(within(end).getByRole("cell", { name: "雨傘" })).toBeInTheDocument();
    // After the last block in the page.
    expect(block.compareDocumentPosition(end) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("prints Follow the steps below. above the numbered steps, in the preview and in 複製", () => {
    const base = sheet("student");
    const block = {
      ...base.blocks[0]!,
      instructions: "The first three stanzas show the speaker examining two roads.",
      steps: ["Underline the clues.", "Work with a partner."],
      steps_lead: "Follow the steps below.",
    };
    render(<SheetPreview sheet={{ ...base, blocks: [block] }} highlight={new Set()} interactionFor={() => null} />);
    const region = screen.getByRole("region", { name: "區塊 A 活動" });
    const lead = within(region).getByText("Follow the steps below.");
    expect(lead.nextElementSibling?.tagName).toBe("OL");
    expect(sheetBlockText(block)).toContain(
      "The first three stanzas show the speaker examining two roads.\nFollow the steps below.\n1. Underline the clues.",
    );
  });
});
