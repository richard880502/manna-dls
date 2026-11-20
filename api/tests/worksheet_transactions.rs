//! Compound worksheet/SOP commands commit both documents and their research event together.
#[macro_use]
mod common;

use manna_api::application::worksheets::{
    synchronization as sync, CreateOverrides, CreateWorksheet, PastedMaterial, WorksheetService,
};
use manna_api::error::AppError;
use manna_api::worksheet::{store, Content};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

struct Fixture {
    teacher: i64,
    unit: i64,
    lesson: i64,
    worksheet: i64,
}

async fn fixture(app: &common::TestApp) -> Fixture {
    let token = app.teacher("T-ATOMIC", "general").await;
    let (_, me) = app.get("/api/me", Some(&token)).await;
    let teacher = me["id"].as_i64().unwrap();
    let unit = app.create_unit(&token, "Atomic unit").await;
    let lesson = app.create_lesson(&token, unit, "Original lesson").await;
    let worksheet = WorksheetService::new(&app.db)
        .create(
            teacher,
            CreateWorksheet {
                material: Some(PastedMaterial {
                    title: Some("A story".into()),
                    text: "A teacher opens a book.".into(),
                    source_note: None,
                }),
                ..CreateWorksheet::default()
            },
            CreateOverrides::default(),
        )
        .await
        .unwrap();
    let content: Content = serde_json::from_value(json!({"objectives":[{
        "id":"o1","type":"content","text":"說明故事","student_facing_en":"I can explain the story."
    }]}))
    .unwrap();
    store::save_content(&app.db, worksheet, &content, "edit")
        .await
        .unwrap();
    sync::link(
        &app.db,
        teacher,
        worksheet,
        sync::LinkLesson {
            lesson_id: Some(lesson),
        },
    )
    .await
    .unwrap();
    Fixture {
        teacher,
        unit,
        lesson,
        worksheet,
    }
}

async fn request(app: &common::TestApp, f: &Fixture) -> Value {
    let preview = sync::sync_preview(&app.db, f.teacher, f.worksheet)
        .await
        .unwrap();
    assert!(!preview.changes.is_empty());
    json!({"confirm":true, "keys":preview.changes.iter().map(|c| c.key.clone()).collect::<Vec<_>>(),
        "sop_version_no":preview.sop_version_no, "worksheet_version_no":preview.worksheet_version_no,
        "worksheet_updated_at":preview.worksheet_updated_at,"lesson_id":preview.lesson_id})
}

async fn counts(app: &common::TestApp) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
        'lessons',(SELECT count(*) FROM lessons),
        'sop_versions',(SELECT count(*) FROM lesson_sop_versions),
        'materials',(SELECT count(*) FROM materials),
        'links',(SELECT count(*) FROM material_links),
        'worksheet_versions',(SELECT count(*) FROM worksheet_versions),
        'usage',(SELECT count(*) FROM usage_logs))",
    )
    .fetch_one(&app.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_failed_worksheet_write_rolls_back_the_sop_and_research_event() {
    let app = test_app!();
    let f = fixture(&app).await;
    let req = request(&app, &f).await;
    let before = counts(&app).await;
    let original = store::latest(&app.db, f.worksheet).await.unwrap().content;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_ws_version() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'injected worksheet version failure'; END $$;
        CREATE TRIGGER reject_ws_version BEFORE INSERT ON worksheet_versions
        FOR EACH ROW EXECUTE FUNCTION reject_ws_version();",
    )
    .execute(&app.db)
    .await
    .unwrap();
    let result = sync::sync_apply(
        &app.db,
        f.teacher,
        f.worksheet,
        serde_json::from_value(req.clone()).unwrap(),
    )
    .await;
    assert!(matches!(result, Err(AppError::Internal(_))));
    assert_eq!(
        counts(&app).await,
        before,
        "SOP and usage event must roll back too"
    );
    assert_eq!(
        store::latest(&app.db, f.worksheet).await.unwrap().content,
        original
    );
    sqlx::query("DROP TRIGGER reject_ws_version ON worksheet_versions")
        .execute(&app.db)
        .await
        .unwrap();
    let done = sync::sync_apply(
        &app.db,
        f.teacher,
        f.worksheet,
        serde_json::from_value(req).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        done["applied"], 1,
        "the unchanged preview can be retried after rollback"
    );
}

