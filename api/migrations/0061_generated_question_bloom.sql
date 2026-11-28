-- Label every question in its original generation call, including the guided SOP stages.
INSERT INTO prompt_versions (prompt_key, version_no, system_text, user_template, note)
SELECT v.prompt_key, v.version_no + 1, v.system_text || $p$

每一題都要填 bloom。依學生實際作答必須用到的最高認知層次標記，不能只看題型、動詞或難度：
- remember：回想或找出明示事實。
- understand：解釋、改述、歸納或推知意思。
- apply：把概念或語言用在新情境。
- analyze：分析關係、結構、因果或比較。
- evaluate：依證據或準則判斷並解釋理由。
- create：構思新的作品、計畫或問題。
只用這六個英文代號。暖身的 prompts_en 每一項改為物件，text 放學生要回答的英文題目，bloom 放該題層次。純指示和字彙表不是題目。$p$,
       v.user_template,
       '每道題各標一個 Bloom 層次，含暖身與 Exit ticket。'
  FROM prompt_versions v
 WHERE v.prompt_key IN ('worksheet_tasks', 'worksheet_warmup', 'worksheet_exit_ticket',
                        'sop_paragraph_tasks', 'sop_warmup', 'sop_exit_ticket')
   AND v.version_no = (SELECT MAX(version_no) FROM prompt_versions WHERE prompt_key = v.prompt_key);
