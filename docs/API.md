# Manna DLS API 合約（Phase 1）

前後端都以這份為準。所有路徑前綴 `/api`，JSON UTF-8。時間為 RFC 3339 字串。

## 共通

- 驗證：`Authorization: Bearer <token>`（登入取得）。未帶或失效 → `401`。
- 錯誤格式：`{ "error": { "code": "not_found", "message": "給人看的中文訊息" } }`
  - code 取值：`bad_request`、`unauthorized`、`forbidden`、`not_found`、`conflict`、`internal`
- 權限：教師只能存取自己的單元／課例；`/api/admin/*` 只限 `role = "pi"`。

### 型別

```ts
type SchoolTrack = "general" | "vocational";        // 普高 | 技高
type Role = "teacher" | "pi" | "ra" | "rater" | "engineer";
type QuestionId = "Q1" | "Q2" | "Q3" | "Q4" | "Q5" | "Q6" | "Q7" | "Q8";
type ResponseStatus = "filled" | "blank" | "no_idea";  // no_idea = 目前沒有想法
type ContentSource = "self" | "self_after_ai" | "modified_ai_example" | "adopted_ai_example";
type IcapLevel = "passive" | "active" | "constructive" | "interactive";

interface Teacher { id: number; research_id: string; school_track: SchoolTrack; role: Role; }

interface Unit {
  id: number; title: string; created_at: string;
  core_status: "none" | "draft" | "completed";
  lesson_count: number;
}

interface CoreResponse { answer: string; status: ResponseStatus; content_source: ContentSource; updated_at: string | null; }
interface CoreFocus { id: number; position: number; big_idea: string; responses: Record<QuestionId, CoreResponse>; }
interface CoreVersion {
  id: number; unit_id: number; phase: "pre" | "post"; version_no: number;
  completed_at: string | null; saw_ai_assist: boolean;
  focuses: CoreFocus[];            // 0–3 個，依 position 排序
}

interface CoreQuestion { id: QuestionId; prompt: string; help: string; }

interface LessonStep { position: number; activity_type: string; icap_level: IcapLevel | null; content: string; }
interface LessonVersion { id: number; version_no: number; objective: string; steps: LessonStep[]; created_at: string; }
interface Lesson { id: number; unit_id: number; title: string; lesson_date: string | null; class_description: string; created_at: string; }
interface LessonDetail extends Lesson {
  latest_version: LessonVersion | null;
  versions_count: number;
  core_focus_ids: number[];        // 本課連結的 CoRe 重點（0–3）
  siop_feature_ids: number[];
  previous_next_change: string | null;   // 前一課的「下次要改」（Phase 2 才有值，先回 null）
}

interface Source { item_code: string; title: string; category: string; source_doc: string; source_page: string; }

// AI 呼叫的共同回傳
interface AiResult<T> {
  interaction_id: number | null;   // AI 停用時為 null
  status: "shown" | "blocked" | "timeout" | "error" | "disabled";
  message: string | null;          // status 不是 shown 時給教師看的中文說明，例如「目前暫無建議，你可以繼續手動完成。」
  output: T | null;                // 只有 shown 時有值
  sources: Source[];               // 本次引用到的知識庫條目（shown 時）
}

interface CoreAssistOutput { level: 1 | 2 | 3; text: string; }
interface PlanSuggestion {
  title: string; body: string;
  activity_type: string | null; icap_level: IcapLevel | null;
  citations: string[];             // item_code，至少一個
}
interface PlanSuggestOutput { kb_empty: boolean; suggestions: PlanSuggestion[]; }  // kb_empty=true 時 suggestions 為空，前端顯示「知識庫查無」
```

## 端點

