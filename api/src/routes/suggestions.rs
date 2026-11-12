//! RAG lesson-plan suggestions (spec §4.2, §5.1–5.2).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ai::pipeline::{self, AiResult, Finish, InteractionMeta, Source};
use crate::ai::schemas::{self, PlanSuggestOutput};
use crate::ai::LlmRequest;
use crate::auth::AuthTeacher;
use crate::checks::{self, RetrievedRef};
use crate::core_questions::CORE_QUESTIONS;
use crate::error::{ApiPath, AppError, AppResult};
use crate::prompts;
use crate::retrieval::{self, Scored};
use crate::routes::lessons::{latest_version, linked_focus_ids, selected_siop_ids};
use crate::routes::{owned_lesson, scaffold_id, track_label};
use crate::AppState;

#[derive(Debug, Default, Deserialize)]
pub struct SuggestRequest {
    #[serde(default)]
    pub request_note: Option<String>,
}

fn or_blank(s: &str) -> String {
    if s.trim().is_empty() {
        "（尚未填寫）".to_string()
    } else {
        s.to_string()
    }
}

fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn format_items(items: &[Scored]) -> String {
    items
        .iter()
        .map(|s| {
            let i = &s.item;
            format!(
                "[{}]（類別：{}｜適用：{}）{}\n{}",
                i.item_code,
                i.category,
                track_label(&i.school_track),
                i.title,
                clip_chars(&i.content, 1200)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(sqlx::FromRow)]
struct FocusInfo {
    id: i64,
    big_idea: String,
}

#[derive(sqlx::FromRow)]
struct SiopInfo {
    component: String,
    summary: String,
}

pub async fn suggest(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    body: Bytes,
) -> AppResult<Json<AiResult<PlanSuggestOutput>>> {
    let req: SuggestRequest = if body.iter().all(u8::is_ascii_whitespace) {
        SuggestRequest::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| AppError::bad_request(format!("請求內容格式不正確：{e}")))?
    };
    let lesson = owned_lesson(&state.db, id, teacher.id()).await?;
    if !state.ai.enabled() {
        return Ok(Json(AiResult::disabled()));
    }
    let prompt = prompts::latest(&state.db, prompts::KEY_PLAN_SUGGEST)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("no prompt version for plan_suggest")))?;
    let track = teacher.track().to_string();
    let request_note = req.request_note.unwrap_or_default();

    // ---- inputs
    let version = latest_version(&state.db, id).await?;
    let objective = version
        .as_ref()
        .map(|v| v.objective.clone())
        .unwrap_or_default();
    let steps = version
        .as_ref()
        .map(|v| v.steps.clone())
        .unwrap_or_default();
    let steps_text = if steps.is_empty() {
        "（尚未填寫）".to_string()
    } else {
        steps
            .iter()
            .map(|s| {
                format!(
                    "{}. [活動型態：{}｜ICAP：{}] {}",
                    s.position,
                    or_blank(&s.activity_type),
                    s.icap_level.as_deref().unwrap_or("未設定"),
                    s.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let focus_ids = linked_focus_ids(&state.db, id).await?;
    let focuses: Vec<FocusInfo> = sqlx::query_as(
        "SELECT id, big_idea FROM core_focuses WHERE id = ANY($1) AND deleted_at IS NULL ORDER BY position, id",
    )
    .bind(&focus_ids)
    .fetch_all(&state.db)
    .await?;
    let answers: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT core_focus_id, question_id, answer FROM core_responses
          WHERE core_focus_id = ANY($1) AND status = 'filled' ORDER BY question_id",
    )
    .bind(&focus_ids)
    .fetch_all(&state.db)
    .await?;
    let core_text = if focuses.is_empty() {
        "（本課沒有連結 CoRe 學習重點）".to_string()
    } else {
        focuses
            .iter()
            .enumerate()
            .map(|(n, f)| {
                let mut s = format!("重點 {}：{}", n + 1, or_blank(&f.big_idea));
                for (fid, qid, answer) in &answers {
                    if *fid == f.id {
                        let prompt = CORE_QUESTIONS
                            .iter()
                            .find(|q| q.id == qid)
                            .map(|q| q.prompt)
                            .unwrap_or_default();
                        s.push_str(&format!("\n  {qid} {prompt}：{}", clip_chars(answer, 400)));
                    }
                }
                s
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let siop_ids = selected_siop_ids(&state.db, id).await?;
    let siop: Vec<SiopInfo> = sqlx::query_as(
        "SELECT component, summary FROM siop_features WHERE id = ANY($1) ORDER BY internal_id",
    )
    .bind(&siop_ids)
    .fetch_all(&state.db)
    .await?;
    let siop_text = if siop.is_empty() {
        "（未選用）".to_string()
    } else {
        siop.iter()
            .map(|s| format!("・{}：{}", s.component, s.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // ---- retrieval (keyword, track-filtered)
    let query_text = [
        lesson.title.as_str(),
        &objective,
        &steps
            .iter()
            .map(|s| format!("{} {}", s.activity_type, s.content))
            .collect::<Vec<_>>()
            .join("\n"),
        &focuses
            .iter()
            .map(|f| f.big_idea.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        &request_note,
    ]
    .join("\n");
    let retrieved =
        retrieval::retrieve(&state.db, &track, &query_text, state.config.rag_top_k).await?;
    let codes: Vec<String> = retrieved.iter().map(|s| s.item.item_code.clone()).collect();

    let vars: HashMap<&str, String> = HashMap::from([
        ("school_track", track_label(&track).to_string()),
        ("unit_title", lesson.unit_title.clone()),
        ("lesson_title", lesson.title.clone()),
        ("class_description", or_blank(&lesson.class_description)),
        ("objective", or_blank(&objective)),
        ("steps", steps_text),
        ("core_focuses", core_text),
        ("siop", siop_text),
        (
            "request_note",
            if request_note.trim().is_empty() {
                "（無）".to_string()
            } else {
                request_note.clone()
            },
        ),
        ("knowledge_items", format_items(&retrieved)),
        ("allowed_codes", codes.join("、")),
    ]);
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render(&prompt.user_template, &vars);
    let snapshot = json!({
        "inputs": vars,
        "retrieval": {
            "query": query_text,
            "top_k": state.config.rag_top_k,
            "items": retrieved.iter().map(|s| json!({
                "item_code": s.item.item_code, "version": s.item.version, "score": s.score
            })).collect::<Vec<_>>(),
        },
        "prompt": {
            "key": prompts::KEY_PLAN_SUGGEST, "version_no": prompt.version_no,
            "system": system, "user": user
        },
        "schema": schemas::PLAN_SUGGEST_SCHEMA_NAME,
        "model_called": !retrieved.is_empty(),
    });
    let meta = InteractionMeta {
        teacher_id: teacher.id(),
        module: "plan_suggest",
        unit_id: Some(lesson.unit_id),
        lesson_id: Some(lesson.id),
        scaffold_id: scaffold_id(&state.db, "SC-RAG").await?,
        prompt_version_id: Some(prompt.id),
    };
    let first_id = pipeline::insert_interaction(
        &state.db,
        &meta,
        state.ai.provider_name(),
        &state.ai.model,
        &snapshot,
        1,
        None,
    )
    .await?;
    sqlx::query(
        "INSERT INTO retrieval_logs (ai_interaction_id, query_text, school_track, item_codes)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(first_id)
    .bind(&query_text)
    .bind(&track)
    .bind(&codes)
    .execute(&state.db)
    .await?;

    // Nothing retrieved: answer 「知識庫查無」 without calling the model (spec §5.1).
    if retrieved.is_empty() {
        let output = PlanSuggestOutput {
            kb_empty: true,
            suggestions: Vec::new(),
        };
        pipeline::finish_interaction(
            &state.db,
            first_id,
            Finish {
                status: "shown",
                output: Some(serde_json::to_value(&output).map_err(anyhow::Error::from)?),
                citations: Vec::new(),
                check_results: Some(json!({
                    "passed": true, "checks": [],
                    "skipped": "retrieval returned no items; model not called"
                })),
                model_version: None,
                error_detail: None,
                latency_ms: 0,
            },
        )
        .await?;
        return Ok(Json(AiResult::shown(first_id, output, Vec::new())));
    }

    let refs: Vec<RetrievedRef> = retrieved
        .iter()
        .map(|s| RetrievedRef {
            item_code: s.item.item_code.clone(),
            school_track: s.item.school_track.clone(),
        })
        .collect();
    let llm_req = LlmRequest {
        system,
        user,
        schema_name: schemas::PLAN_SUGGEST_SCHEMA_NAME,
        schema: schemas::plan_suggest_schema(),
        fake_context: json!({
            "task": "plan_suggest",
            "request_note": request_note,
            "items": retrieved.iter().map(|s| json!({
                "item_code": s.item.item_code, "title": s.item.title
            })).collect::<Vec<_>>(),
        }),
        attempt: 1,
    };
    let outcome = pipeline::run(
        &state,
        &meta,
        first_id,
        &snapshot,
        llm_req,
        |raw: &Value| {
            pipeline::parse_and_check::<PlanSuggestOutput>(raw, |o| {
                (checks::check_plan(o, &refs, &track), checks::cited_codes(o))
            })
        },
    )
    .await?;

    Ok(Json(outcome.into_result(|out| {
        let by_code: HashMap<&str, &Scored> = retrieved
            .iter()
            .map(|s| (s.item.item_code.as_str(), s))
            .collect();
        checks::cited_codes(out)
            .iter()
            .filter_map(|c| by_code.get(c.as_str()))
            .map(|s| Source {
                item_code: s.item.item_code.clone(),
                title: s.item.title.clone(),
                category: s.item.category.clone(),
                source_doc: s.item.source_doc.clone(),
                source_page: s.item.source_page.clone(),
            })
            .collect()
    })))
}
