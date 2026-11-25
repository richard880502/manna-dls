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
  /** 社群（主持人的研究開關，預設關）；關閉時網頁不顯示任何與分享有關的東西。舊伺服器沒有這個鍵 */
  community_enabled?: boolean;
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

/** 後台管理 → 文字模型：一組正在使用（或預設）的設定 */
export interface AiModelValue {
  provider: string;
  model: string;
  effort: string;
  timeout_secs: number;
}

export interface AiModelProvider {
  id: "openai" | "openai_compatible";
  label: string;
  /** 環境有設定這個來源的金鑰或位址才能選 */
  available: boolean;
  /** 建議的模型名稱（含環境變數預設的那個） */
  models: string[];
  /** 只有 OpenAI 會用到推理強度 */
  uses_effort: boolean;
}

export interface AiModelSettings {
  active: AiModelValue;
  /** env = 環境變數預設；admin = 主持人在後台設定 */
  source: "env" | "admin";
  changed_by: string | null;
  changed_at: string | null;
  env_default: AiModelValue;
  providers: AiModelProvider[];
  efforts: string[];
  timeout_range: { min: number; max: number };
  /** 只有儲存成功的回應才有：測試呼叫花的毫秒數 */
  test_latency_ms?: number;
}

export interface AiModelRequest {
  provider: string;
  model: string;
  effort?: string;
  timeout_secs: number;
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
  /** 哪一頁開始這個課例（只記研究紀錄；首頁的「逐步設計」卡片傳 "home"） */
  entry?: "home";
}

/** 我的作品裡的一堂逐步設計課例（GET /stepwise-lessons） */
export interface StepwiseLesson {
  id: number;
  title: string;
  unit_id: number;
  unit_title: string;
  updated_at: string;
  /** 七步裡已有內容的步數（0 到 7） */
  steps_done: number;
  /** 第一個還空著的步驟（1 到 7）；null＝七步都有內容 */
  next_step: number | null;
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
  | "SC-SOP-OBJECTIVES"
  | "SC-SOP-GOALS"
  | "SC-SOP-TASKS"
  | "SC-SOP-WARMUP"
  | "SC-SOP-EXIT"
  | "SC-SOP-ENGLISH";
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
  /** 這次改了什麼（選填，最多 200 字） */
  note?: string | null;
  /** 這一版是從哪一版還原的（version_no） */
  restored_from?: number | null;
  /** 儲存的人（研究編號）；初始版本沒有 */
  created_by_research_id?: string | null;
  /** 是目前登入的主持人自己存的 */
  by_me?: boolean;
}

/** One `{{variable}}` the code fills into a prompt. */
export interface PromptVar {
  name: string;
  label: string;
  /** 程式一定會填、而且不能沒有：新版本少了它會被拒絕 */
  required: boolean;
  /** 缺少時程式會自動補在訊息最後 */
  auto: boolean;
}

export interface PromptInfo {
  key: string;
  name_zh: string;
  group: string;
  description: string;
  /** false：程式只讀 system_text，使用者範本可以留空 */
  uses_user_template: boolean;
  vars: PromptVar[];
}

export interface PromptList {
  /** 每個 key 的最新版（含還沒登記的 key） */
  prompts: PromptVersion[];
  groups: string[];
  registry: PromptInfo[];
  /** 可以「試產生」的 key */
  try_keys: string[];
}

export interface CreatePromptRequest {
  prompt_key: string;
  system_text: string;
  user_template: string;
  note?: string;
  restored_from?: number;
}

export interface TrySample {
  /** 英文教材（段落之間空一行） */
  passage: string;
  /** prep_ask：老師的問題 */
  question?: string;
  class_profile?: string;
}

export interface TryCheck {
  name: string;
  passed: boolean;
  detail?: string;
}

/** 試產生的結果：預覽，不會存成版本、也不會給任何老師看到。 */
export interface TryResponse {
  run_id: number;
  preview: true;
  status: "ok" | "disabled" | "timeout" | "refused" | "error";
  message: string | null;
  /** 實際送給模型的訊息 */
  system: string;
  user: string;
  /** 模型的原始 JSON（還沒經過任何檢查或修補） */
  output: unknown;
  checks: TryCheck[];
  notes: string[];
  unknown_variables: string[];
}

export interface SavedPrompt extends PromptVersion {
  /** 存下了，但文字裡有程式不會填的 {{名稱}} */
  unknown_variables: string[];
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
  /** 舊資料沒有這一欄：當作教師寫的 */
  origin?: SopOrigin;
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
  /** 作答方式（學習單產生器的標籤；空字串＝還沒選） */
  answer_format?: string;
  options?: string[];
  /** 參考答案（教師版） */
  answer?: string;
  /** 原文依據（逐字摘自段落） */
  evidence?: string;
}

/** 一段，或合併數段，的問題或任務 */
export interface SopTaskBlock {
  id: string;
  paragraph_ids: string[];
  tasks: SopTask[];
  /** 學生任務、作答方式：學習單產生器的標籤，在這一步逐段選 */
  student_tasks?: string[];
  answer_formats?: string[];
  /** Manna 先選的（✦） */
  ai_student_tasks?: string[];
  ai_answer_formats?: string[];
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
  /** 大考中心詞彙表的級別；由伺服器依詞彙表帶入 */
  level?: number | null;
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
  /** 形成性評量檢核的目標（第 3 步的目標 id；空字串＝沒有） */
  objective_id?: string;
  /** 參考答案（教師版） */
  answer?: string;
}

/** 老師在各步驟為學習單做的選擇；null＝沒選，用 SOP 內容推出的預設 */
export interface SopOptions {
  material_types: string[] | null;
  generation: string[] | null;
  levels: WsLevel[] | null;
  minutes: number | null;
  /** false＝學習單不要 Exit ticket */
  exit_ticket: boolean | null;
}

