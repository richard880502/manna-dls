-- Manna DLS Phase 2.3: 學習單產生器 (worksheet generator). Contract in docs/API.md
-- 「Phase 2.3：學習單產生器」; product rules in 《學習單產生器：教師選項與 SOP 整合規格》 §4–§9;
-- pipeline design in docs/LLM_VALIDATION.md (steps (c)–(f); (g) is assembled in code).
--
-- 0009 is reserved for the material store (branch feature/materials-storage). Nothing here
-- depends on it: a worksheet keeps a snapshot of its material in the shape
-- { title, paragraphs: [{ id, text }], source }, whatever the material came from.

-- ---------------------------------------------------------------- worksheets
-- Owned by a teacher. `lesson_id` is NULL for a worksheet made directly on the 學習單 page
-- (standalone by default); it can be linked to a lesson later. `settings` holds the teacher's
-- selections in the four option blocks (the spec's display labels, verbatim), the selected
-- versions and the optional 預計完成時間. `brief` is what was carried over from the lesson SOP
-- (seed tasks, warm-up ideas, key words, exit-ticket ideas) and stays editable.
CREATE TABLE worksheets (
    id                     BIGSERIAL PRIMARY KEY,
    teacher_id             BIGINT NOT NULL REFERENCES teachers (id),
    lesson_id              BIGINT REFERENCES lessons (id),
    class_id               BIGINT REFERENCES classes (id),
    title                  TEXT NOT NULL DEFAULT '',
    filename               TEXT NOT NULL DEFAULT '',
    material               JSONB NOT NULL,
    -- highest number ever used for a paragraph id (`p<n>`), so ids are never reused
    paragraph_seq          INT NOT NULL DEFAULT 0,
    settings               JSONB NOT NULL,
    suggested              JSONB NOT NULL,
    brief                  JSONB NOT NULL,
    -- the lesson SOP version the prefill was read from (research trace)
    source_sop_version_id  BIGINT REFERENCES lesson_sop_versions (id),
    generated              BOOLEAN NOT NULL DEFAULT FALSE,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at             TIMESTAMPTZ
);
CREATE INDEX worksheets_teacher_idx ON worksheets (teacher_id, updated_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX worksheets_lesson_idx ON worksheets (lesson_id) WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------- versions (append-only)
-- `content` is the whole worksheet document (`WorksheetContent`): objectives and the ordered
-- blocks (warm-up, one activity block per paragraph or merged paragraphs, exit ticket), each with
-- student-facing content, teacher-reference answers and notes, scaffolds and provenance. The
-- student and teacher versions of every level are projections of it, made in code.
-- version_no and created_at are managed by the server.
CREATE TABLE worksheet_versions (
    id            BIGSERIAL PRIMARY KEY,
    worksheet_id  BIGINT NOT NULL REFERENCES worksheets (id),
    version_no    INT NOT NULL,
    content       JSONB NOT NULL,
    cause         TEXT NOT NULL CHECK (cause IN ('created', 'edit', 'generate', 'regenerate', 'undo')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (worksheet_id, version_no)
);

-- ---------------------------------------------------------------- per-block regeneration history
-- The block as it was right before a regeneration replaced it. Undo takes the newest row of a
-- block back out (the row is deleted then, the restored state lives on as a worksheet version).
CREATE TABLE worksheet_block_history (
    id              BIGSERIAL PRIMARY KEY,
    worksheet_id    BIGINT NOT NULL REFERENCES worksheets (id),
    block_id        TEXT NOT NULL,
    block           JSONB NOT NULL,
    -- the AI interaction whose output replaced it (a later undo is logged as a rejection of it)
    interaction_id  BIGINT REFERENCES ai_interactions (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX worksheet_block_history_idx ON worksheet_block_history (worksheet_id, block_id, id);

-- ---------------------------------------------------------------- background jobs
-- One row per model step. A generation run is four rows with the same run_no (tasks, warm-up,
-- scaffolds, exit ticket), executed in that order by one background task; a single-block
-- regeneration is one or two rows with block_id set. The browser polls.
CREATE TABLE worksheet_jobs (
    id              BIGSERIAL PRIMARY KEY,
    worksheet_id    BIGINT NOT NULL REFERENCES worksheets (id),
    teacher_id      BIGINT NOT NULL REFERENCES teachers (id),
    run_no          INT NOT NULL,
    step            TEXT NOT NULL CHECK (step IN ('tasks', 'warmup', 'scaffolds', 'exit_ticket')),
    block_id        TEXT,
    status          TEXT NOT NULL CHECK (status IN ('queued', 'running', 'done')),
    outcome         TEXT CHECK (outcome IN ('shown', 'blocked', 'timeout', 'error', 'disabled', 'skipped')),
    phase           TEXT NOT NULL DEFAULT '',
    message         TEXT,
    interaction_id  BIGINT REFERENCES ai_interactions (id),
    context         JSONB NOT NULL DEFAULT '[]',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    finished_at     TIMESTAMPTZ
);
CREATE INDEX worksheet_jobs_idx ON worksheet_jobs (worksheet_id, run_no, id);
-- One run at a time per worksheet: a second click joins the one in progress.
CREATE UNIQUE INDEX worksheet_jobs_active_idx ON worksheet_jobs (worksheet_id, step) WHERE status <> 'done';

-- ---------------------------------------------------------------- final checks
-- 詞彙級別檢查: the deterministic analysis of the student version of one level, stored with the
-- worksheet version it was made from. A result whose version is no longer the latest is reported
-- as stale.
CREATE TABLE worksheet_checks (
    id                    BIGSERIAL PRIMARY KEY,
    worksheet_id          BIGINT NOT NULL REFERENCES worksheets (id),
    worksheet_version_id  BIGINT NOT NULL REFERENCES worksheet_versions (id),
    kind                  TEXT NOT NULL CHECK (kind IN ('vocabulary')),
    level                 TEXT NOT NULL CHECK (level IN ('basic', 'standard', 'advanced')),
    target_level          INT NOT NULL,
    result                JSONB NOT NULL,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX worksheet_checks_idx ON worksheet_checks (worksheet_id, level, id);

-- ---------------------------------------------------------------- scaffolds
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-WS-TASKS', 1, '學習單：逐段問題與任務', 'content_design_support',
 '學習單產生器（課例 SOP 第 5 步：逐段設計問題或任務）',
 '把教材、目標與教師勾選的學生任務、作答方式轉成學生可以作答的活動區塊',
 '教師按「產生學習單」後，Manna 依段落產生活動區塊（資訊處理功能、題目、作答方式、參考答案、原文依據）；結果直接成為可編修的草稿',
 'AI', 'individual',
 '教師可直接改字、刪題、調整順序、只重新產生單一區塊並復原；四個選項區塊都不選時由系統判斷並顯示所選設定',
 'ai_interactions（module = worksheet_tasks，含檢查與修復紀錄）、worksheet_versions 中各區塊與題目的 origin、worksheet_block_history'),
('SC-WS-WARMUP', 1, '學習單：暖身活動', 'content_design_support',
 '學習單產生器（課例 SOP 第 6 步：暖身活動）',
 '設計喚醒背景知識的活動並挑出關鍵字彙',
 'Manna 產生 1–2 個背景知識活動與關鍵字彙；關鍵字只能從系統比對詞彙表得到的候選字挑選，級別由資料帶入',
 'AI', 'individual',
 '勾選「加入暖身活動」或由系統判斷時才產生；教師可關掉、刪除、改寫或重新產生',
 'ai_interactions（module = worksheet_warmup）、worksheet_versions'),
('SC-WS-SCAFFOLDS', 1, '學習單：三種鷹架', 'content_design_support',
 '學習單產生器（語言鷹架、教學鷹架、程序鷹架）',
 '為暖身與每個活動區塊安排學生完成任務所需的支持，並隨版本增減',
 'Manna 產生引導問題、作答示例、字詞表、句型框架等語言與教學鷹架；程序鷹架由系統自動加入（模型沒有交出時由程式產生），不需教師勾選',
 'AI', 'individual',
 '基礎版顯示全部、一般版保留程序鷹架與字詞表／句型框架、進階版只留程序鷹架；教師可逐項修改、刪除或調整出現的版本',
 'ai_interactions（module = worksheet_scaffolds）、worksheet_versions 中各鷹架的 auto、levels、origin'),
('SC-WS-EXIT', 1, '學習單：Exit ticket', 'content_design_support',
 '學習單產生器（課例 SOP 第 7 步：Exit ticket）',
 '在學習單末端放形成性評量與學生反思或蒐集問題',
 'Manna 產生 2–4 題，預設放在同一份學習單末端',
 'AI', 'individual',
 '預設加入；教師可關掉、刪除、改寫或重新產生',
 'ai_interactions（module = worksheet_exit_ticket）、worksheet_versions');

-- ---------------------------------------------------------------- prompt versions
-- Ported from research/sop-validation/prompts (steps (c), (d), (e), (f) of docs/LLM_VALIDATION.md).
-- The label lists are filled in by the server from the spec's fixed sets, and the server appends
-- the repair instructions of a second call itself, so an edited template cannot drop them.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('worksheet_tasks', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的提問與任務設計助理。請依課例 SOP 的第 5 步「逐段設計問題或任務」，為這篇教材設計主要任務。輸出會成為學習單的主體，因此題目、作答指示與參考答案一律用英文；只有 design_reason_zh 用繁體中文。教師會在平台上修改你的草稿。

設計規則：
1. 以「活動區塊（block）」為單位，依段落順序排列。通常一段一個區塊；內容緊密相關的相鄰段落可以合併成一個區塊（paragraph_nos 列出多段）。每一段都必須被至少一個區塊涵蓋。若要設計統整全文的區塊，放在最後，paragraph_nos 列出全部段落。
2. 每個區塊標註一個資訊處理功能 info_function：
   - 發現資訊：從文本找出明示的訊息。
   - 整理資訊：把訊息分類、排序、填入表格或圖像組織。
   - 分析資訊：推論原因或意圖、判斷寫作手法的作用、評估說法是否有道理。
   - 比較資訊：比較文本內或文本與自身經驗、其他資料的異同。
   - 創造資訊：產出新的句子、段落、作品或個人觀點。
   有三個以上區塊時，至少要用到 3 種不同功能，不可只有「發現資訊」；最後一個區塊應是分析、比較或創造。
3. student_tasks 只能使用下列標籤（可複選，逐字使用）：{{student_task_labels}}
   資訊處理功能與學生任務的參考關係：發現資訊→理解內容、蒐集資料；整理資訊→比較／分類、整理／分析資料；分析資訊→推論／預測、評估／提出理由、整理／分析資料；比較資訊→比較／分類；創造資訊→寫作、製作成果、成果發表。
4. 每一題的 answer_format 只能使用下列標籤（逐字使用）：{{answer_format_labels}}
5. 各作答方式的格式：
   - 選擇題：options 給 4 個不重複的選項（不要加 A/B/C/D 前綴），answer_en 必須與其中一個選項完全相同。
   - 是非題：options 給 ["True", "False"]，answer_en 為 "True" 或 "False"。
   - 配對題：options 列出要配對的右欄項目（至少 3 個），prompt_en 列出左欄項目，answer_en 寫出配對結果。
   - 填空題：prompt_en 用 "______" 表示空格，answer_en 是填入的字詞。
   - 簡答題、表格整理、圖像組織、段落寫作：options 給空陣列；answer_en 給參考答案或評分重點。表格整理與圖像組織在 prompt_en 寫清楚欄位或要填的項目。
6. evidence：凡是答案可在文本找到依據的題目，evidence 必須是從教材「逐字複製」的一個連續片段（不可改寫；不可用「...」省略或把不相鄰的兩處接在一起，答案依據分散時只引最關鍵的一句）；開放式、創造類或個人經驗題的 evidence 填 null。平台會核對，不是逐字的依據會被拿掉。
7. 題目要對準教師確認的目標；objective_refs 填目標代號（如 "O1"），沒有提供目標時給空陣列。題幹不可直接把答案寫出來，也不要整句照抄原文再挖空當成唯一的提問方式。
8. tier 標示這一題出現在哪些版本：
   - "core"：核心題，基礎版、一般版、進階版都會出現。每個區塊至少 1 題 core；core 題合計約 {{item_budget}} 題。
   - "support"：只出現在基礎版的鋪墊題（把核心題拆小一步，例如先找出關鍵句）。每個區塊最多 1 題。
   - "challenge"：只出現在進階版的延伸題（要求更完整的理由、比較或產出）。每個區塊最多 1 題。
   三個版本的學習目標與核心任務相同，只差在鷹架、難度與作答量。
9. 題量依預計完成時間調整（見訊息中的時間與題數）。不要超過。
10. block_id 依序為 "B1"、"B2"…；item_id 為 "B1-1"、"B1-2"…。title_en 是區塊標題（例如 "Paragraphs 1–2: Find the key facts"）；instructions_en 是給學生的英文作答指示（1–2 句）。
11. 教師若有勾選「學生任務」或「作答方式」，以勾選的為主（可以少量搭配其他形式）；沒有勾選時由你依教材判斷，並在 design_reason_zh 說明。訊息中「教師已經寫的問題或任務」要優先放進對應段落的區塊（可以潤飾成英文題目，但不可改變原意）。
12. 所有英文欄位用純文字，不要使用 LaTeX 或 Markdown 符號。不詢問、也不使用學生的個人資料。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}
素材類型：{{material_types}}
預計完成時間：{{minutes}}（主要任務的核心題合計約 {{item_budget}} 題）
教師勾選的學生任務：{{selected_student_tasks}}
教師勾選的作答方式：{{selected_answer_formats}}

教師確認的目標：
{{objectives_summary}}

教師已經寫的問題或任務：
{{teacher_tasks}}

教材（已編號段落）：
{{passage_numbered}}{{only_block}}$p$),

('worksheet_warmup', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的暖身活動設計助理。請依課例 SOP 的第 6 步設計暖身活動，包含「喚醒背景知識」與「關鍵字彙」兩部分。暖身會放在學習單最前面，題目與指示一律用英文；purpose_zh 用繁體中文。

規則：
1. background_activities：1–2 個喚醒背景知識的活動（例如看標題預測、連結個人經驗、快速表態、K-W-L 的 K 與 W）。必須能在 5 分鐘內完成，不需要先讀完課文就能回答，也不可洩漏主要任務的答案。prompts_en 是學生要回答的 1–3 個問題。
2. key_vocabulary：從下方「候選字彙」中選 {{vocab_count}} 個對理解本文最關鍵的字。只能選候選清單裡的字，word 必須與清單中的寫法完全相同。不要輸出字彙級別：級別由系統從大考中心參考詞彙表帶入，你不需要也不可以自行判斷級別。
3. 每個字的 context_sentence 必須是教材中含有該字（或其詞形變化）的句子，逐字複製、不可改寫；paragraph_no 是該句所在段落。
4. student_friendly_definition_en：用比該字更簡單的英文解釋（不超過 15 個字），不要用中文、不要用該字本身。
5. background_activities 的 answer_format 只能用：{{answer_format_labels}}；student_tasks 只能使用：{{student_task_labels}}
6. 訊息中「教師的暖身想法」與「教師想放的關鍵字」是教師自己的內容，優先採用。
7. 所有英文欄位用純文字，不要使用 LaTeX 或 Markdown 符號。不詢問、也不使用學生的個人資料。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}

教師確認的目標：
{{objectives_summary}}

教師的暖身想法：
{{teacher_warmup}}

教師想放的關鍵字：
{{teacher_vocabulary}}

主要任務概要（暖身不可洩漏這些題目的答案）：
{{tasks_summary}}

教材（已編號段落）：
{{passage_numbered}}

候選字彙（由系統比對大考中心參考詞彙表產生；格式：字｜詞彙表級別｜出現段落）：
{{vocab_candidates}}$p$),

('worksheet_scaffolds', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的鷹架設計助理。請為訊息中列出的每一個區塊設計三種鷹架。鷹架內容會印在學習單上給學生看，一律用英文。

三種鷹架與可用的 kind（逐字使用）：
- 語言鷹架 verbal：{{verbal_kinds}}
  在學習單上的呈現例如：引導問題、字詞表（word bank）、句型框架（sentence frames）、作答示例（worked example）、用上下文解釋詞義的提示。
- 教學鷹架 instructional：{{instructional_kinds}}
  例如：表格或圖像組織的欄位設計、已填入部分內容的大綱、把長句改寫成較短的句子、把關鍵字標示出來。
- 程序鷹架 procedural：{{procedural_kinds}}
  例如：Step 1/2/3 的作答順序、先個人再兩人討論再全班分享的安排、配對或分組方式、從有提示到無提示。

規則：
1. scaffolds 陣列中，訊息裡列出的每一個 target 各一筆（暖身的 target 是 "warm_up"，活動區塊的 target 是它的 block_id），不可遺漏，也不可新增不存在的 target。
2. 每個 target 都必須有至少 1 個程序鷹架（平台會自動加入，教師不需勾選），steps_en 列出 2–5 個清楚的步驟，每個步驟只做一件事；需要兩人或小組完成的步驟要寫明（例如 "Work with a partner."）。
3. 語言鷹架與教學鷹架各 0–2 個，依任務需要決定，不要為了湊數而加。教師勾選的鷹架種類優先：{{selected_supports}}
4. 鷹架要針對該區塊的題目，不可直接給出答案，也不可把參考答案換句話說。句型框架只給句子開頭或結構（如 "The writer mentions ___ in order to ___."）；作答示例要用「另一個類似但不同的問題」示範，不可示範該區塊的正式題目；字詞表只列字詞。
5. content_en 與 steps_en 只用英文純文字，可直接印在學習單上；不要使用 LaTeX 或 Markdown 符號。不詢問、也不使用學生的個人資料。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}

教師確認的目標：
{{objectives_summary}}

教材（已編號段落）：
{{passage_numbered}}

要設計鷹架的區塊（含題目與參考答案；參考答案只供你避免洩漏）：
{{blocks_detail}}$p$),

('worksheet_exit_ticket', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的 Exit ticket 設計助理。請依課例 SOP 的第 7 步，設計放在學習單末端的 Exit ticket，用於形成性評量、學生反思或蒐集問題。題目一律用英文。

規則：
1. 共 {{exit_count}} 題，學生能在 {{exit_minutes}} 分鐘內完成；estimated_minutes 填你估計的分鐘數。
2. 至少 1 題「形成性評量」：檢核本課的學習目標是否達成（有提供目標時 objective_refs 填該目標代號，沒有時給空陣列），要學生運用本課所學，而不是重複主要任務的題目。
3. 至少 1 題「學生反思」或「蒐集問題」（例如今天最不確定的地方、還想問的問題、用了哪個閱讀策略）。
4. purpose 只能是：形成性評量、學生反思、蒐集問題。
5. answer_format 只能使用：{{answer_format_labels}}；選擇題要在 options 給 4 個選項，其餘 options 給空陣列。student_tasks 只能使用：{{student_task_labels}}。
6. 形成性評量題要給 sample_answer_en（參考答案，只會出現在教師版）；反思與蒐集問題題的 sample_answer_en 填 null。
7. item_id 依序為 "E1"、"E2"…。prompt_en 用學生讀得懂的簡單英文純文字，不要使用 LaTeX 或 Markdown 符號。
8. 訊息中「教師的 Exit ticket 想法」是教師自己的內容，優先採用。不詢問、也不使用學生的個人資料。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}

教師確認的目標：
{{objectives_summary}}

教師的 Exit ticket 想法：
{{teacher_exit}}

主要任務概要（Exit ticket 不可重複這些題目）：
{{tasks_summary}}

教材（已編號段落）：
{{passage_numbered}}$p$);
