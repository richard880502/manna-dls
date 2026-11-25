//! Where a free-form habit applies. The closed kinds always go to every step in
//! [`HABIT_STEPS`]; a free-form habit goes only to the steps its scope maps to, so that a habit
//! about page layout never reaches the step that writes a warm-up question.
use super::HABIT_STEPS;

/// `(code, 繁體中文名稱, steps)`. `language` and `all` are every habit step.
pub const SCOPES: &[(&str, &str, &[&str])] = &[
    (
        "content",
        "題目內容",
        &[
            "worksheet_tasks",
            "worksheet_warmup",
            "worksheet_exit_ticket",
            "worksheet_bloom_level",
            "worksheet_tiered_level",
            "worksheet_template_section",
            "worksheet_block_revise",
            "prep_activity",
            "prep_ask",
            "prep_revise",
            "prep_chat",
            "sop_paragraph_tasks",
            "sop_warmup",
            "sop_exit_ticket",
        ],
    ),
    (
        "format",
        "題型與作答方式",
        &[
            "worksheet_tasks",
            "worksheet_warmup",
            "worksheet_exit_ticket",
            "worksheet_bloom_level",
            "worksheet_tiered_level",
            "worksheet_template_section",
            "worksheet_block_revise",
            "prep_revise",
            "prep_chat",
            "sop_paragraph_tasks",
            "sop_warmup",
            "sop_exit_ticket",
        ],
    ),
    (
        "layout",
        "版面與格式",
        &[
            "worksheet_tasks",
            "worksheet_warmup",
            "worksheet_scaffolds",
            "worksheet_exit_ticket",
            "worksheet_bloom_level",
            "worksheet_tiered_level",
            "worksheet_template_section",
            "worksheet_block_revise",
            "prep_revise",
            "prep_chat",
        ],
    ),
    ("language", "語言與用字", HABIT_STEPS),
    (
        "scaffold",
        "鷹架與支持",
        &[
            "worksheet_tasks",
            "worksheet_scaffolds",
            "worksheet_bloom_level",
            "worksheet_tiered_level",
            "worksheet_template_section",
            "worksheet_block_revise",
            "prep_activity",
            "prep_revise",
            "prep_chat",
            "sop_paragraph_tasks",
        ],
    ),
    (
        "workflow",
        "備課活動與流程",
        &["prep_activity", "prep_ask", "prep_revise", "prep_chat"],
    ),
    ("all", "全部", HABIT_STEPS),
];

pub fn is_scope(code: &str) -> bool {
    SCOPES.iter().any(|(c, _, _)| *c == code)
}

pub fn codes() -> Vec<&'static str> {
    SCOPES.iter().map(|(c, _, _)| *c).collect()
}

pub fn label(code: &str) -> Option<&'static str> {
    SCOPES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, l, _)| *l)
}

/// Whether a free-form habit with this scope goes to the generation step `module`.
pub fn applies(scope: &str, module: &str) -> bool {
    SCOPES
        .iter()
        .find(|(c, _, _)| *c == scope)
        .is_some_and(|(_, _, steps)| steps.contains(&module))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scope_maps_only_to_real_habit_steps_and_every_step_is_reachable() {
        for (code, _, steps) in SCOPES {
            assert!(!steps.is_empty(), "{code}");
            for s in *steps {
                assert!(HABIT_STEPS.contains(s), "{code} maps to unknown step {s}");
            }
        }
        for step in HABIT_STEPS {
            assert!(applies("all", step) && applies("language", step), "{step}");
            assert!(
                SCOPES
                    .iter()
                    .any(|(c, _, _)| *c != "all" && *c != "language" && applies(c, step)),
                "{step} is only reachable through `all` or `language`"
            );
        }
        assert!(!applies("layout", "prep_ask"));
        assert!(applies("workflow", "prep_ask"));
        assert!(!applies("unknown", "worksheet_tasks"));
        assert!(!applies("all", "worksheet_bloom_check"));
    }
}
