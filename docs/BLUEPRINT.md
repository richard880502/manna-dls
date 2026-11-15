# Manna DLS 平台：設計藍圖（Phase 1：AI 備課）

> 依據：《蔓娜計畫 DLS 平台：工程師規格（v1.0）》（2026-11-08，以下稱「規格」）、《InterAct 與 LingoAct 技術盤點》、交接前端 `Manna_DLS_工程交接_網站程式碼_20261109`。
> 規格與本文件不一致時，以規格（及研究者版 v0.4）為準。

## 1. 本輪範圍

規格的 12/15 雛形要走完八步。本輪（Phase 1）先做 **AI 備課**，其餘步驟的資料表與事件欄位先預留：

| 規格步驟 | Phase 1 | 備註 |
|---|---|---|
| 1. 單元前 CoRe | ✅ 做 | 2–3 個學習重點 × Q1–Q8；三層「需要協助」 |
| 2. 各課備課（RAG、SIOP、連結 CoRe） | ✅ 做 | RAG 建議附引用；採用／修訂／拒絕＋理由 |
| 3. 課中書籤 | ✅ 做（Phase 1.2） | 👍／⚠ 書籤、離線同步、手機 QR 遙控、子母畫面；見 [API.md](API.md)「Phase 1.2」 |
| 4. 課後補寫 | ✅ 做（Phase 1.2） | 書籤依時間排列、旁列教案步驟；活動歸屬、學生可觀察行為、處理方式、補寫（全部選填） |
| 5. Reflect AI 追問 | 預留（共用 AI 管線） | Phase 2 |
| 6. 「下次要改」 | 預留 `carry_forward_links` | Phase 2 |
| 7. 單元後 CoRe 修訂 | 資料模型已支援（版本複製） | Phase 2 做畫面 |
| 8. 研究資料匯出 | 預留 | Phase 3 |

驗收（Phase 1）：
- 一位教師能建立單元 → 寫 CoRe（可留白、可選「目前沒有想法」、可求助三層 AI）→ 建課例 → 寫教案步驟 → 取得附引用的 RAG 建議並採用／修訂／拒絕。
- AI 失效（逾時、格式錯、檢核不過）時，CoRe 與教案仍可手動完成。
- 每筆 AI 輸出記錄：供應商、模型版本、提示詞版本、鷹架編號、檢索條目、檢核結果、狀態（顯示／擋下／逾時）。

## 2. 技術選型

| 層 | 選擇 | 理由 |
|---|---|---|
| 前端 | React 19 + TypeScript + Vite | 與 InterAct／LingoAct 同技術棧，團隊熟悉；沿用交接版的視覺（styles.css、頭像） |
| 後端 | Rust（axum + tokio + sqlx） | 型別安全、單一執行檔、記憶體小；manna 只有 2 核 7.7 GB |
| 資料庫 | PostgreSQL 17 + pgvector | 規格 7.1 建議；全文檢索＋向量混合檢索 |
| AI 呼叫 | 後端 `LlmProvider` 抽象 | `anthropic`（示範）與 `openai_compatible`（國網中心）兩種實作，用環境變數切換 |
| 部署 | Docker（單一映像：前端靜態檔由後端提供）＋ Coolify | manna 已納入 Coolify；nginx 負責 `manna.biobank.tw` 與 HTTPS |
| CI | GitHub Actions | 前端 lint／型別／測試、後端 fmt／clippy／測試（含 Postgres）、Docker 建置；通過後觸發 Coolify 部署 |

Rust 的取捨：Anthropic 沒有官方 Rust SDK，所以後端用 `reqwest` 直接呼叫 HTTP API（官方文件允許無 SDK 的語言這樣做）；編譯較慢，CI 用快取減輕。

## 3. 系統架構

```
瀏覽器（教師）
   │ HTTPS
   ▼
nginx（manna.biobank.tw，Certbot 憑證）── manna 主機
   │ http://127.0.0.1:8080
   ▼
manna-dls 容器（Coolify 管理）
   ├─ /            React 靜態檔
   └─ /api/*       axum
         ├─ 帳號／權限（邀請碼登入，研究編號）
         ├─ CoRe、單元、課例、教案版本
         ├─ AI 中介：檢索 → 組提示詞 → 呼叫模型 → 自動檢核 → 紀錄
         └─ sqlx ──► PostgreSQL 17 + pgvector（同一 compose，資料存 volume）
                        │
AI 供應商 ◄──────────────┘（只經後端；金鑰只在 Coolify 環境變數）
  ├─ 示範：Anthropic Claude（只允許示範資料）
  └─ 正式：國網中心模型（拿到 API 後切換）
```

