import { loadToken } from "../lib/storage";
import type {
  Account,
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
  CreateAnnotationRequest,
  CreateLessonRequest,
  CreateLibraryMaterialRequest,
  CreateMaterialRequest,
  CreatePromptRequest,
  CreateUnitRequest,
  CurriculumByCodeResult,
  CurriculumQuery,
  CurriculumSearchResult,
  DecisionRequest,
  EmbeddingStatus,
  Health,
  ImportResult,
  InviteRequest,
  InviteResponse,
  KnowledgeImportResult,
  KnowledgeItemRow,
  KnowledgeItemsQuery,
  KnowledgeSummary,
  Lesson,
  LessonDetail,
  LessonRun,
  LessonSiopResponse,
  LessonVersion,
  LoginResponse,
  MaterialAnnotation,
  MaterialDeleted,
  MaterialDetail,
  MaterialFileLink,
  MaterialLibrary,
  MaterialFocusesOutput,
  OkResponse,
  Page,
  PairResponse,
  Pairing,
  PlanSuggestOutput,
  PreferenceQuestionsResponse,
  Preferences,
  PromptVersion,
  RecordEvent,
  RecordOp,
  RemoteRun,
  RetireDemoResult,
  ReviewRequest,
  Role,
  RevokePairingResponse,
  RunDetail,
  SaveLessonVersionRequest,
  SavePreferencesRequest,
  SaveSopMaterialRequest,
  SavedSop,
  ScaffoldEventRequest,
  SchoolClass,
  SiopFeature,
  SopContent,
  SopJob,
  SopJobKind,
  SopState,
  SuggestionsRequest,
  SyncResponse,
  Teacher,
  TeacherCase,
  Unit,
  UnitMaterial,
  UpdateAnnotationRequest,
  UpdateClassRequest,
  UpdateLibraryMaterialRequest,
  UpdateLessonRequest,
  UpdateUnitRequest,
  VocabularyAnalysis,
  VocabularyAnalyzeRequest,
  VocabularyItemsQuery,
  VocabularyRow,
  CreateWorksheetRequest,
  SaveWorksheetMaterialRequest,
  SaveWorksheetRequest,
  SopSyncPreview,
  SopSyncResult,
  WorksheetImages,
  WorksheetState,
  WorksheetSummary,
  WsExportQuery,
  WsJobsState,
  WsLevel,
  WsVocabCheck,
  CreateWsTemplateRequest,
  UpdateWsTemplateRequest,
  WsTemplateDetail,
  MaterialBrowse,
  MaterialBrowseQuery,
  MaterialPurpose,
  TemplateBrowse,
  TemplateBrowseQuery,
  WsTemplateList,
  WsSheet,
  WsVersionRow,
  WsAudience,
  WorksheetComment,
  CreateWorksheetCommentRequest,
  UpdateWorksheetCommentRequest,
  WsProposal,
  WsProposalAccepted,
  ReviseWorksheetBlockRequest,
  PrepSession,
  PrepSessionList,
  CreatePrepSessionRequest,
  ObjectiveDraftRequest,
  ObjectiveDraftResult,
  UpdatePrepSessionRequest,
  CreatePrepTurnRequest,
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
  /** multipart/form-data (file uploads); the browser sets the boundary header itself. */
  form?: FormData;
  query?: Query;
  /** Skip the global 401 handler (used by login and the phone remote). */
  skipAuthHandler?: boolean;
  /** Bearer token to send instead of the teacher's session (phone remote; null = none). */
  token?: string | null;
}

/** Query values; a list repeats the key (`tag=a&tag=b`). Empty values are left out. */
type Query = Record<string, string | string[] | undefined>;

function buildUrl(path: string, query?: Query): string {
  const url = `/api${path}`;
  if (!query) return url;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (Array.isArray(value)) {
      for (const one of value) if (one !== "") params.append(key, one);
    } else if (value !== undefined && value !== "") {
      params.set(key, value);
    }
  }
  const qs = params.toString();
  return qs ? `${url}?${qs}` : url;
}

