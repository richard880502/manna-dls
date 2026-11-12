# Manna DLS

AI 輔助英文教師備課的研究型平台（國科會蔓娜計畫）。Phase 1 範圍：單元前 CoRe（含三層 AI 協助）與 Study & Plan（RAG 備課建議與教師判斷紀錄）。

- 設計藍圖：[docs/BLUEPRINT.md](docs/BLUEPRINT.md)
- API 合約：[docs/API.md](docs/API.md)

## 結構

```
web/        React 19 + TypeScript + Vite（前端）
api/        Rust（axum + sqlx）後端，migrations 在 api/migrations
Dockerfile  單一映像：前端靜態檔由後端提供
docker-compose.yml  Coolify 部署用（app + pgvector Postgres）
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

`main` 分支的 CI 全部通過後，`deploy` job 會呼叫 Coolify 的部署 webhook。需要在 GitHub repo 的 Secrets 設定：

- `COOLIFY_WEBHOOK_URL`：Coolify 應用程式 → Webhooks 頁的 Deploy Webhook 網址
- `COOLIFY_TOKEN`：Coolify → Keys & Tokens → API tokens 建立的權杖（需要 deploy 權限）

金鑰（`ANTHROPIC_API_KEY` 等）只設在 Coolify 的環境變數，不進版本庫。
