import { act, render, screen } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { vi } from "vitest";
import type {
  AppConfig,
  ClassDetail,
  ClassUnit,
  CoreFocus,
  CoreQuestion,
  CoreResponse,
  CoreVersion,
  LessonDetail,
  LessonStep,
  Material,
  MaterialBrowse,
  MaterialCard,
  MaterialLibrary,
  PreferenceAnswers,
  Preferences,
  QuestionId,
  SchoolClass,
  Teacher,
  TemplateBrowse,
  TemplateCard,
  Unit,
  UnitClassRef,
} from "../api/types";
import { QUESTION_IDS } from "../api/types";
import { App } from "../App";
import { FALLBACK_QUESTIONS } from "../lib/core";
import { FALLBACK_PREFERENCE_QUESTIONS } from "../lib/preferences";
import { TOKEN_STORAGE_KEY } from "../lib/storage";

export interface MockCall {
  method: string;
  /** Path without query string, e.g. /api/units/1/core */
  path: string;
  url: string;
  headers: Record<string, string>;
  rawBody: string | undefined;
  /** Parsed JSON body (undefined if not JSON). */
  body: unknown;
}

export class MockResponse {
  constructor(
    readonly status: number,
    readonly body: unknown,
  ) {}
}

/** Respond with a non-2xx status from a route handler. */
export const respond = (status: number, body: unknown) => new MockResponse(status, body);

type RouteValue = unknown | ((call: MockCall) => unknown);

/**
 * Stub global fetch with a route table keyed by "METHOD /api/path".
 * Unknown routes return 404 with the API error shape. Scaffold events default to { ok: true }.
 */
export function mockApi(routes: Record<string, RouteValue>) {
  const calls: MockCall[] = [];
  const table: Record<string, RouteValue> = { "POST /api/scaffold-events": { ok: true }, ...routes };

  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    const method = (init?.method ?? "GET").toUpperCase();
    const path = url.split("?")[0] ?? url;
    const rawBody = typeof init?.body === "string" ? init.body : undefined;
    let body: unknown;
    try {
      body = rawBody !== undefined ? JSON.parse(rawBody) : undefined;
    } catch {
      body = undefined;
    }
    const headers = { ...((init?.headers as Record<string, string> | undefined) ?? {}) };
    const call: MockCall = { method, path, url, headers, rawBody, body };
    calls.push(call);

    const key = `${method} ${path}`;
    let status = 200;
    let payload: unknown;
    if (key in table) {
      const value = table[key];
      const result = typeof value === "function" ? (value as (c: MockCall) => unknown)(call) : value;
      const resolved = result instanceof Promise ? await result : result;
      if (resolved instanceof MockResponse) {
        status = resolved.status;
        payload = resolved.body;
      } else {
        payload = resolved;
      }
    } else {
      status = 404;
      payload = { error: { code: "not_found", message: `測試未定義：${key}` } };
    }
    const text = payload === undefined ? "" : JSON.stringify(payload);
    return {
      ok: status >= 200 && status < 300,
      status,
      text: async () => text,
      json: async () => JSON.parse(text) as unknown,
    } as unknown as Response;
  });

  vi.stubGlobal("fetch", fetchMock);

  return {
    fetchMock,
    calls,
    callsTo: (method: string, path: string) => calls.filter((c) => c.method === method && c.path === path),
    lastBody: (method: string, path: string) => {
      const matching = calls.filter((c) => c.method === method && c.path === path);
      return matching[matching.length - 1]?.body;
    },
  };
}

// ---------- lesson page helpers ----------

/**
 * The lesson page has no save button: it saves a moment after an edit, when a field is left,
 * and when the teacher looks away from the window. Tests use the last one, then wait for the
 * saved line.
 */
export async function settleAutosave(_user?: unknown) {
  act(() => {
    window.dispatchEvent(new Event("blur"));
  });
  await screen.findByText(/^已自動儲存/);
}

/** Renders the app in a memory data router (as main.tsx does), so pages can use useBlocker. */
export function renderApp(path: string) {
  const router = createMemoryRouter([{ path: "*", element: <App /> }], { initialEntries: [path] });
  return { router, ...render(<RouterProvider router={router} />) };
}

export function signIn(token = "test-token") {
  window.localStorage.setItem(TOKEN_STORAGE_KEY, token);
}

// ---------- fixtures ----------

export const config = (overrides: Partial<AppConfig> = {}): AppConfig => ({
  demo_mode: false,
  ai_enabled: true,
  ai_provider: "anthropic",
  ai_model: "claude-opus-5-5",
  route_choice_enabled: true,
  ...overrides,
});

export const teacher: Teacher = { id: 1, research_id: "T-2026-001", school_track: "general", role: "teacher" };

export const unit: Unit = {
  id: 1,
  title: "倒裝句單元",
  created_at: "2026-11-10T08:00:00Z",
  core_status: "draft",
  lesson_count: 1,
  prep_route: "goal",
  class_profile: "",
  class_profile_source: null,
  classes: [],
};

