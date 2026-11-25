# Manna DLS API 合約（Phase 1）

前後端都以這份為準。所有路徑前綴 `/api`，JSON UTF-8。時間為 RFC 3339 字串。

## 共通

- 驗證：`Authorization: Bearer <token>`（登入取得）。未帶或失效 → `401`。
- 錯誤格式：`{ "error": { "code": "not_found", "message": "給人看的中文訊息" } }`
  - code 取值：`bad_request`、`unauthorized`、`forbidden`、`not_found`、`conflict`、`internal`
- 權限：教師只能存取自己的單元／課例；`/api/admin/*` 只限 `role = "pi"`（Phase 2.0 起，知識庫管理的端點也開放給 `kb_manager`，見「Phase 2.0：知識庫與詞彙」的「角色與權限」）。
- 角色：帳號第一次登入時取邀請碼上的角色；之後主持人可以在後台變更（見文末「帳號與角色」）。每個請求都重新讀取角色，變更後不必重新登入。

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
| GET | `/api/health` | – | `{ ok: boolean, db: boolean, commit: string }`（`commit`：映像建置時的 commit，沒有時為 `"unknown"`；見 README「`/api/health` 的 commit」） |
| GET | `/api/config` | – | `{ demo_mode: boolean, ai_enabled: boolean, ai_provider: string, ai_model: string }`（`ai_provider`、`ai_model` 是目前生效的文字模型，見「文字模型設定」） |
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
| GET | `/api/admin/prompts` | – | `{ prompts: PromptRow[]; groups: string[]; registry: PromptInfo[] }`：每個 key 的最新版（含還沒登記的 key）；`registry`（`api/src/prompt_registry.rs`）是每個 key 的中文名稱、分組、說明、`uses_user_template` 與變數（`required` 必要、`auto` 缺少時程式自動補在訊息最後） |
| GET | `/api/admin/prompts/{key}/versions` | – | `PromptRow[]`：這個 key 的所有版本，新的在前；沒有這個 key → 404 |
| POST | `/api/admin/prompts` | `{ prompt_key; system_text; user_template; note?: string（≤200 字，這次改了什麼）; restored_from?: number（還原自哪一版）}` | 新版本（version_no + 1；從不修改或刪除舊版）＋ `unknown_variables: string[]`（程式不會填的 `{{名稱}}`，只警告、照樣儲存）。系統提示詞空白、使用者範本空白（`uses_user_template = false` 的 key 除外）、note 太長、`restored_from` 不存在 → 400；少了 registry 標為 `required` 的變數 → 400（訊息列出變數）。寫入 `usage_logs` 事件 `prompt_version_created`（`prompt_key`、`version_no`、`restored_from`） |

| POST | `/api/admin/prompts/{key}/try` | `{ system_text; user_template; sample: { passage; question?; class_profile? } }` | **試產生**（預覽）：用還沒儲存的草稿對貼上的短文跑一次真的模型呼叫，回 `{ run_id; preview: true; status: ok｜disabled｜timeout｜refused｜error; message; system; user（實際送出的訊息）; output（模型原始 JSON，未經修補）; checks: [{ name; passed; detail? }]; notes; unknown_variables }`。只支援 `try_keys`（`GET /api/admin/prompts` 回傳）：`prep_ask`、`objective_draft`、`worksheet_warmup`，其他 key → 400；草稿少了必要變數、空白、教材空白 → 400；每位主持人每小時最多 30 次 → 409。**不寫 `ai_interactions`、學習單、`prompt_versions`**，只在 `prompt_try_runs`（`preview` 恆為 true；範例教材只存字數與雜湊） |
`PromptRow` = `{ id; prompt_key; version_no; system_text; user_template; created_at; note; restored_from; created_by_research_id（只給研究編號，不給姓名）; by_me }`。
| POST | `/api/admin/knowledge/import` | `Content-Type: text/csv`，欄位：`item_code,category,school_track,title,content,source_doc,source_version,source_page` | `{ imported: number; sha256: string }`（Phase 2.0 起多了選填欄位、`dry_run` 與更多回應欄位，並開放給 `kb_manager`） |
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
| POST | `/api/units` | `{ title; prep_route?; class_profile? }` | `Unit`。未帶時：prep_route 取偏好 start_from（無則 `goal`），class_profile 依文末「班級特性沿用（R3）」（上一個單元，第一個單元才取偏好 class_profile）。`route_choice_enabled=false` 時一律 `goal` |
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

---

# Phase 1.2：Teach & Record（課中書籤與課後補寫）

規格 §4.3（課中書籤）、§4.4 表格中的「結束本節課並回顧」（課後補寫）、§3（`lesson_runs`、`record_events`、`record_event_revisions`）、§6.1、§6.4、§7.1。以下為**新增**；未提到的端點維持原樣。

原則：
- 課中不呼叫 AI；不提供錄音、拍照；補寫與「加一句話」旁提示「不要寫學生姓名」。
- 書籤先存裝置端、時間戳在裝置端產生，恢復連線立即同步；伺服器以 `(lesson_run_id, device_event_id)` upsert，重送不會重複；裝置時間原樣保存，另存伺服器收到時間 `received_at`。
- 原始書籤保留；復原、改 👍／⚠、改一句話、刪除、課後補寫都新增一筆 `record_event_revisions`；刪除一律軟刪除。教師端只看到最新狀態（已刪除的不回傳）。
- 沒有書籤、未補寫、全部留白都不阻擋完成（§6.4）。

## 型別

```ts
type Polarity = "good" | "stuck";                       // 👍 順利 | ⚠ 卡住
type DeviceType = "phone" | "tablet" | "desktop";
type RecordVia = "console" | "pip" | "remote";          // 課堂紀錄頁 | 子母畫面小視窗 | 手機遙控器
// 學生可觀察行為：聆聽、筆記、回答、提問、解釋、討論、相互追問、共同完成任務、其他（可複選）
type StudentBehavior = "listening" | "note_taking" | "answering" | "asking" | "explaining"
                     | "discussing" | "probing" | "co_completing" | "other";
// 處理方式：重新解釋、補充例子、調整提問、改變活動、調整時間、稍後處理（單選，可不選）
type Handling = "re_explain" | "add_example" | "adjust_questions" | "change_activity" | "adjust_time" | "defer";

interface LessonRun {                 // 一次上課（lesson_runs）
  id: number; lesson_id: number; lesson_title: string; unit_id: number; unit_title: string;
  started_at: string; ended_at: string | null;        // ended_at = null：進行中
  review_completed_at: string | null;                  // 按「完成課後補寫」的時間（可不按）
  good_count: number; stuck_count: number;             // 未刪除的書籤數
}

interface RecordEvent {               // 書籤的最新狀態
  id: number;
  device_event_id: string;            // 裝置端產生的 ID（UUID）
  device_ts: string;                  // 裝置端時間戳，原樣保存
  received_at: string;                // 伺服器第一次收到的時間
  polarity: Polarity; note: string;   // 一句話（可空）
  device_type: DeviceType; via: RecordVia;
  // 課後補寫（全部選填）
  activity_step_id: number | null;    // 活動歸屬：lesson_steps.id（同一課例的教案步驟）
  behaviors: StudentBehavior[]; handling: Handling | null;
  followup_text: string; followup_at: string | null;
  revision_count: number;             // 更正紀錄筆數
}

interface PlanStepRef extends LessonStep { id: number; }   // 帶 id 的教案步驟（給活動歸屬用）

interface RemoteDevice { id: number; device_type: DeviceType; paired_at: string; last_seen_at: string | null; }

interface RunDetail extends LessonRun {
  events: RecordEvent[];              // 不含已刪除；依 device_ts、id 排序
  plan: { lesson_version_id: number; version_no: number; objective: string; steps: PlanStepRef[] } | null;  // 課例最新教案
  remotes: RemoteDevice[];            // 目前有效的手機遙控器
}

// 裝置端操作紀錄（離線佇列）。每個操作帶 op_id（UUID），重送時伺服器依 op_id 判斷已處理過。
type RecordOp =
  | { op_id: string; kind: "create"; device_event_id: string; device_ts: string; polarity: Polarity;
      note?: string; device_type: DeviceType; via?: "console" | "pip" }
  | { op_id: string; kind: "edit"; device_event_id: string; polarity?: Polarity; note?: string; at?: string }
  | { op_id: string; kind: "undo"; device_event_id: string; at?: string }     // 當下復原誤按
  | { op_id: string; kind: "delete"; device_event_id: string; at?: string };  // 刪除誤按（課中或課後）

interface SyncOpResult {
  op_id: string;
  status: "applied" | "duplicate" | "rejected";   // duplicate＝之前已處理過（重送）；rejected＝不合法，裝置端丟棄
  code?: string; message?: string;                // rejected 時的原因
}
interface SyncResponse { results: SyncOpResult[]; events: RecordEvent[]; ended_at: string | null; }

interface Pairing { pairing_id: number; code: string; expires_at: string; }   // code 只回這一次
interface RemoteRun { run_id: number; lesson_title: string; started_at: string; ended_at: string | null; events: RecordEvent[]; }  // events 只含這支遙控器按的書籤
```

## 端點

### 教師（Bearer 登入權杖；只能存取自己課例的上課紀錄，其他人的回 404）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/runs` | – | `LessonRun[]`（進行中的在前，其餘依開始時間新到舊；最多 100 筆） |
| GET | `/api/lessons/:id/runs` | – | `LessonRun[]`（該課例的所有上課紀錄，新到舊） |
| POST | `/api/lessons/:id/runs` | – | `LessonRun`。「開始本節課」：建立 lesson_runs（記下當時的最新教案版本）。同一課例已有進行中的上課 → 409 |
| GET | `/api/runs/:id` | – | `RunDetail` |
| POST | `/api/runs/:id/end` | – | `LessonRun`。「結束本節課並回顧」：設定 ended_at；重送不改變第一次的結束時間 |
| POST | `/api/runs/:id/events/sync` | `{ ops: RecordOp[] }`（依裝置端發生順序，最多 500 個） | `SyncResponse`。create 以 `(run, device_event_id)` upsert：已存在回 duplicate、不覆蓋原始內容；edit／undo／delete 以 op_id 去重。課程結束後教師仍可同步與修改（課後更正） |
| PUT | `/api/runs/:id/events/:device_event_id/review` | `{ activity_step_id: number \| null; behaviors: StudentBehavior[]; handling: Handling \| null; followup_text: string }` | `RecordEvent`。課後補寫；內容有變才新增一筆 review 修訂；已刪除的書籤 → 409；步驟必須屬於同一課例 |
| POST | `/api/runs/:id/review/complete` | – | `LessonRun`。「完成課後補寫」：不檢查是否填寫（§6.4）；進行中 → 409 |
| POST | `/api/runs/:id/pairing` | `{}` | `Pairing`。產生手機遙控 QR 配對碼（10 分鐘、單次使用）；同時撤銷這節課還沒用過的舊配對碼；課程已結束 → 409 |
| DELETE | `/api/runs/:id/pairing` | – | `{ ok: true; revoked_pairings: number; revoked_remotes: number }`。撤銷未使用的配對碼與所有已配對的遙控器 |

### 手機遙控器（不需登入）

QR Code 內容為 `<網站>/remote#pair=<code>`：配對碼放在 `#` 之後，不會送到伺服器紀錄或 Referer。

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/remote/pair` | `{ code: string; device_type: DeviceType }` | `{ token: string; run: RemoteRun }`。配對碼無效、已用過、已過期、已撤銷 → 401（訊息分別說明）；課程已結束 → 409 |
| GET | `/api/remote/run` | Bearer 遙控權杖 | `RemoteRun` |
| POST | `/api/remote/events/sync` | Bearer 遙控權杖；`{ ops: RecordOp[] }` | `SyncResponse`（events 只含這支遙控器的書籤） |

遙控權杖的範圍：
- 只限配對的那一節課；不能呼叫任何教師端點（回 401），教師的登入權杖也不能當遙控權杖。
- 只能 `create`（via 一律記為 `remote`）與 `undo` **自己按的**書籤；`edit`、`delete`、復原別的裝置按的書籤 → 該操作 rejected（code `forbidden`）。
- 課程結束後不能再新增或復原；但為了離線的手機，結束後 12 小時內仍收「結束前按下」的書籤（`device_ts` ≤ 結束時間＋5 分鐘時鐘誤差），其餘 rejected（code `run_ended`）。結束 12 小時後或被撤銷 → 401。

## 工程決定（規格 §7.1）

| 項目 | 決定 |
|---|---|
| 離線書籤：裝置端 ID | UUID v4（`crypto.randomUUID`；非 HTTPS 環境以 `crypto.getRandomValues` 產生）。每個修改操作另有 op_id |
| 重複同步防止 | `record_events` 唯一鍵 `(lesson_run_id, device_event_id)`；修訂以 `(record_event_id, client_op_id)` 唯一。重送只會得到 duplicate |
| 時間戳 | 裝置端產生，伺服器原樣保存；另存 `received_at`（伺服器第一次收到）。排序用裝置時間 |
| 重試 | 按下就先寫進裝置（localStorage，失敗時退回記憶體），立即同步；失敗時在恢復連線（`online` 事件）與 5→10→30→60 秒間隔重試；畫面顯示待同步筆數 |
| 衝突處理 | 依裝置端操作順序套用；同一書籤的後一個修改覆蓋前值，每次都留修訂；已刪除的書籤不能再修改（rejected） |
| 本機清除 | 只有伺服器回覆該操作（applied／duplicate／rejected）後才從裝置刪除；401／404 等錯誤時保留在裝置、停止自動重試並提示。Safari 會清除久未使用網站的暫存，畫面提示「加入主畫面」並盡快連線同步 |
| 開始本節課 | 需要連線（lesson_runs 由伺服器編號）；開始之後的書籤可離線 |
| QR 配對 | 配對碼 256 位元隨機、只存 SHA-256；10 分鐘有效；單次使用；重新產生會撤銷舊碼；教師可一鍵撤銷配對碼與已配對的手機 |
| 跨裝置登入 | 手機不登入教師帳號；只拿到這節課的遙控權杖（只存雜湊），看不到其他資料 |
| 「加一句話」自動收起 | 8 秒未動作（前端常數 `NOTE_IDLE_SECONDS`，待主持人定案）；收起時已打的字照樣存；不擋下一次標記 |

## 鷹架

| code | 名稱 | 類別 |
|---|---|---|
| `SC-RECORD` | 課中書籤與課後補寫 | low_burden_evidence |

前端記錄：課堂紀錄頁顯示（show）、第一次按書籤（use）、打開 QR 遙控或子母畫面（expand，trigger `qr`／`pip`）、課後補寫頁顯示（show，trigger `review`）、儲存補寫（use，trigger `review`）。

---

# UX 審查 R1–R3（`docs/UX_REVIEW.md`，`docs/ux-review` 分支；已核准的三項）

以下為**新增或變更**；未提到的端點維持原樣。

## 班級特性沿用（R3／決定 D2）

新單元的班級特性預設沿用教師**上一個單元**，第一個單元才用偏好問卷 P6。`POST /api/units` 沒帶 `class_profile` 時：

1. 該教師**最近建立**、未刪除（`deleted_at IS NULL`）、班級特性不是空白的單元 → 沿用它的 `class_profile`（來源 `previous_unit`）。
2. 沒有這樣的單元 → 偏好最新版的 P6 `class_profile`（不是空白時為 `preference`）。
3. 都沒有 → 空字串（`none`）。

有帶 `class_profile` 時一律以請求為準，空字串也是（來源 `teacher`）。只看同一位教師自己的單元；只是複製教師自己寫過的文字，不經 AI。

```ts
type ClassProfileSource = "teacher" | "previous_unit" | "preference" | "none";

// Unit 新增
interface Unit { /* 原欄位… */ class_profile_source: ClassProfileSource | null; }
// 建立單元時班級特性的來源；此功能之前建立的單元為 null。之後用 PUT 修改班級特性不會改變它

interface ClassProfileDefault {
  class_profile: string;
  source: Exclude<ClassProfileSource, "teacher">;
  source_unit: { id: number; title: string } | null;   // 只有 previous_unit 時有值
}
```

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/units/class-profile-default` | – | `ClassProfileDefault`：現在建立新單元、不帶 `class_profile` 時會用的班級特性與來源。建立表單用它顯示可修改的預填內容，例如「沿用上一個單元『L3 倒裝句』的班級特性，可修改」 |
| POST | `/api/units` | 同原本 | `Unit`。依上方規則決定預設；來源記在 `units.class_profile_source`，沿用的單元記在 `units.class_profile_source_unit_id`，供研究追溯 |

前端：教師沒有修改預填內容時不帶 `class_profile`（由後端套用預設並記錄來源）；修改過才帶上修改後的文字（記為 `teacher`）。

## 課例頁首屏與收合（R1）

課例頁首屏只有「今天要教什麼？」（教案的 `objective`）與正下方的「開始本節課」。CoRe 連結、教學步驟（含時間與差異化欄位）、上課紀錄、本課背景、SIOP 提示、其他老師怎麼教都收合，標題帶數量（例如「教學步驟（3）」「連結的 CoRe 重點（2）」）；課例已有步驟時「教學步驟」預設展開。沒有新增端點。

`POST /api/scaffold-events` 新的 trigger 值：

| scaffold_code | action | trigger | 時機 |
|---|---|---|---|
| `SC-SIOP` | `show` | `collapsed` | 課例頁載入、有 SIOP 提示：收合的「SIOP 提示（n）」出現（原本是內容直接顯示，沒有 trigger） |
| `SC-SIOP` | `expand`／`skip` | `section` | 教師展開／收合該區 |
| `SC-CASES` | `show` | `collapsed` | 同上，「其他老師怎麼教（n）」 |
| `SC-CASES` | `expand`／`skip` | `section` | 同上 |
| `SC-PREF` | `show` | `preference:hardest=differentiation` | 改為在「教學步驟」展開、差異化欄位真的出現時才記（原本在頁面載入時記） |

其他既有紀錄（`SC-RAG` 的 request／show／use／skip、`SC-SIOP` 的 use 與 `more`、`SC-CASES` 的個別案例與 `search`、`SC-PREF` 的 `manual:differentiation` 與 `preference:hardest=material`）不變。

## 不會默默丟掉沒存的內容（R2）

- 「開始本節課」：教案有未儲存的修改時，先 `POST /api/lessons/:id/versions` 存成新版本，再 `POST /api/lessons/:id/runs`（上課紀錄因此記到正確的教案版本）；存檔失敗就不開始，並顯示原因。本課背景有未儲存的修改時一併先 `PUT /api/lessons/:id`。
- 「完成課後補寫」：有改過但沒按「儲存這張書籤」的書籤，先逐張 `PUT …/review`，全部成功才 `POST /api/runs/:id/review/complete`；任何一張失敗就不完成。
- 課例頁與課後補寫頁有未儲存的內容時，站內換頁先在頁面上確認（儲存後離開／留在這頁／不儲存，直接離開），關閉或重新整理分頁由瀏覽器提醒（`beforeunload`）。
- 仍是教師按鈕觸發的存檔，「每次存檔產生新版本」不變；沒有改動時不會多存一個版本。

---

# Phase 1.4：班級與備課庫

以下為**新增或變更**；未提到的端點維持原樣。Migration `0006_classes.sql`。

## 模型

```
教師 ─┬─ 備課庫：單元設計（單元 ＋ 單元前 CoRe ＋ 課例與教案）   一份設計，可給多個班級用
      └─ 班級（高一 3 班、高一 7 班…）
            └─ 取用（class_units）：班級 ↔ 單元，多對多、班級內有順序、可附「這班的備註」
                  └─ 上課紀錄（lesson_runs.class_id）：同一個課例在 A 班、B 班各有各的紀錄、書籤與補寫
```

- **備課庫**是教師自己已經準備好的範本集合。單元設計只有一份：在 A 班修改教案，B 班看到的是同一份（不做各班分支；各班差異先用「這班的備註」記下）。
- **班級**只有名稱、年級、學校類型、課程名稱、班級特性、學年度與學期。**不存任何學生資料。** 班級只封存、不刪除。
- **取用**：班級從備課庫取用單元。從班級移除單元是軟移除（`class_units.removed_at`），單元設計留在備課庫、在該班上過的紀錄也保留；重新加入會新增一筆取用紀錄。
- **上課紀錄屬於班級**：`lesson_runs.class_id`。同一個課例、同一個班級同時只能有一節進行中的課；不同班級可以同時進行。
- 「不同的課」指同一科目的不同單元（L1、L2…）。教選修等其他課程的教師，可在班級填選填的課程名稱（`course_label`）。

## 型別

```ts
interface UnitClassRef { id: number; name: string; course_label: string; class_profile: string; note: string; }  // note＝這個取用的「這班的備註」

// Unit 新增（GET /api/units 即備課庫清單）
interface Unit { /* 原欄位… */ classes: UnitClassRef[]; }   // 取用這個單元、未封存的班級，依教師的班級順序

type ClassProfileOrigin = "teacher" | "preference" | "migration" | "none";
// teacher＝建立時教師填的；preference＝第一個班級沿用偏好 P6；migration＝0006 由舊單元帶入；none＝空白

type ClassNextAction =
  | { kind: "resume_run"; run_id: number; lesson_id: number; lesson_title: string; unit_id: number; unit_title: string }  // 這班有進行中的課
  | { kind: "start_lesson"; lesson_id: number; lesson_title: string; unit_id: number; unit_title: string }  // 依單元順序，這班第一個還沒上過的課例
  | { kind: "plan_unit"; unit_id: number; unit_title: string }   // 下一個單元還沒有課例
  | { kind: "add_unit" };                                         // 班級還沒有單元

interface SchoolClass {
  id: number; name: string;                 // 例：「高一 3 班」
  grade: string;                            // 例：「高一」（可空）
  school_track: SchoolTrack;                // 預設教師的學校類型
  course_label: string;                     // 選填，例：「英文閱讀選修」
  class_profile: string;                    // 班級特性（不含學生個資，上限同 P6：2,000 字）
  class_profile_source: ClassProfileOrigin;
  school_year: string; term: string;        // 選填，例：「115」「上學期」
  position: number; created_at: string;
  archived_at: string | null;               // 封存時間；null＝使用中
  unit_count: number;                       // 目前取用的單元數
  next_action: ClassNextAction | null;      // null＝取用的單元都上完了
}

interface ClassUnit {                       // 一個班級取用的一個單元
  unit: Unit;
  position: number; note: string; assigned_at: string;
  lessons_total: number;
  lessons_taught: number;                   // 在這個班級至少上過一次的課例數
  last_run_at: string | null;               // 這個班級最近一次上這個單元的時間
  next_lesson: { id: number; title: string } | null;   // 這個班級還沒上過的第一個課例
}
interface ClassDetail extends SchoolClass { units: ClassUnit[]; }   // 依班級內順序

// LessonRun（與 RunDetail）新增
interface LessonRun { /* 原欄位… */ class_id: number | null; class_name: string | null; }   // null＝沒有班級的上課紀錄

// LessonDetail 新增
type ClassProfileSourceUsed = "lesson" | "class" | "unit" | "preference" | "none";
interface LessonDetail { /* 原欄位… */ class_profile_source: ClassProfileSourceUsed; }   // effective_class_profile 取自哪裡
```

## 端點

教師只能存取自己的班級；別人的班級一律 404（包含把它當成 `class_id` 帶入其他端點）。

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/classes` | query：`include_archived?=true` | `SchoolClass[]`。依教師排的順序；預設不含已封存，帶 `include_archived=true` 時封存的排在最後 |
| POST | `/api/classes` | `{ name: string; grade?; school_track?; course_label?; class_profile?; school_year?; term? }` | `SchoolClass`。`name` 必填（60 字內）。未帶 `school_track` 用教師的學校類型。未帶 `class_profile`：教師的**第一個班級**沿用偏好 P6（`preference`），之後的班級為空白（`none`）；有帶（含空字串）以請求為準（`teacher`） |
| GET | `/api/classes/:id` | – | `ClassDetail`（已封存的也可以讀） |
| PUT | `/api/classes/:id` | `{ name?; grade?; school_track?; course_label?; class_profile?; school_year?; term? }` | `SchoolClass`。只改有帶的欄位；變更的欄位名稱寫入 usage_logs（event=`class_updated`） |
| POST | `/api/classes/:id/archive` | – | `SchoolClass`。封存：不出現在清單與單元的 `classes`，資料與上課紀錄都保留；重送不改變第一次的封存時間。**沒有刪除班級的端點** |
| POST | `/api/classes/:id/unarchive` | – | `SchoolClass` |
| PUT | `/api/classes/order` | `{ class_ids: number[] }` | `SchoolClass[]`（未封存）。列出的班級依序排在最前，沒列出的維持原相對順序排在後面 |
| POST | `/api/classes/:id/units` | `{ unit_id: number; note?: string }` | `ClassDetail`。「從備課庫加入單元」：排在班級最後。已取用則不重複新增（有帶 `note` 時更新備註）。班級已封存 → 409 |
| PUT | `/api/classes/:id/units/:unit_id` | `{ note: string }`（500 字內） | `ClassDetail`。這班的備註，例：「這班用基礎版」。未取用 → 404 |
| DELETE | `/api/classes/:id/units/:unit_id` | – | `ClassDetail`。從這個班級移除（軟移除）；單元留在備課庫，這班上過的紀錄保留。未取用時不做事 |
| PUT | `/api/classes/:id/units/order` | `{ unit_ids: number[] }` | `ClassDetail`。同班級排序規則；含未取用的單元 → 404 |
| GET | `/api/units` | – | `Unit[]`（含 `classes`）＝備課庫清單 |
| POST | `/api/units` | 原欄位＋`class_id?: number` | `Unit`。帶 `class_id` 時：單元建在備課庫，同時由該班級取用（一步完成）；班級已封存 → 409。**教師只要有任何班級**，未帶 `class_profile` 時單元的 `class_profile` 就留空（來源 `none`）：班級特性在班級上，不再沿用上一個單元（R3 的沿用只適用於還沒有班級的教師） |
| GET | `/api/units/class-profile-default` | – | 教師有任何班級時一律回 `{ class_profile: "", source: "none", source_unit: null }` |
| POST | `/api/lessons/:id/runs` | `{ class_id?: number \| null }`（可不帶請求內容） | `LessonRun`。班級的決定見下方。同一課例在同一班級已有進行中的課 → 409 |
| GET | `/api/runs` | query：`class_id?` | `LessonRun[]`。帶 `class_id` 只回該班級的上課紀錄 |
| GET | `/api/lessons/:id/runs` | query：`class_id?` | 同上（該課例） |
| GET／PUT | `/api/lessons/:id` | query：`class_id?` | `LessonDetail`。`effective_class_profile` 與 `class_profile_source` 依下方優先順序計算 |
| POST | `/api/lessons/:id/suggestions` | `{ request_note?; class_id?: number }` | 同原本 |
| POST | `/api/core/assist` | 原欄位＋`class_id?: number` | 同原本 |
| POST | `/api/units/:id/material-focuses` | `{ material_id; class_id?: number }` | 同原本 |

### 開始本節課時的班級

`POST /api/lessons/:id/runs`：

1. 有帶 `class_id`：必須是自己的班級（否則 404）、未封存、且有取用這個課例的單元（否則 409）。
2. 沒帶（或 null）：看取用這個單元、未封存的班級數——
   - **0 個**：建立沒有班級的上課紀錄（`class_id = null`）。不阻擋。
   - **1 個**：預設就是那個班級。
   - **多個**：409（訊息請教師選擇是哪一班），由前端先讓教師選好再帶 `class_id`。

### AI 帶入的班級特性

有帶 `class_id`（在班級情境中工作）時，班級特性的優先順序：

**課例的班級描述（覆寫）＞ 班級的班級特性 ＞ 單元的 `class_profile`（舊資料）＞ 偏好 P6**

沒帶 `class_id` 時維持原行為（課例描述，否則單元 `class_profile`；不使用 P6）。偏好 P6 另外只作為教師第一個班級的預設內容。

`AiResult.context` 的班級特性項目標示實際用的是哪一段：

| 情境 | label |
|---|---|
| 沒帶 `class_id` | `班級特性` |
| 用班級的班級特性 | `班級特性（高一 3 班）`（班級名稱） |
| 用課例的班級描述 | `班級特性（本課覆寫）` |
| 用單元的舊設定 | `班級特性（單元設定）` |
| 用偏好 P6 | `班級特性（備課偏好）` |

每筆 AI 互動的輸入快照（`ai_interactions.input_snapshot`）多一個 `class_context: { class_id: number | null, class_profile_source }`。其餘紀錄（`ai_interactions`、`teacher_decisions`、`scaffold_events`、`usage_logs`）不變。

## 舊資料搬遷（`0006_classes.sql`）

對每一位**已經有單元**的教師：

1. 建立一個預設班級「我的班級」：學校類型取教師的；班級特性取該教師**最近建立、未刪除、班級特性不是空白**的單元的 `class_profile`（來源 `migration`；都沒有則空白）。
2. 該教師所有未刪除的單元都由「我的班級」取用，依建立時間由舊到新排序。
3. 既有的上課紀錄（`lesson_runs`）都掛到「我的班級」。

沒有單元的教師不會有預設班級；第一次進工作檯時會被邀請建立第一個班級（可略過，預填偏好 P6）。單元原本的 `class_profile` 原樣保留。

## usage_logs 新事件

`class_created`（`{class_id, class_profile_source}`）、`class_updated`（`{class_id, fields}`）、`class_archived`、`class_unarchived`、`class_unit_assigned`、`class_unit_removed`（`{class_id, unit_id}`）。

## 前端

改版第 1 步（`docs/DESIGN_OVERHAUL.md`）之後的畫面；**API 沒有變動**，只是用法不同：

- **班級（首頁）** `/`：最上面只有一件事，「接下來」那一班的下一節與「開始上課」。沒有課表，所以挑選順序是：有課正在上的班級（`next_action.kind = resume_run`）→ `GET /api/runs` 裡最近上過課、且有下一節可上的班級 → 第一個有下一節的班級 → 第一個有單元待備課的班級。下面是「我的班級」（一班一列）與「上完還沒補寫」（`GET /api/runs` 中已結束、`review_completed_at` 為 null 的課）。沒有班級時邀請建立第一個班級（可略過）。
- **班級頁** `/classes/:id`：這班的下一節與「開始上課」、這班的單元（順序、進度、這班的備註；排序與移出收在「更多」）、上課紀錄（`GET /api/runs?class_id=`，補寫／回顧）、班級特性、班級資料與封存。
- **備課庫** `/library`（`/plan` 亦同）：單元一列一個（課例數、單元想法寫到哪、哪些班在用）、「新增單元」、「加到班級」。
- **單元頁** `/units/:id`：課例清單（哪些班上過，取自 `GET /api/runs`）與旁欄「單元想法」（CoRe）。`core_status = "none"` 的單元不會呼叫 `GET /api/units/:id/core`（那個端點第一次呼叫會建立空白版本），旁欄只顯示空白題目。
- **班級是情境，不是欄位**：從班級進入單元或課例時，那一班就是這次的班級，只在麵包屑出現一次，頁面上沒有切換班級的欄位；存在瀏覽器的 sessionStorage（`manna.currentClass`）。從備課庫進入時清除；單元只有一個班級取用時自動視為該班。帶 `class_id` 的呼叫（課例、建議、CoRe 協助、教材候選）都用這個班級。
- **班級只問一次**：從首頁或班級頁按「開始上課」直接帶該班的 `class_id`。只有從備課庫進到課例、而單元有多個班級取用時，按「開始上課」才跳出「這節課在哪一班上？」對話框（預設選這節課正在上的班，其次是還沒上過這一節的班）。單元沒有班級取用時照舊建立沒有班級的上課紀錄。伺服器不接受「多個班級取用卻不指定班級」（409），所以對話框沒有「不記在任何班級」的選項。
- **上課紀錄** `/record`：所有上課紀錄依班級分組（含沒有班級的），只供補寫與回顧；不再從這裡開始上課。上課畫面與課後補寫頁顯示班級。
- 已移除的頁面（`/reflect`、`/share`、`/profile`、`/community`）一律導回首頁；`/worksheets`（清單）轉到 `/prep`「我的作品」。

---

# Phase 2.0：知識庫與詞彙

以下為**新增或變更**；未提到的端點維持原樣。Migration `0007_knowledge_base.sql`。背景與擷取品質見 [`LLM_VALIDATION.md`](LLM_VALIDATION.md) §3、§8。

## 重點

- **課綱條目是真的了**：`knowledge_items` 的課綱條目帶官方代碼、學習表現／學習內容、類別、學習階段、科目、`*`／`◎` 標記，以及來源文件、版本、頁碼、來源 PDF 的 SHA-256 與匯入批次。
- **條文一律由資料庫帶出**。模型只能引用代碼；顯示給教師的條文、來源、版本、頁碼都用 `GET /api/knowledge/curriculum/by-code` 查。查不到的代碼列在 `not_found`，前端必須標示「尚未完成課綱連結」，不得補寫條文（規格 §2.1 第 9 點）。
- **詞彙級別一律查表**。`POST /api/vocabulary/analyze` 是純規則比對，不呼叫模型。
- **只停用、不刪除**：內容有變的列成為新版本，舊版本設 `retired_at`。重複匯入同一份檔案不會產生任何變動。
- 課綱與詞彙表的**資料不在 repo 內**（授權未確認）；資料表一開始是空的，由知識庫管理頁匯入。測試一律用合成的假資料。

## 角色與權限

新角色 `kb_manager`（介面顯示「知識庫管理員」）：在同一個網站內管理知識庫，沒有主持人的其他權限。主持人在後台建立邀請碼時選這個角色；**已經登入過的帳號**，主持人也可以在後台「帳號與角色」改成這個角色（或改回來），對方不必重新登入（見文末「帳號與角色」）。

| 能力（capability） | 內容 | `pi` | `kb_manager` | `teacher`／`ra`／`rater`／`engineer` |
|---|---|---|---|---|
| `administer` | 邀請碼、帳號與角色、提示詞版本、平台設定、教師備課經驗匯入（碰到身分、研究資料或 AI 行為的都算） | ✓ | ✗（403） | ✗（403） |
| `manage_knowledge` | 課綱條目與詞彙表的匯入、預覽、停用示範條目、現況、查看條目 | ✓ | ✓ | ✗（403） |
| （登入即可） | 自己的班級、單元、課例；課綱查詢；詞彙分析 | ✓ | ✓ | ✓ |

- 後端只在一處決定角色有哪些能力（`auth::role_can`），端點用 `teacher.require(Capability::…)` 檢查；之後新增能力（例如教材管理）是多一個列舉值與一行對應，不必到處比對角色字串。
- `kb_manager` 也是一個帳號（有研究編號與學校類型），可以用自己的備課功能，但**看不到其他教師的任何資料**：教師端點本來就只回本人的資料（別人的回 404）。
- `GET /api/me` 多回 `capabilities: ("administer" | "manage_knowledge")[]`；前端依它決定顯示哪些入口（`web/src/lib/permissions.ts`，登入回應沒有這個欄位時依角色推得）。
- 「共通」一節的「`/api/admin/*` 只限 `role = "pi"`」改為：下表標示「知識庫管理」的端點需要 `manage_knowledge`，其餘 `/api/admin/*` 需要 `administer`。

## 型別變更

```ts
type SchoolTrack = "general" | "vocational" | "comprehensive";   // 新增綜合型高中（綜高）
type ItemTrack = SchoolTrack | "both";                            // 知識庫條目、教師備課經驗；both＝各類型共用
type Role = "teacher" | "pi" | "kb_manager" | "ra" | "rater" | "engineer";
interface Teacher { /* … */ capabilities?: ("administer" | "manage_knowledge")[] }

interface KnowledgeItem {
  item_code: string;        // 匯入用的唯一代碼，例如 108-G-3-V-7（普高）、108-V-…（技高）、108-C-…（綜高）、外語-技-英語IV-2
  version: number;
  category: string;         // curriculum | grammar | vocab | siop | …
  school_track: ItemTrack;
  title: string;            // 課綱條目：標記＋官方代碼＋學習表現／學習內容｜類別
  content: string;          // 課綱條目：官方條文
  official_code: string;    // 來源上印的代碼，例如 ◎3-Ⅴ-7；非課綱條目為 ""
  item_kind: "" | "learning_performance" | "learning_content";
  strand_name: string;      // 例如 語言能力（讀）
  stage: string;            // 學習階段 II | III | IV | V；"" 表示不適用
  subject: string;          // 英語文、英語、職場、日語…
  advanced: boolean;        // * 較高階
  recurring: boolean;       // ◎ 跨階段重複
  source_doc: string; source_version: string; source_page: string;
}
```

綜合型高中：`teachers`、`invite_codes`、`classes`、`knowledge_items`、`teacher_cases` 的 `school_track` 都接受 `comprehensive`，彼此一致。綜高有自己的領綱（112 條），所以綜高教師查到的是綜高條目加上 `both` 的共用條目，不會混入普高或技高的條文。

## 端點

### 教師（登入即可）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/knowledge/curriculum` | query：`q?`（關鍵字）、`track?`（預設教師的學校類型）、`kind?`（`learning_performance` \| `learning_content`）、`stage?`（`II`–`V` 或 `all`，預設 `V`）、`subject?`（科目或 `all`，預設英語相關科目）、`limit?`（預設 20，最多 100） | `{ track; filter: { kind; stage; subject }; ranked: boolean; items: { item: KnowledgeItem; score: number; title_score: number }[] }` |
| GET | `/api/knowledge/curriculum/by-code` | query：`codes`（逗號分隔，最多 50 個）、`track?`（預設教師的學校類型；`all` 表示不限） | `{ track; items: KnowledgeItem[]; not_found: string[] }` |
| POST | `/api/vocabulary/analyze` | `{ text: string; target_level?: 1–6（預設 4）; phrases?: string[]; list_version?: string }` | `VocabularyAnalysis`（見下）；尚未匯入詞彙表 → `409` |

**課綱檢索**（`api/src/retrieval.rs`）

- 範圍：教師的學校類型＋`both`；只取未停用條目的最新版本；只取有官方代碼的課綱條目（內建的 `DEMO-CUR-*` 示範條目自稱「不是課綱原文」，不會出現在這兩個端點）。
- 預設只找**高中（第五學習階段）、英語相關科目**（`英語文`、外語群的 `英語`、`職場`）。知識庫收了全部 1,352 條，其中普高領綱含國小、國中階段 254 條，外語群含日語、商業概論、數位科技等 508 條；這些要用 `stage`、`subject` 明確指定，或用代碼直接查。教案建議（`POST /api/lessons/:id/suggestions`）檢索時套用同一個預設範圍。
- 排序沿用 Phase 1：英文詞（小寫、去停用詞、簡單複數還原）加中文二字組，分數＝命中的不同 token 數＋標題命中數；同分依代碼。沒有可用關鍵字時依代碼順序列出（`ranked: false`）。
- **依代碼查**：`item_code`（唯一）一定查得到；官方代碼（`3-Ⅴ-7` 或 `3-V-7` 皆可，`*`、`◎`、空白忽略）在 `track` 範圍內比對，因為三份領綱的官方代碼彼此重複。只做完全比對，沒有模糊比對。

**詞彙級別分析**（`api/src/vocabulary.rs`，移植自 `research/sop-validation/vocab/analyze.py`）

```ts
interface VocabularyAnalysis {
  wordlist_version: string;            // 使用的詞彙表版本
  target_level: number;
  basis: {
    counted_tokens: number;            // token_ratio 的分母：對應得到詞彙表的詞次
    counted_types: number;             // type_ratio 的分母：對應得到詞彙表的不同詞
    denominator_note: string;          // 分母的中文說明
    excluded_tokens: { proper_nouns: number; unknown: number; multiword: number; appendix: number };  // 未計入分母
    match_types: { exact?: number; inflection?: number; derived?: number };
  };
  level_distribution: { level: 1|2|3|4|5|6; tokens: number; token_ratio: number; types: number; type_ratio: number }[];
  above_target: { base: string; level: number; count: number;
                  positions: { form: string; line: number; col: number; context: string;
                               match: "exact" | "inflection" | "ambiguous" | "derived";
                               also_listed_level?: number; rule?: string }[] }[];   // 級別高的在前
  ambiguous_forms: { word; base; level; also_listed_level }[];   // 詞形歧義，取較低級
  derived_matches: { word; base; level; rule }[];                // 規則衍生的推定對應（re-、non-、un-、-less、-ly、-ness）
  multi_level_words: { base: string; levels: number[]; used_level: number }[];   // 一詞多級，取最低級
  proper_nouns: Listed[];   // 專有名詞
  unknown: Listed[];        // 未收錄或無法判定（occurrences 有 note 與 context）
  multiword: Listed[];      // 連字號詞、複合詞、呼叫端提供的片語（occurrences 有 kind 與 component_levels）
  appendix_words: Listed[]; // 附錄詞彙：數字、星期、月份、國名…（occurrences 有 category）
  limits: string[];         // 給教師看的資料限制
  disclaimer: string;       // 「不是大考中心的官方評定…」
}
interface Listed { word: string; count: number; occurrences: { line: number; col: number; /* … */ }[] }  // 最多列 20 處
```

- 詞形還原只靠規則：原形比對 → 不規則變化表 → 詞尾規則（-s/-es/-ies、-ed、-ing、重複子音、-er/-est、所有格、縮寫）。`line`、`col` 從 1 起算，`col` 以字元計。
- 顯示比例時**必須同時顯示分母與未計入的數量**（規格 §9.6）；`basis` 與四個另列清單就是為此。
- 文字上限 100,000 字元；`phrases` 最多 200 個。**不儲存文字**，`usage_logs` 只記數量（`vocabulary_analyzed`）。
- 已知限制（回應的 `limits` 也會帶）：不判斷詞性與詞義（`broke`、`learned` 取較低級並標示）；詞彙表沒有片語，除非由 `phrases` 提供否則偵測不到；句首大寫的未收錄詞無法判定是不是專有名詞；英式拼法與不在表內的不規則變化會落入未收錄；未收錄詞不計入任何一級，只看比例會低估難度；還原正確率尚未以人工標註語料量測。
- 和研究用腳本的兩處差異（270 份學習單文字逐一比對，其餘輸出相同）：`-ed`／`-ing` 優先選動詞時改為比對完整的詞類（原腳本把 `adv.` 當成含 `v.`，會把 `pasting` 還原成 `past`）；全文開頭有空白時，第一個詞仍視為句首。

### 知識庫管理（`pi` 或 `kb_manager`）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/admin/knowledge/import` | `Content-Type: text/csv`；query：`dry_run?=true`。必要欄位同 Phase 1；選填欄位：`official_code, item_kind, strand, strand_name, stage, subject, advanced, recurring, source_sha256` | `ImportResult` |
| POST | `/api/admin/vocabulary/import` | `Content-Type: text/csv`；query：`dry_run?=true`。欄位：`word, headword, level, pos, kind, appendix_category, list_version, source_doc, source_page, source_sha256` | `ImportResult` |
| GET | `/api/admin/knowledge/summary` | – | `{ knowledge: { active; real_active; demo_active; groups: { school_track; category; item_kind; demo; count }[] }; vocabulary: { active_forms; groups: { list_version; level \| null; forms; entries }[] }; recent_batches: { id; kind; file_sha256; row_count; inserted; updated; unchanged; imported_by; created_at }[] }` |
| POST | `/api/admin/knowledge/retire-demo` | query：`dry_run?=true` | `{ dry_run; demo_active; demo_by_category; real_active; categories_left_empty: string[]; retired }`；沒有任何正式條目時 `409` |
| GET | `/api/admin/knowledge/items` | query：`q?`（代碼、標題或內文）、`category?`、`track?`、`kind?`、`status?`（`active` 預設 \| `retired` \| `all`）、`limit?`、`offset?` | `{ total; items: (KnowledgeItem & { source_sha256; import_batch_id; created_at; retired_at })[] }` |
| GET | `/api/admin/vocabulary/items` | query：`q?`（詞形或詞條的開頭）、`level?`（`1`–`6` \| `appendix`）、`status?`、`limit?`、`offset?` | `{ total; items: VocabularyRow[] }` |

```ts
interface ImportResult {
  dry_run: boolean;
  rows: number;                 // 檔案列數
  inserted: number;             // 第一次出現的代碼／詞形
  updated: number;              // 內容有變：新增一版，舊版停用
  unchanged: number;            // 和目前的最新版相同：不動
  by_track?: Record<string, number>;   // 知識庫
  by_level?: Record<string, number>;   // 詞彙表；"appendix"＝附錄
  sha256: string;               // 上傳的 CSV 的 SHA-256
  batch_id: number | null;      // 沒有任何變動或 dry_run 時為 null
  imported: number;             // 沿用 Phase 1：實際受理的列數（dry_run 為 0）
}
```

**匯入規則**

- 全有或全無；任何一列不合格就整份拒絕，錯誤訊息指出第幾列。
- 先 `dry_run=true` 看 `inserted`／`updated`／`unchanged`，再正式匯入。檔案沒列到的條目不會被動到（不會因為漏列而停用）。
- 知識庫條目以 `item_code` 為單位；詞彙表以（`list_version`、`word`、`headword`、`pos`、`kind`、`appendix_category`）為單位。
- `source_sha256` 是**來源文件**（PDF）的 SHA-256；知識庫 CSV 沒有這一欄時沿用 Phase 1 的行為，存 CSV 本身的 SHA-256。上傳檔案的 SHA-256 記在 `import_batches.file_sha256`。
- 詞彙表一列是一個**可比對的詞形**：`a/an` 展開成 `a`、`an` 兩列，`agree(ment)` 展開成 `agree`、`agreement`，`headword` 保留原本印的詞條。`kind`：`base` \| `variant` \| `derived` \| `appendix`；`appendix`（附錄詞彙，未分級）的 `level` 留空。

**示範條目**：內建的 16 條 `DEMO-*` **不會自動停用**，由主持人或知識庫管理員按「停用示範條目」。選這個做法的原因：示範條目除了 3 條假的課綱描述，還有文法、詞彙教學、SIOP 共 13 條，課綱匯入並不會取代它們；自動停用會讓教案建議可引用的內容在沒有人決定的情況下改變。預覽會列出停用後哪些類別會沒有任何條目；知識庫還沒有正式條目時拒絕停用。

## 匯入檔

由 `research/sop-validation/kb/` 的腳本產生，放在 repo 外（`~/manna-data/kb/import/`）：

```bash
python kb/ingest_curriculum.py <PDF 資料夾>     # → curriculum_items.csv
python kb/ingest_vocab.py <詞彙表 PDF>          # → ceec_vocab.csv
python kb/build_import_csv.py                   # → import/curriculum_import.csv、import/vocabulary_import.csv、import/import_manifest.json
```

| 檔案 | 列數 | 內容 |
|---|---|---|
| `curriculum_import.csv` | 1,352 | 普高 401（含國小、國中階段 254）、技高 839（語文領域 120＋外語群 719）、綜高 112 |
| `vocabulary_import.csv` | 6,373 | 6,012 個詞條展開成 6,183 個詞形，加附錄詞彙 190 個 |

全部 1,352 條都匯入的理由：四份文件擷取到的代碼與全文出現的代碼完全一致（無遺漏、無多出）；外語群 719 條另以逐頁文字比對，711 條逐字相同，其餘 8 條只差在比對用文字的換行位置。與高中英文無關的條目靠檢索的預設範圍排除，而不是不匯入，這樣任何被引用的代碼都查得到條文。普高領綱有 8 條跨頁接續的條目（3-Ⅴ-13、Ae-Ⅲ-1、Ae-Ⅳ-1、B-Ⅳ-3、B-Ⅴ-2、B-Ⅴ-15、D-Ⅲ-3、D-Ⅳ-2），建議匯入後在管理頁對一次原文。

## 資料表

- `knowledge_items` 新增 `official_code, item_kind, strand, strand_name, stage, subject, advanced, recurring, import_batch_id`。
- `vocabulary_items`（新）：`word, headword, level（1–6，附錄為 NULL）, pos, kind, appendix_category, list_version, source_doc, source_page, source_sha256, version, import_batch_id, retired_at`。
- `import_batches`（新）：每次**有變動**的匯入一筆：`kind, file_sha256, row_count, inserted, updated, unchanged, imported_by, created_at`。
- `teachers`、`invite_codes` 的 `role` 多 `kb_manager`；五個資料表的 `school_track` 多 `comprehensive`。

## usage_logs 新事件

誰（`teacher_id`）、何時（`created_at`）之外，`payload` 為：

| 事件 | payload |
|---|---|
| `knowledge_imported`、`vocabulary_imported` | `{ batch_id, sha256, rows, inserted, updated, unchanged }`（每次正式匯入都記，沒有變動也記；`dry_run` 不記） |
| `demo_knowledge_retired` | `{ retired, by_category }` |
| `vocabulary_analyzed` | `{ wordlist_version, target_level, counted_tokens, above_target, unknown }`（不含文字） |

## 前端

- **知識庫管理** `/knowledge`：只有 `pi` 與 `kb_manager` 在頂端列看得到入口。其他角色直接開這個網址會看到「這個頁面只開放給計畫主持人與知識庫管理員」，頁面不會呼叫任何管理端點（呼叫了也是 403）。內容：目前內容（各學校類型的課綱條目數、詞彙表各級詞條數、示範條目與「停用示範條目」、最近的匯入與檔案 SHA-256）、匯入課綱條目、匯入詞彙表（選檔 →「預覽」→「確認匯入」；一次只有一個匯入在等確認）、查看條目與查看詞彙表（只能看，可篩選，一次 20 筆）。
- **後台管理** `/admin`（只有 `pi`）：邀請碼的角色多「知識庫管理員」、學校類型多「綜高」；知識庫匯入移到知識庫管理頁，其餘不變。
- 初次使用的備課偏好問卷不會擋在知識庫管理頁前面。
- 教師端目前還沒有課綱查詢與詞彙分析的畫面；API 用法已在 `web/src/api/client.ts`（`searchCurriculum`、`curriculumByCode`、`analyzeVocabulary`），等課例 SOP 與學習單產生器接上。

## 尚未有的資料

- **學測、統測歷屆考題：完全沒有。** 規格 §9.6 的「歷屆考題能力對應度」無資料可做。
- 各來源文件的**官方原始連結**：交付的 PDF 沒有附網址，系統目前只能顯示文件名稱、版本與頁碼，不由工程端或模型填寫網址。
- 技高、綜高兩份語文領綱的發布日期（PDF 內未標示）與詞彙表的使用授權，待計畫確認。
# Phase 2.1：課例 SOP（七步備課；改版第 2 步）

以下為**新增或變更**；未提到的端點維持原樣。Migration `0008_lesson_sop.sql`。依據：《學習單產生器：教師選項與 SOP 整合規格》§1–§3、§8、§9.5，`docs/DESIGN_OVERHAUL.md` 方向 A，`docs/LLM_VALIDATION.md`。

## 與既有教案的關係

- **既有教案（`lesson_versions`：`objective` ＋ `lesson_steps`）不變**，仍是「今天要教什麼？」與教學步驟，也仍是上課與課後補寫使用的教案（`lesson_runs.lesson_version_id`、書籤的活動歸屬）。舊課例不需要任何 SOP 內容就能開啟、編輯、開始上課。
- **課例 SOP 是另一份、獨立版本化的文件**（`lesson_sop_versions`）。沒有任何欄位必填；沒有 SOP 內容的課例 `GET …/sop` 回傳空白文件。
- **舊的單一目標不自動改寫成 SOP 目標**：舊欄位沒有區分內容目標與語言目標，規格 §2.1(7)、§9.5 也不允許自動寫入。`GET …/sop` 會把最新教案的 `objective` 放在 `today_objective`；第 3 步把它顯示成「你在『今天要教什麼？』寫的」，由教師一鍵放進內容目標或語言目標（來源記為 `teacher`）。AI 推薦目標時也會把它當作教師已寫的內容帶入。
- 開始上課時另外記下當時的 SOP 版本（`lesson_runs.lesson_sop_version_id`，可為 null），供研究追溯。

## 模型

```ts
type SopOrigin = "teacher" | "ai_adopted" | "ai_edited";
type InfoFunction = "discover" | "organize" | "analyze" | "compare" | "create";   // 發現／整理／分析／比較／創造資訊
type ExitKind = "formative" | "reflection" | "questions";                         // 形成性評量／學生反思／蒐集問題

interface SopParagraph { id: string; no: number; text: string }   // id 例如 "p3"：同一段文字在教材改版後沿用原 id
interface SopMaterial {
  id: number; version_no: number; title: string; source_note: string; text: string;
  paragraphs: SopParagraph[]; word_count: number; created_at: string;
}

interface SopFeature { id: string; feature: string; evidence: string; explanation: string }
interface SopParagraphAnalysis {
  paragraph_id: string; function: string; summary: string; features: SopFeature[]; origin: SopOrigin;
}
interface SopLanguageFocus { id: string; kind: string; item: string; paragraph_id: string | null; evidence: string; note: string }
interface SopAnalysis {
  text_type: string; main_idea: string; structure: string; overall_origin: SopOrigin;
  language_focus: SopLanguageFocus[]; paragraphs: SopParagraphAnalysis[];
}

interface SopCurriculumLink {
  item_code: string; why: string;     // item_code 是知識庫的唯一代碼（例如 108-G-3-V-7）
  // 以下一律由伺服器依 item_code 從 knowledge_items 帶出（Phase 2.0 的精確代碼查詢）；用戶端送來的值會被忽略
  official_code: string;              // 來源上印的代碼（例如 3-Ⅴ-7），畫面顯示這個
  title: string; text: string; category: string; source_doc: string; source_version: string; source_page: string;
}
interface SopObjective {
  id: string; type: "content" | "language"; text: string; student_facing_en: string; reason: string;
  paragraph_ids: string[]; links: SopCurriculumLink[];
  link_status: "linked" | "unlinked";      // 由伺服器依 links 是否為空決定
  origin: SopOrigin;
}

interface SopParagraphGoal { paragraph_id: string; objective_ids: string[]; note: string }       // 第 4 步
interface SopScaffolds { verbal: string; instructional: string; procedural: string }              // 語言／教學／程序鷹架
interface SopTask { id: string; function: InfoFunction | null; text: string; scaffolds: SopScaffolds; origin: SopOrigin }
interface SopTaskBlock { id: string; paragraph_ids: string[]; tasks: SopTask[] }                  // 一段或合併數段
interface SopWarmup {
  background: Array<{ id: string; text: string; scaffolds: SopScaffolds; origin: SopOrigin }>;    // 喚醒背景知識
  vocabulary: Array<{ id: string; word: string; note: string; origin: SopOrigin }>;               // 關鍵字彙
}
interface SopExitTicket { items: Array<{ id: string; kind: ExitKind; text: string; origin: SopOrigin }> }

interface SopContent {
  analysis: SopAnalysis; objectives: SopObjective[]; paragraph_goals: SopParagraphGoal[];
  task_blocks: SopTaskBlock[]; warmup: SopWarmup; exit_ticket: SopExitTicket;
}

interface SopState {
  lesson_id: number;
  version_no: number | null;         // null：還沒有任何 SOP 版本
  versions_count: number;
  saved_at: string | null;
  material: SopMaterial | null;
  content: SopContent;               // 沒有版本時是空白文件
  today_objective: string;           // 最新教案的 objective（見上）
  jobs: { analysis: SopJob | null; objectives: SopJob | null };   // 各自最新一次的 AI 工作
}
```

所有欄位都可留白或省略（省略＝空字串／空陣列）。`id` 由用戶端產生（文件內唯一即可，≤ 40 字元）。指到已不存在段落的 `paragraph_id` 會被保留（教師改教材不會默默刪掉分析或任務），前端標示為「原段落已變更」。

## 教材與段落

- 教材目前只收貼上的文字（≤ 50,000 字）。**段落由程式切分**（貼上的文字與上傳的 TXT 同一套規則）：
  1. 有空行時以空行分段（段內換行視為空白）。
  2. 沒有空行時，**預設每一行一段**；但文字看起來是「一行一行折斷」的（從 PDF 或 e-mail 複製的）時，改用 PDF 的分段規則接回段落（`sop::wrap`）：行尾連字號斷字接回（`experi-` ＋ `ment` → `experiment`，下一行要以小寫字母開頭）；一行以句末標點（`. ? ! " ” ’ 。？！」』…`）結尾、而且明顯短於最長行（< 80%）就結束一段；單獨一行的短標題（≤ 12 字、不以句末標點結尾、< 最長行的 60%）自成一段；中文行與行之間不補空格（中文字寬算 2）。
  3. 「看起來是折斷的」要同時符合（寧可不接，有疑問就每行一段）：至少 5 行；最長一行寬 ≥ 40（中文字算 2）；清單項目（`1.`、`-`、`•`、`(a)`）少於 40% 的行；至少 3 行、且至少 40% 的行**沒有**以句末標點結尾；這些行中至少 3/4 的長度 ≥ 最長行的 70%；這些行的下一行至少 40% 不是以大寫字母開頭。詩、清單、對話、一句一行的文字都不符合，維持每行一段。
  4. 空白段落略過。已經存下的教材**不會**重新分段（沒有搬遷）；新貼上的文字與之後的修改用這個規則。修改教材（`PUT /api/materials/:id` 的 `text`）時重新分段，文字（空白正規化後）沒變的段落沿用 `id`，所以註記與選過的段落都還在。
  5. `POST /api/materials/split-preview` `{ text }` → `{ paragraph_count }`：貼上的文字會被切成幾段（不存、不記錄；首頁用它決定要不要提醒教材很長）。需要「使用教材」權限；超過 50,000 字 → 400。
- 段落 `id` 穩定：新教材中文字（空白正規化後）與前一版相同的段落沿用原 `id`，其餘給新的 `p<n>`（同一課例內不重複使用）。`no` 是目前的順序（從 1）。
- 教材與 SOP 內容分開存（Phase 2.2 起：教材是教材庫的一份教材，就地修改、不保留舊文字；`lesson_sop_materials` 已刪除），SOP 版本以 `material_id` 指向它，所以自動儲存不會重複存整篇教材。

## 端點

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/lessons/:id/sop` | – | `SopState` |
| PUT | `/api/lessons/:id/sop/material` | `{ title?, source_note?, text }` | `SopState`。教材有變才新增教材列，並產生一個指向它的新 SOP 版本（內容不變）；完全相同時不產生版本。`text` 空白＝移除教材（SOP 內容保留）。**Phase 2.2 起教材改存在教材庫、就地修改，並可用 `material_id` 從教材庫選，見「Phase 2.2」** |
| PUT | `/api/lessons/:id/sop` | `{ content: SopContent }` | `{ version_no, versions_count, saved_at, content }`（`content` 是正規化後的結果：課綱條文與來源已由資料庫帶出）。與最新版本相同時不產生新版本。課綱連結的 `item_code` 在知識庫找不到 → 400（已停用的條目仍可讀，教師先前存的連結不會因此存不了） |
| GET | `/api/lessons/:id/sop/versions` | – | `Array<{ version_no, created_at, material_version_no: number \| null }>`，新到舊 |
| GET | `/api/lessons/:id/sop/versions/:version_no` | – | `SopState`（該版本；`jobs` 皆為 null） |
| POST | `/api/lessons/:id/sop/analysis` | `{ class_id? }` | 202 `SopJob`：逐段教材分析（第 2 步） |
| POST | `/api/lessons/:id/sop/objectives` | `{ class_id?, only_type?: "content" \| "language" }` | 202 `SopJob`：SIOP 目標推薦＋課綱連結（第 3 步）。內容目標先決定，語言目標之後才依它推薦，所以一次只推薦一種：`content` 推薦 2–3 項內容目標；`language` 要這一課已有內容目標（沒有 → 400），依教師已決定的內容目標推薦 2–3 項語言目標（user message 最後附上它們）；沒帶時這一課沒有內容目標就推薦內容目標，否則推薦語言目標。`sop_objectives` 第 2 版起 system text 有這個流程；結果與 `check_results` 檢查的是「要的那一種」（`asked_siop_type_present`），不再要求兩種都有 |
| GET | `/api/sop-jobs/:id` | – | `SopJob` |

教師自己連結課綱條目時，前端用 Phase 2.0 的 `GET /api/knowledge/curriculum`（不呼叫 AI）搜尋，把選到的 `item_code` 放進目標的 `links` 再照常儲存；只有正式課綱條目（有 `official_code`）可以連結，知識庫中手寫的示範條目不行。

版本只增不改（append-only）；`version_no` 每個課例從 1 連號。所有端點只能存取自己的課例（其他人的回 404）。

## AI：只有驗證過的兩步

```ts
interface SopJob {
  id: number; lesson_id: number; kind: "analysis" | "objectives";
  status: "running" | "done";
  phase: string;                       // 給教師看的進度，例如「正在讀教材」「補上缺少的段落」；完成（done）後為空字串
  started_at: string; finished_at: string | null;
  sop_version_no: number | null;       // 這次工作讀的是哪一版 SOP
  result: AiResult<SopAnalysisSuggestion | SopObjectivesSuggestion> | null;   // done 時才有
  decided: number[];                   // 教師已經決定過的 item_index（重新整理後不再顯示）
}

interface SopAnalysisSuggestion {
  text_type: string; main_idea: string; structure: string;
  language_focus: Array<{ kind: string; item: string; paragraph_id: string; evidence: string; note: string }>;
  paragraphs: Array<{ paragraph_id: string; paragraph_no: number; function: string; summary: string;
                      features: Array<{ feature: string; evidence: string; explanation: string }> }>;
  missing_paragraph_ids: string[];     // 模型兩次都沒有交出的段落（教師可以自己寫）
  dropped_evidence: number;            // 因為不是原文連續片段而被拿掉的證據筆數
}
interface SopObjectivesSuggestion {
  objectives: Array<{ type: "content" | "language"; text: string; student_facing_en: string; reason: string;
                      paragraph_ids: string[]; link_status: "linked" | "unlinked"; links: SopCurriculumLink[] }>;
  curriculum_available: number;        // 這次提供給模型挑選的課綱條目數（0＝知識庫沒有適用條目）
}
```

### 長時間呼叫：背景工作＋輪詢

一次呼叫實測 15–50 秒、偶爾 200 秒，加上一次修復就可能超過 nginx 的 180 秒 proxy 逾時，所以**不在 HTTP 請求裡等模型**：`POST` 只建立 `lesson_sop_jobs` 一列、啟動背景工作並立刻回 202；前端每 2 秒 `GET /api/sop-jobs/:id`，顯示 `phase` 與已經過的秒數，教師可以繼續做別的步驟。

- 同一課例、同一種工作還在跑時再 `POST`，回傳同一個工作（不重複呼叫模型）。
- 每次模型呼叫仍受 `AI_TIMEOUT_SECS` 限制（國網中心模型建議設 120 以上）；一個工作最多兩次呼叫。
- 伺服器重啟會讓進行中的工作中斷：`running` 超過 `2 × AI_TIMEOUT_SECS + 60` 秒的工作在下次讀取時結束為 `error`。
- AI 關閉時 `POST` 仍回 202，工作立即 `done`、`result.status = "disabled"`。
- 工作讀的是**已儲存**的最新 SOP 版本；前端在送出前會先把未儲存的修改存好。

### 共同規則（依 `docs/LLM_VALIDATION.md`）

- 一次只產生一步；每次呼叫都帶 `max_tokens`（分析 6000、目標 4000）與 `temperature`。
- 閘道會快取完全相同的請求，所以**修復呼叫一定改變請求**：temperature 0.2 → 0.6，並附上未通過的檢查；教師重按「再試一次」時 temperature 也會依該課例先前的工作次數遞增。
- 每次模型呼叫各寫一列 `ai_interactions`（`module` = `sop_analysis`／`sop_objectives`，第 2 次的 `retry_of` 指向第 1 次），含 prompt 版本、鷹架、檢查結果；輸入快照中的教材只留字數與雜湊（與單元教材相同的規則）。教師看到的是程式整理後的結果，整理了什麼記在最後一列的 `check_results.repairs`。
- 教師的決定沿用 `POST /api/ai/:interaction_id/decision`（`adopt`／`revise`／`reject`，`revise` 帶 `revised_content`）。`item_index`：分析＝0 是全文（文體、主旨、結構、語言重點），n 是第 n 段；目標＝建議陣列的索引（從 0）。
- AI 的內容**只有教師按了才寫入**：採用是前端把建議放進 `SopContent`（`origin` = `ai_adopted` 或 `ai_edited`）再照常儲存；伺服器不會把 AI 輸出寫進 SOP。
- Prompt key（`0008` 預先建立 v1，可在後台改版）：`sop_passage_analysis`、`sop_objectives`。鷹架：`SC-SOP-ANALYSIS`、`SC-SOP-OBJECTIVES`（皆為 `content_design_support`）。

### (a) 逐段教材分析 `sop_passage_analysis`

1. 程式把教材編號為 `[P1]…[Pn]` 後送出。沒有教材時兩個 `POST` 都回 400；教材段數不設上限（以前超過 40 段回 400）：輸出上限 `max_tokens` 依段數放大（`1000 + 200 × 段數`，最少 6000、最多 12000）；輸出仍然被截斷、或有段落沒寫時，照原本的「缺段重做一次、仍缺的列在 `missing_paragraph_ids`」處理，不會默默少掉。
2. 檢查：段落編號是否每段各一筆；每筆寫作特色與語言重點的 `evidence` 是否為**該段原文的連續片段**（空白與引號正規化後的子字串；含「...」拼接的不算）；中文欄位是否為中文。
3. 有缺段或有證據不合格 → 第 2 次呼叫只要求這些段落重做（附原因）。合併兩次結果後，仍不合格的證據**整筆拿掉**（`dropped_evidence`），仍缺的段落列在 `missing_paragraph_ids`。
4. 至少有一段可用就 `shown`；完全沒有可用內容 `blocked`。

### (b) SIOP 目標與課綱連結 `sop_objectives`

1. 可連結的條目由 `curriculum::items_for_track_hybrid` 取得，底下是 Phase 2.0 的 `retrieval::retrieve_curriculum`：教師學校類型（含 `both`）的正式課綱條目，預設範圍為高中階段、英語相關科目；超過 150 條時以教材與教師目標排序取前 150 條（Phase 2.4 起為混合檢索，沒有語意索引時只用關鍵字），不足再依代碼補滿。清單（代碼｜標題｜條文）放進 prompt。
2. 檢查：內容目標與語言目標都要有；`item_code` 必須在清單內；目標不可照抄條文（中文二字組重疊 ≥ 80%）；`objective_zh` 為中文、`student_facing_en` 為英文且以 "I can" 開頭。
3. 未通過 → 第 2 次呼叫（附原因）。之後仍不在清單內的代碼**拿掉**，該目標若因此沒有連結就標 `unlinked`；仍照抄條文或語言不符的目標拿掉；仍缺一種目標類型 → `blocked`。
4. `link_status` 一律由程式依剩下的連結決定。**條文、官方代碼、來源、版本、頁碼一律依代碼從資料庫帶出**（`SopCurriculumLink`；儲存時用 `retrieval::curriculum_by_codes` 精確比對 `item_code`），模型寫的條文不會顯示。沒有任何適用條目時仍會呼叫模型，所有目標都是 `unlinked`（前端顯示「尚未完成課綱連結」）。
5. 教師已寫的目標與「今天要教什麼？」會放進 prompt，請模型不要重複；推薦永遠是另外列出，不會改動教師的目標。

### 測試用假模型（`AI_PROVIDER=fake`）新增的標記（寫在教材文字裡）

| 標記 | 效果 |
|---|---|
| `FAKE_SOP_STOP_EARLY` | 分析第 1 次只交前 2 段；第 2 次補齊 |
| `FAKE_SOP_STOP_ALWAYS` | 兩次都不交第 3 段以後 → `missing_paragraph_ids` |
| `FAKE_SOP_SPLICE` | 第 1 次有一筆以「...」拼接的證據；第 2 次改正 |
| `FAKE_SOP_SPLICE_ALWAYS` | 兩次都拼接 → 該筆證據被拿掉 |
| `FAKE_SOP_INVENT` | 目標第 1 次引用清單外的代碼；第 2 次改正 |
| `FAKE_SOP_INVENT_ALWAYS` | 兩次都引用清單外的代碼 → 連結被拿掉、標 `unlinked` |
| `FAKE_SOP_UNLINKED` | 所有目標都不連結 |
| `FAKE_SOP_ONE_TYPE` | 只交內容目標 → `blocked` |
| `FAKE_SLOW` | 每次呼叫多等 3 秒（看進度畫面用） |

既有的 `FAKE_TIMEOUT`、`FAKE_ERROR`、`FAKE_REFUSAL` 照舊。

## 前端

- 課例頁改為七步工作區：左側進度列（760px 以下收成一行「第 n 步，共 7 步　看全部步驟」），中間一次一步；進度列最上方是「本節概要」（今天要教什麼？、開始上課、教學步驟與其他備課工具，即原本的首屏與收合區）。還沒有 SOP 內容的課例開在「本節概要」，有內容的開在上次停留的步驟。
- 自動儲存：SOP 與教案（今天要教什麼？＋教學步驟）在停止輸入約 1.5 秒後各自存成新版本；頁首顯示「儲存中…／已自動儲存 14:32／沒有存到」。沒有「儲存教案」按鈕。離開頁面、開始上課、請 Manna 分析之前會先把還沒存的內容存好；存不成功時照舊在頁面上確認。
- 第 2–7 步右上的「對照教材」從左側滑出教材（印刷樣式、段落編號），可與步驟並排。
- 「開始上課」在每一步的頁首都在，從不檢查 SOP 是否完成。

## 逐步設計：每一步的 AI 協助（Migration `0045_sop_every_step.sql`）

「全流程都要有 AI 介入輔助」：第 3–7 步都照第 2、3 步的模式：Manna 建議、教師採用／修改後採用／不用，AI 不覆寫教師寫的內容，採用的項目保留 `origin`。第 1 步（閱讀教材）不用 AI。所有建議都是背景工作（`lesson_sop_jobs`，共用 `jobs::run`、工作開始時固定模型 pin），同一課例、同一種工作、同一個 slot 還在跑時再按一次回傳同一個工作。

### 新增的文件欄位（都向下相容，舊文件讀成預設值）

```ts
interface SopParagraphGoal { …; origin: SopOrigin }                       // 預設 "teacher"
interface SopTask { …; answer_format: string; options: string[]; answer: string; evidence: string }
interface SopTaskBlock { …;
  student_tasks: string[]; answer_formats: string[];                      // 學習單產生器的標籤，逐段選（規格 §7）
  ai_student_tasks: string[]; ai_answer_formats: string[];                // Manna 先選的（✦）
}
interface SopVocabularyItem { …; level: number | null }                  // 儲存時一律由伺服器依詞彙表帶入
interface SopExitItem { …; objective_id: string; answer: string }        // 形成性評量檢核的目標與參考答案
interface SopOptions {                                                    // null＝沒選，用 SOP 內容推出的預設
  material_types: string[] | null; generation: string[] | null; levels: ("basic"|"standard"|"advanced")[] | null;
  minutes: number | null; exit_ticket: boolean | null;                   // false＝學習單不要 Exit ticket
}
interface SopContent { …; options: SopOptions }
```

標籤只能是產生器的標籤（規格 §5）；表外的值 → 400。`SopState` 與 `PUT …/sop` 的回應多了 `worksheet_settings`（從這個課例做學習單時一開始的設定）與 `worksheet_suggested`（只看 SOP 內容推出的預設，即 ✦）。`SopState.jobs` 多了 `goals`、`tasks`、`warmup`、`exit_ticket` 與 `redo`（上次整批之後的單項重做，舊到新）。`SopJob` 多了 `slot`（整批為 `""`）與 `parent_job_id`；`kind` 多了 `goals | tasks | warmup | exit_ticket`。

### 端點

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/lessons/:id/sop/goals` | `{ class_id?, paragraph_id?, parent_job_id? }` | 202 `SopJob`：第 4 步逐段整合目標；帶 `paragraph_id` ＝「這一段再建議一次」（slot `p:<段落 id>`）。沒有目標 → 400 |
| POST | `/api/lessons/:id/sop/tasks` | `{ class_id? }` | 202 `SopJob`：第 5 步逐段問題或任務＋語言與教學鷹架 |
| POST | `/api/lessons/:id/sop/warmup` | `{ class_id? }` | 202 `SopJob`：第 6 步暖身與關鍵字彙 |
| POST | `/api/lessons/:id/sop/exit-ticket` | `{ class_id? }` | 202 `SopJob`（`kind = "exit_ticket"`）：第 7 步。沒有目標 → 400 |
| POST | `/api/lessons/:id/sop/objectives` | `{ parent_job_id, item_index }` | 202 `SopJob`：**換一個**。被換掉的建議與其他建議由伺服器從 `parent_job_id`（可以是上一次換一個的工作，`item_index` 0）的結果讀，不信任請求；同類型、不重複；slot `<parent>:<index>`。結果只有 1 項 |
| POST | `/api/lessons/:id/sop/objective-english` | `{ type, text, student_facing_en }` | 200 `AiResult<{ english, changed, note }>`：**幫我修英文**，一次短呼叫（`max_tokens` 400），不是背景工作；`student_facing_en` 空白時依中文目標寫英文版；`text` 空白 → 400 |
| POST | `/api/lessons/:id/sop/procedural-steps` | `{ paragraph_ids, student_tasks, answer_formats }` | 200 `{ steps, text }`：**產生程序鷹架**，由程式依任務產生（與學習單同一套規則，`worksheet::assemble::auto_steps`），不呼叫模型；記一筆 `sop_procedural_steps` 使用事件 |

教師的決定沿用 `POST /api/ai/:interaction_id/decision`。`item_index`：第 4 步＝段落編號；第 5 步＝建議區塊的位置（從 0）；第 6 步＝暖身活動的位置，關鍵字彙這一組固定是 100；第 7 步＝題目的位置；換一個＝新建議在它自己的工作裡是 0（被換掉的那項在原工作記為 `reject`）；幫我修英文＝0（用這個 `adopt`、保留我的 `reject`）。

### 各步的建議與檢查

| 步驟 | prompt key（後台「逐步設計」群組） | 檢查（不通過 → 第 2 次呼叫附原因；仍不合格的拿掉） |
|---|---|---|
| 3 換一個 | `sop_objectives`（只要 1 項） | 與第 3 步相同（代碼在清單內、不照抄課綱、中文目標、I can 英文），且必須有 1 項要求的類型 |
| 3 幫我修英文 | `sop_objective_english`（新） | 以「I can 」開頭、只有英文、4–25 個字；失敗時回 `blocked`，教師原本的英文不動 |
| 4 逐段整合目標 | `sop_paragraph_goals`（新；**沒有用真實模型驗證過**） | 每一段各一筆且依段落順序；`objective_codes` 只能是教師目標的代號（`O1…`，回傳時換成真的 id）；每段最多 3 項；`note_zh` 為中文、≤ 120 字；不對應任何目標的段落要在 note 說明 |
| 5 問題或任務 | `sop_paragraph_tasks`、`sop_scaffolds`（複製學習單 `worksheet_tasks`／`worksheet_scaffolds` 的最新版再補 SOP 規則） | 就是學習單的檢查：標籤只能用產生器的、每段都涵蓋且依序、`evidence` 為原文連續片段、學生看到的只有英文、選擇題選項數與配對題下限（含老師已確認的選項數習慣）、鷹架不洩漏答案。鷹架那一次失敗時任務仍保留，只有程式的程序鷹架 |
| 6 暖身 | `sop_warmup`（複製 `worksheet_warmup`） | 學習單的檢查；候選字與級別由程式（`worksheet::ai::key_word_candidates`、大考中心詞彙表）決定，模型只挑、不輸出級別；`context_sentence` 為原文逐字 |
| 7 Exit ticket | `sop_exit_ticket`（複製 `worksheet_exit_ticket`） | 學習單的檢查（2–4 題、至少 1 題形成性評量與 1 題反思／蒐集問題、不重複主要任務）；另外形成性評量要有 `objective_refs`（轉成目標 id）與參考答案 |

- 每次模型呼叫各寫一列 `ai_interactions`（`module` 與 prompt key 相同，除了第 3 步沿用 `sop_objectives`；`lesson_id`、`unit_id` 有值；第 2 次的 `retry_of` 指向第 1 次）。鷹架：`SC-SOP-GOALS`、`SC-SOP-TASKS`（問題與鷹架兩次呼叫）、`SC-SOP-WARMUP`、`SC-SOP-EXIT`、`SC-SOP-ENGLISH`。教材文字只以字數與雜湊記進輸入快照；教材刪除時，這些輸出裡的 `evidence`／`context_sentence` 與 SOP 任務的 `evidence` 一併清掉。
- **教師記憶**：`sop_paragraph_tasks`、`sop_warmup`、`sop_exit_ticket` 在 `memory::HABIT_STEPS`（主持人開啟開關時帶入已確認的習慣，第 5 步的選項數檢查同樣放寬）；英文修改、整合目標、鷹架不帶。見 `docs/TEACHER_MEMORY.md`。
- 後台提示詞「逐步設計」群組列出 8 個提示詞（含原本的 `sop_passage_analysis`、`sop_objectives`）。

### 學習單的選擇放在哪一步（規格 §7）

| 選項 | 在哪裡選（SOP 文件欄位） |
|---|---|
| 素材類型 | 第 1 步（`options.material_types`） |
| 提供引導問題 | 第 2 步；第 5 步的「鷹架在學習單上的呈現」（`options.generation`） |
| 顯示學習目標 | 第 3 步（`options.generation`） |
| 逐段整合目標 | 不是學習單選項，只是產生的依據 |
| **學生任務、作答方式** | **第 5 步每一段（或合併的區塊）逐段選**，用產生器的同一批標籤與 ✦ 元件（`task_blocks[].student_tasks / answer_formats`）；Manna 的建議先選好 |
| 顯示作答示例、提供字詞表、提供句型框架、基礎／一般／進階版 | 第 5 步的語言與教學鷹架（`options.generation`、`options.levels`） |
| 程序鷹架 | 自動，不用選 |
| 加入暖身活動 | 第 6 步（`options.generation`） |
| Exit ticket 要不要、預計完成時間 | 第 7 步「產生設定」（`options.exit_ticket`、`options.minutes`），同一處也看得到全部選項 |

沒選過的項目用 SOP 內容推出的預設（`worksheet::suggest_from_sop`）。**從 SOP 產生學習單**（`POST /api/worksheets { lesson_id }`）的起始設定就是這些選擇：學生任務與作答方式為各區塊標籤的聯集（沒有標籤時依資訊處理功能推）、其他選項取 `options`；`Settings.skip_exit_ticket`（新，預設 false）為 true 時產生流程略過 Exit ticket 那一步，學習單最後不放。`worksheet_created` 使用事件多了 `from_sop` 與 `settings`（標籤陣列）。課例頁的「產生學習單」先顯示一個畫面列出這些設定：「產生」建立學習單並立刻開始產生；「調整設定」建立後開啟產生器表單，同樣的設定已經填好。直接從首頁／`/create` 做學習單的流程不變。

### 測試用假模型新增的標記（寫在教材文字裡）

| 標記 | 效果 |
|---|---|
| `FAKE_SOP_GOALS_INVENT` / `_ALWAYS` | 第 4 步：第 1 次（或兩次都）引用不存在的目標代號 |
| `FAKE_SOP_GOALS_SKIP` / `_ALWAYS` | 第 4 步：第 1 次（或兩次都）漏掉第 2 段 |
| `FAKE_SOP_GOALS_LONG` | 第 4 步：第 1 次有超過字數上限的筆記 |
| `FAKE_BLOCK` | 第 4 步與幫我修英文：兩次都不合格 → `blocked` |

第 5–7 步使用學習單的假輸出與標記（`worksheet::fake`，例如 `FAKE_WS_CJK`）。

---

# Phase 2.3：學習單產生器

以下為**新增**；既有端點不變。Migration `0010_worksheets.sql`（`0009` 保留給教材庫分支）。依據：《學習單產生器：教師選項與 SOP 整合規格》§4.1、§4.2、§4.6、§5–§9.5、§9.7，`docs/LLM_VALIDATION.md` §5–§10（步驟 (c)(d)(e)(f)，(g) 改由程式組裝），`docs/DESIGN_OVERHAUL.md` 第 3 步。工程說明與規格 §12 第 2–6 項的回覆在 `docs/ENGINEERING_REPLY_WORKSHEET.md`。

不在這一版：外部資源搜尋（§4.3）、教材註記與教材庫（另一分支，見「教材接縫」）、AI 資深教師（§4.5）、歷屆考題分析（沒有資料）。§9.6 只做決定性的詞彙級別檢查。

## 固定標籤（規格 §5，逐字；伺服器只接受這些值）

| 區塊 | `WorksheetSettings` 欄位 | 可用的值 |
|---|---|---|
| 素材類型 | `material_types` | 閱讀文本、聽力素材、影片素材、圖像素材、資料／數據 |
| 學生任務 | `student_tasks` | 語言與內容學習：理解內容、字彙練習、文法／句型練習｜思考與資訊處理：比較／分類、推論／預測、評估／提出理由、蒐集資料、整理／分析資料｜互動與合作：討論／分享、小組共同完成｜表達與成果：寫作、製作成果、成果發表｜反思與評量：學習反思、自我評量、同儕互評 |
| 作答方式 | `answer_formats` | 填空題、選擇題、是非題、配對題、簡答題、表格整理、圖像組織、段落寫作 |
| 產生設定 | `generation` | 加入暖身活動、顯示學習目標、提供引導問題、顯示作答示例、提供字詞表、提供句型框架 |
| 產生設定 →「產生基礎版／一般版／進階版」 | `levels` | `basic`（基礎版）、`standard`（一般版）、`advanced`（進階版） |

- 四個區塊都是複選、都可以是空陣列；沒有任何必選規則。五個小標只用來排版，不是欄位。
- 陣列裡出現表外的字串（包含「課綱議題」「SDGs」「SEL」「探究」「高層次思考」「專題任務」「合作活動」）→ 400。重複值會被去掉，順序依上表。
- `minutes`（預計完成時間，分鐘）可為 null；有值時需為 5–240。

## 型別

```ts
type WsLevel = "basic" | "standard" | "advanced";
type WsAudience = "student" | "teacher";
type WsOrigin = "teacher" | "ai" | "ai_edited";       // 教師寫的｜Manna 產生的｜Manna 產生後教師改過
type WsBlockKind = "warmup" | "activity" | "exit_ticket";
type WsScaffoldType = "verbal" | "instructional" | "procedural";   // 語言／教學／程序鷹架（規格 §3）

interface WorksheetSettings {
  material_types: string[]; student_tasks: string[]; answer_formats: string[]; generation: string[];
  levels: WsLevel[]; minutes: number | null;
}

// 教材接縫：產生器只認這個形狀。目前的來源是「貼上的文字」與「課例 SOP 的教材」。
interface WorksheetMaterial {
  title: string;
  paragraphs: Array<{ id: string; text: string }>;       // id 例如 "p3"；由程式分段
  source: { kind: "pasted" | "lesson_sop"; lesson_id: number | null; material_version_no: number | null; note: string;
            material_id: number | null };   // 教材庫（Phase 2.2）裡的那一份
}
// 貼上的文字先存進教師的教材庫（`materials`，並以 `material_links` 連到這份學習單），學習單保存當時的快照；
// 從課例進入時用課例目前的教材庫教材。教材欄顯示教材庫的 MaterialViewer（含註記）。
// 產生時，該教材上教師勾了「提供給 AI」的註記（`materials::annotations::shared_with_ai`）會以
// 「教師註記」的標題放進每一步的 prompt，並列在「Manna 已帶入的資訊」；其他註記不會送出，也不會出現在學習單上。

interface WsObjective {
  id: string; type: "content" | "language";
  text: string;                 // 教師看的說明（可中文）
  student_facing_en: string;    // 印在學習單「Today I can …」的英文（存成 "I can …"；印在標題下時去掉開頭的 I can，見下）
  origin: WsOrigin;
  sop_id: string | null;        // 從課例帶入時，對應的 SOP 目標 id
}

// 產生前可以編修的「帶入內容」（從課例預帶；直接使用時是空的）。全部可改、可刪。
interface WorksheetBrief {
  tasks: Array<{ id: string; paragraph_ids: string[]; function: InfoFunction | null; text: string;
                 sop_block_id: string | null; sop_task_id: string | null }>;
  warmup: string[];             // 暖身想法
  vocabulary: string[];         // 想放進暖身的關鍵字
  exit_ticket: Array<{ kind: ExitKind; text: string }>;
}

interface WsVocab { word: string; level: number | null; definition: string; context: string; paragraph_id: string | null }
// level 一律由伺服器查 vocabulary_items；查不到或詞彙表未匯入時是 null（不由模型產生，也不採用用戶端送來的值）
interface WsItem {
  id: string; answer_format: string | null;   // 規格 §5.3 的標籤
  prompt: string; options: string[];
  answer: string;                // 參考答案：只出現在教師參考版
  evidence: string;              // 答案在教材中的逐字依據：只出現在教師參考版
  purpose: ExitKind | null;      // 只有 Exit ticket 的題目有
  levels: WsLevel[];             // 這一題出現在哪些版本（核心題＝全部版本）
  origin: WsOrigin;
  table?: { columns: string[]; rows: number } | null;  // 表格整理的表格：1–6 欄、欄名下 1–12 列空白列；沒有時見下
  kept_options?: string[];       // 改成是非題時收起來的選項（≤12）；不印，改回選擇題或配對題時編輯器放回去
}
```

**作答方式怎麼印**（投影、預覽、DOCX、PDF 一致；編輯器照同樣的規則畫）：

- `options` 只在**選擇題、是非題、配對題**（與沒有作答方式的舊題目）印出，空白的選項不印；其他作答方式的題目若還有選項，留在資料裡但不印（教師換回去時就回來）。
- 印出的選項依序加上 `(A)`、`(B)`、`(C)`、`(D)`…（投影的 `options` 是選項本身的文字，字母由印的地方加：`assemble::printed_option`／`printedOption`）。選項自己開頭已有同位置的字母標籤（`A. …`、`(A) …`、`A) …`、`（A）…`、`A、…`）時先去掉，不會印成「(A) A. …」。教師版**選擇題**的參考答案是其中一個選項時印成「(B) London」，和學生版的字母一致（`printed_answer`／`printedAnswer`）；其他作答方式的答案照原樣。
- 學習目標印在「Today I can …」下面時，每一行開頭的「I can」（不分大小寫，也接受「I can:」與多餘空白）不重複印（`objectives::printed_objective`／`printedObjective`）；不是以 I can 開頭的行照原樣。編輯器裡可以改的目標欄位仍顯示完整的「I can …」。
- **表格整理**印成表格：題目的 `table: { columns, rows, row_labels? }`。`columns` 是欄名（要比較的事物），`row_labels` 是每一列要寫什麼（「Claim」「Supporting detail」），印在多出來的第一欄；沒有（或全空白）就是沒有標籤的空白列。生成時模型一定要寫具體的欄名與（兩欄的表格）列標籤（輸出的 `table` 欄位，`null` 給其他作答方式；`worksheet/table.rs`）：欄名是空白或占位字（`Column 1`、`Header`）時，伺服器改用題目寫出的欄名（`…with two columns: A and B.`、`…: A / B`），題目沒寫就請模型重寫一次（仍然沒有就留空，編輯器會提醒教師補上）；兩欄的表格沒有列標籤也會請模型重寫。沒有 `table` 時用題目最後一個冒號後以「/」或「|」分開的 2–6 個短欄名，或題目寫出的欄名（`two columns: A and B`），再沒有就是兩欄、沒有欄名；列數預設 3。舊學習單裡存的 `Column 1` 這類占位欄名不會印出（用題目寫出的欄名，沒有就空白；資料本身不動）。有任何欄名時第一列印欄名。投影的題目多一個 `table: { columns, rows, row_labels? }`（只有表格整理有）：有列標籤時 `columns` 多一個空白的第一欄（放標籤的那一欄），`row_labels` 每列一個；作答行數是 0。
- **圖像組織**印一個作答框；**簡答題**依版本印 2／3／4 行，**段落寫作**再多 2 行；其他作答方式不印作答線。
- 編輯器換作答方式時保留題目、答案、依據、提示與各版本的支持：換成是非題 → 選項變成 True／False，原本的選項放進 `kept_options`；換成選擇題／配對題且沒有選項（或只有 True／False）→ 放回 `kept_options`，沒有就給 4／3 個空白選項；`table` 不論作答方式都留著。
- 存檔檢查：`table` 要有 1–6 欄、1–12 列，欄名最多 200 字；`kept_options` 與 `options` 一樣最多 12 個。

**填空題的空格**（每個寫題目的步驟都檢查：完整學習單的主要任務與 Exit ticket、照版型產生、閱讀理解題、差異化學習單、逐區修改、備課對話的修改）：填空題不印作答線，學生寫在題目的空格裡，所以題目一定要有空格——三個以上的底線（`______`、`___`）、全形 `＿＿＿`、`( )` 或 `（　）`。沒有空格 → `items_wellformed` 不通過，修正呼叫會寫明問題；修正後仍沒有 → 程式把參考答案的片語（以逗號、分號、斜線或換行分開，去掉編號與句尾標點，三個字元以上，長的先換）在題目裡逐字出現的地方（不分大小寫、整個字）換成 `______`；一個都找不到（或換完題目沒有字可讀）→ 改成簡答題（印作答線）。怎麼處理的記在該次呼叫的 `check_results.repairs`。`0030_fill_in_blanks.sql` 的提示詞新版本寫明空格一定要在題目裡、答案的字詞不可以留在題目裡。

**句型框架不是題目**（完整學習單的主要任務與 Exit ticket）：語言鷹架（句型框架，SIOP Q2／T4）印在區塊的「Sentence frames」方框裡（`scaffolds` 的 `句型框架`），不編題號。模型把句型框架本身寫成一題時（題目有空格、用學生的口吻 I／my／we、沒有問號、沒有一句以指示或疑問詞開頭，而且不是逐字取自教材的克漏字；`checks::sentence_frame_only`）→ `items_wellformed` 不通過，修正呼叫會寫明要寫出要學生做什麼、不要用 Hi! 之類的招呼語開頭；修正後仍是句型框架 → 組成區塊時（`assemble::lift_frames`）框架移到這一區的 `句型框架` 鷹架（印的版本＝那一題印的版本），前一題是簡答題或段落寫作時這一題拿掉（那是前一題的框架），否則這一題改成「Use the sentence frame above to write your answer.」（簡答題，印作答線），所以 Exit ticket 的題數與目標檢核不變；後面的題號連續。教師自己寫的題目不動。`0031_sentence_frames_apart.sql` 的提示詞新版本（Exit ticket、主要任務、逐區修改、備課對話的修改）寫明句型框架不能單獨當一題、題目不用招呼語開頭；兩種修改沒有放鷹架的欄位，句型框架接在它要幫助的那一題最後。

**招呼語開頭的分享句型、句型框架裡的步驟、是非題**（`0036_template_supports.sql`，所有產生學習單的路徑）：
- 題目是以 Hi!、Hello! 等招呼語開頭、有空格、不是逐字取自教材的句型（口頭分享用的句型框架；`checks::greeting_frame`），或把這樣的句型包在題目裡（「Write a short analysis paragraph using the frame: Hi! …」「Complete the analysis paragraph: Hi! …」）→ 一樣是 `items_wellformed` 不通過；修正後仍是 → 完整學習單照上面的方式移到 Sentence frames 方框（不含外面那層題目）；照版型產生的區塊見「提示框」。
- 鷹架步驟的「句型框架」裡寫了作答步驟（沒有空格、以祈使動詞開頭的句子，例如「Identify the common idea in the first sentence.」）→ `supports_wellformed` 不通過（`worksheet_scaffolds` 第 2 版要求步驟寫在程序鷹架）；修正後仍是 → 步驟移到這一區的分步指示（程序鷹架的 `steps`），句型框架只留有空格的句子；「Use this frame:」這類只是指向框架的開頭拿掉。
- **是非題要是可以依課文判斷對錯的直述句**（「The second sentence of Paragraph 1 is the topic sentence.」）。題目是問句，或問學生的看法（「Do you think this sentence serves as a topic sentence?」「In your opinion …」）→ `items_wellformed` 不通過（主要任務、Exit ticket、閱讀理解題、照版型產生與兩種修改的檢查都有；提示詞新版本寫明規則）；修正後仍是 → 「Do you think X?」「Do you agree that X?」「Is it true that X?」改成判斷「X.」（仍是是非題、答案不變），其他改成簡答題，題目後加「Answer Yes or No, and give one reason from the text.」，選項拿掉，答案 True／False 改成 Yes.／No.。

```ts
**逐題句型框架與提示**（`0050_per_question_frames.sql`）：句型框架不再集中成區塊最上面的一個方框，而是放在它要幫助的那一題下面。`WsItem` 多一個選填的 `frame: string`（一小段學生可以照著說或寫的話：招呼、帶理由的回答、結尾，空格用 `___`；沒有時是空字串或沒有這個鍵，舊學習單照舊載入）。`worksheet_scaffolds` 的輸出每個 target 多 `item_frames: [{ item_no, frame_en }]` 與 `item_hints: [{ item_no, hint_en }]`（`item_no` 是題目在區塊裡的題號）；`checks::evaluate_scaffolds` 檢查（`supports_wellformed`）：只有簡答題與段落寫作可以有句型框架，框架要有 `___`、只用英文、不可含參考答案，每題一個，題號不存在的拿掉；提示放進那一題的 `hint`（印在基礎版，標「Hint:」，不編號），所以不再有一排獨立的「引導問題」方框。`assemble::attach_scaffolds` 把它們放到題目上，並且在區塊已有逐題框架（提示）時不再放模型多寫的集中的句型框架（引導問題）鷹架；教師只勾其他鷹架種類時不放逐題框架與提示。印出的 `SheetItem` 多選填的 `frame`（基礎版與一般版；只有進階版時進階版也印），預覽、複製、DOCX、PDF 都印在題目下面（「Sentence frame: …」）。教師可以在編輯畫面逐題修改或「刪除」。逐區修改的題目多 `frame`（空字串＝不動，`(none)`＝拿掉，其他＝新的句型框架，同樣的檢查）。課例 SOP 第 5 步、Exit ticket 與照版型產生的區塊不使用逐題框架。另：有程序鷹架的區塊，作答指示（`instructions_en`）改成這一區課文的簡短介紹，不重複步驟（提示詞規則）。暖身也一樣（`0053_warmup_intro.sql`，`worksheet_warmup`、`sop_warmup` 與逐區修改的提示詞規則）：暖身的作答指示是活動介紹，以「In this activity, you will …」開頭，作答步驟只印在程序鷹架。

interface WsScaffold {
  id: string; type: WsScaffoldType;
  kind: string;                  // 規格 §3 的例子名稱，例如 句型框架、字詞表、圖像組織、清楚的分步指示及完成順序
  content: string; steps: string[];   // 程序鷹架用 steps，其餘用 content
  levels: WsLevel[];             // 這個鷹架出現在哪些版本
  auto: boolean;                 // 系統自動加入的程序鷹架
  origin: WsOrigin;
  heading?: string;              // 印在它上面的標題（≤80）；沒有時印種類的英文標題（Sentence frames、Hints…）。照版型產生的區塊印原學習單的標題（Directions、Sentence Frame for Sharing…）；有標題的程序鷹架印成一個方框（一行一步），不併進區塊的編號步驟
}
interface WsBlock {
  id: string; kind: WsBlockKind;
  enabled: boolean;              // 暖身與 Exit ticket 可以關掉（保留內容、不印出）
  title: string; instructions: string;
  paragraph_ids: string[];       // 一段，或合併的數段
  info_function: InfoFunction | null; student_tasks: string[]; objective_ids: string[];
  vocabulary: WsVocab[];         // 只有暖身有
  items: WsItem[]; scaffolds: WsScaffold[];
  teacher_notes: string;         // 只出現在教師參考版（設計理由，可中文）
  origin: WsOrigin;
  message: string | null;        // 這一區塊沒有產生成功時的說明；不影響手動編輯
  sop_block_id: string | null;
}

interface WsChosen {             // 系統這次實際用的設定（規格 §4.6：要讓教師看到）
  settings: WorksheetSettings;
  auto: Array<"material_types" | "student_tasks" | "answer_formats" | "generation" | "levels" | "minutes">;  // 哪些是系統決定的
  item_budget: number;           // 主要任務的核心題數目標
  reasons: string[];
}
interface WorksheetContent {
  objectives: WsObjective[]; show_objectives: boolean;
  blocks: WsBlock[];             // 順序＝印出的順序；可拖曳調整
  levels: WsLevel[];             // 版本分頁（只有選了的）
  chosen: WsChosen | null;       // 還沒產生過是 null
  notices: string[];             // 產生時要讓教師知道的事（例如某段沒有對應任務）
}

interface WsJob {
  id: number; worksheet_id: number; run_no: number;
  step: "tasks" | "warmup" | "scaffolds" | "exit_ticket";
  block_id: string | null;       // 單一區塊重新產生時
  status: "queued" | "running" | "done";
  outcome: "shown" | "blocked" | "timeout" | "error" | "disabled" | "skipped" | null;   // done 時才有
  phase: string; message: string | null;
  interaction_id: number | null; context: AiContextItem[];
  started_at: string | null; finished_at: string | null;
}

interface WorksheetSummary {
  id: number; title: string; filename: string;
  lesson: { id: number; title: string; unit_id: number; unit_title: string } | null;
  class_id: number | null; generated: boolean; levels: WsLevel[];
  version_no: number; created_at: string; updated_at: string;
}
interface WorksheetState extends WorksheetSummary {
  material: WorksheetMaterial;
  settings: WorksheetSettings;         // 教師目前勾的
  suggested: WorksheetSettings;        // 系統依教材與課例建議的（畫面上打星號）；建立時 settings 以它起始
  brief: WorksheetBrief;
  versions_count: number; saved_at: string;
  content: WorksheetContent;
  jobs: WsJob[];                       // 最近一次產生（或區塊重新產生）的各步驟
  running: boolean;
  undoable: string[];                  // 有上一版可以復原的區塊 id
  sop_sync: { linked: boolean; changes: number };   // 連結課例時：與課例 SOP 不同的地方有幾項
}

// 學生版／教師版的投影（由程式組裝；匯出與這個端點用同一份）
interface WsSheet {
  title: string; level: WsLevel; audience: WsAudience; objectives: string[];
  blocks: Array<{
    id: string; kind: WsBlockKind; label: string;      // "A"、"B"…；Exit ticket 沒有
    title: string; instructions: string; steps: string[];
    vocabulary: Array<{ word: string; level: number | null; definition: string }>;
    supports: Array<{ type: WsScaffoldType; kind: string; content: string }>;
    items: Array<{ no: number; answer_format: string | null; prompt: string; options: string[]; lines: number;
                   table?: { columns: string[]; rows: number };   // 只有表格整理
                   answer?: string; evidence?: string }>;   // 學生版沒有 answer 與 evidence 這兩個鍵
    teacher_notes?: string;                                 // 學生版沒有這個鍵
  }>;
}
```

## 端點（皆需登入；只能存取自己的學習單，其他人的回 404）

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/worksheets` | `?lesson_id=` | `WorksheetSummary[]`，新到舊 |
| POST | `/api/worksheets` | `{ lesson_id?, class_id?, material?: { title?, text, source_note? } }` | 201 `WorksheetState`。有 `lesson_id`：從課例 SOP 帶入教材、目標、任務、暖身、Exit ticket 與建議選項（沒有 SOP 內容也可以）。沒有：直接使用，`material.text` 可以先空著 |
| GET | `/api/worksheets/:id` | – | `WorksheetState` |
| PUT | `/api/worksheets/:id` | `{ title?, filename?, settings?, brief?, content?, base_version_no? }` | `WorksheetState`。`content` 與最新版本不同才新增一個版本（只增不改）。標籤不在規格集合內 → 400；產生中送 `content` → 409（產生完會整份取代）；`base_version_no`（編輯器上次看到的版本）不是最新版本、而 `content` 又和最新版本不同 → 409，什麼都不存（見「學習單：逐區修改與註記」） |
| PUT | `/api/worksheets/:id/material-selection` | `{ paragraph_ids: string[] \| null }` | `WorksheetState`。**只選要用的段落**（見「長教材」） |
| PUT | `/api/worksheets/:id/material` | `{ title?, text, source_note? }` | `WorksheetState`。換教材：重新分段（文字相同的段落沿用 id），`source.kind` 變成 `pasted`，重新計算 `suggested` |
| DELETE | `/api/worksheets/:id` | – | `{ ok: true }`（軟刪除） |
| POST | `/api/worksheets/:id/generate` | `{}` | 202 `WorksheetState`：建立四個步驟的背景工作並立刻回來。已在執行時回傳同一批 |
| GET | `/api/worksheets/:id/jobs` | – | `{ running, jobs: WsJob[], version_no }`（輪詢用） |
| POST | `/api/worksheets/:id/blocks/:block_id/regenerate` | `{}` | 202 `WorksheetState`：只重新產生這一個區塊 |
| POST | `/api/worksheets/:id/blocks/:block_id/undo` | `{}` | `WorksheetState`：復原成重新產生（或採用）之前的那一版，採用的圖片動作也一起復原；沒有上一版 → 409；採用之後這一區又改過 → 409（見「復原與修改歷程」） |
| POST | `/api/worksheets/:id/blocks/:block_id/restore-word-table` | `{}` | `WorksheetState`：恢復字詞表。老師按「刪除」（或請 Manna 拿掉每一列）時，各列留在區塊的 `vocabulary_removed`；這裡把它們放回 `vocabulary`，級別重新查詞彙表、指示用語重新由程式找出（不用記下來的值）。寫一個新版本（`cause = "edit"`，`source.action = "restore_word_table"`）、記 `worksheet_word_table_restored`。這一區現在有字詞表、或沒有刪掉的可以恢復 → 409；學習單正在產生 → 409 |
| GET | `/api/worksheets/:id/sheet` | `?level=&audience=` | `WsSheet` |
| GET | `/api/worksheets/:id/versions` | – | `Array<{ version_no, cause, created_at }>`，新到舊；`cause` = `created`／`edit`／`generate`／`regenerate`／`undo`／`revise`（備課對話的修改與逐區修改的「採用」）／`vocabulary`（詞彙級別檢查的「套用」） |
| GET | `/api/worksheets/:id/sop-sync` | – | `SopSyncPreview`（沒有連結課例 → 409） |
| POST | `/api/worksheets/:id/sop-sync` | `{ confirm: true, keys: string[], sop_version_no, worksheet_version_no?, worksheet_updated_at?, lesson_id? }` | `{ sop_version_no, applied }`。沒有 `confirm: true` → 400；課例／學習單版本或連結與預覽不符 → 409 |
| PUT | `/api/worksheets/:id/link` | `{ lesson_id: number \| null }` | `WorksheetState`：連結既有課例或取消連結（不改課例內容） |
| POST | `/api/worksheets/:id/save-as-lesson` | `{ unit_id, title }` | `WorksheetState`：在這個單元建立新課例，把教材、目標與任務寫成它的第一版 SOP，並連結 |
| POST | `/api/worksheets/:id/checks/vocabulary` | `{ level, target_level? }` | `WsVocabCheck`。詞彙表未匯入 → 409 |
| GET | `/api/worksheets/:id/checks` | – | `WsVocabCheck[]`：每個版本最近一次的結果 |
| POST | `/api/worksheets/:id/checks/:check_id/suggest` | `{ action, words }` | `WsVocabSuggestions`（見「詞彙級別檢查：處理超出級別的字」） |
| POST | `/api/worksheets/:id/checks/:check_id/apply` | `WsVocabApplyRequest` | `WsVocabApplied` |
| GET | `/api/worksheets/:id/export` | `?format=docx\|pdf&level=&audience=` | 檔案本體（`Content-Disposition: attachment`）。`level` 不在這份學習單的版本裡 → 400 |
| GET | `/api/worksheets/:id/thumbnail` | – | 學生版第 1 頁的預覽圖（JPEG，寬 480 px），給「我的作品」的卡片。見「我的作品：卡片預覽」 |

## 兩個入口

- **從課例**（`POST /api/worksheets { lesson_id }`）：伺服器讀最新的 SOP 版本，把教材（`source.kind = "lesson_sop"`，是當時的快照）、目標（`content.objectives`）、任務／暖身／Exit ticket（`brief`）帶進來，並依課例內容給 `suggested`：有教材→「閱讀文本」；SOP 任務的資訊處理功能依規格 §8 對應到學生任務（發現→理解內容、整理→整理／分析資料、分析→推論／預測、比較→比較／分類、創造→寫作）；有暖身→「加入暖身活動」；有目標→「顯示學習目標」；任務裡寫了語言鷹架→「提供句型框架」「提供引導問題」。全部只是建議：`settings` 以它起始，教師可以全部取消。課例不需要先完成任何一步。
- **直接使用**：貼上教材即可。`suggested` 只由教材判斷（素材類型：一般文字→「閱讀文本」，數字很多→再加「資料／數據」；其餘給常用的起始值）。學習單預設獨立（`lesson = null`），不會建立課例；之後可用 `link` 或 `save-as-lesson`。
- **四個區塊完全不選也能產生**：每個空的區塊由系統決定（素材類型依教材；學生任務與作答方式由模型依教材設計後回報；產生設定預設「加入暖身活動、顯示學習目標、提供字詞表、提供句型框架」；版本預設一般版，班級特性提到程度落差或教材詞彙偏難時加基礎版）。實際用的設定寫在 `content.chosen`，`chosen.auto` 列出哪些是系統決定的。

## 產生流程

`POST …/generate` 建立一輪（`run_no`）四個工作，依序執行，每個工作是**一次模型步驟**（最多兩次呼叫：第一次＋一次修復）：

| 步驟 | prompt key | 模組 `ai_interactions.module` | 鷹架 | `max_tokens` | 內容 |
|---|---|---|---|---|---|
| `tasks` | `worksheet_tasks` | `worksheet_tasks` | `SC-WS-TASKS` | 1500 + 600 × 段數（3000–6000） | 逐段（或合併段落）的活動區塊：資訊處理功能、學生任務、題目、作答方式、參考答案、逐字依據；每題標 `core`／`support`／`challenge` |
| `warmup` | `worksheet_warmup` | `worksheet_warmup` | `SC-WS-WARMUP` | 2000 | 喚醒背景知識 1–2 個活動＋關鍵字彙（只能從程式算出的候選字挑；級別由 `vocabulary_items` 帶入） |
| `scaffolds` | `worksheet_scaffolds` | `worksheet_scaffolds` | `SC-WS-SCAFFOLDS` | 800 + 350 × 區塊數（2000–5000） | 每個區塊的語言鷹架、教學鷹架，以及**自動加入**的程序鷹架 |
| `exit_ticket` | `worksheet_exit_ticket` | `worksheet_exit_ticket` | `SC-WS-EXIT` | 1200 | 2–4 題：形成性評量＋學生反思或蒐集問題 |

最後一步的工作會維持 `running`（`phase`「正在把結果組合成學習單」）直到新版本存好，所以輪詢看到 `running = false` 時，`GET /api/worksheets/:id` 一定已是新內容。這一步被略過時也一樣（`skip_exit_ticket`：`outcome = "skipped"`，一樣等到存好才結束；進度不列「寫 Exit ticket」）。之後由**程式組裝**（不呼叫模型）：暖身固定在最前、活動區塊依段落順序、Exit ticket 在最後；題號、版本差異、學生版／教師版都由程式決定，存成一個新的學習單版本（`cause = "generate"`）。

- 不選「加入暖身活動」時 `warmup` 步驟為 `skipped`。Exit ticket 預設加入（不是選項），教師之後可以關掉或刪除。
- **版本差異（規格 §5.4）**：三個版本共用同一份目標與核心題（`core`，`levels` = 全部選到的版本）。`support` 題只在基礎版、`challenge` 題只在進階版。鷹架：基礎版顯示全部；一般版顯示程序鷹架＋字詞表、句型框架；進階版只留程序鷹架。開放題的作答行數基礎版 2、一般版 3、進階版 4。教師可以逐題、逐鷹架改 `levels`。
- **份量（規格 §9.2）**：`item_budget` = 主要任務的核心題數。有 `minutes`：扣掉暖身 5 分鐘與 Exit ticket 3 分鐘後，每 2.5 分鐘一題（3–24）。沒有：每段 2 題（4–14）。超過 1.5 倍時先請模型修一次，仍超過由程式刪去多的非核心題與區塊末端的題目（每個區塊至少留一題）。
- **程序鷹架**：每個區塊至少一個，`auto = true`。模型沒有交出時，程式依作答方式與學生任務產生分步指示（例如有「小組共同完成」就寫明分工步驟），所以一定會有。
- **自動檢查**（移植自 `research/sop-validation/chain/checks.py`）與處理：
  - 標籤只能來自規格集合；全英文（學生看得到的欄位含中日韓文字即不通過）；逐字依據必須是教材的連續片段；選擇題 4 個不重複選項且答案在選項內、是非題答案為 True/False、填空題要有空格；題幹不可含答案；每段都要被涵蓋、區塊依段落順序；區塊數 ≥ 3 時至少 3 種資訊處理功能；暖身關鍵字只能來自候選清單；鷹架不可含答案原文；Exit ticket 2–4 題且兩種用途都有、不重複主要任務的題目。
  - 程式能完全修好的不另外花一次修復呼叫，記為通過並寫明修了什麼（`check_results.checks[].detail` 與 `repairs`）：不逐字的依據改用其中最長、至少 5 個字的逐字片段，沒有就清空（與課例 SOP 的分析一致）；鷹架 target 的寫法差異（`Warm-up`、`warmup`、`b1`、`[B1]`）視為同一個，這次沒有暖身時模型多給的暖身鷹架直接拿掉，重複的 target 只留第一筆；字詞表裡括號中的中文註解拿掉。
  - 其他檢查第一次沒過 → **修復一次**，請求一定不同（temperature +0.4，並附上未通過的項目）。
  - 修復後仍不過 → 不整份擋下：拿掉不合格的部分（不逐字的依據清空、不合格的題目或鷹架移除），記在 `ai_interactions.check_results.repairs`；整個步驟沒有可用內容時，對應區塊留一個空的區塊並在 `message` 說明（例如「暖身沒有產生成功，你可以自己寫，或按重新產生」），其餘步驟照常進行。
  - 輸出會清掉 LaTeX／Markdown 殘留。
- **輸出失控**：模型寫到 `max_tokens` 時該次呼叫為 `blocked`（檢查 `max_tokens`，說明是哪一種：`whitespace` 空白、`repetition` 重複、`long` 單純太長），`ai_interactions.output` 留下 `{ truncated_at_max_tokens, runaway, chars, longest_whitespace, runaway_excerpt }`：模型輸出的頭尾各 1500 字、長串空白縮成「⟨N whitespace⟩」（只有模型輸出，不含輸入的教材；刪除教材時與 `evidence` 一起清掉）。送出的 JSON schema 保持撰寫時的欄位順序（`serde_json` 的 `preserve_order`）：閘道的受限解碼會要模型照 schema 的順序寫欄位，按字母排序時模型會在想結束物件的地方只能寫空白而失控（`docs/llm-eval/worksheet-real/`）。
- 逾時、供應商錯誤、AI 關閉：該步驟 `outcome` = `timeout`／`error`／`disabled`，其餘照上面的方式處理。**任何失敗都不會擋住手動編輯。**
- 每次呼叫各寫一列 `ai_interactions`（輸入快照中的教材只留字數與雜湊）；`WsJob.interaction_id` 指向教師看到的那一次。教師復原一個重新產生的區塊時，記一筆 `teacher_decisions`（`reject`）。
- 伺服器重啟會中斷工作：`queued`／`running` 超過 `8 × AI_TIMEOUT_SECS + 60` 秒的在下次讀取時結束為 `error`。

### 單一區塊重新產生與復原

- `regenerate`：活動區塊＝對那幾段重跑 `tasks`（只要這個區塊）再跑 `scaffolds`；暖身＝`warmup`＋`scaffolds`；Exit ticket＝`exit_ticket`。temperature 依這份學習單先前產生的次數遞增（閘道會快取相同的請求）。
- 取代之前，先把目前的區塊存進 `worksheet_block_history`。`undo` 取回最近一筆**還沒復原的**並在那一列記上 `undone_at`（**不刪除**；可一路復原到最早）。兩者都產生新的學習單版本（`regenerate`／`undo`），所以整份的版本紀錄也還在。詳見「復原與修改歷程」。
- 重新產生失敗時區塊維持原樣，`WsJob.message` 說明原因。

## 結果編修

- 前端直接改 `content` 後 `PUT`（自動儲存，約 1.5 秒）。教師改過 AI 產生的區塊、題目或鷹架時，把該筆的 `origin` 改成 `ai_edited`；自己新增的是 `teacher`。
- 可以改的：所有文字、參考答案、題目與鷹架出現的版本、區塊順序、`enabled`（關掉暖身或 Exit ticket）、刪除任何區塊／題目／鷹架、新增區塊與題目、學習目標、`show_objectives`、標題與檔名。
- `WsItem.answer_format`、`student_tasks`、鷹架的 `kind` 在儲存時由伺服器重新驗證；`WsVocab.level` 一律重新查表。

## 與課例 SOP 的同步（規格 §9.5）

學習單裡改目標或任務**不會**動到課例。`sop_sync.changes > 0` 時前端顯示一行提示；教師按「同步回課例」才呼叫預覽：

```ts
interface SopSyncChange {
  key: string;                              // 例如 "objective:O1"、"block:b2"
  target: "objective" | "task_block";
  action: "add" | "update" | "remove";      // remove：課例裡有、學習單已刪掉（預設不勾）
  label: string; before: string; after: string;
}
interface SopSyncPreview {
  lesson_id: number; sop_version_no: number | null; worksheet_version_no: number; worksheet_updated_at: string;
  changes: SopSyncChange[];
  core_suggestions: [];                     // 預留：「建議補充至單元想法（CoRe）」；這一版永遠是空陣列
}
```

`POST …/sop-sync { confirm: true, keys, sop_version_no, worksheet_version_no, worksheet_updated_at, lesson_id }` 只套用 `keys` 裡的項目，寫成課例 SOP 的**新版本**（只增不改，舊版本仍在）。對應方式：目標以 `sop_id` 對應；活動區塊以 `sop_block_id` 對應，寫回時每一題成為一個 SOP 任務（`text` = 題目，`function` = 區塊的資訊處理功能，三種鷹架各自合併成文字，`origin` 依學習單的 `origin` 轉成 `teacher`／`ai_adopted`／`ai_edited`）。套用後學習單的 `sop_id`／`sop_block_id` 會補上。

預覽從同一個資料庫快照讀取課例與學習單。確認時先鎖學習單、再鎖課例，於鎖內重讀所有權、
課例連結及兩邊版本；不符回 409，請重新預覽。`worksheet_version_no`、`worksheet_updated_at` 與 `lesson_id` 是新增的
確認保護；更新時間也涵蓋未新增內容版本的教材／設定變更。舊客戶端未傳仍相容；新版網頁固定帶入預覽回傳的值。課例版本、學習單對應 ids 與
`worksheet_sop_synced` 研究事件在**同一交易**提交，任一步失敗全部回滾。連結／另存新課例
也使用單一交易，另存失敗不會留下半完成課例、教材或連結。

## 詞彙級別檢查（規格 §9.6 的決定性部分）

```ts
interface WsVocabCheck {
  id: number; level: WsLevel; audience: "student"; target_level: number;
  version_no: number;            // 分析的是哪一個學習單版本
  checked_at: string;
  stale: boolean;                // 學習單在那之後改過
  analysis: VocabularyAnalysis;  // Phase 2.0 的結果：級別分布、分母、超出目標級別的詞與位置、專有名詞／未收錄／複合詞與片語另列、限制、聲明
  lines: string[];               // analysis 中 position.line（從 1）對應的位置說明，例如 "A 暖身 · 字詞表"、"C · 第 4 題"
}
```

分析的文字是**學生版**（該版本實際印出的字）。沒有模型呼叫；級別、版本、分母與限制全部來自 Phase 2.0 的 `vocabulary::analyze`。結果連同學習單版本存進 `worksheet_checks`；之後學習單有新版本，舊結果以 `stale = true` 回傳。這是備課輔助，不是官方評定（`analysis.disclaimer`）。

## 詞彙級別檢查：處理超出級別的字

教師回饋：「針對超出級別的字，讓老師選要提供單字意思，還是換字？」每個超出目標級別的字，教師選一種處理方式：

- **提供字義**（`gloss`）：保留原字，在這一區最後的 **Glossary** 印出「字 (詞性) 中文意思 — 簡短英文解釋」。
- **換字**（`replace`）：題目、選項、作答指示裡**有這個字的每一句**，由模型整句改寫一次（避開這個字、意思不變、只改那個字附近），伺服器逐句檢查；教師看到每一句「改寫前 → 改寫後」，可以逐句取消，套用時照畫面上的句子寫入。
- **不處理**（`leave`）：例如專有名詞，或這節課要教的字。記在這個版本的檢查結果裡，下次分析仍標「已標記不處理」。
- 一次選全部：「全部提供字義」「可以換的全部換字」「全部不處理」「照建議選」。

### 設計決定

- **字義印在哪裡**：每一區最後的 Glossary（表格：Word／Meaning），在題目之後、教師備註之前；學生版與教師版都印，編輯器、備課對話的預覽、DOCX、PDF 一致。不用括號內嵌：會拉長題目與選項（選項長短不一、中文字義可能洩漏答案），也改動分析的文字；不用註腳：DOCX 與 PDF 的產生器都沒有註腳機制，而 Glossary 沿用既有的「Key words」表格與區塊框。Glossary 不計入詞彙級別分析（它解釋字，不是閱讀量）。
- **字義從哪裡來**：大考中心參考詞彙表只有字、級別與詞性，**沒有中文意思**，所以用一次有上限的模型呼叫（所選的字一次送出，最多 20 個）。詞性以詞彙表為準（模型給的不在詞彙表列的詞性裡時改用詞彙表的）；中文意思一定要有中文；英文解釋用到超出目標級別的字（常常就是這個字本身）時由程式拿掉。
- **課文不換字**：學習單的閱讀文本（教材）本身不在學習單內容裡，也不在分析範圍，永遠不會被改。換字也不動：字詞表（這節課要教的字，預設「不處理」）、字庫（字詞表鷹架與題目的 word bank：它們是答案）、調整文本（改寫的課文，T8）、標題與學習目標、圖說、圖像組織的中心詞與格子（預填的示範與參考答案：它們是答案的內容），以及**引號內**的字（「What does "abandon" mean?」是在問這個字本身或引用課文）。這些地方的出現次數列在 `fixed`。圖像組織的標題與標籤（欄名、列名、框名）和表格欄位一樣可以換字。分析讀的是學生版印出的圖像組織：標題、中心詞、標籤與這個版本預填的格子（參考答案只印在教師版，不分析）。課文也有的字（`in_material`）預設建議「提供字義」：換掉題目裡的字，學生回課文就對不上。
- **為什麼整句改寫**：最早的做法是逐字形替換（abandoned → left），用真實模型（gemma）測試時造出不通的英文：「according to the text」→「as said to the text」（according 換了、to 留著）、「might accidentally hurt」→「might by mistake hurt」、「the most accurate summary」→「the most correct short report」。字形替換看不到句子，所以改成把整句交給模型改寫，再由伺服器檢查改寫的範圍與用字，教師逐句確認。
- **版本範圍**：分析是逐版本的。換字改的是這個版本印出的文字；題目與作答指示是幾個版本共用的（T1、T2：同一題在各版問同一件事，只差在支持），所以共用的題目其他版本也會一起改，畫面上有說明；只屬於別的版本的題目、鷹架與分層支持不會動。字義預設只加在分析的版本（例如只給基礎版，對應 T8「難字的意思直接寫在文中」與 V1–V3：關鍵字要少），也可以選「所有印出這個字的版本」。每個版本的字義放在那個版本**第一個印出這個字的區塊**。

### 端點

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/worksheets/:id/checks/:check_id/suggest` | `{ action: "gloss" \| "replace", words: string[] }`（`words` 是這次檢查 `words[].base`，1–20 個） | `WsVocabSuggestions`。這次檢查已經過期（學習單之後改過）→ 409；字不在這次檢查裡、`replace` 了一個沒有可換位置的字、這些字一共超過 40 句 → 400 |
| POST | `/api/worksheets/:id/checks/:check_id/apply` | `WsVocabApplyRequest` | `WsVocabApplied`（含 `worksheet: WorksheetState` 與重新分析的 `check`）。`base_version_no` 不是最新版本或檢查已過期 → 409（「請重新分析」）；學習單正在產生 → 409；句子和學習單現在的文字對不上 → 409；改寫句沒通過檢查、這一句還有沒選換字的字、選了換字卻沒勾任何一句 → 400 |

```ts
interface WsVocabCheck {            // 既有的檢查結果多了 words（舊的結果沒有，重新分析就有）
  …;
  words?: WsOverLevelWord[];        // 依 analysis.above_target 的順序（級別高的在前）
}
interface WsOverLevelWord {
  base: string; level: number;
  pos: string;                      // 詞彙表印的詞性，"v."、"n./v."；沒有時 ""
  count: number;
  forms: string[];                  // 學習單裡的字形（小寫），依出現順序
  replaceable: number;              // 換字會改到的次數
  fixed: string[];                  // 換字不會動的地方與原因，例如 "A 活動 · 字詞表：字詞表（這節課要教的字）"、"A 活動 · 第 2 題（引號內）"
  in_material: boolean;             // 課文也有
  key_word: boolean;                // 字詞表裡的字
  glossed: string | null;           // 這個版本已經附上的中文意思
  kept: boolean;                    // 教師之前選了不處理
  suggested: "gloss" | "replace" | "leave"; reason: string;
  contexts: string[];               // 最多 3 個上下文（送給模型）
}
interface WsVocabSuggestions {
  action: "gloss" | "replace";
  status: "shown" | "blocked" | "timeout" | "error" | "disabled";   // shown：至少一個字（一句）有建議
  message: string | null;           // 例如「有 1 句這次沒有可用的改寫…，這些地方不會改。」
  interaction_id: number | null;    // 最後一次呼叫
  words: Array<{                    // 依請求的順序
    base: string;
    interaction_id: number | null;  // 寫出這個字建議的那一次呼叫（第二次呼叫補上的是第二次的 id）
    gloss: { pos: string; meaning_zh: string; definition_en: string } | null;
    replacement: string | null;     // 換字：改寫句主要用來代替它的字（提示；在目標級別以內）
    places: string[];               // 換字：有這個字的句子（places[].id）
    notes: string[];                // 程式拿掉的建議與原因（超出級別、不在詞彙表、英文解釋太難、句子都沒有可用的改寫）
  }>;
  places: Array<{                   // 只有換字：每一句，依學習單的順序；兩個所選的字在同一句時是同一筆
    id: string;                     // "p1"、"p2"…（只在這次回應裡唯一）
    field: { block: string;
             part: "title" | "instructions" | "step" | "support" | "prompt" | "option"
                 | "table_heading" | "organizer_title" | "organizer_label" | "hint" | "tier_support";
             id?: string;           // 鷹架或題目的 id
             index?: number };      // 第幾個步驟／選項／欄位／圖像組織的格（section）／支持（從 0）
    place: string;                  // 「A 活動 · 第 1 題」
    start: number; end: number;     // original 在那段文字裡的位置（字元）
    original: string;               // 原句
    bases: string[];                // 這一句要避開的字
    rewritten: string | null;       // 改寫後的整句；沒有通過檢查時是 null（這一句不會改）
    interaction_id: number | null;
    problem: string | null;         // 為什麼沒有可用的改寫
  }>;
}
interface WsVocabApplyRequest {
  base_version_no: number;          // check.version_no
  gloss_scope?: "level" | "all";    // 預設 level
  words: Array<{
    base: string; action: "gloss" | "replace" | "leave";
    gloss?: { pos?: string; meaning_zh: string; definition_en?: string };     // gloss 必填 meaning_zh（教師可以改，也可以不經模型自己寫）
    interaction_id?: number | null; // 這個字的建議來自哪一次呼叫（記 teacher_decisions）
    offered?: number;               // 換字：教師看到幾句有改寫（全部套用＝adopt，取消了幾句＝revise）
  }>;
  rewrites?: Array<{                // 換字：教師勾選的句子，照畫面上的樣子
    field: …; start: number; end: number; original: string; rewritten: string; bases: string[];
  }>;
}
interface WsVocabApplied {
  version_no: number | null;        // 新版本；只有「不處理」時學習單不變，是 null
  glossed: string[]; left: string[];
  replaced: Array<{ base: string; sentences: number; left: number }>;  // sentences：改寫了幾句；left：本來可以換、但沒改（取消或沒有改寫）的句子
  check: WsVocabCheck;              // 套用後重新分析（同一個版本、同一個目標級別）
  worksheet: WorksheetState;
}
```

**學習單內容多了 `blocks[].glosses`**（只增不改；沒有時沒有這個欄位，編輯器存檔時原樣送回）：

```ts
interface WsGloss { word: string; pos: string; meaning_zh: string; definition_en: string;
                    levels: WsLevel[]; origin: "teacher" | "ai" | "ai_edited"; }
// WsSheetBlock.glosses?: Array<{ word, pos, meaning_zh, definition_en }>   這個版本要印的，印在區塊最後
```

儲存時驗證：每區最多 30 筆，`word` ≤60、`pos` ≤20、`meaning_zh` ≤60（必填）、`definition_en` ≤200，同一區同一個字只能一筆，沒有任何版本的那一筆拿掉。編輯器在 Glossary 每一筆旁有「移除」（只從目前顯示的版本拿掉）。重新產生區塊時 Glossary 不保留（字可能已經不在了；「復原上一版」可以拿回來），逐區修改的「採用」會保留。

### 套用

換字的每一句先檢查：`field` 指向這個版本印出、而且換字可以改的文字，`start..end` 的文字和 `original` 完全一樣（否則 409），`rewritten` 再跑一次下面的改寫檢查（不通過 400），句子裡每個要避開的字都選了換字。同一段文字的幾句從後往前換，不能重疊。

一個交易裡：鎖住學習單 → 確認最新版本仍是 `base_version_no` → 每個改到的區塊先存進 `worksheet_block_history`（附上那個字的建議呼叫，教師「復原上一版」時記一筆 `teacher_decisions` reject）→ 新版本（`cause = "vocabulary"`）→ 決定與紀錄。之後用同一個版本與目標級別重新分析，回傳新的結果：換掉的字不見了，提供字義的字標「已提供字義」，不處理的標「已標記不處理」。所以「復原上一版」照常逐區可用，`base_version_no` 的 409 也和編輯器一致。

### 模型呼叫

| action | prompt key／`ai_interactions.module` | 鷹架 | schema | `max_tokens` |
|---|---|---|---|---|
| `gloss` | `worksheet_vocab_gloss` | `SC-WS-VOCAB` | `glosses`：每字一筆（min = max = 字數），依序 `word`（enum：所問的字）、`pos`（≤12）、`meaning_zh`（≤30）、`definition_en`（≤120） | 300 + 120 × 字數 |
| `replace` | `worksheet_vocab_replace` | `SC-WS-VOCAB` | 先 `words`：每字一筆，依序 `word`（enum）、`replacement`（≤40，主要的代替字，只當提示）；再 `sentences`：每句一筆（min = max = 句數），依序 `id`（enum：p1…）、`rewritten`（≤ 原句長度 + 60） | 300 + 40 × 字數 + Σ(40 + 句長 ÷ 2)，最多 8000 |

- 每個字一筆陣列項目（gemma 想換行時會提早結束字串）；字串與陣列都有上限；欄位依寫出順序。
- 送出的是字、級別、詞彙表詞性、學習單裡的字形與最多 3 個上下文；換字再加上每一句（id、位置、要避開的字、原句）。不送整篇課文；輸入快照裡上下文與句子只留字數與雜湊。
- **句子怎麼切**：換字可以改的每一段文字（區塊標題、作答指示、步驟、鷹架、題目、選項、表格欄位、圖像組織標題與標籤、提示、分層支持），從這個字往前找到上一個句尾（. ! ? 後面接空白）或換行，往後到自己的句尾（含後面的引號、括號）或換行；選項這類沒有句號的就是整段。引號內的出現不算。
- 第一次呼叫沒交出某些字、或那些字的建議全部不合格（超出級別、不在詞彙表、缺字形）→ 再呼叫一次（temperature +0.4，附上問題），只補上缺的字；兩次都有的以第一次為準。同一份學習單再次請求建議時 temperature 依次數遞增，提示也註明第幾次（閘道會快取相同的請求）。
- **改寫檢查**（`check_rewrite`，建議時與套用時各跑一次）：
  - 一行英文、和原句不同；要避開的字不能再出現（引號內的除外）。
  - 每個**新加**的字都要在詞彙表、在目標級別以內（含片語與連字號詞的每個部分）；原句本來就有的字（包括未收錄的字、專有名詞、其他超出級別而沒選換字的字）可以留著。
  - 改動要局部：以字為單位比對（LCS），每個要避開的字最多改掉原句 2 個字、加入 3 個字（另加 1）；每一段改動都要在這個字前後 3 個字以內。整句改寫、改到遠處的句子不採用。
  - 不通過的句子在 `places[].problem` 說明，第二次呼叫只補上缺的；兩次都沒有的句子不改。

### 教師的決定（研究用）

- 有模型建議的字寫 `teacher_decisions`（`ai_interaction_id` = 這個字的 `interaction_id`，`item_index` = 模型輸出中這個字的位置）：照建議 → `adopt`（換字：教師看到的改寫句全部套用，`reason_note` 記「換字 abandon（套用 3／3 句）」）；改了中文意思、或換字取消了幾句 → `revise`（`revised_content` 是改後的字義，或套用的句子以「 / 」連起來）；請了建議卻改選別的處理方式或不處理 → `reject`。不必另外呼叫 `POST /api/ai/:id/decision`。
- 每次套用寫一筆 `usage_logs`：`worksheet_vocabulary_applied`，payload `{ worksheet_id, check_id, level, target_level, base_version_no, version_no, gloss_scope, words: [{ base, level, action, … }] }`（只有字與選擇，不含學習單文字）。沒有模型呼叫的選擇（不處理、自己寫的字義）只在這裡。

### 假模型標記

| 標記（出現在送出的上下文裡） | 效果 |
|---|---|
| `FAKE_VOCAB_OVER` | 每個字的提示字是詞彙表外的字 → 被拿掉並寫在 `notes`；第一句的改寫用了第 6 級的字 → 不採用 |
| `FAKE_VOCAB_WHOLE` | 第一句改寫成另一句話 → 改動太多，不採用 |
| `FAKE_VOCAB_HARD_DEF` | 英文解釋用到這個字本身 → 英文解釋被拿掉 |
| `FAKE_VOCAB_MISSING` | 第一次呼叫少最後一個字（字義）或最後一句（換字），第二次補上 |

假模型的改寫：先整個換片語（according to → based on、an enormous → a huge），再換字形（abandoned → left…）。

### 真實模型可能的困難（給測試 gemma 的人）

- 局部改寫：模型可能把整句重寫或順手改別的字，這些會被「改動太多／改到較遠的地方」拿掉；局部規則（每個字改 2 加 3、前後 3 個字）是依目前的例子定的，真實測試後可能要調。
- 語序：「accidentally hurt」→「hurt … by mistake」需要把片語移到句尾，這在規則內（插入點在 3 個字以內）；更遠的移動會被拒絕，那一句就不改。
- 意思漂移：「accurate summary」→「correct short report」這類在級別上合格、改動也小，伺服器擋不住，只能靠教師逐句看改寫前後時取消。
- 在目標級別（例如第 4 級）以內找到意思相同的說法不一定做得到，常見結果是句子沒有可用的改寫；這時改用提供字義。
- 中文意思要符合上下文的詞義（例如 bank 是銀行還是河岸）；詞彙表不分詞義，伺服器只能檢查有沒有中文。
- 英文解釋常常用到這個字本身或更難的字，會被拿掉，只剩中文。

## 匯出

`GET …/export` 由後端即時產生並串流回瀏覽器（不經物件儲存）：

- 沒有內容可印的區塊（例如某一步沒有成功而留下的空區塊，只有標題與 `message`）不會出現在投影、DOCX 與 PDF，後面的區塊字母依序遞補；編輯器仍顯示它與說明，並註明「印出與匯出時會略過」。只有作答指示的區塊（例如討論活動）照常印出。
- **DOCX**：直接寫 WordprocessingML（`zip` crate），段落、編號、表格、作答線皆為 Word 原生物件，可編輯。
- **PDF**：純 Rust 產生，字型子集化後內嵌。內嵌的拉丁字型是 Libertinus Serif（OFL，來自 `typst-assets` crate）；中文字由 `WORKSHEET_CJK_FONT`（或映像檔內建的中文字型）提供。學習單含中文而環境沒有中文字型時回 409，請教師改用 DOCX。
- 檔名：`<filename>_<版本>_<學生版|教師參考版>.<副檔名>`（RFC 5987 `filename*`）。教師參考版在每題下方印參考答案與依據，並印 `teacher_notes`。

## 測試用假模型（`AI_PROVIDER=fake`）新增的標記（寫在教材文字裡）

| 標記 | 效果 |
|---|---|
| `FAKE_WS_CJK` | `tasks` 第 1 次有一題題目含中文；修復後改正 |
| `FAKE_WS_CJK_ALWAYS` | 兩次都含中文 → 該題被拿掉 |
| `FAKE_WS_BAD_LABEL` | `tasks` 第 1 次用了表外的學生任務標籤；修復後改正 |
| `FAKE_WS_SPLICE_ALWAYS` | `tasks` 有一筆依據以「...」拼接、沒有 5 個字以上的逐字片段 → 依據被清空（不需修復呼叫） |
| `FAKE_WS_WARMUP_BLOCK` | `warmup` 兩次都不合格 → 暖身區塊留空並附說明 |
| `FAKE_WS_NO_PROCEDURAL` | `scaffolds` 不交程序鷹架 → 由程式補上 |
| `FAKE_WS_LEAK` | `scaffolds` 第 1 次有一個鷹架含答案原文；修復後改正 |
| `FAKE_WS_TARGET_ALIAS` | `scaffolds` 把 target 寫成 `Warm-up`／`b1`，沒有暖身時還多給一筆 `warmup` → 由程式對應，不需修復 |
| `FAKE_RUNAWAY` | 每一步第 1 次都寫到 `max_tokens`（物件寫到一半後一直輸出空白）；修復呼叫正常 |

既有的 `FAKE_TIMEOUT`、`FAKE_ERROR`、`FAKE_REFUSAL`、`FAKE_BLOCK`、`FAKE_SLOW` 照舊（對四個步驟都有效）。

## usage_logs 新事件

`worksheet_created`、`worksheet_generated`、`worksheet_block_regenerated`、`worksheet_block_undone`（含復原採用：`proposal_id`、`undone_version_no`、`pictures_reverted`）、`worksheet_sop_synced`、`worksheet_linked`、`worksheet_vocabulary_checked`、`worksheet_vocabulary_applied`、`worksheet_exported`（payload 只有 id、版本、格式等，不含內容）。

## 前端

- `/prep`「我的作品」：一份清單，依 `updated_at` 新到舊，含每一次備課對話（連到 `/prep/:id`）和不屬於任何備課對話的獨立學習單（`prep_session_id` 為空，連到 `/worksheets/:id`，可刪除）；屬於備課對話的學習單不另列。上方篩選晶片「全部／學習單／課堂活動／問 Manna」（單選，記在 `?type=worksheet|activity|ask`；學習單包含 kind 為 worksheet、comprehension、tiered、kit、assessment、revise 的備課對話與獨立學習單）。`/worksheets`（清單）轉到 `/prep`；已沒有「直接做一份」的表單，新的學習單一律從首頁的備課開始（`POST /api/worksheets` 仍供課例頁「產生學習單」使用）。`/worksheets/:id`：還沒產生過→設定頁（教材在左、四個區塊在右）；產生過→結果頁（版本分頁、學生版／教師參考版、區塊工具）。`/worksheets/:id/check`：詞彙級別檢查。課例頁的「產生學習單」會建立並開啟一份連結這個課例的學習單。
- 教材欄是一個可替換的插槽（`materialPane`）：這一版放唯讀的教材檢視；教材庫分支的 `MaterialViewer`（含註記）之後接在同一個位置。

---

# Phase 2.4：語意索引與混合檢索

以下為**新增或變更**；未提到的端點維持原樣。Migration `0011_embeddings.sql`。評估見 [`RETRIEVAL_EVAL.md`](RETRIEVAL_EVAL.md)。

## 重點

- 知識庫條目可以建立**語意索引**（embedding 向量）。檢索時同時用原本的關鍵字（英文詞＋中文二字組）與語意相似度排序，再合併（混合檢索）。英文教材或目標因此也找得到中文條文；只用關鍵字時一條都找不到。
- **語意索引是加分，不是依賴**：沒設定模型（`EMBEDDING_PROVIDER=none`，預設）、模型逾時或出錯、條目還沒建索引時，一律退回只用關鍵字，AI 功能、匯入與啟動都不會因此失敗或變慢超過查詢逾時（預設 2.5 秒）。
- 過濾條件不變：學校類型（含 `both`）、`kind`、`stage`、`subject`、只取未停用的最新版本，都在排序**之前**套用；語意相似度只在這些候選內排序。
- 條文仍一律由資料庫依代碼帶出；模型只能引用檢索清單中的代碼（[LLM_VALIDATION.md](LLM_VALIDATION.md)）。語意索引只改變「哪些條目進清單、排第幾」。

## 設定（環境變數）

| 變數 | 預設 | 說明 |
|---|---|---|
| `EMBEDDING_PROVIDER` | `none` | `openai_compatible`（國網中心）\| `fake`（測試用，決定性）\| `none` |
| `EMBEDDING_BASE_URL` | – | `openai_compatible` 必填，例如 `https://portal.genai.nchc.org.tw/api/v1`；呼叫 `{base}/embeddings` |
| `EMBEDDING_API_KEY` | – | Bearer 金鑰。國網中心的 embeddings 金鑰與對話模型的金鑰**不同** |
| `EMBEDDING_MODEL` | `embeddinggemma-2` | |
| `EMBEDDING_DIMS` | `768` | 必須等於模型輸出的維度；不符的回應視為失敗 |
| `EMBEDDING_QUERY_TIMEOUT_MS` | `2500` | 檢索時等查詢向量的上限，超過就只用關鍵字 |
| `EMBEDDING_TIMEOUT_SECS` | `60` | 建立索引時每批（32 條）的上限 |

設定值不合法（未知的 provider、缺 `EMBEDDING_BASE_URL`、維度不是數字或超過 16,000）時**不會擋住啟動**：語意索引關閉，啟動紀錄寫一行警告。金鑰不會出現在紀錄或錯誤訊息（錯誤只記 HTTP 狀態碼）。

## 嵌入的內容

每個條目嵌入一段文字（`kb_embeddings::embed_text`）：

- 課綱條目：`科目｜學習表現或學習內容｜類別名稱：條文`，例如 `英語文｜學習表現｜語言能力（讀）：<條文>`。**不含代碼**：代碼對模型只是符號，精確代碼查詢另有 `by-code`。四種寫法的比較見 RETRIEVAL_EVAL.md。
- 其他條目（文法、SIOP 等）：`標題：內容`。超過 2,000 字的截斷。
- 教師備課經驗（`teacher_cases`）沒有向量，只用關鍵字參與排序。
- 查詢：送出的文字去頭尾空白，取前 2,000 字。同一段查詢在同一個執行個體內只嵌入一次（最近 64 筆的快取）。

## 資料表

- `knowledge_items.embedding`（`vector(1024)`，從未使用）**移除**。
- `knowledge_embeddings`（新）：每個 `knowledge_items` 列（＝一個版本）一筆：`model`、`dims`、`content_hash`（嵌入文字與版本標記的 SHA-256）、`embedding`（`vector`，不固定維度）。檢索**只讀目前設定的 `model`＋`dims`** 的列；換模型時舊向量自動被忽略，重新建立後取代。768 維的列有 HNSW 索引（cosine）；目前的檢索在已過濾的候選內精確計算相似度（幾千條只要數毫秒，也不會因近似搜尋漏掉過濾後的條目）。
- `embedding_jobs`（新）：每次建立索引一筆：`trigger`（`import` \| `manual` \| `startup`）、`model`、`dims`、`status`（`running` \| `done` \| `failed`）、`total`、`embedded`、`error`、`started_by`、時間。
- `retrieval_logs` 新增：`method`（`keyword` \| `hybrid` \| `all`＝全部條目都給模型、沒有排序）、`embedding_model`、`fallback_reason`（有設定模型卻沒用上的原因：`empty_query`、`embedding_timeout`、`embedding_failed`、`no_vectors`、`vector_lookup_failed`）、`candidates`（回傳的條目依序：`{ item_code, keyword_rank, keyword_score, vector_rank, similarity, fused }`）。

## 建立索引

- **匯入後自動**：課綱或知識庫匯入**提交之後**，在背景開始；匯入的回應不等它。啟動時也會在背景補做缺少的部分（第一次部署、換模型、上次中斷）。
- **冪等**：一列的 `model`、`dims`、`content_hash` 都相符就不再嵌入。沒有變動的重複匯入不會開始建立。
- **可續做**：每批 32 條，每批存一次；失敗的批次重試 2 次，仍失敗就停下並記在 `embedding_jobs.error`，已存的保留，下次從缺少的繼續。
- 同時只有一個建立在跑；跑的期間又有匯入，結束時會再掃一次。停用（舊版本）條目的向量會被刪除。

## 端點

### 知識庫管理（`pi` 或 `kb_manager`，`manage_knowledge`）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/admin/knowledge/embeddings` | – | `EmbeddingStatus` |
| POST | `/api/admin/knowledge/embeddings/rebuild` | `{ force?: boolean }`（可不帶內容；`force` 全部重做，預設只補缺少或過期的） | `202` `EmbeddingStatus & { started: boolean }`（`started: false`：已有一次在跑）；沒設定模型 → `409` |

```ts
interface EmbeddingStatus {
  enabled: boolean;              // false：沒設定模型，只用關鍵字
  provider: "none" | "fake" | "openai_compatible";
  model: string | null; dims: number | null;
  active_items: number;          // 使用中的條目（含示範條目）
  embedded: number;              // 有目前模型向量的使用中條目
  other_model: number;           // 舊模型或其他維度的向量（檢索不用）
  running: boolean;
  job: { id; trigger; model; dims; status; total; embedded; error: string | null; created_at; finished_at } | null;  // 最近一次
}
```

`usage_logs` 新事件 `knowledge_embeddings_rebuild`：`{ force, model }`。

### 教師端變更

- `GET /api/knowledge/curriculum`：有 `q` 時用混合檢索；回應多 `method: "keyword" | "hybrid"`，每筆多 `similarity?`（hybrid 才有）；`score` 仍是關鍵字分數，只靠語意找到的條目為 0。`ranked` 在 hybrid 時為 `true`。沒有 `q` 時照舊依代碼列出。
- `POST /api/lessons/:id/suggestions`：檢索改為混合（範圍不變，`RAG_TOP_K` 筆）；`input_snapshot.retrieval` 多 `method`、`fallback_reason`，每筆多 `similarity`。有設定模型且條目有索引時，幾乎一定會取回條目（語意相似度沒有門檻，理由見 RETRIEVAL_EVAL.md），所以「知識庫查無」主要剩在沒有索引、關鍵字也找不到時。
- SOP 目標（`POST /api/lessons/:id/sop/objectives`）：條目不超過 150 條的學校類型（普高、綜高）照舊全部給模型（`method: "all"`）；超過的（技高 331 條）以混合檢索取前段、再依代碼補滿 150 條。

## 排序與融合

`retrieval::fuse`：加權倒數排名融合。

```
fused = 1.0 × (關鍵字分數 / 本次最高關鍵字分數) / (60 + 關鍵字名次)
      + 1.0 / (60 + 向量名次)
```

- 關鍵字名次為並列名次（同分同名次）；沒有關鍵字命中或沒有向量的條目，該項為 0。
- 同分依相似度高者、再依代碼。
- 參數由 RETRIEVAL_EVAL.md 的 31 題決定；k 與權重的其他組合差異在雜訊範圍內。

## 前端

知識庫管理頁多「語意索引」區塊：「1,352 條中 1,352 條已建立語意索引」、模型與維度、舊模型的向量數、最近一次的結果（中斷時顯示原因與已完成數），以及「建立語意索引」按鈕（全部完成時為「重新檢查」）。建立中每 1.5 秒更新進度。沒設定模型時只顯示「尚未設定語意索引模型，目前只用關鍵字檢索。」

---

# Phase 2.2：教材與註記

以下為**新增或變更**；未提到的端點維持原樣。Migration `0009_materials.sql`。依據：《學習單產生器：教師選項與 SOP 整合規格》§4.2、§4.4、§9.7、§10 第 23–28 項，`docs/BLUEPRINT.md` §13.3（教材只屬於一位教師、不共用、不訓練；刪除即實際刪除原檔與衍生資料）。給規格 §12 第 1、11、12、13 項的回覆在 `docs/ENGINEERING_REPLY.md`。外部資源搜尋（§4.3）不在這一版。

## 重點

- **教師自己的教材庫**：貼上文字、上傳檔案、或記下一個網址。只有擁有者看得到；其他人存取任何一份都是 404。
- **原檔放物件儲存**（`STORAGE_BACKEND=s3|fs|none`，見 README「教材檔案儲存」），瀏覽器從不直接連到儲存空間。
- **刪除是真的刪除**：原檔物件、擷取的文字與段落、註記、課例連結一起刪；課例 SOP 留下「教材已刪除」，逐字引文從 SOP 與 AI 紀錄移除（見「刪除」）。
- **註記另外存**，不改教材；每則可勾「提供給 AI」（預設不勾）。只有勾了、沒隱藏、找得到位置的註記會給 AI，並標明是教師註記。註記不會出現在學生版或任何學生端輸出。
- 課例 SOP 第 1 步改用教材庫（貼上、從教材庫選、上傳），舊的 `lesson_sop_materials` 搬進教材庫後刪除。

## 能力（capability）

新增 `use_materials`（教材庫）：目前所有角色都有（每個帳號都可以備課）；放成能力是為了日後要收窄時只改 `auth::role_can` 一處。`GET /api/me` 的 `capabilities` 多這一項；`GET /api/config` 多 `uploads_enabled: boolean`。

## 可以上傳什麼

| 類型 `kind` | 副檔名 | 單檔上限 | 處理 |
|---|---|---|---|
| `document` | `.txt` `.docx` `.pdf` | 20 MB | 擷取文字（見下），可註記、可給 Manna 分析 |
| `image` | `.png` `.jpg`（`.jpeg`）`.webp` `.gif` | 10 MB | 保存、在檢視區顯示；開啟 OCR 時辨識文字（Phase 2.10） |
| `audio` | `.mp3` `.m4a` `.wav` | 30 MB | 保存、瀏覽器內建播放器（可拖曳）；不分析 |
| `video` | `.mp4` `.webm` | 100 MB | 同上 |
| `text` | （貼上） | 50,000 字 | 程式分段 |
| `link` | （網址） | 2,000 字元 | 只存網址、標題、來源說明；**不抓取網頁** |

- 每位教師：檔案合計 1 GB、教材 300 份（任何類型）。上傳時同時最多處理 2 個（其餘排隊）。
- 先看副檔名，再看檔頭（magic bytes）：副檔名對但內容不符（例如改名的執行檔、叫 `.png` 的 HTML）→ 400。`.doc`、`.ppt`、`.svg`、`.html` 等一律不收。
- 同一位教師重複上傳同一個檔案（SHA-256 相同）→ 409，訊息說已經在哪一份。
- 回應的 `Content-Type` 由伺服器依副檔名決定，並一律加 `X-Content-Type-Options: nosniff`、`Content-Security-Policy: default-src 'none'; sandbox`、`Cache-Control: private, no-store`；文件以下載（attachment）回應，圖片與影音以 inline 回應。

## 文字擷取與段落

- **TXT**：UTF-8（含 BOM）、UTF-16（有 BOM）、Big5；分段規則與貼上的文字相同。
- **DOCX**：讀 `word/document.xml` 的每個段落（含表格內的段落與文字方塊），依文件順序；追蹤修訂中刪除的文字不算；tab 與換行視為空白。
- **PDF**：純 Rust（`pdf-extract`）逐頁擷取。有空行時以空行分段，沒有時以「句末標點＋明顯短於最長行」判斷段尾；行尾連字號斷字會接回。每段記下起始頁碼（`page`）。
- **掃描的 PDF**（沒有文字層）：`text_status = "no_text"`，原檔照常保存與下載；開啟 OCR 時在背景辨識文字（見「Phase 2.10：掃描檔與 OCR」）。讀不出來（加密、損壞、解析器不支援的字型）：`failed`，開啟 OCR 時同樣改用 OCR 辨識。超過 300,000 字的文件只保留前面（`text_truncated = true`），原檔完整保存。
- 段落 `id`（`p1`…）在同一份教材內穩定且不重複使用：貼上的文字修改後，文字沒變的段落沿用原 id，教材的 `revision` 加 1；**舊文字不保留**。上傳的檔案不能改文字（要換請刪除後重傳，或另外貼上）。

## 型別

```ts
type MaterialKind = "text" | "document" | "image" | "audio" | "video" | "link";
type MaterialTextStatus = "ready" | "no_text" | "failed" | "not_applicable";

interface Material {            // 清單用，不含文字
  id: number; kind: MaterialKind; origin: "paste" | "upload" | "url";
  title: string; source_note: string; url: string | null; filename: string | null;
  content_type: string | null; size_bytes: number; sha256: string | null;
  has_file: boolean;            // 有原檔可預覽／下載
  text_status: MaterialTextStatus; text_truncated: boolean; page_count: number | null;
  paragraph_count: number; word_count: number; revision: number;
  created_at: string; updated_at: string;
  lessons: Array<{ lesson_id: number; lesson_title: string; unit_title: string }>;
  worksheet_count: number; annotation_count: number;
}
interface MaterialDetail extends Material {
  text: string;
  paragraphs: Array<{ id: string; no: number; text: string; page?: number }>;
}

interface MaterialAnnotation {
  id: number; material_id: number;
  quote: string;                // 標記的原文（伺服器從段落取，不採用用戶端送的文字）
  note: string; share_with_ai: boolean; hidden: boolean;
  anchor: {                     // 註記「現在」在哪裡，每次讀取時重新定位
    status: "ok" | "moved" | "lost";     // lost → 前端顯示「需要重新確認位置」
    paragraph_id: string | null; paragraph_no: number | null;
    start: number | null; end: number | null;   // 段落文字內的 UTF-16 位置（與瀏覽器選取相同）
  };
  created_at: string; updated_at: string;
}
```

`SopMaterial`（Phase 2.1）多了 `kind`、`origin`、`url`、`filename`、`has_file`、`text_status`、`text_truncated`、`page_count`，段落可能有 `page`；`version_no` 現在是教材的 `revision`。`SopState` 多 `material_deleted: boolean`；`GET …/sop/versions` 每列多 `material_deleted`。

## 端點（登入即可；只能存取自己的教材，其他人的回 404）

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/materials` | `?lesson_id=`（選填：只列連到該課例的） | `{ materials: Material[], usage: { used_bytes, quota_bytes, count, max_count }, limits: { files: [{ ext, kind, max_bytes }], max_pasted_chars, max_note_chars }, storage: "s3"\|"fs"\|"none", uploads_enabled }`，新到舊 |
| POST | `/api/materials` | `{ kind: "text", title?, source_note?, text, lesson_id? }` 或 `{ kind: "link", title?, source_note?, url, lesson_id? }` | `MaterialDetail` |
| POST | `/api/materials/upload` | `multipart/form-data`：`file`（必填）、`title`、`source_note`、`lesson_id` | `MaterialDetail`。`title` 空白時用檔名。儲存未設定 → 503 `unavailable`「這個環境還沒有設定檔案儲存空間…」 |
| GET | `/api/materials/:id` | – | `MaterialDetail` |
| PUT | `/api/materials/:id` | `{ title?, source_note?, text?, url? }` | `MaterialDetail`。`text` 只限貼上的教材（不能改成空白：不要了請刪除），`url` 只限網址教材 |
| DELETE | `/api/materials/:id` | – | `{ deleted: true, annotations: number, lessons: number[] }`（見「刪除」） |
| POST | `/api/materials/:id/file-link` | – | `{ url, expires_at }`：給 `<img>`／`<audio>`／`<video>` 用的 60 分鐘連結（見下）。沒有原檔 → 404 |
| GET | `/api/materials/:id/file` | Bearer 權杖，或 `?t=<file-link 的權杖>`；`?download=1` 改為下載；支援 `Range: bytes=…` | 原檔（200 或 206；範圍不合 416） |
| GET | `/api/materials/:id/thumbnail` | Bearer 權杖 | 預覽圖（JPEG，寬 480 px）：上傳的 PDF 第 1 頁或圖片。見「知識庫：教材牆與預覽圖」 |
| PUT／DELETE | `/api/lessons/:id/materials/:material_id` | – | 把教材連到課例／取消連結（多對多）。課例 SOP 第 1 步正在用的教材不能在這裡取消（409，請到第 1 步換教材） |
| GET | `/api/materials/:id/annotations` | – | `MaterialAnnotation[]`，依閱讀順序；隱藏的也列（`hidden: true`），找不到位置的排最後 |
| POST | `/api/materials/:id/annotations` | `{ paragraph_id, start, end, note?, share_with_ai? }` | `MaterialAnnotation`。範圍必須在該段落內；一份教材最多 500 則；註記 ≤ 2,000 字、標記 ≤ 2,000 字 |
| PUT | `/api/annotations/:id` | `{ note?, share_with_ai?, hidden?, anchor?: { paragraph_id, start, end } }` | `MaterialAnnotation`。`anchor` 用於教師重新確認位置 |
| DELETE | `/api/annotations/:id` | – | `{ deleted: true }`。只刪註記，教材不變 |
| GET | `/api/materials/:id/annotations/ai` | – | `{ material_id, notes: Array<{ annotation_id, paragraph_id, paragraph_no, quote, note }>, prompt_block }`：**只有**勾了提供給 AI、未隱藏、找得到位置的註記 |

### 為什麼是後端串流，不是 MinIO 的 presigned URL

`file-link` 發的是**本站 API** 的連結：權杖以後端程序內的隨機金鑰（HMAC-SHA256）簽章，綁定教材 id、教師 id 與到期時間；使用時仍重新檢查教材存在且屬於該教師。所以 MinIO 不必對外開放、連結外流也打不開別人的東西、教材一刪除連結立即失效。代價是檔案流量經過後端，影音以 Range 分段傳送，對 2 核心主機可以接受。

### 給 AI 管線的註記

Rust 端：`materials::annotations::shared_with_ai(db, teacher_id, material_id)` 回傳上述 `notes`，`ai_context_block(&notes)` 產生 prompt 文字，開頭固定為「教師註記（教師自己寫的備課筆記，不是教材原文，也不是外部來源；只供理解教學重點，不得寫進學生版）」。這是註記進入任何 AI 管線的唯一入口；沒勾的註記不會被讀到。輸入快照記錄註記時比照教材：只記 id 與雜湊，不記內容（學習單產生器接上時遵守）。

## 註記的定位

- 存的是：段落 id、段落內 UTF-16 起訖位置、標記的原文（由伺服器從段落取出）、前後各 32 字的上下文，以及做註記時教材的 `revision`。
- 每次讀取重新定位：同段同位置文字相同 → `ok`；同段內找得到原文（有多處時取上下文最吻合、再取最近）→ `moved`；別的段落中有一處明顯最吻合 → `moved`；否則 `lost`（前端顯示「需要重新確認位置」，註記內容保留，教師可重新選取位置或刪除）。
- 前端選取若落在英文單字中間，會延伸到整個單字；跨段落的選取不能成為一則註記（請只選一段）。

## 刪除

`DELETE /api/materials/:id` 依序：

1. 刪除儲存空間中的原檔物件（以及它的預覽圖，見「知識庫：教材牆與預覽圖」；其他教材也是同一個檔案時預覽圖保留）。**刪不掉就整個不刪**，回 503「原檔沒有刪除成功…所以這份教材還沒有刪除」（預覽圖刪不掉時是「預覽圖沒有刪除成功…」）；儲存未設定而教材有原檔時同樣不刪。
2. 同一個交易內：刪除教材列（文字、段落）、註記、課例與學習單連結；用到它的 SOP 版本 `material_id` 設為 null、`material_deleted = true`，指向它段落的參照改名為 `m<id>-p3`（不會被誤認為下一份教材的段落），**從它逐字引用的片段**（分析與語言重點的 `evidence`）換成「［教材已刪除］」；這些課例的 AI 紀錄（`ai_interactions.output`、`lesson_sop_jobs.result`）中的逐字引文與教材名稱同樣移除。教師自己寫的分析、目標、任務都保留。
3. `material_deletions` 記一筆（教材 id、類型、大小、是否有原檔、註記數、時間；沒有標題、檔名、雜湊或內容），`usage_logs` 記 `material_deleted`。

教師自己的教材還是某個範本的範例時，這裡不刪（409，請刪除範本，原檔會跟著一起刪）；刪除範本時範例也走同一條刪除路徑。見「我的範本：刪除範本連同原檔」。

前端在頁面上確認，明說「永久刪除、無法復原」以及會一起刪掉什麼、哪些課例會顯示「教材已刪除」。

## 課例 SOP（Phase 2.1）的變更

- `PUT /api/lessons/:id/sop/material` 另收 `{ material_id: number }`（從教材庫選）與 `{ material_id: null }`（從課例移除，教材留在教材庫）；原本的 `{ title, source_note, text }`（貼上）照舊。空白 `text` 也是「從課例移除」，**不再**代表刪除。
- 貼上：課例目前的教材是貼上的、且沒有其他課例或學習單在用 → **就地修改**（revision + 1，舊文字不保留）；否則建立一份新的貼上教材，段落 id 接續原本的教材（文字沒變的段落沿用 id）。
- 換教材時，SOP 內容裡指向舊教材段落的參照改名為 `m<舊教材 id>-p3`，換回來時復原；前端把找不到的段落顯示為「原段落已變更」。
- 教材不再隨 SOP 版本保存：舊版本顯示的是教材**現在**的樣子（`material_version_no` 是當時的修訂次數）。這是硬刪除規則的必然結果。
- 沒有文字的教材（圖片、影音、網址、掃描的 PDF）可以是第 1 步的教材，但請 Manna 分析時回 400「這份教材沒有可以讀的文字…」。
- AI 紀錄：第 3 步課綱檢索的查詢字串改為只記教師自己的文字與教材的字數＋雜湊（`retrieval_logs.query_text`、`input_snapshot.retrieval.query`）；工作列多 `material_id`。

## 舊資料搬遷（`0009_materials.sql`）

- 每個「最新 SOP 版本仍有教材」的課例：把**最新**那一份 `lesson_sop_materials` 變成該教師的一份貼上教材（標題、來源說明、文字、段落與 id 計數照搬，`revision` = 原本的版本號），並連到該課例；所有指向該課例舊教材列的 SOP 版本改指向它。
- 較早的教材文字，以及教師已經移除教材的課例的文字：**不搬、隨舊表一起刪除**；曾經有教材的版本標 `material_deleted = true`。
- 已記錄的 `sop_objectives` 檢索查詢字串改寫為字數＋雜湊。
- `lesson_sop_materials` 刪除。

## usage_logs 新事件

| event | payload |
|---|---|
| `material_created` | `{ material_id, kind, origin, ext?, size_bytes?, text_status? }` |
| `material_deleted` | `{ material_id, kind, lessons }`（`lessons` 是受影響課例數） |

## 前端

- **`MaterialViewer`**（`web/src/components/material/MaterialViewer.tsx`）：一個元件兩種用法。
  - `mode="panel"`：課例 SOP 第 2–7 步的「對照教材」、備課庫教材的檢視，從左側滑出。
  - `mode="pane"`：與工作區並排的一欄，可拖曳或用方向鍵調整寬度（320–760px，記在瀏覽器）、收合成直立的「對照教材」、全螢幕。學習單產生器使用：
    ```tsx
    <div className="material-split">
      <MaterialViewer mode="pane" materialId={id} />
      <div className="material-split-main">…設定、結果或編修區…</div>
    </div>
    ```
  - Props：`materialId: number | null`、`mode`、`open?`／`onClose?`（panel）、`material?`（已有資料時免再請求）、`empty?`（沒有教材時顯示）、`defaultCollapsed?`（pane）。
  - 1100px 以下 pane 改為蓋在頁面上；760px 以下兩種都是全螢幕的教材頁，頂端固定「收起教材」。
  - 內容：印刷樣式、頁邊段落編號、PDF 頁碼分隔、註記（藍色底線＋藍色淡底小塊，絕不用金色）、選取文字出現「加註記」、提供給 AI、修改、隱藏／顯示隱藏的註記、刪除（頁內確認）、重新確認位置；圖片／音訊／影片以瀏覽器內建元件預覽，可下載原檔；網址教材顯示連結（新分頁開啟）。
- **備課庫 → 教材**：列出教材（類型、段數、頁數、大小、註記數、用在哪些課例）、上傳檔案／貼上文字／加入網址（每種都有隱私與著作權提醒）、改名、永久刪除（頁內確認）。
- **課例第 1 步**：貼上文字、從教材庫選、上傳檔案；有教材時可修改（貼上的）、換一份、從這個課例移除；教材被刪除時顯示「教材已刪除」。

---

# Phase 2.5：我的學習單版型

以下為**新增**；既有端點只多了選填欄位。Migration `0012_worksheet_templates.sql`。依據：`docs/WORKSHEET_TEMPLATE_PROBE.md` 與 `research/worksheet-template/probe.py`：v4 抽版型（兩次呼叫）與 gen2 照版型產生（每個區塊一次呼叫）的 prompt 與 schema，加上這裡寫的修改（`needs_external_material`、區塊上限 16 等）。

教師上傳一份自己做過的學習單，Manna 抽出它的設計，存成**版型**。教師檢查、修改後儲存。為新的一課做學習單時選這個版型，Manna 就照它產生；之後的編修、檢查與匯出都和一般學習單一樣。

## 資料

- `worksheet_templates`：`id`、`teacher_id`（只有擁有者能存取）、`title`（≤200）、`status`（`extracting`｜`ready`｜`failed`）、`error`（失敗原因，中文）、`source_material_id`（教材庫教材；刪除教材時變成 null）、`created_at`、`updated_at`。另有內部欄位 `extract_run`、`extract_started_at`，用來處理重試與伺服器重啟。
- `worksheet_template_versions`：只增不改，`(template_id, version_no)` 唯一，`content`（`WsTemplate`），`origin`（`ai`｜`teacher`）。版型目前的內容就是最新版本。
- **刪除版型是真的刪除**：版型與所有版本都刪掉。用過它的學習單保留內容，只是 `template` 變成 null。為它上傳的學習單（範例）沒有其他版型在用時一起真的刪除（見「我的範本：刪除範本連同原檔」）。
- `worksheets` 新增 `template_id`（刪除版型時變成 null）與 `template_version_no`。
- `prompt_versions` 新增 `worksheet_template_outline`、`worksheet_template_details`、`worksheet_template_section`。`scaffolds` 新增 `SC-WS-TPL-EXTRACT`、`SC-WS-TPL-GEN`。
- `worksheet_jobs.step` 新增 `template`（見「照版型產生」）。

## 型別

```ts
interface WsTemplate {
  worksheet_kind: string;                       // 例如「課文深度閱讀」（≤200）
  instruction_language: "英文" | "中文" | "中英並用";
  header_fields: string[];                      // 例如 ["Name","No","Class"]（≤20 個，每個 ≤100）
  shows_objectives: boolean;
  includes_passage_text: boolean;
  sections: WsTemplateSection[];                // 1–12 個
  generation_options: string[];                 // 只能用「產生設定」的固定標籤
  style_notes: string[];                        // 0–8 條，每條 ≤200
}
interface WsTemplateSection {
  id: string;                                   // "s1"、"s2"…，編修時不變；新增的區塊可送空字串，由伺服器配 id
  title_pattern: string;                        // 1–200 字，可有 [n]、[主題] 這類佔位符
  purpose: string;                              // ≤300
  block_kind: "warmup" | "activity" | "exit_ticket";
  per_paragraph: boolean;                       // 新教材的每一段各做一次
  student_tasks: string[];                      // 只能用「學生任務」的固定標籤
  answer_formats: string[];                     // 只能用「作答方式」的固定標籤
  item_count: number;                           // 1–12
  text_scope: string;                           // ≤100，例如「逐段」「全文」「課文外」
  scaffolds: string[];                          // 只能用規格 §3 的鷹架種類（語言、教學、程序三類）
  supports?: WsTemplateSupport[];               // 0–6 個，原學習單在這一區印出的提示框，依原本順序；沒有時不出現（見「提示框」）
  instruction_style: string;                    // ≤200
  item_patterns: string[];                      // 0–6 條，每條 ≤300；只有有題號的題目，句型框架不算
  needs_external_material: boolean;             // 需要教師另外提供的影片、錄音、圖片或另一篇文字
}
interface WsTemplateSupport {
  heading: string;                              // 1–60 字，原學習單上的標題，逐字（例如 "Directions"、"Sentence Frame for Sharing"）
  role: "directions" | "sentence_frame" | "sharing_frame" | "word_bank" | "hints" | "guiding_questions" | "example";
  pattern: string;                              // ≤300，內容的通用樣式（佔位符與空格），可以是空字串
}
interface WsTemplateSummary {
  id: number; title: string; status: "extracting" | "ready" | "failed"; section_count: number;
  source_material: { id: number; title: string } | null;
  version_no: number | null;                    // 還沒有任何版本（讀取中或失敗）時是 null
  updated_at: string;
}
interface WsTemplateDetail extends WsTemplateSummary { error: string | null; content: WsTemplate | null; }
```

既有型別的變更：

```ts
interface WsBlock { /* …既有欄位… */ template_section_id: string | null; }   // 照版型產生的區塊：來自哪個 section
interface WorksheetMaterial { source: { kind: "pasted" | "lesson_sop" | "library"; /* … */ } }
interface WorksheetState { /* … */ template: { id: number; title: string; version_no: number } | null; }
interface WsJob { step: "tasks" | "warmup" | "scaffolds" | "exit_ticket" | "template"; /* … */ }
```

## 端點（皆需登入；只能存取自己的版型，其他人的 id 回 404）

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/worksheet-templates` | – | `{ templates: WsTemplateSummary[] }`，依建立時間新到舊 |
| POST | `/api/worksheet-templates` | `{ material_id: number, title?: string }` | 202 `WsTemplateDetail`（`status: "extracting"`） |
| GET | `/api/worksheet-templates/:id` | – | `WsTemplateDetail`（輪詢用） |
| PUT | `/api/worksheet-templates/:id` | `{ title?: string, content?: WsTemplate }` | `WsTemplateDetail`。有 `content` 且與最新版本不同才新增一個版本（`origin = "teacher"`）。讀取中 → 409；`failed` 時送 `content` → 409（只能改名稱或重試） |
| POST | `/api/worksheet-templates/:id/retry` | – | 202 `WsTemplateDetail`。只有 `failed` 可以（其他 → 409）；原本的教材已刪除 → 400 |
| DELETE | `/api/worksheet-templates/:id` | – | `{ ok: true }`（真的刪除；範例的原檔刪不掉時 503，什麼都不刪。見「我的範本：刪除範本連同原檔」） |

**驗證（400，訊息為中文）**

- `POST`：`material_id` 必須是自己教材庫裡、已擷取到文字（`text_status = ready`）的教材。不是自己的或不存在 → 「找不到這份教材。」；圖片、影音、網址 → 「這份教材沒有文字可以讀…」；掃描檔或讀不出文字 → 「這份檔案讀不到文字（可能是掃描檔）…」；文字超過 30000 字 → 400（不像是一份學習單）。`title` 選填（≤200）；沒給時是教材標題＋「版型」。
- `PUT`：`title` 不可空白、≤200。`content` 依上面型別的規則檢查（提示框：最多 6 個、標題不可空白且 ≤60、樣式 ≤300、`role` 只能是上面七個值）：標籤不在固定集合內（包含「課綱議題」「SDGs」「SEL」「探究」「高層次思考」「專題任務」「合作活動」）、題數不在 1–12、區塊數不在 1–12、id 重複、長度超過 → 400，訊息會指出是第幾個區塊。重複的標籤會被去掉，順序照送來的。`block_kind` 不是三個值之一 → 400（「請求內容格式不正確…」）。

## 抽版型（背景工作）

1. **呼叫 A（大綱）** `worksheet_template_outline`：結構化大綱 `{ title_pattern, per_paragraph }[]`，加上表頭資訊（`worksheet_kind`、`instruction_language`、`header_fields`、`shows_objectives`、`includes_passage_text`）。規則照 probe v4：區塊內的小步驟不另列；逐段重複的區塊只列一次；學生版與教師版只算一次；只用佔位符；不寫人名、學校、出版社或班級。第 2 版：區塊裡有標題的提示框（Directions、Sentence Frame for Sharing…）屬於那個區塊，不另列。`max_tokens` 2000。
2. **呼叫 B（細節）** `worksheet_template_details`：`sections` 的 `minItems = maxItems =` 大綱項數，另有 `generation_options` 與 `style_notes`。比 probe 多了 `needs_external_material`：需要學習單本身課文以外的影片、錄音、圖片或另一篇文章時為 true。第 3 版多了 `supports`（在 `item_patterns` 之前寫）：見「提示框」。`max_tokens` 5000。
3. schema 的欄位照撰寫順序（閘道依 schema 順序輸出；字母順序曾造成輸出空白失控）。
4. 任一次呼叫寫到 `max_tokens`、輸出讀不懂，或細節數量和大綱不同時，**重試一次**，temperature +0.4（0.2 → 0.6；閘道會快取相同的請求）。還是失敗，或逾時、供應商錯誤：`status = "failed"`，`error` 寫中文原因，教師可以按「重試」（`POST …/retry`）。
5. 伺服器整理結果：配 section id（`s1`…）；`item_count` 限制在 1–12；拿掉固定集合以外的標籤；最多 12 個區塊；`text_scope` 明確指定某一段或某幾段（例如「第 2 段」「Paragraphs 2–3」「P4」）時，`per_paragraph` 改成 false。各文字欄位截到上限長度。提示框：`role` 不在七個值內的拿掉，沒有標題的用 role 的預設標題（Directions、Sentence Frames、Sentence Frame for Sharing、Word Bank、Hints、Guiding Questions、Example），`scaffolds` 補上提示框對應的鷹架種類。`item_patterns` 裡帶標籤的提示框（「Directions: …」「Sentence Frame for Sharing: …」，標籤當標題）與句型框架（招呼語開頭、有空格的分享句型，或用學生口吻、只有空格的句型）移出題型樣式，成為分享句型（或句型框架）提示框的 `pattern`（沒有這個提示框時加一個；原本一個提示框都沒有時，先加 `scaffolds` 第一個種類的提示框，因為舊做法會印它）；題型樣式原本一題一條（條數＝`item_count`）時，`item_count` 跟著減少。
6. 存成第 1 版（`origin = "ai"`），`status = "ready"`。
7. 每次呼叫一列 `ai_interactions`（`module` = prompt key）。輸入快照裡的學習單文字只留字數與雜湊。
8. 伺服器重啟會中斷工作：`extracting` 超過 `6 × AI_TIMEOUT_SECS + 60` 秒的版型，下次讀取時改成 `failed`（「這次讀取中斷了…請按「重試」」）。

一份學習單的抽取大約 15–25 秒（兩次呼叫）。前端每 3 秒輪詢 `GET …/:id`。

## 用版型建立學習單

`POST /api/worksheets` 新增兩個選填欄位：

- `template_id`：必須是自己的、`status = "ready"` 的版型，否則 400。學習單記下 `template_id` 與版型目前的版本號。
- `material_id`：自己教材庫裡、有擷取文字的教材，否則 400。它就是這份學習單的教材，優先於貼上的 `material`（這時貼上的文字不會存進教材庫）。有 `lesson_id` 且課例 SOP 有教材時，仍以課例的教材為準。教材來源記為 `source.kind = "library"` 與它的 `material_id`，並以 `material_links` 連到這份學習單（教材庫顯示的使用次數會加一）。

**設定**：有版型時，`settings`（以及 `suggested`）以版型預填：`student_tasks` 與 `answer_formats` 是各區塊的聯集；`generation` 是版型的 `generation_options`；`shows_objectives` 為 true 時加「顯示學習目標」，false 時拿掉。素材類型、版本與時間沿用原本的建議。教師之後仍可修改。

`PUT /api/worksheets/:id` 新增 `template_id?: number | null`：

- 給數字：同樣的檢查（不是自己的或不是 `ready` → 400），記下版型目前的版本號，並依版型重新算 `settings`（同一個請求也送了 `settings` 時，以送來的為底再套用版型）。
- `null`：不再使用版型，`settings` 不變。
- 產生中送 `template_id` → 409。

## 照版型產生

`POST /api/worksheets/:id/generate`（既有端點）遇到有版型的學習單時，不跑 tasks／warmup／scaffolds／exit_ticket 四步，改成一個 `step = "template"` 的工作：

- 用版型的**最新版本**，並把 `template_version_no` 更新成這個版本。單一區塊重新產生則用學習單記下的版本，所以同一份學習單的各區塊來自同一個版本。
- **展開**：每個 section 一個區塊；`per_paragraph` 的 section 依新教材每一段各一個區塊（`paragraph_ids = [那一段]`，標題寫出段號：`[n]` 換成段號，標題裡沒有段號時前面加 `Paragraph n:`，指示語言為中文時加「第 n 段：」）。**總數上限 16 個區塊**：依序展開，展開某個逐段 section 會超過 16 時，那個 section 改成對全文只做一次，並在 `content.notices` 說明原因。
- **每個區塊一次呼叫**（`worksheet_template_section`，`max_tokens` 2500），最多同時 3 個。prompt 照 probe gen2，內容有：版型的整體資訊（`worksheet_kind`、`instruction_language`、`style_notes`）、目標 section、要涵蓋的段落（逐段時）、已完成區塊的標題、加了段落標記（`[第 n 段]`）的教材。輸出 `{ title, instructions, scaffold, items[{ answer_format, prompt, options, answer }] }`。
- **檢查**：作答方式只能用固定標籤；選擇題至少 2 個不重複選項，答案必須是其中之一（模型只寫字母 A、B… 時換成該選項）；配對題至少 2 個選項；題幹不可直接含答案；至少要有 1 題。題目超過 `item_count` 時由程式只留前面幾題（不另外花一次呼叫）。答案是教材裡至少 5 個字的連續原文時，記為該題的 `evidence`（教師參考版會印出依據）。指示語可以是中文（由版型的 `instruction_language` 決定），所以這裡不做「全英文」檢查。
- 輸出失控、讀不懂或沒通過檢查時**重試一次**（temperature +0.4，附上沒通過的項目）。還是失敗的區塊保留下來（標題、`student_tasks`），沒有題目，`message` 說明原因；其他區塊照常產生。工作的 `message` 會寫有幾個區塊沒有成功。
- **需要教師提供素材的 section**（`needs_external_material = true`）：prompt 禁止寫出或編造任何素材（名稱、網址、作者、內容），題目只留空位；檢查會拿掉含網址或影片網站的欄位與題目。區塊的 `message` 一律是「這一區需要你提供素材（例如影片或另一篇文章），Manna 不會自己編造。」，`paragraph_ids` 是空的。呼叫失敗時程式放一題空位題（「放上你提供的素材後，在這裡寫題目。」）。
- **對應到 `WsBlock`**：`block_kind` → `kind`；`title`、`instructions` 照輸出；`student_tasks` 照 section；題目 `origin = "ai"`、`levels` = 這次產生的所有版本；`answer` 只在教師參考版；每個提示框成為一個鷹架，依版型的順序，`heading` 是原學習單的標題（見「提示框」）；section 沒有提示框（提示框出現以前讀的版型）時，`scaffold` 文字成為一個鷹架，種類是 section 的第一個 `scaffolds`（沒有時是語言鷹架「引導問題」；程序鷹架的文字依行拆成 `steps`），沒有 `heading`；文字是空的就不加；`teacher_notes` = section 的 `purpose`；`template_section_id` = section 的 `id`。不另外加程式自動產生的程序鷹架。
- **一份學習單一個用語**：各區塊是分開同時寫的，看不到彼此，所以同一個概念的不同說法（目前是 informative／expository／informational 加 writing、text、essay、paragraph… 這一組）由程式統一：整份學習單混用時，用版型裡出現的那個詞，版型沒有就用教材的，都沒有就用學習單裡用得多的（同樣多時用先出現的），其他的換掉（保留字首大寫；不改教材的引文 `evidence`）。單一區塊重新產生時，跟著其他區塊用的詞。`worksheet_template_section` 第 5 版也寫明這條規則。

### 提示框

原學習單在一個區塊裡常有幾個有自己標題的部分，例如「Directions」（祈使句的作答步驟）與「Sentence Frame for Sharing」（口頭分享用、以 Hi! 開頭的句型）。它們是鷹架，不是題目，也各有用途。

- **抽版型**：每個這樣的部分是 section 的一個 `supports`（`heading` 逐字照抄原學習單、`role`、`pattern`），依原本順序。七種 `role` 與印出的鷹架種類：`directions` → 程序鷹架「清楚的分步指示及完成順序」；`sentence_frame`、`sharing_frame` → 語言鷹架「句型框架」；`word_bank` → 「字詞表」；`hints` → 「提示與追問」；`guiding_questions` → 「引導問題」；`example` → 「作答示例」。句型框架（包括 Hi! 開頭的分享句型）一律是提示框，不是題型樣式，也不算進 `item_count`（伺服器另外檢查，見「抽版型」第 5 點）。教師的編輯頁在「看 Manna 讀到的細節」列出提示框；改版型時原樣保留。
- **產生**：section 有提示框時，輸出沒有 `scaffold`，改成 `supports`：`minItems = maxItems =` 提示框數，每項 `{ heading（只能是這個 section 的標題之一）, lines[] }`，寫在 `items` 之前。輸出依標題對回版型的提示框（標題對不上時依位置），每個提示框成為區塊的一個鷹架，順序與標題照版型。`directions` 是一行一步的祈使句（不得有空格）；`sentence_frame` 只寫有空格的句型；`sharing_frame` 可以用 Hi! 開頭，但永遠不編題號。
- **檢查**（`checks::evaluate_template_section`，`supports_wellformed`）：句型框架的提示框裡寫了作答步驟 → 步驟移到 Directions 提示框（版型沒有就在句型框架前面加一個標題為「Directions」的）；Directions 裡寫了有空格的句型 → 移到句型框架的提示框；「Use this frame:」開頭拿掉；招呼語開頭的分享句型寫在其他提示框（例如 Sentence Frames、Hints）時，移到分享句型的提示框；版型有、輸出卻空著的提示框會被指出（修正後仍空著就不印）。題目是招呼語開頭的分享句型（含包在題目裡的）→ `items_wellformed` 不通過，修正後仍是 → 移出題號，放進分享句型的提示框（它已經有句型時不重複；版型沒有就在最後加一個標題為「Sentence Frame for Sharing」的）；只有句型框架、用學生口吻的題目照「句型框架不是題目」的做法移到句型框架的提示框。都照「檢查 → 修正呼叫一次 → 程式處理」的流程。
- **舊版型**（提示框出現以前讀的，沒有 `supports`）：產生時（整份與單一區塊重新產生），伺服器先在記憶體裡用「抽版型」第 5 點同樣的規則整理：題型樣式裡帶標籤的提示框（「Directions: …」「Sentence Frame for Sharing: …」「Word Bank: …」等，標籤照原文當標題）、招呼語開頭的分享句型、只有空格的句型框架，移成這個 section 的提示框（`WsTemplate::for_generation`），之後照有提示框的方式產生。存著的版型不改。題型樣式裡沒有這些時，照舊產生一個 `scaffold`，同樣的檢查也適用：這個鷹架是句型框架、裡面寫了步驟時拆成「Directions」與「Sentence frames」兩個方框；分享句型寫成題目，或寫在這個鷹架裡（不論它是哪一種），都移到「Sentence Frame for Sharing」方框（寫在鷹架裡的不另外花一次修正呼叫，因為舊版型沒有別的地方可寫）。舊版型沒有讀到、題型樣式裡也沒有的提示框（例如原學習單的 Directions 被讀成指示語寫法），要重新讀一次範例學習單才會有。
- **印出**（預覽、複製、DOCX、PDF 共用 `assemble::project`）：每個鷹架印在 `heading`（沒有時是種類的英文標題）下面；有標題的程序鷹架印成一個方框，不併進區塊開頭的編號步驟。編輯器仍以種類顯示鷹架。
- `content.chosen`：照一般流程決定設定，`reasons` 第一條是「依版型：<版型名稱>」，`item_budget` 是各區塊題數的總和。`show_objectives` 依「顯示學習目標」。
- 既有的檢查照常適用：詞彙級別檢查（`POST …/checks/vocabulary`）、編修時的標籤驗證、匯出。
- **單一區塊重新產生**（`POST …/blocks/:block_id/regenerate`）：有 `template_section_id` 的區塊重跑那個 section 的呼叫（逐段的區塊用原本的段落），工作是 `step = "template"`、`block_id` 為該區塊；失敗時區塊維持原樣。可以照常 `undo`。教師自己新增、沒有 `template_section_id` 的區塊，用原本的流程重新產生。
- 每次呼叫一列 `ai_interactions`（`module = "worksheet_template_section"`，鷹架 `SC-WS-TPL-GEN`），教材與已完成的標題只記字數與雜湊。

## 測試用假模型（`AI_PROVIDER=fake`）

抽版型時，學習單文字中每一行 `## <標題>` 是一個大綱項目：標題含 `[n]` → `per_paragraph`；含 video 或「影片」→ `needs_external_material`。

| 標記 | 效果 |
|---|---|
| `FAKE_TPL_BAD_LABEL` | 細節用了固定集合以外的標籤，`item_count` 是 40 → 伺服器拿掉標籤、限制成 12 |
| `FAKE_BLOCK` | 寫在學習單：兩次呼叫的輸出都讀不懂 → `failed`。寫在某個 section 的標題：只有那個區塊失敗 |
| `FAKE_RUNAWAY` | 每次呼叫的第 1 次都寫到 `max_tokens`，重試成功 |

照版型產生時，假模型每個區塊給 `item_count + 1` 題（由伺服器截掉多的），題目根據目標段落的第一句。既有的 `FAKE_ERROR`、`FAKE_TIMEOUT`、`FAKE_SLOW` 照舊。

提示框：抽版型時，`## ` 區塊裡標題為 Directions、Sentence Frame(s) for Sharing、Sentence Frames、Sentence Starters、Word Bank、Hints、Guiding Questions 或 Example 的「標題: 內容」行，是那個區塊的提示框；有提示框的區塊照版型產生時，每個提示框依 role 給內容。

| 標記 | 效果 |
|---|---|
| `FAKE_TPL_FRAME_AS_ITEM` | 抽版型：沒有提示框，分享句型是四條題型樣式的最後一條（`item_count` 4），像提示框出現以前那樣 → 伺服器移回提示框，`item_count` 3 |
| `FAKE_TPL_BAD_SHAPES` | 照版型產生的第 1 次呼叫：第一題是問看法的是非題，最後兩題是分享句型（一題包在「Write … using the frame:」裡），步驟和「Use this frame:」寫在第一個句型框架提示框（或 `scaffold`），Directions 與分享句型提示框空著 → 修正呼叫寫好 |
| `FAKE_TPL_BAD_SHAPES_ALWAYS` | 兩次呼叫都這樣 → 伺服器拆開、移出題號、改成直述句 |
| `FAKE_TPL_TERMS` | 照版型產生：單數段的指示語寫 expository writing，雙數段寫 informative writing → 伺服器統一成一個 |
| `FAKE_WS_FRAME_STEPS`／`FAKE_WS_FRAME_STEPS_ALWAYS` | 完整學習單的鷹架步驟：句型框架裡有兩句步驟與「Use this frame:」（第 1 次／兩次都是）→ 修正，或步驟移到分步指示 |

## usage_logs 新事件

`worksheet_template_created`、`worksheet_template_extracted`（`status`、`sections`）、`worksheet_template_saved`、`worksheet_template_retried`、`worksheet_template_deleted`。`worksheet_generated` 有版型時多了 `template_id`、`template_version_no`、`failed_blocks`。

## 試用真模型

prompt key：`worksheet_template_outline`、`worksheet_template_details`、`worksheet_template_section`（可在管理頁改版本）。程式進入點：`worksheet::template::extract::extract`（兩次呼叫，回傳 `WsTemplate`）與 `worksheet::ai::start_generation`（有 `template_id` 的學習單會走 `worksheet::ai::template`）。

---

# Phase 2.6：備課對話

以下為**新增**；既有端點只多了一個回應欄位（`WorksheetSummary.prep_session_id`）。Migration `0013_prep_sessions.sql`。

老師帶著這一課的課文（上傳、從教材庫選或貼上；沒有課文時也可以只寫主題），按一個動作（做學習單／規劃教學活動／設計評量／找教學資源），或直接問 AI。每一個請求是對話裡的一**輪**（turn），結果依序顯示；之後可以接著做下一步，或在底部輸入框用一句話修改剛做好的學習單。互動方式參考築學網。做出來的學習單就是一般的學習單：編輯器、檢查、匯出、版型、班級／單元／SOP 全部照舊。

**Phase 2.7 的擴充**（Migration `0015_prep_bloom.sql`，以下各節已併入）：新的輪次 `comprehension`（閱讀理解題）、`tiered`（分層練習）、`kit`（教學套件＝課堂活動＋分層學習單）；對話的**出題選項** `options`（帶進對話做的每一份學習單）；活動規劃的每一步標上 Bloom 層次（`prep_activity` 第 2 版）；教學套件下載成一份 DOCX；`next` 改成 `comprehension, tiered, activity, kit, worksheet`。

**課堂活動的學習目標分成內容目標與語言目標、中英文並陳**（Migration `0019_prep_activity_goals.sql`，以下各節已併入）：`prep_activity` 第 3 版依 SIOP 分成內容目標（`content`）與語言目標（`language`），各 1–3 個，每個目標有中文（`zh`，給教師）與英文（`en`，給學生）；`ActivityPlan.goals` 每一項多了 `kind`、`zh`、`en`，取代 `text`。已存的舊規劃不改寫：它們的目標仍是 `{ text, curriculum_codes }`，照舊顯示成一張清單。

**課堂活動依 SIOP 設計規則**（Migration `0023_prep_activity_siop.sql`，以下各節已併入；規則見 `docs/SIOP_DESIGN_RULES.md` 第一、六節）：`prep_activity` 第 4 版。目標各 1–2 個（R1），先寫內容目標、再由它推出語言目標；語言目標以學習策略優先（R2），每個都寫出它支持哪個內容目標與「學生要用［語言目標］，才能［內容目標］」的關係，說不出關係或用 learn／know／understand 這類動詞的目標不採用；教師給的內容目標、語言目標一字不改。`ActivityPlan` 多了關鍵字彙、教材與教具、高層次問題、背景連結（A1）、複習、評量，每一步多了分組與分工（A2）、語言技能（A3）、教師巡視與鷹架（A6）。舊規劃（第 1–3 版）沒有這些欄位，照舊顯示。

**SIOP 自我檢核**（Migration `0024_siop_selfcheck.sql`，見下面「SIOP 自我檢核」一節）：`/create` 的產出做好後自動檢核，`PrepTurn` 多了 `siop_check`，新增 `GET`／`POST /api/prep-sessions/:id/turns/:turn_id/siop-check`。

## 資料

- `prep_sessions`：`id`、`teacher_id`（只有擁有者能存取，其他人的 id 回 404）、`title`（≤200；預設為教材標題，沒有教材時為主題，再沒有就是第一個請求的前 40 字）、`material_id`（教材庫教材；教材刪除時變成 null）、`topic`（≤200，沒有教材時用）、`class_id`、`template_id`（刪除時都變成 null）、`options`（JSONB，出題選項，預設 `{}`）、`created_at`、`updated_at`。
- `prep_turns`：`id`、`session_id`（刪除對話時一起刪除）、`seq`（對話內從 1 起編號）、`kind`、`request`（≤2000：老師打的字，或按鈕的名稱）、`status`（`running`｜`done`｜`failed`）、`error`（失敗原因，中文）、`worksheet_id`（這一輪做出的學習單）、`result`（JSONB）、`interaction_id`（產生結果的 `ai_interactions`）、`created_at`、`finished_at`。同一個對話同時只會有一輪在跑（部分唯一索引）。
- **刪除備課＝整份隱藏**（migration 0033）：對話和在對話裡做的每一份學習單在同一個交易裡標成已刪除（`prep_sessions.deleted_at`、`worksheets.deleted_at`），教師從此看不到（都回 404、不在任何清單裡），資料連同所有輪次保留給研究紀錄。教師附上的圖片（`prep_images`）仍真的刪掉：先刪儲存空間裡的檔案（失敗就整個不刪並告訴教師），再刪它們的列。學習單保存的預覽圖也刪掉。記 `prep_session_deleted`。0033 以前刪掉的對話是真的刪除，它們的學習單留在 /worksheets（`prep_session_id` 為 null）。
- `worksheets` 新增 `prep_session_id`（做出這份學習單的對話）。`worksheet_versions.cause` 新增 `revise`。
- `prompt_versions` 新增 `prep_activity`（第 1、2、3、4 版）、`prep_ask`、`prep_revise`、`siop_selfcheck`。`scaffolds` 新增 `SC-PREP-ACTIVITY`（第 4 版的 prompt 起用 scaffold 第 2 版）、`SC-PREP-ASK`、`SC-PREP-REVISE`、`SC-SIOP-SELFCHECK`。
- `siop_checks`：每一輪的 SIOP 自我檢核，每檢核一次一列（見「SIOP 自我檢核」）；刪除對話時一起刪除。
- `prep_turns.kind` 多了 `comprehension`、`tiered`、`kit`（`0015_prep_bloom.sql`）。
- 每次模型呼叫一列 `ai_interactions`（`module` = prompt key）；輸入快照裡的教材（以及由教材衍生的學習單內容、先前的對話摘要）只記字數與雜湊。檢索記在 `retrieval_logs`（查詢文字含教材時同樣只記字數與雜湊）。

## 型別

```ts
type PrepKind = "worksheet" | "assessment" | "activity" | "resources" | "ask" | "revise"
              | "comprehension" | "tiered" | "kit";
interface PrepSessionOptions {     // 出題選項；沒有設定的鍵不會出現（都沒設定時是 {}）
  bloom?: Bloom[];                 // 依 Bloom 順序、去重；沒有＝六個層次
  question_count?: number;         // 3–30；沒有＝該產出的預設（閱讀理解題 8、差異化學習單 6）
  bloom_counts?: Partial<Record<Bloom, number>>;   // 各 0–12、合計 3–30；有值時取代 bloom 與 question_count
  answer_formats?: string[];       // 固定標籤（作答方式），依標籤順序；沒有＝每個層次適合的
  content_objective?: string;      // 內容目標（≤300，一行一項）；舊資料的 objective 讀成這一欄（見「SIOP 目標」）
  language_objective?: string;     // 語言目標（≤300，一行一項）
  paragraph_ids?: string[];        // 只選要用的段落（教材庫那份的段落 id）；沒有＝全部（見「長教材」）
}
interface PrepSessionSummary {
  id: number; title: string;
  material: { id: number; title: string } | null; topic: string | null;
  class: { id: number; name: string } | null;
  template: { id: number; title: string } | null;
  turn_count: number; last_kind: PrepKind | null; updated_at: string; created_at: string;
  preview_worksheet: { id: number; version_no: number } | null;   // 卡片預覽：見「我的作品：卡片預覽」
  preview_text: string | null;                                    // 同上（≤200 字）
}
interface PrepTurn {
  id: number; seq: number; kind: PrepKind; request: string;
  status: "running" | "done" | "failed"; error: string | null;
  worksheet: { id: number; title: string; version_no: number; running: boolean } | null;   // 學習單已刪除時為 null
  result: ActivityPlan | Resources | { answer: string; follow_ups: string[] }
        | { reply: string; changed_block_ids: string[]; version_no: number | null } | null;
  interaction_id: number | null;   // 有幫助／不合用 → POST /api/ai/:interaction_id/decision { decision: "skip", thumbs: "up"|"down" }
  created_at: string; finished_at: string | null;
  siop_check?: SiopCheck | null;   // worksheet／comprehension／tiered／activity／kit 才有這一欄：最新一次的 SIOP 自我檢核；
                                   // 還沒有（這一輪還在跑、失敗了，或是在檢核功能之前做的）是 null。其他種類沒有這一欄
}
interface PrepSession extends PrepSessionSummary {
  turns: PrepTurn[];               // 依 seq
  running: boolean;
  next: PrepKind[];                // 建議的下一步：[comprehension, tiered, activity, kit, worksheet] 中這個對話還沒有 done 的，
                                   // 依這個順序；只在有教材時出現（沒有教材時是 []）。assessment、resources 仍是有效的 kind，但不再建議
  options: PrepSessionOptions;     // 只在 PrepSession（列表的 PrepSessionSummary 沒有）
  material_paragraphs: number | null;  // 教材有幾段（沒有教材是 null）
  material_selected: number | null;    // 選的段落還在教材裡的有幾段；全部都用是 null
}
type ActivityGoal =
  // prep_activity 第 3 版起（0019_prep_activity_goals.sql）：SIOP 的兩組，中英文並陳
  | { kind: "content" | "language";   // 內容目標｜語言目標
      zh: string;                     // ≤150，給教師的繁體中文，一句（「學生能……」）；教師給的英文目標沒有中文時可為 ""
      en: string;                     // ≤200，給學生的英文，一句（"Students will be able to …"）；可省略（""）
      curriculum_codes: string[];     // ≤3，只會是這次提供的候選代碼
      // 第 4 版起（0023_prep_activity_siop.sql），只有語言目標有前三項；沒有的鍵不出現
      category?: "strategy" | "function" | "structure" | "vocabulary";  // 學習策略｜語言功能｜句型文法｜學術字彙
      supports?: number;              // 支持第幾個內容目標（從 1 起，依 goals 裡內容目標的順序）
      relation?: string;              // ≤200，「學生要用［語言目標］，才能［內容目標］」
      from_teacher?: true }           // 教師給的目標，一字不改（≤300）
  // 第 1、2 版做的規劃（資料庫裡原樣保留）：一行中文、沒有 kind
  | { text: string; curriculum_codes: string[] };
interface ActivityPlan {
  title: string;                                                    // ≤100
  goals: ActivityGoal[];             // 第 4 版：先內容目標、再語言目標，各 1–2 個（重試後仍缺一組時，只有另一組）；
                                     // 第 3 版各 1–3 個；舊規劃：1–4 個 { text }
  // 以下到 assessment（除了 steps）是第 4 版（SIOP A6）新增；舊規劃讀出來是 [] 或 ""（API 原樣回傳資料庫的 JSON，舊規劃沒有這些鍵）
  key_vocabulary: Array<{ word: string; zh: string }>;              // ≤8，關鍵字彙：word ≤40（英文，不重複），zh ≤80
  materials: string[];                                              // ≤5、每項 ≤60，教材與教具
  higher_order_questions: Array<{ question: string; bloom: "analyze" | "evaluate" | "create" }>;  // ≤4，question ≤200（英文）
  background: string;                                               // ≤200，背景連結：引起動機要連結的學生經驗或舊知（A1）
  steps: Array<{
    phase: "引起動機" | "發展活動" | "統整與評量";
    title: string;                                                  // ≤60
    minutes: number;                                                // 1–50
    idea: string;                                                   // ≤300，教學思路
    options: Array<{ title: string; idea: string; script: string[]; minutes: number }>;  // 0–3，只有引起動機；script 每行 ≤200、≤4 行
    script: string[];                                               // ≤6 行、每行 ≤200，英文課堂用語
    stuck_tip: string;                                              // ≤200，卡住了怎麼辦；可為 ""
    bloom: Bloom[];                                                 // 這一步練到的 Bloom 層次，≤3，依 Bloom 順序（第 1 版 prompt 做的規劃讀出來是 []）
    // 第 4 版起（舊規劃是 "" 或 []）
    grouping: "全班" | "兩人一組" | "小組" | "個人" | "";            // 分組方式（A2）
    roles: Array<{ role: string; task: string }>;                   // 兩人一組 2 項（兩人各做什麼）；小組 ≤6 項（成員角色與責任）；全班、個人是 []。role ≤20、task ≤100
    monitoring: string;                                             // ≤150，教師巡視；可為 ""
    skills: Array<"聽" | "說" | "讀" | "寫">;                       // 語言技能（A3），依這個順序、不重複
    scaffolding: string;                                            // ≤150，鷹架方式；可為 ""
  }>;                                                               // 3–6
  review: string;                                                   // ≤200，複習；可為 ""
  assessment: string;                                               // ≤200，評量；可為 ""
  curriculum: Array<{ code: string; text: string }>;                // 實際引用的條目，條文由知識庫帶出
}
interface Resources {
  curriculum: Array<{ code: string; text: string; school_track: string | null; similarity: number | null }>;
  materials: Array<{ id: number; title: string }>;
  note: string;
}
interface WorksheetSummary { /* …既有欄位… */ prep_session_id: number | null; }   // 「回到備課對話」
```

課綱的 `code` 是知識庫的 `item_code`（例如 `108-G-3-V-7`），`text` 是條文內容。

## 端點（皆需登入；只能存取自己的對話，其他人的 id 回 404）

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/prep-sessions` | – | `{ sessions: PrepSessionSummary[] }`，依 `updated_at` 新到舊 |
| POST | `/api/prep-sessions` | `{ kind, request?, material_id?, material?: { title?, text }, topic?, class_id?, template_id?, options?: PrepSessionOptions }` | 201 `PrepSession`（第一輪已開始） |
| GET | `/api/prep-sessions/:id` | – | `PrepSession`（輪詢用） |
| PATCH | `/api/prep-sessions/:id` | `{ title?, class_id?: number \| null, template_id?: number \| null, options?: PrepSessionOptions \| null }` | `PrepSession`（之後的輪次才會用到） |
| DELETE | `/api/prep-sessions/:id` | – | `{ ok: true, worksheets_deleted }`：對話和它的學習單一起刪除（隱藏，資料保留；見上） |
| POST | `/api/prep-sessions/:id/turns` | `{ kind?, request }` | 202 `PrepSession` |
| PUT | `/api/prep-sessions/:id/material-selection` | `{ paragraph_ids: string[] \| null }` | 200 `PrepSession`。**只選要用的段落**（見「長教材」） |
| GET | `/api/prep-sessions/:id/turns/:turn_id/kit?format=docx` | – | 教學套件的 DOCX（見 `kit`） |
| GET | `/api/prep-sessions/:id/turns/:turn_id/siop-check` | – | 這一輪最新的 `SiopCheck`（檢核中時輪詢）；還沒有 → 404（見「SIOP 自我檢核」） |
| POST | `/api/prep-sessions/:id/turns/:turn_id/siop-check` | `{}` | 202 `SiopCheck`（新的一次，`running`）：再試一次或重新檢核 |

**建立對話**

- `kind` 不可以是 `revise`（400）。`request` 選填（≤2000）；沒有時記成按鈕名稱（做學習單、設計評量、規劃教學活動、找教學資源）。
- 教材：`material_id` 必須是自己教材庫裡、已擷取到文字的教材（不是自己的 → 400「找不到這份教材。」；讀不到文字 → 400「這份檔案讀不到文字（可能是掃描檔）…」）。`material`（貼上的文字）先存進教材庫（和建立學習單時一樣），再當作 `material_id`；兩者都給時以 `material_id` 為準。
- `class_id` 不是自己的 → 404；`template_id` 必須是自己的、`ready` 的版型 → 否則 400。
- **需要什麼（400，訊息為中文）**：`worksheet`、`assessment`、`comprehension`、`tiered`、`kit`、`activity` 需要教材；`resources` 需要教材或 `topic`；`ask` 需要 `request`。
- **出題選項 `options`**：用學習單設定的同一套規則檢查（400，訊息同「Phase 2.7」的驗證：「題數需為 3 到 30 題；也可以不填。」「每個 Bloom 層次的題數需為 0 到 12 題，合計 3 到 30 題。」「學習目標最多 300 字。」、作答方式不是固定標籤、Bloom id 不在表內），任何一個不合格就整個請求 400，什麼都不建立。各鍵可以省略或送 `null`（＝沒有設定）；`objective` 空字串＝沒有。存起來的是整理過的值（層次依 Bloom 順序、作答方式依標籤順序），`PrepSession.options` 原樣回傳。
- PATCH 的 `options` **整份取代**（`null` 或 `{}` 清空）；沒給 `options` 就不變。之後的輪次才會用到。

**新增一輪**

- 沒有 `kind`（底部輸入框）：對話裡有做出學習單的輪次（`worksheet`、`assessment`、`comprehension`、`tiered`、`kit`）→ `revise`，否則 → `ask`。`ask`、`revise` 需要 `request`；其他 kind 沒有 `request` 時記成按鈕名稱。需要什麼的規則同上，用的是對話目前的教材、主題、班級與版型。
- 409：另一輪還在跑（「Manna 還在處理上一個請求…」）；`revise` 但對話裡沒有學習單；`revise` 但那份學習單正在產生。

**背景與輪詢**：每一輪都在背景執行，前端在有輪次 `running` 時每 2 秒 `GET /api/prep-sessions/:id`。伺服器重啟會中斷工作：`running` 超過 `8 × AI_TIMEOUT_SECS + 60` 秒的輪次，下次讀取時改成 `failed`（「這次處理中斷了…請按「再試一次」」）。失敗的輪次可以用同樣的 `kind` 與 `request` 再送一次。

## 各種輪次

### `worksheet`（做學習單）

用建立學習單的同一段程式（`material_id` = 對話的教材，`class_id`、`template_id` = 對話的設定）建立學習單，再開始產生（等同 `POST /api/worksheets/:id/generate`）。`worksheet_id` 立刻有值；學習單產生中這一輪是 `running`，工作結束就是 `done`（即使某個區塊失敗：區塊的 `message` 會說明），只有建立或開始失敗時是 `failed`（例如這份教材沒有文字）。對話裡已有 `done` 的 `activity`、而且沒有用版型時，那份規劃的引起動機想法（`標題：想法`，有選項時是每個選項）放進學習單的 `brief.warmup`。

**對話做的每一份學習單**（`worksheet`、`assessment`、`comprehension`、`tiered`、`kit`）都帶入：

- 出題選項：`settings` 的 `bloom`、`question_count`、`bloom_counts`、`answer_formats`、`objective`（等同 `POST /api/worksheets { settings }` 的部分設定；沒有設定的鍵維持預設）。`assessment` 的作答方式固定，不套用 `answer_formats`。
- 「教師的要求」`request`：這一輪老師打的字；這一輪只是按鈕（`request` 是按鈕名稱）時，用對話**第一輪**的 `request`。按鈕或工具的名稱（做學習單、設計評量、規劃教學活動、找教學資源、直接問 AI、修改學習單、閱讀理解題、差異化學習單（與舊名分層練習）、課堂活動、教學套件、學習單、完整學習單）不算要求，學習單的 `request` 是 `""`。

### `comprehension`（閱讀理解題）、`tiered`（差異化學習單，原名分層練習）

和 `worksheet` 一樣（學習單立刻建立、產生中 `running`、結束 `done`），但學習單的 `settings.output` 是 `comprehension`／`tiered`，依 Bloom 層次產生（見「Phase 2.7」）；**不用對話的版型**（版型只做完整學習單），也沒有暖身。標題是學習單預設的標題。

### `assessment`（設計評量）

和 `worksheet` 一樣，但：標題「評量：<教材標題>」，不用版型，`settings` 固定為 `student_tasks` = 理解內容、字彙練習、文法／句型練習，`answer_formats` = 選擇題、是非題、填空題、配對題、簡答題，`generation` = 顯示學習目標（沒有暖身），`levels` = 一般版。

### `activity`（規劃教學活動）

一次呼叫（`prep_activity` 第 4 版，`max_tokens` 8000）。輸入：加了段落標記（`[第 n 段]`，全部段落，不設上限；有「只選要用的段落」時只有選的）的教材、班級特性、以教材做混合檢索得到的前 8 個課綱條目（和 SOP 學習目標相同的檢索）、教師的目標與教師的要求（規則同上面的學習單，變數 `{{teacher_request}}`）。沒有時都是「（沒有）」。

**教師的目標**有三個變數：`{{teacher_content_objective}}`（內容目標）、`{{teacher_language_objective}}`（語言目標）、`{{teacher_objective}}`（合併的一行，目前就是 `options.objective`）。前兩個由 `prep::TeacherObjectives::of` 從對話的選項取得；選項還沒有分開的內容目標、語言目標欄位時是「（沒有）」，模型自己草擬。教師給了哪一組，schema 那一組就只要 1 個目標，伺服器把教師的原文一字不改放進去（有中文的放 `zh`，否則放 `en`；另一種語言的那一行與課綱代碼用模型寫的），標上 `from_teacher`；教師給的目標不會因為下面的檢查被拿掉。只給了內容目標時，語言目標由模型從它推出。

模型的輸出依序是 `title`、`content_goals`、`language_goals`、`key_vocabulary`、`materials`、`higher_order_questions`、`background`、`steps`、`review`、`assessment`（schema 的順序就是寫的順序：先內容目標，再由它推出語言目標）。內容目標依序是 `zh`（≤150）、`en`（≤200）、`curriculum_codes`；語言目標依序是 `supports`（`C1`／`C2`，enum）、`category`（「學習策略」「語言功能」「句型文法」「學術字彙」，enum，學習策略排第一）、`zh`、`en`、`relation`（≤200）、`curriculum_codes`；兩組各 1–2 個。每一步依序是 `phase`、`title`、`minutes`、`idea`、`grouping`（四個標籤的 enum）、`roles`（≤6，`role` ≤20、`task` ≤100）、`monitoring`（≤150）、`skills`（聽說讀寫的 enum，1–4 個）、`scaffolding`（≤150）、`bloom`、`options`、`script`、`stuck_tip`。schema 裡每個字串與陣列都有 `maxLength`／`maxItems`；需要分行的內容（`script`、`roles`、`materials`）是陣列，其他文字各只寫一句（gemma 想換行時會提早結束字串）。

伺服器檢查：

- **目標**：`zh` 沒有中文字或 `en` 不是英文的目標拿掉；開頭的動詞看不到表現的（中文「學生能」後面是了解、瞭解、理解、認識、知道、學會、學習、明白、熟悉、體會、欣賞、懂；英文 learn、know、understand、appreciate、realize、comprehend、grasp、be aware、be familiar）拿掉；每組最多留 2 個。語言目標的 `supports` 指向的內容目標被拿掉了，或 `relation` 不成立（不是「學生要用……，才能……」、任一半太短，或「才能」後面那一半和它支持的內容目標沒有共同的中文雙字詞或四個字母以上的英文字），這個語言目標就拿掉——說不出關係的語言目標，寫得再好也不採用。留下的語言目標學習策略排前面，`supports` 改成在 `goals` 裡內容目標的序號（從 1 起）。
- **其他**：不是候選的代碼拿掉；`phase` 不是三個標籤之一的步驟拿掉；分鐘數限制在 1–50；`options` 只留給引起動機（最多 3 個）；`bloom` 去掉重複、留模型寫的前 3 個、再依 Bloom 順序排；`roles` 兩人一組最多 2 個、小組最多 6 個、全班與個人一律清空，沒有 `task` 的拿掉；`skills` 只留四個標籤、不重複、依聽說讀寫排；`key_vocabulary` 的 `word` 要有英文字母、不重複（不分大小寫），最多 8 個；`higher_order_questions` 只留 analyze／evaluate／create、題目有英文字母的，最多 4 個；所有文字經過 `clean_markup` 並截到上限；`curriculum` 由伺服器依引用的代碼從知識庫帶出。
- **重試**：兩組目標都沒有可用的目標、可用步驟少於 3 個、輸出寫到 `max_tokens` 或讀不懂時**重試一次**（temperature +0.4，附上問題）；還是不行 → `failed`。下列情形也重試一次並附上問題（第一次記成 `blocked`），但規劃本身可以用——重試的規劃比較好就用重試的，重試還是有問題就照樣給教師看重試的規劃（`ai_interactions` 的檢查結果記下沒有通過）：只缺一組目標；教師沒有給語言目標，而語言目標裡沒有學習策略（R2）；教師給的語言目標說不出關係；`background` 是空的（A1）；整節課的分組方式少於兩種、兩人一組沒有寫出兩人各做什麼、小組的角色少於 3 個或沒有寫教師巡視（A2）；整節課沒有用到聽說讀寫四種技能（A3）。

### `kit`（教學套件）

一次做兩件事：**課堂活動**（和 `activity` 相同的呼叫，結果在 `result`）與**差異化學習單**（和 `tiered` 相同，學習單在 `worksheet`）。學習單在請求當下建立並開始產生（`worksheet_id` 立刻有值），活動規劃同時在背景進行。

- 兩者都結束前這一輪是 `running`。活動規劃一做好，`result` 就有值（輪詢時可以先顯示）；`error` 只在這一輪結束後才出現。
- 兩者都成功 → `done`。任何一個失敗 → `failed`，成功的那一部分保留可見（`result` 或 `worksheet`）。`error`：「教學套件的課堂活動沒有完成：…」「教學套件的差異化學習單沒有完成：…」，兩個都失敗時「教學套件沒有完成。課堂活動：…差異化學習單：…」。差異化學習單失敗是指：建立或開始失敗（例如這份教材沒有文字），或產生結束時一題都沒有。
- 下一步建議裡 `kit` 做完不代表 `activity` 做完（反之亦然）。

**下載**：`GET /api/prep-sessions/:id/turns/:turn_id/kit?format=docx`（`format` 省略也是 DOCX；其他值 → 400「format 需為 docx。」）。一份 DOCX，每部分從新的一頁開始：

1. **教學流程**：規劃的標題、學習目標（分成「內容目標」「語言目標」兩組，各自編號；每個目標是中文一行、附課綱代碼，英文斜體在下一行；語言目標前面標類別「〔學習策略〕」，下一行是「支持內容目標 1：學生要用……，才能……」；教師給的目標標「（教師提供）」；舊規劃照舊是一張中文清單）、關鍵字彙、教材與教具、高層次問題、背景連結、每一步（階段｜標題（分鐘）、`分組：小組｜語言技能：說、讀`、`Bloom：理解、應用`、教學思路、分工、教師巡視、鷹架、引起動機的選項、課堂用語、卡住了怎麼辦）、複習與評量、對應的課綱條目。舊規劃沒有的部分不印。
2. **學生版（A 版）**、**學生版（B 版）**、**學生版（C 版）**（學習單的每個版本；學生頁面不印程度名稱，見「差異化學習單」）：和 `GET /api/worksheets/:id/export?audience=student` 印的一樣（各版本自己的支持，沒有 Bloom 標記）。
3. **教師版**：差異化學習單是每個版本一份教師版（「教師版（基礎版＝A 版）」…），附各版本的答案、評分說明、依據、Bloom 標記與分層說明；改版前的分層練習照舊：所有版本的題目各一次、依序編號，附答案、依據與 Bloom 標記（〔Bloom：分析 Analyze〕）與區塊的 Bloom 摘要，另印版本與層次的對應（「基礎版：記憶＋理解＋應用（附提示）；…」）。

- 用的是學習單**目前**的版本（之後的修改、`revise` 都會反映）。檔名：`<對話標題>_教學套件.docx`（`Content-Disposition` 同學習單匯出，UTF-8 檔名）。
- 404：不是自己的對話、或這一輪不在這個對話；400：這一輪不是 `kit`；409：這一輪還在跑（「教學套件還在產生，好了之後再下載。」）。`failed` 的教學套件照樣可以下載成功的那一部分（缺的部分印一行說明）；兩部分都沒有 → 409。

### `resources`（找教學資源）

不呼叫模型。混合檢索：有教材時用「教材標題＋前兩段」，否則用 `topic`，取前 8 個課綱條目（知識庫課綱搜尋的同一套檢索，只列出有關聯的條目）。另外以關鍵字比對老師自己教材庫的教材（標題與內文），取前 5 份，不含這個對話的教材。兩者都沒有時 `note` =「知識庫裡沒有找到相關條目。」；`note` 最後一律是「外部資源（影片、網站）搜尋還沒有開放。」。`interaction_id` 為 null。

### `ask`（直接問 AI）

一次呼叫（`prep_ask`，`max_tokens` 3000）。輸入：問題；教材（有的話，同樣加段落標記）；班級特性；以問題做混合檢索得到的前 5 個課綱條目；這個對話前 3 輪的 `{kind, request, 摘要}`（摘要是回覆文字；學習單是它的區塊標題；活動規劃是標題與各步驟；資源是找到的代碼）。輸出 `{ answer ≤1500, follow_ups ≤3（每個 ≤40） }`。純文字：伺服器拿掉 Markdown 的標題記號與粗體，空的 `follow_ups` 拿掉。答案是空的、輸出失控或讀不懂時重試一次。

### `revise`（用一句話修改學習單）

套用在對話裡最新一個做出學習單的輪次（`worksheet`、`assessment`、`comprehension`、`tiered`、`kit`）的學習單。一次呼叫（`prep_revise`，`max_tokens` 6000）。輸入：老師的指示、加段落標記的教材（學習單自己的教材快照）、學習單目前的區塊 `{block_id, label（學生版上的 A、B…）, kind, title, instructions, items[{answer_format, prompt, options, answer}]}`。輸出 `{ reply ≤300, changes ≤6: [{ block_id（enum）, title, instructions, items ≤12 }] }`。

- 每個 change 用照版型產生時的同一個檢查（`evaluate_section`，題數上限 12、不需要外部素材）：作答方式只能用固定標籤、選擇題的答案必須是選項之一、題幹不可含答案、音節題依字典修正…。有 change 沒通過時重試一次（附上沒通過的區塊與原因）；重試後還沒通過的 change 不套用，`reply` 後面加上「（B 區沒有改成：修改沒有通過學習單的檢查，維持原樣。）」。
- 通過的 change 取代該區塊的 `title`、`instructions` 與 `items`：新題目 `origin = "ai"`；出口卡的 `purpose`、題目的 `bloom`、`hint` 與 **`levels`（出現在哪些版本）依位置保留**（取代同一個位置的題目）；多出來的新題目 `bloom` = 區塊的層次（依 Bloom 層次產生的區塊才有），出現在學習單的所有版本（差異化學習單每一題都在三個版本）（保留的版本只取學習單現有的版本，一個都不剩時也是所有版本）；鷹架與字詞保留。被取代的區塊先放進 `worksheet_block_history`（編輯器的「復原」照常可用），全部改動寫成**一個**新版本（`cause = "revise"`）。內容沒有實際改變的區塊不算。
- 沒有改動 → 不新增版本（`reply` 本身就是回答，例如一個澄清問題）。`result = { reply, changed_block_ids, version_no }`。
- 呼叫期間學習單開始產生時，這一輪 `failed`，不套用。
- `revise` 只改學習單；活動規劃（包括學習目標）不會被改動，`activity`／`kit` 存的 `result` 原樣保留。

## 測試用假模型（`AI_PROVIDER=fake`）

| 標記（寫在請求或教材裡） | 效果 |
|---|---|
| `FAKE_BLOCK` | activity／ask：兩次都交出不能用的輸出 → `failed` |
| `FAKE_BLOCK_ONCE` | activity／ask：第一次不能用，重試成功 |
| `FAKE_PREP_BAD_REVISE` | revise：兩次的 change 都用了固定標籤以外的作答方式 → 不套用，`reply` 說明 |
| `FAKE_PREP_NO_PLAN` | activity／kit：活動規劃兩次都不能用（不影響學習單的呼叫）→ `activity` 是 `failed`；`kit` 是 `failed`、學習單保留 |
| `FAKE_PREP_ONE_GROUP` | activity：兩次都沒有語言目標 → 重試後仍照樣 `done`，規劃只有內容目標 |
| `FAKE_PREP_ONE_GROUP_ONCE` | activity：第一次沒有語言目標，重試兩組都有 |
| `FAKE_PREP_FLAT` | activity：兩次都是每一步全班、沒有分工、只有「讀」→ 重試後仍照樣 `done` |
| `FAKE_PREP_FLAT_ONCE` | activity：第一次如上，重試有三種分組與聽說讀寫 |
| `FAKE_PREP_REVISE_ADD` | revise：改的是**最後一個**有題目的區塊，並在最後加一題 |
| `FAKE_SIOP_BLOCK` | SIOP 自我檢核：兩次都不能用 → 檢核 `failed`（產出不受影響） |
| `FAKE_SIOP_PARTIAL_ONCE` | SIOP 自我檢核：第一次漏了最後一項，重試補齊 |
| `FAKE_SIOP_FAIL_FIRST` | SIOP 自我檢核：這一輪的第一次檢核逾時；之後再檢核（再試一次）就成功 |

假模型的活動規劃（第 4 版）有 3 個內容目標，第 2 個用「了解」開頭（伺服器拿掉）；5 個語言目標中伺服器留 2 個、學習策略排第一：一個支持被拿掉的內容目標、一個 `en` 寫成中文、一個的 `relation` 不是「學生要用……，才能……」，都拿掉。第一個內容目標引用第一個候選代碼和一個不存在的代碼（伺服器拿掉），發展活動寫 80 分鐘（限制成 50），只有引起動機以外的步驟也帶了選項（伺服器拿掉）；三步依序是兩人一組（兩人各做什麼）、小組（4 個角色與教師巡視）、個人（帶了一個角色，伺服器拿掉），合起來聽說讀寫都有；每一步的 `bloom` 順序打亂、有重複，發展活動寫了 4 個（伺服器留前 3 個並排序）；關鍵字彙有一個大小寫不同的重複字與一個中文字（伺服器拿掉），高層次問題有一題 remember（伺服器拿掉）。`fake_context` 帶了教師的內容目標或語言目標時，那一組只寫 1 個、文字前面加「（模型改寫）」（伺服器換回教師的原文）。假模型的自我檢核依序判斷每一項：第 3 項「部分符合」、第 17 項「未見」、其餘「符合」，理由提到產出種類。假模型的 revise 修改第一個有題目的區塊：第一題的題幹後面加上 " (revised)"。既有的 `FAKE_TIMEOUT`、`FAKE_ERROR`、`FAKE_RUNAWAY`、`FAKE_SLOW` 照舊（寫在教材裡的標記也會出現在自我檢核的訊息裡）。

## usage_logs 新事件

`prep_session_created`（`session_id`、`kind`、`material_id`、`class_id`、`template_id`、`entry`、`ui_version`）、`prep_session_deleted`（`session_id`、`worksheet_ids`：一起刪除的學習單）、`prep_turn_created`（`session_id`、`turn_id`、`kind`）、`prep_kit_exported`（`session_id`、`turn_id`、`worksheet_id`、`version_no`、`has_plan`、`bytes`）、`siop_check_rerun`（`session_id`、`turn_id`、`run_no`：教師按了再試一次、重新檢核或開始檢核）。對話做出的學習單照常記 `worksheet_created`（多了 `prep_session_id`）與 `worksheet_generated`。

## SIOP 自我檢核

規則見 `docs/SIOP_DESIGN_RULES.md` 第八節。`/create` 的產出做好後，Manna 依 SIOP 觀察量表檢核**產出上看得到的項目**，每項「符合／部分符合／未見」加一句理由，**不產生總分**；結果是給教師參考的建議。

**哪些產出**：`worksheet`（完整學習單、照我的學習單做）、`comprehension`、`tiered`、`activity`、`kit` 的輪次。輪次變成 `done` 的同一個交易裡開一筆檢核（`running`），在背景跑；所以 `PrepTurn` 一變成 `done` 就帶著 `siop_check`。`failed` 的輪次不檢核。教學套件是一筆檢核，看活動規劃加分層學習單。

**項目**（伺服器的 `prep::selfcheck::ITEMS`，依量表題號；合併的照規格合併）：

| 範圍 `scope` | 項目 |
|---|---|
| `page`：從產出判斷 | 1–2 內容目標與語言目標、3 概念適切、4 補充教材、5 依程度調整、6 有意義的活動、7–8 連結經驗與舊知、9 關鍵字彙、11 任務說明、13 學習策略、14 鷹架、15 高層次思考、16 互動、17 分組、19 母語支持、20 動手操作、21 應用、22 聽說讀寫、27–28 複習、30 評量 |
| `partial`：只看書面的那一部分（`note` 說明） | 12 讓概念清楚的技巧（只看得到書面的視覺輔助）、23–24 教學流程支持目標（只看得到教學流程是否對應目標）、29 回饋（只看得到是否設計回饋的時機） |
| `classroom`：需課堂觀察，AI 不評 | 10 教師語速與用語、18 等待時間、25 學生投入、26 節奏 |

`classroom` 的四項**不送給模型**（不在 prompt、不在 schema 的 enum），`rating` 永遠是 `null`。

**一次呼叫**（`siop_selfcheck` 第 1 版，`max_tokens` 3000），照 `worksheet_bloom_check` 的做法：schema 要求剛好 22 筆，每筆依序是 `id`（22 個題號的 enum，依項目順序）、`reason`（≤100，先寫看到了什麼）、`rating`（「符合」「部分符合」「未見」的 enum）。輸入：產出種類、班級特性、教材（學習單用它自己的教材快照，活動規劃用對話的教材；加段落標記）、**產出的文字版**（不是 JSON：活動規劃逐項列出目標與關係、關鍵字彙、教材、高層次問題、背景連結、每一步的分組／技能／Bloom／思路／分工／巡視／鷹架／選項／課堂用語，與複習評量；學習單列出教師寫的目標、版本、目標、每個啟用的區塊與它的作答指示、字彙、鷹架、每一題（作答方式、Bloom、在哪些版本、選項、答案、依據、基礎版提示）；學習單裡這裡不認得的欄位以「欄位名：值」照印，學習單之後加的欄位也會被看到；最多 16000 字）與 22 個項目。教材與產出在 `ai_interactions` 只記字數與雜湊。伺服器檢查：不是要檢核的題號、`rating` 不是三個之一的拿掉；同一題寫兩次留第一次；有漏的或沒寫理由的 → **重試一次**（temperature +0.4，附上問題）。重試還有漏的，漏的那幾項 `rating` 是 `null`（頁面寫「這次沒有判斷」）。一項都不能用、逾時或錯誤 → 這次檢核 `failed`，`error` 是中文原因；**產出完全不受影響**。

**儲存**：`siop_checks` 一次檢核一列：`turn_id`、`run_no`（這一輪的第幾次，從 1 起）、`status`（`running`｜`done`｜`failed`）、`items`（只存判斷過的 `[{ id, rating: "met"|"partial"|"not_seen", reason }]`）、`error`、`worksheet_id` 與 `worksheet_version_no`（檢核的是哪一版學習單）、`interaction_id`、`created_at`、`finished_at`。同一輪同時只有一次在跑（部分唯一索引）。每次都保留。`running` 超過 `3 × AI_TIMEOUT_SECS + 60` 秒（伺服器重啟中斷）在下次讀取時改成 `failed`（「這次檢核中斷了…請按「再試一次」」）。

```ts
interface SiopCheck {
  id: number; run_no: number;
  status: "running" | "done" | "failed";
  error: string | null;
  worksheet_version_no: number | null;   // 學習單與教學套件：檢核的版本；比 PrepTurn.worksheet.version_no 舊 → 學習單之後改過了
  items: Array<{                         // 量表全部 26 項（含 classroom），依題號順序
    id: string;                          // "1-2"、"3"…"30"
    name: string; question: string;      // classroom 的 question 是 ""
    scope: "page" | "partial" | "classroom";
    note: string;                        // partial：看得到的部分；classroom：為什麼要在課堂上看；page：""
    rating: "met" | "partial" | "not_seen" | null;   // null：classroom，或這次沒有判斷（running／failed 時全部是 null）
    reason: string;                      // ≤100，一句
  }>;
  interaction_id: number | null;
  created_at: string; finished_at: string | null;
}
```

**端點**（只能存取自己的對話；別人的、或這一輪不在這個對話 → 404）

- `GET /api/prep-sessions/:id/turns/:turn_id/siop-check` → 最新一次的 `SiopCheck`；這一輪沒有檢核 → 404「這一輪還沒有 SIOP 自我檢核。」。頁面在 `running` 時每 2 秒問一次。
- `POST /api/prep-sessions/:id/turns/:turn_id/siop-check`（body `{}`）→ 202，新的一次（`run_no` +1，`running`）。用於失敗後的「再試一次」、學習單改過之後的「重新檢核」，以及檢核功能之前做的輪次（`siop_check` 是 `null`）的「開始檢核」。新的一次的 temperature 比上一次高 0.1（最多 +0.4），閘道不會回快取的同一個答案。400：這一輪的種類沒有檢核（「這種產出沒有 SIOP 自我檢核。」）；409：這一輪不是 `done`（「這一輪還沒有完成，沒有可以檢核的產出。」）、這一輪的檢核還在跑（「SIOP 自我檢核還在進行，好了之後再試。」）。

**前端**：每個學習單、課堂活動、教學套件的結果下方有一個「SIOP 自我檢核」區塊。檢核中顯示「Manna 正在做 SIOP 自我檢核…」；完成後是收合的面板，打開是一句說明（這是建議、不是評分，由教師判斷）、每一項的題號、名稱、結果與理由（只能部分判斷的項目標「只能部分判斷：只看得到……」）、最後列出需課堂觀察的四項。失敗時說「這次的 SIOP 自我檢核沒有完成，產出不受影響。」與原因，附「再試一次」；學習單改過之後附「重新檢核」；沒有檢核的舊輪次附「開始檢核」。

## 試用真模型

prompt key：`prep_activity`（第 4 版）、`prep_ask`、`prep_revise`、`siop_selfcheck`（可在管理頁改版本）。程式進入點：`prep::run::start`（各輪次）、`prep::selfcheck::spawn`（自我檢核）；JSON schema 在 `prep::activity_schema`、`prep::ask_schema`、`prep::revise_schema`、`prep::selfcheck::schema`；教學套件的 DOCX 在 `prep::kit`（用 `worksheet::export::combined_docx`）。

---

# Phase 2.7：Bloom 認知層次與閱讀理解題／分層練習

以下為**新增**；既有端點只多了選填欄位與回應欄位，舊的學習單照常讀寫。Migration `0014_bloom.sql`。

學習單產生器多了兩種產出：**閱讀理解題**（`comprehension`）與**分層練習**（`tiered`）。兩者都依 Bloom 認知層次出題：每個層次一次模型呼叫，再由另一次呼叫獨立判斷每一題的層次。原本的完整學習單（`worksheet`）流程不變，只是 `tasks` 步驟的每一題也標上 Bloom 層次。

## Bloom（固定六個；伺服器只接受這些 id）

| id | 中文 | 學生要做的事 | 適合的作答方式 | 區塊標題 | 區塊的 `student_tasks` |
|---|---|---|---|---|---|
| `remember` | 記憶 | 回想或找出文本明白寫出的事實 | 選擇題、是非題、填空題、配對題、簡答題 | Find the Facts | 理解內容 |
| `understand` | 理解 | 解釋、改述、摘要、從上下文推知詞義 | 選擇題、填空題、簡答題、配對題 | Understand the Text | 理解內容 |
| `apply` | 應用 | 把文本的概念或語言用在新的情境 | 簡答題、填空題 | Use It | 字彙練習 |
| `analyze` | 分析 | 因果、比較、結構、目的、事實與意見 | 表格整理、圖像組織、簡答題、選擇題 | Look Closer | 推論／預測、比較／分類 |
| `evaluate` | 評鑑 | 根據證據判斷並說明理由 | 簡答題、段落寫作 | What Do You Think? | 評估／提出理由 |
| `create` | 創造 | 根據文本產出新的東西 | 段落寫作、簡答題 | Make Something New | 寫作 |

順序一律是 remember → create。表外的值（設定、題目、`bloom_counts` 的鍵）→ 400，訊息「「x」不是 Bloom 認知層次的選項（remember、understand、apply、analyze、evaluate、create）。」（在「請求內容格式不正確：」之後）。

## 型別變更

```ts
type Bloom = "remember" | "understand" | "apply" | "analyze" | "evaluate" | "create";
type WsOutput = "worksheet" | "comprehension" | "tiered";

interface WorksheetSettings {
  /* …既有欄位… */
  output: WsOutput;                       // 預設 "worksheet"（原本的流程）
  bloom: Bloom[];                         // 目標層次，依 Bloom 順序、去重；空陣列＝六個都出
  question_count: number | null;          // 3–30；null＝預設（閱讀理解題 8、差異化學習單 6）
  bloom_counts: Partial<Record<Bloom, number>> | null;   // 每個層次的題數（各 0–12，合計 3–30）；有值時取代 bloom 與 question_count
  content_objective: string | null;       // 內容目標（≤300，一行一項，空字串變 null）；舊的 objective 讀成這一欄（見「SIOP 目標」）
  language_objective: string | null;      // 語言目標（≤300，一行一項）
}
interface WsItem {
  /* …既有欄位… */
  bloom: Bloom | null;                    // 這一題的 Bloom 層次；舊資料是 null。教師可以改或清空
  hint: string;                           // 提示或句子開頭，預設 ""；只印在基礎版
}
interface WsBlock {
  /* …既有欄位… */
  bloom: Bloom | null;                    // 依 Bloom 層次產生的區塊：它的層次（重新產生時重跑這個層次）；其他區塊是 null
}
interface WorksheetState { /* …既有欄位… */ request: string; }   // 「教師的要求」（≤2000），沒有時是 ""
interface WsJob { step: "tasks" | "warmup" | "scaffolds" | "exit_ticket" | "template" | "bloom" | "bloom_check"; /* … */ }

// WsSheet（GET …/sheet 與匯出用同一份）
interface WsSheetItem { /* …既有欄位… */
  bloom?: Bloom | null;                   // 只有教師版有這個鍵（沒有層次時是 null）
  hint?: string;                          // 只有基礎版（學生版與教師版）、而且有提示時才有
}
interface WsSheetBlock { /* …既有欄位… */
  bloom_summary?: string;                 // 只有教師版：這個版本裡這一區題目的層次，例如「Bloom：記憶 2、理解 1」；沒有任何題目有層次時沒有這個鍵
}
```

- `WsItem.bloom`、`WsItem.hint` 在 `PUT …/:id { content }` 時重新驗證：層次不在表內 → 400；`hint` 最多 4000 字。
- `bloom_counts` 依 Bloom 順序回傳，值為 0 的鍵保留（「有指定、不出」）；只有值大於 0 的層次會產生。

## 端點變更

| Method | Path | 變更 |
|---|---|---|
| POST | `/api/worksheets` | 新增 `settings?`（**部分** `WorksheetSettings`：有給的欄位取代建議值，沒給的照今天的預設／版型預填）與 `request?`（≤2000） |
| PUT | `/api/worksheets/:id` | `settings` 改為**部分更新**：有給的欄位取代目前的值，沒給的不變（送完整的 `WorksheetSettings` 和以前一樣整份取代）；`minutes`、`question_count`、`bloom_counts`、`objective` 送 `null` 會清空。新增 `request?: string \| null`（`null` 或 `""` 清空） |
| PUT | `/api/worksheets/:id/material` | 換教材時，`output`、`bloom`、`question_count`、`bloom_counts`、`objective` 與 `request` 保留（不是依教材建議的） |

**驗證（400，訊息為中文）**

- `question_count` 不在 3–30 →「題數需為 3 到 30 題；也可以不填。」
- `bloom_counts` 有任一個不在 0–12，或合計不在 3–30 →「每個 Bloom 層次的題數需為 0 到 12 題，合計 3 到 30 題。」
- `objective` 超過 300 字 →「學習目標最多 300 字。」；`request` 超過 2000 字 →「給 Manna 的要求最多 2000 字。」
- `output` 不是三個值之一 →「「x」不是「要做什麼」的選項（worksheet、comprehension、tiered）。」
- **版型只用在完整學習單**：建立或儲存後，學習單有版型而 `output ≠ "worksheet"` →「版型只適用於完整學習單。」。要把有版型的學習單改成閱讀理解題，同一個請求送 `template_id: null`。

## 教師的要求與學習目標

> 學習目標已拆成 `content_objective`（內容目標）與 `language_objective`（語言目標），並成為學習單的 `content.objectives`；變數與帶入方式見「SIOP 目標」。下面是拆開以前的說明，`objective` 現在讀成 `content_objective`。

`request`（存在 `worksheets.request`）與 `settings.objective` 會送進這份學習單**每一次**模型呼叫：

- 新的 prompt（`worksheet_bloom_level`）有自己的變數 `{{teacher_request}}`、`{{teacher_objective}}`（沒有時是「（沒有）」）。
- 既有的 prompt（`worksheet_tasks`、`worksheet_warmup`、`worksheet_scaffolds`、`worksheet_exit_ticket`、`worksheet_template_section`）不必改：它們接在教材註記（`teacher_notes_for_ai`）同一段後面，各自一行「學習目標（教師寫的）：…」「教師的要求：…」。
- 兩者都列在工作的 `context`（「Manna 已帶入的資訊」），只顯示前 80 字。
- 例外：判斷層次的 `worksheet_bloom_check` 不帶這兩者，讓判斷不受教師期待影響。

## 產生：閱讀理解題（`output = "comprehension"`）

`POST …/generate` 建立一輪兩個工作：`bloom`（所有層次的出題呼叫）與 `bloom_check`（判斷、重出與第二次判斷）。後者維持 `running` 直到新版本存好（`phase`「正在把結果組合成學習單」）。

**題數分配**（`worksheet::bloom::plan`）

- `bloom_counts` 有值：照它（值大於 0 的層次）。
- 否則：層次＝`bloom`（空的＝六個），總題數＝`question_count`（沒有時 8）。每個層次至少 1 題，其餘從低的層次開始依序輪流加（記憶、理解、應用、分析、評鑑、創造、再回到記憶…）。8 題六個層次＝2、2、1、1、1、1；12 題＝各 2。

**作答方式**：每個層次可用的作答方式＝該層次適合的作答方式 ∩ 教師勾的 `answer_formats`；交集是空的就用教師勾的；教師沒勾就用該層次適合的全部。

**每個層次一次呼叫**（`worksheet_bloom_level`，鷹架 `SC-WS-BLOOM`），依 Bloom 順序一個接一個（同時跑時，「理解」會重問「記憶」剛出過的題目），`WsJob.phase` 例如「正在出『分析』層次的題目」。`max_tokens` = 400 + 300 × 題數（1500–6000）。

- 輸入：用途（閱讀理解題；差異化學習單用自己的 prompt，見「差異化學習單」，兩者的請求因此不同，閘道不會拿快取的題目回應）、層次的定義、英文題幹、可用的作答方式、題數、學校類型、班級特性、學習目標、教師的要求、加了段落代號（`[P1]`…）的教材、較低層次已經出好的題目（不要重複），以及教師註記（有勾「提供給 AI」時）。
- 輸出 schema（第 1 版；第 2 版把 `instructions` 換成 `steps` 與 `example`，見「差異化學習單」一節的 R5）（欄位照撰寫順序，每個字串與陣列都有上限）：`{ instructions ≤200, items: [正好 n 題] { answer_format: 該層次可用的作答方式（enum）, prompt ≤500, options ≤6×150, answer ≤400, evidence ≤300, paragraph_ids: ["P1"…]（enum，≤3）, hint ≤150 } }`。`paragraph_ids` 由伺服器換成教材的段落 id，聯集成區塊的 `paragraph_ids`。
- **檢查**（和照版型產生的 `evaluate_section` 同一套）：作答方式只能是這個層次可用的；選擇題至少 2 個不重複選項、答案必須是其中之一（只寫字母時換成該選項）；配對題至少 2 個選項；填空題要有 `___`；題幹不可含答案；音節題依字典修正；是非題沒給選項時補 True／False；超過題數由程式只留前面幾題。**記憶、理解**層次每一題都要有逐字依據：不是逐字的依據改成其中最長、至少 5 個字的逐字片段；找不到就算這一題沒通過。其他層次不需要依據，不是逐字的直接清空。
- 沒通過 → **修復一次**（temperature +0.4，附上沒通過的項目）；還是沒通過的題目拿掉；一題都沒有時，這個層次的區塊留空並在 `message` 說明。

**判斷層次**（`worksheet_bloom_check`，鷹架 `SC-WS-BLOOM-CHECK`，`max_tokens` = 300 + 80 × 題數（1000–3000））：所有層次出完後，一次呼叫獨立判斷每一題（不告訴模型原本的目標）：`{ items: [{ id: "Q1"…（enum）, bloom（enum）, reason ≤80 }] }`，每題一筆；漏掉的題目修復一次，之後仍沒判斷的維持目標層次。

- **不符**的定義：判斷的層次比目標低（任何一層；真模型實測 90 題有 9 題「理解」其實只是找事實），或比目標高 2 層以上。
- 不符的題目**重出一次**：每個受影響的層次一次呼叫（同一個 prompt，`phase`「正在重出層次不符的『評鑑』題目」），附上判斷的層次與理由；新題目取代原題、再判斷一次（只判斷新題目）。
- 仍不符（或重出沒有成功）的題目保留，`bloom` 改成判斷的層次，並在 `content.notices` 加一行，例如「第 7 題原本要出『評鑑』，Manna 判斷是『理解』，已改標為『理解』。」（題號是整份學習單依序的編號）。
- 判斷呼叫失敗時題目都標出題時的目標層次，`notices` 說明檢查沒有完成。

**組裝**（程式）：

- 每個層次一個區塊，依 Bloom 順序；`kind = "activity"`；`bloom` = 該層次；標題固定為該層次的名稱（記憶「Find the Facts」、理解「Understand the Text」、應用「Use It」、分析「Look Closer」、評鑑「What Do You Think?」、創造「Make Something New」）；字母不寫在標題裡，由學習單、編輯器與匯出依區塊順序印在前面（「A. Find the Facts」），教師調整順序後也不會對不上；`instructions` 是模型寫的一句英文（沒有時用預設句）；`student_tasks` 見上表；沒有鷹架、沒有暖身、沒有 Exit ticket。
- 題目：`origin = "ai"`，`bloom` = 層次（被改標的是判斷的層次），`hint` 照模型輸出，`levels` = 學習單的版本（`settings.levels`，沒有時是一般版）。
- 一個新的學習單版本（`cause = "generate"`）。
- `content.chosen`：`settings` 是實際用的（`levels`、`bloom` = 實際產生的層次、`question_count` = 總題數），`item_budget` = 總題數，`reasons` 第一條是「Bloom：記憶 2、理解 2、應用 1、分析 1、評鑑 1、創造 1」，再說明題數與版本怎麼決定。

## 產生：分層練習（`output = "tiered"`）

> **已由「差異化學習單」取代**（見下一個大節）：三個版本題目相同、只差在支持。以下是改版前的規則，改版前存下的學習單照這個規則顯示與匯出。

和閱讀理解題相同，差別：

- 版本一律是基礎版、一般版、進階版（不論 `settings.levels`）；預設 12 題。
- 每一題的 `levels` 依它的 Bloom 層次（被改標的題目依判斷的層次）：

  | Bloom | 出現在 |
  |---|---|
  | 記憶 | 基礎版 |
  | 理解、應用 | 三個版本（核心） |
  | 分析 | 一般版、進階版 |
  | 評鑑、創造 | 進階版 |

- 每一題都請模型給 `hint`，只印在基礎版。
- `content.chosen.reasons` 另加「基礎版：記憶＋理解＋應用（附提示）；一般版：理解到分析；進階版：理解、應用、分析到創造」。
- 投影時，依 Bloom 層次產生的區塊在某個版本裡沒有任何題目，就不出現在那個版本（區塊字母依序遞補）。

## 原本的完整學習單（`output = "worksheet"`）

流程不變。唯一的變更：`worksheet_tasks` 新增**第 2 版** prompt（第 1 版保留），多一條規則請模型為每一題標 `bloom`，schema 的題目多一個 `bloom` 欄位（六個 id 的 enum）。表外的值由伺服器丟掉（`bloom = null`）。暖身與 Exit ticket 的題目沒有層次。

## 單一區塊重新產生與復原

- 依 Bloom 層次產生的學習單（`content.chosen.settings.output` 是 `comprehension` 或 `tiered`）裡、有層次的區塊（`WsBlock.bloom`；沒有時依固定標題，再沒有時依多數題目的 `bloom`）：`POST …/blocks/:block_id/regenerate` 重跑那個層次的呼叫（題數＝區塊原本的題數；原本沒有題目時用分配的題數），其他區塊的題目列為不要重複；再**只判斷這個區塊**（不符的重出一次、仍不符的改標並加 `notices`）。工作是 `bloom` 與 `bloom_check`，`block_id` 為該區塊。標題保留原本的字母。
- 失敗時區塊維持原樣（`bloom_check` 為 `skipped`）。`undo` 照舊。
- 教師自己新增、沒有層次的區塊，用原本的流程重新產生。

## 學生版與教師版

- **教師版**：投影（`GET …/sheet`）的每一題有 `bloom`；DOCX／PDF 在題目後面印「〔Bloom：分析 Analyze〕」，每個區塊標題下印 `bloom_summary`（例如「Bloom：記憶 2」）。這些是中文字，所以教師版 PDF 需要中文字型（和教師備註一樣）。
- **學生版**永遠沒有 Bloom 標記（投影沒有 `bloom`、`bloom_summary` 這兩個鍵）。
- **基礎版**（學生版與教師版）在題目下方印「Hint: …」；投影的題目有 `hint`。其他版本沒有。
- 詞彙級別檢查會把基礎版印出的提示一起分析（位置寫「第 n 題提示」）。

## 測試用假模型（`AI_PROVIDER=fake`）

每個層次的呼叫依可用的作答方式輪流出題，題目根據各段的第一句；記憶、理解層次以那一句作為依據；每題都有提示。判斷呼叫一律判為目標層次。

| 標記（寫在教材裡） | 效果 |
|---|---|
| `FAKE_BLOOM_MISMATCH` | 判斷把第一個評鑑／創造題判為 `understand`，兩次判斷都一樣 → 重出一次後改標，`notices` 有一行說明 |
| `FAKE_BLOOM_NO_EVIDENCE` | 第一個記憶題的依據不在教材裡，只有第 1 次 → 修復呼叫改正 |
| `FAKE_BLOCK` | 每個層次的呼叫只用表外的作答方式 → 區塊留空並附說明 |

既有的 `FAKE_TIMEOUT`、`FAKE_ERROR`、`FAKE_RUNAWAY`、`FAKE_SLOW` 照舊。

## 資料

- `worksheets.request`（`TEXT NOT NULL DEFAULT ''`，≤2000）。`output`、`bloom`、`question_count`、`bloom_counts`、`objective` 在 `worksheets.settings`（JSONB）裡。
- `worksheet_jobs.step` 新增 `bloom`、`bloom_check`。
- `prompt_versions` 新增 `worksheet_bloom_level`（第 1 版）、`worksheet_bloom_check`（第 1 版）、`worksheet_tasks`（第 2 版）。`scaffolds` 新增 `SC-WS-BLOOM`、`SC-WS-BLOOM-CHECK`。
- 每次呼叫一列 `ai_interactions`（`module` = prompt key）；輸入快照裡的教材、已出過的題目與要判斷的題目只記字數與雜湊。

## usage_logs

`worksheet_created` 多了 `output`。依 Bloom 層次產生時，`worksheet_generated` 多了 `output`、`bloom`（`[層次, 題數]`）、`failed_blocks`、`relabelled`。

## 試用真模型

prompt key：`worksheet_bloom_level`、`worksheet_bloom_check`、`worksheet_tasks`（第 2 版）。程式進入點：`worksheet::ai::start_generation`（`settings.output` 是 `comprehension`／`tiered` 時走 `worksheet::ai::bloom`）；題數分配、版本對應、不符的定義與 JSON schema 在 `worksheet::bloom`，檢查在 `worksheet::checks::evaluate_bloom_level`／`evaluate_bloom_check`。

## 備課對話（`0015_prep_bloom.sql`）

備課對話用上面的引擎做學習單，細節併入「Phase 2.6：備課對話」：

- 新的輪次 `comprehension`、`tiered`（`settings.output` 為該值的學習單，不用對話的版型）與 `kit`（教學套件：活動規劃＋差異化學習單，兩者都結束才不是 `running`；任何一個失敗是 `failed`，成功的部分保留）。
- 對話的出題選項 `options: { bloom?, question_count?, bloom_counts?, answer_formats?, objective? }`（建立與 PATCH 都可以給，用本節的驗證規則），和對話的「教師的要求」一起帶進對話做的每一份學習單（`settings` 的同名欄位與 `request`）；`objective` 也送進活動規劃。
- `prep_activity` 第 2 版：每一步多了 `bloom`（≤3，enum，依 Bloom 順序），prompt 多了 `{{teacher_objective}}`、`{{teacher_request}}`。
- `next` 的順序：`comprehension, tiered, activity, kit, worksheet`（都需要教材）。
- 教學套件下載：`GET /api/prep-sessions/:id/turns/:turn_id/kit?format=docx`，一份 DOCX：教學流程、每個版本的學生版、教師版（答案與 Bloom 標記）。
- `revise` 適用最新一份任何種類的學習單；改過的題目保留原本的版本（改版前的分層練習不會變成三個版本都有）。
- 資料：`prep_sessions.options`（JSONB，預設 `{}`）；`prep_turns.kind` 多三個值；`prompt_versions` 新增 `prep_activity` 第 2 版（第 1 版保留）。

---

# 差異化學習單（原名分層練習）與分步作答指示

依 `docs/SIOP_DESIGN_RULES.md` 第三節（T0–T9）與 R5 重新設計 `output = "tiered"`；`閱讀理解題` 的作答指示改成分步驟。Migration `0025_siop_differentiated.sql`。以下取代「Phase 2.7」中「產生：分層練習」一節；改版前存下的學習單照舊顯示與匯出（見最後「舊資料」）。

## 名稱

- 介面名稱「分層練習」改為**差異化學習單**，頁面標題讀作「來做一份 差異化學習單」；網址 `/create/tiered`、`output = "tiered"`、輪次 `kind = "tiered"` 都不變。
- 副標：「同一篇文章做成基礎／一般／進階三個版本。三個版本目標相同，差在提示與支持的多寡。」
- 按鈕名稱（「教師的要求」不算的那些，見 Phase 2.6）同時接受「差異化學習單」與舊名「分層練習」。伺服器的按鈕名稱（`turn.request`）是「差異化學習單」。
- 教學套件與錯誤訊息中的「分層學習單」改為「差異化學習單」（例如「教學套件的差異化學習單沒有完成：…」）。

## 三個版本怎麼不同（T1–T6）

- 三個版本（基礎版／一般版／進階版）用**同一組學習目標**（`content.objectives`，三版頂端都印；差異化學習單產生後 `show_objectives = true`）、**同一組題目**、**同樣的 Bloom 分布**。每一題 `levels` 都是三個版本。
- 差別只在每一題下方的**支持**與**預期的作答**，存在題目的 `tiers`（每個版本一份）。
- 預設 **6 題**（每個層次 1 題；以前的 12 題是分散到三個版本，現在每題三版都有）。`question_count`、`bloom_counts` 規則不變。
- 只用**開放式作答方式**：簡答題、段落寫作、表格整理、圖像組織（各層次適合的開放式作答方式 ∩ 教師勾的；交集是空的就用該層次適合的開放式全部）。教師勾的選擇題、是非題、配對題、填空題不用在差異化學習單。

```ts
type WsSupportKind = "cloze" | "word_bank" | "sentence_frame" | "sentence_starter" | "none";

interface WsItem {
  /* …既有欄位… */
  tiers: WsItemTier[];   // 差異化學習單：每個版本一份，依版本順序；其他題目 []（舊資料沒有這個鍵＝[]）
}
interface WsItemTier {
  level: WsLevel;
  support_kind: WsSupportKind;
  model_sentence: string;   // 克漏字與句型框架挖空前的完整範句（T4）；只在編輯器與資料裡，不印
  support: string[];        // 題目下方印的支持，一行一項；空格是 ___
  word_bank: string[];      // 字庫（克漏字與句型框架：從範句挖掉的字詞）
  answer: string;           // 這個版本預期的答案；只在教師版
  scoring: string;          // 這個版本的評分說明（中文）；只在教師版
}
```

- **支持形式**由多到少（T3）：`cloze` 預填大部分字詞的克漏字 → `word_bank` 字庫 → `sentence_frame` 句型框架 → `sentence_starter` 句首 → `none` 不給支持。各版本可用：

  | 版本 | 可用的形式 |
  |---|---|
  | 基礎版 | cloze、word_bank、sentence_frame |
  | 一般版 | word_bank、sentence_frame、sentence_starter |
  | 進階版 | sentence_starter、none |

  同一題由基礎版到進階版**嚴格遞減**。
- 支持不可以**指出答案位置**（「Look at paragraph 2.」「Read the second paragraph again.」「The answer is in line 3.」、`[P2]`、「第 2 段」），也不可以把題目改簡單（題目三版共用，只寫一次）。一般版與進階版的支持不可以寫出完整答案。
- **句型框架**（T4）：先寫範句（`model_sentence`），再把目標字詞換成 `___`，挖掉的字詞是 `word_bank`；程度越低句構越簡單。克漏字同樣從範句挖空。
- **句首不印字庫**（T3：字庫比句首的支持多）：`support_kind = sentence_starter` 時投影的 `support.word_bank` 一律是 `[]`；版本裡的 `word_bank` 留著（換回別的形式時用得到）。
- **編輯器換支持形式**：由這個版本的 `model_sentence` 與 `word_bank` 依固定規則做出新的支持（`web/src/lib/worksheet.ts` 的 `buildSupport`，同樣的輸入一定得到同樣的結果），寫進 `support`／`word_bank` 後照常自動儲存，所以預覽、列印與匯出印的都是存下來的這幾行；教師之後仍可以手改。預填克漏字＝範句挖掉字庫裡的字（字庫沒有範句裡的字時挖掉最長的 1–3 個實詞，加進字庫）；字庫＝只印字庫（沒有字庫時取範句的實詞）加作答線；句型框架＝範句保留代名詞、助動詞、連接詞、疑問詞、介系詞、think／feel 等框架字，其餘每一段換成一個 `___`（沒有字庫時挖掉的字詞成為字庫）；句首＝範句開頭幾個字加「…」。版本沒有範句（克漏字、句型框架、句首）或字庫與範句都沒有（字庫）時，形式照換、原本的支持不動，編輯器說明原因；教師可以在編輯器寫範句後按「依範句重做」。同一次編輯中換回先前用過的形式，會放回那個形式原本的那幾行。
- **答案**（T6）：每個版本各有 `answer` 與 `scoring`。基礎版可接受片語；一般版與進階版要 1–2 個完整句。題目本身的 `answer` 是進階版的答案（給 Bloom 判斷與舊的流程用）。
- **轉折詞**（T5）：每個層次的區塊依語言功能給轉折詞，基礎版最基本的幾個，一般版再加，進階版再加較正式的（例如 first, next, then → previously, subsequently）。印成各版本自己的字詞表鷹架（`kind = "字詞表"`，`levels` 只有那個版本），內容「Linking words for giving reasons: because, so, since」。
- **改寫文本**（T8）：任何版本印出「調整文本」（Simplified text）鷹架時，同一區的原文段落也一起印（投影的 `original_text`，先印原文，再印簡化文本當輔助）。這條規則對所有學習單都適用；差異化學習單的產生本身不寫改寫文本。

## 產生（`worksheet_tiered_level`）

流程與閱讀理解題相同（每個層次一次呼叫，依 Bloom 順序一個接一個，再一次判斷層次），差別：

- prompt key `worksheet_tiered_level`（第 1 版），鷹架 `SC-WS-TIERED`，`WsJob.phase` 例如「正在出『分析』層次的題目與三個版本的支持」。`max_tokens` = 800 + 900 × 題數（2500–8000）。
- 輸入多了學習單的學習目標（`content.objectives` 的中英文，`{{objectives}}`）。
- 輸出 schema（欄位照撰寫順序，字串與陣列都有上限）：

  ```
  { items: [正好 n 題] {
      answer_format（enum，開放式）, prompt ≤500, evidence ≤300, paragraph_ids（enum，≤3）,
      advanced: Version, standard: Version, basic: Version },   // 先寫進階版的完整答案
    steps: [2–5 × ≤150], example ≤300,
    linking_words: { function ≤40, basic ≤4×30, standard_more ≤4×30, advanced_more ≤4×30 } }
  Version = { support_kind（該版本可用的 enum）, model_sentence ≤300, support ≤4×200,
              word_bank ≤10×40, answer ≤400, scoring_zh ≤150 }
  ```

- **檢查**（`worksheet::checks::evaluate_tiered_level`）：題目本身照閱讀理解題的檢查（作答方式、題幹不含答案〔用進階版的答案比〕、記憶與理解的逐字依據…）；再檢查三個版本：
  - 形式是該版本可用的，而且由基礎版到進階版嚴格遞減 → 否則拿掉這一題。
  - 指出答案位置的那一行支持拿掉（並要求修復）。
  - 克漏字／句型框架要有 `___`、範句與字庫；字庫裡不在範句的字詞拿掉；句首要有一行；`none` 的支持清空 → 不合格的拿掉這一題。
  - 每個版本要有答案；一般版與進階版的答案不是完整句（簡答題、段落寫作）或沒有評分說明 → 要求修復，題目保留。
  - 一般版、進階版的支持寫出了完整答案 → 拿掉這一題。
  - 步驟少於 2 個 → 要求修復，之後仍不夠時由程式寫步驟；寫出答案的步驟或範例拿掉。轉折詞依版本累加，基礎版沒有時要求修復。
  - 沒通過 → 修復一次；還是沒通過的題目拿掉（缺一個版本就不是差異化的題目）。
- **判斷層次**：每一題只判斷一次（三版題目相同，Bloom 分布也相同）；重出用同一個 `worksheet_tiered_level` 呼叫。
- **組裝**：每個層次一個區塊；題目 `levels` = 三個版本、`tiers` = 三個版本、`hint = ""`；區塊 `instructions = ""`，作答步驟是程序鷹架（`steps`，三個版本都印，`auto = false`；模型沒給時程式寫，`auto = true`）；範例是「作答示例」鷹架（基礎版、一般版）；轉折詞見上。`content.chosen.reasons` 加分層說明：「差異化學習單：三個版本的學習目標、題目與 Bloom 分布相同，只差在支持的多寡。…」。
- 單一區塊重新產生照舊（重跑 `worksheet_tiered_level`）。
- **修改學習單（`revise`）**：新加的題目出現在三個版本；改過的題目只有在題目文字沒變時保留各版本的支持，題目變了就沒有 `tiers`（三版印同樣的題目、沒有支持），可以重新產生那一區。

## 學生版只印 A／B／C（T0、T9）

```ts
interface WsSheet { /* …既有欄位… */
  version_code: string | null;   // 這個版本的中性代號 "A"／"B"／"C"（依學習單版本的順序）；只有一個版本時 null
  tiering_note?: string;          // 只有差異化學習單的教師版：分層說明
}
interface WsSheetItem { /* …既有欄位… */
  support?: { kind: WsSupportKind; heading: string; lines: string[]; word_bank: string[] };  // 這個版本的支持；字庫依字母排序
  scoring?: string;               // 只有教師版
}
interface WsSheetBlock { /* …既有欄位… */
  original_text?: string[];       // 這個版本印了簡化文本時，同一區的原文段落
}
```

- **學生版**（預覽、DOCX、PDF、檔名、教學套件標題）只印中性代號，不印基礎／一般／進階：
  - DOCX／PDF 的姓名欄後面印「Version A」；PDF 頁尾「Version A · 1/2」（只有一個版本時頁尾只有頁碼）。用「Version」是為了和區塊字母（「A. Find the Facts」、舊的「Part A · …」）分開。
  - 檔名 `<檔名>_A版_學生版.docx`（只有一個版本時 `<檔名>_學生版.docx`）。
  - 題目下方印這個版本的支持：「Sentence frame: …」「Fill in the blanks: …」「Start like this: …」，字庫印「Word bank: a  /  b」。
- **教師版**保留程度名稱：橫幅「Teacher reference · Basic version (students' sheet: Version A) · …」、PDF 頁尾「Basic (Version A) · 1/2」、檔名 `<檔名>_基礎版_教師參考版.docx`；每題印這個版本的「Answer: …」「Scoring: …」，橫幅下印分層說明。
- 代號的順序存在學習單內容的 `student_order`（`Content.student_order`）：學習單**第一次產生時隨機排**（例如基礎版＝B、一般版＝C、進階版＝A），重新產生時沿用，所以學生不會從「A」猜出哪一版支持最多（T9）。教師版、教學套件與預覽照這個順序印對應（「教師版（基礎版＝B 版）」）。沒有 `student_order` 的舊學習單（或它沒有剛好列出學習單的版本時）照版本在學習單的順序：基礎版＝A、一般版＝B、進階版＝C。這個規則適用於所有有兩個以上版本的學習單（完整學習單、照我的學習單做也是）。
- **字詞表放在學習單最後**：內容的 `vocabulary_at_end`（`true` 時才有這個鍵；預設各區各印各的）。為 `true` 時，投影（`GET …/sheet`、DOCX、PDF、教學套件、詞彙級別檢查）把各區的字詞表合成一張（同一個字只列一次）放在 `end_words`，印在最後一區之後（標題「Key words」），各區的 `vocabulary` 是空的；區塊內容裡的 `vocabulary` 不動（編輯、重新產生照舊）。重新產生整份學習單時沿用這個選擇。只是內容的一個欄位，老師勾選後和其他編輯一樣由 `PUT /api/worksheets/:id` 存成新版本。
- **教學套件**：學生部分的標題「學生版（A 版）」「學生版（B 版）」「學生版（C 版）」；差異化學習單的教師部分是每個版本一份教師版，標題「教師版（基礎版＝A 版）」…（各版本的支持、答案與評分說明不同）。改版前的分層練習照舊印一份合併的教師版與舊的對應說明。
- 編輯器與備課對話的預覽：學生版標示「Version A」，預覽的名稱是「A 版 學生版」；分頁（基礎版／一般版／進階版）與教師版照舊用程度名稱。

## 閱讀理解題的分步作答指示（R5）

- `worksheet_bloom_level` **第 2 版**（第 1 版保留）：不再寫一句 `instructions`，改寫 `steps`（2–5 個，依學生實際操作的順序，每個步驟一個陣列元素）與選填的 `example`（用不在學習單上的問題示範或部分完成的範例）；提示不可指出答案位置。
- 輸出 schema：`{ items: [正好 n 題] {…同第 1 版…}, steps: [2–5 × ≤150], example ≤300 }`。
- 區塊：`instructions = ""`；步驟是程序鷹架（和完整學習單同樣印成「Step 1: …」）；範例是「作答示例」鷹架（學習單的所有版本）。步驟少於 2 個 → 修復一次，之後仍不夠時由程式寫步驟。寫出答案的步驟或範例拿掉。
- 讀到第 1 版的輸出（只有 `instructions`）時照舊：那一句放在 `instructions`，步驟由程式寫。

## 舊資料

- 不需要資料 migration。改版前存的分層練習：題目沒有 `tiers`（讀成 `[]`），照舊依 `levels` 分到各版本、基礎版印 `hint`；學生版一樣只印 A／B／C；教學套件照舊印合併的教師版。
- 改版前存的閱讀理解題：區塊照舊印一句 `instructions`。

## 測試用假模型

`worksheet_tiered_level`：題目同閱讀理解題的假模型；三個版本依序輪流用（基礎版、一般版、進階版）＝（句型框架、句首、不給）、（克漏字、字庫、句首）、（字庫、句型框架、不給）；三個步驟、一個範例、轉折詞（記憶層次沒有）。`worksheet_bloom_level` 的假模型改成第 2 版的形狀（`steps`、`example`）。

| 標記（寫在教材裡） | 效果 |
|---|---|
| `FAKE_TIER_LOCATION` | 每一次呼叫：第一題的基礎版支持多一行「Look at paragraph 1 again.」→ 要求修復，那一行被拿掉 |
| `FAKE_TIER_FLAT` | 第 1 次：第一題的一般版和基礎版一樣多支持 → 修復呼叫改正 |
| `FAKE_BLOCK` | 只用表外的作答方式 → 區塊留空並附說明 |

## 試用真模型

prompt key：`worksheet_tiered_level`（第 1 版）、`worksheet_bloom_level`（第 2 版）、`worksheet_bloom_check`（第 1 版，不變）。程式：`worksheet::tiered`（形式、代號、schema）、`worksheet::checks::evaluate_tiered_level`、`worksheet::assemble::project`（三個版本的投影）。

---

# 圖像組織（Graphic organizers）

作答方式是「圖像組織」的題目可以帶一張**由程式畫的圖表**（不是 AI 產圖）：模型只選類型、寫標題與每一格的參考答案；伺服器檢查後在編輯器預覽、DOCX、PDF 用同一份投影畫出來。學生拿到空白或部分預填的圖表，教師版印出參考答案。依據：docs/SIOP_DESIGN_RULES.md Q2（教學鷹架：圖表組織）、A4（應用活動：完成圖表組織）、T3（基礎版支持最多）。Migration：`0029_graphic_organizers.sql`。

## 八種類型與版面

| `type` | 名稱 | 版面（family） | sections | 每格 cells |
|---|---|---|---|---|
| `t_chart` | T 字表 | columns：有標題的欄，一列一格 | 正好 2 | ≤ 6 |
| `kwl` | KWL 表 | columns | 正好 3（What I Know／What I Want to Know／What I Learned） | ≤ 6 |
| `venn` | 文氏圖 | venn：兩個交疊的圓（只屬於 A／兩者都有／只屬於 B） | 正好 3 | ≤ 5 |
| `sequence` | 順序鏈 | chain：編號方框加箭頭，一列三個 | 3–6 | ≤ 2 |
| `cause_effect` | 因果鏈 | chain | 2–6 | ≤ 2 |
| `problem_solution` | 問題與解決 | chain | 2–4 | ≤ 2 |
| `story_map` | 故事地圖 | rows：左欄標題、右欄作答（Setting／Characters／Problem／Events／Solution） | 正好 5 | ≤ 4 |
| `mind_map` | 心智圖 | web：中心詞在中間，分支在上下 | 3–6 | ≤ 3 |

選這八種：它們是 SIOP 教學鷹架與應用活動提到、高中英文閱讀學習單實際會用的圖表（比較、排序、因果、問題解決、記敘結構、讀前讀後、概念網）。魚骨圖、有分支的流程圖、有日期的時間軸、矩陣沒有收：需要自由版面或兩個以上的維度，表格類的需求已有「表格整理」。

## 資料

```ts
interface WsItem {
  // …
  organizer?: WsOrganizer | null;   // 只在 answer_format 是「圖像組織」時印出；舊題目沒有這個鍵 → 印空白方框
}
interface WsOrganizer {
  type: WsOrganizerType;            // 上表八種之一（存檔時不是這八種 → 400）
  title: string;                    // 印在圖表上方；"" 不印（≤ 200 字）
  center: string;                   // 只有 mind_map 用（≤ 100 字）
  sections: Array<{                 // ≤ 6
    label: string;                  // 欄標題、方框標題或分支（≤ 100 字）
    cells: Array<{                  // ≤ 6
      answer: string;               // 參考答案，只在教師版（≤ 300 字）
      given: string;                // 預填時學生版印的文字（≤ 300 字）
      prefilled: WsLevel[];         // 學生版印 given 的版本；[] 不預填。伺服器排成版本順序
    }>;
  }>;
}
```

- `PUT /api/worksheets/:id` 檢查上面的上限，超過回 400（「一個圖像組織最多 6 格。」等）。存檔不檢查各類型的最少格數（老師可以暫時刪到少於最少），也不調整格數；格數依類型調整只發生在模型的輸出與編輯器換類型時。
- 作答方式換成別的時 `organizer` 留著但不印（換回圖像組織就回來）。

## 投影（`GET …/sheet`、匯出）

`items[].organizer`（只有印出圖表的題目有）：

```ts
{ type, name /* "T-chart"… */, title, center,
  sections: [{ label, cells: [{ given: string /* 預填的字，空白格 "" */, answer?: string /* 只有教師版，空白格的參考答案 */ }] }] }
```

- 學生版：這個版本預填的格子印 `given`，其他格子空白（作答線或空格）；沒有 `answer`。
- 教師版：預填的格子一樣印 `given`；空白格印 `answer`（教師用的金色）。
- 有圖表的題目 `lines = 0`（圖表取代作答線）。題目層級的 `answer`（評分重點）照舊印在圖表下方。
- 沒有 `organizer` 的圖像組織題目（舊學習單）照舊印一個空白方框。

**DOCX**：Word 原生表格（固定欄寬，可以在 Word 裡改）。columns 與 rows 是表格；chain 是「方框｜→｜方框｜→｜方框」的表格，方框標題前加編號；**venn 用三欄表格近似**（A｜Both｜B，欄標題加底色），因為 Word 的圖形加文字在不同版本的 Word 裡位置不穩、也不好編輯；mind_map 是 3 欄的表格，中心格加底色，分支在上下兩列，中間的列放 ↖ ↑ ↗／↙ ↓ ↘ 箭頭。
**PDF**：真的畫線：columns／rows 是格線（可以在列之間換頁），chain 是方框加箭頭，venn 是兩個橢圓、三區文字，mind_map 是方框與從中心拉出的線。標題和圖表的第一部分一起換頁。
**編輯器與對話頁預覽**：同樣的版面（CSS grid＋SVG）；手機寬度時順序鏈改成直排、箭頭朝下。

## 模型輸出

四個提示詞的 schema 多一個 `organizer`，放在 `options` 後面、`answer` 前面；其他作答方式的題目填 `null`（`anyOf: [organizer, null]`）：

| prompt key | 新版本 | schema 何時有 `organizer` |
|---|---|---|
| `worksheet_tasks` | 4 | 一律（模型可以選任何作答方式） |
| `worksheet_bloom_level` | 3 | 這個層次能用的作答方式有圖像組織時 |
| `worksheet_tiered_level` | 2 | 同上；格子用 `prefill_in` |
| `worksheet_template_section` | 3 | 版型這一區的作答方式有圖像組織（或沒有指定）時 |

（版本號是「目前最新版 + 1」，上表是 main 上的情形。新版本＝前一版全文＋一段圖像組織的規則。）

```
organizer: { type: enum(八種), title: ≤80, center: ≤60,
             sections: [2–6] { label: ≤40, cells: [1–6] { answer: ≤120, prefilled: boolean } } }
差異化學習單的 cell：{ answer: ≤120, prefill_in: "none" | "basic" | "basic_standard" | "all" }
```

伺服器的整理（都不會多花一次修正呼叫，記在 `check_results.repairs`，檢查名 `organizer_wellformed`）：

- 不在八種裡的 `type` → 改成容得下輸出的類型：有 `center` → mind_map；2 格以下 → t_chart；其他 → sequence。
- 格數超過類型的上限 → 刪掉後面的；不到下限 → 補空的格子；格子裡的空格超過上限 → 刪掉後面的；沒有標題的格子用類型的預設標題；`center` 只留給 mind_map。
- 每一段文字壓成一行、去掉 Markdown、截到上限。
- 預填的格子印它的答案（`given = answer`）；答案是空的格子不預填。`prefilled: true` → 三個版本都預填；`prefill_in` → 對應的版本（一定包含基礎版，所以基礎版的預填最多）。
- 每個版本至少留一格給學生寫：某個版本每一格都預填時，預填版本最少的那一格（同樣少時取最後一格）改成不預填。
- 不是圖像組織的題目帶了 `organizer` → 拿掉。圖像組織的題目沒有 `organizer` → 照舊印空白方框。
- 完整學習單（`worksheet_tasks`）的圖表含中文 → `english_only` 不通過（會修正一次），拿掉圖表印成方框。照版型的區塊依版型的指示語言，不檢查。
- 修改學習單（`worksheet_block_revise`、`revise`）的 schema 沒有 `organizer`：題目保留原本的圖表。

## 編輯器

- 圖像組織的題目在題目下方畫出圖表，標題、中心詞、每一格的標題與空格都直接在圖上改；「圖表類型」可以換（老師寫的標題留著，還是預設標題的跟著換成新類型的預設，格數依新類型調整）；「加一格」「刪除這一格」「＋空格」「×」在類型的範圍內。
- 學生版的檢視改預填的文字，每一格有「預填」勾選：差異化學習單的題目只改目前的版本，其他題目改全部版本；勾起來時空的預填文字先填入參考答案。教師版的檢視改參考答案。
- 作答方式換成圖像組織、題目還沒有圖表時，自動建立一張 T 字表（Topic A／Topic B，可以改）。舊的圖像組織題目顯示「這一題印成空白方框。改成圖表」。

## SIOP 自我檢核

`render_worksheet` 在圖像組織題目下多一行：`圖像組織：因果鏈（cause_effect）｜標題：…｜Cause：It rained.〔預填：基礎版〕｜Effect：…`，第 4 項（補充教材）與第 14 項（鷹架）看得到類型、標題與預填的格子。

## 測試用假模型

每一題圖像組織依題號輪流用八種類型（`fake_organizer`）：標題依類型、答案取自段落的字、第一格預填（差異化學習單：第一格三版都預填，第二格只有基礎版）。`FAKE_ORG_BAD`：完整學習單與閱讀理解題的圖表類型是 `fishbone`、8 格各 3 個空格、全部預填（伺服器改成順序鏈、6 格各 2 個空格、留一格給學生寫）。

## 試用真模型

gemma 可能：把 `organizer` 寫給不是圖像組織的題目（會被拿掉）、圖像組織題目給 `null`（印方框）、把好幾格寫在同一個字串、預填太多格（程式留一格）、選和題目不搭的類型（例如說明文用 story_map）。看 `ai_interactions` 的 `check_results.repairs` 中 `organizer_wellformed` 的紀錄。

# Phase 2.8：照我的學習單做

以下為**新增**；既有端點只多了選填欄位與回應欄位。Migration `0016_mimic.sql`。

老師上傳一份自己以前的學習單（範本），Manna 讀出它的設計（就是「我的版型」，Phase 2.5），再照同樣的樣式為這節課的課文出題。以前要先到 教材庫 → 我的版型 做好版型才能用；現在建立備課對話時直接給範本，一步完成。範本誤放在課文的位置時，Manna 也認得出來。照版型做的學習單，每一題也標上 Bloom 層次。

## 教材看起來是不是學習單（`looks_like_worksheet`）

`Material`（教材庫清單、上傳與貼上的回應、`GET /api/materials/:id`）多一個欄位：

```ts
interface Material { /* …既有欄位… */ looks_like_worksheet: boolean; }
```

- 由程式依文字計分，不呼叫模型（`materials::detect`）。**分數 ≥ 3 就是學習單**：

  | 分數 | 條件 |
  |---|---|
  | +2 | 至少 3 個作答空格：連續 3 個以上底線（`___`、全形 `＿＿＿`）或空的作答括號（`（　）`、`( )`、`()`） |
  | +1 | 表頭欄位：`Name`、`Class`、`No.`、`Date`（首字大寫或全大寫）或 `姓名`、`班級`、`座號`，後面接冒號（`:`／`：`）、底線或行尾 |
  | +1 | 至少 4 行以題號開頭：`1.` `2)` `(3)` `（4）` `A.`…`H)`（前面可以有作答括號，例如 `(  ) 1.`；`3.5 million` 這種小數不算） |
  | +1 | 任務或段落詞：`Task`、`Part`、`Activity` 後面接數字、A–H、羅馬數字、冒號或行尾；`Worksheet`、`Exit ticket`、`Warm-up`（不分大小寫）；`學習單`、`練習` |
  | +1 | 至少 25% 的非空白行有問號（`?`／`？`） |

- 什麼時候算：貼上、修改貼上的文字、上傳（用擷取出來的文字）時計算並存在 `materials.looks_like_worksheet`；網址教材是 `false`。這個欄位在 0016 之前的教材是空的，**第一次讀取時**（清單或單一教材）用同一套規則算好並存起來（`updated_at` 不變）；migration 不另外寫一份 SQL 版本的規則。
- 前端用它提示「這份看起來是一份學習單。要照它的樣式為新課文出題嗎？」；**不會**擋住把它當課文產生。

## 從範本建立備課對話

`POST /api/prep-sessions` 新增 `template_material_id?: number`：老師的學習單範本，教材庫裡自己的、有文字的教材。

- 只能和 `kind: "worksheet"` 一起用，否則 400「範本只適用於完整學習單」。同時給了 `template_id` 時以 `template_id` 為準，`template_material_id` 完全不看（也不檢查）。
- 範本的檢查和 `POST /api/worksheet-templates` 相同（400）：不是自己的或不存在 →「找不到這份教材。」；圖片、影音、網址 →「這份教材沒有文字可以讀…」；讀不到文字 →「這份檔案讀不到文字（可能是掃描檔）…」；超過 30000 字 → 400。任何一項不合格，整個請求 400，什麼都不建立。
- **版型**：沿用老師由這份教材做出的最新一個 `ready` 版型；沒有的話，沿用一個還在讀取（`extracting`）的（同一份範本不會同時讀兩次）；都沒有就建立一個新的版型，和 `POST /api/worksheet-templates` 同樣的新增與抽取（標題是教材標題＋「版型」，記 `worksheet_template_created`，多了 `prep_session_id`）。讀取失敗的版型不沿用。這個版型就是對話的 `template`（`PrepSession.template` 照舊回傳）。
- **第一輪**：版型還在讀取時，這一輪是 `running`、沒有學習單（`worksheet: null`），等版型讀好才建立學習單並照版型產生（和用 `template_id` 時一樣）；之後照舊，學習單產生完就是 `done`。版型已經 `ready` 時，學習單立刻建立。
- **讀取失敗**：這一輪 `failed`，`error` 就是版型的 `error`（中文）；版型維持 `failed`，可以在版型頁按「重試」。在對話裡按「再試一次」（同樣 `kind: "worksheet"` 的新一輪）也會重新讀取這個版型（等同 `POST …/retry`，記 `worksheet_template_retried`，多了 `prep_session_id`），新的一輪同樣等它讀好；原本的教材已刪除時，這個請求 400「原本的學習單檔案已經刪除，無法重試；請重新上傳。」。
- 等待中的版型被刪除時，這一輪 `failed`（「這次要照著做的版型已經刪除了…」）。伺服器重啟會中斷讀取：版型照 Phase 2.5 的規則改成 `failed`，這一輪跟著 `failed`。
- 201 的回應在讀取開始之前讀出，所以新版型的第一輪一定是 `running`、`template.status = "extracting"`。

```ts
interface PrepTurn {
  /* …既有欄位… */
  template: { id: number; title: string; status: "extracting" | "ready" | "failed" } | null;
}
```

- `template`：`worksheet` 輪次照著做的版型（這一輪開始時對話的版型）與它**現在**的狀態；其他輪次、沒有用版型、或版型已刪除時是 `null`。`status = "extracting"` 時 `worksheet` 一定是 `null`。資料存在 `prep_turns.template_id`（刪除版型時變成 null；0016 之前做的輪次依學習單的 `template_id` 補上）。
- `prep_session_created` 多 `template_material_id`。按鈕或工具名稱「照我的學習單做」也不算「教師的要求」。

### `entry`（研究紀錄用）

`POST /api/prep-sessions` 可帶 `entry?: "home" | "create"`：哪一頁開始這個對話（首頁的快速框、或 `/create/:tool`）。只寫進 `prep_session_created` 的 `entry`，並固定帶 `ui_version: 4`（首頁三張卡片：「AI 幫我設計」「照我的範本設計」「我想逐步設計」；`3` 是只有前兩張卡片的首頁，`2` 是導覽改版後六張卡片的首頁）。其他值（別的字串、別的型別）一律忽略，記成 `null`，不是錯誤；舊的客戶端不帶 `entry` 也照常。沒有資料表變動。

## 照版型產生：每一題的 Bloom 層次

`worksheet_template_section` **第 2 版**（第 1 版保留）：

- 每一題多一個 `bloom`（schema 是六個 id 的 enum，放在 `answer` 之後），規則文字和 `worksheet_tasks` 第 2 版的第 13 條相同。題目保留它（`WsItem.bloom`）；表外的值丟掉（`null`）。教師版與匯出照 Phase 2.7 印出 Bloom 標記與區塊的 `bloom_summary`，學生版沒有。
- `scaffold` 改成**字串陣列**（每行一項，≤6 行、每行 ≤200 字）：gemma 想換行時會直接結束 JSON 字串。伺服器把各行用換行接起來，照舊變成一個鷹架（程序鷹架依行拆成 `steps`）；讀到舊的字串格式也接受。
- prompt 另外寫清楚各作答方式的格式（配對題的左欄寫在同一行、右欄放 `options`；是非題的 `options` 是 True／False），並要求其他文字欄位不換行。schema 的每個字串與陣列都有上限，欄位照撰寫順序。
- 假模型：每一題都有層次（段落寫作是 `create`，其他依序 `remember`、`understand`、`analyze`），`scaffold` 是陣列。新標記 `FAKE_TPL_BAD_BLOOM`（寫在教材）：每個區塊第一題的層次不在六個之內 → 伺服器丟掉。

## 課文位置放了學習單時的訊息

教材不再因為段數超過 40 而被拒絕（見「長教材」），所以這則附加訊息只剩「教材沒有段落」的拒絕可以接在後面（實際上不會發生：沒有段落就沒有可偵測的文字）。教材看起來像學習單時，網頁在產生前就問（`looks_like_worksheet`，「照這份的樣式做」／「不用，當成課文」）。

## 其他

- `prep_turns` 的「學習單產生完就是 `done`」改成「學習單的產生**開始過**而且結束」：學習單剛建立、產生還沒開始的那一瞬間，不會被輪詢誤判成 `done`（開始失敗時由開始的程式把這一輪設成 `failed`）。
- 資料：`materials.looks_like_worksheet`（BOOLEAN，0016 之前的列第一次讀取時補上）、`prep_turns.template_id`、`prompt_versions` 新增 `worksheet_template_section` 第 2 版。
- 程式進入點：偵測 `materials::detect::looks_like_worksheet`；對話等版型 `prep::run::start`（`spawn_after_template`）；schema `worksheet::template::section_out_schema`。

**抽取提示詞 `worksheet_template_details` v2**：逐段（`per_paragraph = true`）的區塊，`item_count` 是「每一段」的題數，不是全部題數。真模型實測時，一份「每段各問一題、共 4 題」的範本被讀成每段 4 題，套到 5 段的新課文就出了 20 題。

---

# Phase 2.9：教材參考、我的教材、學習單範本

以下為**新增或變更**；既有端點只多了選填欄位與回應欄位。Migration `0017_library_areas.sql`。

教材與學習單分開：平台有兩個參考區「平台教材」（知識庫的課綱、詞彙表、歷屆學測／統測英文試題）與「學習單範本」；教師自己上傳的是「我的教材」，自己的學習單範例移到「我的範本」。挑選教材或範本用一個像商店的視窗：搜尋框、有數量的分類籤、篩選籤、卡片格。管理員（`pi`、`kb_manager`）上傳與管理平台的兩個區。

**這一版沒有**：把考卷切成一篇篇文章（OCR）、自動匯入範例、推薦。分類與標籤先放好。

## 資料

- `materials` 新增：
  - `scope`：`personal`（只有擁有者，和以前一樣）｜`platform`（所有教師可讀可用，只有 ManageKnowledge 能改或刪）。
  - `purpose`：`material`（教材）｜`worksheet_sample`（學習單範例，版型的來源）。範例不會出現在教材清單。
  - `category`：平台教材用固定清單（伺服器檢查）：
    - 平台教材（`purpose = material`）：學測英文、統測英文、課綱、詞彙表、考試說明、其他。
    - 範例與範本（`purpose = worksheet_sample`）：字彙、文法句型、閱讀理解、課文講義、自學講義、寫作、聽說、學習反思、其他。
    - 個人的教材通常是空字串。
  - `tags TEXT[]`：自由標籤（例如年度 `114`、`試題`、`參考答案`、`三民`）。每個 ≤20 字、每列最多 12 個；前後空白去掉、重複的去掉（保留第一次的寫法與順序）。超過 → 400。
  - **補資料**：當過版型 `source_material_id` 的教材，`purpose` 改成 `worksheet_sample`。
- `worksheet_templates` 新增 `scope`、`category`（範本清單；可以是空字串）、`tags`。平台範本來自一份平台的範例教材。
- 平台的列保留上傳的管理員為 `teacher_id`（稽核用）。管理員自己的「我的教材」不包含她上傳的平台教材，個人額度也不算。

## 存取

- **平台的列**：每位登入的教師都可以**讀**（清單、詳細、文字、原檔 `/materials/:id/file` 與 `file-link`、`/worksheet-templates/:id`），也可以**用**（學習單與備課對話的 `material_id`／`template_id`、`template_material_id`、`POST /api/worksheet-templates { material_id }`）。改與刪只有 ManageKnowledge：其他人經由 `PUT`／`DELETE /api/materials/:id`、`PUT`／`PATCH`／`DELETE /api/worksheet-templates/:id`、`POST …/retry` 都是 **403**（版型的訊息請教師「另存成我的範本」）。ManageKnowledge 經由這些教師端點也可以改（和管理端點同一套規則）。
- **個人的列**：只有擁有者，和以前一樣；別人的 id → 404。
- **課例連結與課例 SOP 第 1 步**仍然只接受自己的個人教材（平台教材 → 404）。
- **用平台教材做學習單**照常留下教材快照：之後平台教材被刪，已經做好的學習單不受影響。平台範本被刪，學習單的 `template` 變成 null，內容保留。
- **註記是個人的**：在平台教材上，每位教師只看得到、只能改刪自己的註記；`annotation_count`、`GET …/annotations`、`GET …/annotations/ai` 與送給 AI 的註記都只算這位教師自己的（`material_annotations.teacher_id`）。教材詳細的 `lessons`、`worksheet_count` 同樣只算自己的。平台教材被刪時，所有人的註記一起刪除。

## 範例離開「我的教材」

- 用個人教材做出版型（任何路徑：`POST /api/worksheet-templates`、備課對話的 `template_material_id`、照我的學習單做的上傳），那份教材的 `purpose` 就變成 `worksheet_sample`（`updated_at` 不變）。平台教材不會被改。
- 教師上傳與貼上可以帶 `purpose`（上傳是 multipart 欄位，貼上與網址是 JSON 欄位；預設 `material`；其他值 → 400）。照我的學習單做的範例上傳送 `worksheet_sample`。
- `GET /api/materials` 預設只列 `purpose = material` 的個人教材；`?purpose=worksheet_sample` 列自己的範例；`?purpose=material` 同預設。有 `lesson_id` 而沒有 `purpose` 時，列出連到該課例的所有教材（不分用途）。`?purpose=worksheet_sample&orphaned=true` 只列沒有任何範本在用的範例（見「我的範本：刪除範本連同原檔」）。
- 備課對話「找教學資源」比對的「自己的教材」只看「我的教材」（個人、`purpose = material`）。

## 型別

```ts
// 既有型別多的欄位
interface Material { /* …既有欄位… */
  scope: "personal" | "platform"; purpose: "material" | "worksheet_sample";
  category: string; tags: string[];
}
interface WsTemplateSummary { /* …既有欄位… */ scope: "personal" | "platform"; category: string; tags: string[]; }

interface MaterialCard {
  id: number; title: string; scope: "personal" | "platform"; category: string; tags: string[];
  kind: string;
  file_type: "pdf" | "docx" | "text" | "image" | "audio" | "video" | "link";  // .txt 上傳與貼上的文字都是 text
  page_count: number | null; paragraph_count: number; word_count: number;
  excerpt: string;            // 文字的前 280 字，空白合併成一個空格（沒有文字時是 ""）
  text_status: string; looks_like_worksheet: boolean; created_at: string;
  owner_label: "Manna 知識庫" | "我";
  uploaded_by?: string;       // 只有管理清單：上傳者的研究編號
  size_bytes?: number;        // 只有管理清單
}
interface TemplateCard {
  id: number; title: string; scope: "personal" | "platform"; status: "extracting" | "ready" | "failed";
  category: string; tags: string[];
  worksheet_kind: string; instruction_language: string;   // 還沒有版本時是 ""
  section_count: number; section_titles: string[];        // 前 6 個 title_pattern
  source_material: { id: number; title: string; file_type: string; page_count: number | null } | null;
  updated_at: string; owner_label: "Manna 範本" | "我";
  uploaded_by?: string; size_bytes?: number;               // 只有管理清單（size_bytes 是範例原檔的大小）
  error?: string | null;                                   // 只有管理清單：讀取失敗的原因
}
interface Facet { key: string; count: number }
interface BrowsePage<C, F> { items: C[]; next_cursor: string | null; facets: F; total: number }
type MaterialBrowse = BrowsePage<MaterialCard, { categories: Facet[]; tags: Facet[]; file_types: Facet[] }>;
type TemplateBrowse = BrowsePage<TemplateCard, { categories: Facet[]; tags: Facet[]; langs: Facet[] }>;
```

## 瀏覽（登入即可）

| Method | Path | 查詢參數 | 回應 |
|---|---|---|---|
| GET | `/api/browse/materials` | `area=reference\|mine`（預設 `reference`）、`q`、`category`、`tag`（可重複，全部都要符合）、`file_type`、`fits_lesson=true`、`sort=new\|title`（預設 `new`）、`cursor`、`limit`（預設 24，最多 60） | `MaterialBrowse` |
| GET | `/api/browse/templates` | `area=platform\|mine`（預設 `platform`）、`q`、`category`、`tag`、`lang=英文\|中文\|中英並用`、`sort`、`cursor`、`limit` | `TemplateBrowse` |

- 列：`reference` = 平台 + `material`；`mine` = 自己的個人 + `material`。`platform` = 平台範本中 `ready` 的；`mine` = 自己的範本（任何狀態）。
- `fits_lesson=true`：有文字（`text_status = ready`）且至少 1 段。段數不設上限（以前是 1–40 段）；網頁的篩選名稱是「有文字、可直接出題」。
- **搜尋** `q`（最多 200 字）：不分大小寫的子字串。教材比對標題、`source_note`、標籤與全文；範本比對標題、分類、標籤、`worksheet_kind` 與範例教材的標題。
- **facets** 只依 `area` 與 `q` 計算（不看其他篩選），所以每個籤顯示選它會有幾筆。分類依上面的固定清單順序（再來是其他值；空字串不列）；標籤依數量多到少（最多 40 個）；`file_types` 依 pdf、docx、text、image、audio、video、link；`langs` 依 英文、中文、中英並用。只列數量大於 0 的。
- `total`：符合 `area`、`q` 與所有篩選的總數。
- **cursor**：穩定的 keyset（`new` 依 `created_at, id` 由新到舊；`title` 依 `title, id`）。不透明字串，照 `next_cursor` 原樣送回；換了 `sort` 的 cursor → 400。
- 參數值不合（`area`、`file_type`、`lang`、`sort`、`limit` 不是數字、`fits_lesson` 不是 true/false、cursor 讀不懂）→ 400。

## 版型

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/worksheet-templates` | 多了選填 `category`（範本清單或空字串）、`tags: string[]` | 同前 |
| PUT／PATCH | `/api/worksheet-templates/:id` | 多了 `category?`（範本清單或 `""`）、`tags?`（陣列或逗號分隔字串）。讀取中也可以改分類與標籤（`title`、`content` 仍是 409） | `WsTemplateDetail`。平台範本 → 403（ManageKnowledge 除外） |
| POST | `/api/worksheet-templates/:id/copy` | – | 201 `WsTemplateDetail`：「另存成我的範本」。自己看得到的 `ready` 版型（平台的或自己的）複製成個人版型：`ready`、第 1 版是目前的內容（`origin = "teacher"`）、標題「<原標題>（我的版本）」、沿用範例教材（看原檔）、分類與標籤。還沒有內容（讀取中或失敗）→ 409；別人的個人版型 → 404 |

`GET /api/worksheet-templates`（清單）只列自己的個人版型。

**分類建議**：版型的分類是空的時，抽版型完成時由伺服器依 `worksheet_kind`（找不到再看標題）的關鍵字填上：單字／字彙／vocab → 字彙；文法／句型／grammar／sentence → 文法句型；閱讀／reading → 閱讀理解；自學 → 自學講義；課文／講義 → 課文講義；寫作／writing → 寫作；反思 → 學習反思；其他 → 其他（不分大小寫；「自學」比「講義」先比，「自學講義」才不會變成課文講義）。教師或管理員選了分類就不會被改。0017 之前的版型維持空字串。

## 管理（`pi` 或 `kb_manager`，`manage_knowledge`；其他人 403）

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/admin/library` | `kind=materials\|templates`（預設 materials）、`q`、`category`，另可用瀏覽的 `tag`、`file_type`、`lang`、`sort`、`cursor`、`limit` | `MaterialBrowse` 或 `TemplateBrowse`，卡片多 `uploaded_by`、`size_bytes`（範本另有 `error`）。教材是所有平台教材（不含範例）；範本是所有平台範本（任何狀態） |
| POST | `/api/admin/library` | multipart：`file`（或 `text`，可配 `title`）、`title?`、`category`（平台教材清單，必填）、`tags?`（逗號分隔，`,`、`，`、`、` 都可以）、`source_note?` | `MaterialCard`（含管理欄位）。平台教材，`purpose = material`；擷取和教師上傳相同 |
| POST | `/api/admin/library/templates` | multipart：`file`（或 `text`）、`title?`、`category?`（範本清單；空的 = 讀完後建議）、`tags?` | 202 `TemplateCard`（`status: "extracting"`，回應在抽取開始之前讀出）。建立平台範例教材（`purpose = worksheet_sample`）與平台範本，抽版型和 `POST /api/worksheet-templates` 相同 |
| PATCH | `/api/admin/library/:id` | `{ title?, category?, tags? }`（`tags` 是陣列或逗號分隔字串） | `MaterialCard`。分類依那份教材的用途檢查；標題不可空白 |
| DELETE | `/api/admin/library/:id` | – | `{ deleted, annotations, lessons }`（同教師刪除，真的刪除）。用它做的版型繼續可用，`source_material` 變成 null |
| PATCH | `/api/admin/library/templates/:id` | `{ title?, category?, tags? }` | `TemplateCard`。分類送 `""` 而版型已有內容時，立刻依內容建議 |
| DELETE | `/api/admin/library/templates/:id` | – | `{ ok: true }`。真的刪除；它的平台範例教材（沒有其他版型在用時）與原檔一起刪 |
| POST | `/api/admin/library/templates/:id/retry` | – | 202 `TemplateCard`（`status: "extracting"`）。只有 `failed` 可以（其他 → 409） |

- 上傳的檢查與教師相同（副檔名、檔頭、單檔大小、同時最多 2 個）。重複的檔案（平台教材裡已有相同 SHA-256）→ 409「這個檔案已經在平台教材庫裡了（…）」；教師個人的教材庫不算重複。
- **容量**：教師的 1 GB／300 份不適用；所有平台原檔合計以 `PLATFORM_LIBRARY_BYTES`（預設 5 GB）為上限，超過 → 400「平台教材庫的空間不夠…」。原檔存在 `platform/` 之下。
- **範本上傳**：檔案要讀得到文字、不超過 30000 字（和 `POST /api/worksheet-templates` 同樣的 400 訊息）；不合格時什麼都不存（檔案與教材列都沒有）。
- 非管理員經過這些端點一律 403（在讀取上傳內容之前）。

## usage_logs 新事件

`worksheet_template_copied`（`template_id`、`from_template_id`、`from_scope`）、`library_material_updated`、`library_template_updated`。`material_created` 多了 `scope`、`purpose`；`material_deleted`、`worksheet_template_deleted` 多了 `scope`；平台範本的 `worksheet_template_created` 多了 `scope`。

## 程式進入點

範圍與分類規則 `materials::catalog`；瀏覽與管理清單的查詢、卡片 `materials::browse`；存取 `materials::store::{readable, owned, writable}` 與 `worksheet::template::store::{readable, writable}`；端點 `routes::browse`、`routes::library_admin`、`routes::templates::copy`。
# Phase 2.10：掃描檔與 OCR

以下為**新增**；既有端點只多了回應欄位與 409。Migration `0018_ocr.sql`。

老師和管理員會上傳掃描的 PDF、手機拍的課本頁面、掃描的舊學習單。以前這些只會得到「讀不到文字（可能是掃描檔）」；有些 PDF 有文字層但內容是亂的（滿是零寬字元、表格被拆散）。現在用國網中心的 `chandra-ocr-2` 逐頁辨識：它回傳**有版面標籤的 HTML**（`<div data-bbox="x0 y0 x1 y1" data-label="Section-Header|Text|List-Group|Table|Picture|Page-Header|Page-Footer|…">`）。

## 設定（環境變數，README「掃描檔文字辨識」）

| 變數 | 說明 |
|---|---|
| `OCR_PROVIDER` | `none`（後端程式沒設時的預設：什麼都不變）、`openai_compatible`（`docker-compose.yml` 也就是正式環境的預設；要關掉就在 Coolify 設 `none` 再重新部署）、`fake`（測試；`AI_PROVIDER=fake` 且沒設這個變數時的預設） |
| `OCR_BASE_URL`、`OCR_API_KEY` | 沒設時沿用 `EMBEDDING_BASE_URL`、`EMBEDDING_API_KEY` |
| `OCR_MODEL` | 預設 `chandra-ocr-2` |
| `OCR_TIMEOUT_SECS` | 每一頁請求的時限，預設 120 |
| `OCR_MAX_PAGES` | 每個檔案最多辨識幾頁，預設 40；其餘略過並註明 |
| `OCR_CONCURRENCY` | 全站同時辨識的頁數（全域 semaphore），預設 2；同時處理的檔案數也以此為上限 |

設定有誤只會關掉 OCR 並在啟動日誌警告。Docker 映像安裝 `poppler-utils`（`pdftoppm`、`pdfinfo`）。

## 什麼時候辨識

上傳後在背景自動辨識（上傳的回應先回來，`ocr.status = "queued"`）：

1. **圖片**（`kind = "image"`：png、jpg、webp、gif 第一格）：一律。
2. **沒有文字層的 PDF**（`text_status = "no_text"`），以及**文字層讀不出來的 PDF**（`failed`：例如 Word 轉出、用 CID 字型的考卷，解析器放棄但頁面照樣能轉成圖；真正加密或損壞的檔案在轉圖時失敗，記為 OCR 失敗）：一律。
3. **文字層看起來壞掉的 PDF**：程式依擷取出的原始逐頁文字判斷（`materials::ocr::poor_text`，不呼叫模型），**任一條件成立**就辨識：
   - 零寬或格式字元（U+200B–U+200F、U+FEFF、U+2060）**超過**全部字元的 2%；
   - 平均每頁可見字元（不含空白、控制與格式字元）**少於** 80；
   - 非空白行中，長度 1–2 字（去掉頭尾空白與格式字元）的行**超過** 40%；
   - 取代字元 U+FFFD **超過** 5 個。
4. **手動**：`POST /api/materials/:id/ocr`（「用 OCR 重新讀取」，文字看起來不對時）。
5. **啟動時補跑（知識庫）**：每次啟動（接手未完成的工作之後），`scope = "platform"`、從沒跑過 OCR（`ocr.status = "none"`）、而且今天上傳會自動辨識的檔案——`no_text`／`failed` 的 PDF 與圖片——各開一次辨識（`materials::ocr::backfill_platform`），例如 OCR 上線前已上傳的知識庫考卷。老師自己的檔案不補跑，用「用 OCR 重新讀取」。辨識過（`done`／`failed`）的不會再跑。

DOCX 與 TXT 從不辨識。`OCR_PROVIDER=none` 時以上都不發生（手動 → 503）。

## 流程

- **PDF → 圖**：`pdftoppm -png -f n -l n -scale-to-x 1400 -scale-to-y -1 -singlefile`，一次一頁，寬 1400 px（A4 約 170 dpi，就是實測讀得乾淨、一頁約 10 秒的大小），放在暫存資料夾，工作結束即刪除。頁數用擷取時數到的，數不到時用 `pdfinfo`。
- **圖片**：解碼（GIF 取第一格）、依 EXIF 方向轉正、長邊縮到 1600 px 以內；不需要轉正也不需要縮小的 PNG／JPEG（≤ 4 MB）原樣送出。PNG／GIF 重新編碼成 PNG，JPG／WebP 成 JPEG（品質 90）。解不開的圖 ≤ 4 MB 時原樣送，否則失敗。
- **請求**：`POST {base}/chat/completions`，`{"model":"chandra-ocr-2","max_tokens":4000,"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,…"}},{"type":"text","text":"OCR this page to markdown."}]}]}`，一頁一個請求，取 `choices[0].message.content`（輸出被 `max_tokens` 截斷時也照用）。逾時、5xx 或連線中斷重試一次；仍失敗的頁記下來，其他頁繼續。
- **HTML → 段落**（`materials::ocr::html`，純函式）：依 chandra 回傳的順序（不依 bbox 重排）；去標籤、解 HTML entity、去零寬字元、合併空白（兩個中日韓字元之間不補空格）。

  | 標籤 | 變成 |
  |---|---|
  | `Section-Header` | 自己一段（區塊保留標籤，所以仍知道它是標題） |
  | `Text`、`Caption`、`Footnote`，以及其他未列出的標籤 | 一段 |
  | `List-Group` | 每個 `<li>` 一段（巢狀項目各自一段，依閱讀順序；`<li>` 外的文字自成一段） |
  | `Table` | 每個 `<tr>` 一段，儲存格以 ` \| ` 相接（空儲存格保留位置，全空的列略過） |
  | `Page-Header`、`Page-Footer`、`Picture`（以及 `Image`、`Figure`） | 不進文字；圖片計數 |

  `List-Group`／`Table` 裡找不到 `<li>`／`<tr>` 時當一般文字。整頁沒有任何有標籤的元素（純 HTML 或 markdown）時，每一行一段（`#` 開頭的行視為標題）。
- **結果**：教材的 `text`、`paragraphs`（每段有 `page`，圖片是第 1 頁）換成辨識結果，`text_status = "ready"`、`text_source = "ocr"`；文字和原本相同的段落沿用原 id，文字有變時 `revision` 加 1。字數上限與上傳相同（300,000 字，超過截斷並 `text_truncated = true`；因頁數上限略過頁時也是 `true`）。`looks_like_worksheet` 依辨識後的文字重算。舊的註記照常重新定位（多半變成「需要重新確認位置」）。
- 原始逐頁區塊存在 `materials.ocr_pages`（不在 API 回應裡，給之後依 `Section-Header` 切分考卷用）：`[{ page, blocks: [{ label, text, bbox: [x0,y0,x1,y1] | null }], pictures, error? }]`。表格與清單的區塊文字一列／一項一行。`pictures` 是該頁圖片數（「此頁有 n 張圖」只放在這裡，不寫進文字）。
- **沒有一頁成功**：`ocr.status = "failed"`，`ocr.error` 說明，教材文字不變。**部分頁失敗或略過**：`done`，`ocr.error` 是附註，例如「第 2 頁沒有辨識成功。」「這份檔案有 52 頁，只辨識了前 40 頁。」。**都讀了但沒有字**：`done`，附註「辨識完成，但沒有找到文字。」，教材文字不變。
- **紀錄**：每一頁的每一次呼叫一筆 `ai_interactions`（`module = "material_ocr"`，重試是 `attempt = 2`、`retry_of`）；`input_snapshot` 只有教材 id、頁碼、圖的位元組數、SHA-256 與 content type、prompt 與 `max_tokens`；`output` 只有 `{ chars }`（輸出長度）。**從不記圖或文字。** `usage_logs`：`material_ocr_requested`（手動）、`material_ocr_finished`（狀態、頁數、失敗頁、略過頁、字數）。

## 型別變更

```ts
interface Material {            // 清單、單一教材、上傳回應（以及之後 browse 的 MaterialCard）
  /* …既有欄位… */
  ocr: {
    status: "none" | "queued" | "running" | "done" | "failed";
    done_pages: number;         // 已處理的頁（成功或失敗）
    total_pages: number;        // 要辨識的頁數（PDF 頁數不明時先是 0）
    error: string | null;       // failed：原因；done：附註（失敗或略過的頁）或 null
  };
  text_source: "file" | "ocr";
}

interface PrepTurn {
  /* …既有欄位… */
  phase?: "ocr";                // 這一輪在等範本的 OCR（「Manna 正在辨識掃描檔（3／8 頁）」）；其他時候沒有這個欄位
  ocr?: { done_pages: number; total_pages: number };   // 與 phase 一起出現
}
```

- **OCR 進行中**（`queued`／`running`），`text_status` 維持原狀（例如 `no_text`、`not_applicable`，文字層壞掉的 PDF 是 `ready` 但文字是舊的）。

## 端點

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| POST | `/api/materials/:id/ocr` | – | 202 `MaterialDetail`（`ocr.status = "queued"`，`done_pages = 0`）。只限自己上傳的 PDF 與圖片，其他 400「只有上傳的 PDF 與圖片可以用 OCR 重新讀取。」；已在辨識 → 409；OCR 未開啟 → 503；別人的 → 404 |

**OCR 進行中的教材**：建立學習單（`POST /api/worksheets` 的 `material_id`，備課對話做學習單的輪次也是）、建立備課對話（`material_id`）、建立版型（`POST /api/worksheet-templates`，以及對話裡重試版型）→ 409「這份檔案正在辨識文字（第 n／m 頁），完成後再試一次。」（`n` 是正在辨識的頁 = 已完成頁數＋1，不超過 `m`；頁數還不知道時是「這份檔案正在辨識文字，完成後再試一次。」）。

**唯一的例外：`POST /api/prep-sessions` 的 `template_material_id`（照我的學習單做）。** 範本還在辨識時不擋：建立一個新版型（已有一個由這份範本、還在 `extracting` 的版型時沿用它），第一輪照 Phase 2.8 等版型（`running`、`worksheet: null`），回應的這一輪多 `phase: "ocr"`、`ocr: { done_pages, total_pages }`。OCR 完成後版型才開始讀取（範本的其他檢查此時才做，不合格時版型 `failed`、訊息照 Phase 2.8），讀好後照舊建立學習單。OCR 失敗時版型 `failed`（「這份學習單的文字辨識（OCR）沒有成功，所以沒有讀出版型…」），這一輪跟著 `failed`。201 的回應在任何背景工作開始前讀出。

等 OCR 期間，版型每 5 秒更新 `extract_started_at`，所以版型與這一輪都不會被當成中斷；這一輪的中斷計時改為從「建立時間」與「版型開始讀取時間」較晚的一個起算。

## 重啟

- 啟動時（應用程式只跑一個實例），上一個程序留下的 `queued`／`running` 一律**接手續做**：換一個新的 `ocr_started_at`（每一次寫入都以它為條件，所以舊容器若還在跑，下一次寫入就停），已在 `ocr_pages` 的頁不再辨識。OCR 或檔案儲存現在沒有開啟、或沒有原檔時改成 `failed`（「系統更新後無法繼續這次文字辨識…」）。
- 讀取教材清單、單一教材與手動辨識時，這個程序沒有在做、而且開始超過 `OCR_TIMEOUT_SECS × 頁數 + 120` 秒的辨識改成 `failed`（「文字辨識中斷了…」），可以再按「用 OCR 重新讀取」。
- 等 OCR 的版型工作不跨重啟：重啟後版型照 Phase 2.5 的規則逾時變成 `failed`，這一輪跟著 `failed`；按「再試一次」會用辨識好的文字重新讀取。

## 測試用假 OCR（`OCR_PROVIDER=fake`）

每頁回傳固定的 HTML：`Section-Header`「Page n」、兩個 `Text`（第一個是「Scanned text on page n」加上檔名裡的大寫標記，例如 `scan LESSON9.pdf` → `LESSON9`；第二個含 HTML entity）、一個 `Picture`、一個 `Page-Footer`。檔名標記：`FAKE_OCR_FAIL` 第 2 頁失敗（兩次都失敗）、`FAKE_OCR_DOWN` 每頁都失敗、`FAKE_OCR_SLOW` 每頁 2 秒。

## 其他

- 資料（`0018_ocr.sql`）：`materials.text_source`、`ocr_status`、`ocr_done_pages`、`ocr_total_pages`、`ocr_error`、`ocr_pages`、`ocr_started_at`，以及開放中工作的部分索引。
- 程式進入點：`materials::ocr`（`poor_text`、`wants_auto`、`check_idle`、`start_manual`、`recover`、`backfill_platform`）、`materials::ocr::job`（背景工作）、`materials::ocr::html`（HTML → 區塊／段落）、`materials::ocr::render`（`pdftoppm`、圖片）；範本等 OCR：`worksheet::template::extract::spawn_after_ocr`。

# 學習單的圖片

以下為**新增**；既有端點只多了回應欄位。Migration `0021_worksheet_images.sql`。設計見 `docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md` §6（本期做的是 §10 的第 2 期：圖片位置＋OpenAI 產圖；第 3 期「從範例裁圖」還沒做，見最後）。

學習單（一般學習單與「照我的學習單做」）的區塊可以有圖。寫區塊的文字模型在同一次結構化輸出裡多寫 `image_slots`：這裡需要什麼圖、替代文字、圖說。區塊通過檢查並存檔後，伺服器在背景用 OpenAI 的 Image API 產圖（固定的風格前綴，圖裡不放字），存進教材檔案儲存，顯示在學習單、DOCX 與 PDF。產不出來或不該產的位置印成空框，教師版寫明這張圖的用途。老師可以逐張「重畫」（可附一句說明）、「換成我的圖」或「拿掉」。

## 設定（環境變數，README「學習單的圖片」）

| 變數 | 說明 |
|---|---|
| `IMAGE_PROVIDER` | `none`（後端程式沒設時的預設）、`openai`（`docker-compose.yml` 也就是正式環境的預設；要關掉就在 Coolify 設 `none` 再重新部署）、`fake`（測試；`AI_PROVIDER=fake` 且沒設這個變數時的預設） |
| `OPENAI_API_KEY` | `openai` 必填；沒有時圖片關閉並警告 |
| `IMAGE_BASE_URL` | 預設 `https://api.openai.com/v1`；必須是 http(s) |
| `IMAGE_MODEL` | 預設 `gpt-image-2.5-flare` |
| `IMAGE_QUALITY` | `low`／`medium`（預設）／`high`／`xhigh`／`max`／`auto`（`xhigh`、`max` 只有 2.5／2.6 系列） |
| `IMAGE_SIZE` | 預設 `1536x1024`；`auto` 或 `寬x高`，兩邊都是 16 的倍數、16–3840、長寬比 1:3 到 3:1 |
| `IMAGE_TIMEOUT_SECS` | 預設 180 |
| `IMAGE_CONCURRENCY` | 全站同時產圖的張數（全域 semaphore），預設 2 |
| `IMAGE_MAX_PER_WORKSHEET` | 每份學習單自動產生的圖最多幾張（依學習單順序），預設 6；`0` 表示一律不自動產圖 |
| `IMAGE_MAX_REGENERATIONS` | 每份學習單「重畫」的次數上限（所有圖合計），預設 10 |

設定有誤只會關掉圖片並在啟動日誌警告。`ImageConfig` 的 `Debug` 不印金鑰；錯誤只帶 HTTP 狀態（被擋下時另帶 OpenAI 的固定錯誤碼 `moderation_blocked`），不帶回應內容或網址。

**圖片關閉時什麼都不變**：送給文字模型的請求不含 `image_slots` 與圖片規則（閘道快取與以前相同），學習單沒有圖片位置。

## OpenAI Image API（依官方文件，2026-11-17 查核）

- 端點 `POST {IMAGE_BASE_URL}/images/generations`，`Authorization: Bearer {OPENAI_API_KEY}`。
- 送出：`{"model","prompt","n":1,"size","quality","output_format":"jpeg","output_compression":90,"moderation":"auto","safety_identifier"}`。
  - GPT Image 模型一律回傳 base64：`data[0].b64_json`；`response_format`、`url`、`style` 對它們不支援，所以不送。
  - `gpt-image-2.5-flare` 接受：`background`（`transparent`／`opaque`／`auto`）、`quality` 到 `xhigh`／`max`、任意合規的 `寬x高`、`output_format` `png`／`webp`／`jpeg`（另有需另外開通的 `gif`、`svg`）、`output_compression`（jpeg／webp）、`moderation` `auto`／`low`、`n` 1–10、`stream`／`partial_images`、`safety_identifier`（≤128 字）。我們固定用 JPEG（Word 與 PDF 都能放，官方也說比 PNG 快）、`moderation: auto`（不用 `low`）。
  - `safety_identifier` 是教師 id 的 SHA-256 雜湊（`manna-teacher:<id>`），不是姓名或研究編號。
- 回應：`{ created, data: [{ b64_json, generation_id }], usage: { input_tokens, input_tokens_details: { image_tokens, text_tokens }, output_tokens, output_tokens_details: { image_tokens, text_tokens }, total_tokens } }`（`generation_id` 與 `usage` 的形狀依主持人以正式金鑰實測：1024x1024、low 的一張 PNG 約 1.29 MB 的 base64，usage 20／196／216 tokens；當時沒有被組織驗證擋下）。`usage` 只記數字。
- 被安全機制擋下：`error.type = "image_generation_user_error"`、`error.code = "moderation_blocked"`（可能附 `moderation_details`）。只看 `code`，不重試，給老師一般性的說明；`moderation_details` 不記錄（錯誤內容一律不留）。
- 重試：429、5xx、連線中斷各重試一次（間隔 2 秒）；**逾時不重試**（官方說複雜的圖最多約 2 分鐘，逾時的那張可能已經計費）。
- 官方指南：使用 GPT Image 模型前，組織「may need to complete the API Organization Verification」；沒驗證會回 403（老師看到「產圖服務暫時無法使用（HTTP 403）」）。

## 圖片位置（`image_slots`）

```ts
interface WsBlock {
  /* …既有欄位… */
  image_slots?: WsImageSlot[];     // 最多 2 個；沒有時 JSON 裡沒有這個欄位
}

interface WsImageSlot {
  id: string;                      // 伺服器給的，整份學習單唯一：r1b2-p1；重新產生的區塊換新 id（r1b2-r3p1）
  kind: "scene" | "object" | "icon";   // 舊的學習單可能還有 "photo_needed"：現在一樣會產圖（照插圖畫）
  purpose_zh: string;              // ≤80，給老師看：這張圖在這裡做什麼
  prompt_en: string;               // ≤300，給產圖模型：只描述畫面
  alt_en: string;                  // ≤120，替代文字
  caption: string;                 // ≤80，印在圖下方（"" 表示沒有）；字放這裡，不畫在圖裡
}
```

- **誰寫**：圖片開啟時，
  - 一般學習單的逐段任務步驟（`worksheet_tasks`）：每個區塊最多 1 個（一次呼叫寫全部區塊，輸出預算有限），schema 放在 `items` 之後；
  - 照版型產生（`worksheet_template_section`，「照我的學習單做」）：每個區塊最多 2 個，放在最後（先寫區塊，再寫需要的圖）。版型的來源範例是掃描檔、OCR 數到圖（`materials.ocr_pages[].pictures`）時，規則多一行告訴模型範例裡有幾張圖。需要教師自己素材的區塊（`needs_external_material`）通常不會要圖，假模型也不給；
  - 暖身、Exit ticket、閱讀理解題／分層練習、備課對話的「改學習單」不寫圖片位置（改學習單會保留區塊原本的圖片位置）。
- **schema**：每個字串有 `maxLength`、陣列有 `maxItems`，屬性依寫出順序，`kind` 是 enum（scene／object／icon）。圖片規則（prompt key `worksheet_image_slots`，可在管理頁改）以「圖片位置：…」附在 user 訊息後。
- **伺服器清理**（`images::clean_slots`，不會因此多一次修正呼叫）：字串截到上限；寫了網址或 e-mail 的拿掉；模型仍寫出 `photo_needed` 時改成 `scene` 照常產圖（沒有畫面描述就用替代文字）；沒有畫面描述也沒有替代文字的拿掉；重複的拿掉；超過上限的只留前面；`alt_en`／`purpose_zh` 空的由其他欄位補。**需要真實照片的改成 `photo_needed`**：提到地圖、國旗、肖像、名人、總統、國王、歷史、戰爭、地標、紀念碑、商標、品牌、照片，或出現連續兩個大寫開頭的字（`Eiffel Tower`、`Taylor Swift`；`Mrs. Lin` 這種有句點的稱謂不算）。`photo_needed` **從不產圖**，只留空框請老師放。
- **存檔**：`PUT /api/worksheets/:id` 送回的 `image_slots` 照樣檢查：每區最多 2 個、id 1–40 字元且整份唯一、各欄位不超過上限、`kind` 只能是四個值之一，否則 400。編輯器的存檔會原樣送回；老師改區塊、搬移、逐區修改都會保留。
- **重新產生區塊**：圖片開啟時，新區塊用模型新寫的位置（新 id，所以會產新圖；舊圖還在，按「復原上一版」就回來）；圖片關閉時保留原本的位置與圖。

## 產圖

**所有圖片位置都會產圖**（主持人 2026-11-17 決定）：以前地圖、國旗、真實人物、地標、歷史這類「需要真實照片」的位置會跳過、留給老師；現在一律由 AI 畫成插圖，畫得不對由老師重畫、換圖或拿掉（`0026_images_always_drawn.sql`：`worksheet_image_slots` 第 2 版拿掉 `photo_needed`）。舊學習單裡已經被跳過的位置可以直接按「重畫」。

1. 學習單或單一區塊產生完、存檔後（`cause = generate`／`regenerate`），伺服器依學習單順序看每個啟用區塊的圖片位置；從沒有圖片紀錄的位置各新增一筆（`images::enqueue`）：
   - 圖片關閉 → `skipped`；沒有檔案儲存 → `skipped`；
   - 這份學習單自動產生的圖已達 `IMAGE_MAX_PER_WORKSHEET` → `skipped`（「…已經到上限；這一張可以按「重畫」，或上傳自己的圖。」）；
   - 其他 → `queued`，開一個背景工作。
2. 背景工作（`images::job`）：等全站的產圖名額（`IMAGE_CONCURRENCY`）→ `running`。工作開始時這個位置已不在學習單上（區塊被重新產生了）就 `skipped`，不呼叫模型。
3. 提示 = 固定風格前綴（prompt key `image_style` 的 system_text）＋ user_template（`Picture: {{description}}{{teacher_note}}`）：
   > Simple flat vector illustration for a Taiwanese senior-high English worksheet. Clean outlines, soft limited palette, plain white background, one clear subject, classroom-appropriate. No text, letters, numbers, labels, logos, watermarks or brand names anywhere in the picture. No real or famous people.

   重畫時老師的說明附在後面（`Teacher's note: …`）。每張圖記下用的是第幾版風格。
4. 解碼後重新編碼：轉正、最長邊縮到 1536 px、透明處變白、存成 JPEG（品質 85），去掉 EXIF 等所有中繼資料；存到 `worksheet-images/w<學習單>/<圖片 id>-<亂數>.jpg`。
5. 失敗（逾時、HTTP 錯誤、被擋下、讀不出圖、存不進去）→ `failed`，`message` 是給老師的一句話；學習單印空框。

## 型別

```ts
type WsImageStatus = "queued" | "running" | "done" | "failed" | "skipped" | "removed";

interface WsSlotImage {             // 一個圖片位置目前的圖
  slot_id: string;
  block_id: string;
  status: WsImageStatus;            // skipped：沒有送去產圖；removed：老師拿掉了
  source: "generated" | "upload" | "none";
  image_id: number | null;          // done 時才有
  width: number | null;             // 存好的 JPEG 的像素
  height: number | null;
  message: string | null;           // 為什麼沒有圖（給老師看）
  updated_at: string;
}

interface WorksheetImages {
  enabled: boolean;                 // 這個環境有沒有開 AI 產圖
  regenerations_left: number;
  slots: WsSlotImage[];             // 只有有紀錄的位置；沒有紀錄的位置視為空框
}

interface WorksheetState {
  /* …既有欄位… */
  images: WorksheetImages;
}

interface WsSheetBlock {            // GET /sheet 的投影
  /* …既有欄位… */
  images?: WsSheetImage[];          // 印在作答指示之後；沒有時沒有這個欄位
}

interface WsSheetImage {
  slot_id: string;
  status: Exclude<WsImageStatus, "removed">;   // 拿掉的不印
  image_id: number | null;
  width: number | null;
  height: number | null;
  alt: string;
  caption: string;
  ai_generated: boolean;
  purpose?: string;                 // 只有教師版
  message?: string | null;          // 只有教師版
}
```

## 端點（都只限學習單的擁有者；別人的 → 404，未登入 → 401）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/worksheets/:id/images` | – | `WorksheetImages`（產圖中時前端每 2 秒問一次） |
| GET | `/api/worksheets/:id/images/:image_id` | Bearer 權杖（`If-None-Match` 選填） | 200 `image/jpeg`；304。`ETag` 是圖的 SHA-256，`Cache-Control: private, max-age=3600`，`Vary: Authorization`，`X-Content-Type-Options: nosniff`。舊的（被換掉的）圖也讀得到 |
| POST | `/api/worksheets/:id/image-slots/:slot_id/regenerate` | `{ hint?: string }`（≤200 字，不能有網址） | 202 `WorksheetImages`（這個位置 `queued`） |
| POST | `/api/worksheets/:id/image-slots/:slot_id/upload` | `multipart/form-data`，欄位 `file`：png、jpg、webp、gif，≤10 MB（與教材圖片相同的檔頭檢查） | 200 `WorksheetImages`（`done`，`source = "upload"`） |
| DELETE | `/api/worksheets/:id/image-slots/:slot_id` | – | 200 `WorksheetImages`（`removed`） |

- **重畫**：圖片關閉 → 503；沒有檔案儲存 → 503；沒有這個位置 → 404；這張正在產生 → 409；這份學習單的重畫次數用完 → 409（「…已經重畫 10 次，到上限了；可以上傳自己的圖。」）。自動產生時因上限被略過的位置也可以重畫（算在重畫次數裡）。
- **換成我的圖**：不是圖片 → 400；檔頭與副檔名不符或讀不出來 → 400；沒有檔案儲存 → 503。上傳的圖同樣轉成 JPEG（最長邊 1536 px，去掉中繼資料）。這張正在產生時直接取代（產圖工作在下一次寫入時停下）。
- **拿掉**：這個位置不再印出；重畫或上傳就會回來。
- 這三個動作都只動那一張圖，**不會產生新的學習單版本**，也不受「Manna 正在產生這份學習單」的限制。每次都新增一筆紀錄成為目前的圖，舊的保留。
- `GET /api/worksheets/:id`、`/sheet`、`/export` 讀取時，開始超過 `IMAGE_TIMEOUT_SECS × 2 + 120` 秒、這個程序也沒有在做的產圖改成 `failed`（「產圖中斷了…」）。

## 學習單與匯出

- `GET /api/worksheets/:id/sheet`：每一區的 `images` 依區塊裡的圖片位置排列，`done` 的帶 `image_id`，其他是空框；拿掉的不列。
- **DOCX**：圖放在作答指示之後，置中，最寬 4.2 吋、最高 3 吋（維持長寬比），是 Word 原生的內嵌圖片（`word/media/imageN.jpeg`，有替代文字）；下面是圖說（斜體）。沒有圖的位置是一個置中的虛線框（約 10 × 4.6 cm），教師版框裡寫「請放一張圖：用途」。教師版在 AI 產的圖下面標「AI-generated picture」（英文，所以教師版 PDF 不會因此需要中文字型）。
- **PDF**：同樣的位置與規則（`krilla` 開啟 `raster-images`）；圖最寬為內文寬的 62%、最高 210 pt。
- `usage_logs` 的 `worksheet_exported` 多了 `pictures`（印出的圖與框）與 `pictures_embedded`（實際放進去的圖）。
- 教學套件（`/prep-sessions/:id/turns/:turn_id/kit`）的學習單是分層練習，沒有圖片位置，不受影響。

## 前端

- **編輯器**（`/worksheets/:id`）與**備課對話的學習單預覽**：圖在作答指示之後（`SlotPicture`），圖片用帶權杖的請求取得（object URL）。產圖中顯示「Manna 正在畫這張圖…」並每 2 秒問一次；沒有圖時是虛線框，寫出用途與原因。教師版在 AI 產的圖下面標「AI 生成」。
- 每張圖的工具：「重畫」（展開一個選填的說明欄，例如「改成晴天」）、「換成我的圖」、「拿掉」，以及「還能重畫 n 次」。`photo_needed` 沒有重畫；圖片關閉時只有上傳與拿掉。拿掉後編輯器留一行「這一區的圖已經拿掉了（不會印出）」與工具，可以再放回來。
- 這些動作不經過自動存檔。

## 紀錄

- **`ai_interactions`**（`module = "worksheet_image"`，`scaffold = SC-WS-IMAGE`，`prompt_version_id` = 用的 `image_style` 版本）：每一次呼叫一筆，重試是 `attempt = 2`、`retry_of`。`input_snapshot` 只有學習單、圖片、位置的 id、`kind`、是否重畫、**提示的字數與 SHA-256**、老師說明的字數與 SHA-256、風格版本、請求設定（model、size、quality、n、output_format、output_compression、moderation）；`output` 只有圖的位元組數、SHA-256 與 `usage` 的 token 數。**從不記提示文字、老師的說明或圖。** 被擋下是 `blocked`，逾時 `timeout`，其他 `error`（`error_detail` 只有狀態）。
- **伺服器日誌**：只有 id、張數、位元組數、長寬、耗時。
- **`usage_logs`**：`worksheet_images_queued`（`queued`、`skipped` 張數）、`worksheet_image_finished`（狀態、失敗種類、位元組數、耗時）、`worksheet_image_regenerated`（說明字數、已用次數）、`worksheet_image_uploaded`（原檔與存檔位元組數、長寬）、`worksheet_image_removed`。
- **`worksheet_images`**：每筆嘗試的狀態、來源、說明（老師打的字，重啟後續做需要）、提示的字數與 SHA-256、模型與設定、風格版本、存檔位置、大小、長寬、SHA-256、`interaction_id`。

## 重啟

啟動時（應用程式只跑一個實例），上一個程序留下的 `queued`／`running` 一律接手：換一個新的 `started_at`（每一次寫入都以它為條件，所以舊容器若還在跑，下一次寫入就停）再產一次。圖片或檔案儲存現在沒有開啟時改成 `failed`（「系統更新後無法繼續產這張圖…」）。（`images::recover`）

## 測試用假模型

- `IMAGE_PROVIDER=fake`：回傳一張 48 × 32 的 PNG（顏色依提示而定），回應形狀與正式 API 相同（`data[0].b64_json`、`generation_id`、`usage` 20／196／216）。提示裡的標記：`FAKE_IMG_FAIL`（HTTP 503，兩次都失敗）、`FAKE_IMG_BLOCKED`（`moderation_blocked`）、`FAKE_IMG_TIMEOUT`（逾時）、`FAKE_IMG_SLOW`（2 秒）。
- 假文字模型（請求有 `image_slots` 時）：逐段任務的第一個區塊一個 `scene` 位置；教材有 `FAKE_IMG_PHOTO` 時第二個區塊要一張地圖（描述帶 `FAKE_IMG_FAIL`，這張會失敗、留空框）；版型的每個區塊一個（需要教師素材的區塊除外）。教材裡的 `FAKE_IMG_*` 標記會帶進畫面描述。

## 費用與上限

- 每份學習單：自動產生最多 `IMAGE_MAX_PER_WORKSHEET`（6）張，重畫最多 `IMAGE_MAX_REGENERATIONS`（10）次；以研究文件的估價（`medium`、1536x1024 約 US$0.041–0.053 一張），一份學習單最多約 16 張、US$0.85；一般情況（3 張、重畫一半）約 US$0.2。
- 每位老師每月的上限還沒有做（研究文件 §11 第 3 點待主持人決定）。

## 還沒做（之後的期）

- **從老師的範例找出圖（第 3 期）**：抽版型時讀出範例裡的圖（OCR 的 `Picture` bbox 已存在 `materials.ocr_pages`；DOCX 的 `word/media` 與有文字層的 PDF 還沒讀），用視覺模型描述，對應到版型區塊的圖片位置；裝飾性的圖可以直接裁用。目前版型沒有「這一區有圖」的資訊，所以照版型產生時是由文字模型依每一區的內容判斷要不要圖；範例是掃描檔（OCR 數到圖）時，圖片規則後面多一行「這位老師的範例學習單裡有 N 張圖…」當提示（`images::sample_pictures`）。
- 圖放在某一題旁邊（研究文件的 `position: item:<id>`）：目前一律放在區塊的作答指示之後。
- Openverse／Wikimedia 搜圖、用第一張圖當參考讓整份風格一致（`/v1/images/edits`）、每位老師每月上限、「有 N 張 AI 產生的圖還沒看過」的匯出提醒、把換掉的舊圖換回來的介面、刪除學習單時清掉圖檔（學習單目前只有軟刪除）。

## 程式進入點

`worksheet::images`（`client`：設定、請求與回應、假模型；`job`：背景工作；`clean_slots`、`enqueue`、`regenerate`、`replace_with_upload`、`remove`、`decorate`、`bank_for`、`recover`）、`worksheet::export::{docx_with_images, pdf_with_images}`、`routes::worksheets::{images_state, image_file, regenerate_image, upload_image, remove_image}`；前端 `web/src/components/worksheet/SlotPicture.tsx`。測試 `api/tests/worksheet_images.rs`、`web/src/__tests__/worksheetImages.test.tsx`。

# 知識庫：教材牆與預覽圖

以下為**新增**；沒有新的資料表或 migration。

`/knowledge`（只有 `manage_knowledge`：`pi`、`kb_manager`）改名為「知識庫」，分兩個分頁：

- **教材**（預設）：所有平台教材（即 `GET /api/browse/materials?area=reference` 的列）排成一面像 Pinterest 的牆：卡片依內容有各自的高度（PDF 第 1 頁、圖片、或文字的前幾行），用 CSS columns 排成數欄，手機上一欄。上面是瀏覽的搜尋框、分類籤（facets）、只看籤與標籤。點卡片打開預覽視窗：PDF 的第 1 頁在左、擷取的文字與段落在右（教材檢視的唯讀版，不載入註記），圖片與影音在檢視裡直接顯示，可下載原檔。頁首「上傳教材」連到「教材與範本管理」（`/admin/library`）。
- **課綱條目與詞彙**（`?tab=curriculum`）：原本的目前內容、語意索引、CSV 匯入、查看條目與詞彙表，功能不變。

## 預覽圖

| Method | Path | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/materials/:id/thumbnail` | Bearer 權杖（`If-None-Match` 選填） | 200 `image/jpeg`；304（`If-None-Match` 相符） |

- **權限**同原檔（`GET /api/materials/:id/file`）：`use_materials`，而且這位教師讀得到這份教材（自己的，或平台的）；別人的個人教材 → 404。沒登入 → 401。
- **有預覽圖的教材**：上傳的 PDF 與圖片（有原檔）。其他（DOCX、TXT、貼上的文字、網址、影音）→ 404「這份教材沒有預覽圖（只有上傳的 PDF 與圖片有）。」，前端改顯示文字的前幾行。
- **怎麼做**：PDF 用 `pdftoppm` 把第 1 頁轉成寬 480 px 的圖（與 OCR 同一個轉圖程式）；圖片依 EXIF 方向轉正、縮到寬 480 px、高 1440 px 以內（只縮小不放大）。一律輸出 JPEG（品質 80），透明的地方變白色。全站同時最多轉 2 份（semaphore）。
- **保存**：第一次轉好後存進檔案儲存空間 `thumbs/w480/<原檔 SHA-256>.jpg`，之後直接讀出。同一個檔案（相同 SHA-256）的教材共用一張；讀得到其中任一份教材的人本來就有這個檔案。存不進去時照樣回應，下次再轉。
- **刪除**：硬刪除教材時（`DELETE /api/materials/:id`、`DELETE /api/admin/library/:id`），沒有其他教材持有同一個檔案就一併刪除預覽圖；刪不掉時整個不刪（503），和原檔相同。
- **HTTP 快取**：`Cache-Control: private, max-age=3600`、`ETag: "<SHA-256>-w480"`（教材的原檔不會被換掉）、`Vary: Authorization`（共用電腦換帳號登入時不會拿到上一個帳號的快取）；另有 `X-Content-Type-Options: nosniff`、`Content-Security-Policy: default-src 'none'; sandbox`。
- **錯誤**：儲存空間沒有設定 → 503；原檔不在儲存空間 → 404；原檔暫時讀不到 → 503；沒有安裝 `pdftoppm` → 503「這個環境沒有安裝 PDF 轉圖工具（pdftoppm），無法產生預覽圖。」；PDF 加密或損壞、圖片解不開 → 404「這份檔案轉不成預覽圖（可能已加密或損壞）。」。前端遇到任何錯誤都改顯示文字卡片。
- 前端用帶權杖的 `fetch` 取圖（轉成 blob URL），卡片接近畫面時才要。
- 程式進入點：`materials::thumbnail`（`preview`、`shrink`、`forget`）、`routes::library::thumbnail`。

# 帳號與角色

以下為**新增**；沒有新的資料表或 migration（`teachers.role` 本來就有，`0007` 的 CHECK 已含六種角色）。

帳號的角色原本只能在建立邀請碼時決定。現在主持人可以在後台變更已經存在的帳號的角色，例如把一位已在使用平台的教師改成知識庫管理員。

## 型別

```ts
interface Account {
  id: number;
  research_id: string;        // 研究編號（假名）；不回姓名、Email 等 identity_map 的資料
  school_track: SchoolTrack;
  role: Role;
  created_at: string;         // 第一次用邀請碼登入、建立帳號的時間
  is_self: boolean;           // 目前登入的主持人自己
}
```

## 端點（`administer`：只有 `pi`；其他角色 403，未登入 401）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/admin/accounts` | – | `Account[]`：未刪除（`deleted_at IS NULL`）的帳號，最早加入的在前。還沒有人用過的邀請碼不是帳號，不會列出 |
| PATCH | `/api/admin/accounts/:id` | `{ role: Role }`（不接受其他欄位） | 變更後的 `Account` |

`PATCH` 的錯誤：

- `400`：角色不是六種之一、缺 `role` 或帶了其他欄位。
- `404`：沒有這個帳號，或帳號已刪除。
- `409`：變更自己的角色（「不能變更自己的角色：請另一位主持人幫你變更…」，避免主持人把自己鎖在後台外）；或變更後平台上不再有任何未刪除的 `pi`（「平台至少要有一位主持人…」）。
- 角色和現在相同時不算變更：回 `200` 與帳號，什麼都不改、不記錄（自己的帳號也一樣）。

## 規則

- **立即生效，不必重新登入**：每個請求都從 `teachers` 重新讀角色（`sessions` JOIN `teachers`），所以對方的下一個請求就依新角色檢查，`GET /api/me` 的 `capabilities` 也跟著變。前端只在載入網站時讀 `/api/me`，所以對方重新整理網頁後才會看到（或不再看到）對應的入口。
- **主持人可以變更另一位主持人**（例如有第二位主持人後，把原本的主持人改成知識庫管理員），但不能變更自己。
- **一次一個變更**：在交易裡用 advisory lock 排隊，並在鎖內重新讀取呼叫者自己的角色。兩位主持人同時把對方改掉，不會兩個都成功；剛被拿掉主持人角色的人，也不能接著變更別人（403）。
- **紀錄**：伺服器日誌記一行 `account role changed`，只有 `actor_id`、`account_id`、`from`、`to`（id 與角色，沒有研究編號或其他個資）。不寫 `usage_logs`（那是研究資料）。
- 不讀也不寫 `identity_map`。

## 前端

後台管理 `/admin`（只有 `administer`）新增「帳號與角色」：每列是研究編號、學校類型、加入日期與角色下拉選單（`ROLE_LABELS`）。選了新角色後，先在該列下方確認（寫出帳號、原角色與新角色，以及新角色代表什麼），按「確定變更」才送出；成功或失敗的訊息顯示在該列。自己的那列只顯示角色並標示「你」，不能變更。區塊上方一行說明知識庫管理員能做與不能做的事（管理知識庫的教材、學習單範本、課綱條目與詞彙；不能建立邀請碼、變更平台設定，也看不到其他教師的資料），建立邀請碼的說明用同一句。

## 程式進入點

`routes::admin`（`list_accounts`、`update_account_role`、`role_change_conflict`）、`auth::require_role`；測試 `api/tests/accounts.rs`、`web/src/__tests__/admin.test.tsx`。

---

# 學習單：逐區修改與註記

設計見 [WORKSHEET_BLOCK_EDIT_RESEARCH.md](WORKSHEET_BLOCK_EDIT_RESEARCH.md) §4–§5。Migration `0020_worksheet_block_revise.sql`（兩張新表、prompt `worksheet_block_revise` 第 1 版、鷹架 `SC-WS-BLOCK-REVISE`）。既有端點只多了選填欄位；舊的學習單照常讀寫。

`0044_block_revise_scaffolds.sql`：**「改這一區」涵蓋整區，鷹架、字詞表與 Glossary 也在內**（見「一次修改」）；`worksheet_block_revise` 新增一版提示詞（接在當時最新的文字後面，舊版保留；使用者訊息不用改，這些內容在 `{{block_detail}}` 裡），`SC-WS-BLOCK-REVISE` 的 `support_form` 補一句。建議卡（`WsProposal.changes.extras`）列出鷹架、字詞表與 Glossary 的修改，「採用」整個區塊一起寫入，復原與修改歷程用的是整個區塊，所以一併還原。編輯器的字詞表上方也有「刪除」（整張表，不經 AI）。

教師在產生好的學習單上指定**一個區塊**（或選取區塊裡的一段文字），寫下註記或一句話，Manna 只改那一區。結果是一個**修改建議**（proposal）：學習單在教師按「採用」之前完全不變；採用後寫成一個新版本（`cause = "revise"`），被取代的區塊放進 `worksheet_block_history`，所以「復原上一版」照常可用。

## 資料

- `worksheet_comments`：教師的註記。錨點 = `block_id` + `field`（`block`／`title`／`instructions`／`item`）+ `item_id`（`field = item`）+ 欄位文字中的 UTF-16 位移（選字時才有）+ `quote`／`prefix`／`suffix`。`share_with_ai`（「提供給 AI」）預設 `true`；`status` = `open`（待處理）／`applied`（已依此修改）／`dismissed`（已略過）；`proposal_id` = 套用它的那次修改。註記**不是學習單的一部分**：投影（`/sheet`）、DOCX、PDF 都沒有它。
- `worksheet_block_proposals`：一個區塊的修改建議。`base_block` 是送給模型時的區塊原樣；`round` 是這一區第幾次請 Manna 改；`status` = `running`／`proposed`／`accepted`／`rejected`／`stale`／`failed`；`decided_at` 在採用、放棄（或關掉失敗的那一個）時寫入。一份學習單同時只有一個 `running`（partial unique index）：同一份學習單的不同部分同時呼叫會互相重複（`docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md` §5.4）。
- 修改**不是**學習單的背景工作（`worksheet_jobs`）：它在採用前什麼都不寫，所以不鎖編輯器、不讓預覽變成進度條。

## 型別

```ts
type WsCommentField = "block" | "title" | "instructions" | "item";
interface WorksheetComment {
  id: number; worksheet_id: number;
  block_id: string; field: WsCommentField; item_id: string | null;   // 建立時的位置
  quote: string; note: string; share_with_ai: boolean;
  status: "open" | "applied" | "dismissed"; proposal_id: number | null;
  anchor: {                                  // 每次讀取時重新定位
    status: "ok" | "moved" | "lost";         // lost：區塊或題目已經不在了
    block_id: string | null;                 // null：區塊不在了（例如重新產生了整份）
    item_id: string | null; start: number | null; end: number | null;
  };
  created_at: string; updated_at: string;
}
interface WsProposal {
  id: number; worksheet_id: number; block_id: string;
  status: "running" | "proposed" | "accepted" | "rejected" | "stale" | "failed";
  instruction: string; comment_ids: number[]; whole_block: boolean; round: number;
  reply: string;              // Manna 說它改了什麼（≤200，含沒有通過檢查而維持原樣的說明）
  error: string | null;       // failed／stale 的原因
  base_version_no: number;
  before: WsBlock;            // 送給 Manna 時的區塊
  block: WsBlock | null;      // 改好的區塊（proposed 之後）
  changes: { title: boolean; instructions: boolean;
             items: Array<{ id: string; change: "kept" | "modified" | "new" | "removed" }>;
             // 鷹架、字詞表、Glossary 裡改了、新增或刪除的（沒變的不列；舊的伺服器沒有這一欄）
             extras?: Array<{ group: "scaffold" | "word" | "gloss";
                              id: string;       // 鷹架的 id；字詞表與 Glossary 是字（小寫）
                              label: string;    // 鷹架的種類（句型框架…）或字
                              change: "modified" | "new" | "removed" }> } | null;
  interaction_id: number | null; accepted_version_no: number | null;
  created_at: string; finished_at: string | null; decided_at: string | null;
}
```

`WsSheet` 的每一題多了 `id`（題目在學習單裡的 id，註記錨在它上面）。

## 端點（皆需登入；只能存取自己的學習單、註記與修改，其他人的回 404）

| 方法 | 路徑 | 請求 | 回應 |
|---|---|---|---|
| GET | `/api/worksheets/:id/comments` | – | `WorksheetComment[]`，依區塊、題目、位置排序（區塊不在了的排最後） |
| POST | `/api/worksheets/:id/comments` | `{ block_id, field?, item_id?, start?, end?, quote?, note?, share_with_ai? }` | `WorksheetComment`。`field` 省略時有 `item_id` 是 `item`，否則 `block`。`start`／`end` 是欄位文字（學生版印出的、去掉頭尾空白的標題／作答指示／題幹）中的 UTF-16 位移；引文與前後文由伺服器從學習單取。位移對不上而有 `quote` 時，伺服器在欄位裡找那段文字；找不到就照存 `quote`、標在整個欄位（例如分層練習印出的標題是「Part B · …」，存的是「Part A · …」）。區塊或題目不在最新版本 → 400；`note` > 2000、`quote` > 500 字 → 400；一份學習單最多 500 則 |
| PATCH | `/api/worksheets/:id/comments/:cid` | `{ note?, share_with_ai?, status?: "open" \| "dismissed" }` | `WorksheetComment`。`applied` 只由「採用」設定（400） |
| DELETE | `/api/worksheets/:id/comments/:cid` | – | `{ deleted: true }`（只刪註記） |
| POST | `/api/worksheets/:id/blocks/:block_id/revise` | `{ instruction?, comment_ids?, whole_block? }` | 202 `WsProposal`（`running`）。要有 `instruction`（≤2000）或註記：`comment_ids` 的每一則都要在這一區、`open`、`share_with_ai`（否則 400）；兩者都沒給時用這一區所有待處理、提供給 AI 的註記，一則都沒有 → 400。區塊不在 → 404；學習單正在產生 → 409；已有修改在跑 → 409（「Manna 正在改另一區…」）。這一區還在等決定的建議先放到一旁（`proposed` 記為 `rejected`，並寫一筆 `teacher_decisions` reject），所以「再改一次」就是再送一次 |
| GET | `/api/worksheets/:id/proposals` | – | `WsProposal[]`：每一區最新的那一個，只列還在等教師的（`running`、`proposed`，以及還沒關掉的 `failed`、`stale`） |
| GET | `/api/worksheets/:id/proposals/:pid` | – | `WsProposal`（輪詢用） |
| POST | `/api/worksheets/:id/proposals/:pid/accept` | `{}` | `{ proposal, pictures, worksheet: WorksheetState }`。在一個交易裡：確認最新版本裡這一區**和 `before` 完全相同**（之後被改過 → 這個建議變成 `stale`，回 409，什麼都不寫）→ 新版本（`cause = "revise"`，`source` 見「復原與修改歷程」）→ 舊區塊進 `worksheet_block_history`（附 `proposal_id`、採用後的區塊 `after_block`）→ 這次用到的註記標成 `applied` → 同一輪的其他建議關閉 → `teacher_decisions` 記 `adopt`。其他區塊之間改過不影響。可以採用的是 `proposed`，或「再改一次」放到一旁、同一輪還沒結束的 `rejected`（見「同一輪的各次建議」）；其他 → 409；學習單正在產生 → 409 |
| POST | `/api/worksheets/:id/proposals/:pid/reject` | `{}` | `WsProposal`。`proposed` → `rejected`（記 `teacher_decisions` reject），註記維持待處理；`failed`／`stale` 只是關掉（「知道了」）。`running` 或已經決定過 → 409 |

### 同一輪的各次建議（`0034_append_only_history.sql`）

「再改一次」不會讓前面的建議消失。這一區從上次改動以來（`base_block` 相同、還沒採用也沒放棄）的所有建議叫**同一輪**：

- `WsProposal.earlier`（只在 `GET /proposals` 的每一項上，沒有就省略）：同一輪較早的建議（`rejected`、`proposed` 內容都還在，舊的在前）。前端在建議框標題做「‹ 第 2 次（共 3 次） ›」，預設顯示最新一次，每一次各有自己的差異、說明、要求與圖片動作。
- `worksheet_block_proposals.set_aside_at`：被「再改一次」放到一旁的時間（`status = rejected`，仍保留 `proposed` 內容）；`round_closed_at`：這一輪結束的時間（採用其中一次，或教師按「放棄」「知道了」）。結束後那一輪的建議都不能再採用（409「這個修改已經處理過了。」）。
- 採用一個較早的建議：用同一個 `accept` 端點。一樣要求這一區和那個建議的 `before` 完全相同，否則 **409「這一區之後改過了，這個版本不能直接採用。」**，什麼都不改（同一輪的其他建議不受影響）；Manna 還在改這一區 → 409。成功時：被採用的記 `accepted`；還在 `proposed` 的最新一次記 `rejected` 並寫一筆 `teacher_decisions` reject（「採用了同一輪的另一次建議」）；其他已被放到一旁的只記 `round_closed_at`。**先前「再改一次」那筆 reject 原樣保留，只新增一筆 `adopt`**（同一個 `ai_interaction` 先有 reject 再有 adopt，晚採用的資料也留著）。圖片動作照採用的方式執行。
- 「放棄」結束整輪；「再改一次」用目前顯示的那一次的要求再問，新的一次加進同一輪。
- 研究紀錄：`usage_logs` `worksheet_block_revise_accepted` 的 payload 多 `attempt_no`（採用的是第幾次）、`attempts`（同一輪共幾次）、`attempt_proposal_ids`（同一輪的 proposal id，舊的在前）。

## 一次修改（`worksheet_block_revise`）

背景一次呼叫（重試一次），經過與備課對話相同的呼叫與紀錄（`application::generation::call`：每次嘗試一列 `ai_interactions`、`max_tokens` 一定設、重試時溫度 +0.4 並列出沒通過的檢查、教材只記字數與雜湊）。`max_tokens` = 1000 + 220 × 可寫的題數（2500–6000）。第 n 次修改同一區（`round`）溫度從 0.2 起每次 +0.1，訊息裡也寫明「這是第 n 次、上一次的改法教師沒有採用」，所以請求一定不同（閘道會快取相同的請求）。

**輸入**：這一區的標題、作答指示和每一題（代號 `Q1`、`Q2`…，附學生版題號、作答方式、Bloom 層次、選項、參考答案），**以及學生版這一區印出來的其他部分，各有代號**：鷹架 `S1`…（程序鷹架列出每一步；句型框架、引導問題、作答示例等列出內容，附印在哪些版本與標題）、字詞表 `W1`…（學生版的 Word／Meaning／級別 表格，每一列一個字，附級別或「指示用語」與解釋）、Glossary `G1`…。每一類沒有時寫明「（沒有）」，所以模型說「這一區沒有」時對得上看到的內容（`block_detail`，`ai/revise.rs::extras_detail`）；這一區的註記（`[Q2]「引文」→ 註記`、`[作答指示]`、`[整區]`；只有這次用到的）；可以修改的範圍；其他區塊的標題與題幹（唯讀，避免重複）；學習單的學習目標（不得修改）；照版型做的區塊附上版型的指示語言、指示語寫法、題型樣式與設計習慣；依 Bloom 層次產生的區塊註明固定層次；需要教師提供素材的區塊註明不得編造；班級特性、教師寫的學習目標與要求、勾了「提供給 AI」的教材註記；教材（段落標記）。不送學生資料。

**輸出 schema（依寫出順序）**：`{ title ≤150, instructions ≤500, items ≤max(n+6, 12): [{ source: enum("Q1"…, "new"), keep: boolean, answer_format: enum, prompt ≤600, options ≤6×150, answer ≤400, bloom: enum }], scaffolds: [{ source: enum("S1"…, "new"), action: enum("keep","change","remove"), kind: enum, content ≤1200, steps ≤12×300 }], words: [{ source: enum("W1"…, "new"), action: enum, word ≤60, meaning ≤200 }], glossary: [{ source: enum("G1"…), action: enum("keep","remove") }], picture_actions, reply ≤200 }`。`title`／`instructions` 寫空字串表示不改；`keep` 為 true 的題目只寫代號與標籤，內容寫空字串。`scaffolds`、`words`（最多比現有的多 5 筆）、`glossary` 只需列出要改、要刪或要新增的；沒列出的一律保留（所以輸出被截斷也不會誤刪）。`max_tokens` 另加 400 + 90 × 鷹架、字詞與 Glossary 的筆數（上限 4000）。

**伺服器怎麼組回去**（`worksheet::revise::merge`，從存著的區塊出發，只換模型改的地方）：

- `keep: true` 的題目**原封不動**沿用存著的那一題（id、教師改過的字、`levels`、`hint`、`purpose`、`origin` 都不變），不採用模型抄回的文字。
- 修改的題目保留 id、`levels`、`hint`、`purpose`，換掉作答方式、題幹、選項與 Bloom 層次；參考答案變了才換答案與依據。內容和原本一樣的不算修改。`origin` 改成 `ai`。差異化學習單的題目只有在題幹沒變時保留各版本的支持與答案（`tiers`），題幹變了就沒有 `tiers`。新題目有新 id（`<block>-p<proposal>n<k>`），出現在學習單的所有版本（差異化學習單每一題都在三個版本）。依 Bloom 層次產生的區塊，每一題都用那個層次。
- 模型看不到的欄位（教師備註、`template_section_id`，以及之後加的欄位，例如圖片）全部沿用。
- **鷹架、字詞表、Glossary 也是這一區的一部分**（Issue：教師說「不要單字表格」三次，Manna 三次回答「這一區沒有單字表格」，因為以前送給模型的只有標題、指示與題目，輸出也沒有放鷹架的欄位）。範圍開放（整區）時模型可以：
  - 鷹架（`worksheet::revise::merge_scaffolds`）：`remove` 刪除；`change` 程序鷹架換 `steps`（空白步驟拿掉，1–12 步）、其他鷹架換 `content`（不可為空），id、種類、`levels`、標題保留，`origin` 改成 `ai`，內容沒變不算修改；`new` 新增，`kind` 只能是句型框架、引導問題、作答示例或清楚的分步指示及完成順序，`levels` 是這份學習單的版本，id 是 `<block>-p<proposal>s<k>`，一次最多新增 5 個。
  - 字詞表（`merge_words`）：`remove` 刪一列（每一列都 remove 就是整張表不見）；`change` 只能換 `meaning`（不可為空），**標了指示用語（`task_word`）的列解釋由程式決定，模型寫什麼都不採用**，字與級別不動；`new` 新增一個字：要是英文字或片語（≤60 字元）、有解釋、**真的出現在教材或這一區的文字裡**（容許詞尾變化）、表裡還沒有，**級別由程式查詞彙表（`vocabulary::word_level`），不用模型寫**，`paragraph_id` 與例句取自第一個出現的段落；一次最多新增 5 個。
  - Glossary（`merge_glosses`）：只能 `remove`。
  - 沒通過上面任一項的修改：該項維持原樣（新增的不加入）、列進重試的問題、`reply` 後面註明「（鷹架 S2 的修改沒有通過學習單的檢查，維持原樣。）」。
  - 範圍只有標在題目／標題／作答指示上的註記時，三者一律沿用，模型寫什麼都不採用。
- **「這一區沒有 X」的回覆檢查**（`false_absences`，沒有新的檢查名稱，併在 `checks` 的問題裡）：`reply` 在「沒有／找不到／看不到／不存在…」之後 6 個字內出現字詞表（單字表、字詞表、字彙表、word table…）、Glossary、程序鷹架（步驟）、句型框架、引導問題、作答示例或鷹架的名稱，而改完之後這一區**還有**那一部分 → 失敗：重試時會告訴模型「回覆說這一區沒有字詞表，但這一區有…」。「沒有改動鷹架」這類說明做了什麼的句子不算。重試後仍是 → 這個建議 `failed`，訊息說 Manna 誤以為這一區沒有那一部分、可以再改一次或直接按旁邊的「刪除」，**不會把那句話轉告教師**（有其他修改時 `reply` 改用預設的「已依你的註記修改這一區。」）。
- **範圍由程式決定**：有一句話、整區的註記或 `whole_block: true` → 整區都可以改（可以新增、刪除、調整順序；模型沒列出的題目就是刪除）。只有標在題目／標題／作答指示上的註記 → 只有那幾處可以改，其他題、標題、作答指示不論模型寫了什麼都沿用原樣，模型沒列出的題目放回原位，題目順序維持；新題目可以加，排在它在模型輸出裡跟著的那一題後面。
- 修改與新題目用照版型產生時的同一套檢查（`evaluate_section` 的每一題檢查：固定標籤、選擇題答案在選項內、題幹不含答案、需要教師素材時不得寫出網址或影片、音節依字典）。沒通過 → 重試一次；重試後還是沒通過的那一題維持原樣（新題目不加入），`reply` 後面註明「（第 5 題的修改沒有通過學習單的檢查，維持原樣。）」。
- 結果和原本完全一樣 → 沒有建議，`failed`：模型的 `reply` 是澄清問題時照實轉告（「Manna 沒有改動這一區：…」）；所有修改都沒通過檢查時說明。逾時、供應商錯誤同樣是 `failed` 並附原因。
- 伺服器重啟會中斷：`running` 超過 `2 × AI_TIMEOUT_SECS + 60` 秒的在下次讀取時結束為 `failed`。

## 註記給模型

- 逐區修改：只有這次用到的註記（見上）。
- **重新產生整份學習單**：所有待處理、提供給 AI 的註記，接在教材註記之後，標題寫明是「教師在上一版學習單上的註記…不是教材原文…不得寫進學生版」，每則寫出它的區塊標題與題號。Job 的「Manna 已帶入的資訊」把它算進「教師註記（提供給 AI）」。重新產生後原本的區塊不在了，註記的 `anchor.block_id` 變成 `null`，前端列在「找不到位置的註記」。
- 備課對話底部「改學習單」（`prep_revise`，整份）沒有變，不讀學習單註記。

## 編輯器存檔的版本檢查

編輯器的自動儲存一律帶 `base_version_no`（它上次看到的 `version_no`）。伺服器在鎖住學習單後比對：最新版本不是那一版、而且這次的 `content` 和最新版本不同 → 409「這份學習單在你編輯的同時有了新的版本（例如在備課對話採用了 Manna 的逐區修改），所以你最近的修改沒有存進去。請重新載入，再改一次。」，**標題、設定等其他欄位也不存**。送的內容和最新版本相同不算衝突。沒有 `base_version_no` 的請求照舊（後存的蓋掉先存的）。編輯器在送出修改與採用之前先存檔（`flush`）。

## 測試用假模型（`AI_PROVIDER=fake`）

假模型修改註記指到的第一題（沒有指定就是第一題）：題幹後面加 " (easier r<round>)"、Bloom 層次 `remember`；其他題 `keep: true`。標記寫在註記或指示裡：

| 標記 | 效果 |
|---|---|
| `FAKE_REVISE_TOUCH_ALL` | 每一題和標題都改（" (touched)"）：範圍限定時由程式還原 |
| `FAKE_REVISE_ADD` | 在最後加一題 |
| `FAKE_REVISE_DROP` | 少列最後一題：範圍允許時刪除，否則放回 |
| `FAKE_REVISE_BAD` | 兩次的修改都用了固定標籤以外的作答方式 → 維持原樣，`failed` |
| `FAKE_REVISE_BAD_ONCE` | 只有第一次如此，重試成功 |
| `FAKE_REVISE_NOTHING` | 全部 `keep`，`reply` 是一個問題 → `failed` 並轉告 |

既有的 `FAKE_TIMEOUT`、`FAKE_ERROR`、`FAKE_RUNAWAY`、`FAKE_SLOW` 照舊。

## usage_logs 新事件

`worksheet_comment_created`（`worksheet_id`、`comment_id`、`field`、`selection`、`share_with_ai`）、`worksheet_block_revise_started`（`worksheet_id`、`block_id`、`proposal_id`、`round`、`comments`、`instruction`（布林）、`whole_block`）、`worksheet_block_revise_accepted`（加 `version_no`）、`worksheet_block_revise_rejected`（加 `status`）。不含內容。

## 前端

- **備課對話的學習單預覽**：每一區的工具多了「改這一區」「加註記」，有待處理的 AI 註記時「依註記修改（n 則）」，採用過的區塊「復原上一版」。按「改這一區」，底部文字框前出現「針對 B 區 ✕」，送出就是這一區的修改（不是新的對話輪次）；拿掉標籤或切到「問 Manna」回到整份修改。選取學習單上的文字（標題、作答指示、題幹，或一題的選項）時，選取處下方浮出「已選取第 4 題的「…」 加註記」；選字會對齊整個英文字。新增註記可以「儲存註記」或「儲存並請 Manna 改」。修改進行中那一區顯示「Manna 正在改這一區…」，其他區照常；建議出現在那一區下方：修改的題目附原本的題幹（刪除線）、新增與刪除的題目有標示，下面「採用」「放棄」「再改一次」。採用後那一區重新載入並閃一下。註記列在所屬區塊下方，可以改內容、切換提供給 AI、略過或刪除。
- **編輯器（`/worksheets/:id`）**：區塊工具多了「改這一區」（區塊裡出現一個文字框）、「加註記」和「依註記修改」；每一題多了「註記」。建議與註記顯示在區塊上方；建議還在跑或等決定時，這一區不能編輯。自動儲存遇到 409 時，儲存狀態旁出現「重新載入」。
- 進入點：`web/src/components/worksheet/BlockRevise.tsx`（`useBlockRevise`、`ProposalPanel`、`NoteDraft`…）、`web/src/lib/blockRevise.ts`（`selectionOnSheet`）。

## 程式進入點

`routes::worksheet_revise`；`worksheet::comments`（錨點與註記）、`worksheet::revise`（schema、範圍、`merge`、建議的存取與 `accept`）、`worksheet::ai::revise`（背景呼叫與 prompt 內容）、`worksheet::checks::evaluate_revision`；`store::save_content_over`（存檔的版本檢查）。測試 `api/tests/worksheet_revise.rs`、`web/src/__tests__/blockRevise.test.tsx`。

# SIOP 目標：內容目標與語言目標（G1–G3）與完整學習單的 E1、Q4、V2

依據 `docs/SIOP_DESIGN_RULES.md`。`0022_siop_objectives.sql` 只新增 prompt 與鷹架，沒有改資料表。

## G1：兩個欄位

- 原本一格的「學習目標（選填）」拆成兩格：**內容目標**（`content_objective`）與**語言目標**（`language_objective`），都是選填，各 ≤300 字。一行一項（R1：各一項為原則，最多兩項；教師自己寫的不限制）。伺服器把每一行前後空白去掉、拿掉空行，用 `\n` 接起來；空的是 `null`（不存）。超過 300 字 →「內容目標最多 300 字。」／「語言目標最多 300 字。」
- 出現在：`WorksheetSettings`（`POST`／`PUT /api/worksheets` 的 `settings`，部分更新時送 `null` 清空）、`PrepSessionOptions`（`POST`／`PATCH /api/prep-sessions` 的 `options`）、`CreatePrepSessionRequest.options`。
- **舊資料**：`worksheets.settings`、`prep_sessions.options` 裡的 `objective` 讀成 `content_objective`（舊的一格沒有分內容或語言，範例與用法都是內容目標）；舊的前端送 `objective` 也一樣。回應只有新的兩個欄位。同時送 `objective` 與 `content_objective` 是 400（重複欄位）。資料庫裡的 JSON 不改寫，下一次存檔時才換成新名稱。
- `/create` 的六種產出不再把目標當成「教師的要求」再送一次：`request` 一律是工具名稱（不算教師的要求）。

### 帶進 prompt

| 變數 | 內容 |
|---|---|
| `{{teacher_content_objective}}` | 內容目標，多項用「；」接起來；沒有是「（沒有）」 |
| `{{teacher_language_objective}}` | 語言目標，同上 |
| `{{teacher_objective}}` | 兩組合成一行，給拆開以前的 prompt 版本：「內容目標：…｜語言目標：…」（只有一組就只有那一組）；沒有是「（沒有）」 |

- 每個帶目標的呼叫三個變數都有。prompt 版本的文字用到兩個新變數之一時，另一個也一定送到（沒寫在文字裡就接在訊息最後，標題「內容目標（教師寫的或採用的）」「語言目標（教師寫的或採用的）」），不再附 `{{teacher_objective}}`；兩個都沒用到時附 `{{teacher_objective}}`（標題「學習目標（教師寫的）」）。所以 `prep_activity` 第 3 版、`worksheet_bloom_level` 第 1 版照樣拿到兩組目標（合成一行），新版改用兩個變數即可。
- 各產生器的原則：教師寫的或採用的目標要**照原句使用，不要改寫**。

### 學習單的結構化目標（`content.objectives`）

- 建立學習單（`POST /api/worksheets`，或備課對話的學習單輪次）時，兩個欄位變成學習單的目標：`{ id: "teacher-c1" | "teacher-c2" | "teacher-l1" …, type: "content" | "language", text, student_facing_en（和 text 相同：印在學習單最上面「Today I can …」的那一行）, origin: "teacher", sop_id: null }`，接在從課例帶入的目標後面。
- 有這些目標、請求沒有指定 `settings.generation`、也沒有版型時，`generation` 加上「顯示學習目標」（T1：目標印在最上面，三個版本相同）。
- `PUT /api/worksheets/:id` 的兩個欄位有變（而且沒有正在產生）：學習單的 `teacher-*` 目標換成新的（請求有帶 `content` 就換在它上面，否則換在最新版本上，存成新版本）；原本沒有目標、現在有 → `generation` 加上「顯示學習目標」。課例帶入的目標、教師在學習單上自己加的目標不動。
- 產生時：學習單已有 `teacher-*` 目標 → 照學習單上的（教師可能在學習單上改過字）；沒有（拆開以前存的學習單）→ 依欄位加上。完整學習單的四個步驟以 `O1`、`O2`… 看到它們（`{{objectives_summary}}`，例如「O1（內容目標）：I can …」），不再另外附「學習目標（教師寫的）」；閱讀理解題／分層練習與照版型產生依上表拿到變數或合成的一行。產生出來的版本保留這些目標。
- 前端：學習單「要做什麼」頁的「學習目標」是兩格（`settings` 的兩個欄位）；「從課例帶入的內容」只列不是 `teacher-` 開頭的目標。

## G2、G3：教師沒寫目標時，Manna 先草擬

`POST /api/objective-drafts`（登入即可，同步回應，通常兩次呼叫約 10 秒）。

```ts
interface ObjectiveDraftRequest {
  kind: "comprehension" | "tiered" | "activity" | "kit" | "worksheet";   // 照我的學習單做是 worksheet；也接受 assessment
  material_id?: number;            // 自己的或平台的教材（要有文字；OCR 還在讀 → 409，同建立對話）
  material?: { title?: string; text: string };   // 貼上的文字：只用來草擬，不存進教材庫
  class_id?: number;               // 自己的班級，帶入班級特性（不是自己的 → 404）
  request?: string;                // 教師的要求（≤2000；只是工具名稱的不算）
  stage?: "content" | "language";  // 先決定內容目標，再依它擬語言目標：預設 "content"（只擬內容目標）；"language" 只擬語言目標
  content?: string[];              // stage "language" 必填：教師已決定的內容目標（1–2 句，取前 2 句，各 ≤300 字）；stage "content" 不可帶
  avoid?: string[];                // 「換一組」「換掉沒勾的」：要換掉的句子（最多 8 句），不要重複
  keep?: { code: "C1" | "C2" | "L1" | "L2"; text: string }[];   // 「換掉沒勾的」：教師留下來的目標（最多 3 個、每個代號一個；text 是她現在的句子，改過的也算），新的要和它們搭配；沒有 keep ＝ 整組重擬
}
interface ObjectiveDraftResult {
  interaction_id: number | null;   // 顯示的這一輪草擬；教師的決定記在這一筆
  status: "shown" | "blocked" | "timeout" | "error" | "disabled";
  message: string | null;          // 沒有草稿、或只剩一組時說明
  objectives: ObjectiveDraft[];    // 通過 R1 與 R3 的草稿（只有 stage 要的那一種）；語言目標學習策略在前
  hidden: number;                  // 這一輪沒有通過檢查、沒有列出的數量
  context: AiContextItem[];        // 「Manna 已帶入的資訊」
}
interface ObjectiveDraft {
  index: number;                   // 在這一次呼叫輸出裡的位置＝決定的 item_index（語言目標從 0 起算）
  kind: "content" | "language";
  code: string;                    // "C1" "C2" "L1" "L2"
  text: string;                    // 給學生的 "I can …"（採用時放進欄位的句子）
  zh: string;                      // 給老師的中文（以動詞開頭）
  category?: "strategy" | "function" | "structure" | "vocabulary";   // 語言目標才有（R2 的四類）
  supports?: string;               // 語言目標支持的內容目標代號
  relation_zh?: string;            // 語言目標才有：「學生要用……，才能……」
  check_zh: string;                // 下課前怎麼檢核
}
```

**先內容目標、再語言目標**：語言目標是為了達成內容目標，學生需要用什麼語言（要讀、聽、說、寫出什麼成果，老師看到成果就知道學生有沒有學會內容目標），所以內容目標決定之後才擬語言目標，每次請求只擬一種。第一次請求（`stage: "content"`）擬內容目標；教師決定之後，第二次請求（`stage: "language"`、`content` 帶她留下的內容目標，代號依序是 C1、C2）擬語言目標，`supports` 只能是這些代號，審查（R3）時它們當作已決定的上下文、不再判斷。`objective_draft` 第 2 版起 user template 有 `{{stage_note}}`（說明這次要寫哪一組、語言目標要依哪些內容目標推導）；一次只擬一種，所以輸出 schema 另一組的陣列是空的（`maxItems: 0`）。前端：「請 Manna 擬內容目標」與「產生」（兩格都空）先擬內容目標，按「用勾選的 N 個內容目標，請 Manna 擬語言目標」才擬語言目標（或「只用勾選的 N 個內容目標」略過）；語言目標欄旁的「請 Manna 依內容目標擬語言目標」在內容目標欄有字時才能按。

缺教材或貼上的文字是空的、`kind` 不對、`stage` 不是 content／language、`stage: "language"` 沒有 `content`（或 `stage: "content"` 帶了 `content`）、`keep` 的代號不對／重複／超過 3 個／句子是空的 → 400；沒有開啟 AI → 200 `status: "disabled"`（前端直接產生）。

**流程**（`objectives::draft`；每次呼叫各一列 `ai_interactions`，教材只記字數與雜湊）：

1. **草擬**（`objective_draft`，鷹架 `SC-OBJ-DRAFT`，`max_tokens` 1500）。輸入：用途（工具，例如「閱讀理解題（依 Bloom 認知層次出題…）」；不同工具的請求因此不同）、學校類型、班級特性、教師的要求、這位老師最近 3 次草擬的語言目標類別、以教材做混合檢索的前 8 個課綱條目、加段落標記的教材（全部段落，不設上限；`paragraph_ids` 選了的話只有選的）。輸出 schema（欄位照撰寫順序，每個字串都有上限）：`{ content_objectives: [1–2] { zh ≤100, en ≤180, check_zh ≤100 }, language_objectives: [1–2] { category（enum 四類）, supports（"C1"｜"C2"）, zh ≤100, en ≤180, relation_zh ≤160, check_zh ≤100 } }`——先寫內容目標，再從內容目標推出語言目標。
2. **R1 規則檢查**（程式，`objectives::check_rules`），每一項各自判斷，不合格的拿掉：`en` 是英文、以 "I can" 開頭（"Students will be able to …" 改成 "I can …"）、不超過 30 個字、主要動詞（I can 後的第一個字與 and／or 後的字）不是 learn、know、understand 等看不到表現的動詞；`zh` 有中文、開頭（去掉「學生能」）不是了解、理解、知道、學會…；語言目標的類別是四類之一、`supports` 是通過的內容目標、`relation_zh` 寫成「學生要……才能……」；不重複。**R2**：有學習策略時排到語言目標的第一個——除非最近兩次草擬的語言目標都以策略為主（這時 prompt 請模型這次改用其他三類，讓一個單元的語言目標有變化；「單元」以這位老師最近的草擬近似，備課對話沒有單元）。規則檢查後一組都沒有時，同一次呼叫**重試一次**（temperature +0.4，附上問題）。
3. **R3 審查**（`objective_check`，鷹架 `SC-OBJ-CHECK`，`max_tokens` 900），和 `worksheet_bloom_check` 一樣一次呼叫逐項判斷：`{ items: [正好 n 筆] { id（enum：這一輪的代號）, reason ≤100, problems: ("not_core" | "not_assessable" | "activity" | "not_readable" | "not_needed" | "no_relation")[] } }`。後兩種只用於語言目標（用在內容目標上不算）。有任何問題的草稿**不顯示**；語言目標支持的內容目標沒通過，它也不顯示；審查漏掉的項目重試一次後仍沒判斷的不顯示；審查呼叫失敗 → 這一輪都不顯示（G3）。
4. **再草擬一次**：這次要的那一種沒有通過的草稿時，伺服器再草擬一輪（附上沒通過的原因，並列出已通過、可以照原句保留的目標），一樣經過 2、3。有要的那一種的那一輪優先，否則留下通過較多的一輪。沒有被選的那一輪的草擬記錄標成 `blocked`。
5. 顯示的那一輪的草擬記錄：有草稿是 `shown`，沒有是 `blocked`；`check_results` 記下 `r1_rules`、`r3_judge`、每一項的審查（`verdicts`）、顯示了哪些（`shown`）與程式的修改（`repairs`）。

**教師的決定**：用既有的 `POST /api/ai/:interaction_id/decision`（`interaction_id` 是回應的那一筆、`item_index` 是草稿的 `index`）：

| 教師 | 送出 |
|---|---|
| 採用（或沒選、按「全部採用並產生」） | `{ decision: "adopt" }` |
| 修改（改草稿的句子） | `{ decision: "revise", revised_content: 改後的句子 }`（沒改字就是 adopt） |
| 改寫（不要這句，自己寫） | `{ decision: "reject", reason: "better_idea", revised_content: 自己的句子 }`；留空＝不放這一項：`{ decision: "reject" }` |
| 不用目標，直接產生 | 每一項 `{ decision: "skip" }` |

**每一項各自勾選**（前端）：每一項有「使用」勾選（預設勾；沒勾的變淡、還是可以改）。按鈕「用勾選的 N 個目標(產生)」只用勾了的（沒勾任何一項時按鈕關閉，說明「不用目標，直接產生」才是出口）；沒勾的目標不放進欄位。決定照每一項的狀態送：沒勾＝`skip`；勾了沒改字＝`adopt`；勾了改了字＝`revise`；勾了但清空＝`reject`（和以前一樣）。只勾了一種目標時（只有內容目標或只有語言目標）在按鈕旁提醒 R1（兩種各寫一個以上，語言目標用來支持內容目標），不擋——是否採用由教師決定（G2）。

**換掉沒勾的**：有勾也有沒勾的時候，「換一組」變成「換掉沒勾的」，只重擬沒勾的。請求帶 `avoid`（沒勾的句子）與 `keep`（勾了的，句子是她現在的版本）；伺服器照常整組草擬並經過 R1、R3 檢查，`{{avoid_note}}` 另外說明留下來的目標（照教師的句子、不重複、不衝突、語言目標仍要說明支持哪個內容目標），不需要新的 prompt 版本。前端把新的草稿放進沒勾的位置（同一種、先取代號相同的），勾了的原地不動（改過的字也留著）、全部重新勾選。紀錄：沒勾而被換掉的在換的當下記 `skip`（記在原本那一輪）；留下的在最後用的時候記 `adopt`／`revise`（也記在原本那一輪，`item_index` 是原本的）；新的記在新的那一輪。`objectives_drafted` 事件多 `kept`（留下的代號），`again` 也是 true。全勾或全沒勾時還是「換一組」（整組重擬，沒有 `keep`）；換不成（失敗、沒有草稿）就維持原樣並說明，不記任何決定。

決定送出後才建立備課對話，目標放在 `options.content_objective`／`language_objective`；記錄失敗不擋產生。

**前端**：`/create` 的六種工具與首頁的快速輸入（選了工具、不是直接問 AI 時）在兩個目標都空著時，按「產生」／送出先草擬：出現「Manna 先幫你草擬目標」，每一項顯示學生版句子、中文、（語言目標）類別與「學生要用…才能…」、怎麼檢核，可以採用、修改、改寫或重選；「全部採用並產生」一鍵完成；另有「不用目標，直接產生」「換一組」（有勾有沒勾時是「換掉沒勾的」）「收起」。教師自己寫了任何一個目標就不草擬（在草擬時開始寫，草擬就收起）。`usage_logs` 新事件 `objectives_drafted`（`kind`、`status`、`shown`、`hidden`、`again` 等，沒有文字）。

## 完整學習單的三條規則

### E1：Exit ticket 檢核所有目標

- `worksheet_exit_ticket` 第 2 版：「教師確認的目標」的每個代號（內容與語言目標都算）至少出現在一題的 `objective_refs`，一題可以檢核兩個目標；檢核語言目標的題目要學生真的用到那個語言。
- 檢查新增 `covers_every_objective`：有目標沒被任何一題檢核 → 修復一次（附上沒檢核到的代號）；還是沒有 → 照樣使用，並在 `content.notices` 加一行「Exit ticket 沒有檢核到語言目標「I can …」。可以在 Exit ticket 加一題，或按它的「重新產生」。」（重新產生 Exit ticket 也一樣）。

### Q4：逐段任務的順序

- `worksheet_tasks` 第 3 版（第 1、2 版保留）：每個區塊多一個 `practice`（schema 是四個固定標籤的 enum，寫在 `info_function` 之後）：示範 → 共同練習 → 同儕練習 → 獨立應用，依區塊順序不可倒退；第一個區塊示範（老師放聲思考先做第一題）、最後一個獨立應用；兩個區塊時是示範、獨立應用；四個以上區塊時四個都要有。同一版也要求每個語言目標由一個示範或共同練習的區塊教到（R4）。
- 檢查新增 `practice_in_order`（只在整份產生時；在依段落重排區塊之後判斷，階段跟著區塊走）：不合 → 修復一次，並先由程式依位置重新標好（`Practice::for_position`：1 個區塊＝獨立應用；2 個＝示範、獨立應用；3 個＝示範、同儕練習、獨立應用；4 個以上依比例分成四段）；修復後仍不合就用程式標的。對其他檢查沒有影響：題數、資訊處理功能、段落涵蓋與順序都照舊（示範區塊的示範寫在作答指示，不寫在題目裡，所以「題幹不可含答案」不受影響）。
- `WsBlock.practice?: "model" | "guided" | "peer" | "independent"`（只有完整學習單的活動區塊有；其他沒有這一欄）；區塊的 `teacher_notes` 第一行是這個階段的教學說明（例如「教學順序：示範——老師先放聲思考，做第一題給學生看，再讓學生接著做。」）。單一區塊重新產生保留原本的階段（prompt 也會說明）。

### V2：作答指示的功能詞

- 程式（`worksheet::task_words`，不呼叫模型）在完整學習單產生後（四個步驟；照版型與閱讀理解題／分層練習不做）找出學生要照著做的文字（區塊的作答指示、題目、分步驟指示）裡的學術功能詞，例如 compare、summarize、explain、infer、support your answer、in your own words、according to、main idea、give reasons、evidence（固定的 30 個詞與它們的詞形），依出現順序最多 8 個，每個附固定的中文提示（例如 compare「比較（找出相同與不同）」），列在暖身的字彙表（關鍵字後面；沒有暖身時放在第一個活動區塊）。已經是關鍵字的不重複列。
- `WsVocab.task_word: true`（一般關鍵字沒有這一欄），`definition` 是中文提示，`level` 照詞彙表；學習單投影的 `SheetWord` 也有 `task_word`。重新產生區塊後依當時的指示重列。編輯器的「級別」欄顯示「指示用語」。
- 學生版因此會有中文（這些提示）：伺服器沒有中文字型時，學生版 PDF 也會像教師版一樣回 409（訊息改成「…例如教師備註或字彙的中文提示…」）；Docker 映像有安裝中文字型。

## 測試用假模型（`AI_PROVIDER=fake`）新增的標記（寫在教材文字裡）

| 標記 | 效果 |
|---|---|
| `FAKE_OBJ_TWO` | 草擬兩個內容目標與兩個語言目標，學習策略在第二個（伺服器排到第一個） |
| `FAKE_OBJ_VAGUE` | 第一次草擬的內容目標用 understand（規則拿掉，連同它的語言目標，同一次呼叫重試） |
| `FAKE_OBJ_R3` | 第一輪審查把語言目標判成 `not_needed`（伺服器草擬第二輪，通過） |
| `FAKE_OBJ_R3_ALWAYS` | 每一輪審查都把語言目標判成 `no_relation`（只顯示內容目標） |
| `FAKE_BLOCK` | 草擬的學生版句子都是中文（沒有可用的草稿） |
| `FAKE_WS_EXIT_PARTIAL` | Exit ticket 兩次都只檢核第一個目標（E1：加提醒） |
| `FAKE_WS_PRACTICE_OFF` | 逐段任務第一次全部標成獨立應用（Q4：修復後照順序） |
| `FAKE_WS_PRACTICE_ALWAYS` | 兩次都全部標成獨立應用（伺服器依位置重新標） |
| `FAKE_WS_NO_BLANK` | 逐段任務第一次的第一個填空題是完整的句子、沒有空格（修正呼叫後有空格） |
| `FAKE_WS_NO_BLANK_ALWAYS` | 兩次都這樣（伺服器把句子裡的參考答案換成 `______`） |
| `FAKE_WS_BLANK_LOST` | 兩次的第一個填空題都沒有空格，也沒有參考答案的字（伺服器改成簡答題） |
| `FAKE_WS_EXIT_FRAME` | Exit ticket 第一次的第一題只有句型框架（修正呼叫後是一般的題目） |
| `FAKE_WS_EXIT_FRAME_ALWAYS` | 兩次都這樣（框架印在 Exit ticket 的 Sentence frames 方框，這一題改成請學生用框架作答） |

## 程式進入點

`objectives`（欄位、變數、`content.objectives`、R1 規則、R3 審查的檢查）、`objectives::draft`（草擬流程）、`objectives::fake`、`routes::objectives`；`worksheet::checks`（`covers_every_objective`、`practice_in_order`）、`worksheet::task_words`、`worksheet::Practice`。測試 `api/tests/objectives.rs`、`web/src/__tests__/objectives.test.tsx`。

# 產生進度（進度表）

Migration `0027_generation_progress.sql`。產生東西時（學習單、閱讀理解題、差異化學習單、課堂活動、教學套件、評量、修改學習單、草擬學習目標），頁面顯示一個「進度」表：依序列出這次真正會做的步驟，做完的變灰、正在做的有橘點、還沒做的照常顯示；失敗的那一步附上原本就會顯示的錯誤文字。步驟由伺服器在工作進行時寫入（每換一步寫一次），不是前端的計時器。

## 型別

```ts
interface ProgressStep {
  key: string;      // 同一次產生裡不變：read、tasks、warmup、scaffolds、exit_ticket、template、bloom:<level>、bloom_check、pictures、curriculum、plan、siop、sample、revise、save、draft、check…
  label: string;    // 給老師看的繁體中文（見下表）
  status: "pending" | "running" | "done" | "failed";
  note?: string;    // 進行中的補充：「重試中」（同一步的第二次呼叫）、「重出層次不符的題目」、「再判斷一次」、「辨識掃描檔」
  count?: { done: number; total: number };  // 由好幾件組成的一步：配圖中（3／6）、照版型寫各區塊（2／8）、讀範本時的 OCR 頁數
  message?: string; // failed 的一步：原本的錯誤文字（例如工作的 message、這一輪的 error）
}
interface RunProgress {
  steps: ProgressStep[];       // 依執行順序；略過的步驟（沒勾暖身、圖片關閉…）不列出
  current: number | null;      // 正在做的那一步（教學套件兩部分同時進行時是第一個）；結束後是 null
  status: "running" | "done" | "failed";  // 還有步驟在做或還沒做 → running；之後有一步失敗 → failed，否則 done
}
```

回傳的地方（都是頁面原本就在輪詢的回應）：

- `WorksheetState.progress`（`GET /api/worksheets/:id`、產生與重新產生的 202）與 `GET /api/worksheets/:id/jobs` 的 `progress`：這份學習單**最近一次**產生或單區重新產生的步驟。
- `PrepTurn.progress`（`GET /api/prep-sessions/:id` 等回傳整個對話的端點）：這一輪自己的步驟、它的學習單第一次產生的步驟、照版型的輪次在等的範本，以及 SIOP 自我檢核。**只有頁面還用得到的輪次才有**：還在進行的輪次、SIOP 自我檢核還在跑的輪次、結束不到 60 秒的輪次（頁面看得到結果：全部完成或失敗的那一步），以及做了學習單、結束時間還在圖片工作的逾時上限內（圖片開啟時約 8 分鐘：兩倍 `IMAGE_TIMEOUT_SECS` 加 120 秒）的輪次（配圖可能還在畫）。其他輪次一律 `null`，不為它們查資料庫；需要的那幾輪的學習單步驟與圖片張數各用一個查詢一起讀（`WHERE id = ANY(…)`），所以對話再長，每次輪詢多出的查詢最多兩個。頁面載入時已經結束的產生本來就不顯示進度表。
- `POST /api/objective-drafts` 多一個選填的 `progress_id`（頁面自己取，8–64 個英數、`-` 或 `_`；格式不對 400）；等待時 `GET /api/objective-drafts/progress/:progress_id` → `{ progress: RunProgress | null }`（只看得到自己的；還沒開始、別人的鍵或結束 15 分鐘後是 `null`）。

`null`：進度功能之前做的學習單與輪次（欄位是 NULL）、還沒產生過的學習單、找教學資源與直接問 AI。頁面這時照舊顯示原本的「Manna 正在…」與工作列表。

## 存在哪裡

- `worksheets.progress`（JSONB）：`{ run_no, steps, pictures, finished }`。`run_no` 對應 `worksheet_jobs.run_no`；不是最近一次的就不回傳。`pictures` 是這次排進去的 `worksheet_images` id，讀的時候算還在畫的張數（配圖那一步在文字完成後仍可能是 running，頁面繼續輪詢到畫完）。
- `prep_turns.progress`（JSONB）：`{ steps, finished }`，只有課堂活動、教學套件的活動規劃與修改學習單這三種自己跑的輪次。學習單類的輪次讀它的學習單（只取第一次產生，之後在編輯器裡的重新產生屬於學習單本身）；SIOP 自我檢核讀 `siop_checks`。
- 草擬學習目標是一個同步請求，進度只放在記憶體（伺服器重啟就沒有，請求本身也一起中斷）。
- 工作死在伺服器重啟裡（工作被 stale 規則關掉、步驟沒有結束）：讀取時把當時在做的那一步標成 failed（「這次產生中斷了…」或這一輪的錯誤）。讀取從不寫資料庫：開著太久（超過圖片工作的 stale 上限）的圖片算畫完了。
- 寫入：每換一步寫一次，沒有改變就不寫。完整學習單約 12 次（開始 1、讀教材完成 1、四步各開始與完成 8、記下排進去的圖 1、結束 1；修正呼叫的「重試中」每次多 1）；照版型約 7＋區塊數次（每完成一個區塊 1 次，最多 16 個區塊）。配圖進度從 `worksheet_images` 讀，不寫。

## 各種產出的步驟

| 產出 | 步驟 |
|---|---|
| 完整學習單 | 讀教材 → 寫逐段任務 → 寫暖身（有勾或沒有設定時）→ 寫鷹架 → 寫 Exit ticket → 配圖（圖片開啟時；沒有排到圖就拿掉） |
| 照版型／照我的學習單做 | （備課對話：讀你的學習單範本，OCR 時附頁數）→ 讀教材 → 照版型寫各區塊（n／m，三個同時寫）→ 配圖 |
| 閱讀理解題 | 讀教材 → 每個層次一步「出『記憶』題」…（依 Bloom 順序，一層一次呼叫）→ 判斷 Bloom 層次（重出層次不符的題目、再判斷一次寫在 note） |
| 差異化學習單 | 讀教材 → 每個層次「出『X』題與三版支持」（三個版本的支持在同一次呼叫裡寫，沒有另外一步）→ 判斷 Bloom 層次 |
| 單區重新產生 | 讀教材 → 重寫這一區的任務 → 寫鷹架 → 配圖；暖身：重寫暖身 → 寫鷹架；Exit ticket：重寫 Exit ticket；照版型：照版型重寫這一區 → 配圖；Bloom：重出『X』題 → 判斷 Bloom 層次 |
| 課堂活動 | 讀教材 → 找課綱條目 → 規劃教學活動（重試寫在 note）→ SIOP 自我檢核 |
| 教學套件 | 讀教材（兩部分共用一步）→ 找課綱條目 → 規劃教學活動 → 差異化學習單的各層次 → 判斷 Bloom 層次 → SIOP 自我檢核（活動規劃與學習單同時進行） |
| 評量、做學習單（備課對話） | 同完整學習單，最後加 SIOP 自我檢核（評量沒有） |
| 修改學習單 | 讀學習單 → 照你的指示修改 → 存成新版本 |
| 草擬學習目標（/create、首頁） | 讀教材 → 找課綱條目 → 草擬學習目標 → 檢查目標（第二輪時加「重新草擬沒通過的目標」「再檢查一次」） |

失敗的輪次：失敗的那一步之後還沒做的步驟不列出；沒有任何一步失敗時（例如教學套件的學習單沒有題目）把原因放在最後一步或「做差異化學習單」。

## 前端

`RunProgressPanel`（`web/src/components/RunProgress.tsx`）：標題「進度」、× 關閉（只關這一次產生）、`<ol>` 每步一行、正在做的那一步 `aria-current="step"`、`role="status"` 的 polite live region 說「進行中：寫暖身（第 3 步，共 5 步）」「全部完成」「沒有成功：寫暖身」。全部完成後停留約 4 秒再收起；失敗會一直留著直到老師關掉。頁面載入時已經結束的產生不顯示（頁面本來就會說結果）。顯示在備課對話底部輸入列、學習單頁的產生進度處，以及 /create 與首頁的草擬目標面板。

## 測試用假模型

教材文字裡放 `FAKE_PACED`：每一次呼叫先等 400 ms（在 `ai::execution::attempt`），測試可以一步一步看到進度（`api/tests/progress.rs`）。

# 我的作品：卡片預覽

`/prep`（我的作品）預設是卡片格狀：每張卡片上方是一張 A4 直式的預覽框。沒有資料表變動。

## 對話列表多兩個欄位

`GET /api/prep-sessions`（和 `GET /api/prep-sessions/:id` 的 `PrepSessionSummary` 部分）每個對話多：

- `preview_worksheet: { id, version_no } | null`：這個對話最近一個做出學習單的輪次的學習單（沒有被刪除的），與它最新的版本；沒有學習單（課堂活動、問 Manna、找教學資源）→ `null`。卡片用它的 `id` 去要預覽圖；`version_no` 讓前端在版本變了時換掉網址參數。
- `preview_text: string | null`：最後一輪的幾行字，卡片沒有學習單圖片、或圖片載入失敗時用（≤200 字，行與行之間 `\n`）：課堂活動與教學套件是計畫的標題與前 3 個步驟（`1. …`）；問 Manna 是老師的問題，接著回答的開頭；其他是老師的要求（沒有要求時是按鈕名稱）。輪次還在跑或失敗時沒有計畫／回答，就只有要求。

只有一個查詢：`prep_sessions` 左接兩個 `LATERAL` 子查詢（各自 `LIMIT 1`，走 `prep_turns (session_id, seq)` 的唯一索引），沒有 N+1。舊的客戶端會忽略這兩個欄位。單獨存在的學習單（沒有對話）用 `GET /api/worksheets` 既有的資料，卡片直接要 `/api/worksheets/:id/thumbnail`；它沒有文字預覽，失敗時卡片顯示標題。

## `GET /api/worksheets/:id/thumbnail`

學生版第 1 頁的預覽圖：最新版本、預設程度（這份學習單的第一個程度）、學生版，**含圖片**；走和 PDF 匯出同一條路（`export::pdf_with_images`，配上圖庫），再用 `pdftoppm` 取第 1 頁、縮到寬 480 px 的 JPEG（和教材預覽圖同一組函式，`state.thumb_slots` 限制同時進行的份數）。

- 只有學習單的主人看得到（別人的或不存在 → 404，同其他學習單端點）。
- **這不是匯出**：不寫 `worksheet_exported`，也不寫任何其他使用記錄。
- 學習單還沒產生，或這一刻還有產生工作在跑 → **409**（「這份學習單還沒產生…」「這份學習單還在產生中…」）。卡片這時顯示文字預覽（或標題），**不會重試**。伺服器沒有中文字型而學習單有中文 → 409。
- 沒有 `pdftoppm`、沒有檔案儲存空間 → 503，訊息同教材預覽圖；轉不出圖 → 404（「這份檔案轉不成預覽圖…」）。
- **快取鍵**（檔案儲存空間）：`thumbs/worksheets/<id>/v<version_no>-p<完成的圖片數>-<最新一張圖的 id>-<標題與教材文字的雜湊前 8 碼>-w480.jpg`。圖片是產生之後才陸續完成的（不會產生新版本），換圖、拿掉圖、改標題或教材文字也不產生新版本，所以鍵除了版本，還帶這個「印出來的頁面可能變了」的指紋；任何一項變了就是新的鍵、重新轉一次。
- **驗證**：`ETag: "w<id>-v<version_no>-<指紋>-w480"`，`Cache-Control: private, max-age=0, must-revalidate`，`Vary: Authorization`；`If-None-Match` 相符 → 304（不轉圖、不讀儲存空間）。每次都問一次，沒變就 304。
- **刪除**：刪除學習單（`DELETE /api/worksheets/:id`）時，盡力刪掉「目前狀態」的那一張預覽圖；失敗只記警告（`removing a worksheet preview failed`），不擋刪除。**舊版本、舊指紋的預覽圖不會清掉**（儲存空間沒有列出前綴的功能，它們也只是幾十 KB）。

測試：`api/tests/mywork_previews.rs`（沒有 poppler 的機器上，需要圖片的部分改查 503；CI 會裝 poppler 並檢查圖片）。

## 復原與修改歷程（`0034_append_only_history.sql`）

**每一步都保留，復原不刪任何東西。**

- `worksheet_versions` 本來就只新增。新欄位：`source JSONB`（這個版本是什麼做的）與 `pictures JSONB`（那一步每個 slot 目前用的圖，`{ slot_id: worksheet_images.id }`，那一步的圖片都處理完才寫一次）。舊的版本兩欄都是 `null`。`cause` 多一個 `restore`（保留給「回到這一步」）。`GET /api/worksheets/:id/versions` 的每一項多 `source`。
- `source` 一定有 `cause`，再加上相關的編號：

  | `source.cause` | 其他欄位 |
  |---|---|
  | `block_proposal` | `proposal_id`、`interaction_id`、`round`、`block_ids`（`worksheet_versions.cause = "revise"`） |
  | `block_proposal_pictures` | 同上；只改了圖片的建議，內容與前一版相同（`cause = "revise"`） |
  | `prep_turn` | `turn_id`、`interaction_id`、`block_ids`（備課對話整份修改，`cause = "revise"`） |
  | `regenerate` | `interaction_id`、`block_ids` |
  | `vocabulary` | `block_ids` |
  | `undo` | `history_id`、`proposal_id`、`interaction_id`、`undone_version_no`（被復原的那一步寫成的版本）、`from_version_no`（復原前的最新版本）、`block_ids` |
  | `generate`、`edit`、`created` | 只有 `cause` |

- `worksheet_block_history`（復原用的堆疊）新欄位：`proposal_id`、`version_no`（那一步寫的版本）、`after_block`（那一步寫入的區塊，採用才有）、`picture_undo`（`{ slots: [{ slot_id, added, before, after }] }`：那一步動到的圖片位置，前後各用哪一列 `worksheet_images`）、`undone_at`、`undone_in_version_no`。只有 `undone_at` 為 `null` 的列才是堆疊（`WorksheetState.undoable`）。`worksheet_block_proposals.undone_at`：採用後被復原的時間（`status` 維持 `accepted`），`WsProposal.undone_at`。
- **採用現在一定是一步**：只改圖片的建議（以前不寫版本、不進歷史）也寫一個內容與前一版相同的版本（`source.cause = "block_proposal_pictures"`）和一列歷史，前提是圖片真的有變；圖片動作全部被略過就什麼都不寫。`accepted_version_no` 是那個版本。
- **復原（`POST …/blocks/:block_id/undo`）**，一個交易：取這一區最新一列沒復原的歷史 → 若那一列有 `after_block` 而這一區現在和它不同 → **409「這一區在採用之後又改過了，沒辦法直接復原到採用前；先把這一區改回採用後的樣子，再按復原。」**；若那一步動過的圖片現在已不是它留下的那一張（又重畫、上傳或移除過）→ **409「這一區的圖片在採用之後又動過了，沒辦法直接復原到採用前。」**；學習單正在產生 → 409。通過後：區塊（文字與圖片位置）換回採用前的樣子；圖片：那一步新增的 slot 隨區塊拿掉，重畫或移除的 slot 把**前一列** `worksheet_images` 設回 `current = TRUE`（現在那一列設為不是目前的；仍在畫的那一張結束）——**不呼叫圖片模型，也不刪任何一列**；寫新版本（`cause = "undo"`，內容沒變也寫）、`pictures` 快照、歷史那一列記 `undone_at`／`undone_in_version_no`、提案記 `undone_at`；有 `interaction_id` 時記一筆 `teacher_decisions` reject（「學習單：復原上一版」，和以前一樣）。重新產生、備課對話修改、字詞動作寫的歷史沒有 `after_block`，復原的行為和以前完全一樣。
- 研究紀錄：`usage_logs` `worksheet_block_undone` 的 payload：`{ worksheet_id, block_id, version_no, from_version_no, undone_version_no, proposal_id, interaction_id, pictures_reverted }`。
- 前端：採用之後，那一區下方出現「已採用 Manna 的修改。」與「復原」（`role=status`，焦點移到這則訊息），直到這一區又被任何動作改動（版本號變了）、再改、重新產生或復原；區塊工具裡的「復原上一版」照舊。

## 修改歷程與採用過往的版本（`0039_block_history_browse.sql`）

老師說「每一步修改的歷程都要保留」「可以採用過往拋棄的選項」。每一區的歷程是從**已經只新增的紀錄**推出來的，瀏覽不寫任何東西；只有「採用」會新增一個版本。

### 一步是什麼

| `kind` | `label` | 從哪裡推出來 |
|---|---|---|
| `original` | 原本 | 這一區第一次出現的版本（`created`／`generate`） |
| `proposal_adopted` | Manna 的提案（你採用） | `source.cause = block_proposal`／`block_proposal_pictures`（舊的 `revise` 版本若有提案的 `accepted_version_no` 指向它也算）；`proposal_id`、`undone`（採用後被復原） |
| `proposal_rejected` | Manna 的提案（沒採用） | `worksheet_block_proposals` 裡 `status` 為 `rejected`／`stale` 且有 `proposed` 的列（放棄、被「再改一次」放到一旁、過期）。**不是版本**：`version_no = null`、`proposal_id` 有值；`block` 是它會寫成的區塊，`picture_actions` 是還沒做過的圖片動作。還在等你決定的提案不列（在區塊下方處理） |
| `regenerate` | Manna 重新產生 | `regenerate`，或不是第一次的 `generate` |
| `prep_revise` | 備課對話修改 | `prep_turn` |
| `vocabulary` | 換字 | `vocabulary` |
| `manual` | 你手動修改 | `edit`。**間隔 5 分鐘以內的連續手動修改合成一步**（編輯器隨打隨存），顯示最後的樣子，較早的版本號在 `merged_version_nos`，仍可用 `version_no` 採用 |
| `pictures` | 改圖片 | 老師自己按的 重畫／拿掉／換成我的圖（見下） |
| `undo` | 復原 | `undo` |
| `restore` | 回到這一步 | `restore`（採用的結果） |

版本只在**這一區或這一區圖片位置的圖和前一版不同**時才算一步（別區的修改不會出現）。

### 老師自己按的圖片按鈕也是一步

`POST …/image-slots/:slot_id/regenerate`、`…/upload`、`DELETE …/image-slots/:slot_id` 成功之後，各寫一個內容與前一版相同的新版本：`cause = "pictures"`、`source = { cause: "pictures", action: "regenerate"|"upload"|"remove", block_id, block_ids, slot_id, image_id, hint }`。（採用提案或對話框 `revise` 做的圖片動作不另寫，屬於那一步。）為了不讓這些版本擋住編輯器，`PUT /api/worksheets/:id` 帶 `base_version_no` 時，只要「編輯器看到的那一版的內容」和最新版內容相同就不算 409。注意：詞彙檢查綁版本號，按過圖片按鈕後檢查會顯示過期，需重新檢查。

### 每一版都留圖片快照

`worksheet_versions.pictures`（`{ slot_id: worksheet_images.id }`）現在每個新版本寫入時就一併記下，產生／重新產生後才排進佇列的新圖會補進最新版（`enqueue`），備課對話修改的圖片動作做完後重寫。舊版本是 `null`：預覽的 `pictures = null`，採用時圖片維持現在的樣子（`adopt.pictures_unknown = true`）。

### API

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/api/worksheets/:id/blocks/:block_id/history` | `{ block_id, version_no, busy, steps: Step[] }`，新的在前。`busy`：Manna 正在產生學習單或在改這一區（暫時不能採用）。區塊不在最新版 → 404。記 `worksheet_block_history_viewed` |
| POST | `/api/worksheets/:id/blocks/:block_id/adopt` | `{ version_no }` **或** `{ proposal_id }`（兩個都給或都沒給 → 400）→ `{ version_no, kind: "version"\|"proposal", pictures: PictureOutcome[], worksheet: WorksheetState }` |

`Step`：`kind`、`label`、`at`、`version_no`、`proposal_id`、`current`（這一區現在就是這樣）、`detail`（一行說明）、`merged_version_nos`、`undone`、`adopted_later_version_no`、`block`（預覽用的區塊；提案是它會寫成的區塊）、`pictures`（`[{ slot_id, image_id, status, source, purpose_zh }]`，`image_id` 只有 `done` 才有，圖用 `GET /api/worksheets/:id/images/:image_id`；沒有紀錄時 `null`）、`picture_actions`、`adopt`：`{ possible, clean, warn_older, draws_pictures, pictures_unknown, reason }`。`clean` = 不用警告也不用先畫圖。

### 採用

一個交易、鎖學習單：

- **過往的版本**：這一區換回那一版的樣子（整個區塊），圖片：該版快照裡每個圖片位置的那一列 `worksheet_images` 設回 `current`（現在的那一列保留、不再是目前的；那一版沒有這個位置的圖就不留圖）。**不呼叫圖片模型、不刪任何一列。** 就是現在的樣子 → 409。
- **沒採用的提案**（`proposal_id`；已採用的 → 409「請在歷程裡找它採用後的那一步」；還在等決定 → 409）：提案的區塊換進來，新的圖片位置（`<block>-xh<新版本號>n<k>`）成為版本的一部分，之後和「採用」一樣執行 `picture_actions`（畫新圖、重畫、移除；結果在 `pictures`）。介面會先說「會用到 AI 產圖」。提案的 `before` 與現在這一區不同時 `adopt.warn_older = true`，介面先警告：「這個版本是根據比較早的內容改的，採用後會取代這一區之後的修改（之後的修改仍保留在歷程裡，可以再換回來）。」伺服器本身不擋。
- 寫新版本 `cause = "restore"`，`source = { cause: "restore", adopted_kind: "version", adopted_version_no, from_version_no, block_ids }`（提案是 `adopted_kind: "proposal"`、`proposal_id`、`interaction_id`、`round`、`warned_older`）；`worksheet_block_history` 多一列（`version_no`、`after_block`、`picture_undo`，所以「復原上一版」可以把這次採用也復原）；`pictures` 快照寫最後的狀態。
- 提案：`worksheet_block_proposals.adopted_later_at`／`adopted_later_version_no`（`status`、`decided_at` 維持當時的樣子）；`teacher_decisions` **新增**一列 `adopt`（原本的 `reject` 保留）。
- 學習單正在產生 → 409「Manna 正在產生這份學習單，好了之後再採用。」；這一區有提案在跑 → 409。別的老師的學習單 → 404。
- 研究紀錄：`worksheet_block_version_adopted`：`{ worksheet_id, block_id, from_version_no, version_no, kind, adopted_version_no, proposal_id, warned_older, pictures_restored }`；`worksheet_block_history_viewed`：`{ worksheet_id, block_id, steps }`。

### 前端

編輯器每區工具列與備課對話卡片的「修改歷程」按鈕開啟共用 `Dialog`（`BlockHistory.tsx`）：左邊步驟清單，右邊預覽（文字、圖片縮圖）、「‹ ›」逐步看、「採用這個版本」；需要警告或會畫圖時先在對話框內確認「仍要採用／先不要」。「已採用 Manna 的修改。〔復原〕」與「復原上一版」照舊。

## 改圖片與附上圖片（`0032_revise_pictures_and_uploads.sql`）

老師在備課對話說「B 區加一張蜜蜂在花上的圖」「這張圖重畫，不要有文字」，或附上截圖說明要改哪裡。兩條修改路徑（對話框的 `revise` 輪次、逐區的 `worksheet_block_revise`）的輸出多了 `picture_actions`，prompt 各多一版（`COALESCE(MAX(version_no),0)+1`：在當下最新的文字後面接上圖片規則與 `{{picture_rules}}`、`{{pictures_note}}`、`{{attachments_note}}`）。模型只提出動作，伺服器檢查，**採用／同意之前什麼都不畫、不移除**。

### `picture_actions` schema

`array, maxItems = 3`（沒開啟 AI 產圖時 `0`，prompt 也告知），每項（依寫出順序，全部必填）：

| 欄位 | 說明 |
|---|---|
| `action` | enum `add`／`redraw`／`remove` |
| `block_id` | `add` 用：要加圖的區塊（enum：這份學習單的 block_id 加 `""`；逐區修改只有這一區）；其他給 `""` |
| `slot_id` | `redraw`／`remove` 用：enum 為「目前的圖片」列出的 slot_id 加 `""` |
| `kind` | enum `scene`／`object`／`icon` |
| `purpose_zh` | `add`：這張圖做什麼（教師的語言）；`redraw`：這次要改什麼（教師的語言） |
| `prompt_en` | `add`：給產圖模型的英文畫面描述（≤300）；`redraw`：英文簡短說明要怎麼改（≤200，當作「Teacher's note」） |
| `caption` | `add`：圖說（≤80） |

模型讀到的「目前的圖片」：turn 層級是 `blocks_json` 每個區塊的 `pictures: [{slot_id, number, purpose, caption, prompt, state}]`；逐區修改是訊息裡這一區的圖片清單。規則文字用 `worksheet_image_slots` 最新版（圖裡不要字、不要真實人物與品牌、不要網址）；沒開啟產圖時改告知「picture_actions 一律給空陣列」。

### 伺服器檢查（`worksheet::images::actions::check`）

- 沒開啟 AI 產圖 → 全部丟掉，`reply` 後面加「（這個環境沒有開啟 AI 產圖，圖片的部分沒有處理。）」。
- 動作最多 3 個（多的丟掉）；每張圖最多一個動作；逐區修改只能動這一區。
- `add`：區塊要存在且啟用；內容走 `clean_slots`（字數上限、不可有網址、要有畫面描述、與已有的重複就丟掉）；一個區塊最多 2 個圖片位置（`MAX_SLOTS_PER_BLOCK`；「拿掉」的位置仍佔位）。`redraw`／`remove`：slot 要在這份學習單。`redraw` 的說明不可有網址、≤200 字。
- 丟掉的動作不會觸發重試；數量寫進 `reply`（「有 N 個圖片動作沒有通過檢查」）。通過的動作帶一行給老師看的 `text`：「加一張圖：…（B 區）」「重畫第 2 張圖：…（B 區）」「移除圖：第 1 張圖「…」（B 區）」。

### 確認流程

- **逐區修改**：`WsProposal.picture_actions`（`PictureAction[]`，含 `text`）隨建議一起顯示；只有圖片動作的建議也能採用（`block` 與原區塊相同、`changes` 全為 `kept`）。**採用**時，同一個交易把 `add` 的新 slot 加進區塊（和文字修改同一個新版本；沒有文字也沒有新 slot 時，圖片處理完才寫一個內容相同的版本，圖片真的有變才寫），交易後依序：`add` 的圖排進產圖佇列（`enqueue`，受每份學習單的自動產圖上限與 `IMAGE_CONCURRENCY` 限制）、`redraw` 走 `regenerate`（受 `max_regenerations` 限制）、`remove` 走 `remove`。回應 `{ proposal, pictures: PictureOutcome[], worksheet }`。放棄 → 什麼都不做。
- **對話框 `revise` 輪次**維持原本的做法：修改直接套用（寫成一個新版本，編輯器可以復原），所以圖片動作也**直接**執行（新 slot 與文字改動同一個版本，之後排進佇列；redraw、remove 緊接著做），結果寫在 `result.pictures` 與 `reply`（「圖片：加了 1 張圖、重畫了 1 張圖（畫好會自動出現）」）。`remove` 與 `重畫` 不會進版本歷史（和老師自己按按鈕一樣），要找回圖片按「重畫」。（逐區修改採用的圖片動作則會記錄，採用後可以復原，見「復原與修改歷程」。）
- `PictureOutcome = { action, text, status: "queued" | "removed" | "skipped", message }`。`skipped` 的原因（重畫已到上限、這張圖正在畫、沒有儲存空間、這一區已滿…）寫在 `message`；絕不讓整個修改失敗。畫好的圖照原本的方式出現（`GET /api/worksheets/:id/images` 輪詢）。
- 研究紀錄：`ai_interactions.output` 照舊存模型的原始輸出（含 `picture_actions`）；套用時寫一筆 `usage_logs` `worksheet_picture_actions_applied`：`{ worksheet_id, source: "prep_turn" | "block_revise", ref, add, redraw, remove, queued, removed, skipped }`。

### 附上圖片

先上傳、再送出（和教材、圖片的上傳一樣用 multipart 與 `state.storage`）：

| 方法 | 路徑 | 說明 |
|---|---|---|
| POST | `/api/prep-sessions/:id/images` | multipart 欄位 `file`：PNG／JPEG／WebP（副檔名與檔頭都要對，GIF 不收），≤ 5 MB（讀到超過就停）。轉正、長邊縮到 ≤ 1568 px、重新編成 JPEG（去掉 EXIF）後存在 `prep-images/s<session>/<隨機>.jpg`。201 `PrepImage { id, session_id, width, height, bytes }`。錯誤：副檔名不對／內容對不上／讀不出來／太大 → 400；還沒送出的圖已有 4 張 → 409；沒有儲存空間 → 503；別人的對話 → 404 |
| GET | `/api/prep-sessions/:id/images/:image_id` | 圖（JPEG，`private`、`nosniff`），只有該老師 |
| DELETE | `/api/prep-sessions/:id/images/:image_id` | 移除還沒送出的圖；已附在訊息上 → 409 |

送出：`POST /api/prep-sessions/:id/turns` 與 `POST /api/worksheets/:id/blocks/:block_id/revise` 多一個 `image_ids`（≤ 2；必須是這位老師、這個對話、還沒附到訊息上的；逐區修改用學習單所屬備課對話裡的圖，學習單不是在備課對話做的 → 400；超過 → 400「一則訊息最多附 2 張圖片」；`kind` 不是 `revise` 卻帶 `image_ids` → 400）。圖片附到該輪次（`prep_images.turn_id`）或該建議（`proposal_id`）；`PrepTurn.images` 與 `WsProposal.images` 回傳 `PrepImage[]`。24 小時沒人送出的圖由下一次上傳順手刪掉。刪除備課對話時先刪儲存空間裡的圖（失敗就不刪對話），資料列隨對話一起刪（`ON DELETE CASCADE`）。

**給模型**：圖是 user 訊息的圖片內容：OpenAI 相容 API 送 `[{type: "text"}, {type: "image_url", image_url: {url: "data:image/jpeg;base64,…"}}]`（沒有圖時仍是純字串，其他呼叫不受影響）；Anthropic 送 `image` 區塊（base64）接著文字。訊息文字多一句「教師附上了 N 張圖片」，prompt 說明圖片是用來指出要改哪裡、不要把學生的臉孔或姓名寫進學習單。輸出照舊用 strict JSON schema（沒有用 tool call）。

**研究紀錄**：`ai_interactions.input_snapshot.prep.attached_images = [{ id, bytes, original_bytes, width, height, sha256 }]`，沒有圖片內容；使用事件 `prep_image_uploaded`（大小與尺寸）、`prep_turn_created.images`、`worksheet_block_revise_started.images`。

### 測試用假模型新增的標記

| 標記 | 效果 |
|---|---|
| `FAKE_REVISE_NO_WORDS`／`NO_SCAFFOLDS`／`NO_GLOSS` | 逐區修改：題目全部 keep，刪掉每一列字詞表／每一個鷹架／每一筆 Glossary |
| `FAKE_REVISE_FRAME`／`ADD_FRAME` | 逐區修改：第一個句型框架與第一個程序鷹架的第一步加「 (reworded)」／新增一個句型框架 |
| `FAKE_REVISE_ADD_WORD`（加 `_BAD`） | 逐區修改：字詞表新增 umbrella（加 `_BAD` 時另有教材裡沒有的 zebra） |
| `FAKE_REVISE_CLAIM_NONE`（`_ALWAYS`） | 逐區修改：回覆說「這一區沒有單字表格」（只有第一次呼叫；重試時刪掉字詞表／兩次都說） |
| `FAKE_PREP_PIC_ADD`／`REDRAW`／`REMOVE` | revise：另外提出加圖（第一個有題目的區塊）／重畫第一張圖（「no text in the picture」）／移除最後一張圖 |
| `FAKE_PREP_PIC_BAD` | revise：四個不合格的動作（不存在的區塊、不存在的 slot ×2、含網址） |
| `FAKE_PREP_PIC_MANY` | revise：每個有題目的區塊兩個 `add`（超過 3 個，伺服器只留前 3 個） |
| `FAKE_PREP_PIC_ONLY` | revise：沒有文字修改，只有圖片動作 |
| `FAKE_REVISE_PIC_ADD`／`REDRAW`／`REMOVE`／`BAD`／`ONLY` | 逐區修改的同類標記 |
| （自動）請求帶圖片 | 假模型在 `reply` 後面加「（收到 N 張圖片）」 |

測試：`api/tests/revise_pictures.rs`；網頁：`web/src/__tests__/prepImages.test.tsx`。

### 沒做的

- 附圖只用在 `revise` 與逐區修改，「問 Manna」不收圖。逐區修改的建議卡只顯示附上的小縮圖（網頁）。
- 模型不能新增超過一區 2 個位置，也不能真正刪除位置（「移除」是不印）。
- 沒有用 tool call（gemma 支援，但其他呼叫都是 strict JSON schema，維持一致）。

## 教師長期記憶（Issue #82）

Phase 1 的確認／修改／停止使用與不可覆寫歷程，以及 Phase 2 的可重建 Graphiti 索引、
研究開關、模型轉發及完整端點契約，見 [TEACHER_MEMORY.md](TEACHER_MEMORY.md)。摘要：

| 方法／路徑 | 說明 |
|---|---|
| `GET /api/me/memories` | 本人全歷程（每項有 `kind`、`scope`；`kind=custom` 為自由習慣）、`enabled`（研究開關）、`max_active`（100）、`scopes`（適用範圍代碼與名稱）、`pending`（對話中偵測、還沒回答的建議）、`graph_enabled`、`index`（未設定索引時 `null`） |
| `POST /api/me/memories/preview` | `{text}`（1–1000 字）→ `{id, statement, kind, scope, replaces_id, replaces_statement}`；單次要求、含個人資訊、不支援 400 |
| `POST /api/me/memories` | `{candidate_id, confirmed: true}` → `{id}`，與有效習慣相同時 `{id, unchanged: true}`（不新增版本）；`{candidate_id, confirmed: false}` 取消預覽或「不用」→ `{ok, cancelled: true}`（記 `teacher_decisions` reject）；加 `edit: true`（「修改後記住」）記為 revise；過期／已確認／已取消／過時 409；已有 100 項有效習慣且不是取代時 409 |
| `GET /api/me/memories/candidates` | 對話中偵測到、等老師回答的建議（15 分鐘內）：`{candidates: [{id, kind, scope, statement, replaces_id, replaces_statement, source, prep_session_id, prep_turn_id, worksheet_id, proposal_id, created_at, expires_at, intro}]}`；可加 `prep_session_id` 或 `worksheet_id` 篩選。`source` 為 `prep_turn`、`block_revise` 或 `worksheet_request` |
| `POST /api/me/memories/candidates/{id}/shown` | 頁面已顯示這則建議（研究紀錄 `memory_candidate_shown`，只記一次） |
| `DELETE /api/me/memories/{id}` | 停止使用（只標失效） |
| `GET`／`PUT /api/admin/memory-settings` | PI；`{enabled}`，預設 false，切換寫 `usage_logs` |
| `GET /api/admin/memory-index` | PI；`{pending, failed, graph_enabled, providers_ready, compatible, sidecar}`，未設定索引時計數為 0 |
| `POST /api/admin/memory-index/rebuild` | PI；從正本完整重建（一般變更只索引被改的習慣） |

- `graph_enabled` 代表部署接線完整；`providers_ready` 依目前生效（含管理員覆蓋）的文字模型與 embedding 設定計算；`compatible` 獨立表示 sidecar 健康與介面相容。provider 不可用時仍保留 outbox 與待同步數，恢復可用後自動接續。
- 研究開關關閉時，任何產生步驟的提示詞與學習單檢查都不變。開啟時只有出題／內容產生步驟
  （`memory::HABIT_STEPS`）附加版本化提示詞 `teacher_memory_context`，`ai_interactions.input_snapshot.teacher_memory`
  記錄習慣、附加的原文（`system_appended`／`user_appended`）、提示詞版本與 Graphiti 排序 metadata。
- 選擇題／配對題的選項數檢查：預設不變；開關開啟且有確認的選項數習慣時，另外接受該數目。
- 對話中偵測（研究開關開啟時）：老師在備課對話打的訊息（含首頁快速輸入）、逐區修改的修改指示、建立學習單時的
  「教師的要求」，先經確定性的前置篩選（之後／以後／每次／一律／from now on…），命中才在背景呼叫
  `teacher_memory_extract`（使用該工作固定的模型，獨立一筆 `ai_interactions`，`input_snapshot.source` 記出處）。
  只有判斷為長期、支援、不含個人資訊的習慣才產生候選；候選一律要老師按「記住」才進入記憶。
  不處理教材、附件與註記。
- 自由習慣（`kind=custom`）：一句繁體中文（≤120 字）加 `scope`（`content`／`format`／`layout`／`language`／`scaffold`／`workflow`／`all`）。
  每位老師最多 100 項有效習慣；封閉類型各自仍只有一項有效值。
- 研究紀錄：`memory_detection_run`、`memory_candidate_detected`、`memory_candidate_shown`、`memory_confirmed`、
  `memory_candidate_dismissed`、`memory_candidate_edit_requested`、`memory_candidate_expired`（皆不含老師的文字）；
  `teacher_decisions` 記 adopt／reject／revise／no_response。
- `input_snapshot.teacher_memory.selection` 記錄這一步實際帶入與被排除的習慣、排序方式與分數。
- 模型轉發 `POST /api/internal/memory/v1/chat/completions`、`/embeddings` **不在公開連接埠**：
  只由 `INTERNAL_PORT`（預設 8081，設定索引時才開啟）提供，需 `X-Manna-Service-Token`（`GRAPHITI_TOKEN`）、
  job token、`X-Manna-Memory-Api-Version` 與領取時的 `X-Manna-Graphiti-Version`；欄位白名單、
  `n=1`、`max_tokens ≤ 8192`、embedding batch ≤ 64、每工作 300 次。

## 文字模型設定（後台管理 → 平台設定 → 文字模型）

`AI_PROVIDER`、`AI_MODEL`、`AI_EFFORT`、`AI_TIMEOUT_SECS` 現在是**預設值**：主持人可以在後台管理切換正式環境的文字模型，不必改 Coolify 環境變數、也不必重新部署。後台設定存在 `app_settings.ai_text_model`（`{provider, model, effort, timeout_secs}`），有就蓋過環境變數；刪除（「恢復環境變數預設」）就回到環境變數。金鑰與位址（`OPENAI_API_KEY`、`OPENAI_COMPAT_BASE_URL`、`OPENAI_COMPAT_API_KEY`）仍只能由環境變數設定。

可以切換：模型來源（`openai`＝OpenAI，需要 `OPENAI_API_KEY`；`openai_compatible`＝國網中心，需要 `OPENAI_COMPAT_BASE_URL`）、模型名稱、推理強度（只有 `openai` 用得到）、等待上限秒數。**不能**切換：embedding、OCR、圖片；也不能選 `anthropic`、`fake`、`none`（`anthropic` 受 `DEMO_MODE` 與境外資料規定限制，伺服器一律拒絕）。環境沒有設定金鑰或位址的來源不能選。

| 方法／路徑 | 說明 |
|---|---|
| `GET /api/admin/ai-model` | PI。`{active, source, changed_by, changed_at, env_default, providers, efforts, timeout_range}`。`active`／`env_default` 為 `{provider, model, effort, timeout_secs}`；`source` 是 `env`（環境變數預設）或 `admin`（後台設定，此時 `changed_by` 為設定者的研究編號、`changed_at` 為時間）；`providers` 是 `[{id, label, available, models, uses_effort}]`（`models` 是建議名稱：`openai` 為 `gpt-5.6-sol`、`gpt-5.6-terra`，`openai_compatible` 為 `gemma-4-31B-it`，再加上環境變數預設的模型；也可以自行輸入）；`timeout_range` 為 `{min: 30, max: 900}` |
| `PUT /api/admin/ai-model` | PI。`{provider, model, effort?, timeout_secs}`。先驗證（來源、模型名稱 1–100 字元且只含英數與 `. - _ : /`、`effort` 屬於 `low／medium／high／xhigh／max`、秒數 30–900；沒給 `effort` 沿用環境變數的值），再用**新設定**做一次很小的測試呼叫（最多等 120 秒）。測試成功才儲存並立刻生效，回傳與 GET 相同的內容加上 `test_latency_ms`；測試失敗或驗證不過回 400（中文原因，例如模型名稱可能打錯），**什麼都不儲存**，目前的模型不變 |
| `DELETE /api/admin/ai-model` | PI。刪除後台設定，回到環境變數預設，回傳與 GET 相同的內容；沒有後台設定時什麼也不做 |

- **生效時機**：新開始的 AI 工作立刻使用新設定；已經在跑的工作（例如正在產生的學習單）會用開始時的模型跑完，不會半途混用。每次呼叫與每個背景工作（學習單產生、備課對話的一輪及其 SIOP 自我檢核、版型讀取、區塊修改、課程 SOP 分析與目標、記憶索引同步）在開始時取一份設定快照，請求、逾時與 `ai_interactions` 的記錄都用這一份；一個備課對話的輪次（含事後由頁面輪詢觸發的自我檢核）沿用輪次開始時的設定。使用者在頁面按「再試一次」是新的工作，用當時的設定。伺服器重新啟動時會讀回後台設定（沒有就用環境變數），並在啟動記錄寫明使用哪一種來源；若後台設定的來源已不在環境中（例如金鑰被移除）會被忽略並改用環境變數。目前只有一個 API 實例，切換與這些快照都存在該實例的記憶體；若日後有多個實例，需要各自重新讀取 `app_settings`。
- **研究記錄**：每筆 `ai_interactions` 的 `provider`／`model` 是該次呼叫快照裡實際使用的值。測試呼叫也記成一筆，`module = admin_model_test`（`teacher_id` 是主持人、`input_snapshot` 是固定的測試題目與新設定、失敗時 `status = error／timeout` 並有 `error_detail`），不會混進教師的流程。
- **稽核**：每次儲存或恢復預設都寫 `usage_logs`（`event = setting_changed`、`payload = {key: "ai_text_model", from, to}`；儲存另有 `test_latency_ms`，恢復另有 `reset_to_env`）。
- `GET /api/config` 的 `ai_provider`、`ai_model`、`ai_enabled` 反映目前生效的設定。
- 逾時改成每次呼叫由快照決定，所以「執行中工作被視為中斷」的門檻（`n × AI_TIMEOUT_SECS + 60`）改用**自啟動以來用過的最長逾時**計算，避免主持人縮短逾時後把還在跑的工作誤判為中斷。
- 記憶索引（Graphiti）：每個索引工作在被領取時固定設定，模型轉發（sidecar 另開的 HTTP 請求）依工作的權杖查出該工作固定的來源、模型、推理強度與逾時，工作進行中切換不影響它。

# 逐步設計（首頁第三張卡片）

首頁多一張卡片「我想逐步設計」：到 `/lessons/:id` 的七步課例工作區（`LessonPage`）一步步設計。它不是備課對話，也不是產生器的工具。選它再送出會開一個對話框：選現有單元（`GET /api/units`）或建立新單元（`POST /api/units`），再給課例名稱，然後 `POST /api/units/:id/lessons`；首頁附上的文章用 `PUT /api/lessons/:id/sop/material` 放進第 1 步（貼上的文字 `{ title, text }`，教材庫的 `{ material_id }`），最後前往 `/lessons/:id`。沒附文章就直接開在第 1 步。

- `POST /api/units/:id/lessons` 可帶 `entry?: "home"`：由首頁卡片開始。其他值一律忽略（記成 `null`），不是錯誤；舊的客戶端不帶也照常。`entry` 為 `"home"` 時課例標為逐步設計（資料表 `lessons.stepwise`，migration 0043）。
- 研究紀錄：每次建立課例都記 `lesson_created`（`lesson_id`、`unit_id`、`entry`、`ui_version`）；`entry` 是 `"home"` 時 `ui_version` 為 `4`，否則兩者為 `null`。只記研究用 id，不記名稱。
- `GET /api/stepwise-lessons`：我的作品用。只回登入教師自己（單元屬於他、未刪除）的課例，且是「首頁卡片開始的課例」或「已有存過的 SOP 版本（教材或任一步）的課例」；只有舊教案、沒在工作區開過的課例不列。依 `updated_at`（最近一次 SOP 存檔，沒有則為建立時間）新到舊。每筆 `{ id, title, unit_id, unit_title, updated_at, steps_done, next_step }`：`steps_done` 是七步裡已有內容的步數（0 到 7），`next_step` 是第一個還空著的步驟（1 到 7，對應 `STAGES`），七步都有內容時為 `null`（顯示「已完成」）。「有內容」與工作區進度欄的 `stageSummary` 同一套規則。沒有刪除課例的 API，所以卡片也沒有刪除。
- 「我的作品」把這些課例和備課對話、獨立學習單混排（依更新時間），卡片標籤「逐步設計」、副標是「單元名稱 · 第 N 步：步驟名稱」或「已完成」；多一個篩選晶片「逐步設計」。

---

# 我的範本：刪除範本連同原檔

依據 `docs/BLUEPRINT.md` §13.3（教師的教材只屬於她；刪除就是真的刪除原檔與衍生資料）。以前刪除教師自己的範本（`DELETE /api/worksheet-templates/:id`）只刪範本，為它上傳的學習單（`purpose = worksheet_sample` 的教材，不在「我的教材」裡）留在原處，教師在任何地方都看不到、也刪不掉；只有平台範本會帶走它的平台範例。沒有資料表變動，也沒有刪除資料的 migration。

## 刪除範本時，原檔一起刪除

`DELETE /api/worksheet-templates/:id`（以及管理端的 `DELETE /api/admin/library/templates/:id`）回應照舊 `{ ok: true }`。範本的範例（`source_material`）**同時符合**以下條件時一起真的刪除（`worksheet::template::store::SAMPLE_GOES_SQL`，兩條路徑同一個規則）：

- 是範例（`purpose = worksheet_sample`），不是課文教材；
- 和範本同一區：教師自己的範本 → 她自己的個人範例；平台範本 → 平台範例。教師「另存成我的範本」的版本用的是平台的範例，**刪除她的版本不會動到平台的檔案**；
- 沒有其他範本在用（例如另存的版本、平台範本）。同一份範例有好幾個範本在用時，跟著**最後一個**範本一起刪除。

順序（`routes::templates::remove`，一個交易）：

1. 先鎖住範例與範本（範例先鎖，和刪除範例本身時 `ON DELETE SET NULL` 寫範本的順序相同），鎖住之後才判斷上面的條件；同一時間用這份範例新建的範本（外鍵會鎖範例那一列）要等這次刪除結束。
2. **先刪儲存空間裡的原檔與預覽圖**（`materials::store::remove_files`）。刪不掉（儲存空間沒有設定或暫時無法使用）→ 交易還原，**什麼都不刪**，回原本的 503「原檔沒有刪除成功（儲存空間暫時無法使用），所以這份教材還沒有刪除。請稍後再試一次。」（或「這個環境目前沒有連上檔案儲存空間…」）。
3. 同一個交易內：範例的真的刪除（`materials::store::delete_rows`：教材列與文字、段落、註記、連結；SOP 與 AI 紀錄裡的逐字引文移除；`material_deletions` 一筆；`usage_logs` `material_deleted`），再刪範本與所有版本，`usage_logs` `worksheet_template_deleted` 多一個 `deleted_sample_id`（一起刪除的範例 id，沒有時是 `null`），然後提交。所以範本不會指向一份刪到一半的範例。萬一提交失敗而原檔已經刪掉，資料列都還在，再送一次同樣的刪除就會完成（找不到的原檔不算錯）。

用這個範本做過的學習單照舊保留內容，只是 `template` 變成 null。

`WsTemplateDetail` 多一個欄位，讓確認畫面說得準確：

```ts
interface WsTemplateDetail { /* …既有欄位… */
  deletes_source_material: boolean;   // 刪除這個範本時 source_material 也會一起永久刪除（條件同上）
}
```

## 還有範本在用的範例不能單獨刪除

`DELETE /api/materials/:id`：教師自己的（個人）教材還有範本用它當範例時 → **409**，不刪任何東西，訊息指出是哪個範本：「這份檔案是範本「L2 學習單版型」的原檔，範本還在用它。要刪除的話，請刪除那個範本，原檔會一起刪除。」（好幾個範本時：「這份檔案是 2 個範本（「A」、「B」）的原檔…原檔會跟著最後一個範本一起刪除。」）。以前這樣刪會讓範本失去原檔。平台教材的規則不變（管理員刪除後，用它做的範本繼續可用，`source_material` 變成 null）。

## 沒有範本在用的原檔

這次修正之前刪除的範本留下的範例，以及上傳了卻沒有做成範本的範例（例如讀取失敗後被刪除、上傳後取消），都還在。不自動刪除，列給教師自己決定：

| Method | Path | 回應 |
|---|---|---|
| GET | `/api/materials?purpose=worksheet_sample&orphaned=true` | 和 `GET /api/materials` 相同的 `{ materials, usage, limits, storage, uploads_enabled }`；`materials` 只有**自己的**個人範例中**沒有任何範本在用**的，新到舊。`orphaned=true` 沒有配 `purpose=worksheet_sample` → 400 |

刪除走既有的 `DELETE /api/materials/:id`（真的刪除，同「刪除」一節）；別人的 → 404。

## 前端

- **我的範本的卡片**（`/templates?tab=mine`）：和我的教材一樣，卡片上有「更多動作」（⋯）→「刪除」。只出現在自己的範本；平台範本沒有（平台範本在「教材與範本管理」由 `pi`／`kb_manager` 管理）。按下後開確認視窗（`role="alertdialog"`，標題「永久刪除「…」？」），讀取範本詳細後說明會刪掉什麼，取消時焦點回到 ⋯。
- **確認文字**（卡片與範本頁「刪除這個版型」共用 `templateDeleteConsequences`）：「刪除後無法復原：這個版型的所有版本都會刪除。用它做過的學習單會留著，只是不再連到版型；原本上傳的檔案「…」也會一起永久刪除。」；別的範本還在用時是「原本的檔案「…」還有別的範本在用，所以會留著。」；沒有原檔時只有前兩句。
- **「沒有範本在用的原檔」**：我的範本卡片下方的小區塊，列出上面的清單（標題、檔名與上傳日期、「查看」在側邊檢視器打開原檔或文字、「刪除」用和我的教材相同的確認視窗）。沒有任何一份（或讀不到清單）時整個區塊不出現。後端拒絕（409、503）時，訊息顯示在確認視窗裡。

---

# 長教材、斷行的貼上文字與「回上一步」

來源：老師從 PDF 複製一篇文章貼上，每一個折斷的行都成了一段（一篇 10–20 段的文章變成 132 段），產生被「超過 40 段」擋下，而且找不到改課文的地方。

## 貼上文字的分段

見「教材與段落」。貼上與 TXT 沒有空行時，一行一行折斷的文字會接回段落（沿用 PDF 的規則）；有疑問就每行一段。

## 不再有段數上限

以前 `MAX_AI_PARAGRAPHS = 40`：學習單產生（`POST /api/worksheets/:id/generate`、單一區塊重新產生、備課對話的學習單輪次）與 SOP 分析回 400，目標草擬與備課活動只讀前 40 段。現在**都不擋、不截斷**：

- 學習單：`worksheet::ai::check_material` 只要求至少一段。「完整學習單」的出題步驟的 `max_tokens` = `1500 + 600 × 段數`，最少 3000、最多 12000（`tasks_max_tokens`）；分層練習與閱讀理解的預算依題數，不依段數。
- SOP 分析：`analysis_max_tokens`（`1000 + 200 × 段數`，6000–12000）。
- 目標草擬（`objective_draft`）、備課活動／問答（`prep_activity`、`prep_ask`）：全部段落都標 `[第 n 段]` 送出。
- 教材庫瀏覽 `fits_lesson=true`：有文字、至少 1 段。
- 輸出被截斷或某段沒被涵蓋時，照原本的檢查與看得見的錯誤處理（缺段重做一次、仍缺的列出；`failed` 的輪次顯示訊息），不會默默少掉。
- 常數 `worksheet::LONG_MATERIAL_PARAGRAPHS = 40`（網頁的 `LONG_MATERIAL_PARAGRAPHS`）只決定「比一般一節課長」的提醒。

## 提醒：這份課文比較長

教材超過 40 段、而且沒有「只選要用的段落」時，網頁在產生**之前**顯示（不擋、可以關）：

> 這份課文有 N 段，比一般一節課長；產生會比較久，後面的段落也比較可能寫得簡略。

三個動作：「修改課文」、「只選要用的段落」、「照樣產生」（照原樣送出；對話頁與學習單頁的「照樣產生」＝知道了，學習單頁的會同時開始產生）。出現的地方：

- 首頁快速框：附上文章後、送出前（貼上的文字用 `POST /api/materials/split-preview` 問段數；教材庫的教材用 `paragraph_count`）。
- 備課對話頁：教材的 `material_paragraphs` > 40 且沒有選段落時，顯示在「接下來可以」上面。
- 學習單頁：選項頁的產生鈕上面。

## 修改課文

- 貼上的教材（`origin = "paste"`）：`PUT /api/materials/:id` `{ title?, text, edit_from? }`（`edit_from`：`home`／`prep_session`／`worksheet`／`library`，只寫進研究紀錄，其他值忽略）。就地修改並重新分段，沒變的段落沿用 `id`。上傳的檔案不能改文字（原本的 400 訊息）：網頁改提供「只選要用的段落」或「換一份教材」／「回上一步」。
- 首頁：貼上的文字還沒存進教材庫，「修改課文」重新開啟貼上視窗（文字還在）；已在教材庫的用文字編輯視窗。
- 備課對話：編輯視窗存好後，對話讀的是教材庫的最新文字；選過的段落若有被改掉，只留還在的（全沒了就改回全部）。失敗的輪次的按鈕變成「重新產生」。
- 學習單頁：學習單的教材是一份複本，編輯後網頁再呼叫 `PUT /api/worksheets/:id/material-selection` 重新取一次複本（保留還在的選段）。

## 只選要用的段落

選的是這一次備課對話或這一份學習單要用的段落，**教材本身不變**。

| 位置 | 儲存 | 讀取 |
|---|---|---|
| 備課對話 | `PrepSession.options.paragraph_ids`（`PUT /api/prep-sessions/:id/material-selection`；也可在 `POST /api/prep-sessions` 的 `options.paragraph_ids` 一開始就給，教材要是教材庫的教材） | 之後每一輪都讀選的段落（含「重新產生」）；對話做出的學習單的教材複本也只有這些段落（`material.source.selected_ids`） |
| 學習單 | `WorksheetState.material.source.selected_ids`／`total_paragraphs`（`PUT /api/worksheets/:id/material-selection`；`POST /api/worksheets` 也可帶 `paragraph_ids` 配 `material_id`） | 產生、重新產生、檢查都只用這些段落 |
| 目標草擬 | `POST /api/objective-drafts` 的 `paragraph_ids`（配 `material_id`） | 只讀這些段落 |

- `paragraph_ids` 是教材庫那份教材的段落 `id`（`MaterialDetail.paragraphs[].id`），照教材的順序使用；`null` 或空陣列＝全部。最多 1000 個；不在教材裡的 `id` → 400「選的段落不在這份教材裡。請重新選一次。」
- `PrepSession` 多兩欄：`material_paragraphs`（教材有幾段，沒有教材是 `null`）與 `material_selected`（選的段落還在教材裡的有幾段；全部都用是 `null`）。
- 備課對話的 `PATCH` 送 `options` 時，已存的 `paragraph_ids` 保留（選段落只用上面的 `PUT`）。
- 學習單是唯一一份「複本」：沒有教材庫來源（`source.material_id` 是 `null`，例如空白教材）→ 400，請用「換教材」貼上要用的部分。學習單產生中 → 409。
- 選了之後，學習單的建議（`suggested`）與標題照新的教材重算；教師自己改過的設定保留（和「換教材」相同）。

## 失敗或提醒時的路：回上一步

任何一個因為教材而失敗或提醒的地方（長教材提醒、教材沒有文字、失敗的輪次、學習單產生失敗）都有**回上一步**，而且輸入不會不見：

- 首頁：快速框（文字、附的教材、範本、選的卡片、選的段落）每次改動都存進 `sessionStorage`（以瀏覽器歷史這一筆的 `location.key` 為鍵），所以送出後按瀏覽器的「上一頁」，回到的是填好的快速框。建立對話成功時另存一份給那個對話（`manna:prep-origin:<id>`）。
- 備課對話頁，失敗的輪次的按鈕：**回上一步**（回首頁，快速框照送出時的樣子填好；沒有那份暫存時——換了瀏覽器——用對話的第一個要求與教材重建）、**修改課文**、**只選要用的段落**、**再試一次**（教材改過之後叫**重新產生**）。對話標頭也有「修改課文」「只選要用的段落」。
- 首頁在建立對話之前失敗（例如教材沒有文字）：快速框原樣保留，錯誤下面有「修改課文」「換一份教材」「再試一次」。
- 學習單頁：產生失敗的訊息旁有「修改課文」「只選要用的段落」「再試一次」，從備課對話來的另有「回上一步」（回對話）。選項頁的設定沒有任何改變。教材欄也常駐「修改課文」「只選要用的段落」。
- 不在這個範圍：`/create/:tool` 的完整表單不暫存（它的「回上一步」是瀏覽器的上一頁）。

## 研究紀錄

不記錄任何文字，只記大小。

| 事件 | 時機 | 內容 |
|---|---|---|
| `material_text_edited` | `PUT /api/materials/:id` 且文字真的變了 | `material_id`、`from`（`home`／`prep_session`／`worksheet`／`library` 或 `null`）、`paragraphs_before`、`paragraphs_after`、`chars` |
| `prep_material_selection` | `PUT /api/prep-sessions/:id/material-selection` | `session_id`、`material_id`、`total`、`selected`、`all` |
| `worksheet_material_selection` | `PUT /api/worksheets/:id/material-selection` | `worksheet_id`、`material_id`、`total`、`selected`、`all` |
| `objectives_drafted` | （既有）多一欄 `selected`：是否只讀選的段落 | |

## 社群（第一期；Migration `0052_community.sql`）

設計、隱私與研究紀錄見 [COMMUNITY.md](COMMUNITY.md)。全部需要登入。主持人的研究開關 `community_enabled` 預設關閉：
關閉時下列教師端點（包括分享與取消分享）一律回 **503**「社群功能目前沒有開放。」，什麼都不讀不寫。
`GET /api/config` 多一欄 `community_enabled`。

| 方法／路徑 | 說明 |
|---|---|
| `GET`／`PUT /api/admin/community-settings` | PI；`{enabled}`，預設 false；改動寫 `usage_logs`（`setting_changed`）。其他人 403 |
| `PUT /api/worksheets/{id}/share` | 只有擁有者（別人 404）。`{shared: true, name: "code"｜"anonymous", allow_remix?: bool}` 分享或改選項（第一次分享 `name` 必填，沒有預設；沒有內容的學習單 409；`allow_remix` 沒給：新分享為 true、改選項時維持原值）。`{shared: false}` 取消分享（立即生效，重複取消無害）。→ `{id, shared, shared_at, name, allow_remix}` |
| `GET /api/community/worksheets` | 探索：所有分享中、未刪除的學習單（含自己的，`is_mine`）。查詢：`q`（標題，≤100 字，`%`、`_` 當一般字元）、`sort` = `newest`（預設）／`remixed`／`saved`、`output` = `worksheet`／`comprehension`／`tiered`、`level` = `basic`／`standard`／`advanced`（有這個版本）、`limit`（預設 24，最多 60）、`offset`。→ `{items: [Card], total, limit, offset}` |
| `GET /api/community/saves` | 我的收藏，最新收藏在前；同樣的查詢（沒有 `sort`）。→ 同上，每項多 `saved_at`，另有 `unavailable`（已取消分享或刪除、現在看不到的收藏數） |
| `GET /api/community/mine` | 我的分享：自己有內容的學習單，最近修改在前（`q`、`limit`、`offset`）。→ `{items: [{id, title, updated_at, shared, shared_at, name, allow_remix, saves, remixes, output, levels, cover}], total, limit, offset}` |
| `GET /api/community/worksheets/{id}` | 預覽（唯讀）：`?level=` 選版本（預設第一個）。→ 下面的 `Meta` 加 `level` 與 `sheet`（學生版 `WsSheet`，含它印的圖）。不在分享中、已刪除 404。記 `community_view` |
| `GET /api/community/worksheets/{id}/images/{image_id}` | 預覽裡的一張圖（JPEG）；只有分享中、該位置目前的圖 |
| `POST`／`DELETE /api/community/worksheets/{id}/save` | 收藏／取消收藏 → `{id, saved}`。不能收藏自己的（400）；不在分享中 404（取消收藏不受限）；重複無害 |
| `POST /api/community/worksheets/{id}/remix` | 拿來改 → 201 `{worksheet_id}`（副本）。`allow_remix` 為 false 403；自己的 400；不在分享中 404 |

`Meta`（探索、收藏、預覽的標頭，**沒有任何人的編號**）：
`{id, title, owner_name, shared_at, saves, remixes, is_mine, saved, allow_remix, output, levels}`。
`owner_name` 依 `share_name`：`code` 是研究代號，`anonymous` 是「一位老師」（匿名時絕不回代號）。
`Card` = `Meta` + `cover`（學生版 `WsSheet` 的開頭：最多 2 區、每區 3 題、沒有圖，沒有內容時 `null`）。

`WorksheetState`（`GET /api/worksheets/{id}` 等）多一欄 `remixed_from: {id, title, owner_name} | null`：
這份是拿社群的學習單改的，且原件仍在分享、開關開著；否則 `null`。

`usage_logs` 新事件：`community_share`、`community_unshare`、`community_view`、`community_save`、`community_unsave`、
`community_remix`（只記編號與固定標籤，欄位見 COMMUNITY.md）。