export interface SopContent {
  analysis: SopAnalysis;
  objectives: SopObjective[];
  paragraph_goals: SopParagraphGoal[];
  task_blocks: SopTaskBlock[];
  warmup: SopWarmup;
  exit_ticket: { items: SopExitItem[] };
  options?: SopOptions;
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

export interface SopGoalSuggestion {
  paragraph_id: string;
  paragraph_no: number;
  objective_ids: string[];
  note: string;
}

export interface SopGoalsSuggestion {
  paragraphs: SopGoalSuggestion[];
  missing_paragraph_ids: string[];
}

export interface SopTaskSuggestion {
  function: InfoFunction | null;
  text: string;
  answer_format: string;
  options: string[];
  answer: string;
  evidence: string;
  scaffolds: SopScaffolds;
}

export interface SopBlockSuggestion {
  paragraph_ids: string[];
  paragraph_nos: number[];
  title: string;
  student_tasks: string[];
  answer_formats: string[];
  objective_ids: string[];
  reason: string;
  tasks: SopTaskSuggestion[];
}

export interface SopTasksSuggestion {
  blocks: SopBlockSuggestion[];
  missing_paragraph_ids: string[];
  scaffolds_ok: boolean;
}

export interface SopWarmupSuggestion {
  background: Array<{ text: string; scaffolds: SopScaffolds; purpose: string }>;
  vocabulary: Array<{
    word: string;
    level: number | null;
    paragraph_id: string | null;
    evidence: string;
    definition_en: string;
  }>;
  candidates_available: number;
  levels_available: boolean;
}

export interface SopExitSuggestion {
  items: Array<{ kind: ExitKind; text: string; objective_id: string; answer: string }>;
  uncovered_objective_ids: string[];
}

export interface SopEnglishSuggestion {
  english: string;
  changed: boolean;
  note: string;
}

export interface SopProceduralSteps {
  steps: string[];
  text: string;
}

/** 換一個（objectives）、這一段再建議一次（goals） */
export interface SopRedoRequest {
  /** objectives：先推薦內容目標（`content`），有了內容目標之後才依它推薦語言目標（`language`） */
  only_type?: ObjectiveType;
  parent_job_id?: number;
  item_index?: number;
  paragraph_id?: string;
}

export type SopJobKind = "analysis" | "objectives" | "goals" | "tasks" | "warmup" | "exit_ticket";

/** 暖身的字彙這一組在決定紀錄裡的 item_index（活動是 0、1…） */
export const SOP_VOCABULARY_ITEM_INDEX = 100;

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
  /** 空字串＝整批；否則是「換一個」「這一段再建議一次」的那一項 */
  slot?: string;
  parent_job_id?: number | null;
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
  /** 學習單從這個課例開始時的設定：SOP 內容加上各步驟的選擇 */
  worksheet_settings?: WorksheetSettings;
  /** 只看 SOP 內容推出的預設（✦） */
  worksheet_suggested?: WorksheetSettings;
  jobs: {
    analysis: SopJob<SopAnalysisSuggestion> | null;
    objectives: SopJob<SopObjectivesSuggestion> | null;
    goals?: SopJob<SopGoalsSuggestion> | null;
    tasks?: SopJob<SopTasksSuggestion> | null;
    warmup?: SopJob<SopWarmupSuggestion> | null;
    exit_ticket?: SopJob<SopExitSuggestion> | null;
    /** 換一個、這一段再建議一次：上次整批之後的單項重做 */
    redo?: SopJob[];
  };
}

export interface SavedSop {
  version_no: number;
  versions_count: number;
  saved_at: string;
  content: SopContent;
  worksheet_settings?: WorksheetSettings;
  worksheet_suggested?: WorksheetSettings;
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
  /** 在哪裡修改課文（只寫進研究紀錄） */
  edit_from?: "home" | "prep_session" | "worksheet" | "library";
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
/** 要做什麼：學習單（原本的流程）｜閱讀理解題｜差異化學習單（原名分層練習） */
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
  /** 3–30；null＝閱讀理解題 8 題、差異化學習單 6 題 */
  question_count?: number | null;
  /** 每個層次的題數；有值時取代 bloom 與 question_count（大於 0 的層次就是要出的層次） */
  bloom_counts?: BloomCounts | null;
  /** true＝學習單不要 Exit ticket（課例 SOP 的產生設定） */
  skip_exit_ticket?: boolean;
  /** 內容目標（≤300，一行一項）：教師寫的或採用 Manna 草擬的，送給每一步的模型。舊資料的 objective 讀成這一欄 */
  content_objective?: string | null;
  /** 語言目標（≤300，一行一項） */
  language_objective?: string | null;
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
    /** 只選要用的段落：這份學習單只用這些段落（教材庫那份的段落 id）；全部都用時沒有這一欄 */
    selected_ids?: string[];
    /** 教材庫那份有幾段（有 selected_ids 時才有） */
    total_paragraphs?: number;
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
  /** 英文字義 */
  definition: string;
  context: string;
  paragraph_id: string | null;
  /** 作答指示裡的功能詞（compare、support your answer…；Key Vocabulary 的 Academic 欄，SIOP V2）；一般關鍵字沒有這一欄 */
  task_word?: boolean;
  /** 詞性（n.、v.、adj.、adv.、prep.、conj.、pron.、det.、aux.、interj.、phr.）；舊學習單沒有 */
  pos?: string;
  /** 中文字義；還沒有時沒有這一欄（印英文字義） */
  meaning_zh?: string;
}

/** Key Vocabulary 字義的語言：en（預設）或 zh */
export type WsVocabLang = "en" | "zh";

/** 字詞表鷹架（Word bank）的一個字 */
export interface WsBankWord {
  word: string;
  pos?: string;
  /** 英文字義 */
  definition?: string;
  meaning_zh?: string;
  /** 老師勾選：學生版這個字旁邊也印字義 */
  show_meaning?: boolean;
}

/** 字詞表鷹架的字（每個字都印詞性；勾選的字才印字義）。純文字的字詞表由伺服器讀成這個；不是一串單字的（Linking words for …）沒有 */
export interface WsWordBank {
  /** 印出來的字義語言（預設 en） */
  lang?: WsVocabLang;
  words: WsBankWord[];
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
  /** 提示或句型開頭：只印在基礎版（閱讀理解題，與改版前的分層練習） */
  hint?: string;
  /**
   * 這一題自己的句型框架：一小段學生可以照著說或寫的話（招呼、帶理由的回答、結尾，空格用 ___），
   * 印在題目下面。沒有時是空字串或沒有這個鍵（舊資料）。
   */
  frame?: string;
  /**
   * 差異化學習單：這一題在三個版本的支持、參考答案與評分說明（依版本順序）；其他題目是 []。
   * 舊資料沒有這個鍵。
   */
  tiers?: WsItemTier[];
  /**
   * 表格整理：學生要填的表格（欄名與空白列數）。沒有這個鍵時用題目裡列出的欄（"…: Who? / Why?"），
   * 沒有列出就是兩個沒有欄名的欄、3 列。換成別的作答方式時留著，只在表格整理印出。
   */
  table?: WsItemTable | null;
  /** 改成是非題時收起來的選項（True／False 取代了它們）；改回選擇題或配對題時放回去。不會印出。 */
  kept_options?: string[];
  /**
   * 圖像組織：題目的圖表（只在 answer_format 是「圖像組織」時印出）。舊題目與手寫的題目沒有這個鍵，
   * 印成一個空白方框。
   */
  organizer?: WsOrganizer | null;
}

export interface WsItemTable {
  /** 1–6 欄；有任何欄名時第一列印欄名 */
  columns: string[];
  /** 欄名下方的空白列數，1–12 */
  rows: number;
  /**
   * 每一列要學生寫什麼或比較什麼（"Claim"、"Supporting detail"），印在第一欄；沒有（或全空白）就是
   * 沒有列標籤的空白列，舊的表格也沒有這個鍵。第 i 列用第 i 個，沒有對到的列不印標籤。
   * 學習單投影（sheet）裡有列標籤時，columns 多一個空白的第一欄（放標籤的那一欄）。
   */
  row_labels?: string[];
}

// ---- 圖像組織（docs/API.md「圖像組織」）----

/** 八種圖像組織 */
export type WsOrganizerType =
  | "t_chart"
  | "venn"
  | "sequence"
  | "cause_effect"
  | "problem_solution"
  | "story_map"
  | "kwl"
  | "mind_map";