export const unitWith = (overrides: Partial<Unit> = {}): Unit => ({ ...unit, ...overrides });

// ---------- classes (Phase 1.4) ----------

export const schoolClass = (overrides: Partial<SchoolClass> = {}): SchoolClass => ({
  id: 3,
  name: "高一 3 班",
  grade: "高一",
  school_track: "general",
  course_label: "",
  class_profile: "三班：不太敢開口",
  class_profile_source: "teacher",
  school_year: "",
  term: "",
  position: 1,
  created_at: "2026-11-10T08:00:00Z",
  archived_at: null,
  unit_count: 1,
  next_action: null,
  ...overrides,
});

/** A class as a unit lists it (`Unit.classes`). */
export const classRef = (c: SchoolClass, note = ""): UnitClassRef => ({
  id: c.id,
  name: c.name,
  course_label: c.course_label,
  class_profile: c.class_profile,
  note,
});

export const classUnit = (u: Unit, overrides: Partial<ClassUnit> = {}): ClassUnit => ({
  unit: u,
  position: 1,
  note: "",
  assigned_at: "2026-11-10T08:00:00Z",
  lessons_total: u.lesson_count,
  lessons_taught: 0,
  last_run_at: null,
  next_lesson: null,
  ...overrides,
});

export const classDetail = (c: SchoolClass, units: ClassUnit[]): ClassDetail => ({
  ...c,
  unit_count: units.length,
  units,
});

export const preferences = (
  answers: Partial<PreferenceAnswers> = {},
  extra: Partial<Preferences> = {},
): Preferences => ({
  version_no: 1,
  question_set_version: 1,
  answers: {
    experience: null,
    start_from: null,
    hardest: [],
    ai_experience: null,
    detail: null,
    class_profile: "",
    ...answers,
  },
  skipped: false,
  source: "onboarding",
  created_at: "2026-11-10T08:00:00Z",
  ...extra,
});

export const step = (overrides: Partial<LessonStep> = {}): LessonStep => ({
  position: 1,
  activity_type: "",
  icap_level: null,
  content: "",
  duration_min: null,
  support_note: "",
  extension_note: "",
  ...overrides,
});

export const questions: CoreQuestion[] = FALLBACK_QUESTIONS;

export function blankResponses(
  overrides: Partial<Record<QuestionId, Partial<CoreResponse>>> = {},
): Record<QuestionId, CoreResponse> {
  const out = {} as Record<QuestionId, CoreResponse>;
  for (const q of QUESTION_IDS) {
    out[q] = { answer: "", status: "blank", content_source: "self", updated_at: null, ...overrides[q] };
  }
  return out;
}

export function focus(id: number, position: number, big_idea: string, overrides = {}): CoreFocus {
  return { id, position, big_idea, responses: blankResponses(overrides) };
}

export function coreVersion(focuses: CoreFocus[], extra: Partial<CoreVersion> = {}): CoreVersion {
  return {
    id: 501,
    unit_id: 1,
    phase: "pre",
    version_no: 1,
    completed_at: null,
    saw_ai_assist: false,
    focuses,
    ...extra,
  };
}

export const lessonDetail = (overrides: Partial<LessonDetail> = {}): LessonDetail => ({
  id: 7,
  unit_id: 1,
  title: "第 1 課：Not only… but also",
  lesson_date: null,
  class_description: "高一普通班",
  created_at: "2026-11-10T08:00:00Z",
  my_example: "",
  effective_class_profile: "高一普通班",
  latest_version: {
    id: 70,
    version_no: 2,
    objective: "學生能說明倒裝的語意功能",
    steps: [step({ position: 1, activity_type: "暖身", icap_level: "active", content: "比較兩組句子" })],
    created_at: "2026-11-11T08:00:00Z",
  },
  versions_count: 2,
  core_focus_ids: [],
  siop_feature_ids: [],
  previous_next_change: null,
  ...overrides,
});

/** Mirrors the server: P6's privacy hint is part of its prompt text. */
export const preferenceQuestions = FALLBACK_PREFERENCE_QUESTIONS.map((q) =>
  q.id === "P6" ? { ...q, prompt: "我的班級特性（選填；不要寫學生姓名或個人資料）" } : q,
);

/** Routes every authenticated page needs. Preferences default to an answered survey with no P3 picks. */
export function baseRoutes(cfg: Partial<AppConfig> = {}, prefs: Preferences | null = preferences()): Record<string, RouteValue> {
  return {
    "GET /api/config": config(cfg),
    "GET /api/me": teacher,
    "GET /api/me/preferences": prefs,
    "GET /api/me/memories": { memories: [], enabled: false, graph_enabled: false, index: null },
    "GET /api/admin/memory-settings": { enabled: false, graph_enabled: false },
    "GET /api/admin/memory-index": { pending: 0, failed: 0 },
    "GET /api/preferences/questions": { question_set_version: 1, questions: preferenceQuestions },
    "GET /api/units": [unit],
    "GET /api/units/1": unit,
    "GET /api/classes": [],
    "GET /api/core/questions": questions,
    "GET /api/worksheet-templates": { templates: [] },
    // 備課 (the home page) lists recent prep sessions and offers the material library.
    "GET /api/prep-sessions": { sessions: [] },
    "GET /api/stepwise-lessons": [],
    "GET /api/materials": emptyLibrary(),
    // 平台教材／我的教材／學習單範本 and the pickers.
    "GET /api/browse/materials": materialBrowse([]),
    "GET /api/browse/templates": templateBrowse([]),
    // SIOP G2: without AI there is nothing to draft, so 產生 goes straight on.
    "POST /api/objective-drafts": {
      interaction_id: null,
      status: "disabled",
      message: "這個環境沒有開啟 AI 協助。",
      objectives: [],
      hidden: 0,
      context: [],
    },
  };
}