#[tokio::test]
async fn save_as_lesson_and_link_roll_back_when_the_research_event_fails() {
    let app = test_app!();
    let f = fixture(&app).await;
    let before = counts(&app).await;
    let original = store::owned(&app.db, f.worksheet, f.teacher).await.unwrap();
    sqlx::raw_sql("CREATE FUNCTION reject_link_event() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.event = 'worksheet_linked' THEN RAISE EXCEPTION 'injected research failure'; END IF; RETURN NEW; END $$;
        CREATE TRIGGER reject_link_event BEFORE INSERT ON usage_logs FOR EACH ROW EXECUTE FUNCTION reject_link_event();")
        .execute(&app.db).await.unwrap();
    let result = sync::save_as_lesson(
        &app.db,
        f.teacher,
        f.worksheet,
        sync::SaveAsLesson {
            unit_id: f.unit,
            title: "Must roll back".into(),
        },
    )
    .await;
    assert!(matches!(result, Err(AppError::Internal(_))));
    assert_eq!(
        counts(&app).await,
        before,
        "no orphan lesson, material, versions or links"
    );
    let after = store::owned(&app.db, f.worksheet, f.teacher).await.unwrap();
    assert_eq!(after.lesson_id, original.lesson_id);
    assert_eq!(after.source_sop_version_id, original.source_sop_version_id);
    let result = sync::link(
        &app.db,
        f.teacher,
        f.worksheet,
        sync::LinkLesson { lesson_id: None },
    )
    .await;
    assert!(matches!(result, Err(AppError::Internal(_))));
    assert_eq!(
        store::owned(&app.db, f.worksheet, f.teacher)
            .await
            .unwrap()
            .lesson_id,
        original.lesson_id
    );
}

#[tokio::test]
async fn worksheet_edits_and_relinks_invalidate_the_preview() {
    let app = test_app!();
    let f = fixture(&app).await;
    let req = request(&app, &f).await;
    // A material edit has no new content version, but must still invalidate this preview.
    let ws = store::owned(&app.db, f.worksheet, f.teacher).await.unwrap();
    let version = store::latest(&app.db, f.worksheet)
        .await
        .unwrap()
        .version_no;
    let mut material = ws.material.clone();
    material.title = "Changed material snapshot".into();
    store::set_material(
        &app.db,
        ws.id,
        &material,
        ws.paragraph_seq,
        &ws.settings,
        &ws.suggested,
        &ws.title,
        &ws.filename,
    )
    .await
    .unwrap();
    assert_eq!(
        store::latest(&app.db, f.worksheet)
            .await
            .unwrap()
            .version_no,
        version
    );
    let before = counts(&app).await;
    let result = sync::sync_apply(
        &app.db,
        f.teacher,
        f.worksheet,
        serde_json::from_value(req).unwrap(),
    )
    .await;
    assert!(matches!(result, Err(AppError::Conflict(_))));
    assert_eq!(counts(&app).await, before);
    let req = request(&app, &f).await;
    let mut content = store::latest(&app.db, f.worksheet).await.unwrap().content;
    content.objectives[0].text = "The teacher changed this after preview".into();
    store::save_content(&app.db, f.worksheet, &content, "edit")
        .await
        .unwrap();
    let before = counts(&app).await;
    let result = sync::sync_apply(
        &app.db,
        f.teacher,
        f.worksheet,
        serde_json::from_value(req.clone()).unwrap(),
    )
    .await;
    assert!(matches!(result, Err(AppError::Conflict(_))));
    assert_eq!(counts(&app).await, before);
    // Another lesson may have the same SOP version number; the lesson id must also match.
    let token = app.teacher("T-ATOMIC", "general").await;
    let other = app.create_lesson(&token, f.unit, "Relinked lesson").await;
    sync::link(
        &app.db,
        f.teacher,
        f.worksheet,
        sync::LinkLesson {
            lesson_id: Some(other),
        },
    )
    .await
    .unwrap();
    let before = counts(&app).await;
    let result = sync::sync_apply(
        &app.db,
        f.teacher,
        f.worksheet,
        serde_json::from_value(req).unwrap(),
    )
    .await;
    assert!(matches!(result, Err(AppError::Conflict(_))));
    assert_eq!(counts(&app).await, before);
}

#[tokio::test]
async fn a_concurrent_sop_save_is_checked_after_the_lesson_lock_and_one_connection_is_enough() {
    let app = test_app!();
    let f = fixture(&app).await;
    let req = request(&app, &f).await;
    let runner = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            app.db
                .connect_options()
                .as_ref()
                .clone()
                .application_name("atomic_sync_runner"),
        )
        .await
        .unwrap();
    let mut editor = app.db.begin().await.unwrap();
    sqlx::query("SELECT id FROM lessons WHERE id=$1 FOR UPDATE")
        .bind(f.lesson)
        .execute(&mut *editor)
        .await
        .unwrap();
    let db = runner.clone();
    let (teacher, worksheet) = (f.teacher, f.worksheet);
    let task = tokio::spawn(async move {
        sync::sync_apply(
            &db,
            teacher,
            worksheet,
            serde_json::from_value(req).unwrap(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity
                WHERE datname=current_database() AND application_name='atomic_sync_runner' AND wait_event_type='Lock')")
                .fetch_one(&app.db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("sync waits for the lesson editor");
    sqlx::query("INSERT INTO lesson_sop_versions(lesson_id,version_no,content) VALUES($1,1,$2)")
        .bind(f.lesson)
        .bind(json!({"objectives":[]}))
        .execute(&mut *editor)
        .await
        .unwrap();
    editor.commit().await.unwrap();
    assert!(
        matches!(task.await.unwrap(), Err(AppError::Conflict(_))),
        "version check must run after acquiring the lock"
    );
    // A fresh operation, including curriculum resolution, finishes with one pool slot.
    let fresh = request(&app, &f).await;
    let done = tokio::time::timeout(
        Duration::from_secs(5),
        sync::sync_apply(
            &runner,
            f.teacher,
            f.worksheet,
            serde_json::from_value(fresh).unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(done["applied"], 1);
    runner.close().await;
}

#[tokio::test]
async fn two_confirmations_of_the_same_preview_cannot_both_write() {
    let app = test_app!();
    let f = fixture(&app).await;
    let req = request(&app, &f).await;
    let (a, b) = tokio::join!(
        sync::sync_apply(
            &app.db,
            f.teacher,
            f.worksheet,
            serde_json::from_value(req.clone()).unwrap()
        ),
        sync::sync_apply(
            &app.db,
            f.teacher,
            f.worksheet,
            serde_json::from_value(req).unwrap()
        )
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a, Err(AppError::Conflict(_))) || matches!(b, Err(AppError::Conflict(_))));
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM usage_logs WHERE event='worksheet_sop_synced'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(events, 1);
}
