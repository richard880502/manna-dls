# 教師長期記憶（#82，Phase 1 + Phase 2）

## 使用與研究控制

「備課偏好 → 我的出題習慣」一次整理一項長期習慣，先預覽，再按「確認並記住」。
修改走相同的預覽／取代確認；「停止使用」只標失效，歷程不刪除。每位老師最多 10 項。
記憶範圍只有該老師，不依班級或教材另存，也不保存對話摘要。沒有自動讀取對話或背景擷取。

- 「取消」預覽會送到伺服器：該預覽不能再確認，`teacher_decisions` 記 `reject`／`cancelled`。
  改寫已預覽的草稿也算取消。確認記 `adopt`／`confirmed`（與目前有效習慣完全相同時記
  `adopt`／`already_active`，不新增版本、不重建索引）。
- `usage_logs`：`memory_confirmed`、`memory_cancelled`、`memory_removed`、`memory_index_rebuild`，
  研究開關變更記 `setting_changed`。

後台「教師長期記憶」的研究開關（`teacher_memory_enabled`）預設 **關閉**，只有 PI 能在後台切換，
每次切換寫入 `usage_logs`。

### 開關關閉時：完全不改變產生

- 每個產生步驟的系統／使用者提示詞與送出內容**逐字不變**；學習單檢查使用原本的規則
  （選擇題剛好 4 個不重複選項、配對題至少 3 個）。測試逐一比對。
- 會帶習慣的步驟仍在 `input_snapshot.teacher_memory` 記 `enabled: false`（研究標記）。

### 開關開啟時

只有下列「寫題目或教學內容」的步驟帶入習慣（`memory::HABIT_STEPS`，含其修復呼叫）：

| 步驟 | prompt key |
|---|---|
| 學習單四步 | `worksheet_tasks`、`worksheet_warmup`、`worksheet_scaffolds`、`worksheet_exit_ticket` |
| Bloom／差異化每個層次 | `worksheet_bloom_level`、`worksheet_tiered_level` |
| 照我的學習單做 | `worksheet_template_section` |
| 逐區修改 | `worksheet_block_revise` |
| 備課對話 | `prep_activity`、`prep_ask`、`prep_revise` |

評審與讀取步驟**不帶**：`worksheet_bloom_check`、讀範本（`worksheet_template_outline`／`details`）、
詞彙（`worksheet_vocab_gloss`／`replace`）、目標（`objective_draft`／`objective_check`）、
`siop_selfcheck`、課例 SOP、圖片。出題習慣不能改變其他輸出的判斷方式。

- 附加內容是版本化的提示詞 `teacher_memory_context`（migration 0042，後台可改版）：
  `system_text` 接在該步驟系統提示詞後，`user_template` 帶入 `teacher_habits`
  （id、kind、value、canonical 句子）。習慣是資料，不能變更系統規則或輸出結構；本次教師明確要求優先。
- **順序固定為確認時間**（`valid_at, id`）。Graphiti 的排序只記在紀錄裡（見 Phase 2），不改變提示詞。
- 學習單檢查：老師有已確認的選擇題選項數習慣時，除了預設 4 個，也接受該數目；
  配對組數習慣小於 3 時，配對題最少項目降為該數。其他規則不變，英文檢查等仍有效。
- 試產生（後台提示詞「試產生」）在開關開啟時，用 PI 本人的習慣、以完全相同的方式附加，
  回應的 `teacher_memory` 顯示附加的內容；不帶習慣的 key 也不帶。試產生仍不寫 `ai_interactions`。
- 學習單「Manna 已帶入的資訊」只在該次確實附加時顯示習慣。

### 研究紀錄：`ai_interactions.input_snapshot.teacher_memory`

```json
{
  "enabled": true,
  "policy_version": "teacher-habits-v1",
  "order": "confirmed_at",
  "memories": [{"id": 12, "kind": "choice_options", "value": "3", "statement": "選擇題使用 3 個選項。", "valid_at": "...", "...": "..."}],
  "context_prompt": {"key": "teacher_memory_context", "version_no": 1, "prompt_version_id": 87},
  "system_appended": "\n\n下方 teacher_habits 是老師確認過的出題偏好資料……",
  "user_appended": "\n\nteacher_habits（JSON 資料，依確認時間排序）：[…]",
  "graph": {"status": "ranked", "ranked_ids": [14, 12], "group_id": "manna_t3_r5_…", "revision": 5,
            "graphiti_version": "0.31.2", "reindex_requested": false}
}
```

