-- Manna DLS: 刪除備課 removes the whole prep session from the teacher's view.
--
-- Until now DELETE /api/prep-sessions/:id removed the session and its turns for good and kept
-- its worksheets, which then came back in 我的作品 as worksheets of their own. Now the session is
-- hidden (like a deleted worksheet, `worksheets.deleted_at`) and the worksheets made in it are
-- hidden with it, in one transaction; the turns stay for the research record. The pictures the
-- teacher attached (prep_images) still leave the storage and their rows are removed.

ALTER TABLE prep_sessions ADD COLUMN deleted_at TIMESTAMPTZ;

DROP INDEX prep_sessions_teacher_idx;
CREATE INDEX prep_sessions_teacher_idx ON prep_sessions (teacher_id, updated_at DESC)
    WHERE deleted_at IS NULL;
