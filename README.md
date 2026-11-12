# Manna DLS

AI 輔助英文教師備課的研究型平台（國科會蔓娜計畫）。Phase 1 範圍：單元前 CoRe（含三層 AI 協助）與 Study & Plan（RAG 備課建議與教師判斷紀錄）；Phase 1.2 加上 Teach & Record（課中書籤、手機遙控、離線同步）與課後補寫。

- 設計藍圖：[docs/BLUEPRINT.md](docs/BLUEPRINT.md)
- API 合約：[docs/API.md](docs/API.md)

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

用 `ADMIN_BOOTSTRAP_CODE` 的值登入就是主持人帳號，可在後台建立教師邀請碼、匯入知識庫。

## AI 供應商

| `AI_PROVIDER` | 用途 |
|---|---|
| `none` | 預設。AI 功能停用，所有流程可手動完成 |
| `fake` | 測試與 CI，固定輸出、不連網 |
| `anthropic` | 示範環境。必須同時設 `DEMO_MODE=true`，否則後端拒絕啟動（規格禁止把真實教師資料送境外） |
| `openai_compatible` | 國網中心模型（拿到 API 後設定 `OPENAI_COMPAT_BASE_URL`、`OPENAI_COMPAT_API_KEY`） |

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
