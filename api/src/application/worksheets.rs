//! Worksheet application operations shared by HTTP endpoints and prep jobs.
//!
//! Creation and generation input accept plain commands and a database handle. Editing
//! and synchronization commands live in submodules. None depend on HTTP extractors or views.

pub mod editing;
pub mod synchronization;

use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::materials::store::{self as material_store, LinkTarget};
use crate::sop::{self, store as sop_store, MAX_MATERIAL_CHARS};
use crate::teaching_context::{owned_class, owned_lesson, resolved_profile};
use crate::worksheet::ai::RunInput;
use crate::worksheet::material::{MSG_BAD_SELECTION, SOURCE_LIBRARY, SOURCE_PASTED};
use crate::worksheet::store;
use crate::worksheet::template::extract::{MSG_MATERIAL_NOT_FOUND, MSG_NO_TEXT};
use crate::worksheet::template::prefill_settings;
use crate::worksheet::template::store as tpl_store;
use crate::worksheet::{
    auto_filename, auto_title, suggest_direct, suggest_from_sop, Brief, Content, Material,
    Objective, Origin, Output, SeedExit, SeedTask, Settings, SettingsPatch, GEN_OBJECTIVES,
    MAX_TITLE_CHARS,
};

/// Most characters of 「教師的要求」.
pub const MAX_REQUEST_CHARS: usize = 2000;
pub const MSG_TEMPLATE_ONLY_WORKSHEET: &str = "版型只適用於完整學習單。";

/// The teacher's request, trimmed; too long is a 400.
pub fn checked_request(request: &str) -> AppResult<String> {
    let request = request.trim();
    if request.chars().count() > MAX_REQUEST_CHARS {
        return Err(AppError::bad_request(format!(
            "給 Manna 的要求最多 {MAX_REQUEST_CHARS} 字。"
        )));
    }
    Ok(request.to_string())
}

/// A template only makes the full worksheet.
pub(crate) fn check_template_output(has_template: bool, settings: &Settings) -> AppResult<()> {
    if has_template && settings.output != Output::Worksheet {
        return Err(AppError::bad_request(MSG_TEMPLATE_ONLY_WORKSHEET));
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
pub struct PastedMaterial {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub source_note: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CreateWorksheet {
    #[serde(default)]
    pub lesson_id: Option<i64>,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub material: Option<PastedMaterial>,
    /// One of the teacher's library materials with extracted text; wins over `material`.
    #[serde(default)]
    pub material_id: Option<i64>,
    /// 只選要用的段落: with `material_id`, the snapshot keeps only these paragraphs (ids of the
    /// library material). Empty or absent = all.
    #[serde(default)]
    pub paragraph_ids: Option<Vec<String>>,
    /// One of the teacher's `ready` templates.
    #[serde(default)]
    pub template_id: Option<i64>,
    /// A partial `WorksheetSettings` applied over the suggested (and template) settings: e.g.
    /// `{ output: "comprehension", bloom: [...], question_count: 8 }`.
    #[serde(default)]
    pub settings: Option<SettingsPatch>,
    /// 「教師的要求」 (≤2000), sent to every model call of the worksheet.
    #[serde(default)]
    pub request: Option<String>,
}

/// A library material the teacher chose (her own, or a platform one from 教材參考), as the
/// worksheet's material snapshot: deleting the material later does not change the worksheet.
async fn chosen_material(
    db: &sqlx::PgPool,
    teacher_id: i64,
    material_id: i64,
    paragraph_ids: Option<&[String]>,
) -> AppResult<(Material, i32)> {
    let row = material_store::readable(db, material_id, teacher_id)
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    crate::materials::ocr::check_idle(&row)?;
    if row.text_status != "ready" || row.paragraphs.0.is_empty() {
        return Err(AppError::bad_request(MSG_NO_TEXT));
    }
    let stored = sop_store::Material::from(row);
    match paragraph_ids.filter(|ids| !ids.is_empty()) {
        Some(ids) => Material::from_library_selection(SOURCE_LIBRARY, None, &stored, ids)
            .ok_or_else(|| AppError::bad_request(MSG_BAD_SELECTION)),
        None => Ok(Material::from_library(SOURCE_LIBRARY, None, &stored)),
    }
}

pub(crate) fn check_pasted(m: &PastedMaterial) -> AppResult<()> {
    if m.text.chars().count() > MAX_MATERIAL_CHARS {
        return Err(AppError::bad_request(format!(
            "教材最多 {MAX_MATERIAL_CHARS} 字。"
        )));
    }
    if m.title.as_deref().unwrap_or_default().chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::bad_request(format!(
            "教材標題最多 {MAX_TITLE_CHARS} 字。"
        )));
    }
    Ok(())
}

/// Pasted text goes into the teacher's material library (so it can be annotated and reused) and
/// the worksheet keeps a snapshot of it. Blank text stays a blank, library-less material.
pub(crate) async fn library_material(
    db: &sqlx::PgPool,
    teacher_id: i64,
    previous: Option<&Material>,
    previous_seq: i32,
    pasted: &PastedMaterial,
) -> AppResult<(Material, i32)> {
    let title = pasted.title.as_deref().unwrap_or_default().trim();
    let note = pasted.source_note.as_deref().unwrap_or_default().trim();
    if pasted.text.trim().is_empty() {
        return Ok(Material::pasted(
            previous,
            previous_seq,
            title,
            &pasted.text,
            note,
        ));
    }
    let row = material_store::create_text(db, teacher_id, title, note, &pasted.text).await?;
    let stored = sop_store::material(db, Some(row.id))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("material {} vanished", row.id)))?;
    Ok(Material::from_library(SOURCE_PASTED, None, &stored))
}

