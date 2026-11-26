//! Generation from a template (gen2 of docs/WORKSHEET_TEMPLATE_PROBE.md): instead of the four
//! steps (tasks, warm-up, scaffolds, exit ticket), every block of the template is one model
//! call, at most [`MAX_PARALLEL`] at a time, all in one background job (step `template`).
//!
//! Each section becomes one block; a per-paragraph section one block per paragraph of the new
//! material, capped at 16 blocks in all ([`plan_blocks`]). A call is checked
//! ([`evaluate_section`]) and retried once with a changed request (higher temperature, the
//! problems listed) when it ran away, could not be read or failed the checks; a block that
//! still fails keeps a `message` and the others are generated anyway. A section that needs the
//! teacher's own material (a video, another text) never gets an invented one: the prompt
//! forbids it, the checks remove any web address, and the block says so in its `message`.

use std::collections::HashMap;
use std::sync::Mutex;

use futures_util::stream::{self, StreamExt, TryStreamExt};
use serde_json::json;

use super::{
    close, drive, empty_message, expire_stale, mark_held, open_progress, open_run, pictures_step,
    queue_block, queue_run, replace_block, run_step, Run, RunInput, Start, StepCall, StepInfo,
    StepResult, STEP_TEMPLATE,
};
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::progress::Step;
use crate::text::or_blank;
use crate::worksheet::assemble::{self, scaffold_levels};
use crate::worksheet::checks::{
    evaluate_template_section, CleanSupport, Eval, SectionCtx, SupportsCtx, TemplateSection,
};
use crate::worksheet::images::{self, ImageSlot, WithSlots, MAX_SLOTS_PER_BLOCK};
use crate::worksheet::store::{self, Worksheet};
use crate::worksheet::template::store::{self as tpl_store, STATUS_READY};
use crate::worksheet::template::{
    plan_blocks, scaffold_type_of, section_out_schema, title_for, RawSectionOut, WsTemplate,
    WsTemplateSection, MSG_EXTERNAL, SECTION_SCHEMA_NAME,
};
use crate::worksheet::{
    resolve, Block, Content, Item, Level, Material, Origin, Scaffold, ScaffoldType, FMT_SHORT,
    GEN_OBJECTIVES, KIND_GUIDING,
};
use crate::AppState;

pub const KEY_SECTION: &str = "worksheet_template_section";

const SECTION: StepInfo = StepInfo {
    key: KEY_SECTION,
    module: KEY_SECTION,
    scaffold: "SC-WS-TPL-GEN",
    schema_name: SECTION_SCHEMA_NAME,
    phase: "正在依版型產生區塊",
    repair_phase: "有區塊沒有通過檢查，正在重試",
    progress: STEP_TEMPLATE,
    // The blocks are written side by side: the step counts them instead.
    retry_note: false,
};

pub const L_TEMPLATE: &str = "照版型寫各區塊";
pub const L_TEMPLATE_AGAIN: &str = "照版型重寫這一區";

/// Output budget of one block (probe gen2: 2500).
pub const SECTION_MAX_TOKENS: u32 = 2500;
/// Section calls running at once.
pub const MAX_PARALLEL: usize = 3;

/// The template (one version of it) a worksheet is generated from.
pub struct TemplateUse {
    pub id: i64,
    pub title: String,
    pub version_no: i32,
    pub content: WsTemplate,
    /// Pictures OCR found on the teacher's sample (0 when it was not scanned): a hint for the
    /// picture slots until templates record where the sample has pictures.
    pub sample_pictures: i64,
}

/// For a whole generation: the latest version of the worksheet's template, if it has one.
pub(super) async fn for_generation(
    db: &sqlx::PgPool,
    ws: &Worksheet,
) -> AppResult<Option<TemplateUse>> {
    let Some(id) = ws.template_id else {
        return Ok(None);
    };
    let Some(row) = tpl_store::find(db, id, ws.teacher_id).await? else {
        return Ok(None);
    };
    if row.status != STATUS_READY {
        return Err(AppError::Conflict(
            "這份學習單用的版型還沒有準備好；請等它讀取完成，或改成不使用版型。".to_string(),
        ));
    }
    let sample_pictures = images::sample_pictures(db, row.source_material_id).await?;
    Ok(tpl_store::latest_version(db, id)
        .await?
        .map(|(version_no, content)| TemplateUse {
            id,
            title: row.title,
            version_no,
            // A template read before supports gets the boxes its patterns hold.
            content: content.for_generation(),
            sample_pictures,
        }))
}

