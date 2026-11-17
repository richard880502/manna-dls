//! 教學套件 export (`GET /api/prep-sessions/:id/turns/:turn_id/kit?format=docx`): one DOCX with
//! 1. 教學流程, the kit's activity plan;
//! 2. the student version of each of the worksheet's levels, exactly as the worksheet's own
//!    export prints them, headed by the neutral code (「學生版（A 版）」: T9, the level's name is
//!    never on a student page);
//! 3. 教師版: for a 差異化學習單, the teacher version of each level (its support, answers and
//!    scoring notes differ per version, T6), headed 「教師版（基礎版＝A 版）」, with the 分層說明;
//!    for one made before the redesign, every question once, numbered through, with its answer,
//!    evidence and Bloom tag.
//!
//! The sheets come from the same projection as the editor and the worksheet export
//! ([`project`]); the document is written by the worksheet's DOCX writer
//! ([`export::combined_docx`]). A part that the kit could not make is replaced by a line that
//! says so.

use super::{ActivityPlan, Goal, GoalKind, Step, OPTION_NAMES_ZH};
use crate::worksheet::assemble::{keep_original_text, project, Sheet};
use crate::worksheet::bloom::Bloom;
use crate::worksheet::export::{self, Line, Part};
use crate::worksheet::tiered::{version_code, version_label};
use crate::worksheet::{Audience, Content, Level, Material};

pub const HEADING_PLAN: &str = "教學流程";
pub const HEADING_TEACHER: &str = "教師版";
const HEADING_WORKSHEET: &str = "差異化學習單";
const NO_PLAN: &str = "這個教學套件的課堂活動沒有完成，所以沒有教學流程。";
const NO_WORKSHEET: &str = "這個教學套件的差異化學習單沒有完成（或已經刪除），所以沒有學習單。";
const TEACHER_BANNER: &str =
    "Teacher reference · all versions · answers, evidence and Bloom levels are not on the student sheets";
/// How a kit made before the 差異化學習單 redesign split its questions (its questions still do).
const LEGACY_TIERED_REASON: &str =
    "基礎版：記憶＋理解＋應用（附提示）；一般版：理解到分析；進階版：理解、應用、分析到創造";

/// The heading of a level's student version: 「學生版（A 版）」 (never the level's name); 「學生版」
/// when the worksheet has one version. `order`: the worksheet's `student_order`.
pub fn student_heading(levels: &[Level], order: &[Level], level: Level) -> String {
    match version_code(levels, order, level) {
        Some(code) => format!("學生版（{}）", version_label(&code)),
        None => "學生版".to_string(),
    }
}

/// The heading of a level's teacher version in a 差異化學習單 kit: 「教師版（基礎版＝A 版）」.
pub fn teacher_heading(levels: &[Level], order: &[Level], level: Level) -> String {
    match version_code(levels, order, level) {
        Some(code) => format!(
            "{HEADING_TEACHER}（{}＝{}）",
            level.label(),
            version_label(&code)
        ),
        None => format!("{HEADING_TEACHER}（{}）", level.label()),
    }
}

/// Whether the worksheet's questions carry their own versions (a 差異化學習單 made after the
/// redesign).
fn differentiated(content: &Content) -> bool {
    content
        .blocks
        .iter()
        .any(|b| b.items.iter().any(|i| !i.tiers.is_empty()))
}

/// 「Bloom：理解、應用」
fn bloom_line(levels: &[Bloom]) -> String {
    format!(
        "Bloom：{}",
        levels
            .iter()
            .map(|b| b.label())
            .collect::<Vec<_>>()
            .join("、")
    )
}

/// 「分組：小組｜技能：讀、說」 (version 4 of the plan; empty before).
fn grouping_line(s: &Step) -> Option<String> {
    let mut parts = Vec::new();
    if !s.grouping.trim().is_empty() {
        parts.push(format!("分組：{}", s.grouping.trim()));
    }
    if !s.skills.is_empty() {
        parts.push(format!("語言技能：{}", s.skills.join("、")));
    }
    (!parts.is_empty()).then(|| parts.join("｜"))
}

