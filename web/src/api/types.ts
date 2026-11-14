// Mirrors docs/API.md (Manna DLS API 合約，Phase 1 + 1.1 + 1.2). Keep in sync with the contract.

export type SchoolTrack = "general" | "vocational";
export type Role = "teacher" | "pi" | "ra" | "rater" | "engineer";
export type QuestionId = "Q1" | "Q2" | "Q3" | "Q4" | "Q5" | "Q6" | "Q7" | "Q8";
export type ResponseStatus = "filled" | "blank" | "no_idea";
export type ContentSource = "self" | "self_after_ai" | "modified_ai_example" | "adopted_ai_example";
export type IcapLevel = "passive" | "active" | "constructive" | "interactive";

export const QUESTION_IDS: readonly QuestionId[] = ["Q1", "Q2", "Q3", "Q4", "Q5", "Q6", "Q7", "Q8"];

// ---- Phase 1.1：初次使用設定與備課菜單 ----

/** 目標／教材／學生／活動優先 */
export type PrepRoute = "goal" | "material" | "student" | "activity";
export type Hardest = "goal" | "material" | "differentiation" | "activity" | "worksheet" | "ai_communication";
export type PreferenceQuestionId = "P1" | "P2" | "P3" | "P4" | "P5" | "P6";

export interface PreferenceAnswers {
  experience: "0-2" | "3-9" | "10+" | null; // P1
  start_from: PrepRoute | null; // P2
  hardest: Hardest[]; // P3（可空陣列）
  ai_experience: "none" | "sometimes" | "often" | null; // P4
  detail: "concise" | "medium" | "detailed" | null; // P5
  class_profile: string; // P6（可空字串，不含學生個資）
}

export type PreferenceSource = "onboarding" | "settings";

export interface Preferences {
  version_no: number;
  question_set_version: number;
  answers: PreferenceAnswers;
  skipped: boolean;
  source: PreferenceSource;
  created_at: string;
}

export interface PreferenceOption {
  value: string;
  label: string;
}

export interface PreferenceQuestion {
  id: PreferenceQuestionId;
  prompt: string;
  options: PreferenceOption[];
  multi: boolean;
}

export interface PreferenceQuestionsResponse {
  question_set_version: number;
  questions: PreferenceQuestion[];
}

export interface SavePreferencesRequest {
  answers: PreferenceAnswers;
  skipped: boolean;
  source: PreferenceSource;
}

export interface Teacher {
  id: number;
  research_id: string;
  school_track: SchoolTrack;
  role: Role;
}

export interface Unit {
  id: number;
  title: string;
  created_at: string;
  core_status: "none" | "draft" | "completed";
  lesson_count: number;
  prep_route: PrepRoute;
  class_profile: string;
  /** Where the class profile came from when the unit was created; null for older units. */
  class_profile_source: ClassProfileSource | null;
  /** 取用這個單元、未封存的班級（依教師的班級順序）。 */
  classes: UnitClassRef[];
}

/** 取用某個單元的班級；note＝這個取用的「這班的備註」。 */
export interface UnitClassRef {
  id: number;
  name: string;
  course_label: string;
  class_profile: string;
  note: string;
}

// ---- Phase 1.4：班級與備課庫 ----

export type ClassProfileOrigin = "teacher" | "preference" | "migration" | "none";

export type ClassNextAction =
  | { kind: "resume_run"; run_id: number; lesson_id: number; lesson_title: string; unit_id: number; unit_title: string }
  | { kind: "start_lesson"; lesson_id: number; lesson_title: string; unit_id: number; unit_title: string }
  | { kind: "plan_unit"; unit_id: number; unit_title: string }
  | { kind: "add_unit" };

export interface SchoolClass {
  id: number;
  name: string;
  grade: string;
  school_track: SchoolTrack;
  course_label: string;
  class_profile: string;
  class_profile_source: ClassProfileOrigin;
  school_year: string;
  term: string;
  position: number;
  created_at: string;
  /** null＝使用中 */
  archived_at: string | null;
  unit_count: number;
  /** null＝取用的單元都上完了 */
  next_action: ClassNextAction | null;
}

