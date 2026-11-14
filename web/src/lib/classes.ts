// 班級與備課庫 (Phase 1.4): the current class of a unit, and the words for a class's next step.
import type { ClassNextAction, ClassProfileSourceUsed, SchoolClass, Unit, UnitClassRef } from "../api/types";
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
   * 目前班級 for this unit: the class the teacher came from when it uses this unit; otherwise
   * the unit's only class; otherwise null (several classes use it and none was chosen, or none uses it).
   */
  active: UnitClassRef | null;
  choose: (classId: number | null) => void;
}

/** The class context of a unit, lesson or CoRe page. */
export function useActiveClass(unit: Pick<Unit, "classes"> | null | undefined): ActiveClass {
  const { currentClassId, setCurrentClassId } = useApp();
  const classes = unit?.classes ?? [];
  const active =
    classes.find((c) => c.id === currentClassId) ?? (classes.length === 1 ? (classes[0] ?? null) : null);
  return { classes, active, choose: setCurrentClassId };
}

/** 「高一 · 普高 · 英文閱讀選修 · 115 上學期」: only the parts that are filled in. */
export function classSubtitle(
  c: Pick<SchoolClass, "grade" | "school_track" | "course_label" | "school_year" | "term">,
  withTrack = false,
): string {
  const yearTerm = [c.school_year && `${c.school_year} 學年度`, c.term].filter(Boolean).join(" ");
  return [c.grade, withTrack ? SCHOOL_TRACK_LABELS[c.school_track] : "", c.course_label, yearTerm]
    .filter(Boolean)
    .join(" · ");
}

export interface NextStep {
  /** What is next, e.g. 「繼續：L3 倒裝句 第 2 節」. */
  text: string;
  /** The button, e.g. 「開始本節課」. */
  action: string;
  to: string;
}

/** The home card's next step for a class (docs/API.md Phase 1.4, `ClassNextAction`). */
export function nextStep(classId: number, next: ClassNextAction | null): NextStep {
  if (!next) return { text: "取用的單元都上過了", action: "看這個班級", to: `/classes/${classId}` };
  switch (next.kind) {
    case "resume_run":
      return {
        text: `上課中：${next.unit_title} ${next.lesson_title}`,
        action: "回到課堂紀錄",
        to: `/runs/${next.run_id}`,
      };
    case "start_lesson":
      return {
        text: `繼續：${next.unit_title} ${next.lesson_title}`,
        action: "開始本節課",
        to: `/lessons/${next.lesson_id}`,
      };
    case "plan_unit":
      return { text: `下一個單元「${next.unit_title}」還沒有課例`, action: "去備課", to: `/units/${next.unit_id}` };
    case "add_unit":
      return { text: "還沒有單元", action: "從備課庫加入單元", to: `/classes/${classId}` };
  }
}

/** Where the class profile Manna uses comes from, in the teacher's words. */
export function profileSourceText(source: ClassProfileSourceUsed | undefined, className: string | null): string {
  switch (source) {
    case "lesson":
      return "本課覆寫";
    case "class":
      return className ? `${className}的班級特性` : "班級的班級特性";
    case "preference":
      return "沿用備課偏好";
    case "unit":
      return "沿用單元設定";
    default:
      return className ? `${className}還沒有班級特性` : "沿用單元設定";
  }
}
