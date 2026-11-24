-- Manna DLS: sentence frames and guiding prompts go under the question they help, and a block's
-- instruction introduces the passage instead of repeating the steps.
--
-- The PI's feedback on generated worksheets:
--  1. A sentence frame is there so the student can answer each question below it, so it should
--     look like 「Hello, everyone. My answer to the question is ___ because ___. Thank you.」, a
--     little paragraph the student can say aloud, written for that question and printed under
--     it, not one box of unrelated starters at the top of the block.
--  2. The block's instruction ("Let's use a three-step method: …") did the same job as the
--     procedural scaffold printed right under it (steps 1–5). The procedural scaffold stays
--     for the how-to; the instruction becomes a short introduction to the paragraphs the block
--     covers.
--  3. A box of six standalone 引導問題 looked like six more questions. Each guiding prompt now
--     goes under the question it serves, printed as a hint (「Hint:」), never numbered.
--
-- The scaffolds step's schema gets `item_frames` and `item_hints` (the question's number in the
-- block and the text); the server places them on the questions, checks them (English, a ___
-- blank in a frame, no reference answer) and prints them under each question. A block revision
-- may write a question's `frame` too. Old worksheets have no frames and print as before.
--
-- Each new version is the current latest text with one section appended (the version number is
-- the next free one: the admin page and other migrations add versions too). Older versions stay.

-- ---------------------------------------------------------------- worksheet_scaffolds
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

句型框架與引導問題要放在題目底下（item_frames、item_hints）：
- 句型框架不要再寫成區塊最上面的一個方框（verbal 不要再給「句型框架」）。改成為「需要學生用自己的話口說或寫出答案」的題目（簡答題、段落寫作）各寫一個，放在 item_frames：item_no 是這一題在區塊裡的題號（區塊內容裡列的 1、2…），frame_en 是專為這一題寫的一小段話，學生看著就可以完整說出來，例如 "Hello, everyone. My answer to the question is ___ because ___. Thank you."。frame_en 要有招呼、帶理由的回答（用 ___ 留空，至少一個）、結尾；要貼著這一題的內容（例如 "Hello, everyone. In my opinion, the poet feels ___ because the line '___' shows ___. Thank you."），不要每一題都寫一樣的句子。選擇題、是非題、配對題、填空題、表格整理、圖像組織不需要句型框架，除非教師的勾選要求。frame_en 不可寫出參考答案，不可寫步驟或指示（步驟寫在程序鷹架），只用英文。教師勾選的鷹架種類沒有句型框架時，item_frames 給空陣列。
- 引導問題不要再寫成一整排獨立的問題放在區塊最上面（verbal 不要再給「引導問題」）。改成幫助某一題的提示，放在 item_hints：item_no 是它幫助的題號，hint_en 是一兩句提示，提醒學生往哪裡找或先想什麼（例如 "Look at the second stanza. What word repeats?"），不可編號、不可是另一道要學生另外作答的題目、不可給答案。教師勾選的鷹架種類沒有引導問題時，item_hints 給空陣列。
- 暖身（warm_up）的 item_frames 與 item_hints 給空陣列。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_scaffolds'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- sop_scaffolds
-- Step 5 of the lesson SOP shows one text per kind of scaffold under each task: no frames or
-- hints of its own there.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

【逐題句型框架與提示】
- 這一步的鷹架顯示在整段任務下方，不是在每一題下方：item_frames 與 item_hints 一律給空陣列，語言鷹架照上面的規則寫。引導問題只寫幾個幫助學生回答任務的提示，不要寫成另一組要學生作答的問題。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'sop_scaffolds'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_tasks, sop_paragraph_tasks
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

區塊的作答指示（instructions_en）是這一區課文的簡短介紹，不是作答步驟：
- 作答步驟（先找線索、再推論、再說明…）由下一步的程序鷹架另外印在指示下面。instructions_en 不要重複這些步驟，也不要寫「Let's use a three-step method」、「First… then…」、「Step 1」這類方法說明，不要叫學生「比較答案」「照下面的步驟做」。
- 改寫成一小段介紹這一區涵蓋的段落：這幾段在談什麼，學生會讀到或做到什麼。例如 "Paragraphs 4–6 are all about how the town reacts to the news. You will find three short scenes, which show how each person feels."（1–2 句，英文，讀得懂的簡單句子）。
- 題目的 prompt_en 仍要寫出要學生做什麼；句型框架與提示由下一步的鷹架逐題提供，印在每一題的下面，所以不要把句型框架或提示寫進 prompt_en。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_tasks', 'sop_paragraph_tasks')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_block_revise
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

逐題的句型框架與區塊的作答指示：
- 每一題如果有句型框架，「這一區目前的內容」會在那一題下面列出「句型框架（印在這一題下面）」。句型框架是專為那一題寫的一小段學生可以照著說的話，例如 "Hello, everyone. My answer to the question is ___ because ___. Thank you."：有招呼、帶理由的回答（用 ___ 留空，至少一個）、結尾，只用英文，不可寫出參考答案。
- 每一題的 frame 欄位：空字串＝不動（保留目前的句型框架；沒有的就沒有）；寫 "(none)"＝拿掉這一題的句型框架；寫一段新的＝這一題從現在起的句型框架（題目改了、教師要求改句型框架、或教師要求替簡答題或段落寫作加句型框架時才寫）。改了題目但句型框架還適用就留空；keep 為 true 的題目 frame 留空。選擇題、是非題、配對題等不需要句型框架。教師說「句型框架」時，先看題目底下有沒有列出的句型框架，再看有沒有 S 開頭的句型框架鷹架。
- 這一區的作答指示（instructions）是這一區課文的簡短介紹（這幾段在談什麼、學生會讀到或做到什麼），不要重複程序鷹架的步驟；教師沒有要求改指示時不要動它。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_block_revise'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

UPDATE scaffolds
   SET support_form = support_form || '；句型框架與引導問題依題目逐題給：句型框架是專為那一題寫的一小段可以照著說的話（招呼、帶理由的回答、結尾），引導問題是題目下面的提示，不再集中成一個方框；區塊的作答指示改為這一區課文的簡短介紹，不重複程序鷹架的步驟'
 WHERE scaffold_code IN ('SC-WS-SCAFFOLDS') AND version = 1;
