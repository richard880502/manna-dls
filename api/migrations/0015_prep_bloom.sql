-- Manna DLS: 備課對話 meets the Bloom engine. Contract in docs/API.md「Phase 2.6：備課對話」
-- (the additions marked Phase 2.7) and「Phase 2.7」.
--
-- Three new turn kinds: `comprehension` (閱讀理解題) and `tiered` (分層練習) make a worksheet with
-- that output; `kit` (教學套件) makes an activity plan and a tiered worksheet together. A session
-- keeps the teacher's 出題選項 (`options`) and passes them into every worksheet it makes. The
-- activity plan tags every step with the Bloom levels it works on (`prep_activity` version 2;
-- version 1 stays as it was).

-- ---------------------------------------------------------------- turn kinds
ALTER TABLE prep_turns DROP CONSTRAINT prep_turns_kind_check;
ALTER TABLE prep_turns ADD CONSTRAINT prep_turns_kind_check CHECK (kind IN
    ('worksheet', 'assessment', 'activity', 'resources', 'ask', 'revise',
     'comprehension', 'tiered', 'kit'));

-- ---------------------------------------------------------------- session options
-- `{ bloom?, question_count?, bloom_counts?, answer_formats?, objective? }`, validated by the
-- server with the worksheet settings' rules; absent keys are not set.
ALTER TABLE prep_sessions ADD COLUMN options JSONB NOT NULL DEFAULT '{}'::jsonb;

-- ---------------------------------------------------------------- prompt versions
-- Version 2 of `prep_activity`: every step gains `bloom` (1–3 ids, a schema enum; the server
-- keeps at most 3, in Bloom order), and the teacher's objective and request reach the model.
-- The JSON schema (built by the server) caps every string and array: the gateway enforces
-- maxLength / maxItems while decoding, which stops runaways. Text that needs several lines is an
-- array (gemma ends a JSON string where it wants a line break).
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('prep_activity', 2,
$p$你是協助台灣高中英文教師備課的助手。教師提供了這一課的教材（每段前有 [第 n 段] 標記）。請為這一課規劃一節課（約 50 分鐘）的教學活動流程，給教師參考。

規則：
1. title：這份教學活動規劃的名稱（繁體中文，可以夾英文課名）。
2. goals：1–4 個學習目標，寫成「學生能……」的繁體中文句子，具體、可以觀察，並且來自這篇教材的內容與語言重點。教師有寫「學習目標（教師寫的）」時，第一個目標要對準它。curriculum_codes 只能從「可對應的課綱條目」清單裡選代碼，每個目標最多 3 個；沒有合適的就給空陣列，不要自己編代碼。
3. steps：3–6 個教學步驟，依上課順序排列。phase 只能是「引起動機」「發展活動」「統整與評量」三者之一：第一步是「引起動機」，最後一步是「統整與評量」，中間是「發展活動」。
4. 每個步驟：
   - title：簡短的步驟名稱。
   - minutes：建議分鐘數，所有步驟加起來大約 50 分鐘。
   - idea：用繁體中文寫教學思路（做什麼、為什麼這樣做、學生怎麼參與），要具體指出用到教材的哪一段或哪些內容。
   - bloom：這個步驟主要讓學生練習的 Bloom 認知層次，1–3 個，只能用下列英文代號（逐字使用）：remember（記憶：找出文本明白寫出的事實）、understand（理解：解釋、改述、摘要，或從上下文推知詞義）、apply（應用：把文本的概念或語言用在新的情境）、analyze（分析：因果、比較、文章結構、寫作目的、事實與意見）、evaluate（評鑑：根據證據判斷並說明理由）、create（創造：根據文本產出新的作品、計畫或想法）。依學生在這一步實際要做的事來標，不要為了標高層次而多標。整節課的層次通常由低到高。
   - script：老師上課時可以直接說的英文課堂用語（classroom English），每行一句，最多 6 行。
   - stuck_tip：用繁體中文寫「學生卡住了怎麼辦」的一個具體做法；沒有就給空字串。
5. options：只有「引起動機」的步驟提供 2–3 個不同的暖身做法讓教師挑選（例如一個用圖片、一個用提問、一個用小遊戲），每個寫 title、idea、script（英文，最多 4 行）與 minutes。其他步驟的 options 一律給空陣列。
6. 活動要配合班級特性；沒有提供班級特性時，以一般高中班級設計。
7. 遵守訊息中的「學習目標（教師寫的）」與「教師的要求」（例如指定的活動形式、時間分配或 Bloom 層次）；寫「（沒有）」的就不用管。與上面的規則衝突時，以上面的規則為準。
8. 不要寫出任何真實人名、學校或學生資料。不要編造教材以外的影片、網站或文章的名稱與網址；需要額外素材時，寫成「老師準備的圖片」這類說法。
9. 每個文字欄位都是一段純文字，不要換行；需要分行的內容（script）每一行是陣列裡的一項。不要使用 Markdown 符號（例如 **、#）。$p$,
$p$班級特性：{{class_profile}}
學習目標（教師寫的）：{{teacher_objective}}
教師的要求：{{teacher_request}}

可對應的課綱條目（只能從這裡選代碼）：
{{curriculum_items}}

教材（{{material_title}}，共 {{paragraph_count}} 段）：

{{passage_numbered}}$p$);
