-- 產生進度 (docs/API.md「產生進度」): the steps of a run, as the teacher reads them, written by
-- the background work once per step change and returned by the polling responses.
--
-- * `worksheets.progress`: the steps of the worksheet's latest generation or regeneration run
--   (`run_no` names the run; a later run replaces it). The pictures queued by the run are kept
--   as their `worksheet_images` ids, so 「配圖中（n／m）」 is read from those rows.
-- * `prep_turns.progress`: the steps a turn runs itself (課堂活動, the plan of a 教學套件, 修改
--   學習單). A worksheet turn reads its worksheet's; the SIOP 自我檢核 is read from `siop_checks`.
--
-- NULL (every row made before this migration): no progress, the pages show what they showed.
ALTER TABLE worksheets ADD COLUMN progress JSONB;
ALTER TABLE prep_turns ADD COLUMN progress JSONB;
