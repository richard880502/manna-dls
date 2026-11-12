//! Onboarding preferences (docs/ONBOARDING.md §2): question set P1–P6, answer types, and how a
//! teacher's latest answers change AI behaviour (docs/API.md Phase 1.1).

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Bump when the wording or options of [`PREFERENCE_QUESTIONS`] change; each saved
/// preference version records the set it answered.
pub const QUESTION_SET_VERSION: i32 = 1;

/// Upper bound for the free-text class profile (P6 / `units.class_profile`).
pub const MAX_CLASS_PROFILE_CHARS: usize = 2000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrepRoute {
    Goal,
    Material,
    Student,
    Activity,
}

impl PrepRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Material => "material",
            Self::Student => "student",
            Self::Activity => "activity",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Hardest {
    Goal,
    Material,
    Differentiation,
    Activity,
    Worksheet,
    AiCommunication,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Experience {
    #[serde(rename = "0-2")]
    Novice,
    #[serde(rename = "3-9")]
    Mid,
    #[serde(rename = "10+")]
    Senior,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiExperience {
    None,
    Sometimes,
    Often,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    Concise,
    Medium,
    Detailed,
}

/// `PreferenceAnswers` from docs/API.md. Unknown fields or enum values are rejected (400).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PreferenceAnswers {
    #[serde(default)]
    pub experience: Option<Experience>,
    #[serde(default)]
    pub start_from: Option<PrepRoute>,
    #[serde(default)]
    pub hardest: Vec<Hardest>,
    #[serde(default)]
    pub ai_experience: Option<AiExperience>,
    #[serde(default)]
    pub detail: Option<Detail>,
    #[serde(default)]
    pub class_profile: String,
}

impl PreferenceAnswers {
    pub fn has_hardest(&self, h: Hardest) -> bool {
        self.hardest.contains(&h)
    }

    /// Removes duplicate `hardest` entries (first occurrence kept) and trims the class profile.
    pub fn normalized(mut self) -> Self {
        let mut seen = Vec::new();
        self.hardest.retain(|h| {
            let new = !seen.contains(h);
            seen.push(*h);
            new
        });
        self.class_profile = self.class_profile.trim().to_string();
        self
    }

    /// Length instruction injected into every AI call (P5).
    pub fn detail_instruction(&self) -> &'static str {
        detail_instruction(self.detail)
    }
}

pub fn detail_instruction(detail: Option<Detail>) -> &'static str {
    match detail {
        Some(Detail::Concise) => "精簡——每一項內容兩句以內。",
        Some(Detail::Medium) => "適中——每一項內容三句以內。",
        Some(Detail::Detailed) => "詳細——每一項內容五句以內。",
        None => "老師沒有指定，維持簡短、每一項內容不超過五句。",
    }
}

pub fn detail_label(detail: Option<Detail>) -> &'static str {
    match detail {
        Some(Detail::Concise) => "精簡",
        Some(Detail::Medium) => "適中",
        Some(Detail::Detailed) => "詳細",
        None => "未指定",
    }
}

// ---------------------------------------------------------------- question set

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PrefOption {
    pub value: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PreferenceQuestion {
    pub id: &'static str,
    pub prompt: &'static str,
    pub options: &'static [PrefOption],
    pub multi: bool,
}

const fn opt(value: &'static str, label: &'static str) -> PrefOption {
    PrefOption { value, label }
}

/// P1–P6 (ONBOARDING §2). Option values match the enums above; P6 is free text (no options).
pub const PREFERENCE_QUESTIONS: [PreferenceQuestion; 6] = [
    PreferenceQuestion {
        id: "P1",
        prompt: "教學年資",
        options: &[
            opt("0-2", "0–2 年"),
            opt("3-9", "3–9 年"),
            opt("10+", "10 年以上"),
        ],
        multi: false,
    },
    PreferenceQuestion {
        id: "P2",
        prompt: "你通常從哪裡開始備課？",
        options: &[
            opt("material", "課文或教材"),
            opt("goal", "學習目標"),
            opt("student", "學生的問題"),
            opt("activity", "活動點子"),
        ],
        multi: false,
    },
    PreferenceQuestion {
        id: "P3",
        prompt: "哪些步驟最花時間或最困難？（可複選）",
        options: &[
            opt("goal", "學習目標"),
            opt("material", "找合適素材"),
            opt("differentiation", "差異化與時間安排"),
            opt("activity", "設計活動"),
            opt("worksheet", "學習單與評量"),
            opt("ai_communication", "跟 AI 溝通"),
        ],
        multi: true,
    },
    PreferenceQuestion {
        id: "P4",
        prompt: "你使用 AI 備課的經驗",
        options: &[
            opt("none", "沒用過"),
            opt("sometimes", "偶爾"),
            opt("often", "常用"),
        ],
        multi: false,
    },
    PreferenceQuestion {
        id: "P5",
        prompt: "AI 建議希望多詳細？",
        options: &[
            opt("concise", "精簡"),
            opt("medium", "適中"),
            opt("detailed", "詳細"),
        ],
        multi: false,
    },
    PreferenceQuestion {
        id: "P6",
        prompt: "我的班級特性（選填；不要寫學生姓名或個人資料）",
        options: &[],
        multi: false,
    },
];

// ---------------------------------------------------------------- storage

#[derive(Debug, Clone, Serialize)]
pub struct Preferences {
    pub version_no: i32,
    pub question_set_version: i32,
    pub answers: PreferenceAnswers,
    pub skipped: bool,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow)]