### 公開
| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/health` | – | `{ ok: boolean, db: boolean }` |
| GET | `/api/config` | – | `{ demo_mode: boolean, ai_enabled: boolean, ai_provider: string, ai_model: string }` |
| POST | `/api/auth/login` | `{ code: string }` | `{ token: string, teacher: Teacher }`；錯誤碼 401 |
| POST | `/api/auth/logout` | – | `{ ok: true }` |

### 教師
| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/me` | – | `Teacher` |
| GET | `/api/units` | – | `Unit[]` |
| POST | `/api/units` | `{ title: string }` | `Unit` |
| GET | `/api/units/:id` | – | `Unit` |
| GET | `/api/core/questions` | – | `CoreQuestion[]`（Q1–Q8 畫面短題目與第一層題意說明） |
| GET | `/api/units/:id/core` | – | `CoreVersion`（第一次呼叫自動建立空白的單元前版本） |
| PUT | `/api/units/:id/core/draft` | `{ focuses: Array<{ id?: number; position: number; big_idea: string; responses: Partial<Record<QuestionId, { answer: string; status: ResponseStatus; content_source: ContentSource }>> }> }` | `CoreVersion`。最多 3 個重點；沒帶 id 的是新增；請求中沒有的既有重點視為移除（軟刪除）。每格變更寫入修訂紀錄。已完成的版本仍可存草稿（教師可回頭修改），不阻擋。 |
| POST | `/api/units/:id/core/complete` | – | `CoreVersion`（允許留白） |
| POST | `/api/core/assist` | `{ unit_id: number; focus_id: number; question_id: QuestionId; level: 1 \| 2 \| 3 }` | `AiResult<CoreAssistOutput>` |
| GET | `/api/units/:id/lessons` | – | `Lesson[]` |
| POST | `/api/units/:id/lessons` | `{ title: string; lesson_date?: string; class_description?: string }` | `Lesson` |
| GET | `/api/lessons/:id` | – | `LessonDetail` |
| POST | `/api/lessons/:id/versions` | `{ objective: string; steps: LessonStep[] }` | `LessonVersion`（每次存檔產生新版本） |
| PUT | `/api/lessons/:id/core-links` | `{ focus_ids: number[] }`（0–3 個，必須屬於同單元） | `{ focus_ids: number[] }` |
| GET | `/api/siop` | – | `Array<{ id: number; internal_id: string; component: string; summary: string }>`（只回已核准） |
| PUT | `/api/lessons/:id/siop` | `{ siop_feature_ids: number[] }` | `{ siop_feature_ids: number[] }` |
| POST | `/api/lessons/:id/suggestions` | `{ request_note?: string }` | `AiResult<PlanSuggestOutput>` |
| POST | `/api/ai/:interaction_id/decision` | `{ item_index?: number; decision: "adopt" \| "revise" \| "reject" \| "answer" \| "skip"; revised_content?: string; reason?: "source_problem" \| "curriculum_mismatch" \| "not_feasible" \| "unclear" \| "better_idea" \| "other"; reason_note?: string; thumbs?: "up" \| "down" }` | `{ ok: true }` |
| POST | `/api/scaffold-events` | `{ scaffold_code: string; action: "show" \| "expand" \| "use" \| "skip"; trigger?: string; dwell_ms?: number }` | `{ ok: true }` |
| GET | `/api/teacher-cases` | query：`q?`（關鍵字） | `Array<{ case_code: string; school_track: "general" \| "vocational" \| "both"; grade: string; topic: string; summary: string; details: string }>`（只回已核准；依教師學校類型過濾，含 both） |

### 主持人（role = pi）
| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/admin/invites` | `{ research_id: string; school_track: SchoolTrack; role?: Role }` | `{ code: string }`（明碼只回這一次，DB 只存雜湊） |
| GET | `/api/admin/prompts` | – | `Array<{ id; prompt_key; version_no; system_text; user_template; created_at }>`（每個 key 的最新版） |
| POST | `/api/admin/prompts` | `{ prompt_key: string; system_text: string; user_template: string }` | 新版本（version_no + 1） |
| POST | `/api/admin/knowledge/import` | `Content-Type: text/csv`，欄位：`item_code,category,school_track,title,content,source_doc,source_version,source_page` | `{ imported: number; sha256: string }` |
| POST | `/api/admin/teacher-cases/import` | `Content-Type: text/csv`，欄位：`case_code,school_track,grade,topic,summary,details,approved` | `{ imported: number; sha256: string }` |

## 鷹架編號（scaffold_code）

| code | 名稱 | 類別 |
|---|---|---|
| `SC-CORE-HELP` | CoRe 需要協助（三層） | teacher_control_fading |
| `SC-CORE` | CoRe 八題 | pck_externalisation |
| `SC-RAG` | 備課 RAG 建議 | content_design_support |
| `SC-SIOP` | SIOP 提示 | content_design_support |
| `SC-CASES` | 其他老師怎麼教 | content_design_support |

---

# Phase 1.1：初次使用設定與備課菜單（設計見 [ONBOARDING.md](ONBOARDING.md)）

以下為**新增或變更**；未提到的端點維持原樣。所有新欄位都可空，舊資料不需搬遷。

## 型別變更

```ts
type PrepRoute = "goal" | "material" | "student" | "activity";   // 目標／教材／學生／活動優先
type Hardest = "goal" | "material" | "differentiation" | "activity" | "worksheet" | "ai_communication";

interface PreferenceAnswers {
  experience: "0-2" | "3-9" | "10+" | null;          // P1
  start_from: PrepRoute | null;                       // P2
  hardest: Hardest[];                                 // P3（可空陣列）
  ai_experience: "none" | "sometimes" | "often" | null; // P4
  detail: "concise" | "medium" | "detailed" | null;   // P5
  class_profile: string;                              // P6（可空字串，不含學生個資）
}
interface Preferences {
  version_no: number; question_set_version: number;
  answers: PreferenceAnswers;
  skipped: boolean;                                   // 初次設定按「略過」
  source: "onboarding" | "settings";
  created_at: string;
}
interface PreferenceQuestion { id: "P1"|"P2"|"P3"|"P4"|"P5"|"P6"; prompt: string; options: Array<{ value: string; label: string }>; multi: boolean; }

