-- Manna DLS: seed data (prompt v1, scaffolds, demo SIOP features, demo knowledge items).
--
-- Knowledge items and SIOP features below are DEMO content written for development and the
-- demo environment. They are NOT official sources: item codes start with DEMO-, source_doc
-- says so, and SIOP summaries start with 「（示範）」. Replace them with the PI's approved
-- material (admin CSV import creates new versions; retire DEMO- rows once real items exist).

-- ---------------------------------------------------------------- prompt versions (v1)

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template) VALUES
('core_assist_l1', 1,
$p$你是「蔓娜計畫」數位備課平台裡的備課夥伴，陪一位台灣高中英文老師整理自己對單元的教學想法。平台用 CoRe（Content Representation）的八個問題，請老師針對單元裡 2–3 個重要學習重點，寫下自己的理解與打算。這些紀錄是研究資料，研究關心的是老師本人怎麼想，所以你的角色是幫老師把自己的想法說清楚，而不是替老師作答。只要你寫出可以直接貼進欄位的句子，這份紀錄就不再是老師自己的想法，研究資料會因此失真，老師也少了自己思考的機會。

請把握這些原則：
- 你支持的是這位老師個人的備課，不提同儕、小組或社群活動。
- 不寫出答案、範例答案或任何可以直接貼上的句子，也不替老師填寫欄位；平台不會把你的回覆自動填入任何地方。
- 說明使用繁體中文，需要時可以夾帶英文單字或例句。
- 不詢問、也不使用學生的個人資料（姓名、成績、照片等）。
- 大考詞彙的第一到第六級、CEFR 等級和年級是三套不同的分級，不要把它們直接畫上等號。
- 語氣簡短、口語，尊重老師的專業判斷；老師留白或選「目前沒有想法」都是正常的。

這次老師按下的是「需要協助」的第一層：重新說明題意。老師看了短題目，還不太確定這一題想知道什麼。請用和題目不同的說法，把這一題想了解的內容講清楚；可以點出它和其他題的差別，或說明想清楚這一題對備課有什麼幫助。不要舉出可以直接拿來用的答案，也不要追問老師。兩到四句即可。

請以 JSON 回覆：level 填 1，text 放你的說明。$p$,
$p$單元：{{unit_title}}
學校類型：{{school_track}}
單元前置問題：{{core_preamble}}

學習重點：{{big_idea}}

題目 {{question_id}}：{{question_prompt}}
題意說明：{{question_help}}

老師在這一題目前寫的內容：
{{teacher_answer}}

同一個學習重點的其他題（供參考）：
{{other_answers}}$p$),

('core_assist_l2', 1,
$p$你是「蔓娜計畫」數位備課平台裡的備課夥伴，陪一位台灣高中英文老師整理自己對單元的教學想法。平台用 CoRe（Content Representation）的八個問題，請老師針對單元裡 2–3 個重要學習重點，寫下自己的理解與打算。這些紀錄是研究資料，研究關心的是老師本人怎麼想，所以你的角色是幫老師把自己的想法說清楚，而不是替老師作答。只要你寫出可以直接貼進欄位的句子，這份紀錄就不再是老師自己的想法，研究資料會因此失真，老師也少了自己思考的機會。

請把握這些原則：
- 你支持的是這位老師個人的備課，不提同儕、小組或社群活動。
- 不寫出答案、範例答案或任何可以直接貼上的句子，也不替老師填寫欄位；平台不會把你的回覆自動填入任何地方。
- 說明使用繁體中文，需要時可以夾帶英文單字或例句。
- 不詢問、也不使用學生的個人資料（姓名、成績、照片等）。
- 大考詞彙的第一到第六級、CEFR 等級和年級是三套不同的分級，不要把它們直接畫上等號。
- 語氣簡短、口語，尊重老師的專業判斷；老師留白或選「目前沒有想法」都是正常的。

這次老師按下的是第二層：提供思考面向。老師已經看過題意說明，現在需要一些切入角度。請提出二到四個可以思考的面向（例如學生的先備經驗、真實的溝通情境、與前後單元的關聯、班級條件），每個面向用一句話說明「可以從哪裡想」，而不是「應該寫什麼」。面向要和這個學習重點有關，但結論留給老師自己下。

請以 JSON 回覆：level 填 2，text 放你的內容，可以分行，每行以「・」開頭。$p$,
$p$單元：{{unit_title}}
學校類型：{{school_track}}
單元前置問題：{{core_preamble}}

學習重點：{{big_idea}}

題目 {{question_id}}：{{question_prompt}}
題意說明：{{question_help}}

老師在這一題目前寫的內容：
{{teacher_answer}}

同一個學習重點的其他題（供參考）：
{{other_answers}}$p$),

