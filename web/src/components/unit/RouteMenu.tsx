import { Link } from "react-router";
import type { Lesson, PrepRoute, Unit } from "../../api/types";
import { PREP_ROUTE_DESCRIPTIONS, PREP_ROUTE_LABELS } from "../../lib/preferences";

interface MenuStep {
  key: string;
  label: string;
  /** Route path to open, or an element id on this page to focus. */
  to?: string;
  focusId?: string;
  done: boolean;
}

function focusElement(id: string) {
  const el = document.getElementById(id);
  if (!el) return;
  if (typeof el.scrollIntoView === "function") el.scrollIntoView({ behavior: "smooth", block: "center" });
  el.focus();
}

export function routeSteps(
  route: PrepRoute,
  unit: Unit,
  lessons: Lesson[],
  materialsCount: number | null,
): MenuStep[] {
  const coreUrl = `/units/${unit.id}/core`;
  const firstLesson = lessons[0];
  const coreStarted = unit.core_status !== "none";
  const lessonStep: MenuStep = firstLesson
    ? { key: "lesson", label: "打開課例，寫教案步驟", to: `/lessons/${firstLesson.id}`, done: false }
    : { key: "lesson", label: "建立第一個課例", focusId: "lesson-title", done: false };

  switch (route) {
    case "material":
      return [
        { key: "paste", label: "貼上課文或教材", focusId: "material-text", done: (materialsCount ?? 0) > 0 },
        {
          key: "candidates",
          label: "請 Manna 從教材提出候選學習重點",
          focusId: "materials-heading",
          done: coreStarted,
        },
        { key: "core", label: "寫單元前 CoRe", to: coreUrl, done: coreStarted },
        lessonStep,
      ];
    case "student":
      return [
        {
          key: "profile",
          label: "寫班級特性與學生卡點",
          focusId: "unit-class-profile",
          done: !!unit.class_profile?.trim(),
        },
        { key: "core", label: "寫 CoRe Q5、Q6（學生可能怎麼想、影響教學的因素）", to: coreUrl, done: coreStarted },
        lessonStep,
      ];
    case "activity":
      return [
        firstLesson
          ? { key: "lesson", label: "打開課例，先寫活動點子與教案步驟", to: `/lessons/${firstLesson.id}`, done: false }
          : { key: "lesson", label: "建立課例，先寫活動點子", focusId: "lesson-title", done: false },
        {
          key: "links",
          label: "在課例頁連結 1–3 個 CoRe 學習重點",
          ...(firstLesson ? { to: `/lessons/${firstLesson.id}` } : { focusId: "lesson-title" }),
          done: false,
        },
        { key: "core", label: "回頭補寫單元前 CoRe", to: coreUrl, done: unit.core_status === "completed" },
      ];
    case "goal":
    default:
      return [
        { key: "core", label: "寫單元前 CoRe（先寫 Q1–Q3）", to: coreUrl, done: coreStarted },
        lessonStep,
      ];
  }
}

interface RouteMenuProps {
  route: PrepRoute;
  routeChoiceEnabled: boolean;
  unit: Unit;
  lessons: Lesson[];
  materialsCount: number | null;
}

/** 備課菜單：the ordered steps for the unit's prep route with one clear 「下一步」. */
export function RouteMenu({ route, routeChoiceEnabled, unit, lessons, materialsCount }: RouteMenuProps) {
  const steps = routeSteps(route, unit, lessons, materialsCount);
  // For the activity route the lesson stays "next" until the teacher moves on; other routes advance by state.
  const next = steps.find((s) => !s.done) ?? steps[steps.length - 1];

  return (
    <div className="route-menu">
      <div>
        <p className="eyebrow warm">{routeChoiceEnabled ? `備課菜單 · ${PREP_ROUTE_LABELS[route]}` : "備課順序"}</p>
        <p className="route-menu-description">{PREP_ROUTE_DESCRIPTIONS[route]}</p>
      </div>
      {next && <StepAction step={next} primary prefix="下一步：" />}
      <ol className="route-steps" aria-label="備課步驟">
        {steps.map((step, index) => (
          <li key={step.key} className={`route-step${step.done ? " is-done" : ""}${step === next ? " is-next" : ""}`}>
            <span className="route-step-number" aria-hidden="true">
              {step.done ? "✓" : index + 1}
            </span>
            <StepAction step={step} />
            {step.done && <span className="sr-only">（已開始）</span>}
          </li>
        ))}
      </ol>
      <p className="field-hint">順序只是建議：每個畫面隨時都可以打開，留白也不會被擋下。</p>
    </div>
  );
}

function StepAction({ step, primary = false, prefix = "" }: { step: MenuStep; primary?: boolean; prefix?: string }) {
  const className = primary ? "primary-button link-button route-next" : "route-step-link";
  if (step.to) {
    return (
      <Link className={className} to={step.to}>
        {prefix}
        {step.label}
      </Link>
    );
  }
  return (
    <button type="button" className={className} onClick={() => step.focusId && focusElement(step.focusId)}>
      {prefix}
      {step.label}
    </button>
  );
}
