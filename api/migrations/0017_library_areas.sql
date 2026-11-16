-- Manna DLS Phase 2.9: 教材參考、我的教材、學習單範本 (three library areas). Contract in docs/API.md
-- 「Phase 2.9：教材參考、我的教材、學習單範本」.
--
-- Until now every material and template belonged to one teacher. The platform now has two
-- reference areas that every teacher can read and use: 「教材參考」 (the knowledge base's texts:
-- curriculum, vocabulary list, past exam papers) and 「學習單範本」 (worksheet samples and the
-- templates read from them). Admins (`pi`, `kb_manager`) upload and manage them; the uploading
-- admin stays in `teacher_id` for audit. Teachers' own uploads stay personal (「我的教材」), and
-- their worksheet samples move out of that list into 「我的範本」.

-- ---------------------------------------------------------------- materials
-- `scope`: `personal` (only the owner can see it, as before) or `platform` (every teacher can
-- read and use it; only ManageKnowledge can change or delete it).
-- `purpose`: `material` (a text to teach with) or `worksheet_sample` (an example of a worksheet,
-- the source of a template). Samples never appear in the material lists.
-- `category`: free for personal rows (usually empty); platform rows use a fixed list checked by
-- the server (materials: 學測英文…其他; samples: 字彙…其他).
-- `tags`: free tags such as 114, 試題, 三民 (≤20 characters each, at most 12, checked by the server).
ALTER TABLE materials
    ADD COLUMN scope    TEXT NOT NULL DEFAULT 'personal' CHECK (scope IN ('personal', 'platform')),
    ADD COLUMN purpose  TEXT NOT NULL DEFAULT 'material'
                        CHECK (purpose IN ('material', 'worksheet_sample')),
    ADD COLUMN category TEXT NOT NULL DEFAULT '' CHECK (char_length(category) <= 50),
    ADD COLUMN tags     TEXT[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 12);

-- Browsing the platform areas (newest first, keyset cursor).
CREATE INDEX materials_scope_idx ON materials (scope, purpose, created_at DESC, id DESC);

-- Backfill: a material a template was made from is a worksheet sample.
UPDATE materials
   SET purpose = 'worksheet_sample'
 WHERE id IN (SELECT source_material_id FROM worksheet_templates
               WHERE source_material_id IS NOT NULL);

-- ---------------------------------------------------------------- worksheet templates
-- Same `scope` as materials; a platform template comes from a platform sample material.
-- `category` uses the sample list (empty: the server suggests one when the extraction finishes);
-- `tags` as for materials.
ALTER TABLE worksheet_templates
    ADD COLUMN scope    TEXT NOT NULL DEFAULT 'personal' CHECK (scope IN ('personal', 'platform')),
    ADD COLUMN category TEXT NOT NULL DEFAULT '' CHECK (char_length(category) <= 50),
    ADD COLUMN tags     TEXT[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 12);

CREATE INDEX worksheet_templates_scope_idx
    ON worksheet_templates (scope, created_at DESC, id DESC);