/** 一個空格：參考答案（只在教師版）與在 prefilled 版本的學生版預填的文字 */
export interface WsOrganizerCell {
  answer: string;
  given: string;
  /** 學生版印出 given 的版本（基礎版預填最多，T3）；空陣列＝都不預填 */
  prefilled: WsLevel[];
}

/** 一格：欄標題、方框標題或分支，和它的空格 */
export interface WsOrganizerSection {
  label: string;
  cells: WsOrganizerCell[];
}

export interface WsOrganizer {
  type: WsOrganizerType;
  /** 印在圖表上方（"" 不印） */
  title: string;
  /** 只有心智圖：中心詞 */
  center: string;
  sections: WsOrganizerSection[];
}

/** 投影裡的圖像組織（某個版本、某個對象） */
export interface WsSheetOrganizer {
  type: WsOrganizerType;
  /** "T-chart"…（無障礙文字） */
  name: string;
  title: string;
  center: string;
  sections: Array<{
    label: string;
    /** given：學生版預填的文字（"" 是空白）；answer：只有教師版，學生要寫的格子的參考答案 */
    cells: Array<{ given: string; answer?: string }>;
  }>;
}

/** 支持的形式，由多到少：預填克漏字 → 字庫 → 句型框架 → 句首 → 不給支持（SIOP T3） */
export type WsSupportKind = "cloze" | "word_bank" | "sentence_frame" | "sentence_starter" | "none";

/** 差異化學習單的一題在一個版本的部分 */
export interface WsItemTier {
  level: WsLevel;
  support_kind: WsSupportKind;
  /** 克漏字與句型框架挖空前的完整範句（只在教師端） */
  model_sentence: string;
  /** 印在題目下方，一行一項；空格是 ___ */
  support: string[];
  word_bank: string[];
  /** 這個版本預期的答案（只在教師參考版） */
  answer: string;
  /** 這個版本的評分說明（中文，只在教師參考版） */
  scoring: string;
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
  /** 印在它上面的標題（照版型產生時是原學習單的標題）；沒有時印種類的英文標題 */
  heading?: string;
  /** 字詞表鷹架的字與詞性、字義；其他鷹架沒有 */
  bank?: WsWordBank;
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
  /** 完整學習單逐段任務的階段：示範 → 共同練習 → 同儕練習 → 獨立應用（SIOP Q4）；其他沒有這一欄 */
  practice?: WsPractice;
  /** 這一區需要的圖（最多 2 個；伺服器寫的，存檔時原樣送回）。沒有時沒有這個欄位。 */
  image_slots?: WsImageSlot[];
  /** 詞彙級別檢查「提供字義」：印在這一區最後的 Glossary，各自標出印在哪些版本。沒有時沒有這個欄位；存檔時原樣送回。 */
  glosses?: WsGloss[];
  /** 被刪掉的字詞表各列（老師按「刪除」、或請 Manna 拿掉）；「恢復字詞表」用它放回來。沒有時沒有這個欄位；存檔時原樣送回。 */
  vocabulary_removed?: WsVocab[];
}

/** Glossary 的一筆（詞彙級別檢查「提供字義」） */
export interface WsGloss {
  /** 原形，例如 abandon */
  word: string;
  /** 詞性（v.、adj.…），可以是 "" */
  pos: string;
  meaning_zh: string;
  /** 第 1 到目標級別以內的簡短英文解釋，可以是 "" */
  definition_en: string;
  levels: WsLevel[];
  origin: WsOrigin;
}

// ---- 學習單的圖片（docs/API.md「學習單的圖片」）----

/** photo_needed 只出現在舊的學習單（以前「需要真實照片、不產圖」的位置）；現在所有位置都會產圖 */
export type WsImageSlotKind = "scene" | "object" | "icon" | "photo_needed";

/** 區塊裡的一個圖片位置（寫區塊的模型一起寫的） */
export interface WsImageSlot {
  id: string;
  kind: WsImageSlotKind;
  /** 給老師看：這張圖在這裡做什麼 */
  purpose_zh: string;
  /** 給產圖模型的畫面描述 */
  prompt_en: string;
  alt_en: string;
  /** 印在圖下方（"" 表示沒有） */
  caption: string;
}

/** removed：老師拿掉了（不印）；skipped：沒有送去產圖（產圖沒開、沒有檔案儲存、到上限；舊學習單的「需要真實照片」） */
export type WsImageStatus = "queued" | "running" | "done" | "failed" | "skipped" | "removed";
export type WsImageSource = "generated" | "upload" | "none";

/** 一個圖片位置目前的圖 */
export interface WsSlotImage {
  slot_id: string;
  block_id: string;
  status: WsImageStatus;
  source: WsImageSource;
  /** done 時：GET /api/worksheets/:id/images/:image_id */
  image_id: number | null;
  width: number | null;
  height: number | null;
  /** 為什麼沒有圖（給老師看） */
  message: string | null;
  updated_at: string;
}

export interface WorksheetImages {
  /** 這個環境有沒有開 AI 產圖 */
  enabled: boolean;
  /** 這份學習單還能重畫幾次 */
  regenerations_left: number;
  slots: WsSlotImage[];
}

/** 學習單投影裡的一張圖（或空框） */
export interface WsSheetImage {
  slot_id: string;
  status: Exclude<WsImageStatus, "removed">;
  image_id: number | null;
  width: number | null;
  height: number | null;
  alt: string;
  caption: string;
  /** AI 產生的圖（教師版標「AI 生成」） */
  ai_generated: boolean;
  /** 只有教師版：這張圖的用途 */
  purpose?: string;
  /** 只有教師版：為什麼沒有圖 */
  message?: string | null;
}

export type WsPractice = "model" | "guided" | "peer" | "independent";

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
  /** 學生版代碼的順序（T9）：第一個印「Version A」…；差異化學習單產生時隨機排，舊學習單沒有這個鍵（依 levels） */
  student_order?: WsLevel[];
  /** 字詞表放在學習單最後：每一區的字詞表合成一張，印在最後一區之後（預設印在各區裡）；沒有這個鍵就是各區裡 */
  vocabulary_at_end?: boolean;
  /** Key Vocabulary 印哪一種字義：沒有這個鍵就是英文 */
  vocabulary_language?: WsVocabLang;
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
  /** 最近一次產生的步驟（docs/API.md「產生進度」）；在進度功能之前產生的、或還沒產生過是 null；舊的伺服器沒有這一欄 */
  progress?: RunProgress | null;
  /** 有上一版可以復原的區塊 id */
  undoable: string[];
  sop_sync: { linked: boolean; changes: number };
  /** 照哪個版型產生（docs/WORKSHEET_TEMPLATES_CONTRACT.md）；舊的伺服器沒有這一欄 */
  template?: WsTemplateRef | null;
  /** 給 Manna 的要求（≤2000），送給每一步的模型；舊的伺服器沒有這一欄 */
  request?: string;
  /** 各圖片位置目前的圖；舊的伺服器沒有這一欄 */
  images?: WorksheetImages;
  /** 「改編自 …」：拿社群裡別人分享的學習單改的；原件還在分享、開關也開著時才有，否則是 null */
  remixed_from?: CommunityRemixSource | null;
}

