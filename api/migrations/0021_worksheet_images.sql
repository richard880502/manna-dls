-- Pictures on worksheets (docs/API.md「學習單的圖片」, design: docs/WORKSHEET_BLOCK_EDIT_RESEARCH.md §6).
--
-- The text model that writes a block also writes its picture slots (`image_slots` in the block,
-- inside `worksheet_versions.content`): what picture belongs there, its alt text and caption.
-- What fills a slot lives here, outside the versioned document, so pictures arriving in the
-- background never make a new worksheet version and an editor save never overwrites them.
--
-- One row per attempt to fill a slot; the slot's current row has `current = true` (older rows
-- stay, so a picture that was replaced is not lost):
--
-- source           generated (OpenAI GPT Image), upload (the teacher's own picture), none (a
--                  slot left for the teacher, or removed by her).
-- status           queued | running | done | failed | skipped | removed. skipped: never sent to
--                  the picture model (a slot that needs a real photo, pictures off, the cap per
--                  worksheet reached); removed: the teacher took the picture out.
-- regeneration     a teacher's 「重畫」 (counts against IMAGE_MAX_REGENERATIONS).
-- hint             the teacher's note for a 重畫 (≤200), sent with the slot's description.
-- message          why there is no picture (shown to the teacher).
-- error_kind       disabled | timeout | http | moderated | network | invalid | storage | cap |
--                  photo_needed | gone | stale | interrupted: for counting, never the
--                  provider's text.
-- prompt_*         length and SHA-256 of the prompt sent (the prompt itself is not stored).
-- storage_key …    the stored picture (always JPEG): key, size, dimensions, SHA-256.
-- started_at       identifies the run of a queued or running row: a row taken over after a
--                  restart gets a new one, and the old job stops writing.
CREATE TABLE worksheet_images (
    id                BIGSERIAL PRIMARY KEY,
    worksheet_id      BIGINT NOT NULL REFERENCES worksheets (id),
    teacher_id        BIGINT NOT NULL REFERENCES teachers (id),
    block_id          TEXT NOT NULL,
    slot_id           TEXT NOT NULL,
    source            TEXT NOT NULL CHECK (source IN ('generated', 'upload', 'none')),
    status            TEXT NOT NULL
        CHECK (status IN ('queued', 'running', 'done', 'failed', 'skipped', 'removed')),
    current           BOOLEAN NOT NULL DEFAULT TRUE,
    regeneration      BOOLEAN NOT NULL DEFAULT FALSE,
    hint              TEXT NOT NULL DEFAULT '',
    message           TEXT,
    error_kind        TEXT,
    model             TEXT,
    size              TEXT,
    quality           TEXT,
    style_version_no  INT,
    prompt_chars      INT,
    prompt_sha256     TEXT,
    storage_key       TEXT,
    content_type      TEXT,
    bytes             INT,
    width             INT,
    height            INT,
    sha256            TEXT,
    interaction_id    BIGINT REFERENCES ai_interactions (id),
    started_at        TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at       TIMESTAMPTZ
);
-- One current picture per slot.
CREATE UNIQUE INDEX worksheet_images_current_idx ON worksheet_images (worksheet_id, slot_id)
    WHERE current;
CREATE INDEX worksheet_images_worksheet_idx ON worksheet_images (worksheet_id, id);
-- Startup recovery and the stale check look for open rows only.
CREATE INDEX worksheet_images_open_idx ON worksheet_images (status)
    WHERE status IN ('queued', 'running');

-- ---------------------------------------------------------------- scaffolds
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-WS-IMAGE', 1, '學習單圖片：依圖片位置產生插圖', 'content_design_support',
 '學習單產生器（一般學習單與照我的版型）',
 '寫區塊的模型同時寫出這一區需要的圖（用途、畫面描述、替代文字、圖說），伺服器在區塊通過檢查後用固定風格產圖',
 'Manna 每個圖片位置一次呼叫 OpenAI GPT Image（全站同時最多 IMAGE_CONCURRENCY 張）；需要真實照片的位置（地圖、國旗、真實人物或地標、歷史）不產圖，留空框請教師補；圖裡不放字，字寫在圖說',
 'AI', 'individual',
 '教師可以逐張重畫（可附一句說明）、換成自己上傳的圖或拿掉；失敗或略過時顯示空框與用途說明',
 'ai_interactions（module = worksheet_image，只記提示的長度與雜湊、圖的大小與雜湊、token 數）、worksheet_images');

-- ---------------------------------------------------------------- prompt versions
-- `worksheet_image_slots`: the rules appended (as 「圖片位置」) to the request of a step whose
-- schema asks for `image_slots` (only when pictures are on). `image_style`: the house style
-- prefix of every picture prompt (system_text), followed by the user template with the slot's
-- description and the teacher's note. Both can be edited on the admin page; every picture
-- records the style version it used.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('worksheet_image_slots', 1,
$p$image_slots：這個區塊需要一張圖時才寫（例如情境圖、物件圖、看圖說話、幫助理解課文的場景），不需要就給空陣列；不要為了裝飾而加圖。每個圖片位置：
- kind：scene（情境或場景插圖）、object（單一物件）、icon（簡單圖示）、photo_needed（需要真實、準確的照片：地圖、國旗、真實人物、真實地標、歷史事件或歷史照片；這種位置不會產圖，留給教師自己放）。
- purpose_zh：給教師看的一句繁體中文，說明這張圖在這一區做什麼。
- prompt_en：給產圖模型的英文畫面描述，只描述畫面（人物、動作、地點、物件、氣氛），一到三句。不要寫任何真實人名、名人、品牌、商標或網址；不要要求在圖裡寫字、標籤、數字或對話框（需要的字放在 caption）。
- alt_en：英文替代文字，一句話說明圖裡有什麼。
- caption：印在圖下方的圖說（語言同學生看到的指示語）；不需要時給空字串。題目要學生看圖作答時，題目要寫在題目裡，不要只寫在圖說。$p$,
''),
('image_style', 1,
$p$Simple flat vector illustration for a Taiwanese senior-high English worksheet. Clean outlines, soft limited palette, plain white background, one clear subject, classroom-appropriate. No text, letters, numbers, labels, logos, watermarks or brand names anywhere in the picture. No real or famous people.$p$,
$p$Picture: {{description}}{{teacher_note}}$p$);
