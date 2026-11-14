-- Manna DLS Phase 1.4: classes (班級) and the design library (備課庫).
-- Contract: docs/API.md「Phase 1.4：班級與備課庫」.
--
-- A teacher teaches several classes; each class takes several unit designs from the teacher's
-- library, and one unit design can be taken by several classes (many-to-many, ordered per class).
-- Teaching records (lesson runs with their bookmarks and review) belong to one class.
-- No student data, ever: a class is a name, a few labels and a free-text profile.

-- ---------------------------------------------------------------- classes
CREATE TABLE classes (
    id                   BIGSERIAL PRIMARY KEY,
    teacher_id           BIGINT NOT NULL REFERENCES teachers (id),
    name                 TEXT NOT NULL,                 -- e.g. 高一 3 班
    grade                TEXT NOT NULL DEFAULT '',      -- e.g. 高一
    school_track         TEXT NOT NULL CHECK (school_track IN ('general', 'vocational')),
    course_label         TEXT NOT NULL DEFAULT '',      -- optional, e.g. 英文閱讀選修
    class_profile        TEXT NOT NULL DEFAULT '',      -- 班級特性, no student data
    -- teacher: typed or edited on create; preference: copied from the preference answer P6
    -- (first class only); migration: copied from a legacy unit by this migration; none: empty.
    class_profile_source TEXT NOT NULL DEFAULT 'none'
        CHECK (class_profile_source IN ('teacher', 'preference', 'migration', 'none')),
    school_year          TEXT NOT NULL DEFAULT '',      -- optional, e.g. 115
    term                 TEXT NOT NULL DEFAULT '',      -- optional, e.g. 上學期
    position             INT NOT NULL DEFAULT 0,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at          TIMESTAMPTZ                    -- archive instead of delete
);
CREATE INDEX classes_teacher_idx ON classes (teacher_id, position, id);

-- ---------------------------------------------------------------- 取用: class ↔ unit design
-- Removing a unit from a class is a soft removal (removed_at); the runs taught in that class
-- stay. Adding it again inserts a new row, so the history of assignments is kept.
CREATE TABLE class_units (
    id          BIGSERIAL PRIMARY KEY,
    class_id    BIGINT NOT NULL REFERENCES classes (id),
    unit_id     BIGINT NOT NULL REFERENCES units (id),
    position    INT NOT NULL DEFAULT 0,
    note        TEXT NOT NULL DEFAULT '',               -- per-class note, e.g. 這班用基礎版
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_at  TIMESTAMPTZ
);
CREATE UNIQUE INDEX class_units_active_idx ON class_units (class_id, unit_id) WHERE removed_at IS NULL;
CREATE INDEX class_units_unit_idx ON class_units (unit_id) WHERE removed_at IS NULL;

-- ---------------------------------------------------------------- runs belong to a class
-- NULL: a run without a class (a unit that no class uses; starting a lesson is never blocked).
ALTER TABLE lesson_runs ADD COLUMN class_id BIGINT REFERENCES classes (id);
CREATE INDEX lesson_runs_class_idx ON lesson_runs (class_id);
-- The same lesson can be open in class A and class B at once: one open run per lesson and class.
DROP INDEX lesson_runs_one_open_idx;
CREATE UNIQUE INDEX lesson_runs_one_open_idx
    ON lesson_runs (lesson_id, COALESCE(class_id, 0)) WHERE ended_at IS NULL;

-- ---------------------------------------------------------------- data migration
-- Every teacher who already has units gets one default class 「我的班級」: its profile is the
-- class profile of their most recently created unit that has one, all their units are assigned
-- to it (oldest first) and their existing runs are attached to it. It only touches teachers who
-- have units and no class yet, so running it again changes nothing (the integration test
-- executes exactly this block against legacy-shaped data).
-- >>> legacy-data-migration
INSERT INTO classes (teacher_id, name, school_track, class_profile, class_profile_source)
SELECT t.id, '我的班級', t.school_track,
       COALESCE(p.class_profile, ''),
       CASE WHEN p.class_profile IS NULL THEN 'none' ELSE 'migration' END
  FROM teachers t
  LEFT JOIN LATERAL (
        SELECT btrim(u.class_profile) AS class_profile
          FROM units u
         WHERE u.teacher_id = t.id AND u.deleted_at IS NULL AND btrim(u.class_profile) <> ''
         ORDER BY u.created_at DESC, u.id DESC
         LIMIT 1) p ON true
 WHERE EXISTS (SELECT 1 FROM units u WHERE u.teacher_id = t.id AND u.deleted_at IS NULL)
   AND NOT EXISTS (SELECT 1 FROM classes c WHERE c.teacher_id = t.id);

INSERT INTO class_units (class_id, unit_id, position)
SELECT c.id, u.id, ROW_NUMBER() OVER (PARTITION BY c.id ORDER BY u.created_at, u.id)
  FROM classes c
  JOIN units u ON u.teacher_id = c.teacher_id AND u.deleted_at IS NULL
 WHERE c.class_profile_source IN ('migration', 'none') AND c.name = '我的班級'
   AND (SELECT COUNT(*) FROM classes c2 WHERE c2.teacher_id = c.teacher_id) = 1
   AND NOT EXISTS (SELECT 1 FROM class_units cu WHERE cu.class_id = c.id);

UPDATE lesson_runs r
   SET class_id = cu.class_id
  FROM lessons l
  JOIN class_units cu ON cu.unit_id = l.unit_id AND cu.removed_at IS NULL
  JOIN classes c ON c.id = cu.class_id
 WHERE l.id = r.lesson_id
   AND r.class_id IS NULL
   AND c.name = '我的班級'
   AND (SELECT COUNT(*) FROM classes c2 WHERE c2.teacher_id = c.teacher_id) = 1
   AND (SELECT COUNT(*) FROM class_units cu2 WHERE cu2.unit_id = l.unit_id AND cu2.removed_at IS NULL) = 1;
-- <<< legacy-data-migration
