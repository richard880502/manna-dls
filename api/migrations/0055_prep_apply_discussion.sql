-- Manna DLS: what a teacher worked out in the prep conversation goes back into the worksheet.
--
-- A teacher used 問 Manna to work out a two-interpretation comparison table, asked how to put it
-- back into the worksheet above, and got a text table to paste into Word. Two gaps:
--  1. 問 Manna was shown the conversation so far (`recent_turns`); 改學習單 was not, so
--     「把剛才討論的放進 C 區」 had nothing to refer to. `prep_revise` now gets the same lines.
--  2. A revision could not write a 表格整理 question's table (column headings and row labels, see
--     0051_table_organizer_headers.sql). Its schema (code) now has `table` on every item, checked
--     like a generated table (no 「Column 1」 headings, English only).
--
-- The new version is the current latest text of `prep_revise` (whatever it is: the admin page and
-- other migrations add versions too) with one section appended. Older versions stay.

-- ---------------------------------------------------------------- prep_revise
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

先前的對話與表格整理：
- 訊息最後的「這段對話先前的內容」是備課對話最近幾次的往來：教師問了什麼，以及每次的結果（包含「問 Manna」的回答與先前的修改）。教師說「剛才」「上面」「我們討論的」「那個表」時，指的就是那裡面的結果：直接取用，不要再問一次，也不要自己另想一個。教師沒有提到先前的內容時，不要把它放進學習單。
- 先前的內容只列出每次結果的前面一段。如果教師要放進去的內容不完整（例如表格被截掉），changes 給空陣列，reply 請教師把內容貼在對話裡，不要自己補編。
- 只改教師要求的區塊與題目，其他照原樣抄回。把討論的結果放進某一區時，用原本區塊的 title 與 instructions，加上或替換教師指定的題目，不要動其他題。這份學習單的規則照舊：學生看到的題目與指示語維持原本的語言（原本是英文就用英文），不要把 Chinese 的討論文字直接放給學生。
- 表格整理（answer_format 為「表格整理」的題目）：table 寫出這張表要比較的事物，平台會印成最上面一列欄名、第一欄列標籤，其餘是留給學生寫的空格。其他作答方式的題目 table 一律填 null。columns 是 2–6 個英文欄名（簡短、不超過 8 個字、寫具體內容，不可以留空，也不可以寫 "Column 1"、"Header" 這類占位字）；row_labels 是 1–8 個英文列標籤（例如 "Claim"、"Supporting detail"，每個都不同，不可以寫 "Row 1"），只有 2 欄的表格一定要寫列標籤。prompt 要說明在比什麼，欄名要和 table.columns 一致（例如 "Complete a T-chart with two columns: A and B."）；欄名與列標籤裡不要寫答案；answer 照欄名與列標籤逐格寫出參考答案。教師討論出來的比較表就這樣放進去。沒有要改的表格整理題，table 照原樣抄回。$p$,
       v.user_template || $p$

這段對話先前的內容：
{{recent_turns}}$p$
  FROM prompt_versions v
 WHERE v.prompt_key = 'prep_revise'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
