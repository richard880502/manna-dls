-- Manna DLS: a generated 表格整理 table says what it compares.
--
-- A teacher-researcher read a generated question "Complete a T-chart with two columns:
-- Unconventional Path as a Positive Choice and Speaker Justifying His Decision. For each
-- interpretation, record its claim and one supporting detail." whose printed table showed the
-- headings 「Column 1 | Column 2」 over empty rows: 「Column 需要直接先建議，row 也需要，不然不知道要比什麼」.
-- The model had never been asked for the table: it wrote the columns in the question's words,
-- and the sheet printed blank headings.
--
-- Every question now has a nullable `table` (`columns`: 2–6 headings, `row_labels`: 0–8 labels
-- printed as a first column), after its `organizer` in the output schemas of the five prompts
-- below (the schema is code, `worksheet::table`). The server repairs a placeholder heading
-- (「Column 1」, empty) from the headings the question names, and asks the model again when it
-- cannot. The new prompt versions tell the model how to fill it.
--
-- Each new version is the current latest version of its key (whatever it is: the admin page and
-- other migrations add versions too) with one section appended to the system text; the user
-- message needs no change. Older versions stay, and stored worksheets need no change: a table
-- without `row_labels` prints as before, and one with 「Column 1」 headings shows the teacher a
-- prompt to fill them in.

-- ---------------------------------------------------------------- worksheet_tasks (完整學習單)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

表格整理（answer_format 為「表格整理」的題目）：
- table：平台會把它印成表格：最上面一列是欄名，第一欄是列標籤，其餘是留給學生寫的空格。其他作答方式的題目 table 一律填 null。
- columns：2–6 個欄名，寫出這張表要比較或記錄的事物。例如比較兩件事的 T 字表："Unconventional Path as a Positive Choice"、"Speaker Justifying His Decision"。欄名用英文、簡短（不超過 8 個字），一定要寫具體的內容；不可以留空，也不可以寫 "Column 1"、"Column 2"、"Header" 這類占位字。
- row_labels：1–8 個列標籤，寫出每一列要學生寫什麼或比較什麼（例如 "Claim"、"Supporting detail"），會印在第一欄，學生才知道每一列要比什麼。每個標籤都不同，不可以寫 "Row 1"。只有在表格有 3 欄以上、每一列是獨立的一筆資料、欄名已經說明每一格要寫什麼（例如 Who / What happened / Why）時，row_labels 才可以是空陣列；只有 2 欄的表格一定要寫列標籤。
- prompt_en 要說明在比什麼，欄名要和 table.columns 一致（例如 "Complete a T-chart with two columns: A and B."）；欄名與列標籤裡不要寫答案。answer_en 照欄名與列標籤逐格寫出參考答案。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tasks'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- sop_paragraph_tasks (逐步設計第 5 步)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

表格整理（answer_format 為「表格整理」的題目）：
- table：平台會把它印成表格：最上面一列是欄名，第一欄是列標籤，其餘是留給學生寫的空格。其他作答方式的題目 table 一律填 null。
- columns：2–6 個欄名，寫出這張表要比較或記錄的事物。例如比較兩件事的 T 字表："Unconventional Path as a Positive Choice"、"Speaker Justifying His Decision"。欄名用英文、簡短（不超過 8 個字），一定要寫具體的內容；不可以留空，也不可以寫 "Column 1"、"Column 2"、"Header" 這類占位字。
- row_labels：1–8 個列標籤，寫出每一列要學生寫什麼或比較什麼（例如 "Claim"、"Supporting detail"），會印在第一欄，學生才知道每一列要比什麼。每個標籤都不同，不可以寫 "Row 1"。只有在表格有 3 欄以上、每一列是獨立的一筆資料、欄名已經說明每一格要寫什麼（例如 Who / What happened / Why）時，row_labels 才可以是空陣列；只有 2 欄的表格一定要寫列標籤。
- prompt_en 要說明在比什麼，欄名要和 table.columns 一致（例如 "Complete a T-chart with two columns: A and B."）；欄名與列標籤裡不要寫答案。answer_en 照欄名與列標籤逐格寫出參考答案。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'sop_paragraph_tasks'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_bloom_level (閱讀理解題)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

