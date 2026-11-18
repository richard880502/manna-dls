-- Manna DLS: 圖像組織 drawn by the server (docs/API.md「圖像組織」; SIOP Q2 教學鷹架, A4).
--
-- A question whose answer format is 圖像組織 now carries an `organizer`: one of eight fixed types
-- (t_chart, venn, sequence, cause_effect, problem_solution, story_map, kwl, mind_map), its
-- labels, the answer of every cell and which cells the student sheet prints pre-filled. The
-- server checks it and draws it in the editor, the DOCX and the PDF; nothing is drawn by an
-- image model. The schemas of the four prompts below get an `organizer` property (null for the
-- other formats); the new prompt versions say how to fill it.
--
-- Each new version is the current latest version of its key with one section appended, so the
-- rest of the text stays exactly what it was (the version number is the next one: the admin page
-- also adds versions). Stored worksheets need no change: a question without an organizer prints
-- an empty box as before (`content.blocks[].items[].organizer` is absent).

-- ---------------------------------------------------------------- worksheet_tasks (完整學習單)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_tasks', v.version_no + 1, v.system_text || $p$

圖像組織（answer_format 為「圖像組織」的題目）：
- organizer：平台會依你寫的內容畫出圖表（學生拿到空白或部分預填的圖表，教師版印出參考答案）。其他作答方式的題目 organizer 一律填 null。prompt_en 只寫要學生做什麼（例如 "Complete the cause-and-effect chain for paragraph 2."），不要在 prompt_en 裡畫圖或逐格列出內容。
- type 只能選下列之一（逐字使用），選最符合這一題思考方式的一種：
  t_chart：T 字表，兩欄比較或分類；sections 正好 2 個，label 是兩欄標題。
  venn：文氏圖，比較兩者異同；sections 正好 3 個，依序是「只屬於 A」「兩者都有」「只屬於 B」，第 1、3 個 label 寫 A、B 的名稱，第 2 個寫 "Both"。
  sequence：順序鏈，事件或步驟的先後；sections 3–6 個，label 例如 "First"、"Next"、"Then"、"Finally"。
  cause_effect：因果鏈；sections 2–6 個，依因果順序，label 寫 "Cause" 或 "Effect"。
  problem_solution：問題與解決；sections 2–4 個，依序是 "Problem"、"Solution"（可以 1–2 個）、"Result"。
  story_map：故事地圖，只用在記敘文；sections 正好 5 個，依序是 "Setting"、"Characters"、"Problem"、"Events"、"Solution"。
  kwl：KWL 表，讀前讀後；sections 正好 3 個，依序是 "What I Know"、"What I Want to Know"、"What I Learned"。
  mind_map：心智圖；center 寫中心詞，sections 3–6 個，每個是一個分支，label 是分支標題。
- title：圖表上方的英文標題，不需要時給空字串；center：只有 mind_map 要寫，其他類型給空字串。
- cells：每一格是一個陣列元素（不要把好幾格寫在同一個字串裡），每個 section 1–5 格；表格類（t_chart、kwl、venn）每欄 2–4 格，順序鏈、因果鏈、問題與解決每格 1 格。answer 寫這一格的參考答案（一個片語或一句短句，只印在教師版）；學生自己的想法（例如 kwl 的前兩欄）answer 給空字串。prefilled 為 true 時，這一格的答案會直接印在學生版當作示範；至少要留一半的格子給學生寫，通常只預填第一格或不預填。
- label、title、center、answer 一律用英文，簡短，不要換行。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tasks'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_bloom_level (閱讀理解題)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_bloom_level', v.version_no + 1, v.system_text || $p$

圖像組織（只在「只能用的作答方式」有「圖像組織」時，每一題才會有 organizer 欄位）：
- organizer：answer_format 為「圖像組織」的題目要寫；其他題目一律填 null。平台會依你寫的內容畫出圖表（學生拿到空白或部分預填的圖表，教師版印出參考答案）。prompt 只寫要學生做什麼（例如 "Complete the cause-and-effect chain."），不要在 prompt 裡畫圖或逐格列出內容。
- type 只能選下列之一（逐字使用），選最符合這一層次思考方式的一種：
  t_chart：T 字表，兩欄比較或分類；sections 正好 2 個，label 是兩欄標題。
  venn：文氏圖，比較兩者異同；sections 正好 3 個，依序是「只屬於 A」「兩者都有」「只屬於 B」，第 1、3 個 label 寫 A、B 的名稱，第 2 個寫 "Both"。
  sequence：順序鏈；sections 3–6 個，label 例如 "First"、"Next"、"Then"、"Finally"。
  cause_effect：因果鏈；sections 2–6 個，依因果順序，label 寫 "Cause" 或 "Effect"。
  problem_solution：問題與解決；sections 2–4 個，依序是 "Problem"、"Solution"、"Result"。
  story_map：故事地圖，只用在記敘文；sections 正好 5 個，依序是 "Setting"、"Characters"、"Problem"、"Events"、"Solution"。
  kwl：KWL 表；sections 正好 3 個，依序是 "What I Know"、"What I Want to Know"、"What I Learned"。
  mind_map：心智圖；center 寫中心詞，sections 3–6 個，每個是一個分支。