fn labelled(out: &mut Vec<Line>, label: &str, text: &str) {
    if !text.trim().is_empty() {
        out.push(Line::Labelled(label.into(), text.trim().to_string()));
    }
}

fn step_lines(no: usize, s: &Step, out: &mut Vec<Line>) {
    out.push(Line::Heading(format!(
        "{no}. {}｜{}（{} 分鐘）",
        s.phase, s.title, s.minutes
    )));
    if let Some(line) = grouping_line(s) {
        out.push(Line::Teacher(line));
    }
    if !s.bloom.is_empty() {
        out.push(Line::Teacher(bloom_line(&s.bloom)));
    }
    labelled(out, "教學思路", &s.idea);
    if !s.roles.is_empty() {
        let roles: Vec<String> = s
            .roles
            .iter()
            .map(|r| format!("{}：{}", r.role, r.task))
            .collect();
        out.push(Line::Labelled("分工".into(), roles.join("；")));
    }
    labelled(out, "教師巡視", &s.monitoring);
    labelled(out, "鷹架", &s.scaffolding);
    for (k, o) in s.options.iter().enumerate() {
        let name = if o.title.trim().is_empty() {
            OPTION_NAMES_ZH.get(k).unwrap_or(&"選項").to_string()
        } else {
            o.title.trim().to_string()
        };
        out.push(Line::Labelled(
            format!("{name}（{} 分鐘）", o.minutes),
            o.idea.clone(),
        ));
        out.extend(o.script.iter().map(|l| Line::Script(l.clone())));
    }
    if !s.script.is_empty() {
        out.push(Line::Labelled("課堂用語".into(), String::new()));
        out.extend(s.script.iter().map(|l| Line::Script(l.clone())));
    }
    if !s.stuck_tip.trim().is_empty() {
        out.push(Line::Labelled("卡住了怎麼辦".into(), s.stuck_tip.clone()));
    }
}

/// Goals numbered from 1: the Chinese line (a 語言目標 led by its kind, 「〔學習策略〕」; a
/// teacher's goal marked 「（教師提供）」) with the curriculum codes it cites, the English line (if
/// any) under it, then how a 語言目標 serves its 內容目標.
fn goal_lines<'a>(goals: impl Iterator<Item = &'a Goal>, out: &mut Vec<Line>) {
    for (i, g) in goals.enumerate() {
        let codes = if g.curriculum_codes.is_empty() {
            String::new()
        } else {
            format!("（課綱：{}）", g.curriculum_codes.join("、"))
        };
        let category = g
            .category
            .map(|c| format!("〔{}〕", c.label()))
            .unwrap_or_default();
        let teacher = if g.from_teacher {
            "（教師提供）"
        } else {
            ""
        };
        let zh = if g.zh.trim().is_empty() { &g.en } else { &g.zh };
        out.push(Line::Numbered(
            i + 1,
            format!("{category}{zh}{teacher}{codes}"),
        ));
        if !g.en.trim().is_empty() && !g.zh.trim().is_empty() {
            out.push(Line::Script(g.en.clone()));
        }
        if !g.relation.trim().is_empty() {
            let serves = g
                .supports
                .map(|n| format!("支持內容目標 {n}："))
                .unwrap_or_default();
            out.push(Line::Teacher(format!("{serves}{}", g.relation.trim())));
        }
    }
}

