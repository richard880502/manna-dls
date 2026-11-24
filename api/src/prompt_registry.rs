//! The registry of AI prompts: for every `prompt_key` its Chinese name, group, a one-line
//! description (when it runs, which flow uses it) and the `{{variables}}` the code fills in.
//! The single source of truth for the 後台管理 → 提示詞版本 page, and for the check a new version
//! goes through before it is saved (a prompt that lost a variable the code cannot do without is
//! refused; a `{{name}}` the code never fills is only warned about).
//!
//! The variables were read from the code that renders each prompt (`prompts::render` /
//! `render_user_with` and the `HashMap` of values next to it); a note on each entry names that
//! code. Keep this file in step when a call site gets a new variable.
//!
//! * `required`: the code always fills it and the output cannot be right without it (the
//!   material, the instruction, the list the model must choose from). A new version without it
//!   in `system_text` + `user_template` is refused.
//! * `auto`: the code appends the value (with a Chinese label) to the end of the message when
//!   the prompt has no placeholder for it (`prompts::render_user_with`), so a prompt may drop it
//!   safely. Never blocking.
//! * neither: optional context; leaving it out only makes the prompt poorer.

use serde::Serialize;

use crate::prompts::placeholders;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct VarSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub auto: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PromptInfo {
    pub key: &'static str,
    pub name_zh: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    /// False when the code reads only `system_text` (`worksheet_image_slots`): the user template
    /// may then stay empty.
    pub uses_user_template: bool,
    pub vars: &'static [VarSpec],
}

const fn opt(name: &'static str, label: &'static str) -> VarSpec {
    VarSpec {
        name,
        label,
        required: false,
        auto: false,
    }
}
const fn req(name: &'static str, label: &'static str) -> VarSpec {
    VarSpec {
        name,
        label,
        required: true,
        auto: false,
    }
}
const fn auto(name: &'static str, label: &'static str) -> VarSpec {
    VarSpec {
        name,
        label,
        required: false,
        auto: true,
    }
}

pub const G_WORKSHEET: &str = "學習單產生";
pub const G_TEMPLATE: &str = "照我的學習單做";
pub const G_REVISE: &str = "逐區與對話修改";
pub const G_PREP: &str = "備課對話";
pub const G_OBJECTIVES: &str = "目標與 SIOP";
pub const G_SOP: &str = "逐步設計";
pub const G_IMAGES: &str = "圖片";
pub const G_VOCAB: &str = "詞彙";
pub const G_LEGACY: &str = "單元與課例（舊流程）";

/// Display order of the groups.
pub const GROUPS: [&str; 9] = [
    G_WORKSHEET,
    G_TEMPLATE,
    G_REVISE,
    G_PREP,
    G_SOP,
    G_OBJECTIVES,
    G_IMAGES,
    G_VOCAB,
    G_LEGACY,
];

// Shared pieces. `worksheet/ai.rs` `Run::common_vars` fills these seven for the four steps of a
// worksheet generation (tasks, warmup, scaffolds, exit ticket).
#[cfg(test)]
const WS_COMMON: [VarSpec; 7] = [
    opt("school_track", "學校類型（普高／技高）"),
    auto("class_profile", "班級特性"),
    req("objectives_summary", "學習目標（含代號）"),
    req("passage_numbered", "教材（每段有編號）"),
    opt("student_task_labels", "學生任務的可選種類"),
    opt("answer_format_labels", "作答方式的可選種類"),
    auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
];

macro_rules! vars {
    ($($v:expr),* $(,)?) => { &[$($v),*] };
}

