# 文字模型請求的流量控制：資料庫工作佇列（實作計劃書）

狀態：步驟 1–6 已完成（預設 `AI_QUEUE_ENABLED=false`，部署後由負責人在 Coolify 打開；變數要列在 `docker-compose.yml` 才傳得進容器）。實作紀錄見最後一節「10. 做了什麼、和計劃書的差異」。
決定：使用者選「資料庫裡的工作佇列」（不是只在記憶體裡排隊）。老師送出的請求**不回 429**，而是照現在一樣回 202（已收到），工作進佇列，由伺服器的分派器依規則分配。
前提（已完成）：備課對話改用 OpenAI Responses 介面（#110，`api/src/ai/chat.rs`）；第 2 層包住它的呼叫函式（`ActiveAi::complete_chat`／`complete_chat_counted`）。

---

## 1. 目標

| 問題 | 現況 | 做完之後 |
|---|---|---|
| 尖峰時 OpenAI 回 429 | 每個背景工作收到就直接開始，30 人同時按就同時打 | 同時跑的工作有上限，其餘排隊；畫面顯示「排隊中，前面還有 N 個」 |
| 429、伺服器錯誤直接失敗 | 老師看到「AI 暫時無法使用」 | 依 OpenAI 告知的時間自動等待重試 |
| 額度用完 | 和 429 分不出來，一直失敗 | 立刻停止送出、老師看到明確訊息、後台亮紅燈 |
| 一位老師佔滿資源 | 沒有限制 | 分派時輪流給每位老師；同一位老師同時最多跑 2 個，其餘排隊（不拒絕） |
| 部署、重啟會中斷工作 | 進行中的工作消失，過一段時間才被標成失敗；所以部署前要確認正式站閒置 | 排隊中的工作留在資料庫，重啟後照常分派；進行中而且可以重跑的工作自動重新排隊 |
| 看不到流量 | 只有 `ai_interactions` 一列一列 | 每個工作何時進佇列、等多久、跑多久都在佇列表，直接做成監測頁 |

不在範圍：生圖（自己有全站 2 張的上限與重試）、OCR（國網，自己有上限）、embedding（量極小）、Graphiti 記憶索引（自己有租約機制）。

---

## 2. 兩層設計

```
老師按下按鈕（做學習單、備課對話、改這一區…）
  │  路由照舊回 202 與目前的狀態（前端不用改送出方式）
  ▼
第 1 層：工作佇列（資料庫 ai_jobs）
  │  分派器：同時最多 N 個工作；優先級 → 輪流給每位老師 → 先來先到
  │  每位老師同時最多 2 個在跑（主持人不限），多的照樣排隊
  │  租約＋心跳：伺服器重啟後，逾期的工作自動重新排隊或標成失敗
  ▼
工作內容（照舊：學習單各步驟、備課對話的工具迴圈…）
  │  每次呼叫模型都經過 ↓
  ▼
第 2 層：單次呼叫的保護（ActiveAi::complete / complete_chat）
     錯誤分類 → 429 依 Retry-After 等待重試（最多 2 次）→ 額度用完時冷卻、快速失敗
```

為什麼兩層都要：第 1 層決定「哪個工作先跑、同時跑幾個」，第 2 層處理「這一次呼叫被 OpenAI 擋下時怎麼辦」。即使工作數有上限，OpenAI 仍可能因為 token 用量回 429。

---

## 3. 資料表：`ai_jobs`

新 migration（編號用當時下一個空號）。

```sql
CREATE TABLE ai_jobs (
    id           BIGSERIAL PRIMARY KEY,
    kind         TEXT NOT NULL,              -- 見第 5 節的清單
    teacher_id   BIGINT NOT NULL REFERENCES teachers (id),
    priority     SMALLINT NOT NULL,          -- 0 互動（老師在等）、1 一般產生、2 背景
    status       TEXT NOT NULL DEFAULT 'queued'
                 CHECK (status IN ('queued', 'running', 'done', 'failed', 'cancelled')),
    params       JSONB NOT NULL DEFAULT '{}',-- 只放編號（turn_id、worksheet_id…），不放老師打的字
    ref_kind     TEXT,                       -- 給畫面找到它：'prep_turn'、'worksheet'、'lesson_sop'…
    ref_id       BIGINT,
    attempts     INT NOT NULL DEFAULT 0,
    lease_until  TIMESTAMPTZ,                -- 跑的時候每 30 秒延長；逾期＝那台伺服器不在了
    error        TEXT,                       -- 中文原因，給老師看；不含金鑰或原文
    queued_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ
);
-- 分派時找下一個
CREATE INDEX ai_jobs_queue ON ai_jobs (priority, queued_at) WHERE status = 'queued';
-- 算每位老師在跑的數量、找逾期租約
CREATE INDEX ai_jobs_active ON ai_jobs (teacher_id) WHERE status IN ('queued', 'running');
CREATE INDEX ai_jobs_lease ON ai_jobs (lease_until) WHERE status = 'running';
-- 同一個對象同時只能有一個排隊或進行中的工作（例如同一輪對話不會被排兩次）
CREATE UNIQUE INDEX ai_jobs_one_per_ref ON ai_jobs (ref_kind, ref_id)
    WHERE status IN ('queued', 'running');
```