// ---- 社群（docs/COMMUNITY.md）----

/** 「改編自 …」：原件的標題，和原作者選擇的顯示名稱（代號或「一位老師」）；沒有別的身分資料 */
export interface CommunityRemixSource {
  id: number;
  title: string;
  owner_name: string;
}

export type CommunityShareName = "code" | "anonymous";

/** 探索、我的收藏的一張卡片，以及預覽的標頭 */
export interface CommunityMeta {
  id: number;
  title: string;
  /** 作者的研究代號，或「一位老師」（作者選匿名時絕不是代號） */
  owner_name: string;
  shared_at: string;
  saves: number;
  remixes: number;
  is_mine: boolean;
  saved: boolean;
  allow_remix: boolean;
  /** worksheet、comprehension、tiered */
  output: string | null;
  levels: WsLevel[];
}

export interface CommunityCard extends CommunityMeta {
  /** 學生版的開頭幾題（縮小畫在封面上）；沒有內容時是 null */
  cover: WsSheet | null;
  /** 只有我的收藏 */
  saved_at?: string;
}

export interface CommunityPage {
  items: CommunityCard[];
  total: number;
  limit: number;
  offset: number;
  /** 只有我的收藏：已經取消分享或刪除、現在看不到的收藏數 */
  unavailable?: number;
}

export type CommunitySort = "newest" | "remixed" | "saved";

export interface CommunityQuery {
  q?: string;
  sort?: CommunitySort;
  output?: string;
  level?: string;
  limit?: number;
  offset?: number;
}

/** 我的分享：自己的一份學習單，和它的分享狀態 */
export interface CommunityMine {
  id: number;
  title: string;
  updated_at: string;
  shared: boolean;
  shared_at: string | null;
  name: CommunityShareName | null;
  allow_remix: boolean;
  saves: number;
  remixes: number;
  output: string | null;
  levels: WsLevel[];
  cover: WsSheet | null;
}

export interface CommunityMinePage {
  items: CommunityMine[];
  total: number;
  limit: number;
  offset: number;
}

export interface CommunityPreview extends CommunityMeta {
  level: WsLevel;
  /** 學生版：學習單上印出來的內容，沒有別的 */
  sheet: WsSheet;
}

export interface CommunityShareRequest {
  shared: boolean;
  name?: CommunityShareName;
  allow_remix?: boolean;
}

export interface CommunityShareState {
  id: number;
  shared: boolean;
  shared_at: string | null;
  name: CommunityShareName | null;
  allow_remix: boolean;
}

export interface CommunitySettings {
  enabled: boolean;
}

export interface WsJobsState {
  running: boolean;
  jobs: WsJob[];
  version_no: number;
  /** 同 WorksheetState.progress；舊的伺服器沒有這一欄 */
  progress?: RunProgress | null;
}

/** 產生進度的一步（docs/API.md「產生進度」） */
export interface ProgressStep {
  /** 同一次產生裡不變：read、tasks、bloom:remember、pictures、siop… */
  key: string;
  /** 給老師看的繁體中文：讀教材、寫逐段任務… */
  label: string;
  status: "pending" | "running" | "done" | "failed";
  /** 進行中的補充，例如「重試中」 */
  note?: string;
  /** 由好幾件組成的一步做了幾件（配圖中 3／6） */
  count?: { done: number; total: number };
  /** 失敗的一步：頁面原本就會顯示的錯誤文字 */
  message?: string;
}

/** 一次產生的進度：依序的步驟、正在做哪一步、結果 */
export interface RunProgress {
  steps: ProgressStep[];
  /** 正在做的那一步（兩部分同時進行時是第一個）；結束後是 null */
  current: number | null;
  status: "running" | "done" | "failed";
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
  /**
   * 編輯器上次看到的版本。和 content 一起送：伺服器已有更新的版本、而 content 又和它不同時不存，
   * 回 409（例如在備課對話採用了逐區修改）。省略時照舊（後存的蓋掉先存的）。
   */
  base_version_no?: number;
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
  worksheet_version_no?: number;
  worksheet_updated_at?: string;
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
  /** 超出目標級別的字與可以怎麼處理；舊的結果沒有（重新分析就有） */
  words?: WsOverLevelWord[];
}

export type WsVocabAction = "gloss" | "replace" | "leave";

/** 一個超出目標級別的字（位置在 analysis.above_target 同一個 base） */
export interface WsOverLevelWord {
  base: string;
  level: number;
  /** 詞彙表印的詞性，例如 "v."、"n./v."；沒有時是 "" */
  pos: string;
  count: number;
  /** 學習單裡的字形（小寫） */
  forms: string[];
  /** 換字會改到的次數（題目、選項、作答指示、鷹架…） */
  replaceable: number;
  /** 換字不會動的地方與原因 */
  fixed: string[];
  /** 課文也有這個字（課文本身不會改） */
  in_material: boolean;
  /** 字詞表裡的字（這節課要教的） */
  key_word: boolean;
  /** 這個版本已經附上的中文意思 */
  glossed: string | null;
  /** 老師之前選了不處理 */
  kept: boolean;
  suggested: WsVocabAction;
  reason: string;
  contexts: string[];
}

export interface WsVocabGlossText {
  pos: string;
  meaning_zh: string;
  definition_en: string;
}

export interface WsVocabWordSuggestion {
  base: string;
  /** 寫出這個建議的那一次呼叫 */
  interaction_id: number | null;
  gloss: WsVocabGlossText | null;
  /** 換字：改寫句主要用來代替它的字（提示；一定在目標級別以內） */
  replacement: string | null;
  /** 換字：有這個字的句子（WsVocabSuggestions.places 的 id） */
  places: string[];
  /** 程式拿掉了什麼（超出級別、改動太大…） */
  notes: string[];
}

/** 換字改的是學習單裡的哪一段文字 */
export interface WsVocabFieldKey {
  block: string;
  part:
    | "title"
    | "instructions"
    | "step"
    | "support"
    | "prompt"
    | "option"
    | "table_heading"
    | "organizer_title"
    | "organizer_label"
    | "hint"
    | "tier_support";
  id?: string;
  index?: number;
}

/** 換字的一句：改寫前 → 改寫後 */
export interface WsVocabPlace {
  /** "p1"、"p2"…（同一次請求裡唯一） */
  id: string;
  field: WsVocabFieldKey;
  /** 「A 活動 · 第 1 題」 */
  place: string;
  /** original 在那段文字裡的位置（字元） */
  start: number;
  end: number;
  original: string;
  /** 這一句要避開的字 */
  bases: string[];
  /** 改寫後的句子；沒有通過檢查時是 null */
  rewritten: string | null;
  interaction_id: number | null;
  /** 為什麼沒有可用的改寫 */
  problem: string | null;
}

export interface WsVocabSuggestions {
  action: "gloss" | "replace";
  status: "shown" | "blocked" | "timeout" | "error" | "disabled";
  message: string | null;
  interaction_id: number | null;
  words: WsVocabWordSuggestion[];
  /** 只有換字：每一句，依學習單的順序 */
  places: WsVocabPlace[];
}

