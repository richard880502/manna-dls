//! 教學套件 export (`GET /api/prep-sessions/:id/turns/:turn_id/kit?format=docx`): one DOCX with
//! 1. 教學流程, the kit's activity plan;
//! 2. the student version of each of the worksheet's levels (基礎版、一般版、進階版 for a tiered
//!    worksheet), exactly as the worksheet's own export prints them;
//! 3. 教師版: every question once, numbered through, with its answer, evidence and Bloom tag.
//!
//! The sheets come from the same projection as the editor and the worksheet export
//! ([`project`]); the document is written by the worksheet's DOCX writer
//! ([`export::combined_docx`]). A part that the kit could not make is replaced by a line that
//! says so.

use super::{ActivityPlan, Goal, GoalKind, Step};
use crate::worksheet::assemble::{project, Sheet};
use crate::worksheet::bloom::{Bloom, TIERED_REASON};
use crate::worksheet::export::{self, Line, Part};
use crate::worksheet::{Audience, Content, Level};

pub const HEADING_PLAN: &str = "教學流程";
pub const HEADING_TEACHER: &str = "教師版";
const HEADING_WORKSHEET: &str = "分層學習單";
const NO_PLAN: &str = "這個教學套件的課堂活動沒有完成，所以沒有教學流程。";
const NO_WORKSHEET: &str = "這個教學套件的分層學習單沒有完成（或已經刪除），所以沒有學習單。";
const TEACHER_BANNER: &str =
    "Teacher reference · all versions · answers, evidence and Bloom levels are not on the student sheets";

/// The heading of a level's student version: 「學生版（基礎版）」.
pub fn student_heading(level: Level) -> String {
    format!("學生版（{}）", level.label())
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

fn step_lines(no: usize, s: &Step, out: &mut Vec<Line>) {
    out.push(Line::Heading(format!(
        "{no}. {}｜{}（{} 分鐘）",
        s.phase, s.title, s.minutes
    )));
    if !s.bloom.is_empty() {
        out.push(Line::Teacher(bloom_line(&s.bloom)));
    }
    if !s.idea.trim().is_empty() {
        out.push(Line::Labelled("教學思路".into(), s.idea.clone()));
    }
    for (k, o) in s.options.iter().enumerate() {
        let name = if o.title.trim().is_empty() {
            format!("選項{}", ["一", "二", "三"].get(k).unwrap_or(&""))
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

/// Goals numbered from 1: the Chinese line with the curriculum codes it cites, the English line
/// (if any) under it.
fn goal_lines<'a>(goals: impl Iterator<Item = &'a Goal>, out: &mut Vec<Line>) {
    for (i, g) in goals.enumerate() {
        let codes = if g.curriculum_codes.is_empty() {
            String::new()
        } else {
            format!("（課綱：{}）", g.curriculum_codes.join("、"))
        };
        out.push(Line::Numbered(i + 1, format!("{}{codes}", g.zh)));
        if !g.en.trim().is_empty() {
            out.push(Line::Script(g.en.clone()));
        }
    }
}

/// 教學流程: the plan's title, its goals (內容目標 and 語言目標, each in Chinese and English, with
/// the curriculum codes they cite; a plan made before has one list in Chinese), every step with
/// its Bloom levels, and the curriculum items cited.
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
    for (i, s) in plan.steps.iter().enumerate() {
        step_lines(i + 1, s, &mut out);
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

/// The kit's worksheet: its title, its latest content, and whether it is tiered practice.
pub struct KitWorksheet<'a> {
    pub title: &'a str,
    pub content: &'a Content,
    pub tiered: bool,
}

/// The whole kit as one DOCX.
pub fn docx(
    title: &str,
    plan: Option<&ActivityPlan>,
    worksheet: Option<KitWorksheet<'_>>,
) -> anyhow::Result<Vec<u8>> {
    let student: Vec<(Level, Sheet)> = worksheet
        .as_ref()
        .map(|w| {
            levels_of(w.content)
                .into_iter()
                .map(|l| (l, project(w.title, w.content, l, Audience::Student)))
                .collect()
        })
        .unwrap_or_default();
    let teacher = worksheet
        .as_ref()
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
            heading: student_heading(*level),
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
                vec![Line::Teacher(TIERED_REASON.to_string())]
            } else {
                Vec::new()
            },
        }),
        None => parts.push(Part::Lines {
            heading: HEADING_WORKSHEET.to_string(),
            lines: vec![Line::Muted(NO_WORKSHEET.to_string())],
        }),
    }
    export::combined_docx(&format!("{}：教學套件", title.trim()), &parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prep::{CurriculumRef, StepOption, PHASE_WARMUP};
    use crate::worksheet::{Block, Item};

    fn plan() -> ActivityPlan {
        ActivityPlan {
            title: "信任的雨傘".into(),
            goals: vec![
                Goal {
                    kind: Some(GoalKind::Content),
                    zh: "學生能說出主旨".into(),
                    en: "Students will be able to tell the main idea.".into(),
                    curriculum_codes: vec!["A-1".into()],
                },
                Goal {
                    kind: Some(GoalKind::Language),
                    zh: "學生能用 because 說明原因".into(),
                    en: "Students will be able to use because to give a reason.".into(),
                    curriculum_codes: vec![],
                },
            ],
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
            }],
            curriculum: vec![CurriculumRef {
                code: "A-1".into(),
                text: "條文".into(),
            }],
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
    fn the_kit_is_one_docx_with_all_parts() {
        let c = content();
        let bytes = docx(
            "The Umbrella Stand",
            Some(&plan()),
            Some(KitWorksheet {
                title: "The Umbrella Stand",
                content: &c,
                tiered: true,
            }),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        let at = |s: &str| xml.find(s).unwrap_or_else(|| panic!("{s} missing"));
        assert!(at(HEADING_PLAN) < at("學生版（基礎版）"));
        assert!(at("學生版（基礎版）") < at("學生版（一般版）"));
        assert!(at("學生版（進階版）") < at(HEADING_TEACHER));
        let teacher = at(HEADING_TEACHER);
        assert!(
            xml.find("〔Bloom：").unwrap() > teacher,
            "tags only for the teacher"
        );
        assert!(xml.find("Answer: ").unwrap() > teacher);
        assert!(xml.contains(TIERED_REASON));
        assert!(xml.contains("Hint: "), "the 基礎版 prints hints");

        // Without the parts.
        let bytes = docx("T", None, None).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains(NO_PLAN) && xml.contains(NO_WORKSHEET));
    }
}
