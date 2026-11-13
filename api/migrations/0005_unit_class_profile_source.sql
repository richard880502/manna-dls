-- Manna DLS: a new unit's class profile carries over (docs/UX_REVIEW.md R3, decision D2).
-- When POST /api/units has no class_profile, the default is the teacher's most recent unit with a
-- non-empty class profile; only a teacher with no such unit falls back to the preference survey
-- answer (P6). Where the text came from is recorded when the unit is created, so research can
-- tell the teacher's own words from a carried-over default. Units created earlier keep NULL.

ALTER TABLE units
    -- teacher: sent explicitly on create (the teacher typed or edited it)
    -- previous_unit: copied from class_profile_source_unit_id
    -- preference: copied from the latest preference answers (P6)
    -- none: no default was available, the profile started empty
    ADD COLUMN class_profile_source TEXT
        CHECK (class_profile_source IN ('teacher', 'previous_unit', 'preference', 'none')),
    ADD COLUMN class_profile_source_unit_id BIGINT REFERENCES units (id),
    ADD CONSTRAINT units_class_profile_source_unit_chk
        CHECK ((class_profile_source = 'previous_unit') = (class_profile_source_unit_id IS NOT NULL));
