//! 內容目標與語言目標 (docs/SIOP_DESIGN_RULES.md R1–R3, G1–G3; docs/API.md「SIOP 目標」).
//!
//! * **G1**: every generator takes two optional fields, `content_objective` and
//!   `language_objective` (one objective per line, ≤300 characters each). They reach the
//!   prompts as `{{teacher_content_objective}}` and `{{teacher_language_objective}}`; a prompt
//!   version from before the two fields gets them as one line in `{{teacher_objective}}`
//!   ([`required`]). A worksheet keeps them as structured objectives in `content.objectives`
//!   (ids `teacher-c1`, `teacher-l1` …, [`from_settings`]), which print at the top of the sheet
//!   and which the steps refer to as O1, O2 ….
//! * **G2/G3**: when the teacher left both empty, `POST /api/objective-drafts` drafts them from
//!   the material ([`draft`]). The two kinds are drafted one after the other ([`Stage`]): the
//!   內容目標 first; once the teacher decided them, a second request drafts the 語言目標 from
//!   the ones she kept (which reading, listening, speaking or writing shows that students reached
//!   them; strategy first, R2). The server checks R1 by rule ([`check_rules`]); a second call
//!   judges R3 for every draft on its own ([`evaluate_check`]); only drafts that pass both are
//!   shown, and when the asked-for group is left without one the server drafts once more. The
//!   teacher's 採用／修改／改寫 goes to `POST /api/ai/:id/decision`.

pub mod draft;
pub mod fake;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::checks::contains_cjk;
use crate::prompts::{has_placeholder, PromptVersion};
use crate::sop::checks::clean_markup;
use crate::sop::ObjectiveType;
use crate::worksheet::template::{enum_of, obj, s_max};
use crate::worksheet::{Objective, Origin, Settings};

// ---------------------------------------------------------------- G1: the two fields

/// Most characters of one field (both lines together).
pub const MAX_FIELD_CHARS: usize = 300;

pub const VAR_CONTENT: &str = "teacher_content_objective";
pub const VAR_LANGUAGE: &str = "teacher_language_objective";
/// Both groups in one line, for prompt versions from before the two fields.
pub const VAR_COMBINED: &str = "teacher_objective";
pub const LABEL_CONTENT: &str = "內容目標（教師寫的或採用的）";
pub const LABEL_LANGUAGE: &str = "語言目標（教師寫的或採用的）";
/// The heading the single objective has always had in the prompts.
pub const LABEL_COMBINED: &str = "學習目標（教師寫的）";
/// What a prompt sees for an empty field.
pub const NONE: &str = "（沒有）";

/// The objectives of one field: one per line, trimmed, blank lines dropped.
pub fn lines(field: &str) -> Vec<String> {
    field
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// A field as it is stored: its lines trimmed and joined with `\n`, `None` when empty. Longer
/// than [`MAX_FIELD_CHARS`] is an error naming the field.
pub fn normalised_field(label: &str, field: Option<&str>) -> Result<Option<String>, String> {
    let joined = lines(field.unwrap_or_default()).join("\n");
    if joined.is_empty() {
        return Ok(None);
    }
    if joined.chars().count() > MAX_FIELD_CHARS {
        return Err(format!("{label}最多 {MAX_FIELD_CHARS} 字。"));
    }
    Ok(Some(joined))
}

/// The teacher's objectives, as the prompts get them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeacherObjectives {
    pub content: Vec<String>,
    pub language: Vec<String>,
}

/// An objective of a worksheet in one line: its text, with the students' line after it when
/// that says something else (a lesson SOP's objective has both).
fn objective_line(o: &Objective) -> String {
    let text = o.text.trim();
    let en = o.student_facing_en.trim();
    match (text.is_empty(), en.is_empty()) {
        (true, _) => en.to_string(),
        (_, true) => text.to_string(),
        _ if text == en => text.to_string(),
        _ => format!("{text}（{en}）"),
    }
}

impl TeacherObjectives {
    pub fn from_fields(content: Option<&str>, language: Option<&str>) -> Self {
        Self {
            content: lines(content.unwrap_or_default()),
            language: lines(language.unwrap_or_default()),
        }
    }