/// For regenerating one block: the template version the worksheet was generated from and the
/// section the block came from. `None` (the regular steps are used) for a block that did not
/// come from a template section.
pub(super) async fn for_block(
    db: &sqlx::PgPool,
    ws: &Worksheet,
    block: &Block,
) -> AppResult<Option<(TemplateUse, usize)>> {
    let (Some(id), Some(section_id)) = (ws.template_id, block.template_section_id.as_deref())
    else {
        return Ok(None);
    };
    let Some(row) = tpl_store::find(db, id, ws.teacher_id).await? else {
        return Ok(None);
    };
    let pinned = match ws.template_version_no {
        Some(no) => tpl_store::version(db, id, no).await?.map(|c| (no, c)),
        None => None,
    };
    let Some((version_no, content)) = (match pinned {
        Some(v) => Some(v),
        None => tpl_store::latest_version(db, id).await?,
    }) else {
        return Ok(None);
    };
    let content = content.for_generation();
    let Some(index) = content.sections.iter().position(|s| s.id == section_id) else {
        return Ok(None);
    };
    let sample_pictures = images::sample_pictures(db, row.source_material_id).await?;
    Ok(Some((
        TemplateUse {
            id,
            title: row.title,
            version_no,
            content,
            sample_pictures,
        },
        index,
    )))
}

// ---------------------------------------------------------------- one block: one call

