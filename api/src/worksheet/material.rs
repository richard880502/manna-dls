//! The material seam of the worksheet generator.
//!
//! Everything downstream (prompts, checks, assembly, the vocabulary check) reads a material only
//! through [`Material`]: a title, paragraphs with stable ids, and where it came from. Today there
//! are two sources, both already on `main`: text pasted on the 學習單 page ([`Material::pasted`])
//! and the lesson SOP's material ([`Material::from_lesson_sop`]). The material store
//! (branch `feature/materials-storage`: uploads, text extraction, annotations with an AI-share
//! flag) plugs in by adding one more constructor that returns the same shape and one more
//! `source.kind`; nothing else in `worksheet::*` has to change. Annotations the teacher shares
//! with the AI would be passed next to it as extra prompt context.

use serde::{Deserialize, Serialize};

use crate::sop::{assign_paragraph_ids, split_paragraphs, Paragraph};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MaterialParagraph {
    /// Stable while the paragraph's text is unchanged (`p3`).
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct MaterialSource {
    /// `pasted` | `lesson_sop` (the material store will add its own).
    pub kind: String,
    pub lesson_id: Option<i64>,
    pub material_version_no: Option<i32>,
    pub note: String,
}

pub const SOURCE_PASTED: &str = "pasted";
pub const SOURCE_LESSON_SOP: &str = "lesson_sop";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Material {
    pub title: String,
    pub paragraphs: Vec<MaterialParagraph>,
    pub source: MaterialSource,
}

impl Material {
    /// Pasted text, split into paragraphs in code. A paragraph whose text equals one of
    /// `previous` keeps that id; returns the material and the highest paragraph number used.
    pub fn pasted(
        previous: Option<&Material>,
        previous_seq: i32,
        title: &str,
        text: &str,
        note: &str,
    ) -> (Self, i32) {
        let prev: Vec<Paragraph> = previous
            .map(|m| {
                m.paragraphs
                    .iter()
                    .enumerate()
                    .map(|(i, p)| Paragraph {
                        id: p.id.clone(),
                        no: i as i32 + 1,
                        text: p.text.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (paragraphs, seq) = assign_paragraph_ids(&prev, previous_seq, split_paragraphs(text));
        (
            Self {
                title: title.trim().to_string(),
                paragraphs: paragraphs
                    .into_iter()
                    .map(|p| MaterialParagraph {
                        id: p.id,
                        text: p.text,
                    })
                    .collect(),
                source: MaterialSource {
                    kind: SOURCE_PASTED.to_string(),
                    lesson_id: None,
                    material_version_no: None,
                    note: note.trim().to_string(),
                },
            },
            seq,
        )
    }

    /// A snapshot of the lesson SOP's material: same paragraphs, same ids.
    pub fn from_lesson_sop(lesson_id: i64, m: &crate::sop::store::Material) -> (Self, i32) {
        let seq = m
            .paragraphs
            .iter()
            .filter_map(|p| p.id.trim_start_matches('p').parse::<i32>().ok())
            .max()
            .unwrap_or(0)
            .max(m.paragraphs.len() as i32);
        (
            Self {
                title: m.title.clone(),
                paragraphs: m
                    .paragraphs
                    .iter()
                    .map(|p| MaterialParagraph {
                        id: p.id.clone(),
                        text: p.text.clone(),
                    })
                    .collect(),
                source: MaterialSource {
                    kind: SOURCE_LESSON_SOP.to_string(),
                    lesson_id: Some(lesson_id),
                    material_version_no: Some(m.version_no),
                    note: m.source_note.clone(),
                },
            },
            seq,
        )
    }

    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }

    /// The whole text, paragraphs separated by a blank line.
    pub fn text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// `[P1] text` lines, the form the prompts refer to.
    pub fn numbered(&self) -> String {
        self.paragraphs
            .iter()
            .enumerate()
            .map(|(i, p)| format!("[P{}] {}", i + 1, p.text))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Position of a paragraph id, from 1.
    pub fn no_of(&self, id: &str) -> Option<usize> {
        self.paragraphs
            .iter()
            .position(|p| p.id == id)
            .map(|i| i + 1)
    }

    pub fn id_of(&self, no: i64) -> Option<&str> {
        usize::try_from(no)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| self.paragraphs.get(i))
            .map(|p| p.id.as_str())
    }

    pub fn word_count(&self) -> usize {
        crate::sop::word_count(&self.text())
    }

    /// For 「Manna 已帶入的資訊」.
    pub fn label(&self) -> String {
        let name = if self.title.trim().is_empty() {
            crate::routes::clip_chars(
                self.paragraphs
                    .first()
                    .map(|p| p.text.as_str())
                    .unwrap_or(""),
                24,
            )
        } else {
            self.title.trim().to_string()
        };
        format!(
            "{name}（{} 段，{} 字）",
            self.paragraphs.len(),
            self.word_count()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_text_is_split_and_ids_survive_an_edit() {
        let (m, seq) = Material::pasted(None, 0, " Title ", "One.\n\nTwo.\n\nThree.", "note");
        assert_eq!(m.title, "Title");
        assert_eq!(
            m.paragraphs
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["p1", "p2", "p3"]
        );
        assert_eq!(seq, 3);
        let (m2, seq2) = Material::pasted(Some(&m), seq, "Title", "Zero.\n\nTwo.\n\nThree.", "");
        assert_eq!(
            m2.paragraphs
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["p4", "p2", "p3"]
        );
        assert_eq!(seq2, 4);
        assert_eq!(m2.no_of("p2"), Some(2));
        assert_eq!(m2.id_of(3), Some("p3"));
        assert_eq!(m2.id_of(0), None);
        assert!(m2.numbered().starts_with("[P1] Zero."));
    }
}
