# manna-api

Rust backend for Manna DLS (Phase 1: AI-assisted lesson preparation). axum 0.8 + sqlx 0.8
(PostgreSQL 17 + pgvector). The API contract is `../docs/API.md`; the design is
`../docs/BLUEPRINT.md`.

One binary, `manna-api`: serves `/api/*` and the built frontend from `STATIC_DIR`
(any other non-`/api` path falls back to `index.html`). Migrations in `migrations/` are
embedded at compile time and run on startup.

## Run locally

```bash
# Postgres with pgvector (once)
docker run -d --name manna-pg -e POSTGRES_USER=manna -e POSTGRES_PASSWORD=manna \
  -e POSTGRES_DB=manna -p 127.0.0.1:55432:5432 pgvector/pgvector:pg17

export DATABASE_URL=postgres://manna:manna@127.0.0.1:55432/manna
export AI_PROVIDER=fake                 # deterministic offline AI; or none
export ADMIN_BOOTSTRAP_CODE=local-dev-pi
cargo run
curl localhost:8080/api/health          # {"db":true,"ok":true}
```

Log in as the PI with `POST /api/auth/login {"code":"local-dev-pi"}` (codes are
case-insensitive), then create teacher invites with `POST /api/admin/invites`.

## Test

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
DATABASE_URL=postgres://manna:manna@127.0.0.1:55432/manna cargo test
```

Integration tests (`tests/api.rs`) create a throwaway database `test_<random>` per test on the
`DATABASE_URL` server, run the migrations, and drop it afterwards (`WITH (FORCE)`), so they run
in parallel. The role needs `CREATEDB`, and the server needs the `vector` extension
(`pgvector/pgvector:pg17`). Without `DATABASE_URL` the integration tests are skipped, except
when `CI` is set, where they fail. Tests always use the fake AI provider; no network calls.

## Environment variables

| Variable | Default | Notes |
|---|---|---|
| `DATABASE_URL` | – (required) | |
| `PORT` | `8080` | listens on `0.0.0.0` |
| `STATIC_DIR` | `./web-dist` | built frontend; a missing directory only yields 404s |
| `AI_PROVIDER` | `none` | `anthropic` \| `openai_compatible` \| `fake` \| `none` (`none` → AI endpoints answer `status: "disabled"`) |
| `AI_MODEL` | `claude-opus-5-5` (anthropic), `fake-model` (fake) | required for `openai_compatible` |
| `AI_EFFORT` | `medium` | anthropic `output_config.effort`: `low`…`max` |
| `AI_TIMEOUT_SECS` | `60` | per provider call; a timeout pauses AI (status `timeout`), no retry on another model |
| `ANTHROPIC_API_KEY` | – | required for `anthropic` |
| `OPENAI_COMPAT_BASE_URL` | – | required for `openai_compatible` (NCHC); `…/v1`, we POST `{base}/chat/completions` |
| `OPENAI_COMPAT_API_KEY` | – | optional bearer token |
| `DEMO_MODE` | `false` | shown by `/api/config`; **required** for `anthropic` |
| `ADMIN_BOOTSTRAP_CODE` | – | on startup, creates a PI invite (`PI-001`, general) for this code if none exists |
| `RAG_TOP_K` | `8` | knowledge items retrieved per suggestion request |
| `RUST_LOG` | `info,tower_http=info` | tracing filter |

**Hard rule:** `AI_PROVIDER=anthropic` without `DEMO_MODE=true` refuses to start. The spec
forbids sending real teacher data to overseas APIs; Anthropic is for the demo environment only.

## Layout

| Path | What |
|---|---|
| `src/main.rs` | config → connect → migrate → bootstrap invite → serve |
| `src/lib.rs` | `AppState`, `build_app` (API router + SPA static files) |
| `src/config.rs` | env parsing and validation |
| `src/auth.rs` | invite-code login, bearer sessions (SHA-256 of codes/tokens only), PI check |
| `src/routes/` | `units`, `core` (CoRe + assist), `lessons` (versions, links, SIOP), `suggestions` (RAG), `feedback` (decisions, scaffold events, teacher cases), `admin` |
| `src/ai/` | provider abstraction: `anthropic` (raw HTTP), `openai` (compatible), `fake`; `schemas` (structured output); `pipeline` (log → call → check → regenerate once → blocked/shown/timeout/error) |
| `src/checks.rs` | automatic output checks (citations, track, language, 大考-level ↔ CEFR/grade heuristic, pasteable-answer heuristic, single question) |
| `src/retrieval.rs` | keyword retrieval (English words + CJK bigrams), track-filtered |
| `src/prompts.rs` | prompt versions from `prompt_versions`, `{{placeholder}}` rendering |
| `migrations/` | `0001_init.sql` schema, `0002_seed.sql` prompts v1, scaffolds, DEMO SIOP/knowledge items |

### Fake provider markers (tests)

Text in the request (e.g. a lesson `request_note` or a CoRe answer) containing `FAKE_TIMEOUT`,
`FAKE_ERROR`, `FAKE_REFUSAL`, `FAKE_BLOCK` (fails checks on both attempts), `FAKE_BLOCK_ONCE`
(fails once, regeneration passes) or `FAKE_KB_EMPTY` simulates that outcome.

### Seed content

`0002_seed.sql` knowledge items (`DEMO-*`) and SIOP features (「（示範）」) are placeholders,
not official sources. Replace them with the PI's material through
`POST /api/admin/knowledge/import`.
