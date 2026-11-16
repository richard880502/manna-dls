-- Manna DLS Phase 2.6: 備課對話 (prep sessions). Contract in docs/API.md「Phase 2.6：備課對話」.
--
-- A conversation-first entry point: the teacher brings the lesson text (or only a topic), presses
-- an action chip (做學習單／規劃教學活動／設計評量／找教學資源) or asks directly, and every result
-- is one turn of a session. Worksheets made in a session are ordinary worksheets: deleting the
-- session (a hard delete, turns included) never touches them.

-- ---------------------------------------------------------------- sessions
-- Owned by one teacher; another teacher's id is a 404. `material_id` / `class_id` /
-- `template_id` are the context later turns use; deleting any of them only clears the link.
-- `topic` is used when there is no material (only 找教學資源 then).
CREATE TABLE prep_sessions (
    id           BIGSERIAL PRIMARY KEY,
    teacher_id   BIGINT NOT NULL REFERENCES teachers (id),
    title        TEXT NOT NULL CHECK (char_length(title) <= 200),
    material_id  BIGINT REFERENCES materials (id) ON DELETE SET NULL,
    topic        TEXT CHECK (char_length(topic) <= 200),
    class_id     BIGINT REFERENCES classes (id) ON DELETE SET NULL,
    template_id  BIGINT REFERENCES worksheet_templates (id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX prep_sessions_teacher_idx ON prep_sessions (teacher_id, updated_at DESC);

-- ---------------------------------------------------------------- turns
-- One request and its result, numbered per session. Every turn runs in the background:
-- `running` until it is `done` or `failed` (with a Chinese `error`). A worksheet / assessment
-- turn points at the worksheet it made (`worksheet_id`, set at once) and is `running` while
-- that worksheet's generation runs. `result` holds the kind's output (ActivityPlan, Resources,
-- an answer, a revision reply); `interaction_id` the model call it came from, for 有幫助／不合用.
CREATE TABLE prep_turns (
    id              BIGSERIAL PRIMARY KEY,
    session_id      BIGINT NOT NULL REFERENCES prep_sessions (id) ON DELETE CASCADE,
    seq             INT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN
                      ('worksheet', 'assessment', 'activity', 'resources', 'ask', 'revise')),
    request         TEXT NOT NULL CHECK (char_length(request) <= 2000),
    status          TEXT NOT NULL CHECK (status IN ('running', 'done', 'failed')),
    error           TEXT,
    worksheet_id    BIGINT REFERENCES worksheets (id) ON DELETE SET NULL,
    result          JSONB,
    interaction_id  BIGINT REFERENCES ai_interactions (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ,
    UNIQUE (session_id, seq)
);
-- Only one turn of a session runs at a time.
CREATE UNIQUE INDEX prep_turns_one_running ON prep_turns (session_id) WHERE status = 'running';
CREATE INDEX prep_turns_worksheet_idx ON prep_turns (worksheet_id) WHERE worksheet_id IS NOT NULL;

-- ---------------------------------------------------------------- worksheets made in a session
-- 「回到備課對話」: the session that made a worksheet. Cleared when the session is deleted.
ALTER TABLE worksheets
    ADD COLUMN prep_session_id BIGINT REFERENCES prep_sessions (id) ON DELETE SET NULL;

-- A revision typed in a session is one new worksheet version.
ALTER TABLE worksheet_versions DROP CONSTRAINT worksheet_versions_cause_check;
ALTER TABLE worksheet_versions ADD CONSTRAINT worksheet_versions_cause_check
    CHECK (cause IN ('created', 'edit', 'generate', 'regenerate', 'undo', 'revise'));

-- ---------------------------------------------------------------- scaffolds
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-PREP-ACTIVITY', 1, '備課對話：規劃教學活動', 'content_design_support',
 '備課對話（規劃教學活動）',
 '依教材、班級特性與課綱條目，規劃一節課的教學流程（引起動機、發展活動、統整與評量）',
 'Manna 一次呼叫產出目標（對應課綱）與 3–6 個步驟，引起動機附 2–3 個選項、建議的英文課堂用語與「卡住了怎麼辦」；課綱代碼只能用檢索給的候選，條文由知識庫帶出',
 'AI', 'individual',
 '教師逐項標記有幫助／不合用；之後做學習單時，引起動機的想法會帶進暖身',
 'ai_interactions（module = prep_activity）、prep_turns.result'),
('SC-PREP-ASK', 1, '備課對話：直接問 AI', 'content_design_support',
 '備課對話（直接問 AI）',
 '回答教師備課時的問題，扣緊這一課的教材、班級與最近的對話',
 'Manna 一次呼叫，帶入教材、班級特性、檢索到的課綱條目與最近三則對話摘要，回答純文字並建議接下來可以問的問題',
 'AI', 'individual',
 '教師可以接著追問，或點建議的問題',
 'ai_interactions（module = prep_ask）、prep_turns.result'),
('SC-PREP-REVISE', 1, '備課對話：用一句話修改學習單', 'teacher_control_fading',
 '備課對話（針對產出的學習單提出修改）',
 '依教師用文字下的指示修改學習單的區塊',
 'Manna 一次呼叫，只回傳有改動的區塊；每個改動都經過學習單的固定檢查，沒通過的區塊維持原樣並告訴教師',
 'AI', 'individual',
 '每次修改是學習單的一個新版本；每個被改的區塊都可以在編輯器按「復原」',
 'ai_interactions（module = prep_revise）、worksheet_versions（cause = revise）、worksheet_block_history');

-- ---------------------------------------------------------------- prompt versions
-- Version 1 of the three prompts. The JSON schemas (built by the server) cap every string and
-- array: the gateway enforces maxLength / maxItems while decoding, which stops runaways.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('prep_activity', 1,
$p$你是協助台灣高中英文教師備課的助手。教師提供了這一課的教材（每段前有 [第 n 段] 標記）。請為這一課規劃一節課（約 50 分鐘）的教學活動流程，給教師參考。

規則：
1. title：這份教學活動規劃的名稱（繁體中文，可以夾英文課名）。
2. goals：1–4 個學習目標，寫成「學生能……」的繁體中文句子，具體、可以觀察，並且來自這篇教材的內容與語言重點。curriculum_codes 只能從「可對應的課綱條目」清單裡選代碼，每個目標最多 3 個；沒有合適的就給空陣列，不要自己編代碼。
3. steps：3–6 個教學步驟，依上課順序排列。phase 只能是「引起動機」「發展活動」「統整與評量」三者之一：第一步是「引起動機」，最後一步是「統整與評量」，中間是「發展活動」。
4. 每個步驟：
   - title：簡短的步驟名稱。
   - minutes：建議分鐘數，所有步驟加起來大約 50 分鐘。
   - idea：用繁體中文寫教學思路（做什麼、為什麼這樣做、學生怎麼參與），要具體指出用到教材的哪一段或哪些內容。
   - script：老師上課時可以直接說的英文課堂用語（classroom English），每行一句，最多 6 行。
   - stuck_tip：用繁體中文寫「學生卡住了怎麼辦」的一個具體做法；沒有就給空字串。
5. options：只有「引起動機」的步驟提供 2–3 個不同的暖身做法讓教師挑選（例如一個用圖片、一個用提問、一個用小遊戲），每個寫 title、idea、script（英文，最多 4 行）與 minutes。其他步驟的 options 一律給空陣列。
6. 活動要配合班級特性；沒有提供班級特性時，以一般高中班級設計。
7. 不要寫出任何真實人名、學校或學生資料。不要編造教材以外的影片、網站或文章的名稱與網址；需要額外素材時，寫成「老師準備的圖片」這類說法。
8. 只用純文字，不要使用 Markdown 符號（例如 **、#）。$p$,
$p$班級特性：{{class_profile}}

可對應的課綱條目（只能從這裡選代碼）：
{{curriculum_items}}

教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}$p$),

