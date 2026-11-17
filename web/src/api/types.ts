// Mirrors docs/API.md (Manna DLS API 合約，Phase 1 + 1.1 + 1.2). Keep in sync with the contract.

export type SchoolTrack = "general" | "vocational" | "comprehensive";
/** kb_manager（知識庫管理員）管理課綱條目與詞彙表，沒有主持人的其他權限。 */
export type Role = "teacher" | "pi" | "kb_manager" | "ra" | "rater" | "engineer";
/** 角色擁有的權限（GET /api/me 的 capabilities）。 */
export type Capability = "administer" | "manage_knowledge" | "use_materials";
/** 知識庫條目與教師備課經驗的適用類型；both＝各類型共用。 */
export type ItemTrack = SchoolTrack | "both";
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
  /** 登入回應沒有這個欄位時，前端依角色推得（lib/permissions.ts）。 */
  capabilities?: Capability[];
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
  /** 教材庫能不能上傳檔案（後端有設定儲存空間）；貼上文字不受影響 */
  uploads_enabled?: boolean;
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
  | "SC-RECORD"
  | "SC-SOP-ANALYSIS"
  | "SC-SOP-OBJECTIVES";
export type ScaffoldAction = "show" | "expand" | "use" | "skip";

export interface ScaffoldEventRequest {
  scaffold_code: ScaffoldCode;
  action: ScaffoldAction;
  trigger?: string;
  dwell_ms?: number;
}

