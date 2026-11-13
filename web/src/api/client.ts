import { loadToken } from "../lib/storage";
import type {
  AdminSettings,
  AiResult,
  ApiErrorBody,
  AppConfig,
  ClassProfileDefault,
  CoreAssistOutput,
  CoreAssistRequest,
  CoreDraftRequest,
  CoreLinksResponse,
  CoreQuestion,
  CoreVersion,
  CreateLessonRequest,
  CreateMaterialRequest,
  CreatePromptRequest,
  CreateUnitRequest,
  DecisionRequest,
  Health,
  ImportResult,
  InviteRequest,
  InviteResponse,
  Lesson,
  LessonDetail,
  LessonRun,
  LessonSiopResponse,
  LessonVersion,
  LoginResponse,
  MaterialFocusesOutput,
  OkResponse,
  PairResponse,
  Pairing,
  PlanSuggestOutput,
  PreferenceQuestionsResponse,
  Preferences,
  PromptVersion,
  RecordEvent,
  RecordOp,
  RemoteRun,
  ReviewRequest,
  RevokePairingResponse,
  RunDetail,
  SaveLessonVersionRequest,
  SavePreferencesRequest,
  ScaffoldEventRequest,
  SiopFeature,
  SuggestionsRequest,
  SyncResponse,
  Teacher,
  TeacherCase,
  Unit,
  UnitMaterial,
  UpdateLessonRequest,
  UpdateUnitRequest,
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
  /** Skip the global 401 handler (used by login and the phone remote). */
  skipAuthHandler?: boolean;
  /** Bearer token to send instead of the teacher's session (phone remote; null = none). */
  token?: string | null;
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
  const token = options.token !== undefined ? options.token : loadToken();
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
const del = <T>(path: string) => request<T>("DELETE", path);

export const api = {
  // 公開
  health: () => get<Health>("/health"),
  config: () => get<AppConfig>("/config"),
  login: (code: string) =>
    request<LoginResponse>("POST", "/auth/login", { json: { code }, skipAuthHandler: true }),
  logout: () => request<OkResponse>("POST", "/auth/logout", { json: {}, skipAuthHandler: true }),

  // 教師
  me: () => get<Teacher>("/me"),
  preferenceQuestions: () => get<PreferenceQuestionsResponse>("/preferences/questions"),
  /** null＝尚未做初次設定 */
  myPreferences: () => get<Preferences | null>("/me/preferences"),
  savePreferences: (body: SavePreferencesRequest) => put<Preferences>("/me/preferences", body),
  listUnits: () => get<Unit[]>("/units"),
  createUnit: (body: CreateUnitRequest) => post<Unit>("/units", body),
  classProfileDefault: () => get<ClassProfileDefault>("/units/class-profile-default"),
  getUnit: (unitId: number) => get<Unit>(`/units/${unitId}`),
  updateUnit: (unitId: number, body: UpdateUnitRequest) => put<Unit>(`/units/${unitId}`, body),
  listMaterials: (unitId: number) => get<UnitMaterial[]>(`/units/${unitId}/materials`),
  createMaterial: (unitId: number, body: CreateMaterialRequest) =>
    post<UnitMaterial>(`/units/${unitId}/materials`, body),
  /** 實際刪除原文與切塊 */
  deleteMaterial: (unitId: number, materialId: number) =>
    del<OkResponse>(`/units/${unitId}/materials/${materialId}`),
  materialFocuses: (unitId: number, materialId: number) =>
    post<AiResult<MaterialFocusesOutput>>(`/units/${unitId}/material-focuses`, { material_id: materialId }),
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
  updateLesson: (lessonId: number, body: UpdateLessonRequest) =>
    put<LessonDetail>(`/lessons/${lessonId}`, body),
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

  // Teach & Record
  listRuns: () => get<LessonRun[]>("/runs"),
  listLessonRuns: (lessonId: number) => get<LessonRun[]>(`/lessons/${lessonId}/runs`),
  /** 開始本節課（已有進行中的課 → 409） */
  startRun: (lessonId: number) => post<LessonRun>(`/lessons/${lessonId}/runs`),
  getRun: (runId: number) => get<RunDetail>(`/runs/${runId}`),
  /** 結束本節課並回顧 */
  endRun: (runId: number) => post<LessonRun>(`/runs/${runId}/end`),
  syncRunEvents: (runId: number, ops: RecordOp[]) => post<SyncResponse>(`/runs/${runId}/events/sync`, { ops }),
  reviewEvent: (runId: number, deviceEventId: string, body: ReviewRequest) =>
    put<RecordEvent>(`/runs/${runId}/events/${encodeURIComponent(deviceEventId)}/review`, body),
  completeReview: (runId: number) => post<LessonRun>(`/runs/${runId}/review/complete`),
  createPairing: (runId: number) => post<Pairing>(`/runs/${runId}/pairing`),
  /** 撤銷未使用的配對碼與已配對的遙控器 */
  revokePairing: (runId: number) => del<RevokePairingResponse>(`/runs/${runId}/pairing`),

  // 主持人
  admin: {
    getSettings: () => get<AdminSettings>("/admin/settings"),
    updateSettings: (body: AdminSettings) => put<AdminSettings>("/admin/settings", body),
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

/** Phone remote: sends its run-scoped token (never the teacher session) and never logs anyone out. */
export const remoteApi = {
  pair: (code: string, deviceType: string) =>
    request<PairResponse>("POST", "/remote/pair", {
      json: { code, device_type: deviceType },
      token: null,
      skipAuthHandler: true,
    }),
  getRun: (token: string) => request<RemoteRun>("GET", "/remote/run", { token, skipAuthHandler: true }),
  sync: (token: string, ops: RecordOp[]) =>
    request<SyncResponse>("POST", "/remote/events/sync", { json: { ops }, token, skipAuthHandler: true }),
};
