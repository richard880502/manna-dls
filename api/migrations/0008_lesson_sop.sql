-- Manna DLS Phase 2.1: 課例 SOP (seven-stage lesson preparation). Contract in docs/API.md
-- 「Phase 2.1：課例 SOP」; product rules in 《學習單產生器：教師選項與 SOP 整合規格》 §1–§3, §8, §9.5.
--
-- How it relates to the existing plan: lesson_versions (objective + lesson_steps) stays exactly
-- as it is. It is still 「今天要教什麼？」 and the teaching steps, and still the plan a lesson run
-- and 課後補寫 refer to. The SOP is a second, separately versioned document. Nothing in it is
-- required, so lessons without one keep working unchanged. The old single objective is NOT
-- rewritten into SOP objectives here: it never said whether it was a content or a language
-- objective, and the spec forbids writing on the teacher's behalf. The API hands it to stage 3
-- (`today_objective`), where the teacher files it under one of the two with one click.

-- ---------------------------------------------------------------- material (append-only)
-- One row each time the pasted text, its title or its source note changes. Paragraphs are split
-- by the server; `paragraphs` is [{ "id": "p3", "no": 1, "text": "…" }]. A paragraph keeps its id
-- across material versions while its text is unchanged; `paragraph_seq` is the highest number
-- ever used for an id in this lesson, so ids are never reused.
CREATE TABLE lesson_sop_materials (
    id             BIGSERIAL PRIMARY KEY,
    lesson_id      BIGINT NOT NULL REFERENCES lessons (id),
    version_no     INT NOT NULL,
    title          TEXT NOT NULL DEFAULT '',
    source_note    TEXT NOT NULL DEFAULT '',
    text           TEXT NOT NULL,
    paragraphs     JSONB NOT NULL,
    paragraph_seq  INT NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (lesson_id, version_no)
);

-- ---------------------------------------------------------------- SOP versions (append-only)
-- `content` is the whole SOP document (analysis, objectives, per-paragraph goals, task blocks,
-- warm-up, exit ticket) as validated and normalised by the server (`SopContent` in docs/API.md).
-- The material lives in its own table so that auto-save does not copy the text every time.
CREATE TABLE lesson_sop_versions (
    id           BIGSERIAL PRIMARY KEY,
    lesson_id    BIGINT NOT NULL REFERENCES lessons (id),
    version_no   INT NOT NULL,
    material_id  BIGINT REFERENCES lesson_sop_materials (id),   -- NULL: no material (yet, or removed)
    content      JSONB NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (lesson_id, version_no)
);

-- ---------------------------------------------------------------- long-running AI calls
-- A model call takes 15–50 s (sometimes 200 s), longer than the proxy allows a request to stay
-- open once a repair call is added. The request only creates a row here; a background task does
-- the work and the browser polls. `result` is the AiResult shown to the teacher.
CREATE TABLE lesson_sop_jobs (
    id              BIGSERIAL PRIMARY KEY,
    lesson_id       BIGINT NOT NULL REFERENCES lessons (id),
    teacher_id      BIGINT NOT NULL REFERENCES teachers (id),
    kind            TEXT NOT NULL CHECK (kind IN ('analysis', 'objectives')),
    status          TEXT NOT NULL CHECK (status IN ('running', 'done')),
    phase           TEXT NOT NULL DEFAULT '',
    sop_version_id  BIGINT REFERENCES lesson_sop_versions (id),
    interaction_id  BIGINT REFERENCES ai_interactions (id),    -- the row teacher decisions attach to
    result          JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ
);
CREATE INDEX lesson_sop_jobs_lesson_idx ON lesson_sop_jobs (lesson_id, kind, id);
-- One job of a kind at a time per lesson: a second click joins the running one.
CREATE UNIQUE INDEX lesson_sop_jobs_running_idx ON lesson_sop_jobs (lesson_id, kind) WHERE status = 'running';

-- The SOP version in use when 開始上課 was pressed (research trace; NULL when there was none).
ALTER TABLE lesson_runs
    ADD COLUMN lesson_sop_version_id BIGINT REFERENCES lesson_sop_versions (id);

-- ---------------------------------------------------------------- scaffolds
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-SOP-ANALYSIS', 1, '課例 SOP：逐段教材分析', 'content_design_support',
 '課例 SOP 第 2 步（分析文章或段落的寫作特色）',
 '看出每一段在全文中的功能與寫作特色，作為訂目標與設計提問的依據',
 '教師按下後，Manna 逐段提出功能、摘要與寫作特色，每項特色附該段原文的逐字片段；建議以金色顯示在對應段落下方',
 'AI', 'individual',
 '只在教師按下時啟動；每一段可採用、修改後採用或不用；不會改動教師已寫的分析；可完全略過',
 'ai_interactions（module = sop_analysis，含檢查與修復紀錄）、teacher_decisions、lesson_sop_versions 中各段的 origin'),
('SC-SOP-OBJECTIVES', 1, '課例 SOP：SIOP 內容目標與語言目標推薦', 'content_design_support',
 '課例 SOP 第 3 步（內容目標、語言目標與 108 課綱連結）',
 '把本課目標分成內容目標與語言目標，並對照 108 課綱條目',
 '教師按下後，Manna 依教材、班級特性與教師已寫的目標另外列出建議；每項有類型、理由與課綱連結（條文與來源由知識庫帶出），找不到條目時標示尚未完成課綱連結',
 'AI', 'individual',
 '只在教師按下時啟動；每一項可採用、修改後採用或不用；不覆寫教師既有目標；可完全略過',
 'ai_interactions（module = sop_objectives）、teacher_decisions、lesson_sop_versions 中各目標的 origin 與 link_status');