/// 教學流程: the plan's title, its goals (內容目標 and 語言目標, each in Chinese and English, with
/// the curriculum codes they cite, and each 語言目標's kind and relation; a plan made before has
/// one list in Chinese), 關鍵字彙, 教材與教具, 高層次問題, 背景連結, every step (its 分組,
/// 語言技能, Bloom levels, 分工, 教師巡視, 鷹架), 複習與評量, and the curriculum items cited.
/// What a plan made before version 4 does not have is left out.
pub fn plan_lines(plan: &ActivityPlan) -> Vec<Line> {
    let mut out = Vec::new();
    if !plan.title.trim().is_empty() {
        out.push(Line::Heading(plan.title.clone()));
    }
    let minutes: i64 = plan.steps.iter().map(|s| s.minutes).sum();
    out.push(Line::Muted(format!(
        "{} 個步驟，共約 {minutes} 分鐘",
        plan.steps.len()
    )));
    if !plan.goals.is_empty() {
        out.push(Line::Heading("學習目標".into()));
        goal_lines(plan.goals.iter().filter(|g| g.kind.is_none()), &mut out);
        for kind in GoalKind::ALL {
            let mut group = plan
                .goals
                .iter()
                .filter(|g| g.kind == Some(kind))
                .peekable();
            if group.peek().is_some() {
                out.push(Line::Labelled(kind.label().into(), String::new()));
                goal_lines(group, &mut out);
            }
        }
    }
    if !plan.key_vocabulary.is_empty() {
        out.push(Line::Heading("關鍵字彙".into()));
        out.extend(
            plan.key_vocabulary
                .iter()
                .map(|w| Line::Labelled(w.word.clone(), w.zh.clone())),
        );
    }
    if !plan.materials.is_empty() {
        out.push(Line::Labelled(
            "教材與教具".into(),
            plan.materials.join("、"),
        ));
    }
    if !plan.higher_order_questions.is_empty() {
        out.push(Line::Heading("高層次問題".into()));
        for (i, q) in plan.higher_order_questions.iter().enumerate() {
            out.push(Line::Numbered(
                i + 1,
                format!("{}（{}）", q.question, q.bloom.label()),
            ));
        }
    }
    labelled(&mut out, "背景連結", &plan.background);
    for (i, s) in plan.steps.iter().enumerate() {
        step_lines(i + 1, s, &mut out);
    }
    if !plan.review.trim().is_empty() || !plan.assessment.trim().is_empty() {
        out.push(Line::Heading("複習與評量".into()));
        labelled(&mut out, "複習", &plan.review);
        labelled(&mut out, "評量", &plan.assessment);
    }
    if !plan.curriculum.is_empty() {
        out.push(Line::Heading("對應的課綱條目".into()));
        out.extend(
            plan.curriculum
                .iter()
                .map(|c| Line::Labelled(c.code.clone(), c.text.clone())),
        );
    }
    out
}

/// The worksheet's versions, in order (一般版 when none is set).
pub fn levels_of(content: &Content) -> Vec<Level> {
    if content.levels.is_empty() {
        vec![Level::Standard]
    } else {
        content.levels.clone()
    }
}

/// The teacher version of the kit: every question that appears in at least one of the
/// worksheet's versions, once, numbered through, with its answer, evidence and Bloom tag. It is
/// projected as a version without hints (the hints are on the 基礎版).
pub fn teacher_sheet(title: &str, content: &Content) -> Sheet {
    let levels = levels_of(content);
    let shown = [Level::Standard, Level::Advanced, Level::Basic]
        .into_iter()
        .find(|l| levels.contains(l))
        .unwrap_or(Level::Standard);
    let mut all = content.clone();
    for b in &mut all.blocks {
        for i in &mut b.items {
            i.levels = if i.levels.iter().any(|l| levels.contains(l)) {
                vec![shown]
            } else {
                Vec::new()
            };
        }
    }
    project(title, &all, shown, Audience::Teacher)
}

/// The kit's worksheet: its title, its latest content, its material (a simplified text prints
/// with the original paragraphs, T8) and whether it is a 差異化學習單.
pub struct KitWorksheet<'a> {
    pub title: &'a str,
    pub content: &'a Content,
    pub material: Option<&'a Material>,
    pub tiered: bool,
}

/// A sheet of the kit, with the original paragraphs beside any simplified text.
fn kit_sheet(w: &KitWorksheet<'_>, level: Level, audience: Audience) -> Sheet {
    let mut sheet = project(w.title, w.content, level, audience);
    if let Some(material) = w.material {
        keep_original_text(&mut sheet, w.content, material);
    }
    sheet
}

