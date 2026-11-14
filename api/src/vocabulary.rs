//! Deterministic vocabulary-level analysis against the 大考中心 高中英文參考詞彙表
//! (product spec §9.6, vocabulary part). No model is involved: every level comes from the
//! imported list (`vocabulary_items`), never from an LLM.
//!
//! Port of `research/sop-validation/vocab/analyze.py` (validated in docs/LLM_VALIDATION.md §8).
//!
//! Reports
//! * the level distribution (tokens and distinct words) with its denominator,
//! * words above a target level with their positions,
//! * and, kept OUT of the distribution: proper nouns, unknown words, multiword items (hyphenated
//!   words, closed compounds, phrases from a caller-supplied list), appendix words (numbers,
//!   days, countries…), plus the flags a teacher needs to judge the numbers: ambiguous forms,
//!   rule-derived matches and words listed at more than one level.
//!
//! Lemmatisation is rule-based: exact match → irregular table → inflection rules (-s, -es,
//! -ies, -ed, -ing, doubled consonant, -er/-est, possessive, contractions). A second, labelled
//! pass handles the regular derivations the list deliberately does not enumerate (re-, non-,
//! un-, -less, -ly, -ness): these are reported as `derived` with the base word's level, never
//! silently merged.
//!
//! Limits ([`LIMITS`]): no part-of-speech or word-sense disambiguation; phrases are not detected
//! unless supplied; a sentence-initial capitalised unknown word cannot be told apart from a
//! proper noun; British spellings and irregular forms missing from the table end up unknown;
//! the lemmatiser's accuracy has not yet been measured against hand-labelled worksheets.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use serde::Serialize;
use sqlx::PgPool;

pub const DEFAULT_TARGET_LEVEL: u8 = 4;
pub const MAX_TEXT_CHARS: usize = 100_000;
pub const MAX_PHRASES: usize = 200;

pub const DENOMINATOR_NOTE: &str =
    "比例的分母＝可對應到詞彙表的詞（含可還原的詞形變化與規則衍生詞）；專有名詞、未收錄詞、連字號與複合詞、片語、附錄詞彙不計入分母，另外列出。";
pub const DISCLAIMER: &str =
    "本分析為備課輔助，依公開詞彙表以規則比對，不是大考中心的官方評定、命題預測或品質保證。";
pub const LIMITS: [&str; 6] = [
    "不判斷詞性與詞義：同形的詞（例如 broke 可能是形容詞，也可能是 break 的過去式）取較低級別並列在「詞形歧義」；一詞多級取最低級並列在「多級別詞」。",
    "詞彙表不收片語，本分析也偵測不到片語；連字號詞與複合詞只列出組成詞的級別，不歸級。",
    "句首大寫且未收錄的詞，無法判定是不是專有名詞，列在「未收錄」。",
    "未收錄的詞不計入任何一級；較難的文章未收錄詞多，只看比例會低估難度，請同時看未收錄清單。",
    "英式拼法與不在不規則變化表內的詞形會被列為未收錄。",
    "詞形還原規則尚未以人工標註語料量測正確率。",
];

// ---------------------------------------------------------------- irregular forms

/// `base:form,form …` — hand-written, common items only.
const IRREGULAR_VERBS: &str = "
be:am,is,are,was,were,been,being have:has,had,having do:does,did,done,doing go:goes,went,gone
say:said make:made get:got,gotten know:knew,known think:thought take:took,taken see:saw,seen
come:came give:gave,given find:found tell:told become:became leave:left feel:felt put:put
bring:brought begin:began,begun keep:kept hold:held write:wrote,written stand:stood hear:heard
let:let mean:meant set:set meet:met run:ran pay:paid sit:sat speak:spoke,spoken lie:lay,lain,lying
lead:led read:read grow:grew,grown lose:lost fall:fell,fallen send:sent build:built
understand:understood draw:drew,drawn break:broke,broken spend:spent cut:cut rise:rose,risen
drive:drove,driven buy:bought wear:wore,worn choose:chose,chosen seek:sought throw:threw,thrown
catch:caught deal:dealt win:won forget:forgot,forgotten lay:laid sell:sold fight:fought
bear:bore,born,borne teach:taught eat:ate,eaten sing:sang,sung strike:struck,stricken hang:hung
shake:shook,shaken ride:rode,ridden feed:fed shoot:shot drink:drank,drunk hit:hit arise:arose,arisen
fly:flew,flown blow:blew,blown bite:bit,bitten steal:stole,stolen swim:swam,swum sleep:slept
wake:woke,woken hide:hid,hidden freeze:froze,frozen forgive:forgave,forgiven shine:shone
sink:sank,sunk tear:tore,torn sweep:swept bend:bent bind:bound bleed:bled breed:bred burst:burst
cast:cast cling:clung creep:crept dig:dug dwell:dwelt flee:fled fling:flung forbid:forbade,forbidden
grind:ground kneel:knelt lend:lent light:lit ring:rang,rung shed:shed shrink:shrank,shrunk
shut:shut slide:slid spin:spun spit:spat split:split spread:spread spring:sprang,sprung
stick:stuck sting:stung stink:stank,stunk stride:strode swear:swore,sworn swing:swung
weave:wove,woven weep:wept wind:wound withdraw:withdrew,withdrawn overcome:overcame
undergo:underwent,undergone mistake:mistook,mistaken hurt:hurt cost:cost quit:quit
die:dying tie:tying show:shown sew:sewn sow:sown prove:proven swell:swollen can:could will:would
shall:should may:might learn:learnt burn:burnt dream:dreamt smell:smelt spell:spelt spill:spilt
";
const IRREGULAR_NOUNS: &str = "
man:men woman:women child:children person:people foot:feet tooth:teeth mouse:mice goose:geese
ox:oxen life:lives knife:knives wife:wives leaf:leaves half:halves shelf:shelves wolf:wolves
thief:thieves self:selves loaf:loaves calf:calves criterion:criteria phenomenon:phenomena
analysis:analyses crisis:crises basis:bases thesis:theses hypothesis:hypotheses
datum:data bacterium:bacteria curriculum:curricula fungus:fungi cactus:cacti stimulus:stimuli
";
const IRREGULAR_ADJECTIVES: &str = "
good:better,best bad:worse,worst far:farther,farthest,further,furthest little:less,least
many:more,most well:better,best
";

