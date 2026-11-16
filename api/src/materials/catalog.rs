//! The three library areas (docs/API.md「Phase 2.9：教材參考、我的教材、學習單範本」): who a material
//! or template belongs to (`scope`), what it is for (`purpose`), the fixed category lists of the
//! platform areas, free tags, and the category a worksheet template is given when nobody chose
//! one.
//!
//! - **教材參考**: `scope = platform`, `purpose = material` (the knowledge base's texts).
//! - **我的教材**: `scope = personal`, `purpose = material` (a teacher's own uploads).
//! - **學習單範本**: templates; platform ones come from `scope = platform`,
//!   `purpose = worksheet_sample` materials, a teacher's own from her `worksheet_sample` uploads.
//!
//! Platform rows are readable and usable by every teacher and writable only by
//! [`crate::auth::Capability::ManageKnowledge`]; personal rows stay owner-only.

use crate::error::{AppError, AppResult};

pub const SCOPE_PERSONAL: &str = "personal";
pub const SCOPE_PLATFORM: &str = "platform";

pub const PURPOSE_MATERIAL: &str = "material";
pub const PURPOSE_SAMPLE: &str = "worksheet_sample";

/// 教材參考 (platform materials).
pub const REFERENCE_CATEGORIES: [&str; 6] =
    ["學測英文", "統測英文", "課綱", "詞彙表", "考試說明", "其他"];
/// 學習單範本 (samples and templates).
pub const SAMPLE_CATEGORIES: [&str; 9] = [
    "字彙",
    "文法句型",
    "閱讀理解",
    "課文講義",
    "自學講義",
    "寫作",
    "聽說",
    "學習反思",
    "其他",
];

pub const MAX_TAG_CHARS: usize = 20;
pub const MAX_TAGS: usize = 12;

/// `owner_label` of the cards.
pub const OWNER_PLATFORM_MATERIAL: &str = "Manna 知識庫";
pub const OWNER_PLATFORM_TEMPLATE: &str = "Manna 範本";
pub const OWNER_ME: &str = "我";

/// The `file_type` values of a material card, in the order the facets list them.
pub const FILE_TYPES: [&str; 7] = ["pdf", "docx", "text", "image", "audio", "video", "link"];

/// `file_type` of `materials m` in SQL: the same mapping as [`file_type`].
pub const FILE_TYPE_SQL: &str = "(CASE WHEN m.kind = 'document' AND m.ext = 'pdf' THEN 'pdf'
       WHEN m.kind = 'document' AND m.ext = 'docx' THEN 'docx'
       WHEN m.kind IN ('document', 'text') THEN 'text'
       ELSE m.kind END)";

/// A card's `file_type`: PDF and Word documents by extension, a `.txt` upload and pasted text as
/// `text`, the other kinds by kind.
pub fn file_type(kind: &str, ext: Option<&str>) -> &'static str {
    match (kind, ext) {
        ("document", Some("pdf")) => "pdf",
        ("document", Some("docx")) => "docx",
        ("document", _) | ("text", _) => "text",
        ("image", _) => "image",
        ("audio", _) => "audio",
        ("video", _) => "video",
        _ => "link",
    }
}

/// Where a new material goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub scope: &'static str,
    pub purpose: &'static str,
    pub category: String,
    pub tags: Vec<String>,
}

impl Placement {
    /// A teacher's own material (我的教材) or worksheet sample (我的範本).
    pub fn personal(purpose: &'static str) -> Self {
        Self {
            scope: SCOPE_PERSONAL,
            purpose,
            category: String::new(),
            tags: Vec::new(),
        }
    }

    /// A platform material (教材參考) or sample (學習單範本), already validated.
    pub fn platform(purpose: &'static str, category: String, tags: Vec<String>) -> Self {
        Self {
            scope: SCOPE_PLATFORM,
            purpose,
            category,
            tags,
        }
    }

    pub fn is_platform(&self) -> bool {
        self.scope == SCOPE_PLATFORM
    }
}

