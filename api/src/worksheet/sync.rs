//! 同步回課例 (spec §9.5). Changing objectives or tasks in the generator never touches the lesson
//! SOP. The teacher sees what differs ([`preview`]) and only the changes she confirms are written
//! ([`apply`]), as a new SOP version; earlier versions stay.
//!
//! 「建議補充至 CoRe」 is out of scope here: `core_suggestions` is always empty. The hook for it is
//! [`core_suggestions`], which would read the worksheet and the lesson and return items for the
//! unit's CoRe, again written only on confirmation.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::{Block, BlockKind, Content, Material, Objective, Origin, ScaffoldType};
use crate::sop::{self, Objective as SopObjective, SopContent, Task, TaskBlock};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    pub key: String,
    /// `objective` | `task_block`
    pub target: &'static str,
    /// `add` | `update` | `remove`
    pub action: &'static str,
    pub label: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub lesson_id: i64,
    pub sop_version_no: Option<i32>,
    pub changes: Vec<Change>,
    pub core_suggestions: Vec<serde_json::Value>,
}

/// Hook for 「建議補充至單元想法（CoRe）」 (spec §9.5, last two points). Not built in this version.
pub fn core_suggestions(_content: &Content) -> Vec<serde_json::Value> {
    Vec::new()
}

fn origin_to_sop(o: Origin) -> sop::Origin {
    match o {
        Origin::Teacher => sop::Origin::Teacher,
        Origin::Ai => sop::Origin::AiAdopted,
        Origin::AiEdited => sop::Origin::AiEdited,
    }
}

