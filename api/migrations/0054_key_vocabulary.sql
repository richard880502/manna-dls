-- Manna DLS: the word table (字詞表) becomes 「Key Vocabulary」 (the PI's redesign).
--
--  1. The table is titled Key Vocabulary and is split in two columns: Content (the key words of
--     the text) and Academic (the task words of the instructions). Each row shows the word, its
--     part of speech and its meaning.
--  2. Meanings are English by default for both columns; one switch per worksheet
--     (`vocabulary_language` in the content, next to 「字詞表放在學習單最後」) prints the Chinese
--     ones. A row keeps both, so switching back and forth loses neither.
--  3. The 「Word bank」 list printed under the table repeated the same words; the projection
--     leaves a 字詞表 scaffold out when the table already has all its words.
--
-- Everything stored is in the worksheet's JSON content (`Vocab.pos`, `Vocab.meaning_zh`,
-- `Content.vocabulary_language`, all optional), so there is no table to change: content made
-- before reads as it did (a task word's Chinese hint, stored as its `definition`, is moved to
-- `meaning_zh` when the content is read). What this migration adds are prompt versions.
--
-- Each new version is the current latest text with one section appended (the version number is
-- the next free one: the admin page and other migrations add versions too). Older versions stay.

-- ---------------------------------------------------------------- worksheet_warmup, sop_warmup
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

關鍵字的詞性與中文意思（pos、meaning_zh）：
- key_vocabulary 的每個字，除了 student_friendly_definition_en，還要寫兩個欄位，都會印在學習單的 Key Vocabulary 字詞表上：
  - pos：這個字在 context_sentence 裡的詞性，只能用這幾個之一：n.、v.、adj.、adv.、prep.、conj.、pron.、det.、aux.、interj.、phr.（片語用 phr.）。候選字彙那一行如果在最後列了「詞彙表詞性」，pos 必須是其中之一；平台會檢查，不符的會改用詞彙表的詞性。
  - meaning_zh：繁體中文，這個字在 context_sentence 這個語境裡的簡短意思（2 到 12 個字），只寫這一個語境的意思，不要列多個字義，不要寫英文。
- student_friendly_definition_en 的規則不變（簡單英文、不要用中文、不要用該字本身）。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_warmup', 'sop_warmup')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_scaffolds, sop_scaffolds
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

字詞表鷹架不要重複 Key Vocabulary：
- 學習單上已經有「Key Vocabulary」字詞表，列出本課的關鍵字、詞性與意思。不要再做一個「字詞表」（word bank）鷹架去重複列出這些關鍵字；字詞表鷹架只在要列出「Key Vocabulary 裡沒有、但作答時用得到的字」時才寫，而且只列那些字。沒有這樣的字就不要做字詞表鷹架。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_scaffolds', 'sop_scaffolds')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_block_revise
-- The word bank (the 字詞表 scaffold) lists its words with their part of speech; the teacher
-- ticks the words whose meaning also prints and picks the language of those meanings.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

字詞表鷹架（Word bank）與 Key Vocabulary：
- 學習單上的「Key Vocabulary」就是這一區的字詞表（words，W 開頭的代號）；字詞表鷹架（kind 為「字詞表」）只列出 Key Vocabulary 裡沒有的字，字都在 Key Vocabulary 裡的字詞表鷹架不會印出來。
- 字詞表鷹架的每個字都會印詞性（例如 plain (adj.)）。「這一區目前的內容」會列出每個字的詞性、英文與中文字義，以及老師有沒有勾選「印出字義」。要增減字，就改這個鷹架的 content（用逗號隔開的字，不要寫詞性或字義）；留下的字會保留原有的詞性與字義，新加的字由平台補上詞性。
- 哪些字要印出字義、字義用英文還是中文，是老師在字詞表鷹架上自己勾選的，不是你能改的；老師要求時，reply 告訴她到那個字詞表鷹架勾選即可，不要為此改 content。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_block_revise'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