/// `purpose` from a request: `material` (also when absent or blank) or `worksheet_sample`.
pub fn parse_purpose(raw: Option<&str>) -> AppResult<&'static str> {
    match raw.map(str::trim).unwrap_or_default() {
        "" | PURPOSE_MATERIAL => Ok(PURPOSE_MATERIAL),
        PURPOSE_SAMPLE => Ok(PURPOSE_SAMPLE),
        other => Err(AppError::bad_request(format!(
            "「{other}」不是 purpose 的選項（material、worksheet_sample）。"
        ))),
    }
}

/// The category list of a purpose.
pub fn categories_for(purpose: &str) -> &'static [&'static str] {
    if purpose == PURPOSE_SAMPLE {
        &SAMPLE_CATEGORIES
    } else {
        &REFERENCE_CATEGORIES
    }
}

/// A category from `list`. With `allow_empty`, a blank value is `""` (for a template: let the
/// server suggest one); otherwise blank is a 400 too.
pub fn check_category(list: &[&str], raw: &str, allow_empty: bool) -> AppResult<String> {
    let value = raw.trim();
    if value.is_empty() && allow_empty {
        return Ok(String::new());
    }
    if list.contains(&value) {
        return Ok(value.to_string());
    }
    let options = list.join("、");
    Err(AppError::bad_request(if value.is_empty() {
        format!("請選擇分類（{options}）。")
    } else {
        format!("「{value}」不是分類的選項（{options}）。")
    }))
}

/// Tags as given: trimmed, blanks and repeats dropped (first spelling kept, order kept). More
/// than [`MAX_TAGS`] or one longer than [`MAX_TAG_CHARS`] is a 400.
pub fn clean_tags<I, S>(tags: I) -> AppResult<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.as_ref().trim();
        if tag.is_empty() || out.iter().any(|t| t == tag) {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(AppError::bad_request(format!(
                "標籤「{tag}」太長了：每個標籤最多 {MAX_TAG_CHARS} 字。"
            )));
        }
        out.push(tag.to_string());
    }
    if out.len() > MAX_TAGS {
        return Err(AppError::bad_request(format!("標籤最多 {MAX_TAGS} 個。")));
    }
    Ok(out)
}

/// Comma-separated tags from a form field (`,`, `，` and `、` all separate).
pub fn split_tags(raw: &str) -> AppResult<Vec<String>> {
    clean_tags(raw.split([',', '，', '、']))
}

/// The category a template gets when nobody chose one, from its `worksheet_kind` first and
/// then its title: 單字/字彙/vocab → 字彙; 文法/句型/grammar/sentence → 文法句型;
/// 閱讀/reading → 閱讀理解; 自學 → 自學講義; 課文/講義 → 課文講義; 寫作/writing → 寫作;
/// 反思 → 學習反思; otherwise 其他. (自學 is tried before 講義 so that 「自學講義」 is not taken
/// for 課文講義.)
pub fn suggest_sample_category(worksheet_kind: &str, title: &str) -> &'static str {
    const RULES: [(&[&str], &str); 7] = [
        (&["單字", "字彙", "vocab"], "字彙"),
        (&["文法", "句型", "grammar", "sentence"], "文法句型"),
        (&["閱讀", "reading"], "閱讀理解"),
        (&["自學"], "自學講義"),
        (&["課文", "講義"], "課文講義"),
        (&["寫作", "writing"], "寫作"),
        (&["反思"], "學習反思"),
    ];
    for text in [worksheet_kind, title] {
        let text = text.to_lowercase();
        if let Some((_, category)) = RULES
            .iter()
            .find(|(words, _)| words.iter().any(|w| text.contains(w)))
        {
            return category;
        }
    }
    "其他"
}

/// Characters of text a material card shows.
pub const EXCERPT_CHARS: usize = 280;