/// `[第 n 段] text` paragraphs, the material markers the probe used.
pub(crate) fn marked_passage(material: &Material) -> String {
    material
        .paragraphs
        .iter()
        .enumerate()
        .map(|(i, p)| format!("[第 {} 段] {}", i + 1, p.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

struct SectionRequest<'a> {
    template: &'a WsTemplate,
    section: &'a WsTemplateSection,
    paragraph: Option<usize>,
    /// A per-paragraph section done once over the whole text.
    collapsed: bool,
    block_no: usize,
    block_total: usize,
    finished_titles: &'a [String],
    sample_pictures: i64,
}

/// A checked block and the picture slots the model wrote for it (cleaned, without ids).
type SectionOut = (TemplateSection, Vec<ImageSlot>);

async fn section_call(
    run: &Run,
    job_id: i64,
    r: SectionRequest<'_>,
) -> AppResult<StepResult<SectionOut>> {
    let material = &run.input.worksheet.material;
    let mut target = serde_json::to_value(r.section).map_err(anyhow::Error::from)?;
    if let Some(map) = target.as_object_mut() {
        map.remove("id");
        map.remove("per_paragraph");
    }
    let head = json!({
        "worksheet_kind": r.template.worksheet_kind,
        "instruction_language": r.template.instruction_language,
        "style_notes": r.template.style_notes,
    });
    let paragraph_note = match (r.paragraph, r.collapsed) {
        (Some(p), _) => format!("\n\n這個區塊只針對新教材第 {p} 段。"),
        (None, true) => {
            "\n\n這個區塊在版型裡是逐段重複的；這次對全文只做一次，題目可以涵蓋各段。".to_string()
        }
        (None, false) => String::new(),
    };
    let mut vars: HashMap<&'static str, String> = HashMap::from([
        ("template_head", head.to_string()),
        ("target_section", target.to_string()),
        ("block_no", r.block_no.to_string()),
        ("block_total", r.block_total.to_string()),
        ("paragraph_note", paragraph_note),
        (
            "done_titles",
            if r.finished_titles.is_empty() {
                "（無）".to_string()
            } else {
                r.finished_titles.join("、")
            },
        ),
        (
            "material_title",
            if material.title.trim().is_empty() {
                "未命名教材".to_string()
            } else {
                material.title.trim().to_string()
            },
        ),
        ("paragraph_count", material.paragraphs.len().to_string()),
        ("passage_numbered", marked_passage(material)),
        ("class_profile", or_blank(&run.input.class_profile)),
        ("teacher_notes_for_ai", run.notes_for_ai(true)),
    ]);
    // Pictures on: the model decides per block whether it needs one. Templates do not record
    // where the sample had pictures yet (a later phase); the number OCR counted on a scanned
    // sample is the hint for now. A section that needs the teacher's own material covers the
    // pictures she supplies herself.
    let slots = !run.slot_rules.is_empty();
    if slots {
        vars.insert(
            "image_slot_rules",
            images::rules_with_sample_hint(&run.slot_rules, r.sample_pictures),
        );
    }
    let objectives = run.objective_texts();
    let fake_context = json!({
        "task": KEY_SECTION,
        "image_slots": if slots { MAX_SLOTS_PER_BLOCK } else { 0 },
        "section": target,
        "item_count": r.section.item_count,
        "paragraph": r.paragraph,
        "paragraphs": run.paragraphs,
        "instruction_language": r.template.instruction_language,
        "run_no": run.run_no,
        // For the fake provider's copied-objective outputs only.
        "objectives": objectives,
    });
    let passage = run.paragraphs.join("\n");
    // 圖像組織 is in the section's formats (or it names none, so any format may come).
    let organizers = r.section.answer_formats.is_empty()
        || r.section
            .answer_formats
            .iter()
            .any(|f| f == crate::worksheet::FMT_ORGANIZER);
    let tables = r.section.answer_formats.is_empty()
        || r.section
            .answer_formats
            .iter()
            .any(|f| f == crate::worksheet::FMT_TABLE);
    let ctx = SectionCtx {
        passage: &passage,
        item_count: r.section.item_count.clamp(1, 12) as usize,
        needs_external: r.section.needs_external_material,
        tables_asked: tables,
        english_tables: false,
        objectives: &objectives,
    };
    let mut schema = section_out_schema(ctx.item_count, (organizers, tables), &r.section.supports);
    let legacy = legacy_kind(r.section);
    let supports = SupportsCtx {
        supports: &r.section.supports,
        legacy: (legacy.0, legacy.1.as_str()),
    };
    if slots {
        // Last: the block first, then the pictures it needs.
        images::add_slots_property(&mut schema, None, MAX_SLOTS_PER_BLOCK);
    }
    run_step(
        run,
        &SECTION,
        job_id,
        StepCall {
            vars,
            schema,
            fake_context,
            max_tokens: SECTION_MAX_TOKENS,
        },
        |raw: &WithSlots<RawSectionOut>| {
            let e = evaluate_template_section(&raw.inner, &ctx, &supports);
            // Slots are cleaned, never a reason for a repair call.
            let (slots, _) = images::clean_slots(&raw.image_slots, MAX_SLOTS_PER_BLOCK);
            Eval {
                output: e.output.map(|o| (o, slots)),
                results: e.results,
                problems: e.problems,
                repairs: e.repairs,
            }
        },
    )
    .await
}

// ---------------------------------------------------------------- blocks from outputs

/// The kind of the one scaffold a block gets from a section without support boxes (a template
/// read before supports): the section's first scaffold kind (引導問題 when it names none).
fn legacy_kind(section: &WsTemplateSection) -> (ScaffoldType, String) {
    section
        .scaffolds
        .iter()
        .find_map(|k| scaffold_type_of(k).map(|t| (t, k.clone())))
        .unwrap_or((ScaffoldType::Verbal, KIND_GUIDING.to_string()))
}

