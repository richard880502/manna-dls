use super::*;

// ---------------------------------------------------------------- steps 5–7: 幫我修英文

/// What the teacher wrote: a task (step 5), a warm-up activity (step 6) or an exit-ticket item
/// (step 7).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextField {
    Task,
    Warmup,
    Exit,
}

impl TextField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Task => "第 5 步的問題或任務",
            Self::Warmup => "第 6 步的暖身活動",
            Self::Exit => "第 7 步的 Exit ticket 題目",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawTextEnglish {
    /// One line per line of the teacher's text (gemma ends a JSON string where it wants a line
    /// break, so the lines are joined here).
    pub lines: Vec<String>,
    pub changed: bool,
    pub note_zh: String,
}

pub const TEXT_ENGLISH_SCHEMA_NAME: &str = "sop_text_english";
/// Most lines of a corrected text.
pub const TEXT_ENGLISH_MAX_LINES: usize = 12;
/// Most characters of one line.
pub const TEXT_ENGLISH_LINE_CHARS: usize = 400;

pub fn text_english_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "maxItems": TEXT_ENGLISH_MAX_LINES,
                "items": { "type": "string", "maxLength": TEXT_ENGLISH_LINE_CHARS }
            },
            "changed": { "type": "boolean" },
            "note_zh": { "type": "string", "maxLength": 120 }
        },
        "required": ["lines", "changed", "note_zh"],
        "additionalProperties": false
    })
}

/// The blanks students write in (`___`): a correction keeps every one of them.
fn blank_count(text: &str) -> usize {
    let mut count = 0;
    let mut run = 0;
    for c in text.chars().chain(std::iter::once(' ')) {
        if c == '_' {
            run += 1;
        } else {
            if run >= 2 {
                count += 1;
            }
            run = 0;
        }
    }
    count
}