### 兩條資料管線（規格第 2 節）

- **正式平台**只能用國網中心核准的模型。Phase 1 先用 Claude 示範，因此後端加上硬性開關：
  - `AI_PROVIDER=anthropic` 時必須同時設定 `DEMO_MODE=true`，否則後端拒絕啟動。
  - `DEMO_MODE=true` 時前端每頁顯示「示範環境：請勿輸入真實教師或學生資料」。
- 模型無回應時 AI 功能暫停，**不會自動改送其他模型**（因此不啟用 Anthropic 的 server-side fallback，確保每筆輸出的模型版本可追溯）。
- 離線模型比較工具（5.5）屬第一年正式版，不在 Phase 1。

## 4. AI 管線

```
教師按「需要協助」或「取得建議」
 → 建立 ai_interactions（status=pending，存輸入來源快照、鷹架編號、提示詞版本）
 → 檢索 knowledge_items（先依學校類型與範圍過濾，再以關鍵字＋語意向量混合排序；沒有語意索引時只用關鍵字）→ retrieval_logs（記方法與候選）
 → 呼叫模型（JSON schema 結構化輸出，逾時 30 秒）
 → 自動檢核：
     ・有無引用、引用的條目編號是否在本次檢索結果中
     ・普高／技高是否相符
     ・語言（英文例句不得混入中文、中文說明不得整段英文）
     ・查無條目時必須回「知識庫查無」，不得自行生成
 → 不合格：重生一次 → 仍不合格：status=blocked，告知「暫無建議」
 → 合格：status=shown，回傳前端
 → 教師判斷寫入 teacher_decisions（採用／修訂／拒絕＋理由；或回答／略過＋👍👎）
```

- CoRe 協助三層：①重新說明題意 → ②提供思考面向 → ③根據教師既有內容提一個具體追問。**不提供可貼入的答案、不自動填欄位**（提示詞與檢核都限制）。
- 提示詞存在 `prompt_versions`，後台修改即產生新版本，不寫死在程式碼。
- 預設模型：`claude-opus-5-5`（可用 `AI_MODEL` 環境變數更換）。

## 5. 資料模型（Phase 1）

原則（規格 3）：研究資料庫只存研究編號；身分在 `identity_map`；內容只新增版本不覆蓋；軟刪除；AI 互動與教師判斷分表。

| 群組 | 資料表 | Phase 1 狀態 |
|---|---|---|
| 帳號 | `teachers`（research_id、school_track、consent_scope、role） | 建 |
| 帳號 | `identity_map`（research_id ↔ 姓名、Email；欄位加密） | 建（僅主持人可讀） |
| 帳號 | `invite_codes`、`sessions`（權杖雜湊） | 建 |
| 主線 | `units`、`lessons`、`lesson_versions`、`lesson_steps` | 建 |
| 班級 | `classes`（名稱、年級、學校類型、課程名稱、班級特性；只封存不刪除；不存學生資料）、`class_units`（班級取用單元：多對多、有順序、可軟移除、這班的備註）；`lesson_runs.class_id` | 建（Phase 1.4，migration `0006_classes.sql`） |
| CoRe | `core_versions`、`core_focuses`、`core_responses`、`core_assist_logs`、`lesson_core_links` | 建 |
| SIOP | `siop_features`、`lesson_siop_focus` | 建（資料待主持人核准） |
| AI | `ai_interactions`、`teacher_decisions`、`retrieval_logs`、`prompt_versions` | 建 |
| 鷹架 | `scaffolds`、`scaffold_events` | 建 |
| 知識庫 | `knowledge_items`、`knowledge_embeddings`（每版本一筆向量，記模型與維度）、`embedding_jobs`、`teacher_cases` | 建（向量：Phase 2.4，migration `0011_embeddings.sql`） |
| 書籤 | `lesson_runs`、`record_events`、`record_event_revisions`、`run_pairings`、`run_remotes` | 建（Phase 1.2，migration `0004_teach_record.sql`） |
| 反思 | `reflections`、`carry_forward_links`、`progress_logs` | 預留 |
| 系統 | `notifications`、`export_logs`、`access_logs`、`system_versions`、`usage_logs` | 預留 |
| 其他 | `unit_materials`、`eval_sets`、`eval_runs`、`feedback`、`design_changes`、`expert_ratings` | 預留 |

完整 ERD（主鍵、外鍵、基數、版本與刪除關係）放在 `docs/ERD.md`，由 migration 產生後補齊。

### 階層（Phase 1.4：班級與備課庫）

