//! 學習單：逐區修改與註記 (docs/API.md; docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §4–§5): notes on a
//! worksheet, one block revised by the fake model as a proposal, kept questions preserved by the
//! server, 採用／放棄／再改一次, the block history and undo, ownership, notes for AI in the prompts,
//! and the 409 of an editor that saves over a newer version.

#[macro_use]
mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};

const PASSAGE: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”\n\nHer neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.\n\nIn the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.\n\nToday the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.";

async fn generated(app: &common::TestApp, t: &str) -> (i64, Value) {
    let (s, v) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": { "title": "The Umbrella Stand", "text": PASSAGE } }),
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
    let st = wait_jobs(app, t, id).await;
    // The first activity block gets questions of the teacher's own until it has three.
    let mut content = st["content"].clone();
    let block = content["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["kind"] == "activity")
        .unwrap();
    let levels = block["items"][0]["levels"].clone();
    let items = block["items"].as_array_mut().unwrap();
    let mut k = 0;
    while items.len() < 3 {
        k += 1;
        items.push(json!({
            "id": format!("t-extra-{k}"), "answer_format": "簡答題",
            "prompt": format!("What else did you notice in the story? ({k})"),
            "options": [], "answer": "Answers will vary.", "evidence": "", "purpose": null,
            "levels": levels, "origin": "teacher", "bloom": null, "hint": "",
        }));
    }
    let (s, st) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(t),
            json!({ "content": content, "base_version_no": st["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    (id, st)
}

async fn wait_jobs(app: &common::TestApp, t: &str, id: i64) -> Value {
    for _ in 0..600 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/jobs"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["running"] == false {
            return state(app, t, id).await;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("worksheet {id} did not finish generating");
}

async fn state(app: &common::TestApp, t: &str, id: i64) -> Value {
    let (s, v) = app.get(&format!("/api/worksheets/{id}"), Some(t)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

/// The first activity block with at least three questions.
fn target_block(state: &Value) -> Value {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "activity" && b["items"].as_array().unwrap().len() >= 3)
        .cloned()
        .expect("an activity block with three questions")
}

fn block_of(state: &Value, id: &str) -> Value {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == id)
        .cloned()
        .unwrap()
}

async fn comment(app: &common::TestApp, t: &str, id: i64, body: Value) -> Value {
    let (s, v) = app
        .post(&format!("/api/worksheets/{id}/comments"), Some(t), body)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn revise(app: &common::TestApp, t: &str, id: i64, block: &str, body: Value) -> Value {
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block}/revise"),
            Some(t),
            body,
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["status"], "running");
    wait_proposal(app, t, id, v["id"].as_i64().unwrap()).await
}

async fn wait_proposal(app: &common::TestApp, t: &str, id: i64, pid: i64) -> Value {
    for _ in 0..600 {
        let (s, v) = app
            .get(&format!("/api/worksheets/{id}/proposals/{pid}"), Some(t))
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        if v["status"] != "running" {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("proposal {pid} did not finish");
}

async fn users_of(app: &common::TestApp, module: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT input_snapshot -> 'prompt' ->> 'user' FROM ai_interactions WHERE module = $1 ORDER BY id",
    )
    .bind(module)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

/// The second word of a text and its UTF-16 offsets (what a browser selection gives).
fn second_word(text: &str) -> (usize, usize, String) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let is_word = |u: u16| char::from_u32(u as u32).is_some_and(|c| c.is_ascii_alphanumeric());
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < units.len() {
        if is_word(units[i]) {
            let s = i;
            while i < units.len() && is_word(units[i]) {
                i += 1;
            }
            words.push((s, i));
        } else {
            i += 1;
        }
    }
    let (s, e) = words[1];
    (s, e, String::from_utf16(&units[s..e]).unwrap())
}

fn changes(p: &Value) -> Vec<(String, String)> {
    p["changes"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap().to_string(),
                c["change"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_note_on_one_question_revises_only_that_question_and_waits_for_the_teacher() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block = target_block(&st);
    let block_id = block["id"].as_str().unwrap().to_string();
    let items = block["items"].as_array().unwrap().clone();

    // The teacher edits the first question herself (through the editor's save).
    let mut content = st["content"].clone();
    for b in content["blocks"].as_array_mut().unwrap() {
        if b["id"] == block_id.as_str() {
            b["items"][0]["prompt"] = json!("My own question: why the stand?");
            b["items"][0]["origin"] = json!("ai_edited");
        }
    }
    let (s, st) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": st["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    let before = block_of(&st, &block_id);
    let versions = st["versions_count"].as_i64().unwrap();

    // A note on a word of the second question, one only for herself on the block.
    let q2 = items[1]["id"].as_str().unwrap();
    let prompt = items[1]["prompt"].as_str().unwrap().trim().to_string();
    let (start, end, word) = second_word(&prompt);
    let note = comment(
        &app,
        &t,
        id,
        json!({ "block_id": block_id, "field": "item", "item_id": q2, "start": start,
                "end": end, "quote": word, "note": "這題太難，改簡單一點" }),
    )
    .await;
    assert_eq!(note["quote"], word.as_str());
    assert_eq!(note["share_with_ai"], true, "on by default");
    assert_eq!(note["status"], "open");
    assert_eq!(note["anchor"]["status"], "ok");
    assert_eq!(note["anchor"]["item_id"], q2);
    let private = comment(
        &app,
        &t,
        id,
        json!({ "block_id": block_id, "note": "只給我自己看", "share_with_ai": false }),
    )
    .await;
    assert_eq!(private["field"], "block");

    let p = revise(
        &app,
        &t,
        id,
        &block_id,
        json!({ "comment_ids": [note["id"]] }),
    )
    .await;
    assert_eq!(p["status"], "proposed", "{p}");
    assert_eq!(p["before"], before);
    assert_eq!(p["base_version_no"], st["version_no"]);
    let proposed = p["block"].clone();
    let after_items = proposed["items"].as_array().unwrap();
    assert_eq!(after_items.len(), before["items"].as_array().unwrap().len());
    for (k, (a, b)) in after_items
        .iter()
        .zip(before["items"].as_array().unwrap())
        .enumerate()
    {
        assert_eq!(a["id"], b["id"], "same ids");
        if k == 1 {
            assert!(
                a["prompt"].as_str().unwrap().ends_with("(easier r1)"),
                "{a}"
            );
            assert_eq!(a["levels"], b["levels"], "fields it does not write stay");
            assert_eq!(a["origin"], "ai");
        } else {
            assert_eq!(a, b, "kept byte for byte");
        }
    }
    assert_eq!(after_items[0]["prompt"], "My own question: why the stand?");
    assert_eq!(proposed["title"], before["title"]);
    assert_eq!(proposed["scaffolds"], before["scaffolds"]);
    let ch = changes(&p);
    assert_eq!(ch[1].1, "modified");
    assert!(ch.iter().enumerate().all(|(k, c)| k == 1 || c.1 == "kept"));
    assert!(p["reply"].as_str().unwrap().contains("Q2"), "{p}");

    // Nothing is written before 採用.
    let now = state(&app, &t, id).await;
    assert_eq!(now["versions_count"].as_i64().unwrap(), versions);
    assert_eq!(block_of(&now, &block_id), before);
    assert_eq!(now["running"], false, "a revision is not a generation job");
    let (_, open) = app
        .get(&format!("/api/worksheets/{id}/proposals"), Some(&t))
        .await;
    assert_eq!(open.as_array().unwrap().len(), 1);

    // Only that block, the note shared with the AI, the scope by code.
    let users = users_of(&app, "worksheet_block_revise").await;
    assert_eq!(users.len(), 1);
    assert!(users[0].contains("這題太難，改簡單一點"), "{}", users[0]);
    assert!(users[0].contains("[Q2]"));
    assert!(!users[0].contains("只給我自己看"));
    assert!(users[0].contains("只能改 Q2"));
    assert!(
        !users[0].contains(PASSAGE.split("\n\n").nth(1).unwrap()),
        "the material is logged as a hash"
    );

    // 採用: a new version, the old block in the history, the note applied.
    let (s, d) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", p["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["proposal"]["status"], "accepted");
    assert_eq!(block_of(&d["worksheet"], &block_id), proposed);
    assert_eq!(
        d["worksheet"]["versions_count"].as_i64().unwrap(),
        versions + 1
    );
    assert_eq!(
        d["proposal"]["accepted_version_no"],
        d["worksheet"]["version_no"]
    );
    assert_eq!(d["worksheet"]["undoable"], json!([block_id]));
    let (_, list) = app
        .get(&format!("/api/worksheets/{id}/comments"), Some(&t))
        .await;
    let mine = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == note["id"])
        .unwrap();
    assert_eq!(mine["status"], "applied");
    assert_eq!(mine["proposal_id"], p["id"]);
    let other = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == private["id"])
        .unwrap();
    assert_eq!(other["status"], "open");
    let (_, versions_list) = app
        .get(&format!("/api/worksheets/{id}/versions"), Some(&t))
        .await;
    assert_eq!(versions_list[0]["cause"], "revise");
    let adopted: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM teacher_decisions WHERE decision = 'adopt'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(adopted, 1);
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", p["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "only once");
    let (_, open) = app
        .get(&format!("/api/worksheets/{id}/proposals"), Some(&t))
        .await;
    assert_eq!(open, json!([]));

    // 復原上一版 brings the block back.
    let (s, st) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert_eq!(block_of(&st, &block_id), before);

    // Notes are never printed.
    for audience in ["student", "teacher"] {
        let (_, sheet) = app
            .get(
                &format!("/api/worksheets/{id}/sheet?audience={audience}"),
                Some(&t),
            )
            .await;
        let text = sheet.to_string();
        assert!(!text.contains("只給我自己看") && !text.contains("這題太難"));
        // The sheet carries question ids (notes are anchored to them).
        assert!(text.contains(q2));
    }
}

#[tokio::test]
async fn the_code_keeps_what_the_notes_do_not_cover() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block = target_block(&st);
    let block_id = block["id"].as_str().unwrap();
    let items = block["items"].as_array().unwrap();
    let n = items.len();
    let q1 = items[0]["id"].as_str().unwrap();
    let note = comment(
        &app,
        &t,
        id,
        json!({ "block_id": block_id, "item_id": q1,
                "note": "改簡單 FAKE_REVISE_TOUCH_ALL FAKE_REVISE_DROP FAKE_REVISE_ADD" }),
    )
    .await;
    assert_eq!(
        note["field"], "item",
        "an item id makes it a note on the question"
    );

    // The model changed every question, the title, left the last one out and added one: only Q1
    // changes, the rest comes back as it was, the new question is added after Q1.
    // The model put the new question after the last one it listed (the one before the last).
    let p = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "comment_ids": [note["id"]] }),
    )
    .await;
    assert_eq!(p["status"], "proposed", "{p}");
    let after = p["block"]["items"].as_array().unwrap();
    assert_eq!(after.len(), n + 1);
    assert!(after[0]["prompt"]
        .as_str()
        .unwrap()
        .ends_with("(easier r1)"));
    for k in 1..n - 1 {
        assert_eq!(after[k], items[k], "not touched");
    }
    assert!(after[n - 1]["prompt"]
        .as_str()
        .unwrap()
        .ends_with("(added)"));
    assert!(after[n - 1]["id"]
        .as_str()
        .unwrap()
        .ends_with(&format!("-p{}n1", p["id"])));
    assert_eq!(
        after[n],
        items[n - 1],
        "left out but not covered by the note: kept"
    );
    assert_eq!(p["block"]["title"], block["title"]);
    assert_eq!(changes(&p)[n - 1].1, "new");

    // 「可以動到這一區的其他題」: everything the model wrote is taken; the last one is removed.
    let p = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "comment_ids": [note["id"]], "whole_block": true }),
    )
    .await;
    assert_eq!(p["round"], 2);
    let after = p["block"]["items"].as_array().unwrap();
    assert_eq!(after.len(), n, "one removed, one added");
    assert!(after[1]["prompt"].as_str().unwrap().ends_with("(touched)"));
    assert!(p["block"]["title"].as_str().unwrap().ends_with("(touched)"));
    let ch = changes(&p);
    assert!(ch
        .iter()
        .any(|(i, c)| i == items[n - 1]["id"].as_str().unwrap() && c == "removed"));
    let users = users_of(&app, "worksheet_block_revise").await;
    assert!(users[1].contains("整個區塊"));
    assert!(
        users[1].contains("第 2 次"),
        "「再改一次」 changes the request"
    );
}