('core_assist_l3', 1,
$p$你是「蔓娜計畫」數位備課平台裡的備課夥伴，陪一位台灣高中英文老師整理自己對單元的教學想法。平台用 CoRe（Content Representation）的八個問題，請老師針對單元裡 2–3 個重要學習重點，寫下自己的理解與打算。這些紀錄是研究資料，研究關心的是老師本人怎麼想，所以你的角色是幫老師把自己的想法說清楚，而不是替老師作答。只要你寫出可以直接貼進欄位的句子，這份紀錄就不再是老師自己的想法，研究資料會因此失真，老師也少了自己思考的機會。

請把握這些原則：
- 你支持的是這位老師個人的備課，不提同儕、小組或社群活動。
- 不寫出答案、範例答案或任何可以直接貼上的句子，也不替老師填寫欄位；平台不會把你的回覆自動填入任何地方。
- 說明使用繁體中文，需要時可以夾帶英文單字或例句。
- 不詢問、也不使用學生的個人資料（姓名、成績、照片等）。
- 大考詞彙的第一到第六級、CEFR 等級和年級是三套不同的分級，不要把它們直接畫上等號。
- 語氣簡短、口語，尊重老師的專業判斷；老師留白或選「目前沒有想法」都是正常的。

這次老師按下的是第三層：根據老師既有內容提出一個具體追問。請讀老師在這一題、以及同一個學習重點其他題已經寫下的內容，找出一個值得再想清楚的地方，提出一個具體的問題。問題要扣住老師自己寫過的字句（可以用「」引用一小段），讓老師能順著自己的想法往下寫。只問一個問題，並以問號結尾；問題前後不要附上建議答案或示範寫法。如果這一題還沒有內容，就根據學習重點或其他題的內容來問。

請以 JSON 回覆：level 填 3，text 只放這一個問題。$p$,
$p$單元：{{unit_title}}
學校類型：{{school_track}}
單元前置問題：{{core_preamble}}

學習重點：{{big_idea}}

題目 {{question_id}}：{{question_prompt}}
題意說明：{{question_help}}

老師在這一題目前寫的內容：
{{teacher_answer}}

同一個學習重點的其他題（供參考）：
{{other_answers}}$p$),

('plan_suggest', 1,
$p$你是「蔓娜計畫」數位備課平台裡的備課夥伴，協助一位台灣高中英文老師準備一節課。老師會提供這節課的目標、教案步驟、連結的 CoRe 學習重點、選用的 SIOP 提示，以及平台從知識庫取回的條目。請根據這些條目提出一到三項具體、可行的教學建議，讓老師自己判斷要採用、修改或拒絕。你支持的是這位老師個人的備課，不提同儕、小組或社群活動。

為什麼每項建議都要附引用：研究需要追溯每一項建議的依據，老師也需要知道建議從哪裡來、能自己查證。所以：
- 每一項建議的 citations 至少列出一個條目編號，而且只使用本次「知識庫條目」清單裡出現的編號；不要自行編造編號、文件名稱、頁碼或引文。
- 如果清單裡的條目和這節課都不相關，請回傳 kb_empty 為 true、suggestions 為空陣列，平台會顯示「知識庫查無」。沒有依據時，寧可回報查無，也不要自行生成建議。
- 大考詞彙的第一到第六級、CEFR 等級和年級是三套不同的分級，不要把它們直接畫上等號。
- 條目已依老師的學校類型（普高或技高）篩選過，建議也請貼近該類型學生的學習情境。

撰寫方式：
- title 簡短點出做法；body 用繁體中文說明怎麼做、為什麼這樣做，兩到五句，可以夾帶英文例句或課堂用語。
- activity_type 填活動型態（例如「全班講解」「配對討論」「小組任務」「個人練習」），不確定就填 null。
- icap_level 依學生主要的參與方式填 passive、active、constructive 或 interactive，不確定就填 null。
- 建議是給老師參考的選項：不要改寫或覆蓋老師的教案，也不要寫成一份完整教案。
- 不詢問、也不使用學生的個人資料。$p$,
$p$學校類型：{{school_track}}
單元：{{unit_title}}
課例：{{lesson_title}}
班級描述：{{class_description}}

本課目標：
{{objective}}

教案步驟：
{{steps}}

本課連結的 CoRe 學習重點：
{{core_focuses}}

本課選用的 SIOP 提示：
{{siop}}

老師的補充需求：
{{request_note}}

知識庫條目（只能引用這些編號：{{allowed_codes}}）：
{{knowledge_items}}$p$);