- 現有的 `prep_turns`、`worksheet_jobs`、`lesson_sop_jobs` 照舊保存結果與步驟；`ai_jobs` 只管「排隊與分派」，不取代它們。
- 研究紀錄：佇列是營運資料；另外在 `usage_logs` 記 `ai_job_finished`（`{job_id, kind, wait_ms, run_ms, status, attempts}`），不記內容。

---

## 4. 實作步驟

建議照順序做，每一步各自一個 PR（CI 綠燈、正式站閒置時合併）。

### 步驟 1：單次呼叫的錯誤分類與 token 紀錄（第 2 層的基礎，不改變行為）

**改**：`api/src/ai/mod.rs`（`ProviderError`）、`api/src/ai/openai.rs`、`api/src/ai/chat.rs`（Responses 版）、`api/src/ai/anthropic.rs`、`api/src/ai/pipeline.rs`、`api/src/ai/admin.rs`。

1. `ProviderError` 新增：
   ```rust
   RateLimited { retry_after: Option<Duration>, body: String }, // 429 rate_limit_exceeded
   QuotaExceeded { body: String },                              // 429 insufficient_quota / billing_hard_limit_reached
   Overloaded { status: u16, body: String },                    // 500、502、503（504 照舊當逾時）
   ```
2. 讀 body 之前先讀標頭：`retry-after`（秒）、`retry-after-ms`、`x-ratelimit-reset-requests`／`x-ratelimit-reset-tokens`（格式 `1s`、`6m0s`、`20ms`，取較小者）。寫 `fn retry_after(&HeaderMap) -> Option<Duration>` 加單元測試。
3. 429 的 body 看 `error.code`／`error.type` 分成上面兩種。
4. 所有 OpenAI 呼叫都把 `usage` 存進 `ai_interactions.usage`（`prompt_tokens`、`cached_tokens`、`completion_tokens`、有的話 `reasoning_tokens`）；現在只有備課對話有存。
5. `error_detail` 前面加分類字首（`rate_limited:`、`quota:`、`overloaded:`），方便統計。
6. 後台「測試並套用」的錯誤訊息分成「用量太高，稍後再試」與「額度用完，請到 OpenAI 帳戶提高上限或加值」。

### 步驟 2：單次呼叫的重試與額度冷卻（第 2 層）

**新檔**：`api/src/ai/guard.rs`。**改**：`ActiveAi::complete`、`complete_chat`。

```text
if 額度冷卻中 → 立刻回 QuotaExceeded（不送出）
for attempt in 0..=AI_RATE_LIMIT_RETRIES:
    result = timeout(等待上限, 呼叫)
    RateLimited → 等 retry_after（沒有就 2s、4s）＋0–500ms 隨機，最多 20s，再試
    Overloaded  → 只再試 1 次，等 2s
    QuotaExceeded → 設冷卻 AI_QUOTA_COOLDOWN_SECS；第一次進冷卻記 usage_logs ai_quota_exhausted；不重試
    Timeout、其他 → 不重試（和生圖一致）
```

- 重試都算同一列 `ai_interactions`；`usage` 加 `transport_retries`。
- 老師訊息：額度用完統一為「今天的 AI 用量已達上限，請通知計畫主持人。」（常數放 `pipeline.rs`，各功能的錯誤訊息遇到這種錯誤改用它）。
- 後台「測試並套用」成功時清除冷卻。
- 這一步上線後就已經能擋掉大部分「等一下就好」的失敗，即使佇列還沒做。

### 步驟 3：佇列與分派器（第 1 層核心）

**新檔**：`api/src/queue/mod.rs`（enqueue、分派、租約）、`api/src/queue/handlers.rs`（每種工作怎麼執行）。**改**：`api/src/lib.rs`（`AppState` 持有分派器）、`api/src/main.rs`（啟動、優雅關閉）、`api/src/config.rs`。

