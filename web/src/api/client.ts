import { loadToken } from "../lib/storage";
import type {
  AdminSettings,
  AiResult,
  ApiErrorBody,
  AppConfig,
  ClassDetail,
  ClassProfileDefault,
  CoreAssistOutput,
  CoreAssistRequest,
  CoreDraftRequest,
  CoreLinksResponse,
  CoreQuestion,
  CoreVersion,
  CreateClassRequest,
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
  SavedSop,
  SaveSopMaterialRequest,
  SaveLessonVersionRequest,
  SavePreferencesRequest,
  ScaffoldEventRequest,
  SchoolClass,
  SiopFeature,
  SopContent,
  SopCurriculumLink,
  SopJob,
  SopJobKind,
  SopState,
  SuggestionsRequest,
  SyncResponse,
  Teacher,
  TeacherCase,
  Unit,
  UnitMaterial,
  UpdateClassRequest,
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
const put = <T>(path: string, json: unknown, query?: Record<string, string | undefined>) =>
  request<T>("PUT", path, { json, query });
/** `?class_id=` for requests made in a class context. */
const classQuery = (classId?: number | null) => (classId ? { class_id: String(classId) } : undefined);
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
  materialFocuses: (unitId: number, materialId: number, classId?: number | null) =>
    post<AiResult<MaterialFocusesOutput>>(`/units/${unitId}/material-focuses`, {
      material_id: materialId,
      ...(classId ? { class_id: classId } : {}),
    }),

  // 班級與備課庫（Phase 1.4）
  listClasses: (includeArchived = false) =>
    get<SchoolClass[]>("/classes", includeArchived ? { include_archived: "true" } : undefined),
  createClass: (body: CreateClassRequest) => post<SchoolClass>("/classes", body),
  getClass: (classId: number) => get<ClassDetail>(`/classes/${classId}`),
  updateClass: (classId: number, body: UpdateClassRequest) => put<SchoolClass>(`/classes/${classId}`, body),
  /** 封存（不刪除） */
  archiveClass: (classId: number) => post<SchoolClass>(`/classes/${classId}/archive`),
  unarchiveClass: (classId: number) => post<SchoolClass>(`/classes/${classId}/unarchive`),
  reorderClasses: (classIds: number[]) => put<SchoolClass[]>("/classes/order", { class_ids: classIds }),
  /** 從備課庫加入單元（已取用則不重複） */
  assignUnit: (classId: number, unitId: number, note?: string) =>
    post<ClassDetail>(`/classes/${classId}/units`, { unit_id: unitId, ...(note !== undefined ? { note } : {}) }),
  updateClassUnit: (classId: number, unitId: number, note: string) =>
    put<ClassDetail>(`/classes/${classId}/units/${unitId}`, { note }),
  /** 從這個班級移除（單元留在備課庫，上課紀錄保留） */
  unassignUnit: (classId: number, unitId: number) => del<ClassDetail>(`/classes/${classId}/units/${unitId}`),
  reorderClassUnits: (classId: number, unitIds: number[]) =>
    put<ClassDetail>(`/classes/${classId}/units/order`, { unit_ids: unitIds }),

  coreQuestions: () => get<CoreQuestion[]>("/core/questions"),
  getCore: (unitId: number) => get<CoreVersion>(`/units/${unitId}/core`),
  saveCoreDraft: (unitId: number, body: CoreDraftRequest) =>
    put<CoreVersion>(`/units/${unitId}/core/draft`, body),
  completeCore: (unitId: number) => post<CoreVersion>(`/units/${unitId}/core/complete`),
  coreAssist: (body: CoreAssistRequest) => post<AiResult<CoreAssistOutput>>("/core/assist", body),
  listLessons: (unitId: number) => get<Lesson[]>(`/units/${unitId}/lessons`),
  createLesson: (unitId: number, body: CreateLessonRequest) =>
    post<Lesson>(`/units/${unitId}/lessons`, body),
  getLesson: (lessonId: number, classId?: number | null) =>
    get<LessonDetail>(`/lessons/${lessonId}`, classQuery(classId)),
  updateLesson: (lessonId: number, body: UpdateLessonRequest, classId?: number | null) =>
    put<LessonDetail>(`/lessons/${lessonId}`, body, classQuery(classId)),
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

  // 課例 SOP
  getSop: (lessonId: number) => get<SopState>(`/lessons/${lessonId}/sop`),
  /** 自動儲存：內容有變才會多一個版本 */
  saveSop: (lessonId: number, content: SopContent) => put<SavedSop>(`/lessons/${lessonId}/sop`, { content }),
  /** 教材由伺服器分段；段落 id 在文字不變時沿用 */
  saveSopMaterial: (lessonId: number, body: SaveSopMaterialRequest) =>
    put<SopState>(`/lessons/${lessonId}/sop/material`, body),
  /** 教師自己連結課綱條目用的搜尋（不呼叫 AI） */
  searchCurriculum: (q: string) => get<SopCurriculumLink[]>("/curriculum", { q }),
  /** 建立背景工作並立刻回來；之後用 getSopJob 輪詢 */
  startSopJob: (lessonId: number, kind: SopJobKind, classId?: number | null) =>
    post<SopJob>(`/lessons/${lessonId}/sop/${kind}`, classId ? { class_id: classId } : {}),
  getSopJob: (jobId: number) => get<SopJob>(`/sop-jobs/${jobId}`),

  // Teach & Record
  listRuns: (classId?: number | null) => get<LessonRun[]>("/runs", classQuery(classId)),
  listLessonRuns: (lessonId: number) => get<LessonRun[]>(`/lessons/${lessonId}/runs`),
  /** 開始本節課（同一班已有進行中的課 → 409；單元有多個班級而沒帶班級 → 409） */
  startRun: (lessonId: number, classId?: number | null) =>
    post<LessonRun>(`/lessons/${lessonId}/runs`, classId ? { class_id: classId } : {}),
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
