-- Manna DLS: a sentence frame is not a question of its own (docs/API.md「句型框架不是題目」).
--
-- A teacher found a language scaffold printed as question 20 of a student sheet: 「20. Hi! In the
-- 3rd paragraph, the author describes ______. After reading it carefully, I identified ______ as
-- one of the main characteristics of narrative writing, as seen in the text where ______.」 The
-- exit ticket prompt asks for a question that makes students use the lesson's language (「用指定的
-- 句型或字詞寫一句」) but its schema has no place for a frame, so the model wrote the frame itself
-- as the question. The server now sends such a question back (`items_wellformed`) and, if the
-- repair call writes it again, prints the frame in the block's 「Sentence frames」 box, un-numbered.
--
-- The prompts that write questions without a place for a frame say so: the exit ticket and the
-- tasks (whose frames come from the scaffolds step), and the two revisions (whose frames go at
-- the end of the question they support). None of them may open a question with a greeting. Each
-- new version is the current latest text with one section appended (the version number is the
-- next free one: the admin page also adds versions). Stored worksheets need no change.

-- ---------------------------------------------------------------- worksheet_exit_ticket
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

句型框架不是題目：
- 檢核語言目標的題目，prompt_en 也要寫出完整的題目或指示，說清楚要學生做什麼（例如 "Use because to explain why Mrs. Lin started the stand."）。
- 不要只寫一個句型框架（例如 "I think ___ because ___."、"In paragraph 3, the author describes ___."）當作一題；學習單會把這樣的題目退回。
- 題目不要用 Hi!、Hello! 這類招呼語開頭。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_exit_ticket'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_tasks (完整學習單)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

句型框架不是題目：
- 每一題的 prompt_en 都要寫出要學生做什麼（問題或指示）。不要把句型框架（例如 "I think ___ because ___."）單獨寫成一題；句型框架由下一步的鷹架另外提供，印在題目外。
- 題目不要用 Hi!、Hello! 這類招呼語開頭。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tasks'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- 逐區修改 and the prep session's 修改
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

句型框架不是題目：
- 要加句型框架或句首時，寫在它要幫助的那一題 prompt 的最後（例如 "… Start like this: I think ___ because ___."），不要另外寫成一題。每一題的 prompt 都要寫出要學生做什麼。
- 題目不要用 Hi!、Hello! 這類招呼語開頭。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_block_revise', 'prep_revise')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