1. **送進佇列**
   ```rust
   pub async fn enqueue(db: &PgPool, job: NewJob) -> AppResult<i64>;   // 寫一列 queued，然後 notify
   pub struct NewJob { kind: JobKind, teacher_id: i64, priority: u8, params: Value, ref_kind: &'static str, ref_id: i64 }
   ```
   和建立 `prep_turns`、`worksheet_jobs` 那幾列放在**同一個交易**裡，避免「有紀錄卻沒排入佇列」。

2. **分派器**（每台 API 伺服器一個 tokio 任務）
   - 被叫醒的時機：有工作進佇列、有工作結束（`tokio::sync::Notify`），另外每 2 秒自己醒一次。
   - 只要「本機正在跑的數量 < `AI_MAX_RUNNING_JOBS`」就挑下一個：
     ```sql
     WITH running AS (
       SELECT teacher_id, count(*) AS n FROM ai_jobs WHERE status = 'running' GROUP BY teacher_id
     )
     SELECT j.id
       FROM ai_jobs j
       JOIN teachers t ON t.id = j.teacher_id
       LEFT JOIN running r ON r.teacher_id = j.teacher_id
      WHERE j.status = 'queued'
        AND (t.role = 'pi' OR COALESCE(r.n, 0) < $1)        -- 每位老師同時上限（主持人不限）
      ORDER BY j.priority, COALESCE(r.n, 0), j.queued_at   -- 優先級 → 跑得少的老師先 → 先來先到
      LIMIT 1
      FOR UPDATE OF j SKIP LOCKED;
     ```
     接著 `UPDATE … SET status='running', started_at=now(), attempts=attempts+1, lease_until=now()+租約`。`SKIP LOCKED` 讓之後多台伺服器也能安全共用。
   - 用 `jobs::run`（現有，隔離 panic）執行對應的 handler；跑的期間每 30 秒延長 `lease_until`。
   - 結束：`done`／`failed`（寫 `error`），記 `ai_job_finished`，叫醒分派器。

3. **重啟後接手**（啟動時跑一次，之後每分鐘跑一次）
   - `status='running' AND lease_until < now()` 的工作：
     - 這種工作「可以安全重跑」（第 5 節表格）而且 `attempts < AI_JOB_MAX_ATTEMPTS` → 改回 `queued`；
     - 否則 → `failed`，`error` = 「伺服器重新啟動，這次沒有完成。請按再試一次。」並把對應的 `prep_turns`／`worksheet_jobs` 一起標失敗。
   - 現在 `prep/store.rs` 的 `refresh` 用「超過一段時間就判失敗」的作法，改成以佇列狀態為準（保留它當最後防線）。

4. **優雅關閉**（`main.rs` 收到 SIGTERM）：停止分派新工作，等正在跑的最多 `AI_SHUTDOWN_GRACE_SECS`（預設 60）秒；沒跑完的留給下一次啟動接手（第 3 點）。`docker-compose.yml` 的 `stop_grace_period`（90 秒）要比這個長，否則 Docker 預設 10 秒就強制停止。

5. **取消**：排隊中的工作可以取消（`status='cancelled'`），例如老師刪掉備課對話、或按「取消排隊」。

### 步驟 4：把各功能改成「送進佇列」

先改量最大的兩個（備課對話、整份學習單），確認穩定再改其餘。用設定 `AI_QUEUE_ENABLED` 控制：關閉時照舊直接 spawn，出問題可以立刻切回。

| 工作 kind | 現在在哪裡 spawn | 優先級 | 可以安全重跑？ | 說明 |
|---|---|---|---|---|
| `prep_turn` | `prep/run.rs`（`create_turn` 之後） | 0 | 是 | chat 一輪最後才寫一個版本；做學習單的輪次轉成下面的 `worksheet_run` |
| `worksheet_run` | `worksheet/ai.rs`（`generate`） | 1 | 是，從沒完成的步驟繼續 | `worksheet_jobs` 已經有每一步的狀態，handler 跳過 `done` 的步驟 |
| `block_regenerate` | `worksheet/ai.rs`（`regenerate_block`） | 0 | 是 | |
| `block_revise` | `worksheet/ai/revise.rs`（改這一區） | 0 | 是 | 產生的是提案，老師採用才寫入 |
| `sop_step` | `sop/ai.rs` | 1 | 是 | 用 `lesson_sop_jobs` 的狀態 |
| `template_extract` | `worksheet/template/extract.rs` | 1 | 是 | 等 OCR 的部分照舊 |
| `siop_check` | `prep/selfcheck.rs` | 2 | 是 | |
| `memory_detect` | `memory/detect.rs` | 2 | 是 | 很輕，也可以先不進佇列 |