/// The block's support boxes as scaffolds, in print order, each under its heading (the sample's;
/// none for a template read before supports, which prints the kind's own heading). Procedural
/// boxes keep their lines as steps; the others as lines of text.
fn template_scaffolds(
    block_id: &str,
    supports: &[CleanSupport],
    levels: &[Level],
) -> Vec<Scaffold> {
    supports
        .iter()
        .filter(|s| !s.lines.is_empty())
        .enumerate()
        .map(|(i, s)| {
            let (content, steps) = if s.kind_type == ScaffoldType::Procedural {
                (String::new(), s.lines.iter().take(12).cloned().collect())
            } else {
                (s.lines.join("\n"), Vec::new())
            };
            let shown = scaffold_levels(s.kind_type, &s.kind, levels, &[s.kind.as_str()])
                .unwrap_or_else(|| levels.to_vec());
            Scaffold {
                id: format!("{block_id}-s{}", i + 1),
                kind_type: s.kind_type,
                kind: s.kind.clone(),
                content,
                steps,
                levels: shown,
                auto: false,
                origin: Origin::Ai,
                heading: s.heading.clone(),
                bank: None,
            }
        })
        .collect()
}

fn paragraph_ids(material: &Material, paragraph: Option<usize>) -> Vec<String> {
    paragraph
        .and_then(|p| material.id_of(p as i64))
        .map(|id| vec![id.to_string()])
        .unwrap_or_default()
}

fn section_block(
    id: &str,
    section: &WsTemplateSection,
    paragraph: Option<usize>,
    (out, slots): &SectionOut,
    material: &Material,
    levels: &[Level],
    language: &str,
) -> Block {
    let supports = &out.supports;
    let out = &out.section;
    let title = if out.title.trim().is_empty() {
        &section.title_pattern
    } else {
        &out.title
    };
    let items = out
        .items
        .iter()
        .enumerate()
        .map(|(k, it)| Item {
            id: format!("{id}-i{}", k + 1),
            answer_format: Some(it.answer_format.clone()),
            prompt: it.prompt.clone(),
            options: it.options.clone(),
            answer: it.answer.clone(),
            evidence: it.evidence.clone().unwrap_or_default(),
            purpose: None,
            levels: levels.to_vec(),
            origin: Origin::Ai,
            bloom: it.bloom,
            organizer: it.organizer.clone(),
            table: it.table.as_ref().map(|t| t.to_item_table()),
            ..Item::default()
        })
        .collect();
    Block {
        id: id.to_string(),
        kind: section.block_kind,
        title: title_for(title, paragraph, language),
        instructions: out.instructions.clone(),
        paragraph_ids: paragraph_ids(material, paragraph),
        student_tasks: section.student_tasks.clone(),
        items,
        scaffolds: template_scaffolds(id, supports, levels),
        teacher_notes: section.purpose.clone(),
        origin: Origin::Ai,
        message: section
            .needs_external_material
            .then(|| MSG_EXTERNAL.to_string()),
        template_section_id: Some(section.id.clone()),
        image_slots: images::with_ids(slots.clone(), id),
        ..Block::default()
    }
}

/// A block whose call did not deliver: it says why and can be written by hand or regenerated.
/// A block that needs the teacher's material gets an empty item to fill in instead.
fn failed_block(
    id: &str,
    section: &WsTemplateSection,
    paragraph: Option<usize>,
    reason: &str,
    material: &Material,
    levels: &[Level],
    language: &str,
) -> Block {
    let external = section.needs_external_material;
    Block {
        id: id.to_string(),
        kind: section.block_kind,
        title: title_for(&section.title_pattern, paragraph, language),
        paragraph_ids: paragraph_ids(material, paragraph),
        student_tasks: section.student_tasks.clone(),
        items: if external {
            vec![Item {
                id: format!("{id}-i1"),
                answer_format: Some(
                    section
                        .answer_formats
                        .first()
                        .cloned()
                        .unwrap_or_else(|| FMT_SHORT.to_string()),
                ),
                prompt: "（放上你提供的素材後，在這裡寫題目。）".to_string(),
                levels: levels.to_vec(),
                origin: Origin::Teacher,
                ..Item::default()
            }]
        } else {
            Vec::new()
        },
        teacher_notes: section.purpose.clone(),
        origin: Origin::Teacher,
        message: Some(if external {
            MSG_EXTERNAL.to_string()
        } else {
            empty_message("這一區", reason)
        }),
        template_section_id: Some(section.id.clone()),
        ..Block::default()
    }
}

