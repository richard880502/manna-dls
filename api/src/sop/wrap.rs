//! Lines that were hard-wrapped by a PDF or an e-mail become paragraphs again.
//!
//! One rule serves two callers: a PDF page ([`crate::materials::extract::page_blocks`]) and text
//! pasted (or uploaded as TXT) without blank lines ([`super::split_paragraphs`]). A PDF is known
//! to be wrapped; pasted text is only treated that way when it looks like it
//! ([`looks_hard_wrapped`]), and the test is deliberately strict: a poem, a list, a dialogue or
//! a text that already has one sentence per line keeps one paragraph per line.

use crate::retrieval::is_cjk;

/// A line shorter than this share of the widest line may end a paragraph.
pub const SHORT_LINE: f32 = 0.8;
/// A wrapped line (one that does not end a sentence) reaches at least this share of the widest.
const FULL_LINE: f32 = 0.7;
/// A heading line standing alone is at most this share of the widest line.
const HEADING_LINE: f32 = 0.6;
/// Fewer lines than this are never taken for a wrapped paste: the gain is small and a wrong
/// guess would glue separate lines together.
const MIN_LINES: usize = 5;
/// The widest line must be at least this wide (a Chinese character counts 2) for the text to be
/// prose; verse and lists are short.
const MIN_WIDEST: usize = 40;

fn is_wide(c: char) -> bool {
    is_cjk(c) || matches!(c as u32, 0x3000..=0x30FF | 0xFF00..=0xFFEF)
}

/// Display width: a Chinese (or other wide) character counts 2.
pub fn line_width(line: &str) -> usize {
    line.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

/// Whether a line closes a sentence (or a quotation).
pub fn ends_sentence(line: &str) -> bool {
    line.trim_end().chars().last().is_some_and(|c| {
        matches!(
            c,
            '.' | '?' | '!' | '"' | '”' | '’' | '。' | '？' | '！' | '」' | '』' | '…'
        )
    })
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A line that starts a list item: `- x`, `• x`, `1. x`, `2) x`, `(a) x`.
fn is_list_item(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(
        first,
        '-' | '–' | '—' | '•' | '·' | '*' | '‧' | '●' | '○' | '■'
    ) {
        return chars.next().is_none_or(char::is_whitespace);
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=3).contains(&digits) {
        return line[digits..].starts_with(['.', ')', '、', '）']);
    }
    if let Some(rest) = line.strip_prefix('(') {
        let mut it = rest.chars();
        return matches!((it.next(), it.next()), (Some(c), Some(')')) if c.is_ascii_alphanumeric());
    }
    false
}

/// Whether blank-line-free `lines` (trimmed, none empty) are one text broken at a right margin.
///
/// All of these must hold:
/// - at least [`MIN_LINES`] lines, and the widest one is prose-long ([`MIN_WIDEST`]);
/// - fewer than 40% of the lines are list items;
/// - at least three lines, and at least 40% of them, run on into the next line (do not end a
///   sentence): a one-sentence-per-line text has none;
/// - at least three quarters of those reach 70% of the widest line (a ragged margin, not short
///   verse lines);
/// - after those lines, at least 40% of the next lines do not start with a capital letter (a
///   sentence continued; verse and dialogue start every line with one).
pub fn looks_hard_wrapped(lines: &[&str]) -> bool {
    let n = lines.len();
    if n < MIN_LINES {
        return false;
    }
    let widest = lines.iter().map(|l| line_width(l)).max().unwrap_or(0);
    if widest < MIN_WIDEST {
        return false;
    }
    if lines.iter().filter(|l| is_list_item(l)).count() * 10 >= n * 4 {
        return false;
    }
    // Lines that continue in the next one.
    let open: Vec<usize> = (0..n - 1).filter(|&i| !ends_sentence(lines[i])).collect();
    if open.len() < 3 || open.len() * 10 < (n - 1) * 4 {
        return false;
    }
    let full = open
        .iter()
        .filter(|&&i| line_width(lines[i]) as f32 >= widest as f32 * FULL_LINE)
        .count();
    if full * 4 < open.len() * 3 {
        return false;
    }
    let continued = open
        .iter()
        .filter(|&&i| {
            lines[i + 1]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphabetic() && c.is_uppercase()))
        })
        .count();
    continued * 10 >= open.len() * 4
}