每個 handler 只拿 `params` 裡的編號，從資料庫讀需要的東西（不要把老師的文字塞進 `params`）。

### 步驟 5：畫面顯示排隊

- **排隊位置**：狀態 API（備課對話的 session、學習單的 jobs）在工作排隊時多回 `queue: { ahead: number, eta_secs: number | null }`。
  - `ahead` = 優先級更高或相同、而且更早排入的 queued 工作數（大約值即可）。
  - `eta_secs` = `ahead ÷ AI_MAX_RUNNING_JOBS × 最近一小時同類工作的平均執行秒數`。
- **前端**：`web/src/components/RunProgress.tsx` 與備課對話的進度列，在有 `queue` 時顯示「排隊中，前面還有 N 個（約 X 分鐘）」與「取消排隊」。型別加在 `web/src/api/types.ts`，欄位選填，舊前端不受影響。
- 每位老師排隊上限 `AI_MAX_QUEUED_PER_TEACHER`（預設 10）：超過時才拒絕，訊息「你排隊中的工作太多了，等前面的完成再送」。這是唯一會拒絕的情況。

### 步驟 6：監測（已完成）

> 已做：`GET /api/admin/ai-traffic`（`api/src/routes/admin_ai_traffic.rs`，格式見 `docs/API.md`；額度冷卻用 `quota_blocked_secs`，另有 `queue_enabled`、`errors`、`overloaded`、`reasoning`、`latency_ms`、每日 `calls`）與後台總覽的「AI 請求」一列。「用量」分頁尚未做。

- **新端點** `GET /api/admin/ai-traffic`（主持人）：
  ```jsonc
  {
    "now": { "max_running": 6, "running": 4, "queued": 7, "oldest_wait_secs": 38, "quota_blocked_until": null },
    "last_hour": {
      "jobs": { "done": 52, "failed": 1, "cancelled": 0 },
      "wait_secs": { "p50": 4, "p95": 41 },
      "run_secs": { "worksheet_run": { "p50": 95, "p95": 160 }, "prep_turn": { "p50": 7, "p95": 22 } },
      "calls": 488, "rate_limited": 3, "quota": 0,
      "tokens": { "prompt": 3900000, "cached": 2700000, "completion": 410000 }
    },
    "last_7_days": [ { "date": "2026-11-25", "jobs": 310, "prompt_tokens": 21000000, "cached_tokens": 14000000, "completion_tokens": 2100000 } ]
  }
  ```
- **後台總覽**加一列「AI 佇列：進行中 4／排隊 7（最久 38 秒）」；排隊超過 2 分鐘或額度冷卻中時標黃／紅。
- 之後的「用量」分頁直接用這個端點畫圖（token 成長、等待時間、各功能用量）。

---

## 5. 設定值

| 變數 | 預設 | 意思 |
|---|---|---|
| `AI_QUEUE_ENABLED` | `false`（先部署、確認沒問題，再由負責人改 `true`） | 關掉時照舊直接執行，出問題可立刻切回 |
| `AI_MAX_RUNNING_JOBS` | `6` | 全站同時執行的工作數（每台伺服器） |
| `AI_MAX_RUNNING_PER_TEACHER` | `2` | 每位老師同時執行數（主持人不限），多的排隊 |
| `AI_MAX_QUEUED_PER_TEACHER` | `10` | 每位老師排隊上限，超過才拒絕 |
| `AI_JOB_LEASE_SECS` | `90` | 租約長度（每 30 秒延長） |
| `AI_JOB_MAX_ATTEMPTS` | `2` | 重啟後最多重跑幾次 |
| `AI_SHUTDOWN_GRACE_SECS` | `60` | 關閉時等正在跑的工作多久 |
| `AI_RATE_LIMIT_RETRIES` | `2` | 單次呼叫遇 429 重試次數 |
| `AI_QUOTA_COOLDOWN_SECS` | `600` | 額度用完後多久內不再送出 |

依現有寫法加在 `api/src/config.rs`（`from_env`、`Config::for_tests`），README 的環境變數表也要補。正式環境的值在 Coolify 設定後重新部署（由使用者操作）。