-- ---------------------------------------------------------------- prompt versions
-- Ported from research/sop-validation/prompts (docs/LLM_VALIDATION.md steps (a) and (b)).
-- The server appends the repair instructions of a second call itself, so an edited template
-- cannot drop them. Placeholders: {{school_track}}, {{class_profile}}, {{passage_numbered}},
-- {{analysis_summary}}, {{teacher_objectives}}, {{curriculum_items}}.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('sop_passage_analysis', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的教材分析助理。教師貼上一篇英文閱讀教材，請依「課例 SOP」的第 2 步「分析文章或段落的寫作特色」逐段分析，供教師後續訂定目標與設計提問。你的輸出只是建議，教師會逐段決定採用、修改或不用。

規則：
1. 教材已依段落編號為 [P1]、[P2]…。paragraphs 必須每一段各一筆、依序排列，paragraph_no 與編號一致，不可合併、遺漏或新增段落。
2. function_zh：這一段在全文中的功能（例如：以故事開場、提出問題、說明原因、舉例佐證、轉折、提出解方、總結呼籲）。summary_zh：一句話摘要。
3. writing_features：每段 1–3 個「寫作特色」（例如：主題句位置、因果／對比／時間順序等組織方式、轉折語、舉例、數據、引用、修辭、語氣、指涉詞）。每個特色都要有 evidence。
4. evidence 必須是從該段原文「逐字複製」的連續片段（6–30 個英文單字），不可改寫、不可刪字、不可用「...」省略、不可把兩處文字接在一起。平台會核對 evidence 是否真的是該段的連續文字，不是的會被拿掉。找不到逐字證據的特色就不要列。
5. language_focus：全文值得教的語言重點 3–6 項（字彙、句型或篇章標記），item 寫出該詞語或句型本身，evidence 同樣必須逐字引自所標示的段落。
6. 所有 *_zh 欄位用繁體中文純文字、精簡具體，不要空泛形容（如「內容豐富」「用詞優美」），不要使用 LaTeX 或 Markdown 符號。evidence 與 item 維持英文原文。
7. 只根據教材本身分析；不要補充教材沒有的事實，也不要評論學生程度。不詢問、也不使用學生的個人資料。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}

教材（已編號段落）：
{{passage_numbered}}$p$),

('sop_objectives', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的目標設定助理。請依 SIOP（Sheltered Instruction Observation Protocol）的邏輯，為這一課分別推薦「內容目標」與「語言目標」，並連結 108 課綱條目。你的輸出只是建議，教師會確認、修改或刪除；平台不會用你的建議覆寫教師已寫的目標。

SIOP 目標的區分：
- 內容目標：學生在本課要理解或完成的學科內容、概念或任務（例如理解文章提出的問題與解方、比較兩種觀點、評估作者的論證）。
- 語言目標：學生為了達成內容目標需要使用的語言功能、聽說讀寫能力、詞彙或語言形式（例如用因果連接詞口頭說明原因、用特定句型寫出比較句、從上下文推測關鍵字義）。
- 兩類目標必須分開呈現，至少各 1 項，合計 3–5 項；不可用一個籠統目標取代。

規則：
1. 目標必須具體到這篇教材、這個班級與一節課的時間：寫出學生要「做什麼」、「用什麼材料或段落」、「產出什麼」。不可把課綱條文直接抄成目標。
2. objective_zh 用繁體中文純文字，以「學生能…」開頭；不要使用 LaTeX 或 Markdown 符號，教材中的英文人名、專有名詞維持英文原文，不要自行翻譯成中文。student_facing_en 是給學生看的英文版本，以 "I can ..." 開頭，用簡單英文。
3. reason_zh：說明為什麼推薦這個目標（對應教材哪個特色、班級哪個需求或課堂時間限制）。
4. paragraph_refs：這個目標主要對應的段落編號，填正整數（例如 P1、P2 填 [1, 2]）。
5. 課綱連結：只能從訊息中「可連結的 108 課綱條目」清單選擇，item_code 必須與清單完全相同（逐字複製）。每個目標連結 1–3 條，why_zh 說明該條目與此目標的關係。條文原文由平台從資料庫帶出，你不需要也不可以改寫條文。
6. 如果清單中沒有足以支持的條目（或清單是空的），link_status 填 "unlinked"、curriculum_links 給空陣列，並在 reason_zh 說明尚未完成課綱連結。絕對不可以自行編造或憑記憶寫出清單以外的條目代碼；平台會核對，清單以外的代碼會被拿掉。
7. 有連結時 link_status 填 "linked"。objective_id 依序為 "O1"、"O2"…。
8. 訊息中「教師已經寫的目標」是教師自己的內容：不要重複它們，也不要改寫它們；請推薦能補充它們的目標。
9. 不詢問、也不使用學生的個人資料。「班級特性」是教師對整個班級的概括描述。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}
課堂時間：一節課（未另外說明時以 50 分鐘估計）

教師已經寫的目標：
{{teacher_objectives}}

教材（已編號段落）：
{{passage_numbered}}

教材分析（教師在上一步確認的內容）：
{{analysis_summary}}

可連結的 108 課綱條目（只能從這裡選；格式為「代碼｜標題｜條文」）：
{{curriculum_items}}$p$);
