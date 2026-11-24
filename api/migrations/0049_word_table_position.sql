-- Manna DLS: where the word table (字詞表) is printed is the teacher's own setting, not something
-- 「改這一區」 can do.
--
-- A teacher asked 「改這一區」 to put the word table at the end of the worksheet; Manna answered
-- that it could only change this block and asked whether she meant adding a table here or moving
-- another block's table, which left her with no way forward (she deleted the table, and could not
-- bring it back). The worksheet now has a setting 「字詞表放在學習單最後」 (every block's table is
-- printed once, after the last block) and a block whose table was taken out has 「恢復字詞表」. The
-- model is told about both, so that it points the teacher to them instead of asking.
--
-- New version of `worksheet_block_revise`: the current latest text (whatever it is: the admin
-- page and other migrations add versions too) with one section appended to the system text.
-- Older versions stay.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

字詞表的位置與恢復：
- 字詞表印在每一區裡，還是集中印在學習單最後面，是老師在學習單上方自己設定的（勾選「字詞表放在學習單最後」），不是你能搬動的；刪掉的字詞表，老師可以在那一區按「恢復字詞表」找回來。
- 老師要把字詞表移到最後面、移到別的區塊，或問怎麼把刪掉的字詞表找回來時：題目、鷹架、words、glossary 全部 keep（不要為了「移動」去新增或刪除任何一列），reply 用一兩句話直接告訴她：到學習單上方勾選「字詞表放在學習單最後」，所有區塊的字詞表就會一起印在最後面；刪掉的字詞表，到那一區按「恢復字詞表」。不要反問她是要新增還是要移動。
- 老師要「新增」字詞或要「刪除」字詞表時，照前面「鷹架與字詞表」的規則處理。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_block_revise'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