fn objective_text(o: &Objective) -> String {
    [o.text.trim(), o.student_facing_en.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn sop_objective_text(o: &SopObjective) -> String {
    [o.text.trim(), o.student_facing_en.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The tasks of a worksheet block as SOP tasks: one per item, the block's scaffolds on the first.
fn block_tasks(block: &Block) -> Vec<Task> {
    let joined = |t: ScaffoldType| -> String {
        block
            .scaffolds
            .iter()
            .filter(|s| s.kind_type == t)
            .map(|s| {
                if t == ScaffoldType::Procedural {
                    s.steps.join(" ")
                } else {
                    s.content.clone()
                }
            })
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let scaffolds = sop::Scaffolds {
        verbal: clip(&joined(ScaffoldType::Verbal)),
        instructional: clip(&joined(ScaffoldType::Instructional)),
        procedural: clip(&joined(ScaffoldType::Procedural)),
    };
    block
        .items
        .iter()
        .filter(|i| !i.prompt.trim().is_empty())
        .enumerate()
        .map(|(n, i)| Task {
            id: format!("ws{}", n + 1),
            function: block.info_function,
            text: clip(i.prompt.trim()),
            scaffolds: if n == 0 {
                scaffolds.clone()
            } else {
                sop::Scaffolds::default()
            },
            origin: origin_to_sop(i.origin),
        })
        .collect()
}

fn clip(s: &str) -> String {
    s.chars().take(sop::MAX_FIELD_CHARS).collect()
}

fn task_texts(tasks: &[Task]) -> String {
    tasks
        .iter()
        .map(|t| t.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_label(block: &Block, material: &Material) -> String {
    let nos: Vec<String> = block
        .paragraph_ids
        .iter()
        .filter_map(|id| material.no_of(id))
        .map(|n| n.to_string())
        .collect();
    let title = block.title.trim();
    match (nos.is_empty(), title.is_empty()) {
        (true, true) => "任務".to_string(),
        (true, false) => format!("任務：{title}"),
        (false, _) => format!("第 {} 段的任務", nos.join("、")),
    }
}

fn activity_blocks(content: &Content) -> impl Iterator<Item = &Block> {
    content.blocks.iter().filter(|b| {
        b.kind == BlockKind::Activity && b.items.iter().any(|i| !i.prompt.trim().is_empty())
    })
}

/// What would change in the SOP. Additions and updates; removals only for SOP entries the
/// worksheet was carried over from (they are listed but meant to stay unticked by default).
pub fn diff(content: &Content, material: &Material, sop: &SopContent) -> Vec<Change> {
    let mut changes = Vec::new();
    let came_from_sop = content.objectives.iter().any(|o| o.sop_id.is_some());
    for o in &content.objectives {
        let after = objective_text(o);
        if after.is_empty() {
            continue;
        }
        let current = o
            .sop_id
            .as_deref()
            .and_then(|id| sop.objectives.iter().find(|s| s.id == id));
        match current {
            Some(s) if sop_objective_text(s) == after && s.kind == o.kind => {}
            Some(s) => changes.push(Change {
                key: format!("objective:{}", o.id),
                target: "objective",
                action: "update",
                label: o.kind.label().to_string(),
                before: sop_objective_text(s),
                after,
            }),
            None => changes.push(Change {
                key: format!("objective:{}", o.id),
                target: "objective",
                action: "add",
                label: o.kind.label().to_string(),
                before: String::new(),
                after,
            }),
        }
    }
    if came_from_sop {
        let kept: HashSet<&str> = content
            .objectives
            .iter()
            .filter_map(|o| o.sop_id.as_deref())
            .collect();
        for s in sop
            .objectives
            .iter()
            .filter(|s| !kept.contains(s.id.as_str()))
        {
            changes.push(Change {
                key: format!("remove-objective:{}", s.id),
                target: "objective",
                action: "remove",
                label: s.kind.label().to_string(),
                before: sop_objective_text(s),
                after: String::new(),
            });
        }
    }

    let mut referenced: HashSet<&str> = HashSet::new();
    for b in activity_blocks(content) {
        let after = task_texts(&block_tasks(b));
        let current = b
            .sop_block_id
            .as_deref()
            .and_then(|id| sop.task_blocks.iter().find(|s| s.id == id));
        if let Some(s) = current {
            referenced.insert(s.id.as_str());
            if task_texts(&s.tasks) != after {
                changes.push(Change {
                    key: format!("block:{}", b.id),
                    target: "task_block",
                    action: "update",
                    label: block_label(b, material),
                    before: task_texts(&s.tasks),
                    after,
                });
            }
        } else {
            changes.push(Change {
                key: format!("block:{}", b.id),
                target: "task_block",
                action: "add",
                label: block_label(b, material),
                before: String::new(),
                after,
            });
        }
    }
    if !referenced.is_empty() {
        for s in sop
            .task_blocks
            .iter()
            .filter(|s| !referenced.contains(s.id.as_str()) && !s.tasks.is_empty())
        {
            changes.push(Change {
                key: format!("remove-block:{}", s.id),
                target: "task_block",
                action: "remove",
                label: "課例裡的任務".to_string(),
                before: task_texts(&s.tasks),
                after: String::new(),
            });
        }
    }
    changes
}

/// How many additions and updates there are (what the one-line prompt counts).
pub fn pending(changes: &[Change]) -> usize {
    changes.iter().filter(|c| c.action != "remove").count()
}

fn unique_id(wanted: &str, taken: &HashSet<String>) -> String {
    let base: String = wanted.chars().take(sop::MAX_ID_CHARS - 4).collect();
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains(id))
        .unwrap_or(base)
}

/// Worksheet paragraph id → the SOP material's id of the paragraph with the same text.
fn paragraph_map(
    material: &Material,
    sop_paragraphs: &[sop::Paragraph],
) -> HashMap<String, String> {
    material
        .paragraphs
        .iter()
        .filter_map(|p| {
            sop_paragraphs
                .iter()
                .find(|s| s.text == p.text)
                .map(|s| (p.id.clone(), s.id.clone()))
        })
        .collect()
}

/// Applies the changes named in `keys` to `sop`. Returns how many were applied; `content` gets
/// the SOP ids of what it was added as.
pub fn apply(
    content: &mut Content,
    material: &Material,
    sop: &mut SopContent,
    sop_paragraphs: &[sop::Paragraph],
    keys: &[String],
) -> usize {
    let wanted: HashSet<&str> = keys.iter().map(String::as_str).collect();
    let changes = diff(content, material, sop);
    let pmap = paragraph_map(material, sop_paragraphs);
    let mut applied = 0;
    for change in changes.iter().filter(|c| wanted.contains(c.key.as_str())) {
        applied += 1;
        if let Some(id) = change.key.strip_prefix("remove-objective:") {
            sop.objectives.retain(|o| o.id != id);
        } else if let Some(id) = change.key.strip_prefix("remove-block:") {
            sop.task_blocks.retain(|b| b.id != id);
        } else if let Some(id) = change.key.strip_prefix("objective:") {
            let Some(o) = content.objectives.iter_mut().find(|o| o.id == id) else {
                continue;
            };
            let existing = o
                .sop_id
                .as_deref()
                .and_then(|sid| sop.objectives.iter_mut().find(|s| s.id == sid));
            match existing {
                Some(s) => {
                    s.kind = o.kind;
                    s.text = clip(o.text.trim());
                    s.student_facing_en = clip(o.student_facing_en.trim());
                    s.origin = match s.origin {
                        sop::Origin::Teacher => sop::Origin::Teacher,
                        _ => sop::Origin::AiEdited,
                    };
                }
                None => {
                    let taken: HashSet<String> =
                        sop.objectives.iter().map(|s| s.id.clone()).collect();
                    let new_id = unique_id(&format!("ws-{}", o.id), &taken);
                    sop.objectives.push(SopObjective {
                        id: new_id.clone(),
                        kind: o.kind,
                        text: clip(o.text.trim()),
                        student_facing_en: clip(o.student_facing_en.trim()),
                        reason: String::new(),
                        paragraph_ids: Vec::new(),
                        links: Vec::new(),
                        link_status: sop::LinkStatus::Unlinked,
                        origin: origin_to_sop(o.origin),
                    });
                    o.sop_id = Some(new_id);
                }
            }
        } else if let Some(id) = change.key.strip_prefix("block:") {
            let Some(b) = content.blocks.iter_mut().find(|b| b.id == id) else {
                continue;
            };
            let tasks = block_tasks(b);
            let paragraph_ids: Vec<String> = b
                .paragraph_ids
                .iter()
                .map(|p| pmap.get(p).cloned().unwrap_or_else(|| p.clone()))
                .collect();
            let existing = b
                .sop_block_id
                .as_deref()
                .and_then(|sid| sop.task_blocks.iter_mut().find(|s| s.id == sid));
            match existing {
                Some(s) => {
                    s.tasks = tasks;
                    s.paragraph_ids = paragraph_ids;
                }
                None => {
                    let taken: HashSet<String> =
                        sop.task_blocks.iter().map(|s| s.id.clone()).collect();
                    let new_id = unique_id(&format!("ws-{}", b.id), &taken);
                    sop.task_blocks.push(TaskBlock {
                        id: new_id.clone(),
                        paragraph_ids,
                        tasks,
                    });
                    b.sop_block_id = Some(new_id);
                }
            }
        }
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::ObjectiveType;
    use crate::worksheet::{Item, Level};

    fn material() -> Material {
        Material::pasted(None, 0, "T", "One.\n\nTwo.", "").0
    }

    fn objective(id: &str, sop_id: Option<&str>, text: &str) -> Objective {
        Objective {
            id: id.into(),
            kind: ObjectiveType::Content,
            text: text.into(),
            student_facing_en: String::new(),
            origin: Origin::Teacher,
            sop_id: sop_id.map(str::to_string),
        }
    }

    fn sop_objective(id: &str, text: &str) -> SopObjective {
        SopObjective {
            id: id.into(),
            kind: ObjectiveType::Content,
            text: text.into(),
            student_facing_en: String::new(),
            reason: String::new(),
            paragraph_ids: Vec::new(),
            links: Vec::new(),
            link_status: sop::LinkStatus::Unlinked,
            origin: sop::Origin::Teacher,
        }
    }

    #[test]
    fn nothing_changes_until_applied_and_only_what_is_ticked() {
        let m = material();
        let mut sop = SopContent {
            objectives: vec![
                sop_objective("o1", "學生能說出主旨"),
                sop_objective("o2", "舊的"),
            ],
            ..SopContent::default()
        };
        let mut content = Content {
            objectives: vec![
                objective("a", Some("o1"), "學生能說出主旨並舉例"),
                objective("b", None, "新的目標"),
            ],
            blocks: vec![Block {
                id: "b1".into(),
                paragraph_ids: vec!["p2".into()],
                items: vec![Item {
                    prompt: "Why?".into(),
                    levels: vec![Level::Standard],
                    origin: Origin::Ai,
                    ..Item::default()
                }],
                ..Block::default()
            }],
            ..Content::default()
        };
        let changes = diff(&content, &m, &sop);
        let actions: Vec<(&str, &str)> =
            changes.iter().map(|c| (c.key.as_str(), c.action)).collect();
        assert_eq!(
            actions,
            [
                ("objective:a", "update"),
                ("objective:b", "add"),
                ("remove-objective:o2", "remove"),
                ("block:b1", "add")
            ]
        );
        assert_eq!(pending(&changes), 3);
        let before = sop.clone();
        let sop_paragraphs = vec![sop::Paragraph {
            id: "p9".into(),
            no: 1,
            text: "Two.".into(),
            page: None,
        }];
        let n = apply(
            &mut content,
            &m,
            &mut sop,
            &sop_paragraphs,
            &["block:b1".to_string()],
        );
        assert_eq!(n, 1);
        assert_eq!(
            sop.objectives, before.objectives,
            "unticked changes are not written"
        );
        assert_eq!(sop.task_blocks.len(), 1);
        assert_eq!(sop.task_blocks[0].paragraph_ids, vec!["p9".to_string()]);
        assert_eq!(sop.task_blocks[0].tasks[0].origin, sop::Origin::AiAdopted);
        assert_eq!(content.blocks[0].sop_block_id.as_deref(), Some("ws-b1"));
        assert!(sop::validate(&sop).is_ok());
        // Applied: the block no longer differs.
        assert!(!diff(&content, &m, &sop).iter().any(|c| c.key == "block:b1"));
    }
}
