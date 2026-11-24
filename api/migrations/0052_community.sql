-- 社群 (phase 1, docs/COMMUNITY.md): a teacher may opt in to share a worksheet with the other
-- teachers, who can browse it, save it and remix it into a private copy. Everything is off
-- until the PI turns on the `community_enabled` setting (app_settings, no schema needed).
--
-- Sharing is a property of the worksheet row: `shared_at` set = shared. The columns describe
-- how the owner is shown, never who the owner is:
--   share_name         'code' shows the research id, 'anonymous' shows 「一位老師」.
--   share_allow_remix  whether others may make a copy of it.
--   remixed_from       on a copy made from a shared worksheet: the worksheet it came from. Only
--                      the copy's own teacher reads it, and only as a title (see the API); the
--                      original keeps a count of its copies and no list of whose they are.
ALTER TABLE worksheets
    ADD COLUMN shared_at         TIMESTAMPTZ,
    ADD COLUMN share_name        TEXT CHECK (share_name IN ('code', 'anonymous')),
    ADD COLUMN share_allow_remix BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN remixed_from      BIGINT REFERENCES worksheets (id) ON DELETE SET NULL,
    ADD CONSTRAINT worksheets_share_consistent
        CHECK ((shared_at IS NULL) = (share_name IS NULL));

-- The explore list: shared, not deleted, newest first.
CREATE INDEX worksheets_shared_idx ON worksheets (shared_at DESC, id DESC)
    WHERE shared_at IS NOT NULL AND deleted_at IS NULL;
-- Counting the copies of a worksheet.
CREATE INDEX worksheets_remixed_from_idx ON worksheets (remixed_from)
    WHERE remixed_from IS NOT NULL;

-- 我的收藏. A save outlives an unshare (the row stays, the worksheet is hidden from the saver's
-- list while it is not shared) so that sharing it again brings it back.
CREATE TABLE worksheet_saves (
    teacher_id    BIGINT NOT NULL REFERENCES teachers (id),
    worksheet_id  BIGINT NOT NULL REFERENCES worksheets (id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (teacher_id, worksheet_id)
);
CREATE INDEX worksheet_saves_worksheet_idx ON worksheet_saves (worksheet_id);
