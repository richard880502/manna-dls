#[macro_use]
mod common;

use axum::http::StatusCode;
use manna_api::application::worksheets::{CreateOverrides, CreateWorksheet, WorksheetService};
use manna_api::error::AppError;
use manna_api::worksheet::store;
use serde_json::json;

#[tokio::test]
async fn http_and_background_callers_share_worksheet_creation_and_generation_context() {
    let app = test_app!();
    let token = app.teacher("T-SERVICE", "general").await;
    let (_, teacher) = app.get("/api/me", Some(&token)).await;
    let teacher_id = teacher["id"].as_i64().unwrap();
    let body = json!({
        "material": {"title":"Shared lesson", "text":"A teacher opens a book.\n\nThe class discusses the story."},
        "request":"  Use short questions.  "
    });
    let (status, response) = app
        .post("/api/worksheets", Some(&token), body.clone())
        .await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    let http_id = response["id"].as_i64().unwrap();
    let service = WorksheetService::new(&app.db);
    let direct_id = service
        .create(
            teacher_id,
            serde_json::from_value(body).unwrap(),
            CreateOverrides::default(),
        )
        .await
        .unwrap();
    let http = store::owned(&app.db, http_id, teacher_id).await.unwrap();
    let direct = store::owned(&app.db, direct_id, teacher_id).await.unwrap();
    assert_eq!(direct.settings, http.settings);
    assert_eq!(direct.suggested, http.suggested);
    assert_eq!(direct.request, "Use short questions.");
    assert_eq!(direct.request, http.request);
    assert_eq!(direct.material.title, http.material.title);
    assert_eq!(
        direct
            .material
            .paragraphs
            .iter()
            .map(|p| &p.text)
            .collect::<Vec<_>>(),
        http.material
            .paragraphs
            .iter()
            .map(|p| &p.text)
            .collect::<Vec<_>>()
    );
    let direct = service
        .prepare_generation(teacher_id, "general", direct)
        .await
        .unwrap();
    let http = service
        .prepare_generation(teacher_id, "general", http)
        .await
        .unwrap();
    assert_eq!(direct.content, http.content);
    assert_eq!(direct.class_profile, http.class_profile);
    assert_eq!(direct.class_snapshot, http.class_snapshot);
    assert_eq!(direct.unit_id, http.unit_id);
}

#[tokio::test]
async fn direct_creation_keeps_teacher_ownership_checks_without_an_http_handler() {
    let app = test_app!();
    let owner = app.teacher("T-OWNER", "general").await;
    let other = app.teacher("T-OTHER", "general").await;
    let unit = app.create_unit(&owner, "Private unit").await;
    let lesson = app.create_lesson(&owner, unit, "Private lesson").await;
    let (_, teacher) = app.get("/api/me", Some(&other)).await;
    let err = WorksheetService::new(&app.db)
        .create(
            teacher["id"].as_i64().unwrap(),
            CreateWorksheet {
                lesson_id: Some(lesson),
                ..CreateWorksheet::default()
            },
            CreateOverrides::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
}

#[tokio::test]
async fn http_and_background_callers_share_the_paragraph_selection_of_a_library_material() {
    let app = test_app!();
    let token = app.teacher("T-SELECT", "general").await;
    let (_, teacher) = app.get("/api/me", Some(&token)).await;
    let teacher_id = teacher["id"].as_i64().unwrap();
    let text = (1..=6)
        .map(|i| format!("Paragraph number {i} says something about harbors."))
        .collect::<Vec<_>>()
        .join("\n\n");
    let (_, material) = app
        .post(
            "/api/materials",
            Some(&token),
            json!({ "kind": "text", "title": "Harbors", "text": text }),
        )
        .await;
    let ids: Vec<String> = material["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    // Sent out of order: the snapshot keeps the material's order.
    let body = json!({ "material_id": material["id"], "paragraph_ids": [ids[4], ids[1]] });
    let (status, response) = app
        .post("/api/worksheets", Some(&token), body.clone())
        .await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    let service = WorksheetService::new(&app.db);
    let direct_id = service
        .create(
            teacher_id,
            serde_json::from_value(body).unwrap(),
            CreateOverrides::default(),
        )
        .await
        .unwrap();
    let http = store::owned(&app.db, response["id"].as_i64().unwrap(), teacher_id)
        .await
        .unwrap();
    let direct = store::owned(&app.db, direct_id, teacher_id).await.unwrap();
    for ws in [&http, &direct] {
        let kept: Vec<&str> = ws
            .material
            .paragraphs
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(kept, [ids[1].as_str(), ids[4].as_str()]);
        assert_eq!(
            ws.material.source.selected_ids.as_deref(),
            Some(&[ids[1].clone(), ids[4].clone()][..])
        );
        assert_eq!(ws.material.source.total_paragraphs, Some(6));
    }
    // An empty selection is all of it.
    let all = service
        .create(
            teacher_id,
            CreateWorksheet {
                material_id: material["id"].as_i64(),
                paragraph_ids: Some(Vec::new()),
                ..CreateWorksheet::default()
            },
            CreateOverrides::default(),
        )
        .await
        .unwrap();
    let all = store::owned(&app.db, all, teacher_id).await.unwrap();
    assert_eq!(all.material.paragraphs.len(), 6);
    assert!(all.material.source.selected_ids.is_none());
    // Ids that are not paragraphs of the material are refused the same way on both paths.
    let err = service
        .create(
            teacher_id,
            CreateWorksheet {
                material_id: material["id"].as_i64(),
                paragraph_ids: Some(vec!["gone".to_string()]),
                ..CreateWorksheet::default()
            },
            CreateOverrides::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)));
    let (status, _) = app
        .post(
            "/api/worksheets",
            Some(&token),
            json!({ "material_id": material["id"], "paragraph_ids": ["gone"] }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