export interface ClassUnit {
  unit: Unit;
  position: number;
  note: string;
  assigned_at: string;
  lessons_total: number;
  /** 在這個班級至少上過一次的課例數 */
  lessons_taught: number;
  last_run_at: string | null;
  next_lesson: { id: number; title: string } | null;
}

export interface ClassDetail extends SchoolClass {
  units: ClassUnit[];
}

export interface CreateClassRequest {
  name: string;
  grade?: string;
  school_track?: SchoolTrack;
  course_label?: string;
  class_profile?: string;
  school_year?: string;
  term?: string;
}

export type UpdateClassRequest = Partial<CreateClassRequest>;

/** effective_class_profile 取自哪裡 */
export type ClassProfileSourceUsed = "lesson" | "class" | "unit" | "preference" | "none";

/** teacher = sent on create; previous_unit / preference = carried-over default; none = empty. */
export type ClassProfileSource = "teacher" | "previous_unit" | "preference" | "none";

/** GET /api/units/class-profile-default: what a new unit gets when no class_profile is sent. */
export interface ClassProfileDefault {
  class_profile: string;
  source: Exclude<ClassProfileSource, "teacher">;
  source_unit: { id: number; title: string } | null;
}

export interface CoreResponse {
  answer: string;
  status: ResponseStatus;
  content_source: ContentSource;
  updated_at: string | null;
}

export interface CoreFocus {
  id: number;
  position: number;
  big_idea: string;
  responses: Record<QuestionId, CoreResponse>;
}

export interface CoreVersion {
  id: number;
  unit_id: number;
  phase: "pre" | "post";
  version_no: number;
  completed_at: string | null;
  saw_ai_assist: boolean;
  focuses: CoreFocus[];
}

export interface CoreQuestion {
  id: QuestionId;
  prompt: string;
  help: string;
}

export interface LessonStep {
  position: number;
  activity_type: string;
  icap_level: IcapLevel | null;
  content: string;
  duration_min: number | null;
  support_note: string;
  extension_note: string;
}

export interface LessonVersion {
  id: number;
  version_no: number;
  objective: string;
  steps: LessonStep[];
  created_at: string;
}

export interface Lesson {
  id: number;
  unit_id: number;
  title: string;
  lesson_date: string | null;
  class_description: string;
  created_at: string;
  /** 我的範例（自己的舊教案或學習單文字），只存在該課例 */
  my_example: string;
}

export interface LessonDetail extends Lesson {
  latest_version: LessonVersion | null;
  versions_count: number;
  core_focus_ids: number[];
  siop_feature_ids: number[];
  previous_next_change: string | null;
  /** 課例 class_description 非空時用它，否則（帶 class_id 時）班級的班級特性，否則單元 class_profile */
  effective_class_profile: string;
  /** effective_class_profile 取自哪裡 */
  class_profile_source?: ClassProfileSourceUsed;
}

export interface Source {
  item_code: string;
  title: string;
  category: string;
  source_doc: string;
  source_page: string;
}

export type AiStatus = "shown" | "blocked" | "timeout" | "error" | "disabled";

/** 「Manna 已帶入的資訊」 */
export interface AiContextItem {
  label: string;
  preview: string;
}

export interface AiResult<T> {
  interaction_id: number | null;
  status: AiStatus;
  message: string | null;
  output: T | null;
  sources: Source[];
  context: AiContextItem[];
}

export interface CoreAssistOutput {
  level: 1 | 2 | 3;
  text: string;
}

export interface PlanSuggestion {
  title: string;
  body: string;
  activity_type: string | null;
  icap_level: IcapLevel | null;
  citations: string[];
  duration_min: number | null;
  support_note: string | null;
  extension_note: string | null;
}

export interface PlanSuggestOutput {
  kb_empty: boolean;
  suggestions: PlanSuggestion[];
}

/** 教材優先：AI 從教材提出的候選學習重點；excerpt 逐字出自教師貼上的教材 */
export interface MaterialCandidate {
  big_idea: string;
  rationale: string;
  excerpt: string;
}