export interface WsVocabWordChoice {
  base: string;
  action: WsVocabAction;
  gloss?: WsVocabGlossText;
  interaction_id?: number | null;
  /** 換字：老師看到幾句有改寫（全部套用＝採用，取消了幾句＝修改） */
  offered?: number;
}

/** 老師勾選要套用的一句，照畫面上的樣子送出 */
export interface WsVocabRewrite {
  field: WsVocabFieldKey;
  start: number;
  end: number;
  original: string;
  rewritten: string;
  bases: string[];
}

export interface WsVocabApplyRequest {
  /** 這次分析的學習單版本（check.version_no） */
  base_version_no: number;
  /** level：只加在分析的版本；all：每個印出這個字的版本 */
  gloss_scope?: "level" | "all";
  words: WsVocabWordChoice[];
  /** 換字：要寫進學習單的句子 */
  rewrites?: WsVocabRewrite[];
}

export interface WsVocabApplied {
  /** 新的學習單版本；只有「不處理」時是 null */
  version_no: number | null;
  glossed: string[];
  /** sentences：改寫了幾句；left：還留著這個字、本來可以換的地方 */
  replaced: { base: string; sentences: number; left: number }[];
  left: string[];
  /** 套用後重新分析的結果 */
  check: WsVocabCheck;
  worksheet: WorksheetState;
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
  /** 原學習單在這一區印出、有自己標題的提示框，依原本順序（沒有時不出現） */
  supports?: WsTemplateSupport[];
  instruction_style: string;
  /** 有題號的題目的樣式；句型框架不算 */
  item_patterns: string[];
  /** 需要老師提供影片、錄音、圖片或另一篇文字 */
  needs_external_material: boolean;
}

export type WsTemplateSupportRole =
  | "directions"
  | "sentence_frame"
  | "sharing_frame"
  | "word_bank"
  | "hints"
  | "guiding_questions"
  | "example";

/** 原學習單的一個提示框（例如 Directions、Sentence Frame for Sharing） */
export interface WsTemplateSupport {
  /** 原學習單上的標題，逐字 */
  heading: string;
  role: WsTemplateSupportRole;
  /** 內容的通用樣式 */
  pattern: string;
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
  /**
   * 刪除這個版型時，原本上傳的檔案（source_material）也一起永久刪除：老師自己為它上傳的範例、
   * 而且沒有別的範本在用。false：沒有原檔，或別的範本還在用（例如另存的版本、平台範本）。
   */
  deletes_source_material: boolean;
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
  /** 有文字（段數不限） */
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
  /** 學生版印的中性代號 "A"／"B"／"C"（不印程度名稱）；只有一個版本時是 null。舊伺服器沒有這個鍵。 */
  version_code?: string | null;
  /** 只有差異化學習單的教師版：分層說明 */
  tiering_note?: string;
  objectives: string[];
  blocks: WsSheetBlock[];
  /** 字詞表放在學習單最後時：各區字詞表合成的一張（各區的 vocabulary 都是空的）；沒有時沒有這個鍵 */
  end_words?: WsSheetWord[];
}

/** Key Vocabulary 的一列（definition 已經是選定語言的字義） */
export interface WsSheetWord {
  word: string;
  level: number | null;
  pos?: string;
  definition: string;
  /** Academic 欄（作答指示裡的功能詞）；沒有這個鍵是 Content 欄 */
  task_word?: boolean;
}

export interface WsSheetBlock {
  id: string;
  kind: WsBlockKind;
  /** "A"、"B"…；Exit ticket 沒有 */
  label: string;
  title: string;
  instructions: string;
  steps: string[];
  /** 印在編號步驟上面的一行（Follow the steps below.）；沒有步驟或舊的伺服器時沒有 */
  steps_lead?: string;
  vocabulary: WsSheetWord[];
  supports: Array<{ type: WsScaffoldType; kind: string; heading?: string; content: string }>;
  items: Array<{
    /** 題目在學習單裡的 id（註記錨在題目上）；舊的伺服器沒有這一欄 */
    id?: string;
    no: number;
    answer_format: string | null;
    prompt: string;
    /** 只有選擇題、是非題、配對題（與沒有作答方式的舊題目）印選項，空白的不印 */
    options: string[];
    lines: number;
    /** 只有表格整理：要印的表格（有欄名時第一列是欄名） */
    table?: WsItemTable;
    answer?: string;
    evidence?: string;
    /** 只有教師版 */
    bloom?: Bloom | null;
    /** 只有基礎版（印成 "Hint: …"） */
    hint?: string;
    /** 這一題自己的句型框架（印在題目下面；沒有時沒有這個鍵） */
    frame?: string;
    /** 差異化學習單：這個版本的支持（不給支持時沒有這個鍵） */
    support?: { kind: WsSupportKind; heading: string; lines: string[]; word_bank: string[] };
    /** 差異化學習單，只有教師版：這個版本的評分說明 */
    scoring?: string;
    /** 圖像組織：這個版本印的圖表（取代作答線）；其他題目與沒有圖表的舊題目沒有這個鍵 */
    organizer?: WsSheetOrganizer;
  }>;
  teacher_notes?: string;
  /** 這一區的圖（印在作答指示之後）；沒有圖時沒有這個欄位 */
  images?: WsSheetImage[];
  /** 這個版本印了簡化文本（調整文本）時，同一區的原文段落（T8）；沒有時沒有這個鍵 */
  original_text?: string[];
  /** 這個版本的 Glossary（詞彙級別檢查「提供字義」），印在這一區最後；沒有時沒有這個鍵 */
  glosses?: Array<{ word: string; pos: string; meaning_zh: string; definition_en: string }>;
}

export interface WsVersionRow {
  version_no: number;
  cause: string;
  created_at: string;
}

// ---- 備課對話（prep sessions）----

/**
 * comprehension／tiered：output 為閱讀理解題／差異化學習單的學習單（像 worksheet）。
 * kit：教學套件＝活動流程（result）＋差異化學習單（worksheet），兩者都好才 done。
 */
export type PrepKind =
  | "worksheet"
  | "assessment"
  | "activity"
  | "resources"
  | "ask"
  | "revise"
  | "chat"
  | "comprehension"
  | "tiered"
  | "kit";
export type PrepPhase = "引起動機" | "發展活動" | "統整與評量";

/** 學習目標的兩組（SIOP）：content＝內容目標、language＝語言目標 */
export type ActivityGoalKind = "content" | "language";

/** 語言目標的四類（SIOP）；Manna 以學習策略優先，所以 strategy 排第一 */
export type LanguageGoalCategory = "strategy" | "function" | "structure" | "vocabulary";

/**
 * 課堂活動的一個學習目標。prep_activity 第 3 版起分成內容目標與語言目標，中英文並陳：
 * zh 給教師（「學生能……」），en 給學生（"Students will be able to …"）。
 * 第 4 版起語言目標多了 category、supports（支持第幾個內容目標，從 1 起）與 relation
 * （「學生要用……，才能……」）；教師提供的目標一字不改，from_teacher 為 true（zh 或 en 可能是 ""）。
 * 之前做的規劃每個目標只有一行中文 text、沒有 kind，照舊顯示成一張清單。
 */
