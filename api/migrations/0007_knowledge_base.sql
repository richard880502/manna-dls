-- Manna DLS Phase 2.0: the real knowledge base (108 curriculum items) and the CEEC vocabulary list.
-- Contract: docs/API.md「Phase 2.0：知識庫與詞彙」.
--
-- * Curriculum items keep their official code, kind (學習表現／學習內容), strand, stage, subject
--   and where they came from (document, version, page, SHA-256 of the source PDF, import batch).
-- * school_track gains 'comprehensive' (綜合型高中) everywhere a track is stored, so a
--   comprehensive-school teacher, class and knowledge item can exist and agree with each other.
-- * vocabulary_items holds the 大考中心 高中英文參考詞彙表, one row per matchable word form.
-- * A new role, kb_manager（知識庫管理員）, manages the knowledge base without the PI's other
--   powers.
-- * Nothing is deleted: a changed row becomes a new version and the old one is retired.
--   The seeded DEMO-* items are NOT retired here; the PI retires them explicitly
--   (POST /api/admin/knowledge/retire-demo) once real items are in.
-- No curriculum or vocabulary data is shipped in the repo (licensing unconfirmed): the tables
-- start empty and are filled through the admin import.

-- ---------------------------------------------------------------- school tracks
ALTER TABLE teachers DROP CONSTRAINT teachers_school_track_check;
ALTER TABLE teachers ADD CONSTRAINT teachers_school_track_check
    CHECK (school_track IN ('general', 'vocational', 'comprehensive'));

ALTER TABLE invite_codes DROP CONSTRAINT invite_codes_school_track_check;
ALTER TABLE invite_codes ADD CONSTRAINT invite_codes_school_track_check
    CHECK (school_track IN ('general', 'vocational', 'comprehensive'));

ALTER TABLE classes DROP CONSTRAINT classes_school_track_check;
ALTER TABLE classes ADD CONSTRAINT classes_school_track_check
    CHECK (school_track IN ('general', 'vocational', 'comprehensive'));

-- 'both' keeps meaning "shared by every school type".
ALTER TABLE knowledge_items DROP CONSTRAINT knowledge_items_school_track_check;
ALTER TABLE knowledge_items ADD CONSTRAINT knowledge_items_school_track_check
    CHECK (school_track IN ('general', 'vocational', 'comprehensive', 'both'));

ALTER TABLE teacher_cases DROP CONSTRAINT teacher_cases_school_track_check;
ALTER TABLE teacher_cases ADD CONSTRAINT teacher_cases_school_track_check
    CHECK (school_track IN ('general', 'vocational', 'comprehensive', 'both'));

-- ---------------------------------------------------------------- roles
-- kb_manager（知識庫管理員）: may import, retire and browse the knowledge base and the vocabulary
-- list, nothing else of the PI's (see auth::role_can and docs/API.md「角色與權限」).
ALTER TABLE teachers DROP CONSTRAINT teachers_role_check;
ALTER TABLE teachers ADD CONSTRAINT teachers_role_check
    CHECK (role IN ('teacher', 'pi', 'kb_manager', 'ra', 'rater', 'engineer'));

ALTER TABLE invite_codes DROP CONSTRAINT invite_codes_role_check;
ALTER TABLE invite_codes ADD CONSTRAINT invite_codes_role_check
    CHECK (role IN ('teacher', 'pi', 'kb_manager', 'ra', 'rater', 'engineer'));

-- ---------------------------------------------------------------- import batches
-- One row per committed CSV import (dry runs write nothing).
CREATE TABLE import_batches (
    id           BIGSERIAL PRIMARY KEY,
    kind         TEXT NOT NULL CHECK (kind IN ('knowledge', 'vocabulary')),
    file_sha256  TEXT NOT NULL,              -- SHA-256 of the uploaded CSV
    row_count    INT NOT NULL,
    inserted     INT NOT NULL,               -- codes / word forms seen for the first time
    updated      INT NOT NULL,               -- changed rows (new version, old one retired)
    unchanged    INT NOT NULL,
    imported_by  BIGINT REFERENCES teachers (id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------- knowledge items
-- source_sha256: SHA-256 of the source document when the CSV carries it (curriculum PDFs);
-- for CSVs without that column it stays the SHA-256 of the CSV, as before.
ALTER TABLE knowledge_items
    ADD COLUMN official_code   TEXT NOT NULL DEFAULT '',   -- as printed, e.g. 3-Ⅴ-7
    ADD COLUMN item_kind       TEXT NOT NULL DEFAULT ''
        CHECK (item_kind IN ('', 'learning_performance', 'learning_content')),
    ADD COLUMN strand          TEXT NOT NULL DEFAULT '',   -- e.g. 3, Ae, 英語V
    ADD COLUMN strand_name     TEXT NOT NULL DEFAULT '',   -- e.g. 語言能力（讀）
    ADD COLUMN stage           TEXT NOT NULL DEFAULT '',   -- 學習階段: II | III | IV | V ('' = n/a)
    ADD COLUMN subject         TEXT NOT NULL DEFAULT '',   -- e.g. 英語文, 英語, 日語
    ADD COLUMN advanced        BOOLEAN NOT NULL DEFAULT false,  -- * 較高階
    ADD COLUMN recurring       BOOLEAN NOT NULL DEFAULT false,  -- ◎ 跨階段重複
    ADD COLUMN import_batch_id BIGINT REFERENCES import_batches (id);

CREATE INDEX knowledge_items_official_code_idx ON knowledge_items (official_code)
    WHERE retired_at IS NULL AND official_code <> '';
CREATE INDEX knowledge_items_category_idx ON knowledge_items (category, school_track)
    WHERE retired_at IS NULL;

-- ---------------------------------------------------------------- vocabulary
-- One row per matchable word form of one printed entry (a/an -> a, an; agree(ment) -> agree,
-- agreement). kind = appendix: the list's unlevelled appendix words (numbers, months, countries…).
CREATE TABLE vocabulary_items (
    id                BIGSERIAL PRIMARY KEY,
    word              TEXT NOT NULL,              -- the form to match
    headword          TEXT NOT NULL,              -- the entry as printed, e.g. a/an
    level             SMALLINT CHECK (level BETWEEN 1 AND 6),
    pos               TEXT NOT NULL DEFAULT '',   -- as printed, e.g. n./v.
    kind              TEXT NOT NULL CHECK (kind IN ('base', 'variant', 'derived', 'appendix')),
    appendix_category TEXT NOT NULL DEFAULT '',
    list_version      TEXT NOT NULL,              -- e.g. 大考中心 高中英文參考詞彙表（111學年度起適用），109年7月
    source_doc        TEXT NOT NULL DEFAULT '',
    source_page       TEXT NOT NULL DEFAULT '',
    source_sha256     TEXT NOT NULL DEFAULT '',   -- SHA-256 of the source PDF
    version           INT NOT NULL DEFAULT 1,
    import_batch_id   BIGINT REFERENCES import_batches (id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    retired_at        TIMESTAMPTZ,
    CHECK ((kind = 'appendix') = (level IS NULL)),
    -- appendix_category is part of the identity: a word can sit in two appendix lists
    UNIQUE (list_version, word, headword, pos, kind, appendix_category, version)
);
CREATE INDEX vocabulary_items_active_idx ON vocabulary_items (list_version) WHERE retired_at IS NULL;
