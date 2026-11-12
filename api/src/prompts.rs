//! Prompt versions (stored in `prompt_versions`, edited from the admin page) and
//! `{{placeholder}}` template rendering.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::PgPool;

pub const KEY_CORE_ASSIST: [&str; 3] = ["core_assist_l1", "core_assist_l2", "core_assist_l3"];
pub const KEY_PLAN_SUGGEST: &str = "plan_suggest";

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
}
