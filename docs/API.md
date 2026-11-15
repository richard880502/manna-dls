# Manna DLS API 合約（Phase 1）

前後端都以這份為準。所有路徑前綴 `/api`，JSON UTF-8。時間為 RFC 3339 字串。

## 共通

- 驗證：`Authorization: Bearer <token>`（登入取得）。未帶或失效 → `401`。
- 錯誤格式：`{ "error": { "code": "not_found", "message": "給人看的中文訊息" } }`
  - code 取值：`bad_request`、`unauthorized`、`forbidden`、`not_found`、`conflict`、`internal`
- 權限：教師只能存取自己的單元／課例；`/api/admin/*` 只限 `role = "pi"`（Phase 2.0 起，知識庫管理的端點也開放給 `kb_manager`，見「Phase 2.0：知識庫與詞彙」的「角色與權限」）。

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
- 已移除的頁面（`/reflect`、`/share`、`/profile`、`/community`）一律導回首頁；`/worksheets` 是「學習單產生器開發中」的說明頁，沒有任何功能。

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

新角色 `kb_manager`（介面顯示「知識庫管理員」）：在同一個網站內管理知識庫，沒有主持人的其他權限。主持人在後台建立邀請碼時選這個角色。

| 能力（capability） | 內容 | `pi` | `kb_manager` | `teacher`／`ra`／`rater`／`engineer` |
|---|---|---|---|---|
| `administer` | 邀請碼、提示詞版本、平台設定、教師備課經驗匯入（碰到身分、研究資料或 AI 行為的都算） | ✓ | ✗（403） | ✗（403） |
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

- 教材目前只收貼上的文字（≤ 50,000 字）。**段落由程式切分**：有空行時以空行分段（段內換行視為空白），沒有空行時每一行一段；空白段落略過。
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
| POST | `/api/lessons/:id/sop/objectives` | `{ class_id? }` | 202 `SopJob`：SIOP 內容／語言目標推薦＋課綱連結（第 3 步） |
| GET | `/api/sop-jobs/:id` | – | `SopJob` |

教師自己連結課綱條目時，前端用 Phase 2.0 的 `GET /api/knowledge/curriculum`（不呼叫 AI）搜尋，把選到的 `item_code` 放進目標的 `links` 再照常儲存；只有正式課綱條目（有 `official_code`）可以連結，知識庫中手寫的示範條目不行。

版本只增不改（append-only）；`version_no` 每個課例從 1 連號。所有端點只能存取自己的課例（其他人的回 404）。

## AI：只有驗證過的兩步

