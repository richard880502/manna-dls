//! Prompt versions (stored in `prompt_versions`, edited from the admin page) and
//! `{{placeholder}}` template rendering.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::PgPool;

pub const KEY_CORE_ASSIST: [&str; 3] = ["core_assist_l1", "core_assist_l2", "core_assist_l3"];
pub const KEY_PLAN_SUGGEST: &str = "plan_suggest";
pub const KEY_MATERIAL_FOCUSES: &str = "material_focuses";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PromptVersion {
    pub id: i64,
    pub prompt_key: String,
    pub version_no: i32,
    pub system_text: String,
    pub user_template: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn latest(db: &PgPool, key: &str) -> sqlx::Result<Option<PromptVersion>> {
    sqlx::query_as(
        "SELECT id, prompt_key, version_no, system_text, user_template, created_at
           FROM prompt_versions WHERE prompt_key = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(key)
    .fetch_optional(db)
    .await
}

/// Replaces every `{{name}}` (surrounding spaces inside the braces allowed) with `vars[name]`.
/// Unknown placeholders are left untouched so a typo in an edited template is visible in the
/// logged input snapshot instead of silently disappearing. Substituted values are not
/// re-scanned, so teacher text containing `{{...}}` cannot inject other variables.
pub fn render(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 2 + end + 2]),
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// True if `template` contains the placeholder `{{name}}` (spaces inside the braces allowed).
pub fn has_placeholder(template: &str, name: &str) -> bool {
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                if after[..end].trim() == name {
                    return true;
                }
                rest = &after[end + 2..];
            }
            None => return false,
        }
    }
    false
}

/// The distinct placeholder names in `template`, in order of first appearance.
pub fn placeholders(template: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Renders the user message and appends every `(name, label)` whose placeholder appears in
/// neither the system text nor the user template, as `label：value`. Guarantees that context
/// every AI call must carry (class profile, length preference, …) reaches the model even with
/// older prompt versions or a PI-edited template that dropped the placeholder.
pub fn render_user_with(
    prompt: &PromptVersion,
    vars: &HashMap<&str, String>,
    required: &[(&str, &str)],
) -> String {
    let mut user = render(&prompt.user_template, vars);
    for (name, label) in required {
        if has_placeholder(&prompt.system_text, name)
            || has_placeholder(&prompt.user_template, name)
        {
            continue;
        }
        if let Some(value) = vars.get(name) {
            user.push_str(&format!("\n\n{label}：{value}"));
        }
    }
    user
}

/// Stand-in logged instead of text that must stay deletable (unit materials, 我的範例):
/// AI input snapshots keep the length and a hash prefix, never the text itself.
pub fn redacted(label: &str, text: &str) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }
    format!(
        "［{label}：{} 字，sha256 {}；原文不寫入紀錄，教師刪除即實際刪除］",
        text.chars().count(),
        &crate::auth::sha256_hex(text)[..16]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn substitutes_known_placeholders() {
        let v = vars(&[("name", "王老師"), ("unit", "Unit 3")]);
        assert_eq!(render("{{name}}：{{ unit }}！", &v), "王老師：Unit 3！");
    }

    #[test]
    fn repeated_placeholders() {
        let v = vars(&[("x", "A")]);
        assert_eq!(render("{{x}}{{x}}-{{x}}", &v), "AA-A");
    }

    #[test]
    fn unknown_placeholders_are_kept() {
        let v = vars(&[]);
        assert_eq!(render("hi {{missing}} there", &v), "hi {{missing}} there");
    }

    #[test]
    fn unterminated_braces_are_kept() {
        let v = vars(&[("a", "1")]);
        assert_eq!(render("{{a}} and {{b", &v), "1 and {{b");
    }

    #[test]
    fn values_are_not_rescanned() {
        let v = vars(&[("a", "{{b}}"), ("b", "SECRET")]);
        assert_eq!(render("{{a}}", &v), "{{b}}");
    }

    #[test]
    fn multibyte_text_is_preserved() {
        let v = vars(&[("q", "你希望學生學會什麼？")]);
        assert_eq!(
            render("題目：{{q}}（請回答）", &v),
            "題目：你希望學生學會什麼？（請回答）"
        );
    }

    fn pv(system: &str, user: &str) -> PromptVersion {
        PromptVersion {
            id: 1,
            prompt_key: "k".into(),
            version_no: 1,
            system_text: system.into(),
            user_template: user.into(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn placeholder_detection() {
        assert!(has_placeholder("a {{ class_profile }} b", "class_profile"));
        assert!(!has_placeholder(
            "a {{class}} b class_profile",
            "class_profile"
        ));
        assert!(!has_placeholder("{{class_profile", "class_profile"));
    }

    #[test]
    fn missing_required_vars_are_appended() {
        let v = vars(&[
            ("class_profile", "高二普通班"),
            ("detail_instruction", "精簡"),
        ]);
        let required = [
            ("class_profile", "班級特性"),
            ("detail_instruction", "回覆長度"),
        ];
        // v1-style template without the placeholders
        let user = render_user_with(&pv("sys", "題目"), &v, &required);
        assert_eq!(user, "題目\n\n班級特性：高二普通班\n\n回覆長度：精簡");
        // present in the user template or the system text → not duplicated
        let user = render_user_with(
            &pv("長度：{{detail_instruction}}", "班級：{{class_profile}}"),
            &v,
            &required,
        );
        assert_eq!(user, "班級：高二普通班");
    }

    #[test]
    fn redaction_keeps_no_text() {
        let r = redacted("教材", "Secret passage text");
        assert!(!r.contains("Secret"));
        assert!(r.contains("19 字"));
        assert_eq!(redacted("教材", ""), "");
    }
}
