// 單元頁: lessons as ruled rows with one filled action, the page's main content, and 單元想法
// (CoRe) as one summary line that opens on click and never blocks.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { Lesson, LessonRun } from "../api/types";
import {
  baseRoutes,
  classRef,
  coreVersion,
  focus,
  mockApi,
  renderApp,
  schoolClass,
  signIn,
  unitWith,
} from "../test/mockApi";

const lesson = (id: number, title: string): Lesson => ({
  id,
  unit_id: 1,
  title,
  lesson_date: null,
  class_description: "",
  created_at: "2026-11-10T08:00:00Z",
  my_example: "",
});

const taught = (lessonId: number, className: string): LessonRun => ({
  id: 40 + lessonId,
  lesson_id: lessonId,
  lesson_title: "",
  unit_id: 1,
  unit_title: "倒裝句單元",
  started_at: "2026-11-12T01:00:00Z",
  ended_at: "2026-11-12T02:00:00Z",
  review_completed_at: null,
  good_count: 0,
  stuck_count: 0,
  class_id: 3,
  class_name: className,
});

describe("unit page", () => {
  it("lists the lessons with who has had them, and leads with the next lesson to prepare", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1/lessons": [lesson(7, "第 1 節 讀懂故事"), lesson(8, "第 2 節 精讀")],
      "GET /api/units/1/core": coreVersion([]),
      "GET /api/runs": [taught(7, "高一 3 班")],
    });
    renderApp("/units/1");

    expect(await screen.findByRole("heading", { name: "倒裝句單元", level: 1 })).toBeInTheDocument();
    const rows = within(screen.getByRole("region", { name: "課例" })).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByRole("link", { name: "第 1 節 讀懂故事" })).toHaveAttribute("href", "/lessons/7");
    expect(await within(rows[0] as HTMLElement).findByText("高一 3 班上過")).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText("還沒上")).toBeInTheDocument();

    // The one filled action: carry on with the first lesson not taught yet.
    await waitFor(() =>
      expect(screen.getByRole("link", { name: "繼續備課：第 2 節 精讀" })).toHaveAttribute("href", "/lessons/8"),
    );
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);
  });

  it("shows 單元想法 as one line, 「寫了 2/8 題」, that opens on click to the eight questions", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1/lessons": [lesson(7, "第 1 節")],
      "GET /api/units/1/core": coreVersion([
        focus(11, 1, "用倒裝句強調", {
          Q1: { answer: "用倒裝句強調，不只讀懂字面。", status: "filled" },
          Q4: { answer: "助動詞要搬到主詞前面。", status: "filled" },
          Q5: { answer: "", status: "no_idea" },
        }),
      ]),
    });
    const user = userEvent.setup();
    renderApp("/units/1");

    const summary = await screen.findByText("：寫了 2/8 題");
    const details = summary.closest("details") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    // The lessons come first; 單元想法 follows them.
    const lessons = screen.getByRole("region", { name: "課例" });
    expect(lessons.compareDocumentPosition(details) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    await user.click(summary);
    expect(details.open).toBe(true);
    const ideas = within(details);
    // CoRe stays in small print; nothing here is required.
    expect(ideas.getByText("CoRe")).toBeInTheDocument();
    expect(ideas.getByText("想到再寫，不影響備課。")).toBeInTheDocument();
    expect(ideas.getByText("要學會什麼：用倒裝句強調，不只讀懂字面。")).toBeInTheDocument();
    expect(ideas.getByText("可能哪裡難教：助動詞要搬到主詞前面。")).toBeInTheDocument();
    expect(ideas.getByRole("link", { name: "Q2 為什麼重要：寫一點" })).toHaveAttribute("href", "/units/1/core");
    expect(ideas.getByRole("link", { name: "Q8 怎麼知道學生懂了：寫一點" })).toBeInTheDocument();
    expect(ideas.getAllByRole("link", { name: /寫一點$/ })).toHaveLength(6);
    expect(ideas.getByRole("link", { name: "打開單元想法" })).toHaveAttribute("href", "/units/1/core");
    expect(ideas.queryByRole("button")).not.toBeInTheDocument();
  });

  it("keeps to the lessons: no prep menu, class profile or materials on the unit page", async () => {
    signIn();
    const c = schoolClass();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ class_profile: "單元自己的班級特性", classes: [classRef(c)] }),
      "GET /api/units/1/lessons": [lesson(7, "第 1 節")],
      "GET /api/units/1/core": coreVersion([]),
    });
    renderApp("/units/1");
    expect(await screen.findByRole("heading", { name: "倒裝句單元", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "課例", level: 2 })).toBeInTheDocument();
    for (const gone of [/備課菜單/, /班級特性/, /單元教材/, /候選學習重點/, /順序只是建議/]) {
      expect(screen.queryByText(gone)).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("textbox", { name: /班級特性/ })).not.toBeInTheDocument();
  });

  it("does not create a CoRe version just by looking: an untouched unit says nothing is written yet", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ core_status: "none", lesson_count: 0 }),
      "GET /api/units/1/lessons": [],
    });
    const user = userEvent.setup();
    renderApp("/units/1");

    expect(await screen.findByText("：還沒寫")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "開始寫單元想法（8 題）" })).toHaveAttribute("href", "/units/1/core");
    expect(api.callsTo("GET", "/api/units/1/core")).toHaveLength(0);

    // With no lesson yet the form is open, and creating one is the page's filled action. The
    // class profile is not asked for per lesson any more: only a title and a date.
    await user.type(screen.getByLabelText("課例名稱"), "第 1 節");
    expect(screen.queryByLabelText(/班級描述/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "建立課例" })).toHaveClass("primary-button");
    expect(document.querySelectorAll(".primary-button")).toHaveLength(1);
  });

  it("creates a lesson with a title and a date only", async () => {
    signIn();
    const api = mockApi({
      ...baseRoutes(),
      "GET /api/units/1/lessons": [],
      "GET /api/units/1/core": coreVersion([]),
      "POST /api/units/1/lessons": lesson(9, "第 1 節"),
    });
    const user = userEvent.setup();
    renderApp("/units/1");
    await user.type(await screen.findByLabelText("課例名稱"), "第 1 節");
    await user.click(screen.getByRole("button", { name: "建立課例" }));
    await waitFor(() => expect(api.callsTo("POST", "/api/units/1/lessons")).toHaveLength(1));
    expect(api.lastBody("POST", "/api/units/1/lessons")).toEqual({ title: "第 1 節" });
    expect(await screen.findByRole("link", { name: "第 1 節" })).toHaveAttribute("href", "/lessons/9");
  });
});
