# 教師長期記憶（#82，Phase 1 + Phase 2）

## 使用與研究控制

「備課偏好 → 我的出題習慣」一次整理一項長期習慣，先預覽，再按「確認並記住」。
修改走相同的預覽／取代確認；「停止使用」只標失效，歷程不刪除。每位老師最多 10 項。
記憶範圍只有該老師，不依班級或教材另存，也不保存對話摘要。沒有自動讀取對話或背景擷取。

後台「教師長期記憶」的研究開關預設 **關閉**。PI 同意介入後才開啟；設定變更寫入
`usage_logs`。開啟後，學習單與備課的模型步驟（包含修復）帶入所有有效習慣，
本次教師明確要求優先。記憶是 JSON 資料，不能變更系統規則或輸出結構。
學習單「Manna 已帶入的資訊」會顯示該次確實帶入的習慣。

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
決定（要用 terra 就設定成該模型），不偷偷切換供應商。偏好只能在該生成步驟的結構與
既有內容檢查允許時生效，例如英文教材題目的英文檢查仍有效，不能藉習慣繞過。
未進行付費供應商品質評估；測試使用合成輸入與模型 fixture。

原始草稿只在整理請求記憶體中存在，會傳給已設定的模型，但不寫 Postgres／Graphiti。
研究紀錄只存其字數、SHA-256 摘要及不含草稿的完整提示詞／版本。模型回覆只保留通過
封閉代碼驗證的欄位，失敗時不存自由文字或供應商錯誤本文。
這是刻意的隱私例外：**不能從研究紀錄還原原始草稿**。

確認後的習慣、後續生成的記憶上下文、Graphiti 全部模型請求／回覆則完整可追溯。
Graphiti 的 episode 只有程式產生、老師確認過的習慣句子，不包含原始對話／教材／草稿。
新增習慣類型需擴充代碼、驗證與 canonical renderer，並更新 `POLICY_VERSION`。

## Phase 1：Postgres 正本

Migration `0040_teacher_memory.sql`：

- `memory_candidates`：老師專屬的安全預覽，15 分鐘有效，確認只能使用一次。
- `teacher_memories`：`valid_at`、`invalid_at`、`superseded_by`、失效原因及整理 interaction。
  資料庫 trigger 禁止 DELETE、修改文字與重改失效紀錄。停止使用保留研究歷程，並非個資硬刪除。
- 確認與停止使用鎖該老師列；取代、時點與索引 outbox 在同一交易內完成。
  同類型只能有一項有效；過期或與目前版本不一致的候選回 409，必須重新預覽。
- 新舊版本在同一個資料庫時間點交接。刪除候選或取消預覽不會修改有效記憶。

`ai_interactions.input_snapshot.teacher_memory` 記錄研究開關、policy 版本、
每項記憶的 id／句子／確認時間與排序來源。關閉時記錄 `enabled=false`、空記憶列表。
原有的 provider、model、prompt_version_id、輸出和重試紀錄繼續適用。

## Phase 2：Graphiti 索引

