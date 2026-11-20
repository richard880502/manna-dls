-- 逐步設計：第 4–7 步的 AI 協助（逐段整合目標、逐段問題或任務、暖身、Exit ticket），
-- 第 3 步的「換一個」與「幫我修英文」。
--
-- * 背景工作多了四種 kind，並可帶一個 slot：同一課例、同一種工作、同一個 slot 只會有一個在跑
--   （整批是 slot ''，單一建議的重做是 slot = 建議的位置或段落 id）。parent_job_id 指到被重做的建議
--   所在的工作；params 存重做的細節（被換掉的建議、只做哪一段）。
-- * 新的提示詞與鷹架（研究紀錄用）。第 5–7 步沿用已驗證的學習單提示詞（複製目前最新版再補上 SOP 的規則），
--   所以兩邊的規則與檢查一致；之後各自在後台改版。

ALTER TABLE lesson_sop_jobs DROP CONSTRAINT lesson_sop_jobs_kind_check;
ALTER TABLE lesson_sop_jobs ADD CONSTRAINT lesson_sop_jobs_kind_check
    CHECK (kind IN ('analysis', 'objectives', 'goals', 'tasks', 'warmup', 'exit_ticket'));
ALTER TABLE lesson_sop_jobs
    ADD COLUMN slot TEXT NOT NULL DEFAULT '',
    ADD COLUMN parent_job_id BIGINT REFERENCES lesson_sop_jobs (id),
    ADD COLUMN params JSONB;
DROP INDEX lesson_sop_jobs_running_idx;
CREATE UNIQUE INDEX lesson_sop_jobs_running_idx
    ON lesson_sop_jobs (lesson_id, kind, slot) WHERE status = 'running';

-- ---------------------------------------------------------------- scaffolds
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-SOP-GOALS', 1, '課例 SOP：逐段整合目標', 'content_design_support',
 '課例 SOP 第 4 步（逐段整合目標）',
 '決定每一段課文可以帶到哪幾項目標，以及怎麼帶',
 '教師按下後，Manna 逐段建議這一段對應第 3 步的哪些目標，並用一兩句話說明怎麼帶；建議以金色顯示在對應段落下方',
 'AI', 'individual',
 '只在教師按下時啟動；每一段可採用、修改後採用或不用；不改動教師已選的對應與筆記；可完全略過',
 'ai_interactions（module = sop_paragraph_goals）、teacher_decisions、lesson_sop_versions 中各段對應的 origin'),
('SC-SOP-TASKS', 1, '課例 SOP：逐段問題或任務與鷹架', 'content_design_support',
 '課例 SOP 第 5 步（逐段設計問題或任務）',
 '為每一段設計問題或任務、選定學生任務與作答方式，並安排語言、教學與程序鷹架',
 '教師按下後，Manna 逐段（或合併相鄰段落）建議任務、學生任務標籤、作答方式與三種鷹架；程序鷹架由程式依任務自動產生；建議以金色顯示在對應段落下方',
 'AI', 'individual',
 '只在教師按下時啟動；每一段可採用、修改後採用或不用；不覆寫教師寫的任務；標籤可再更改',
 'ai_interactions（module = sop_paragraph_tasks、sop_scaffolds）、teacher_decisions、lesson_sop_versions 中各任務的 origin'),
('SC-SOP-WARMUP', 1, '課例 SOP：暖身與關鍵字彙', 'content_design_support',
 '課例 SOP 第 6 步（暖身）',
 '設計喚醒背景知識的活動，並挑出讀之前先處理的關鍵字彙',
 '教師按下後，Manna 建議暖身活動，並從程式依詞彙表挑出的候選字裡選出關鍵字彙；字彙級別一律由程式帶入',
 'AI', 'individual',
 '只在教師按下時啟動；每一項可採用、修改後採用或不用；不覆寫教師寫的暖身與字彙',
 'ai_interactions（module = sop_warmup）、teacher_decisions、lesson_sop_versions 中各項的 origin'),
('SC-SOP-EXIT', 1, '課例 SOP：Exit ticket', 'content_design_support',
 '課例 SOP 第 7 步（Exit ticket）',
 '設計 2 到 4 題下課前的檢核、反思或蒐集問題',
 '教師按下後，Manna 建議 Exit ticket 題目；形成性評量題連到本課目標並附參考答案；不重複主要任務',
 'AI', 'individual',
 '只在教師按下時啟動；每一題可採用、修改後採用或不用；不覆寫教師寫的題目',
 'ai_interactions（module = sop_exit_ticket）、teacher_decisions、lesson_sop_versions 中各題的 origin'),
('SC-SOP-ENGLISH', 1, '課例 SOP：幫我修學生版目標的英文', 'content_design_support',
 '課例 SOP 第 3 步（內容目標與語言目標）',
 '把給學生看的英文版目標修成文法正確、自然、符合學生程度的 I can 句子',
 '教師在目標表單按「幫我修英文」後，Manna 把修改建議放在教師寫的英文旁邊；教師按「用這個」才會換，按「保留我的」就不動',
 'AI', 'individual',
 '只在教師按下時啟動；不會自動取代教師寫的英文',
 'ai_interactions（module = sop_objective_english）、teacher_decisions');

