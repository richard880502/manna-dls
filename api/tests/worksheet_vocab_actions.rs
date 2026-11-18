//! 詞彙級別檢查：處理超出級別的字 (docs/API.md「詞彙級別檢查：處理超出級別的字」): what the
//! check says about each word above the level, meanings from the fake model, 換字 as whole
//! sentences rewritten (phrases included) and checked on the server (every new word within the
//! level, the word gone, the change local), applied exactly as shown and only where ticked;
//! glosses in the content and in DOCX / PDF, the new version and per-block undo, and the
//! teacher's choices in `teacher_decisions` and `usage_logs`.

#[macro_use]
mod common;

use axum::http::Method;
use serde_json::{json, Value};

use axum::http::StatusCode;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn vocabulary_csv() -> String {
    let rows = [
        "the,the,1,art.,base,",
        "a,a/an,1,art.,base,",
        "an,a/an,1,art.,base,",
        "family,family,1,n.,base,",
        "house,house,1,n.,base,",
        "farm,farm,1,n.,base,",
        "leave,leave,1,v.,base,",
        "give,give,1,v.,base,",
        "up,up,1,adv./prep.,base,",
        "big,big,1,adj.,base,",
        "huge,huge,2,adj.,base,",
        "problem,problem,1,n.,base,",
        "change,change,2,n./v.,base,",
        "base,base,2,n./v.,base,",
        "on,on,1,prep.,base,",
        "text,text,3,n.,base,",
        "abandon,abandon,5,v.,base,",
        "accord,accord,5,n./v.,base,",
        "enormous,enormous,5,adj.,base,",
        "obstacle,obstacle,5,n.,base,",
        "resilient,resilient,6,adj.,base,",
        "gigantic,gigantic,6,adj.,base,",
    ];
    let body: Vec<String> = rows
        .iter()
        .map(|r| format!("{r},TEST LIST v1,測試詞彙表.pdf,5,{SHA}"))
        .collect();
    format!(
        "word,headword,level,pos,kind,appendix_category,list_version,source_doc,source_page,source_sha256\n{}\n",
        body.join("\n")
    )
}

const MATERIAL: &str = "The family decided to abandon the old farm. Life on the farm was hard, but they were resilient.";

fn item(id: &str, prompt: &str, levels: &[&str], format: &str, options: &[&str]) -> Value {
    json!({ "id": id, "answer_format": format, "prompt": prompt, "options": options,
            "answer": "", "evidence": "", "purpose": null, "levels": levels, "origin": "ai",
            "bloom": null, "hint": "", "tiers": [] })
}

/// `marker` goes next to 「according to」 (in its sentence and in the word's context).
fn content(marker: &str) -> Value {
    let both = ["basic", "standard"];
    json!({
        "objectives": [], "show_objectives": false, "levels": both, "chosen": null, "notices": [],
        "blocks": [
            {
                "id": "b1", "kind": "activity", "enabled": true, "title": "Part A",
                "instructions": "Read the text. Do not abandon a question you find hard.",
                "paragraph_ids": [], "info_function": null, "student_tasks": [], "objective_ids": [],
                "vocabulary": [{ "word": "resilient", "level": null, "definition": "有韌性的",
                                 "context": "", "paragraph_id": null }],
                "items": [
                    item("i1", "Why did the family abandon their enormous house?", &both, "簡答題", &[]),
                    item("i2", "What does \"abandon\" mean in paragraph 1?", &both, "選擇題",
                         &["to leave", "to build", "to buy", "to sell"]),
                    item("i3", "Abandoned houses can be an enormous obstacle. Do you agree?", &["standard"], "簡答題", &[]),
                ],
                "scaffolds": [{ "id": "b1-s1", "type": "verbal", "kind": "字詞表",
                                "content": "abandoned, house", "steps": [], "levels": ["basic"],
                                "auto": false, "origin": "ai" }],
                "teacher_notes": "", "origin": "ai", "message": null, "sop_block_id": null,
            },
            {
                "id": "b2", "kind": "activity", "enabled": true, "title": "Part B",
                "instructions": "Talk about an enormous change.",
                "paragraph_ids": [], "info_function": null, "student_tasks": [], "objective_ids": [],
                "vocabulary": [],
                "items": [
                    item("i4", "Name two obstacles you face.", &both, "簡答題", &[]),
                    item("i5", &format!("What would happen if bees disappeared{marker}, according to the text?"),
                         &both, "簡答題", &[]),
                ],
                "scaffolds": [], "teacher_notes": "", "origin": "ai", "message": null, "sop_block_id": null,
            },
        ],
    })
}