export type ActivityGoal =
  | {
      kind: ActivityGoalKind;
      zh: string;
      en: string;
      curriculum_codes: string[];
      category?: LanguageGoalCategory;
      supports?: number;
      relation?: string;
      from_teacher?: boolean;
    }
  | { kind?: undefined; text: string; curriculum_codes: string[] };

/** 分組方式（SIOP A2） */
export type ActivityGrouping = "全班" | "兩人一組" | "小組" | "個人";
/** 語言技能（SIOP A3） */
export type LanguageSkill = "聽" | "說" | "讀" | "寫";

export interface ActivityStep {
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
  /** 第 4 版起（舊的規劃沒有或是 ""）：分組方式 */
  grouping?: ActivityGrouping | "";
  /** 兩人一組：兩人各做什麼；小組：成員角色與責任；其他是 [] */
  roles?: Array<{ role: string; task: string }>;
  /** 教師巡視；可能是 "" */
  monitoring?: string;
  /** 聽說讀寫，依這個順序 */
  skills?: LanguageSkill[];
  /** 鷹架方式；可能是 "" */
  scaffolding?: string;
}

export interface ActivityPlan {
  title: string;
  /** 先內容目標、再語言目標（第 3 版；第 4 版各 1–2 個）；舊的規劃是一張沒有 kind 的清單 */
  goals: ActivityGoal[];
  /** 第 4 版起（SIOP A6）；舊的規劃沒有這些欄位 */
  key_vocabulary?: Array<{ word: string; zh: string }>;
  /** 教材與教具 */
  materials?: string[];
  /** 高層次問題（英文），層次只會是 analyze／evaluate／create */
  higher_order_questions?: Array<{ question: string; bloom: Bloom }>;
  /** 背景連結：引起動機連結的學生經驗或舊知 */
  background?: string;
  steps: ActivityStep[];
  /** 複習 */
  review?: string;
  /** 評量 */
  assessment?: string;
  curriculum: Array<{ code: string; text: string }>;
}

// ---- SIOP 自我檢核（docs/SIOP_DESIGN_RULES.md 第八節）----

/** page：從產出判斷；partial：只看書面的那一部分（note 說明）；classroom：需課堂觀察，AI 不評 */
export type SiopScope = "page" | "partial" | "classroom";
/** 符合／部分符合／未見 */
export type SiopRating = "met" | "partial" | "not_seen";

export interface SiopCheckItem {
  /** 量表題號："1-2"、"12"… */
  id: string;
  name: string;
  /** 檢核問題；classroom 是 "" */
  question: string;
  scope: SiopScope;
  /** partial：看得到的部分；classroom：為什麼要在課堂上看；page 是 "" */
  note: string;
  /** null：需課堂觀察，或這次沒有判斷 */
  rating: SiopRating | null;
  reason: string;
}