/// Joins lines (trimmed; empty ones part paragraphs) into paragraphs.
///
/// With `break_on_short` (no blank lines to go by), a line that ends a sentence and is clearly
/// shorter than the widest ([`SHORT_LINE`]) closes the paragraph, and a short heading-like line
/// at a paragraph's start stands alone. A word hyphenated at the end of a line is rejoined, and
/// no space is put between lines at a Chinese character.
pub fn join_wrapped(lines: &[&str], break_on_short: bool) -> Vec<String> {
    let widest = lines.iter().map(|l| line_width(l)).max().unwrap_or(0);
    join_wrapped_within(lines, break_on_short, widest)
}

/// [`join_wrapped`] measured against a given widest line: a piece of a page (an exam paper's
/// cover is cut at its bullets first, `materials::exam`) does not know how wide the page is.
pub fn join_wrapped_within(lines: &[&str], break_on_short: bool, widest: usize) -> Vec<String> {
    let widest = widest as f32;
    let mut out = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, out: &mut Vec<String>| {
        let text = collapse_ws(current);
        if !text.is_empty() {
            out.push(text);
        }
        current.clear();
    };
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut current, &mut out);
            continue;
        }
        let width = line_width(line) as f32;
        if break_on_short
            && current.is_empty()
            && width < widest * HEADING_LINE
            && super::is_heading_like(line)
        {
            out.push(collapse_ws(line));
            continue;
        }
        let first = line.chars().next();
        let hyphenated = current.ends_with('-')
            && current
                .chars()
                .rev()
                .nth(1)
                .is_some_and(|c| c.is_alphabetic() && !is_wide(c))
            && first.is_some_and(|c| c.is_lowercase());
        if hyphenated {
            current.pop();
        } else if let (Some(last), Some(first)) = (current.chars().last(), first) {
            if !is_wide(last) && !is_wide(first) {
                current.push(' ');
            }
        }
        current.push_str(line);
        if break_on_short && ends_sentence(line) && width < widest * SHORT_LINE {
            flush(&mut current, &mut out);
        }
    }
    flush(&mut current, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<&str> {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect()
    }

    #[test]
    fn a_hard_wrapped_article_becomes_a_few_paragraphs() {
        let text = "The Umbrella Stand\n\
            Mrs. Lin put an umbrella stand outside her shop on a rainy\n\
            Monday morning. Nobody took one, and the stand stayed full\n\
            until the evening, when it stopped.\n\
            In the first month, twelve of the thirty umbrellas did go\n\
            missing. However, something unexpected followed, because\n\
            people brought their own umbrellas back.\n\
            By spring the stand was always full again, and Mrs. Lin\n\
            never bought a single new umbrella for it.\n";
        let l = lines(text);
        assert!(looks_hard_wrapped(&l));
        let p = join_wrapped(&l, true);
        assert_eq!(p.len(), 4, "{p:?}");
        assert_eq!(p[0], "The Umbrella Stand");
        assert!(p[1].starts_with("Mrs. Lin put") && p[1].ends_with("stopped."));
        assert!(p[2].contains("did go missing."));
    }

    #[test]
    fn one_sentence_per_line_is_not_wrapped() {
        let text = "The rain started early in the morning on Monday.\n\
            Mrs. Lin opened her small shop at eight o'clock sharp.\n\
            She put an umbrella stand outside the front door.\n\
            Nobody took an umbrella during the first morning.\n\
            By the evening the whole street had learned about it.\n\
            Everyone agreed that it was a kind and clever idea.\n";
        assert!(!looks_hard_wrapped(&lines(text)));
    }

    #[test]
    fn lists_poems_and_dialogue_are_not_wrapped() {
        let list = "1. Wash two cups of rice in cold water until clear\n\
            2. Put the rice in the pot with three cups of water\n\
            3. Cook over high heat until the water starts to boil\n\
            4. Turn the heat down and cover the pot for fifteen minutes\n\
            5. Let it rest for ten minutes before you open the lid";
        assert!(!looks_hard_wrapped(&lines(list)));
        let bullets = "- bring a pencil and an eraser to the exam room today\n\
            - bring a bottle of water and a small snack for the break\n\
            - wear comfortable shoes because we will walk a long way\n\
            - read the instructions on the board before you start now\n\
            - ask the teacher if there is something you do not know";
        assert!(!looks_hard_wrapped(&lines(bullets)));
        let poem = "The rain is knocking on my window,\n\
            Tap, tap, tap, it says,\n\
            Come out and play with me,\n\
            Dance in the puddles all day,\n\
            And I will sing you to sleep.\n\
            The clouds are grey and soft,";
        assert!(!looks_hard_wrapped(&lines(poem)));
        // A long-lined poem: every line starts with a capital letter.
        let long_poem = "The quiet river runs beneath the old stone bridge at dawn,\n\
            The morning light is falling on the fields of golden wheat,\n\
            The farmers walk together down the narrow dusty road,\n\
            The children laugh and chase the geese across the village green,\n\
            The bells begin to ring above the roofs of the sleepy town,\n\
            The smoke is rising slowly from the chimneys, one by one,";
        assert!(!looks_hard_wrapped(&lines(long_poem)));
        let dialogue = "\"Where are you going this early?\" asked Mei.\n\
            \"To the market, because we need some fresh fish,\" said Dad.\n\
            \"Can I come with you to carry the bag?\" she asked.\n\
            \"Only if you wear your warm coat today,\" he answered.\n\
            \"I promise that I will wear it,\" Mei said happily.\n\
            \"Then hurry up, the bus will not wait for us,\" said Dad.";
        assert!(!looks_hard_wrapped(&lines(dialogue)));
        // Too few lines to tell.
        assert!(!looks_hard_wrapped(&lines(
            "A short article wrapped over only a few lines so it\nhas too little to go on, and it\ncontinues here for a moment\nand then it stops."
        )));
    }

    #[test]
    fn hyphenated_words_are_rejoined_and_real_dashes_kept() {
        let l = [
            "The umbrella stand was an experi-",
            "ment in trust, and a well-",
            "known one in the old town.",
        ];
        assert_eq!(
            join_wrapped(&l, true),
            ["The umbrella stand was an experiment in trust, and a wellknown one in the old town."]
        );
        // A dash after a space or before a capital letter is not a hyphenation.
        let l = ["He said it -", "Then he left, a Rio-", "Grande story."];
        assert_eq!(
            join_wrapped(&l, false),
            ["He said it - Then he left, a Rio- Grande story."]
        );
    }

    #[test]
    fn chinese_lines_join_without_spaces_and_break_at_punctuation() {
        let text = "林太太在下雨的星期一早上，把一個舊的傘架放在她的小書店門口，\n\
            旁邊貼了一張紙，上面寫著：需要的人可以借用。第一個月，\n\
            三十把雨傘有十二把不見了，但是接下來發生了意想不到的事。\n\
            到了春天，傘架又滿了。\n\
            街上的人都說這是一個很好的主意，而且越來越多的人開始跟著做，\n\
            他們在自己的門口也放了傘架，讓路過的人躲雨，大家都很開心。\n\
            現在這條街被叫做傘架街。\n\
            後來連外地來的客人也特地來看，他們都覺得這條街非常溫暖，\n\
            也很想把這個習慣帶回自己住的城市去。\n";
        let l = lines(text);
        assert!(looks_hard_wrapped(&l), "{l:?}");
        let p = join_wrapped(&l, true);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(!p[0].contains(' '));
        assert!(p[0].contains("借用。第一個月，三十把"));
        assert_eq!(p[2].chars().last(), Some('。'));
    }

    #[test]
    fn blank_lines_always_part_paragraphs() {
        let l = ["First wrapped", "paragraph here.", "", "Second one."];
        assert_eq!(
            join_wrapped(&l, false),
            ["First wrapped paragraph here.", "Second one."]
        );
    }
}
