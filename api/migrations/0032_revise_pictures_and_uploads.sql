-- Manna DLS: pictures in revisions, and pictures attached to a revise request.
--
-- 1. A revision (the prep session's 改學習單 and 改這一區) may propose picture actions beside its
--    text changes: add a picture, redraw one, remove one (docs/API.md「改圖片」). A block
--    proposal keeps them in `picture_actions` and runs them only when the teacher accepts.
-- 2. The teacher may attach screenshots or photos (≤ 2, JPEG after re-encoding) to a revise
--    request. They are stored in the file storage under `prep-images/…`, belong to one prep
--    session (deleted with it) and are attached to the turn or the block proposal that sent them.
--    Only their ids, sizes and hashes go into the research record, never their bytes.

ALTER TABLE worksheet_block_proposals
    ADD COLUMN picture_actions JSONB NOT NULL DEFAULT '[]';

CREATE TABLE prep_images (
    id             BIGSERIAL PRIMARY KEY,
    teacher_id     BIGINT NOT NULL REFERENCES teachers (id),
    session_id     BIGINT NOT NULL REFERENCES prep_sessions (id) ON DELETE CASCADE,
    -- NULL while the image waits for the message it will be sent with
    turn_id        BIGINT REFERENCES prep_turns (id) ON DELETE CASCADE,
    proposal_id    BIGINT REFERENCES worksheet_block_proposals (id) ON DELETE CASCADE,
    storage_key    TEXT NOT NULL,
    content_type   TEXT NOT NULL DEFAULT 'image/jpeg',
    bytes          INT NOT NULL,
    original_bytes INT NOT NULL,
    width          INT NOT NULL,
    height         INT NOT NULL,
    sha256         TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (turn_id IS NULL OR proposal_id IS NULL)
);
CREATE INDEX prep_images_session_idx ON prep_images (session_id, id);
CREATE INDEX prep_images_turn_idx ON prep_images (turn_id) WHERE turn_id IS NOT NULL;
CREATE INDEX prep_images_proposal_idx ON prep_images (proposal_id) WHERE proposal_id IS NOT NULL;

-- ---------------------------------------------------------------- prompts
-- New versions of `prep_revise` and `worksheet_block_revise`: the current latest text (whatever
-- it is: the admin page and other migrations add versions too) with one section appended to the
-- system text and the pictures to the user message. The schema (built by the server) adds
-- `picture_actions`, with no entries while pictures are off.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

圖片（picture_actions）：
- 只有教師要求加圖、重畫圖或移除圖時，才在 picture_actions 寫動作；沒有要動圖片就給空陣列。最多 3 個動作，每個區塊最多 2 個圖片位置；你只是提出建議，教師同意後平台才會畫圖或移除。
- add（加一張圖）：block_id 寫要加圖的區塊的 block_id，slot_id 給空字串；kind 選 scene、object 或 icon；purpose_zh 用繁體中文一句話說明這張圖在這一區做什麼；prompt_en 是給產圖模型的英文畫面描述（一到三句，只描述畫面）；caption 是印在圖下方的圖說（語言同學生看到的指示語，不需要就給空字串）。
- redraw（重畫）：slot_id 只能寫「目前的圖片」列出的 slot_id，block_id 給空字串；prompt_en 用英文簡短寫出要怎麼改（例如 "no text in the picture"、"a sunny day"）；purpose_zh 用繁體中文說明這次要改什麼。教師說「不要有文字」時，也要在 prompt_en 寫明圖裡不要有任何文字。
- remove（移除）：slot_id 只能寫「目前的圖片」列出的 slot_id；其他欄位給空字串或照填 kind。
- 圖裡不可以有字母、文字、數字、標籤或對話框，也不要真實人物、名人、品牌、商標或網址。
- 教師附上圖片（截圖或照片）時，圖片是用來說明要改哪裡（例如圈出有問題的地方）：和教師寫的字一起判斷。圖片裡若有學生的臉孔或姓名，不要把它們寫進學習單或回覆。$p$,
       v.user_template || $p$

{{picture_rules}}

{{pictures_note}}{{attachments_note}}$p$
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_block_revise', 'prep_revise')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

UPDATE scaffolds
   SET support_form = support_form || '；教師也可以請 Manna 加、重畫或移除圖片（同樣先給教師看，採用後才畫），並附上截圖或照片說明要改哪裡'
 WHERE scaffold_code IN ('SC-WS-BLOCK-REVISE') AND version = 1;
