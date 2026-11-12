//! CoRe eight questions (spec §4.1 table): short on-screen prompt and the level-1
//! explanation shown after the teacher asks for help.

use serde::Serialize;

/// The opening question shown above the focuses (spec §4.1 前置問題).
pub const CORE_PREAMBLE: &str = "學完這個單元後，你最希望學生真正理解、能運用或能表達的 2–3 個重要學習重點是什麼？可以包含語言知識、溝通功能、文本理解、思考能力或文化內容。";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CoreQuestion {
    pub id: &'static str,
    pub prompt: &'static str,
    pub help: &'static str,
}

pub const CORE_QUESTIONS: [CoreQuestion; 8] = [
    CoreQuestion {
        id: "Q1",
        prompt: "你希望學生學會什麼？",
        help: "學生最後要真正學會、能運用或能表達什麼",
    },
    CoreQuestion {
        id: "Q2",
        prompt: "為什麼要學這個重點？",
        help: "與本單元、後續學習或真實溝通有什麼關係",
    },
    CoreQuestion {
        id: "Q3",
        prompt: "還有哪些相關內容這次先不教？",
        help: "你知道、但這個單元不打算教或暫不展開的內容",
    },
    CoreQuestion {
        id: "Q4",
        prompt: "這個重點可能哪裡難教？",
        help: "可能遇到的困難或限制",
    },
    CoreQuestion {
        id: "Q5",
        prompt: "學生可能怎麼想或誤解？",
        help: "學生原本可能的理解、誤解或回應；影響教法的經驗、語言能力或常見困難",
    },
    CoreQuestion {
        id: "Q6",
        prompt: "還有哪些因素會影響教學？",
        help: "班級差異、時間、課綱、考試、教材或設備",
    },
    CoreQuestion {
        id: "Q7",
        prompt: "你準備怎麼教？為什麼？",
        help: "活動、方法或材料，以及這樣安排的理由",
    },
    CoreQuestion {
        id: "Q8",
        prompt: "你怎麼知道學生懂了？",
        help: "如何知道學生已理解、仍混淆或能運用；預期的反應或作品",
    },
];

pub const QUESTION_IDS: [&str; 8] = ["Q1", "Q2", "Q3", "Q4", "Q5", "Q6", "Q7", "Q8"];

pub fn find(id: &str) -> Option<&'static CoreQuestion> {
    CORE_QUESTIONS.iter().find(|q| q.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_questions_in_order() {
        let ids: Vec<_> = CORE_QUESTIONS.iter().map(|q| q.id).collect();
        assert_eq!(ids, QUESTION_IDS);
        assert!(find("Q5").unwrap().help.contains("誤解"));
        assert!(find("Q9").is_none());
    }
}
