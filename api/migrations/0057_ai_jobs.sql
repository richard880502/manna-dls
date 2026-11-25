CREATE TABLE ai_jobs (
    id BIGSERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    teacher_id BIGINT NOT NULL REFERENCES teachers(id),
    priority SMALLINT NOT NULL CHECK (priority BETWEEN 0 AND 2),
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'done', 'failed', 'cancelled')),
    params JSONB NOT NULL DEFAULT '{}'::jsonb,
    ref_kind TEXT,
    ref_id BIGINT,
    attempts INT NOT NULL DEFAULT 0,
    lease_until TIMESTAMPTZ,
    error TEXT,
    queued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);
CREATE INDEX ai_jobs_queue ON ai_jobs (priority, queued_at) WHERE status = 'queued';
CREATE INDEX ai_jobs_active ON ai_jobs (teacher_id) WHERE status IN ('queued', 'running');
CREATE INDEX ai_jobs_lease ON ai_jobs (lease_until) WHERE status = 'running';
CREATE UNIQUE INDEX ai_jobs_one_per_ref ON ai_jobs (ref_kind, ref_id)
    WHERE status IN ('queued', 'running');
