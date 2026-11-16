//! Does a material's text look like a worksheet (rather than a lesson text)? A deterministic
//! score over the text, no model: docs/API.md「Phase 2.8：照我的學習單做」. The page uses it to
//! offer 「照我的學習單做」 when a teacher puts a worksheet where the lesson text goes, and the
//! generator adds the same hint when it refuses such a material.
//!
//! Signals (a worksheet scores at least [`WORKSHEET_THRESHOLD`]):
//!
//! | points | signal |
//! |---|---|
//! | +2 | at least 3 blanks: runs of 3+ underscores (`___`, full-width `＿＿＿`) or empty answer brackets (`（　）`, `( )`, `()`) |
//! | +1 | a header field: `Name`, `Class`, `No.`, `Date` (capitalised or all caps) or `姓名`, `班級`, `座號`, followed by `:`／`：`, a blank or the end of the line |
//! | +1 | at least 4 numbered question lines: a line starting with `1.` `2)` `(3)` `（4）` `A.` … `H)` (an answer bracket may come first: `(  ) 1.`) |
//! | +1 | a task or section word: `Task`, `Part`, `Activity` followed by a number, a letter A–H, a Roman numeral, a colon or the end of the line; `Worksheet`, `Exit ticket`, `Warm-up` (any case); `學習單`, `練習` |
//! | +1 | question marks (`?`／`？`) on at least 25% of the non-empty lines |

use std::sync::LazyLock;

use regex::Regex;

/// A text scoring this much or more looks like a worksheet.
pub const WORKSHEET_THRESHOLD: u32 = 3;
/// Blanks needed for the blank signal.
const MIN_BLANKS: usize = 3;
/// Numbered lines needed for the numbered-question signal.
const MIN_NUMBERED_LINES: usize = 4;

/// A writing line (`___`, `＿＿＿`) or an empty answer bracket (`（　）`, `( )`).
static BLANK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[_＿]{3,}|[（(][ \t\u{3000}\u{a0}]*[)）]").expect("valid regex"));

/// A header field: the label, then a colon, a blank or the end of the line.
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)(?:^|[^A-Za-z])(?:Name|NAME|Class|CLASS|No\.|NO\.|Date|DATE)[ \t\u{3000}]*(?:[:：_＿]|$)|(?:姓名|班級|座號)[ \t\u{3000}]*(?:[:：_＿(（]|$)",
    )
    .expect("valid regex")
});

/// A numbered question line (`1.`, `2)`, `(3)`, `（4）`, `A.`–`H)`), optionally after an answer
/// bracket. One to three digits, not followed by another digit (`3.5 million` is no question).
static NUMBERED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[ \t\u{3000}]*(?:[（(][ \t\u{3000}]*[)）][ \t\u{3000}]*)?(?:\d{1,3}[.)．、](?:[^\d]|$)|[（(]\d{1,3}[)）]|[A-H][.)．](?:[^A-Za-z.]|$))",
    )
    .expect("valid regex")
});

/// A task or section word.
static TASK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)\b(?:Task|TASK|Part|PART|Activity|ACTIVITY)\b[ \t]*(?:\d|[A-H]\b|[IVX]{1,4}\b|[:：]|$)|(?i:\bworksheet\b|\bexit[ \t-]*ticket\b|\bwarm[ \t-]?up\b)|學習單|練習",
    )
    .expect("valid regex")
});

/// The points of each signal, for tests and for explaining a result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorksheetSignals {
    pub blanks: usize,
    pub header: bool,
    pub numbered_lines: usize,
    pub task_words: bool,
    pub question_lines: usize,
    pub lines: usize,
}