步驟本身的 `prompt.system`／`prompt.user` 紀錄維持原樣；實際送出的內容＝該紀錄＋
`system_appended`／`user_appended`（逐字）。`graph.status`：`not_configured`、`unavailable`、
`not_indexed`、`core_changed`、`missing_group`、`empty_group`、`error`、`ranked`。

### 支援的習慣與隱私邊界

為避免模型整理後仍留下姓名／健康資訊，第一版採 **封閉代碼 → 程式產生句子**。
不保存任意模型改寫的自由文字。支援：

| 類型 | 值 |
|---|---|
| 選擇題選項數 | 2–5 |
| 配對組數 | 2–10 |
| 避免題幹開頭 | Which of the following |
| 作答說明／題目語言 | 英文、繁中、中英雙語 |
| 教師參考答案、字彙提示、題號 | 提供／省略 |
| 題幹長度 | 簡短、一般 |
| 預設難度 | 基礎、一般、挑戰 |

超出範圍、多項要求、單次修改不建立候選。整理模型由部署的 `AI_PROVIDER`／`AI_MODEL`
決定，不偷偷切換供應商。未進行付費供應商品質評估；測試使用合成輸入與模型 fixture。

#### 整理呼叫的紀錄（`teacher_memory_extract`）

- 回覆只由封閉代碼、布林值與 id 組成（`kind`∈KINDS、`value`∈VALUES）時，**一律保存在 output**，
  伺服器拒絕的也保存，`check_results.reasons` 寫原因：`unsupported`、`not_persistent`、
  `value_not_allowed_for_kind`、`replaces_id_mismatch`。
- 其他回覆（可能含自由文字）不保存，`error_detail` 只寫失敗類別，不寫供應商本文
  （本文可能引用草稿）：`bad_format: …`、`refusal: …`、`http_error: HTTP 503`、
  `http_error: network error`、`timeout: …`。

#### ⚠️ 待 PI 核准：原始草稿只存雜湊與字數

老師輸入的原始草稿只在整理請求的記憶體中存在，會傳給已設定的模型，但**不寫入** Postgres／
Graphiti／研究紀錄。研究紀錄只有其字數與 SHA-256 摘要（`input_snapshot.draft`）及不含草稿的
完整提示詞／版本。因此 **無法從研究紀錄重播或還原原始草稿**。這是實作者採取的隱私預設，
與「每次模型呼叫輸入都可追溯」的研究原則有出入，**尚待 PI 正式核准**；若 PI 決定要保存全文，
需另行調整（並評估學生資訊的風險）。

確認後的習慣、生成時附加的內容、Graphiti 的全部模型請求／回覆則完整可追溯。
Graphiti 的 episode 只有程式產生、老師確認過的習慣句子，不包含原始對話／教材／草稿。
新增習慣類型需擴充代碼、驗證與 canonical renderer，並更新 `POLICY_VERSION`。

## Phase 1：Postgres 正本

Migration `0040_teacher_memory.sql`、`0041_memory_sidecar_version.sql`、
`0042_teacher_memory_hardening.sql`（0040／0041 未修改；所有修正都在新的 0042）：

- `memory_candidates`：老師專屬的安全預覽，15 分鐘有效，確認或取消（`cancelled_at`）只能一次。
- `teacher_memories`：`valid_at`、`invalid_at`、`superseded_by`、失效原因及整理 interaction。
  資料庫 trigger：
  - 禁止 DELETE 與 **TRUNCATE**；
  - INSERT 必須是有效狀態（無 `invalid_at`／`superseded_by`／原因），`valid_at` 必須是該交易內
    的資料庫時間（不能補登過去或未來）；
  - UPDATE 只允許第一次失效，內容與確認時間不可改，`invalid_at` 必須是當下時間；
  - `superseded_by` 必須是**同一位老師、同一類型**、在失效同一時點生效的新版本（commit 時檢查）。
  停止使用保留研究歷程，並非個資硬刪除。
- 確認與停止使用鎖該老師列；取代、時點與索引 outbox 在同一交易內完成。
  同類型只能有一項有效；過期、已取消或與目前版本不一致的候選回 409，必須重新預覽。
  同時確認同一預覽、同時兩個取代、確認與停止使用同時發生，都只會有一個成功（測試涵蓋）。