// The bloom / tiered level prompts share one call (`worksheet/ai/bloom.rs` `level_call`).
const BLOOM_VARS: &[VarSpec] = vars![
    opt("school_track", "學校類型"),
    auto("class_profile", "班級特性"),
    req("objectives", "學習目標（含代號）"),
    opt("teacher_request", "老師的要求"),
    auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
    opt("bloom_id", "層次代號（remember…）"),
    opt("bloom_label", "層次名稱"),
    opt("bloom_definition", "層次的定義"),
    opt("bloom_stems", "常用的題幹"),
    req("answer_formats", "可用的作答方式"),
    req("question_count", "這個層次要幾題"),
    opt("purpose", "學習單的用途"),
    req("evidence_rule", "原文依據的規則（程式會核對）"),
    opt("earlier_questions", "已經出過的題目（避免重複）"),
    req("passage_numbered", "教材（每段有編號）"),
    opt("redo_note", "重做時的提醒（第一次是空的）"),
    auto("teacher_content_objective", "老師寫的內容目標"),
    auto("teacher_language_objective", "老師寫的語言目標"),
    opt(
        "teacher_objective",
        "老師寫的目標（內容與語言併成一行，舊版用）"
    ),
];

const CORE_VARS: &[VarSpec] = vars![
    opt("unit_title", "單元名稱"),
    opt("school_track", "學校類型"),
    opt("core_preamble", "CoRe 八個問題的說明"),
    opt("big_idea", "這個學習重點"),
    opt("question_id", "問題代號"),
    req("question_prompt", "CoRe 問題"),
    opt("question_help", "問題的補充說明"),
    req("teacher_answer", "老師目前寫的回答"),
    opt("other_answers", "老師在其他問題的回答"),
    opt("level", "協助程度（1–3）"),
    auto("class_profile", "班級特性"),
    auto("detail_instruction", "回覆長度"),
    auto("focus_emphasis", "這次請特別強調（有勾選才附上）"),
];

