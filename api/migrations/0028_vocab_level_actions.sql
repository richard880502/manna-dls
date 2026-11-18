-- Manna DLS: 詞彙級別檢查：處理超出級別的字. Contract in docs/API.md「詞彙級別檢查：處理超出級別的字」.
--
-- For each word above the teacher's target level the teacher chooses 提供字義 (the word stays and
-- its meaning goes in the block's Glossary), 換字 (the sentences of the questions, instructions and
-- options that have it are rewritten within the level) or 不處理. The 大考中心 參考詞彙表 has levels
-- and parts of speech but no meanings, so meanings and rewrites come from one bounded call per
-- action for all the chosen words. The server checks every rewrite against the word list; nothing
-- here is trusted.
-- No new table: the Glossary lives in the worksheet content (`blocks[].glosses`), the words the
-- teacher leaves are kept with the check result, and the choices go to `teacher_decisions`
-- (when a call suggested something) and `usage_logs`.

-- ---------------------------------------------------------------- versions
-- A version written by 套用 on the vocabulary check page.
ALTER TABLE worksheet_versions DROP CONSTRAINT worksheet_versions_cause_check;
ALTER TABLE worksheet_versions ADD CONSTRAINT worksheet_versions_cause_check
    CHECK (cause IN ('created', 'edit', 'generate', 'regenerate', 'undo', 'revise', 'vocabulary'));

-- ---------------------------------------------------------------- scaffold
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-WS-VOCAB', 1, '超出級別的字：提供字義或換字', 'content_design_support',
 '建立（學習單產生之後的詞彙級別檢查）',
 '針對超出目標級別的字，讓教師決定提供字義、換字或不處理',
 'Manna 一次呼叫寫出所選字在上下文中的中文意思（含詞性與簡短英文解釋），或把含有這些字的句子各改寫一次（只改那個字附近）；改寫句的每個新字由程式依參考詞彙表檢查級別，改動太大的不採用；教師逐句確認改寫前後',
 'AI', 'individual',
 '教師逐字決定；可以全部提供字義或全部換字；課文本身不會改，字詞表與字庫不換字；套用後寫成新版本，可以逐區復原',
 'ai_interactions（module = worksheet_vocab_gloss／worksheet_vocab_replace）、teacher_decisions、usage_logs（worksheet_vocabulary_applied）');

-- ---------------------------------------------------------------- prompts
-- The JSON schemas are built by the server: one entry per word (min = max = the number of words),
-- `word` from an enum of the words asked about, every string bounded, properties in the order
-- written here.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_vocab_gloss', COALESCE(MAX(version_no), 0) + 1,
$p$你是「蔓娜計畫」數位備課平台裡的英文字彙助理，協助臺灣高中英文教師準備學習單。教師用大考中心高中英文參考詞彙表檢查學習單，發現一些字超出學生的級別，決定保留這些字、在學習單上附上字義。請為每個字寫出它在這份學習單上下文中的意思。

規則：
1. glosses 依訊息中字的順序，每個字一筆，word 照抄（小寫原形），不可遺漏，也不可新增。
2. 每一筆依序寫：
   - pos：這個字在上下文中的詞性，用參考詞彙表的寫法（n.、v.、adj.、adv.、prep.、conj.）。詞彙表列了詞性時，從其中選一個。
   - meaning_zh：繁體中文意思，只寫在這個上下文中的意思，2 到 12 個字，不要換行；有兩個相近的說法時用「；」隔開。
   - definition_en：一句簡短的英文解釋，只能用第 1 到 {{target_level}} 級的簡單字，不要用到這個字本身或它的變化形；寫不出夠簡單的解釋時留空字串。
3. 只根據上下文判斷意思，不要列出上下文用不到的其他字義。
4. 只用純文字，不要使用 Markdown 符號，字串中不要換行。$p$,
$p$學習單的版本：{{version_label}}
目標級別：第 1 到 {{target_level}} 級
要附上字義的字（共 {{word_count}} 個）：
{{words}}
{{round_note}}$p$
  FROM prompt_versions WHERE prompt_key = 'worksheet_vocab_gloss';

-- 換字 rewrites whole sentences: swapping a word form in place gave ungrammatical English
-- (「according to the text」→「as said to the text」). The schema asks first for one hint word per
-- word, then one rewrite per sentence (`id` from an enum of the sentence ids, `rewritten` bounded
-- by the sentence's length). The server keeps a rewrite only when every word it adds is within the
-- target level, the word is gone and the change stays near it.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_vocab_replace', COALESCE(MAX(version_no), 0) + 1,
$p$你是「蔓娜計畫」數位備課平台裡的英文編輯，協助臺灣高中英文教師準備學習單。教師用大考中心高中英文參考詞彙表檢查學習單，發現題目、選項或指示裡有些字超出學生的級別。請把含有這些字的句子各改寫一次：避開這些字，意思不變，其餘部分盡量不動。

規則：
1. words：依訊息中字的順序，每個字一筆，word 照抄（小寫原形）；replacement 寫你主要用來代替它的字或片語（小寫原形，最多三個字），只能用第 1 到 {{target_level}} 級的字。
2. sentences：依訊息中句子的順序，每一句一筆，id 照抄（例如 "p1"），不可遺漏，也不可新增。
3. rewritten 是改寫後的整句：
   - 句子裡「要避開」的字都不能再出現；引號內的字是在指稱那個字本身，保留不動。
   - 意思要和原句相同，文法要正確、讀起來自然。含有這個字的片語要整個一起改（例如 according to the text → based on the text，不要只換 according）；動詞時態、名詞單複數、主詞動詞一致、前面的 a／an 都要跟著調整。
   - 只改這個字和它旁邊必要的幾個字（最多改動兩三個字），句子其他部分一字不改，也不要改寫成另一句話。
   - 新加的字只能用參考詞彙表第 1 到 {{target_level}} 級的字；不要用詞彙表外的字、專有名詞或縮寫。原句本來就有的其他字可以保留。
   - 保持原句的大小寫、標點與空格（例如句首大寫、問號）。
4. 平台會逐字檢查：用了超出級別的字、要避開的字還在、或改動太大的句子都會被拿掉，那個地方就不改。
5. 只用純文字，不要使用 Markdown 符號，字串中不要換行，也不要解釋理由。$p$,
$p$學習單的版本：{{version_label}}
目標級別：第 1 到 {{target_level}} 級
要避開的字（共 {{word_count}} 個）：
{{words}}

要改寫的句子（共 {{sentence_count}} 句；格式：id｜位置｜要避開的字，下一行是原句）：
{{sentences}}
{{round_note}}$p$
  FROM prompt_versions WHERE prompt_key = 'worksheet_vocab_replace';