#[tokio::test]
async fn discard_try_again_and_a_sentence_without_notes() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block = target_block(&st);
    let block_id = block["id"].as_str().unwrap();
    let versions = st["versions_count"].clone();

    let p1 = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "instruction": "整區改簡單一點" }),
    )
    .await;
    assert_eq!(p1["status"], "proposed");
    // 再改一次 while it waits: the first is set aside as discarded.
    let p2 = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "instruction": "整區改簡單一點" }),
    )
    .await;
    assert_eq!(p2["round"], 2);
    assert!(p2["block"]["items"][0]["prompt"]
        .as_str()
        .unwrap()
        .ends_with("(easier r2)"));
    let (_, first) = app
        .get(
            &format!("/api/worksheets/{id}/proposals/{}", p1["id"]),
            Some(&t),
        )
        .await;
    assert_eq!(first["status"], "rejected");
    let temps: Vec<f64> = sqlx::query_scalar(
        "SELECT (input_snapshot -> 'request' ->> 'temperature')::float8 FROM ai_interactions
          WHERE module = 'worksheet_block_revise' ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    assert!(temps[1] > temps[0], "{temps:?}");

    // 放棄: nothing written, recorded as a rejection.
    let (s, r) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/reject", p2["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "rejected");
    let now = state(&app, &t, id).await;
    assert_eq!(now["versions_count"], versions);
    assert_eq!(block_of(&now, block_id), block);
    let rejected: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM teacher_decisions WHERE decision = 'reject'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(rejected, 2);
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", p2["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, open) = app
        .get(&format!("/api/worksheets/{id}/proposals"), Some(&t))
        .await;
    assert_eq!(open, json!([]));
}