**`AI_MAX_RUNNING_JOBS` 怎麼抓**：一個工作同一時間只會有一個模型呼叫在進行（各步驟依序執行），所以「同時呼叫數 ≈ 執行中的工作數」。到 platform.openai.com → Settings → Limits 查這個模型的 TPM 與 RPM：

```
上限 ≈ min( TPM × 平均每次秒數 ÷ 60 ÷ 平均每次 token,  RPM × 平均每次秒數 ÷ 60 )
```

目前每次呼叫約 8,000 輸入＋1,000 輸出 token、15–20 秒。TPM 若是 2,000,000，約可同時 60 個；保守先設 6，看監測的「等待 p95」再調：常超過 60 秒就調高，429 變多就調低。注意快取命中的 token 仍算在 TPM 裡。

---

## 6. 測試計畫

- **單元**：標頭解析與錯誤分類（步驟 1）；重試等待與冷卻（步驟 2，`#[tokio::test(start_paused = true)]`）。
- **資料庫測試**（`api/tests/queue.rs`）：
  - 分派順序：優先級、輪流給老師（老師 A 送 5 個、B 送 1 個時，B 不用等 A 全部跑完）、先來先到。
  - 每位老師同時上限、主持人不限、排隊上限才拒絕。
  - `SKIP LOCKED`：兩個分派器同時挑，不會挑到同一個。
  - 租約逾期：可重跑的回到 `queued`，不可重跑或超過次數的標 `failed` 並連帶標 `prep_turns`。
  - 取消排隊中的工作。
  - 同一個 `ref` 不會排兩次。
- **整合**（fake provider）：開 20 份學習單、`AI_MAX_RUNNING_JOBS=2`，全部完成且同時執行數從未超過 2；備課對話（優先級 0）在一堆學習單排隊時先被處理；模擬重啟（把 running 的租約改成過期再啟動分派器）後工作完成。
- **假模型延遲**：fake provider 新增環境變數 `FAKE_LATENCY_MS`（目前沒有），讓每次假呼叫等一段時間，才測得出排隊。
- **不要**對 OpenAI 或國網做壓力測試。
- **前端**：排隊文字、預估時間、取消排隊。

---

## 7. 上線順序

1. **步驟 1＋2**：錯誤分類、token 紀錄、429 重試、額度冷卻。行為只會變好，可以先上。
2. **步驟 3＋4（只改備課對話與整份學習單）＋6**：`AI_QUEUE_ENABLED=false` 部署 → 確認沒問題 → 改成 `true` 重新部署 → 看監測數字。
3. **步驟 4 其餘工作＋5**：排隊顯示與取消。
4. 確認重啟接手正常後，部署前的「正式站閒置」檢查（`scratchpad/merge_pr_safe.sh` 的 `idle.sql`）可以放寬成「沒有不可重跑的工作在跑」。

---

## 8. 檔案清單

| 檔案 | 步驟 |
|---|---|
| `api/migrations/00xx_ai_jobs.sql`（新） | 3 |
| `api/src/queue/mod.rs`、`api/src/queue/handlers.rs`（新） | 3、4 |
| `api/src/ai/guard.rs`（新） | 2 |
| `api/src/ai/mod.rs`、`openai.rs`、`chat.rs`、`anthropic.rs`、`pipeline.rs`、`admin.rs` | 1、2 |
| `api/src/lib.rs`、`api/src/main.rs`、`api/src/config.rs`、`README.md` | 3、5 |
| `api/src/prep/run.rs`、`api/src/prep/store.rs`（`refresh`）、`api/src/routes/prep.rs` | 4 |
| `api/src/worksheet/ai.rs`、`api/src/routes/worksheets.rs`（`generate`、`regenerate_block`） | 4 |
| `api/src/worksheet/ai/revise.rs`、`api/src/routes/worksheet_revise.rs`（`revise_block`） | 4 |
| `api/src/sop/ai.rs`、`api/src/worksheet/template/extract.rs`、`api/src/prep/selfcheck.rs`、`api/src/memory/detect.rs` | 4 |
| `api/src/ai/fake.rs`、`api/src/prep/fake.rs`（`FAKE_HTTP_429_ONCE`、`FAKE_QUOTA`、`FAKE_LATENCY_MS`） | 2、6 |
| `web/src/api/types.ts`、`web/src/components/RunProgress.tsx`、備課對話進度列 | 5 |
| `api/src/routes/`（`/api/admin/ai-traffic`）、`web/src/components/admin/AdminOverview.tsx` | 6 |
| `docs/API.md` | 各步驟的契約 |

