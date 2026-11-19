-- Manna DLS: a 填空題 always has a blank to write in (docs/API.md「填空題的空格」).
--
-- A teacher got a 填空題 whose prompt was the full sentences, with the answer's phrases listed
-- apart as its 參考答案: the student had no blank to write in (a 填空題 prints no answer lines). The
-- server now checks every generated 填空題 for a blank (「______」, 「___」, 「( )」, 「（　）」): one
-- without fails `items_wellformed` and goes through the repair call; if it still has none, the
-- server blanks the answer's phrases out of the prompt, or makes it a 簡答題.
--
-- Only some prompts said how a 填空題 is written, and none that the blank must be there and the
-- answer must not. Each item-writing prompt gets a new version: its current latest text with one
-- section appended, so the rest stays exactly what it was (the version number is the next free
-- one: the admin page also adds versions). Stored worksheets need no change.

-- ---------------------------------------------------------------- worksheet_tasks (完整學習單)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

填空題（answer_format 為「填空題」的題目）：
- prompt_en 裡一定要有至少一個空格 "______"，放在答案原本的位置；學生就在空格裡寫答案，填空題不會另外印作答線。
- 答案的字詞不可以留在 prompt_en 裡：不要把完整的句子或段落抄進 prompt_en、再另外在 answer_en 列出答案。
- answer_en 依空格的順序寫出要填入的字詞（好幾個空格時用逗號分開）。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tasks'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_exit_ticket
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

填空題（answer_format 為「填空題」的題目）：
- prompt_en 裡一定要有至少一個空格 "______"，放在答案原本的位置；學生就在空格裡寫答案，填空題不會另外印作答線。
- 答案的字詞不可以留在 prompt_en 裡：不要把完整的句子抄進 prompt_en、再另外在 sample_answer_en 列出答案。
- sample_answer_en 依空格的順序寫出要填入的字詞（好幾個空格時用逗號分開）。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_exit_ticket'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- the prompts that write `prompt` / `answer`
-- 照版型產生, 閱讀理解題, 差異化學習單, 逐區修改 and the prep session's 修改.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

填空題（answer_format 為「填空題」的題目）：
- prompt 裡一定要有至少一個空格 "______"，放在答案原本的位置；學生就在空格裡寫答案，填空題不會另外印作答線。
- 答案的字詞不可以留在 prompt 裡：不要把完整的句子或段落抄進 prompt、再另外在 answer 列出答案。
- answer 依空格的順序寫出要填入的字詞（好幾個空格時用逗號分開）。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_template_section', 'worksheet_bloom_level',
                        'worksheet_block_revise', 'prep_revise')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_tiered_level (差異化學習單)
-- Its answers are per version.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

填空題（answer_format 為「填空題」的題目）：
- prompt 裡一定要有至少一個空格 "______"，放在答案原本的位置；學生就在空格裡寫答案，填空題不會另外印作答線。
- 答案的字詞不可以留在 prompt 裡：不要把完整的句子或段落抄進 prompt、再另外在各版本的 answer 列出答案。
- 各版本的 answer 依空格的順序寫出要填入的字詞（好幾個空格時用逗號分開）。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tiered_level'
 ORDER BY v.version_no DESC
 LIMIT 1;
