-- Manna DLS: every modification step is kept, and a step taken back is marked, never deleted.
--
-- 1. worksheet_versions says what produced each version (`source`, JSON: cause plus the ids
--    of the proposal, interaction, prep turn, blocks, or the version an undo / restore went
--    back to) and which picture was current in every slot at that step (`pictures`, JSON
--    object slot id -> worksheet_images.id, written once when the step's pictures are
--    settled). Old rows keep NULL. `restore` is a new cause: 「回到這一步」 writes a new version.
-- 2. worksheet_block_history is an undo stack that never loses a row: an undo marks the row
--    (`undone_at`, `undone_in_version_no`); only rows that are not undone are the stack. A row
--    written by 採用 of a block proposal also keeps the proposal (`proposal_id`), the block as
--    it was written (`after_block`, so an undo can tell the block was changed since), the
--    version the step wrote (`version_no`) and what happened to the pictures
--    (`picture_undo`: the slots touched, with the image row current before and after).
-- 3. worksheet_block_proposals.undone_at: when the teacher took the accepted proposal back.
-- worksheet_images rows are never deleted either: a superseded picture stays with
-- current = FALSE and an undo makes the earlier row current again.

ALTER TABLE worksheet_versions DROP CONSTRAINT worksheet_versions_cause_check;
ALTER TABLE worksheet_versions ADD CONSTRAINT worksheet_versions_cause_check
    CHECK (cause IN ('created', 'edit', 'generate', 'regenerate', 'undo', 'revise', 'vocabulary',
                     'restore'));
ALTER TABLE worksheet_versions
    ADD COLUMN source   JSONB,
    ADD COLUMN pictures JSONB;

ALTER TABLE worksheet_block_history
    ADD COLUMN proposal_id          BIGINT REFERENCES worksheet_block_proposals (id),
    ADD COLUMN version_no           INT,
    ADD COLUMN after_block          JSONB,
    ADD COLUMN picture_undo         JSONB,
    ADD COLUMN undone_at            TIMESTAMPTZ,
    ADD COLUMN undone_in_version_no INT;
CREATE INDEX worksheet_block_history_open_idx
    ON worksheet_block_history (worksheet_id, block_id, id) WHERE undone_at IS NULL;

ALTER TABLE worksheet_block_proposals ADD COLUMN undone_at TIMESTAMPTZ;

-- 再改一次 keeps the earlier attempts of a block's round adoptable: `set_aside_at` is when an
-- attempt was set aside by asking again (it keeps its proposed block); `round_closed_at` is when
-- the round ended (one attempt was adopted, or the teacher gave the round up): no attempt of a
-- closed round can be adopted any more. Rows are never deleted.
ALTER TABLE worksheet_block_proposals
    ADD COLUMN set_aside_at    TIMESTAMPTZ,
    ADD COLUMN round_closed_at TIMESTAMPTZ;