```ts
interface SopJob {
  id: number; lesson_id: number; kind: "analysis" | "objectives";
  status: "running" | "done";
  phase: string;                       // 給教師看的進度，例如「正在讀教材」「補上缺少的段落」
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

1. 程式把教材編號為 `[P1]…[Pn]` 後送出。沒有教材時兩個 `POST` 都回 400；教材超過 40 段時分析回 400（長陣列容易提早結束，請教師先貼這節課要讀的部分）。
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

---

# Phase 2.3：學習單產生器

以下為**新增**；既有端點不變。Migration `0010_worksheets.sql`（`0009` 保留給教材庫分支）。依據：《學習單產生器：教師選項與 SOP 整合規格》§4.1、§4.2、§4.6、§5–§9.5、§9.7，`docs/LLM_VALIDATION.md` §5–§10（步驟 (c)(d)(e)(f)，(g) 改由程式組裝），`docs/DESIGN_OVERHAUL.md` 第 3 步。工程說明與規格 §12 第 2–6 項的回覆在 `docs/ENGINEERING_REPLY_WORKSHEET.md`。

不在這一版：外部資源搜尋（§4.3）、教材注記與教材庫（另一分支，見「教材接縫」）、AI 資深教師（§4.5）、歷屆考題分析（沒有資料）。§9.6 只做決定性的詞彙級別檢查。

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
// 從課例進入時用課例目前的教材庫教材。教材欄顯示教材庫的 MaterialViewer（含注記）。
// 產生時，該教材上教師勾了「提供給 AI」的注記（`materials::annotations::shared_with_ai`）會以
// 「教師注記」的標題放進每一步的 prompt，並列在「Manna 已帶入的資訊」；其他注記不會送出，也不會出現在學習單上。

interface WsObjective {
  id: string; type: "content" | "language";
  text: string;                 // 教師看的說明（可中文）
  student_facing_en: string;    // 印在學習單「Today I can …」的英文
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
}
interface WsScaffold {
  id: string; type: WsScaffoldType;
  kind: string;                  // 規格 §3 的例子名稱，例如 句型框架、字詞表、圖像組織、清楚的分步指示及完成順序
  content: string; steps: string[];   // 程序鷹架用 steps，其餘用 content
  levels: WsLevel[];             // 這個鷹架出現在哪些版本
  auto: boolean;                 // 系統自動加入的程序鷹架
  origin: WsOrigin;
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
| PUT | `/api/worksheets/:id` | `{ title?, filename?, settings?, brief?, content? }` | `WorksheetState`。`content` 與最新版本不同才新增一個版本（只增不改）。標籤不在規格集合內 → 400；產生中送 `content` → 409（產生完會整份取代） |
| PUT | `/api/worksheets/:id/material` | `{ title?, text, source_note? }` | `WorksheetState`。換教材：重新分段（文字相同的段落沿用 id），`source.kind` 變成 `pasted`，重新計算 `suggested` |
| DELETE | `/api/worksheets/:id` | – | `{ ok: true }`（軟刪除） |
| POST | `/api/worksheets/:id/generate` | `{}` | 202 `WorksheetState`：建立四個步驟的背景工作並立刻回來。已在執行時回傳同一批 |
| GET | `/api/worksheets/:id/jobs` | – | `{ running, jobs: WsJob[], version_no }`（輪詢用） |
| POST | `/api/worksheets/:id/blocks/:block_id/regenerate` | `{}` | 202 `WorksheetState`：只重新產生這一個區塊 |
| POST | `/api/worksheets/:id/blocks/:block_id/undo` | `{}` | `WorksheetState`：復原成重新產生之前的那一版；沒有上一版 → 409 |
| GET | `/api/worksheets/:id/sheet` | `?level=&audience=` | `WsSheet` |
| GET | `/api/worksheets/:id/versions` | – | `Array<{ version_no, cause, created_at }>`，新到舊；`cause` = `created`／`edit`／`generate`／`regenerate`／`undo` |
| GET | `/api/worksheets/:id/sop-sync` | – | `SopSyncPreview`（沒有連結課例 → 409） |
| POST | `/api/worksheets/:id/sop-sync` | `{ confirm: true, keys: string[], sop_version_no }` | `{ sop_version_no, applied }`。沒有 `confirm: true` → 400；課例在預覽之後改過 → 409 |
| PUT | `/api/worksheets/:id/link` | `{ lesson_id: number \| null }` | `WorksheetState`：連結既有課例或取消連結（不改課例內容） |
| POST | `/api/worksheets/:id/save-as-lesson` | `{ unit_id, title }` | `WorksheetState`：在這個單元建立新課例，把教材、目標與任務寫成它的第一版 SOP，並連結 |
| POST | `/api/worksheets/:id/checks/vocabulary` | `{ level, target_level? }` | `WsVocabCheck`。詞彙表未匯入 → 409 |
| GET | `/api/worksheets/:id/checks` | – | `WsVocabCheck[]`：每個版本最近一次的結果 |
| GET | `/api/worksheets/:id/export` | `?format=docx\|pdf&level=&audience=` | 檔案本體（`Content-Disposition: attachment`）。`level` 不在這份學習單的版本裡 → 400 |

## 兩個入口

- **從課例**（`POST /api/worksheets { lesson_id }`）：伺服器讀最新的 SOP 版本，把教材（`source.kind = "lesson_sop"`，是當時的快照）、目標（`content.objectives`）、任務／暖身／Exit ticket（`brief`）帶進來，並依課例內容給 `suggested`：有教材→「閱讀文本」；SOP 任務的資訊處理功能依規格 §8 對應到學生任務（發現→理解內容、整理→整理／分析資料、分析→推論／預測、比較→比較／分類、創造→寫作）；有暖身→「加入暖身活動」；有目標→「顯示學習目標」；任務裡寫了語言鷹架→「提供句型框架」「提供引導問題」。全部只是建議：`settings` 以它起始，教師可以全部取消。課例不需要先完成任何一步。
- **直接使用**：貼上教材即可。`suggested` 只由教材判斷（素材類型：一般文字→「閱讀文本」，數字很多→再加「資料／數據」；其餘給常用的起始值）。學習單預設獨立（`lesson = null`），不會建立課例；之後可用 `link` 或 `save-as-lesson`。
- **四個區塊完全不選也能產生**：每個空的區塊由系統決定（素材類型依教材；學生任務與作答方式由模型依教材設計後回報；產生設定預設「加入暖身活動、顯示學習目標、提供字詞表、提供句型框架」；版本預設一般版，班級特性提到程度落差或教材詞彙偏難時加基礎版）。實際用的設定寫在 `content.chosen`，`chosen.auto` 列出哪些是系統決定的。

## 產生流程

`POST …/generate` 建立一輪（`run_no`）四個工作，依序執行，每個工作是**一次模型步驟**（最多兩次呼叫：第一次＋一次修復）：

| 步驟 | prompt key | 模組 `ai_interactions.module` | 鷹架 | `max_tokens` | 內容 |
|---|---|---|---|---|---|
| `tasks` | `worksheet_tasks` | `worksheet_tasks` | `SC-WS-TASKS` | 6000 | 逐段（或合併段落）的活動區塊：資訊處理功能、學生任務、題目、作答方式、參考答案、逐字依據；每題標 `core`／`support`／`challenge` |
| `warmup` | `worksheet_warmup` | `worksheet_warmup` | `SC-WS-WARMUP` | 3000 | 喚醒背景知識 1–2 個活動＋關鍵字彙（只能從程式算出的候選字挑；級別由 `vocabulary_items` 帶入） |
| `scaffolds` | `worksheet_scaffolds` | `worksheet_scaffolds` | `SC-WS-SCAFFOLDS` | 5000 | 每個區塊的語言鷹架、教學鷹架，以及**自動加入**的程序鷹架 |
| `exit_ticket` | `worksheet_exit_ticket` | `worksheet_exit_ticket` | `SC-WS-EXIT` | 1500 | 2–4 題：形成性評量＋學生反思或蒐集問題 |

最後一步的工作會維持 `running`（`phase`「正在把結果組合成學習單」）直到新版本存好，所以輪詢看到 `running = false` 時，`GET /api/worksheets/:id` 一定已是新內容。之後由**程式組裝**（不呼叫模型）：暖身固定在最前、活動區塊依段落順序、Exit ticket 在最後；題號、版本差異、學生版／教師版都由程式決定，存成一個新的學習單版本（`cause = "generate"`）。

- 不選「加入暖身活動」時 `warmup` 步驟為 `skipped`。Exit ticket 預設加入（不是選項），教師之後可以關掉或刪除。
- **版本差異（規格 §5.4）**：三個版本共用同一份目標與核心題（`core`，`levels` = 全部選到的版本）。`support` 題只在基礎版、`challenge` 題只在進階版。鷹架：基礎版顯示全部；一般版顯示程序鷹架＋字詞表、句型框架；進階版只留程序鷹架。開放題的作答行數基礎版 2、一般版 3、進階版 4。教師可以逐題、逐鷹架改 `levels`。
- **份量（規格 §9.2）**：`item_budget` = 主要任務的核心題數。有 `minutes`：扣掉暖身 5 分鐘與 Exit ticket 3 分鐘後，每 2.5 分鐘一題（3–24）。沒有：每段 2 題（4–14）。超過 1.5 倍時先請模型修一次，仍超過由程式刪去多的非核心題與區塊末端的題目（每個區塊至少留一題）。
- **程序鷹架**：每個區塊至少一個，`auto = true`。模型沒有交出時，程式依作答方式與學生任務產生分步指示（例如有「小組共同完成」就寫明分工步驟），所以一定會有。
- **自動檢查**（移植自 `research/sop-validation/chain/checks.py`）與處理：
  - 標籤只能來自規格集合；全英文（學生看得到的欄位含中日韓文字即不通過）；逐字依據必須是教材的連續片段；選擇題 4 個不重複選項且答案在選項內、是非題答案為 True/False、填空題要有空格；題幹不可含答案；每段都要被涵蓋、區塊依段落順序；區塊數 ≥ 3 時至少 3 種資訊處理功能；暖身關鍵字只能來自候選清單；鷹架不可含答案原文；Exit ticket 2–4 題且兩種用途都有、不重複主要任務的題目。
  - 第一次沒過 → **修復一次**，請求一定不同（temperature +0.4，並附上未通過的項目）。
  - 修復後仍不過 → 不整份擋下：拿掉不合格的部分（不逐字的依據清空、不合格的題目或鷹架移除），記在 `ai_interactions.check_results.repairs`；整個步驟沒有可用內容時，對應區塊留一個空的區塊並在 `message` 說明（例如「暖身沒有產生成功，你可以自己寫，或按重新產生」），其餘步驟照常進行。
  - 輸出會清掉 LaTeX／Markdown 殘留。
- 逾時、供應商錯誤、AI 關閉：該步驟 `outcome` = `timeout`／`error`／`disabled`，其餘照上面的方式處理。**任何失敗都不會擋住手動編輯。**
- 每次呼叫各寫一列 `ai_interactions`（輸入快照中的教材只留字數與雜湊）；`WsJob.interaction_id` 指向教師看到的那一次。教師復原一個重新產生的區塊時，記一筆 `teacher_decisions`（`reject`）。
- 伺服器重啟會中斷工作：`queued`／`running` 超過 `8 × AI_TIMEOUT_SECS + 60` 秒的在下次讀取時結束為 `error`。

### 單一區塊重新產生與復原

- `regenerate`：活動區塊＝對那幾段重跑 `tasks`（只要這個區塊）再跑 `scaffolds`；暖身＝`warmup`＋`scaffolds`；Exit ticket＝`exit_ticket`。temperature 依這份學習單先前產生的次數遞增（閘道會快取相同的請求）。
- 取代之前，先把目前的區塊存進 `worksheet_block_history`。`undo` 取回最近一筆並把它從歷史移除（可一路復原到最早）。兩者都產生新的學習單版本（`regenerate`／`undo`），所以整份的版本紀錄也還在。
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
  lesson_id: number; sop_version_no: number | null;
  changes: SopSyncChange[];
  core_suggestions: [];                     // 預留：「建議補充至單元想法（CoRe）」；這一版永遠是空陣列
}
```