// ---------- 平台教材、我的教材、學習單範本 (library contract) ----------

/** One material card as GET /api/browse/materials lists it (all made up). */
export const materialCard = (overrides: Partial<MaterialCard> = {}): MaterialCard => ({
  id: 5,
  title: "The Umbrella Stand",
  scope: "personal",
  category: "",
  tags: [],
  kind: "document",
  file_type: "docx",
  page_count: null,
  paragraph_count: 4,
  word_count: 300,
  excerpt: "Mrs. Lin put an umbrella stand outside her shop.",
  text_status: "ready",
  looks_like_worksheet: false,
  created_at: "2026-11-14T08:00:00Z",
  owner_label: "我",
  ...overrides,
});

/** A library material (GET /api/materials) as its card. */
export const cardOf = (m: Material, overrides: Partial<MaterialCard> = {}): MaterialCard =>
  materialCard({
    id: m.id,
    title: m.title,
    kind: m.kind,
    file_type: m.kind === "text" ? "text" : (m.filename?.split(".").pop() ?? m.kind),
    page_count: m.page_count,
    paragraph_count: m.paragraph_count,
    word_count: m.word_count,
    text_status: m.text_status,
    looks_like_worksheet: m.looks_like_worksheet ?? false,
    created_at: m.created_at,
    ...overrides,
  });

/** Counts per key, as the server's facets. */
function countBy(keys: string[]): Array<{ key: string; count: number }> {
  const counts = new Map<string, number>();
  for (const k of keys) counts.set(k, (counts.get(k) ?? 0) + 1);
  return [...counts].map(([key, count]) => ({ key, count }));
}

/** A browse answer with these cards; facets are counted from them. */
export const materialBrowse = (items: MaterialCard[], extra: Partial<MaterialBrowse> = {}): MaterialBrowse => ({
  items,
  next_cursor: null,
  facets: {
    categories: countBy(items.map((c) => c.category)),
    tags: countBy(items.flatMap((c) => c.tags)),
    file_types: countBy(items.map((c) => c.file_type)),
  },
  total: items.length,
  ...extra,
});

/** One template card as GET /api/browse/templates lists it (all made up). */
export const templateCard = (overrides: Partial<TemplateCard> = {}): TemplateCard => ({
  id: 9,
  title: "L2 學習單版型",
  scope: "personal",
  status: "ready",
  category: "閱讀理解",
  tags: [],
  worksheet_kind: "課文深度閱讀",
  instruction_language: "英文",
  section_count: 3,
  section_titles: ["Warm-up: [主題]", "Paragraph [n]: Find", "Video talk"],
  source_material: { id: 4, title: "L2 學習單", file_type: "docx", page_count: 2 },
  updated_at: "2026-11-14T08:00:00Z",
  owner_label: "我",
  ...overrides,
});

export const templateBrowse = (items: TemplateCard[], extra: Partial<TemplateBrowse> = {}): TemplateBrowse => ({
  items,
  next_cursor: null,
  facets: {
    categories: countBy(items.map((c) => c.category)),
    tags: countBy(items.flatMap((c) => c.tags)),
    langs: countBy(items.map((c) => c.instruction_language)),
  },
  total: items.length,
  ...extra,
});

/** The query a browse call was made with (repeated keys as lists). */
export function queryOf(call: MockCall): Record<string, string | string[]> {
  const params = new URL(call.url, "http://x").searchParams;
  const out: Record<string, string | string[]> = {};
  for (const key of new Set(params.keys())) {
    const all = params.getAll(key);
    out[key] = all.length > 1 ? all : (all[0] ?? "");
  }
  return out;
}

/** A material library with nothing in it (uploads allowed). */
export const emptyLibrary = (): MaterialLibrary => ({
  materials: [],
  usage: { used_bytes: 0, quota_bytes: 1024 * 1024 * 1024, count: 0, max_count: 300 },
  limits: {
    files: [
      { ext: "docx", kind: "document", max_bytes: 20 * 1024 * 1024 },
      { ext: "pdf", kind: "document", max_bytes: 20 * 1024 * 1024 },
    ],
    max_pasted_chars: 50000,
    max_note_chars: 2000,
  },
  storage: "fs",
  uploads_enabled: true,
});
