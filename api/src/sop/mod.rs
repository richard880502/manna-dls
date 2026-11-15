//! 課例 SOP (seven-stage lesson preparation): the document model, paragraph splitting and the
//! validation applied before a version is stored. Contract: docs/API.md「Phase 2.1」.
//!
//! Nothing in the document is required. The server never writes AI output into it: adopting a
//! suggestion is the client putting it into the document (with `origin`) and saving as usual.

pub mod ai;
pub mod checks;
pub mod fake;
pub mod schemas;
pub mod store;

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::curriculum::CurriculumItem;

/// Upper bound for the pasted material (same as unit materials and 我的範例).
pub const MAX_MATERIAL_CHARS: usize = 50_000;
/// Upper bound for any single text field of the SOP document.
pub const MAX_FIELD_CHARS: usize = 4_000;
/// Upper bound for any list in the SOP document.
pub const MAX_LIST_ITEMS: usize = 200;
pub const MAX_ID_CHARS: usize = 40;
pub const MAX_TITLE_CHARS: usize = 300;

// ---------------------------------------------------------------- material

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Paragraph {
    /// Stable across material versions while the paragraph's text is unchanged (`p3`).
    pub id: String,
    /// Position in the current material, from 1.
    pub no: i32,
    pub text: String,
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Splits pasted text into paragraphs, in code (never by the model). With blank lines in the
/// text, a paragraph is a run of non-blank lines (line breaks inside it become spaces, as in
/// hard-wrapped pastes); without any, every line is a paragraph. Blank paragraphs are skipped.
pub fn split_paragraphs(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.trim().split('\n').collect();
    let has_blank = lines.iter().any(|l| l.trim().is_empty());
    let mut out = Vec::new();
    if has_blank {
        let mut current: Vec<&str> = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                if !current.is_empty() {
                    out.push(collapse_ws(&current.join(" ")));
                    current.clear();
                }
            } else {
                current.push(line);
            }
        }
        if !current.is_empty() {
            out.push(collapse_ws(&current.join(" ")));
        }
    } else {
        out.extend(lines.iter().map(|l| collapse_ws(l)));
    }
    out.retain(|p| !p.is_empty());
    out
}

/// Gives every paragraph an id: a paragraph whose text equals one of the previous material's
/// keeps that id (each previous id is used once, first match first); the others get the next
/// `p<n>`. Returns the paragraphs and the new highest `n`.
pub fn assign_paragraph_ids(
    previous: &[Paragraph],
    previous_seq: i32,
    texts: Vec<String>,
) -> (Vec<Paragraph>, i32) {
    let mut used: HashSet<&str> = HashSet::new();
    let mut seq = previous_seq;
    let paragraphs = texts
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let kept = previous
                .iter()
                .find(|p| p.text == text && !used.contains(p.id.as_str()));
            let id = match kept {
                Some(p) => {
                    used.insert(p.id.as_str());
                    p.id.clone()
                }
                None => {
                    seq += 1;
                    format!("p{seq}")
                }
            };
            Paragraph {
                id,
                no: i as i32 + 1,
                text,
            }
        })
        .collect();
    (paragraphs, seq)
}