---

## 9. 這次不做的

- **另外架 Redis／訊息佇列服務**：Postgres 的 `FOR UPDATE SKIP LOCKED` 就夠，少一個服務要維護。
- **多把 API key 輪流使用**：OpenAI 的上限以組織計算，多把 key 不會增加上限。
- **工作執行到一半的細部接續**（例如學習單某一步的一半）：以「步驟」為單位重跑即可。
- **生圖、OCR 併進同一個佇列**：它們已有自己的上限，用的也是不同額度；之後若要統一監測，再把它們的紀錄接到監測端點。


---

## 10. 做了什麼、和計劃書的差異（後端核心，2026-11-25）

### 已完成

- **步驟 1、2**（擁有者先做）：錯誤分類、429 依 `Retry-After` 重試、額度冷卻。這次補上：`Guard` 計算重試次數並存進 `ai_interactions.usage.transport_retries`（成功與失敗的呼叫都記）；用 `tokio::time::pause` 測 429 重試延遲與次數、5xx 只重試一次、逾時不重試、冷卻只進入一次；進入冷卻時記一次 `usage_logs ai_quota_exhausted`（不含內容）；老師訊息：額度用完 `MSG_QUOTA`、重試用盡 `MSG_BUSY`（「現在使用的人很多，請稍後再試一次。」），套用在 `ai/pipeline.rs`、各步驟共用的 `ai/execution.rs`（`Failure::Error(ErrorKind)`）、學習單各步驟、SOP、草擬目標、版型讀取、`prompt_try.rs`、備課對話失敗路徑。
- **步驟 3**：`api/src/queue/`：`mod.rs`（資料層）、`dispatcher.rs`（分派器、租約心跳、復原、優雅關閉）、`handlers.rs`（種類 → handler 表）、`routes.rs`（取消）。每個程序一個分派器任務，只在 `AI_QUEUE_ENABLED=true` 時由 `main.rs` 啟動；被 `Notify`（進佇列、工作結束）或每 2 秒叫醒；本機執行中的數量 < `AI_MAX_RUNNING_JOBS` 才 `claim_next`；handler 經 `jobs::run`（隔離 panic）執行；結束時記 `ai_job_finished`。
- **審查發現的修正**：
  - 租約圍欄：`renew_lease`、`finish` 都帶「第幾次執行（`attempts`）」並回傳是否真的改到一列；心跳續約失敗（租約被別人接手）會 `Abortable` 中止這次的工作，且舊 worker 的 `finish` 不寫任何東西。
  - 每位老師排隊上限在 `pg_advisory_xact_lock`（以老師為鍵）之下計數，同時送出不會超過上限。
  - `ahead()` 不在 `queued` 時回 `None`（同優先級以 `(queued_at, id)` 排序，不會因同一毫秒算錯）。
  - 復原：`recover_expired` 回傳這次標失敗的工作（`Recovery { requeued, failed }`），分派器逐一把對應的備課輪次或學習單工作列一起結束。
- **步驟 4（備課對話、整份學習單）**：
  - `prep_turn`（優先級 0，可重跑）：`activity`、`ask`、`revise`、`chat` 與教學套件的活動規劃。輪次與工作在 `store::start_turn` 的**同一個交易**建立；handler 讀 `params` 的編號（`turn_id`、`track`、`worksheet_id`）重建 `TurnInput`，跑原本 spawn 的同一段程式（`prep::run::execute`、`execute_kit`，原本的 `spawn` 只是 `tokio::spawn(execute…)`）。輪次已不是 `running`（取消、失敗）就什麼都不做。
  - `worksheet_run`（優先級 1，可重跑）：三種產生（四步、Bloom 各層、版型）都在 `open_run` 的同一個交易裡排工作（`params`：`worksheet_id`、`run_no`、`track`）。每種產生拆成「開進度＋做事」（`begin_generation`）；`drive(Start::Spawn | Queued | Await, …)` 決定直接 spawn、只排隊、或在 handler 裡等到結束。`spawn_run` 的收尾邏輯拆成 `finish_run`，handler 與 spawn 共用。其餘種類見下一條。