`POST …/sop-sync { confirm: true, keys, sop_version_no }` 只套用 `keys` 裡的項目，寫成課例 SOP 的**新版本**（只增不改，舊版本仍在）。對應方式：目標以 `sop_id` 對應；活動區塊以 `sop_block_id` 對應，寫回時每一題成為一個 SOP 任務（`text` = 題目，`function` = 區塊的資訊處理功能，三種鷹架各自合併成文字，`origin` 依學習單的 `origin` 轉成 `teacher`／`ai_adopted`／`ai_edited`）。套用後學習單的 `sop_id`／`sop_block_id` 會補上。

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

## 匯出

`GET …/export` 由後端即時產生並串流回瀏覽器（不經物件儲存）：

- **DOCX**：直接寫 WordprocessingML（`zip` crate），段落、編號、表格、作答線皆為 Word 原生物件，可編輯。
- **PDF**：純 Rust 產生，字型子集化後內嵌。內嵌的拉丁字型是 Libertinus Serif（OFL，來自 `typst-assets` crate）；中文字由 `WORKSHEET_CJK_FONT`（或映像檔內建的中文字型）提供。學習單含中文而環境沒有中文字型時回 409，請教師改用 DOCX。
- 檔名：`<filename>_<版本>_<學生版|教師參考版>.<副檔名>`（RFC 5987 `filename*`）。教師參考版在每題下方印參考答案與依據，並印 `teacher_notes`。