struct PrefRow {
    version_no: i32,
    question_set_version: i32,
    answers: serde_json::Value,
    skipped: bool,
    source: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl PrefRow {
    fn into_prefs(self) -> Preferences {
        Preferences {
            version_no: self.version_no,
            question_set_version: self.question_set_version,
            // Stored answers were validated on write; a later schema change falls back to
            // defaults rather than failing every request.
            answers: serde_json::from_value(self.answers).unwrap_or_default(),
            skipped: self.skipped,
            source: self.source,
            created_at: self.created_at,
        }
    }
}

pub const PREF_COLS: &str =
    "version_no, question_set_version, answers, skipped, source, created_at";

pub async fn latest(db: &PgPool, teacher_id: i64) -> sqlx::Result<Option<Preferences>> {
    let row: Option<PrefRow> = sqlx::query_as(&format!(
        "SELECT {PREF_COLS} FROM teacher_preferences WHERE teacher_id = $1
          ORDER BY version_no DESC LIMIT 1"
    ))
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(PrefRow::into_prefs))
}

/// The latest answers, or all-default answers when the teacher has none yet.
pub async fn latest_answers(db: &PgPool, teacher_id: i64) -> sqlx::Result<PreferenceAnswers> {
    Ok(latest(db, teacher_id)
        .await?
        .map(|p| p.answers)
        .unwrap_or_default())
}

/// Inserts a new version (version_no = previous + 1) inside `tx`.
pub async fn insert_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    teacher_id: i64,
    answers: &PreferenceAnswers,
    skipped: bool,
    source: &str,
) -> sqlx::Result<Preferences> {
    // Serialize concurrent saves of the same teacher so version_no stays gapless and unique.
    sqlx::query("SELECT id FROM teachers WHERE id = $1 FOR UPDATE")
        .bind(teacher_id)
        .execute(&mut **tx)
        .await?;
    let answers_json = serde_json::to_value(answers).map_err(|e| sqlx::Error::Encode(e.into()))?;
    let row: PrefRow = sqlx::query_as(&format!(
        "INSERT INTO teacher_preferences (teacher_id, version_no, question_set_version, answers, skipped, source)
         VALUES ($1, (SELECT COALESCE(MAX(version_no), 0) + 1 FROM teacher_preferences WHERE teacher_id = $1),
                 $2, $3, $4, $5)
         RETURNING {PREF_COLS}"
    ))
    .bind(teacher_id)
    .bind(QUESTION_SET_VERSION)
    .bind(answers_json)
    .bind(skipped)
    .bind(source)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.into_prefs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn question_options_match_enums() {
        let p2: Vec<&str> = PREFERENCE_QUESTIONS[1]
            .options
            .iter()
            .map(|o| o.value)
            .collect();
        for v in p2 {
            serde_json::from_value::<PrepRoute>(json!(v)).unwrap();
        }
        for o in PREFERENCE_QUESTIONS[0].options {
            serde_json::from_value::<Experience>(json!(o.value)).unwrap();
        }
        for o in PREFERENCE_QUESTIONS[2].options {
            serde_json::from_value::<Hardest>(json!(o.value)).unwrap();
        }
        for o in PREFERENCE_QUESTIONS[3].options {
            serde_json::from_value::<AiExperience>(json!(o.value)).unwrap();
        }
        for o in PREFERENCE_QUESTIONS[4].options {
            serde_json::from_value::<Detail>(json!(o.value)).unwrap();
        }
        assert!(PREFERENCE_QUESTIONS[2].multi);
        assert!(PREFERENCE_QUESTIONS[5].options.is_empty());
        let ids: Vec<&str> = PREFERENCE_QUESTIONS.iter().map(|q| q.id).collect();
        assert_eq!(ids, ["P1", "P2", "P3", "P4", "P5", "P6"]);
    }

    #[test]
    fn answers_roundtrip_and_reject_unknown_values() {
        let v = json!({
            "experience": "10+", "start_from": "material", "hardest": ["differentiation", "ai_communication"],
            "ai_experience": "none", "detail": "concise", "class_profile": "高二普通班"
        });
        let a: PreferenceAnswers = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(a.experience, Some(Experience::Senior));
        assert!(a.has_hardest(Hardest::AiCommunication));
        assert_eq!(serde_json::to_value(&a).unwrap(), v);
        assert!(
            serde_json::from_value::<PreferenceAnswers>(json!({"start_from": "textbook"})).is_err()
        );
        assert!(serde_json::from_value::<PreferenceAnswers>(json!({"hardest": ["time"]})).is_err());
        assert!(serde_json::from_value::<PreferenceAnswers>(json!({"colour": "red"})).is_err());
        let empty: PreferenceAnswers = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty, PreferenceAnswers::default());
    }

    #[test]
    fn normalization_dedups_hardest() {
        let a = PreferenceAnswers {
            hardest: vec![Hardest::Goal, Hardest::Worksheet, Hardest::Goal],
            class_profile: "  三十人  ".into(),
            ..Default::default()
        }
        .normalized();
        assert_eq!(a.hardest, vec![Hardest::Goal, Hardest::Worksheet]);
        assert_eq!(a.class_profile, "三十人");
    }

    #[test]
    fn detail_instructions_follow_contract() {
        assert!(detail_instruction(Some(Detail::Concise)).contains("兩句"));
        assert!(detail_instruction(Some(Detail::Medium)).contains("三句"));
        assert!(detail_instruction(Some(Detail::Detailed)).contains("五句"));
    }
}