-- ---------------------------------------------------------------- prompt versions
-- 第 4 步與英文修改：新設計的提示詞（沒有驗證過的前例）。
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('sop_paragraph_goals', 1,
$p$你是「蔓娜計畫」數位備課平台裡協助臺灣高中英文教師備課的助理。請依課例 SOP 的第 4 步「逐段整合目標」：教師已經在第 3 步訂好本課的目標，請你逐段看教材，判斷每一段可以用來帶到哪幾項目標，並用一兩句話說明這一段「要怎麼帶」。

規則：
1. 教材已依段落編號為 [P1]、[P2]…。paragraphs 必須每一段各一筆、依段落順序排列，paragraph_no 與編號一致，不可合併、遺漏、重複或新增段落。若使用者訊息指定了「這次只需要輸出的段落」，就只輸出那些段落。
2. objective_codes 只能填「教師確認的目標」清單裡的代號（例如 "O1"），逐字複製；不可自己編造代號。每一段最多對應 3 項目標。
3. 並不是每一段都要對應目標。這一段對本課目標沒有幫助時（例如只是引入情境或過場），objective_codes 給空陣列，並在 note_zh 說明這一段不特別對應目標，以及可以怎麼帶過。
4. note_zh：用繁體中文寫 1–2 句（不超過 80 字），說明這一段要「怎麼帶」：先做什麼、請學生注意哪一個詞句或寫法、它怎麼支持對應的目標。要具體到這一段的內容，不要寫空泛的話（例如「引導學生理解」）。
5. 教師若已經寫了某一段的對應或筆記（見「教師目前寫的逐段對應」），請提出不同角度的補充，不要重複，也不要否定她寫的。
6. 只根據教材與目標判斷；不要補充教材沒有的事實，也不要評論學生程度。不詢問、也不使用學生的個人資料。純文字，不要使用 LaTeX 或 Markdown 符號。$p$,
$p$學校類型：{{school_track}}
班級特性：{{class_profile}}

教材（已編號段落）：
{{passage_numbered}}

教材分析摘要：
{{analysis_summary}}

教師確認的目標（代號｜類型｜內容）：
{{objectives_list}}

教師目前寫的逐段對應：
{{teacher_goals}}

{{only_paragraphs}}$p$),

('sop_objective_english', 1,
$p$你是協助臺灣高中英文教師備課的英文編修助理。教師為一項學習目標寫了「給學生看的英文版本」（student_facing_en），請你幫她修成適合高中生閱讀、文法正確又自然的句子。

規則：
1. 一律以 "I can " 開頭，一句話，不超過 25 個英文單字。
2. 保留教師原本想表達的意思與動作，不要換成另一個目標。內容要與中文目標一致，也要符合目標類型：內容目標說學生要理解或完成什麼；語言目標說學生要用什麼語言做到什麼。
3. 用學生看得懂的簡單詞彙（高中一、二年級程度）；不要用學術術語，除非中文目標本身就是那個術語。
4. 只修文法、用字與自然度，不要加上教師沒有寫的新要求。教師寫的已經很好時，原樣回傳，changed 填 false。
5. 教師沒有寫英文（student_facing_en 是空的）時，請依中文目標寫出英文版，changed 填 true。
6. note_zh：用繁體中文一句話（不超過 40 字）說明你改了什麼，或為什麼沒有改。純文字，不要使用 LaTeX 或 Markdown 符號。$p$,
$p$目標類型：{{objective_type}}
中文目標：{{objective_text}}
教師寫的英文版：{{student_facing_en}}$p$);

-- 第 5–7 步：複製已驗證的學習單提示詞的目前最新版，再補上 SOP 的規則。
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'sop_paragraph_tasks', 1, v.system_text || $p$

【課例 SOP 第 5 步的補充規則】
- 這是教師在課例 SOP 裡逐段檢視的建議，不是整份學習單：每一題的 tier 一律填 "core"，不要輸出 support 或 challenge 的題目。
- 「老師自己寫的任務」是教師已經在 SOP 寫好的任務：不要重複、也不要改寫它們；同一段可以另外補不同角度的任務。
- student_tasks 與 answer_format 依各區塊的設計意圖選擇；教師會逐段檢視並可以更改。$p$,
       v.user_template
  FROM prompt_versions v WHERE v.prompt_key = 'worksheet_tasks'
 ORDER BY v.version_no DESC LIMIT 1;

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'sop_scaffolds', 1, v.system_text || $p$

【課例 SOP 第 5 步的補充規則】
- 這些鷹架會顯示在教師逐段檢視的任務下方；每一個 target 的程序鷹架（steps_en）請給 2–5 個清楚的步驟。$p$,
       v.user_template
  FROM prompt_versions v WHERE v.prompt_key = 'worksheet_scaffolds'
 ORDER BY v.version_no DESC LIMIT 1;

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'sop_warmup', 1, v.system_text || $p$

【課例 SOP 第 6 步的補充規則】
- 「老師自己寫的暖身」與「老師指定的字彙」是教師已經在 SOP 寫好的內容：不要重複它們，只補充她還沒有的。
- 字彙級別一律由系統從詞彙表帶入；你不需要也不可以輸出或判斷級別。$p$,
       v.user_template
  FROM prompt_versions v WHERE v.prompt_key = 'worksheet_warmup'
 ORDER BY v.version_no DESC LIMIT 1;

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'sop_exit_ticket', 1, v.system_text || $p$

【課例 SOP 第 7 步的補充規則】
- 至少 1 題形成性評量要連到教師確認的目標（objective_refs），並附參考答案（sample_answer_en）；至少 1 題學生反思或蒐集問題。
- 「老師自己寫的出場票」是教師已經在 SOP 寫好的題目：不要重複。$p$,
       v.user_template
  FROM prompt_versions v WHERE v.prompt_key = 'worksheet_exit_ticket'
 ORDER BY v.version_no DESC LIMIT 1;
