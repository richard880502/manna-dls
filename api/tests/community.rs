//! 社群 phase 1 (docs/COMMUNITY.md): the PI switch, sharing, explore, preview, save, remix, and
//! what is never exposed.
#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};

const P: [&str; 3] = [
    "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”",
    "Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.",
    "Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.",
];

/// Strings that only exist in the owner's private things: none may ever reach another teacher.
const PRIVATE_REQUEST: &str = "SECRET-REQUEST-ZEBRA 請多出推論題";
const PRIVATE_NOTE: &str = "SECRET-SOURCE-NOTE-OTTER";
const PRIVATE_LESSON: &str = "SECRET-LESSON-TITLE-HERON";
const PRIVATE_UNIT: &str = "SECRET-UNIT-TITLE-FALCON";
const PRIVATE_CLASS: &str = "SECRET-CLASS-NAME-BADGER";
const PRIVATE_SEED_TASK: &str = "SECRET-SEED-TASK-LYNX";

async fn wait(app: &common::TestApp, t: &str, id: i64) {
    for _ in 0..800 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish generating");
}

/// The owner's worksheet, full of private links, generated.
async fn private_worksheet(app: &common::TestApp, t: &str) -> i64 {
    let unit = app.create_unit(t, PRIVATE_UNIT).await;
    let lesson = app.create_lesson(t, unit, PRIVATE_LESSON).await;
    let (s, class) = app
        .post(
            "/api/classes",
            Some(t),
            json!({"name": PRIVATE_CLASS, "class_profile": ""}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{class}");
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({
                "lesson_id": lesson, "class_id": class["id"],
                "material": {"title": "The Umbrella Stand", "text": P.join("\n\n"),
                             "source_note": PRIVATE_NOTE},
                "request": PRIVATE_REQUEST,
            }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    // The teacher's own seed idea (the brief) stays hers.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(t),
            json!({"brief": {"tasks": [], "warmup": [PRIVATE_SEED_TASK], "vocabulary": [],
                             "exit_ticket": []}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    wait(app, t, id).await;
    id
}

async fn simple_worksheet(app: &common::TestApp, t: &str, title: &str) -> i64 {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({"material": {"title": title, "text": P.join("\n\n")}}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    wait(app, t, id).await;
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(t),
            json!({"title": title}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    id
}

async fn switch(app: &common::TestApp, on: bool) {
    let pi = app.pi_token().await;
    let (s, v) = app
        .put(
            "/api/admin/community-settings",
            Some(&pi),
            json!({"enabled": on}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

async fn share(app: &common::TestApp, t: &str, id: i64, name: &str, remix: bool) {
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}/share"),
            Some(t),
            json!({"shared": true, "name": name, "allow_remix": remix}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["shared"], true);
}

async fn unshare(app: &common::TestApp, t: &str, id: i64) {
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}/share"),
            Some(t),
            json!({"shared": false}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["shared"], false);
}

async fn explore(app: &common::TestApp, t: &str, query: &str) -> Value {
    let (s, v) = app
        .get(&format!("/api/community/worksheets{query}"), Some(t))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

fn ids(page: &Value) -> Vec<i64> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect()
}

async fn count(app: &common::TestApp, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&app.db).await.unwrap()
}

// ---------------------------------------------------------------- the switch

#[tokio::test]
async fn the_switch_is_off_by_default_and_only_a_pi_changes_it() {
    let app = test_app!();
    let owner = app.teacher("T-OWNER", "general").await;
    let (s, config) = app.get("/api/config", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(config["community_enabled"], false);

    let (s, _) = app
        .put(
            "/api/admin/community-settings",
            Some(&owner),
            json!({"enabled": true}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.get("/api/admin/community-settings", Some(&owner)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app
        .put(
            "/api/admin/community-settings",
            None,
            json!({"enabled": true}),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let pi = app.pi_token().await;
    let (s, v) = app.get("/api/admin/community-settings", Some(&pi)).await;
    assert_eq!((s, &v["enabled"]), (StatusCode::OK, &json!(false)));

    switch(&app, true).await;
    let (_, config) = app.get("/api/config", None).await;
    assert_eq!(config["community_enabled"], true);
    switch(&app, false).await;
    let (_, config) = app.get("/api/config", None).await;
    assert_eq!(config["community_enabled"], false);

    // Each change is logged: key, from, to; ids only.
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT payload FROM usage_logs WHERE event = 'setting_changed'
          AND payload ->> 'key' = 'community_enabled' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    let rows: Vec<Value> = rows.into_iter().map(|r| r.0).collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        (&rows[0]["from"], &rows[0]["to"]),
        (&Value::Null, &json!(true))
    );
    assert_eq!(
        (&rows[1]["from"], &rows[1]["to"]),
        (&json!(true), &json!(false))
    );
}

#[tokio::test]
async fn with_the_switch_off_every_endpoint_refuses_and_nothing_is_written() {
    let app = test_app!();
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let ws = simple_worksheet(&app, &owner, "Off Sheet").await;

    // Sharing is refused: nothing becomes shared behind the teacher's back.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws}/share"),
            Some(&owner),
            json!({"shared": true, "name": "code"}),
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM worksheets WHERE shared_at IS NOT NULL"
        )
        .await,
        0
    );

    for path in [
        "/api/community/worksheets".to_string(),
        "/api/community/saves".to_string(),
        "/api/community/mine".to_string(),
        format!("/api/community/worksheets/{ws}"),
    ] {
        let (s, v) = app.get(&path, Some(&other)).await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{path} {v}");
        assert!(v.get("items").is_none(), "{path} {v}");
    }
    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{ws}/save"),
            Some(&other),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    let (s, _) = app
        .delete(
            &format!("/api/community/worksheets/{ws}/save"),
            Some(&other),
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&other),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM worksheet_saves").await, 0);
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM usage_logs WHERE event LIKE 'community_%'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn switching_off_hides_what_was_shared_and_on_again_brings_it_back() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let ws = simple_worksheet(&app, &owner, "Comes And Goes").await;
    share(&app, &owner, ws, "code", true).await;
    assert_eq!(ids(&explore(&app, &other, "").await), vec![ws]);

    switch(&app, false).await;
    let (s, _) = app.get("/api/community/worksheets", Some(&other)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    // The worksheet itself still opens for its owner, and says nothing about sharing.
    let (s, v) = app
        .get(&format!("/api/worksheets/{ws}"), Some(&owner))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["remixed_from"], Value::Null);

    switch(&app, true).await;
    assert_eq!(ids(&explore(&app, &other, "").await), vec![ws]);
}

// ---------------------------------------------------------------- sharing

#[tokio::test]
async fn only_the_owner_shares_and_a_new_worksheet_is_private() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let ws = simple_worksheet(&app, &owner, "Mine Only").await;

    // Private by default.
    assert!(ids(&explore(&app, &other, "").await).is_empty());
    assert!(ids(&explore(&app, &owner, "").await).is_empty());

    // Somebody else's worksheet is a 404, and nothing changes.
    for body in [
        json!({"shared": true, "name": "code"}),
        json!({"shared": false}),
    ] {
        let (s, _) = app
            .put(&format!("/api/worksheets/{ws}/share"), Some(&other), body)
            .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM worksheets WHERE shared_at IS NOT NULL"
        )
        .await,
        0
    );
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{ws}/share"),
            None,
            json!({"shared": false}),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // The name is a choice the teacher makes: no default.
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws}/share"),
            Some(&owner),
            json!({"shared": true}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{ws}/share"),
            Some(&owner),
            json!({"shared": true, "name": "T-OWNER"}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A worksheet without content cannot be shared.
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(&owner),
            json!({"material": {"title": "x", "text": P.join("\n\n")}}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let empty = v["id"].as_i64().unwrap();
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{empty}/share"),
            Some(&owner),
            json!({"shared": true, "name": "code"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Share, then change the options without resending everything.
    share(&app, &owner, ws, "anonymous", true).await;
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{ws}/share"),
            Some(&owner),
            json!({"shared": true, "allow_remix": false}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (&v["name"], &v["allow_remix"]),
        (&json!("anonymous"), &json!(false))
    );
}

#[tokio::test]
async fn unsharing_takes_effect_at_once_everywhere() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let ws = simple_worksheet(&app, &owner, "Withdrawn Soon").await;
    share(&app, &owner, ws, "code", true).await;
    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{ws}/save"),
            Some(&other),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ids(&explore(&app, &other, "").await), vec![ws]);

    unshare(&app, &owner, ws).await;
    assert!(ids(&explore(&app, &other, "").await).is_empty());
    let (_, saved) = app.get("/api/community/saves", Some(&other)).await;
    assert!(ids(&saved).is_empty());
    assert_eq!(saved["unavailable"], 1);
    for (method, path) in [
        ("GET", format!("/api/community/worksheets/{ws}")),
        ("POST", format!("/api/community/worksheets/{ws}/save")),
        ("POST", format!("/api/community/worksheets/{ws}/remix")),
    ] {
        let (s, v) = if method == "GET" {
            app.get(&path, Some(&other)).await
        } else {
            app.post(&path, Some(&other), json!({})).await
        };
        assert_eq!(s, StatusCode::NOT_FOUND, "{method} {path} {v}");
    }
    // Withdrawing twice is harmless and logged once.
    unshare(&app, &owner, ws).await;
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM usage_logs WHERE event = 'community_unshare'"
        )
        .await,
        1
    );

    // Sharing it again brings back the earlier save.
    share(&app, &owner, ws, "code", true).await;
    let (_, saved) = app.get("/api/community/saves", Some(&other)).await;
    assert_eq!(ids(&saved), vec![ws]);
}

#[tokio::test]
async fn a_deleted_worksheet_disappears() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let ws = simple_worksheet(&app, &owner, "Deleted Soon").await;
    share(&app, &owner, ws, "code", true).await;
    app.post(
        &format!("/api/community/worksheets/{ws}/save"),
        Some(&other),
        json!({}),
    )
    .await;
    let (s, _) = app
        .delete(&format!("/api/worksheets/{ws}"), Some(&owner))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(ids(&explore(&app, &other, "").await).is_empty());
    let (_, saved) = app.get("/api/community/saves", Some(&other)).await;
    assert!(ids(&saved).is_empty());
    let (s, _) = app
        .get(&format!("/api/community/worksheets/{ws}"), Some(&other))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, mine) = app.get("/api/community/mine", Some(&owner)).await;
    assert!(ids(&mine).is_empty());
}

// ---------------------------------------------------------------- explore

#[tokio::test]
async fn explore_lists_shared_worksheets_with_search_sort_and_counts() {
    let app = test_app!();
    switch(&app, true).await;
    let a = app.teacher("T-AAA", "general").await;
    let b = app.teacher("T-BBB", "general").await;
    let c = app.teacher("T-CCC", "general").await;
    let old = simple_worksheet(&app, &a, "Old Rain Story").await;
    let mid = simple_worksheet(&app, &a, "Middle 50%_Story").await;
    let new = simple_worksheet(&app, &b, "Newest Story").await;
    let hidden = simple_worksheet(&app, &a, "Never Shared").await;
    share(&app, &a, old, "code", true).await;
    tokio::time::sleep(Duration::from_millis(15)).await;
    share(&app, &a, mid, "anonymous", true).await;
    tokio::time::sleep(Duration::from_millis(15)).await;
    share(&app, &b, new, "code", false).await;

    let all = explore(&app, &c, "").await;
    assert_eq!(
        ids(&all),
        vec![new, mid, old],
        "newest first, {hidden} absent"
    );
    assert_eq!(all["total"], 3);
    let first = &all["items"][0];
    for key in [
        "id",
        "title",
        "owner_name",
        "shared_at",
        "saves",
        "remixes",
        "is_mine",
        "saved",
        "allow_remix",
        "output",
        "levels",
        "cover",
    ] {
        assert!(first.get(key).is_some(), "missing {key}: {first}");
    }
    assert_eq!(first["owner_name"], "T-BBB");
    assert_eq!(first["allow_remix"], false);
    assert_eq!(first["is_mine"], false);
    assert!(first["cover"]["blocks"]
        .as_array()
        .is_some_and(|b| !b.is_empty()));
    assert_eq!(all["items"][1]["owner_name"], "一位老師");
    // The caller's own shared worksheets are in the list, marked.
    let mine_view = explore(&app, &b, "").await;
    assert_eq!(mine_view["items"][0]["is_mine"], true);

    // Search by title; wildcards in the search are text.
    assert_eq!(ids(&explore(&app, &c, "?q=rain").await), vec![old]);
    assert_eq!(ids(&explore(&app, &c, "?q=50%25_").await), vec![mid]);
    assert_eq!(
        ids(&explore(&app, &c, "?q=%25").await),
        vec![mid],
        "only the literal %"
    );
    // Filters the data supports.
    assert_eq!(explore(&app, &c, "?output=worksheet").await["total"], 3);
    assert_eq!(explore(&app, &c, "?output=tiered").await["total"], 0);
    let (s, _) = app
        .get("/api/community/worksheets?output=nope", Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .get("/api/community/worksheets?sort=nope", Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .get("/api/community/worksheets?level=nope", Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let level = all["items"][0]["levels"][0].as_str().unwrap().to_string();
    assert_eq!(
        explore(&app, &c, &format!("?level={level}")).await["total"],
        3
    );

    // Saved and remixed counts and their sorts.
    for t in [&b, &c] {
        let (s, _) = app
            .post(
                &format!("/api/community/worksheets/{old}/save"),
                Some(t),
                json!({}),
            )
            .await;
        assert_eq!(s, StatusCode::OK);
    }
    app.post(
        &format!("/api/community/worksheets/{mid}/save"),
        Some(&c),
        json!({}),
    )
    .await;
    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{mid}/remix"),
            Some(&c),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        ids(&explore(&app, &c, "?sort=saved").await)[..2],
        [old, mid]
    );
    assert_eq!(ids(&explore(&app, &c, "?sort=remixed").await)[0], mid);
    let by_saved = explore(&app, &c, "?sort=saved").await;
    assert_eq!(
        (
            &by_saved["items"][0]["saves"],
            &by_saved["items"][0]["saved"]
        ),
        (&json!(2), &json!(true))
    );
    let by_remix = explore(&app, &c, "?sort=remixed").await;
    assert_eq!(by_remix["items"][0]["remixes"], 1);
    // Paging.
    let page = explore(&app, &c, "?limit=2&offset=2").await;
    assert_eq!(
        (
            page["total"].as_i64(),
            page["items"].as_array().unwrap().len()
        ),
        (Some(3), 1)
    );
}

// ---------------------------------------------------------------- privacy

/// Every string that is only the owner's: the research id of an anonymous owner, their ids and
/// everything typed into their private worksheet.
fn assert_clean(body: &Value, extra: &[&str]) {
    assert_no_private_text(body, extra);
    let text = body.to_string();
    for needle in [
        "teacher_id",
        "research_id",
        "lesson_id",
        "class_id",
        "prep_session",
        "interaction",
        "material_id",
    ] {
        assert!(!text.contains(needle), "{needle} leaked: {text}");
    }
}

/// The things the owner typed, anywhere in a response.
fn assert_no_private_text(body: &Value, extra: &[&str]) {
    let text = body.to_string();
    for needle in [
        PRIVATE_REQUEST,
        PRIVATE_NOTE,
        PRIVATE_LESSON,
        PRIVATE_UNIT,
        PRIVATE_CLASS,
        PRIVATE_SEED_TASK,
    ]
    .iter()
    .chain(extra.iter())
    {
        assert!(!text.contains(needle), "{needle} leaked: {text:.300}");
    }
}

#[tokio::test]
async fn an_anonymous_owner_is_never_named_and_private_material_never_leaves() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-ANON-OWNER", "general").await;
    let reader = app.teacher("T-READER", "general").await;
    let ws = private_worksheet(&app, &owner).await;
    share(&app, &owner, ws, "anonymous", true).await;
    let owner_id: i64 =
        sqlx::query_scalar("SELECT id FROM teachers WHERE research_id = 'T-ANON-OWNER'")
            .fetch_one(&app.db)
            .await
            .unwrap();

    let list = explore(&app, &reader, "").await;
    assert_eq!(list["items"][0]["owner_name"], "一位老師");
    assert_clean(&list, &["T-ANON-OWNER", &format!("\"{owner_id}\"")]);

    let (s, preview) = app
        .get(&format!("/api/community/worksheets/{ws}"), Some(&reader))
        .await;
    assert_eq!(s, StatusCode::OK, "{preview}");
    assert_eq!(preview["owner_name"], "一位老師");
    assert!(preview["sheet"]["blocks"]
        .as_array()
        .is_some_and(|b| !b.is_empty()));
    assert_clean(&preview, &["T-ANON-OWNER"]);
    // The material is not handed over as text: only what a sheet prints of it.
    for key in ["\"material\"", "\"paragraphs\"", "paragraph_seq"] {
        assert!(!preview.to_string().contains(key), "{key}");
    }
    assert!(
        !preview
            .to_string()
            .contains("They were sure that every umbrella"),
        "an unprinted passage sentence"
    );
    // The student sheet carries no answers, notes or evidence.
    let text = preview["sheet"].to_string();
    for key in ["\"answer\"", "teacher_notes", "\"evidence\"", "\"scoring\""] {
        assert!(!text.contains(key), "{key} in {text}");
    }

    // The same through every other door: saved list, the owner's view of their shares.
    app.post(
        &format!("/api/community/worksheets/{ws}/save"),
        Some(&reader),
        json!({}),
    )
    .await;
    let (_, saved) = app.get("/api/community/saves", Some(&reader)).await;
    assert_clean(&saved, &["T-ANON-OWNER"]);

    // With a code the research id is shown, and still nothing else.
    share(&app, &owner, ws, "code", true).await;
    let list = explore(&app, &reader, "").await;
    assert_eq!(list["items"][0]["owner_name"], "T-ANON-OWNER");
    assert_clean(&list, &[&format!("\"{owner_id}\"")]);
}

#[tokio::test]
async fn the_remix_is_a_private_copy_with_none_of_the_owners_links() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-REMIX-OWNER", "general").await;
    let me = app.teacher("T-REMIXER", "general").await;
    let ws = private_worksheet(&app, &owner).await;

    // Not shared: nothing to remix. Shared without remix: refused.
    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&me),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    share(&app, &owner, ws, "anonymous", false).await;
    let (s, v) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&me),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM worksheets WHERE remixed_from IS NOT NULL"
        )
        .await,
        0
    );

    // The owner remixing their own is refused too.
    share(&app, &owner, ws, "anonymous", true).await;
    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&owner),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, v) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&me),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let copy = v["worksheet_id"].as_i64().unwrap();
    assert_ne!(copy, ws);

    // The copy is mine and opens in my own editor; the owner cannot reach it.
    let (s, state) = app.get(&format!("/api/worksheets/{copy}"), Some(&me)).await;
    assert_eq!(s, StatusCode::OK, "{state}");
    let (s, _) = app
        .get(&format!("/api/worksheets/{copy}"), Some(&owner))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(state["generated"], true);
    assert!(!state["content"]["blocks"].as_array().unwrap().is_empty());
    assert_eq!(
        state["content"],
        app.get(&format!("/api/worksheets/{ws}"), Some(&owner))
            .await
            .1["content"]
    );
    // 「改編自」: the title and how the owner chose to be shown, nothing more.
    assert_eq!(state["remixed_from"]["id"], ws);
    assert_eq!(state["remixed_from"]["owner_name"], "一位老師");
    assert!(!state["remixed_from"].to_string().contains("T-REMIX-OWNER"));
    // None of the owner's private things came along.
    assert_no_private_text(&state, &["T-REMIX-OWNER"]);
    assert_eq!(state["lesson"], Value::Null);
    assert_eq!(state["class_id"], Value::Null);
    assert_eq!(state["request"], "");
    assert_eq!(state["material"]["source"]["note"], "");
    assert_eq!(state["template"], Value::Null);
    // It is private: not shared, not in the explore list, and has its own history.
    let row: (bool, Option<i64>, i64) = sqlx::query_as(
        "SELECT shared_at IS NOT NULL, lesson_id, teacher_id FROM worksheets WHERE id = $1",
    )
    .bind(copy)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!((row.0, row.1), (false, None));
    let owner_id: i64 =
        sqlx::query_scalar("SELECT id FROM teachers WHERE research_id = 'T-REMIXER'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(row.2, owner_id);
    assert_eq!(ids(&explore(&app, &me, "").await), vec![ws]);
    let versions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM worksheet_versions WHERE worksheet_id = $1")
            .bind(copy)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(versions, 1);
    // The owner's brief is not in the copy's row.
    let brief: Value = sqlx::query_scalar("SELECT brief FROM worksheets WHERE id = $1")
        .bind(copy)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(!brief.to_string().contains(PRIVATE_SEED_TASK), "{brief}");

    // Editing the copy changes nothing of the original.
    let before = app
        .get(&format!("/api/worksheets/{ws}"), Some(&owner))
        .await
        .1;
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{copy}"),
            Some(&me),
            json!({"title": "My Remix"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let after = app
        .get(&format!("/api/worksheets/{ws}"), Some(&owner))
        .await
        .1;
    assert_eq!(before["title"], after["title"]);

    // The original counts its copies and shows no list of them.
    let list = explore(&app, &me, "").await;
    assert_eq!(list["items"][0]["remixes"], 1);
    assert!(!list.to_string().contains("T-REMIXER"));
    let (_, mine) = app.get("/api/community/mine", Some(&owner)).await;
    assert_eq!(mine["items"][0]["remixes"], 1);
    assert!(!mine.to_string().contains("T-REMIXER"));

    // Withdrawing the original: the copy stays with me and stops naming it.
    unshare(&app, &owner, ws).await;
    let (s, state) = app.get(&format!("/api/worksheets/{copy}"), Some(&me)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(state["remixed_from"], Value::Null);
    assert!(!state["content"]["blocks"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_remix_keeps_the_pictures_of_the_original() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-PIC-OWNER", "general").await;
    let me = app.teacher("T-PIC-REMIXER", "general").await;
    let ws = simple_worksheet(&app, &owner, "With Picture").await;
    sqlx::query(
        "INSERT INTO worksheet_images (worksheet_id, teacher_id, block_id, slot_id, source, status,
                                       current, storage_key, content_type, bytes, width, height, sha256)
         SELECT $1, teacher_id, 'B1', 'S1', 'generated', 'done', TRUE, 'k/p.jpg', 'image/jpeg', 10,
                100, 80, 'abc' FROM worksheets WHERE id = $1",
    )
    .bind(ws)
    .execute(&app.db)
    .await
    .unwrap();
    share(&app, &owner, ws, "code", true).await;
    let (s, v) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&me),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let copy = v["worksheet_id"].as_i64().unwrap();
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT teacher_id, slot_id, storage_key FROM worksheet_images WHERE worksheet_id = $1",
    )
    .bind(copy)
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    let me_id: i64 =
        sqlx::query_scalar("SELECT id FROM teachers WHERE research_id = 'T-PIC-REMIXER'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!((rows[0].0, rows[0].1.as_str()), (me_id, "S1"));
    // Another teacher cannot fetch a picture of a worksheet that is not shared, nor an unknown one.
    unshare(&app, &owner, ws).await;
    let (s, _) = app
        .get(
            &format!("/api/community/worksheets/{ws}/images/1"),
            Some(&me),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------- save, preview, logs

#[tokio::test]
async fn saving_is_idempotent_and_not_for_ones_own_worksheet() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let me = app.teacher("T-ME", "general").await;
    let ws = simple_worksheet(&app, &owner, "Savable").await;
    share(&app, &owner, ws, "code", true).await;

    let (s, _) = app
        .post(
            &format!("/api/community/worksheets/{ws}/save"),
            Some(&owner),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    for _ in 0..2 {
        let (s, v) = app
            .post(
                &format!("/api/community/worksheets/{ws}/save"),
                Some(&me),
                json!({}),
            )
            .await;
        assert_eq!((s, &v["saved"]), (StatusCode::OK, &json!(true)));
    }
    let (_, saved) = app.get("/api/community/saves", Some(&me)).await;
    assert_eq!(ids(&saved), vec![ws]);
    assert_eq!(saved["items"][0]["saved"], true);
    assert_eq!(saved["items"][0]["saves"], 1);
    assert!(saved["items"][0]["saved_at"].is_string());
    // Somebody else's saves are theirs alone.
    let (_, none) = app.get("/api/community/saves", Some(&owner)).await;
    assert!(ids(&none).is_empty());

    for _ in 0..2 {
        let (s, v) = app
            .delete(&format!("/api/community/worksheets/{ws}/save"), Some(&me))
            .await;
        assert_eq!((s, &v["saved"]), (StatusCode::OK, &json!(false)));
    }
    let (_, saved) = app.get("/api/community/saves", Some(&me)).await;
    assert!(ids(&saved).is_empty());

    // One log row per real change, ids only.
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM usage_logs WHERE event = 'community_save'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM usage_logs WHERE event = 'community_unsave'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn preview_asks_for_a_version_and_is_logged() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let me = app.teacher("T-ME", "general").await;
    let ws = simple_worksheet(&app, &owner, "Preview Me").await;
    let (s, _) = app
        .get(&format!("/api/community/worksheets/{ws}"), Some(&me))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "private until shared");
    share(&app, &owner, ws, "code", true).await;
    let (s, v) = app
        .get(&format!("/api/community/worksheets/{ws}"), Some(&me))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["sheet"]["audience"], "student");
    assert_eq!(v["is_mine"], false);
    let (s, _) = app
        .get(
            &format!("/api/community/worksheets/{ws}?level=nope"),
            Some(&me),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .get(&format!("/api/community/worksheets/{ws}"), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v) = app
        .get(&format!("/api/community/worksheets/{ws}"), Some(&owner))
        .await;
    assert_eq!((s, &v["is_mine"]), (StatusCode::OK, &json!(true)));
}

#[tokio::test]
async fn research_events_carry_ids_and_labels_only() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let me = app.teacher("T-ME", "general").await;
    let ws = private_worksheet(&app, &owner).await;
    share(&app, &owner, ws, "code", true).await;
    app.get(&format!("/api/community/worksheets/{ws}"), Some(&me))
        .await;
    app.post(
        &format!("/api/community/worksheets/{ws}/save"),
        Some(&me),
        json!({}),
    )
    .await;
    app.delete(&format!("/api/community/worksheets/{ws}/save"), Some(&me))
        .await;
    let (_, v) = app
        .post(
            &format!("/api/community/worksheets/{ws}/remix"),
            Some(&me),
            json!({}),
        )
        .await;
    let copy = v["worksheet_id"].as_i64().unwrap();
    unshare(&app, &owner, ws).await;

    let rows: Vec<(String, Value, i64)> = sqlx::query_as(
        "SELECT event, payload, teacher_id FROM usage_logs WHERE event LIKE 'community_%' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    let events: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        events,
        [
            "community_share",
            "community_view",
            "community_save",
            "community_unsave",
            "community_remix",
            "community_unshare"
        ]
    );
    let share = &rows[0].1;
    assert_eq!(
        share,
        &json!({"worksheet_id": ws, "name": "code", "allow_remix": true, "update": false})
    );
    assert_eq!(rows[4].1, json!({"worksheet_id": ws, "copy_id": copy}));
    for (_, payload, _) in &rows {
        // Numbers, booleans and fixed labels; no free text.
        for (k, v) in payload.as_object().unwrap() {
            assert!(
                v.is_number() || v.is_boolean() || matches!(k.as_str(), "name" | "level"),
                "{k}: {v}"
            );
        }
        assert_clean(payload, &[]);
    }
    // The actor of each event is the teacher who acted.
    assert_ne!(rows[0].2, rows[1].2);
}

#[tokio::test]
async fn my_shares_lists_my_worksheets_with_their_state() {
    let app = test_app!();
    switch(&app, true).await;
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let a = simple_worksheet(&app, &owner, "First").await;
    let b = simple_worksheet(&app, &owner, "Second").await;
    let _theirs = simple_worksheet(&app, &other, "Not Mine").await;
    share(&app, &owner, a, "anonymous", false).await;
    let (s, v) = app.get("/api/community/mine", Some(&owner)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let first = items.iter().find(|i| i["id"] == a).unwrap();
    assert_eq!(
        (&first["shared"], &first["name"], &first["allow_remix"]),
        (&json!(true), &json!("anonymous"), &json!(false))
    );
    let second = items.iter().find(|i| i["id"] == b).unwrap();
    assert_eq!(second["shared"], false);
    assert_eq!(second["name"], Value::Null);
}