async function request<T>(method: string, path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: "application/json" };
  const token = options.token !== undefined ? options.token : loadToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  let body: string | FormData | undefined;
  if (options.form !== undefined) {
    body = options.form;
  } else if (options.text !== undefined) {
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

const get = <T>(path: string, query?: Query) =>
  request<T>("GET", path, { query });
const post = <T>(path: string, json?: unknown) => request<T>("POST", path, { json: json ?? {} });
const put = <T>(path: string, json: unknown, query?: Record<string, string | undefined>) =>
  request<T>("PUT", path, { json, query });
/** `?class_id=` for requests made in a class context. */
const classQuery = (classId?: number | null) => (classId ? { class_id: String(classId) } : undefined);
const del = <T>(path: string) => request<T>("DELETE", path);
/** Query values as strings; numbers included, undefined dropped by buildUrl. */
const queryOf = (params: object): Record<string, string | undefined> =>
  Object.fromEntries(
    Object.entries(params as Record<string, unknown>).map(([k, v]) => [k, v === undefined ? undefined : String(v)]),
  );
/** Browse queries: lists repeat their key, `false` is left out (only `fits_lesson=true` is sent). */
const browseQuery = (params: object): Query =>
  Object.fromEntries(
    Object.entries(params as Record<string, unknown>).map(([k, v]) => [
      k,
      v === undefined || v === null || v === false ? undefined : Array.isArray(v) ? v.map(String) : String(v),
    ]),
  );
/** The file name of a download: RFC 5987 `filename*` first, then the plain `filename`. */
export function dispositionFilename(header: string | null, fallback: string): string {
  if (!header) return fallback;
  const star = /filename\*\s*=\s*(?:UTF-8|utf-8)''([^;]+)/.exec(header);
  if (star?.[1]) {
    try {
      return decodeURIComponent(star[1].trim());
    } catch {
      // fall through to the plain name
    }
  }
  const plain = /filename\s*=\s*"?([^";]+)"?/.exec(header);
  return plain?.[1]?.trim() || fallback;
}

/** A binary download with the teacher's token. Errors carry the server's JSON message. */
async function download(
  path: string,
  query: Query,
  fallbackName: string,
): Promise<{ blob: Blob; filename: string }> {
  const headers: Record<string, string> = {};
  const token = loadToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  let response: Response;
  try {
    response = await fetch(buildUrl(path, query), { method: "GET", headers });
  } catch {
    throw new ApiError(0, "network", NETWORK_ERROR_MESSAGE);
  }
  if (!response.ok) {
    const raw = await response.text().catch(() => "");
    let errBody: Partial<ApiErrorBody> | null = null;
    try {
      errBody = raw ? (JSON.parse(raw) as Partial<ApiErrorBody>) : null;
    } catch {
      errBody = null;
    }
    if (response.status === 401 && unauthorizedHandler) unauthorizedHandler();
    throw new ApiError(
      response.status,
      errBody?.error?.code ?? "internal",
      errBody?.error?.message || FALLBACK_MESSAGES[response.status] || SERVER_ERROR_MESSAGE,
    );
  }
  const blob = await response.blob();
  const filename = dispositionFilename(response.headers?.get?.("Content-Disposition") ?? null, fallbackName);
  return { blob, filename };
}