pub static REGISTRY: &[PromptInfo] = &[
    PromptInfo {
        key: "teacher_memory_extract",
        name_zh: "整理教師長期出題習慣",
        group: G_PREP,
        description: "判斷老師打的一段話（我的出題習慣頁，或備課對話、逐區修改、學習單要求）是否為一項長期習慣：封閉代碼或一句自由習慣（含適用範圍），並判斷是否為單次要求（一項長期習慣夾帶單次修改時只整理那一項習慣）、是否含個人資訊（會拒絕）及要取代的版本；必須由老師按「記住」確認後才記住。原始文字不保留，只存字數與雜湊。",
        uses_user_template: true,
        vars: &[],
    },
    PromptInfo {
        key: "teacher_memory_context",
        name_zh: "帶入已確認的出題習慣",
        group: G_PREP,
        description: "主持人開啟教師長期記憶後，老師有已確認的出題習慣時，system_text 附在出題與備課產生步驟（學習單四步、Bloom／差異化層次、照範本做、逐區修改、備課對話、逐步設計第 5–7 步）的系統提示詞後面，user_template 帶入習慣資料（封閉類型一律帶入；自由習慣依適用範圍篩選，過多時依與本次要求的相關度取前 12 項）。本次要求優先。評審、讀範本、詞彙與目標步驟不會帶入。",
        uses_user_template: true,
        vars: vars![req("teacher_habits", "老師確認的出題習慣（JSON，依確認時間排序）")],
    },
    // ------------------------------------------------------------ 學習單產生
    PromptInfo {
        key: "worksheet_tasks",
        name_zh: "學習任務與題目（一般學習單）",
        group: G_WORKSHEET,
        description: "一般學習單產生的第 1 步：依教材分段設計各區的任務與題目；單獨重做某一區時也用這個。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("material_types", "老師勾選的教材類型"),
            opt("minutes", "預計完成時間"),
            req("item_budget", "題數上限"),
            opt("selected_student_tasks", "老師勾選的學生任務"),
            opt("selected_answer_formats", "老師勾選的作答方式"),
            opt("teacher_tasks", "老師自己寫的任務"),
            req("only_block", "只重做某一區的說明（整份重做時是空的）"),
            auto("image_slot_rules", "圖片位置的規則（開啟圖片時才附上）"),
        ],
    },
    PromptInfo {
        key: "worksheet_warmup",
        name_zh: "暖身活動（一般學習單）",
        group: G_WORKSHEET,
        description: "一般學習單產生的第 2 步：設計暖身（喚醒背景知識、關鍵字彙）。「照我的學習單做」不會用這一個，它用「照我的學習單做：每一區的題目」。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("vocab_count", "關鍵字彙的數量範圍"),
            opt("teacher_warmup", "老師自己寫的暖身"),
            opt("teacher_vocabulary", "老師指定的字彙"),
            opt("tasks_summary", "第 1 步已經寫好的任務摘要"),
            req("vocab_candidates", "程式從教材挑出的候選字彙"),
        ],
    },
    PromptInfo {
        key: "worksheet_scaffolds",
        name_zh: "學習支持／鷹架（一般學習單）",
        group: G_WORKSHEET,
        description: "一般學習單產生的第 3 步：為每一區設計語言、教學、程序三種鷹架。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("verbal_kinds", "語言鷹架的種類"),
            opt("instructional_kinds", "教學鷹架的種類"),
            opt("procedural_kinds", "程序鷹架的種類"),
            opt("selected_supports", "老師勾選的支持"),
            req("blocks_detail", "每一區目前的內容"),
        ],
    },
    PromptInfo {
        key: "worksheet_exit_ticket",
        name_zh: "出場票（一般學習單）",
        group: G_WORKSHEET,
        description: "一般學習單產生的第 4 步：在學習單最後設計出場票（形成性評量、反思）。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            req("exit_count", "出場票題數"),
            opt("exit_minutes", "作答分鐘數"),
            opt("teacher_exit", "老師自己寫的出場票"),
            req("tasks_summary", "已經寫好的任務摘要"),
        ],
    },
    PromptInfo {
        key: "worksheet_bloom_level",
        name_zh: "Bloom 分層學習單：每個層次的題目",
        group: G_WORKSHEET,
        description: "做 Bloom 認知層次學習單時，每個層次各呼叫一次，寫出該層次的題目。",
        uses_user_template: true,
        vars: BLOOM_VARS,
    },
    PromptInfo {
        key: "worksheet_tiered_level",
        name_zh: "差異化（分層）學習單：每個層次的題目",
        group: G_WORKSHEET,
        description: "做基礎／一般／進階三版的差異化學習單時，每個 Bloom 層次各呼叫一次，寫出共用的題目與各版的語言支持。",
        uses_user_template: true,
        vars: BLOOM_VARS,
    },
    PromptInfo {
        key: "worksheet_bloom_check",
        name_zh: "Bloom 層次檢查（評審）",
        group: G_WORKSHEET,
        description: "Bloom 與差異化學習單寫好題目後，由另一次呼叫獨立判斷每題實際考的是哪個層次。",
        uses_user_template: true,
        vars: vars![
            req("passage_numbered", "教材（每段有編號）"),
            opt("question_count", "題數"),
            req("questions", "要判斷的題目"),
        ],
    },
    // ------------------------------------------------------------ 照我的學習單做
    PromptInfo {
        key: "worksheet_template_outline",
        name_zh: "讀懂範本：版面大綱",
        group: G_TEMPLATE,
        description: "老師上傳自己的學習單當範本時，第 1 步：列出大綱與表頭。程式只用 system_text 與 user_template 本身，不會自動補上缺的變數。",
        uses_user_template: true,
        vars: vars![req("worksheet_text", "從檔案抽出的學習單文字")],
    },
    PromptInfo {
        key: "worksheet_template_details",
        name_zh: "讀懂範本：每一區的設計細節",
        group: G_TEMPLATE,
        description: "老師上傳自己的學習單當範本時，第 2 步：依大綱抽出每一區的設計（作答方式、題數、指示語）。",
        uses_user_template: true,
        vars: vars![
            req("worksheet_text", "從檔案抽出的學習單文字"),
            req("outline_count", "大綱的區數"),
            req("outline_lines", "第 1 步的大綱"),
        ],
    },
    PromptInfo {
        key: "worksheet_template_section",
        name_zh: "照我的學習單做：每一區的題目",
        group: G_TEMPLATE,
        description: "用範本為新教材產生學習單時，每一區（或每一段）各呼叫一次。「照我的學習單做」只用這一個，不會用暖身、鷹架、出場票的提示詞。程式只用 system_text 與 user_template 本身，不會自動補上缺的變數。",
        uses_user_template: true,
        vars: vars![
            req("template_head", "範本的整體設計"),
            req("target_section", "這一次要做的區塊設計"),
            opt("block_no", "第幾區"),
            opt("block_total", "共幾區"),
            opt("paragraph_note", "這一區對應的段落說明"),
            opt("done_titles", "前面已完成的區塊標題"),
            opt("material_title", "教材標題"),
            opt("paragraph_count", "教材段數"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("class_profile", "班級特性"),
            opt("teacher_notes_for_ai", "老師給 AI 的教材註記"),
            opt("image_slot_rules", "圖片位置的規則（開啟圖片時才有）"),
        ],
    },
    // ------------------------------------------------------------ 逐區與對話修改
    PromptInfo {
        key: "worksheet_block_revise",
        name_zh: "逐區修改：改一個區塊",
        group: G_REVISE,
        description: "老師在學習單上指定一個區塊、寫下註記或指示請 AI 修改時使用。可以改題目，也可以改、刪或新增這一區的鷹架（程序鷹架、句型框架…）、字詞表和 Glossary；它們都列在「這一區目前的內容」裡，各有代號。",
        uses_user_template: true,
        vars: vars![
            auto("instruction", "老師的修改指示"),
            auto("block_notes", "老師在這一區的註記"),
            auto("scope", "可以修改的範圍"),
            opt("round_note", "第幾次修改的提醒"),
            opt("block_name", "區塊名稱"),
            auto("block_detail", "這一區目前的內容（含每一題、鷹架、字詞表、Glossary，各有代號）"),
            opt("style_note", "風格提醒"),
            opt("other_blocks", "其他區塊的概況"),
            opt("objectives_summary", "學習目標"),
            auto("class_profile", "班級特性"),
            opt("teacher_request", "老師建立時的要求"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("material_title", "教材標題"),
            opt("paragraph_count", "教材段數"),
            req("passage_numbered", "教材（每段有編號）"),
            req("answer_format_labels", "作答方式的可選種類"),
            req("picture_rules", "圖片的規則（沒開圖片時是「不要畫圖」）"),
            opt("pictures_note", "這一區目前的圖片"),
            opt("attachments_note", "老師附上的圖片說明"),
            auto("teacher_content_objective", "老師寫的內容目標"),
            auto("teacher_language_objective", "老師寫的語言目標"),
            opt("teacher_objective", "老師寫的目標（併成一行，舊版用）"),
        ],
    },
    PromptInfo {
        key: "prep_revise",
        name_zh: "對話修改整份學習單",
        group: G_REVISE,
        description: "在「建立」的對話裡，老師用文字要求修改已產生的整份學習單時使用。",
        uses_user_template: true,
        vars: vars![
            auto("instruction", "老師的修改指示"),
            opt("worksheet_title", "學習單標題"),
            auto("blocks_json", "學習單目前的區塊"),
            req("picture_rules", "圖片的規則（沒開圖片時是「不要畫圖」）"),
            opt("pictures_note", "目前的圖片"),
            opt("attachments_note", "老師附上的圖片說明"),
            opt("material_title", "教材標題"),
            opt("paragraph_count", "教材段數"),
            req("passage_numbered", "教材（每段有編號）"),
            req("answer_format_labels", "作答方式的可選種類"),
        ],
    },
    // ------------------------------------------------------------ 備課對話
    PromptInfo {
        key: "prep_activity",
        name_zh: "備課對話：規劃一節課的教學活動",
        group: G_PREP,
        description: "在「建立」的備課對話裡，老師要 AI 依教材規劃一節課（約 50 分鐘）的教學活動時使用。",
        uses_user_template: true,
        vars: vars![
            auto("class_profile", "班級特性"),
            auto("teacher_request", "老師的要求"),
            auto("curriculum_items", "可對應的課綱條目（只能從這裡選代碼）"),
            opt("material_title", "教材標題"),
            opt("paragraph_count", "教材段數"),
            req("passage_numbered", "教材（每段有編號）"),
            auto("teacher_content_objective", "老師寫的內容目標"),
            auto("teacher_language_objective", "老師寫的語言目標"),
            opt("teacher_objective", "老師寫的目標（併成一行，舊版用）"),
        ],
    },
    PromptInfo {
        key: "prep_ask",
        name_zh: "備課對話：回答老師的問題",
        group: G_PREP,
        description: "老師在備課對話裡直接問問題（沒有要產生東西）時使用。",
        uses_user_template: true,
        vars: vars![
            auto("question", "老師的問題"),
            opt("recent_turns", "先前的對話"),
            auto("class_profile", "班級特性"),
            req("curriculum_items", "可引用的課綱條目"),
            auto("passage_numbered", "教材（每段有編號）"),
        ],
    },
    // ------------------------------------------------------------ 目標與 SIOP
    PromptInfo {
        key: "objective_draft",
        name_zh: "草擬學習目標（內容與語言目標）",
        group: G_OBJECTIVES,
        description: "老師在「建立」頁按「請 Manna 擬一組」或不寫目標就產生時：依教材與 SIOP 先草擬內容目標；老師決定內容目標之後，再依它草擬語言目標（分兩次呼叫）。",
        uses_user_template: true,
        vars: vars![
            auto("purpose", "這一課要做什麼"),
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            auto("teacher_request", "老師的要求"),
            opt("recent_categories", "老師最近用過的目標類別"),
            opt("variety_note", "多樣性提醒"),
            opt("avoid_note", "要避開的目標"),
            req("curriculum_items", "可對應的課綱條目"),
            opt("material_title", "教材標題"),
            opt("paragraph_count", "教材段數"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("redo_note", "重做時的提醒（第一次是空的）"),
            opt("stage_note", "這次要寫哪一組目標（內容目標先寫；語言目標在教師決定內容目標之後，依它寫）"),
        ],
    },
    PromptInfo {
        key: "objective_check",
        name_zh: "檢查學習目標草稿（評審）",
        group: G_OBJECTIVES,
        description: "草擬目標之後，由另一次呼叫逐項判斷每個目標能不能交給老師。只判斷，不改寫。",
        uses_user_template: true,
        vars: vars![
            auto("class_profile", "班級特性"),
            auto("purpose", "這一課要做什麼"),
            opt("objective_count", "目標數量"),
            auto("objectives", "要檢查的目標"),
            req("curriculum_items", "可對應的課綱條目"),
            opt("material_title", "教材標題"),
            req("passage_numbered", "教材（每段有編號）"),
        ],
    },
    PromptInfo {
        key: "siop_selfcheck",
        name_zh: "SIOP 自我檢核",
        group: G_OBJECTIVES,
        description: "學習單、課堂活動或教學套件做好後，依 SIOP 觀察量表逐項檢核這份產出（給老師參考，不打分數）。",
        uses_user_template: true,
        vars: vars![
            opt("output_kind", "產出的種類"),
            auto("class_profile", "班級特性"),
            opt("passage_numbered", "教材（每段有編號）"),
            req("output_text", "要檢核的產出"),
            opt("item_count", "檢核項目數"),
            req("items", "SIOP 檢核項目"),
        ],
    },
    // ------------------------------------------------------------ 圖片
    PromptInfo {
        key: "image_style",
        name_zh: "學習單插圖的畫風",
        group: G_IMAGES,
        description: "畫學習單圖片時：system_text 是整體畫風，user_template 帶入這張圖要畫什麼。",
        uses_user_template: true,
        vars: vars![
            req("description", "這張圖要畫的內容"),
            opt("teacher_note", "老師對這張圖的補充（沒有就是空的）"),
        ],
    },
    PromptInfo {
        key: "worksheet_image_slots",
        name_zh: "圖片位置與畫面描述的規則",
        group: G_IMAGES,
        description: "開啟圖片時，這段規則（只用 system_text）會原文附在產生、修改學習單的其他提示詞後面，告訴 AI 什麼時候該放圖、畫面描述怎麼寫。user_template 不會被使用，可以留空。",
        uses_user_template: false,
        vars: vars![],
    },
    // ------------------------------------------------------------ 詞彙
    PromptInfo {
        key: "worksheet_vocab_gloss",
        name_zh: "超出級別的字：提供字義",
        group: G_VOCAB,
        description: "詞彙級別檢查後，老師選「提供字義」時：為每個超出級別的字寫出在學習單上下文裡的意思。",
        uses_user_template: true,
        vars: vars![
            opt("target_level", "目標級別"),
            opt("word_count", "字數"),
            auto("words", "要處理的字"),
            opt("sentence_count", "句子數"),
            opt("sentences", "字出現的句子"),
            opt("version_label", "學習單版本名稱"),
            opt("round_note", "第幾次建議的提醒"),
        ],
    },
    PromptInfo {
        key: "worksheet_vocab_replace",
        name_zh: "超出級別的字：換字（改寫句子）",
        group: G_VOCAB,
        description: "詞彙級別檢查後，老師選「換字」時：把含有超出級別的字的句子各改寫一次。",
        uses_user_template: true,
        vars: vars![
            auto("target_level", "目標級別"),
            opt("word_count", "字數"),
            auto("words", "要處理的字"),
            opt("sentence_count", "句子數"),
            auto("sentences", "要改寫的句子"),
            opt("version_label", "學習單版本名稱"),
            opt("round_note", "第幾次建議的提醒"),
        ],
    },
    // ------------------------------------------------------------ 單元與課例（舊流程）
    PromptInfo {
        key: "core_assist_l1",
        name_zh: "CoRe 備課協助：程度 1",
        group: G_LEGACY,
        description: "單元的 CoRe 八個問題頁，老師要 AI 協助某一題時，依協助程度 1 使用（舊流程）。",
        uses_user_template: true,
        vars: CORE_VARS,
    },
    PromptInfo {
        key: "core_assist_l2",
        name_zh: "CoRe 備課協助：程度 2",
        group: G_LEGACY,
        description: "單元的 CoRe 八個問題頁，老師要 AI 協助某一題時，依協助程度 2 使用（舊流程）。",
        uses_user_template: true,
        vars: CORE_VARS,
    },
    PromptInfo {
        key: "core_assist_l3",
        name_zh: "CoRe 備課協助：程度 3",
        group: G_LEGACY,
        description: "單元的 CoRe 八個問題頁，老師要 AI 協助某一題時，依協助程度 3 使用（舊流程）。",
        uses_user_template: true,
        vars: CORE_VARS,
    },
    PromptInfo {
        key: "material_focuses",
        name_zh: "從教材找候選學習重點",
        group: G_LEGACY,
        description: "單元頁老師貼上教材後，請 AI 提出三到五個候選的 CoRe 學習重點（舊流程）。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            opt("unit_title", "單元名稱"),
            auto("class_profile", "班級特性"),
            auto("detail_instruction", "回覆長度"),
            opt("material_filename", "教材檔名"),
            auto("material_text", "教材全文"),
        ],
    },
    PromptInfo {
        key: "plan_suggest",
        name_zh: "教案建議（單元與課例）",
        group: G_LEGACY,
        description: "課例頁按「給我建議」時：依目標、教案步驟、CoRe 學習重點與知識庫條目提出一到三項教學建議（舊流程）。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            opt("unit_title", "單元名稱"),
            opt("lesson_title", "課例名稱"),
            opt("class_description", "班級描述（舊版範本用的名字）"),
            auto("class_profile", "班級特性"),
            auto("my_example", "老師自己的範例（有才附上）"),
            auto("detail_instruction", "建議正文的長度"),
            auto("differentiation_instruction", "差異化欄位的要求（有勾選才附上）"),
            opt("objective", "這節課的目標"),
            req("steps", "教案步驟"),
            opt("core_focuses", "連結的 CoRe 學習重點"),
            opt("siop", "選用的 SIOP 提示"),
            opt("request_note", "老師這次的要求"),
            req("knowledge_items", "知識庫取回的條目"),
            req("allowed_codes", "可以引用的條目代碼"),
        ],
    },
    PromptInfo {
        key: "sop_passage_analysis",
        name_zh: "逐步設計第 2 步：分析教材的寫作特色",
        group: G_SOP,
        description: "逐步設計第 2 步：逐段分析教材的寫作特色。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("passage_numbered", "教材（每段有編號）"),
        ],
    },
    PromptInfo {
        key: "sop_objectives",
        name_zh: "逐步設計第 3 步：推薦內容目標與語言目標",
        group: G_SOP,
        description: "逐步設計第 3 步：依教材推薦內容目標，老師有了內容目標之後再依它推薦語言目標（分兩次請求），並連結課綱條目；「換一個」只要一項時也用這一個。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            opt("teacher_objectives", "老師已經寫的目標"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("analysis_summary", "教材分析的摘要"),
            auto("curriculum_items", "可連結的課綱條目（只能從這裡選）"),
        ],
    },
    PromptInfo {
        key: "sop_objective_english",
        name_zh: "逐步設計第 3 步：幫我修學生版目標的英文",
        group: G_SOP,
        description: "老師在目標表單按「幫我修英文」時：把給學生看的英文版目標修成「I can …」的一句話，或依中文目標寫出英文版。一次呼叫，直接回答，不是背景工作。",
        uses_user_template: true,
        vars: vars![
            opt("objective_type", "目標類型（內容目標或語言目標）"),
            req("objective_text", "中文目標"),
            opt("student_facing_en", "老師寫的英文版（可能是空的）"),
        ],
    },
    PromptInfo {
        key: "sop_paragraph_goals",
        name_zh: "逐步設計第 4 步：逐段整合目標",
        group: G_SOP,
        description: "逐步設計第 4 步：逐段建議這一段對應第 3 步的哪些目標，並說明要怎麼帶；「這一段再建議一次」只要一段時也用這一個。這個提示詞是新設計的，還沒有用真實模型驗證過。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("analysis_summary", "教材分析的摘要"),
            req("objectives_list", "教師確認的目標（代號｜類型｜內容）"),
            opt("teacher_goals", "教師目前寫的逐段對應"),
            opt("only_paragraphs", "這次只需要輸出的段落（整批時是空的）"),
        ],
    },
    PromptInfo {
        key: "sop_paragraph_tasks",
        name_zh: "逐步設計第 5 步：逐段問題或任務",
        group: G_SOP,
        description: "逐步設計第 5 步的第 1 次呼叫：逐段（或合併相鄰段落）設計任務、學生任務標籤與作答方式。沿用學習單「學習任務與題目」的規則與檢查（含老師確認的選項數習慣）。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("material_types", "老師勾選的教材類型"),
            opt("minutes", "預計完成時間"),
            req("item_budget", "題數上限"),
            opt("selected_student_tasks", "老師勾選的學生任務"),
            opt("selected_answer_formats", "老師勾選的作答方式"),
            opt("teacher_tasks", "老師自己寫的任務"),
            req("only_block", "只重做某一區的說明（逐步設計用不到，是空的）"),
            auto("image_slot_rules", "圖片位置的規則（逐步設計用不到）"),
        ],
    },
    PromptInfo {
        key: "sop_scaffolds",
        name_zh: "逐步設計第 5 步：語言與教學鷹架",
        group: G_SOP,
        description: "逐步設計第 5 步的第 2 次呼叫：為每一段的任務安排語言與教學鷹架。程序鷹架一律由程式依任務自動產生；這一次沒成功時，任務仍保留程式產生的程序鷹架。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("verbal_kinds", "語言鷹架的種類"),
            opt("instructional_kinds", "教學鷹架的種類"),
            opt("procedural_kinds", "程序鷹架的種類"),
            opt("selected_supports", "老師勾選的支持"),
            req("blocks_detail", "每一區目前的內容"),
        ],
    },
    PromptInfo {
        key: "sop_warmup",
        name_zh: "逐步設計第 6 步：暖身與關鍵字彙",
        group: G_SOP,
        description: "逐步設計第 6 步：設計喚醒背景知識的活動，並從程式依詞彙表挑出的候選字裡選關鍵字彙。字彙級別一律由程式帶入，模型不輸出級別。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            opt("vocab_count", "關鍵字彙的數量範圍"),
            opt("teacher_warmup", "老師自己寫的暖身"),
            opt("teacher_vocabulary", "老師指定的字彙"),
            opt("tasks_summary", "第 5 步已經寫好的任務摘要"),
            req("vocab_candidates", "程式從教材挑出的候選字彙"),
        ],
    },
    PromptInfo {
        key: "sop_exit_ticket",
        name_zh: "逐步設計第 7 步：Exit ticket",
        group: G_SOP,
        description: "逐步設計第 7 步：設計 2 到 4 題 Exit ticket；至少一題形成性評量連到第 3 步的目標並附參考答案，至少一題反思或蒐集問題，不重複主要任務。",
        uses_user_template: true,
        vars: vars![
            opt("school_track", "學校類型"),
            auto("class_profile", "班級特性"),
            req("objectives_summary", "學習目標（含代號）"),
            req("passage_numbered", "教材（每段有編號）"),
            opt("student_task_labels", "學生任務的可選種類"),
            opt("answer_format_labels", "作答方式的可選種類"),
            auto("teacher_notes_for_ai", "老師給 AI 的教材註記（有才附上）"),
            req("exit_count", "出場票題數"),
            opt("exit_minutes", "作答分鐘數"),
            opt("teacher_exit", "老師自己寫的出場票"),
            req("tasks_summary", "已經寫好的任務摘要"),
        ],
    },
];