## Phase 2：Graphiti 索引（可選）

Rust API → 內部驗證 HTTP → 自寫薄 FastAPI sidecar → graphiti-core **0.31.2** → FalkorDB。
只需要維護 `memory-sidecar/app.py`。採薄服務而非官方參考 REST server，原因是官方
`POST /messages` 先回 202，工作留在不持久的程序內佇列；這裡需要同步完成回覆與
每個老師／revision 專屬的模型追蹤 token。來源：
[官方 0.31.2 ingest](https://github.com/getzep/graphiti/blob/v0.31.2/server/graph_service/routers/ingest.py)。

**索引只產生研究用的排序 metadata，永遠不改變提示詞內容或順序。** Graphiti 失效、離線或
不相容時，生成照常帶入全部有效正本。

### 啟用條件

主服務同時設定 `GRAPHITI_URL`、`GRAPHITI_TOKEN`、`MEMORY_PROXY_URL`，**而且**
`AI_PROVIDER` 是 `openai` 或 `openai_compatible`、`EMBEDDING_PROVIDER=openai_compatible`
（有 `EMBEDDING_BASE_URL`）。任一不符時啟動 log 警告一次，索引保持關閉（只用 Postgres）：
不啟動 worker、不開內部連接埠、不排入索引工作，後台「索引待同步／失敗」都是 0。
之後才設定索引時，worker 啟動會比對每位老師最後變更時間與已發布快照時間，自動排入需要重建的老師
（`indexed_snapshot_at`），也可以按「從正本重建」。

### 同步、搜尋、失敗與重建

1. 每次確認／停止使用（有設定索引時）在交易內增加 `memory_indexes.revision`。worker 每 10 秒接手；
   lease 10 分鐘，HTTP 索引上限 8 分鐘。中斷後 lease 到期就能由新程序恢復。
2. 每次嘗試使用獨立 group（老師 + revision + 隨機碼），把 **目前有效的正本快照**
   按確認時間順序寫入。`reference_time` 使用老師確認時間，不用索引重建時間。
3. 寫完、版本相符、而且 **lease 仍有效、revision 未變** 時才 CAS 發布，並記下建立它的核心版本
   （`indexed_graphiti_version`）。索引期間老師又改了習慣：舊 revision 不發布，該次 group 排入清除，
   下一輪重建最新快照（測試涵蓋）。
4. HTTP／模型錯誤指數退避，最長 1 小時；失敗時重新檢查 sidecar 健康狀態。
5. 搜尋使用 **BM25 + RRF**，不呼叫 LLM 或 embedding；查詢只有 canonical 習慣。只送 FalkorDB
   **唯讀查詢**（`GRAPH.RO_QUERY`），sidecar 也先確認 group 存在，**搜尋絕不會建立或重建 group**。
   排除 `invalid_at`／`expired_at` 的事實，以 episode 來源回傳 id，Rust 再與本人有效 id 取交集。
6. 以下情況不搜尋、改為排入該老師重建（`reindex_requested`）：
   - 已發布 group 由不同的 graphiti-core 版本建立（核心升級後）；
   - sidecar 回報 group 不存在（`missing`）或沒有任何 episode（`empty`），但 Postgres 有有效習慣。
7. 清除：`memory_index_garbage` 持久記錄未發布的嘗試（15 分鐘後）與被取代的 group
   （2 分鐘寬限，讓進行中的搜尋結束）；**目前發布的 group 永不刪除**，即使誤排入。
   worker 每 10 分鐘列出 FalkorDB 中的 Manna group（`GET /groups`），沒有任何紀錄指向的
   （例如舊版 sidecar 搜尋時意外建立的空 group）排入清除。FalkorDB 一個 group 是一張圖，清理用 graph delete。

**中文全文檢索的現況**：FalkorDB 全文索引只依空白與 ASCII 標點切詞，一段連續中文是一個 token。
中文事實只有在與查詢共享完全相同的一段文字（或其中的 ASCII 詞如「3」）時才會命中；
模型改寫過的中文事實通常查不到。sidecar 的 Chinese smoke test 記錄了這個行為。這也是
排序只當 metadata、提示詞順序固定的原因。

本版每次變更都重建該老師的完整有效快照（最多 10 項），寫入模型成本會隨有效習慣數增加。

### 模型轉發（只在內部連接埠）

Sidecar 沒有上游 API key。它的 chat／embedding 請求回到主服務的 **內部連接埠**
`INTERNAL_PORT`（預設 **8081**）的 `/api/internal/memory/v1/*`：

- 只在設定索引時開啟；公開的 `PORT`（8080）路由裡**沒有**這些路徑（回 404）。
  主服務 compose 只發布 `127.0.0.1:8080`，nginx／Coolify proxy 只轉到 8080，8081 從未發布。
- 驗證在讀取 body **之前**完成：`X-Manna-Service-Token`（= `GRAPHITI_TOKEN`）**與**
  每個工作的短期 job token（`Authorization: Bearer`，資料庫只存 hash，完成、失敗或 lease 到期即失效）
  都必須正確，否則 401。
- 必須帶 `X-Manna-Memory-Api-Version`（不符 409）；`X-Manna-Graphiti-Version` 必須等於
  **領取工作時檢查到的核心版本**，否則 409（升級途中不混用）。研究紀錄的 `graphiti_version`
  是 API 在領取工作時檢查的版本；sidecar 回報的另記為 `graphiti_version_reported_by_sidecar`。
- 請求只接受白名單欄位，其餘一律 400：
  - chat：`messages`（1–32 則，role 只能 system／user／assistant，內容為純文字）、`temperature`
    （0–2）、`max_tokens`（夾在 256–8192）、`response_format`（text／json_object／json_schema）、
    `n`（只能 1，並強制送 `n: 1`）、`model`（忽略，一律用設定的 `AI_MODEL`）。不接受 `tools`、
    `stream` 等。
  - embeddings：`input`（字串或最多 64 筆，每筆 ≤ 8192 字）、`encoding_format`（float／base64）、
    `model`（忽略，用 `EMBEDDING_MODEL`）。
- 每個工作最多轉發 300 次模型呼叫，超過回 429。
- 每次上游嘗試各自寫 `ai_interactions`（`memory_graph_extract`／`memory_graph_embedding`），
  含實際轉發的 request、revision、版本、`job_call_no`。chat 回覆完整保存；**embedding 回覆只存每個向量
  的維度與 SHA-256**，不存向量本身。失敗的 `error_detail` 分 `http_error`／`bad_format`／`timeout`。
- Graphiti telemetry 在 import 前關閉。任何故障都不改用別的模型。

### 健康檢查與版本

`GET /health`（服務 token）只對 FalkorDB 做一次 PING 並回報：

```json
{"ok":true,"api_version":1,"graphiti_version":"0.31.2","policy_version":"teacher-habits-v1"}
```

不建立 driver、不建索引（索引在 `/index` 建立該 group 時建立一次；每個 group 是獨立的圖）。
Manna 將結果（相容或不相容）**快取 45 秒**，不會在每次搜尋前呼叫；索引失敗時立即重新檢查。
只接受相容的 `api_version`、`policy_version` 及有明確核心版本的回應，不把 graphiti-core
鎖死成某個 patch 版本。離線、缺欄位或不相容時不領取寫入工作、不搜尋圖，直接帶入 Postgres 的有效習慣，
不讓老師的生成請求出錯，也不修改研究開關。服務恢復相容後自動接續（最多等一個快取週期）。

## 獨立 Coolify 資源（先在測試環境）

主服務使用根目錄 `docker-compose.yml`；`memory-sidecar/docker-compose.yml` 是**另一個 Compose 專案**，
只包含 sidecar 與 FalkorDB。兩邊沒有 build 或 depends_on 關係，不合併成同一個 compose。

### 1. 在同一台 Docker 主機建立專用網路

```sh
docker network create --internal manna-memory
```

只給兩個資源使用的 external network，不是所有 Coolify 應用共用的網路。
注意：Coolify 會自動把它自己的「每個資源一個」網路（以資源 UUID 命名，非 internal）接到每個用
`networks:` 的服務上，所以 sidecar 仍能經由該網路連外；`--internal` 只保證 `manna-memory` 這段
不能當出口。FalkorDB 用 `network_mode` 只接本資源的 internal 網路（見下），完全沒有對外路徑。

### 2. 建立記憶資源

在 Coolify 同一主機建立 **Git repository / Docker Compose Application**：

| 欄位 | 設定 |
|---|---|
| Resource name | `manna-memory` |
| Repository | 與 Manna 相同的 GitHub repo |
| Branch | `main`（獨立追蹤，不依賴主服務的 `deploy` 分支快轉） |
| Base Directory | `/memory-sidecar` |
| Docker Compose Location | `/docker-compose.yml`（相對於 Base Directory） |
| Watch Paths | `memory-sidecar/**`（相對於 repo 根目錄） |
| Connect to predefined network | **關閉**；不接全域 Coolify 應用網路 |
| Domains | 留空；sidecar 與 FalkorDB 都不設定網域、不發布 host port |

Environment Variables：

| 變數 | 誰設定 | 值 |
|---|---|---|
| `GRAPHITI_TOKEN` | **人工**（產生一次，兩個資源都填同一個） | 例如 `openssl rand -hex 32` 的輸出 |
| `SERVICE_PASSWORD_FALKORDB` | **Coolify magic variable**（自動產生，不要手填） | FalkorDB 的 `requirepass`，同時傳給 sidecar 的 `FALKORDB_PASSWORD` 與健康檢查 |
| `MEMORY_NETWORK_NAME` | 人工（可省略） | `manna-memory`（預設值） |
| `MEMORY_PROXY_URL` | 人工（可省略） | `http://manna-api:8081/api/internal/memory/v1`（預設值；**8081 內部連接埠**） |

FalkorDB 設定（compose 已寫好）：

- 映像 `falkordb/falkordb:v4.12.0@sha256:7927eb19…`（digest 固定），`BROWSER=0` 關閉內建瀏覽器 UI。
  （`falkordb/falkordb-server` 沒有 v4.12.0 版本，為了不在同一 PR 換資料庫版本，沿用原映像並關閉 browser。）
- `REDIS_ARGS="--requirepass ${SERVICE_PASSWORD_FALKORDB}"`；`FALKORDB_ARGS` 覆寫時保留映像預設：
  `THREAD_COUNT 2 MAX_QUEUED_QUERIES 25 TIMEOUT 1000 RESULTSET_SIZE 10000`。
- **資料 volume 掛在 `/var/lib/falkordb/data`**（映像的 `FALKORDB_DATA_PATH`；舊設定的 `/data`
  不會被寫入，重啟後索引會消失）。
- `network_mode: "${COMPOSE_PROJECT_NAME}_memory-private"` + `hostname: falkordb`：FalkorDB 只在本資源的
  internal 網路上（名稱 `<資源 UUID>_memory-private`），連 Coolify 的資源網路也不接
  （Coolify 不替使用 `network_mode` 的服務加網路）。sidecar 以 `falkordb` 解析它。
- 兩個容器各限 512 MB。這是限制，**不是實測峰值或容量保證**。不要設定固定 container_name。

[Coolify 官方 Compose 說明](https://coolify.io/docs/applications/builds/docker-compose)
說明 Base Directory 與 Compose Location 的組合方式；載入設定後檢查 deployable compose 確實保留
external network、aliases 與 FalkorDB 的 `network_mode`，再部署此資源。

### 3. 設定主服務

主服務保持原本根目錄 compose，設定：

| 變數 | 誰設定 | 值 |
|---|---|---|
| `MEMORY_NETWORK_EXTERNAL` | 人工 | `true` |
| `MEMORY_NETWORK_NAME` | 人工 | `manna-memory` |
| `GRAPHITI_URL` | 人工 | `http://manna-memory:8000` |
| `GRAPHITI_TOKEN` | **人工**，與記憶資源完全相同 | （秘密） |
| `INTERNAL_PORT` | 可省略 | `8081`（預設；不可等於 `PORT`） |
| `MEMORY_PROXY_URL` | 可省略 | `http://manna-api:8081/api/internal/memory/v1`（預設） |

主服務在 `manna-memory` 網路的固定別名為 `manna-api`。**不要**為 8081 設定網域、port 對應或
nginx 轉發。既有 Postgres／MinIO 網路、`127.0.0.1:8080` 入口不變（主服務原本就開啟
predefined network 以連到資料庫；同網路上的其他容器若連到 8081，仍需服務秘密與有效 job token）。
模型仍由 `AI_PROVIDER=openai_compatible`（或 `openai`）、`AI_MODEL` 與對應 key 決定，embedding 設定也留在主服務。
主服務 Watch Paths 設為自己的檔案範圍，例如 `api/**`、`web/**`、`Dockerfile`、`docker-compose.yml`、
`.dockerignore`，不含 `memory-sidecar/**`。

未啟用 Phase 2 時，`MEMORY_NETWORK_EXTERNAL` 預設 false、網路名稱預設專案專屬，
不要求主機預先建 external network；空的 `GRAPHITI_URL`／`GRAPHITI_TOKEN` 不啟動 worker、不開 8081。
Preview／測試資源必須用不同的 network name，避免固定別名指到正式資源。

### 4. 部署後檢查

```sh
# FalkorDB 只在 <UUID>_memory-private 上，沒有其他網路（network_mode）
docker inspect --format '{{.HostConfig.NetworkMode}} {{json .NetworkSettings.Networks}}' <falkordb 容器>
# 資料目錄與 volume
docker inspect --format '{{json .Mounts}}' <falkordb 容器>   # Destination 應為 /var/lib/falkordb/data
# 沒有密碼不能連
docker exec <falkordb 容器> sh -c 'REDISCLI_AUTH= redis-cli ping'   # 應回 NOAUTH
```

再確認兩邊可用固定別名解析、後台顯示記憶介面版本與 Graphiti 版本、待同步／失敗數歸零，
最後由 PI 在後台開啟研究開關。沒有相容模型或 embedding 時，索引不啟用，正本不受影響。

### 本機分開啟動與獨立升級

```sh
# 建好網路及環境變數後，兩條命令屬於不同資源；不要合併成同一個 compose 專案。
MEMORY_NETWORK_EXTERNAL=true MEMORY_NETWORK_NAME=manna-memory docker compose up -d --build
# 第二個命令只部署 memory + falkordb（需要 GRAPHITI_TOKEN 與 SERVICE_PASSWORD_FALKORDB）。
docker compose --project-directory memory-sidecar -f memory-sidecar/docker-compose.yml up -d --build
```

相容的核心升級：修改 `requirements.in`、重新產生鎖定檔、通過 sidecar CI、合併後只由其 Watch Paths
觸發獨立部署。新核心啟動後，舊核心建立的 group 不再被搜尋，worker 依序為每位老師重建。
介面不相容時必須升 `API_VERSION`；主服務會留在 Postgres，直到另外升級支援該介面。
若圖索引格式或核心升級失敗，可先回退記憶資源版本，保留 Postgres 正本，再清空此獨立資源的
FalkorDB volume 並重建。**不要**刪主服務或主資料庫的 volume。

### 鎖定相依套件

`memory-sidecar/Dockerfile` 使用 digest 固定的 `python:3.12-slim`，以
`pip install --require-hashes --only-binary=:all: -r requirements.txt` 安裝。`requirements.in` 是頂層版本，
`requirements.txt` 是含 hash 的鎖定檔，在同一個 digest 的映像中產生：

```sh
docker run --rm -v "$PWD/memory-sidecar":/w -w /w \
  python:3.12-slim@sha256:5e2bdb1b0d542d5039cffb8181c65930093949194a65c365dfb1b0fe13a4fa5b \
  sh -c 'pip install -q pip-tools==7.5.2 && pip-compile --allow-unsafe --generate-hashes \
         --no-emit-index-url --strip-extras --output-file=/tmp/r.txt requirements.in && cp /tmp/r.txt requirements.txt'
```

### 備份還原與回復

Coolify Postgres 備份涵蓋所有正本、歷程、研究與 outbox 表。`memory-graph` volume
不在該資料庫備份範圍；它可整個丟棄再由正本重建。

1. 還原 Postgres；啟動 sidecar 與空 FalkorDB（API 暫時依正本工作；搜尋發現 group 不存在時自動排入重建）。
2. PI 到後台按「從正本重建記憶索引」，或 `POST /api/admin/memory-index/rebuild`。
3. worker 重新寫每個老師有效的習慣，使用原確認時間，等待待同步／失敗數歸零。
4. 若需停止介入，PI 關閉研究開關；若需停用索引，清空主服務的 `GRAPHITI_URL`／`GRAPHITI_TOKEN`，
   並獨立停止記憶資源。不要移除 Postgres 表；老師歷程與研究紀錄應保留。

## API

所有教師端點要求有效登入，僅能讀寫本人：

| 方法／路徑 | 輸入／用途 |
|---|---|
| `GET /api/me/memories` | 全歷程、研究開關、`graph_enabled`、索引狀態（未設定索引時 `index: null`） |
| `POST /api/me/memories/preview` | `{text}`；安全候選 `{id,statement,replaces_id}` |
| `POST /api/me/memories` | `{candidate_id,confirmed:true}` 確認 → `{id}`；與有效習慣相同 → `{id,unchanged:true}`；`{candidate_id,confirmed:false}` 取消 → `{ok,cancelled:true}`；過期／已確認／已取消／過時 409 |
| `DELETE /api/me/memories/{id}` | 停止使用本人有效的習慣 |
| `GET/PUT /api/admin/memory-settings` | PI；`{enabled}` 與 `graph_enabled` |
| `GET /api/admin/memory-index` | PI；`{pending,failed,graph_enabled,compatible,sidecar}`（未設定索引時皆為 0／false） |
| `POST /api/admin/memory-index/rebuild` | PI；從全部正本重建 |

內部連接埠（`INTERNAL_PORT`，不經 proxy）：

| 方法／路徑 | 用途 |
|---|---|
| `POST /api/internal/memory/v1/chat/completions` | 服務秘密 + job token + 版本 header；白名單欄位；研究轉發 |
| `POST /api/internal/memory/v1/embeddings` | 同上；batch ≤ 64 |

Sidecar（只在記憶資源內部，服務 token）：`GET /health`、`POST /index`、`POST /search`
（回 `{memory_ids, group: ok|missing|empty}`）、`GET /groups`、`DELETE /groups/{id}`（冪等）。

## 驗證

- API（`api/tests/teacher_memory.rs`）：確認前不存記憶、取消送到伺服器並記錄、跨老師隔離、
  取代／移除歷程、相同習慣重複確認不新增版本、trigger 擋補登／預先失效 INSERT、TRUNCATE、
  跨老師 superseded_by；開關關閉時每個步驟提示詞逐字不變、檢查規則不變；開關開啟時只有名單內步驟
  帶入、紀錄附加內容與版本；端到端學習單（開關關：4 個選項；開：3 個選項且不需修復）；試產生；
  同時確認／取代／停止使用；索引失敗重試、lease 復原、索引期間有新變更不發布、核心升級／group 消失
  觸發重建且不搜尋、清除不刪已發布 group 並回收孤兒 group、健康檢查快取、不相容（介面與 policy）
  回到 Postgres；轉發只在內部連接埠、先驗證再讀 body、服務秘密與 job token、版本 header、
  欄位白名單、n／max_tokens／batch／每工作次數上限、embedding 只存維度與 hash。
- Web：先預覽再明確確認、取消送到伺服器、重複確認提示、取代提示與歷程、停止使用二次確認、
  PI 開關與索引重建。
- Sidecar（`memory-sidecar/test_app.py`、`test_deployment.py`）：服務驗證、canonical episode／reference_time、
  失效／過期事實過濾、搜尋只用唯讀 driver 且不建立 group、missing／empty 回報、`/health` 只 PING、
  SDK 實際送出的欄位都在 API 白名單內且帶兩種憑證；真正的 Graphiti + FalkorDB 寫入／搜尋／清除、
  刪除後搜尋不會重建 group、中文全文檢索現況（模型／embedding 為合成 fixture）；兩份 compose 的
  網路隔離、FalkorDB 只在 private 網路、volume 路徑、密碼與 browser 設定、8081 不發布。

```sh
python -m pip install --require-hashes -r memory-sidecar/requirements.txt
python -m unittest discover -s memory-sidecar -v
# 有測試 FalkorDB（--requirepass）時：
MEMORY_SMOKE=true FALKORDB_HOST=127.0.0.1 FALKORDB_PASSWORD=<密碼> python -m unittest discover -s memory-sidecar -v
```

CI 的 `memory` job 在 `memory-sidecar/**`、`api/src/memory/**` 或根目錄 `docker-compose.yml` 變更時執行
上述測試（FalkorDB 服務同樣關 browser、設密碼）、兩份 Compose 的檢查與 sidecar 映像建置；
單改 sidecar 不觸發主應用建置／部署。
