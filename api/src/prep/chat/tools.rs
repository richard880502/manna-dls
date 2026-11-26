//! The tools of the prep chat and the working copy they edit (docs/PREP_CHAT.md section 3).
//!
//! Every tool edits `Work::content`, an in-memory copy of the worksheet's latest version, and
//! checks what it writes with the checks the rest of the platform uses (`check_block_change` for
//! blocks, the sentence-frame and hint checks, the word list, the picture limits). A change that
//! fails a check is not made: the reasons go back to the model, which may try again (a tool may
//! fail 3 times on one block: the first try and 2 retries). Nothing reaches the database here.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::ai::chat::ToolSpec;
use crate::prep::run::{block_name, check_block_change, is_tiered, revised_block, same_content};
use crate::prep::{RawChange, CODE_CHARS};
use crate::text::clip_chars;
use crate::vocabulary::Lexicon;
use crate::worksheet::checks::{frame_format, item_frame, item_hint};
use crate::worksheet::images::{self, SlotState};
use crate::worksheet::revise::{
    occurs_in, valid_word, MAX_STEPS, MEANING_CHARS, SCAFFOLD_STEP_CHARS, WORD_CHARS,
};
use crate::worksheet::store::Worksheet;
use crate::worksheet::table::RawTable;
use crate::worksheet::template::{
    enum_of, obj, s_max, strings_max, RawOutItem, ITEM_ANSWER_CHARS, ITEM_OPTIONS,
    ITEM_OPTION_CHARS, ITEM_PROMPT_CHARS, SECTION_INSTRUCTIONS_CHARS, SECTION_TITLE_CHARS,
};
use crate::worksheet::{
    key_vocab, Block, BlockKind, Content, Level, Origin, Scaffold, ScaffoldType, VocabLang,
    ANSWER_FORMATS, KIND_STEPS, KIND_WORD_LIST, MAX_BANK_WORDS, MAX_BLOCKS,
};

/// A failed tool may be tried again twice on the same block.
pub const MAX_ATTEMPTS: u32 = 3;
const MAX_VOCAB_ADD: usize = 10;

pub const T_READ: &str = "read_block";
pub const T_EDIT: &str = "edit_block";
pub const T_SCAFFOLDS: &str = "edit_scaffolds";
pub const T_VOCAB: &str = "edit_key_vocabulary";
pub const T_REDRAW: &str = "redraw_picture";
pub const T_ADD: &str = "add_block";
pub const T_REMOVE: &str = "remove_block";
pub const T_MOVE: &str = "move_block";

// ---------------------------------------------------------------- the specs

fn nullable(v: Value) -> Value {
    json!({ "anyOf": [v, { "type": "null" }] })
}

fn item_schema() -> Value {
    obj(vec![
        ("answer_format", enum_of(&ANSWER_FORMATS)),
        ("prompt", s_max(ITEM_PROMPT_CHARS)),
        ("options", strings_max(ITEM_OPTIONS, ITEM_OPTION_CHARS)),
        ("answer", s_max(ITEM_ANSWER_CHARS)),
        ("table", crate::worksheet::table::schema()),
    ])
}

fn items_schema() -> Value {
    json!({ "type": "array", "items": item_schema(), "maxItems": crate::prep::CHANGE_ITEMS_MAX })
}

fn numbered(field: &str, chars: usize) -> Value {
    json!({
        "type": "array", "maxItems": 30,
        "items": obj(vec![("item_no", json!({ "type": "integer" })), (field, s_max(chars))]),
    })
}