export interface SiopCheck {
  id: number;
  /** 這一輪的第幾次檢核 */
  run_no: number;
  status: "running" | "done" | "failed";
  error: string | null;
  /** 檢核的是學習單的第幾版（學習單與教學套件） */
  worksheet_version_no: number | null;
  /** 量表全部項目，依題號順序（不產生總分） */
  items: SiopCheckItem[];
  interaction_id: number | null;
  created_at: string;
  finished_at: string | null;
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

// ---- 學習單：逐區修改與註記（docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §4–§5）----

/** 註記標在哪裡：整區、標題、作答指示或一題 */
export type WsCommentField = "block" | "title" | "instructions" | "item";
/** 待處理｜已依此修改｜已略過 */
export type WsCommentStatus = "open" | "applied" | "dismissed";

/** 註記現在在學習單的哪裡；lost＝它的區塊或題目已經不在了 */
export interface WsCommentAnchor {
  status: AnchorStatus;
  block_id: string | null;
  item_id: string | null;
  /** 欄位文字中的位置（UTF-16，與瀏覽器選取相同）；整個欄位時是 null */
  start: number | null;
  end: number | null;
}

/** 教師在學習單上的註記：不改學習單，不印、不匯出 */
export interface WorksheetComment {
  id: number;
  worksheet_id: number;
  block_id: string;
  field: WsCommentField;
  item_id: string | null;
  quote: string;
  note: string;
  /** 提供給 AI：逐區修改與重新產生整份時讀得到（預設勾選） */
  share_with_ai: boolean;
  status: WsCommentStatus;
  /** 套用它的那次修改 */
  proposal_id: number | null;
  anchor: WsCommentAnchor;
  created_at: string;
  updated_at: string;
}

export interface CreateWorksheetCommentRequest {
  block_id: string;
  /** 省略：有 item_id 是 item，否則 block */
  field?: WsCommentField;
  item_id?: string;
  start?: number;
  end?: number;
  /** 選取的文字（找不到確切位置時照存，標在整個欄位） */
  quote?: string;
  note?: string;
  share_with_ai?: boolean;
}

export interface UpdateWorksheetCommentRequest {
  note?: string;
  share_with_ai?: boolean;
  status?: "open" | "dismissed";
}

/** 備課對話裡附上的圖片（一律 JPEG）：GET /prep-sessions/{session_id}/images/{id} */
export interface PrepImage {
  id: number;
  session_id: number;
  width: number;
  height: number;
  bytes: number;
}

export type PictureOp = "add" | "redraw" | "remove";

/** 提案裡 Manna 想做的圖片動作；`text` 是伺服器寫好的中文，照原樣顯示 */
export interface WsPictureAction {
  action: PictureOp;
  block_id: string;
  slot_id: string;
  text: string;
}

/** 圖片動作做了沒有（採用之後／直接修改之後） */
export interface PictureOutcome {
  action: PictureOp;
  text: string;
  status: "queued" | "removed" | "skipped";
  message: string | null;
}

export type WsProposalStatus = "running" | "proposed" | "accepted" | "rejected" | "stale" | "failed";

export interface WsProposalChange {
  id: string;
  /** removed 的題目在 before 裡 */
  change: "kept" | "modified" | "new" | "removed";
}

/** 鷹架、字詞表、Glossary 裡被修改、新增或刪除的一項（沒變的不列） */
export interface WsProposalExtraChange {
  group: "scaffold" | "word" | "gloss";
  /** 鷹架的 id；字詞表與 Glossary 是字本身（小寫） */
  id: string;
  /** 鷹架的種類（句型框架…）或字 */
  label: string;
  change: "modified" | "new" | "removed";
}

/** Manna 對一個區塊的修改建議：教師按「採用」才寫入學習單 */
export interface WsProposal {
  id: number;
  worksheet_id: number;
  block_id: string;
  status: WsProposalStatus;
  instruction: string;
  comment_ids: number[];
  whole_block: boolean;
  /** 這一區第幾次請 Manna 改 */
  round: number;
  reply: string;
  /** failed／stale 的原因 */
  error: string | null;
  base_version_no: number;
  /** 送給 Manna 時的區塊 */
  before: WsBlock;
  /** 改好的區塊（proposed 之後才有） */
  block: WsBlock | null;
  changes: {
    title: boolean;
    instructions: boolean;
    items: WsProposalChange[];
    /** 鷹架、字詞表、Glossary 的修改；舊的伺服器沒有這一欄 */
    extras?: WsProposalExtraChange[];
  } | null;
  /** 按「採用」後才會畫圖或移除圖片；舊的伺服器沒有這一欄 */
  picture_actions?: WsPictureAction[];
  /** 這次修改附上的圖片；舊的伺服器沒有這一欄 */
  images?: PrepImage[];
  interaction_id: number | null;
  accepted_version_no: number | null;
  created_at: string;
  finished_at: string | null;
  decided_at: string | null;
  /** 採用後又按「復原」的時間；舊的伺服器沒有這一欄 */
  undone_at?: string | null;
  /** 「再改一次」留下的同一輪較早的建議（舊的在前）；每一次都還可以採用。舊的伺服器沒有這一欄 */
  earlier?: WsProposal[];
}

export interface ReviseWorksheetBlockRequest {
  instruction?: string;
  comment_ids?: number[];
  /** 可以動到這一區的其他題 */
  whole_block?: boolean;
  /** 先用 POST /prep-sessions/:id/images 上傳的圖片 */
  image_ids?: number[];
}

// ---- 修改歷程（docs/API.md「修改歷程與採用過往的版本」）----

export type WsHistoryKind =
  | "original"
  | "proposal_adopted"
  | "proposal_rejected"
  | "regenerate"
  | "prep_revise"
  | "vocabulary"
  | "manual"
  | "pictures"
  | "undo"
  | "restore";

/** 某一步裡一個圖片位置的圖（`done` 才有 image_id） */
export interface WsHistoryPicture {
  slot_id: string;
  image_id: number | null;
  status: WsImageStatus | "none";
  source: WsImageSource;
  purpose_zh: string;
}

export interface WsHistoryStep {
  kind: WsHistoryKind;
  /** 伺服器寫好的白話名稱，照原樣顯示 */
  label: string;
  at: string;
  /** 學習單的第幾版（版本的一步）；沒採用的提案沒有 */
  version_no: number | null;
  proposal_id: number | null;
  /** 這一區現在就是這個樣子 */
  current: boolean;
  /** 一行說明（問了什麼、Manna 說了什麼） */
  detail: string;
  /** 併進這一步的較早版本（連續的手動修改） */
  merged_version_nos: number[];
  /** 採用過的提案後來被復原 */
  undone: boolean;
  /** 沒採用的提案，後來從歷程採用，寫成第幾版 */
  adopted_later_version_no: number | null;
  /** 這一步的區塊（提案則是它會寫成的區塊） */
  block: WsBlock;
  /** 這一步各圖片位置的圖；這一步沒留下圖片紀錄時是 null */
  pictures: WsHistoryPicture[] | null;
  /** 提案還沒做的圖片動作（採用時才會畫） */
  picture_actions: { action: PictureOp; slot_id: string; text: string }[];
  adopt: {
    possible: boolean;
    clean: boolean;
    /** 這個提案是根據比較早的內容改的 */
    warn_older: boolean;
    /** 採用時會請產圖模型畫圖 */
    draws_pictures: boolean;
    pictures_unknown: boolean;
    reason: string | null;
  };
}

export interface WsBlockHistory {
  block_id: string;
  version_no: number;
  /** Manna 正在產生或修改：暫時不能採用 */
  busy: boolean;
  /** 新的在前 */
  steps: WsHistoryStep[];
}

export interface AdoptWsBlockVersionRequest {
  version_no?: number;
  proposal_id?: number;
}

export interface WsBlockAdopted {
  version_no: number;
  kind: "version" | "proposal";
  pictures: PictureOutcome[];
  worksheet: WorksheetState;
}

export interface WsProposalAccepted {
  proposal: WsProposal;
  worksheet: WorksheetState;
  /** 圖片動作的結果；舊的伺服器沒有這一欄 */
  pictures?: PictureOutcome[];
}

/** 備課對話的一輪（工具式對話，docs/PREP_CHAT.md §4）：回答，加上這一輪對學習單做的修改 */
export interface PrepChatChange {
  target: "block" | "picture" | "vocabulary";
  block_id: string | null;
  /** 「C 區」「C 區的圖」「Key Vocabulary」 */
  label: string;
  /** 中文一句：改了標題、第 2 題改成表格題… */
  summary: string;
}

export interface PrepChatResult {
  /** 段落以 \n\n 分隔 */
  answer: string;
  /** ≤3 */
  follow_ups: string[];
  changes: PrepChatChange[];
  not_applied: Array<{ label: string; reason: string }>;
  /** 這一輪寫成的版本；沒有修改是 null */
  version_no: number | null;
  /** 這一輪的修改目前還能整輪復原 */
  undoable: boolean;
  /** 老師已經復原了這一輪；舊的伺服器沒有這一欄 */
  undone?: boolean;
  tool_calls: number;
}

export interface PrepReviseResult {
  reply: string;
  changed_block_ids: string[];
  version_no: number | null;
  pictures?: PictureOutcome[];
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
  /** 我的作品卡片的預覽：這個對話最近的學習單（沒有則 null）。舊的伺服器沒有這一欄。 */
  preview_worksheet?: { id: number; version_no: number } | null;
  /** 沒有學習單圖片時卡片上的幾行字（≤200 字，行與行之間 \n）。舊的伺服器沒有這一欄。 */
  preview_text?: string | null;
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
  result: ActivityPlan | PrepResources | PrepAskResult | PrepReviseResult | PrepChatResult | null;
  /** 這一輪附上的圖片；舊的伺服器沒有這一欄 */
  images?: PrepImage[];
  /** 有幫助／不合用 → POST /api/ai/:interaction_id/decision { decision: "skip", thumbs } */
  interaction_id: number | null;
  created_at: string;
  finished_at: string | null;
  /** 這一輪在等學習單範本的 OCR；其他時候沒有這一欄（Phase 2.10） */
  phase?: "ocr";
  /** 與 phase 一起出現 */
  ocr?: { done_pages: number; total_pages: number };
  /**
   * 最新的 SIOP 自我檢核：學習單（完整學習單、照我的學習單做、閱讀理解題、分層練習）、課堂活動與教學套件
   * 才有這一欄；還沒有檢核（這一輪還在跑、失敗了，或是在檢核功能之前做的）是 null。其他種類與舊的伺服器沒有這一欄
   */
  siop_check?: SiopCheck | null;
  /** 這一輪的步驟（docs/API.md「產生進度」）；找教學資源、直接問 AI 與進度功能之前的輪次是 null；舊的伺服器沒有這一欄 */
  progress?: RunProgress | null;
}

/** 對話的出題選項：之後在對話裡做的每一份學習單都帶入（settings 的同名欄位）。 */
export interface PrepSessionOptions {
  bloom?: Bloom[];
  question_count?: number;
  bloom_counts?: BloomCounts;
  answer_formats?: string[];
  /** 內容目標（一行一項）；舊的對話的 objective 讀成這一欄 */
  content_objective?: string;
  /** 語言目標（一行一項） */
  language_objective?: string;
  /** 只選要用的段落：教材的段落 id；省略＝全部（用 putPrepMaterialSelection 改） */
  paragraph_ids?: string[];
}

export interface PrepSession extends PrepSessionSummary {
  /** seq 順序 */
  turns: PrepTurn[];
  running: boolean;
  /** 伺服器建議的下一步 */
  next: PrepKind[];
  /** 建立或 PATCH 時給的選項；舊的伺服器沒有這一欄 */
  options?: PrepSessionOptions | null;
  /** 教材的段落數；沒有教材是 null；舊的伺服器沒有這一欄 */
  material_paragraphs?: number | null;
  /** 有「只選要用的段落」時，選到的段落還在教材裡的有幾段；全部都用是 null */
  material_selected?: number | null;
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
  /** 哪一頁開始的（只寫進研究紀錄；其他值伺服器會忽略） */
  entry?: "home" | "create";
  /** 帶進這次備課做的每一份學習單（省略＝全部層次、自動題數） */
  options?: {
    bloom?: Array<"remember" | "understand" | "apply" | "analyze" | "evaluate" | "create">;
    question_count?: number;
    /** 每個層次的題數（0–12，合計 3–30）；有它就不看 bloom 與 question_count */
    bloom_counts?: Partial<Record<"remember" | "understand" | "apply" | "analyze" | "evaluate" | "create", number>>;
    /** 固定標籤（選擇題、是非題…）；省略＝自動 */
    answer_formats?: string[];
    /** 內容目標（≤300，一行一項） */
    content_objective?: string;
    /** 語言目標（≤300，一行一項） */
    language_objective?: string;
    /** 只選要用的段落（教材庫那份教材的段落 id）；省略＝全部 */
    paragraph_ids?: string[];
  };
}

// =============================================================================================
// 內容目標與語言目標的草擬（SIOP G2、G3）。docs/API.md「SIOP 目標」。
// =============================================================================================

/** 草擬目標的工具（照我的學習單做是 worksheet） */
export type ObjectiveDraftKind = "comprehension" | "tiered" | "activity" | "kit" | "worksheet";

/** POST /api/objective-drafts */
export interface ObjectiveDraftRequest {
  kind: ObjectiveDraftKind;
  material_id?: number;
  /** 貼上的文字：只用來草擬，不存進教材庫 */
  material?: { title?: string; text: string };
  /** 只選要用的段落（與 material_id 一起）：只讀這些段落 */
  paragraph_ids?: string[];
  class_id?: number;
  request?: string;
  /**
   * 先決定內容目標，再依它擬語言目標：`content`（預設）只擬內容目標；`language` 只擬語言目標，
   * 並用 `content` 帶上教師已決定的內容目標。
   */
  stage?: "content" | "language";
  /** `stage: "language"`：教師已決定的內容目標（1–2 項），語言目標由它們推導 */
  content?: string[];
  /** 「換一組」：上一組的句子，不要重複 */
  avoid?: string[];
  /** 「換掉沒勾的」：教師留下來的目標（代號是目前這一組的 C1、C2、L1、L2，句子是她現在的版本）；新的要和它們搭配 */
  keep?: { code: string; text: string }[];
  /** 頁面自己取的鍵（8–64 個英數、- 或 _），等待時用 GET /api/objective-drafts/progress/:key 看進度 */
  progress_id?: string;
}

/** GET /api/objective-drafts/progress/:key */
export interface ObjectiveDraftProgress {
  /** 還沒開始、別人的鍵、或結束很久之後是 null */
  progress: RunProgress | null;
}

export type ObjectiveCategory = "strategy" | "function" | "structure" | "vocabulary";

export interface ObjectiveDraft {
  /** 決定時的 item_index */
  index: number;
  kind: ObjectiveType;
  /** C1、C2、L1、L2 */
  code: string;
  /** 給學生的 "I can …"：採用時放進欄位的句子 */
  text: string;
  /** 給老師的中文 */
  zh: string;
  /** 語言目標才有 */
  category?: ObjectiveCategory;
  /** 語言目標支持的內容目標代號 */
  supports?: string;
  /** 語言目標才有：「學生要用……，才能……」 */
  relation_zh?: string;
  /** 下課前怎麼檢核 */
  check_zh: string;
}

export interface ObjectiveDraftResult {
  /** 決定記在這一筆：POST /api/ai/:interaction_id/decision */
  interaction_id: number | null;
  status: "shown" | "blocked" | "timeout" | "error" | "disabled";
  message: string | null;
  /** 通過 R1 與 R3 檢查的草稿：內容目標在前 */
  objectives: ObjectiveDraft[];
  /** 沒有通過檢查、沒有列出的數量 */
  hidden: number;
  context: AiContextItem[];
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
  /** chat：要改的學習單（那份學習單的 id）；省略＝這個對話最新的一份 */
  worksheet_id?: number;
  /** 先用 POST /prep-sessions/:id/images 上傳；只用在修改學習單 */
  image_ids?: number[];
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

/** 自由習慣的適用範圍代碼（封閉類型沒有）。 */
export type MemoryScopeCode = "content" | "format" | "layout" | "language" | "scaffold" | "workflow" | "all";
export interface TeacherMemory {
  id: number;
  kind: string;
  value: string;
  statement: string;
  /** 只有自由習慣（kind = "custom"）有。 */
  scope?: MemoryScopeCode | null;
  valid_at: string;
  invalid_at: string | null;
  superseded_by: number | null;
  invalidation_reason: "replaced" | "removed" | null;
  interaction_id: number;
}
export interface MemoryState {
  memories: TeacherMemory[];
  /** 每位老師最多幾項有效習慣（100）。 */
  max_active?: number;
  scopes?: { code: MemoryScopeCode; name: string }[];
  /** 對話中偵測到、還沒回答的建議。 */
  pending?: MemorySuggestion[];
  enabled: boolean;
  graph_enabled: boolean;
  index: { revision: number; indexed_revision: number; error: string | null } | null;
}
export interface MemoryCandidate {
  id: number;
  statement: string;
  replaces_id: number | null;
  replaces_statement?: string | null;
  kind?: string;
  scope?: MemoryScopeCode | null;
}
/** 在備課對話、逐區修改或學習單要求中偵測到的習慣建議（老師按「記住」才會記）。 */
export interface MemorySuggestion {
  id: number;
  kind: string;
  scope: MemoryScopeCode | null;
  statement: string;
  replaces_id: number | null;
  replaces_statement: string | null;
  source: "prep_turn" | "block_revise" | "worksheet_request";
  prep_session_id: number | null;
  prep_turn_id: number | null;
  worksheet_id: number | null;
  proposal_id: number | null;
  created_at: string;
  expires_at: string;
  /** 老師第一次看到建議：附上一次性說明。 */
  intro: boolean;
}
export interface MemorySettings { enabled: boolean; graph_enabled: boolean }

export interface MemoryIndexStatus {
  pending: number;
  failed: number;
  /** false：沒有設定 Graphiti 索引，兩個計數都是 0。 */
  graph_enabled?: boolean;
  /** 目前生效的文字模型與 embeddings 能否供索引使用。 */
  providers_ready?: boolean;
  compatible?: boolean;
  sidecar?: { ok: boolean; api_version: number; graphiti_version: string; policy_version: string } | null;
}