## 測試用假模型（`AI_PROVIDER=fake`）新增的標記（寫在教材文字裡）

| 標記 | 效果 |
|---|---|
| `FAKE_WS_CJK` | `tasks` 第 1 次有一題題目含中文；修復後改正 |
| `FAKE_WS_CJK_ALWAYS` | 兩次都含中文 → 該題被拿掉 |
| `FAKE_WS_BAD_LABEL` | `tasks` 第 1 次用了表外的學生任務標籤；修復後改正 |
| `FAKE_WS_SPLICE_ALWAYS` | `tasks` 有一筆依據兩次都以「...」拼接 → 依據被清空 |
| `FAKE_WS_WARMUP_BLOCK` | `warmup` 兩次都不合格 → 暖身區塊留空並附說明 |
| `FAKE_WS_NO_PROCEDURAL` | `scaffolds` 不交程序鷹架 → 由程式補上 |
| `FAKE_WS_LEAK` | `scaffolds` 第 1 次有一個鷹架含答案原文；修復後改正 |

既有的 `FAKE_TIMEOUT`、`FAKE_ERROR`、`FAKE_REFUSAL`、`FAKE_BLOCK`、`FAKE_SLOW` 照舊（對四個步驟都有效）。

## usage_logs 新事件

`worksheet_created`、`worksheet_generated`、`worksheet_block_regenerated`、`worksheet_block_undone`、`worksheet_sop_synced`、`worksheet_linked`、`worksheet_vocabulary_checked`、`worksheet_exported`（payload 只有 id、版本、格式等，不含內容）。

