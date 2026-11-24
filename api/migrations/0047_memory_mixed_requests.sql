-- Manna DLS: a lasting habit said together with a one-off edit is still offered.
--
-- In 「改這一區」 a teacher who mentions a habit almost always adds what to change this time
-- ("之後選擇題都用三個選項，這一區的單字題改成選擇題"). v2 of `teacher_memory_extract` said
-- 「多項要求 → supported=false」, so the model counted the one-off edit as a second request and
-- refused the whole sentence (reason multiple_habits): the habit was never offered. A one-off
-- request is now ignored when it comes with one lasting habit; only two or more lasting habits
-- are multiple_habits.
--
-- New version: the current latest text (whatever it is: the admin page adds versions too) with
-- rule 2 narrowed and one section appended. The user message needs no change. Older versions
-- stay.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       replace(v.system_text, '2. 一次只處理一項。多項要求、', '2. 一次只處理一項。兩項以上長期習慣、') || $p$

同時有長期習慣與單次要求：
- 教師常在同一段話裡說一項長期習慣，再說這一次要怎麼改（例如「之後選擇題都用三個選項，這一區的單字題改成選擇題」「以後都附中文解釋，這題先改簡單一點」）。針對這一區、這一題、這一份、這一次的單次要求不算第二項習慣：忽略單次要求，只整理那一項長期習慣，persistent=true，可以記住時 supported=true、reason=none。
- 只有出現兩項以上的長期習慣（例如「之後選擇題都用三個選項，而且每份都附中文詞彙表」）才是 multiple_habits。
- 整段話只有單次要求、沒有長期習慣時，persistent=false、reason=one_time。$p$,
       v.user_template,
       '混合句：長期習慣夾帶單次修改時只整理那一項習慣；兩項以上長期習慣才 multiple_habits。'
  FROM prompt_versions v
 WHERE v.prompt_key = 'teacher_memory_extract'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