    /// A worksheet's objectives, grouped by kind (the ones from a lesson SOP included).
    pub fn from_worksheet(objectives: &[Objective]) -> Self {
        let mut out = Self::default();
        for o in objectives {
            let line = objective_line(o);
            if line.is_empty() {
                continue;
            }
            match o.kind {
                ObjectiveType::Content => out.content.push(line),
                ObjectiveType::Language => out.language.push(line),
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty() && self.language.is_empty()
    }

    /// The 內容目標, joined with 「；」 ("" when none).
    pub fn content_text(&self) -> String {
        self.content.join("；")
    }

    /// The 語言目標, joined with 「；」 ("" when none).
    pub fn language_text(&self) -> String {
        self.language.join("；")
    }

    /// Both groups in one line, 「內容目標：…｜語言目標：…」 ("" when none).
    pub fn combined(&self) -> String {
        let mut parts = Vec::new();
        if !self.content.is_empty() {
            parts.push(format!("內容目標：{}", self.content_text()));
        }
        if !self.language.is_empty() {
            parts.push(format!("語言目標：{}", self.language_text()));
        }
        parts.join("｜")
    }

    /// The three prompt variables; an empty one is 「（沒有）」.
    pub fn vars(&self) -> [(&'static str, String); 3] {
        let or_none = |s: String| if s.is_empty() { NONE.to_string() } else { s };
        [
            (VAR_CONTENT, or_none(self.content_text())),
            (VAR_LANGUAGE, or_none(self.language_text())),
            (VAR_COMBINED, or_none(self.combined())),
        ]
    }
}

/// The objective variables a prompt version must get even when its text does not mention
/// them (appended under their heading by `prompts::render_user_with`): the two new ones when
/// the version uses either of them, otherwise the combined line it has always had.
pub fn required(prompt: &PromptVersion) -> Vec<(&'static str, &'static str)> {
    let has = |name: &str| {
        has_placeholder(&prompt.system_text, name) || has_placeholder(&prompt.user_template, name)
    };
    if has(VAR_CONTENT) || has(VAR_LANGUAGE) {
        vec![(VAR_CONTENT, LABEL_CONTENT), (VAR_LANGUAGE, LABEL_LANGUAGE)]
    } else {
        vec![(VAR_COMBINED, LABEL_COMBINED)]
    }
}

// ---------------------------------------------------------------- G1: a worksheet's objectives

/// Ids of the objectives that come from the two fields: `teacher-c1`, `teacher-l2` ….
pub const ID_PREFIX: &str = "teacher-";

pub fn is_from_settings(o: &Objective) -> bool {
    o.id.starts_with(ID_PREFIX)
}

/// The objectives of the two fields, as a worksheet keeps them: the 內容目標 first. Their
/// text is also the students' line (it is what prints under "Today I can …").
pub fn from_settings(settings: &Settings) -> Vec<Objective> {
    let t = settings.teacher_objectives();
    let group = |lines: &[String], kind: ObjectiveType, code: &str| -> Vec<Objective> {
        lines
            .iter()
            .enumerate()
            .map(|(i, line)| Objective {
                id: format!("{ID_PREFIX}{code}{}", i + 1),
                kind,
                text: line.clone(),
                student_facing_en: line.clone(),
                origin: Origin::Teacher,
                sop_id: None,
            })
            .collect()
    };
    let mut out = group(&t.content, ObjectiveType::Content, "c");
    out.extend(group(&t.language, ObjectiveType::Language, "l"));
    out
}

/// `stored` with the objectives of the two fields replaced by those of `settings`. The others
/// (a lesson SOP's, one the teacher added on the sheet) stay first.
pub fn synced(stored: &[Objective], settings: &Settings) -> Vec<Objective> {
    let mut out: Vec<Objective> = stored
        .iter()
        .filter(|o| !is_from_settings(o))
        .cloned()
        .collect();
    out.extend(from_settings(settings));
    out
}

/// What a generation works with: `stored` as it is when it already has the objectives of the
/// fields (the teacher may have edited them on the sheet), otherwise [`synced`] (a worksheet
/// stored before the fields went into `content.objectives`).
pub fn ensured(stored: &[Objective], settings: &Settings) -> Vec<Objective> {
    if stored.iter().any(is_from_settings) {
        stored.to_vec()
    } else {
        synced(stored, settings)
    }
}

// ---------------------------------------------------------------- G2: what the model drafts

pub const KEY_DRAFT: &str = "objective_draft";
pub const KEY_CHECK: &str = "objective_check";
pub const DRAFT_SCHEMA_NAME: &str = "objective_draft";
pub const CHECK_SCHEMA_NAME: &str = "objective_check";

/// R1: one objective per group as a rule, at most two.
pub const GROUP_MIN: usize = 1;
pub const GROUP_MAX: usize = 2;
pub const ZH_CHARS: usize = 100;
pub const EN_CHARS: usize = 180;
pub const RELATION_CHARS: usize = 160;
pub const CHECK_CHARS: usize = 100;
pub const REASON_CHARS: usize = 100;
/// A student's line longer than this is not one a teacher reads out in one breath.
pub const EN_WORDS_MAX: usize = 30;

/// R2's four kinds of language objective, in Manna's order: a learning strategy first.
pub const CATEGORIES: [(&str, &str); 4] = [
    ("strategy", "學習策略"),
    ("function", "語言功能"),
    ("structure", "語言結構或文法"),
    ("vocabulary", "學術字彙"),
];
pub const STRATEGY: &str = "strategy";

pub fn category_label(id: &str) -> &'static str {
    CATEGORIES
        .iter()
        .find(|(c, _)| *c == id)
        .map_or("", |(_, l)| *l)
}

/// The codes of the 內容目標 in a draft, which a 語言目標 says it supports.
pub const CONTENT_CODES: [&str; GROUP_MAX] = ["C1", "C2"];
pub const LANGUAGE_CODES: [&str; GROUP_MAX] = ["L1", "L2"];

/// Which kind a draft request asks for. The 語言目標 depend on the 內容目標 (they say which
/// language students need to show they reached them), so they are drafted after the teacher
/// decided the 內容目標, from the ones she kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    #[default]
    Content,
    Language,
}

impl Stage {
    /// The kind of objective this stage drafts.
    pub fn kind(self) -> ObjectiveType {
        match self {
            Self::Content => ObjectiveType::Content,
            Self::Language => ObjectiveType::Language,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Language => "language",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawContentDraft {
    pub zh: String,
    pub en: String,
    #[serde(default)]
    pub check_zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawLanguageDraft {
    pub category: String,
    pub supports: String,
    pub zh: String,
    pub en: String,
    #[serde(default)]
    pub relation_zh: String,
    #[serde(default)]
    pub check_zh: String,
}

/// What the model writes for `objective_draft`: the 內容目標 first (the schema's order is the
/// order the gateway writes them), then the 語言目標 derived from them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawDrafts {
    #[serde(default)]
    pub content_objectives: Vec<RawContentDraft>,
    #[serde(default)]
    pub language_objectives: Vec<RawLanguageDraft>,
}

impl RawDrafts {
    /// The number of drafts, which is also where the 語言目標 start in [`Draft::index`].
    pub fn content_len(&self) -> usize {
        self.content_objectives.len()
    }
}

/// `objective_draft` for one stage: 1–2 of the asked-for kind (the other group stays empty),
/// every string capped.
pub fn draft_schema(stage: Stage) -> Value {
    let content = obj(vec![
        ("zh", s_max(ZH_CHARS)),
        ("en", s_max(EN_CHARS)),
        ("check_zh", s_max(CHECK_CHARS)),
    ]);
    let categories: Vec<&str> = CATEGORIES.iter().map(|(c, _)| *c).collect();
    let language = obj(vec![
        ("category", enum_of(&categories)),
        ("supports", enum_of(&CONTENT_CODES)),
        ("zh", s_max(ZH_CHARS)),
        ("en", s_max(EN_CHARS)),
        ("relation_zh", s_max(RELATION_CHARS)),
        ("check_zh", s_max(CHECK_CHARS)),
    ]);
    let group = |items: Value, asked: bool| {
        json!({
            "type": "array", "items": items,
            "minItems": if asked { GROUP_MIN } else { 0 },
            "maxItems": if asked { GROUP_MAX } else { 0 }
        })
    };
    obj(vec![
        (
            "content_objectives",
            group(content, stage == Stage::Content),
        ),
        (
            "language_objectives",
            group(language, stage == Stage::Language),
        ),
    ])
}

/// One draft as the teacher gets it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Draft {
    /// Its position in the model's output (the 內容目標 first): the `item_index` of the
    /// teacher's decision on it.
    pub index: usize,
    pub kind: ObjectiveType,
    /// "C1", "C2", "L1", "L2".
    pub code: String,
    /// The students' line, "I can …": what goes into the field when adopted.
    pub text: String,
    /// The same objective for the teacher, in Chinese.
    pub zh: String,
    /// 語言目標 only: one of [`CATEGORIES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// 語言目標 only: the code of the 內容目標 it serves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports: Option<String>,
    /// 語言目標 only: 「學生要用……，才能……」.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub relation_zh: String,
    /// How the teacher checks it before the end of the lesson.
    pub check_zh: String,
}

// ---------------------------------------------------------------- G2: R1 by rule

/// Verbs that name no performance (R1: not learn, know, understand).
const VAGUE_EN: [&str; 14] = [
    "learn",
    "learns",
    "learning",
    "know",
    "knows",
    "understand",
    "understands",
    "appreciate",
    "realize",
    "realise",
    "be",
    "become",
    "get",
    "feel",
];
/// The same in the teacher's Chinese line, at its start (after an optional 「學生能」).
const VAGUE_ZH: [&str; 9] = [
    "了解", "瞭解", "理解", "知道", "認識", "學會", "學習", "熟悉", "體會",
];

/// The words of `s`, lowercased, letters and apostrophes only.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_ascii_alphabetic() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The students' line in the form the sheet prints: "I can …". "Students will be able to …"
/// is put in that form; anything else is `None`.
pub fn i_can_line(en: &str) -> Option<String> {
    let t = en.trim();
    for prefix in [
        "i can ",
        "students will be able to ",
        "students will ",
        "students can ",
    ] {
        if t.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            let rest = t[prefix.len()..].trim_start();
            return (!rest.is_empty()).then(|| format!("I can {rest}"));
        }
    }
    None
}

/// "I can" at the start of a students' line: any case, spaces or tabs between and after the
/// words, or a colon ("I can: identify …"). What follows must start with something printable.
/// The web preview uses the same pattern (`printedObjective`, web/src/lib/objectives.ts).
static I_CAN_LEAD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^[ \t]*[Ii][ \t]+[Cc][Aa][Nn](?:[ \t]*[:：][ \t]*|[ \t]+)([^ \t][\s\S]*)$")
        .expect("valid regex")
});