Rust API → 內部驗證 HTTP → 自寫薄 FastAPI sidecar → graphiti-core **0.31.2** → FalkorDB。
只需要維護 `memory-sidecar/app.py`。採薄服務而非官方參考 REST server，原因是官方
`POST /messages` 先回 202，工作留在不持久的程序內佇列；這裡需要同步完成回覆與
每個老師／revision 專屬的模型追蹤 token。來源：
[官方 0.31.2 ingest](https://github.com/getzep/graphiti/blob/v0.31.2/server/graph_service/routers/ingest.py)。

### 同步、搜尋、失敗與重建

1. 每次確認／停止使用，在交易內增加 `memory_indexes.revision`。worker 每 10 秒接手；
   lease 10 分鐘，HTTP 索引上限 8 分鐘。中斷後 lease 到期就能由新程序恢復。
2. 每次嘗試使用獨立 group（老師 + revision + 隨機碼），把 **目前有效的正本快照**
   按確認時間順序寫入。`reference_time` 使用老師確認時間，不用索引重建時間。
3. 所有寫入完成且 core／policy 版本符合後，CAS 發布該 revision 的 group。
   索引過程若老師改變習慣，舊 revision 不會發布；下一輪重建最新快照。
4. HTTP／模型錯誤指數退避，最長 1 小時。原本的 PostgreSQL 習慣不受影響；
   不完整或過期索引永不參與搜尋。
5. 搜尋使用 **BM25 + RRF**，不呼叫 LLM 或 embedding。搜尋文字只有 canonical 習慣，
   不送原始對話／教材。排除 `invalid_at`／`expired_at` 的事實，以 episode 來源回傳 id。
   Rust 再與目前老師有效 id 取交集；只調整所有有效習慣的順序，不加入圖中的自由文字。
6. 沒有索引、搜尋失敗／超過 2 秒時，仍帶入全部有效正本（最多 10 項）。
7. `memory_index_garbage` 持久記錄失敗嘗試與舊 group；延後清除並在失敗時重試。
   不會刪目前發布的 group。FalkorDB 將 group 對應到獨立圖，因此清理使用 graph delete。

本版每次變更都重建該老師的完整有效快照（最多 10 項），所以寫入模型成本會隨
有效習慣數增加，不是只處理新增的一項。請在測試環境用目標模型確認延遲與額度。

Graphiti 只索引目前有效的句子；**完整歷程與取代關係以 Postgres 為準**，不依賴模型重推。
目前的檢索僅對確認習慣排序；班級／教材關聯、對話摘要仍需獨立確認隱私與研究設計，
本實作不把這些尚未決定的資料送入 Graphiti。

### 模型轉發與驗證

Sidecar 沒有上游 API key。每次同步領取隨機的短期 job token，資料庫只存 hash；
完成、失敗或 lease 過期即不能使用。所有 chat／embedding 請求只轉發到 API 設定的
模型和 URL（忽略 sidecar 傳入的 model）；每次上游嘗試各自寫 `ai_interactions`，
含完整 request／response、revision、graphiti／policy 版本及供應商回報的 model。
重試的 HTTP 呼叫也各自記錄，不會被 SDK 隱藏。SDK HTTP 自動重試關閉；Graphiti
本身的重試仍會重新通過轉發端點。串流不支援。

- `GRAPHITI_TOKEN`：API 到 sidecar 的服務間秘密，不能給瀏覽器。
- 每個工作獨立 token：sidecar 到 API 的模型轉發驗證，不是老師登入 token。
- Sidecar 只接受與設定一致的 `MEMORY_PROXY_URL`，避免任意 URL 轉發。
- Graphiti telemetry 在 import 前關閉；`GRAPHITI_TELEMETRY_ENABLED=false`。
- 認證後的 sidecar API 仍只放內部網路，不開外部 port。任何故障都不啟用別的模型。

## 部署（先在測試環境）

現有主應用保持不加 Graphiti 也能運作。Phase 2 使用 opt-in overlay：

```sh
# 在部署平台秘密設定內設定 GRAPHITI_TOKEN（例如 openssl rand -hex 32 的結果）
# 同時設定 AI_PROVIDER=openai_compatible 或 openai、AI_MODEL 與對應 key。
# embedding 必須設定 EMBEDDING_PROVIDER=openai_compatible、BASE_URL、API_KEY、MODEL、DIMS。
docker compose -f docker-compose.yml -f docker-compose.memory.yml up -d --build
```

Coolify 選這兩份 compose 檔案；維持預定網路連線以存取現有 Postgres。
Sidecar 與 FalkorDB 在 `internal: true` 網路，只有 app 同時在該網路與原本的網路。
它們各限制 512 MB（合計 1 GB）；這是部署上限，**不是已量測的峰值或容量保證**。
應先以目標模型實測寫入量與主機記憶體。FalkorDB 映像固定 4.12.0 及 multiarch digest，
core／SDK／driver 固定版本，明確安裝 httpx。沒有 Neo4j、Qdrant 或額外公開入口。

| API 環境變數 | 範例 |
|---|---|
| `GRAPHITI_URL` | `http://memory:8000` |
| `GRAPHITI_TOKEN` | 部署平台秘密 |
| `MEMORY_PROXY_URL` | `http://app:8080/api/internal/memory/v1` |

缺任何一個 Phase 2 變數，就不啟動 worker；Phase 1 仍完整可用。
不支援 Anthropic／fake 作為 Graphiti 正式轉發來源；沒有相容模型或 embedding 時索引會重試，
不影響正本。測試環境應確認後台待同步／失敗數歸零，再由 PI 決定是否開啟研究開關。

### 備份還原與回復

Coolify Postgres 備份涵蓋所有正本、歷程、研究與 outbox 表。`memory-graph` volume
不在該資料庫備份範圍；它可整個丟棄再由正本重建。

1. 還原 Postgres；啟動 sidecar 與空 FalkorDB（API 暫時依正本工作）。
2. PI 到後台按「從正本重建記憶索引」，或 `POST /api/admin/memory-index/rebuild`。
3. worker 重新寫每個老師有效的習慣，使用原確認時間，等待待同步／失敗數歸零。
4. 若需停止介入，PI 關閉研究開關；若需停用索引，移除 overlay／Phase 2 環境變數。
   不要移除 Postgres 表；老師歷程與研究紀錄應保留。

## API

所有教師端點要求有效登入，僅能讀寫本人：

| 方法／路徑 | 輸入／用途 |
|---|---|
| `GET /api/me/memories` | 全歷程、研究開關、索引狀態 |
| `POST /api/me/memories/preview` | `{text}`；安全候選 `{id,statement,replaces_id}` |
| `POST /api/me/memories` | `{candidate_id,confirmed:true}`；確認記住 |
| `DELETE /api/me/memories/{id}` | 停止使用本人有效的習慣 |
| `GET/PUT /api/admin/memory-settings` | PI；`{enabled}` 與 graph_enabled |
| `GET /api/admin/memory-index` | PI；pending／failed 數 |
| `POST /api/admin/memory-index/rebuild` | PI；從全部正本重建 |
| `POST /api/internal/memory/v1/chat/completions` | 僅有效 job token；研究轉發 |
| `POST /api/internal/memory/v1/embeddings` | 同上 |

## 驗證

- API：確認前不存記憶、跨老師隔離、取代／移除歷程、資料庫不可改寫歷程、過期與過時預覽、
  PI 開關、記憶上下文、失敗重試與 lease 復原、空快照、過期索引排除、id 取交集、模型轉發追蹤。
- Web：先預覽再明確確認、取代提示與歷程、停止使用二次確認、PI 開關與索引重建。
- Sidecar：服務驗證、canonical episode／reference_time、失效／過期事實過濾、BM25 不用模型、
  真正的 Graphiti + FalkorDB 寫入／搜尋／清除 smoke test（模型／embedding 為合成 fixture）。

```sh
pip install -r memory-sidecar/requirements.txt
python -m unittest discover -s memory-sidecar -v
# 有測試 FalkorDB 時：MEMORY_SMOKE=true FALKORDB_HOST=127.0.0.1 加在上方指令前
```

CI 對 sidecar／overlay 的變更執行上述 smoke test 並 build sidecar 映像。
