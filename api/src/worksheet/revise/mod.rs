//! 逐區修改 (docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §4–§5; contract: docs/API.md「學習單：逐區修改與
//! 註記」): what `worksheet_block_revise` writes, how it is merged with the block, and the
//! proposals the teacher accepts or discards. The model call itself is in [`super::ai::revise`].
//!
//! The model sees one block and lists every question of it, marking each `keep` or not. The
//! server builds the result from the stored block: a kept question is the stored one, byte for
//! byte (same id, the teacher's edits, levels, hint); a changed one keeps everything the model
//! does not write (id, levels, hint, purpose). The model also sees what the block prints besides
//! its questions (the scaffolds, the word table, the Glossary) and may keep, change, remove or
//! add them, each by its code (`S1`, `W1`, `G1`): the stored one is kept unless the model lists it
//! (a word added to the table must be in the material, its level comes from the word list, an
//! 指示用語 keeps the hint the code gave it). Fields it never sees (notes, and whatever later work
//! adds) are untouched. What may change is decided by the code ([`Scope`]), not by the prompt:
//! with notes only on some questions, the others, the title, the instructions and the scaffolds
//! are kept whatever the model wrote.

pub const KEY_BLOCK_REVISE: &str = "worksheet_block_revise";
pub const SCHEMA_NAME: &str = "worksheet_block_revise";
pub const SCAFFOLD: &str = "SC-WS-BLOCK-REVISE";
pub const REPLY_CHARS: usize = 200;
pub const MAX_INSTRUCTION_CHARS: usize = 2000;
/// What a new code for a question added by the model is called in the schema.
pub const NEW: &str = "new";

pub const STATUS_RUNNING: &str = "running";
pub const STATUS_PROPOSED: &str = "proposed";
pub const STATUS_ACCEPTED: &str = "accepted";
pub const STATUS_REJECTED: &str = "rejected";
pub const STATUS_STALE: &str = "stale";
pub const STATUS_FAILED: &str = "failed";

pub const MSG_STALE_RUN: &str = "這次修改中斷了（可能是系統剛更新）。請再請 Manna 改一次。";
pub const MSG_CHANGED_MEANWHILE: &str =
    "這一區在 Manna 改好之後又被改過了，所以這個修改沒有套用。可以再請 Manna 改一次。";
/// An earlier attempt of the round whose block was changed since.
pub const MSG_EARLIER_CHANGED: &str = "這一區之後改過了，這個版本不能直接採用。";
pub const MSG_ANOTHER_RUNNING: &str = "Manna 正在改另一區，好了之後再送出。";

mod persistence;
mod rules;

pub use persistence::*;
pub use rules::*;