pub fn find(key: &str) -> Option<&'static PromptInfo> {
    REGISTRY.iter().find(|p| p.key == key)
}

/// What a draft of a prompt text gets wrong against the registry.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VarCheck {
    /// Variables the code always fills and the prompt cannot do without, but the text lacks.
    pub missing: Vec<&'static VarSpec>,
    /// `{{names}}` in the text that the code never fills (they would reach the model as typed).
    pub unknown: Vec<String>,
}

/// Checks `system_text` + `user_template` of a new version of `key`. A key that is not in the
/// registry passes: there is nothing to compare with.
pub fn check_variables(key: &str, system_text: &str, user_template: &str) -> VarCheck {
    let Some(info) = find(key) else {
        return VarCheck::default();
    };
    let mut used = placeholders(system_text);
    for name in placeholders(user_template) {
        if !used.contains(&name) {
            used.push(name);
        }
    }
    let missing = info
        .vars
        .iter()
        .filter(|v| v.required && !used.iter().any(|u| u == v.name))
        .collect();
    let unknown = used
        .into_iter()
        .filter(|u| !info.vars.iter().any(|v| v.name == u))
        .collect();
    VarCheck { missing, unknown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_are_unique_and_groups_known() {
        let mut seen = HashSet::new();
        for p in REGISTRY {
            assert!(seen.insert(p.key), "duplicate key {}", p.key);
            assert!(GROUPS.contains(&p.group), "unknown group for {}", p.key);
            assert!(!p.name_zh.is_empty() && !p.description.is_empty());
            let mut names = HashSet::new();
            for v in p.vars {
                assert!(names.insert(v.name), "{}: duplicate var {}", p.key, v.name);
                assert!(!(v.required && v.auto), "{}: {} both", p.key, v.name);
            }
        }
    }

    #[test]
    fn worksheet_common_vars_match_the_four_steps() {
        for key in [
            "worksheet_tasks",
            "worksheet_warmup",
            "worksheet_scaffolds",
            "worksheet_exit_ticket",
        ] {
            let info = find(key).unwrap();
            for c in WS_COMMON {
                let found = info.vars.iter().find(|v| v.name == c.name);
                let found = found.unwrap_or_else(|| panic!("{key} lacks {}", c.name));
                assert_eq!((found.required, found.auto), (c.required, c.auto), "{key}");
            }
        }
    }

    #[test]
    fn missing_and_unknown_variables() {
        let ok = check_variables(
            "worksheet_bloom_check",
            "判斷 {{questions}}",
            "教材：{{ passage_numbered }}",
        );
        assert_eq!(ok, VarCheck::default());
        let bad = check_variables("worksheet_bloom_check", "沒有變數", "{{typo_var}}");
        let names: Vec<_> = bad.missing.iter().map(|v| v.name).collect();
        assert_eq!(names, ["passage_numbered", "questions"]);
        assert_eq!(bad.unknown, ["typo_var"]);
        // auto variables never block
        let auto_only = check_variables("prep_ask", "x {{curriculum_items}}", "y");
        assert!(auto_only.missing.is_empty());
        assert!(check_variables("some_new_key", "{{a}}", "{{b}}") == VarCheck::default());
    }
}
