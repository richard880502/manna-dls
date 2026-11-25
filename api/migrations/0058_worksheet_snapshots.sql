-- 快照: a teacher's name on one version of a worksheet (docs/WORKSHEET_WORKSPACE.md §2).
-- Versions stay append-only; removing a snapshot only removes the name.
CREATE TABLE worksheet_snapshots (
    id           BIGSERIAL PRIMARY KEY,
    worksheet_id BIGINT      NOT NULL REFERENCES worksheets(id) ON DELETE CASCADE,
    version_no   INT         NOT NULL,
    name         TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (worksheet_id, version_no)
);
CREATE INDEX worksheet_snapshots_worksheet_idx ON worksheet_snapshots (worksheet_id);