- title：圖表上方的英文標題，不需要時給空字串；center：只有 mind_map 要寫，其他類型給空字串。
- cells：每一格一個陣列元素，每個 section 1–5 格。answer 寫這一格的參考答案（一個片語或一句短句，只印在教師版）；學生自己的想法 answer 給空字串。prefilled 為 true 時這一格的答案直接印在學生版當作示範；至少要留一半的格子給學生寫。
- 所有文字用英文、簡短、不要換行。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_bloom_level'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_tiered_level (差異化學習單)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_tiered_level', v.version_no + 1, v.system_text || $p$

圖像組織（只在「只能用的作答方式」有「圖像組織」時，每一題才會有 organizer 欄位）：
- organizer：answer_format 為「圖像組織」的題目要寫；其他題目一律填 null。三個版本用同一張圖表（同樣的類型與格子），差別只在預填的格子多寡。prompt 只寫要學生做什麼，不要在 prompt 裡畫圖或逐格列出內容。
- type 只能選下列之一（逐字使用）：
  t_chart：T 字表；sections 正好 2 個，label 是兩欄標題。
  venn：文氏圖；sections 正好 3 個，依序是「只屬於 A」「兩者都有」「只屬於 B」，第 2 個 label 寫 "Both"。
  sequence：順序鏈；sections 3–6 個，label 例如 "First"、"Next"、"Then"、"Finally"。
  cause_effect：因果鏈；sections 2–6 個，label 寫 "Cause" 或 "Effect"。
  problem_solution：問題與解決；sections 2–4 個，依序是 "Problem"、"Solution"、"Result"。
  story_map：故事地圖，只用在記敘文；sections 正好 5 個，依序是 "Setting"、"Characters"、"Problem"、"Events"、"Solution"。
  kwl：KWL 表；sections 正好 3 個，依序是 "What I Know"、"What I Want to Know"、"What I Learned"。
  mind_map：心智圖；center 寫中心詞，sections 3–6 個，每個是一個分支。
- title：英文標題，不需要時給空字串；center：只有 mind_map 要寫。
- cells：每一格一個陣列元素，每個 section 1–5 格。answer 寫這一格的參考答案（一個片語或一句短句，只印在教師版）。prefill_in 決定哪些版本的學生版直接印出這一格的答案當作示範：none（都不預填）、basic（只有基礎版）、basic_standard（基礎版與一般版）、all（三個版本都預填）。基礎版預填的格子最多，進階版最少（通常不預填，最多一格）；每個版本都要留至少一半的格子給學生寫。
- 所有文字用英文、簡短、不要換行。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_tiered_level'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_template_section (照版型)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_template_section', v.version_no + 1, v.system_text || $p$
圖像組織（輸出的題目有 organizer 欄位時）：answer_format 為「圖像組織」的題目要寫 organizer，其他題目填 null。平台會依你寫的內容畫出圖表，prompt 只寫要學生做什麼。type 只能是 t_chart（兩欄，sections 2 個）、venn（sections 3 個：只屬於 A、兩者都有、只屬於 B）、sequence（3–6 個）、cause_effect（2–6 個）、problem_solution（2–4 個：Problem、Solution、Result）、story_map（5 個：Setting、Characters、Problem、Events、Solution）、kwl（3 個：What I Know、What I Want to Know、What I Learned）、mind_map（center 寫中心詞，3–6 個分支）之一；版型的題型樣式畫的是哪一種圖表，就選最接近的一種。cells 每一格一個陣列元素，answer 是參考答案（只印在教師版），prefilled 為 true 的格子會直接印在學生版當作示範，至少要留一半的格子給學生寫。文字依版型的 instruction_language，簡短，不要換行。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_section'
 ORDER BY v.version_no DESC
 LIMIT 1;