/// A worksheet with the content above, the word list imported, and the standard version
/// checked against levels 1–4.
async fn setup(app: &common::TestApp, t: &str, marker: &str) -> (i64, Value) {
    let pi = app.pi_token().await;
    let (s, v) = app
        .post_csv("/api/admin/vocabulary/import", Some(&pi), &vocabulary_csv())
        .await;
    assert!(s == StatusCode::OK || s == StatusCode::CONFLICT, "{s} {v}");
    let (s, ws) = app
        .post(
            "/api/worksheets",
            Some(t),
            json!({ "material": { "title": "The Farm", "text": MATERIAL } }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{ws}");
    let id = ws["id"].as_i64().unwrap();
    let (s, v) = app
        .put(
            &format!("/api/worksheets/{id}"),
            Some(t),
            json!({ "content": content(marker) }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, check) = app
        .post(
            &format!("/api/worksheets/{id}/checks/vocabulary"),
            Some(t),
            json!({ "level": "standard", "target_level": 4 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{check}");
    (id, check)
}

fn word<'a>(check: &'a Value, base: &str) -> &'a Value {
    check["words"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["base"] == base)
        .unwrap_or_else(|| panic!("{base} in {check}"))
}

fn suggestion<'a>(s: &'a Value, base: &str) -> &'a Value {
    s["words"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["base"] == base)
        .unwrap()
}

fn block<'a>(state: &'a Value, id: &str) -> &'a Value {
    state["content"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == id)
        .unwrap()
}

fn prompt_of(state: &Value, block_id: &str, item: &str) -> String {
    block(state, block_id)["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == item)
        .unwrap()["prompt"]
        .as_str()
        .unwrap()
        .to_string()
}

fn place<'a>(s: &'a Value, original_starts: &str) -> &'a Value {
    s["places"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["original"].as_str().unwrap().starts_with(original_starts))
        .unwrap_or_else(|| panic!("{original_starts} in {s}"))
}

/// The rewrites to send for the places shown (with a rewrite), except those starting with `skip`.
fn rewrites(s: &Value, skip: &[&str]) -> Vec<Value> {
    s["places"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| !p["rewritten"].is_null())
        .filter(|p| {
            !skip
                .iter()
                .any(|k| p["original"].as_str().unwrap().starts_with(k))
        })
        .map(|p| {
            json!({ "field": p["field"], "start": p["start"], "end": p["end"],
                    "original": p["original"], "rewritten": p["rewritten"], "bases": p["bases"] })
        })
        .collect()
}

