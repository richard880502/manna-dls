-- Manna DLS Phase 2.5: 我的學習單版型 (worksheet templates). Contract in docs/API.md
-- 「Phase 2.5：我的學習單版型」; the extraction and generation prompts are ported from the
-- validated probe (docs/WORKSHEET_TEMPLATE_PROBE.md, research/worksheet-template/probe.py: v4
-- extraction in two calls, gen2 one call per block).
--
-- A teacher uploads one of her own past worksheets (a library material), Manna extracts its
-- design as a template, she checks and edits it, and new worksheets can then be generated to
-- follow it. Templates are the teacher's own: never shared, never readable by anyone else.

-- ---------------------------------------------------------------- templates
-- `status`: `extracting` while the background job runs, `ready` once it has a version,
-- `failed` with a Chinese `error` (POST …/retry re-runs it). `extract_run` numbers the
-- extraction attempts so an old job that outlived a restart can never overwrite a newer one;
-- `extract_started_at` lets a job that died with the server be reported as failed.
-- Deleting a template is a hard delete (versions go with it); deleting its source material
-- only clears `source_material_id`.
CREATE TABLE worksheet_templates (
    id                  BIGSERIAL PRIMARY KEY,
    teacher_id          BIGINT NOT NULL REFERENCES teachers (id),
    title               TEXT NOT NULL CHECK (char_length(title) <= 200),
    status              TEXT NOT NULL CHECK (status IN ('extracting', 'ready', 'failed')),
    error               TEXT,
    source_material_id  BIGINT REFERENCES materials (id) ON DELETE SET NULL,
    extract_run         INT NOT NULL DEFAULT 1,
    extract_started_at  TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX worksheet_templates_teacher_idx ON worksheet_templates (teacher_id, created_at DESC);

-- ---------------------------------------------------------------- versions (append-only)
-- `content` is the template (`WsTemplate`). The current content is the latest version.
-- `origin`: `ai` for the extracted first version, `teacher` for every save.
CREATE TABLE worksheet_template_versions (
    id           BIGSERIAL PRIMARY KEY,
    template_id  BIGINT NOT NULL REFERENCES worksheet_templates (id) ON DELETE CASCADE,
    version_no   INT NOT NULL,
    content      JSONB NOT NULL,
    origin       TEXT NOT NULL CHECK (origin IN ('ai', 'teacher')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (template_id, version_no)
);

-- ---------------------------------------------------------------- worksheets made from a template
ALTER TABLE worksheets
    ADD COLUMN template_id BIGINT REFERENCES worksheet_templates (id) ON DELETE SET NULL,
    ADD COLUMN template_version_no INT;

-- A template generation is one background job (step `template`) that makes one model call per
-- block, at most three at a time; its phase reports the progress.
ALTER TABLE worksheet_jobs DROP CONSTRAINT worksheet_jobs_step_check;
ALTER TABLE worksheet_jobs ADD CONSTRAINT worksheet_jobs_step_check
    CHECK (step IN ('tasks', 'warmup', 'scaffolds', 'exit_ticket', 'template'));

-- ---------------------------------------------------------------- scaffolds
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-WS-TPL-EXTRACT', 1, '學習單版型：讀出教師學習單的設計', 'pck_externalisation',
 '我的學習單版型（上傳一份做過的學習單）',
 '把教師自己學習單裡隱含的設計（區塊順序、題型、指示語、鷹架、逐段重複）外顯成可重複使用的版型',
 'Manna 分兩次呼叫：先列大綱（逐段重複的區塊只列一次），再依大綱數量寫出每個區塊的細節；去除課文專屬內容與個人資料',
 'AI', 'individual',
 '教師可以改標題、刪除或調整區塊順序、切換「逐段」與「需要我提供素材」、改作答方式與題數；每次儲存是一個新版本',
 'ai_interactions（module = worksheet_template_outline／worksheet_template_details）、worksheet_template_versions 的 origin'),
('SC-WS-TPL-GEN', 1, '學習單版型：照版型為新課產生學習單', 'content_design_support',
 '學習單產生器（照我的版型）',
 '依教師確認的版型，為新教材逐一產生每個區塊',
 'Manna 每個區塊一次呼叫（最多同時三個）；逐段區塊依新教材段數展開，總數上限 16 個；需要教師提供素材的區塊只留空位與提示，不編造素材',
 'AI', 'individual',
 '產生後與一般學習單一樣可以編修、單一區塊重新產生與復原、匯出',
 'ai_interactions（module = worksheet_template_section）、worksheet_versions、worksheet_block_history');

-- ---------------------------------------------------------------- prompt versions
-- Ported from research/worksheet-template/probe.py: OUTLINE4_SYS (call A), DETAIL_SYS (call B,
-- with the rule for needs_external_material added) and GEN2_SYS (one block per call, with the
-- rules for external material and per-paragraph blocks added). The label lists are enums in the
-- JSON schemas built by the server from the spec's fixed sets.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('worksheet_template_outline', 1,
$p$你是協助高中英文教師的備課助手。教師上傳一份自己做過的學習單（文字是從 DOCX/PDF 抽出來的，表格已被攤平，可能有斷行或順序錯亂）。
請抽出這份學習單的「設計版型」，讓系統之後能用同樣的設計，為「另一課的新教材」產生一份相似的學習單。

這一步只要列出大綱。
1. outline：依出現順序列出學習單的主要區塊（從開頭到最後一題，包含最後的寫作、反思、評量或 Wrap up）。先從頭到尾看完全文再寫，不要漏掉後半段。
2. 一個區塊裡的小步驟（Step 1/2/3、小題、同一個表格的各欄）不要另外列；它們屬於同一個區塊。
3. 針對課文「每一段」重複出現的同一組活動只列一次，per_paragraph 設為 true，標題用 [n] 表示段落。其他區塊 per_paragraph 為 false。
4. 學生版與教師版（或答案版）重複出現時只算一次。
5. 標題寫成可套用到其他課的通用樣式，把課文專屬字詞換成 [佔位符]。
6. 不要寫出任何人名、學校、班級、出版社、教師或學生資料。
7. 學習單最上方的標題列、姓名／班級／座號／日期欄不是區塊（它們屬於 header_fields），不要列進 outline。$p$,
$p$以下是學習單文字：

{{worksheet_text}}$p$),

