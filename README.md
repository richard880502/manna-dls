# Manna DLS

AI 輔助英文教師備課的研究型平台（國科會蔓娜計畫）。Phase 1 範圍：單元前 CoRe（含三層 AI 協助）與 Study & Plan（RAG 備課建議與教師判斷紀錄）；Phase 1.2 加上 Teach & Record（課中書籤、手機遙控、離線同步）與課後補寫。

- 程式模組與依賴邊界：[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- 設計藍圖：[docs/BLUEPRINT.md](docs/BLUEPRINT.md)
- API 合約：[docs/API.md](docs/API.md)
- 教材與註記的工程回覆（規格 §12）：[docs/ENGINEERING_REPLY.md](docs/ENGINEERING_REPLY.md)

## 結構

```
web/        React 19 + TypeScript + Vite（前端）
api/        Rust（axum + sqlx）後端，migrations 在 api/migrations
Dockerfile  單一映像：前端靜態檔由後端提供
docker-compose.yml  Coolify 部署用（只有 app；資料庫是 Coolify 獨立的 PostgreSQL 資源，見 docs/BLUEPRINT.md §10）
.github/workflows/ci.yml  CI：前端、後端測試、Docker 建置，main 通過後觸發 Coolify 部署
```

## 本機開發

```bash
# 資料庫
docker run -d --name manna-pg -e POSTGRES_USER=manna -e POSTGRES_PASSWORD=manna \
  -e POSTGRES_DB=manna -p 127.0.0.1:55432:5432 pgvector/pgvector:pg17

# 後端（AI_PROVIDER=fake 不需要任何金鑰）
cd api
DATABASE_URL=postgres://manna:manna@127.0.0.1:55432/manna AI_PROVIDER=fake \
  ADMIN_BOOTSTRAP_CODE=dev-admin cargo run

# 前端（另一個終端機；/api 會代理到 127.0.0.1:8080）
cd web && pnpm install && pnpm dev
```

用 `ADMIN_BOOTSTRAP_CODE` 的值登入就是主持人帳號，可在後台建立教師邀請碼、在「帳號與角色」變更已登入過的帳號的角色（例如改成知識庫管理員），並在「知識庫」的「課綱條目與詞彙」分頁匯入課綱條目與詞彙表（資料不在 repo，匯入檔的產生方式見 `docs/API.md`「Phase 2.0：知識庫與詞彙」）。

## AI 供應商

| `AI_PROVIDER` | 用途 |
|---|---|
| `none` | 預設。AI 功能停用，所有流程可手動完成 |
| `fake` | 測試與 CI，固定輸出、不連網 |
| `anthropic` | 示範環境。必須同時設 `DEMO_MODE=true`，否則後端拒絕啟動（規格禁止把真實教師資料送境外） |
| `openai_compatible` | 國網中心模型（拿到 API 後設定 `OPENAI_COMPAT_BASE_URL`、`OPENAI_COMPAT_API_KEY`） |
| `openai` | OpenAI 的 GPT-5 以後模型（2026-11-19 起正式環境用 `AI_MODEL=gpt-5.6-terra`）。用配圖已經設好的 `OPENAI_API_KEY`，不需要另設網址；思考程度用 `AI_EFFORT`（`low`／`medium`／`high`，預設 `medium`）。資料會送到境外，經主持人同意才使用。要換回國網：`AI_PROVIDER=openai_compatible`、`AI_MODEL=gemma-4-31B-it` |

`AI_PROVIDER`、`AI_MODEL`、`AI_EFFORT`、`AI_TIMEOUT_SECS` 是**預設值**：主持人可以在「後台管理 → 平台設定 → 文字模型」改用 `openai` 或 `openai_compatible`（該來源的金鑰或位址要先設在環境變數）、換模型、推理強度與等待上限，系統會先試呼叫一次，成功才套用，不必重新部署；也可以一鍵恢復成環境變數的預設。契約見 `docs/API.md`「文字模型設定」。

**AI 請求流量控制**（設計見 `docs/AI_REQUEST_CONTROL.md`）：

| 變數 | 預設 | 意思 |
|---|---|---|
| `AI_QUEUE_ENABLED` | `false` | 開啟資料庫工作佇列（備課對話、整份學習單排隊分派）。**預設關閉**：先部署、確認沒問題，再由負責人改成 `true`；關閉時照舊直接執行 |
| `AI_MAX_RUNNING_JOBS` | `6` | 全站同時執行的工作數（每台伺服器） |
| `AI_MAX_RUNNING_PER_TEACHER` | `2` | 每位老師同時執行數（主持人不限），多的排隊 |
| `AI_MAX_QUEUED_PER_TEACHER` | `10` | 每位老師排隊上限，超過才拒絕（409） |
| `AI_JOB_LEASE_SECS` | `90` | 工作租約長度（每 1/3 租約延長一次） |
| `AI_JOB_MAX_ATTEMPTS` | `2` | 重啟後最多重跑幾次 |
| `AI_SHUTDOWN_GRACE_SECS` | `60` | 關閉時等正在跑的工作多久（Coolify 的停止等待時間要更長） |
| `AI_RATE_LIMIT_RETRIES` | `2` | 單次呼叫遇 429 的重試次數（依 Retry-After） |
| `AI_QUOTA_COOLDOWN_SECS` | `600` | 額度用完後多久內不再送出 |
| `FAKE_LATENCY_MS` | `0` | 只給 `AI_PROVIDER=fake`：每次假呼叫等這麼久，本機壓力測試用 |

### AI 請求控制

| 環境變數 | 預設 | 說明 |
|---|---:|---|
| `AI_RATE_LIMIT_RETRIES` | `2` | 單次模型呼叫遇到一般 429 時的重試次數；依服務回傳的等待時間重試 |
| `AI_QUOTA_COOLDOWN_SECS` | `600` | 偵測到額度用完後，暫停送出模型呼叫的秒數 |
| `AI_QUEUE_ENABLED` | `true` | 啟用資料庫工作佇列；關閉時保留既有背景工作行為 |
| `AI_MAX_RUNNING_JOBS` | `6` | 每台 API 執行個體同時執行的 AI 工作上限 |
| `AI_MAX_RUNNING_PER_TEACHER` | `2` | 每位教師同時執行的上限；主持人不受此限制 |
| `AI_MAX_QUEUED_PER_TEACHER` | `10` | 每位教師可等待的工作數上限 |
| `AI_JOB_LEASE_SECS` | `90` | 執行中工作租約的秒數；逾期會在啟動恢復時接手或標成失敗 |
| `AI_JOB_MAX_ATTEMPTS` | `2` | 因執行個體中斷而可重新排入的最大嘗試次數 |
| `AI_SHUTDOWN_GRACE_SECS` | `60` | 收到關閉訊號後等待執行中工作結束的秒數 |

### 語意索引（混合檢索）

知識庫檢索預設只用關鍵字。設定下列變數後，條目會建立語意索引，檢索改為關鍵字＋語意相似度（評估見 `docs/RETRIEVAL_EVAL.md`，細節見 `docs/API.md`「Phase 2.4」）。模型無法使用時自動退回關鍵字，不影響啟動與 AI 功能。

| 變數 | 正式環境的值 |
|---|---|
| `EMBEDDING_PROVIDER` | `openai_compatible`（預設 `none`；測試用 `fake`） |
| `EMBEDDING_BASE_URL` | `https://portal.genai.nchc.org.tw/api/v1` |
| `EMBEDDING_API_KEY` | 國網中心 embeddings 專用金鑰（與對話模型的金鑰不同；只設在 Coolify） |
| `EMBEDDING_MODEL` | `embeddinggemma-2` |
| `EMBEDDING_DIMS` | `768` |

設好重新部署後，啟動時會在背景建立索引；到「知識庫」→「課綱條目與詞彙」的「語意索引」確認「1,352 條中 1,352 條已建立語意索引」，沒完成可按「建立語意索引」續做。

### 掃描檔文字辨識（OCR）

掃描的 PDF、手機拍的課本頁面與文字層壞掉的 PDF，上傳後會在背景用國網中心的 `chandra-ocr-2` 逐頁辨識文字（細節見 `docs/API.md`「Phase 2.10」）。`docker-compose.yml`（正式環境）預設開啟，網址和金鑰沿用 embedding 那組，不用另外設定；要關掉就在 Coolify 設 `OCR_PROVIDER=none` 再重新部署。後端程式本身沒設這個變數時是關閉的（本機開發），關閉時這些檔案照舊保存，文字狀態維持「讀不到文字」。PDF 轉圖用 `pdftoppm`（Docker 映像已安裝 `poppler-utils`；本機開發要自己裝，例如 `brew install poppler`）。

| 變數 | 說明 |
|---|---|
| `OCR_PROVIDER` | `openai_compatible`（`docker-compose.yml` 的預設）、`fake`（測試；`AI_PROVIDER=fake` 且沒設這個變數時的預設）或 `none`（後端程式沒設時的預設） |
| `OCR_BASE_URL`、`OCR_API_KEY` | 沒設時沿用 `EMBEDDING_BASE_URL`、`EMBEDDING_API_KEY`（同一個國網中心閘道；金鑰只設在 Coolify） |
| `OCR_MODEL` | 預設 `chandra-ocr-2` |
| `OCR_TIMEOUT_SECS` | 每一頁請求的時限，預設 `120` |
| `OCR_MAX_PAGES` | 每個檔案最多辨識幾頁，預設 `40`（其餘頁略過並在教材上註明） |
| `OCR_CONCURRENCY` | 全站同時辨識的頁數，預設 `2` |

設定有誤（例如 `OCR_PROVIDER=openai_compatible` 卻沒有任何閘道網址）只會關掉 OCR 並在日誌警告，不會讓後端起不來。

### 學習單的圖片（OpenAI GPT Image）

學習單（尤其「照我的學習單做」）的區塊可以有圖：寫區塊的文字模型同時寫出「這裡需要什麼圖」，伺服器在區塊通過檢查後用 OpenAI 的 Image API 產圖，存進教材檔案儲存（MinIO），顯示在學習單、DOCX 與 PDF；老師可以逐張重畫、換成自己的圖或拿掉。細節見 `docs/API.md`「學習單的圖片」。`docker-compose.yml`（正式環境）**預設開啟**（`IMAGE_PROVIDER=openai`，金鑰 `OPENAI_API_KEY` 只設在 Coolify）；要關掉就在 Coolify 設 `IMAGE_PROVIDER=none` 再重新部署，關閉時送給文字模型的請求與以前完全相同。後端程式本身沒設時是關閉的（本機開發）。

- 使用 GPT Image 模型前，OpenAI 可能要求帳號先完成 **API Organization Verification**（由帳號負責人在 OpenAI 後台做證件驗證）；沒驗證時產圖會回 403，老師看到「產圖服務暫時無法使用（HTTP 403）」。
- 開啟後，老師的教材內容衍生的畫面描述會送到境外（OpenAI）；「真實教師資料不能送國外」的限制目前由主持人決定暫不適用，正式上線前要確認同意書。

| 變數 | 說明 |
|---|---|
| `IMAGE_PROVIDER` | `openai`（`docker-compose.yml` 的預設）、`none`（後端程式沒設時的預設）、`fake`（測試；`AI_PROVIDER=fake` 且沒設這個變數時的預設） |
| `OPENAI_API_KEY` | OpenAI 金鑰（只設在 Coolify；`openai` 沒有金鑰時圖片關閉並在日誌警告） |
| `IMAGE_BASE_URL` | 預設 `https://api.openai.com/v1` |
| `IMAGE_MODEL` | 預設 `gpt-image-2.5-flare`（模型汰換很快，所以做成變數） |
| `IMAGE_QUALITY` | `low`、`medium`（預設）、`high`、`xhigh`、`max`、`auto` |
| `IMAGE_SIZE` | 預設 `1536x1024`；`auto` 或 `寬x高`（邊長為 16 的倍數、最長 3840、長寬比不超過 3:1） |
| `IMAGE_TIMEOUT_SECS` | 每一張的時限，預設 `180`（官方說複雜的圖最多約 2 分鐘；逾時不重試，避免重複計費） |
| `IMAGE_CONCURRENCY` | 全站同時產圖的張數，預設 `2` |
| `IMAGE_MAX_PER_WORKSHEET` | 每份學習單自動產生的圖最多幾張，預設 `6`（其餘留空框，老師可按「重畫」或上傳） |
| `IMAGE_MAX_REGENERATIONS` | 每份學習單老師最多「重畫」幾次，預設 `10` |

設定有誤（例如 `IMAGE_QUALITY=ultra`、`IMAGE_SIZE=1000x1000`）只會關掉產圖並在日誌警告，不會讓後端起不來。金鑰不會出現在日誌。

## 教材檔案儲存（Phase 2.2）

教材原檔（PDF、DOCX、圖片、影音）存在 S3 相容的物件儲存（正式環境用 MinIO）；本機開發與測試存在本機資料夾。瀏覽器從不直接連到儲存空間，下載與預覽一律經過後端並檢查擁有者。

| 環境變數 | 說明 |
|---|---|
| `STORAGE_BACKEND` | `s3`、`fs` 或 `none`。後端程式沒設時是 `fs`；`docker-compose.yml` 預設 `s3`（同一份 compose 裡的 `minio` 服務）。`none` 時上傳回「還沒有設定檔案儲存空間」，貼上文字與網址照常可用。設定有誤（例如少了 `S3_BUCKET`）也只會關掉上傳，不會讓後端起不來 |
| `STORAGE_DIR` | `fs` 用的資料夾（預設 `./data/materials`；映像內是 `/app/data/materials`） |
| `S3_ENDPOINT` | compose 預設 `http://minio:9000`（同一份 compose 的內部網址；path-style） |
| `S3_BUCKET` | 例如 `manna-materials`。私有、**不要開版本保留（versioning）**，刪除才是真的刪除 |
| `S3_REGION` | 預設 `us-east-1`（MinIO 不在意，但簽章需要） |
| `S3_ACCESS_KEY`、`S3_SECRET_KEY` | compose 裡接 Coolify 自動產生的 `SERVICE_USER_MINIO`／`SERVICE_PASSWORD_MINIO`（MinIO 的帳密），不進版本庫、不用手打 |
| `PLATFORM_LIBRARY_BYTES` | 平台教材庫（教材參考與學習單範本，Phase 2.9）所有原檔合計的上限，單位 byte；預設 5 GB（`5368709120`）。教師個人的 1 GB 額度不包含平台教材 |

主機 nginx（`/etc/nginx/sites-available/manna-dls`）要允許大檔：`client_max_body_size 105m;`（nginx 預設 1 MB，超過時上傳會在 nginx 被擋成 413）。

後端啟動時會在日誌寫一行儲存方式，`s3` 時另外檢查 bucket 連得到（連不到只記錯誤，不會停機）。

本機試 S3 後端：`minio/minio` 映像已不在 Docker Hub，用 Coolify 自己的版本：

```bash
docker run -d --name manna-minio-test -p 127.0.0.1:59000:9000 \
  -e MINIO_ROOT_USER=manna-test-access -e MINIO_ROOT_PASSWORD=manna-test-secret-key \
  ghcr.io/coollabsio/minio server /data
cd api && S3_TEST_ENDPOINT=http://127.0.0.1:59000 S3_TEST_ACCESS_KEY=manna-test-access \
  S3_TEST_SECRET_KEY=manna-test-secret-key cargo test --test storage_s3
```

CI 只用 `fs`（`api/tests/materials.rs`）；`storage_s3` 測試沒有 `S3_TEST_ENDPOINT` 時自動略過。

## 部署

### 開發流程（一律走 PR）

1. 從 `main` 開分支開發，推上去開 PR。
2. PR 會跑 CI 與 Coolify 連線檢查；**全部通過才合併**。CI 只跑有變動的部分：改 `web/` 才跑前端、改 `api/` 才跑後端，只改文件則全部跳過；Docker 建置只在 PR 上跑。
3. 合併進 `main` 後，CI 再跑一次相關測試（不重建 Docker 映像），全過且這次有改到程式或部署設定，才把該 commit 快轉到 `deploy` 分支；只改文件不會重新部署。
4. Coolify（manna 主機）監看 **`deploy` 分支**，一有更新就自動部署。
5. CI 會等 Coolify 部署完成，並用 `coolify-deployments-richard` App 在 GitHub 建立 Deployment 紀錄（repo 首頁右側的 Deployments）。

不要直接推 `main` 或 `deploy`。私有 repo 在 GitHub 免費方案無法設分支保護，所以「CI 過了才合併」靠慣例；`deploy` 分支是保底：即使誤推 `main`，CI 沒過網站也不會更新。

Actions 設定（Settings → Secrets and variables → Actions）：

| 類型 | 名稱 | 說明 |
|---|---|---|
| secret | `COOLIFY_API_TOKEN` | Coolify API token（read 權限即可） |
| secret | `DEPLOY_APP_PRIVATE_KEY` | `coolify-deployments-richard` App 的私鑰 |
| variable | `COOLIFY_URL`、`COOLIFY_APP_UUIDS`、`DEPLOY_APP_ID`、`DEPLOY_ENV_NAME`、`DEPLOY_ENV_URL` | 已設定 |

沒有兩個 secret 時，部署照常進行，只是不會在 GitHub 留下 Deployment 紀錄。
- 主機 nginx 把 `manna.biobank.tw` 代理到 `127.0.0.1:8080`（設定檔 `/etc/nginx/sites-available/manna-dls`）。

金鑰（`ANTHROPIC_API_KEY` 等）只設在 Coolify 的環境變數，不進版本庫。

### `/api/health` 的 commit

`GET /api/health` 回 `{"ok", "db", "commit"}`，`commit` 是正在跑的映像是從哪個 commit 建的，用來確認部署真的上線了。來源依序是：

1. **建置時寫進映像的 `BUILD_COMMIT`**：`docker-compose.yml` 把 `SOURCE_COMMIT` 當 build argument 傳給 Dockerfile，Dockerfile 最後一層存成 `BUILD_COMMIT`（放最後一層，換 commit 不會讓其他層的快取失效）。
2. **執行時的 `SOURCE_COMMIT` 環境變數**（備援）。
3. 兩者都沒有（或不是 commit hash，例如空字串、`unknown`、`HEAD`）時才回 `"unknown"`。

Coolify 預設**不會**在建置時提供 `SOURCE_COMMIT`（它會讓 Docker 快取失效）。執行時也靠不住：compose 檔引用了 `${SOURCE_COMMIT}`，Coolify 就會在這個應用程式的環境變數清單自動建一個同名變數，而清單裡有 `SOURCE_COMMIT` 時 Coolify 不再填入自己的值。所以正式環境要在 Coolify 這個應用程式的設定（Advanced 頁）打開 **Include Source Commit in Build**；之後從 `deploy` 分支部署的映像就帶著 commit。清單裡自動出現的 `SOURCE_COMMIT` 保持空白，不要手填（填了會蓋過真正的值）。部署後可用 `curl -s https://manna.biobank.tw/api/health` 對照 `deploy` 分支最新的 commit。

本機 `docker build .` 不用帶參數（`commit` 會是 `unknown`）；要帶就用 `docker build --build-arg SOURCE_COMMIT=$(git rev-parse HEAD) .`。

### 教師長期記憶（#82）

「備課偏好 → 我的出題習慣」支援確認後記住、取代／停止使用與完整歷程。
研究介入開關預設關閉，由 PI 在後台決定是否讓習慣影響出題。
Postgres 是正本；可選擇另建以 `memory-sidecar/docker-compose.yml` 部署的 Coolify 資源，加入 Graphiti + FalkorDB，
不加索引也能使用全部確認習慣。部署、隱私限制、備份還原與驗證見
[教師長期記憶](docs/TEACHER_MEMORY.md)。

#97 起，Manna 也會在老師打的話（備課對話、逐區修改、學習單要求）裡偵測長期習慣，並在該訊息旁詢問
「要記住這個習慣嗎？」，老師按「記住」才會記；新增不含個人資訊的自由習慣，每位老師上限 100 項。
這是研究設計的變更，見 TEACHER_MEMORY.md 開頭與〈待 PI 核准〉。