#[tokio::test]
async fn a_block_changed_meanwhile_is_not_overwritten() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block = target_block(&st);
    let block_id = block["id"].as_str().unwrap();
    let p = revise(&app, &t, id, block_id, json!({ "instruction": "改簡單" })).await;

    // The editor changes the same block before 採用.
    let mut content = st["content"].clone();
    for b in content["blocks"].as_array_mut().unwrap() {
        if b["id"] == block_id {
            b["instructions"] = json!("Changed by hand.");
        }
    }
    let (s, edited) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": st["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{edited}");
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", p["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("又被改過了"));
    assert_eq!(
        block_of(&state(&app, &t, id).await, block_id)["instructions"],
        "Changed by hand."
    );
    let (_, open) = app
        .get(&format!("/api/worksheets/{id}/proposals"), Some(&t))
        .await;
    assert_eq!(open[0]["status"], "stale");
    // 知道了 closes it.
    let (s, closed) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/reject", p["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{closed}");
    assert_eq!(closed["status"], "stale");
    let (_, open) = app
        .get(&format!("/api/worksheets/{id}/proposals"), Some(&t))
        .await;
    assert_eq!(open, json!([]));

    // Another block edited meanwhile does not matter.
    let other = st["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] != block_id && b["kind"] == "activity")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let p = revise(&app, &t, id, block_id, json!({ "instruction": "改簡單" })).await;
    let latest = state(&app, &t, id).await;
    let mut content = latest["content"].clone();
    for b in content["blocks"].as_array_mut().unwrap() {
        if b["id"] == other.as_str() {
            b["title"] = json!("Another block, renamed");
        }
    }
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": content, "base_version_no": latest["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, d) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", p["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(
        block_of(&d["worksheet"], &other)["title"],
        "Another block, renamed"
    );
}

