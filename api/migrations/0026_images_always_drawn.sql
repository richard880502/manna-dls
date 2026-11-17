-- Manna DLS: worksheet pictures are always drawn (the PI's decision, 2026-11-17). There is no
-- "needs a real photo" kind any more: the model only writes scene, object or icon, the server
-- draws a `photo_needed` it still receives (or finds in an older worksheet) as a scene, and a
-- picture that came out wrong is the teacher's to redraw, replace or remove.
--
-- Version 2 of `worksheet_image_slots` is version 1 without `photo_needed`, and with one line on
-- what to do with maps, flags or real places (draw a simple, generic illustration). The version
-- number is the next one, not a fixed 2: the admin page also adds versions.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template)
SELECT 'worksheet_image_slots', COALESCE(MAX(version_no), 0) + 1,
$p$image_slots：這個區塊需要一張圖時才寫（例如情境圖、物件圖、看圖說話、幫助理解課文的場景），不需要就給空陣列；不要為了裝飾而加圖。每個圖片位置：
- kind：scene（情境或場景插圖）、object（單一物件）、icon（簡單圖示）。所有圖都由 AI 畫成插圖；地圖、國旗、地點或動植物也一樣畫成簡單、通用的插圖，不要求真實照片。
- purpose_zh：給教師看的一句繁體中文，說明這張圖在這一區做什麼。
- prompt_en：給產圖模型的英文畫面描述，只描述畫面（人物、動作、地點、物件、氣氛），一到三句。不要寫任何真實人名、名人、品牌、商標或網址；不要要求在圖裡寫字、標籤、數字或對話框（需要的字放在 caption）。
- alt_en：英文替代文字，一句話說明圖裡有什麼。
- caption：印在圖下方的圖說（語言同學生看到的指示語）；不需要時給空字串。題目要學生看圖作答時，題目要寫在題目裡，不要只寫在圖說。$p$,
''
  FROM prompt_versions WHERE prompt_key = 'worksheet_image_slots';

UPDATE scaffolds
   SET support_form = 'Manna 每個圖片位置一次呼叫 OpenAI GPT Image（全站同時最多 IMAGE_CONCURRENCY 張）；所有位置都畫成插圖（地圖、國旗、地點也畫成通用插圖），圖裡不放字，字寫在圖說'
 WHERE scaffold_code = 'SC-WS-IMAGE' AND version = 1;
