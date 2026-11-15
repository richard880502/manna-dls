//! Phase 2.0: knowledge base (curriculum items), vocabulary list, the kb_manager role.
//! All rows are small synthetic fixtures; no real curriculum or CEEC data is used.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

const CURRICULUM_HEADER: &str = "item_code,category,school_track,title,content,source_doc,\
source_version,source_page,official_code,item_kind,strand,strand_name,stage,subject,advanced,\
recurring,source_sha256\n";

/// Synthetic curriculum rows (texts are made up, shaped like the real ones).
fn curriculum_csv() -> String {
    let rows = [
        format!("T-G-3-V-2,curriculum,general,◎3-Ⅴ-2 學習表現｜測試閱讀,能讀懂測試用的短文並找出主旨。,測試領綱.pdf,測試版,12,◎3-Ⅴ-2,learning_performance,3,測試閱讀,V,英語文,False,True,{SHA_A}"),
        format!("T-G-3-V-10,curriculum,general,*3-Ⅴ-10 學習表現｜測試閱讀,能分析測試用短文的結構。,測試領綱.pdf,測試版,13,*3-Ⅴ-10,learning_performance,3,測試閱讀,V,英語文,True,False,{SHA_A}"),
        format!("T-G-Ae-V-1,curriculum,general,Ae-Ⅴ-1 學習內容｜測試篇章,測試用的短文與故事。,測試領綱.pdf,測試版,20,Ae-Ⅴ-1,learning_content,Ae,測試篇章,V,英語文,False,False,{SHA_A}"),
        format!("T-G-3-IV-1,curriculum,general,3-Ⅳ-1 學習表現｜測試閱讀,能讀懂國中測試用的短文。,測試領綱.pdf,測試版,11,3-Ⅳ-1,learning_performance,3,測試閱讀,IV,英語文,False,False,{SHA_A}"),
        format!("T-C-3-V-2,curriculum,comprehensive,3-Ⅴ-2 學習表現｜測試閱讀,能讀懂綜高測試用的短文。,測試綜高領綱.pdf,測試版,9,3-Ⅴ-2,learning_performance,3,測試閱讀,V,英語文,False,False,{SHA_B}"),
        format!("T-V-JP-1,curriculum,vocational,外語-技-日語I-A-a 學習內容｜測試日語,測試用的日語短文閱讀。,測試群綱.pdf,測試版,30,外語-技-日語I-A-a,learning_content,日語I,測試日語,V,日語,False,False,{SHA_B}"),
    ];
    format!("{CURRICULUM_HEADER}{}\n", rows.join("\n"))
}

const VOCABULARY_HEADER: &str =
    "word,headword,level,pos,kind,appendix_category,list_version,source_doc,source_page,source_sha256\n";

/// Synthetic word list (levels are made up).
fn vocabulary_csv() -> String {
    let rows = [
        "a,a/an,1,art.,base,",
        "an,a/an,1,art.,variant,",
        "the,the,1,art.,base,",
        "she,she,1,pron.,base,",
        "study,study,1,v./n.,base,",
        "in,in,1,prep.,base,",
        "class,class,1,n.,base,",
        "room,room,1,n.,base,",
        "agree,agree(ment),2,v.,base,",
        "agreement,agree(ment),2,n.,derived,",
        "conserve,conserve,6,v.,base,",
        "capital,capital,2,n./adj.,base,",
        "capital,capital(ism),4,n.,base,",
        "Monday,Monday,,,appendix,Days of the Week",
    ];
    let body: Vec<String> = rows
        .iter()
        .map(|r| format!("{r},TEST LIST v1,測試詞彙表.pdf,5,{SHA_A}"))
        .collect();
    format!("{VOCABULARY_HEADER}{}\n", body.join("\n"))
}

async fn invite(app: &common::TestApp, research_id: &str, track: &str, role: &str) -> String {
    let pi = app.pi_token().await;
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": research_id, "school_track": track, "role": role }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    app.login(v["code"].as_str().unwrap()).await.0
}