```
教師 ─┬─ 備課庫：單元設計（單元 ＋ 單元前 CoRe ＋ 課例與教案版本）   一份設計，可給多個班級用
      └─ 班級 ──（取用 class_units，多對多、有順序）── 單元設計
            └─ 上課紀錄 lesson_runs（屬於一個班級）→ 書籤 record_events → 課後補寫
```

- 備課內容（CoRe、教案）屬於單元設計，不分班；教學紀錄（上課、書籤、補寫）分班。同一課例可在 A 班、B 班各上一次。
- AI 帶入的班級特性：課例覆寫 ＞ 班級 ＞ 單元舊欄位 `class_profile` ＞ 偏好 P6（只在班級情境中）。
- 舊資料：每位已有單元的教師得到預設班級「我的班級」，原有單元與上課紀錄都歸到它。合約見 [API.md](API.md)「Phase 1.4」。

## 6. API 清單（Phase 1）

| 方法 | 路徑 | 說明 |
|---|---|---|
| POST | `/api/auth/login` | 邀請碼登入，回傳權杖（只存 SHA-256 雜湊） |
| GET | `/api/me` | 目前教師（研究編號、學校類型） |
| GET/POST | `/api/units` | 單元列表／建立 |
| GET | `/api/units/:id/core` | 最新 CoRe 版本（重點＋回答） |
| PUT | `/api/units/:id/core/draft` | 存草稿（新增版本紀錄，不覆蓋） |
| POST | `/api/units/:id/core/complete` | 完成單元前 CoRe（允許留白） |
| POST | `/api/core/assist` | 三層協助（question_id、level） |
| GET/POST | `/api/units/:id/lessons` | 課例列表／建立 |
| GET | `/api/lessons/:id` | 課例（最新教案版本、連結的 CoRe 重點） |
| POST | `/api/lessons/:id/versions` | 存教案（每次產生新版本） |
| PUT | `/api/lessons/:id/core-links` | 連結 1–3 個 CoRe 重點（可空） |
| POST | `/api/lessons/:id/suggestions` | RAG 備課建議 |
| POST | `/api/ai/:interaction_id/decision` | 教師判斷 |
| POST | `/api/scaffold-events` | 鷹架顯示／展開／使用／略過 |
| GET | `/api/teacher-cases` | 其他老師怎麼教（已核准的備課經驗） |
| GET/POST | `/api/admin/prompts` | 提示詞版本（主持人） |
| POST | `/api/admin/knowledge/import` | 匯入知識庫條目 CSV（含來源 SHA-256）；Phase 2.0 的課綱、詞彙表匯入與查詢見 `docs/API.md` |
| POST | `/api/admin/teacher-cases/import` | 匯入教師備課經驗 CSV |
| GET | `/api/health` | 健康檢查（含 DB、AI 設定狀態） |
| – | `/api/runs/*`、`/api/lessons/:id/runs`、`/api/remote/*` | Teach & Record 與課後補寫（Phase 1.2，完整合約見 API.md） |

## 7. 前端頁面

沿用交接版的導覽與視覺，改寫成 React：

1. 登入（邀請碼）
2. 班級（首頁）：接下來那一班的下一節與「開始上課」、我的班級、上完還沒補寫；班級頁（下一節、這班的單元與進度、上課紀錄、班級特性）；備課庫（所有單元與使用它的班級）。導覽只有班級、備課庫、學習單三項（見 `DESIGN_OVERHAUL.md`）
3. **CoRe**：重點 2–3 個 × Q1–Q8；每格「目前沒有想法」、內容來源；「需要協助」逐層展開
4. **Study & Plan**：連結 CoRe 重點（旁列 CoRe 答案）、教案步驟（活動型態、ICAP）、RAG 建議卡（引用、採用／修訂／拒絕＋理由）、SIOP 提示、其他老師怎麼教
5. **Teach & Record**（Phase 1.2）：開始本節課 → 課堂紀錄頁（兩顆大按鈕、加一句話、復原與更正、待同步筆數、手機 QR 遙控、子母畫面小視窗）→ 結束本節課並回顧 → 課後補寫；手機遙控器頁 `/remote` 不需登入
6. Reflect & Progress、Share & Connect：尚未開發。交接版的靜態展示頁已在改版第 1 步移除，做出來之前不放入口；學習單目前是「開發中」說明頁
7. 後台（主持人）：提示詞版本、知識庫與教師經驗匯入

## 8. 教師備課經驗（問卷）

- 試算表是「一起聊聊您的備課方式」問卷的 9 份回覆，屬於**需求分析資料**，不是教案案例。分析與據此提出的修改見 [TEACHER_NEEDS.md](TEACHER_NEEDS.md)。
- 原始回覆含姓名與 Email，不匯入平台、不進版本庫；文件只用去識別化代號。
- `teacher_cases` 資料表保留給日後主持人核准、且取得分享同意的教案案例（「其他老師怎麼教」）。是否進 RAG 仍由主持人決定，預設不進。