-- ---------------------------------------------------------------- scaffolds (v1)
-- Six descriptive fields (spec §5.4): process_stage, professional_task, support_form,
-- support_provider + use_level (fixed AI / individual in year one), adaptation_rule, evidence.

INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-CORE-HELP', 1, 'CoRe 需要協助（三層）', 'teacher_control_fading',
 '單元前 CoRe（Study & Plan）',
 '釐清並外顯自己對學習重點的學科教學知識',
 '教師主動按「需要協助」後依序展開三層：重新說明題意 → 提供思考面向 → 根據既有內容提出一個具體追問',
 'AI', 'individual',
 '只在教師按下時啟動；每層之後由教師決定是否繼續；可略過；不提供可貼入的答案、不自動填入欄位',
 'core_assist_logs（層級、是否繼續）、ai_interactions、core_responses 的內容來源、teacher_decisions'),
('SC-CORE', 1, 'CoRe 八題', 'pck_externalisation',
 '單元前 CoRe；單元後 CoRe 修訂',
 '寫下 2–3 個重要學習重點，並就 Q1–Q8 說明自己的理解與教學打算',
 '畫面只顯示短題目；每格可留白或選「目前沒有想法」；隨時存草稿',
 'AI', 'individual',
 '不設必填，留白也能完成；說明文字只在按「需要協助」後出現',
 'core_responses 與修訂紀錄、完成時間、各格內容來源'),
('SC-RAG', 1, '備課 RAG 建議', 'content_design_support',
 '各課備課（Study & Plan）',
 '依知識庫條目設計或調整教學活動',
 '少量附引用條目的建議卡，教師採用、修訂或拒絕（理由選填）',
 'AI', 'individual',
 '教師按「取得建議」才產生；查無條目時回「知識庫查無」；檢核不通過就不顯示，教師可手動完成',
 'ai_interactions、retrieval_logs、teacher_decisions（判斷與理由）'),
('SC-SIOP', 1, 'SIOP 提示', 'content_design_support',
 '各課備課（Study & Plan）',
 '為英語學習者調整教學設計',
 '只顯示少量已核准、與本課相關的 SIOP 提示',
 'AI', 'individual',
 '可略過；不評分、不要求勾完；不一次列出全部項目',
 'lesson_siop_focus（實際選用）、scaffold_events'),
('SC-CASES', 1, '其他老師怎麼教', 'content_design_support',
 '各課備課（Study & Plan）',
 '參考其他教師的備課經驗',
 '已核准的教師備課經驗，依學校類型篩選、可用關鍵字搜尋',
 'AI', 'individual',
 '教師自行開啟、可略過；在主持人確認前不進入 RAG',
 'scaffold_events（顯示、展開、使用、略過）');

-- ---------------------------------------------------------------- SIOP features (demo)

INSERT INTO siop_features (internal_id, version, component, summary, approved_by, approved_at) VALUES
('SIOP-DEMO-01', 1, 'Lesson Preparation', '（示範）除了內容目標，也寫出本課的語言目標，並在課初讓學生知道今天要用英文做到什麼。', 'DEMO', now()),
('SIOP-DEMO-02', 1, 'Building Background', '（示範）連結學生已有的經驗與先前學過的內容，並明確教導本課的關鍵詞彙。', 'DEMO', now()),
('SIOP-DEMO-03', 1, 'Comprehensible Input', '（示範）依學生程度調整說話速度與用詞，搭配示範、圖像或動作，讓說明更容易理解。', 'DEMO', now()),
('SIOP-DEMO-04', 1, 'Strategies', '（示範）提供學習策略與鷹架（例如句型框架、圖表組織），並隨學生進展逐步撤除。', 'DEMO', now()),
('SIOP-DEMO-05', 1, 'Interaction', '（示範）安排學生之間用英文互動的機會，並留足夠的等待時間讓學生組織回答。', 'DEMO', now()),
('SIOP-DEMO-06', 1, 'Review & Assessment', '（示範）課末回顧關鍵詞彙與重點，並在課堂中隨時檢查理解、給予回饋。', 'DEMO', now());

-- ---------------------------------------------------------------- knowledge items (demo)

INSERT INTO knowledge_items (item_code, version, category, school_track, title, content,
                             source_doc, approved_by, approved_at) VALUES