/// The tools offered to the model, every parameter list a strict JSON schema (optional
/// parameters are nullable). Always the same list, in the same order: the request prefix stays
/// the same for the prompt cache.
pub fn specs() -> Vec<ToolSpec> {
    let block_id = || s_max(CODE_CHARS);
    vec![
        ToolSpec {
            name: T_READ,
            description: "讀一區的完整內容：每一題（含參考答案與表格）、程序鷹架與句型框架全文、字詞表、字義註解、圖片格。修改前先讀。",
            parameters: obj(vec![("block_id", block_id())]),
        },
        ToolSpec {
            name: T_EDIT,
            description: "修改一區的標題、作答指示或題目。title、instructions 給 null 表示不改；items 給 null 表示題目不改，否則要寫出這一區完整的題目清單（沒改的題目照抄，包含表格），會整個取代這一區的題目。",
            parameters: obj(vec![
                ("block_id", block_id()),
                ("title", nullable(s_max(SECTION_TITLE_CHARS))),
                ("instructions", nullable(s_max(SECTION_INSTRUCTIONS_CHARS))),
                ("items", nullable(items_schema())),
            ]),
        },
        ToolSpec {
            name: T_SCAFFOLDS,
            description: "修改一區的鷹架：steps（程序鷹架的步驟）、frames（逐題的句型框架，frame 給空字串表示拿掉）、hints（逐題的提示，hint 給空字串表示拿掉）、word_bank（Word bank 的字、字義語言與要印出字義的字）。不改的部分給 null。item_no 是這一區題目的順序（從 1 開始）。",
            parameters: obj(vec![
                ("block_id", block_id()),
                ("steps", nullable(strings_max(MAX_STEPS, SCAFFOLD_STEP_CHARS))),
                ("frames", nullable(numbered("frame", crate::worksheet::checks::ITEM_FRAME_CHARS))),
                ("hints", nullable(numbered("hint", crate::worksheet::checks::ITEM_HINT_CHARS))),
                (
                    "word_bank",
                    nullable(obj(vec![
                        ("words", strings_max(MAX_BANK_WORDS, WORD_CHARS)),
                        ("lang", enum_of(&["en", "zh"])),
                        ("show_meaning", strings_max(MAX_BANK_WORDS, WORD_CHARS)),
                    ])),
                ),
            ]),
        },
        ToolSpec {
            name: T_VOCAB,
            description: "增減 Key Vocabulary（字詞表）的字，或切換字義語言。add 的字要出現在教材或這一區裡；級別由平台查詞彙表，不用寫。block_id 給 null 表示有 Key Vocabulary 的那一區。language 給 null 表示不改字義語言（en 英文、zh 中文）。",
            parameters: obj(vec![
                ("block_id", nullable(block_id())),
                (
                    "add",
                    json!({ "type": "array", "maxItems": MAX_VOCAB_ADD, "items": obj(vec![
                        ("word", s_max(WORD_CHARS)),
                        ("pos", s_max(20)),
                        ("definition", s_max(MEANING_CHARS)),
                        ("meaning_zh", s_max(60)),
                    ]) }),
                ),
                ("remove", strings_max(MAX_VOCAB_ADD, WORD_CHARS)),
                ("language", nullable(enum_of(&["en", "zh"]))),
            ]),
        },
        ToolSpec {
            name: T_REDRAW,
            description: "依老師的要求重畫一張圖。note 是給繪圖的簡短說明（英文或中文，例如要改成女生、不要有文字）。每份學習單的重畫次數有上限；圖在整輪結束、新版本寫好之後才開始畫。",
            parameters: obj(vec![
                ("slot_id", s_max(CODE_CHARS)),
                ("note", s_max(crate::worksheet::images::HINT_CHARS)),
            ]),
        },
        ToolSpec {
            name: T_ADD,
            description: "新增一個活動區塊（title、instructions、items 的規則同 edit_block）。after_block_id 給 null 表示放在最前面。",
            parameters: obj(vec![
                ("after_block_id", nullable(block_id())),
                ("title", s_max(SECTION_TITLE_CHARS)),
                ("instructions", s_max(SECTION_INSTRUCTIONS_CHARS)),
                ("items", items_schema()),
            ]),
        },
        ToolSpec {
            name: T_REMOVE,
            description: "關閉一個區塊（不會真的刪除，老師可以復原）。學習單至少要留一個區塊。",
            parameters: obj(vec![("block_id", block_id())]),
        },
        ToolSpec {
            name: T_MOVE,
            description: "調整區塊的順序：把 block_id 移到 after_block_id 後面；after_block_id 給 null 表示移到最前面。",
            parameters: obj(vec![
                ("block_id", block_id()),
                ("after_block_id", nullable(block_id())),
            ]),
        },
    ]
}

// ---------------------------------------------------------------- arguments

#[derive(Deserialize)]
struct ReadArgs {
    block_id: String,
}

#[derive(Deserialize)]
struct EditArgs {
    block_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    items: Option<Vec<RawOutItem>>,
}

#[derive(Deserialize)]
struct Numbered {
    item_no: i64,
    #[serde(default)]
    frame: String,
}

#[derive(Deserialize)]
struct HintNo {
    item_no: i64,
    #[serde(default)]
    hint: String,
}

#[derive(Deserialize)]
struct BankArgs {
    #[serde(default)]
    words: Vec<String>,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    show_meaning: Vec<String>,
}

#[derive(Deserialize)]
struct ScaffoldArgs {
    block_id: String,
    #[serde(default)]
    steps: Option<Vec<String>>,
    #[serde(default)]
    frames: Option<Vec<Numbered>>,
    #[serde(default)]
    hints: Option<Vec<HintNo>>,
    #[serde(default)]
    word_bank: Option<BankArgs>,
}

#[derive(Deserialize)]
struct AddWord {
    word: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    definition: String,
    #[serde(default)]
    meaning_zh: String,
}

#[derive(Deserialize)]
struct VocabArgs {
    #[serde(default)]
    block_id: Option<String>,
    #[serde(default)]
    add: Vec<AddWord>,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Deserialize)]
struct RedrawArgs {
    slot_id: String,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct AddBlockArgs {
    #[serde(default)]
    after_block_id: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    items: Vec<RawOutItem>,
}

#[derive(Deserialize)]
struct RemoveArgs {
    block_id: String,
}

#[derive(Deserialize)]
struct MoveArgs {
    block_id: String,
    #[serde(default)]
    after_block_id: Option<String>,
}

// ---------------------------------------------------------------- the working copy

/// What one tool call came to.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub ok: bool,
    /// What the model reads (`role: "tool"`).
    pub text: String,
    /// Logged with `prep_chat_tool` (never the teacher's words): `ok`, `bad_arguments`,
    /// `unknown_block`, `check_failed`, `retry_limit`, `limit`, `unavailable`.
    pub code: &'static str,
    pub block_id: Option<String>,
    /// The progress step: 「修改 C 區」.
    pub step: String,
}

impl ToolResult {
    fn ok(text: impl Into<String>, block_id: Option<String>, step: String) -> Self {
        Self {
            ok: true,
            text: text.into(),
            code: "ok",
            block_id,
            step,
        }
    }