/// The first [`EXCERPT_CHARS`] characters of `text` with all whitespace collapsed to single
/// spaces ("" when there is no text).
pub fn excerpt(text: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for word in text.split_whitespace() {
        if count > 0 {
            // A space only when a character can still follow it.
            if count + 1 >= EXCERPT_CHARS {
                break;
            }
            out.push(' ');
            count += 1;
        }
        for c in word.chars().take(EXCERPT_CHARS - count) {
            out.push(c);
            count += 1;
        }
        if count >= EXCERPT_CHARS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_types_follow_kind_and_extension() {
        assert_eq!(file_type("document", Some("pdf")), "pdf");
        assert_eq!(file_type("document", Some("docx")), "docx");
        assert_eq!(file_type("document", Some("txt")), "text");
        assert_eq!(file_type("text", None), "text");
        assert_eq!(file_type("image", Some("png")), "image");
        assert_eq!(file_type("audio", Some("mp3")), "audio");
        assert_eq!(file_type("video", Some("mp4")), "video");
        assert_eq!(file_type("link", None), "link");
        for t in ["pdf", "docx", "text", "image", "audio", "video", "link"] {
            assert!(FILE_TYPES.contains(&t));
        }
    }

    #[test]
    fn purposes_and_categories_are_checked() {
        assert_eq!(parse_purpose(None).unwrap(), PURPOSE_MATERIAL);
        assert_eq!(parse_purpose(Some(" ")).unwrap(), PURPOSE_MATERIAL);
        assert_eq!(
            parse_purpose(Some("worksheet_sample")).unwrap(),
            PURPOSE_SAMPLE
        );
        assert!(parse_purpose(Some("sample")).is_err());

        assert_eq!(
            check_category(&REFERENCE_CATEGORIES, " 學測英文 ", false).unwrap(),
            "學測英文"
        );
        assert!(check_category(&REFERENCE_CATEGORIES, "", false).is_err());
        assert!(check_category(&REFERENCE_CATEGORIES, "字彙", false).is_err());
        assert_eq!(check_category(&SAMPLE_CATEGORIES, "", true).unwrap(), "");
        assert_eq!(categories_for(PURPOSE_SAMPLE), &SAMPLE_CATEGORIES);
        assert_eq!(categories_for(PURPOSE_MATERIAL), &REFERENCE_CATEGORIES);
    }

    #[test]
    fn tags_are_trimmed_deduplicated_and_bounded() {
        assert_eq!(
            split_tags(" 114, 試題，三民、114 ,, ").unwrap(),
            ["114", "試題", "三民"]
        );
        assert!(split_tags(&"字".repeat(21)).is_err());
        assert!(split_tags(&"字".repeat(20)).is_ok());
        let many: Vec<String> = (0..13).map(|i| format!("t{i}")).collect();
        assert!(clean_tags(&many).is_err());
        assert_eq!(clean_tags(&many[..12]).unwrap().len(), 12);
    }

    #[test]
    fn categories_are_suggested_from_the_kind_then_the_title() {
        assert_eq!(suggest_sample_category("單字練習", ""), "字彙");
        assert_eq!(suggest_sample_category("Vocabulary Builder", ""), "字彙");
        assert_eq!(suggest_sample_category("句型練習", ""), "文法句型");
        assert_eq!(suggest_sample_category("Sentence patterns", ""), "文法句型");
        assert_eq!(suggest_sample_category("課文深度閱讀", ""), "閱讀理解");
        assert_eq!(suggest_sample_category("Reading Guide", ""), "閱讀理解");
        assert_eq!(suggest_sample_category("自學講義", ""), "自學講義");
        assert_eq!(suggest_sample_category("課文講義", ""), "課文講義");
        assert_eq!(suggest_sample_category("寫作引導", ""), "寫作");
        assert_eq!(suggest_sample_category("Writing task", ""), "寫作");
        assert_eq!(suggest_sample_category("學習反思單", ""), "學習反思");
        // The kind says nothing: the title decides.
        assert_eq!(
            suggest_sample_category("學習單", "L3 Grammar 版型"),
            "文法句型"
        );
        assert_eq!(suggest_sample_category("", "Unit 2"), "其他");
    }

    #[test]
    fn excerpts_collapse_whitespace_and_stop_at_280() {
        assert_eq!(excerpt(""), "");
        assert_eq!(excerpt("  a\n\n b\tc  "), "a b c");
        let long = "word ".repeat(200);
        let e = excerpt(&long);
        assert!(
            e.chars().count() <= EXCERPT_CHARS && e.chars().count() > 270,
            "{e}"
        );
        assert!(e.starts_with("word word") && !e.ends_with(' '));
        assert_eq!(excerpt(&"字".repeat(500)).chars().count(), EXCERPT_CHARS);
    }
}
