-- Manna DLS: Phase 1 schema (AI-assisted lesson preparation)
-- Principles (spec §3):
--   * research data keyed by research_id only; identity lives in identity_map
--   * content is versioned (append-only); deletes are soft (deleted_at) except unit materials
--   * AI interactions and teacher decisions are separate tables (no forced 1:1)

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS vector;

-- ---------------------------------------------------------------- accounts
CREATE TABLE teachers (
    id            BIGSERIAL PRIMARY KEY,
    research_id   TEXT NOT NULL UNIQUE,
    school_track  TEXT NOT NULL CHECK (school_track IN ('general', 'vocational')),
    consent_scope TEXT NOT NULL DEFAULT 'pending',
    role          TEXT NOT NULL DEFAULT 'teacher'
                  CHECK (role IN ('teacher', 'pi', 'ra', 'rater', 'engineer')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ
);

-- Kept apart from research data; encrypted at the application layer.
CREATE TABLE identity_map (
    research_id      TEXT PRIMARY KEY REFERENCES teachers (research_id),
    name_enc         BYTEA,
    email_enc        BYTEA,
    google_sub_enc   BYTEA,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE invite_codes (
    id           BIGSERIAL PRIMARY KEY,
    code_hash    TEXT NOT NULL UNIQUE,
    research_id  TEXT NOT NULL,
    school_track TEXT NOT NULL CHECK (school_track IN ('general', 'vocational')),
    role         TEXT NOT NULL DEFAULT 'teacher'
                 CHECK (role IN ('teacher', 'pi', 'ra', 'rater', 'engineer')),
    created_by   BIGINT REFERENCES teachers (id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    used_at      TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ
);

CREATE TABLE sessions (
    id          BIGSERIAL PRIMARY KEY,
    token_hash  TEXT NOT NULL UNIQUE,
    teacher_id  BIGINT NOT NULL REFERENCES teachers (id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ
);

-- ---------------------------------------------------------------- main line
CREATE TABLE units (
    id          BIGSERIAL PRIMARY KEY,
    teacher_id  BIGINT NOT NULL REFERENCES teachers (id),
    title       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ
);
CREATE INDEX units_teacher_idx ON units (teacher_id) WHERE deleted_at IS NULL;

CREATE TABLE lessons (
    id                BIGSERIAL PRIMARY KEY,
    unit_id           BIGINT NOT NULL REFERENCES units (id),
    title             TEXT NOT NULL,
    lesson_date       DATE,
    class_description TEXT NOT NULL DEFAULT '',   -- no student data
    producer_type     TEXT NOT NULL DEFAULT 'individual',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ
);
CREATE INDEX lessons_unit_idx ON lessons (unit_id) WHERE deleted_at IS NULL;

CREATE TABLE lesson_versions (
    id          BIGSERIAL PRIMARY KEY,
    lesson_id   BIGINT NOT NULL REFERENCES lessons (id),
    version_no  INT NOT NULL,
    objective   TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (lesson_id, version_no)
);

CREATE TABLE lesson_steps (
    id                 BIGSERIAL PRIMARY KEY,
    lesson_version_id  BIGINT NOT NULL REFERENCES lesson_versions (id),
    position           INT NOT NULL,
    activity_type      TEXT NOT NULL DEFAULT '',
    icap_level         TEXT CHECK (icap_level IN ('passive', 'active', 'constructive', 'interactive')),
    content            TEXT NOT NULL DEFAULT '',
    UNIQUE (lesson_version_id, position)
);

-- ---------------------------------------------------------------- CoRe
CREATE TABLE core_versions (
    id               BIGSERIAL PRIMARY KEY,
    unit_id          BIGINT NOT NULL REFERENCES units (id),
    phase            TEXT NOT NULL CHECK (phase IN ('pre', 'post')),
    version_no       INT NOT NULL,
    copied_from_id   BIGINT REFERENCES core_versions (id),
    completed_at     TIMESTAMPTZ,
    saw_ai_assist    BOOLEAN NOT NULL DEFAULT false,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (unit_id, phase, version_no)
);

-- Focuses removed by the teacher are soft-deleted. position is an ordering key only
-- (no UNIQUE): teachers reorder focuses, and a unique (version, position) pair would
-- make swaps fail mid-transaction and block re-adding at a removed focus's position.
CREATE TABLE core_focuses (
    id               BIGSERIAL PRIMARY KEY,
    core_version_id  BIGINT NOT NULL REFERENCES core_versions (id),
    position         INT NOT NULL,
    big_idea         TEXT NOT NULL DEFAULT '',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at       TIMESTAMPTZ
);
CREATE INDEX core_focuses_version_idx ON core_focuses (core_version_id) WHERE deleted_at IS NULL;

CREATE TABLE core_responses (
    id             BIGSERIAL PRIMARY KEY,
    core_focus_id  BIGINT NOT NULL REFERENCES core_focuses (id),
    question_id    TEXT NOT NULL CHECK (question_id IN ('Q1','Q2','Q3','Q4','Q5','Q6','Q7','Q8')),
    answer         TEXT NOT NULL DEFAULT '',
    status         TEXT NOT NULL DEFAULT 'blank'
                   CHECK (status IN ('filled', 'blank', 'no_idea')),
    content_source TEXT NOT NULL DEFAULT 'self'
                   CHECK (content_source IN ('self', 'self_after_ai', 'modified_ai_example', 'adopted_ai_example')),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (core_focus_id, question_id)
);

-- Every save of a response is kept (revision history).
CREATE TABLE core_response_revisions (
    id               BIGSERIAL PRIMARY KEY,
    core_response_id BIGINT NOT NULL REFERENCES core_responses (id),
    answer           TEXT NOT NULL,
    status           TEXT NOT NULL,
    content_source   TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE lesson_core_links (
    id             BIGSERIAL PRIMARY KEY,
    lesson_id      BIGINT NOT NULL REFERENCES lessons (id),
    core_focus_id  BIGINT NOT NULL REFERENCES core_focuses (id),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_at     TIMESTAMPTZ
);

-- ---------------------------------------------------------------- SIOP
CREATE TABLE siop_features (
    id           BIGSERIAL PRIMARY KEY,
    internal_id  TEXT NOT NULL,
    version      INT NOT NULL DEFAULT 1,
    component    TEXT NOT NULL,
    summary      TEXT NOT NULL,
    approved_by  TEXT,
    approved_at  TIMESTAMPTZ,
    UNIQUE (internal_id, version)
);

CREATE TABLE lesson_siop_focus (
    id               BIGSERIAL PRIMARY KEY,
    lesson_id        BIGINT NOT NULL REFERENCES lessons (id),
    siop_feature_id  BIGINT NOT NULL REFERENCES siop_features (id),
    selected_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_at       TIMESTAMPTZ
);

-- ---------------------------------------------------------------- knowledge base
CREATE TABLE knowledge_items (
    id              BIGSERIAL PRIMARY KEY,
    item_code       TEXT NOT NULL,           -- cited by AI, e.g. "CUR-E-001"
    version         INT NOT NULL DEFAULT 1,
    category        TEXT NOT NULL,           -- curriculum | vocab | grammar | siop | ...
    school_track    TEXT NOT NULL CHECK (school_track IN ('general', 'vocational', 'both')),
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    source_doc      TEXT NOT NULL DEFAULT '',
    source_version  TEXT NOT NULL DEFAULT '',
    source_page     TEXT NOT NULL DEFAULT '',
    source_sha256   TEXT NOT NULL DEFAULT '',
    approved_by     TEXT,
    approved_at     TIMESTAMPTZ,
    embedding       vector(1024),            -- reserved until an approved embedding model exists
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    retired_at      TIMESTAMPTZ,
    UNIQUE (item_code, version)
);
CREATE INDEX knowledge_items_track_idx ON knowledge_items (school_track) WHERE retired_at IS NULL;

-- Teachers' lesson-preparation experiences (Google sheet import). Not part of the
-- official knowledge base; shown under "其他老師怎麼教". in_rag stays false until the PI approves.
CREATE TABLE teacher_cases (
    id            BIGSERIAL PRIMARY KEY,
    case_code     TEXT NOT NULL UNIQUE,
    school_track  TEXT NOT NULL CHECK (school_track IN ('general', 'vocational', 'both')),
    grade         TEXT NOT NULL DEFAULT '',
    topic         TEXT NOT NULL DEFAULT '',
    summary       TEXT NOT NULL,
    details       TEXT NOT NULL DEFAULT '',
    source_sha256 TEXT NOT NULL DEFAULT '',
    approved      BOOLEAN NOT NULL DEFAULT false,
    in_rag        BOOLEAN NOT NULL DEFAULT false,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------- AI
CREATE TABLE prompt_versions (
    id          BIGSERIAL PRIMARY KEY,
    prompt_key  TEXT NOT NULL,          -- core_assist_l1 | core_assist_l2 | core_assist_l3 | plan_suggest
    version_no  INT NOT NULL,
    system_text TEXT NOT NULL,
    user_template TEXT NOT NULL,
    created_by  BIGINT REFERENCES teachers (id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (prompt_key, version_no)
);

CREATE TABLE scaffolds (
    id               BIGSERIAL PRIMARY KEY,
    scaffold_code    TEXT NOT NULL,
    version          INT NOT NULL DEFAULT 1,
    name             TEXT NOT NULL,
    category         TEXT NOT NULL CHECK (category IN
                       ('pck_externalisation', 'content_design_support', 'low_burden_evidence',
                        'evidence_based_reflection', 'teacher_control_fading')),
    process_stage    TEXT NOT NULL DEFAULT '',
    professional_task TEXT NOT NULL DEFAULT '',
    support_form     TEXT NOT NULL DEFAULT '',
    support_provider TEXT NOT NULL DEFAULT 'AI',
    use_level        TEXT NOT NULL DEFAULT 'individual',
    adaptation_rule  TEXT NOT NULL DEFAULT '',
    evidence         TEXT NOT NULL DEFAULT '',
    UNIQUE (scaffold_code, version)
);

CREATE TABLE ai_interactions (
    id                BIGSERIAL PRIMARY KEY,
    teacher_id        BIGINT NOT NULL REFERENCES teachers (id),
    module            TEXT NOT NULL,          -- core_assist | plan_suggest | reflect (phase 2)
    unit_id           BIGINT REFERENCES units (id),
    lesson_id         BIGINT REFERENCES lessons (id),
    scaffold_id       BIGINT REFERENCES scaffolds (id),
    prompt_version_id BIGINT REFERENCES prompt_versions (id),
    input_snapshot    JSONB NOT NULL,
    provider          TEXT NOT NULL,
    model             TEXT NOT NULL,
    model_version     TEXT,                   -- model id echoed by the provider
    attempt           INT NOT NULL DEFAULT 1,
    retry_of          BIGINT REFERENCES ai_interactions (id),   -- attempt 2 points at attempt 1
    output            JSONB,
    citations         TEXT[] NOT NULL DEFAULT '{}',
    check_results     JSONB,
    status            TEXT NOT NULL CHECK (status IN ('pending', 'shown', 'blocked', 'timeout', 'error')),
    error_detail      TEXT,
    latency_ms        INT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ai_interactions_teacher_idx ON ai_interactions (teacher_id, created_at);

CREATE TABLE teacher_decisions (
    id                 BIGSERIAL PRIMARY KEY,
    ai_interaction_id  BIGINT NOT NULL REFERENCES ai_interactions (id),
    teacher_id         BIGINT NOT NULL REFERENCES teachers (id),
    item_index         INT NOT NULL DEFAULT 0,   -- which suggestion inside the output
    decision           TEXT NOT NULL CHECK (decision IN
                         ('adopt', 'revise', 'reject', 'answer', 'skip', 'no_response')),
    revised_content    TEXT,
    reason             TEXT CHECK (reason IN
                         ('source_problem', 'curriculum_mismatch', 'not_feasible', 'unclear',
                          'better_idea', 'other')),
    reason_note        TEXT,
    thumbs             TEXT CHECK (thumbs IN ('up', 'down')),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE retrieval_logs (
    id                 BIGSERIAL PRIMARY KEY,
    ai_interaction_id  BIGINT REFERENCES ai_interactions (id),
    query_text         TEXT NOT NULL,
    school_track       TEXT NOT NULL,
    item_codes         TEXT[] NOT NULL DEFAULT '{}',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE core_assist_logs (
    id                 BIGSERIAL PRIMARY KEY,
    core_version_id    BIGINT NOT NULL REFERENCES core_versions (id),
    core_focus_id      BIGINT REFERENCES core_focuses (id),
    question_id        TEXT NOT NULL,
    level              INT NOT NULL CHECK (level BETWEEN 1 AND 3),
    ai_interaction_id  BIGINT REFERENCES ai_interactions (id),
    continued          BOOLEAN,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scaffold_events (
    id           BIGSERIAL PRIMARY KEY,
    teacher_id   BIGINT NOT NULL REFERENCES teachers (id),
    scaffold_id  BIGINT NOT NULL REFERENCES scaffolds (id),
    trigger      TEXT NOT NULL DEFAULT '',
    action       TEXT NOT NULL CHECK (action IN ('show', 'expand', 'use', 'skip')),
    dwell_ms     INT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------- reserved (phase 2+)
CREATE TABLE lesson_runs (
    id          BIGSERIAL PRIMARY KEY,
    lesson_id   BIGINT NOT NULL REFERENCES lessons (id),
    started_at  TIMESTAMPTZ NOT NULL,
    ended_at    TIMESTAMPTZ
);

CREATE TABLE record_events (
    id                BIGSERIAL PRIMARY KEY,
    lesson_run_id     BIGINT NOT NULL REFERENCES lesson_runs (id),
    device_event_id   TEXT NOT NULL,
    device_ts         TIMESTAMPTZ NOT NULL,
    polarity          TEXT NOT NULL CHECK (polarity IN ('good', 'stuck')),
    note              TEXT NOT NULL DEFAULT '',
    device_type       TEXT NOT NULL DEFAULT '',
    followup_text     TEXT,
    followup_at       TIMESTAMPTZ,
    activity_step_id  BIGINT REFERENCES lesson_steps (id),
    icap_cues         TEXT[] NOT NULL DEFAULT '{}',
    handling          TEXT,
    deleted_at        TIMESTAMPTZ,
    UNIQUE (lesson_run_id, device_event_id)
);

CREATE TABLE record_event_revisions (
    id               BIGSERIAL PRIMARY KEY,
    record_event_id  BIGINT NOT NULL REFERENCES record_events (id),
    change_type      TEXT NOT NULL CHECK (change_type IN ('undo', 'edit', 'delete')),
    before_value     JSONB,
    after_value      JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE reflections (
    id           BIGSERIAL PRIMARY KEY,
    lesson_id    BIGINT NOT NULL REFERENCES lessons (id),
    version_no   INT NOT NULL,
    body         TEXT NOT NULL DEFAULT '',
    followups    JSONB NOT NULL DEFAULT '[]',
    next_change  TEXT NOT NULL DEFAULT '',
    completed_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (lesson_id, version_no)
);

CREATE TABLE carry_forward_links (
    id              BIGSERIAL PRIMARY KEY,
    reflection_id   BIGINT NOT NULL REFERENCES reflections (id),
    target_step_id  BIGINT REFERENCES lesson_steps (id),
    implemented     BOOLEAN,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE progress_logs (
    id          BIGSERIAL PRIMARY KEY,
    teacher_id  BIGINT NOT NULL REFERENCES teachers (id),
    payload     JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE unit_materials (
    id           BIGSERIAL PRIMARY KEY,
    unit_id      BIGINT NOT NULL REFERENCES units (id) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    content      BYTEA,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE unit_material_chunks (
    id                BIGSERIAL PRIMARY KEY,
    unit_material_id  BIGINT NOT NULL REFERENCES unit_materials (id) ON DELETE CASCADE,
    page              TEXT,
    text              TEXT NOT NULL,
    embedding         vector(1024)
);

CREATE TABLE notifications (
    id          BIGSERIAL PRIMARY KEY,
    teacher_id  BIGINT NOT NULL REFERENCES teachers (id),
    kind        TEXT NOT NULL,
    state       TEXT NOT NULL DEFAULT 'pending',
    email_on    BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE export_logs (
    id           BIGSERIAL PRIMARY KEY,
    exported_by  BIGINT NOT NULL REFERENCES teachers (id),
    scope        JSONB NOT NULL,
    file_id      TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE access_logs (
    id          BIGSERIAL PRIMARY KEY,
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    granted_until TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE system_versions (
    id          BIGSERIAL PRIMARY KEY,
    version     TEXT NOT NULL,
    released_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE usage_logs (
    id          BIGSERIAL PRIMARY KEY,
    teacher_id  BIGINT REFERENCES teachers (id),
    event       TEXT NOT NULL,
    payload     JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE eval_sets (
    id          BIGSERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    version     INT NOT NULL,
    payload     JSONB NOT NULL
);

CREATE TABLE eval_runs (
    id                BIGSERIAL PRIMARY KEY,
    eval_set_id       BIGINT NOT NULL REFERENCES eval_sets (id),
    provider          TEXT NOT NULL,
    model_version     TEXT NOT NULL,
    prompt_version_id BIGINT REFERENCES prompt_versions (id),
    output            JSONB,
    score             JSONB,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE feedback (
    id          BIGSERIAL PRIMARY KEY,
    teacher_id  BIGINT REFERENCES teachers (id),
    body        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE design_changes (
    id          BIGSERIAL PRIMARY KEY,
    description TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE expert_ratings (
    id                 BIGSERIAL PRIMARY KEY,
    ai_interaction_id  BIGINT NOT NULL REFERENCES ai_interactions (id),
    rater_id           BIGINT NOT NULL REFERENCES teachers (id),
    rubric             JSONB NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