- **步驟 4 的其餘種類**（`worksheet/ai.rs` 的 `queue_block`、`run_block_queued`；`worksheet/ai/revise.rs` 的 `run_queued`；`sop/ai.rs` 的 `run_queued`；`worksheet/template/extract.rs` 的 `start`、`run_queued`；`prep/selfcheck.rs` 的 `spawn`、`run_queued`）。全部可重跑；`AI_QUEUE_ENABLED` 為 false 時照舊直接 spawn。

  | kind | 優先級 | `ref_kind`／`ref_id` | `params`（只有編號與 `track`） | 功能列怎麼開、怎麼收 |
  |---|---|---|---|---|
  | `block_regenerate` | 0 | `worksheet`／學習單（和 `worksheet_run` 共用：同一份學習單同時只會有一個進行中的產生，由 `open_run` 保證） | `worksheet_id`、`run_no`、`track` | 工作列與單筆佇列項目在 `open_run` 的同一個交易；三種（四步、Bloom 層、版型區段）各拆成 `start_block` 與 `begin_block(…, Start::Spawn｜Queued｜Await)`；handler 從工作列的 `block_id` 找回區塊、重新判斷是哪一種。取消、復原失敗：同 `worksheet_run`（`close_run_failed`），但不會去結束等這份學習單的備課輪次 |
  | `block_revise` | 0 | `worksheet_proposal`／提案 | `proposal_id`、`track` | 提案（`running`）與佇列項目在 `insert_running` 的同一個交易。handler 從提案列（指示、註記編號、當時送出的區塊、輪次）重建 `Job`，「上一次 Manna 說了什麼」從同一區前一筆提案讀。取消、復原失敗：提案 `failed`（`error` 為訊息）。提案本來就只在老師採用時才寫進學習單，重跑沒有副作用 |
  | `sop_step` | 1 | `sop_job`／`lesson_sop_jobs` 的列 | `sop_job_id`、`class_id`、`track` | 列（`running`，`phase` =「排隊中」）與佇列項目在 `start` 的同一個交易。handler 從列（`kind`、`slot`、`parent_job_id`、`params`、`sop_version_id`、`material_id`）重建 `JobInput`；換一個的 `Swap`（被換掉的建議、要避開的目標）照舊從母工作的結果與版本讀（`swap_of` 從 route 搬進 `sop::ai`）。取消、復原失敗：列 `done`、`result` 是 `status: error` 加訊息（頁面本來就這樣顯示失敗） |
  | `template_extract` | 1 | `worksheet_template`／範本 | `template_id`、`run`（`extract_run`） | 範本列早就存在（`extracting`），`extract::start` 才排佇列（取代 `spawn`，改成 async）；handler 重新從範本的來源教材讀文字（`source`，不存在 `params`）。**等 OCR 的部分照舊**：`spawn_after_ocr` 是普通背景任務，OCR 好了才排佇列。排不進去（太多）：範本 `failed`，訊息就是「排隊太多」。取消、復原失敗：範本 `failed` |
  | `siop_check` | 2 | `siop_check`／檢核列 | `check_id` | 檢核列在關閉輪次的交易裡開（原樣），`spawn` 在背景任務裡排佇列（呼叫端是關閉輪次的程式，不能等）；排不進去：檢核 `failed`。handler 用輪次當初的模型設定（`pinned_turn`，重啟後用現行設定）。取消、復原失敗：檢核 `failed` |

  - 進度：`block_regenerate` 用學習單原本的 `progress.queue`；其餘的功能沒有進度物件，所以各加一個選填的 `queue`（形狀同 `Progress.queue`，只在排隊時有）：`Proposal.queue`、SOP `Job.queue`、`SiopCheck.queue`（備課輪次完成後 SIOP 還在排隊時，輪次的 `progress.queue` 也帶同一個值）。版型讀取沒有進度物件，所以不回 `queue`，排隊中 `status` 照舊是 `extracting`。
  - stale：`rv::expire_stale`、`sop::ai::expire_stale`、`tpl_store::expire_stale`、`selfcheck::expire_stale` 都多一個條件：有排隊中或執行中的佇列項目就不判失敗（同樣只在佇列開啟時）；備課輪次的 stale 判斷也把它等的範本的佇列項目算進去，等範本讀取的輪次不會因為排太久被關掉。`tpl_store::expire_stale` 與 `rv::expire_stale` 因此多一個 `queued: bool` 參數。
  - `spawn_after_template`（等範本的 `worksheet` 輪次）不用改：它輪詢範本狀態，範本的狀態現在由佇列的 handler 寫。有測試。
  - **`memory_detect` 不進佇列**。理由：(1) 沒有功能列可以回報或收尾（失敗與取消都沒有地方寫，也沒有人在等）；(2) 它要判斷的是老師「打的那一段字」，這段字不一定存在資料庫（備課輪次存的可能只是標籤、學習單的「教師的要求」之後可改），要進佇列就得把老師的文字放進 `params`，違反「`params` 只放編號」；(3) 先經關鍵字預篩、又要研究開關開著才會呼叫模型，量很小，也不影響老師正在等的畫面。它照舊 `tokio::spawn`，不佔佇列名額。

