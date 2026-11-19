-- Manna DLS: a template keeps the sample's labelled support boxes (docs/API.md「提示框」).
--
-- A teacher made a worksheet with 照我的學習單做 from her own sample, which has, in every paragraph
-- block, a 「Directions」 part (imperative steps) and a 「Sentence Frame for Sharing」 part (an oral
-- sharing frame that opens with "Hi! In Paragraph 1, the author describes …"). On the printed sheet
-- the sharing frame was numbered as a question in every block (「Write a short analysis paragraph
-- using the frame: Hi! …」, 「Hi! …」), and the box headed 「Sentence frames」 held the directions.
-- The extraction had no place for a support box with its own heading, so it read the sharing frame
-- as the fourth question pattern, and a block printed one scaffold under a fixed heading.
--
-- Templates now keep each labelled box of the sample (`supports`: heading as written, role, pattern;
-- the server builds the schemas), and a block prints each box under the sample's heading. The new
-- versions say so: the outline does not list a box as a block, the details write the boxes and keep
-- frames out of the question patterns, and a template block writes each box apart (directions are
-- steps without blanks; frames have blanks; a sharing frame may open with Hi! and is never a
-- question). The same versions (and the other question generators) say a 是非題 is a statement to
-- judge, not a question or an opinion, and that one sheet uses one name for one idea. The scaffolds
-- step writes directions as steps, not inside a sentence frame. Each new version is the current
-- latest text with one section appended (the version number is the next free one: the admin page
-- also adds versions). Stored templates and worksheets need no change.

-- ---------------------------------------------------------------- worksheet_template_outline
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$
8. 區塊裡有自己標題的提示框（例如 Directions、Sentence Frame for Sharing、Sentence Frames、Sentence Starters、Word Bank、Hints）屬於它所在的區塊，不是一個區塊，不要另外列進 outline。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_outline'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_template_details
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

提示框（supports）：
- 原學習單在區塊裡印出、有自己標題的提示框（例如 "Directions"、"Sentence Frame for Sharing"、"Sentence Frames"、"Word Bank"、"Hints"），每一個是 supports 的一項，依原本出現的順序：
  - heading：原學習單上的標題，逐字照抄（不要翻譯、改寫或加上編號）。原本沒有標題的提示框，用最貼近它用途的英文標題。
  - role：directions（作答步驟：簡短的祈使句，沒有空格）、sentence_frame（句型框架：有空格 _____ 讓學生照著寫的句子）、sharing_frame（口頭分享用的句型：常以 Hi!、Hello! 開頭，有空格）、word_bank（字詞表）、hints（提示與追問）、guiding_questions（引導問題）、example（作答示例）。
  - pattern：這個提示框內容的通用樣式，課文專屬的字詞換成佔位符，保留空格 _____（例如「Hi! In Paragraph [n], the author describes _____. …」）。
- 句型框架（有空格要學生完成的句子，包括以 Hi!、Hello! 開頭、口頭分享用的句子）一律是提示框，不是題目：不要寫進 item_patterns，也不要算進 item_count。原學習單把它印在題號旁時也一樣。
- item_patterns 只寫有題號、要學生作答的題目；item_count 只算這些題目。
- scaffolds 要包含提示框對應的鷹架種類：directions → 清楚的分步指示及完成順序；sentence_frame、sharing_frame → 句型框架；word_bank → 字詞表；hints → 提示與追問；guiding_questions → 引導問題；example → 作答示例。
- 沒有提示框的區塊，supports 給空陣列。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_details'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_template_section
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

提示框（目標區塊有 supports 時，輸出 supports，沒有 scaffold）：
- supports 是原學習單在這一區印出的提示框。輸出的 supports 每個提示框一項，順序與版型相同；heading 照版型逐字使用（不要翻譯或改寫），lines 每一行一項，照 role 與 pattern 為新教材改寫：
  - directions：簡短的祈使句步驟（例如 "Underline the topic sentence."），一行一步；不要有空格 ______，不要寫句型框架，也不要寫「Use this frame:」。
  - sentence_frame：有空格 ______ 讓學生照著寫的句子；只寫句型本身，不要寫步驟或指示。
  - sharing_frame：口頭分享用的句型，可以照 pattern 用 Hi! 開頭，要有空格 ______；改成這一段的內容。
  - word_bank：只列字詞，用逗號分開。hints：簡短的提示。guiding_questions：引導思考的問題。example：一個作答示例（用不同的例子，不要給出答案）。
- 提示框不是題目：句型框架與分享句型（包括 Hi!、Hello! 開頭的句子）只寫在 supports，不要寫成 items 裡的一題，也不要包成「Write … using the frame: …」「Complete the analysis paragraph: …」這樣的題目。版型的 item_patterns 裡如果有這樣的句型，也寫在提示框，不要寫成題目。items 只放有題號、要學生作答的題目。
- 目標區塊沒有 supports 時照舊寫 scaffold：scaffold 只寫版型第一個鷹架種類的內容；那是句型框架時，只寫有空格的句子，作答步驟寫在 instructions。句型框架或 Hi! 開頭的分享句型一樣不可以寫成題目。

是非題（answer_format 為「是非題」）：
- prompt 寫成一句可以依課文判斷對錯的直述句（例如 "The second sentence of Paragraph 1 is the topic sentence."），不要寫成問句，也不要問學生的看法（例如 "Do you think this sentence serves as a topic sentence?"）。
- 要問學生的看法時，改用簡答題，請學生回答 Yes 或 No 並說明理由。

用語一致：
- 同一個概念在整份學習單只用一個說法。版型（或新教材）用哪個詞，就一直用那個詞，不要換成同義詞（例如版型用 informative writing，就不要寫 expository writing）。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_template_section'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_scaffolds (完整學習單的鷹架)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

句型框架與作答步驟分開：
- 「句型框架」的 content_en 只寫有空格 ___ 讓學生照著寫的句子，不要寫步驟或指示（例如 "Identify the topic sentence."），也不要寫「Use this frame:」。
- 作答步驟（祈使句）寫在程序鷹架的 steps_en。學習單會把句型框架裡的步驟退回。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_scaffolds'
 ORDER BY v.version_no DESC
 LIMIT 1;

-- ---------------------------------------------------------------- worksheet_tasks, worksheet_exit_ticket
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

是非題與包在題目裡的句型框架：
- 是非題的 prompt_en 寫成一句可以依課文判斷對錯的直述句（例如 "The second sentence of Paragraph 1 is the topic sentence."），不要寫成問句，也不要問學生的看法（例如 "Do you think …?"）；要問看法時改用簡答題，請學生回答 Yes 或 No 並說明理由。
- 不要把句型框架包在題目裡（例如 "Write a short paragraph using the frame: Hi! In Paragraph 2, the author describes ___."）；學習單會把這樣的題目退回。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_tasks', 'worksheet_exit_ticket')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

-- ---------------------------------------------------------------- worksheet_bloom_level (閱讀理解題)
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

是非題（answer_format 為「是非題」）：prompt 寫成一句可以依課文判斷對錯的直述句（例如 "The second sentence of Paragraph 1 is the topic sentence."），不要寫成問句，也不要問學生的看法（例如 "Do you think …?"）；要問看法時改用簡答題，請學生回答 Yes 或 No 並說明理由。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'worksheet_bloom_level'
 ORDER BY v.version_no DESC
 LIMIT 1;