/// A students' line as it prints under the heading 「Today I can …」: without its own leading
/// "I can", which the heading already says (「Today I can … / 1. I can identify …」 read it twice).
/// The stored line keeps it (SIOP: objectives are written "I can …"). A line that does not
/// start with "I can" ("I can't …", "Identify …"), or has nothing after it, prints unchanged.
pub fn printed_objective(line: &str) -> &str {
    I_CAN_LEAD
        .captures(line)
        .and_then(|c| c.get(1))
        .map_or(line, |m| m.as_str())
}

/// The verbs of an "I can …" line that say what the student does: the first word after
/// "I can", and every word right after "and" / "or" in the line.
fn main_verbs(line: &str) -> Vec<String> {
    let w = words(line);
    let mut out = Vec::new();
    if w.len() > 2 {
        out.push(w[2].clone());
    }
    for pair in w.windows(2) {
        if pair[0] == "and" || pair[0] == "or" {
            out.push(pair[1].clone());
        }
    }
    out
}

/// R1 for one draft: why its lines cannot be shown, if they cannot.
fn line_problem(zh: &str, en: &str) -> Option<String> {
    if zh.is_empty() || !contains_cjk(zh) {
        return Some("zh 要用繁體中文寫一句".to_string());
    }
    let zh_start = zh.trim_start_matches("學生能").trim_start_matches("能");
    if let Some(v) = VAGUE_ZH.iter().find(|v| zh_start.starts_with(*v)) {
        return Some(format!(
            "zh 以「{v}」開頭：看不到學生的表現，要用看得到表現的動詞"
        ));
    }
    if contains_cjk(en) || !en.chars().any(|c| c.is_ascii_alphabetic()) {
        return Some("en 要用英文寫".to_string());
    }
    if en.split_whitespace().count() > EN_WORDS_MAX {
        return Some(format!(
            "en 超過 {EN_WORDS_MAX} 個字：學生一次讀不完，也不容易唸給學生聽"
        ));
    }
    if let Some(v) = main_verbs(en)
        .into_iter()
        .find(|v| VAGUE_EN.contains(&v.as_str()))
    {
        return Some(format!(
            "en 用了「{v}」：看不到學生的表現，要用 identify、describe、compare、explain 這類動詞"
        ));
    }
    None
}

