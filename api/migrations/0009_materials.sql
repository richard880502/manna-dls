-- Manna DLS Phase 2.2: 教材庫、原檔儲存與教師注記. Contract in docs/API.md「Phase 2.2：教材與注記」;
-- product rules in 《學習單產生器：教師選項與 SOP 整合規格》 §4.2, §4.4, §9.7 and
-- docs/BLUEPRINT.md §13.3 (materials belong to one teacher; deleting one physically deletes
-- the original file, the extracted text, its paragraphs and every annotation).
--
-- What changes for the lesson SOP: `lesson_sop_materials` was append-only, so text a teacher
-- removed or rewrote stayed in the database. It is replaced by the teacher's material library:
-- a pasted material is edited in place (no earlier text is kept) and SOP versions point at it.

-- ---------------------------------------------------------------- the material library
-- One row per material. The original file (if any) is one private object in the storage
-- backend (`storage_key`); everything derived from it lives in this row, so deleting the row
-- and the object removes all of it. `paragraphs` is [{ "id": "p3", "no": 1, "text": "…",
-- "page": 2 }] (`page` only for PDFs), split by the server. Ids are stable while a paragraph's
-- text is unchanged and never reused within a material (`paragraph_seq`).
CREATE TABLE materials (
    id             BIGSERIAL PRIMARY KEY,
    teacher_id     BIGINT NOT NULL REFERENCES teachers (id),
    kind           TEXT NOT NULL CHECK (kind IN ('text', 'document', 'image', 'audio', 'video', 'link')),
    origin         TEXT NOT NULL CHECK (origin IN ('paste', 'upload', 'url')),
    title          TEXT NOT NULL DEFAULT '',
    source_note    TEXT NOT NULL DEFAULT '',
    url            TEXT,                        -- origin = 'url': the link itself, never fetched
    filename       TEXT,                        -- origin = 'upload': the name the teacher's file had
    ext            TEXT,
    content_type   TEXT,                        -- decided by the server from the allowed list
    size_bytes     BIGINT NOT NULL DEFAULT 0,   -- the file, or the pasted text in UTF-8
    sha256         TEXT,                        -- of the file, or of the pasted text
    storage_key    TEXT UNIQUE,
    text_status    TEXT NOT NULL CHECK (text_status IN ('ready', 'no_text', 'failed', 'not_applicable')),
    text           TEXT NOT NULL DEFAULT '',
    text_truncated BOOLEAN NOT NULL DEFAULT false,
    paragraphs     JSONB NOT NULL DEFAULT '[]',
    paragraph_seq  INT NOT NULL DEFAULT 0,
    page_count     INT,
    revision       INT NOT NULL DEFAULT 1,      -- +1 each time the text changes (pasted text only)
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    legacy_lesson_id BIGINT                     -- only used by the data migration below
);
CREATE INDEX materials_teacher_idx ON materials (teacher_id, id);

-- Where a material is used. Many-to-many: one material can serve several lessons and
-- worksheets. `target_kind = 'worksheet'` rows are written by the worksheet generator through
-- `materials::store::link`; there is no foreign key because that table arrives separately.
CREATE TABLE material_links (
    id           BIGSERIAL PRIMARY KEY,
    material_id  BIGINT NOT NULL REFERENCES materials (id) ON DELETE CASCADE,
    target_kind  TEXT NOT NULL CHECK (target_kind IN ('lesson', 'worksheet')),
    target_id    BIGINT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (material_id, target_kind, target_id)
);
CREATE INDEX material_links_target_idx ON material_links (target_kind, target_id);