/// What a lesson's SOP carries over: objectives into the document, the rest into the brief.
fn prefill_from_sop(content: &sop::SopContent) -> (Vec<Objective>, Brief) {
    let objectives = content
        .objectives
        .iter()
        .filter(|o| !o.text.trim().is_empty() || !o.student_facing_en.trim().is_empty())
        .map(|o| Objective {
            id: o.id.clone(),
            kind: o.kind,
            text: o.text.clone(),
            student_facing_en: o.student_facing_en.clone(),
            origin: Origin::Teacher,
            sop_id: Some(o.id.clone()),
        })
        .collect();
    let tasks = content
        .task_blocks
        .iter()
        .flat_map(|b| {
            b.tasks
                .iter()
                .filter(|t| !t.text.trim().is_empty())
                .map(move |t| SeedTask {
                    id: format!("{}-{}", b.id, t.id).chars().take(40).collect(),
                    paragraph_ids: b.paragraph_ids.clone(),
                    function: t.function,
                    text: t.text.clone(),
                    sop_block_id: Some(b.id.clone()),
                    sop_task_id: Some(t.id.clone()),
                })
        })
        .collect();
    let brief = Brief {
        tasks,
        warmup: content
            .warmup
            .background
            .iter()
            .map(|a| a.text.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        vocabulary: content
            .warmup
            .vocabulary
            .iter()
            .map(|v| v.word.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        exit_ticket: content
            .exit_ticket
            .items
            .iter()
            .filter(|i| !i.text.trim().is_empty())
            .map(|i| SeedExit {
                kind: i.kind,
                text: i.text.clone(),
            })
            .collect(),
    };
    (objectives, brief)
}

/// What a worksheet made somewhere else (a 備課對話 turn) changes in the usual creation.
#[derive(Debug, Default)]
pub struct CreateOverrides {
    pub title: Option<String>,
    /// Replaces the suggested settings (and wins over a template's).
    pub settings: Option<Settings>,
    /// Warm-up ideas added to the brief.
    pub warmup: Vec<String>,
    pub prep_session_id: Option<i64>,
}

/// Application service for creating a worksheet and preparing its generation input.
pub struct WorksheetService<'a> {
    db: &'a PgPool,
}

impl<'a> WorksheetService<'a> {
    pub fn new(db: &'a PgPool) -> Self {
        Self { db }
    }

    /// Creates a worksheet as `POST /api/worksheets` does and returns its id.
    pub async fn create(
        &self,
        teacher_id: i64,
        req: CreateWorksheet,
        overrides: CreateOverrides,
    ) -> AppResult<i64> {
        let db = self.db;
        if let Some(class_id) = req.class_id {
            owned_class(db, class_id, teacher_id).await?;
        }
        let template = match req.template_id {
            Some(template_id) => Some(tpl_store::ready_for_use(db, teacher_id, template_id).await?),
            None => None,
        };
        let request = checked_request(req.request.as_deref().unwrap_or_default())?;
        let pasted = req.material.unwrap_or_default();
        check_pasted(&pasted)?;
        // A library material wins over pasted text (which is then not stored).
        let pasted_material = match req.material_id {
            Some(material_id) => {
                chosen_material(db, teacher_id, material_id, req.paragraph_ids.as_deref()).await?
            }
            None => library_material(db, teacher_id, None, 0, &pasted).await?,
        };

        let (material, seq, settings, objectives, brief, sop_version_id, lesson_title) = match req
            .lesson_id
        {
            Some(lesson_id) => {
                let lesson = owned_lesson(db, lesson_id, teacher_id).await?;
                let version = sop_store::latest(db, lesson_id).await?;
                let sop_material =
                    sop_store::material(db, version.as_ref().and_then(|v| v.material_id)).await?;
                let (material, seq) = match &sop_material {
                    Some(m) if !m.paragraphs.is_empty() => Material::from_lesson_sop(lesson_id, m),
                    _ => pasted_material,
                };
                let sop_content = version
                    .as_ref()
                    .map(|v| v.content.clone())
                    .unwrap_or_default();
                let (objectives, brief) = prefill_from_sop(&sop_content);
                let settings = suggest_from_sop(&material, &sop_content);
                (
                    material,
                    seq,
                    settings,
                    objectives,
                    brief,
                    version.map(|v| v.id),
                    Some(lesson.title),
                )
            }
            None => {
                let (material, seq) = pasted_material;
                let settings = suggest_direct(&material);
                (
                    material,
                    seq,
                    settings,
                    Vec::new(),
                    Brief::default(),
                    None,
                    None,
                )
            }
        };
        // A template decides the student tasks, answer formats and generation options it implies
        // (the teacher can still change them).
        let settings = match &template {
            Some((_, _, content)) => prefill_settings(&settings, content),
            None => settings,
        };
        let settings = match overrides.settings {
            Some(s) => s.normalised().map_err(AppError::bad_request)?,
            None => settings,
        };
        let settings = match &req.settings {
            Some(patch) => patch
                .apply(&settings)
                .normalised()
                .map_err(AppError::bad_request)?,
            None => settings,
        };
        check_template_output(template.is_some(), &settings)?;
        // G1: the 內容目標 and 語言目標 of the settings are the worksheet's objectives too, after
        // the lesson's; T1: they print at the top of the sheet (unless the request said which
        // generation options, or a template decides).
        let objectives = crate::objectives::synced(&objectives, &settings);
        let mut settings = settings;
        let asked_generation = req
            .settings
            .as_ref()
            .is_some_and(|p| p.generation.is_some());
        if objectives.iter().any(crate::objectives::is_from_settings)
            && !asked_generation
            && template.is_none()
            && !settings.wants(GEN_OBJECTIVES)
        {
            settings.generation.push(GEN_OBJECTIVES.to_string());
            settings = settings.normalised().map_err(AppError::bad_request)?;
        }
        let mut brief = brief;
        brief.warmup.extend(overrides.warmup);
        brief.validate().map_err(AppError::bad_request)?;
        let mut title = auto_title(&material);
        if material.title.trim().is_empty() {
            if let Some(t) = &lesson_title {
                title = format!("{}: Worksheet", t.trim());
            }
        }
        if let Some(t) = overrides.title {
            title = crate::text::clip_chars(t.trim(), MAX_TITLE_CHARS - 1);
        }
        let filename = auto_filename(&material, lesson_title.as_deref());
        let content = Content {
            objectives,
            levels: settings.levels.clone(),
            ..Content::default()
        };
        let id = store::insert(
            db,
            store::NewWorksheet {
                teacher_id,
                lesson_id: req.lesson_id,
                class_id: req.class_id,
                title: &title,
                filename: &filename,
                material: &material,
                paragraph_seq: seq,
                settings: &settings,
                brief: &brief,
                source_sop_version_id: sop_version_id,
                content: &content,
                template: template.as_ref().map(|(row, no, _)| (row.id, *no)),
                prep_session_id: overrides.prep_session_id,
                request: &request,
            },
        )
        .await?;
        if let (Some(material_id), true) = (
            material.source.material_id,
            material.source.kind == SOURCE_PASTED || material.source.kind == SOURCE_LIBRARY,
        ) {
            material_store::link(db, material_id, LinkTarget::Worksheet(id)).await?;
        }
        log_usage(
            db,
            teacher_id,
            "worksheet_created",
            json!({
                "worksheet_id": id, "lesson_id": req.lesson_id, "class_id": req.class_id,
                "material_source": material.source.kind, "paragraphs": material.paragraphs.len(),
                "template_id": req.template_id, "prep_session_id": overrides.prep_session_id,
                "output": settings.output.as_str(),
            }),
        )
        .await?;
        Ok(id)
    }

    /// What a generation of `ws` works with, for the teacher who owns it.
    pub async fn prepare_generation(
        &self,
        teacher_id: i64,
        track: &str,
        worksheet_id: i64,
    ) -> AppResult<RunInput> {
        let db = self.db;
        let ws = store::owned(db, worksheet_id, teacher_id).await?;
        let content = store::latest(db, ws.id).await?.content;
        let lesson = match ws.lesson_id {
            Some(lesson_id) => owned_lesson(db, lesson_id, teacher_id).await.ok(),
            None => None,
        };
        let (description, unit_profile) = lesson
            .as_ref()
            .map(|l| (l.class_description.clone(), l.unit_class_profile.clone()))
            .unwrap_or_default();
        let profile =
            resolved_profile(db, teacher_id, ws.class_id, &description, &unit_profile).await?;
        Ok(RunInput {
            content,
            track: track.to_string(),
            class_profile: profile.text.clone(),
            class_profile_label: profile.context_label(),
            class_snapshot: profile.snapshot(),
            unit_id: lesson.map(|l| l.unit_id),
            worksheet: ws,
        })
    }
}
