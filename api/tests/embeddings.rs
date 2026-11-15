//! Phase 2.4: the semantic index of the knowledge base and hybrid retrieval.
//! All rows are small synthetic fixtures and the embedder is the deterministic fake (or a
//! local stand-in for the HTTP provider); no real curriculum data or embedding model is used.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use manna_api::config::Config;
use manna_api::embedding::{
    fake_embedding, vector_literal, EmbeddingConfig, EmbeddingProviderKind,
};
use manna_api::retrieval::{self, CurriculumFilter, Method};
use manna_api::{kb_embeddings, AppState};
use serde_json::{json, Value};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const HEADER: &str = "item_code,category,school_track,title,content,source_doc,\
source_version,source_page,official_code,item_kind,strand,strand_name,stage,subject,advanced,\
recurring,source_sha256\n";

fn row(code: &str, track: &str, official: &str, stage: &str, subject: &str, text: &str) -> String {
    format!(
        "{code},curriculum,{track},{official} 學習表現｜測試類別,{text},測試領綱.pdf,測試版,1,\
         {official},learning_performance,3,測試類別,{stage},{subject},False,False,{SHA_A}"
    )
}

/// Synthetic curriculum rows (made-up texts).
fn curriculum_csv() -> String {
    let rows = [
        row(
            "T-G-READ",
            "general",
            "3-Ⅴ-2",
            "V",
            "英語文",
            "能讀懂測試用的短文並找出主旨。",
        ),
        row(
            "T-G-WRITE",
            "general",
            "4-Ⅴ-1",
            "V",
            "英語文",
            "能寫出測試用的段落。",
        ),
        row(
            "T-G-LISTEN",
            "general",
            "1-Ⅴ-1",
            "V",
            "英語文",
            "能聽懂測試用的廣播內容。",
        ),
        row(
            "T-G-JUNIOR",
            "general",
            "3-Ⅳ-1",
            "IV",
            "英語文",
            "能讀懂國中測試用的短文。",
        ),
        row(
            "T-V-READ",
            "vocational",
            "3-Ⅴ-6",
            "V",
            "英語文",
            "能讀懂技高測試用的短文。",
        ),
    ];
    format!("{HEADER}{}\n", rows.join("\n"))
}

/// The fixtures plus enough filler rows to need more than one embedding batch.
fn large_csv() -> String {
    let filler: Vec<String> = (1..=30)
        .map(|i| {
            row(
                &format!("T-G-FILL-{i:02}"),
                "general",
                &format!("9-Ⅴ-{i}"),
                "V",
                "英語文",
                &format!("測試用的填充條目第{i}條。"),
            )
        })
        .collect();
    format!("{}{}\n", curriculum_csv(), filler.join("\n"))
}

fn with_fake(c: &mut Config) {
    c.embedding = EmbeddingConfig::fake();
}

fn http_provider(base_url: String) -> EmbeddingConfig {
    EmbeddingConfig {
        provider: EmbeddingProviderKind::OpenAiCompatible,
        base_url: Some(base_url),
        api_key: Some("test-embedding-key".into()),
        model: "test-model".into(),
        dims: 16,
        query_timeout_ms: 300,
        batch_timeout_secs: 2,
        warning: None,
    }
}

async fn invite(app: &common::TestApp, research_id: &str, role: &str) -> String {
    let pi = app.pi_token().await;
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": research_id, "school_track": "general", "role": role }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.login(v["code"].as_str().unwrap()).await.0
}

