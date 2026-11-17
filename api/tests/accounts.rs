//! 帳號與角色 (docs/API.md「帳號與角色」): the PI lists accounts and changes their roles; the
//! change applies from the account's next request; nobody else may do either; a PI cannot change
//! their own role.

#[macro_use]
mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

fn err_code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<none>")
}

/// Invites `research_id` with `role` through the PI and logs the account in.
async fn account(app: &common::TestApp, pi: &str, research_id: &str, role: &str) -> (String, i64) {
    let (s, v) = app
        .post(
            "/api/admin/invites",
            Some(pi),
            json!({ "research_id": research_id, "school_track": "general", "role": role }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (token, teacher) = app.login(v["code"].as_str().unwrap()).await;
    (token, teacher["id"].as_i64().unwrap())
}

async fn capabilities(app: &common::TestApp, token: &str) -> Vec<String> {
    let (s, v) = app.get("/api/me", Some(token)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    serde_json::from_value(v["capabilities"].clone()).unwrap()
}

async fn role_in_db(app: &common::TestApp, id: i64) -> String {
    sqlx::query_scalar("SELECT role FROM teachers WHERE id = $1")
        .bind(id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn lists_accounts_without_personal_data() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (_, teacher_id) = account(&app, &pi, "T-ROLE-1", "teacher").await;
    let (_, kb_id) = account(&app, &pi, "KB-ROLE-1", "kb_manager").await;
    let (_, gone_id) = account(&app, &pi, "T-ROLE-GONE", "teacher").await;
    sqlx::query("UPDATE teachers SET deleted_at = now() WHERE id = $1")
        .bind(gone_id)
        .execute(&app.db)
        .await
        .unwrap();
    // An identity on file must never reach this list.
    sqlx::query("INSERT INTO identity_map (research_id, name_enc) VALUES ('T-ROLE-1', 'secret')")
        .execute(&app.db)
        .await
        .unwrap();
    // An invite nobody has used yet is not an account.
    let (s, _) = app
        .post(
            "/api/admin/invites",
            Some(&pi),
            json!({ "research_id": "T-NOT-YET", "school_track": "vocational" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);

    let (s, v) = app.get("/api/admin/accounts", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let rows = v.as_array().unwrap();
    let ids: Vec<&str> = rows
        .iter()
        .map(|r| r["research_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["PI-001", "T-ROLE-1", "KB-ROLE-1"],
        "oldest first, no deleted account"
    );
    for row in rows {
        let mut keys: Vec<&str> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "created_at",
                "id",
                "is_self",
                "research_id",
                "role",
                "school_track"
            ]
        );
    }
    assert!(!v.to_string().contains("secret"));
    assert_eq!(rows[0]["role"], "pi");
    assert_eq!(rows[0]["is_self"], true);
    assert_eq!(rows[1]["id"], teacher_id);
    assert_eq!(rows[1]["role"], "teacher");
    assert_eq!(rows[1]["school_track"], "general");
    assert_eq!(rows[1]["is_self"], false);
    assert_eq!(rows[2]["id"], kb_id);
    assert_eq!(rows[2]["role"], "kb_manager");
    assert!(rows[2]["created_at"].is_string());
}

#[tokio::test]
async fn role_change_applies_on_the_next_request() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (token, id) = account(&app, &pi, "T-ROLE-2", "teacher").await;
    assert_eq!(capabilities(&app, &token).await, ["use_materials"]);
    let (s, _) = app.get("/api/admin/knowledge/summary", Some(&token)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (s, v) = app
        .patch(
            &format!("/api/admin/accounts/{id}"),
            Some(&pi),
            json!({ "role": "kb_manager" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["id"], id);
    assert_eq!(v["research_id"], "T-ROLE-2");
    assert_eq!(v["role"], "kb_manager");
    assert_eq!(v["is_self"], false);

    // Same session, no new login: the role is read from `teachers` on every request.
    let (s, me) = app.get("/api/me", Some(&token)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["role"], "kb_manager");
    let caps = capabilities(&app, &token).await;
    assert!(caps.iter().any(|c| c == "manage_knowledge"), "{caps:?}");
    assert!(!caps.iter().any(|c| c == "administer"), "{caps:?}");
    let (s, v) = app.get("/api/admin/knowledge/summary", Some(&token)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // ...and still nothing of the PI's own.
    let (s, _) = app.get("/api/admin/accounts", Some(&token)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (s, v) = app.get("/api/admin/accounts", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK);
    let row = v
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(row["role"], "kb_manager");

    // Setting the same role again changes nothing and is not an error.
    let (s, v) = app
        .patch(
            &format!("/api/admin/accounts/{id}"),
            Some(&pi),
            json!({ "role": "kb_manager" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["role"], "kb_manager");

    // And back.
    let (s, _) = app
        .patch(
            &format!("/api/admin/accounts/{id}"),
            Some(&pi),
            json!({ "role": "teacher" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(capabilities(&app, &token).await, ["use_materials"]);
    let (s, _) = app.get("/api/admin/knowledge/summary", Some(&token)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn only_the_pi_may_list_or_change_roles() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (teacher, teacher_id) = account(&app, &pi, "T-ROLE-3", "teacher").await;
    let (kb, kb_id) = account(&app, &pi, "KB-ROLE-3", "kb_manager").await;

    let (s, _) = app.get("/api/admin/accounts", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app
        .patch(
            &format!("/api/admin/accounts/{teacher_id}"),
            None,
            json!({ "role": "pi" }),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    for (who, token) in [("teacher", &teacher), ("kb_manager", &kb)] {
        let (s, v) = app.get("/api/admin/accounts", Some(token)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{who}: {v}");
        assert_eq!(err_code(&v), "forbidden");
        // Neither on someone else's account nor on their own.
        for id in [teacher_id, kb_id] {
            let (s, v) = app
                .patch(
                    &format!("/api/admin/accounts/{id}"),
                    Some(token),
                    json!({ "role": "pi" }),
                )
                .await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{who} → {id}: {v}");
            assert_eq!(err_code(&v), "forbidden");
        }
    }
    assert_eq!(role_in_db(&app, teacher_id).await, "teacher");
    assert_eq!(role_in_db(&app, kb_id).await, "kb_manager");
}

#[tokio::test]
async fn rejects_invalid_roles_and_unknown_or_deleted_accounts() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (_, id) = account(&app, &pi, "T-ROLE-4", "teacher").await;
    let path = format!("/api/admin/accounts/{id}");

    for body in [
        json!({ "role": "admin" }),
        json!({ "role": "" }),
        json!({}),
        json!({ "role": "kb_manager", "research_id": "X" }),
    ] {
        let (s, v) = app.patch(&path, Some(&pi), body.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}: {v}");
        assert_eq!(err_code(&v), "bad_request");
    }
    let (s, _) = app
        .patch(
            "/api/admin/accounts/abc",
            Some(&pi),
            json!({ "role": "teacher" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(role_in_db(&app, id).await, "teacher");

    let (s, v) = app
        .patch(
            "/api/admin/accounts/987654",
            Some(&pi),
            json!({ "role": "kb_manager" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(err_code(&v), "not_found");

    sqlx::query("UPDATE teachers SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await
        .unwrap();
    let (s, v) = app
        .patch(&path, Some(&pi), json!({ "role": "kb_manager" }))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(role_in_db(&app, id).await, "teacher");
}

#[tokio::test]
async fn a_pi_cannot_change_their_own_role() {
    let app = test_app!();
    let pi = app.pi_token().await;
    let (s, me) = app.get("/api/me", Some(&pi)).await;
    assert_eq!(s, StatusCode::OK);
    let pi_id = me["id"].as_i64().unwrap();

    let (s, v) = app
        .patch(
            &format!("/api/admin/accounts/{pi_id}"),
            Some(&pi),
            json!({ "role": "teacher" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(err_code(&v), "conflict");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("自己的角色"),
        "{v}"
    );
    assert_eq!(role_in_db(&app, pi_id).await, "pi");
    // Not a change: fine.
    let (s, v) = app
        .patch(
            &format!("/api/admin/accounts/{pi_id}"),
            Some(&pi),
            json!({ "role": "pi" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["is_self"], true);

    // With a second PI, either may change the other's role, but still not their own.
    let (second, second_id) = account(&app, &pi, "PI-002", "teacher").await;
    let (s, _) = app
        .patch(
            &format!("/api/admin/accounts/{second_id}"),
            Some(&pi),
            json!({ "role": "pi" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = app
        .patch(
            &format!("/api/admin/accounts/{pi_id}"),
            Some(&second),
            json!({ "role": "kb_manager" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["role"], "kb_manager");
    assert_eq!(v["is_self"], false);
    // The demoted PI has lost the admin area at once.
    let (s, _) = app.get("/api/admin/accounts", Some(&pi)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // The one PI left cannot step down.
    let (s, v) = app
        .patch(
            &format!("/api/admin/accounts/{second_id}"),
            Some(&second),
            json!({ "role": "teacher" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(role_in_db(&app, second_id).await, "pi");
}
