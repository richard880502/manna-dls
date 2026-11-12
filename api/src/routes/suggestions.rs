//! RAG lesson-plan suggestions (spec §4.2, §5.1–5.2).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ai::pipeline::{self, AiResult, ContextList, Finish, InteractionMeta, Source};
use crate::ai::schemas::{self, PlanSuggestOutput};
use crate::ai::LlmRequest;
use crate::auth::AuthTeacher;
use crate::checks::{self, RetrievedRef};
use crate::core_questions::CORE_QUESTIONS;
use crate::error::{ApiPath, AppError, AppResult};
use crate::preferences::{self, Hardest};
use crate::prompts;
use crate::retrieval::{self, Scored};
use crate::routes::lessons::{latest_version, linked_focus_ids, selected_siop_ids, LessonStep};
use crate::routes::preferences::{preference_summary, snapshot as pref_snapshot};
use crate::routes::{clip_chars, or_blank, owned_lesson, scaffold_id, track_label};
use crate::AppState;

/// How much of 我的範例 is sent to the model (style reference only).
pub const MY_EXAMPLE_PROMPT_CHARS: usize = 3000;

pub const DIFFERENTIATION_REQUIRED: &str = "這位老師表示「差異化與時間安排」是備課時最花時間或最困難的部分，所以每一項建議都必須填寫：duration_min（這個活動預估需要幾分鐘，1 到 120 的整數；老師會看到「AI 估計，請依班級調整」）、support_note（給程度較弱學生的支援，一到兩句）、extension_note（給程度較好學生的延伸，一到兩句）。";
pub const DIFFERENTIATION_OPTIONAL: &str = "duration_min、support_note、extension_note 可以填 null；若要提供，duration_min 必須是 1 到 120 的整數分鐘。";

#[derive(Debug, Default, Deserialize)]
pub struct SuggestRequest {
    #[serde(default)]
    pub request_note: Option<String>,
}

fn format_step(s: &LessonStep) -> String {
    let mut line = format!(
        "{}. [活動型態：{}｜ICAP：{}",
        s.position,
        or_blank(&s.activity_type),
        s.icap_level.as_deref().unwrap_or("未設定"),
    );
    if let Some(d) = s.duration_min {
        line.push_str(&format!("｜預估 {d} 分鐘"));
    }
    line.push_str(&format!("] {}", s.content));
    if !s.support_note.trim().is_empty() {
        line.push_str(&format!("\n   程度較弱的支援：{}", s.support_note.trim()));
    }
    if !s.extension_note.trim().is_empty() {
        line.push_str(&format!("\n   程度較好的延伸：{}", s.extension_note.trim()));
    }
    line
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
        steps.iter().map(format_step).collect::<Vec<_>>().join("\n")
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

    let prefs = preferences::latest_answers(&state.db, teacher.id()).await?;
    let differentiation = prefs.has_hardest(Hardest::Differentiation);
    let class_profile = lesson.effective_class_profile();
    let my_example = lesson.my_example.trim().to_string();
    let my_example_prompt = if my_example.is_empty() {
        "（沒有提供）".to_string()
    } else {
        clip_chars(&my_example, MY_EXAMPLE_PROMPT_CHARS)
    };

    let mut vars: HashMap<&str, String> = HashMap::from([
        ("school_track", track_label(&track).to_string()),
        ("unit_title", lesson.unit_title.clone()),
        ("lesson_title", lesson.title.clone()),
        // v1 templates name the effective class profile 班級描述.
        ("class_description", or_blank(&class_profile)),
        ("class_profile", or_blank(&class_profile)),
        ("my_example", my_example_prompt),
        ("detail_instruction", prefs.detail_instruction().to_string()),
        (
            "differentiation_instruction",
            if differentiation {
                DIFFERENTIATION_REQUIRED
            } else {
                DIFFERENTIATION_OPTIONAL
            }
            .to_string(),
        ),
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
    let mut required = vec![
        ("class_profile", "班級特性"),
        ("detail_instruction", "建議正文的長度"),
    ];
    if !my_example.is_empty() {
        required.push(("my_example", "老師自己的範例（只作風格參考，不是依據）"));
    }
    if differentiation {
        required.push(("differentiation_instruction", "差異化欄位"));
    }
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    // 我的範例 is deletable by the teacher (D5): the logged copy keeps only a hash.
    vars.insert("my_example", prompts::redacted("我的範例", &my_example));
    let logged_user = prompts::render_user_with(&prompt, &vars, &required);
    let snapshot = json!({
        "inputs": vars,
        "preferences": pref_snapshot(&prefs),
        "retrieval": {
            "query": query_text,
            "top_k": state.config.rag_top_k,
            "items": retrieved.iter().map(|s| json!({
                "item_code": s.item.item_code, "version": s.item.version, "score": s.score
            })).collect::<Vec<_>>(),
        },
        "prompt": {
            "key": prompts::KEY_PLAN_SUGGEST, "version_no": prompt.version_no,
            "system": system, "user": logged_user
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

    let mut context = ContextList::default();
    let first_step = steps
        .first()
        .map(|s| s.content.as_str())
        .unwrap_or_default();
    context
        .add("班級特性", &class_profile)
        .add("本課目標", &objective)
        .add(
            "教案步驟",
            &if steps.is_empty() {
                String::new()
            } else {
                format!("共 {} 步：{}", steps.len(), first_step)
            },
        )
        .add(
            "本課連結的 CoRe 重點",
            &focuses
                .iter()
                .map(|f| f.big_idea.as_str())
                .collect::<Vec<_>>()
                .join("、"),
        )
        .add(
            "SIOP 提示",
            &siop
                .iter()
                .map(|s| s.component.as_str())
                .collect::<Vec<_>>()
                .join("、"),
        )
        .add("我的範例", &my_example)
        .add("補充需求", &request_note)
        .add(
            "你的備課偏好",
            &preference_summary(
                &prefs,
                if differentiation {
                    &["每項建議附預估時間與分層支援"]
                } else {
                    &[]
                },
            ),
        );
    let context = context.into_vec();

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
        return Ok(Json(
            AiResult::shown(first_id, output, Vec::new()).with_context(context),
        ));
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
            "differentiation": differentiation,
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
                let mut results = checks::check_plan(o, &refs, &track);
                results.extend(checks::check_plan_differentiation(o, differentiation));
                (results, checks::cited_codes(o))
            })
        },
    )
    .await?;

    Ok(Json(
        outcome
            .into_result(|out| {
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
            })
            .with_context(context),
    ))
}
