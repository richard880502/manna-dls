-- Manna DLS: browsing and adopting the earlier states of a block (「修改歷程」).
--
-- 1. worksheet_versions.cause gets `pictures`: the teacher's own picture buttons (重畫 / 拿掉 /
--    換成我的圖) are a step of their own. The version has the same content as the one before it;
--    its `source` says which button, on which slot, and which worksheet_images row it made
--    current (`pictures` holds every slot's current row, as for any version).
-- 2. worksheet_block_proposals keeps when a proposal that was not adopted at the time was adopted
--    later from the history (`adopted_later_at`, and the version that wrote). Its status, its
--    `decided_at` and its earlier decision rows stay as they were: the later adoption is a new
--    decision row on its interaction and a new `restore` version.
-- Nothing is deleted or overwritten by browsing or adopting.

ALTER TABLE worksheet_versions DROP CONSTRAINT worksheet_versions_cause_check;
ALTER TABLE worksheet_versions ADD CONSTRAINT worksheet_versions_cause_check
    CHECK (cause IN ('created', 'edit', 'generate', 'regenerate', 'undo', 'revise', 'vocabulary',
                     'restore', 'pictures'));

ALTER TABLE worksheet_block_proposals
    ADD COLUMN adopted_later_at         TIMESTAMPTZ,
    ADD COLUMN adopted_later_version_no INT;