fn clean(s: &str, max: usize) -> String {
    let c = clean_markup(s);
    let c = c.split_whitespace().collect::<Vec<_>>().join(" ");
    c.chars().take(max).collect::<String>().trim().to_string()
}

/// What the rule check left of a draft output.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleOutcome {
    /// The drafts that pass R1, the 內容目標 first; a 學習策略 first among the 語言目標.
    pub drafts: Vec<Draft>,
    /// Why each dropped draft was dropped ("C1：…"), for the record and the next draft.
    pub dropped: Vec<String>,
    /// What the code changed ("Students will be able to" → "I can", a strategy moved first).
    pub repairs: Vec<String>,
}

impl RuleOutcome {
    pub fn has(&self, kind: ObjectiveType) -> bool {
        self.drafts.iter().any(|d| d.kind == kind)
    }

    /// The group the stage asked for has a draft.
    pub fn complete(&self, stage: Stage) -> bool {
        self.has(stage.kind())
    }
}

/// R1 (and the content → language relation) by rule, for every draft on its own:
///
/// * the students' line is English, starts "I can" ("Students will be able to …" is put in that
///   form), has at most [`EN_WORDS_MAX`] words and no vague verb (learn, know, understand …);
/// * the teacher's line is Chinese and does not start with 了解／理解／知道／學會 …;
/// * a 語言目標 has a category from the four, supports a 內容目標 that passed, and says its
///   relation as 「學生要用……，才能……」;
/// * at most two per group, no repeats.
///
/// A 學習策略 is moved to the front of the 語言目標 (R2: strategy first), unless the unit
/// needs another kind this time (`strategy_first = false`, see [`draft`]).
pub fn check_rules(raw: &RawDrafts, strategy_first: bool) -> RuleOutcome {
    check_rules_after(raw, strategy_first, 0)
}

