-- Manna DLS: 「改這一區」 covers the whole block, scaffolds and word table included.
--
-- A teacher asked three times for 「不要單字表格」 on a block that printed one; each time Manna
-- answered that the block had no word table and changed nothing. The block text sent to the
-- model listed only the title, the instructions and the questions, and the output schema had
-- no place for anything else, so the model honestly saw no table. The server now lists every
-- part of the block with a code (S1… scaffolds, W1… rows of the word table, G1… Glossary
-- entries) in `{{block_detail}}`, and the schema gets three arrays (`scaffolds`, `words`,
-- `glossary`) whose entries say `keep`, `change` or `remove`. The code checks them: a word
-- added to the table must be in the material, its level comes from the word list, the hint of
-- an 指示用語 is the code's, and a reply that says the block has no X while it has X is sent back.
--
-- New version of `worksheet_block_revise`: the current latest text (whatever it is: the admin
-- page and other migrations add versions too) with one section appended to the system text. The
-- user message needs no change: the parts are in `{{block_detail}}`. Older versions stay.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

鷹架與字詞表（scaffolds、words、glossary）：
- 「這一區目前的內容」除了題目，還會列出學生版這一區印出來的其他部分，每一項都有代號：鷹架 S1、S2…（程序鷹架的步驟、句型框架、引導問題、作答示例等）、字詞表 W1、W2…（學生版的 Word / Meaning / 級別 表格，每一列一個字）、Glossary G1、G2…。教師說的「單字表格」「單字表」「字詞表」就是 W 開頭的字詞表；「鷹架」「步驟」「句型框架」是 S 開頭的鷹架。內容裡寫「（沒有）」才表示這一區真的沒有；有列出來的就是有。
- scaffolds、words、glossary 的每一項都寫 source（原本的代號；新增的寫 "new"）與 action：keep（不改）、change（改）、remove（刪除）。沒有列出的項目平台一律保留，所以只需要列出要改、要刪或要新增的項目，不改的可以不寫；沒有要動的就給空陣列。
- 教師要拿掉字詞表（例如「不要單字表格」），就把 W 開頭的每一列都列出並寫 action: remove；要拿掉鷹架（例如「不要步驟」「不要句型框架」）同理，把對應的 S 項目寫 remove；要拿掉 Glossary 就把 G 項目寫 remove。教師要求的是刪掉，就要真的寫出 remove，不要改問教師。
- 修改鷹架：程序鷹架改 steps（每步一句，1 到 12 步，content 寫空字串），其他鷹架改 content（steps 寫空陣列）；kind 照原樣寫。新增鷹架時 action 寫 change、source 寫 "new"，kind 只能是 句型框架、引導問題、作答示例 或 清楚的分步指示及完成順序。句型框架用 ___ 留空，放在句型框架鷹架裡，不要寫成一題。
- 修改字詞表只能改 meaning（解釋，簡短、用學生看得懂的字）；標了「指示用語」的列，解釋由平台決定，不要改。新增字詞時 word 要寫教材或這一區的文字裡真的出現的英文字（不要編造），meaning 寫解釋；級別由平台查詞表，不用寫。Glossary 只能 keep 或 remove，不能新增或改寫。
- 「可以修改的範圍」不是整個區塊時，scaffolds、words、glossary 都給空陣列。
- reply 要和實際內容一致：只有「這一區目前的內容」裡真的沒有某個部分，才可以說這一區沒有它；列出來的部分不可以說不存在。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_block_revise'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

UPDATE scaffolds
   SET support_form = support_form || '；教師也可以請 Manna 改、刪除或新增這一區的鷹架與字詞表（程序鷹架、句型框架、字詞表、Glossary），一樣先給教師看，採用後才寫入'
 WHERE scaffold_code IN ('SC-WS-BLOCK-REVISE') AND version = 1;
