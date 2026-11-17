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
    close, empty_message, expire_stale, mark_held, open_run, replace_block, run_step, spawn_run,
    Run, RunInput, StepCall, StepInfo, StepResult, STEP_TEMPLATE,
};
use crate::error::{AppError, AppResult};
use crate::routes::{log_usage, or_blank};
use crate::worksheet::assemble::scaffold_levels;
use crate::worksheet::checks::{evaluate_section, CleanSection, Eval, SectionCtx};
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
};

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
            content,
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
type SectionOut = (CleanSection, Vec<ImageSlot>);

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
    let fake_context = json!({
        "task": KEY_SECTION,
        "image_slots": if slots { MAX_SLOTS_PER_BLOCK } else { 0 },
        "section": target,
        "item_count": r.section.item_count,
        "paragraph": r.paragraph,
        "paragraphs": run.paragraphs,
        "instruction_language": r.template.instruction_language,
        "run_no": run.run_no,
    });
    let passage = run.paragraphs.join("\n");
    let ctx = SectionCtx {
        passage: &passage,
        item_count: r.section.item_count.clamp(1, 12) as usize,
        needs_external: r.section.needs_external_material,
    };
    let mut schema = section_out_schema(ctx.item_count);
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
            let e = evaluate_section(&raw.inner, &ctx);
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

/// The section's scaffold text as one scaffold of the section's first scaffold kind (引導問題
/// when it names none). Procedural kinds keep their lines as steps.
fn template_scaffold(
    block_id: &str,
    section: &WsTemplateSection,
    text: &str,
    levels: &[Level],
) -> Option<Scaffold> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (kind_type, kind) = section
        .scaffolds
        .iter()
        .find_map(|k| scaffold_type_of(k).map(|t| (t, k.clone())))
        .unwrap_or((ScaffoldType::Verbal, KIND_GUIDING.to_string()));
    let (content, steps) = if kind_type == ScaffoldType::Procedural {
        (
            String::new(),
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .take(12)
                .collect(),
        )
    } else {
        (text.to_string(), Vec::new())
    };
    let shown = scaffold_levels(kind_type, &kind, levels, &[kind.as_str()])
        .unwrap_or_else(|| levels.to_vec());
    Some(Scaffold {
        id: format!("{block_id}-s1"),
        kind_type,
        kind,
        content,
        steps,
        levels: shown,
        auto: false,
        origin: Origin::Ai,
    })
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
        scaffolds: template_scaffold(id, section, &out.scaffold, levels)
            .into_iter()
            .collect(),
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
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &[STEP_TEMPLATE],
        None,
    )
    .await?
    else {
        return Ok(());
    };
    let job_id = ids[STEP_TEMPLATE];
    let state2 = state.clone();
    spawn_run(state, worksheet_id, run_no, async move {
        let run = Run::new(&state2, input, run_no, Some(job_id)).await?;
        generate(&run, job_id, &tpl).await
    });
    Ok(())
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
    mark_held(db, job_id, outcome, message.as_deref(), None, &context).await?;
    let failed = failures.len();
    let blocks: Vec<Block> = made.into_iter().map(|(b, _)| b).collect();

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
    images::enqueue_logged(&run.state, ws.id, ws.teacher_id, &content).await;
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
    let Some((run_no, ids)) = open_run(
        &state.db,
        worksheet_id,
        input.worksheet.teacher_id,
        &[STEP_TEMPLATE],
        Some(&old.id),
    )
    .await?
    else {
        return Ok(());
    };
    let job_id = ids[STEP_TEMPLATE];
    let state2 = state.clone();
    spawn_run(state, worksheet_id, run_no, async move {
        let run = Run::new(&state2, input, run_no, Some(job_id)).await?;
        regenerate(&run, job_id, old, &tpl, section).await
    });
    Ok(())
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
    let Ok((out, interaction_id)) = close(run, job_id, result, &context).await? else {
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
    replace_block(run, &old, block, interaction_id, Vec::new()).await
}
