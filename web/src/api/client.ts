import { loadToken } from "../lib/storage";
import type {
  AiResult,
  ApiErrorBody,
  AppConfig,
  CoreAssistOutput,
  CoreAssistRequest,
  CoreDraftRequest,
  CoreLinksResponse,
  CoreQuestion,
  CoreVersion,
  CreateLessonRequest,
  CreatePromptRequest,
  DecisionRequest,
  Health,
  ImportResult,
  InviteRequest,
  InviteResponse,
  Lesson,
  LessonDetail,
  LessonSiopResponse,
  LessonVersion,
  LoginResponse,
  OkResponse,
  PlanSuggestOutput,
  PromptVersion,
  SaveLessonVersionRequest,
  ScaffoldEventRequest,
  SiopFeature,
  SuggestionsRequest,
  Teacher,
  TeacherCase,
  Unit,
} from "./types";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

const FALLBACK_MESSAGES: Record<number, string> = {
  400: "送出的資料格式有誤，請檢查後再試一次。",
  401: "登入已過期，請重新登入。",
  403: "你沒有權限執行這個動作。",
  404: "找不到這筆資料。",
  409: "資料已被更新，請重新整理後再試一次。",
};

export const NETWORK_ERROR_MESSAGE = "無法連線到伺服器，請確認網路後再試一次。";
const SERVER_ERROR_MESSAGE = "伺服器暫時發生問題，請稍後再試。";

/** Friendly Chinese text for any thrown value. */
export function errorMessage(err: unknown): string {
  if (err instanceof ApiError) return err.message;
  return NETWORK_ERROR_MESSAGE;
}

type UnauthorizedHandler = () => void;
let unauthorizedHandler: UnauthorizedHandler | null = null;

/** Registered by the auth provider: called when an authenticated request returns 401. */
export function setUnauthorizedHandler(handler: UnauthorizedHandler | null): void {
  unauthorizedHandler = handler;
}

interface RequestOptions {
  json?: unknown;
  text?: string;
  contentType?: string;
  query?: Record<string, string | undefined>;
  /** Skip the global 401 handler (used by login). */
  skipAuthHandler?: boolean;
}

function buildUrl(path: string, query?: Record<string, string | undefined>): string {
  const url = `/api${path}`;
  if (!query) return url;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== "") params.set(key, value);
  }
  const qs = params.toString();
  return qs ? `${url}?${qs}` : url;
}

async function request<T>(method: string, path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: "application/json" };
  const token = loadToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  let body: string | undefined;
  if (options.text !== undefined) {
    body = options.text;
    headers["Content-Type"] = options.contentType ?? "text/plain";
  } else if (options.json !== undefined) {
    body = JSON.stringify(options.json);
    headers["Content-Type"] = "application/json";
  }

  let response: Response;
  try {
    response = await fetch(buildUrl(path, options.query), { method, headers, body });
  } catch {
    throw new ApiError(0, "network", NETWORK_ERROR_MESSAGE);
  }

  const raw = await response.text().catch(() => "");
  let data: unknown = null;
  if (raw) {
    try {
      data = JSON.parse(raw);
    } catch {
      data = null;
    }
  }

  if (!response.ok) {
    const errBody = data as Partial<ApiErrorBody> | null;
    const code = errBody?.error?.code ?? "internal";
    const message =
      errBody?.error?.message ||
      FALLBACK_MESSAGES[response.status] ||
      SERVER_ERROR_MESSAGE;
    if (response.status === 401 && !options.skipAuthHandler && unauthorizedHandler) {
      unauthorizedHandler();
    }
    throw new ApiError(response.status, code, message);
  }

  return data as T;
}

const get = <T>(path: string, query?: Record<string, string | undefined>) =>
  request<T>("GET", path, { query });
const post = <T>(path: string, json?: unknown) => request<T>("POST", path, { json: json ?? {} });
const put = <T>(path: string, json: unknown) => request<T>("PUT", path, { json });

export const api = {
  // 公開
  health: () => get<Health>("/health"),
  config: () => get<AppConfig>("/config"),
  login: (code: string) =>
    request<LoginResponse>("POST", "/auth/login", { json: { code }, skipAuthHandler: true }),
  logout: () => request<OkResponse>("POST", "/auth/logout", { json: {}, skipAuthHandler: true }),

  // 教師
  me: () => get<Teacher>("/me"),
  listUnits: () => get<Unit[]>("/units"),
  createUnit: (title: string) => post<Unit>("/units", { title }),
  getUnit: (unitId: number) => get<Unit>(`/units/${unitId}`),
  coreQuestions: () => get<CoreQuestion[]>("/core/questions"),
  getCore: (unitId: number) => get<CoreVersion>(`/units/${unitId}/core`),
  saveCoreDraft: (unitId: number, body: CoreDraftRequest) =>
    put<CoreVersion>(`/units/${unitId}/core/draft`, body),
  completeCore: (unitId: number) => post<CoreVersion>(`/units/${unitId}/core/complete`),
  coreAssist: (body: CoreAssistRequest) => post<AiResult<CoreAssistOutput>>("/core/assist", body),
  listLessons: (unitId: number) => get<Lesson[]>(`/units/${unitId}/lessons`),
  createLesson: (unitId: number, body: CreateLessonRequest) =>
    post<Lesson>(`/units/${unitId}/lessons`, body),
  getLesson: (lessonId: number) => get<LessonDetail>(`/lessons/${lessonId}`),
  saveLessonVersion: (lessonId: number, body: SaveLessonVersionRequest) =>
    post<LessonVersion>(`/lessons/${lessonId}/versions`, body),
  setCoreLinks: (lessonId: number, focusIds: number[]) =>
    put<CoreLinksResponse>(`/lessons/${lessonId}/core-links`, { focus_ids: focusIds }),
  listSiop: () => get<SiopFeature[]>("/siop"),
  setLessonSiop: (lessonId: number, featureIds: number[]) =>
    put<LessonSiopResponse>(`/lessons/${lessonId}/siop`, { siop_feature_ids: featureIds }),
  requestSuggestions: (lessonId: number, body: SuggestionsRequest) =>
    post<AiResult<PlanSuggestOutput>>(`/lessons/${lessonId}/suggestions`, body),
  sendDecision: (interactionId: number, body: DecisionRequest) =>
    post<OkResponse>(`/ai/${interactionId}/decision`, body),
  scaffoldEvent: (body: ScaffoldEventRequest) => post<OkResponse>("/scaffold-events", body),
  listTeacherCases: (q?: string) => get<TeacherCase[]>("/teacher-cases", { q }),

  // 主持人
  admin: {
    createInvite: (body: InviteRequest) => post<InviteResponse>("/admin/invites", body),
    listPrompts: () => get<PromptVersion[]>("/admin/prompts"),
    createPrompt: (body: CreatePromptRequest) => post<PromptVersion>("/admin/prompts", body),
    importKnowledge: (csv: string) =>
      request<ImportResult>("POST", "/admin/knowledge/import", { text: csv, contentType: "text/csv" }),
    importTeacherCases: (csv: string) =>
      request<ImportResult>("POST", "/admin/teacher-cases/import", {
        text: csv,
        contentType: "text/csv",
      }),
  },
};