/// [`check_rules`] when the teacher already decided `decided` 內容目標 (`C1` …): a 語言目標 may
/// support any of them.
pub fn check_rules_after(raw: &RawDrafts, strategy_first: bool, decided: usize) -> RuleOutcome {
    let mut out = RuleOutcome {
        drafts: Vec::new(),
        dropped: Vec::new(),
        repairs: Vec::new(),
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut passed_content: HashSet<&str> = CONTENT_CODES.iter().copied().take(decided).collect();
    let fix_line = |code: &str, en: &str, out: &mut RuleOutcome| -> Option<String> {
        let en = clean(en, EN_CHARS);
        match i_can_line(&en) {
            Some(line) => {
                if !en.to_lowercase().starts_with("i can ") {
                    out.repairs.push(format!("{code}：學生版改成以 I can 開頭"));
                }
                Some(line)
            }
            None => {
                out.dropped.push(format!(
                    "{code}：en 要以 \"I can\" 開頭，寫成學生讀得懂的一句"
                ));
                None
            }
        }
    };
    for (i, c) in raw.content_objectives.iter().take(GROUP_MAX).enumerate() {
        let code = CONTENT_CODES[i];
        let zh = clean(&c.zh, ZH_CHARS);
        let Some(en) = fix_line(code, &c.en, &mut out) else {
            continue;
        };
        if let Some(why) = line_problem(&zh, &en) {
            out.dropped.push(format!("{code}：{why}"));
            continue;
        }
        if !seen.insert(en.to_lowercase()) {
            out.dropped.push(format!("{code}：與另一個目標重複"));
            continue;
        }
        passed_content.insert(code);
        out.drafts.push(Draft {
            index: i,
            kind: ObjectiveType::Content,
            code: code.to_string(),
            text: en,
            zh,
            category: None,
            supports: None,
            relation_zh: String::new(),
            check_zh: clean(&c.check_zh, CHECK_CHARS),
        });
    }
    let first_language = raw.content_len();
    let mut language: Vec<Draft> = Vec::new();
    for (i, l) in raw.language_objectives.iter().take(GROUP_MAX).enumerate() {
        let code = LANGUAGE_CODES[i];
        let category = l.category.trim();
        if category_label(category).is_empty() {
            out.dropped.push(format!(
                "{code}：category「{}」不是四類之一",
                clean(category, 20)
            ));
            continue;
        }
        let zh = clean(&l.zh, ZH_CHARS);
        let Some(en) = fix_line(code, &l.en, &mut out) else {
            continue;
        };
        if let Some(why) = line_problem(&zh, &en) {
            out.dropped.push(format!("{code}：{why}"));
            continue;
        }
        let supports = l.supports.trim();
        if !passed_content.contains(supports) {
            out.dropped.push(format!(
                "{code}：supports「{}」不是一個可用的內容目標",
                clean(supports, 10)
            ));
            continue;
        }
        let relation = clean(&l.relation_zh, RELATION_CHARS);
        if !(relation.starts_with("學生要") && relation.contains("才能")) {
            out.dropped.push(format!(
                "{code}：relation_zh 要寫成「學生要用……，才能……」，說清楚它怎麼幫學生達成內容目標"
            ));
            continue;
        }
        if !seen.insert(en.to_lowercase()) {
            out.dropped.push(format!("{code}：與另一個目標重複"));
            continue;
        }
        language.push(Draft {
            index: first_language + i,
            kind: ObjectiveType::Language,
            code: code.to_string(),
            text: en,
            zh,
            category: Some(category.to_string()),
            supports: Some(supports.to_string()),
            relation_zh: relation,
            check_zh: clean(&l.check_zh, CHECK_CHARS),
        });
    }
    // R2: a learning strategy first (stable otherwise).
    if strategy_first
        && language.len() > 1
        && language[0].category.as_deref() != Some(STRATEGY)
        && language
            .iter()
            .any(|d| d.category.as_deref() == Some(STRATEGY))
    {
        language.sort_by_key(|d| d.category.as_deref() != Some(STRATEGY));
        out.repairs.push("語言目標：學習策略移到第一個".to_string());
    }
    out.drafts.extend(language);
    out
}

// ---------------------------------------------------------------- G3: R3 by the model

/// What the judge may find wrong with a draft (R3, plus R1's activity and readability).
pub const PROBLEMS: [(&str, &str); 6] = [
    ("not_core", "沒有對應這篇教材的核心概念或課綱"),
    ("not_assessable", "看不到學生的表現，無法觀察或檢核"),
    ("activity", "寫的是活動或流程，不是學生要學會的事"),
    ("not_readable", "學生讀不懂"),
    (
        "not_needed",
        "不是學生還做不好、能推進學術英語、與主題直接相關的語言",
    ),
    (
        "no_relation",
        "說不出「學生要用語言目標，才能達成內容目標」的關係",
    ),
];
/// Problems that only a 語言目標 can have.
const LANGUAGE_ONLY: [&str; 2] = ["not_needed", "no_relation"];

pub fn problem_label(id: &str) -> &'static str {
    PROBLEMS
        .iter()
        .find(|(p, _)| *p == id)
        .map_or("", |(_, l)| *l)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawVerdict {
    pub id: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub problems: Vec<String>,
}

/// The judge's output: one verdict per draft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawCheck {
    pub items: Vec<RawVerdict>,
}

/// `objective_check`: exactly one verdict per code, its problems from [`PROBLEMS`].
pub fn check_schema(codes: &[String]) -> Value {
    let codes: Vec<&str> = codes.iter().map(String::as_str).collect();
    let problems: Vec<&str> = PROBLEMS.iter().map(|(p, _)| *p).collect();
    obj(vec![(
        "items",
        json!({
            "type": "array",
            "items": obj(vec![
                ("id", enum_of(&codes)),
                ("reason", s_max(REASON_CHARS)),
                ("problems", json!({
                    "type": "array", "items": enum_of(&problems), "maxItems": PROBLEMS.len()
                })),
            ]),
            "minItems": codes.len(),
            "maxItems": codes.len()
        }),
    )])
}

/// One draft as the judge saw it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Verdict {
    pub code: String,
    /// The problems that apply to its kind; empty: it passes R3.
    pub problems: Vec<String>,
    pub reason: String,
}

/// The judge's output checked: one verdict per draft it was given (the first one when it wrote
/// two), problems outside the set and problems of the other kind dropped. `missing` are the
/// codes it left out (a failure: one repair call, after which they do not pass).
pub fn evaluate_check(raw: &RawCheck, drafts: &[Draft]) -> (Vec<Verdict>, Vec<String>) {
    let mut out: Vec<Verdict> = Vec::new();
    for v in &raw.items {
        let code = v.id.trim();
        let Some(d) = drafts.iter().find(|d| d.code == code) else {
            continue;
        };
        if out.iter().any(|o| o.code == code) {
            continue;
        }
        let mut problems: Vec<String> = Vec::new();
        for p in &v.problems {
            let p = p.trim();
            let applies = !problem_label(p).is_empty()
                && (d.kind == ObjectiveType::Language || !LANGUAGE_ONLY.contains(&p));
            if applies && !problems.iter().any(|x| x == p) {
                problems.push(p.to_string());
            }
        }
        out.push(Verdict {
            code: code.to_string(),
            problems,
            reason: clean(&v.reason, REASON_CHARS),
        });
    }
    let missing = drafts
        .iter()
        .filter(|d| !out.iter().any(|v| v.code == d.code))
        .map(|d| d.code.clone())
        .collect();
    (out, missing)
}

/// The drafts that pass R3: a verdict without problems, and (for a 語言目標) the 內容目標 it
/// supports passed too. The others with why, for the record and the next draft.
pub fn passing(drafts: &[Draft], verdicts: &[Verdict]) -> (Vec<Draft>, Vec<String>) {
    passing_after(drafts, verdicts, 0)
}