/// Inflected form → base forms.
static IRREGULAR: LazyLock<HashMap<&'static str, Vec<&'static str>>> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
    for block in [IRREGULAR_VERBS, IRREGULAR_NOUNS, IRREGULAR_ADJECTIVES] {
        for item in block.split_whitespace() {
            let (base, forms) = item.split_once(':').expect("base:forms");
            for form in forms.split(',') {
                let bases = map.entry(form).or_default();
                if !bases.contains(&base) {
                    bases.push(base);
                }
            }
        }
    }
    map
});

/// Fragments left over from contractions; never vocabulary items.
const CONTRACTION_PARTS: [&str; 7] = ["s", "t", "re", "ve", "ll", "d", "m"];
/// Longest first, so the leftmost match wins (`n't` before `'`).
const CONTRACTION_SUFFIXES: [&str; 8] = ["n't", "'re", "'ve", "'ll", "'s", "'d", "'m", "'"];
/// Appendix categories whose words also count when written in lower case.
const LOWERCASE_APPENDIX: [&str; 3] = ["Cardinal Numbers", "Ordinal Numbers", "Seasons"];

// ---------------------------------------------------------------- lexicon

/// One matchable word form of the list (a row of `vocabulary_items`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LexiconRow {
    pub word: String,
    /// `None` for appendix words.
    pub level: Option<i16>,
    pub pos: String,
    pub kind: String,
    pub appendix_category: String,
}

#[derive(Debug, Clone, Default)]
pub struct Lexicon {
    pub version: String,
    /// lower-cased word → listed levels
    levels: HashMap<String, BTreeSet<u8>>,
    /// lower-cased word → listed parts of speech (as printed, e.g. `n./v.`)
    pos: HashMap<String, Vec<String>>,
    /// lower-cased appendix word → category
    appendix: HashMap<String, String>,
}

impl Lexicon {
    pub fn new(version: impl Into<String>, rows: impl IntoIterator<Item = LexiconRow>) -> Self {
        let mut lex = Self {
            version: version.into(),
            ..Self::default()
        };
        for r in rows {
            let word = r.word.replace('’', "'").to_lowercase();
            match r.level {
                Some(level) if r.kind != "appendix" => {
                    lex.levels
                        .entry(word.clone())
                        .or_default()
                        .insert(level.clamp(1, 6) as u8);
                    lex.pos.entry(word).or_default().push(r.pos);
                }
                _ => {
                    lex.appendix.insert(word, r.appendix_category);
                }
            }
        }
        lex
    }

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// (lowest listed level, listed at more than one level)
    fn level(&self, word: &str) -> Option<(u8, bool)> {
        let levels = self.levels.get(word)?;
        Some((*levels.first()?, levels.len() > 1))
    }

    fn is_verb(&self, word: &str) -> bool {
        self.pos.get(word).is_some_and(|all| {
            all.iter().any(|pos| {
                pos.split('/')
                    .any(|p| p.trim_matches(|c| c == '(' || c == ')') == "v.")
            })
        })
    }
}

/// Loads the active list. With several list versions active, the requested one, otherwise the
/// most recently imported. `None` when no vocabulary has been imported.
pub async fn load_lexicon(
    db: &PgPool,
    list_version: Option<&str>,
) -> sqlx::Result<Option<Lexicon>> {
    let version: Option<String> = match list_version {
        Some(v) => {
            sqlx::query_scalar(
                "SELECT list_version FROM vocabulary_items
              WHERE retired_at IS NULL AND list_version = $1 LIMIT 1",
            )
            .bind(v)
            .fetch_optional(db)
            .await?
        }
        None => {
            sqlx::query_scalar(
                "SELECT list_version FROM vocabulary_items
              WHERE retired_at IS NULL ORDER BY id DESC LIMIT 1",
            )
            .fetch_optional(db)
            .await?
        }
    };
    let Some(version) = version else {
        return Ok(None);
    };
    let rows: Vec<LexiconRow> = sqlx::query_as(
        "SELECT word, level, pos, kind, appendix_category FROM vocabulary_items
          WHERE retired_at IS NULL AND list_version = $1 ORDER BY id",
    )
    .bind(&version)
    .fetch_all(db)
    .await?;
    Ok(Some(Lexicon::new(version, rows)))
}

// ---------------------------------------------------------------- lemma lookup

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// `stem` ends in a doubled consonant (`stopp`, `runn`).
fn doubled_consonant(stem: &str) -> bool {
    let b = stem.as_bytes();
    b.len() > 2 && b[b.len() - 1] == b[b.len() - 2] && !is_vowel(b[b.len() - 1])
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !out.contains(&s) {
        out.push(s);
    }
}

/// Possible base forms of an inflected word, most specific first. `w` is lower-cased.
pub fn inflection_candidates(w: &str, comparatives: bool) -> Vec<String> {
    let mut c: Vec<String> = Vec::new();
    if !w.is_ascii() {
        return c;
    }
    let n = w.len();
    if let Some(bases) = IRREGULAR.get(w) {
        for b in bases {
            push_unique(&mut c, (*b).to_string());
        }
    }
    if w.ends_with("ies") && n > 4 {
        push_unique(&mut c, format!("{}y", &w[..n - 3]));
    }
    if w.ends_with("ves") && n > 4 {
        push_unique(&mut c, format!("{}f", &w[..n - 3]));
        push_unique(&mut c, format!("{}fe", &w[..n - 3]));
    }
    if w.ends_with("es") && n > 3 {
        push_unique(&mut c, w[..n - 2].to_string());
        push_unique(&mut c, w[..n - 1].to_string());
    }
    if w.ends_with('s') && !w.ends_with("ss") && n > 2 {
        push_unique(&mut c, w[..n - 1].to_string());
    }
    if w.ends_with("ied") && n > 4 {
        push_unique(&mut c, format!("{}y", &w[..n - 3]));
    }
    if w.ends_with("ed") && n > 3 {
        let stem = &w[..n - 2];
        push_unique(&mut c, stem.to_string());
        push_unique(&mut c, format!("{stem}e"));
        if doubled_consonant(stem) {
            push_unique(&mut c, stem[..stem.len() - 1].to_string());
        }
    }
    if w.ends_with("ing") && n > 4 {
        let stem = &w[..n - 3];
        push_unique(&mut c, stem.to_string());
        push_unique(&mut c, format!("{stem}e"));
        if doubled_consonant(stem) {
            push_unique(&mut c, stem[..stem.len() - 1].to_string());
        }
        if let Some(head) = stem.strip_suffix('y') {
            // dying -> die
            push_unique(&mut c, format!("{head}ie"));
        }
    }
    if comparatives {
        for suffix in ["er", "est"] {
            if w.ends_with(suffix) && n > suffix.len() + 2 {
                let stem = &w[..n - suffix.len()];
                push_unique(&mut c, stem.to_string());
                push_unique(&mut c, format!("{stem}e"));
                if let Some(head) = stem.strip_suffix('i') {
                    push_unique(&mut c, format!("{head}y"));
                }
                if doubled_consonant(stem) {
                    push_unique(&mut c, stem[..stem.len() - 1].to_string());
                }
            }
        }
    }
    c
}