    fn fail(
        code: &'static str,
        reasons: &[String],
        block_id: Option<String>,
        step: String,
    ) -> Self {
        let list = reasons
            .iter()
            .take(12)
            .map(|r| format!("- {}", clip_chars(r, 300)))
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            ok: false,
            text: format!("沒有改成（沒有通過學習單的檢查，這一區維持原樣）：\n{list}"),
            code,
            block_id,
            step,
        }
    }
}

/// A picture the turn asked to redraw; queued after the version is written.
#[derive(Debug, Clone)]
pub struct Redraw {
    pub slot_id: String,
    pub block_id: String,
    pub note: String,
}

pub struct Work<'a> {
    pub ws: &'a Worksheet,
    pub turn_id: i64,
    pub original: Content,
    pub content: Content,
    /// The versions the worksheet has (a block of a 差異化學習單 is in all three).
    levels: Vec<Level>,
    pub passage: String,
    pub lexicon: Option<Lexicon>,
    pub states: Vec<SlotState>,
    pub pictures_on: bool,
    pub storage_on: bool,
    pub regenerations_left: i64,
    /// Letters of the blocks as the student sheet prints them (original and working copy).
    pub labels: HashMap<String, String>,
    original_labels: HashMap<String, String>,
    printed: HashMap<String, usize>,
    /// Failed attempts per (tool, block key).
    attempts: HashMap<(String, String), u32>,
    /// The last failure of every (tool, block) that has not succeeded since: what was not done.
    pub failed: Vec<((String, String), (String, String))>,
    pub redraws: Vec<Redraw>,
    /// Blocks moved by `move_block`.
    pub moved: Vec<String>,
    pub added: Vec<String>,
    new_block_no: usize,
}

