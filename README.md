# Manna DLS

AI 輔助英文教師備課的研究型平台（國科會蔓娜計畫）。Phase 1 範圍：單元前 CoRe（含三層 AI 協助）與 Study & Plan（RAG 備課建議與教師判斷紀錄）；Phase 1.2 加上 Teach & Record（課中書籤、手機遙控、離線同步）與課後補寫。

- 設計藍圖：[docs/BLUEPRINT.md](docs/BLUEPRINT.md)
- API 合約：[docs/API.md](docs/API.md)
- 教材與注記的工程回覆（規格 §12）：[docs/ENGINEERING_REPLY.md](docs/ENGINEERING_REPLY.md)

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

用 `ADMIN_BOOTSTRAP_CODE` 的值登入就是主持人帳號，可在後台建立教師邀請碼，並在「知識庫」的「課綱條目與詞彙」分頁匯入課綱條目與詞彙表（資料不在 repo，匯入檔的產生方式見 `docs/API.md`「Phase 2.0：知識庫與詞彙」）。

## AI 供應商

| `AI_PROVIDER` | 用途 |
|---|---|
| `none` | 預設。AI 功能停用，所有流程可手動完成 |
| `fake` | 測試與 CI，固定輸出、不連網 |
| `anthropic` | 示範環境。必須同時設 `DEMO_MODE=true`，否則後端拒絕啟動（規格禁止把真實教師資料送境外） |
| `openai_compatible` | 國網中心模型（拿到 API 後設定 `OPENAI_COMPAT_BASE_URL`、`OPENAI_COMPAT_API_KEY`） |

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

掃描的 PDF、手機拍的課本頁面與文字層壞掉的 PDF，上傳後會在背景用國網中心的 `chandra-ocr-2` 逐頁辨識文字（細節見 `docs/API.md`「Phase 2.10」）。預設關閉；關閉時這些檔案照舊保存，文字狀態維持「讀不到文字」。PDF 轉圖用 `pdftoppm`（Docker 映像已安裝 `poppler-utils`；本機開發要自己裝，例如 `brew install poppler`）。

| 變數 | 說明 |
|---|---|
| `OCR_PROVIDER` | `openai_compatible`（正式環境）、`fake`（測試；`AI_PROVIDER=fake` 且沒設這個變數時的預設）或 `none`（預設） |
| `OCR_BASE_URL`、`OCR_API_KEY` | 沒設時沿用 `EMBEDDING_BASE_URL`、`EMBEDDING_API_KEY`（同一個國網中心閘道；金鑰只設在 Coolify） |
| `OCR_MODEL` | 預設 `chandra-ocr-2` |
| `OCR_TIMEOUT_SECS` | 每一頁請求的時限，預設 `120` |
| `OCR_MAX_PAGES` | 每個檔案最多辨識幾頁，預設 `40`（其餘頁略過並在教材上註明） |
| `OCR_CONCURRENCY` | 全站同時辨識的頁數，預設 `2` |

設定有誤（例如 `OCR_PROVIDER=openai_compatible` 卻沒有任何閘道網址）只會關掉 OCR 並在日誌警告，不會讓後端起不來。

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