fn codes(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            i.get("item")
                .unwrap_or(i)
                .get("item_code")
                .and_then(Value::as_str)
                .unwrap()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn roles_and_permissions() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let manager = invite(&app, "KB-001", "general", "kb_manager").await;
    let teacher = app.teacher("T-001", "general").await;

    let (_, me) = app.get("/api/me", Some(&manager)).await;
    assert_eq!(me["role"], "kb_manager");
    assert_eq!(me["capabilities"], json!(["manage_knowledge"]));
    let (_, me) = app.get("/api/me", Some(&pi)).await;
    assert_eq!(
        me["capabilities"],
        json!(["administer", "manage_knowledge"])
    );
    let (_, me) = app.get("/api/me", Some(&teacher)).await;
    assert_eq!(me["capabilities"], json!([]));

    // Knowledge management: PI and kb_manager yes, teacher 403, anonymous 401.
    let csv = curriculum_csv();
    for (token, expect) in [
        (Some(&pi), StatusCode::OK),
        (Some(&manager), StatusCode::OK),
        (Some(&teacher), StatusCode::FORBIDDEN),
        (None, StatusCode::UNAUTHORIZED),
    ] {
        let token = token.map(String::as_str);
        let (s, v) = app
            .post_csv("/api/admin/knowledge/import?dry_run=true", token, &csv)
            .await;
        assert_eq!(s, expect, "knowledge import: {v}");
        let (s, _) = app
            .post_csv(
                "/api/admin/vocabulary/import?dry_run=true",
                token,
                &vocabulary_csv(),
            )
            .await;
        assert_eq!(s, expect, "vocabulary import");
        let (s, _) = app
            .send(
                axum::http::Method::POST,
                "/api/admin/knowledge/retire-demo?dry_run=true",
                token,
                None,
                Vec::new(),
            )
            .await;
        assert_eq!(s, expect, "retire-demo");
        for path in [
            "/api/admin/knowledge/summary",
            "/api/admin/knowledge/items",
            "/api/admin/vocabulary/items",
        ] {
            let (s, _) = app.get(path, token).await;
            assert_eq!(s, expect, "{path}");
        }
    }

    // Everything else under /api/admin stays PI-only: kb_manager gets 403.
    for (path, body) in [
        (
            "/api/admin/invites",
            json!({ "research_id": "X", "school_track": "general" }),
        ),
        (
            "/api/admin/prompts",
            json!({ "prompt_key": "plan_suggest", "system_text": "s", "user_template": "u" }),
        ),
    ] {
        let (s, v) = app.post(path, Some(&manager), body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(v["error"]["code"], "forbidden");
    }
    for path in ["/api/admin/prompts", "/api/admin/settings"] {
        let (s, _) = app.get(path, Some(&manager)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }
    let (s, _) = app
        .put(
            "/api/admin/settings",
            Some(&manager),
            json!({ "route_choice_enabled": false }),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app
        .post_csv(
            "/api/admin/teacher-cases/import",
            Some(&manager),
            "case_code,school_track,grade,topic,summary,details,approved\n",
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // A kb_manager sees no other teacher's data.
    let unit = app.create_unit(&teacher, "老師的單元").await;
    let (s, _) = app.get(&format!("/api/units/{unit}"), Some(&manager)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, units) = app.get("/api/units", Some(&manager)).await;
    assert_eq!(units, json!([]));

    // An unknown role cannot be invited; a comprehensive-school teacher can.
    let (s, _) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": "X", "school_track": "general", "role": "admin" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let comprehensive = app.teacher("T-C-001", "comprehensive").await;
    let (_, me) = app.get("/api/me", Some(&comprehensive)).await;
    assert_eq!(me["school_track"], "comprehensive");
}

#[tokio::test]
async fn curriculum_import_is_idempotent_versioned_and_logged() {
    let app = test_app!();
    let manager = invite(&app, "KB-001", "general", "kb_manager").await;
    let csv = curriculum_csv();

    // Dry run: counts only, nothing written.
    let (s, v) = app
        .post_csv(
            "/api/admin/knowledge/import?dry_run=true",
            Some(&manager),
            &csv,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["dry_run"], true);
    assert_eq!(
        (&v["rows"], &v["inserted"], &v["updated"], &v["unchanged"]),
        (&json!(6), &json!(6), &json!(0), &json!(0))
    );
    assert_eq!(v["imported"], 0);
    assert_eq!(
        v["by_track"],
        json!({ "comprehensive": 1, "general": 4, "vocational": 1 })
    );
    assert_eq!(v["batch_id"], Value::Null);
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_items WHERE item_code LIKE 'T-%'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 0, "a dry run writes nothing");

    // Commit.
    let (s, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&manager), &csv)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((&v["inserted"], &v["imported"]), (&json!(6), &json!(6)));
    let batch = v["batch_id"].as_i64().expect("batch id");
    let row: (
        String,
        String,
        String,
        String,
        bool,
        bool,
        String,
        String,
        i64,
        String,
    ) = sqlx::query_as(
        "SELECT official_code, item_kind, stage, subject, advanced, recurring, source_sha256,
                    source_page, import_batch_id, approved_by
               FROM knowledge_items WHERE item_code = 'T-G-3-V-2' AND retired_at IS NULL",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "◎3-Ⅴ-2".into(),
            "learning_performance".into(),
            "V".into(),
            "英語文".into(),
            false,
            true,
            SHA_A.into(),
            "12".into(),
            batch,
            "KB-001".into()
        )
    );

    // Re-import: all unchanged, no new versions, no new batch.
    let (_, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&manager), &csv)
        .await;
    assert_eq!(
        (
            &v["inserted"],
            &v["updated"],
            &v["unchanged"],
            &v["batch_id"]
        ),
        (&json!(0), &json!(0), &json!(6), &Value::Null)
    );
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_items WHERE item_code LIKE 'T-%'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(n, 6);

    // One changed row: new version, old one retired (not deleted), the rest untouched.
    let changed = csv.replace("並找出主旨。", "並找出主旨與細節。");
    let (_, v) = app
        .post_csv(
            "/api/admin/knowledge/import?dry_run=true",
            Some(&manager),
            &changed,
        )
        .await;
    assert_eq!(
        (&v["inserted"], &v["updated"], &v["unchanged"]),
        (&json!(0), &json!(1), &json!(5))
    );
    let (_, v) = app
        .post_csv("/api/admin/knowledge/import", Some(&manager), &changed)
        .await;
    assert_eq!(v["updated"], 1);
    let versions: Vec<(i32, bool)> = sqlx::query_as(
        "SELECT version, retired_at IS NULL FROM knowledge_items
          WHERE item_code = 'T-G-3-V-2' ORDER BY version",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(versions, vec![(1, false), (2, true)]);

    // Validation errors name the row.
    for (bad, needle) in [
        (csv.replace("learning_content", "content"), "item_kind"),
        (csv.replace(SHA_B, "xyz"), "source_sha256"),
        (csv.replace(",comprehensive,", ",college,"), "school_track"),
        (csv.replace(",True,", ",maybe,"), "第 2 列"),
    ] {
        let (s, v) = app
            .post_csv("/api/admin/knowledge/import", Some(&manager), &bad)
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{needle}");
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(msg.contains(needle), "{needle}: {msg}");
    }

    // Audit trail: who, counts, SHA-256 (the dry runs are not logged).
    let logs: Vec<(String, Value)> = sqlx::query_as(
        "SELECT t.research_id, l.payload FROM usage_logs l JOIN teachers t ON t.id = l.teacher_id
          WHERE l.event = 'knowledge_imported' ORDER BY l.id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(logs.len(), 3);
    assert!(logs.iter().all(|(who, _)| who == "KB-001"));
    assert_eq!(logs[0].1["inserted"], 6);
    assert_eq!(logs[0].1["sha256"], manna_api::auth::sha256_hex(&csv));
    assert_eq!(logs[1].1["unchanged"], 6);
    assert_eq!(logs[2].1["updated"], 1);

    // Summary and browse.
    let (_, summary) = app
        .get("/api/admin/knowledge/summary", Some(&manager))
        .await;
    assert_eq!(summary["knowledge"]["real_active"], 6);
    assert_eq!(summary["knowledge"]["demo_active"], 16);
    assert_eq!(summary["recent_batches"].as_array().unwrap().len(), 2);
    assert_eq!(summary["recent_batches"][0]["imported_by"], "KB-001");
    let (_, list) = app
        .get(
            "/api/admin/knowledge/items?category=curriculum&q=%E7%9F%AD%E6%96%87",
            Some(&manager),
        )
        .await;
    assert_eq!(list["total"], 6);
    let (_, list) = app
        .get(
            "/api/admin/knowledge/items?status=retired&q=T-G-3-V-2",
            Some(&manager),
        )
        .await;
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["version"], 1);
    assert!(list["items"][0]["retired_at"].is_string());
    let (_, list) = app
        .get(
            "/api/admin/knowledge/items?track=comprehensive&limit=1",
            Some(&manager),
        )
        .await;
    assert_eq!(codes(&list), vec!["T-C-3-V-2"]);
    let (s, _) = app
        .get("/api/admin/knowledge/items?status=gone", Some(&manager))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn demo_items_are_retired_only_on_request() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let retire = |dry: bool| {
        let app = &app;
        let pi = pi.clone();
        async move {
            app.send(
                axum::http::Method::POST,
                &format!("/api/admin/knowledge/retire-demo?dry_run={dry}"),
                Some(&pi),
                None,
                Vec::new(),
            )
            .await
        }
    };
    let active_demo = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge_items
              WHERE item_code LIKE 'DEMO-%' AND retired_at IS NULL",
        )
        .fetch_one(&app.db)
        .await
        .unwrap()
    };

    // Refused while the knowledge base has nothing else.
    let (s, v) = retire(false).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(active_demo().await, 16);

    // Importing real items does not retire the demo items by itself.
    let (s, _) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), &curriculum_csv())
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(active_demo().await, 16);

    let (s, v) = retire(true).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((&v["demo_active"], &v["retired"]), (&json!(16), &json!(0)));
    assert_eq!(v["real_active"], 6);
    // the real items are all curriculum: these demo categories would be left without items
    assert_eq!(
        v["categories_left_empty"],
        json!(["grammar", "siop", "vocab"])
    );
    assert_eq!(active_demo().await, 16);

    let (s, v) = retire(false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["retired"], 16);
    assert_eq!(active_demo().await, 0);
    let kept: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_items WHERE item_code LIKE 'DEMO-%'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(kept, 16, "retired, not deleted");
    let logged: Value =
        sqlx::query_scalar("SELECT payload FROM usage_logs WHERE event = 'demo_knowledge_retired'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(logged["retired"], 16);
    // again: nothing left to retire
    let (_, v) = retire(false).await;
    assert_eq!(v["retired"], 0);
}