async fn import(app: &common::TestApp, pi: &str, csv: &str) {
    let (s, v) = app
        .post_csv("/api/admin/knowledge/import", Some(pi), csv)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

const STATUS: &str = "/api/admin/knowledge/embeddings";
const REBUILD: &str = "/api/admin/knowledge/embeddings/rebuild";

/// Polls the status until no run is in progress and `done` holds.
async fn wait_for(app: &common::TestApp, pi: &str, done: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        let (s, v) = app.get(STATUS, Some(pi)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false && done(&v) {
            return v;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "indexing did not finish: {v}"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

fn fully_indexed(v: &Value) -> bool {
    v["embedded"] == v["active_items"]
}

async fn search(app: &common::TestApp, token: &str, query: &str) -> Value {
    let (s, v) = app
        .get(
            &format!("/api/knowledge/curriculum?q={}", urlencode(query)),
            Some(token),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| format!("%{b:02X}")).collect()
}

fn codes(v: &Value) -> Vec<&str> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["item"]["item_code"].as_str().unwrap())
        .collect()
}

/// Stores `vector` as the current fake-model embedding of the active item `code`.
async fn set_vector(app: &common::TestApp, code: &str, vector: &[f32]) {
    let done = sqlx::query(
        "UPDATE knowledge_embeddings e SET embedding = $2::vector
           FROM knowledge_items k
          WHERE k.id = e.knowledge_item_id AND k.item_code = $1 AND k.retired_at IS NULL",
    )
    .bind(code)
    .bind(vector_literal(vector))
    .execute(&app.db)
    .await
    .unwrap();
    assert_eq!(done.rows_affected(), 1, "{code}");
}

#[tokio::test]
async fn items_are_embedded_after_import_and_runs_are_idempotent() {
    let app = test_app!(with_fake);
    let pi = app.pi_token().await;

    let (_, before) = app.get(STATUS, Some(&pi)).await;
    assert_eq!(before["enabled"], true);
    assert_eq!(before["model"], "fake-embedding");
    assert_eq!(before["embedded"], 0);
    assert!(before["job"].is_null());

    // A dry run embeds nothing.
    let (s, _) = app
        .post_csv(
            "/api/admin/knowledge/import?dry_run=true",
            Some(&pi),
            &large_csv(),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = app.get(STATUS, Some(&pi)).await;
    assert!(v["job"].is_null() && v["running"] == false, "{v}");

    // The import returns at once; the index follows in the background and covers every
    // active item (the seeded demo items too), in more than one batch.
    import(&app, &pi, &large_csv()).await;
    let v = wait_for(&app, &pi, fully_indexed).await;
    let active = v["active_items"].as_i64().unwrap();
    assert!(
        active > kb_embeddings::BATCH as i64,
        "fixtures span batches"
    );
    assert_eq!(v["other_model"], 0);
    assert_eq!(v["job"]["trigger"], "import");
    assert_eq!(v["job"]["status"], "done");
    assert_eq!(v["job"]["total"], active);
    assert_eq!(v["job"]["embedded"], active);
    let (dims, hash): (i32, String) = sqlx::query_as(
        "SELECT e.dims, e.content_hash FROM knowledge_embeddings e
           JOIN knowledge_items k ON k.id = e.knowledge_item_id WHERE k.item_code = 'T-G-READ'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(dims, 64);
    assert_eq!(
        hash,
        kb_embeddings::content_hash("英語文｜學習表現｜測試類別：能讀懂測試用的短文並找出主旨。")
    );

    // Asking again finds nothing to do and rewrites nothing.
    let stamp = |db: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT MAX(created_at) FROM knowledge_embeddings",
        )
        .fetch_one(&db)
        .await
        .unwrap()
    };
    let first = stamp(app.db.clone()).await;
    let (s, v) = app.post(REBUILD, Some(&pi), json!({})).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["started"], true);
    let v = wait_for(&app, &pi, |v| v["job"]["trigger"] == "manual").await;
    assert_eq!(
        (&v["job"]["total"], &v["job"]["status"]),
        (&json!(0), &json!("done"))
    );
    assert_eq!(stamp(app.db.clone()).await, first);

    // An unchanged re-import does not start a run; a changed row embeds exactly that row and
    // drops the vector of the version it retires.
    import(&app, &pi, &large_csv()).await;
    let changed = large_csv().replace("能寫出測試用的段落。", "能寫出測試用的完整段落。");
    import(&app, &pi, &changed).await;
    let v = wait_for(&app, &pi, |v| {
        v["job"]["trigger"] == "import" && fully_indexed(v)
    })
    .await;
    assert_eq!(v["job"]["total"], 1);
    assert_eq!(v["active_items"], active);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_embeddings")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(
        rows, active,
        "one vector per active item, none for retired versions"
    );

    // force: everything again.
    let (s, _) = app.post(REBUILD, Some(&pi), json!({ "force": true })).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let v = wait_for(&app, &pi, |v| {
        v["job"]["trigger"] == "manual" && v["job"]["total"] == active
    })
    .await;
    assert_eq!(v["job"]["embedded"], active);
    assert!(stamp(app.db.clone()).await > first);

    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM usage_logs WHERE event = 'knowledge_embeddings_rebuild'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(events, 2);
}

