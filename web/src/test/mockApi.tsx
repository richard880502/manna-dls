import { render } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { vi } from "vitest";
import type {
  AppConfig,
  CoreFocus,
  CoreQuestion,
  CoreResponse,
  CoreVersion,
  LessonDetail,
  QuestionId,
  Teacher,
  Unit,
} from "../api/types";
import { QUESTION_IDS } from "../api/types";
import { App } from "../App";
import { FALLBACK_QUESTIONS } from "../lib/core";
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

export function renderApp(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <App />
    </MemoryRouter>,
  );
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
  ...overrides,
});

export const teacher: Teacher = { id: 1, research_id: "T-2026-001", school_track: "general", role: "teacher" };

export const unit: Unit = {
  id: 1,
  title: "倒裝句單元",
  created_at: "2026-11-10T08:00:00Z",
  core_status: "draft",
  lesson_count: 1,
};

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
  latest_version: {
    id: 70,
    version_no: 2,
    objective: "學生能說明倒裝的語意功能",
    steps: [{ position: 1, activity_type: "暖身", icap_level: "active", content: "比較兩組句子" }],
    created_at: "2026-11-11T08:00:00Z",
  },
  versions_count: 2,
  core_focus_ids: [],
  siop_feature_ids: [],
  previous_next_change: null,
  ...overrides,
});

/** Routes every authenticated page needs. */
export function baseRoutes(cfg: Partial<AppConfig> = {}): Record<string, RouteValue> {
  return {
    "GET /api/config": config(cfg),
    "GET /api/me": teacher,
    "GET /api/units": [unit],
    "GET /api/units/1": unit,
    "GET /api/core/questions": questions,
  };
}