## 9. CI／CD

- `.github/workflows/ci.yml`（push 與 PR 都跑）：
  - web：`pnpm install` → `lint` → `typecheck` → `vitest` → `build`
  - api：`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test`（GitHub Actions 的 Postgres service，跑 migrations）
  - docker：建置正式映像（不推送）
- `main` 全部通過後，`deploy` job 呼叫 Coolify 部署 webhook（`COOLIFY_WEBHOOK_URL`、`COOLIFY_TOKEN` 由維護者存成 GitHub Secrets）。CI 失敗就不部署。

## 10. 部署（manna）

- Coolify 專案 `manna-dls`（production 環境）內有兩個資源：
  - **manna-dls**：Docker Compose 應用（來源 `richard880502/manna-dls` 的 `deploy` 分支），埠 `127.0.0.1:8080:8080`，只對本機開放。
  - **manna-dls-db**：獨立的 Coolify PostgreSQL 資源（`pgvector/pgvector:pg17`），不對外開 port，只在 Coolify 內部網路可見；使用 Coolify 的排程備份。
- 應用的 Advanced 設定開啟「Connect to predefined network」，才能連到資料庫。
- 環境變數（在 Coolify 設定，不進版本庫）：`DATABASE_URL`（資料庫資源的 Postgres URL (internal)）、`AI_PROVIDER`、`AI_MODEL`、`ANTHROPIC_API_KEY`、`DEMO_MODE`；`ADMIN_BOOTSTRAP_CODE` 由 Coolify 自動產生（`SERVICE_PASSWORD_ADMINCODE`）。
- nginx：`manna.biobank.tw` 反向代理到 `127.0.0.1:8080`，沿用 Certbot 憑證。
- 備份：資料庫資源每日排程備份（規格 6.2 要求每日加密備份並定期還原演練；異地備份目的地待定）。

## 11. 時程（對應規格里程碑）

| 時間 | 工作 |
|---|---|
| 11/12–11/15 | 藍圖、專案骨架、CI、部署管線、資料庫 migration、邀請碼登入 |
| 11/16–11/22 | CoRe（含三層協助）、AI 管線與紀錄 |
| 11/23–11/29 | Study & Plan＋RAG＋教師判斷紀錄；知識庫與教師經驗匯入 |
| 11/30–12/08 | Phase 2：Reflect AI、「下次要改」、單元後 CoRe 修訂、匯出（書籤與課後補寫已在 Phase 1.2 先做） |
| 12/09–12/15 | 主持人走一次完整單元；修正 |

## 12. 待決事項

| 項目 | 需要誰 |
|---|---|
| 國網中心模型 API（端點、JSON 結構化輸出、速率）；embedding 已取得另一把金鑰並採用 `embeddinggemma-2`（2026-11-15） | 共同主持人 |
| ~~知識庫條目的整理檔~~ **課綱與大考詞彙表已可匯入**（2026-11-15）：108 課綱 1,352 條（普高、技高、綜高、外語群）與大考中心參考詞彙表 6,012 詞條已擷取成匯入檔，由主持人或知識庫管理員在「知識庫管理」頁上傳到正式環境（`docs/API.md`「Phase 2.0」）。**仍缺**：文法 76 項、SIOP 摘要的正式整理檔（目前只有內建的示範條目） | 主持人 |
| **學測、統測歷屆考題：完全沒有取得。** 規格 §9.6 的「歷屆考題能力對應度」無資料可做；需決定來源（大考中心、技專校院入學測驗中心的公開試題）、年度範圍與授權方式 | 主持人 |
| 知識庫來源的待確認事項：各文件的官方原始連結（交付的 PDF 沒有附網址）、技高與綜高語文領綱的發布日期、大考中心詞彙表的使用授權（頁首聲明限非營利並須註明出處）；確認前資料不進 repo | 主持人 |
| 正式條目匯入後是否停用內建示範條目（停用後文法、詞彙教學、SIOP 類別沒有條目，教案建議只會引用課綱） | 主持人 |
| 教師備課經驗可否進 RAG、是否需去識別化 | 主持人 |
| AI 模型：暫不使用 Claude 示範模式（2026-11-12 決定），等國網中心模型 API 後以 `openai_compatible` 接上 | 共同主持人 |
| 資料庫異地備份的目的地（須為核准的儲存環境） | 主持人 |
| 第一年是否開放任何同儕互動（見 §13.2） | 主持人 |
| 每次取回條目數、追問自動收起秒數等參數（「加一句話」目前 8 秒，前端常數 `NOTE_IDLE_SECONDS`） | 主持人 |