const importCsv = (path: string, csv: string, dryRun: boolean) =>
  request<KnowledgeImportResult>("POST", path, {
    text: csv,
    contentType: "text/csv",
    query: dryRun ? { dry_run: "true" } : undefined,
  });

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
  /** 建立背景工作並立刻回來；之後用 getSopJob 輪詢 */
  startSopJob: (lessonId: number, kind: SopJobKind, classId?: number | null) =>
    post<SopJob>(`/lessons/${lessonId}/sop/${kind}`, classId ? { class_id: classId } : {}),
  getSopJob: (jobId: number) => get<SopJob>(`/sop-jobs/${jobId}`),

  // 學習單產生器（Phase 2.3）
  listWorksheets: (lessonId?: number) =>
    get<WorksheetSummary[]>("/worksheets", lessonId ? { lesson_id: String(lessonId) } : undefined),
  /** 有 lesson_id：從課例帶入；沒有：直接使用 */
  createWorksheet: (body: CreateWorksheetRequest) => post<WorksheetState>("/worksheets", body),
  getWorksheet: (id: number) => get<WorksheetState>(`/worksheets/${id}`),
  /** 自動儲存：content 有變才多一個版本 */
  saveWorksheet: (id: number, body: SaveWorksheetRequest) => put<WorksheetState>(`/worksheets/${id}`, body),
  saveWorksheetMaterial: (id: number, body: SaveWorksheetMaterialRequest) =>
    put<WorksheetState>(`/worksheets/${id}/material`, body),
  deleteWorksheet: (id: number) => del<OkResponse>(`/worksheets/${id}`),
  /** 建立背景工作並立刻回來；之後用 worksheetJobs 輪詢 */
  generateWorksheet: (id: number) => post<WorksheetState>(`/worksheets/${id}/generate`),
  worksheetJobs: (id: number) => get<WsJobsState>(`/worksheets/${id}/jobs`),
  regenerateWorksheetBlock: (id: number, blockId: string) =>
    post<WorksheetState>(`/worksheets/${id}/blocks/${encodeURIComponent(blockId)}/regenerate`),
  undoWorksheetBlock: (id: number, blockId: string) =>
    post<WorksheetState>(`/worksheets/${id}/blocks/${encodeURIComponent(blockId)}/undo`),
  // 逐區修改與註記
  listWorksheetComments: (id: number) => get<WorksheetComment[]>(`/worksheets/${id}/comments`),
  createWorksheetComment: (id: number, body: CreateWorksheetCommentRequest) =>
    post<WorksheetComment>(`/worksheets/${id}/comments`, body),
  updateWorksheetComment: (id: number, commentId: number, body: UpdateWorksheetCommentRequest) =>
    request<WorksheetComment>("PATCH", `/worksheets/${id}/comments/${commentId}`, { json: body }),
  deleteWorksheetComment: (id: number, commentId: number) =>
    del<{ deleted: boolean }>(`/worksheets/${id}/comments/${commentId}`),
  /** 202：在背景改這一區（status running），之後輪詢 getWorksheetProposal */
  reviseWorksheetBlock: (id: number, blockId: string, body: ReviseWorksheetBlockRequest) =>
    post<WsProposal>(`/worksheets/${id}/blocks/${encodeURIComponent(blockId)}/revise`, body),
  /** 每一區還在等教師決定的那一個（running、proposed、failed、stale） */
  listWorksheetProposals: (id: number) => get<WsProposal[]>(`/worksheets/${id}/proposals`),
  getWorksheetProposal: (id: number, proposalId: number) =>
    get<WsProposal>(`/worksheets/${id}/proposals/${proposalId}`),
  /** 採用：寫成新版本；區塊之後被改過 → 409 */
  acceptWorksheetProposal: (id: number, proposalId: number) =>
    post<WsProposalAccepted>(`/worksheets/${id}/proposals/${proposalId}/accept`),
  /** 放棄（failed／stale 時是「知道了」） */
  rejectWorksheetProposal: (id: number, proposalId: number) =>
    post<WsProposal>(`/worksheets/${id}/proposals/${proposalId}/reject`),
  worksheetSopSyncPreview: (id: number) => get<SopSyncPreview>(`/worksheets/${id}/sop-sync`),
  /** 只有教師按了確認才呼叫 */
  applyWorksheetSopSync: (id: number, keys: string[], sopVersionNo: number | null) =>
    post<SopSyncResult>(`/worksheets/${id}/sop-sync`, { confirm: true, keys, sop_version_no: sopVersionNo }),
  linkWorksheet: (id: number, lessonId: number | null) =>
    put<WorksheetState>(`/worksheets/${id}/link`, { lesson_id: lessonId }),
  saveWorksheetAsLesson: (id: number, unitId: number, title: string) =>
    post<WorksheetState>(`/worksheets/${id}/save-as-lesson`, { unit_id: unitId, title }),
  checkWorksheetVocabulary: (id: number, level: WsLevel, targetLevel?: number) =>
    post<WsVocabCheck>(`/worksheets/${id}/checks/vocabulary`, {
      level,
      ...(targetLevel !== undefined ? { target_level: targetLevel } : {}),
    }),
  worksheetChecks: (id: number) => get<WsVocabCheck[]>(`/worksheets/${id}/checks`),
  /** 後端即時產生的檔案；回傳 Blob 與檔名 */
  /** 學生版／教師版的投影（唯讀預覽用）；不給 level 時伺服器用第一個版本 */
  worksheetSheet: (id: number, audience: WsAudience, level?: WsLevel) =>
    get<WsSheet>(`/worksheets/${id}/sheet`, { level, audience }),
  /** 學習單的圖片：各圖片位置目前的圖 */
  worksheetImages: (id: number) => get<WorksheetImages>(`/worksheets/${id}/images`),
  /** 一張存好的圖（JPEG；帶老師的權杖） */
  worksheetImage: (id: number, imageId: number) =>
    download(`/worksheets/${id}/images/${imageId}`, {}, "picture.jpg").then((r) => r.blob),
  /** 重畫（可附一句說明，≤200 字）→ 202 */
  regenerateWorksheetImage: (id: number, slotId: string, hint?: string) =>
    post<WorksheetImages>(
      `/worksheets/${id}/image-slots/${encodeURIComponent(slotId)}/regenerate`,
      hint?.trim() ? { hint: hint.trim() } : {},
    ),
  /** 換成我的圖（png、jpg、webp、gif） */
  uploadWorksheetImage: (id: number, slotId: string, file: File) => {
    const form = new FormData();
    form.append("file", file);
    return request<WorksheetImages>("POST", `/worksheets/${id}/image-slots/${encodeURIComponent(slotId)}/upload`, {
      form,
    });
  },
  /** 拿掉這張圖 */
  removeWorksheetImage: (id: number, slotId: string) =>
    del<WorksheetImages>(`/worksheets/${id}/image-slots/${encodeURIComponent(slotId)}`),
  worksheetVersions: (id: number) => get<WsVersionRow[]>(`/worksheets/${id}/versions`),
  exportWorksheet: (id: number, query: WsExportQuery) =>
    download(`/worksheets/${id}/export`, { ...query }, `worksheet.${query.format}`),
  /** 設定或拿掉版型；伺服器依版型重設 settings（拿掉時 settings 不動） */
  setWorksheetTemplate: (id: number, templateId: number | null) =>
    put<WorksheetState>(`/worksheets/${id}`, { template_id: templateId }),

  // ---- 我的學習單版型（docs/WORKSHEET_TEMPLATES_CONTRACT.md）
  /** 新的在前 */
  // 備課對話（prep sessions）
  listPrepSessions: () => get<PrepSessionList>("/prep-sessions"),
  /** 201：第一個 turn 已開始 */
  createPrepSession: (body: CreatePrepSessionRequest) => post<PrepSession>("/prep-sessions", body),
  /** 兩個目標都沒寫時，產生前先請 Manna 草擬（SIOP G2、G3） */
  draftObjectives: (body: ObjectiveDraftRequest) => post<ObjectiveDraftResult>("/objective-drafts", body),
  getPrepSession: (id: number) => get<PrepSession>(`/prep-sessions/${id}`),
  updatePrepSession: (id: number, body: UpdatePrepSessionRequest) =>
    request<PrepSession>("PATCH", `/prep-sessions/${id}`, { json: body }),
  /** 硬刪除對話；做出來的學習單保留 */
  deletePrepSession: (id: number) => del<OkResponse>(`/prep-sessions/${id}`),
  /** 202；另一個 turn 進行中 → 409 */
  createPrepTurn: (id: number, body: CreatePrepTurnRequest) =>
    post<PrepSession>(`/prep-sessions/${id}/turns`, body),
  /** The URL of a 教學套件 turn's single DOCX (教學流程, each level's student version, 教師版). */
  prepKitPath: (id: number, turnId: number) => `/api/prep-sessions/${id}/turns/${turnId}/kit?format=docx`,
  /** 409 while the kit is still being made; 400 for a turn that is not a kit. */
  exportPrepKit: (id: number, turnId: number) =>
    download(`/prep-sessions/${id}/turns/${turnId}/kit`, { format: "docx" }, "教學套件.docx"),

  listWorksheetTemplates: () => get<WsTemplateList>("/worksheet-templates"),
  /** 202：在背景讀這份學習單的設計（status=extracting），之後輪詢 */
  createWorksheetTemplate: (body: CreateWsTemplateRequest) => post<WsTemplateDetail>("/worksheet-templates", body),
  getWorksheetTemplate: (id: number) => get<WsTemplateDetail>(`/worksheet-templates/${id}`),
  /** 有 content 就存成新的一版；讀取中 409 */
  updateWorksheetTemplate: (id: number, body: UpdateWsTemplateRequest) =>
    put<WsTemplateDetail>(`/worksheet-templates/${id}`, body),
  /** 只有 failed 可以重來 */
  retryWorksheetTemplate: (id: number) => post<WsTemplateDetail>(`/worksheet-templates/${id}/retry`),
  /** 永久刪除版型與所有版本 */
  deleteWorksheetTemplate: (id: number) => del<OkResponse>(`/worksheet-templates/${id}`),
  /** 平台範本另存成我的範本（scope personal、標題加「（我的版本）」），回傳新的那一份 */
  copyWorksheetTemplate: (id: number) => post<WsTemplateDetail>(`/worksheet-templates/${id}/copy`),

  // ---- 教材參考、我的教材、學習單範本（Phase 2.9 瀏覽）
  /** area=reference：知識庫的教材；area=mine：我的教材。分面只依 area 與 q 計算 */
  browseMaterials: (query: MaterialBrowseQuery) => get<MaterialBrowse>("/browse/materials", browseQuery(query)),
  /** area=platform：平台範本（ready）；area=mine：我的範本（任何狀態） */
  browseTemplates: (query: TemplateBrowseQuery) => get<TemplateBrowse>("/browse/templates", browseQuery(query)),

  // ---- 教材庫與註記（Phase 2.2）
  /** 自己的教材、用量與上傳限制；帶 lessonId 時只列連到那個課例的教材 */
  listLibraryMaterials: (lessonId?: number) =>
    get<MaterialLibrary>("/materials", lessonId ? { lesson_id: String(lessonId) } : undefined),
  getLibraryMaterial: (id: number) => get<MaterialDetail>(`/materials/${id}`),
  /** 貼上文字或加入網址 */
  createLibraryMaterial: (body: CreateLibraryMaterialRequest) => post<MaterialDetail>("/materials", body),
  /** 上傳檔案（multipart）；後端檢查副檔名、內容、大小與用量 */
  uploadLibraryMaterial: (
    file: File,
    fields: { title?: string; source_note?: string; lesson_id?: number; purpose?: MaterialPurpose } = {},
  ) => {
    const form = new FormData();
    if (fields.title) form.append("title", fields.title);
    if (fields.source_note) form.append("source_note", fields.source_note);
    if (fields.lesson_id) form.append("lesson_id", String(fields.lesson_id));
    // 學習單範本（照我的學習單做、學習單範本頁）：不列進我的教材
    if (fields.purpose) form.append("purpose", fields.purpose);
    form.append("file", file, file.name);
    return request<MaterialDetail>("POST", "/materials/upload", { form });
  },
  updateLibraryMaterial: (id: number, body: UpdateLibraryMaterialRequest) =>
    put<MaterialDetail>(`/materials/${id}`, body),
  /** 永久刪除：原檔、擷取文字、段落與註記一併刪除 */
  deleteLibraryMaterial: (id: number) => del<MaterialDeleted>(`/materials/${id}`),
  /**
   * 用 OCR 重新讀取（Phase 2.10）：202 回來時 ocr.status = "queued"。辨識中 → 409；不是上傳的 PDF
   * 或圖片 → 400；這個環境沒有開啟 OCR → 503。自己的教材，或管理員的平台教材
   */
  runMaterialOcr: (id: number) => post<MaterialDetail>(`/materials/${id}/ocr`),
  /** 給 img／audio／video 用的短時效連結（指向本站 API，不是儲存空間） */
  materialFileLink: (id: number) => post<MaterialFileLink>(`/materials/${id}/file-link`),
  /** 知識庫的預覽圖：上傳的 PDF 第 1 頁或圖片，480 px 寬的 JPEG；其他教材 404 */
  materialThumbnail: (id: number) => download(`/materials/${id}/thumbnail`, {}, "thumbnail.jpg").then((r) => r.blob),
  listAnnotations: (materialId: number) => get<MaterialAnnotation[]>(`/materials/${materialId}/annotations`),
  createAnnotation: (materialId: number, body: CreateAnnotationRequest) =>
    post<MaterialAnnotation>(`/materials/${materialId}/annotations`, body),
  updateAnnotation: (id: number, body: UpdateAnnotationRequest) => put<MaterialAnnotation>(`/annotations/${id}`, body),
  deleteAnnotation: (id: number) => del<{ deleted: boolean }>(`/annotations/${id}`),

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

  // 知識庫與詞彙（Phase 2.0）：教師查詢
  /** 課綱條目：依關鍵字排序；沒有關鍵字時依代碼列出。預設高中、英語相關科目。 */
  searchCurriculum: (query: CurriculumQuery = {}) =>
    get<CurriculumSearchResult>("/knowledge/curriculum", queryOf(query)),
  /** 依代碼取官方條文與來源；查不到的在 not_found，不可自行補上條文。 */
  curriculumByCode: (codes: string[], track?: string) =>
    get<CurriculumByCodeResult>("/knowledge/curriculum/by-code", { codes: codes.join(","), track }),
  /** 詞彙級別分析（規則比對，無 AI）。 */
  analyzeVocabulary: (body: VocabularyAnalyzeRequest) => post<VocabularyAnalysis>("/vocabulary/analyze", body),

  // 知識庫管理（主持人或知識庫管理員）
  knowledge: {
    summary: () => get<KnowledgeSummary>("/admin/knowledge/summary"),
    importItems: (csv: string, dryRun: boolean) => importCsv("/admin/knowledge/import", csv, dryRun),
    importVocabulary: (csv: string, dryRun: boolean) => importCsv("/admin/vocabulary/import", csv, dryRun),
    /** 語意索引的涵蓋率與最近一次建立 */
    embeddings: () => get<EmbeddingStatus>("/admin/knowledge/embeddings"),
    /** 在背景建立語意索引（只補缺少或過期的；force 全部重做），回 202 */
    rebuildEmbeddings: (force = false) =>
      post<EmbeddingStatus>("/admin/knowledge/embeddings/rebuild", { force }),
    retireDemo: (dryRun: boolean) =>
      request<RetireDemoResult>("POST", "/admin/knowledge/retire-demo", { query: { dry_run: String(dryRun) } }),
    items: (query: KnowledgeItemsQuery = {}) =>
      get<Page<KnowledgeItemRow>>("/admin/knowledge/items", queryOf(query)),
    vocabulary: (query: VocabularyItemsQuery = {}) =>
      get<Page<VocabularyRow>>("/admin/vocabulary/items", queryOf(query)),
  },

  // 主持人
  admin: {
    getSettings: () => get<AdminSettings>("/admin/settings"),
    updateSettings: (body: AdminSettings) => put<AdminSettings>("/admin/settings", body),
    createInvite: (body: InviteRequest) => post<InviteResponse>("/admin/invites", body),
    /** 帳號與角色：未刪除的帳號，最早加入的在前 */
    listAccounts: () => get<Account[]>("/admin/accounts"),
    /** 變更角色；對方下一次操作時生效。自己的角色、最後一位主持人 → 409 */
    updateAccountRole: (id: number, role: Role) =>
      request<Account>("PATCH", `/admin/accounts/${id}`, { json: { role } }),
    listPrompts: () => get<PromptVersion[]>("/admin/prompts"),
    createPrompt: (body: CreatePromptRequest) => post<PromptVersion>("/admin/prompts", body),
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

// =============================================================================================
// 教材與範本管理（admin library）— agent F2. ManageKnowledge only; others get 403.
// docs/API.md「Phase 2.9：教材參考、我的教材、學習單範本」, "Admin APIs".
// =============================================================================================

// Imports are hoisted; kept here so this section merges on its own.
import type {
  AdminLibraryList,
  AdminLibraryMaterial,
  AdminLibraryMaterialRow,
  AdminLibraryQuery,
  AdminLibraryTemplate,
  AdminLibraryTemplateRow,
  AdminLibraryUpdate,
  AdminLibraryUploadFields,
} from "./types";

/** Multipart body of an admin upload: the fields first, then the file (as the teacher upload). */
function adminLibraryForm(file: File, fields: AdminLibraryUploadFields): FormData {
  const form = new FormData();
  if (fields.title) form.append("title", fields.title);
  if (fields.category) form.append("category", fields.category);
  if (fields.tags && fields.tags.length > 0) form.append("tags", fields.tags.join(","));
  if (fields.source_note) form.append("source_note", fields.source_note);
  form.append("file", file, file.name);
  return form;
}

/** The list as an envelope even if the server answers with a bare array. */
const adminLibraryList = <T>(res: AdminLibraryList<T> | T[]): AdminLibraryList<T> =>
  Array.isArray(res) ? { items: res, total: res.length, next_cursor: null } : res;

export const adminLibraryApi = {
  listMaterials: (query: AdminLibraryQuery = {}) =>
    get<AdminLibraryList<AdminLibraryMaterialRow>>("/admin/library", { kind: "materials", ...query }).then(
      adminLibraryList,
    ),
  listTemplates: (query: AdminLibraryQuery = {}) =>
    get<AdminLibraryList<AdminLibraryTemplateRow>>("/admin/library", { kind: "templates", ...query }).then(
      adminLibraryList,
    ),
  /** A platform material (purpose material), extracted as a teacher upload is. */
  uploadMaterial: (file: File, fields: AdminLibraryUploadFields) =>
    request<AdminLibraryMaterial>("POST", "/admin/library", { form: adminLibraryForm(file, fields) }),
  /** A platform sample and its template; the template comes back with status extracting. */
  uploadTemplate: (file: File, fields: AdminLibraryUploadFields) =>
    request<AdminLibraryTemplate>("POST", "/admin/library/templates", { form: adminLibraryForm(file, fields) }),
  updateMaterial: (id: number, body: AdminLibraryUpdate) =>
    request<AdminLibraryMaterial>("PATCH", `/admin/library/${id}`, { json: body }),
  updateTemplate: (id: number, body: AdminLibraryUpdate) =>
    request<AdminLibraryTemplate>("PATCH", `/admin/library/templates/${id}`, { json: body }),
  /** Hard delete; a template made from it keeps working (its source_material becomes null). */
  deleteMaterial: (id: number) => del<unknown>(`/admin/library/${id}`),
  deleteTemplate: (id: number) => del<unknown>(`/admin/library/templates/${id}`),
  retryTemplate: (id: number) => post<AdminLibraryTemplate>(`/admin/library/templates/${id}/retry`),
};
