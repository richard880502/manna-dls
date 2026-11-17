-- Manna DLS: SIOP 自我檢核 (docs/SIOP_DESIGN_RULES.md section 八). Contract in docs/API.md
-- 「SIOP 自我檢核」.
--
-- After a /create output is made (a `worksheet`, `comprehension`, `tiered`, `activity` or `kit`
-- turn of a 備課對話 closes `done`), one model call judges the SIOP observation items that can be
-- seen on the page: 符合／部分符合／未見 and a one-sentence reason each, no total score. Items
-- 10, 18, 25 and 26 need a classroom: they are never sent to the model. Items 12, 23–24 and 29
-- are judged on their visible part only. The item list lives in the server
-- (`prep::selfcheck::ITEMS`) and reaches the prompt as {{items}}.

-- ---------------------------------------------------------------- checks
-- One row per run of a turn's check, numbered per turn: the first opens when the turn closes
-- `done` (same transaction), later ones when the teacher asks again (再試一次, or after the
-- worksheet was revised). `items` holds the judged items only, `[{ id, rating, reason }]`
-- (rating: met | partial | not_seen); `worksheet_version_no` is the worksheet version judged.
-- A check is advice for the teacher: a failed one never touches the output.
CREATE TABLE siop_checks (
    id                    BIGSERIAL PRIMARY KEY,
    turn_id               BIGINT NOT NULL REFERENCES prep_turns (id) ON DELETE CASCADE,
    run_no                INT NOT NULL,
    status                TEXT NOT NULL CHECK (status IN ('running', 'done', 'failed')),
    items                 JSONB NOT NULL DEFAULT '[]'::jsonb,
    error                 TEXT,
    worksheet_id          BIGINT REFERENCES worksheets (id) ON DELETE SET NULL,
    worksheet_version_no  INT,
    interaction_id        BIGINT REFERENCES ai_interactions (id),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at           TIMESTAMPTZ,
    UNIQUE (turn_id, run_no)
);
-- Only one run of a turn's check at a time.
CREATE UNIQUE INDEX siop_checks_one_running ON siop_checks (turn_id) WHERE status = 'running';

-- ---------------------------------------------------------------- scaffold
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-SIOP-SELFCHECK', 1, '產出後的 SIOP 自我檢核', 'evidence_based_reflection',
 '建立（學習單、課堂活動、教學套件產生之後）',
 '依 SIOP 觀察量表，檢核產出上看得到的項目，提醒教師哪些做到了、哪些還看不到',
 'Manna 一次呼叫逐項判斷「符合／部分符合／未見」並附一句理由；需課堂觀察的項目（10、18、25、26）不評，只能部分判斷的項目（12、23–24、29）只看書面；不產生總分',
 'AI', 'individual',
 '教師自己判斷要不要採納；檢核失敗不影響產出，可以再試一次；學習單改過之後可以重新檢核',
 'ai_interactions（module = siop_selfcheck）、siop_checks');

-- ---------------------------------------------------------------- prompt
-- The JSON schema (built by the server) asks for exactly one judgement per item, ids from an
-- enum in the items' order, `reason` (≤100) before `rating`.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'siop_selfcheck', COALESCE(MAX(version_no), 0) + 1,
$p$你是「蔓娜計畫」數位備課平台裡的 SIOP 檢核助理。教師剛用平台做好一份教學產出（學習單、課堂活動規劃或教學套件）。請依 SIOP（Sheltered Instruction Observation Protocol）觀察量表裡「從書面產出看得出來」的項目，逐項檢核這份產出。結果是給教師參考的建議，由教師自己判斷，不是評分。

規則：
1. items 依訊息中「要檢核的項目」的順序，每一項一筆，id 照抄（例如 "1-2"、"12"），不可遺漏，也不可新增。
2. 每一項先寫 reason，再給 rating：
   - reason：用繁體中文一句話說明你在產出裡看到什麼（指出是哪一步、哪個區塊或哪一題），或缺了什麼；不超過 80 字，不要換行。
   - rating 只能是「符合」「部分符合」「未見」其中之一：符合＝產出裡清楚看得到，而且做得完整；部分符合＝看得到一部分，或做得不完整；未見＝這份產出裡看不到。
3. 只根據這份產出的文字判斷。不要猜課堂上會怎麼進行，也不要因為「老師上課時可能會做」就給符合。
4. 標著「只看書面部分」的項目，只判斷括號裡說的那一部分。
5. 不同的產出本來就不會有每一項（例如學習單通常不寫分組，課堂活動規劃通常沒有分版本）；看不到就給「未見」，理由寫「這份產出沒有……」，語氣中性，不要責備。
6. 不要給總分，也不要寫項目以外的建議。只用純文字，不要使用 Markdown 符號。$p$,
$p$產出種類：{{output_kind}}
班級特性：{{class_profile}}

教材（產出依據的文本，供對照）：
{{passage_numbered}}

要檢核的產出：
{{output_text}}

要檢核的項目（共 {{item_count}} 項）：
{{items}}$p$
  FROM prompt_versions WHERE prompt_key = 'siop_selfcheck';