export interface MaterialFocusesOutput {
  candidates: MaterialCandidate[];
}

export interface UnitMaterial {
  id: number;
  filename: string;
  char_count: number;
  created_at: string;
}

// ---- Request / response shapes for endpoints ----

export interface Health {
  ok: boolean;
  db: boolean;
}

export interface AppConfig {
  demo_mode: boolean;
  ai_enabled: boolean;
  ai_provider: string;
  ai_model: string;
  route_choice_enabled: boolean;
}

export interface LoginRequest {
  code: string;
}

export interface LoginResponse {
  token: string;
  teacher: Teacher;
}

export interface OkResponse {
  ok: true;
}

export interface CreateUnitRequest {
  title: string;
  prep_route?: PrepRoute;
  class_profile?: string;
  /** 建在備課庫，同時由這個班級取用 */
  class_id?: number;
}

export interface UpdateUnitRequest {
  title?: string;
  prep_route?: PrepRoute;
  class_profile?: string;
}

export interface UpdateLessonRequest {
  title?: string;
  /** null clears the date */
  lesson_date?: string | null;
  class_description?: string;
  my_example?: string;
}

export interface CreateMaterialRequest {
  text: string;
  filename?: string;
}

export interface MaterialFocusesRequest {
  material_id: number;
}

export interface AdminSettings {
  route_choice_enabled: boolean;
}

export interface CoreDraftResponse {
  answer: string;
  status: ResponseStatus;
  content_source: ContentSource;
}

export interface CoreDraftFocus {
  id?: number;
  position: number;
  big_idea: string;
  responses: Partial<Record<QuestionId, CoreDraftResponse>>;
}

export interface CoreDraftRequest {
  focuses: CoreDraftFocus[];
}

export type AssistLevel = 1 | 2 | 3;

export interface CoreAssistRequest {
  unit_id: number;
  focus_id: number;
  question_id: QuestionId;
  level: AssistLevel;
  class_id?: number;
}

export interface CreateLessonRequest {
  title: string;
  lesson_date?: string;
  class_description?: string;
  my_example?: string;
}

export interface SaveLessonVersionRequest {
  objective: string;
  steps: LessonStep[];
}

export interface CoreLinksRequest {
  focus_ids: number[];
}

export interface CoreLinksResponse {
  focus_ids: number[];
}

export interface SiopFeature {
  id: number;
  internal_id: string;
  component: string;
  summary: string;
}

export interface LessonSiopRequest {
  siop_feature_ids: number[];
}

export interface LessonSiopResponse {
  siop_feature_ids: number[];
}

export interface SuggestionsRequest {
  request_note?: string;
  class_id?: number;
}

export type DecisionKind = "adopt" | "revise" | "reject" | "answer" | "skip";
export type DecisionReason =
  | "source_problem"
  | "curriculum_mismatch"
  | "not_feasible"
  | "unclear"
  | "better_idea"
  | "other";
export type Thumbs = "up" | "down";

export interface DecisionRequest {
  item_index?: number;
  decision: DecisionKind;
  revised_content?: string;
  reason?: DecisionReason;
  reason_note?: string;
  thumbs?: Thumbs;
}

export type ScaffoldCode =
  | "SC-CORE-HELP"
  | "SC-CORE"
  | "SC-RAG"
  | "SC-SIOP"
  | "SC-CASES"
  | "SC-MATERIAL"
  | "SC-PREF"
  | "SC-CONTEXT"
  | "SC-RECORD";
export type ScaffoldAction = "show" | "expand" | "use" | "skip";

export interface ScaffoldEventRequest {
  scaffold_code: ScaffoldCode;
  action: ScaffoldAction;
  trigger?: string;
  dwell_ms?: number;
}

export interface TeacherCase {
  case_code: string;
  school_track: "general" | "vocational" | "both";
  grade: string;
  topic: string;
  summary: string;
  details: string;
}

export interface InviteRequest {
  research_id: string;
  school_track: SchoolTrack;
  role?: Role;
}