#[tokio::test]
async fn an_editor_saving_over_a_newer_version_gets_a_409() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block_id = target_block(&st)["id"].as_str().unwrap().to_string();
    let seen = st["version_no"].clone();
    let p = revise(&app, &t, id, &block_id, json!({ "instruction": "改簡單" })).await;
    let (s, d) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", p["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    let accepted = d["worksheet"]["content"].clone();

    // The editor still holds the version before 採用 and saves an edit of it.
    let mut stale = st["content"].clone();
    stale["show_objectives"] = json!(!stale["show_objectives"].as_bool().unwrap());
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "title": "Not saved either", "content": stale, "base_version_no": seen }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("重新載入"));
    let now = state(&app, &t, id).await;
    assert_eq!(
        now["content"], accepted,
        "the accepted revision is not overwritten"
    );
    assert_ne!(
        now["title"], "Not saved either",
        "nothing of the save is kept"
    );

    // Sending what is already there is no conflict; the current version saves as usual.
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": accepted, "base_version_no": seen }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let mut edit = accepted.clone();
    edit["show_objectives"] = json!(!edit["show_objectives"].as_bool().unwrap());
    let (s, saved) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": edit, "base_version_no": now["version_no"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["version_no"].as_i64().unwrap(),
        now["version_no"].as_i64().unwrap() + 1
    );
    // Without base_version_no the last save wins, as before.
    let (s, _) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(&t),
            json!({ "content": stale }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn notes_and_proposals_belong_to_their_teacher_and_requests_are_checked() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let other = app.teacher("T-002", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block_id = target_block(&st)["id"].as_str().unwrap().to_string();
    let note = comment(
        &app,
        &t,
        id,
        json!({ "block_id": block_id, "field": "title", "note": "標題改短" }),
    )
    .await;
    let cid = note["id"].as_i64().unwrap();
    let p = revise(&app, &t, id, &block_id, json!({ "comment_ids": [cid] })).await;
    assert_eq!(
        p["status"], "failed",
        "a title note in a block the fake changes nowhere: {p}"
    );
    let pid = p["id"].as_i64().unwrap();

    for (s, _) in [
        app.get(&format!("/api/worksheets/{id}/comments"), Some(&other))
            .await,
        app.post(
            &format!("/api/worksheets/{id}/comments"),
            Some(&other),
            json!({ "block_id": block_id, "note": "x" }),
        )
        .await,
        app.patch(
            &format!("/api/worksheets/{id}/comments/{cid}"),
            Some(&other),
            json!({ "note": "x" }),
        )
        .await,
        app.delete(
            &format!("/api/worksheets/{id}/comments/{cid}"),
            Some(&other),
        )
        .await,
        app.post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/revise"),
            Some(&other),
            json!({ "instruction": "x" }),
        )
        .await,
        app.get(&format!("/api/worksheets/{id}/proposals"), Some(&other))
            .await,
        app.get(
            &format!("/api/worksheets/{id}/proposals/{pid}"),
            Some(&other),
        )
        .await,
        app.post(
            &format!("/api/worksheets/{id}/proposals/{pid}/accept"),
            Some(&other),
            json!({}),
        )
        .await,
        app.post(
            &format!("/api/worksheets/{id}/proposals/{pid}/reject"),
            Some(&other),
            json!({}),
        )
        .await,
    ] {
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
    // A note of this worksheet through another of the teacher's worksheets is not found either.
    let (id2, _) = generated(&app, &t).await;
    let (s, _) = app
        .patch(
            &format!("/api/worksheets/{id2}/comments/{cid}"),
            Some(&t),
            json!({ "note": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Edit, 略過, back to open, delete.
    let (s, v) = app
        .patch(
            &format!("/api/worksheets/{id}/comments/{cid}"),
            Some(&t),
            json!({ "note": "標題改短一點", "share_with_ai": false, "status": "dismissed" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["note"].as_str(),
            v["share_with_ai"].as_bool(),
            v["status"].as_str()
        ),
        (Some("標題改短一點"), Some(false), Some("dismissed"))
    );
    let (s, _) = app
        .patch(
            &format!("/api/worksheets/{id}/comments/{cid}"),
            Some(&t),
            json!({ "status": "applied" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "applied is set by 採用 only");

    for body in [
        json!({ "block_id": "nope", "note": "x" }),
        json!({ "block_id": block_id, "field": "item", "item_id": "nope", "note": "x" }),
        json!({ "block_id": block_id, "note": "x".repeat(2001) }),
        json!({ "block_id": block_id, "field": "title", "start": 2, "end": 900, "note": "x" }),
    ] {
        let (s, v) = app
            .post(&format!("/api/worksheets/{id}/comments"), Some(&t), body)
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    }
    // A revise needs something to go by; a note must be shared with Manna and still open.
    for body in [
        json!({}),
        json!({ "comment_ids": [cid] }),
        json!({ "comment_ids": [999_999] }),
        json!({ "instruction": "x".repeat(2001) }),
    ] {
        let (s, v) = app
            .post(
                &format!("/api/worksheets/{id}/blocks/{block_id}/revise"),
                Some(&t),
                body,
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    }
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/nope/revise"),
            Some(&t),
            json!({ "instruction": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // One revision runs at a time.
    let (s, slow) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/revise"),
            Some(&t),
            json!({ "instruction": "改簡單 FAKE_SLOW" }),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{slow}");
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/{block_id}/revise"),
            Some(&t),
            json!({ "instruction": "x" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/proposals/{}/accept", slow["id"]),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "still running");
    assert_eq!(
        wait_proposal(&app, &t, id, slow["id"].as_i64().unwrap()).await["status"],
        "proposed"
    );

    let (s, _) = app
        .delete(&format!("/api/worksheets/{id}/comments/{cid}"), Some(&t))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = app
        .get(&format!("/api/worksheets/{id}/comments"), Some(&t))
        .await;
    assert_eq!(list, json!([]));
}

#[tokio::test]
async fn failures_say_why_and_never_touch_the_worksheet() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block = target_block(&st);
    let block_id = block["id"].as_str().unwrap();
    let versions = st["versions_count"].clone();

    let p = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "instruction": "改簡單 FAKE_REVISE_BAD" }),
    )
    .await;
    assert_eq!(p["status"], "failed", "{p}");
    assert!(
        p["error"]
            .as_str()
            .unwrap()
            .contains("沒有通過學習單的檢查"),
        "{p}"
    );
    assert_eq!(
        users_of(&app, "worksheet_block_revise").await.len(),
        2,
        "retried once"
    );

    let p = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "instruction": "改簡單 FAKE_REVISE_BAD_ONCE" }),
    )
    .await;
    assert_eq!(p["status"], "proposed", "the retry fixed it: {p}");
    assert_eq!(p["round"], 2);

    let p = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "instruction": "改 FAKE_REVISE_NOTHING" }),
    )
    .await;
    assert_eq!(p["status"], "failed");
    assert!(
        p["error"].as_str().unwrap().contains("哪一種題型"),
        "the model's question: {p}"
    );

    let p = revise(
        &app,
        &t,
        id,
        block_id,
        json!({ "instruction": "改 FAKE_TIMEOUT" }),
    )
    .await;
    assert_eq!(p["status"], "failed");
    assert!(p["error"].as_str().unwrap().contains("沒有回應"), "{p}");

    let now = state(&app, &t, id).await;
    assert_eq!(now["versions_count"], versions);
    assert_eq!(block_of(&now, block_id), block);
}

#[tokio::test]
async fn open_notes_for_ai_reach_a_new_generation_of_the_worksheet() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, st) = generated(&app, &t).await;
    let block = target_block(&st);
    let block_id = block["id"].as_str().unwrap();
    comment(
        &app,
        &t,
        id,
        json!({ "block_id": block_id, "item_id": block["items"][1]["id"], "note": "多一點推論題" }),
    )
    .await;
    comment(
        &app,
        &t,
        id,
        json!({ "block_id": block_id, "note": "私人筆記", "share_with_ai": false }),
    )
    .await;
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/generate"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::ACCEPTED, "{v}");
    let st = wait_jobs(&app, &t, id).await;
    let users = users_of(&app, "worksheet_tasks").await;
    let last = users.last().unwrap();
    assert!(last.contains("多一點推論題"), "{last}");
    assert!(last.contains("上一版學習單上的註記"));
    assert!(!last.contains("私人筆記"));
    let tasks = st["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["step"] == "tasks")
        .unwrap();
    assert!(tasks["context"]
        .to_string()
        .contains("教師註記（提供給 AI）"));
    // The block is gone after a new generation: the note says so.
    let (_, list) = app
        .get(&format!("/api/worksheets/{id}/comments"), Some(&t))
        .await;
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .all(|c| c["anchor"]["status"] == "lost"),
        "{list}"
    );
}
