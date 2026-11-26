// 表格整理 tables name what they compare: headings (the things compared) and row labels (what each row
// asks for, a first column). The editor reads the headings a question names in its own words the way
// the server does (api/src/worksheet/table.rs), never shows 「Column 1」 as a heading, warns about blank
// headings and edits the row labels. An old table without labels prints as before.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import type { WsItem, WsSheet } from "../api/types";
import { SheetPreview } from "../components/prep/SheetPreview";
import { WritingArea } from "../components/worksheet/ItemAnswer";
import {
  columnsNamedInPrompt,
  isPlaceholderHeading,
  newItem,
  tableOf,
  withAnswerFormat,
  withoutPlaceholders,
} from "../lib/worksheet";

const NO_PICTURES = { worksheetId: 1, load: () => Promise.reject(new Error("no pictures")) };

// The same cases as api/src/worksheet/table.rs.
const PI_PROMPT =
  "Complete a T-chart with two columns: Unconventional Path as a Positive Choice and Speaker Justifying His Decision. For each interpretation, record its claim and one supporting detail.";

describe("headings and row labels: the rules the editor shares with the server", () => {
  it("knows a placeholder heading", () => {
    for (const p of ["", " ", "Column 1", "column2", "Col #3", "Header", "Row 1", "Heading 2", "欄 1", "...", "N/A"])
      expect(isPlaceholderHeading(p), p).toBe(true);
    for (const p of ["Claim", "Who?", "Cause", "Unconventional Path", "Item 1 of the poem", "Column of smoke"])
      expect(isPlaceholderHeading(p), p).toBe(false);
  });

  it("reads the headings a question names in its own words", () => {
    expect(columnsNamedInPrompt(PI_PROMPT)).toEqual([
      "Unconventional Path as a Positive Choice",
      "Speaker Justifying His Decision",
    ]);
    expect(columnsNamedInPrompt("Fill in a table with three columns: Who, What happened, and Why. (Question 2)")).toEqual([
      "Who",
      "What happened",
      "Why",
    ]);
    expect(columnsNamedInPrompt("Complete the chart with two columns labeled Pros / Cons.")).toEqual(["Pros", "Cons"]);
    // A stated number that does not match, or no list at all.
    expect(columnsNamedInPrompt("Make a chart with three columns: Pros and Cons.")).toBeNull();
    expect(columnsNamedInPrompt("Complete the table about the speaker.")).toBeNull();
  });

  it("never shows a placeholder heading: the question's own, else empty", () => {
    expect(withoutPlaceholders(["Column 1", "Column 2"], PI_PROMPT)).toEqual([
      "Unconventional Path as a Positive Choice",
      "Speaker Justifying His Decision",
    ]);
    expect(withoutPlaceholders(["Who", "Column 2", ""], "Complete the table.")).toEqual(["Who", "", ""]);
    expect(withoutPlaceholders(["", ""], PI_PROMPT)).toEqual(["", ""]);
    const old = { prompt: PI_PROMPT, table: { columns: ["Column 1", "Column 2"], rows: 3 } };
    expect(tableOf(old).columns[1]).toBe("Speaker Justifying His Decision");
    expect(tableOf({ prompt: PI_PROMPT, table: null }).columns[0]).toBe("Unconventional Path as a Positive Choice");
  });
});

/** The area under a question, with its state, as the editor holds it. */
function Area({ start, log }: { start: WsItem; log: (it: WsItem) => void }) {
  const [item, setItem] = useState(start);
  return (
    <WritingArea
      item={item}
      no={1}
      lines={0}
      disabled={false}
      onChange={(change) =>
        setItem((it) => {
          const next = change(it);
          log(next);
          return next;
        })
      }
    />
  );
}

function tableItem(prompt: string, table?: WsItem["table"]): WsItem {
  const base = withAnswerFormat({ ...newItem(["standard"]), prompt }, "表格整理");
  return table === undefined ? base : { ...base, table };
}

