// 班級與備課庫 (Phase 1.4): the class a unit or lesson is worked on in, and the words for a class's next lesson.
import type { ClassNextAction, LessonRun, SchoolClass, Unit, UnitClassRef } from "../api/types";
import { useApp } from "../context/AppContext";
import { SCHOOL_TRACK_LABELS } from "./labels";

export const CLASS_NAME_MAX_CHARS = 60;
export const CLASS_LABEL_MAX_CHARS = 60;
export const CLASS_SHORT_MAX_CHARS = 20;
export const CLASS_NOTE_MAX_CHARS = 500;

export interface ActiveClass {
  /** The classes that use this unit (empty while the unit loads). */
  classes: UnitClassRef[];
  /**
   * The class in play for this unit: the class the teacher came from when it uses this unit;
   * otherwise the unit's only class; otherwise null (several classes use it and the teacher came
   * from the library, or none uses it). It is never asked for on the page: the class is context,
   * shown once in the breadcrumb (docs/DESIGN_OVERHAUL.md, principle 6).
   */
  active: UnitClassRef | null;
  choose: (classId: number | null) => void;
}

/** The class context of a unit, lesson or 單元想法 page. */
export function useActiveClass(unit: Pick<Unit, "classes"> | null | undefined): ActiveClass {
  const { currentClassId, setCurrentClassId } = useApp();
  const classes = unit?.classes ?? [];
  const active =
    classes.find((c) => c.id === currentClassId) ?? (classes.length === 1 ? (classes[0] ?? null) : null);
  return { classes, active, choose: setCurrentClassId };
}

/** 「高一」「普高」「英文閱讀選修」「115 學年度 上學期」, one item each: only the parts that are filled in. */
export function classSubtitle(
  c: Pick<SchoolClass, "grade" | "school_track" | "course_label" | "school_year" | "term">,
  withTrack = false,
): string[] {
  const yearTerm = [c.school_year && `${c.school_year} 學年度`, c.term].filter(Boolean).join(" ");
  return [c.grade, withTrack ? SCHOOL_TRACK_LABELS[c.school_track] : "", c.course_label, yearTerm]
    .filter(Boolean);
}

/** 「高一 3 班、高一 7 班在用」 / 「還沒有班級在用」. */
export function usedByText(classes: Array<Pick<UnitClassRef, "name">>): string {
  return classes.length === 0 ? "還沒有班級在用" : `${classes.map((c) => c.name).join("、")}在用`;
}

/** Unit title, a full-width space, lesson title: the lesson a class is about to have. */
export function nextLessonTitle(next: Extract<ClassNextAction, { lesson_title: string }>): string {
  return `${next.unit_title}　${next.lesson_title}`;
}

/** One quiet line saying what is next for a class (the right-hand side of a class row). */
export function nextLine(next: ClassNextAction | null): string {
  if (!next) return "取用的單元都上過了";
  switch (next.kind) {
    case "resume_run":
      return `上課中：${next.lesson_title}`;
    case "start_lesson":
      return `下一節：${next.lesson_title}`;
    case "plan_unit":
      return `「${next.unit_title}」還沒有課例`;
    case "add_unit":
      return "還沒有單元";
  }
}

/** Whether a class has a lesson to teach now (to start, or already open). */
export function hasLessonNext(
  next: ClassNextAction | null,
): next is Extract<ClassNextAction, { lesson_title: string }> {
  return next !== null && (next.kind === "start_lesson" || next.kind === "resume_run");
}

/**
 * The class whose next lesson the home page leads with. A class with a lesson in progress comes
 * first. Otherwise, there being no timetable, it is the class taught most recently that has a
 * lesson to teach (`runs` come newest first); failing that, the first class with one; failing
 * that, the first class with a unit to plan.
 */
export function focalClass(classes: SchoolClass[], runs: LessonRun[]): SchoolClass | null {
  const inClass = classes.find((c) => c.next_action?.kind === "resume_run");
  if (inClass) return inClass;
  const ready = classes.filter((c) => hasLessonNext(c.next_action));
  for (const run of runs) {
    const taught = ready.find((c) => c.id === run.class_id);
    if (taught) return taught;
  }
  return ready[0] ?? classes.find((c) => c.next_action?.kind === "plan_unit") ?? null;
}
