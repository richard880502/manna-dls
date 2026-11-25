-- Manna DLS: a block's instruction only introduces it; how to work is in the steps, which the
-- sheet introduces with 「Follow the steps below.」.
--
-- 1. The warm-up's instruction introduces the activity instead of repeating its steps.
--
-- The PI's feedback (0050_per_question_frames.sql) made a block's instruction a short
-- introduction and left the how-to to the procedural scaffold printed under it. That change
-- covered the activity blocks (worksheet_tasks, sop_paragraph_tasks) but not the warm-up, which
-- is written by its own step: a worksheet made after 0050 still printed
--   "Read the title … Spend two minutes writing brief answers, and then share one prediction
--    with a partner."
-- right above the steps "1. Read the title and imagine a scene. 2. Write one idea … 3. Work
-- with a partner. 4. Share one idea with the class." The PI: the instruction becomes an
-- introduction to the activity, beginning "In this activity, you will …"; the steps stay as
-- they are.
--
-- 2. An activity block's instruction (0050) still ended with a how-to sentence: 「The first three
--    stanzas show the speaker examining two roads … The class will reread the lines and examine
--    important clues together.」 The PI: keep only the introduction; the how-to belongs in the
--    steps, and the sheet prints 「Follow the steps below.」 above them (code, not the model:
--    `assemble::STEPS_LEAD`).
--
-- Each new version is the current latest text with one section appended (the version number is
-- the next free one). Older versions stay; old worksheets print as before.

-- ---------------------------------------------------------------- worksheet_warmup, sop_warmup
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

暖身活動的作答指示（background_activities 的 instructions_en）是這個活動的簡短介紹，不是作答步驟：
- 一律以「In this activity, you will」開頭，1–2 句簡單的英文，說明學生在這個活動會想什麼、做什麼，例如 "In this activity, you will look at the title and imagine what kind of choice the poem is about." 或 "In this activity, you will think about a time you had to make a hard choice."
- 作答步驟（看標題、寫下想法、和同伴討論、和全班分享）由系統另外印成程序鷹架的步驟，就在指示下面。instructions_en 不要重複這些步驟，不要寫 First… then…、Step 1、Spend two minutes…、share with a partner 這類做法說明。
- 要回答的問題放在 prompts_en，不要寫進 instructions_en。
- 系統會在步驟上面自動印「Follow the steps below.」，instructions_en 不要寫這句。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_warmup', 'sop_warmup')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_tasks, sop_paragraph_tasks
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

區塊的作答指示（instructions_en）只寫這一區課文的介紹：
- 1–2 句簡單的英文，說這幾段在談什麼、學生會讀到什麼，例如 "The first three stanzas show the speaker examining two roads, trying to choose, and questioning whether he can return."
- 不要寫學生或全班要怎麼做，例如 "The class will reread the lines and examine important clues together."、"Work with a partner …"、"Watch your teacher answer question 1 first."、"On your own, …"。怎麼做（誰和誰一起、先後順序、老師示範）是下一步程序鷹架的步驟。
- 系統會在步驟上面自動印「Follow the steps below.」，instructions_en 不要寫這句。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_tasks', 'sop_paragraph_tasks')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_block_revise
-- 改這一區 on the warm-up keeps the same shape: an introduction, not the steps.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

- 暖身區塊（Warm-up）的作答指示是這個活動的簡短介紹，以「In this activity, you will」開頭，說明學生會想什麼、做什麼；不要重複程序鷹架的步驟。
- 其他區塊的作答指示只寫這一區課文的介紹（這幾段在談什麼），不寫學生或全班要怎麼做（The class will…、Work with a partner…、Watch your teacher…）；怎麼做寫在程序鷹架的步驟。
- 系統會在步驟上面自動印「Follow the steps below.」，作答指示不要寫這句。教師要求改指示時照這個樣子寫；教師沒有要求改指示時不要動它。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_block_revise'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