-- Teacher notes on a span of a material's text. Stored apart from the material and never
-- written into it. The anchor is the paragraph id plus UTF-16 offsets inside that paragraph
-- (what a browser selection gives), and the quoted text with a little context on both sides so
-- the note can be found again after the text changed. `share_with_ai` is off unless the
-- teacher turns it on; only such notes are ever handed to a model.
CREATE TABLE material_annotations (
    id             BIGSERIAL PRIMARY KEY,
    material_id    BIGINT NOT NULL REFERENCES materials (id) ON DELETE CASCADE,
    teacher_id     BIGINT NOT NULL REFERENCES teachers (id),
    paragraph_id   TEXT NOT NULL,
    start_offset   INT NOT NULL,
    end_offset     INT NOT NULL,
    quote          TEXT NOT NULL,
    prefix         TEXT NOT NULL DEFAULT '',
    suffix         TEXT NOT NULL DEFAULT '',
    note           TEXT NOT NULL DEFAULT '',
    share_with_ai  BOOLEAN NOT NULL DEFAULT false,
    hidden         BOOLEAN NOT NULL DEFAULT false,
    material_revision INT NOT NULL DEFAULT 1,   -- the revision the anchor was made against
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX material_annotations_material_idx ON material_annotations (material_id, id);

-- What remains of a deleted material: that it existed and when it went. No title, no file name,
-- no hash, no text.
CREATE TABLE material_deletions (
    id           BIGSERIAL PRIMARY KEY,
    material_id  BIGINT NOT NULL,
    teacher_id   BIGINT NOT NULL REFERENCES teachers (id),
    kind         TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL,
    had_file     BOOLEAN NOT NULL,
    annotations  INT NOT NULL,
    deleted_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------- lesson SOP: move to the library
-- The newest text of every lesson that still has a material becomes one library material of the
-- lesson's teacher (paragraph ids and their counter carried over, so the SOP's paragraph
-- references stay valid). Earlier texts, and the texts of lessons whose material the teacher had
-- already removed, are not carried over: they are dropped with the old table.
INSERT INTO materials (teacher_id, kind, origin, title, source_note, size_bytes, sha256,
                       text_status, text, paragraphs, paragraph_seq, revision,
                       created_at, updated_at, legacy_lesson_id)
SELECT u.teacher_id, 'text', 'paste', m.title, m.source_note,
       octet_length(m.text), encode(sha256(convert_to(m.text, 'UTF8')), 'hex'),
       'ready', m.text, m.paragraphs, m.paragraph_seq, m.version_no,
       (SELECT MIN(f.created_at) FROM lesson_sop_materials f WHERE f.lesson_id = m.lesson_id),
       m.created_at, m.lesson_id
  FROM lesson_sop_materials m
  JOIN lessons l ON l.id = m.lesson_id
  JOIN units u ON u.id = l.unit_id
 WHERE m.id = (SELECT v.material_id FROM lesson_sop_versions v
                WHERE v.lesson_id = m.lesson_id ORDER BY v.version_no DESC LIMIT 1);

ALTER TABLE lesson_sop_versions
    ADD COLUMN library_material_id BIGINT REFERENCES materials (id) ON DELETE SET NULL,
    ADD COLUMN material_revision INT,
    -- true: this version had a material and the teacher has since deleted it (「教材已刪除」)
    ADD COLUMN material_deleted BOOLEAN NOT NULL DEFAULT false;

UPDATE lesson_sop_versions v
   SET library_material_id = nm.id, material_revision = old.version_no
  FROM lesson_sop_materials old
  JOIN materials nm ON nm.legacy_lesson_id = old.lesson_id
 WHERE v.material_id = old.id;
-- Versions whose text is not carried over keep only the tombstone.
UPDATE lesson_sop_versions
   SET material_deleted = true
 WHERE material_id IS NOT NULL AND library_material_id IS NULL;

INSERT INTO material_links (material_id, target_kind, target_id)
SELECT id, 'lesson', legacy_lesson_id FROM materials WHERE legacy_lesson_id IS NOT NULL;

ALTER TABLE lesson_sop_versions DROP COLUMN material_id;
ALTER TABLE lesson_sop_versions RENAME COLUMN library_material_id TO material_id;
CREATE INDEX lesson_sop_versions_material_idx ON lesson_sop_versions (material_id);
DROP TABLE lesson_sop_materials;
ALTER TABLE materials DROP COLUMN legacy_lesson_id;

-- Which material an AI job read, so that deleting the material can take the verbatim quotes
-- out of the job's result. No foreign key: the id outlives nothing, it is cleared on delete.
ALTER TABLE lesson_sop_jobs ADD COLUMN material_id BIGINT;
UPDATE lesson_sop_jobs j SET material_id = v.material_id
  FROM lesson_sop_versions v WHERE v.id = j.sop_version_id;

-- ---------------------------------------------------------------- AI logs: no material text
-- The objectives stage logged its curriculum search query, which included up to 2,000
-- characters of the material. From now on it logs the length and a hash; what was already
-- logged is replaced the same way.
UPDATE retrieval_logs r
   SET query_text = '［查詢文字含教材內容，已移除：' || char_length(r.query_text) || ' 字，sha256 '
                    || left(encode(sha256(convert_to(r.query_text, 'UTF8')), 'hex'), 16) || '］'
  FROM ai_interactions a
 WHERE a.id = r.ai_interaction_id AND a.module = 'sop_objectives';
UPDATE ai_interactions
   SET input_snapshot = jsonb_set(
         input_snapshot, '{retrieval,query}',
         to_jsonb('［查詢文字含教材內容，已移除：' || char_length(input_snapshot #>> '{retrieval,query}')
                  || ' 字，sha256 '
                  || left(encode(sha256(convert_to(input_snapshot #>> '{retrieval,query}', 'UTF8')), 'hex'), 16)
                  || '］'))
 WHERE module = 'sop_objectives' AND input_snapshot #>> '{retrieval,query}' IS NOT NULL;