/// English words plus CJK characters: what a teacher would call the length of the text.
pub fn word_count(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for c in text.chars() {
        if crate::retrieval::is_cjk(c) {
            count += 1;
            in_word = false;
        } else if c.is_alphanumeric() || c == '\'' || c == '’' || c == '-' {
            if !in_word && c.is_alphanumeric() {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    count
}

/// `[P1] text` lines, the form the prompts refer to.
pub fn numbered_passage(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(|p| format!("[P{}] {}", p.no, p.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ---------------------------------------------------------------- document

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Teacher,
    AiAdopted,
    AiEdited,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveType {
    Content,
    Language,
}

impl ObjectiveType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Content => "內容目標",
            Self::Language => "語言目標",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    Linked,
    #[default]
    Unlinked,
}

/// 資訊處理功能 (spec §2, §8): 發現／整理／分析／比較／創造資訊.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InfoFunction {
    Discover,
    Organize,
    Analyze,
    Compare,
    Create,
}

/// What an exit-ticket item is for (spec §2 step 7).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExitKind {
    #[default]
    Formative,
    Reflection,
    Questions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Feature {
    pub id: String,
    pub feature: String,
    pub evidence: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ParagraphAnalysis {
    pub paragraph_id: String,
    pub function: String,
    pub summary: String,
    pub features: Vec<Feature>,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LanguageFocus {
    pub id: String,
    pub kind: String,
    pub item: String,
    pub paragraph_id: Option<String>,
    pub evidence: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Analysis {
    pub text_type: String,
    pub main_idea: String,
    pub structure: String,
    pub overall_origin: Origin,
    pub language_focus: Vec<LanguageFocus>,
    pub paragraphs: Vec<ParagraphAnalysis>,
}

/// A link from an objective to a 108 curriculum item. Everything except `item_code` and `why`
/// is filled in by the server from `knowledge_items`; what a client sends there is ignored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct CurriculumLink {
    pub item_code: String,
    pub why: String,
    /// The code as printed in the source (`3-Ⅴ-7`); `item_code` is the knowledge base's own.
    pub official_code: String,
    pub title: String,
    pub text: String,
    pub category: String,
    pub source_doc: String,
    pub source_version: String,
    pub source_page: String,
}

impl CurriculumLink {
    pub fn from_item(item: &CurriculumItem, why: &str) -> Self {
        Self {
            item_code: item.item_code.clone(),
            why: why.to_string(),
            official_code: item.official_code.clone(),
            title: item.title.clone(),
            text: item.content.clone(),
            category: item.category.clone(),
            source_doc: item.source_doc.clone(),
            source_version: item.source_version.clone(),
            source_page: item.source_page.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Objective {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ObjectiveType,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub student_facing_en: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub paragraph_ids: Vec<String>,
    #[serde(default)]
    pub links: Vec<CurriculumLink>,
    #[serde(default)]
    pub link_status: LinkStatus,
    #[serde(default)]
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ParagraphGoal {
    pub paragraph_id: String,
    pub objective_ids: Vec<String>,
    pub note: String,
}

/// The three scaffold types of spec §3. Procedural scaffolds are a plain editable field for now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Scaffolds {
    pub verbal: String,
    pub instructional: String,
    pub procedural: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub function: Option<InfoFunction>,
    pub text: String,
    pub scaffolds: Scaffolds,
    pub origin: Origin,
}

/// Questions or tasks for one paragraph, or for several merged paragraphs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TaskBlock {
    pub id: String,
    pub paragraph_ids: Vec<String>,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct WarmupActivity {
    pub id: String,
    pub text: String,
    pub scaffolds: Scaffolds,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct VocabularyItem {
    pub id: String,
    pub word: String,
    pub note: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Warmup {
    pub background: Vec<WarmupActivity>,
    pub vocabulary: Vec<VocabularyItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ExitItem {
    pub id: String,
    pub kind: ExitKind,
    pub text: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ExitTicket {
    pub items: Vec<ExitItem>,
}

/// The whole SOP document of one version (`lesson_sop_versions.content`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SopContent {
    pub analysis: Analysis,
    pub objectives: Vec<Objective>,
    pub paragraph_goals: Vec<ParagraphGoal>,
    pub task_blocks: Vec<TaskBlock>,
    pub warmup: Warmup,
    pub exit_ticket: ExitTicket,
}

// ---------------------------------------------------------------- validation

struct Validator {
    error: Option<String>,
}

impl Validator {
    fn fail(&mut self, msg: String) {
        if self.error.is_none() {
            self.error = Some(msg);
        }
    }

    fn text(&mut self, what: &str, s: &str) {
        if s.chars().count() > MAX_FIELD_CHARS {
            self.fail(format!("{what}最多 {MAX_FIELD_CHARS} 字。"));
        }
    }

    fn list(&mut self, what: &str, len: usize) {
        if len > MAX_LIST_ITEMS {
            self.fail(format!("{what}最多 {MAX_LIST_ITEMS} 項。"));
        }
    }

    /// Ids are made by the client; they only have to be present, short and unique in their list.
    fn ids<'a>(&mut self, what: &str, ids: impl Iterator<Item = &'a str>) {
        let mut seen = HashSet::new();
        for id in ids {
            if id.trim().is_empty() || id.chars().count() > MAX_ID_CHARS {
                self.fail(format!("{what}的 id 需為 1 到 {MAX_ID_CHARS} 個字元。"));
            } else if !seen.insert(id) {
                self.fail(format!("{what}的 id「{id}」重複。"));
            }
        }
    }

    fn scaffolds(&mut self, s: &Scaffolds) {
        self.text("語言鷹架", &s.verbal);
        self.text("教學鷹架", &s.instructional);
        self.text("程序鷹架", &s.procedural);
    }
}

/// Size and id checks. No field is required: an empty document is valid.
pub fn validate(content: &SopContent) -> Result<(), String> {
    let mut v = Validator { error: None };
    let a = &content.analysis;
    v.text("文體", &a.text_type);
    v.text("主旨", &a.main_idea);
    v.text("結構", &a.structure);
    v.list("語言重點", a.language_focus.len());
    v.ids("語言重點", a.language_focus.iter().map(|f| f.id.as_str()));
    for f in &a.language_focus {
        v.text("語言重點", &f.kind);
        v.text("語言重點", &f.item);
        v.text("語言重點的原文", &f.evidence);
        v.text("語言重點的說明", &f.note);
    }
    v.list("段落分析", a.paragraphs.len());
    v.ids(
        "段落分析",
        a.paragraphs.iter().map(|p| p.paragraph_id.as_str()),
    );
    for p in &a.paragraphs {
        v.text("段落功能", &p.function);
        v.text("段落摘要", &p.summary);
        v.list("寫作特色", p.features.len());
        v.ids("寫作特色", p.features.iter().map(|f| f.id.as_str()));
        for f in &p.features {
            v.text("寫作特色", &f.feature);
            v.text("寫作特色的原文", &f.evidence);
            v.text("寫作特色的說明", &f.explanation);
        }
    }
    v.list("目標", content.objectives.len());
    v.ids("目標", content.objectives.iter().map(|o| o.id.as_str()));
    for o in &content.objectives {
        v.text("目標", &o.text);
        v.text("學生版目標", &o.student_facing_en);
        v.text("推薦理由", &o.reason);
        v.list("目標對應的段落", o.paragraph_ids.len());
        v.list("課綱連結", o.links.len());
        for l in &o.links {
            v.text("課綱連結的說明", &l.why);
        }
    }
    v.list("逐段整合", content.paragraph_goals.len());
    v.ids(
        "逐段整合",
        content
            .paragraph_goals
            .iter()
            .map(|g| g.paragraph_id.as_str()),
    );
    for g in &content.paragraph_goals {
        v.list("段落對應的目標", g.objective_ids.len());
        v.text("逐段整合的筆記", &g.note);
    }
    v.list("問題或任務區塊", content.task_blocks.len());
    v.ids(
        "問題或任務區塊",
        content.task_blocks.iter().map(|b| b.id.as_str()),
    );
    for b in &content.task_blocks {
        v.list("區塊的段落", b.paragraph_ids.len());
        v.list("問題或任務", b.tasks.len());
        v.ids("問題或任務", b.tasks.iter().map(|t| t.id.as_str()));
        for t in &b.tasks {
            v.text("問題或任務", &t.text);
            v.scaffolds(&t.scaffolds);
        }
    }
    let w = &content.warmup;
    v.list("暖身活動", w.background.len());
    v.ids("暖身活動", w.background.iter().map(|b| b.id.as_str()));
    for b in &w.background {
        v.text("暖身活動", &b.text);
        v.scaffolds(&b.scaffolds);
    }
    v.list("關鍵字彙", w.vocabulary.len());
    v.ids("關鍵字彙", w.vocabulary.iter().map(|x| x.id.as_str()));
    for x in &w.vocabulary {
        v.text("關鍵字彙", &x.word);
        v.text("關鍵字彙的說明", &x.note);
    }
    v.list("Exit ticket", content.exit_ticket.items.len());
    v.ids(
        "Exit ticket",
        content.exit_ticket.items.iter().map(|i| i.id.as_str()),
    );
    for i in &content.exit_ticket.items {
        v.text("Exit ticket", &i.text);
    }
    match v.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Every curriculum code the document links to (distinct, in first-seen order).
pub fn linked_codes(content: &SopContent) -> Vec<String> {
    let mut seen = HashSet::new();
    content
        .objectives
        .iter()
        .flat_map(|o| o.links.iter())
        .filter(|l| seen.insert(l.item_code.clone()))
        .map(|l| l.item_code.clone())
        .collect()
}

/// Replaces what the client sent for each link with the database's own text and source, drops
/// duplicate links inside an objective and derives `link_status`. A code that is not in the
/// knowledge base is an error (the code is returned): curriculum text never comes from outside.
pub fn resolve_links(
    content: &mut SopContent,
    items: &HashMap<String, CurriculumItem>,
) -> Result<(), String> {
    for o in &mut content.objectives {
        let mut seen = HashSet::new();
        let mut links = Vec::with_capacity(o.links.len());
        for l in &o.links {
            let code = l.item_code.trim();
            let Some(item) = items.get(code) else {
                return Err(code.to_string());
            };
            if seen.insert(code.to_string()) {
                links.push(CurriculumLink::from_item(item, &l.why));
            }
        }
        o.link_status = if links.is_empty() {
            LinkStatus::Unlinked
        } else {
            LinkStatus::Linked
        };
        o.links = links;
    }
    Ok(())
}

/// True when the teacher has not put anything into the document.
pub fn is_blank(content: &SopContent) -> bool {
    *content == SopContent::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_blank_lines_and_joins_wrapped_lines() {
        let text = "Title line\r\n\r\nFirst paragraph is\nwrapped over two lines.\n\n\n  \nSecond   paragraph.\n";
        assert_eq!(
            split_paragraphs(text),
            vec![
                "Title line",
                "First paragraph is wrapped over two lines.",
                "Second paragraph."
            ]
        );
    }

    #[test]
    fn without_blank_lines_every_line_is_a_paragraph() {
        assert_eq!(
            split_paragraphs("One.\nTwo.\n Three. \n"),
            vec!["One.", "Two.", "Three."]
        );
        assert!(split_paragraphs("  \n \n").is_empty());
        assert_eq!(split_paragraphs("Only one."), vec!["Only one."]);
    }

    #[test]
    fn paragraph_ids_survive_edits_and_are_never_reused() {
        let (v1, seq) = assign_paragraph_ids(&[], 0, vec!["A.".into(), "B.".into(), "C.".into()]);
        assert_eq!(
            v1.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["p1", "p2", "p3"]
        );
        assert_eq!(seq, 3);
        // B is rewritten, a paragraph is inserted before A, C is unchanged.
        let (v2, seq) = assign_paragraph_ids(
            &v1,
            seq,
            vec!["New.".into(), "A.".into(), "B changed.".into(), "C.".into()],
        );
        assert_eq!(
            v2.iter().map(|p| (p.id.as_str(), p.no)).collect::<Vec<_>>(),
            [("p4", 1), ("p1", 2), ("p5", 3), ("p3", 4)]
        );
        assert_eq!(seq, 5);
        // Two identical paragraphs: the second one does not steal the first one's id.
        let (v3, _) = assign_paragraph_ids(&v2, seq, vec!["A.".into(), "A.".into()]);
        assert_eq!(v3[0].id, "p1");
        assert_eq!(v3[1].id, "p6");
    }

    #[test]
    fn counts_words_and_cjk_characters() {
        assert_eq!(word_count("Mrs. Lin's umbrella-stand is open."), 5);
        assert_eq!(word_count("信任 is open"), 4);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn empty_document_is_valid_and_ids_must_be_unique() {
        assert!(validate(&SopContent::default()).is_ok());
        let doc: SopContent = serde_json::from_value(serde_json::json!({
            "objectives": [
                {"id": "o1", "type": "content", "text": "a"},
                {"id": "o1", "type": "language", "text": "b"}
            ]
        }))
        .unwrap();
        assert!(validate(&doc).unwrap_err().contains("重複"));
        let long: SopContent = serde_json::from_value(serde_json::json!({
            "exit_ticket": {"items": [{"id": "e1", "text": "字".repeat(MAX_FIELD_CHARS + 1)}]}
        }))
        .unwrap();
        assert!(validate(&long).is_err());
    }

    #[test]
    fn links_take_their_text_from_the_database() {
        let mut doc: SopContent = serde_json::from_value(serde_json::json!({
            "objectives": [{
                "id": "o1", "type": "content", "text": "學生能…", "link_status": "unlinked",
                "links": [
                    {"item_code": "CUR-1", "why": "因為", "text": "模型寫的條文", "source_doc": "假來源"},
                    {"item_code": "CUR-1", "why": "重複"}
                ]
            }, {"id": "o2", "type": "language", "link_status": "linked"}]
        }))
        .unwrap();
        let item = CurriculumItem {
            item_code: "CUR-1".into(),
            version: 2,
            category: "curriculum".into(),
            school_track: "general".into(),
            title: "3-V-7".into(),
            content: "官方條文".into(),
            source_doc: "課綱".into(),
            source_version: "107".into(),
            source_page: "12".into(),
            official_code: "3-Ⅴ-7".into(),
            item_kind: "learning_performance".into(),
            strand_name: String::new(),
            stage: "V".into(),
            subject: "英語文".into(),
            advanced: false,
            recurring: false,
        };
        let items = HashMap::from([("CUR-1".to_string(), item)]);
        resolve_links(&mut doc, &items).unwrap();
        let o = &doc.objectives[0];
        assert_eq!(o.links.len(), 1);
        assert_eq!(o.links[0].text, "官方條文");
        assert_eq!(o.links[0].source_doc, "課綱");
        assert_eq!(o.links[0].official_code, "3-Ⅴ-7");
        assert_eq!(o.links[0].why, "因為");
        assert_eq!(o.link_status, LinkStatus::Linked);
        assert_eq!(doc.objectives[1].link_status, LinkStatus::Unlinked);
        assert_eq!(linked_codes(&doc), vec!["CUR-1"]);

        doc.objectives[1].links.push(CurriculumLink {
            item_code: "MADE-UP".into(),
            ..Default::default()
        });
        assert_eq!(resolve_links(&mut doc, &items), Err("MADE-UP".to_string()));
    }
}