#[tokio::test]
async fn the_check_says_what_can_be_done_with_each_word() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (_, check) = setup(&app, &t, "").await;
    let bases: Vec<&str> = check["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["base"].as_str().unwrap())
        .collect();
    assert_eq!(
        bases,
        ["resilient", "abandon", "accord", "enormous", "obstacle"]
    );

    let abandon = word(&check, "abandon");
    assert_eq!(abandon["forms"], json!(["abandon", "abandoned"]));
    assert_eq!(abandon["count"], 4);
    assert_eq!(abandon["replaceable"], 3, "the quoted one stays");
    assert!(abandon["fixed"][0].as_str().unwrap().contains("引號內"));
    assert_eq!(abandon["in_material"], true);
    assert_eq!(abandon["suggested"], "gloss");
    assert_eq!(abandon["pos"], "v.");

    let resilient = word(&check, "resilient");
    assert_eq!(resilient["key_word"], true);
    assert_eq!(resilient["replaceable"], 0);
    assert_eq!(resilient["suggested"], "leave");
    assert!(resilient["fixed"][0].as_str().unwrap().contains("字詞表"));

    let obstacle = word(&check, "obstacle");
    assert_eq!(obstacle["forms"], json!(["obstacle", "obstacles"]));
    assert_eq!(obstacle["in_material"], false);
    assert_eq!(obstacle["suggested"], "replace");
    assert_eq!(word(&check, "enormous")["replaceable"], 3);
    assert_eq!(word(&check, "accord")["forms"], json!(["according"]));
}