('prep_ask', 1,
$p$你是 Manna，協助台灣高中英文教師備課的助手。教師在備課對話裡直接問你問題，請用繁體中文回答。

規則：
1. answer：直接回答教師的問題，具體、可以馬上用在課堂上；需要時可以夾帶英文例句或課堂用語。教師有提供教材時，扣緊教材的段落與內容。最多約 600 字。
2. 只用純文字，不要使用 Markdown 符號（例如 **、#、表格）。需要分點時，每點另起一行，用「1.」「2.」編號。
3. 課綱條目只能引用「相關課綱條目」裡列出的代碼；不確定就不要引用。
4. 不知道或教材裡沒有的資訊，直接說不確定；不要編造事實、出處、影片或網址。
5. follow_ups：最多 3 個教師接下來可能想問的簡短問題（每個 40 字以內，用教師的口吻，例如「幫我把這三個活動排進 50 分鐘」）；沒有就給空陣列。
6. 不詢問、也不使用學生的個人資料。$p$,
$p$教師的問題：{{question}}

這段對話先前的內容：
{{recent_turns}}

班級特性：{{class_profile}}

相關課綱條目：
{{curriculum_items}}

教材：
{{passage_numbered}}$p$),

('prep_revise', 1,
$p$你是協助台灣高中英文教師修改學習單的助手。教師針對一份已經產生的學習單，用文字說明想怎麼改。請照教師的指示修改，並且只回傳有改動的區塊。

規則：
1. reply：用繁體中文、一兩句話告訴教師你改了什麼（例如「已把 B 區的第 2、3 題改成選擇題。」）。如果指示不清楚或沒辦法照做，不要亂改：changes 給空陣列，reply 寫一個具體的澄清問題或說明原因。
2. changes：每個有改動的區塊一項，最多 6 項。block_id 必須是下面學習單裡的 block_id；教師用「A 區」「B 區」稱呼區塊時，對照每個區塊的 label。
3. 每一項要寫出這個區塊修改後完整的 title、instructions 和全部 items：沒有改的題目也照原樣抄回來，順序不變；教師要刪的題目就不要抄。一個區塊最多 12 題。沒有被要求改的區塊不要列出。
4. 每題的 answer_format 只能用這些標籤：{{answer_format_labels}}。選擇題在 options 列出選項，answer 必須與其中一個選項完全相同；配對題在 options 列出要配對的項目；其他作答方式的 options 給空陣列。
5. 題幹（prompt）不可以直接寫出答案；答案必須能在教材裡找到根據。
6. 學生看到的題目與指示語維持原本的語言（原本是英文就用英文）。
7. 不要寫出任何真實人名、學校或學生資料，也不要編造教材以外的素材、網址或影片。
8. 只用純文字，不要使用 Markdown 符號。$p$,
$p$教師的修改指示：{{instruction}}

學習單（{{worksheet_title}}）目前的區塊：
{{blocks_json}}

教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}$p$);