表格整理（只在「只能用的作答方式」有「表格整理」時，每一題才會有 table 欄位）：
- table：answer_format 為「表格整理」的題目要寫；其他題目一律填 null。平台會把它印成表格：最上面一列是欄名，第一欄是列標籤，其餘是留給學生寫的空格。
- columns：2–6 個欄名，寫出這張表要比較或記錄的事物。例如比較兩件事的 T 字表："Unconventional Path as a Positive Choice"、"Speaker Justifying His Decision"。欄名用英文、簡短（不超過 8 個字），一定要寫具體的內容；不可以留空，也不可以寫 "Column 1"、"Column 2"、"Header" 這類占位字。
- row_labels：1–8 個列標籤，寫出每一列要學生寫什麼或比較什麼（例如 "Claim"、"Supporting detail"），會印在第一欄，學生才知道每一列要比什麼。每個標籤都不同，不可以寫 "Row 1"。只有在表格有 3 欄以上、每一列是獨立的一筆資料、欄名已經說明每一格要寫什麼（例如 Who / What happened / Why）時，row_labels 才可以是空陣列；只有 2 欄的表格一定要寫列標籤。
- prompt 要說明在比什麼，欄名要和 table.columns 一致（例如 "Complete a T-chart with two columns: A and B."）；欄名與列標籤裡不要寫答案。answer 照欄名與列標籤逐格寫出參考答案。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_bloom_level'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_tiered_level (差異化學習單)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

表格整理（只在「只能用的作答方式」有「表格整理」時，每一題才會有 table 欄位）：
- table：answer_format 為「表格整理」的題目要寫；其他題目一律填 null。三個版本用同一張表格（同樣的欄名與列標籤），差別只在支持與答案。平台會把它印成表格：最上面一列是欄名，第一欄是列標籤，其餘是留給學生寫的空格。
- columns：2–6 個欄名，寫出這張表要比較或記錄的事物。例如比較兩件事的 T 字表："Unconventional Path as a Positive Choice"、"Speaker Justifying His Decision"。欄名用英文、簡短（不超過 8 個字），一定要寫具體的內容；不可以留空，也不可以寫 "Column 1"、"Column 2"、"Header" 這類占位字。
- row_labels：1–8 個列標籤，寫出每一列要學生寫什麼或比較什麼（例如 "Claim"、"Supporting detail"），會印在第一欄，學生才知道每一列要比什麼。每個標籤都不同，不可以寫 "Row 1"。只有在表格有 3 欄以上、每一列是獨立的一筆資料、欄名已經說明每一格要寫什麼（例如 Who / What happened / Why）時，row_labels 才可以是空陣列；只有 2 欄的表格一定要寫列標籤。
- prompt 要說明在比什麼，欄名要和 table.columns 一致（例如 "Complete a T-chart with two columns: A and B."）；欄名與列標籤裡不要寫答案。各版本的 answer 照欄名與列標籤逐格寫出參考答案。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tiered_level'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_template_section (照版型)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

表格整理（輸出的題目有 table 欄位時）：answer_format 為「表格整理」的題目要寫 table，其他題目填 null。平台會把它印成表格：最上面一列是欄名，第一欄是列標籤，其餘是留給學生寫的空格。
- columns：2–6 個欄名，寫出這張表要比較或記錄的事物（例如 "Unconventional Path as a Positive Choice"、"Speaker Justifying His Decision"），簡短、具體；不可以留空，也不可以寫 "Column 1"、"Column 2"、"Header" 這類占位字。欄名的語言和題目一致。
- row_labels：1–8 個列標籤，寫出每一列要學生寫什麼或比較什麼（例如 "Claim"、"Supporting detail"），會印在第一欄，學生才知道每一列要比什麼；每個標籤都不同，不可以寫 "Row 1"。只有表格有 3 欄以上、每一列是獨立的一筆資料而欄名已經說明每一格要寫什麼時，才可以給空陣列；只有 2 欄的表格一定要寫列標籤。
- prompt 要說明在比什麼，欄名要和 table.columns 一致；欄名與列標籤裡不要寫答案。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_section'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