## 前端

- `/worksheets`：做過的學習單清單＋「直接做一份」（貼上教材）。`/worksheets/:id`：還沒產生過→設定頁（教材在左、四個區塊在右）；產生過→結果頁（版本分頁、學生版／教師參考版、區塊工具）。`/worksheets/:id/check`：詞彙級別檢查。課例頁的「產生學習單」會建立並開啟一份連結這個課例的學習單。
- 教材欄是一個可替換的插槽（`materialPane`）：這一版放唯讀的教材檢視；教材庫分支的 `MaterialViewer`（含注記）之後接在同一個位置。

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

# Phase 2.2：教材與注記

以下為**新增或變更**；未提到的端點維持原樣。Migration `0009_materials.sql`。依據：《學習單產生器：教師選項與 SOP 整合規格》§4.2、§4.4、§9.7、§10 第 23–28 項，`docs/BLUEPRINT.md` §13.3（教材只屬於一位教師、不共用、不訓練；刪除即實際刪除原檔與衍生資料）。給規格 §12 第 1、11、12、13 項的回覆在 `docs/ENGINEERING_REPLY.md`。外部資源搜尋（§4.3）不在這一版。

## 重點

- **教師自己的教材庫**：貼上文字、上傳檔案、或記下一個網址。只有擁有者看得到；其他人存取任何一份都是 404。
- **原檔放物件儲存**（`STORAGE_BACKEND=s3|fs|none`，見 README「教材檔案儲存」），瀏覽器從不直接連到儲存空間。
- **刪除是真的刪除**：原檔物件、擷取的文字與段落、注記、課例連結一起刪；課例 SOP 留下「教材已刪除」，逐字引文從 SOP 與 AI 紀錄移除（見「刪除」）。
- **注記另外存**，不改教材；每則可勾「提供給 AI」（預設不勾）。只有勾了、沒隱藏、找得到位置的注記會給 AI，並標明是教師注記。注記不會出現在學生版或任何學生端輸出。
- 課例 SOP 第 1 步改用教材庫（貼上、從教材庫選、上傳），舊的 `lesson_sop_materials` 搬進教材庫後刪除。

## 能力（capability）

新增 `use_materials`（教材庫）：目前所有角色都有（每個帳號都可以備課）；放成能力是為了日後要收窄時只改 `auth::role_can` 一處。`GET /api/me` 的 `capabilities` 多這一項；`GET /api/config` 多 `uploads_enabled: boolean`。

## 可以上傳什麼