fn text_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Checks a corrected English text: English only, every blank kept, as many lines as the
/// teacher wrote (when she wrote several), not much longer than hers.
pub(crate) fn evaluate_text_english(
    raw: &RawTextEnglish,
    original: &str,
) -> Checked<EnglishSuggestion> {
    let mut problems: Vec<String> = Vec::new();
    let lines: Vec<String> = raw
        .lines
        .iter()
        .map(|l| super::super::checks::clean_markup(l))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    let english = lines.join("\n");
    let mine = text_lines(original);
    if english.is_empty() {
        problems.push("lines 是空的。".to_string());
    } else if contains_cjk(&english) {
        problems.push("lines 只能用英文，不可以有中文。".to_string());
    } else {
        let (want, got) = (blank_count(original), blank_count(&english));
        if want != got {
            problems.push(format!(
                "教師的文字有 {want} 個空格（___），修改後也要剛好 {want} 個，現在有 {got} 個。"
            ));
        }
        if mine.len() > 1 && lines.len() != mine.len() {
            problems.push(format!(
                "教師的文字有 {} 行，lines 也要 {} 行（一行對一行），現在有 {} 行。",
                mine.len(),
                mine.len(),
                lines.len()
            ));
        }
        let limit = (original.chars().count() * 2 + 200).min(crate::sop::MAX_FIELD_CHARS);
        if english.chars().count() > limit {
            problems.push(format!(
                "修改後太長了（{} 字），只修文法與用字，不要加上新的內容。",
                english.chars().count()
            ));
        }
    }
    if !problems.is_empty() {
        return Checked {
            output: None,
            problems,
        };
    }
    let same = super::super::checks::norm(&english) == super::super::checks::norm(original);
    let mut note = super::super::checks::clean_markup(&raw.note_zh);
    if note.is_empty() || !contains_cjk(&note) {
        note = if same {
            "這段英文已經很好，不需要改。".to_string()
        } else if contains_cjk(original) {
            "把中文的部分寫成英文。".to_string()
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

/// 「幫我修英文」 in steps 5–7: one short call, answered in the request (no job). `paragraphs`:
/// the paragraphs a task is about (they help keep its meaning). Returns the `AiResult` JSON.
pub async fn suggest_text_english(
    state: &AppState,
    teacher_id: i64,
    track: &str,
    lesson: &crate::teaching_context::LessonRef,
    field: TextField,
    text: &str,
    paragraphs: &[Paragraph],
) -> AppResult<Value> {
    if !state.ai.enabled() {
        return Ok(result_json(AiResult::<Value>::disabled()));
    }
    let passage = super::super::numbered_passage(paragraphs);
    let has_passage = !passage.is_empty();
    let vars: HashMap<&'static str, String> = HashMap::from([
        ("school_track", track_label(track).to_string()),
        ("text_kind", field.label().to_string()),
        (
            "paragraph_text",
            if has_passage {
                passage
            } else {
                "（沒有）".to_string()
            },
        ),
        ("teacher_text", text.trim().to_string()),
    ]);
    let mut call = new_call(
        KEY_TEXT_ENGLISH,
        SC_TEXT_ENGLISH,
        TEXT_ENGLISH_SCHEMA_NAME,
        text_english_schema(),
        vars,
        // Answered by the fake of 幫我修英文 (`sop::fake::english`), which tells the two apart by
        // `text_kind`.
        json!({
            "task": "sop_objective_english",
            "text_kind": field,
            "english": text.trim(),
        }),
        900,
        0.2,
        json!({ "sop": { "lesson_id": lesson.id, "field": field, "paragraphs": paragraphs.len() } }),
    );
    call.required = vec![("teacher_text", "教師寫的文字")];
    // The teacher's own words are the input; only the material's paragraphs are kept out of the
    // log.
    call.redact = if has_passage {
        vec![("paragraph_text", "課例教材")]
    } else {
        Vec::new()
    };
    let original = text.trim().to_string();
    let outcome = generation::call_in(
        state,
        teacher_id,
        (Some(lesson.unit_id), Some(lesson.id)),
        call,
        |_| Vec::new(),
        |raw: &RawTextEnglish| evaluate_text_english(raw, &original),
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

    fn raw(lines: &[&str]) -> RawTextEnglish {
        RawTextEnglish {
            lines: lines.iter().map(|l| l.to_string()).collect(),
            changed: true,
            note_zh: String::new(),
        }
    }

    #[test]
    fn a_correction_keeps_lines_and_blanks_and_reports_no_change() {
        let original = "Why Mrs. Lin start the stand?\nShe started it because ____.";
        let ok = evaluate_text_english(
            &raw(&[
                "Why did Mrs. Lin start the stand?",
                "She started it because ____.",
            ]),
            original,
        );
        assert!(ok.problems.is_empty(), "{:?}", ok.problems);
        let out = ok.output.unwrap();
        assert!(out.changed);
        assert_eq!(out.english.lines().count(), 2);
        assert!(!out.note.is_empty());

        let same = evaluate_text_english(
            &raw(&["Find the topic sentence."]),
            "Find the  topic sentence.",
        );
        assert!(!same.output.unwrap().changed);
    }

    #[test]
    fn a_correction_without_blanks_lines_or_english_is_refused() {
        let original = "Fill in: The stand has ___ umbrellas.\nTalk with partner.";
        for (bad, why) in [
            (
                vec!["The stand has twelve umbrellas.", "Talk with your partner."],
                "空格",
            ),
            (
                vec!["The stand has ___ umbrellas. Talk with your partner."],
                "行",
            ),
            (vec!["攤子有 ___ 把傘。", "和同學討論。"], "中文"),
            (vec![], "空的"),
        ] {
            let c = evaluate_text_english(&raw(&bad), original);
            assert!(c.output.is_none(), "{bad:?}");
            assert!(
                c.problems.iter().any(|p| p.contains(why)),
                "{why}: {:?}",
                c.problems
            );
        }
        let long = "word ".repeat(200);
        let c = evaluate_text_english(&raw(&[&long]), "Short question?");
        assert!(c.problems.iter().any(|p| p.contains("太長")));
    }

    #[test]
    fn chinese_written_by_the_teacher_may_become_one_english_line() {
        let c = evaluate_text_english(
            &raw(&["Underline the sentence that shows the turn."]),
            "請學生畫出轉折的句子",
        );
        assert!(c.problems.is_empty());
        assert!(c.output.unwrap().changed);
    }

    #[test]
    fn blanks_are_runs_of_two_or_more_underscores() {
        assert_eq!(blank_count("a ___ b ____ c _ d"), 2);
        assert_eq!(blank_count("___"), 1);
    }
}