// ---------------------------------------------------------------- a whole generation

/// Creates the job of a template generation (or joins the run in progress) and starts it.
pub(super) async fn start_generation(
    state: &AppState,
    input: RunInput,
    tpl: TemplateUse,
) -> AppResult<()> {
    let worksheet_id = input.worksheet.id;
    expire_stale(state, worksheet_id).await?;
    let queue = queue_run(state, &input);
    let how = if queue.is_some() {
        Start::Queued
    } else {
        Start::Spawn
    };
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &[STEP_TEMPLATE],
        None,
        queue.as_ref(),
    )
    .await?
    else {
        return Ok(());
    };
    begin_generation(state, input, tpl, run_no, ids, how).await;
    Ok(())
}

/// The progress and the work of the template generation of run `run_no`.
pub(super) async fn begin_generation(
    state: &AppState,
    input: RunInput,
    tpl: TemplateUse,
    run_no: i32,
    ids: HashMap<&'static str, i64>,
    how: Start,
) {
    let worksheet_id = input.worksheet.id;
    let job_id = ids[STEP_TEMPLATE];
    let steps = [
        Some(Step::new(STEP_TEMPLATE, L_TEMPLATE)),
        pictures_step(state),
    ];
    let tracker = open_progress(state, worksheet_id, run_no, steps.into_iter().flatten()).await;
    let state2 = state.clone();
    let progress = tracker.clone();
    let work = async move {
        let run = Run::new(&state2, input, run_no, Some(job_id), progress).await?;
        generate(&run, job_id, &tpl).await
    };
    drive(how, state, worksheet_id, run_no, tracker, work).await;
}

