-- Manna DLS: a learning objective is not a question (docs/API.md「學習目標不是題目」).
--
-- A worksheet made with 照我的學習單做 (a poem, a descriptive-writing reading template) printed,
-- in its first block, two 填空題 that were the teacher's two objectives with one word blanked
-- ("Complete the content-goal self-assessment statement with the key idea: I can explain how the
-- two roads symbolize life ______."), under the sheet's own 學習目標, which print the answers.
-- The chain:
-- * the template's analysis merged the sample's objectives header (Content Objective / Language
--   Objective, "I will …") into its first section and labelled that section 自我評量, although the
--   sample has no self-assessment at all;
-- * the section prompt never said what 自我評量 is, nor that the objectives (which reach it as
--   「學習目標（教師寫的）」) are not material for questions; 自我評量 with 填空題 asks for a right
--   answer a self-assessment cannot have, and the model blanked the objectives;
-- * the check sent the bare frames back, and the repair call put a lead in front of them.
--
-- The new versions: the outline says an objectives header is not a block (shows_objectives), the
-- details that it belongs to no section and when a section really is 自我評量; the section prompt
-- and every prompt that writes questions with the objectives in front of it say the objectives
-- are not questions, blanks, options or answers, and what a 自我評量 is. The server now refuses a
-- question that copies six words or more of an objective in a row when the objective gives its
-- answer away (`not_a_copy_of_objective`, `worksheet::checks::copied_objective`).
--
-- Each new version is the current latest text of its key with one section appended (the version
-- number is the next free one: the admin page also adds versions); the user message needs no
-- change. Stored templates and worksheets need no change: a template read before this keeps its
-- labels until the teacher reads her sample again (the generation prompt and the check cover it
-- meanwhile).

-- ---------------------------------------------------------------- worksheet_template_outline
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$
9. 學習單最上方的學習目標（例如 Content Objective、Language Objective、Learning Objectives、Learning Goals，或以 I can …、I will …、Today I can … 開頭的句子、「學習目標」）是整份學習單的目標，不是區塊：有的話 shows_objectives 設為 true；不要把它列進 outline，也不要把它併進第一個區塊。$p$,
       v.user_template,
       '學習單最上方的學習目標不是區塊：shows_objectives 設為 true，不列進大綱，也不併進第一個區塊。'
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_outline'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_template_details
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

學習目標與自我評量：
- 學習單最上方的學習目標（Content Objective、Language Objective、Learning Objectives，或以 I can …、I will …、Today I can … 開頭的句子）不屬於任何區塊。寫第一個 section 時不要把它算進去：不要從它寫出 item_patterns，不要把它算進 item_count，也不要因為它選 student_tasks。
- student_tasks 只有在原學習單真的有自我評量時才選「自我評量」：學生自己勾選的檢核表（checklist）或 can-do 方框、1–4 分的自評量表，或寫出自己學會了什麼、還有哪裡不會的反思。複習概念或課文內容的填空題是「理解內容」或「字彙練習」，不是自我評量。$p$,
       v.user_template,
       '學習目標不屬於任何區塊；只有原學習單真的有自我評量（檢核表、can-do 方框、自評量表、反思自己的學習）才選「自我評量」。'
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_details'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_template_section (照版型)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

學習目標不是題目：
- 訊息裡的「學習目標（教師寫的）」（內容目標與語言目標）只是告訴你這一課要學生學會什麼。題目要對準它們，但不要把學習目標本身（照抄它的句子，或只挖掉、換掉幾個字）寫成題目、空格、選項或答案：例如不要把「I can explain how …」挖空成「I can explain how … ______.」，也不要在這樣的句子前面加上「Complete the self-assessment statement:」。學習單上方會印出學習目標，學生直接看得到答案，平台會把這樣的題目退回。
- 自我評量（student_tasks 的「自我評量」）是學生自己檢查學會了沒有，沒有標準答案：例如勾選 can-do 方框、用 1–4 分自評，或用一兩句話寫出自己現在會做什麼。這種題目可以引用學習目標，answer 寫 "Answers will vary."。
- 目標區塊的 student_tasks 有「自我評量」，但 answer_formats 是要有標準答案的作答方式（填空題、選擇題、是非題、配對題）時，這一區照那個作答方式出針對新教材內容的題目（例如概念或細節的填空），不要把學習目標改寫成填空。$p$,
       v.user_template,
       '學習目標不是題目（不挖空、不加 Complete the statement 開頭）；自我評量沒有標準答案；自我評量配上填空題等作答方式時改出新教材內容的題目。'
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_section'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- the other prompts that write questions
-- 完整學習單 (tasks, warm-up), 閱讀理解題, 差異化學習單, 逐區修改, and 逐步設計 steps 5 and 6: each
-- has the objectives in front of it (「教師確認的目標」, 「學習目標」 or 「學習目標（教師寫的）」).
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

學習目標不是題目：
- 學習目標（訊息中「教師確認的目標」或「學習目標」列出的內容目標與語言目標，包括「學習目標（教師寫的）」）只是告訴你這一課要學生學會什麼。題目要對準它們，但不要把學習目標本身（照抄它的句子，或只挖掉、換掉幾個字）寫成題目、空格、選項或答案：例如不要把「I can explain how …」挖空成「I can explain how … ______.」，也不要在這樣的句子前面加上「Complete the self-assessment statement:」。學習目標會印在學習單上方（或在課堂上公布），學生直接看得到答案，平台會把這樣的題目退回。
- 自我評量是學生自己檢查學會了沒有，沒有標準答案：例如勾選 can-do 方框、用 1–4 分自評，或用一兩句話寫出自己現在會做什麼。自我評量可以引用學習目標，但沒有參考答案（答案寫 "Answers will vary."）；要有標準答案的題目（填空題、選擇題、是非題、配對題）一律針對教材內容出題。$p$,
       v.user_template,
       '學習目標不是題目（不挖空成填空、不當選項或答案）；自我評量沒有標準答案。'
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_tasks', 'worksheet_warmup', 'worksheet_bloom_level',
                        'worksheet_tiered_level', 'worksheet_block_revise',
                        'sop_paragraph_tasks', 'sop_warmup')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- the exit tickets (學習單, 逐步設計第 7 步)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

學習目標不是題目：
- 學習目標（「教師確認的目標」列出的內容目標與語言目標）只是告訴你這一課要學生學會什麼。不要把學習目標本身（照抄它的句子，或只挖掉、換掉幾個字）寫成題目、空格、選項或答案：例如不要把「I can explain how …」挖空成「I can explain how … ______.」，也不要在這樣的句子前面加上「Complete the self-assessment statement:」。學習目標會印在學習單上方（或在課堂上公布），學生直接看得到答案，平台會把這樣的題目退回。
- 「檢核目標」的意思是出一題能看出學生有沒有達成目標的題目（例如要學生用課文內容說明、舉例，或用指定的語言寫一句），不是把目標句挖空或改成選項。
- 自我評量是學生自己檢查學會了沒有，沒有標準答案：學生反思的題目可以請學生依學習目標自評（例如 "Rate yourself from 1 to 4: …"），但形成性評量的題目一律針對教材內容出題。$p$,
       v.user_template,
       '學習目標不是題目；檢核目標是出能看出是否達成的題目，不是把目標句挖空；自我評量沒有標準答案。'
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_exit_ticket', 'sop_exit_ticket')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