#[tokio::test]
async fn a_changed_model_ignores_old_vectors_until_they_are_rebuilt() {
    let app = test_app!(with_fake);
    let pi = app.pi_token().await;
    import(&app, &pi, &curriculum_csv()).await;
    let v = wait_for(&app, &pi, fully_indexed).await;
    let active = v["active_items"].as_i64().unwrap();

    // The same database, now configured with another model of another size.
    let mut config = Config::for_tests("unused");
    config.embedding = EmbeddingConfig {
        model: "fake-embedding-b".into(),
        dims: 32,
        ..EmbeddingConfig::fake()
    };
    let other = AppState::new(app.db.clone(), config);
    let status = kb_embeddings::status(&other).await.unwrap();
    assert_eq!(status["embedded"], 0);
    assert_eq!(status["other_model"], active);

    // Old vectors are not compared with the new model's query: keyword-only, and no error.
    let filter = CurriculumFilter::default();
    let found = retrieval::retrieve_curriculum_hybrid(
        &other.db,
        &other.embedder,
        "general",
        &filter,
        "短文 主旨",
        10,
    )
    .await
    .unwrap();
    assert_eq!(found.trace.method, Method::Keyword);
    assert_eq!(found.trace.fallback_reason, Some("no_vectors"));
    assert_eq!(found.items[0].item.item_code, "T-G-READ");

    assert!(kb_embeddings::spawn_sync(&other, "startup", None, false));
    let start = Instant::now();
    loop {
        let s = kb_embeddings::status(&other).await.unwrap();
        if s["running"] == false && s["embedded"] == active {
            assert_eq!(s["other_model"], 0);
            assert_eq!(s["job"]["trigger"], "startup");
            assert_eq!(s["job"]["model"], "fake-embedding-b");
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{s}");
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let sizes: Vec<i32> = sqlx::query_scalar("SELECT DISTINCT dims FROM knowledge_embeddings")
        .fetch_all(&app.db)
        .await
        .unwrap();
    assert_eq!(sizes, [32]);
    let found = retrieval::retrieve_curriculum_hybrid(
        &other.db,
        &other.embedder,
        "general",
        &filter,
        "短文 主旨",
        10,
    )
    .await
    .unwrap();
    assert_eq!(found.trace.method, Method::Hybrid);
    assert_eq!(
        found.trace.embedding_model.as_deref(),
        Some("fake-embedding-b")
    );
    // The first app (old model) now sees no usable vector and falls back as well.
    let v = search(&app, &pi, "短文 主旨").await;
    assert_eq!(v["method"], "keyword");
}

#[tokio::test]
async fn hybrid_search_fuses_both_rankings_and_keeps_the_filters() {
    let app = test_app!(with_fake);
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-001", "general").await;
    import(&app, &pi, &curriculum_csv()).await;
    wait_for(&app, &pi, fully_indexed).await;

    // An English query shares no keyword with the Chinese items. Give it the meaning of
    // T-G-LISTEN by storing the query's own vector there, and put the same vector on an
    // out-of-scope item (junior high) and on another track's item.
    let query = "understanding announcements";
    let near = fake_embedding(query, 64);
    for code in ["T-G-LISTEN", "T-G-JUNIOR", "T-V-READ"] {
        set_vector(&app, code, &near).await;
    }
    let v = search(&app, &teacher, query).await;
    assert_eq!(v["method"], "hybrid");
    assert_eq!(v["ranked"], true);
    let got = codes(&v);
    assert_eq!(got[0], "T-G-LISTEN", "{got:?}");
    assert_eq!(v["items"][0]["score"], 0, "found by the embedding alone");
    assert!(v["items"][0]["similarity"].as_f64().unwrap() > 0.99);
    for out in ["T-G-JUNIOR", "T-V-READ"] {
        assert!(!got.contains(&out), "{out} escaped the filters: {got:?}");
    }
    // The stage filter still reaches the junior-high item, the track still decides.
    let (_, junior) = app
        .get(
            &format!("/api/knowledge/curriculum?stage=IV&q={}", urlencode(query)),
            Some(&teacher),
        )
        .await;
    assert_eq!(codes(&junior), ["T-G-JUNIOR"]);
    let (_, vocational) = app
        .get(
            &format!(
                "/api/knowledge/curriculum?track=vocational&q={}",
                urlencode(query)
            ),
            Some(&teacher),
        )
        .await;
    assert_eq!(codes(&vocational), ["T-V-READ"]);

    // A Chinese query: T-G-READ is the keyword match and also near the query, so it leads;
    // the item only the embedding likes (T-G-LISTEN, given this query's vector) comes next,
    // ahead of the item with neither.
    let query = "短文 主旨";
    set_vector(&app, "T-G-LISTEN", &fake_embedding(query, 64)).await;
    let v = search(&app, &teacher, query).await;
    let got = codes(&v);
    assert_eq!(got, ["T-G-READ", "T-G-LISTEN", "T-G-WRITE"], "{got:?}");
    assert!(v["items"][0]["score"].as_i64().unwrap() > 0);
    assert_eq!(v["items"][1]["score"], 0);

    // Without a query the items are listed in code order, as before.
    let (_, listed) = app.get("/api/knowledge/curriculum", Some(&teacher)).await;
    assert_eq!(listed["method"], "keyword");
    assert_eq!(listed["ranked"], false);
    assert_eq!(codes(&listed), ["T-G-LISTEN", "T-G-READ", "T-G-WRITE"]);
}

async fn lesson_with_plan(app: &common::TestApp, t: &str, title: &str, objective: &str) -> i64 {
    let unit = app.create_unit(t, "Unit RAG").await;
    let lid = app.create_lesson(t, unit, title).await;
    let (s, v) = app
        .post(
            &format!("/api/lessons/{lid}/versions"),
            Some(t),
            json!({ "objective": objective, "steps": [
                { "position": 1, "activity_type": "全班講解", "icap_level": "passive",
                  "content": "skim the passage" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    lid
}

#[tokio::test]
async fn plan_suggestions_log_the_method_and_the_candidates() {
    let app = test_app!(with_fake);
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-001", "general").await;
    import(&app, &pi, &curriculum_csv()).await;
    wait_for(&app, &pi, fully_indexed).await;

    let lid = lesson_with_plan(
        &app,
        &teacher,
        "Inversion: Not only … but also",
        "學生能用 Not only … but also 倒裝句型介紹自己的家鄉",
    )
    .await;
    let (s, r) = app
        .post(
            &format!("/api/lessons/{lid}/suggestions"),
            Some(&teacher),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown", "{r}");
    let iid = r["interaction_id"].as_i64().unwrap();
    let (codes, method, model, reason, candidates): (
        Vec<String>,
        String,
        Option<String>,
        Option<String>,
        Value,
    ) = sqlx::query_as(
        "SELECT item_codes, method, embedding_model, fallback_reason, candidates
           FROM retrieval_logs WHERE ai_interaction_id = $1",
    )
    .bind(iid)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(method, "hybrid");
    assert_eq!(model.as_deref(), Some("fake-embedding"));
    assert_eq!(reason, None);
    assert!(codes.contains(&"DEMO-GRM-001".to_string()), "{codes:?}");
    assert_eq!(codes.len(), 8, "top_k");
    let candidates = candidates.as_array().unwrap();
    let logged: Vec<&str> = candidates
        .iter()
        .map(|c| c["item_code"].as_str().unwrap())
        .collect();
    assert_eq!(logged, codes, "candidates are the returned items, in order");
    let grammar = candidates
        .iter()
        .find(|c| c["item_code"] == "DEMO-GRM-001")
        .unwrap();
    assert!(grammar["keyword_rank"].is_u64() && grammar["vector_rank"].is_u64());
    assert!(grammar["similarity"].as_f64().unwrap() > 0.0);
    assert!(grammar["fused"].as_f64().unwrap() > 0.0);
    // The interaction snapshot says how the items were found.
    let snapshot: Value =
        sqlx::query_scalar("SELECT input_snapshot FROM ai_interactions WHERE id = $1")
            .bind(iid)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(snapshot["retrieval"]["method"], "hybrid");
}

#[tokio::test]
async fn without_a_provider_everything_is_keyword_only() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-001", "general").await;
    import(&app, &pi, &curriculum_csv()).await;

    let (s, v) = app.get(STATUS, Some(&pi)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["enabled"], false);
    assert_eq!(v["provider"], "none");
    assert!(v["model"].is_null() && v["job"].is_null());
    assert_eq!(v["embedded"], 0);
    assert_eq!(v["running"], false);
    let (s, v) = app.post(REBUILD, Some(&pi), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_embeddings")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    let v = search(&app, &teacher, "短文 主旨").await;
    assert_eq!(v["method"], "keyword");
    assert_eq!(codes(&v), ["T-G-READ"]);
    assert!(v["items"][0].get("similarity").is_none());
    let v = search(&app, &teacher, "understanding announcements").await;
    assert!(codes(&v).is_empty());

    let lid = lesson_with_plan(&app, &teacher, "Inversion", "學生能用倒裝句型介紹家鄉").await;
    let (s, r) = app
        .post(
            &format!("/api/lessons/{lid}/suggestions"),
            Some(&teacher),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (method, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT method, fallback_reason FROM retrieval_logs WHERE ai_interaction_id = $1",
    )
    .bind(r["interaction_id"].as_i64().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((method.as_str(), reason), ("keyword", None));
}

#[tokio::test]
async fn a_failing_provider_never_breaks_imports_or_retrieval() {
    // Nothing listens on this port: every embedding request is refused.
    let app = test_app!(|c: &mut Config| {
        c.embedding = http_provider("http://127.0.0.1:9/v1".into());
    });
    let pi = app.pi_token().await;
    let teacher = app.teacher("T-001", "general").await;

    import(&app, &pi, &curriculum_csv()).await;
    let v = wait_for(&app, &pi, |v| v["job"]["status"] == "failed").await;
    assert_eq!(v["embedded"], 0);
    assert_eq!(v["job"]["embedded"], 0);
    let error = v["job"]["error"].as_str().unwrap();
    assert!(error.contains("embedding request failed"), "{error}");
    assert!(!v.to_string().contains("test-embedding-key"), "{v}");

    // Search and suggestions answer from keywords.
    let v = search(&app, &teacher, "短文 主旨").await;
    assert_eq!(v["method"], "keyword");
    assert_eq!(codes(&v), ["T-G-READ"]);
    let lid = lesson_with_plan(&app, &teacher, "Inversion", "學生能用倒裝句型介紹家鄉").await;
    let (s, r) = app
        .post(
            &format!("/api/lessons/{lid}/suggestions"),
            Some(&teacher),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "shown", "{r}");
    let (method, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT method, fallback_reason FROM retrieval_logs WHERE ai_interaction_id = $1",
    )
    .bind(r["interaction_id"].as_i64().unwrap())
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(method, "keyword");
    assert_eq!(reason.as_deref(), Some("embedding_failed"));
}

#[tokio::test]
async fn a_slow_provider_times_out_into_keyword_retrieval() {
    // Accepts connections and never answers.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let app = test_app!(|c: &mut Config| {
        c.embedding = http_provider(format!("http://{addr}/v1"));
    });
    let pi = app.pi_token().await;
    // Rows inserted directly: no import, so no indexing run competes for the provider.
    sqlx::query(
        "INSERT INTO knowledge_items (item_code, category, school_track, title, content,
                                      official_code, item_kind, stage, subject)
         VALUES ('T-G-READ', 'curriculum', 'general', '3-Ⅴ-2 測試', '能讀懂測試用的短文並找出主旨。',
                 '3-Ⅴ-2', 'learning_performance', 'V', '英語文')",
    )
    .execute(&app.db)
    .await
    .unwrap();

    let start = Instant::now();
    let v = search(&app, &pi, "短文 主旨").await;
    let took = start.elapsed();
    assert_eq!(v["method"], "keyword");
    assert_eq!(codes(&v), ["T-G-READ"]);
    assert!(
        took >= Duration::from_millis(250) && took < Duration::from_secs(3),
        "waited {took:?} for a 300 ms budget"
    );
    let state = AppState::new(app.db.clone(), {
        let mut c = Config::for_tests("unused");
        c.embedding = http_provider(format!("http://{addr}/v1"));
        c
    });
    let found = retrieval::retrieve_hybrid(&state.db, &state.embedder, "general", "短文 主旨", 8)
        .await
        .unwrap();
    assert_eq!(found.trace.fallback_reason, Some("embedding_timeout"));
    assert!(!found.items.is_empty());
}

#[tokio::test]
async fn the_http_provider_sends_batches_with_the_key_and_reads_vectors_by_index() {
    use axum::http::HeaderMap;
    use axum::routing::post;
    use axum::{Json, Router};

    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let stub = Router::new().route(
        "/v1/embeddings",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                if headers.get("authorization").and_then(|h| h.to_str().ok())
                    != Some("Bearer test-embedding-key")
                    || body["model"] != "test-model"
                {
                    return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "no" })));
                }
                let inputs = body["input"].as_array().unwrap();
                assert!(inputs.len() <= kb_embeddings::BATCH);
                // Answer in reverse order: the client must follow `index`.
                let data: Vec<Value> = inputs
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, t)| {
                        json!({ "index": i, "object": "embedding",
                                "embedding": fake_embedding(t.as_str().unwrap(), 16) })
                    })
                    .collect();
                (
                    StatusCode::OK,
                    Json(json!({ "model": "test-model", "data": data })),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, stub).await });

    let app = test_app!(|c: &mut Config| {
        c.embedding = http_provider(format!("http://{addr}/v1/"));
    });
    let pi = app.pi_token().await;
    import(&app, &pi, &large_csv()).await;
    let v = wait_for(&app, &pi, fully_indexed).await;
    assert_eq!(v["job"]["status"], "done", "{v}");
    assert_eq!(
        (&v["model"], &v["dims"]),
        (&json!("test-model"), &json!(16))
    );
    let active = v["active_items"].as_u64().unwrap() as usize;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        active.div_ceil(kb_embeddings::BATCH)
    );

    // Each item got the vector of its own text.
    let stored: String = sqlx::query_scalar(
        "SELECT e.embedding::text FROM knowledge_embeddings e
           JOIN knowledge_items k ON k.id = e.knowledge_item_id WHERE k.item_code = 'T-G-READ'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    let expected = fake_embedding(
        "英語文｜學習表現｜測試類別：能讀懂測試用的短文並找出主旨。",
        16,
    );
    let stored: Vec<f32> = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored.len(), 16);
    for (a, b) in stored.iter().zip(&expected) {
        assert!((a - b).abs() < 1e-5, "{stored:?} vs {expected:?}");
    }

    // One query, embedded once for two identical searches (per-query cache).
    let before = calls.load(Ordering::SeqCst);
    for _ in 0..2 {
        let v = search(&app, &pi, "短文 主旨").await;
        assert_eq!(v["method"], "hybrid");
        assert_eq!(codes(&v)[0], "T-G-READ");
    }
    assert_eq!(calls.load(Ordering::SeqCst), before + 1);
}

