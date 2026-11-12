# Manna DLS 平台：設計藍圖（Phase 1：AI 備課）

> 依據：《蔓娜計畫 DLS 平台：工程師規格（v1.0）》（2026-11-08，以下稱「規格」）、《InterAct 與 LingoAct 技術盤點》、交接前端 `Manna_DLS_工程交接_網站程式碼_20261109`。
> 規格與本文件不一致時，以規格（及研究者版 v0.4）為準。

## 1. 本輪範圍

規格的 12/15 雛形要走完八步。本輪（Phase 1）先做 **AI 備課**，其餘步驟的資料表與事件欄位先預留：

| 規格步驟 | Phase 1 | 備註 |
|---|---|---|
| 1. 單元前 CoRe | ✅ 做 | 2–3 個學習重點 × Q1–Q8；三層「需要協助」 |
| 2. 各課備課（RAG、SIOP、連結 CoRe） | ✅ 做 | RAG 建議附引用；採用／修訂／拒絕＋理由 |
| 3. 課中書籤 | 預留資料表 | Phase 2 |
| 4. 課後補寫 | 預留 | Phase 2 |
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
 → 檢索 knowledge_items（先依普高／技高過濾，再全文＋trigram；向量欄位預留）→ retrieval_logs
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
| CoRe | `core_versions`、`core_focuses`、`core_responses`、`core_assist_logs`、`lesson_core_links` | 建 |
| SIOP | `siop_features`、`lesson_siop_focus` | 建（資料待主持人核准） |
| AI | `ai_interactions`、`teacher_decisions`、`retrieval_logs`、`prompt_versions` | 建 |
| 鷹架 | `scaffolds`、`scaffold_events` | 建 |
| 知識庫 | `knowledge_items`（含 `embedding vector` 欄位預留）、`teacher_cases` | 建 |
| 書籤 | `lesson_runs`、`record_events`、`record_event_revisions` | 預留（建表不做畫面） |
| 反思 | `reflections`、`carry_forward_links`、`progress_logs` | 預留 |
| 系統 | `notifications`、`export_logs`、`access_logs`、`system_versions`、`usage_logs` | 預留 |
| 其他 | `unit_materials`、`eval_sets`、`eval_runs`、`feedback`、`design_changes`、`expert_ratings` | 預留 |

完整 ERD（主鍵、外鍵、基數、版本與刪除關係）放在 `docs/ERD.md`，由 migration 產生後補齊。

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
| POST | `/api/admin/knowledge/import` | 匯入知識庫條目 CSV（含來源 SHA-256） |
| POST | `/api/admin/teacher-cases/import` | 匯入教師備課經驗 CSV |
| GET | `/api/health` | 健康檢查（含 DB、AI 設定狀態） |

## 7. 前端頁面

沿用交接版的導覽與視覺，改寫成 React：

1. 登入（邀請碼）
2. 工作檯：單元列表、目前進度
3. **CoRe**：重點 2–3 個 × Q1–Q8；每格「目前沒有想法」、內容來源；「需要協助」逐層展開
4. **Study & Plan**：連結 CoRe 重點（旁列 CoRe 答案）、教案步驟（活動型態、ICAP）、RAG 建議卡（引用、採用／修訂／拒絕＋理由）、SIOP 提示、其他老師怎麼教
5. Teach & Record、Reflect & Progress、Share & Connect：保留交接版的靜態展示，標示「Phase 2」
6. 後台（主持人）：提示詞版本、知識庫與教師經驗匯入

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

- Coolify：Docker Compose 資源，來源為私有 repo `richard880502/manna-dls`。
- 服務：`app`（埠 `127.0.0.1:8080:8080`，只對本機開放）、`db`（`pgvector/pgvector:pg17`，volume 保存資料）。
- 環境變數（在 Coolify 設定，不進版本庫）：`DATABASE_URL`、`AI_PROVIDER`、`AI_MODEL`、`ANTHROPIC_API_KEY`、`DEMO_MODE`、`ADMIN_BOOTSTRAP_CODE`。
- nginx：`manna.biobank.tw` 改為反向代理到 `127.0.0.1:8080`（取代貪食蛇），沿用現有 Certbot 憑證。
- 備份：每日 `pg_dump` 加密備份（Phase 2 前完成，規格 6.2）。

## 11. 時程（對應規格里程碑）

| 時間 | 工作 |
|---|---|
| 11/12–11/15 | 藍圖、專案骨架、CI、部署管線、資料庫 migration、邀請碼登入 |
| 11/16–11/22 | CoRe（含三層協助）、AI 管線與紀錄 |
| 11/23–11/29 | Study & Plan＋RAG＋教師判斷紀錄；知識庫與教師經驗匯入 |
| 11/30–12/08 | Phase 2：書籤、Reflect AI、「下次要改」、單元後 CoRe 修訂、匯出 |
| 12/09–12/15 | 主持人走一次完整單元；修正 |

## 12. 待決事項

| 項目 | 需要誰 |
|---|---|
| 國網中心模型 API（端點、JSON 結構化輸出、embedding、速率） | 共同主持人 |
| 知識庫條目（課綱、2000 字、大考六級、文法 76 項、SIOP 摘要）的整理檔 | 主持人 |
| 教師備課經驗可否進 RAG、是否需去識別化 | 主持人 |
| 示範環境的 Anthropic API 金鑰 | 維護者（自行填入 Coolify） |
| 每次取回條目數、追問自動收起秒數等參數 | 主持人 |
