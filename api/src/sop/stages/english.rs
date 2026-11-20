use super::*;

// ---------------------------------------------------------------- step 3: the English of an objective

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawEnglish {
    pub english: String,
    pub changed: bool,
    pub note_zh: String,
}

pub const ENGLISH_SCHEMA_NAME: &str = "sop_objective_english";

pub fn english_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "english": { "type": "string" },
            "changed": { "type": "boolean" },
            "note_zh": { "type": "string" }
        },
        "required": ["english", "changed", "note_zh"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EnglishSuggestion {
    pub english: String,
    /// False when the model found nothing to fix (the same sentence comes back).
    pub changed: bool,
    pub note: String,
}

/// Checks the corrected English of an objective: one sentence in the 「I can …」 form, in
/// English only, short enough for students.
pub(crate) fn evaluate_english(raw: &RawEnglish, original: &str) -> Checked<EnglishSuggestion> {
    let mut problems: Vec<String> = Vec::new();
    let mut english = super::super::checks::clean_markup(&raw.english);
    english = english.split_whitespace().collect::<Vec<_>>().join(" ");
    if english.is_empty() {
        problems.push("english 是空的。".to_string());
    } else if contains_cjk(&english) {
        problems.push("english 只能用英文，不可以有中文。".to_string());
    } else {
        let lower = english.to_lowercase();
        if !lower.starts_with("i can ") {
            problems.push("english 必須以「I can 」開頭。".to_string());
        }
        let words = english.split_whitespace().count();
        if !(4..=ENGLISH_MAX_WORDS).contains(&words) {
            problems.push(format!(
                "english 需要是 4 到 {ENGLISH_MAX_WORDS} 個英文單字的一句話，現在有 {words} 個。"
            ));
        }
    }
    if !problems.is_empty() {
        return Checked {
            output: None,
            problems,
        };
    }
    // The opening is always written the same way.
    english = format!("I can {}", english[6..].trim_start());
    let same = super::super::checks::norm(&english) == super::super::checks::norm(original);
    let mut note = super::super::checks::clean_markup(&raw.note_zh);
    if note.is_empty() || !contains_cjk(&note) {
        note = if same {
            "這個英文版本已經很好，不需要改。".to_string()
        } else if original.trim().is_empty() {
            "依中文目標寫的英文版本。".to_string()
        } else {
            "修了文法與用字。".to_string()
        };
    }
    Checked {
        output: Some(EnglishSuggestion {
            changed: !same,
            english,
            note: clip_chars(&note, 80),
        }),
        problems,
    }
}

/// 「幫我修英文」: one short call, answered in the request (no job). Returns the `AiResult` JSON.
pub async fn suggest_english(
    state: &AppState,
    teacher_id: i64,
    lesson: &crate::teaching_context::LessonRef,
    kind: ObjectiveType,
    text: &str,
    english: &str,
) -> AppResult<Value> {
    if !state.ai.enabled() {
        return Ok(result_json(AiResult::<Value>::disabled()));
    }
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("objective_type", kind.label().to_string()),
        ("objective_text", or_blank(text.trim())),
        ("student_facing_en", or_blank(english.trim())),
    ]);
    let mut call = new_call(
        KEY_ENGLISH,
        SC_ENGLISH,
        ENGLISH_SCHEMA_NAME,
        english_schema(),
        vars,
        json!({
            "task": "sop_objective_english",
            "english": english.trim(),
            "text": text.trim(),
        }),
        400,
        0.2,
        json!({ "sop": { "lesson_id": lesson.id } }),
    );
    call.required = vec![("objective_text", "中文目標")];
    // The teacher's own words are the input here, not the material: nothing is redacted.
    call.redact = Vec::new();
    let original = english.trim().to_string();
    let outcome = generation::call_in(
        state,
        teacher_id,
        (Some(lesson.unit_id), Some(lesson.id)),
        call,
        |_| Vec::new(),
        |raw: &RawEnglish| evaluate_english(raw, &original),
        generation::nothing,
    )
    .await?;
    Ok(match outcome {
        Ok((out, id)) => result_json(AiResult::shown(id, out, Vec::new())),
        Err(message) => failed_result(&message, Vec::new()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english(e: &str) -> RawEnglish {
        RawEnglish {
            english: e.into(),
            changed: true,
            note_zh: String::new(),
        }
    }

    #[test]
    fn english_fix_keeps_the_i_can_form_and_reports_no_change() {
        let ok = evaluate_english(&english("i can explain why the stand was useful."), "");
        assert!(ok.problems.is_empty());
        let out = ok.output.unwrap();
        assert_eq!(out.english, "I can explain why the stand was useful.");
        assert!(out.changed);
        let same = evaluate_english(
            &english("I can explain why the stand was useful."),
            "I can  explain why the stand was useful.",
        );
        assert!(!same.output.unwrap().changed);
    }

    #[test]
    fn english_fix_rejects_chinese_other_openings_and_long_sentences() {
        for bad in [
            "我可以說明原因",
            "Students explain why it happened today.",
            "",
            &format!("I can {}", "word ".repeat(30)),
        ] {
            let c = evaluate_english(&english(bad), "I can x y z.");
            assert!(c.output.is_none(), "{bad}");
            assert!(!c.problems.is_empty());
        }
    }
}
