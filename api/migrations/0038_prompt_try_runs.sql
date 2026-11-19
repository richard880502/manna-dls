-- Manna DLS: 提示詞「試產生」(後台管理 → 提示詞版本).
--
-- The project lead can try an UNSAVED prompt draft on a sample she pastes. The call is made
-- directly to the model (never through the pipeline that writes `ai_interactions`), so a try
-- creates no worksheet, job or interaction row and no teacher can see it. This table keeps what
-- she tried so she can compare runs; every row is a preview (the CHECK says so), it is not
-- joined to any teacher's data, and the research exports, 我的作品 and the usage statistics do
-- not read it. The pasted sample is kept as its length and hash only.
CREATE TABLE prompt_try_runs (
    id              BIGSERIAL PRIMARY KEY,
    teacher_id      BIGINT NOT NULL REFERENCES teachers (id),
    prompt_key      TEXT NOT NULL,
    -- the saved version the draft started from (NULL for a key without versions)
    base_version_no INT,
    system_text     TEXT NOT NULL,
    user_template   TEXT NOT NULL,
    sample          JSONB NOT NULL,
    status          TEXT NOT NULL,
    output          JSONB,
    checks          JSONB NOT NULL DEFAULT '[]',
    preview         BOOLEAN NOT NULL DEFAULT true CHECK (preview),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX prompt_try_runs_teacher_idx ON prompt_try_runs (teacher_id, created_at DESC);