describe("the table editor", () => {
  it("asks for headings that are blank and for row labels on a two-column table", () => {
    render(<Area start={tableItem("Complete the table.")} log={() => {}} />);
    expect(screen.getByText(/有欄名還是空白/)).toBeInTheDocument();
    expect(screen.getByText(/還沒有列標籤/)).toBeInTheDocument();
    // The heading fields say what goes there; none says 「Column 1」.
    expect(screen.getByRole("textbox", { name: "欄名 1（題目 1）" })).toHaveAttribute("placeholder", "寫上要比較的事物");
    expect(screen.queryByDisplayValue(/^Column/)).not.toBeInTheDocument();
  });

  it("shows the headings the question names, and no warning once every heading is there", () => {
    render(<Area start={tableItem(PI_PROMPT)} log={() => {}} />);
    expect(screen.getByRole("textbox", { name: "欄名 2（題目 1）" })).toHaveValue("Speaker Justifying His Decision");
    expect(screen.queryByText(/有欄名還是空白/)).not.toBeInTheDocument();
  });

  it("an old table with 「Column 1」 shows the question's headings, or blank ones with the prompt to fill them", () => {
    const old = { columns: ["Column 1", "Column 2"], rows: 3 };
    const { unmount } = render(<Area start={tableItem(PI_PROMPT, old)} log={() => {}} />);
    expect(screen.getByRole("textbox", { name: "欄名 1（題目 1）" })).toHaveValue("Unconventional Path as a Positive Choice");
    unmount();
    render(<Area start={tableItem("Complete the table about the poem.", old)} log={() => {}} />);
    expect(screen.getByRole("textbox", { name: "欄名 1（題目 1）" })).toHaveValue("");
    expect(screen.getByText(/有欄名還是空白/)).toBeInTheDocument();
  });

  it("adds row labels, edits them, keeps them while rows change, and takes them away", async () => {
    const user = userEvent.setup();
    const saved: WsItem[] = [];
    render(<Area start={tableItem(PI_PROMPT)} log={(it) => saved.push(it)} />);
    const table = screen.getByRole("table", { name: "作答表格 1" });
    expect(within(table).queryByRole("rowheader")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "加列標籤（題目 1）" }));
    expect(within(table).getAllByRole("rowheader")).toHaveLength(3);
    expect(screen.queryByText(/還沒有列標籤/)).not.toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "列標籤 1（題目 1）" }), "Claim");
    await user.type(screen.getByRole("textbox", { name: "列標籤 2（題目 1）" }), "Supporting detail");
    await user.click(screen.getByRole("button", { name: "少一列（題目 1）" }));
    expect(within(table).getAllByRole("rowheader")).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "加一列（題目 1）" }));
    await user.click(screen.getByRole("button", { name: "加一列（題目 1）" }));
    expect(screen.getByRole("textbox", { name: "列標籤 2（題目 1）" })).toHaveValue("Supporting detail");
    expect(screen.getByRole("textbox", { name: "列標籤 4（題目 1）" })).toHaveValue("");
    expect(saved[saved.length - 1]?.table).toEqual({
      columns: ["Unconventional Path as a Positive Choice", "Speaker Justifying His Decision"],
      rows: 4,
      row_labels: ["Claim", "Supporting detail", "", ""],
    });

    await user.click(screen.getByRole("button", { name: "拿掉列標籤（題目 1）" }));
    expect(within(table).queryByRole("rowheader")).not.toBeInTheDocument();
    expect(saved[saved.length - 1]?.table?.row_labels).toEqual([]);
  });
});

describe("the printed preview of a table with row labels", () => {
  const sheet = (): WsSheet => ({
    title: "T",
    level: "standard",
    audience: "student",
    version_code: null,
    objectives: [],
    blocks: [
      {
        id: "b1",
        kind: "activity",
        label: "A",
        title: "Compare",
        instructions: "",
        steps: [],
        vocabulary: [],
        supports: [],
        items: [
          {
            no: 1,
            answer_format: "表格整理",
            prompt: PI_PROMPT,
            options: [],
            lines: 0,
            // As the server projects it: an empty corner for the labels, then the headings.
            table: { columns: ["", "Path A", "Path B"], rows: 2, row_labels: ["Claim", "Supporting detail"] },
          },
        ],
      },
    ],
  });

  it("prints the labels in the first column under the headings", () => {
    render(<SheetPreview sheet={sheet()} pictures={NO_PICTURES} />);
    const table = screen.getByRole("table", { name: "作答表格 1" });
    expect(within(table).getAllByRole("columnheader").map((c) => c.textContent)).toEqual(["", "Path A", "Path B"]);
    expect(within(table).getAllByRole("rowheader").map((c) => c.textContent)).toEqual(["Claim", "Supporting detail"]);
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    expect(table.textContent).not.toContain("Column");
  });
});