impl<'a> Work<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ws: &'a Worksheet,
        turn_id: i64,
        content: Content,
        passage: String,
        lexicon: Option<Lexicon>,
        states: Vec<SlotState>,
        pictures_on: bool,
        storage_on: bool,
        regenerations_left: i64,
    ) -> Self {
        let tiered = is_tiered(ws, &content);
        let levels: Vec<Level> = if !content.levels.is_empty() {
            content.levels.clone()
        } else if tiered {
            Level::ALL.to_vec()
        } else {
            vec![Level::Standard]
        };
        let mut work = Self {
            ws,
            turn_id,
            original: content.clone(),
            content,
            levels,
            passage,
            lexicon,
            states,
            pictures_on,
            storage_on,
            regenerations_left,
            labels: HashMap::new(),
            original_labels: HashMap::new(),
            printed: HashMap::new(),
            attempts: HashMap::new(),
            failed: Vec::new(),
            redraws: Vec::new(),
            moved: Vec::new(),
            added: Vec::new(),
            new_block_no: 0,
        };
        work.refresh_labels();
        work.original_labels = work.labels.clone();
        work
    }

    /// Letters and printed numbers of the working copy.
    pub fn refresh_labels(&mut self) {
        let level = self
            .content
            .levels
            .first()
            .copied()
            .unwrap_or(Level::Standard);
        let sheet = crate::worksheet::assemble::project(
            &self.ws.title,
            &self.content,
            level,
            crate::worksheet::Audience::Teacher,
        );
        self.labels = sheet
            .blocks
            .iter()
            .map(|b| (b.id.clone(), b.label.clone()))
            .collect();
        self.printed = sheet
            .blocks
            .iter()
            .flat_map(|b| b.items.iter().map(|i| (i.id.clone(), i.no)))
            .collect();
    }

    pub fn printed(&self) -> &HashMap<String, usize> {
        &self.printed
    }

    /// 「C 區」, or the title of a block that has no letter; looks at the original too (a block
    /// that is gone from the working copy).
    pub fn name(&self, block_id: &str) -> String {
        let block = self
            .content
            .block(block_id)
            .or_else(|| self.original.block(block_id));
        let label = self
            .labels
            .get(block_id)
            .or_else(|| self.original_labels.get(block_id))
            .cloned()
            .unwrap_or_default();
        match block {
            Some(b) => block_name(&label, &b.title),
            None => format!("「{}」", clip_chars(block_id, 30)),
        }
    }

    fn enabled_ids(&self) -> Vec<String> {
        self.content
            .blocks
            .iter()
            .filter(|b| b.enabled)
            .map(|b| b.id.clone())
            .collect()
    }

    fn unknown_block(&self, id: &str, step: String) -> ToolResult {
        ToolResult::fail(
            "unknown_block",
            &[format!(
                "block_id「{}」不在這份學習單裡。可用的 block_id：{}",
                clip_chars(id, 40),
                self.enabled_ids().join("、")
            )],
            None,
            step,
        )
    }

    fn step_for(&self, tool: &str, block_id: &str) -> String {
        let name = self.name(block_id);
        match tool {
            T_READ => format!("讀 {name}"),
            T_EDIT => format!("修改 {name}"),
            T_SCAFFOLDS => format!("改 {name} 的鷹架"),
            T_VOCAB => "改 Key Vocabulary".to_string(),
            T_REDRAW => format!("重畫 {name} 的圖"),
            T_ADD => "新增一區".to_string(),
            T_REMOVE => format!("關閉 {name}"),
            T_MOVE => format!("移動 {name}"),
            _ => "未知的工具".to_string(),
        }
    }

    /// The block (or slot) a call is about, for the progress step and the log.
    pub fn target_of(&self, tool: &str, args: &Value) -> String {
        let id = args["block_id"].as_str().or_else(|| {
            args["slot_id"]
                .as_str()
                .and_then(|s| images::find_slot(&self.content, s).map(|(b, _)| b.id.as_str()))
        });
        let id = id.unwrap_or_default();
        self.step_for(tool, id)
    }

    fn key_of(tool: &str, args: &Value) -> (String, String) {
        let key = args["block_id"]
            .as_str()
            .or_else(|| args["slot_id"].as_str())
            .unwrap_or("")
            .to_string();
        (tool.to_string(), key)
    }

    /// Runs one tool call. A failure is counted: the third failure of a tool on a block is the
    /// last one tried.
    pub fn exec(&mut self, tool: &str, arguments: &str) -> ToolResult {
        let args: Value = match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult::fail(
                    "bad_arguments",
                    &[format!("參數不是正確的 JSON：{e}")],
                    None,
                    self.step_for(tool, ""),
                )
            }
        };
        let key = Self::key_of(tool, &args);
        let step = self.target_of(tool, &args);
        let block = args["block_id"].as_str().map(str::to_string);
        if self.attempts.get(&key).copied().unwrap_or(0) >= MAX_ATTEMPTS {
            return ToolResult {
                ok: false,
                text: format!(
                    "這個工具在這一區已經失敗 {MAX_ATTEMPTS} 次，不再重試。請在回答裡說明沒有改成，並問老師要不要換個方式。"
                ),
                code: "retry_limit",
                block_id: block,
                step,
            };
        }
        let result = match tool {
            T_READ => self.read(arguments, step),
            T_EDIT => self.edit(arguments, step),
            T_SCAFFOLDS => self.scaffolds(arguments, step),
            T_VOCAB => self.vocabulary(arguments, step),
            T_REDRAW => self.redraw(arguments, step),
            T_ADD => self.add_block(arguments, step),
            T_REMOVE => self.remove_block(arguments, step),
            T_MOVE => self.move_block(arguments, step),
            other => ToolResult::fail(
                "bad_arguments",
                &[format!("沒有「{}」這個工具。", clip_chars(other, 40))],
                None,
                step,
            ),
        };
        if tool != T_READ {
            if result.ok {
                self.attempts.remove(&key);
                self.failed.retain(|(k, _)| *k != key);
            } else {
                *self.attempts.entry(key.clone()).or_insert(0) += 1;
                let label = match key.1.as_str() {
                    _ if tool == T_VOCAB => "Key Vocabulary".to_string(),
                    "" => "學習單".to_string(),
                    k if tool == T_REDRAW => format!(
                        "{}的圖",
                        images::find_slot(&self.content, k)
                            .map(|(b, _)| self.name(&b.id))
                            .unwrap_or_else(|| "學習單".to_string())
                    ),
                    k => self.name(k),
                };
                let reason = result
                    .text
                    .lines()
                    .skip(1)
                    .map(|l| l.trim_start_matches("- ").to_string())
                    .collect::<Vec<_>>()
                    .join("；");
                let reason = if reason.is_empty() {
                    result.text.clone()
                } else {
                    reason
                };
                self.failed.retain(|(k, _)| *k != key);
                self.failed.push((key, (label, clip_chars(&reason, 300))));
            }
        }
        result
    }

    fn parse<T: for<'de> Deserialize<'de>>(
        &self,
        arguments: &str,
        tool: &str,
        step: &str,
    ) -> Result<T, ToolResult> {
        serde_json::from_str(arguments).map_err(|e| {
            ToolResult::fail(
                "bad_arguments",
                &[format!("{tool} 的參數不符合格式：{e}")],
                None,
                step.to_string(),
            )
        })
    }

    /// The change is kept only when the whole worksheet still validates.
    fn validated(&mut self, before: Content, step: String, block: &str) -> Result<(), ToolResult> {
        if let Err(e) = self.content.normalise() {
            self.content = before;
            return Err(ToolResult::fail(
                "check_failed",
                &[e],
                Some(block.to_string()),
                step,
            ));
        }
        self.refresh_labels();
        Ok(())
    }

    // ------------------------------------------------------------ read_block

    fn read(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: ReadArgs = match self.parse(arguments, T_READ, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let Some(block) = self.content.block(&a.block_id) else {
            return self.unknown_block(&a.block_id, step);
        };
        let detail =
            crate::worksheet::ai::revise::block_detail(block, &self.printed, block.bloom, false);
        let pictures = images::actions::describe_text(block, &self.states);
        let name = self.name(&a.block_id);
        ToolResult::ok(
            format!(
                "{name}（block_id {}，{}）\n{detail}\n圖片：\n{pictures}",
                block.id,
                if block.enabled {
                    "使用中"
                } else {
                    "已關閉"
                }
            ),
            Some(a.block_id),
            step,
        )
    }

    // ------------------------------------------------------------ edit_block

    fn apply_section(
        &mut self,
        old: &Block,
        change: RawChange,
        no: usize,
    ) -> Result<Block, Vec<String>> {
        let objectives = crate::prep::run::sheet_objectives(&self.content);
        let out = check_block_change(&change, old, &self.passage, &objectives)?;
        Ok(revised_block(old, &out, &self.levels, self.turn_id, no))
    }

    fn edit(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: EditArgs = match self.parse(arguments, T_EDIT, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let Some(pos) = self.content.blocks.iter().position(|b| b.id == a.block_id) else {
            return self.unknown_block(&a.block_id, step);
        };
        let old = self.content.blocks[pos].clone();
        let items = a.items.unwrap_or_else(|| {
            old.items
                .iter()
                .map(|i| RawOutItem {
                    answer_format: i.answer_format.clone().unwrap_or_default(),
                    prompt: i.prompt.clone(),
                    options: i.options.clone(),
                    answer: i.answer.clone(),
                    bloom: None,
                    organizer: None,
                    table: i.table.as_ref().map(RawTable::from),
                })
                .collect()
        });
        let change = RawChange {
            block_id: a.block_id.clone(),
            title: a.title.unwrap_or_else(|| old.title.clone()),
            instructions: a.instructions.unwrap_or_else(|| old.instructions.clone()),
            items,
        };
        let block = match self.apply_section(&old, change, pos + 1) {
            Ok(b) => b,
            Err(reasons) => {
                return ToolResult::fail("check_failed", &reasons, Some(a.block_id), step)
            }
        };
        if same_content(&old, &block) {
            return ToolResult::ok(
                "這一區的內容和現在一樣，沒有任何變動。",
                Some(a.block_id),
                step,
            );
        }
        let before = self.content.clone();
        self.content.blocks[pos] = block;
        if let Err(r) = self.validated(before, step.clone(), &a.block_id) {
            return r;
        }
        ToolResult::ok(
            format!("已修改 {}。", self.name(&a.block_id)),
            Some(a.block_id),
            step,
        )
    }

    // ------------------------------------------------------------ edit_scaffolds

    fn scaffolds(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: ScaffoldArgs = match self.parse(arguments, T_SCAFFOLDS, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let Some(pos) = self.content.blocks.iter().position(|b| b.id == a.block_id) else {
            return self.unknown_block(&a.block_id, step);
        };
        let mut block = self.content.blocks[pos].clone();
        let mut problems: Vec<String> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        let levels = self.levels.clone();
        let tag = format!("{}c{}", block.id, self.turn_id);

        if let Some(steps) = &a.steps {
            let cleaned: Vec<String> = steps
                .iter()
                .map(|t| {
                    crate::prep::clean_text(t, SCAFFOLD_STEP_CHARS)
                        .trim()
                        .to_string()
                })
                .filter(|t| !t.is_empty())
                .collect();
            if cleaned.is_empty() || cleaned.len() > MAX_STEPS {
                problems.push(format!("步驟要有 1 到 {MAX_STEPS} 步"));
            } else if let Some(s) = block
                .scaffolds
                .iter_mut()
                .find(|s| s.kind_type == ScaffoldType::Procedural)
            {
                if s.steps != cleaned {
                    s.steps = cleaned;
                    s.origin = Origin::Ai;
                }
            } else {
                block.scaffolds.push(Scaffold {
                    id: format!("{tag}s1"),
                    kind_type: ScaffoldType::Procedural,
                    kind: KIND_STEPS.to_string(),
                    steps: cleaned,
                    levels: levels.clone(),
                    origin: Origin::Ai,
                    ..Scaffold::default()
                });
            }
        }
        for f in a.frames.iter().flatten() {
            let Some(k) = usize::try_from(f.item_no)
                .ok()
                .filter(|k| (1..=block.items.len()).contains(k))
            else {
                problems.push(format!("沒有第 {} 題", f.item_no));
                continue;
            };
            let item = &mut block.items[k - 1];
            let raw = f.frame.trim();
            if raw.is_empty() || raw.eq_ignore_ascii_case("(none)") {
                item.frame.clear();
            } else if !frame_format(item.answer_format.as_deref()) {
                problems.push(format!(
                    "第 {k} 題的作答方式是「{}」，只有簡答題和段落寫作可以有句型框架",
                    item.answer_format.as_deref().unwrap_or("未指定")
                ));
            } else {
                match item_frame(raw, &item.answer) {
                    Ok(text) => item.frame = text,
                    Err(why) => problems.push(format!("第 {k} 題的句型框架：{why}")),
                }
            }
        }
        for h in a.hints.iter().flatten() {
            let Some(k) = usize::try_from(h.item_no)
                .ok()
                .filter(|k| (1..=block.items.len()).contains(k))
            else {
                problems.push(format!("沒有第 {} 題", h.item_no));
                continue;
            };
            let item = &mut block.items[k - 1];
            if h.hint.trim().is_empty() {
                item.hint.clear();
            } else {
                match item_hint(&h.hint, &item.answer) {
                    Ok(text) => item.hint = text,
                    Err(why) => problems.push(format!("第 {k} 題的提示：{why}")),
                }
            }
        }
        if let Some(bank) = &a.word_bank {
            self.word_bank(&mut block, bank, &tag, &mut problems, &mut notes);
        }
        if !problems.is_empty() {
            return ToolResult::fail("check_failed", &problems, Some(a.block_id), step);
        }
        if block == self.content.blocks[pos] {
            return ToolResult::ok("鷹架和現在一樣，沒有任何變動。", Some(a.block_id), step);
        }
        let before = self.content.clone();
        self.content.blocks[pos] = block;
        if self.lexicon.is_some() {
            key_vocab::bank_pos_from_list(&mut self.content, self.lexicon.as_ref());
        }
        if let Err(r) = self.validated(before, step.clone(), &a.block_id) {
            return r;
        }
        let mut text = format!("已修改 {} 的鷹架。", self.name(&a.block_id));
        for n in notes {
            text.push_str(&format!("\n注意：{n}"));
        }
        ToolResult::ok(text, Some(a.block_id), step)
    }

    fn word_bank(
        &self,
        block: &mut Block,
        bank: &BankArgs,
        tag: &str,
        problems: &mut Vec<String>,
        notes: &mut Vec<String>,
    ) {
        let mut words: Vec<String> = Vec::new();
        for w in &bank.words {
            let w = w.trim().to_string();
            if !valid_word(&w) {
                problems.push(format!(
                    "字詞表的字「{}」要是英文字或片語",
                    clip_chars(&w, 30)
                ));
            } else if !words.iter().any(|x| x.eq_ignore_ascii_case(&w)) {
                words.push(w);
            }
        }
        if words.is_empty() || words.len() > MAX_BANK_WORDS {
            problems.push(format!("Word bank 要有 1 到 {MAX_BANK_WORDS} 個字"));
        }
        if !matches!(bank.lang.as_str(), "en" | "zh") {
            problems.push("lang 只能是 en 或 zh".to_string());
        }
        if !problems.is_empty() {
            return;
        }
        let at = block.scaffolds.iter().position(|s| s.is_word_bank());
        let at = at.unwrap_or_else(|| {
            block.scaffolds.push(Scaffold {
                id: format!("{tag}s2"),
                kind_type: ScaffoldType::Verbal,
                kind: KIND_WORD_LIST.to_string(),
                levels: self.levels.clone(),
                origin: Origin::Ai,
                ..Scaffold::default()
            });
            block.scaffolds.len() - 1
        });
        let s = &mut block.scaffolds[at];
        s.set_text(&words.join(", "));
        s.origin = Origin::Ai;
        let Some(b) = s.bank.as_mut() else {
            problems.push("這些字沒辦法放進 Word bank".to_string());
            return;
        };
        b.lang = if bank.lang == "zh" {
            VocabLang::Zh
        } else {
            VocabLang::En
        };
        let mut without_meaning = Vec::new();
        for w in &mut b.words {
            let wanted = bank
                .show_meaning
                .iter()
                .any(|x| x.trim().eq_ignore_ascii_case(&w.word));
            let has = !w.definition.trim().is_empty() || !w.meaning_zh.trim().is_empty();
            if wanted && !has {
                without_meaning.push(w.word.clone());
            }
            w.show_meaning = wanted && has;
        }
        if !without_meaning.is_empty() {
            notes.push(format!(
                "這些字還沒有字義，所以沒有勾選印出字義：{}（老師可以在字詞表上自己填）",
                without_meaning.join("、")
            ));
        }
    }

    // ------------------------------------------------------------ edit_key_vocabulary

    fn vocabulary(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: VocabArgs = match self.parse(arguments, T_VOCAB, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let target = match a
            .block_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(id) => self
                .content
                .blocks
                .iter()
                .position(|b| b.id == id && b.enabled),
            None => self
                .content
                .blocks
                .iter()
                .position(|b| b.enabled && !b.vocabulary.is_empty()),
        };
        let Some(pos) = target else {
            return ToolResult::fail(
                "unknown_block",
                &["找不到有 Key Vocabulary 的那一區；請用 block_id 指定一區。".to_string()],
                a.block_id.clone(),
                step,
            );
        };
        let mut block = self.content.blocks[pos].clone();
        let block_id = block.id.clone();
        let mut problems: Vec<String> = Vec::new();
        for w in &a.remove {
            let w = w.trim();
            match block
                .vocabulary
                .iter()
                .position(|v| v.word.trim().eq_ignore_ascii_case(w))
            {
                Some(i) if block.vocabulary[i].task_word => {
                    problems.push(format!("「{w}」是指示用語，由平台固定，不能拿掉"))
                }
                Some(i) => {
                    block.vocabulary.remove(i);
                }
                None => problems.push(format!("Key Vocabulary 裡沒有「{w}」")),
            }
        }
        let corpus = format!(
            "{}\n{}",
            self.passage,
            block
                .items
                .iter()
                .map(|i| i.prompt.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        for w in &a.add {
            let word = w.word.trim().to_string();
            let name = format!("字「{}」", clip_chars(&word, 30));
            let definition = crate::prep::clean_text(&w.definition, MEANING_CHARS)
                .trim()
                .to_string();
            let zh = crate::prep::clean_text(&w.meaning_zh, 60)
                .trim()
                .to_string();
            let listed = self
                .lexicon
                .as_ref()
                .map(|lex| key_vocab::listed_pos_of(lex, &word))
                .unwrap_or_default();
            if !valid_word(&word) {
                problems.push(format!("{name}要是英文字或片語"));
            } else if block
                .vocabulary
                .iter()
                .any(|v| v.word.trim().eq_ignore_ascii_case(&word))
            {
                problems.push(format!("{name}已經在 Key Vocabulary 裡了"));
            } else if !occurs_in(&word, &corpus) {
                problems.push(format!(
                    "{name}沒有出現在教材或這一區裡，只能列出教材裡真的有的字"
                ));
            } else if definition.is_empty() || key_vocab::has_han(&definition) {
                problems.push(format!("{name}的 definition 要是簡單的英文解釋"));
            } else if !zh.is_empty() && !key_vocab::has_han(&zh) {
                problems.push(format!("{name}的 meaning_zh 要是繁體中文（或留空字串）"));
            } else if !w.pos.trim().is_empty() && key_vocab::normalise_pos(&w.pos).is_none() {
                problems.push(format!(
                    "{name}的 pos 只能是：{}（或留空字串）",
                    key_vocab::POS_SET.join(" ")
                ));
            } else {
                let at = self.passage_paragraph(&word).or_else(|| {
                    self.ws
                        .material
                        .paragraphs
                        .iter()
                        .find(|p| occurs_in(&word, &p.text))
                        .map(|p| p.id.clone())
                });
                let context = self
                    .ws
                    .material
                    .paragraphs
                    .iter()
                    .find(|p| occurs_in(&word, &p.text))
                    .map(|p| {
                        p.text
                            .split_inclusive(['.', '?', '!', '。'])
                            .find(|s| occurs_in(&word, s))
                            .map(|s| clip_chars(s.trim(), 300))
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                block.vocabulary.push(crate::worksheet::Vocab {
                    level: self.lexicon.as_ref().and_then(|lex| {
                        crate::vocabulary::word_level(lex, &word.to_lowercase()).level
                    }),
                    context,
                    paragraph_id: at,
                    pos: key_vocab::resolve_pos(&w.pos, &listed),
                    meaning_zh: zh,
                    word,
                    definition,
                    task_word: false,
                });
            }
        }
        if block.vocabulary.len() > crate::worksheet::MAX_LIST {
            problems.push("Key Vocabulary 的字太多了".to_string());
        }
        let language = match a.language.as_deref().map(str::trim) {
            None | Some("") => None,
            Some("en") => Some(VocabLang::En),
            Some("zh") => Some(VocabLang::Zh),
            Some(_) => {
                problems.push("language 只能是 en 或 zh".to_string());
                None
            }
        };
        if !problems.is_empty() {
            return ToolResult::fail("check_failed", &problems, Some(block_id), step);
        }
        if block.vocabulary.is_empty() && !self.content.blocks[pos].vocabulary.is_empty() {
            // Taking every row out keeps them, so 「恢復字詞表」 can bring the table back.
            block.vocabulary_removed = self.content.blocks[pos].vocabulary.clone();
        }
        let unchanged = block == self.content.blocks[pos]
            && language.is_none_or(|l| l == self.content.vocabulary_language);
        if unchanged {
            return ToolResult::ok(
                "Key Vocabulary 和現在一樣，沒有任何變動。",
                Some(block_id),
                step,
            );
        }
        let before = self.content.clone();
        self.content.blocks[pos] = block;
        if let Some(l) = language {
            self.content.vocabulary_language = l;
        }
        if let Err(r) = self.validated(before, step.clone(), &block_id) {
            return r;
        }
        let mut text = "已修改 Key Vocabulary。".to_string();
        if matches!(language, Some(VocabLang::Zh)) {
            let missing = key_vocab::missing_zh(&self.content);
            if !missing.is_empty() {
                text.push_str(&format!(
                    "\n注意：有 {} 個字還沒有中文字義，會先印英文字義。",
                    missing.len()
                ));
            }
        }
        ToolResult::ok(text, Some(block_id), step)
    }

    fn passage_paragraph(&self, word: &str) -> Option<String> {
        self.ws
            .material
            .paragraphs
            .iter()
            .find(|p| occurs_in(word, &p.text))
            .map(|p| p.id.clone())
    }

    // ------------------------------------------------------------ redraw_picture

    fn redraw(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: RedrawArgs = match self.parse(arguments, T_REDRAW, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let fail = |code: &'static str, why: String, step: String| {
            ToolResult::fail(code, &[why], None, step)
        };
        if !self.pictures_on {
            return fail("unavailable", images::MSG_DISABLED.to_string(), step);
        }
        if !self.storage_on {
            return fail(
                "unavailable",
                "這個環境沒有設定檔案儲存，沒辦法重畫圖。".into(),
                step,
            );
        }
        let Some((block, _)) =
            images::find_slot(&self.content, &a.slot_id).filter(|(b, _)| b.enabled)
        else {
            let ids = images::actions::slot_ids(&self.content, None);
            return fail(
                "unknown_block",
                format!(
                    "slot_id「{}」不在這份學習單裡。可用的 slot_id：{}",
                    clip_chars(&a.slot_id, 40),
                    if ids.is_empty() {
                        "（沒有圖）".to_string()
                    } else {
                        ids.join("、")
                    }
                ),
                step,
            );
        };
        let block_id = block.id.clone();
        if images::find_slot(&self.original, &a.slot_id).is_none() {
            return fail(
                "check_failed",
                "這張圖是這一輪才加的，還沒有畫過，不能重畫。".into(),
                step,
            );
        }
        if self.redraws.iter().any(|r| r.slot_id == a.slot_id) {
            return fail("check_failed", "這張圖這一輪已經要重畫了。".into(), step);
        }
        if let Some(st) = self.states.iter().find(|s| s.slot_id == a.slot_id) {
            if matches!(st.status.as_str(), "queued" | "running") {
                return fail(
                    "check_failed",
                    "這張圖正在繪製中，等畫好再重畫。".into(),
                    step,
                );
            }
        }
        if self.regenerations_left - (self.redraws.len() as i64) <= 0 {
            return fail(
                "check_failed",
                "這份學習單的圖已經重畫到上限了；老師可以上傳自己的圖。".into(),
                step,
            );
        }
        let hint = match images::checked_hint(Some(&a.note)) {
            Ok(h) => h,
            Err(e) => {
                let why = match e {
                    crate::error::AppError::BadRequest(m) => m,
                    other => other.to_string(),
                };
                return fail("check_failed", why, step);
            }
        };
        self.redraws.push(Redraw {
            slot_id: a.slot_id.clone(),
            block_id: block_id.clone(),
            note: hint,
        });
        ToolResult::ok(
            format!(
                "已記下：{} 的圖會在整輪結束、新版本寫好之後重畫。",
                self.name(&block_id)
            ),
            Some(block_id),
            step,
        )
    }

    // ------------------------------------------------------------ add / remove / move

    fn add_block(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: AddBlockArgs = match self.parse(arguments, T_ADD, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        if self.content.blocks.len() >= MAX_BLOCKS {
            return ToolResult::fail(
                "check_failed",
                &[format!("一份學習單最多 {MAX_BLOCKS} 個區塊。")],
                None,
                step,
            );
        }
        let after = match a
            .after_block_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(id) => match self.content.blocks.iter().position(|b| b.id == id) {
                Some(p) => p + 1,
                None => return self.unknown_block(id, step),
            },
            None => 0,
        };
        self.new_block_no += 1;
        let mut id = format!("c{}b{}", self.turn_id, self.new_block_no);
        while self.content.blocks.iter().any(|b| b.id == id) {
            self.new_block_no += 1;
            id = format!("c{}b{}", self.turn_id, self.new_block_no);
        }
        let old = Block {
            id: id.clone(),
            kind: BlockKind::Activity,
            enabled: true,
            origin: Origin::Ai,
            ..Block::default()
        };
        let change = RawChange {
            block_id: id.clone(),
            title: a.title,
            instructions: a.instructions,
            items: a.items,
        };
        let mut block = match self.apply_section(&old, change, self.new_block_no) {
            Ok(b) => b,
            Err(reasons) => return ToolResult::fail("check_failed", &reasons, None, step),
        };
        block.origin = Origin::Ai;
        let before = self.content.clone();
        self.content.blocks.insert(after, block);
        if let Err(r) = self.validated(before, step.clone(), &id) {
            return r;
        }
        self.added.push(id.clone());
        ToolResult::ok(
            format!("已新增 {}（block_id {id}）。", self.name(&id)),
            Some(id),
            step,
        )
    }

    fn remove_block(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: RemoveArgs = match self.parse(arguments, T_REMOVE, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let Some(pos) = self
            .content
            .blocks
            .iter()
            .position(|b| b.id == a.block_id && b.enabled)
        else {
            return self.unknown_block(&a.block_id, step);
        };
        if self.content.blocks.iter().filter(|b| b.enabled).count() <= 1 {
            return ToolResult::fail(
                "check_failed",
                &["學習單至少要留一個區塊。".to_string()],
                Some(a.block_id),
                step,
            );
        }
        let name = self.name(&a.block_id);
        self.content.blocks[pos].enabled = false;
        self.refresh_labels();
        ToolResult::ok(
            format!("已關閉 {name}（沒有刪除，老師可以復原）。"),
            Some(a.block_id),
            step,
        )
    }

    fn move_block(&mut self, arguments: &str, step: String) -> ToolResult {
        let a: MoveArgs = match self.parse(arguments, T_MOVE, &step) {
            Ok(a) => a,
            Err(r) => return r,
        };
        let Some(from) = self.content.blocks.iter().position(|b| b.id == a.block_id) else {
            return self.unknown_block(&a.block_id, step);
        };
        let after = a
            .after_block_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if after == Some(a.block_id.as_str()) {
            return ToolResult::fail(
                "check_failed",
                &["不能把區塊移到自己後面。".to_string()],
                Some(a.block_id),
                step,
            );
        }
        let block = self.content.blocks.remove(from);
        let to = match after {
            Some(id) => match self.content.blocks.iter().position(|b| b.id == id) {
                Some(p) => p + 1,
                None => {
                    self.content.blocks.insert(from, block);
                    return self.unknown_block(id, step);
                }
            },
            None => 0,
        };
        self.content.blocks.insert(to, block);
        if to == from {
            return ToolResult::ok("順序和現在一樣，沒有任何變動。", Some(a.block_id), step);
        }
        self.refresh_labels();
        self.moved.push(a.block_id.clone());
        ToolResult::ok(
            format!(
                "已調整 {} 的順序（區塊的字母會跟著重新排）。",
                self.name(&a.block_id)
            ),
            Some(a.block_id),
            step,
        )
    }
}

/// The ids of every block whose content differs between the two (added blocks included).
pub fn changed_block_ids(before: &Content, after: &Content) -> HashSet<String> {
    after
        .blocks
        .iter()
        .filter(|b| before.block(&b.id) != Some(*b))
        .map(|b| b.id.clone())
        .collect()
}
