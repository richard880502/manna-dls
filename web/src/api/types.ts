// Mirrors docs/API.md (Manna DLS API 合約，Phase 1). Keep in sync with the contract.

export type SchoolTrack = "general" | "vocational";
export type Role = "teacher" | "pi" | "ra" | "rater" | "engineer";
export type QuestionId = "Q1" | "Q2" | "Q3" | "Q4" | "Q5" | "Q6" | "Q7" | "Q8";
export type ResponseStatus = "filled" | "blank" | "no_idea";
export type ContentSource = "self" | "self_after_ai" | "modified_ai_example" | "adopted_ai_example";
export type IcapLevel = "passive" | "active" | "constructive" | "interactive";

export const QUESTION_IDS: readonly QuestionId[] = ["Q1", "Q2", "Q3", "Q4", "Q5", "Q6", "Q7", "Q8"];

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
}

export interface LessonDetail extends Lesson {
  latest_version: LessonVersion | null;
  versions_count: number;
  core_focus_ids: number[];
  siop_feature_ids: number[];
  previous_next_change: string | null;
}

export interface Source {
  item_code: string;
  title: string;
  category: string;
  source_doc: string;
  source_page: string;
}

export type AiStatus = "shown" | "blocked" | "timeout" | "error" | "disabled";

export interface AiResult<T> {
  interaction_id: number | null;
  status: AiStatus;
  message: string | null;
  output: T | null;
  sources: Source[];
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
}

export interface PlanSuggestOutput {
  kb_empty: boolean;
  suggestions: PlanSuggestion[];
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
}

export interface CreateLessonRequest {
  title: string;
  lesson_date?: string;
  class_description?: string;
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

export type ScaffoldCode = "SC-CORE-HELP" | "SC-CORE" | "SC-RAG" | "SC-SIOP" | "SC-CASES";
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
