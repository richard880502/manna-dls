-- Manna DLS: 學習單逐區修改與註記. Design: docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §4–§5; contract
-- in docs/API.md「學習單：逐區修改與註記」.
--
-- The teacher points at one block of a generated worksheet (or a piece of text in it), writes a
-- 註記 or a sentence, and Manna revises only that block. The result is a proposal: nothing in the
-- worksheet changes until the teacher presses 採用, which writes it as one new worksheet version
-- and keeps the old block in `worksheet_block_history` (so 「復原上一版」 still works).

-- ---------------------------------------------------------------- notes on a worksheet
-- A note never changes the worksheet and is never printed or exported. It remembers where it was
-- made (block, field, item, UTF-16 offsets inside the field's text as the sheet shows it) and what
-- was selected (quote plus a little text on both sides); where it is now is worked out on every
-- read, because item ids change when a block is regenerated. `share_with_ai` (「提供給 AI」) is on
-- by default here: a note on a worksheet is mostly an instruction for Manna.
CREATE TABLE worksheet_comments (
    id             BIGSERIAL PRIMARY KEY,
    worksheet_id   BIGINT NOT NULL REFERENCES worksheets (id),
    teacher_id     BIGINT NOT NULL REFERENCES teachers (id),
    block_id       TEXT NOT NULL CHECK (char_length(block_id) BETWEEN 1 AND 40),
    field          TEXT NOT NULL CHECK (field IN ('block', 'title', 'instructions', 'item')),
    -- field = 'item' only
    item_id        TEXT CHECK (char_length(item_id) BETWEEN 1 AND 40),
    -- a selection inside the field's text (UTF-16, what a browser selection gives); NULL = the whole field
    start_offset   INT,
    end_offset     INT,
    quote          TEXT NOT NULL DEFAULT '' CHECK (char_length(quote) <= 500),
    prefix         TEXT NOT NULL DEFAULT '',
    suffix         TEXT NOT NULL DEFAULT '',
    note           TEXT NOT NULL DEFAULT '' CHECK (char_length(note) <= 2000),
    share_with_ai  BOOLEAN NOT NULL DEFAULT TRUE,
    -- open: 待處理; applied: a proposal made from it was accepted; dismissed: 已略過
    status         TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'applied', 'dismissed')),
    -- the proposal that applied it
    proposal_id    BIGINT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((field = 'item') = (item_id IS NOT NULL)),
    CHECK ((start_offset IS NULL) = (end_offset IS NULL)),
    CHECK (start_offset IS NULL OR (start_offset >= 0 AND end_offset > start_offset))
);
CREATE INDEX worksheet_comments_idx ON worksheet_comments (worksheet_id, id);

-- ---------------------------------------------------------------- one block revised by Manna
-- A proposal is made in the background (`running`) and then waits for the teacher (`proposed`).
-- `base_block` is the block exactly as it was sent to the model: 採用 only writes the proposal
-- while the block in the latest version is still exactly that (otherwise `stale`). Only one
-- proposal of a worksheet runs at a time (parts of one worksheet asked for at once repeat each
-- other). `round` counts the proposals of the block, so 「再改一次」 is never the request the
-- gateway has cached. `decided_at` is set by 採用／放棄 (and by closing a failed or stale one).
CREATE TABLE worksheet_block_proposals (
    id                   BIGSERIAL PRIMARY KEY,
    worksheet_id         BIGINT NOT NULL REFERENCES worksheets (id),
    teacher_id           BIGINT NOT NULL REFERENCES teachers (id),
    block_id             TEXT NOT NULL,
    base_version_no      INT NOT NULL,
    base_block           JSONB NOT NULL,
    instruction          TEXT NOT NULL DEFAULT '' CHECK (char_length(instruction) <= 2000),
    comment_ids          BIGINT[] NOT NULL DEFAULT '{}',
    -- 「可以動到這一區的其他題」: notes anchored on items do not restrict the change to them
    whole_block          BOOLEAN NOT NULL DEFAULT FALSE,
    round                INT NOT NULL DEFAULT 1,
    status               TEXT NOT NULL CHECK (status IN
                           ('running', 'proposed', 'accepted', 'rejected', 'stale', 'failed')),
    -- the revised block (`proposed` and later)
    proposed             JSONB,
    reply                TEXT NOT NULL DEFAULT '',
    error                TEXT,
    interaction_id       BIGINT REFERENCES ai_interactions (id),
    accepted_version_no  INT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at          TIMESTAMPTZ,
    decided_at           TIMESTAMPTZ
);
CREATE INDEX worksheet_block_proposals_idx ON worksheet_block_proposals (worksheet_id, block_id, id);
CREATE UNIQUE INDEX worksheet_block_proposals_one_running
    ON worksheet_block_proposals (worksheet_id) WHERE status = 'running';

ALTER TABLE worksheet_comments ADD CONSTRAINT worksheet_comments_proposal_fkey
    FOREIGN KEY (proposal_id) REFERENCES worksheet_block_proposals (id);

