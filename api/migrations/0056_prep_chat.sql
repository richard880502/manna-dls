-- Manna DLS: the tool-based prep chat (docs/PREP_CHAT.md).
--
--  1. A new turn kind `chat`: one box for questions and edits. The model answers and, when the
--     teacher asks for a change, calls tools that edit a working copy of the worksheet; the turn
--     writes ONE new version at the end. `ask` and `revise` stay valid kinds so earlier turns
--     still load, but no new ones are made.
--  2. `prep_turns.chat_worksheet_id` / `chat_undo`: the worksheet a chat turn worked on, and what
--     is needed to take the whole turn back (the blocks as they were and as they were left).
--  3. `ai_interactions.usage`: tokens of a call as the provider reports them
--     (`{prompt_tokens, completion_tokens, cached_tokens}`); only the chat records it so far.
--  4. Prompt `prep_chat` version 1 and its scaffold row.

ALTER TABLE prep_turns DROP CONSTRAINT prep_turns_kind_check;
ALTER TABLE prep_turns ADD CONSTRAINT prep_turns_kind_check CHECK (kind IN
    ('worksheet', 'assessment', 'activity', 'resources', 'ask', 'revise',
     'comprehension', 'tiered', 'kit', 'chat'));

ALTER TABLE prep_turns
    ADD COLUMN chat_worksheet_id BIGINT REFERENCES worksheets (id) ON DELETE SET NULL,
    ADD COLUMN chat_undo         JSONB;

ALTER TABLE ai_interactions ADD COLUMN usage JSONB;

INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-PREP-CHAT', 1, '備課對話：問問題或用工具修改學習單', 'teacher_control_fading',
 '備課對話（對話中提問或修改學習單）',
 '回答教師備課時的問題，並在教師要求時修改學習單的區塊、鷹架、字詞表與圖片',
 'Manna 在同一輪裡讀學習單、呼叫工具修改（每個修改都經過學習單的固定檢查，沒通過的原因會回給模型修正），結束時一次寫成一個新版本，並列出實際改了什麼',
 'AI', 'individual',
 '教師可以一鍵復原整輪的修改（沒有被再改過的區塊）；每個區塊也可以在編輯器單獨復原',
 'ai_interactions（module = prep_chat，每次模型呼叫一列，含 cached tokens）、usage_logs（prep_chat_tool、prep_chat_turn、prep_chat_undo）、worksheet_versions（cause = revise）、worksheet_block_history');

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('prep_chat', 1,
$p$你是協助台灣高中英文教師備課的助手 Manna，在備課對話裡和老師說話。老師可以問問題、要求修改學習單，或兩者一起。每一輪你會先看到：教材（每段前有 [第 n 段] 標記）、學習單目前的內容、班級特性、先前的對話，最後是老師這一輪說的話。

怎麼做：
1. 老師只是在問問題：直接回答，不要呼叫工具。回答要扣緊這一課的教材與班級，具體、簡短；需要時可以引用段落（「第 2 段」）。
2. 老師要求修改學習單：用工具修改，不要只用文字說「可以這樣改」。先用 read_block 看完整內容（答案、鷹架全文、字詞表、圖）再改；學習單裡 block_id 才是工具要用的代號，老師說的「C 區」是印在學生版上的字母，對照學習單內容裡每一區的「印出的字母」。
3. 工具回傳「沒有改成」時，會列出沒通過檢查的原因：照原因修正後再呼叫一次；同一區同一個工具最多重試 2 次，仍然不行就放棄，並在回答裡老實說明沒改成。不要宣稱已經改好但實際沒有成功。
4. 工具一次只做一件事。edit_block 的 items 要把這一區「完整的題目清單」寫出來（沒要改的題目照原樣抄回去，包含它的表格），不是只寫有改的題目。
5. 題目規則：作答方式只能用固定的種類；題幹不要直接寫出答案；英文題目與選項只用英文；「表格整理」題要寫真正的欄位標題與列標籤（不要用 Column 1）；教材以外的內容不要編造，不要出現真實人名、網址或影片名稱。
6. 老師沒有要求的地方不要動。不要改教材原文、學習目標，也不要重新產生整份學習單（那些請老師按對話裡的按鈕）。
7. 圖片：只有「重畫」可以用 redraw_picture；每份學習單的重畫次數有上限，用完了就告訴老師可以上傳自己的圖。這個環境沒有開啟產圖時，不要呼叫它。
8. 工具次數有上限（一輪最多 8 次）。用最少的呼叫完成老師的要求。

最後的回答（工具都做完、或老師只是問問題時）：用繁體中文，分成幾個短段落（paragraphs，每段最多 400 字；可以用 1. 2. 條列），不要用 Markdown 符號（例如 **、#）。如果有改學習單，簡短說明改了什麼（平台會另外列出實際的變更清單，所以不必逐字重述）；沒改成的要說。follow_ups 最多 3 個「老師接下來可能想問或想做」的短句（每個 40 字以內），沒有就給空陣列。$p$,
$p$班級特性：{{class_profile}}

教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}

學習單「{{worksheet_title}}」目前的內容（每一區的 block_id、印出的字母、題目；完整內容與答案用 read_block 看）：

{{worksheet_compact}}

{{picture_rules}}

作答方式的可選種類：{{answer_format_labels}}$p$);