## 13. 後續路線圖（2026-11-12 討論結論）

### 13.1 平台定位：個人工作台＋社群兩層

平台由兩層組成，同一個網站、同一套帳號：

| 層 | 內容 | 資料可見範圍 | 規格時程 |
|---|---|---|---|
| **個人工作台** | CoRe、備課、課中書籤、課後補寫、反思、教材檔案庫；AI 只支持個別教師 | 預設只有教師本人（與研究團隊依權限） | 第一年 |
| **社群** | 分享與檢索教案或案例、留言、共同備課、夥伴媒合 | 教師主動分享的內容 | 第 2 年分享與檢索、第 3 年社群 |

兩層之間有一道明確的**發布邊界**：

- 工作台內容永遠私人。只有教師按「分享」，才把當下內容**複製一份快照**到社群；之後在工作台修改不影響已分享的版本。
- 社群以**化名**顯示，不顯示研究編號或真名；分享前可去除可辨識資訊。
- 教師可隨時收回分享，社群中的快照即下架（保留去識別化紀錄供研究，依 IRB）。
- 互動以非同步為主（分享、留言），不做即時聊天，降低複雜度（技術盤點第九節）。

理由：第一年的研究要觀察「AI 鷹架對個別教師」的影響；同儕互動若混入，無法區分改變來自 AI 或其他教師。發布邊界讓工作台的研究資料保持乾淨，也讓社群日後能獨立開發上線、不必改動工作台。

### 13.2 社群：第一年只預留

- 第一年不做畫面，只在資料模型預留：分享權限（預設私人）、化名、同意範圍、快照來源、收回、留言與檢舉。
- 若主持人希望第一年就開放少量互動（例如「其他老師怎麼教」案例分享），須先確認不影響研究設計（列入 §12）。
- 問卷需求（交流、資深教師經驗、共同備課、媒合）在第 2–3 年依此架構實作。

### 13.3 教材檔案庫：MinIO（收集到教材後再做）

| 放在哪 | 存什麼 |
|---|---|
| **MinIO**（Coolify 服務，與資料庫同專案、只開內部網路） | PDF 等原檔；私有 bucket、伺服器端加密；不開版本保留，刪除即實際刪除；下載只經後端（短時效連結） |
| **PostgreSQL** | 檔名、頁數、抽出的文字切塊與頁碼、向量（pgvector）、擁有者與刪除紀錄 |

- 對應規格 4.5：教材只屬於該教師、該單元，不進共用知識庫、不給其他教師、不用於訓練；只在核准環境處理。
- 教師刪除教材時，原檔、切塊、向量一併實際刪除；備份採延遲刪除並留紀錄（規格 7.1）。
- 上傳畫面提醒不得上傳學生作業、姓名、成績、照片。
- 目前「教材優先」菜單已支援手動貼上文字；PDF 上傳與頁碼定位在收集到教材後實作，屆時估時。

### 13.4 其他延後項目

| 項目 | 狀態 |
|---|---|
| 知識庫與詞彙（Phase 2.0） | **已完成**：課綱條目（含綜高）與大考詞彙表的資料表、匯入（預覽、重複匯入不變動、只停用不刪除）、課綱檢索與依代碼取條文、純規則的詞彙級別分析、知識庫管理員角色與管理頁。資料本身不在 repo，需在管理頁上傳 |
| 語意索引與混合檢索（Phase 2.4） | **已完成**：國網中心 `embeddinggemma-2`（768 維）為知識庫條目建立向量（匯入後背景建立、可續做、換模型自動重建），課綱查詢、教案建議、SOP 目標改用關鍵字＋向量的混合排序；模型不可用時自動只用關鍵字。31 題評估：recall@8 0.26 → 0.71（`docs/RETRIEVAL_EVAL.md`）。需在 Coolify 設 `EMBEDDING_*` 並在知識庫管理頁確認索引完成 |
| 歷屆考題分析（規格 §9.6） | 未開始；沒有考題資料（見 §12） |
| 教師端的課綱連結與詞彙分析畫面 | 待課例 SOP、學習單產生器接上；API 已有 |
| Google 登入＋白名單（規格第一年正式版） | 延後；雛形沿用邀請碼 |
| staging 測試環境 | 待辦；Coolify 同專案另開環境，含獨立網站與資料庫 |
| AI 模型 | 暫不啟用；等國網中心模型 API |
| 資料庫異地備份 | 待主持人決定核准的儲存位置 |