-- ---------------------------------------------------------------- scaffold
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-WS-BLOCK-REVISE', 1, '學習單：依註記逐區修改', 'teacher_control_fading',
 '學習單產生器與備課對話（針對產出的學習單逐區修改）',
 '教師在學習單上指定一個區塊或一段文字並寫下註記，Manna 只修改那一區',
 'Manna 一次呼叫，只看那一區、它的註記、其他區塊的標題與題目和教材；每一題標明保留或修改，保留的題目由平台原封不動沿用；修改經過學習單的固定檢查，先給教師預覽',
 'AI', 'individual',
 '教師按「採用」才寫入學習單（新版本，可以「復原上一版」），也可以「放棄」或「再改一次」；註記標成已依此修改',
 'ai_interactions（module = worksheet_block_revise）、worksheet_block_proposals、worksheet_comments、worksheet_versions（cause = revise）、worksheet_block_history、teacher_decisions');

-- ---------------------------------------------------------------- prompt
-- Version 1 of `worksheet_block_revise`. The JSON schema (built by the server) writes `title`,
-- `instructions`, then `items` (each `source` and `keep` before its content), then `reply`: the
-- gateway emits fields in schema order, so the model decides what to keep before it writes, and
-- explains last. `source` is an enum of the block's item codes plus "new"; every string and array
-- is capped. A kept item is taken from the block by the server, never from the model's copy.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_block_revise', COALESCE(MAX(version_no), 0) + 1,
$p$你是協助台灣高中英文教師修改學習單的助手。教師在一份已經產生的學習單上指定了「一個區塊」，並寫下註記或指示，說明想怎麼改。請只修改這一個區塊，而且只改教師要求的地方。

規則：
1. 訊息裡有這一區目前的標題、作答指示與每一題。每題前面有代號（Q1、Q2…），括號裡是它在學生版上的題號；教師說「第 6 題」時，對照括號裡的題號。
2. title：改後的區塊標題；不需要改標題時寫空字串 ""。
3. instructions：改後的作答指示；不需要改時寫空字串 ""。
4. items：依改後的順序，列出這一區的每一題：
   - source：原本的題目寫它的代號（Q1、Q2…），每個代號最多用一次；新增的題目寫 "new"。
   - keep：這一題完全不改就寫 true，這時 prompt、answer 寫空字串，options 寫空陣列，answer_format 與 bloom 照原樣（平台會原封不動保留原題）。要修改就寫 false，並寫出改後完整的 answer_format、prompt、options、answer 與 bloom。
   - 沒有被要求修改的題目一律 keep: true，不可以順手改寫。教師要刪掉的題目就不要列出；其他原本的題目都要列出。
   - 「可以修改的範圍」只列出某幾題時，其他題一定要 keep: true，標題與作答指示也不要改。
5. 每題的 answer_format 只能用這些標籤：{{answer_format_labels}}。選擇題在 options 列出選項，answer 必須與其中一個選項完全相同；配對題在 options 列出要配對的項目；其他作答方式的 options 給空陣列。
6. 題幹（prompt）不可以直接寫出答案；答案必須能在教材裡找到根據。學生看到的題目與指示語維持這一區原本的語言（原本是英文就用英文）。
7. bloom：這一題主要讓學生練習的 Bloom 認知層次，只能寫 remember、understand、apply、analyze、evaluate、create 其中一個。這一區註明了固定的 Bloom 層次時，每一題都用那個層次。
8. 「改簡單一點」：思考層次不降，先簡化問句的字詞與句構，或加上語言支持（字庫、句型框架、句首）；不要把高層次的題目換成只找字面答案的題目，除非教師明說要降低層次。「改難一點」：減少支持，或在教師要求時提高思考層次。
9. 作答指示依學生實際操作的順序寫成步驟。學習單的學習目標（內容目標與語言目標）不在修改範圍：不要改寫，修改後的題目仍要對準它們。
10. reply：最後用繁體中文、一句話告訴教師你改了什麼（例如「已把 Q2 改成選擇題，選項用比較簡單的字。」）。指示不清楚或沒辦法照做時，不要亂改：所有題目都 keep: true，reply 寫一個具體的澄清問題。
11. 不要和「學習單的其他區塊」的題目重複。不要寫出任何真實人名、學校或學生資料，也不要編造教材以外的素材、網址或影片。
12. 每個文字欄位都是一段純文字，不要換行，不要使用 Markdown 符號（例如 **、#）。$p$,
$p$教師的修改指示：{{instruction}}

教師在這一區的註記：
{{block_notes}}

可以修改的範圍：{{scope}}{{round_note}}

這一區（{{block_name}}）目前的內容：
{{block_detail}}{{style_note}}

學習單的其他區塊（只供參考，不要修改，也不要重複）：
{{other_blocks}}

學習單的學習目標（不要修改）：
{{objectives_summary}}

班級特性：{{class_profile}}
學習目標（教師寫的）：{{teacher_objective}}
教師的要求：{{teacher_request}}

{{teacher_notes_for_ai}}

教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}$p$
  FROM prompt_versions WHERE prompt_key = 'worksheet_block_revise';