type BlockOutcome = Result<i64, (&'static str, String)>;

#[allow(clippy::too_many_arguments)]
async fn make_block(
    run: &Run,
    job_id: i64,
    tpl: &TemplateUse,
    target: crate::worksheet::template::Target,
    j: usize,
    total: usize,
    levels: &[Level],
    done: &Mutex<Vec<(usize, String)>>,
) -> AppResult<(Block, BlockOutcome)> {
    let section = &tpl.content.sections[target.section];
    let material = &run.input.worksheet.material;
    let language = &tpl.content.instruction_language;
    let id = format!("r{}b{}", run.run_no, j + 1);
    let finished: Vec<String> = {
        let mut list = done.lock().map(|d| d.clone()).unwrap_or_default();
        list.sort();
        list.into_iter().map(|(_, t)| t).collect()
    };
    let result = section_call(
        run,
        job_id,
        SectionRequest {
            template: &tpl.content,
            section,
            paragraph: target.paragraph,
            collapsed: target.collapsed,
            block_no: j + 1,
            block_total: total,
            finished_titles: &finished,
            sample_pictures: tpl.sample_pictures,
        },
    )
    .await?;
    run.progress.bump(STEP_TEMPLATE).await;
    Ok(match result {
        StepResult::Shown {
            interaction_id,
            output,
        } => {
            let block = section_block(
                &id,
                section,
                target.paragraph,
                &output,
                material,
                levels,
                language,
            );
            if let Ok(mut d) = done.lock() {
                d.push((j, block.title.clone()));
            }
            (block, Ok(interaction_id))
        }
        StepResult::Failed {
            outcome, reason, ..
        } => (
            failed_block(
                &id,
                section,
                target.paragraph,
                &reason,
                material,
                levels,
                language,
            ),
            Err((outcome, reason)),
        ),
    })
}

async fn generate(run: &Run, job_id: i64, tpl: &TemplateUse) -> AppResult<()> {
    let db = run.db();
    let ws = &run.input.worksheet;
    let material = &ws.material;
    // The worksheet records the version it was generated from (single-block regeneration
    // uses the same one).
    sqlx::query("UPDATE worksheets SET template_version_no = $2 WHERE id = $1")
        .bind(ws.id)
        .bind(tpl.version_no)
        .execute(db)
        .await?;
    let mut chosen = resolve(
        &ws.settings,
        material,
        !run.objectives.is_empty(),
        &run.input.class_profile,
        run.hard_share,
    );
    chosen.reasons.insert(0, format!("依版型：{}", tpl.title));
    let (targets, notices) = plan_blocks(&tpl.content, material.paragraphs.len());
    let levels = chosen.settings.levels.clone();
    let total = targets.len();
    let budget: usize = targets
        .iter()
        .map(|t| tpl.content.sections[t.section].item_count.max(0) as usize)
        .sum();
    chosen.item_budget = budget;
    run.progress.count(STEP_TEMPLATE, 0, total).await;
    let done: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());
    let levels = &levels;
    let done = &done;
    let made: Vec<(Block, BlockOutcome)> = stream::iter(targets.into_iter().enumerate())
        .map(move |(j, target)| make_block(run, job_id, tpl, target, j, total, levels, done))
        .buffered(MAX_PARALLEL)
        .try_collect()
        .await?;

    let failures: Vec<&(&'static str, String)> =
        made.iter().filter_map(|(_, r)| r.as_ref().err()).collect();
    let (outcome, message) = match failures.first() {
        None => ("shown", None),
        Some((first, _)) if failures.len() == total => (
            *first,
            Some("所有區塊都沒有產生成功；你可以按「產生」再試一次，或自己寫。".to_string()),
        ),
        Some(_) => (
            "shown",
            Some(format!(
                "{} 個區塊沒有產生成功；可以在那一區按「重新產生」，或自己寫。",
                failures.len()
            )),
        ),
    };
    let context = run.context(&[
        ("版型", format!("{}（第 {} 版）", tpl.title, tpl.version_no)),
        ("要產生的區塊", format!("{total} 個")),
    ]);
    match (failures.len() == total, &message) {
        (true, Some(m)) => run.progress.fail(STEP_TEMPLATE, m).await,
        _ => run.progress.done(STEP_TEMPLATE).await,
    }
    mark_held(db, job_id, outcome, message.as_deref(), None, &context).await?;
    let failed = failures.len();
    let mut blocks: Vec<Block> = made.into_iter().map(|(b, _)| b).collect();
    // One name per idea on the whole sheet (the blocks were written side by side): the
    // template's, then the material's.
    let template_text = serde_json::to_string(&tpl.content).unwrap_or_default();
    assemble::unify_terms(
        &mut blocks,
        &[&template_text, &run.paragraphs.join("\n")],
        true,
    );

    // Objectives are the teacher's: take them as they are now, not as they were at the start.
    let latest = store::latest(db, ws.id).await?;
    let mut content = Content {
        objectives: run.objectives_for(&latest.content.objectives),
        show_objectives: chosen.settings.wants(GEN_OBJECTIVES),
        blocks,
        // T9: the students' codes of the versions in a random order (kept on regenerate).
        student_order: crate::worksheet::tiered::student_order_for(
            levels,
            &latest.content.student_order,
        ),
        levels: levels.clone(),
        // 字詞表的位置是老師選的，重新產生也留著。
        vocabulary_at_end: latest.content.vocabulary_at_end,
        vocabulary_language: latest.content.vocabulary_language,
        chosen: Some(chosen),
        notices,
    };
    content.normalise().map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "worksheet assembled from a template failed validation: {e}"
        ))
    })?;
    let saved = store::save_content(db, ws.id, &content, "generate").await?;
    store::set_generated(db, ws.id).await?;
    // The blocks passed their checks and are saved: their pictures are made in the background.
    super::queue_pictures(run, &content).await;
    log_usage(
        db,
        ws.teacher_id,
        "worksheet_generated",
        json!({
            "worksheet_id": ws.id, "run_no": run.run_no, "version_no": saved.version_no,
            "blocks": content.blocks.len(), "levels": content.levels,
            "template_id": tpl.id, "template_version_no": tpl.version_no,
            "failed_blocks": failed,
        }),
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------- one block again

/// Creates the job for regenerating one template block (or joins the run in progress).
pub(super) async fn start_block(
    state: &AppState,
    input: RunInput,
    old: Block,
    tpl: TemplateUse,
    section: usize,
) -> AppResult<()> {
    let worksheet_id = input.worksheet.id;
    expire_stale(state, worksheet_id).await?;
    let queue = queue_block(state, &input);
    let how = if queue.is_some() {
        Start::Queued
    } else {
        Start::Spawn
    };
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &[STEP_TEMPLATE],
        Some(&old.id),
        queue.as_ref(),
    )
    .await?
    else {
        return Ok(());
    };
    begin_block(state, input, old, tpl, section, run_no, ids, how).await;
    Ok(())
}