#[tokio::test]
async fn curriculum_search_and_exact_code_lookup() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, _) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), &curriculum_csv())
        .await;
    assert_eq!(s, StatusCode::OK);
    let general = app.teacher("T-001", "general").await;
    let comprehensive = app.teacher("T-002", "comprehensive").await;
    let vocational = app.teacher("T-003", "vocational").await;

    // Keyword search within the teacher's track; default scope = senior high, English subjects.
    let (s, v) = app
        .get(
            "/api/knowledge/curriculum?q=%E7%9F%AD%E6%96%87%E4%B8%BB%E6%97%A8",
            Some(&general),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["track"], "general");
    assert_eq!(v["ranked"], true);
    let got = codes(&v);
    assert_eq!(got[0], "T-G-3-V-2", "{got:?}");
    assert!(got.contains(&"T-G-Ae-V-1".to_string()));
    for out in ["T-G-3-IV-1", "T-C-3-V-2", "T-V-JP-1", "DEMO-CUR-001"] {
        assert!(!got.contains(&out.to_string()), "{out} in {got:?}");
    }
    let item = &v["items"][0]["item"];
    assert_eq!(item["official_code"], "◎3-Ⅴ-2");
    assert_eq!(item["source_doc"], "測試領綱.pdf");
    assert_eq!(item["source_page"], "12");
    assert_eq!(item["recurring"], true);

    // Filters: kind, stage; without q the list is in code order.
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum?kind=learning_content",
            Some(&general),
        )
        .await;
    assert_eq!(codes(&v), vec!["T-G-Ae-V-1"]);
    assert_eq!(v["ranked"], false);
    let (_, v) = app
        .get("/api/knowledge/curriculum?stage=IV", Some(&general))
        .await;
    assert_eq!(codes(&v), vec!["T-G-3-IV-1"]);
    let (_, v) = app
        .get("/api/knowledge/curriculum?stage=all", Some(&general))
        .await;
    assert_eq!(
        codes(&v),
        vec!["T-G-3-IV-1", "T-G-3-V-2", "T-G-3-V-10", "T-G-Ae-V-1"]
    );

    // Comprehensive teachers get the comprehensive 領綱; vocational Japanese needs subject=all.
    let (_, v) = app
        .get("/api/knowledge/curriculum", Some(&comprehensive))
        .await;
    assert_eq!(codes(&v), vec!["T-C-3-V-2"]);
    let (_, v) = app
        .get("/api/knowledge/curriculum", Some(&vocational))
        .await;
    assert!(codes(&v).is_empty());
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum?subject=%E6%97%A5%E8%AA%9E",
            Some(&vocational),
        )
        .await;
    assert_eq!(codes(&v), vec!["T-V-JP-1"]);
    // a teacher may look at another school type explicitly
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum?track=comprehensive",
            Some(&general),
        )
        .await;
    assert_eq!(codes(&v), vec!["T-C-3-V-2"]);
    for bad in ["track=college", "kind=other"] {
        let (s, _) = app
            .get(&format!("/api/knowledge/curriculum?{bad}"), Some(&general))
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (s, _) = app.get("/api/knowledge/curriculum", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Exact codes: the official text comes from the database; unknown codes are reported.
    let (s, v) = app
        .get(
            "/api/knowledge/curriculum/by-code?codes=3-V-2,T-G-3-V-10,9-V-99",
            Some(&general),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(codes(&v), vec!["T-G-3-V-2", "T-G-3-V-10"]);
    assert_eq!(v["items"][0]["content"], "能讀懂測試用的短文並找出主旨。");
    assert_eq!(v["items"][0]["source_version"], "測試版");
    assert_eq!(v["not_found"], json!(["9-V-99"]));
    // the same official code resolves per track
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum/by-code?codes=3-%E2%85%A4-2",
            Some(&comprehensive),
        )
        .await;
    assert_eq!(codes(&v), vec!["T-C-3-V-2"]);
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum/by-code?codes=3-V-2&track=all",
            Some(&vocational),
        )
        .await;
    assert_eq!(codes(&v), vec!["T-C-3-V-2", "T-G-3-V-2"]);
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum/by-code?codes=3-V-2",
            Some(&vocational),
        )
        .await;
    assert!(codes(&v).is_empty());
    assert_eq!(v["not_found"], json!(["3-V-2"]));
    let (s, _) = app
        .get("/api/knowledge/curriculum/by-code?codes=,", Some(&general))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Retired items are no longer found.
    sqlx::query("UPDATE knowledge_items SET retired_at = now() WHERE item_code = 'T-G-3-V-10'")
        .execute(&app.db)
        .await
        .unwrap();
    let (_, v) = app
        .get(
            "/api/knowledge/curriculum/by-code?codes=T-G-3-V-10",
            Some(&general),
        )
        .await;
    assert_eq!(v["not_found"], json!(["T-G-3-V-10"]));
}

