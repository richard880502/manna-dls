import type {
  Hardest,
  PrepRoute,
  PreferenceAnswers,
  PreferenceQuestion,
  PreferenceQuestionId,
  Preferences,
} from "../api/types";

/** Server-side limits (400 otherwise); mirrored in the UI. */
export const CLASS_PROFILE_MAX_CHARS = 2000;
export const MY_EXAMPLE_MAX_CHARS = 50000;

export const PREP_ROUTES: readonly PrepRoute[] = ["goal", "material", "student", "activity"];

export const PREP_ROUTE_LABELS: Record<PrepRoute, string> = {
  goal: "目標優先",
  material: "教材優先",
  student: "學生優先",
  activity: "活動優先",
};

export const PREP_ROUTE_DESCRIPTIONS: Record<PrepRoute, string> = {
  goal: "先寫 CoRe 的學習目標（Q1–Q3），再寫課例教案與看 Manna 的建議。",
  material: "先貼上課文或教材，請 Manna 從教材提出候選學習重點，再寫 CoRe 與教案。",
  student: "先寫班級特性與學生卡點，再寫 CoRe Q5、Q6，然後設計教案活動。",
  activity: "先寫活動點子與教案步驟，再回頭連結 CoRe 重點、補寫 CoRe。",
};

/** CoRe questions emphasised first by a route (the other questions stay visible and editable). */
export const ROUTE_CORE_HIGHLIGHTS: Record<PrepRoute, readonly string[]> = {
  goal: ["Q1", "Q2", "Q3"],
  material: [],
  student: ["Q5", "Q6"],
  activity: [],
};

export const PREFERENCE_KEYS: Record<PreferenceQuestionId, keyof PreferenceAnswers> = {
  P1: "experience",
  P2: "start_from",
  P3: "hardest",
  P4: "ai_experience",
  P5: "detail",
  P6: "class_profile",
};

export const PREFERENCE_QUESTION_IDS: readonly PreferenceQuestionId[] = ["P1", "P2", "P3", "P4", "P5", "P6"];

/** Used only if GET /api/preferences/questions fails; the server copy is authoritative (versioned in the back office). */
export const FALLBACK_PREFERENCE_QUESTIONS: PreferenceQuestion[] = [
  {
    id: "P1",
    prompt: "教學年資",
    options: [
      { value: "0-2", label: "0–2 年" },
      { value: "3-9", label: "3–9 年" },
      { value: "10+", label: "10 年以上" },
    ],
    multi: false,
  },
  {
    id: "P2",
    prompt: "你通常從哪裡開始備課？",
    options: [
      { value: "material", label: "課文或教材" },
      { value: "goal", label: "學習目標" },
      { value: "student", label: "學生的問題" },
      { value: "activity", label: "活動點子" },
    ],
    multi: false,
  },
  {
    id: "P3",
    prompt: "哪些步驟最花時間或最困難？（可複選）",
    options: [
      { value: "goal", label: "學習目標" },
      { value: "material", label: "找合適素材" },
      { value: "differentiation", label: "差異化與時間安排" },
      { value: "activity", label: "設計活動" },
      { value: "worksheet", label: "學習單與評量" },
      { value: "ai_communication", label: "跟 AI 溝通" },
    ],
    multi: true,
  },
  {
    id: "P4",
    prompt: "你使用 AI 備課的經驗",
    options: [
      { value: "none", label: "沒用過" },
      { value: "sometimes", label: "偶爾" },
      { value: "often", label: "常用" },
    ],
    multi: false,
  },
  {
    id: "P5",
    prompt: "AI 建議希望多詳細？",
    options: [
      { value: "concise", label: "精簡" },
      { value: "medium", label: "適中" },
      { value: "detailed", label: "詳細" },
    ],
    multi: false,
  },
  { id: "P6", prompt: "我的班級特性（選填）", options: [], multi: false },
];

export const PREFERENCE_SOURCE_LABELS: Record<Preferences["source"], string> = {
  onboarding: "初次使用設定",
  settings: "我的備課偏好",
};

export function emptyAnswers(): PreferenceAnswers {
  return {
    experience: null,
    start_from: null,
    hardest: [],
    ai_experience: null,
    detail: null,
    class_profile: "",
  };
}

/** Normalises answers from the server: known keys only (the API rejects unknown keys), missing fields → defaults. */
export function answersFrom(prefs: Preferences | null | undefined): PreferenceAnswers {
  const a = prefs?.answers;
  return {
    experience: a?.experience ?? null,
    start_from: a?.start_from ?? null,
    hardest: Array.isArray(a?.hardest) ? [...a.hardest] : [],
    ai_experience: a?.ai_experience ?? null,
    detail: a?.detail ?? null,
    class_profile: a?.class_profile ?? "",
  };
}

/** Keeps only P1–P6 in their canonical order; unknown ids from a newer question set are ignored. */
export function normaliseQuestions(list: PreferenceQuestion[] | null | undefined): PreferenceQuestion[] {
  if (!Array.isArray(list) || list.length === 0) return FALLBACK_PREFERENCE_QUESTIONS;
  const byId = new Map(list.map((q) => [q.id, q]));
  const out = PREFERENCE_QUESTION_IDS.map((id) => byId.get(id)).filter((q): q is PreferenceQuestion => !!q);
  return out.length > 0 ? out : FALLBACK_PREFERENCE_QUESTIONS;
}

export function isTextQuestion(q: PreferenceQuestion): boolean {
  return q.id === "P6" || q.options.length === 0;
}

export function answerValue(answers: PreferenceAnswers, id: PreferenceQuestionId): string | string[] {
  const value = answers[PREFERENCE_KEYS[id]];
  if (Array.isArray(value)) return value;
  return value ?? "";
}

export function withAnswer(
  answers: PreferenceAnswers,
  id: PreferenceQuestionId,
  value: string | string[],
): PreferenceAnswers {
  const key = PREFERENCE_KEYS[id];
  if (key === "hardest") {
    return { ...answers, hardest: (Array.isArray(value) ? value : [value]).filter(Boolean) as Hardest[] };
  }
  if (key === "class_profile") {
    return { ...answers, class_profile: Array.isArray(value) ? value.join("") : value };
  }
  const single = Array.isArray(value) ? (value[0] ?? "") : value;
  return { ...answers, [key]: single === "" ? null : single } as PreferenceAnswers;
}

export function hasHardest(prefs: Preferences | null | undefined, value: Hardest): boolean {
  return Array.isArray(prefs?.answers?.hardest) && prefs.answers.hardest.includes(value);
}

export function preferenceTrigger(value: Hardest): string {
  return `preference:hardest=${value}`;
}

/** The route a unit actually follows: fixed goal-first when route choice is disabled platform-wide. */
export function effectiveRoute(routeChoiceEnabled: boolean, route: PrepRoute | null | undefined): PrepRoute {
  if (!routeChoiceEnabled) return "goal";
  return route && PREP_ROUTES.includes(route) ? route : "goal";
}
