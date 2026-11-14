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
interface UnitClassRef { id: number; name: string; course_label: string; note: string; }  // note＝這個取用的「這班的備註」

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
| POST | `/api/units` | 原欄位＋`class_id?: number` | `Unit`。帶 `class_id` 時：單元建在備課庫，同時由該班級取用（一步完成）；未帶 `class_profile` 時單元的 `class_profile` 留空（來源 `none`），因為班級特性在班級上、不複製。未帶 `class_id` 時行為同 R3。班級已封存 → 409 |
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

- **工作檯**以班級為中心：班級卡片（名稱、課程名稱、單元數、下一步，例如「繼續：L3 倒裝句 第 2 節 → 開始本節課」）、「新增班級」、「備課庫」入口。沒有班級時邀請建立第一個班級（可略過）。
- **班級頁** `/classes/:id`：班級特性（可改）、取用的單元（順序、這班的進度：上過的課例數／總數、最近一次上課）、「從備課庫加入單元」、「新增單元」（建立＋取用）、這班的備註、封存。
- **備課庫** `/library`（`/plan` 亦同）：所有單元設計與取用它的班級、建立單元、加入班級。
- **目前班級**：從班級進入單元或課例時，頁面顯示「目前班級：…」，可在取用這個單元的班級之間切換；存在瀏覽器的 sessionStorage（`manna.currentClass`）。從備課庫進入時清除；單元只有一個班級取用時自動視為該班。「開始本節課」用目前班級；沒有目前班級而單元有多個班級取用時，在頁面上請教師選。課堂紀錄頁與課後補寫頁顯示班級。Teach & Record 首頁依班級分組。