| 類型 `kind` | 副檔名 | 單檔上限 | 處理 |
|---|---|---|---|
| `document` | `.txt` `.docx` `.pdf` | 20 MB | 擷取文字（見下），可注記、可給 Manna 分析 |
| `image` | `.png` `.jpg`（`.jpeg`）`.webp` `.gif` | 10 MB | 保存、在檢視區顯示；不分析 |
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
- **掃描的 PDF**（沒有文字層）：`text_status = "no_text"`，原檔照常保存與下載，**不做 OCR**。讀不出來（加密、損壞）：`failed`。超過 300,000 字的文件只保留前面（`text_truncated = true`），原檔完整保存。
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
  anchor: {                     // 注記「現在」在哪裡，每次讀取時重新定位
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
| PUT／DELETE | `/api/lessons/:id/materials/:material_id` | – | 把教材連到課例／取消連結（多對多）。課例 SOP 第 1 步正在用的教材不能在這裡取消（409，請到第 1 步換教材） |
| GET | `/api/materials/:id/annotations` | – | `MaterialAnnotation[]`，依閱讀順序；隱藏的也列（`hidden: true`），找不到位置的排最後 |
| POST | `/api/materials/:id/annotations` | `{ paragraph_id, start, end, note?, share_with_ai? }` | `MaterialAnnotation`。範圍必須在該段落內；一份教材最多 500 則；注記 ≤ 2,000 字、標記 ≤ 2,000 字 |
| PUT | `/api/annotations/:id` | `{ note?, share_with_ai?, hidden?, anchor?: { paragraph_id, start, end } }` | `MaterialAnnotation`。`anchor` 用於教師重新確認位置 |
| DELETE | `/api/annotations/:id` | – | `{ deleted: true }`。只刪注記，教材不變 |
| GET | `/api/materials/:id/annotations/ai` | – | `{ material_id, notes: Array<{ annotation_id, paragraph_id, paragraph_no, quote, note }>, prompt_block }`：**只有**勾了提供給 AI、未隱藏、找得到位置的注記 |

### 為什麼是後端串流，不是 MinIO 的 presigned URL

`file-link` 發的是**本站 API** 的連結：權杖以後端程序內的隨機金鑰（HMAC-SHA256）簽章，綁定教材 id、教師 id 與到期時間；使用時仍重新檢查教材存在且屬於該教師。所以 MinIO 不必對外開放、連結外流也打不開別人的東西、教材一刪除連結立即失效。代價是檔案流量經過後端，影音以 Range 分段傳送，對 2 核心主機可以接受。

### 給 AI 管線的注記

Rust 端：`materials::annotations::shared_with_ai(db, teacher_id, material_id)` 回傳上述 `notes`，`ai_context_block(&notes)` 產生 prompt 文字，開頭固定為「教師注記（教師自己寫的備課筆記，不是教材原文，也不是外部來源；只供理解教學重點，不得寫進學生版）」。這是注記進入任何 AI 管線的唯一入口；沒勾的注記不會被讀到。輸入快照記錄注記時比照教材：只記 id 與雜湊，不記內容（學習單產生器接上時遵守）。

## 注記的定位

- 存的是：段落 id、段落內 UTF-16 起訖位置、標記的原文（由伺服器從段落取出）、前後各 32 字的上下文，以及做注記時教材的 `revision`。
- 每次讀取重新定位：同段同位置文字相同 → `ok`；同段內找得到原文（有多處時取上下文最吻合、再取最近）→ `moved`；別的段落中有一處明顯最吻合 → `moved`；否則 `lost`（前端顯示「需要重新確認位置」，注記內容保留，教師可重新選取位置或刪除）。
- 前端選取若落在英文單字中間，會延伸到整個單字；跨段落的選取不能成為一則注記（請只選一段）。

## 刪除

`DELETE /api/materials/:id` 依序：

1. 刪除儲存空間中的原檔物件。**刪不掉就整個不刪**，回 503「原檔沒有刪除成功…所以這份教材還沒有刪除」；儲存未設定而教材有原檔時同樣不刪。
2. 同一個交易內：刪除教材列（文字、段落）、注記、課例與學習單連結；用到它的 SOP 版本 `material_id` 設為 null、`material_deleted = true`，指向它段落的參照改名為 `m<id>-p3`（不會被誤認為下一份教材的段落），**從它逐字引用的片段**（分析與語言重點的 `evidence`）換成「［教材已刪除］」；這些課例的 AI 紀錄（`ai_interactions.output`、`lesson_sop_jobs.result`）中的逐字引文與教材名稱同樣移除。教師自己寫的分析、目標、任務都保留。
3. `material_deletions` 記一筆（教材 id、類型、大小、是否有原檔、注記數、時間；沒有標題、檔名、雜湊或內容），`usage_logs` 記 `material_deleted`。

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
  - 內容：印刷樣式、頁邊段落編號、PDF 頁碼分隔、注記（藍色底線＋藍色淡底小塊，絕不用金色）、選取文字出現「加注記」、提供給 AI、修改、隱藏／顯示隱藏的注記、刪除（頁內確認）、重新確認位置；圖片／音訊／影片以瀏覽器內建元件預覽，可下載原檔；網址教材顯示連結（新分頁開啟）。
- **備課庫 → 教材**：列出教材（類型、段數、頁數、大小、注記數、用在哪些課例）、上傳檔案／貼上文字／加入網址（每種都有隱私與著作權提醒）、改名、永久刪除（頁內確認）。
- **課例第 1 步**：貼上文字、從教材庫選、上傳檔案；有教材時可修改（貼上的）、換一份、從這個課例移除；教材被刪除時顯示「教材已刪除」。