/// The progress and the work of regenerating one template block of run `run_no`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn begin_block(
    state: &AppState,
    input: RunInput,
    old: Block,
    tpl: TemplateUse,
    section: usize,
    run_no: i32,
    ids: HashMap<&'static str, i64>,
    how: Start,
) {
    let worksheet_id = input.worksheet.id;
    let job_id = ids[STEP_TEMPLATE];
    let steps = [
        Some(Step::new(STEP_TEMPLATE, L_TEMPLATE_AGAIN)),
        pictures_step(state),
    ];
    let tracker = open_progress(state, worksheet_id, run_no, steps.into_iter().flatten()).await;
    let state2 = state.clone();
    let progress = tracker.clone();
    let work = async move {
        let run = Run::new(&state2, input, run_no, Some(job_id), progress).await?;
        regenerate(&run, job_id, old, &tpl, section).await
    };
    drive(how, state, worksheet_id, run_no, tracker, work).await;
}

async fn regenerate(
    run: &Run,
    job_id: i64,
    old: Block,
    tpl: &TemplateUse,
    index: usize,
) -> AppResult<()> {
    let section = &tpl.content.sections[index];
    let material = &run.input.worksheet.material;
    let content = &run.input.content;
    let paragraph = if section.per_paragraph {
        old.paragraph_ids.first().and_then(|id| material.no_of(id))
    } else {
        None
    };
    let levels: Vec<Level> = if !content.levels.is_empty() {
        content.levels.clone()
    } else if let Some(c) = content
        .chosen
        .as_ref()
        .filter(|c| !c.settings.levels.is_empty())
    {
        c.settings.levels.clone()
    } else {
        vec![Level::Standard]
    };
    let block_no = content
        .blocks
        .iter()
        .position(|b| b.id == old.id)
        .map_or(1, |p| p + 1);
    // The other blocks that have content, so the new one does not repeat them.
    let finished: Vec<String> = content
        .blocks
        .iter()
        .filter(|b| b.id != old.id && !b.items.is_empty())
        .map(|b| b.title.clone())
        .collect();
    let result = section_call(
        run,
        job_id,
        SectionRequest {
            template: &tpl.content,
            section,
            paragraph,
            collapsed: section.per_paragraph && paragraph.is_none(),
            block_no,
            block_total: content.blocks.len().max(block_no),
            finished_titles: &finished,
            sample_pictures: tpl.sample_pictures,
        },
    )
    .await?;
    let context = run.context(&[("版型", format!("{}（第 {} 版）", tpl.title, tpl.version_no))]);
    let Ok((out, interaction_id)) = close(run, job_id, STEP_TEMPLATE, result, &context).await?
    else {
        return Ok(());
    };
    let block = section_block(
        &old.id,
        section,
        paragraph,
        &out,
        material,
        &levels,
        &tpl.content.instruction_language,
    );
    // The name the rest of the sheet uses for an idea.
    let others: Vec<Block> = content
        .blocks
        .iter()
        .filter(|b| b.id != old.id)
        .cloned()
        .collect();
    let mut one = [block];
    assemble::unify_terms(&mut one, &[&assemble::blocks_text(&others)], false);
    let [block] = one;
    replace_block(run, &old, block, interaction_id, Vec::new()).await
}