impl WorksheetSignals {
    pub fn of(text: &str) -> Self {
        let mut s = Self {
            blanks: BLANK.find_iter(text).count(),
            header: HEADER.is_match(text),
            task_words: TASK.is_match(text),
            ..Self::default()
        };
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            s.lines += 1;
            if NUMBERED.is_match(line) {
                s.numbered_lines += 1;
            }
            if line.contains(['?', '？']) {
                s.question_lines += 1;
            }
        }
        s
    }

    pub fn score(&self) -> u32 {
        let mut score = 0;
        if self.blanks >= MIN_BLANKS {
            score += 2;
        }
        if self.header {
            score += 1;
        }
        if self.numbered_lines >= MIN_NUMBERED_LINES {
            score += 1;
        }
        if self.task_words {
            score += 1;
        }
        // At least a quarter of the lines.
        if self.lines > 0 && self.question_lines * 4 >= self.lines {
            score += 1;
        }
        score
    }
}

pub fn worksheet_score(text: &str) -> u32 {
    WorksheetSignals::of(text).score()
}

/// Whether the text looks like a worksheet (score ≥ [`WORKSHEET_THRESHOLD`]).
pub fn looks_like_worksheet(text: &str) -> bool {
    worksheet_score(text) >= WORKSHEET_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------ synthetic worksheets

    /// An English reading worksheet with a header, parts, numbered questions and blanks.
    const WS_READING: &str = "Name: __________  Class: ______  No.: ____\n\
        \n\
        Part A: Vocabulary\n\
        1. The word ______ means \"very happy\".\n\
        2. She felt ______ when she lost her keys.\n\
        3. Write the opposite of \"open\": ______\n\
        \n\
        Part B: Reading Comprehension\n\
        4. Why did the boy go to the market?\n\
        5. What did he buy?\n\
        6. How did his mother feel? Why?\n";

    /// A Chinese-style header and multiple-choice lines with answer brackets.
    const WS_CHOICE: &str = "高一英文 第三課 學習單\n\
        班級：＿＿＿＿　座號：＿＿＿＿　姓名：＿＿＿＿＿＿\n\
        一、選擇題\n\
        (  ) 1. The writer thinks the plan is ______. (A) cheap (B) useless (C) brave (D) late\n\
        (  ) 2. Which word means \"very big\"? (A) tiny (B) huge (C) thin (D) soft\n\
        (  ) 3. What happened in the end? (A) He left (B) He stayed (C) He cried (D) He slept\n\
        (  ) 4. Who helped the old man? (A) a girl (B) a dog (C) a teacher (D) nobody\n";

    /// An exit ticket without any blanks.
    const WS_EXIT: &str = "Exit Ticket\n\
        1. What is the main idea of the article?\n\
        2. Which sentence shows the writer's opinion?\n\
        3. What would you do in the girl's place?\n\
        4. What is one new word you learned today?\n";

    /// A table flattened out of a DOCX: the header cells on lines of their own.
    const WS_DOCX_TABLE: &str = "Lesson 5 Worksheet\n\
        Name\n\
        \n\
        Class\n\
        \n\
        Date\n\
        \n\
        Warm-up: Look at the picture. What do you see?\n\
        Task 1 Find the details in paragraph 2.\n\
        Who wrote the letter?\n\
        Where did she live?\n\
        Task 2 Write two sentences about your weekend.\n";

    /// A cloze passage with answer lines and a section word.
    const WS_CLOZE: &str = "Activity 2: Fill in the blanks\n\
        Tom lives in a small ____ near the sea. Every morning he ____ to school by bike. \
        He likes ____ because it is fun, and he wants to be a ____ when he grows up.\n";

    // ------------------------------------------------------------ plain reading passages

    const PASSAGE_STORY: &str = "Mrs. Lin put an umbrella stand outside her shop on a rainy Monday. A sign said, “Take one if you need it.”\n\
        \n\
        Her neighbors laughed at the idea. They were sure that every umbrella would be gone within a week.\n\
        \n\
        In the first month, twelve of the thirty umbrellas did disappear. However, something unexpected followed.\n\
        \n\
        Today the stand holds more umbrellas than it did at the start. Mrs. Lin says trust is like an umbrella.\n";

    /// Dialogue with a few questions, words like "class", "name" and "part" in running text.
    const PASSAGE_DIALOGUE: &str = "“Where are you going?” Amy asked as the class ended.\n\
        Ben smiled. “To the library. I need a book for the science part of our project.”\n\
        The librarian wrote his name on a card and handed him a heavy book about volcanoes.\n\
        He sat by the window and read until the sun went down.\n\
        On the way home he kept thinking about the date of the next eruption.\n\
        His mother was waiting at the door with dinner on the table.\n";

    /// An informational text with a numbered list of steps.
    const PASSAGE_STEPS: &str = "Making tea the traditional way takes patience.\n\
        1. Heat the water until it is almost boiling.\n\
        2. Warm the teapot with a little hot water.\n\
        3. Add the leaves and pour in the water.\n\
        4. Wait three minutes before you serve it.\n\
        Many families in Taiwan still follow these steps when guests visit.\n";

    /// A text with a scene break of underscores and one rhetorical question.
    const PASSAGE_ESSAY: &str = "3.5 million people visited the old harbor in 2019.\n\
        2019. That was the year the new bridge opened.\n\
        \n\
        ______\n\
        \n\
        Why do so many people come back every year? The answer is simple: the food, the sea \
        and the friendly people who have lived there for generations. Part of the charm is \
        that nothing seems to change, and visitors like it that way.\n";

    #[test]
    fn synthetic_worksheets_are_recognised() {
        for (name, text) in [
            ("reading", WS_READING),
            ("choice", WS_CHOICE),
            ("exit", WS_EXIT),
            ("docx table", WS_DOCX_TABLE),
            ("cloze", WS_CLOZE),
        ] {
            let s = WorksheetSignals::of(text);
            assert!(
                looks_like_worksheet(text),
                "{name}: score {} ({s:?})",
                s.score()
            );
        }
    }

    #[test]
    fn plain_passages_are_not_worksheets() {
        for (name, text) in [
            ("story", PASSAGE_STORY),
            ("dialogue", PASSAGE_DIALOGUE),
            ("steps", PASSAGE_STEPS),
            ("essay", PASSAGE_ESSAY),
            ("empty", ""),
        ] {
            let s = WorksheetSignals::of(text);
            assert!(
                !looks_like_worksheet(text),
                "{name}: score {} ({s:?})",
                s.score()
            );
        }
    }

    #[test]
    fn each_signal_scores_as_documented() {
        let reading = WorksheetSignals::of(WS_READING);
        assert!(reading.blanks >= 3, "{reading:?}");
        assert!(reading.header);
        assert_eq!(reading.numbered_lines, 6);
        assert!(reading.task_words);
        assert_eq!(reading.score(), 6);

        let choice = WorksheetSignals::of(WS_CHOICE);
        assert_eq!(
            choice.numbered_lines, 4,
            "answer brackets before the number"
        );
        assert!(choice.header, "班級／座號／姓名");
        assert!(choice.task_words, "學習單");

        let exit = WorksheetSignals::of(WS_EXIT);
        assert_eq!(exit.blanks, 0);
        assert_eq!(exit.score(), 3, "{exit:?}");

        assert!(
            WorksheetSignals::of(WS_DOCX_TABLE).header,
            "a label alone on its line"
        );

        // Running text: lower-case words, numbers and decimals are no signals.
        let dialogue = WorksheetSignals::of(PASSAGE_DIALOGUE);
        assert!(!dialogue.header && !dialogue.task_words, "{dialogue:?}");
        let essay = WorksheetSignals::of(PASSAGE_ESSAY);
        assert_eq!(essay.blanks, 1);
        assert!(!essay.task_words, "“Part of the charm” is no section");
        assert_eq!(
            essay.numbered_lines, 0,
            "a year or a decimal is no question number"
        );
        assert_eq!(WorksheetSignals::of(PASSAGE_STEPS).score(), 1);

        // Three blanks alone are two points; one more signal makes a worksheet.
        assert_eq!(worksheet_score("a ___ b ___ c ___"), 2);
        assert!(looks_like_worksheet("Name: \na ___ b ___ c ___"));
        assert_eq!(worksheet_score("（　）（ ）()"), 2);
        // A quarter of the lines with a question mark.
        assert_eq!(worksheet_score("Why?\nA.\nB.\nC."), 1);
        assert_eq!(worksheet_score("Why?\na\nb\nc\nd"), 0);
    }
}