export interface TeacherCase {
  case_code: string;
  school_track: ItemTrack;
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

/** 帳號與角色：一個未刪除的帳號。只有研究編號，沒有姓名或 Email。 */
export interface Account {
  id: number;
  research_id: string;
  school_track: SchoolTrack;
  role: Role;
  created_at: string;
  /** 目前登入的主持人自己（不能在這裡變更自己的角色） */
  is_self: boolean;
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

// ---- Phase 2.0：知識庫與詞彙 ----

/** 課綱條目或詞彙表匯入的結果；dry_run 時只有統計，沒有寫入。 */
export interface KnowledgeImportResult extends ImportResult {
  dry_run: boolean;
  rows: number;
  inserted: number;
  updated: number;
  unchanged: number;
  /** 課綱與知識庫條目：各學校類型的列數 */
  by_track?: Record<string, number>;
  /** 詞彙表：各級別的列數（appendix＝附錄詞彙） */
  by_level?: Record<string, number>;
  batch_id: number | null;
}

export type ItemKind = "" | "learning_performance" | "learning_content";

export interface KnowledgeSummary {
  knowledge: {
    active: number;
    real_active: number;
    demo_active: number;
    groups: { school_track: ItemTrack; category: string; item_kind: ItemKind; demo: boolean; count: number }[];
  };
  vocabulary: {
    active_forms: number;
    groups: { list_version: string; level: number | null; forms: number; entries: number }[];
  };
  recent_batches: {
    id: number;
    kind: "knowledge" | "vocabulary";
    file_sha256: string;
    row_count: number;
    inserted: number;
    updated: number;
    unchanged: number;
    imported_by: string | null;
    created_at: string;
  }[];
}

/** 語意索引（Phase 2.4）：目前設定的模型下，有多少使用中的條目已建立向量。 */
export interface EmbeddingStatus {
  /** false：沒有設定語意索引模型，檢索只用關鍵字 */
  enabled: boolean;
  provider: "none" | "fake" | "openai_compatible";
  model: string | null;
  dims: number | null;
  active_items: number;
  /** 有目前模型向量的使用中條目 */
  embedded: number;
  /** 舊模型或其他維度的向量：檢索不使用，重建後取代 */
  other_model: number;
  running: boolean;
  job: {
    id: number;
    trigger: "import" | "manual" | "startup";
    model: string;
    dims: number;
    status: "running" | "done" | "failed";
    total: number;
    embedded: number;
    error: string | null;
    created_at: string;
    finished_at: string | null;
  } | null;
  /** 只有 rebuild 的回應有：false 表示已有一次建立在進行 */
  started?: boolean;
}

export interface RetireDemoResult {
  dry_run: boolean;
  demo_active: number;
  demo_by_category: Record<string, number>;
  real_active: number;
  /** 停用後會沒有任何條目的類別 */
  categories_left_empty: string[];
  retired: number;
}

/** 知識庫條目（課綱條目的 content 是官方條文，一律來自資料庫）。 */
export interface KnowledgeItem {
  item_code: string;
  version: number;
  category: string;
  school_track: ItemTrack;
  title: string;
  content: string;
  /** 來源上印的代碼，例如 3-Ⅴ-7；非課綱條目為空字串 */
  official_code: string;
  item_kind: ItemKind;
  strand_name: string;
  stage: string;
  subject: string;
  advanced: boolean;
  recurring: boolean;
  source_doc: string;
  source_version: string;
  source_page: string;
}

/** 管理頁看到的條目：多了來源雜湊、匯入批次與停用時間。 */
export interface KnowledgeItemRow extends KnowledgeItem {
  source_sha256: string;
  import_batch_id: number | null;
  created_at: string;
  retired_at: string | null;
}

export interface KnowledgeItemsQuery {
  q?: string;
  category?: string;
  track?: string;
  kind?: string;
  status?: "active" | "retired" | "all";
  limit?: number;
  offset?: number;
}

export interface VocabularyRow {
  word: string;
  headword: string;
  level: number | null;
  pos: string;
  kind: "base" | "variant" | "derived" | "appendix";
  appendix_category: string;
  list_version: string;
  source_doc: string;
  source_page: string;
  source_sha256: string;
  version: number;
  import_batch_id: number | null;
  retired_at: string | null;
}

export interface VocabularyItemsQuery {
  q?: string;
  /** 1–6 或 appendix */
  level?: string;
  status?: "active" | "retired" | "all";
  limit?: number;
  offset?: number;
}

export interface Page<T> {
  total: number;
  items: T[];
}

export interface CurriculumQuery {
  q?: string;
  track?: SchoolTrack;
  kind?: Exclude<ItemKind, "">;
  /** II–V 或 all；預設 V（高中） */
  stage?: string;
  /** 科目或 all；預設英語相關科目 */
  subject?: string;
  limit?: number;
}

export interface CurriculumSearchResult {
  track: SchoolTrack;
  filter: { kind: string | null; stage: string; subject: string | string[] };
  /** false：沒有關鍵字，依代碼順序列出 */
  ranked: boolean;
  /** hybrid：關鍵字與語意相似度合併排序；keyword：只用關鍵字（未設定模型或模型沒有及時回應） */
  method: "keyword" | "hybrid";
  /** score：關鍵字分數（只靠語意找到的為 0）；similarity：與查詢的語意相似度（hybrid 才有） */
  items: { item: KnowledgeItem; score: number; title_score: number; similarity?: number }[];
}

export interface CurriculumByCodeResult {
  track: SchoolTrack | "all";
  items: KnowledgeItem[];
  /** 查不到的代碼：不可自行補上條文 */
  not_found: string[];
}

export interface VocabularyAnalyzeRequest {
  text: string;
  /** 1–6，預設 4 */
  target_level?: number;
  phrases?: string[];
  list_version?: string;
}

export type VocabularyMatch =
  | { match: "exact" }
  | { match: "inflection" }
  | { match: "ambiguous"; also_listed_level: number }
  | { match: "derived"; rule: string };

export interface VocabularyOccurrence {
  line: number;
  col: number;
  note?: string;
  context?: string;
  kind?: "hyphenated" | "compound" | "phrase";
  component_levels?: { part: string; level: number | null }[];
  listed_as_single_word_level?: number;
  category?: string;
}

export interface VocabularyListed {
  word: string;
  count: number;
  occurrences: VocabularyOccurrence[];
}

export interface VocabularyFlaggedForm {
  word: string;
  base: string;
  level: number;
  also_listed_level?: number;
  rule?: string;
}

/** 規則比對的詞彙級別分析（無 AI）。比例一定要和 basis 的分母、未計入數量一起呈現。 */
export interface VocabularyAnalysis {
  wordlist_version: string;
  target_level: number;
  basis: {
    counted_tokens: number;
    counted_types: number;
    denominator_note: string;
    excluded_tokens: { proper_nouns: number; unknown: number; multiword: number; appendix: number };
    match_types: Partial<Record<"exact" | "inflection" | "derived", number>>;
  };
  level_distribution: { level: number; tokens: number; token_ratio: number; types: number; type_ratio: number }[];
  above_target: {
    base: string;
    level: number;
    count: number;
    positions: ({ form: string; line: number; col: number; context: string } & VocabularyMatch)[];
  }[];
  ambiguous_forms: VocabularyFlaggedForm[];
  derived_matches: VocabularyFlaggedForm[];
  multi_level_words: { base: string; levels: number[]; used_level: number }[];
  proper_nouns: VocabularyListed[];
  unknown: VocabularyListed[];
  multiword: VocabularyListed[];
  appendix_words: VocabularyListed[];
  limits: string[];
  disclaimer: string;
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

// ---------------------------------------------------------------- 課例 SOP（docs/API.md Phase 2.1）

export type SopOrigin = "teacher" | "ai_adopted" | "ai_edited";
/** 發現／整理／分析／比較／創造資訊 */
export type InfoFunction = "discover" | "organize" | "analyze" | "compare" | "create";
/** 形成性評量／學生反思／蒐集問題 */
export type ExitKind = "formative" | "reflection" | "questions";
export type ObjectiveType = "content" | "language";

export interface SopParagraph {
  /** 同一段文字在教材改版後沿用原 id */
  id: string;
  no: number;
  text: string;
  /** PDF 才有：這一段從第幾頁開始 */
  page?: number;
}

/** 課例第 1 步的教材：教材庫裡的一份教材（Phase 2.2）。`version_no` 是教材的修訂次數 */
export interface SopMaterial {
  id: number;
  version_no: number;
  title: string;
  source_note: string;
  text: string;
  paragraphs: SopParagraph[];
  word_count: number;
  created_at: string;
  kind?: MaterialKind;
  origin?: MaterialOrigin;
  url?: string | null;
  filename?: string | null;
  has_file?: boolean;
  text_status?: MaterialTextStatus;
  text_truncated?: boolean;
  page_count?: number | null;
}

export interface SopFeature {
  id: string;
  feature: string;
  evidence: string;
  explanation: string;
}

export interface SopParagraphAnalysis {
  paragraph_id: string;
  function: string;
  summary: string;
  features: SopFeature[];
  origin: SopOrigin;
}

export interface SopLanguageFocus {
  id: string;
  kind: string;
  item: string;
  paragraph_id: string | null;
  evidence: string;
  note: string;
}

export interface SopAnalysis {
  text_type: string;
  main_idea: string;
  structure: string;
  overall_origin: SopOrigin;
  language_focus: SopLanguageFocus[];
  paragraphs: SopParagraphAnalysis[];
}

/** 條文與來源一律由伺服器依 item_code 從知識庫帶出 */
export interface SopCurriculumLink {
  item_code: string;
  why: string;
  /** 來源上印的代碼，例如 3-Ⅴ-7（item_code 是知識庫自己的代碼） */
  official_code: string;
  title: string;
  text: string;
  category: string;
  source_doc: string;
  source_version: string;
  source_page: string;
}

export interface SopObjective {
  id: string;
  type: ObjectiveType;
  text: string;
  student_facing_en: string;
  reason: string;
  paragraph_ids: string[];
  links: SopCurriculumLink[];
  link_status: "linked" | "unlinked";
  origin: SopOrigin;
}

export interface SopParagraphGoal {
  paragraph_id: string;
  objective_ids: string[];
  note: string;
}

/** 語言／教學／程序鷹架 */
export interface SopScaffolds {
  verbal: string;
  instructional: string;
  procedural: string;
}

export interface SopTask {
  id: string;
  function: InfoFunction | null;
  text: string;
  scaffolds: SopScaffolds;
  origin: SopOrigin;
}

/** 一段，或合併數段，的問題或任務 */
export interface SopTaskBlock {
  id: string;
  paragraph_ids: string[];
  tasks: SopTask[];
}

export interface SopWarmupActivity {
  id: string;
  text: string;
  scaffolds: SopScaffolds;
  origin: SopOrigin;
}

export interface SopVocabularyItem {
  id: string;
  word: string;
  note: string;
  origin: SopOrigin;
}

export interface SopWarmup {
  background: SopWarmupActivity[];
  vocabulary: SopVocabularyItem[];
}

export interface SopExitItem {
  id: string;
  kind: ExitKind;
  text: string;
  origin: SopOrigin;
}

export interface SopContent {
  analysis: SopAnalysis;
  objectives: SopObjective[];
  paragraph_goals: SopParagraphGoal[];
  task_blocks: SopTaskBlock[];
  warmup: SopWarmup;
  exit_ticket: { items: SopExitItem[] };
}

export interface SopAnalysisSuggestion {
  text_type: string;
  main_idea: string;
  structure: string;
  language_focus: Array<{ kind: string; item: string; paragraph_id: string; evidence: string; note: string }>;
  paragraphs: Array<{
    paragraph_id: string;
    paragraph_no: number;
    function: string;
    summary: string;
    features: Array<{ feature: string; evidence: string; explanation: string }>;
  }>;
  /** 模型兩次都沒有交出的段落 */
  missing_paragraph_ids: string[];
  /** 因為不是原文連續片段而被拿掉的證據筆數 */
  dropped_evidence: number;
}

export interface SopObjectiveSuggestion {
  type: ObjectiveType;
  text: string;
  student_facing_en: string;
  reason: string;
  paragraph_ids: string[];
  link_status: "linked" | "unlinked";
  links: SopCurriculumLink[];
}

export interface SopObjectivesSuggestion {
  objectives: SopObjectiveSuggestion[];
  /** 這次提供給模型挑選的課綱條目數 */
  curriculum_available: number;
}

export type SopJobKind = "analysis" | "objectives";

export interface SopJob<T = unknown> {
  id: number;
  lesson_id: number;
  kind: SopJobKind;
  status: "running" | "done";
  /** 給教師看的進度（完成後為空字串） */
  phase: string;
  started_at: string;
  finished_at: string | null;
  sop_version_no: number | null;
  result: AiResult<T> | null;
  /** 教師已經決定過的 item_index */
  decided: number[];
}

export interface SopState {
  lesson_id: number;
  version_no: number | null;
  versions_count: number;
  saved_at: string | null;
  material: SopMaterial | null;
  /** 這個版本原本有教材，教師後來把它刪除了（顯示「教材已刪除」） */
  material_deleted?: boolean;
  content: SopContent;
  /** 最新教案的「今天要教什麼？」 */
  today_objective: string;
  jobs: {
    analysis: SopJob<SopAnalysisSuggestion> | null;
    objectives: SopJob<SopObjectivesSuggestion> | null;
  };
}

export interface SavedSop {
  version_no: number;
  versions_count: number;
  saved_at: string;
  content: SopContent;
}

/** 貼上文字：`{ title, source_note, text }`；從教材庫選：`{ material_id }`；從課例移除：`{ material_id: null }` */
export interface SaveSopMaterialRequest {
  material_id?: number | null;
  title?: string;
  source_note?: string;
  text?: string;
}

// ---------------------------------------------------------------- Phase 2.2：教材與註記

export type MaterialKind = "text" | "document" | "image" | "audio" | "video" | "link";
export type MaterialOrigin = "paste" | "upload" | "url";
/** ready：有文字可讀、可註記；no_text：文件沒有文字層（掃描檔）；failed：讀不出來；not_applicable：圖片、影音、網址 */
export type MaterialTextStatus = "ready" | "no_text" | "failed" | "not_applicable";

// ---- Phase 2.10：掃描檔與 OCR ----

/** none：沒有辨識；queued／running：辨識中（教材暫時不能拿來做學習單、備課或版型）；done；failed */
export type OcrStatus = "none" | "queued" | "running" | "done" | "failed";

export interface MaterialOcr {
  status: OcrStatus;
  /** 已處理的頁（成功或失敗） */
  done_pages: number;
  /** 要辨識的頁數（PDF 頁數不明時先是 0） */
  total_pages: number;
  /** failed：原因；done：附註（失敗或略過的頁）或 null */
  error: string | null;
}

/** file：從檔案擷取或貼上的文字；ocr：由 OCR 辨識 */
export type TextSource = "file" | "ocr";

export interface MaterialLessonUse {
  lesson_id: number;
  lesson_title: string;
  unit_title: string;
}

export interface Material {
  id: number;
  kind: MaterialKind;
  origin: MaterialOrigin;
  title: string;
  source_note: string;
  url: string | null;
  filename: string | null;
  content_type: string | null;
  size_bytes: number;
  sha256: string | null;
  has_file: boolean;
  text_status: MaterialTextStatus;
  text_truncated: boolean;
  page_count: number | null;
  paragraph_count: number;
  word_count: number;
  revision: number;
  created_at: string;
  updated_at: string;
  lessons: MaterialLessonUse[];
  worksheet_count: number;
  annotation_count: number;
  /** 文字看起來像一份學習單（照我的學習單做 的範本）；舊的伺服器沒有這一欄 */
  looks_like_worksheet?: boolean;
  /** Phase 2.9：personal＝我的教材；platform＝教材參考（知識庫）。舊的伺服器沒有這幾欄 */
  scope?: LibraryScope;
  purpose?: MaterialPurpose;
  category?: string;
  tags?: string[];
  /** Phase 2.10：掃描檔的文字辨識。舊的伺服器沒有這兩欄（當作沒有辨識） */
  ocr?: MaterialOcr;
  text_source?: TextSource;
}

export interface MaterialDetail extends Material {
  text: string;
  paragraphs: SopParagraph[];
}

export interface MaterialFileRule {
  ext: string;
  kind: MaterialKind;
  max_bytes: number;
}

export interface MaterialLibrary {
  materials: Material[];
  usage: { used_bytes: number; quota_bytes: number; count: number; max_count: number };
  limits: { files: MaterialFileRule[]; max_pasted_chars: number; max_note_chars: number };
  storage: "s3" | "fs" | "none";
  uploads_enabled: boolean;
}

export interface CreateLibraryMaterialRequest {
  kind: "text" | "link";
  title?: string;
  source_note?: string;
  text?: string;
  url?: string;
  lesson_id?: number;
  /** Phase 2.9：省略＝material；學習單範本＝worksheet_sample */
  purpose?: MaterialPurpose;
}

export interface UpdateLibraryMaterialRequest {
  title?: string;
  source_note?: string;
  text?: string;
  url?: string;
}

export interface MaterialDeleted {
  deleted: boolean;
  annotations: number;
  lessons: number[];
}

export interface MaterialFileLink {
  url: string;
  expires_at: string;
}

export type AnchorStatus = "ok" | "moved" | "lost";

/** 註記現在在教材的哪裡；`lost`＝找不到原本標的文字，需要重新確認位置 */
export interface AnnotationAnchor {
  status: AnchorStatus;
  paragraph_id: string | null;
  paragraph_no: number | null;
  /** 段落文字中的位置（UTF-16，與瀏覽器選取相同） */
  start: number | null;
  end: number | null;
}

export interface MaterialAnnotation {
  id: number;
  material_id: number;
  quote: string;
  note: string;
  share_with_ai: boolean;
  hidden: boolean;
  anchor: AnnotationAnchor;
  created_at: string;
  updated_at: string;
}

export interface AnnotationPosition {
  paragraph_id: string;
  start: number;
  end: number;
}

export interface CreateAnnotationRequest extends AnnotationPosition {
  note?: string;
  share_with_ai?: boolean;
}

export interface UpdateAnnotationRequest {
  note?: string;
  share_with_ai?: boolean;
  hidden?: boolean;
  anchor?: AnnotationPosition;
}

// ---- Phase 2.3：學習單產生器 ----

export type WsLevel = "basic" | "standard" | "advanced";
export type WsAudience = "student" | "teacher";
/** 教師寫的｜Manna 產生的｜Manna 產生後教師改過 */
export type WsOrigin = "teacher" | "ai" | "ai_edited";
export type WsBlockKind = "warmup" | "activity" | "exit_ticket";
/** 語言／教學／程序鷹架 */
export type WsScaffoldType = "verbal" | "instructional" | "procedural";

/** Bloom 認知層次（固定集合，順序一律由記憶到創造；伺服器只接受這些 id）。中文標籤在 lib/bloom.ts。 */
export type Bloom = "remember" | "understand" | "apply" | "analyze" | "evaluate" | "create";
/** 要做什麼：學習單（原本的流程）｜閱讀理解題｜分層練習 */
export type WsOutput = "worksheet" | "comprehension" | "tiered";
/** 每個層次的題數（0–12，合計 3–30）；有值時取代 `bloom` 與 `question_count` */
export type BloomCounts = Partial<Record<Bloom, number>>;

/** 四個區塊的選取（規格 §5 的標籤，逐字）；都可以是空陣列。 */
export interface WorksheetSettings {
  material_types: string[];
  student_tasks: string[];
  answer_formats: string[];
  generation: string[];
  levels: WsLevel[];
  minutes: number | null;
  // Phase 2.7 (Bloom). Servers since 0014 always send these; older stored data may not have them.
  /** 預設 "worksheet"（原本的流程） */
  output?: WsOutput;
  /** 目標層次，依 Bloom 順序、不重複；空陣列＝六個都要 */
  bloom?: Bloom[];
  /** 3–30；null＝閱讀理解題 8 題、分層練習 12 題 */
  question_count?: number | null;
  /** 每個層次的題數；有值時取代 bloom 與 question_count（大於 0 的層次就是要出的層次） */
  bloom_counts?: BloomCounts | null;
  /** 教師寫的學習目標（≤300），送給每一步的模型 */
  objective?: string | null;
}

/** 教材接縫：產生器只認這個形狀。 */
export interface WorksheetMaterial {
  title: string;
  paragraphs: { id: string; text: string }[];
  source: {
    /** library：從教材庫選或上傳的那一份（`material_id`） */
    kind: "pasted" | "lesson_sop" | "library";
    lesson_id: number | null;
    material_version_no: number | null;
    note: string;
    /** 教材庫裡的那一份（檢視與註記用）；舊資料可能沒有 */
    material_id?: number | null;
  };
}

export interface WsObjective {
  id: string;
  type: ObjectiveType;
  text: string;
  student_facing_en: string;
  origin: WsOrigin;
  sop_id: string | null;
}

export interface WsSeedTask {
  id: string;
  paragraph_ids: string[];
  function: InfoFunction | null;
  text: string;
  sop_block_id: string | null;
  sop_task_id: string | null;
}

/** 產生前可以編修的「帶入內容」。 */
export interface WorksheetBrief {
  tasks: WsSeedTask[];
  warmup: string[];
  vocabulary: string[];
  exit_ticket: { kind: ExitKind; text: string }[];
}

export interface WsVocab {
  word: string;
  /** 由伺服器查詞彙表；查不到是 null */
  level: number | null;
  definition: string;
  context: string;
  paragraph_id: string | null;
}

export interface WsItem {
  id: string;
  answer_format: string | null;
  prompt: string;
  options: string[];
  /** 只出現在教師參考版 */
  answer: string;
  /** 只出現在教師參考版 */
  evidence: string;
  purpose: ExitKind | null;
  levels: WsLevel[];
  origin: WsOrigin;
  /** Bloom 認知層次；舊題目與未標的是 null。教師改的值由伺服器重新驗證。 */
  bloom?: Bloom | null;
  /** 提示或句型開頭：只印在基礎版 */
  hint?: string;
}

export interface WsScaffold {
  id: string;
  type: WsScaffoldType;
  kind: string;
  content: string;
  steps: string[];
  levels: WsLevel[];
  /** 系統自動加入的程序鷹架 */
  auto: boolean;
  origin: WsOrigin;
}

export interface WsBlock {
  id: string;
  kind: WsBlockKind;
  enabled: boolean;
  title: string;
  instructions: string;
  paragraph_ids: string[];
  info_function: InfoFunction | null;
  student_tasks: string[];
  objective_ids: string[];
  vocabulary: WsVocab[];
  items: WsItem[];
  scaffolds: WsScaffold[];
  teacher_notes: string;
  origin: WsOrigin;
  message: string | null;
  sop_block_id: string | null;
  /** The template section this block was generated from; keep it when saving (regenerate uses it). */
  template_section_id?: string | null;
}

export type WsAutoField = "material_types" | "student_tasks" | "answer_formats" | "generation" | "levels" | "minutes";

export interface WsChosen {
  settings: WorksheetSettings;
  auto: WsAutoField[];
  item_budget: number;
  reasons: string[];
}

export interface WorksheetContent {
  objectives: WsObjective[];
  show_objectives: boolean;
  blocks: WsBlock[];
  levels: WsLevel[];
  chosen: WsChosen | null;
  notices: string[];
}

/** `template`: one job for a whole template generation (or one template block regenerate). */
export type WsJobStep = "tasks" | "warmup" | "scaffolds" | "exit_ticket" | "template" | "bloom" | "bloom_check";
export type WsJobOutcome = "shown" | "blocked" | "timeout" | "error" | "disabled" | "skipped";

export interface WsJob {
  id: number;
  worksheet_id: number;
  run_no: number;
  step: WsJobStep;
  block_id: string | null;
  status: "queued" | "running" | "done";
  outcome: WsJobOutcome | null;
  phase: string;
  message: string | null;
  interaction_id: number | null;
  context: AiContextItem[];
  started_at: string | null;
  finished_at: string | null;
}

export interface WorksheetSummary {
  id: number;
  title: string;
  filename: string;
  lesson: { id: number; title: string; unit_id: number; unit_title: string } | null;
  class_id: number | null;
  generated: boolean;
  levels: WsLevel[];
  version_no: number;
  created_at: string;
  updated_at: string;
  /** 從哪一次備課對話產生（docs: 備課對話）；舊的伺服器沒有這一欄 */
  prep_session_id?: number | null;
}

export interface WorksheetState extends WorksheetSummary {
  material: WorksheetMaterial;
  settings: WorksheetSettings;
  /** 系統建議的（畫面上打星號） */
  suggested: WorksheetSettings;
  brief: WorksheetBrief;
  versions_count: number;
  saved_at: string;
  content: WorksheetContent;
  jobs: WsJob[];
  running: boolean;
  /** 有上一版可以復原的區塊 id */
  undoable: string[];
  sop_sync: { linked: boolean; changes: number };
  /** 照哪個版型產生（docs/WORKSHEET_TEMPLATES_CONTRACT.md）；舊的伺服器沒有這一欄 */
  template?: WsTemplateRef | null;
  /** 給 Manna 的要求（≤2000），送給每一步的模型；舊的伺服器沒有這一欄 */
  request?: string;
}

export interface WsJobsState {
  running: boolean;
  jobs: WsJob[];
  version_no: number;
}

export interface CreateWorksheetRequest {
  lesson_id?: number;
  class_id?: number;
  material?: { title?: string; text: string; source_note?: string };
  /** 教材庫裡有文字的一份；優先於貼上的 material */
  material_id?: number;
  /** 自己的、已讀好（ready）的版型 */
  template_id?: number;
  /** 一部分設定；沒給的照原本的預設。版型＋output ≠ worksheet → 400 */
  settings?: Partial<WorksheetSettings>;
  /** 給 Manna 的要求（≤2000） */
  request?: string;
}

export interface SaveWorksheetRequest {
  title?: string;
  filename?: string;
  settings?: WorksheetSettings;
  brief?: WorksheetBrief;
  content?: WorksheetContent;
  /** 設定版型：記下目前版本並依版型重設 settings；null 拿掉版型（settings 不動）。產生中送出會 409。 */
  template_id?: number | null;
  /** 給 Manna 的要求（≤2000） */
  request?: string;
}

export interface SaveWorksheetMaterialRequest {
  title?: string;
  text: string;
  source_note?: string;
}

export interface SopSyncChange {
  key: string;
  target: "objective" | "task_block";
  action: "add" | "update" | "remove";
  label: string;
  before: string;
  after: string;
}

export interface SopSyncPreview {
  lesson_id: number;
  sop_version_no: number | null;
  changes: SopSyncChange[];
  /** 預留：建議補充至單元想法（CoRe）；這一版永遠是空的 */
  core_suggestions: unknown[];
}

export interface SopSyncResult {
  sop_version_no: number;
  applied: number;
}

export interface WsVocabCheck {
  id: number;
  level: WsLevel;
  audience: "student";
  target_level: number;
  version_no: number;
  checked_at: string;
  stale: boolean;
  analysis: VocabularyAnalysis;
  /** analysis 中 position.line（從 1）對應的位置說明 */
  lines: string[];
}

export type WsExportFormat = "docx" | "pdf";

export interface WsExportQuery {
  format: WsExportFormat;
  level: WsLevel;
  audience: WsAudience;
}

// ---- 我的學習單版型（docs/WORKSHEET_TEMPLATES_CONTRACT.md）----

export type WsTemplateStatus = "extracting" | "ready" | "failed";
export type WsInstructionLanguage = "英文" | "中文" | "中英並用";

export interface WsTemplateSection {
  /** "s1"、"s2"…；編修後不變 */
  id: string;
  title_pattern: string;
  purpose: string;
  block_kind: WsBlockKind;
  /** 新教材的每一段各做一次 */
  per_paragraph: boolean;
  /** 固定標籤（學生任務） */
  student_tasks: string[];
  /** 固定標籤（作答方式） */
  answer_formats: string[];
  /** 1–12 */
  item_count: number;
  text_scope: string;
  /** 固定的鷹架種類 */
  scaffolds: string[];
  instruction_style: string;
  item_patterns: string[];
  /** 需要老師提供影片、錄音、圖片或另一篇文字 */
  needs_external_material: boolean;
}

export interface WsTemplate {
  worksheet_kind: string;
  instruction_language: WsInstructionLanguage;
  header_fields: string[];
  shows_objectives: boolean;
  includes_passage_text: boolean;
  /** 1–12 */
  sections: WsTemplateSection[];
  /** 固定標籤（產生設定） */
  generation_options: string[];
  /** 0–8，每則 ≤200 字 */
  style_notes: string[];
}

export interface WsTemplateSummary {
  id: number;
  title: string;
  status: WsTemplateStatus;
  section_count: number;
  source_material: { id: number; title: string } | null;
  version_no: number | null;
  updated_at: string;
  /** Phase 2.9：platform＝平台範本（唯讀，可另存成我的範本）。舊的伺服器沒有這幾欄 */
  scope?: LibraryScope;
  category?: string;
  tags?: string[];
}

export interface WsTemplateDetail extends WsTemplateSummary {
  error: string | null;
  content: WsTemplate | null;
}

export interface WsTemplateList {
  templates: WsTemplateSummary[];
}

export interface CreateWsTemplateRequest {
  material_id: number;
  title?: string;
}

export interface UpdateWsTemplateRequest {
  title?: string;
  /** 有 content 就多一個版本（origin=teacher） */
  content?: WsTemplate;
}

// ---- Phase 2.9：教材參考、我的教材、學習單範本（瀏覽；教師端）----

/** personal：老師自己的；platform：知識庫（教材參考、平台範本），每位老師都能讀、能用 */
export type LibraryScope = "personal" | "platform";
/** material：教材；worksheet_sample：學習單範本的原檔（不列在教材裡） */
export type MaterialPurpose = "material" | "worksheet_sample";
export type BrowseFileType = "pdf" | "docx" | "text" | "image" | "audio" | "video" | "link";

export interface FacetCount {
  key: string;
  count: number;
}

/** GET /api/browse/materials 的一張卡 */
export interface MaterialCard {
  id: number;
  title: string;
  scope: LibraryScope;
  category: string;
  tags: string[];
  kind: string;
  file_type: string;
  page_count: number | null;
  paragraph_count: number;
  word_count: number;
  /** 文字的前約 280 字，空白已合併（沒有文字時為 ""） */
  excerpt: string;
  text_status: string;
  looks_like_worksheet: boolean;
  created_at: string;
  owner_label: "Manna 知識庫" | "我";
  /** Phase 2.10；舊的伺服器沒有這兩欄 */
  ocr?: MaterialOcr;
  text_source?: TextSource;
}

/** 分面只依 area 與 q 計算（每個 chip 顯示選了會有幾份） */
export interface MaterialBrowse {
  items: MaterialCard[];
  next_cursor: string | null;
  facets: { categories: FacetCount[]; tags: FacetCount[]; file_types: FacetCount[] };
  total: number;
}

export interface MaterialBrowseQuery {
  area: "reference" | "mine";
  q?: string;
  category?: string;
  /** 可重複；全部都要符合 */
  tag?: string[];
  file_type?: BrowseFileType;
  /** 有文字且 ≤ 40 段 */
  fits_lesson?: boolean;
  /** 預設 new */
  sort?: "new" | "title";
  cursor?: string;
  /** 預設 24，最多 60 */
  limit?: number;
}

/** GET /api/browse/templates 的一張卡 */
export interface TemplateCard {
  id: number;
  title: string;
  scope: LibraryScope;
  status: WsTemplateStatus;
  category: string;
  tags: string[];
  worksheet_kind: string;
  instruction_language: string;
  section_count: number;
  /** 前 6 個區塊的標題寫法 */
  section_titles: string[];
  source_material: { id: number; title: string; file_type: string; page_count: number | null } | null;
  updated_at: string;
  owner_label: "Manna 範本" | "我";
}

export interface TemplateBrowse {
  items: TemplateCard[];
  next_cursor: string | null;
  facets: { categories: FacetCount[]; tags: FacetCount[]; langs: FacetCount[] };
  total: number;
}

export interface TemplateBrowseQuery {
  /** platform：平台範本（只有 ready）；mine：我的範本（任何狀態） */
  area: "platform" | "mine";
  q?: string;
  category?: string;
  tag?: string[];
  lang?: WsInstructionLanguage;
  cursor?: string;
  limit?: number;
}

/** 學習單記下的版型與版本 */
export interface WsTemplateRef {
  id: number;
  title: string;
  version_no: number;
}

// ---- 學習單的投影與版本（GET /api/worksheets/:id/sheet、/versions）----

/** 學生版／教師版的投影（伺服器組裝；匯出用同一份）。學生版沒有 answer、evidence、teacher_notes。 */
export interface WsSheet {
  title: string;
  level: WsLevel;
  audience: WsAudience;
  objectives: string[];
  blocks: WsSheetBlock[];
}

export interface WsSheetBlock {
  id: string;
  kind: WsBlockKind;
  /** "A"、"B"…；Exit ticket 沒有 */
  label: string;
  title: string;
  instructions: string;
  steps: string[];
  vocabulary: Array<{ word: string; level: number | null; definition: string }>;
  supports: Array<{ type: WsScaffoldType; kind: string; heading?: string; content: string }>;
  items: Array<{
    no: number;
    answer_format: string | null;
    prompt: string;
    options: string[];
    lines: number;
    answer?: string;
    evidence?: string;
    /** 只有教師版 */
    bloom?: Bloom | null;
    /** 只有基礎版（印成 "Hint: …"） */
    hint?: string;
  }>;
  teacher_notes?: string;
}

export interface WsVersionRow {
  version_no: number;
  cause: string;
  created_at: string;
}

// ---- 備課對話（prep sessions）----

/**
 * comprehension／tiered：output 為閱讀理解題／分層練習的學習單（像 worksheet）。
 * kit：教學套件＝活動流程（result）＋分層學習單（worksheet），兩者都好才 done。
 */
export type PrepKind =
  | "worksheet"
  | "assessment"
  | "activity"
  | "resources"
  | "ask"
  | "revise"
  | "comprehension"
  | "tiered"
  | "kit";
export type PrepPhase = "引起動機" | "發展活動" | "統整與評量";

/** 學習目標的兩組（SIOP）：content＝內容目標、language＝語言目標 */
export type ActivityGoalKind = "content" | "language";

/**
 * 課堂活動的一個學習目標。prep_activity 第 3 版起分成內容目標與語言目標，中英文並陳：
 * zh 給教師（「學生能……」），en 給學生（"Students will be able to …"）。
 * 之前做的規劃每個目標只有一行中文 text、沒有 kind，照舊顯示成一張清單。
 */
export type ActivityGoal =
  | { kind: ActivityGoalKind; zh: string; en: string; curriculum_codes: string[] }
  | { kind?: undefined; text: string; curriculum_codes: string[] };

export interface ActivityPlan {
  title: string;
  /** 先內容目標、再語言目標（第 3 版）；舊的規劃是一張沒有 kind 的清單 */
  goals: ActivityGoal[];
  steps: Array<{
    phase: PrepPhase;
    title: string;
    minutes: number;
    /** 教學思路 */
    idea: string;
    /** 只有引起動機：選項一／二／三 */
    options: Array<{ title: string; idea: string; script: string[]; minutes: number }>;
    /** 老師說的話（英文課室用語） */
    script: string[];
    /** 卡住了怎麼辦；可能是 "" */
    stuck_tip: string;
    /** 這一步練到的 Bloom 層次（≤3）；prep_activity 第 1 版的規劃沒有 */
    bloom?: Bloom[];
  }>;
  curriculum: Array<{ code: string; text: string }>;
}

export interface PrepResources {
  curriculum: Array<{ code: string; text: string; school_track: string | null; similarity: number | null }>;
  materials: Array<{ id: number; title: string }>;
  note: string;
}

export interface PrepAskResult {
  answer: string;
  follow_ups: string[];
}

export interface PrepReviseResult {
  reply: string;
  changed_block_ids: string[];
  version_no: number | null;
}

export interface PrepSessionSummary {
  id: number;
  title: string;
  material: { id: number; title: string } | null;
  topic: string | null;
  class: { id: number; name: string } | null;
  template: { id: number; title: string } | null;
  turn_count: number;
  last_kind: PrepKind | null;
  updated_at: string;
  created_at: string;
}

export interface PrepTurn {
  id: number;
  seq: number;
  kind: PrepKind;
  request: string;
  status: "running" | "done" | "failed";
  error: string | null;
  worksheet: { id: number; title: string; version_no: number; running: boolean } | null;
  /** 照版型做的學習單：用的版型與它的狀態；extracting 時 worksheet 是 null。舊的伺服器沒有這一欄 */
  template?: { id: number; title: string; status: WsTemplateStatus } | null;
  result: ActivityPlan | PrepResources | PrepAskResult | PrepReviseResult | null;
  /** 有幫助／不合用 → POST /api/ai/:interaction_id/decision { decision: "skip", thumbs } */
  interaction_id: number | null;
  created_at: string;
  finished_at: string | null;
  /** 這一輪在等學習單範本的 OCR；其他時候沒有這一欄（Phase 2.10） */
  phase?: "ocr";
  /** 與 phase 一起出現 */
  ocr?: { done_pages: number; total_pages: number };
}

/** 對話的出題選項：之後在對話裡做的每一份學習單都帶入（settings 的同名欄位）。 */
export interface PrepSessionOptions {
  bloom?: Bloom[];
  question_count?: number;
  bloom_counts?: BloomCounts;
  answer_formats?: string[];
  objective?: string;
}

export interface PrepSession extends PrepSessionSummary {
  /** seq 順序 */
  turns: PrepTurn[];
  running: boolean;
  /** 伺服器建議的下一步 */
  next: PrepKind[];
  /** 建立或 PATCH 時給的選項；舊的伺服器沒有這一欄 */
  options?: PrepSessionOptions | null;
}

export interface PrepSessionList {
  sessions: PrepSessionSummary[];
}

export interface CreatePrepSessionRequest {
  /** 不能是 revise */
  kind: Exclude<PrepKind, "revise">;
  request?: string;
  material_id?: number;
  /** 貼上的文字：先存進教材庫 */
  material?: { title?: string; text: string };
  topic?: string;
  class_id?: number;
  template_id?: number;
  /**
   * 照我的學習單做：老師的一份有文字的教材（學習單範本）。只用於 kind worksheet；伺服器沿用或建立
   * 它的版型。同時給 template_id 時以 template_id 為準。
   */
  template_material_id?: number;
  /** 帶進這次備課做的每一份學習單（省略＝全部層次、自動題數） */
  options?: {
    bloom?: Array<"remember" | "understand" | "apply" | "analyze" | "evaluate" | "create">;
    question_count?: number;
    /** 每個層次的題數（0–12，合計 3–30）；有它就不看 bloom 與 question_count */
    bloom_counts?: Partial<Record<"remember" | "understand" | "apply" | "analyze" | "evaluate" | "create", number>>;
    /** 固定標籤（選擇題、是非題…）；省略＝自動 */
    answer_formats?: string[];
    /** 學習目標（≤300） */
    objective?: string;
  };
}

export interface UpdatePrepSessionRequest {
  title?: string;
  class_id?: number | null;
  template_id?: number | null;
  options?: PrepSessionOptions;
}

export interface CreatePrepTurnRequest {
  /** 省略（下方輸入列）：有學習單就是 revise，否則 ask */
  kind?: PrepKind;
  request: string;
}

// =============================================================================================
// 教材與範本管理（admin library）— agent F2. docs/API.md「Phase 2.9：教材參考、我的教材、學習單範本」,
// "Admin APIs". ManageKnowledge (pi, kb_manager) only. The card fields mirror the browse API's
// MaterialCard and TemplateCard; they are named apart here so this section merges on its own.
// =============================================================================================

/** A platform material as the admin APIs return it (POST /api/admin/library, PATCH). */
export interface AdminLibraryMaterial {
  id: number;
  title: string;
  scope: "personal" | "platform";
  /** 教材參考's fixed list: 學測英文、統測英文、課綱、詞彙表、考試說明、其他 */
  category: string;
  tags: string[];
  kind: string;
  /** pdf / docx / text / image / audio / video / link */
  file_type: string;
  page_count: number | null;
  paragraph_count: number;
  word_count: number;
  /** The first ~280 characters of the text ("" when there is none). */
  excerpt: string;
  text_status: string;
  looks_like_worksheet: boolean;
  created_at: string;
  owner_label: string;
  /** Text recognition of a scanned file (Phase 2.10); absent from older servers. */
  ocr?: MaterialOcr;
  text_source?: TextSource;
}

/** A platform template as the admin APIs return it (POST /api/admin/library/templates, PATCH, retry). */
export interface AdminLibraryTemplate {
  id: number;
  title: string;
  scope: "personal" | "platform";
  status: "extracting" | "ready" | "failed";
  /** The sample list (字彙、文法句型…); "" while the server has not suggested one yet. */
  category: string;
  tags: string[];
  worksheet_kind: string;
  instruction_language: string;
  section_count: number;
  /** The first 6 section titles. */
  section_titles: string[];
  source_material: { id: number; title: string; file_type: string; page_count: number | null } | null;
  updated_at: string;
  owner_label: string;
}

/** What the admin list adds to a card. */
export interface AdminLibraryRowExtra {
  /** The uploading admin's research id (null if that account is gone). */
  uploaded_by: string | null;
  size_bytes: number;
}

export type AdminLibraryMaterialRow = AdminLibraryMaterial & AdminLibraryRowExtra;
export type AdminLibraryTemplateRow = AdminLibraryTemplate & AdminLibraryRowExtra;

/** GET /api/admin/library?kind=materials|templates */
export interface AdminLibraryList<T> {
  items: T[];
  total?: number;
  next_cursor?: string | null;
}

export interface AdminLibraryQuery {
  q?: string;
  category?: string;
  cursor?: string;
}

/** The multipart fields of an admin upload, besides the file. */
export interface AdminLibraryUploadFields {
  title?: string;
  /** Required for materials; "" for a template means 自動判斷 */
  category?: string;
  tags?: string[];
  source_note?: string;
}

/** PATCH /api/admin/library/:id and /api/admin/library/templates/:id */
export interface AdminLibraryUpdate {
  title?: string;
  category?: string;
  tags?: string[];
}