('worksheet_template_details', 1,
$p$你是協助高中英文教師的備課助手。教師上傳一份自己做過的學習單（文字是從 DOCX/PDF 抽出來的，表格已被攤平，可能有斷行或順序錯亂）。
請抽出這份學習單的「設計版型」，讓系統之後能用同樣的設計，為「另一課的新教材」產生一份相似的學習單。

規則：
1. 描述的是設計，不是這一課的內容。title_pattern、item_patterns 要寫成可套用到其他課的通用樣式，把課文專屬的字詞換成 [段落編號]、[關鍵字]、[人物] 這類佔位符。例如「Paragraph 3: Coherence」→「Paragraph [n]: [寫作概念]」；「What do they put on the altars?」→「What do [人物] do/put [地點]?（依段落細節提問）」。
2. sections 必須與給定的大綱一一對應：數量相同、順序相同，每個大綱項目一個 section。
3. student_tasks、answer_formats、scaffolds、generation_options 只能從給定的選項中選；不確定就少選，不要猜。
4. item_count 是該區塊大約有幾題或幾格要學生作答。
5. text_scope 說明這個區塊針對課文哪裡（例如「逐段」「全文」「第 1 段」「課文外」）。
6. instruction_style 描述指示語的寫法（語言、語氣、長短，例如「英文祈使句，一句話」）。
7. 不要寫出任何人名、學校、班級、出版社、教師或學生資料。
8. style_notes 寫 3–6 條這位老師的設計習慣（例如「每段先問細節再問寫作手法」「句型練習都附中文提示」），也要是可套用到其他課的寫法。
9. needs_external_material：這個區塊需要學習單本身課文以外的素材（影片、錄音、圖片，或另一篇文章）才能作答時為 true；只用課文就能完成時為 false。
所有說明文字用繁體中文。$p$,
$p$大綱（共 {{outline_count}} 項，sections 要逐項對應）：
{{outline_lines}}

以下是學習單文字：

{{worksheet_text}}$p$),

('worksheet_template_section', 1,
$p$你是協助高中英文教師的備課助手。教師提供了自己學習單的「設計版型」和一篇新教材。
請完全依照版型，為新教材產生一份學習單：
1. 區塊數量與順序、每個區塊的作答方式、題數、指示語寫法、鷹架，都照版型。
2. 題目內容全部依據新教材，答案必須能在新教材找到根據；不要出現版型以外、也不要出現舊課文的內容。
3. item_patterns 是題型樣式，請把佔位符換成新教材的內容。
4. 新教材沒有適合某區塊的內容時，仍保留該區塊並以最接近的方式改寫，不要刪除。
4b. 訊息說這個區塊「只針對新教材第 n 段」時，題目只針對那一段，標題寫出段落編號。
5. 不要寫出任何真實人名、學校或學生資料。
6. 目標區塊的 needs_external_material 為 true 時，這個區塊需要教師另外提供素材（影片、錄音、圖片或另一篇文章）：不要寫出、也不要編造任何素材的名稱、網址、作者或內容；題目只寫成教師拿到素材後可以直接使用的空位（例如「看完老師提供的影片後，寫出兩個重點：______」）。
7. 選擇題在 options 列出選項，answer 與其中一個選項完全相同；其他作答方式的 options 給空陣列。answer 是參考答案，只會出現在教師版。
學生看到的題目與指示語依版型的 instruction_language；scaffold 欄位寫給學生的提示（可空字串）。
這次只產生「一個區塊」：給定的目標區塊。題數照版型的 item_count（最多 12 題）。$p$,
$p$版型整體：
{{template_head}}

目標區塊（第 {{block_no}} 個，共 {{block_total}} 個）：
{{target_section}}{{paragraph_note}}

已完成的區塊標題：{{done_titles}}

新教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}$p$);