#[tokio::test]
async fn vocabulary_import_and_analysis() {
    let app = test_app!();
    let manager = invite(&app, "KB-001", "general", "kb_manager").await;
    let teacher = app.teacher("T-001", "vocational").await;
    let text =
        "She studies in the classroom. An agreement in Taipei on Monday: conserve the capital.";

    // Nothing imported yet: the teacher is told, no level is made up.
    let (s, v) = app
        .post(
            "/api/vocabulary/analyze",
            Some(&teacher),
            json!({ "text": text }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("尚未匯入"));

    let csv = vocabulary_csv();
    let (s, v) = app
        .post_csv(
            "/api/admin/vocabulary/import?dry_run=true",
            Some(&manager),
            &csv,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (&v["rows"], &v["inserted"], &v["dry_run"]),
        (&json!(14), &json!(14), &json!(true))
    );
    assert_eq!(
        v["by_level"],
        json!({ "1": 8, "2": 3, "4": 1, "6": 1, "appendix": 1 })
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vocabulary_items")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(n, 0);

    let (s, v) = app
        .post_csv("/api/admin/vocabulary/import", Some(&manager), &csv)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["inserted"], 14);
    assert_eq!(v["sha256"], manna_api::auth::sha256_hex(&csv));
    let (_, v) = app
        .post_csv("/api/admin/vocabulary/import", Some(&manager), &csv)
        .await;
    assert_eq!(
        (&v["inserted"], &v["updated"], &v["unchanged"]),
        (&json!(0), &json!(0), &json!(14))
    );

    // A corrected level is a new version of that word form.
    let changed = csv.replace("conserve,conserve,6,", "conserve,conserve,5,");
    let (_, v) = app
        .post_csv("/api/admin/vocabulary/import", Some(&manager), &changed)
        .await;
    assert_eq!((&v["updated"], &v["unchanged"]), (&json!(1), &json!(13)));
    let rows: Vec<(i32, Option<i16>, bool)> = sqlx::query_as(
        "SELECT version, level, retired_at IS NULL FROM vocabulary_items
          WHERE word = 'conserve' ORDER BY version",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows, vec![(1, Some(6), false), (2, Some(5), true)]);

    for (bad, needle) in [
        (
            csv.replace("conserve,conserve,6,", "conserve,conserve,7,"),
            "level",
        ),
        (csv.replace("Monday,Monday,,", "Monday,Monday,3,"), "level"),
        (csv.replace(",variant,", ",other,"), "kind"),
        (
            format!("{csv}a,a/an,1,art.,base,,TEST LIST v1,x,1,\n"),
            "重複",
        ),
        ("word,level\na,1\n".to_string(), "缺少欄位"),
    ] {
        let (s, v) = app
            .post_csv("/api/admin/vocabulary/import", Some(&manager), &bad)
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{needle}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains(needle),
            "{needle}: {v}"
        );
    }

    // Browse (managers) and summary.
    let (_, list) = app
        .get("/api/admin/vocabulary/items?q=agree", Some(&manager))
        .await;
    assert_eq!(list["total"], 2);
    assert_eq!(list["items"][1]["headword"], "agree(ment)");
    let (_, list) = app
        .get("/api/admin/vocabulary/items?level=appendix", Some(&manager))
        .await;
    assert_eq!(list["items"][0]["word"], "Monday");
    let (_, summary) = app
        .get("/api/admin/knowledge/summary", Some(&manager))
        .await;
    assert_eq!(summary["vocabulary"]["active_forms"], 14);

    // Analysis (any signed-in account).
    let (s, a) = app
        .post(
            "/api/vocabulary/analyze",
            Some(&teacher),
            json!({ "text": text, "target_level": 4 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["wordlist_version"], "TEST LIST v1");
    // she studies in the | an agreement in | conserve the capital  (on: unknown)
    assert_eq!(a["basis"]["counted_tokens"], 10);
    assert_eq!(
        a["basis"]["excluded_tokens"],
        json!({ "proper_nouns": 1, "unknown": 1, "multiword": 1, "appendix": 1 })
    );
    assert_eq!(a["above_target"][0]["base"], "conserve");
    assert_eq!(
        a["above_target"][0]["level"], 5,
        "the corrected level is used"
    );
    assert_eq!(a["above_target"][0]["positions"][0]["line"], 1);
    assert_eq!(a["proper_nouns"][0]["word"], "Taipei");
    assert_eq!(a["appendix_words"][0]["word"], "Monday");
    assert_eq!(a["multiword"][0]["word"], "classroom");
    assert_eq!(a["unknown"][0]["word"], "on");
    assert_eq!(a["multi_level_words"][0]["levels"], json!([2, 4]));
    assert!(a["disclaimer"]
        .as_str()
        .unwrap()
        .contains("不是大考中心的官方評定"));

    // target_level defaults to 4 and is validated; the text is required and never stored.
    let (_, a) = app
        .post(
            "/api/vocabulary/analyze",
            Some(&teacher),
            json!({ "text": text, "target_level": 6 }),
        )
        .await;
    assert_eq!(a["above_target"], json!([]));
    for body in [
        json!({ "text": "  " }),
        json!({ "text": text, "target_level": 0 }),
        json!({ "text": text, "target_level": 7 }),
    ] {
        let (s, _) = app
            .post("/api/vocabulary/analyze", Some(&teacher), body)
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let (s, _) = app
        .post(
            "/api/vocabulary/analyze",
            Some(&teacher),
            json!({ "text": text, "list_version": "no such list" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = app
        .post("/api/vocabulary/analyze", None, json!({ "text": text }))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let logged: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'vocabulary_analyzed' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(logged.len(), 2);
    assert!(
        !logged[0].to_string().contains("classroom"),
        "only counts are logged"
    );
}

#[tokio::test]
async fn plan_suggestions_stay_inside_the_default_curriculum_scope() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, _) = app
        .post_csv("/api/admin/knowledge/import", Some(&pi), &curriculum_csv())
        .await;
    assert_eq!(s, StatusCode::OK);
    for (track, expect, never) in [
        (
            "general",
            "T-G-3-V-2",
            vec!["T-G-3-IV-1", "T-C-3-V-2", "T-V-JP-1"],
        ),
        ("comprehensive", "T-C-3-V-2", vec!["T-G-3-V-2", "T-V-JP-1"]),
    ] {
        let found = manna_api::retrieval::retrieve(&app.db, track, "測試用的短文 閱讀", 20)
            .await
            .unwrap();
        let got: Vec<&str> = found.iter().map(|s| s.item.item_code.as_str()).collect();
        assert!(got.contains(&expect), "{track}: {got:?}");
        for n in never {
            assert!(!got.contains(&n), "{track}: {n} in {got:?}");
        }
    }
    // the vocational teacher's only curriculum fixture is Japanese: out of the default scope
    let found = manna_api::retrieval::retrieve(&app.db, "vocational", "測試用的日語短文", 20)
        .await
        .unwrap();
    assert!(found.iter().all(|s| s.item.item_code != "T-V-JP-1"));
}