// Unit 新增
interface Unit { /* 原欄位… */ prep_route: PrepRoute; class_profile: string; }

// Lesson / LessonDetail 新增
interface Lesson { /* 原欄位… */ my_example: string; }        // 我的範例（自己的舊教案或學習單文字）
interface LessonDetail { /* 原欄位… */ effective_class_profile: string; }  // 課例 class_description 非空時用它，否則用單元 class_profile

// LessonStep 新增（皆可空）
interface LessonStep { /* 原欄位… */ duration_min: number | null; support_note: string; extension_note: string; }

// PlanSuggestion 新增
interface PlanSuggestion { /* 原欄位… */ duration_min: number | null; support_note: string | null; extension_note: string | null; }

// 教材優先：AI 從教材提出的候選學習重點
interface MaterialCandidate { big_idea: string; rationale: string; excerpt: string; }  // excerpt 必須逐字出自教師貼上的教材
interface MaterialFocusesOutput { candidates: MaterialCandidate[]; }  // 3–5 項；教材不足時可為空
interface UnitMaterial { id: number; filename: string; char_count: number; created_at: string; }

// AiContext：每次 AI 回應附上「Manna 已帶入的資訊」，給 P3 選了 ai_communication 的教師看
interface AiContextItem { label: string; preview: string; }   // 例：{label:"班級特性", preview:"高一普通班，…"}
// AiResult<T> 新增欄位：context: AiContextItem[]
```

## 端點

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/config` | – | 原欄位＋`route_choice_enabled: boolean` |
| GET | `/api/preferences/questions` | – | `{ question_set_version: number; questions: PreferenceQuestion[] }` |
| GET | `/api/me/preferences` | – | `Preferences \| null`（null＝尚未做初次設定 → 前端顯示設定精靈） |
| PUT | `/api/me/preferences` | `{ answers: PreferenceAnswers; skipped: boolean; source: "onboarding" \| "settings" }` | `Preferences`（新版本，version_no + 1；不覆蓋舊版） |
| POST | `/api/units` | `{ title; prep_route?; class_profile? }` | `Unit`。未帶時：prep_route 取偏好 start_from（無則 `goal`），class_profile 取偏好 class_profile。`route_choice_enabled=false` 時一律 `goal` |
| PUT | `/api/units/:id` | `{ title?; prep_route?; class_profile? }` | `Unit`。prep_route 變更寫入 usage_logs（event=`prep_route_changed`）；`route_choice_enabled=false` 時改 prep_route 回 409 |
| PUT | `/api/lessons/:id` | `{ title?; lesson_date?; class_description?; my_example? }` | `LessonDetail` |
| POST | `/api/lessons/:id/versions` | steps 帶新欄位 | 同原本 |
| GET | `/api/units/:id/materials` | – | `UnitMaterial[]` |
| POST | `/api/units/:id/materials` | `{ text: string; filename?: string }`（手動貼上的教材文字，上限 50,000 字） | `UnitMaterial` |
| DELETE | `/api/units/:id/materials/:mid` | – | `{ ok: true }`（**實際刪除**原文與切塊，規格 4.5） |
| POST | `/api/units/:id/material-focuses` | `{ material_id: number }` | `AiResult<MaterialFocusesOutput>`；檢核：每項 excerpt 必須是教材原文的子字串、big_idea 含中文 |
| POST | `/api/scaffold-events` | 原欄位；`trigger` 可帶 `preference:hardest=differentiation` 這類值 | 同原本 |
| GET | `/api/admin/settings` | – | `{ route_choice_enabled: boolean }` |
| PUT | `/api/admin/settings` | `{ route_choice_enabled: boolean }` | 同上 |

## AI 行為變更（提示詞 v2，舊版保留）

- 所有 AI 呼叫帶入：有效班級特性、（備課建議）我的範例、教師偏好的 `detail`（concise＝每項正文兩句內；medium＝三句內；detailed＝五句內）。
- CoRe 第二層：偏好 `hardest` 含 `goal` 時，思考面向強調選文理由、語言以外的內容目標、與課綱及考點的連結。仍只給面向、不給答案。
- 備課建議：偏好 `hardest` 含 `differentiation` 時，每項建議必須填 `duration_min`、`support_note`、`extension_note`；其他情況可為 null。時間在前端標示「AI 估計，請依班級調整」。
- 新鷹架：`SC-MATERIAL`（教材優先候選重點，content_design_support）、`SC-PREF`（偏好驅動的預設展開，teacher_control_fading）、`SC-CONTEXT`（Manna 已帶入的資訊，teacher_control_fading）。