/// The whole kit as one DOCX.
pub fn docx(
    title: &str,
    plan: Option<&ActivityPlan>,
    worksheet: Option<KitWorksheet<'_>>,
) -> anyhow::Result<Vec<u8>> {
    let levels: Vec<Level> = worksheet
        .as_ref()
        .map(|w| levels_of(w.content))
        .unwrap_or_default();
    let order: Vec<Level> = worksheet
        .as_ref()
        .map(|w| w.content.student_order.clone())
        .unwrap_or_default();
    let student: Vec<(Level, Sheet)> = worksheet
        .as_ref()
        .map(|w| {
            levels
                .iter()
                .map(|l| (*l, kit_sheet(w, *l, Audience::Student)))
                .collect()
        })
        .unwrap_or_default();
    // A 差異化學習單: one teacher version per level, matching its student version page for page.
    let per_level: Vec<(Level, Sheet)> = worksheet
        .as_ref()
        .filter(|w| differentiated(w.content))
        .map(|w| {
            levels
                .iter()
                .map(|l| (*l, kit_sheet(w, *l, Audience::Teacher)))
                .collect()
        })
        .unwrap_or_default();
    let teacher = worksheet
        .as_ref()
        .filter(|_| per_level.is_empty())
        .map(|w| (teacher_sheet(w.title, w.content), w.tiered));
    let mut parts = vec![Part::Lines {
        heading: HEADING_PLAN.to_string(),
        lines: match plan {
            Some(p) => plan_lines(p),
            None => vec![Line::Muted(NO_PLAN.to_string())],
        },
    }];
    for (level, sheet) in &student {
        parts.push(Part::Sheet {
            heading: student_heading(&levels, &order, *level),
            sheet,
            banner: None,
            note: Vec::new(),
        });
    }
    // Each teacher sheet prints the 分層說明 itself (`Sheet::tiering_note`).
    for (level, sheet) in &per_level {
        parts.push(Part::Sheet {
            heading: teacher_heading(&levels, &order, *level),
            sheet,
            banner: None,
            note: Vec::new(),
        });
    }
    match &teacher {
        Some((sheet, tiered)) => parts.push(Part::Sheet {
            heading: HEADING_TEACHER.to_string(),
            sheet,
            banner: Some(TEACHER_BANNER.to_string()),
            note: if *tiered {
                vec![Line::Teacher(LEGACY_TIERED_REASON.to_string())]
            } else {
                Vec::new()
            },
        }),
        None if per_level.is_empty() => parts.push(Part::Lines {
            heading: HEADING_WORKSHEET.to_string(),
            lines: vec![Line::Muted(NO_WORKSHEET.to_string())],
        }),
        None => {}
    }
    export::combined_docx(&format!("{}：教學套件", title.trim()), &parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prep::{
        CurriculumRef, HigherQuestion, KeyWord, LanguageCategory, Role, StepOption, PHASE_WARMUP,
    };
    use crate::worksheet::tiered::TIERED_REASON;
    use crate::worksheet::{Block, Item};

    fn plan() -> ActivityPlan {
        ActivityPlan {
            title: "信任的雨傘".into(),
            goals: vec![
                Goal::new(
                    GoalKind::Content,
                    "學生能說出主旨",
                    "Students will be able to tell the main idea.",
                    vec!["A-1".into()],
                ),
                Goal::new(
                    GoalKind::Language,
                    "學生能用 because 說明原因",
                    "Students will be able to use because to give a reason.",
                    vec![],
                ),
            ],
            key_vocabulary: vec![],
            materials: vec![],
            higher_order_questions: vec![],
            background: String::new(),
            steps: vec![Step {
                phase: PHASE_WARMUP.into(),
                title: "看圖".into(),
                minutes: 5,
                idea: "看圖猜主題".into(),
                options: vec![StepOption {
                    title: "選項一：圖片".into(),
                    idea: "展示圖片".into(),
                    script: vec!["What do you see?".into()],
                    minutes: 5,
                }],
                script: vec!["Good morning.".into()],
                stuck_tip: "給兩個選項".into(),
                bloom: vec![Bloom::Remember, Bloom::Understand],
                grouping: String::new(),
                roles: vec![],
                monitoring: String::new(),
                skills: vec![],
                scaffolding: String::new(),
            }],
            review: String::new(),
            assessment: String::new(),
            curriculum: vec![CurriculumRef {
                code: "A-1".into(),
                text: "條文".into(),
            }],
        }
    }

    /// The same plan with what version 4 adds.
    fn plan_v4() -> ActivityPlan {
        let mut p = plan();
        p.goals[1] = Goal {
            category: Some(LanguageCategory::Strategy),
            supports: Some(1),
            relation: "學生要用 because 說明原因，才能說出主旨".into(),
            from_teacher: true,
            ..p.goals[1].clone()
        };
        p.key_vocabulary = vec![KeyWord {
            word: "trust".into(),
            zh: "信任".into(),
        }];
        p.materials = vec!["課文".into(), "圖片".into()];
        p.higher_order_questions = vec![HigherQuestion {
            question: "Why did it work?".into(),
            bloom: Bloom::Evaluate,
        }];
        p.background = "借東西的經驗".into();
        let s = &mut p.steps[0];
        s.grouping = "小組".into();
        s.skills = vec!["聽".into(), "說".into()];
        s.roles = vec![
            Role {
                role: "主持".into(),
                task: "帶討論".into(),
            },
            Role {
                role: "記錄".into(),
                task: "寫重點".into(),
            },
        ];
        s.monitoring = "巡視各組".into();
        s.scaffolding = "教師示範".into();
        p.review = "字卡".into();
        p.assessment = "exit ticket".into();
        p
    }

    #[test]
    fn the_plan_prints_the_siop_fields_of_version_4() {
        let lines = plan_lines(&plan_v4());
        let at = |l: Line| {
            lines
                .iter()
                .position(|x| *x == l)
                .unwrap_or_else(|| panic!("{l:?} missing from {lines:#?}"))
        };
        let language = at(Line::Labelled("語言目標".into(), String::new()));
        assert_eq!(
            lines[language + 1],
            Line::Numbered(
                1,
                "〔學習策略〕學生能用 because 說明原因（教師提供）".into()
            )
        );
        assert_eq!(
            lines[language + 3],
            Line::Teacher("支持內容目標 1：學生要用 because 說明原因，才能說出主旨".into())
        );
        let words = at(Line::Heading("關鍵字彙".into()));
        assert_eq!(
            lines[words + 1],
            Line::Labelled("trust".into(), "信任".into())
        );
        let materials = at(Line::Labelled("教材與教具".into(), "課文、圖片".into()));
        let questions = at(Line::Numbered(1, "Why did it work?（評鑑）".into()));
        let background = at(Line::Labelled("背景連結".into(), "借東西的經驗".into()));
        let step = at(Line::Heading("1. 引起動機｜看圖（5 分鐘）".into()));
        assert!(words < materials && materials < questions && questions < background);
        assert!(background < step);
        assert_eq!(
            lines[step + 1],
            Line::Teacher("分組：小組｜語言技能：聽、說".into())
        );
        at(Line::Labelled(
            "分工".into(),
            "主持：帶討論；記錄：寫重點".into(),
        ));
        at(Line::Labelled("教師巡視".into(), "巡視各組".into()));
        at(Line::Labelled("鷹架".into(), "教師示範".into()));
        let review = at(Line::Heading("複習與評量".into()));
        assert!(step < review);
        assert_eq!(
            lines[review + 1],
            Line::Labelled("複習".into(), "字卡".into())
        );
        assert_eq!(
            lines[review + 2],
            Line::Labelled("評量".into(), "exit ticket".into())
        );
        // A plan without them prints none of these headings.
        let old = plan_lines(&plan());
        for heading in ["關鍵字彙", "高層次問題", "複習與評量"] {
            assert!(!old.contains(&Line::Heading(heading.into())), "{heading}");
        }
    }

    fn content() -> Content {
        let item = |id: &str, levels: Vec<Level>, bloom: Bloom| Item {
            id: id.into(),
            answer_format: Some("簡答題".into()),
            prompt: format!("Question {id}?"),
            answer: format!("Answer {id}."),
            levels,
            bloom: Some(bloom),
            hint: "Look again.".into(),
            ..Item::default()
        };
        Content {
            blocks: vec![
                Block {
                    id: "b1".into(),
                    title: "Part A · Find the Facts".into(),
                    bloom: Some(Bloom::Remember),
                    items: vec![item("r1", vec![Level::Basic], Bloom::Remember)],
                    ..Block::default()
                },
                Block {
                    id: "b2".into(),
                    title: "Part B · Make Something New".into(),
                    bloom: Some(Bloom::Create),
                    items: vec![
                        item("c1", vec![Level::Advanced], Bloom::Create),
                        item("gone", vec![], Bloom::Create),
                    ],
                    ..Block::default()
                },
            ],
            levels: Level::ALL.to_vec(),
            ..Content::default()
        }
    }

    #[test]
    fn the_plan_is_printed_with_its_bloom_levels() {
        let lines = plan_lines(&plan());
        assert!(lines.contains(&Line::Heading("1. 引起動機｜看圖（5 分鐘）".into())));
        assert!(lines.contains(&Line::Teacher("Bloom：記憶、理解".into())));
        assert!(lines.contains(&Line::Labelled(
            "選項一：圖片（5 分鐘）".into(),
            "展示圖片".into()
        )));
        assert!(lines.contains(&Line::Labelled("A-1".into(), "條文".into())));
    }

    #[test]
    fn the_goals_are_printed_in_their_groups_in_both_languages() {
        let lines = plan_lines(&plan());
        let at = |l: Line| {
            lines
                .iter()
                .position(|x| *x == l)
                .unwrap_or_else(|| panic!("{l:?} missing from {lines:#?}"))
        };
        let heading = at(Line::Heading("學習目標".into()));
        let content = at(Line::Labelled("內容目標".into(), String::new()));
        let language = at(Line::Labelled("語言目標".into(), String::new()));
        let zh = at(Line::Numbered(1, "學生能說出主旨（課綱：A-1）".into()));
        let en = at(Line::Script(
            "Students will be able to tell the main idea.".into(),
        ));
        assert!(heading < content && content < zh && zh + 1 == en && en < language);
        // Each group is numbered from 1; a goal without codes has no 課綱 note.
        assert_eq!(
            lines[language + 1],
            Line::Numbered(1, "學生能用 because 說明原因".into())
        );
        assert_eq!(
            lines[language + 2],
            Line::Script("Students will be able to use because to give a reason.".into())
        );
    }

    #[test]
    fn a_plan_made_before_the_goal_groups_prints_one_list() {
        // As stored by version 1 or 2 of `prep_activity`.
        let old: ActivityPlan = serde_json::from_value(serde_json::json!({
            "title": "舊的規劃",
            "goals": [
                { "text": "學生能說出主旨", "curriculum_codes": ["A-1"] },
                { "text": "學生能寫出兩句", "curriculum_codes": [] },
            ],
            "steps": [],
            "curriculum": [],
        }))
        .unwrap();
        let lines = plan_lines(&old);
        let goals: Vec<&Line> = lines
            .iter()
            .skip_while(|l| **l != Line::Heading("學習目標".into()))
            .skip(1)
            .collect();
        assert_eq!(
            goals,
            [
                &Line::Numbered(1, "學生能說出主旨（課綱：A-1）".into()),
                &Line::Numbered(2, "學生能寫出兩句".into()),
            ]
        );
    }

    #[test]
    fn the_teacher_version_has_every_question_once_without_hints() {
        let sheet = teacher_sheet("T", &content());
        let items: Vec<_> = sheet.blocks.iter().flat_map(|b| &b.items).collect();
        let prompts: Vec<&str> = items.iter().map(|i| i.prompt.as_str()).collect();
        assert_eq!(
            prompts,
            ["Question r1?", "Question c1?"],
            "not in any version: left out"
        );
        assert_eq!(items[1].no, 2);
        assert!(items.iter().all(|i| i.answer.is_some() && i.hint.is_none()));
        assert_eq!(items[0].bloom, Some(Some(Bloom::Remember)));
        assert_eq!(sheet.audience, Audience::Teacher);
    }

    #[test]
    fn a_kit_made_before_the_redesign_still_exports() {
        // Tiered practice as stored before 差異化學習單: questions split over the versions.
        let c = content();
        let bytes = docx(
            "The Umbrella Stand",
            Some(&plan()),
            Some(KitWorksheet {
                title: "The Umbrella Stand",
                content: &c,
                material: None,
                tiered: true,
            }),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        let at = |s: &str| xml.find(s).unwrap_or_else(|| panic!("{s} missing"));
        // Student pages carry the neutral code, never the level's name (T9).
        assert!(at(HEADING_PLAN) < at("學生版（A 版）"));
        assert!(at("學生版（A 版）") < at("學生版（B 版）"));
        assert!(at("學生版（C 版）") < at(HEADING_TEACHER));
        assert!(!xml.contains("學生版（基礎版）"));
        let teacher = at(HEADING_TEACHER);
        assert!(
            xml.find("〔Bloom：").unwrap() > teacher,
            "tags only for the teacher"
        );
        assert!(xml.find("Answer: ").unwrap() > teacher);
        assert!(xml.contains(LEGACY_TIERED_REASON));
        assert!(!xml.contains(TIERED_REASON));
        assert!(xml.contains("Hint: "), "the 基礎版 prints hints");

        // Without the parts.
        let bytes = docx("T", None, None).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains(NO_PLAN) && xml.contains(NO_WORKSHEET));
    }

    /// A 差異化學習單: every question in all three versions, each with its own support, answer
    /// and scoring note.
    fn differentiated() -> Content {
        use crate::worksheet::{ItemTier, SupportKind};
        let tier = |level: Level, kind: SupportKind, support: &str, answer: &str| ItemTier {
            level,
            support_kind: kind,
            model_sentence: String::new(),
            support: if support.is_empty() {
                vec![]
            } else {
                vec![support.into()]
            },
            word_bank: vec![],
            answer: answer.into(),
            scoring: format!("{}評分", level.label()),
        };
        Content {
            blocks: vec![Block {
                id: "b1".into(),
                title: "Look Closer".into(),
                bloom: Some(Bloom::Analyze),
                items: vec![Item {
                    id: "b1-i1".into(),
                    answer_format: Some("簡答題".into()),
                    prompt: "Why did the neighbors laugh?".into(),
                    answer: "They thought the umbrellas would be gone.".into(),
                    levels: Level::ALL.to_vec(),
                    bloom: Some(Bloom::Analyze),
                    tiers: vec![
                        tier(
                            Level::Basic,
                            SupportKind::SentenceStarter,
                            "BASIC SUPPORT ___",
                            "gone",
                        ),
                        tier(
                            Level::Standard,
                            SupportKind::SentenceStarter,
                            "They laughed because …",
                            "They thought the umbrellas would be gone.",
                        ),
                        tier(
                            Level::Advanced,
                            SupportKind::None,
                            "",
                            "They were sure that every umbrella would be taken.",
                        ),
                    ],
                    ..Item::default()
                }],
                ..Block::default()
            }],
            levels: Level::ALL.to_vec(),
            ..Content::default()
        }
    }

    #[test]
    fn a_differentiated_kit_has_a_teacher_version_per_level() {
        let c = differentiated();
        let bytes = docx(
            "The Umbrella Stand",
            None,
            Some(KitWorksheet {
                title: "The Umbrella Stand",
                content: &c,
                material: None,
                tiered: true,
            }),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        let at = |s: &str| xml.find(s).unwrap_or_else(|| panic!("{s} missing"));
        let first_teacher = at("教師版（基礎版＝A 版）");
        assert!(at("學生版（C 版）") < first_teacher);
        assert!(first_teacher < at("教師版（一般版＝B 版）"));
        assert!(at("教師版（一般版＝B 版）") < at("教師版（進階版＝C 版）"));
        // The student pages never name a level (T9) and have no answers or scoring notes.
        let students = &xml[..first_teacher];
        for name in [
            "基礎",
            "一般版",
            "進階",
            "Basic",
            "Standard",
            "Advanced",
            "評分",
        ] {
            assert!(!students.contains(name), "{name} on a student page");
        }
        assert!(students.contains("Version A") && students.contains("BASIC SUPPORT ___"));
        // Each teacher version: its own answer and scoring note, and the 分層說明.
        let teachers = &xml[first_teacher..];
        assert!(teachers.contains("基礎版評分") && teachers.contains("進階版評分"));
        assert!(teachers.contains("every umbrella would be taken."));
        assert_eq!(teachers.matches(TIERED_REASON).count(), 3);
        assert!(!xml.contains(LEGACY_TIERED_REASON));
    }
}
