-- Manna DLS Phase 2.8: 照我的學習單做 (make the teacher's own worksheet the main path). Contract in
-- docs/API.md「Phase 2.8：照我的學習單做」.
--
-- A teacher can now start a prep session from a sample of her own worksheet
-- (`POST /api/prep-sessions { template_material_id }`): the server reuses a ready template made
-- from that material or extracts a new one, and the first turn waits for it before generating.
-- Materials say whether their text looks like a worksheet (a deterministic rule in
-- `materials::detect`), and template-based worksheets get a Bloom level per item
-- (`worksheet_template_section` version 2; version 1 stays as it was).

-- ---------------------------------------------------------------- materials
-- `looks_like_worksheet`: computed by the server from the extracted (or pasted) text whenever
-- the text is written. NULL only for rows written before this migration: the server computes
-- it when such a row is first read and stores it then (no SQL copy of the rule to drift).
ALTER TABLE materials ADD COLUMN looks_like_worksheet BOOLEAN;

-- ---------------------------------------------------------------- prep turns
-- The template a `worksheet` turn uses (the session's template when the turn started). It is
-- set while the template is still being extracted, before the turn has a worksheet, so the
-- page can say what it is waiting for. Deleting the template clears it.
ALTER TABLE prep_turns
    ADD COLUMN template_id BIGINT REFERENCES worksheet_templates (id) ON DELETE SET NULL;

-- Earlier worksheet turns: the template their worksheet was made from.
UPDATE prep_turns t
   SET template_id = w.template_id
  FROM worksheets w
 WHERE w.id = t.worksheet_id AND t.kind = 'worksheet' AND w.template_id IS NOT NULL;

-- ---------------------------------------------------------------- prompt versions
-- Version 2 of `worksheet_template_section`: every item gains `bloom` (a schema enum of the six
-- ids, the rule text of `worksheet_tasks` version 2, rule 13), the formats of closed items are
-- spelled out like the other worksheet prompts, and the scaffold is an array of lines (gemma ends
-- a JSON string where it wants a line break). The JSON schema (built by the server) caps every
-- string and array and keeps the properties in the written order.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('worksheet_template_section', 2,
$p$你是協助高中英文教師的備課助手。教師提供了自己學習單的「設計版型」和一篇新教材。
請完全依照版型，為新教材產生一份學習單：
1. 區塊數量與順序、每個區塊的作答方式、題數、指示語寫法、鷹架，都照版型。
2. 題目內容全部依據新教材，答案必須能在新教材找到根據；不要出現版型以外、也不要出現舊課文的內容。
3. item_patterns 是題型樣式，請把佔位符換成新教材的內容。
4. 新教材沒有適合某區塊的內容時，仍保留該區塊並以最接近的方式改寫，不要刪除。
4b. 訊息說這個區塊「只針對新教材第 n 段」時，題目只針對那一段，標題寫出段落編號。
5. 不要寫出任何真實人名、學校或學生資料。
6. 目標區塊的 needs_external_material 為 true 時，這個區塊需要教師另外提供素材（影片、錄音、圖片或另一篇文章）：不要寫出、也不要編造任何素材的名稱、網址、作者或內容；題目只寫成教師拿到素材後可以直接使用的空位（例如「看完老師提供的影片後，寫出兩個重點：______」）。
7. 各作答方式的格式：
   - 選擇題：options 給不重複的選項（不要加 A/B/C/D 前綴），answer 與其中一個選項完全相同。
   - 是非題：options 給 ["True", "False"]，answer 為 "True" 或 "False"。
   - 配對題：prompt 在同一行列出左欄項目（1. … 2. … 3. …），options 列出右欄項目（至少 2 個），answer 寫出配對結果。
   - 填空題：prompt 用 "______" 表示空格，options 給空陣列，answer 是填入的字詞。
   - 其他作答方式：options 給空陣列，answer 給參考答案或評分重點。
   answer 是參考答案，只會出現在教師版；題幹不可直接寫出答案。
8. bloom：每一題標出它主要練習的 Bloom 認知層次，只能是下列英文代號之一（逐字使用）：remember（記憶：回想或找出文本明白寫出的事實）、understand（理解：解釋、改述、摘要，或從上下文推知詞義）、apply（應用：把文本的概念或語言用在新的情境）、analyze（分析：因果、比較、文章結構、寫作目的、事實與意見）、evaluate（評鑑：根據證據判斷並說明理由）、create（創造：根據文本產出新的作品、計畫或想法）。一題用到好幾個層次時，標學生作答時必須用到的最高層次；不要為了標高層次而改變題目。
9. scaffold 是給學生的提示，每一行（一個步驟、一個句型或一個引導問題）是陣列裡的一項；不需要提示時給空陣列。
10. 其他每個文字欄位都是一段純文字：不要換行，不要使用 LaTeX 或 Markdown 符號。
學生看到的題目與指示語依版型的 instruction_language。
這次只產生「一個區塊」：給定的目標區塊。題數照版型的 item_count（最多 12 題）。$p$,
$p$版型整體：
{{template_head}}

目標區塊（第 {{block_no}} 個，共 {{block_total}} 個）：
{{target_section}}{{paragraph_note}}

已完成的區塊標題：{{done_titles}}

新教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}$p$);