- **步驟 5 的後端**：`progress::Progress.queue: Option<{job_id, ahead, eta_secs}>`；取消 `DELETE /api/ai-jobs/{id}`。
- **假模型延遲**：`FAKE_LATENCY_MS`（`Config.fake_latency_ms`，每次假呼叫在 `ActiveAi` 裡先等這麼久，一般與備課對話兩種呼叫都有）。
- **備課對話的呼叫（Responses API）**：`POST /v1/responses` 的錯誤用同一個 `http_error` 分類（429 分 `rate_limited`／`quota`、500–503 `overloaded`、408／504 逾時、其他 `HTTP <狀態>`），所以重試、冷卻、老師訊息、`error_detail` 字首與一般呼叫相同；守門的重試次數失敗時也記進 `usage.transport_retries`（`complete_chat_counted`）。用本機假 OpenAI 測過（`api/tests/responses_api.rs`：400、429 重試、額度冷卻）。

### 決定

- **整份學習單重跑 = 從頭重做**（不是跳過 `done` 的步驟）。理由：一次產生的結果只在最後組合、存成一個版本，中間步驟的輸出只在記憶體裡，跳過已完成的步驟就沒有東西可接；而且各步驟的提示會用到前面步驟的輸出。重跑前把這次的工作列全部設回 `queued`、清掉結果。已知的小缺口：存完新版本、還沒結束最後一個工作之間當機，重跑會多出一個內容相近的版本（`AI_JOB_MAX_ATTEMPTS` 限制最多重跑次數）。
- **備課輪次與學習單的兩筆佇列不會互相等待，所以不會死結**：做學習單的輪次（`worksheet`／`assessment`／`comprehension`／`tiered`）**自己沒有佇列項目**，它的工作就是它的學習單那一筆 `worksheet_run`；輪次在 `store::refresh` 裡等學習單的工作列結束才關閉（原本就是這樣）。教學套件有兩筆（活動規劃的 `prep_turn`、學習單的 `worksheet_run`），兩筆都只做自己的事、不等對方，由 `close_kit` 在兩邊都結束後關閉。教學套件的活動規劃是在學習單**建立（`start_worksheet`）之後**才排隊（不在建立輪次的交易裡），因為它要知道學習單有沒有成功開始（`params.worksheet_error` 是系統固定訊息，不含老師打的字）。等範本讀完的 `worksheet` 輪次（`spawn_after_template`）仍是一般的背景任務，不進佇列，因為它大部分時間在等別的工作，佔著名額只會卡住；等範本好了才呼叫 `start_worksheet`，那時才排學習單那一筆。
- **stale 規則**：`prep/store.rs` `refresh` 與 `worksheet/ai.rs` `expire_stale` 對「有排隊中或執行中的佇列工作」的輪次／工作列不判失敗（執行中的是否還活著由租約決定）；沒有佇列工作的照舊以年齡判斷（所以關閉佇列時行為不變）。
- `resources`（找教學資源）不呼叫文字模型，不進佇列。

### 偏離計劃書的地方

- `queue.rs` 改成 `queue/` 目錄，資料層函式仍在 `queue::`；分派器放 `queue::dispatcher`，handler 表放 `queue::handlers`（計劃書只有 `mod.rs` 與 `handlers.rs`）。
- `enqueue` 沒有集中成一個函式：交易內用 `enqueue_tx`，沒有交易的呼叫端用 `enqueue`。
- 沒有單獨的 `NewJob` 種類列舉；種類用字串常數（`handlers::KIND_*`）。
- 取消端點回 204，沒有內容。
- 優雅關閉多做一件事：關閉時還在跑的工作立刻讓租約過期，下一個程序啟動就接手，不必等 90 秒。

### 還沒做

步驟 5 的前端、步驟 6 的監測端點與頁面。`memory_detect` 刻意不進佇列（見上）。已知的小缺口：`siop_check` 在背景任務裡才排佇列，所以輪次關閉、排佇列之間若程序當機，檢核列會停在 `running`，由 stale 規則（照年齡）在幾分鐘後標失敗，老師按「再試一次」即可；`template_extract` 排隊中，範本的回應不帶 `job_id`，所以畫面沒辦法取消它（後端的取消對它有效）。