#[tokio::test]
async fn only_knowledge_managers_see_or_rebuild_the_index() {
    let app = test_app!(with_fake);
    let pi = app.pi_token().await;
    let manager = invite(&app, "KB-001", "kb_manager").await;
    let teacher = app.teacher("T-001", "general").await;
    let engineer = invite(&app, "ENG-001", "engineer").await;

    for (token, read, write) in [
        (Some(&pi), StatusCode::OK, StatusCode::ACCEPTED),
        (Some(&manager), StatusCode::OK, StatusCode::ACCEPTED),
        (Some(&teacher), StatusCode::FORBIDDEN, StatusCode::FORBIDDEN),
        (
            Some(&engineer),
            StatusCode::FORBIDDEN,
            StatusCode::FORBIDDEN,
        ),
        (None, StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED),
    ] {
        let token = token.map(String::as_str);
        let (s, v) = app.get(STATUS, token).await;
        assert_eq!(s, read, "{v}");
        // No body at all is accepted as `{}`.
        let (s, v) = app
            .send(axum::http::Method::POST, REBUILD, token, None, Vec::new())
            .await;
        assert_eq!(s, write, "{v}");
        if write == StatusCode::ACCEPTED {
            assert_eq!(v["running"], true);
            wait_for(&app, &pi, fully_indexed).await;
        }
    }
    let (s, v) = app
        .send(
            axum::http::Method::POST,
            REBUILD,
            Some(&pi),
            Some("application/json"),
            b"{\"force\": \"yes\"}".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    wait_for(&app, &pi, fully_indexed).await;
}

#[tokio::test]
async fn sop_objectives_rank_curriculum_items_only_when_they_do_not_all_fit() {
    use manna_api::curriculum;

    let app = test_app!(with_fake);
    let pi = app.pi_token().await;
    import(&app, &pi, &curriculum_csv()).await;
    wait_for(&app, &pi, fully_indexed).await;
    let state = AppState::new(app.db.clone(), {
        let mut c = Config::for_tests("unused");
        with_fake(&mut c);
        c
    });
    let query = "Unit 3 Airport announcements";
    set_vector(&app, "T-G-WRITE", &fake_embedding(query, 64)).await;

    // Everything fits: all three in-scope items, in code order, no ranking and no embedding.
    let (items, trace) =
        curriculum::items_for_track_hybrid(&state.db, &state.embedder, "general", query, 150)
            .await
            .unwrap();
    let got: Vec<&str> = items.iter().map(|i| i.item_code.as_str()).collect();
    assert_eq!(got, ["T-G-LISTEN", "T-G-READ", "T-G-WRITE"]);
    assert_eq!(trace.method, Method::All);
    assert_eq!(trace.candidates.len(), 3);

    // More items than the limit: the nearest first (an English query has no keyword match).
    let (items, trace) =
        curriculum::items_for_track_hybrid(&state.db, &state.embedder, "general", query, 2)
            .await
            .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].item_code, "T-G-WRITE");
    assert_eq!(trace.method, Method::Hybrid);
    // The keyword-only variant can only fill up in code order and misses it.
    let keyword = curriculum::items_for_track(&state.db, "general", query, 2)
        .await
        .unwrap();
    let got: Vec<&str> = keyword.iter().map(|i| i.item_code.as_str()).collect();
    assert_eq!(got, ["T-G-LISTEN", "T-G-READ"]);

    // Embeddings off: same call, keyword ranking, then filled up in code order.
    let off = AppState::new(app.db.clone(), Config::for_tests("unused"));
    let (items, trace) =
        curriculum::items_for_track_hybrid(&off.db, &off.embedder, "general", "測試用的段落", 2)
            .await
            .unwrap();
    assert_eq!(trace.method, Method::Keyword);
    assert_eq!(items[0].item_code, "T-G-WRITE");
    assert_eq!(items.len(), 2);
}