/// [`passing`] when the teacher already decided `decided` 內容目標 (they need no verdict).
pub fn passing_after(
    drafts: &[Draft],
    verdicts: &[Verdict],
    decided: usize,
) -> (Vec<Draft>, Vec<String>) {
    let mut shown: Vec<Draft> = Vec::new();
    let mut hidden: Vec<String> = Vec::new();
    for d in drafts {
        let Some(v) = verdicts.iter().find(|v| v.code == d.code) else {
            hidden.push(format!("{}：沒有完成檢查", d.code));
            continue;
        };
        if !v.problems.is_empty() {
            let why: Vec<&str> = v.problems.iter().map(|p| problem_label(p)).collect();
            hidden.push(format!("{}：{}（{}）", d.code, why.join("、"), v.reason));
            continue;
        }
        if let Some(s) = &d.supports {
            let decided_code = CONTENT_CODES.iter().take(decided).any(|c| c == s);
            if !decided_code && !shown.iter().any(|c| &c.code == s) {
                hidden.push(format!("{}：它支持的 {s} 沒有通過檢查", d.code));
                continue;
            }
        }
        shown.push(d.clone());
    }
    (shown, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(zh: &str, en: &str) -> RawContentDraft {
        RawContentDraft {
            zh: zh.into(),
            en: en.into(),
            check_zh: "Exit ticket 寫兩個例子".into(),
        }
    }

    fn language(category: &str, supports: &str, en: &str) -> RawLanguageDraft {
        RawLanguageDraft {
            category: category.into(),
            supports: supports.into(),
            zh: "用 for example 指出例子".into(),
            en: en.into(),
            relation_zh: "學生要用 for example 指出例子，才能說明作者如何支持論點".into(),
            check_zh: "請學生用 for example 寫一句".into(),
        }
    }

    #[test]
    fn fields_are_lines() {
        assert_eq!(
            normalised_field("內容目標", Some("  a \n\n b ")).unwrap(),
            Some("a\nb".into())
        );
        assert_eq!(normalised_field("內容目標", Some(" \n ")).unwrap(), None);
        assert_eq!(normalised_field("內容目標", None).unwrap(), None);
        assert!(normalised_field("語言目標", Some(&"字".repeat(301)))
            .unwrap_err()
            .contains("語言目標"));
        let t = TeacherObjectives::from_fields(Some("A\nB"), Some("C"));
        assert_eq!(t.content, ["A", "B"]);
        assert_eq!(t.combined(), "內容目標：A；B｜語言目標：C");
        let vars = TeacherObjectives::from_fields(None, Some("C")).vars();
        assert_eq!(vars[0], (VAR_CONTENT, NONE.to_string()));
        assert_eq!(vars[1], (VAR_LANGUAGE, "C".to_string()));
        assert_eq!(vars[2], (VAR_COMBINED, "語言目標：C".to_string()));
        assert_eq!(TeacherObjectives::default().vars()[2].1, NONE);
    }

    fn version(system: &str, user: &str) -> PromptVersion {
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
    fn an_old_prompt_gets_the_combined_line_and_a_new_one_the_two() {
        let old = version("目標：{{teacher_objective}}", "");
        assert_eq!(required(&old), [(VAR_COMBINED, LABEL_COMBINED)]);
        let none = version("", "教材");
        assert_eq!(required(&none), [(VAR_COMBINED, LABEL_COMBINED)]);
        let new = version("", "內容目標：{{teacher_content_objective}}");
        assert_eq!(
            required(&new),
            [(VAR_CONTENT, LABEL_CONTENT), (VAR_LANGUAGE, LABEL_LANGUAGE)]
        );
    }

    /// The same cases as `printedObjective` in web/src/__tests__/worksheetPrint.test.ts: the
    /// preview and the exports print a line the same way.
    #[test]
    fn a_printed_objective_drops_its_own_i_can() {
        for (line, printed) in [
            (
                "I can identify the key elements of the story.",
                "identify the key elements of the story.",
            ),
            ("i CAN predict the main idea.", "predict the main idea."),
            ("I can: explain why.", "explain why."),
            ("I can : explain why.", "explain why."),
            ("I can:explain why.", "explain why."),
            ("  I   can   compare two texts.", "compare two texts."),
            ("I can：說明原因", "說明原因"),
            // Not "I can" + something to print: unchanged.
            ("Identify the key elements.", "Identify the key elements."),
            ("I can't stop reading.", "I can't stop reading."),
            ("I cannot wait.", "I cannot wait."),
            ("I canvas the room.", "I canvas the room."),
            ("I can", "I can"),
            ("I can:", "I can:"),
            ("I can  ", "I can  "),
            ("We can compare.", "We can compare."),
            ("", ""),
        ] {
            assert_eq!(printed_objective(line), printed, "{line:?}");
        }
    }

    #[test]
    fn a_worksheet_keeps_the_fields_as_its_objectives() {
        let settings = Settings {
            content_objective: Some("I can explain the main idea.\nI can compare.".into()),
            language_objective: Some("我能用 because 說明原因".into()),
            ..Settings::default()
        };
        let made = from_settings(&settings);
        let ids: Vec<&str> = made.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["teacher-c1", "teacher-c2", "teacher-l1"]);
        assert_eq!(made[2].kind, ObjectiveType::Language);
        assert_eq!(made[2].student_facing_en, "我能用 because 說明原因");
        let sop = Objective {
            id: "o1".into(),
            kind: ObjectiveType::Content,
            text: "學生能說出主旨".into(),
            student_facing_en: "I can tell the main idea.".into(),
            origin: Origin::Teacher,
            sop_id: Some("o1".into()),
        };
        let stale = Objective {
            text: "old".into(),
            ..made[0].clone()
        };
        let synced = synced(&[sop.clone(), stale.clone()], &settings);
        assert_eq!(synced.len(), 4);
        assert_eq!(synced[0], sop, "the lesson's stay first");
        assert_eq!(synced[1].text, "I can explain the main idea.");
        // A generation keeps what is on the sheet once the fields are there …
        let edited = ensured(&[sop.clone(), stale.clone()], &settings);
        assert_eq!(edited[1].text, "old");
        // … and adds them to a worksheet stored before.
        assert_eq!(ensured(std::slice::from_ref(&sop), &settings).len(), 4);
        let t = TeacherObjectives::from_worksheet(&synced);
        assert_eq!(
            t.content,
            [
                "學生能說出主旨（I can tell the main idea.）",
                "I can explain the main idea.",
                "I can compare."
            ]
        );
        assert_eq!(t.language, ["我能用 because 說明原因"]);
    }

    #[test]
    fn the_draft_schema_asks_for_one_kind_per_stage_and_caps_everything() {
        let s = draft_schema(Stage::Language);
        let keys: Vec<&String> = s["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["content_objectives", "language_objectives"]);
        assert_eq!(s["properties"]["language_objectives"]["minItems"], 1);
        assert_eq!(s["properties"]["language_objectives"]["maxItems"], 2);
        assert_eq!(s["properties"]["content_objectives"]["maxItems"], 0);
        let c = draft_schema(Stage::Content);
        assert_eq!(c["properties"]["content_objectives"]["minItems"], 1);
        assert_eq!(c["properties"]["content_objectives"]["maxItems"], 2);
        assert_eq!(c["properties"]["language_objectives"]["maxItems"], 0);
        let l = &s["properties"]["language_objectives"]["items"];
        let fields: Vec<&String> = l["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            fields,
            [
                "category",
                "supports",
                "zh",
                "en",
                "relation_zh",
                "check_zh"
            ]
        );
        assert_eq!(l["properties"]["category"]["enum"][0], "strategy");
        let c = check_schema(&["C1".into(), "L1".into()]);
        assert_eq!(c["properties"]["items"]["minItems"], 2);
        assert_eq!(c["properties"]["items"]["maxItems"], 2);
        let mut missing = Vec::new();
        caps(&s, "draft", &mut missing);
        caps(&c, "check", &mut missing);
        assert!(missing.is_empty(), "{missing:?}");
    }

    fn caps(v: &Value, path: &str, problems: &mut Vec<String>) {
        match v.get("type").and_then(Value::as_str) {
            Some("string") if v.get("enum").is_none() && v.get("maxLength").is_none() => {
                problems.push(format!("{path}: string without maxLength"))
            }
            Some("array") if v.get("maxItems").is_none() => {
                problems.push(format!("{path}: array without maxItems"))
            }
            _ => {}
        }
        if let Some(props) = v.get("properties").and_then(Value::as_object) {
            for (k, p) in props {
                caps(p, &format!("{path}.{k}"), problems);
            }
        }
        if let Some(items) = v.get("items") {
            caps(items, &format!("{path}[]"), problems);
        }
    }

    #[test]
    fn r1_is_checked_by_rule() {
        let raw = RawDrafts {
            content_objectives: vec![
                content(
                    "說明作者用哪些例子支持論點",
                    "Students will be able to explain the writer's examples.",
                ),
                content("了解蜜蜂的重要", "I can understand why bees matter."),
            ],
            language_objectives: vec![
                language(
                    "function",
                    "C1",
                    "I can use for example to point to an example.",
                ),
                language(
                    "strategy",
                    "C1",
                    "I can underline signal words like such as.",
                ),
            ],
        };
        let r = check_rules(&raw, true);
        let codes: Vec<&str> = r.drafts.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["C1", "L2", "L1"], "C2 dropped, the strategy first");
        assert_eq!(r.drafts[0].text, "I can explain the writer's examples.");
        assert_eq!(r.drafts[0].index, 0);
        assert_eq!(r.drafts[1].index, 3, "its place in the model's output");
        assert!(
            r.dropped[0].starts_with("C2：zh 以「了解」開頭"),
            "{:?}",
            r.dropped
        );
        assert!(r.complete(Stage::Content) && r.complete(Stage::Language));
        assert_eq!(r.repairs.len(), 2, "{:?}", r.repairs);

        // A language objective whose content objective did not pass, or without a relation.
        let mut no_relation = language("strategy", "C1", "I can reread a hard sentence.");
        no_relation.relation_zh = "幫助理解".into();
        let raw = RawDrafts {
            content_objectives: vec![content("說明主旨", "I can know the main idea.")],
            language_objectives: vec![
                language("strategy", "C1", "I can predict and check."),
                no_relation,
            ],
        };
        let r = check_rules(&raw, true);
        assert!(r.drafts.is_empty());
        assert!(r.dropped[0].contains("know"), "{:?}", r.dropped);
        assert!(r.dropped[1].contains("supports"), "{:?}", r.dropped);
        assert!(!r.complete(Stage::Content) && !r.complete(Stage::Language));

        // Not "I can", too long, Chinese in the students' line, a category outside the four.
        let long = format!("I can {}", "read ".repeat(30));
        let raw = RawDrafts {
            content_objectives: vec![
                content("說明主旨", "Explain the main idea."),
                content("說明主旨", &long),
            ],
            language_objectives: vec![language("grammar", "C1", "I can use because.")],
        };
        let r = check_rules(&raw, true);
        assert!(r.drafts.is_empty());
        assert_eq!(r.dropped.len(), 3, "{:?}", r.dropped);
        assert!(line_problem("說明主旨", "I can 說明.").is_some());
        assert!(line_problem("Explain", "I can explain.").is_some());
        assert_eq!(
            line_problem("比較兩個角色", "I can compare and contrast them."),
            None
        );
        assert!(line_problem("比較兩個角色", "I can read and understand it.").is_some());
    }

    fn draft(code: &str, kind: ObjectiveType, supports: Option<&str>) -> Draft {
        Draft {
            index: 0,
            kind,
            code: code.into(),
            text: "I can explain.".into(),
            zh: "說明".into(),
            category: None,
            supports: supports.map(str::to_string),
            relation_zh: String::new(),
            check_zh: String::new(),
        }
    }

    #[test]
    fn r3_verdicts_decide_what_is_shown() {
        let drafts = vec![
            draft("C1", ObjectiveType::Content, None),
            draft("C2", ObjectiveType::Content, None),
            draft("L1", ObjectiveType::Language, Some("C1")),
            draft("L2", ObjectiveType::Language, Some("C2")),
        ];
        let raw = RawCheck {
            items: vec![
                // A language-only problem on a content objective does not count.
                RawVerdict {
                    id: "C1".into(),
                    reason: "對準主旨".into(),
                    problems: vec!["no_relation".into()],
                },
                RawVerdict {
                    id: "C2".into(),
                    reason: "是活動".into(),
                    problems: vec!["activity".into(), "made_up".into()],
                },
                RawVerdict {
                    id: "L1".into(),
                    reason: "好".into(),
                    problems: vec![],
                },
                RawVerdict {
                    id: "L1".into(),
                    reason: "second".into(),
                    problems: vec!["not_needed".into()],
                },
                RawVerdict {
                    id: "X9".into(),
                    reason: String::new(),
                    problems: vec![],
                },
            ],
        };
        let (verdicts, missing) = evaluate_check(&raw, &drafts);
        assert_eq!(missing, ["L2"]);
        assert_eq!(verdicts.len(), 3);
        assert!(verdicts[0].problems.is_empty());
        assert_eq!(verdicts[1].problems, ["activity"]);
        let (shown, hidden) = passing(&drafts, &verdicts);
        let codes: Vec<&str> = shown.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["C1", "L1"]);
        assert_eq!(hidden.len(), 2);
        assert!(hidden[0].starts_with("C2：寫的是活動"), "{hidden:?}");
        assert!(hidden[1].contains("沒有完成檢查"), "{hidden:?}");
        // A language objective of a content objective the judge turned down is not shown.
        let raw = RawCheck {
            items: vec![
                RawVerdict {
                    id: "C1".into(),
                    reason: String::new(),
                    problems: vec!["not_core".into()],
                },
                RawVerdict {
                    id: "L1".into(),
                    reason: String::new(),
                    problems: vec![],
                },
            ],
        };
        let two = &drafts[..1]
            .iter()
            .chain(&drafts[2..3])
            .cloned()
            .collect::<Vec<_>>();
        let (verdicts, _) = evaluate_check(&raw, two);
        let (shown, hidden) = passing(two, &verdicts);
        assert!(shown.is_empty());
        assert!(hidden[1].contains("C1 沒有通過"), "{hidden:?}");
    }

    /// The 語言目標 stage: the 內容目標 the teacher decided on are the ones a 語言目標 may
    /// support, and they need no verdict of their own.
    #[test]
    fn a_language_objective_may_support_a_content_objective_the_teacher_decided() {
        let raw = RawDrafts {
            content_objectives: vec![],
            language_objectives: vec![
                language(
                    "strategy",
                    "C1",
                    "I can underline signal words like for example.",
                ),
                language(
                    "function",
                    "C2",
                    "I can compare two reasons with both and but.",
                ),
            ],
        };
        // Nothing decided: both fail (the content objective they support does not exist).
        let none = check_rules_after(&raw, true, 0);
        assert!(none.drafts.is_empty());
        // One decided: C1 is supported, C2 is not.
        let one = check_rules_after(&raw, true, 1);
        let codes: Vec<&str> = one.drafts.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["L1"]);
        assert!(one.complete(Stage::Language));
        assert!(!one.complete(Stage::Content));
        assert_eq!(one.drafts[0].index, 0, "no content drafts before it");
        // The judge's verdicts on the 語言目標 are enough: the decided C1 has none.
        let verdicts = vec![Verdict {
            code: "L1".into(),
            problems: vec![],
            reason: "ok".into(),
        }];
        let (shown, hidden) = passing_after(&one.drafts, &verdicts, 1);
        assert_eq!(shown.len(), 1, "{hidden:?}");
        let (shown, hidden) = passing(&one.drafts, &verdicts);
        assert!(
            shown.is_empty() && hidden[0].contains("沒有通過檢查"),
            "{hidden:?}"
        );
    }
}