export interface InviteResponse {
  code: string;
}

export interface PromptVersion {
  id: number;
  prompt_key: string;
  version_no: number;
  system_text: string;
  user_template: string;
  created_at: string;
}

export interface CreatePromptRequest {
  prompt_key: string;
  system_text: string;
  user_template: string;
}

export interface ImportResult {
  imported: number;
  sha256: string;
}

export type ApiErrorCode =
  | "bad_request"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "internal";

export interface ApiErrorBody {
  error: { code: ApiErrorCode; message: string };
}

// ---- Phase 1.2：Teach & Record（課中書籤與課後補寫） ----

/** 👍 順利 | ⚠ 卡住 */
export type Polarity = "good" | "stuck";
export type DeviceType = "phone" | "tablet" | "desktop";
/** 課堂紀錄頁 | 子母畫面小視窗 | 手機遙控器 */
export type RecordVia = "console" | "pip" | "remote";
/** 學生可觀察行為（可複選） */
export type StudentBehavior =
  | "listening"
  | "note_taking"
  | "answering"
  | "asking"
  | "explaining"
  | "discussing"
  | "probing"
  | "co_completing"
  | "other";
/** 處理方式（單選，可不選） */
export type Handling = "re_explain" | "add_example" | "adjust_questions" | "change_activity" | "adjust_time" | "defer";

export interface LessonRun {
  id: number;
  lesson_id: number;
  lesson_title: string;
  unit_id: number;
  unit_title: string;
  started_at: string;
  /** null：進行中 */
  ended_at: string | null;
  review_completed_at: string | null;
  good_count: number;
  stuck_count: number;
  /** 這節課是哪一班上的；null＝沒有班級的上課紀錄 */
  class_id: number | null;
  class_name: string | null;
}

export interface RecordEvent {
  id: number;
  device_event_id: string;
  device_ts: string;
  received_at: string;
  polarity: Polarity;
  note: string;
  device_type: DeviceType;
  via: RecordVia;
  activity_step_id: number | null;
  behaviors: StudentBehavior[];
  handling: Handling | null;
  followup_text: string;
  followup_at: string | null;
  revision_count: number;
}

export interface PlanStepRef extends LessonStep {
  id: number;
}

export interface RunPlan {
  lesson_version_id: number;
  version_no: number;
  objective: string;
  steps: PlanStepRef[];
}

export interface RemoteDevice {
  id: number;
  device_type: DeviceType;
  paired_at: string;
  last_seen_at: string | null;
}

export interface RunDetail extends LessonRun {
  events: RecordEvent[];
  plan: RunPlan | null;
  remotes: RemoteDevice[];
}

export type RecordOp =
  | {
      op_id: string;
      kind: "create";
      device_event_id: string;
      device_ts: string;
      polarity: Polarity;
      note?: string;
      device_type: DeviceType;
      via?: "console" | "pip";
    }
  | { op_id: string; kind: "edit"; device_event_id: string; polarity?: Polarity; note?: string; at?: string }
  | { op_id: string; kind: "undo"; device_event_id: string; at?: string }
  | { op_id: string; kind: "delete"; device_event_id: string; at?: string };

export interface SyncOpResult {
  op_id: string;
  status: "applied" | "duplicate" | "rejected";
  code?: string;
  message?: string;
}

export interface SyncResponse {
  results: SyncOpResult[];
  events: RecordEvent[];
  ended_at: string | null;
}

export interface ReviewRequest {
  activity_step_id: number | null;
  behaviors: StudentBehavior[];
  handling: Handling | null;
  followup_text: string;
}

export interface Pairing {
  pairing_id: number;
  code: string;
  expires_at: string;
}

export interface RevokePairingResponse {
  ok: true;
  revoked_pairings: number;
  revoked_remotes: number;
}

export interface RemoteRun {
  run_id: number;
  lesson_title: string;
  started_at: string;
  ended_at: string | null;
  /** 只含這支遙控器按的書籤 */
  events: RecordEvent[];
}

export interface PairResponse {
  token: string;
  run: RemoteRun;
}
