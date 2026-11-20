//! Small text operations shared by prompts, validation and presentation.

/// Text shown to the model for an optional teacher field.
pub fn or_blank(s: &str) -> String {
    if s.trim().is_empty() {
        "（尚未填寫）".to_string()
    } else {
        s.to_string()
    }
}

/// Clips to `max` characters, adding an ellipsis when something was cut.
pub fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
