-- Manna DLS: the 語言目標 is decided after the 內容目標, and derived from it.
--
-- A 語言目標 says which language students need to reach the 內容目標: which reading, listening,
-- speaking or writing they must produce, so that the teacher, looking at what they produced,
-- knows whether they really learned the content. It cannot be written before the 內容目標 is
-- decided. Two flows used to draft both kinds in one call; now each call drafts one kind:
-- * `objective_draft` (the generators' 「請 Manna 擬一組」 and the draft before 產生): the first
--   request drafts the 內容目標; once the teacher kept some, a second request drafts the 語言目標
--   from the ones she kept. The server tells the model which one (`{{stage_note}}`).
-- * `sop_objectives` (lesson SOP step 3): 「請 Manna 推薦內容目標」 first, then 「請 Manna 依內容目標
--   推薦語言目標」 once the lesson has a 內容目標; the instruction goes at the end of the message
--   (like 換一個 does).
-- * `prep_activity` (the activity plan of a prep conversation) is one plan the teacher revises as a
--   whole, so it keeps one call; its 語言目標 are now defined the same way.
--
-- New versions, each built from the latest text (whatever it is: the admin page and other
-- migrations add versions too) with one section appended. Older versions stay.

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

分兩個階段草擬（優先於上面「先寫內容目標、再寫語言目標」的做法）：
- 內容目標先決定，語言目標之後才依它草擬，所以每次只寫其中一組。訊息裡的「【這次只寫內容目標】」或「【這次只寫語言目標】」說明這次要寫哪一組；另一組的陣列給空陣列。
- 只寫內容目標時：只寫 content_objectives（規則 2），language_objectives 給空陣列；規則 3 到 5 這次用不到。
- 只寫語言目標時：訊息會列出教師已經決定的內容目標，代號是 C1、C2。content_objectives 給空陣列，只寫 language_objectives。
- 語言目標的定義：為了達成內容目標，學生需要用什麼語言，也就是要用英語「讀、聽、說、寫」裡的哪一種能力，做出什麼看得到、聽得到的成果；老師看到這個成果，就知道學生是不是真的學會了內容目標。所以先想「要證明學生達成這個內容目標，學生得讀什麼、聽什麼、說什麼、寫什麼」，再把它寫成語言目標：規則 3 說的類別、字詞、句型或策略，要寫成這個成果會用到的語言。
- supports 只能填教師已決定的內容目標的代號；relation_zh 寫成「學生要用……，才能……」，並說出學生做出的成果。教師已決定的內容目標是教師的，不要改寫，也不要重複寫成語言目標。$p$,
       v.user_template || '{{stage_note}}'
  FROM prompt_versions v
 WHERE v.prompt_key = 'objective_draft'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

分兩步推薦（優先於上面「兩類目標至少各 1 項、合計 3–5 項」）：
- 內容目標先決定，語言目標之後才依它推薦，所以每次只推薦一類。訊息最後的「【這次只要內容目標】」或「【這次只要語言目標】」說明這次要哪一類，objectives 只輸出那一類，siop_type 照它寫；以它為準，每類 2–3 項。
- 語言目標的定義：為了達成內容目標，學生需要用什麼語言，也就是要用英語「讀、聽、說、寫」裡的哪一種能力，做出什麼看得到、聽得到的成果；老師看到這個成果，就知道學生是不是真的學會了內容目標。
- 推薦語言目標時，「教師已經寫的目標」與訊息最後列的內容目標就是教師已經決定的內容目標：先想「要證明學生達成它，學生得讀、聽、說、寫出什麼」，再寫成語言目標。reason_zh 要說明它服務哪一項內容目標、學生會做出什麼成果；不要重寫內容目標，也不要在沒有內容目標時推薦語言目標。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'sop_objectives'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT v.prompt_key,
       (SELECT COALESCE(MAX(version_no), 0) + 1 FROM prompt_versions WHERE prompt_key = v.prompt_key),
       v.system_text || $p$

語言目標的定義（補充規則 2）：語言目標是為了達成內容目標，學生需要用什麼語言，也就是要用英語「讀、聽、說、寫」裡的哪一種能力，做出什麼看得到、聽得到的成果；老師看到這個成果，就知道學生是不是真的學會了內容目標。所以先決定內容目標，再從它推出語言目標：每個語言目標的 relation 要說出學生做出的成果（「學生要用……，才能……」），不能寫成與內容目標無關的字彙或文法清單。$p$,
       v.user_template
  FROM prompt_versions v
 WHERE v.prompt_key = 'prep_activity'
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);

UPDATE scaffolds
   SET support_form = support_form || '；內容目標與語言目標分兩次草擬：先草擬內容目標，教師決定之後才依她留下的內容目標草擬語言目標（學生要讀、聽、說、寫出什麼成果，才看得出達成內容目標）'
 WHERE scaffold_code = 'SC-OBJ-DRAFT' AND version = 1;

UPDATE scaffolds
   SET support_form = support_form || '；先推薦內容目標，教師有了內容目標之後才依它推薦語言目標（兩類分開請求）'
 WHERE scaffold_code = 'SC-SOP-OBJECTIVES' AND version = 1;
