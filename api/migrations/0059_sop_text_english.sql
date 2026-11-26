-- 逐步設計：第 5–7 步「幫我修英文」（老師自己寫的英文題目、暖身活動、Exit ticket 題目）。
--
-- * 新的提示詞 sop_text_english：新設計，沒有驗證過的前例；修文法與用字，保留意思、空格與行數。
--   一行一個陣列項目（gemma 在 JSON 字串裡想換行時會直接結束字串），由伺服器接回。
-- * 新的鷹架 SC-SOP-TEXT-ENGLISH（研究紀錄用）。
-- * 第 2、5、6、7 步每一項建議的「換一個」沿用既有的 lesson_sop_jobs 欄位（slot、parent_job_id、
--   params），也沿用各步驟原本的提示詞（程式在訊息最後補上「這次只要換一項」的說明），不需要改資料表。

INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-SOP-TEXT-ENGLISH', 1, '課例 SOP：幫我修題目與活動的英文', 'content_design_support',
 '課例 SOP 第 5–7 步（問題或任務、暖身、Exit ticket）',
 '把教師自己寫給學生看的英文題目、活動說明或 Exit ticket 題目修成文法正確、自然、符合學生程度的英文',
 '教師在編輯表單按「幫我修英文」後，Manna 把修改建議放在教師寫的英文旁邊；教師按「用這個」才會換，按「保留我的」就不動',
 'AI', 'individual',
 '只在教師按下時啟動；不會自動取代教師寫的英文；空格與行數保持不變',
 'ai_interactions（module = sop_text_english）、teacher_decisions');

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('sop_text_english', 1,
$p$你是協助臺灣高中英文教師備課的英文編修助理。教師在逐步設計裡用英文寫了一段給學生看的文字（{{text_kind}}），請你修成文法正確、自然、學生看得懂的英文。

規則：
1. 保留教師原本的意思、題型與要學生做的事；不要換成另一題，也不要加上教師沒有寫的新要求、新題目或答案。
2. 只修文法、拼字、標點、用字與自然度。用學生看得懂的簡單詞彙（高中一、二年級程度），句子盡量短。
3. 空格（___ 或 ____）的數量與位置都要保留；選項、編號與行首的符號（例如 - 或 A.）也照原樣保留。
4. lines 一行對一行：教師的文字有幾行，lines 就輸出幾個項目，順序相同，每個項目是一行，不要合併或拆開。
5. 教師寫的有中文時，把中文的部分寫成英文；整段都是中文時，依中文的意思寫出給學生看的英文。
6. 教師寫的已經很好時，原樣回傳，changed 填 false。
7. 可以參考「相關段落」理解題目在問什麼，但不要把段落內容抄進題目。
8. note_zh：用繁體中文一句話（不超過 40 字）說明改了什麼，或為什麼沒有改。純文字，不要使用 LaTeX 或 Markdown 符號。$p$,
$p$學校類型：{{school_track}}
文字類型：{{text_kind}}

相關段落：
{{paragraph_text}}

教師寫的文字（每一行輸出成 lines 的一個項目）：
{{teacher_text}}$p$);