/// (base, rule) for the regular derivations the list does not enumerate.
fn derivation_candidates(w: &str) -> Vec<(String, &'static str)> {
    let mut c: Vec<(String, &'static str)> = Vec::new();
    if !w.is_ascii() {
        return c;
    }
    let n = w.len();
    for (prefix, rule) in [("re", "re-"), ("non", "non-"), ("un", "un-")] {
        if w.starts_with(prefix) && n >= prefix.len() + 3 {
            c.push((w[prefix.len()..].trim_start_matches('-').to_string(), rule));
        }
    }
    if w.ends_with("less") && n > 6 {
        c.push((w[..n - 4].to_string(), "-less"));
    }
    if w.ends_with("ness") && n > 6 {
        c.push((w[..n - 4].to_string(), "-ness"));
        if w.as_bytes()[n - 5] == b'i' {
            c.push((format!("{}y", &w[..n - 5]), "-ness"));
        }
    }
    if w.ends_with("ly") && n > 4 {
        c.push((w[..n - 2].to_string(), "-ly"));
        if w.ends_with("ily") {
            c.push((format!("{}y", &w[..n - 3]), "-ly"));
        }
        if w.ends_with("ally") {
            c.push((w[..n - 4].to_string(), "-ly"));
        }
        if w.ends_with("bly") || w.ends_with("ply") || w.ends_with("tly") {
            c.push((format!("{}e", &w[..n - 1]), "-ly"));
        }
    }
    c
}

/// How a word was matched to the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "match", rename_all = "snake_case")]
pub enum Match {
    /// The word itself is listed.
    Exact,
    /// A regular or irregular inflection of a listed word.
    Inflection,
    /// An inflection of a lower-level word that is also listed as a word of its own at
    /// `also_listed_level` (broke / break): the lower level is used.
    Ambiguous { also_listed_level: u8 },
    /// A regular derivation (`rule`, e.g. `un-`, `-ly`) of a listed word: an inferred match.
    Derived { rule: &'static str },
}

impl Match {
    fn label(&self) -> &'static str {
        match self {
            Match::Exact => "exact",
            Match::Inflection | Match::Ambiguous { .. } => "inflection",
            Match::Derived { .. } => "derived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub base: String,
    pub level: u8,
    pub multi_level: bool,
    pub matched: Match,
}

/// Looks up a lower-cased word: exact, then inflection, then derivation.
pub fn lookup(lex: &Lexicon, w: &str) -> Option<Hit> {
    let exact = lex.level(w);
    let hits: Vec<(String, (u8, bool))> = inflection_candidates(w, true)
        .into_iter()
        .filter_map(|b| lex.level(&b).map(|l| (b, l)))
        .collect();
    // "used" -> us / use: for -ed / -ing prefer a candidate listed as a verb.
    let wants_verb = w.ends_with("ed") || w.ends_with("ing");
    let inflected = hits
        .iter()
        .find(|(b, _)| wants_verb && lex.is_verb(b))
        .or(hits.first())
        .cloned();
    if let (Some((exact_level, _)), Some((base, (level, multi)))) = (exact, &inflected) {
        // "broke" (adj., L4) vs the past tense of "break" (L1): without POS tagging the two
        // readings cannot be told apart, so the lower level is used and the word is flagged.
        if *level < exact_level && inflection_candidates(w, false).contains(base) {
            return Some(Hit {
                base: base.clone(),
                level: *level,
                multi_level: *multi,
                matched: Match::Ambiguous {
                    also_listed_level: exact_level,
                },
            });
        }
    }
    if let Some((level, multi_level)) = exact {
        return Some(Hit {
            base: w.to_string(),
            level,
            multi_level,
            matched: Match::Exact,
        });
    }
    if let Some((base, (level, multi_level))) = inflected {
        return Some(Hit {
            base,
            level,
            multi_level,
            matched: Match::Inflection,
        });
    }
    for (base, rule) in derivation_candidates(w) {
        let mut candidates = vec![base.clone()];
        candidates.extend(inflection_candidates(&base, true));
        for cand in candidates {
            if let Some((level, multi_level)) = lex.level(&cand) {
                return Some(Hit {
                    base: cand,
                    level,
                    multi_level,
                    matched: Match::Derived { rule },
                });
            }
        }
    }
    None
}

/// Level by exact match or inflection only (used for the parts of a compound).
fn lookup_simple(lex: &Lexicon, w: &str) -> Option<u8> {
    lex.level(w).map(|l| l.0).or_else(|| {
        inflection_candidates(w, true)
            .iter()
            .find_map(|b| lex.level(b).map(|l| l.0))
    })
}

/// Closed compounds are mostly not listed: classroom -> class + room.
fn split_compound(lex: &Lexicon, w: &str) -> Option<Vec<Component>> {
    if !w.is_ascii() || w.len() < 6 {
        return None;
    }
    (3..w.len() - 2).find_map(|i| {
        let (a, b) = w.split_at(i);
        let (la, _) = lex.level(a)?;
        let lb = lookup_simple(lex, b)?;
        Some(vec![
            Component {
                part: a.to_string(),
                level: Some(la),
            },
            Component {
                part: b.to_string(),
                level: Some(lb),
            },
        ])
    })
}

/// The level of single words (used where a list of key words needs levels from data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WordLevel {
    pub word: String,
    pub level: Option<u8>,
    pub base: Option<String>,
    /// `exact` | `inflection` | `derived` | `multiword` | `unlisted` | `proper_or_unlisted`
    pub status: &'static str,
    pub multi_level: bool,
}

pub fn word_level(lex: &Lexicon, word: &str) -> WordLevel {
    let key = word.trim();
    let none = |status| WordLevel {
        word: key.to_string(),
        level: None,
        base: None,
        status,
        multi_level: false,
    };
    if key.contains(' ') || key.contains('-') {
        return none("multiword");
    }
    match lookup(lex, &key.replace('’', "'").to_lowercase()) {
        Some(hit) => WordLevel {
            word: key.to_string(),
            level: Some(hit.level),
            status: hit.matched.label(),
            base: Some(hit.base),
            multi_level: hit.multi_level,
        },
        None if key.chars().next().is_some_and(char::is_uppercase) => none("proper_or_unlisted"),
        None => none("unlisted"),
    }
}

// ---------------------------------------------------------------- analysis output

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Component {
    pub part: String,
    pub level: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelCount {
    pub level: u8,
    pub tokens: usize,
    /// tokens / counted_tokens
    pub token_ratio: f64,
    pub types: usize,
    /// types / counted_types
    pub type_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Excluded {
    pub proper_nouns: usize,
    pub unknown: usize,
    pub multiword: usize,
    pub appendix: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Basis {
    /// Denominator of `token_ratio`: running words matched to the list.
    pub counted_tokens: usize,
    /// Denominator of `type_ratio`: distinct base words matched to the list.
    pub counted_types: usize,
    pub denominator_note: &'static str,
    /// Running words left out of the denominators, by reason.
    pub excluded_tokens: Excluded,
    /// counted tokens by `exact` | `inflection` | `derived`
    pub match_types: BTreeMap<&'static str, usize>,
}

/// One occurrence in the text. `line` and `col` are 1-based (col counts characters).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Position {
    pub form: String,
    pub line: usize,
    pub col: usize,
    #[serde(flatten)]
    pub matched: Match,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AboveTarget {
    pub base: String,
    pub level: u8,
    pub count: usize,
    pub positions: Vec<Position>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FlaggedForm {
    pub word: String,
    pub base: String,
    pub level: u8,
    /// `ambiguous`: the level the form is also listed at; `derived`: none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub also_listed_level: Option<u8>,
    /// `derived`: the rule applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MultiLevelWord {
    pub base: String,
    pub levels: Vec<u8>,
    pub used_level: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Occurrence {
    pub line: usize,
    pub col: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// multiword: `hyphenated` | `compound` | `phrase`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_levels: Option<Vec<Component>>,
    /// hyphenated: the level when the list has it as one word (e.g. `e-mail`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listed_as_single_word_level: Option<u8>,
    /// appendix words
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Occurrences of one surface form (grouped case-sensitively).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed {
    pub word: String,
    pub count: usize,
    /// At most [`MAX_OCCURRENCES`].
    pub occurrences: Vec<Occurrence>,
}

pub const MAX_OCCURRENCES: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Analysis {
    pub wordlist_version: String,
    pub target_level: u8,
    pub basis: Basis,
    pub level_distribution: Vec<LevelCount>,
    /// Highest level first, then alphabetical.
    pub above_target: Vec<AboveTarget>,
    pub ambiguous_forms: Vec<FlaggedForm>,
    pub derived_matches: Vec<FlaggedForm>,
    pub multi_level_words: Vec<MultiLevelWord>,
    pub proper_nouns: Vec<Listed>,
    pub unknown: Vec<Listed>,
    pub multiword: Vec<Listed>,
    pub appendix_words: Vec<Listed>,
    pub limits: &'static [&'static str],
    pub disclaimer: &'static str,
}

// ---------------------------------------------------------------- analysis

struct Token {
    start: usize,
    end: usize,
    raw: String,
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '’'
}

/// Words made of ASCII letters, with inner apostrophes and hyphens:
/// `[A-Za-z]+(?:['’][A-Za-z]+)*(?:-[A-Za-z]+(?:['’][A-Za-z]+)*)*`.
fn tokens(chars: &[char]) -> Vec<Token> {
    let letter = |i: usize| chars.get(i).is_some_and(char::is_ascii_alphabetic);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !letter(i) {
            i += 1;
            continue;
        }
        let start = i;
        loop {
            while letter(i) {
                i += 1;
            }
            let joiner = chars.get(i).is_some_and(|c| is_apostrophe(*c) || *c == '-');
            if joiner && letter(i + 1) {
                i += 1;
            } else {
                break;
            }
        }
        let raw: String = chars[start..i]
            .iter()
            .map(|c| if *c == '’' { '\'' } else { *c })
            .collect();
        out.push(Token { start, end: i, raw });
    }
    out
}

/// Whether the word starting at `p` begins a sentence, a line, a quotation or a list item.
fn sentence_start(chars: &[char], p: usize) -> bool {
    let mut i = p;
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i == 0 || chars[i..p].contains(&'\n') {
        return true;
    }
    let prev = chars[i - 1];
    if matches!(prev, '"' | '“' | '(') {
        return true;
    }
    if i == p {
        return false;
    }
    matches!(prev, '.' | '!' | '?' | ':' | ';' | '-' | '•' | '*')
        || (prev == ')' && i >= 2 && chars[i - 2].is_ascii_digit())
}

fn strip_contraction(low: &str) -> String {
    let base = if low.contains('\'') {
        CONTRACTION_SUFFIXES
            .iter()
            .find_map(|s| low.strip_suffix(s))
            .unwrap_or(low)
    } else {
        low
    };
    match base {
        "ca" => "can",
        "wo" => "will",
        "sha" => "shall",
        other => other,
    }
    .to_string()
}

fn group(items: Vec<(String, Occurrence)>) -> Vec<Listed> {
    let mut order: Vec<String> = Vec::new();
    let mut by_word: HashMap<String, Vec<Occurrence>> = HashMap::new();
    for (word, occ) in items {
        if !by_word.contains_key(&word) {
            order.push(word.clone());
        }
        by_word.entry(word).or_default().push(occ);
    }
    order.sort_by_key(|w| w.to_lowercase());
    order
        .into_iter()
        .map(|word| {
            let mut occurrences = by_word.remove(&word).unwrap_or_default();
            let count = occurrences.len();
            occurrences.truncate(MAX_OCCURRENCES);
            Listed {
                word,
                count,
                occurrences,
            }
        })
        .collect()
}

fn ratio(n: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (n as f64 / total as f64 * 10_000.0).round() / 10_000.0
    }
}

/// Analyses `text` against `lex`. `target` is the highest level considered within reach (1–6);
/// `phrases` are optional multiword expressions to set aside (matched case-insensitively).
pub fn analyze(text: &str, lex: &Lexicon, target: u8, phrases: &[String]) -> Analysis {
    let chars: Vec<char> = text.chars().collect();
    let mut line_starts = vec![0usize];
    line_starts.extend(
        chars
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i + 1),
    );
    let pos_of = |off: usize| -> (usize, usize) {
        let line = line_starts.partition_point(|s| *s <= off) - 1;
        (line + 1, off - line_starts[line] + 1)
    };
    let context = |a: usize, b: usize| -> String {
        let s: String = chars[a.saturating_sub(30)..(b + 30).min(chars.len())]
            .iter()
            .map(|c| if *c == '\n' { ' ' } else { *c })
            .collect();
        s.trim().to_string()
    };
    let at = |off: usize| -> Occurrence {
        let (line, col) = pos_of(off);
        Occurrence {
            line,
            col,
            ..Occurrence::default()
        }
    };

    let mut multiword: Vec<(String, Occurrence)> = Vec::new();
    let mut masked = chars.clone();
    let word_char = |c: char| c.is_alphanumeric() || c == '_';
    for phrase in phrases {
        let p: Vec<char> = phrase.trim().chars().collect();
        if p.is_empty() || p.len() > chars.len() {
            continue;
        }
        let mut i = 0;
        while i + p.len() <= chars.len() {
            let end = i + p.len();
            let same = chars[i..end]
                .iter()
                .zip(&p)
                .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()));
            let bounded = (i == 0 || !word_char(chars[i - 1]) || !word_char(p[0]))
                && (end == chars.len() || !word_char(chars[end]) || !word_char(p[p.len() - 1]));
            if same && bounded {
                multiword.push((
                    chars[i..end].iter().collect(),
                    Occurrence {
                        kind: Some("phrase"),
                        ..at(i)
                    },
                ));
                masked[i..end].fill(' ');
                i = end;
            } else {
                i += 1;
            }
        }
    }

    struct Counted {
        word: String,
        hit: Hit,
        start: usize,
        end: usize,
    }
    let mut counted: Vec<Counted> = Vec::new();
    let mut proper: Vec<(String, Occurrence)> = Vec::new();
    let mut unknown: Vec<(String, Occurrence)> = Vec::new();
    let mut appendix: Vec<(String, Occurrence)> = Vec::new();
    let mut multi_level: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    for t in tokens(&masked) {
        let raw = t.raw;
        let low = raw.to_ascii_lowercase();
        if CONTRACTION_PARTS.contains(&low.as_str()) || (raw.len() == 1 && low != "a" && low != "i")
        {
            continue;
        }
        let base_raw = strip_contraction(&low);
        let is_cap = raw.starts_with(|c: char| c.is_ascii_uppercase());
        if base_raw.contains('-') {
            let parts: Vec<&str> = base_raw.split('-').filter(|p| !p.is_empty()).collect();
            if is_cap && !parts.iter().any(|p| lookup(lex, p).is_some()) {
                proper.push((raw, at(t.start)));
                continue;
            }
            let single = lookup(lex, &base_raw.replace('-', "")).or_else(|| lookup(lex, &base_raw));
            multiword.push((
                raw,
                Occurrence {
                    kind: Some("hyphenated"),
                    component_levels: Some(
                        parts
                            .iter()
                            .map(|p| Component {
                                part: (*p).to_string(),
                                level: lookup(lex, p).map(|h| h.level),
                            })
                            .collect(),
                    ),
                    listed_as_single_word_level: single
                        .filter(|h| h.matched == Match::Exact)
                        .map(|h| h.level),
                    ..at(t.start)
                },
            ));
            continue;
        }
        let initial = sentence_start(&chars, t.start);
        let hit = lookup(lex, &base_raw);
        if let Some(category) = lex.appendix.get(&low) {
            let applies = is_cap || LOWERCASE_APPENDIX.contains(&category.as_str());
            if applies && (hit.is_none() || (is_cap && !initial)) {
                appendix.push((
                    raw,
                    Occurrence {
                        category: Some(category.clone()),
                        ..at(t.start)
                    },
                ));
                continue;
            }
        }
        // Capitalised mid-sentence and not listed as such (unknown, or reachable only through
        // rules): a name.
        if is_cap && !initial && hit.as_ref().is_none_or(|h| h.matched != Match::Exact) {
            proper.push((raw, at(t.start)));
            continue;
        }
        let Some(hit) = hit else {
            if !is_cap {
                if let Some(parts) = split_compound(lex, &base_raw) {
                    multiword.push((
                        raw,
                        Occurrence {
                            kind: Some("compound"),
                            component_levels: Some(parts),
                            ..at(t.start)
                        },
                    ));
                    continue;
                }
            }
            unknown.push((
                raw,
                Occurrence {
                    note: Some(if is_cap {
                        "句首大寫，可能是專有名詞"
                    } else {
                        "未收錄或無法還原詞形"
                    }),
                    context: Some(context(t.start, t.end)),
                    ..at(t.start)
                },
            ));
            continue;
        };
        if hit.multi_level {
            if let Some(levels) = lex.levels.get(&hit.base) {
                multi_level.insert(hit.base.clone(), levels.iter().copied().collect());
            }
        }
        counted.push(Counted {
            word: raw,
            hit,
            start: t.start,
            end: t.end,
        });
    }

    // A sentence-initial unknown word that also occurs as a proper noun elsewhere is that name.
    let names: HashSet<String> = proper.iter().map(|(w, _)| w.clone()).collect();
    let (moved, unknown): (Vec<_>, Vec<_>) =
        unknown.into_iter().partition(|(w, _)| names.contains(w));
    proper.extend(moved.into_iter().map(|(w, o)| {
        (
            w,
            Occurrence {
                line: o.line,
                col: o.col,
                ..Occurrence::default()
            },
        )
    }));

    let n = counted.len();
    let mut types: BTreeMap<&str, u8> = BTreeMap::new();
    for c in &counted {
        types.entry(c.hit.base.as_str()).or_insert(c.hit.level);
    }
    let level_distribution: Vec<LevelCount> = (1..=6u8)
        .map(|level| {
            let tokens = counted.iter().filter(|c| c.hit.level == level).count();
            let ty = types.values().filter(|l| **l == level).count();
            LevelCount {
                level,
                tokens,
                token_ratio: ratio(tokens, n),
                types: ty,
                type_ratio: ratio(ty, types.len()),
            }
        })
        .collect();

    let mut above: BTreeMap<(std::cmp::Reverse<u8>, String), Vec<Position>> = BTreeMap::new();
    let mut match_types: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut ambiguous: BTreeSet<FlaggedForm> = BTreeSet::new();
    let mut derived: BTreeSet<FlaggedForm> = BTreeSet::new();
    for c in &counted {
        *match_types.entry(c.hit.matched.label()).or_default() += 1;
        let flagged = |also_listed_level, rule| FlaggedForm {
            word: c.word.to_lowercase(),
            base: c.hit.base.clone(),
            level: c.hit.level,
            also_listed_level,
            rule,
        };
        match c.hit.matched {
            Match::Ambiguous { also_listed_level } => {
                ambiguous.insert(flagged(Some(also_listed_level), None));
            }
            Match::Derived { rule } => {
                derived.insert(flagged(None, Some(rule)));
            }
            Match::Exact | Match::Inflection => {}
        }
        if c.hit.level > target {
            let (line, col) = pos_of(c.start);
            above
                .entry((std::cmp::Reverse(c.hit.level), c.hit.base.clone()))
                .or_default()
                .push(Position {
                    form: c.word.clone(),
                    line,
                    col,
                    matched: c.hit.matched.clone(),
                    context: context(c.start, c.end),
                });
        }
    }

    Analysis {
        wordlist_version: lex.version.clone(),
        target_level: target,
        basis: Basis {
            counted_tokens: n,
            counted_types: types.len(),
            denominator_note: DENOMINATOR_NOTE,
            excluded_tokens: Excluded {
                proper_nouns: proper.len(),
                unknown: unknown.len(),
                multiword: multiword.len(),
                appendix: appendix.len(),
            },
            match_types,
        },
        level_distribution,
        above_target: above
            .into_iter()
            .map(|((level, base), positions)| AboveTarget {
                base,
                level: level.0,
                count: positions.len(),
                positions,
            })
            .collect(),
        ambiguous_forms: ambiguous.into_iter().collect(),
        derived_matches: derived.into_iter().collect(),
        multi_level_words: multi_level
            .into_iter()
            .map(|(base, levels)| MultiLevelWord {
                used_level: levels.first().copied().unwrap_or_default(),
                base,
                levels,
            })
            .collect(),
        proper_nouns: group(proper),
        unknown: group(unknown),
        multiword: group(multiword),
        appendix_words: group(appendix),
        limits: &LIMITS,
        disclaimer: DISCLAIMER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small synthetic list (levels are made up for the tests; not the CEEC data).
    fn lexicon() -> Lexicon {
        let listed = [
            ("a", 1, "art."),
            ("an", 1, "art."),
            ("i", 1, "pron."),
            ("the", 1, "art."),
            ("be", 1, "v."),
            ("do", 1, "aux./v."),
            ("not", 1, "adv."),
            ("can", 1, "aux."),
            ("she", 1, "pron."),
            ("we", 1, "pron."),
            ("us", 1, "pron."),
            ("use", 1, "v./n."),
            ("it", 1, "pron."),
            ("and", 1, "conj."),
            ("in", 1, "prep."),
            ("to", 1, "prep."),
            ("class", 1, "n."),
            ("room", 1, "n."),
            ("teacher", 1, "n."),
            ("book", 1, "n./v."),
            ("study", 1, "v./n."),
            ("stop", 1, "v./n."),
            ("big", 1, "adj."),
            ("happy", 1, "adj."),
            ("break", 1, "v./n."),
            ("broke", 4, "adj."),
            ("die", 1, "v."),
            ("may", 1, "aux."),
            ("march", 3, "v./n."),
            ("well", 1, "adv."),
            ("known", 2, "adj."),
            ("know", 1, "v."),
            ("capital", 2, "n./adj."),
            ("capital", 4, "n."),
            ("flood", 2, "n./v."),
            ("resist", 3, "v."),
            ("usual", 2, "adj."),
            ("quick", 1, "adj."),
            ("conserve", 6, "v."),
            ("momentum", 6, "n."),
            ("civic", 5, "adj."),
            ("o'clock", 1, "adv."),
            ("e-mail", 3, "n./v."),
            ("child", 1, "n."),
            ("three", 9, ""), // replaced below by the appendix row
        ];
        let mut rows: Vec<LexiconRow> = listed
            .iter()
            .filter(|(w, _, _)| *w != "three")
            .map(|(w, l, p)| LexiconRow {
                word: (*w).to_string(),
                level: Some(*l),
                pos: (*p).to_string(),
                kind: "base".into(),
                appendix_category: String::new(),
            })
            .collect();
        for (w, cat) in [
            ("three", "Cardinal Numbers"),
            ("May", "Months"),
            ("March", "Months"),
            ("Monday", "Days of the Week"),
            ("Japan", "Countries and Areas"),
        ] {
            rows.push(LexiconRow {
                word: w.into(),
                level: None,
                pos: String::new(),
                kind: "appendix".into(),
                appendix_category: cat.into(),
            });
        }
        Lexicon::new("TEST LIST v1", rows)
    }

    fn hit(w: &str) -> (String, u8, Match) {
        let h = lookup(&lexicon(), w).unwrap_or_else(|| panic!("{w} not found"));
        (h.base, h.level, h.matched)
    }

    #[test]
    fn irregular_table_is_built() {
        assert_eq!(IRREGULAR["went"], vec!["go"]);
        assert_eq!(IRREGULAR["children"], vec!["child"]);
        assert_eq!(IRREGULAR["better"], vec!["good", "well"]);
    }

    #[test]
    fn inflections_resolve_to_the_base_word() {
        assert_eq!(hit("books"), ("book".into(), 1, Match::Inflection));
        assert_eq!(hit("studies"), ("study".into(), 1, Match::Inflection));
        assert_eq!(hit("studied"), ("study".into(), 1, Match::Inflection));
        assert_eq!(hit("stopped"), ("stop".into(), 1, Match::Inflection));
        assert_eq!(hit("stopping"), ("stop".into(), 1, Match::Inflection));
        assert_eq!(hit("dying"), ("die".into(), 1, Match::Inflection));
        assert_eq!(hit("bigger"), ("big".into(), 1, Match::Inflection));
        assert_eq!(hit("happiest"), ("happy".into(), 1, Match::Inflection));
        assert_eq!(hit("children"), ("child".into(), 1, Match::Inflection));
        assert_eq!(hit("was"), ("be".into(), 1, Match::Inflection));
        assert_eq!(hit("classes"), ("class".into(), 1, Match::Inflection));
        // "used" could be us+ed: the verb reading wins
        assert_eq!(hit("used"), ("use".into(), 1, Match::Inflection));
        assert_eq!(hit("room"), ("room".into(), 1, Match::Exact));
        assert!(lookup(&lexicon(), "zzzed").is_none());
        assert!(lookup(&lexicon(), "café").is_none());
    }

    #[test]
    fn ambiguous_forms_take_the_lower_level_and_are_flagged() {
        assert_eq!(
            hit("broke"),
            (
                "break".into(),
                1,
                Match::Ambiguous {
                    also_listed_level: 4
                }
            )
        );
        // "known" is listed (L2) and is also the participle of "know" (L1)
        assert_eq!(hit("known").0, "know");
    }

    #[test]
    fn derivations_are_labelled_not_merged() {
        assert_eq!(
            hit("unhappy"),
            ("happy".into(), 1, Match::Derived { rule: "un-" })
        );
        assert_eq!(
            hit("quickly"),
            ("quick".into(), 1, Match::Derived { rule: "-ly" })
        );
        assert_eq!(
            hit("happily"),
            ("happy".into(), 1, Match::Derived { rule: "-ly" })
        );
        assert_eq!(
            hit("usually"),
            ("usual".into(), 2, Match::Derived { rule: "-ly" })
        );
        assert_eq!(
            hit("happiness"),
            ("happy".into(), 1, Match::Derived { rule: "-ness" })
        );
        assert_eq!(
            hit("restudied"),
            ("study".into(), 1, Match::Derived { rule: "re-" })
        );
        // derivations are not chained
        assert!(lookup(&lexicon(), "unhappily").is_none());
    }

    #[test]
    fn tokenizer_keeps_apostrophes_and_hyphens_inside_words() {
        let chars: Vec<char> = "Don’t stop — flood-resistant O'clock's 3rd x"
            .chars()
            .collect();
        let t: Vec<String> = tokens(&chars).into_iter().map(|t| t.raw).collect();
        assert_eq!(
            t,
            vec!["Don't", "stop", "flood-resistant", "O'clock's", "rd", "x"]
        );
    }

    #[test]
    fn sentence_starts() {
        let text = "We stop. She can.\nMarch on! (May it) \"We\" 1) Stop - Stop and Stop";
        let chars: Vec<char> = text.chars().collect();
        let starts: Vec<(String, bool)> = tokens(&chars)
            .into_iter()
            .map(|t| (t.raw, sentence_start(&chars, t.start)))
            .collect();
        let expect = [
            ("We", true),
            ("stop", false),
            ("She", true),
            ("can", false),
            ("March", true),
            ("on", false),
            ("May", true),
            ("it", false),
            ("We", true),
            ("Stop", true),
            ("Stop", true),
            ("and", false),
            ("Stop", false),
        ];
        assert_eq!(starts, expect.map(|(w, s)| (w.to_string(), s)).to_vec());
    }

    #[test]
    fn analysis_reports_distribution_with_denominators() {
        let text = "She studies in the classroom. We can't conserve momentum, and Taipei is big.\n\
                    Monday in Japan: three flood-resistant books broke. Blorf is civic.";
        let a = analyze(text, &lexicon(), 4, &[]);
        assert_eq!(a.wordlist_version, "TEST LIST v1");
        assert_eq!(a.target_level, 4);
        // she studies in the | we can't conserve momentum and … is big | in | books broke | is civic
        assert_eq!(a.basis.counted_tokens, 16);
        let total: usize = a.level_distribution.iter().map(|l| l.tokens).sum();
        assert_eq!(
            total, a.basis.counted_tokens,
            "every counted token has a level"
        );
        let types: usize = a.level_distribution.iter().map(|l| l.types).sum();
        assert_eq!(types, a.basis.counted_types);
        let ratios: f64 = a.level_distribution.iter().map(|l| l.token_ratio).sum();
        assert!((ratios - 1.0).abs() < 0.001, "{ratios}");
        assert_eq!(
            a.basis.excluded_tokens,
            Excluded {
                proper_nouns: 1,
                unknown: 1,
                multiword: 2,
                appendix: 3
            }
        );
        assert_eq!(a.level_distribution[5].tokens, 2);
        assert_eq!(a.level_distribution[4].tokens, 1);
        assert_eq!(
            a.level_distribution[3].tokens, 0,
            "broke counts as break (L1)"
        );
    }

    #[test]
    fn analysis_lists_words_above_target_with_positions() {
        let text = "She studies in the classroom. We can't conserve momentum, and Taipei is big.\n\
                    Monday in Japan: three flood-resistant books broke. Blorf is civic.";
        let a = analyze(text, &lexicon(), 4, &[]);
        let above: Vec<(&str, u8)> = a
            .above_target
            .iter()
            .map(|x| (x.base.as_str(), x.level))
            .collect();
        assert_eq!(above, vec![("conserve", 6), ("momentum", 6), ("civic", 5)]);
        let p = &a.above_target[0].positions[0];
        assert_eq!((p.line, p.col, p.form.as_str()), (1, 40, "conserve"));
        assert!(p.context.contains("conserve momentum"));
        let civic = &a.above_target[2].positions[0];
        assert_eq!((civic.line, civic.col), (2, 62));
        // a higher target removes them
        assert!(analyze(text, &lexicon(), 6, &[]).above_target.is_empty());
    }

    #[test]
    fn analysis_separates_proper_unknown_multiword_and_appendix() {
        let text = "She studies in the classroom. We can't conserve momentum, and Taipei is big.\n\
                    Monday in Japan: three flood-resistant books broke. Blorf is civic.";
        let a = analyze(text, &lexicon(), 4, &[]);
        let words = |l: &[Listed]| -> Vec<String> { l.iter().map(|x| x.word.clone()).collect() };
        assert_eq!(words(&a.proper_nouns), vec!["Taipei"]);
        assert_eq!(words(&a.unknown), vec!["Blorf"]);
        assert_eq!(
            a.unknown[0].occurrences[0].note,
            Some("句首大寫，可能是專有名詞")
        );
        assert_eq!(words(&a.multiword), vec!["classroom", "flood-resistant"]);
        let compound = &a.multiword[0].occurrences[0];
        assert_eq!(compound.kind, Some("compound"));
        assert_eq!(
            compound.component_levels.as_deref(),
            Some(
                &[
                    Component {
                        part: "class".into(),
                        level: Some(1)
                    },
                    Component {
                        part: "room".into(),
                        level: Some(1)
                    }
                ][..]
            )
        );
        let hyphen = &a.multiword[1].occurrences[0];
        assert_eq!(hyphen.kind, Some("hyphenated"));
        assert_eq!(
            hyphen.component_levels.as_ref().unwrap()[1],
            Component {
                part: "resistant".into(),
                level: None
            }
        );
        assert_eq!(words(&a.appendix_words), vec!["Japan", "Monday", "three"]);
        assert_eq!(
            a.appendix_words[1].occurrences[0].category.as_deref(),
            Some("Days of the Week")
        );
        assert_eq!(
            a.ambiguous_forms,
            vec![FlaggedForm {
                word: "broke".into(),
                base: "break".into(),
                level: 1,
                also_listed_level: Some(4),
                rule: None
            }]
        );
    }

    #[test]
    fn capitalised_listed_words_and_month_names() {
        // Sentence-initial "May" is the modal; mid-sentence "May" / "March" are months.
        let a = analyze(
            "May I use it? We study in May and March.",
            &lexicon(),
            4,
            &[],
        );
        let months: Vec<&str> = a.appendix_words.iter().map(|x| x.word.as_str()).collect();
        assert_eq!(months, vec!["March", "May"]);
        assert_eq!(a.basis.counted_tokens, 8);
        // "I" mid-sentence is the listed pronoun, not a proper noun
        assert!(a.proper_nouns.is_empty());
        // a name seen mid-sentence is also a name at the start of a sentence
        let a = analyze("Blorf can stop. We know Blorf.", &lexicon(), 4, &[]);
        assert_eq!(a.proper_nouns[0].count, 2);
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn multi_level_words_use_the_lowest_level() {
        let a = analyze("The capital is big.", &lexicon(), 4, &[]);
        assert_eq!(
            a.multi_level_words,
            vec![MultiLevelWord {
                base: "capital".into(),
                levels: vec![2, 4],
                used_level: 2
            }]
        );
        assert_eq!(a.level_distribution[1].tokens, 1);
    }

    #[test]
    fn contractions_possessives_and_listed_hyphenated_words() {
        let a = analyze(
            "We don't stop. The teacher's e-mail can't be used at three o'clock.",
            &lexicon(),
            4,
            &[],
        );
        // we do stop the teacher can be used o'clock (+ "at" unknown, "three" appendix)
        assert_eq!(a.basis.counted_tokens, 9);
        assert_eq!(a.unknown.len(), 1);
        let email = &a.multiword[0];
        assert_eq!(email.word, "e-mail");
        assert_eq!(email.occurrences[0].listed_as_single_word_level, Some(3));
    }

    #[test]
    fn supplied_phrases_are_set_aside() {
        let phrases = vec!["break in".to_string()];
        let a = analyze("We Break In and break into it.", &lexicon(), 4, &phrases);
        assert_eq!(a.multiword.len(), 1);
        assert_eq!(a.multiword[0].word, "Break In");
        assert_eq!(a.multiword[0].occurrences[0].kind, Some("phrase"));
        // "break into" is not the phrase (word boundary): break counts, into is unknown
        assert_eq!(a.basis.counted_tokens, 4);
    }

    #[test]
    fn empty_text_and_empty_lexicon_do_not_divide_by_zero() {
        let a = analyze("", &lexicon(), 4, &[]);
        assert_eq!(a.basis.counted_tokens, 0);
        assert!(a.level_distribution.iter().all(|l| l.token_ratio == 0.0));
        let a = analyze("We stop.", &Lexicon::default(), 4, &[]);
        assert_eq!(a.basis.counted_tokens, 0);
        assert_eq!(a.unknown.len(), 2);
    }

    #[test]
    fn single_word_levels() {
        let lex = lexicon();
        assert_eq!(word_level(&lex, "Studies").level, Some(1));
        assert!(word_level(&lex, "capital").multi_level);
        assert_eq!(word_level(&lex, "flood-resistant").status, "multiword");
        assert_eq!(word_level(&lex, "Taipei").status, "proper_or_unlisted");
        assert_eq!(word_level(&lex, "zzz").status, "unlisted");
    }

    #[test]
    fn output_shape() {
        let a = analyze("She conserves it, unhappy.", &lexicon(), 4, &[]);
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["above_target"][0]["positions"][0]["match"], "inflection");
        assert_eq!(v["derived_matches"][0]["rule"], "un-");
        assert!(v["basis"]["denominator_note"]
            .as_str()
            .unwrap()
            .contains("分母"));
        assert_eq!(v["limits"].as_array().unwrap().len(), LIMITS.len());
    }
}