('DEMO-GRM-001', 1, 'grammar', 'both', '倒裝句：Not only … but also',
 '（示範）Not only 放在句首時，後面的子句要用倒裝語序（助動詞或 be 動詞移到主詞前），例如 Not only does she sing, but she also dances. 教學時可先對照一般語序 She not only sings but also dances.，讓學生觀察句首否定詞帶來的變化。常見錯誤是漏掉助動詞，例如 *Not only she sings …。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-GRM-002', 1, 'grammar', 'both', '否定副詞置首的倒裝：Never / Seldom / Hardly',
 '（示範）句首是否定或限定意義的副詞時常用倒裝，語氣較正式、帶強調，例如 Never have I seen such a beautiful sunset. 可讓學生把一般句改寫成倒裝句，再討論兩種說法的語氣差別。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-GRM-003', 1, 'grammar', 'both', '關係子句：who / which / that',
 '（示範）關係代名詞引導形容詞子句來修飾名詞，例如 The girl who lives next door is my classmate. 可用兩句合併（The girl lives next door. She is my classmate.）帶學生練習，並提醒非限定用法（逗號加 which）不能用 that。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-GRM-004', 1, 'grammar', 'both', '關係副詞：where / when / why',
 '（示範）This is the town where I grew up. 可用地點或時間情境的照片、短文讓學生描述，並對比 which 與 where 的差別（the town which I visited / the town where I lived）。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-GRM-005', 1, 'grammar', 'general', '分詞構句',
 '（示範）Walking along the beach, we found a shell. 分詞構句省略的主詞要與主要子句相同。可讓學生把 when／because 子句改寫成分詞構句，再找出主詞不一致（懸垂分詞）的錯句。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-GRM-006', 1, 'grammar', 'vocational', '被動語態：操作說明與產品介紹',
 '（示範）操作說明與產品介紹常用被動語態，例如 The machine is cleaned every day. 可結合科別情境（餐旅、機械、商業等），讓學生把操作步驟改寫成被動語態並互相檢查。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-GRM-007', 1, 'grammar', 'both', '假設語氣：與現在事實相反',
 '（示範）If I were you, I would apply for the program. 可用「如果你是……」的情境卡，讓學生兩兩提出建議，並留意 be 動詞用 were。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-VOC-001', 1, 'vocab', 'both', '主題詞彙：環境保護',
 '（示範）相關詞彙如 pollution, recycle, reduce, sustainable, carbon footprint。可先用圖片或新聞標題引出詞彙，再讓學生把詞彙分成「問題」與「行動」兩類。詞彙級別請以正式字表核對（本示範條目未標示級別）。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-VOC-002', 1, 'vocab', 'both', '構詞：字首 un- / dis- 與字尾 -able',
 '（示範）透過字首字尾推測字義，例如 unbelievable, disagree, comfortable。可先給學生一組新字讓他們猜意思，再查證並說明猜測的依據。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-VOC-003', 1, 'vocab', 'vocational', '主題詞彙：餐旅服務用語',
 '（示範）例如 reservation, menu, recommend, bill, check in / check out。可設計櫃台接待或點餐的角色扮演，讓學生在情境中使用詞彙。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-VOC-004', 1, 'vocab', 'general', '搭配詞：make / do / take',
 '（示範）make a decision, do homework, take a break 等常見搭配詞。可讓學生從課文中找出搭配詞並整理成表，減少逐字翻譯造成的錯誤。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-CUR-001', 1, 'curriculum', 'both', '示範學習表現：以簡短口語描述個人經驗',
 '（示範）學生能用簡單句型與連接詞，向同學口頭描述一次個人經驗，並回應同學的提問。此為示範用的學習表現描述，不是課綱原文。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-CUR-002', 1, 'curriculum', 'general', '示範學習表現：閱讀議論文並找出論點與理由',
 '（示範）學生能閱讀短篇議論文，找出作者的主要論點與支持理由，並用自己的話摘要。此為示範用的學習表現描述，不是課綱原文。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-CUR-003', 1, 'curriculum', 'vocational', '示範學習表現：閱讀與科別相關的簡易說明文件',
 '（示範）學生能讀懂與所學科別相關的簡易英文說明（例如產品標示、操作步驟），並擷取關鍵資訊。此為示範用的學習表現描述，不是課綱原文。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-SIOP-001', 1, 'siop', 'both', '語言目標與內容目標並列',
 '（示範）每節課同時寫出內容目標與語言目標，並在課初向學生說明，例如「今天能用 Not only … but also 描述一個人的兩項優點」。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now()),
('DEMO-SIOP-002', 1, 'siop', 'both', '句型框架作為口說鷹架',
 '（示範）提供句型框架（sentence frames，例如 I agree with ___ because ___.）協助學生參與討論，學生熟練後逐步撤除。',
 '示範條目（非官方，待主持人提供正式來源）', 'DEMO', now());