#[tokio::test]
async fn sentences_are_rewritten_whole_and_applied_as_shown() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    let (id, check) = setup(&app, &t, "").await;
    let check_id = check["id"].as_i64().unwrap();
    let version_before = check["version_no"].as_i64().unwrap();
    let suggest_url = format!("/api/worksheets/{id}/checks/{check_id}/suggest");
    let apply_url = format!("/api/worksheets/{id}/checks/{check_id}/apply");

    let (s, v) = app
        .post(
            &suggest_url,
            Some(&t),
            json!({ "action": "replace", "words": ["resilient"] }),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "a key word cannot be replaced: {v}"
    );

    let (s, sugg) = app
        .post(
            &suggest_url,
            Some(&t),
            json!({ "action": "replace", "words": ["abandon", "enormous", "obstacle", "accord"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sugg}");
    assert_eq!(sugg["status"], "shown", "{sugg}");
    // Every sentence 換字 may change, once, with the words to avoid in it; the quoted one is not.
    let shown: Vec<(&str, &str, Value)> = sugg["places"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["place"].as_str().unwrap(),
                p["rewritten"].as_str().unwrap(),
                p["bases"].clone(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                "A 活動 · 作答指示",
                "Do not leave a question you find hard.",
                json!(["abandon"])
            ),
            (
                "A 活動 · 第 1 題",
                "Why did the family leave their huge house?",
                json!(["abandon", "enormous"])
            ),
            (
                "A 活動 · 第 3 題",
                "Left houses can be a huge problem.",
                json!(["abandon", "enormous", "obstacle"])
            ),
            (
                "B 活動 · 作答指示",
                "Talk about a huge change.",
                json!(["enormous"])
            ),
            (
                "B 活動 · 第 4 題",
                "Name two problems you face.",
                json!(["obstacle"])
            ),
            // A phrase is rewritten as a whole: not 「as said to the text」.
            (
                "B 活動 · 第 5 題",
                "What would happen if bees disappeared, based on the text?",
                json!(["accord"])
            ),
        ]
    );
    assert_eq!(suggestion(&sugg, "accord")["replacement"], "based on");
    assert_eq!(
        suggestion(&sugg, "enormous")["places"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let replace_id = sugg["interaction_id"].as_i64().unwrap();

    // What is applied is checked again: a sentence changed on the way, or one that no longer
    // matches the worksheet, is refused.
    let mut bad = rewrites(&sugg, &[]);
    bad[0]["rewritten"] = json!("Do not leave a question you find gigantic.");
    let words = |offered_enormous: usize| {
        json!([
            { "base": "abandon", "action": "replace", "interaction_id": replace_id, "offered": 3 },
            { "base": "enormous", "action": "replace", "interaction_id": replace_id, "offered": offered_enormous },
            { "base": "obstacle", "action": "replace", "interaction_id": replace_id, "offered": 2 },
            { "base": "accord", "action": "replace", "interaction_id": replace_id, "offered": 1 },
            { "base": "resilient", "action": "leave" },
        ])
    };
    let (s, v) = app
        .post(
            &apply_url,
            Some(&t),
            json!({ "base_version_no": version_before, "words": words(3), "rewrites": bad }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v.to_string().contains("第 6 級"), "{v}");
    let mut moved = rewrites(&sugg, &[]);
    moved[0]["original"] = json!("Do not abandon any question you find hard.");
    let (s, _) = app
        .post(
            &apply_url,
            Some(&t),
            json!({ "base_version_no": version_before, "words": words(3), "rewrites": moved }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // The teacher unticks one sentence: that place keeps the word.
    let (s, applied) = app
        .post(
            &apply_url,
            Some(&t),
            json!({ "base_version_no": version_before, "words": words(3),
                    "rewrites": rewrites(&sugg, &["Talk about"]) }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{applied}");
    let state = &applied["worksheet"];
    assert_eq!(applied["version_no"], version_before + 1);
    assert_eq!(
        block(state, "b1")["instructions"],
        "Read the text. Do not leave a question you find hard."
    );
    assert_eq!(
        prompt_of(state, "b1", "i1"),
        "Why did the family leave their huge house?"
    );
    assert_eq!(
        prompt_of(state, "b1", "i3"),
        "Left houses can be a huge problem. Do you agree?"
    );
    assert_eq!(prompt_of(state, "b2", "i4"), "Name two problems you face.");
    assert_eq!(
        prompt_of(state, "b2", "i5"),
        "What would happen if bees disappeared, based on the text?"
    );
    assert_eq!(
        block(state, "b2")["instructions"],
        "Talk about an enormous change.",
        "unticked"
    );
    // Quoted, a key word, a word bank of another version: left as they were.
    assert_eq!(
        prompt_of(state, "b1", "i2"),
        "What does \"abandon\" mean in paragraph 1?"
    );
    assert_eq!(block(state, "b1")["vocabulary"][0]["word"], "resilient");
    assert_eq!(
        block(state, "b1")["scaffolds"][0]["content"],
        "abandoned, house"
    );
    let replaced = applied["replaced"].as_array().unwrap();
    let enormous = replaced.iter().find(|r| r["base"] == "enormous").unwrap();
    assert_eq!(enormous["sentences"], 2);
    assert_eq!(enormous["left"], 1);

    // Analysed again: fewer words above the level.
    let after = &applied["check"];
    assert_eq!(after["version_no"], version_before + 1);
    let bases: Vec<&str> = after["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["base"].as_str().unwrap())
        .collect();
    assert_eq!(bases, ["resilient", "abandon", "enormous"]);
    assert_eq!(
        word(after, "abandon")["replaceable"],
        0,
        "only the quoted one"
    );
    assert_eq!(word(after, "resilient")["kept"], true);

    // One new version, undoable block by block.
    let (_, versions) = app
        .get(&format!("/api/worksheets/{id}/versions"), Some(&t))
        .await;
    assert_eq!(versions[0]["cause"], "vocabulary");
    assert_eq!(state["undoable"], json!(["b1", "b2"]));
    let (s, undone) = app
        .post(
            &format!("/api/worksheets/{id}/blocks/b2/undo"),
            Some(&t),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{undone}");
    assert_eq!(
        prompt_of(&undone, "b2", "i5"),
        "What would happen if bees disappeared, according to the text?"
    );
    assert_eq!(
        prompt_of(&undone, "b1", "i1"),
        "Why did the family leave their huge house?"
    );
    // The check is stale now: applying it again is refused.
    let (s, v) = app
        .post(
            &apply_url,
            Some(&t),
            json!({ "base_version_no": version_before, "words": [{ "base": "resilient", "action": "leave" }] }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");

    // The choices, for the research: all of a word's sentences → adopt, some unticked → revise.
    let decisions: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT ai_interaction_id, decision, revised_content, reason_note FROM teacher_decisions ORDER BY id",
    )
    .fetch_all(&app.db)
    .await
    .unwrap();
    let mine: Vec<&str> = decisions.iter().map(|d| d.1.as_str()).collect();
    assert_eq!(
        mine,
        ["adopt", "revise", "adopt", "adopt", "reject"],
        "{decisions:?}"
    );
    assert!(decisions.iter().all(|d| d.0 == replace_id));
    assert_eq!(
        decisions[1].3.as_deref(),
        Some("詞彙級別：換字 enormous（套用 2／3 句）")
    );
    assert!(decisions[1]
        .2
        .as_deref()
        .unwrap()
        .contains("Why did the family leave their huge house?"));
    assert_eq!(decisions[4].3.as_deref(), Some("學習單：復原上一版"));
    let log: Value = sqlx::query_scalar(
        "SELECT payload FROM usage_logs WHERE event = 'worksheet_vocabulary_applied'",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    let actions: Vec<(&str, &str)> = log["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| (w["base"].as_str().unwrap(), w["action"].as_str().unwrap()))
        .collect();
    assert_eq!(
        actions,
        [
            ("abandon", "replace"),
            ("enormous", "replace"),
            ("obstacle", "replace"),
            ("accord", "replace"),
            ("resilient", "leave")
        ]
    );
}

#[tokio::test]
async fn a_rewrite_with_a_harder_word_or_a_new_sentence_is_rejected_and_reported() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    for (marker, why) in [
        (" FAKE_VOCAB_WHOLE", "改動太多"),
        (" FAKE_VOCAB_OVER", "第 6 級"),
    ] {
        let (id, check) = setup(&app, &t, marker).await;
        let check_id = check["id"].as_i64().unwrap();
        let (s, sugg) = app
            .post(
                &format!("/api/worksheets/{id}/checks/{check_id}/suggest"),
                Some(&t),
                json!({ "action": "replace", "words": ["accord"] }),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{sugg}");
        assert_eq!(sugg["status"], "blocked", "{sugg}");
        let p = place(&sugg, "What would happen");
        assert!(p["rewritten"].is_null(), "{p}");
        assert!(p["problem"].as_str().unwrap().contains(why), "{p}");
        assert!(suggestion(&sugg, "accord")["notes"]
            .to_string()
            .contains("提供字義"));
        // Both calls were made: the retry said what failed.
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ai_interactions WHERE module = 'worksheet_vocab_replace'
               AND input_snapshot->'worksheet'->>'id' = $1",
        )
        .bind(id.to_string())
        .fetch_one(&app.db)
        .await
        .unwrap();
        assert_eq!(n, 2, "{marker}");
        // And it cannot be sent anyway.
        let sent = json!([{ "field": p["field"], "start": p["start"], "end": p["end"],
            "original": p["original"], "bases": ["accord"],
            "rewritten": if marker.contains("WHOLE") { "The family left the house." }
                         else { "What would happen if bees disappeared FAKE_VOCAB_OVER, gigantic to the text?" } }]);
        let (s, v) = app
            .post(
                &format!("/api/worksheets/{id}/checks/{check_id}/apply"),
                Some(&t),
                json!({ "base_version_no": check["version_no"],
                        "words": [{ "base": "accord", "action": "replace" }], "rewrites": sent }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert!(v.to_string().contains(why), "{v}");
    }
}

#[tokio::test]
async fn glosses_print_in_every_version_chosen_and_in_docx_and_pdf() {
    let app = test_app!();
    let t = app.teacher("T-001", "general").await;
    // The first call leaves out the last word; the retry gives it.
    let (id, check) = setup(&app, &t, " FAKE_VOCAB_MISSING").await;
    let check_id = check["id"].as_i64().unwrap();
    let (s, sugg) = app
        .post(
            &format!("/api/worksheets/{id}/checks/{check_id}/suggest"),
            Some(&t),
            json!({ "action": "gloss", "words": ["abandon", "accord"] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sugg}");
    let first = suggestion(&sugg, "abandon")["interaction_id"]
        .as_i64()
        .unwrap();
    let retry = suggestion(&sugg, "accord")["interaction_id"]
        .as_i64()
        .unwrap();
    assert_ne!(first, retry, "the retry filled the missing word");
    let retry_of: Option<i64> =
        sqlx::query_scalar("SELECT retry_of FROM ai_interactions WHERE id = $1")
            .bind(retry)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(retry_of, Some(first));

    let (s, applied) = app
        .post(
            &format!("/api/worksheets/{id}/checks/{check_id}/apply"),
            Some(&t),
            json!({ "base_version_no": check["version_no"], "gloss_scope": "all", "words": [
                { "base": "abandon", "action": "gloss", "interaction_id": first,
                  "gloss": { "pos": "v.", "meaning_zh": "放棄", "definition_en": "" } },
            ] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{applied}");
    let b1 = block(&applied["worksheet"], "b1");
    assert_eq!(b1["glosses"][0]["levels"], json!(["basic", "standard"]));
    assert_eq!(b1["glosses"][0]["origin"], "ai_edited");

    for (level, audience) in [("basic", "student"), ("standard", "teacher")] {
        let (s, sheet) = app
            .get(
                &format!("/api/worksheets/{id}/sheet?level={level}&audience={audience}"),
                Some(&t),
            )
            .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            sheet["blocks"][0]["glosses"],
            json!([{ "word": "abandon", "pos": "v.", "meaning_zh": "放棄", "definition_en": "" }])
        );
        let docx = app
            .send_raw(
                Method::GET,
                &format!(
                    "/api/worksheets/{id}/export?format=docx&level={level}&audience={audience}"
                ),
                &[("authorization", &format!("Bearer {t}"))],
                Vec::new(),
            )
            .await;
        assert_eq!(docx.status, StatusCode::OK);
        let mut zip = zip_reader(docx.body);
        let xml = read_entry(&mut zip, "word/document.xml");
        assert!(
            xml.contains("Glossary") && xml.contains("(v.) 放棄"),
            "{level}"
        );
        let pdf = app
            .send_raw(
                Method::GET,
                &format!(
                    "/api/worksheets/{id}/export?format=pdf&level={level}&audience={audience}"
                ),
                &[("authorization", &format!("Bearer {t}"))],
                Vec::new(),
            )
            .await;
        if manna_api::worksheet::export::pdf_supports_cjk() {
            assert_eq!(pdf.status, StatusCode::OK);
            let text = pdf_text(&pdf.body);
            assert!(text.contains("Glossary"), "{text}");
            assert!(text.contains("abandon"), "{text}");
        } else {
            // The Chinese meaning needs the CJK font: refused clearly without it.
            assert_eq!(pdf.status, StatusCode::CONFLICT);
        }
    }

    // The edited meaning is a revise, with what the teacher wrote.
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT decision, revised_content FROM teacher_decisions WHERE ai_interaction_id = $1",
    )
    .bind(first)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(row, ("revise".to_string(), Some("放棄".to_string())));

    // Another teacher sees nothing of it.
    let other = app.teacher("T-002", "general").await;
    let (s, _) = app
        .post(
            &format!("/api/worksheets/{id}/checks/{check_id}/suggest"),
            Some(&other),
            json!({ "action": "gloss", "words": ["abandon"] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = app
        .post(
            &format!("/api/worksheets/{id}/checks/{check_id}/suggest"),
            Some(&t),
            json!({ "action": "gloss", "words": ["umbrella"] }),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::CONFLICT,
        "the check is stale after the apply: {v}"
    );
}

fn zip_reader(bytes: Vec<u8>) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()
}

fn read_entry(zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut xml).unwrap();
    xml
}

fn pdf_text(bytes: &[u8]) -> String {
    pdf_extract::extract_text_from_mem(bytes).unwrap_or_default()
}
