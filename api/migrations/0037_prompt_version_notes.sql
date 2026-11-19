-- Manna DLS: 提示詞版本的說明與還原來源（後台管理 → 提示詞版本）.
--
-- `prompt_versions` stays append-only: a version is never edited or deleted, and going back to an
-- older one writes a NEW version with the old text. `created_by` already existed (0001); the page
-- now fills it. `note` is what the author says changed (at most 200 characters, enforced by the
-- API); `restored_from` is the version_no of the version a restore copied.
ALTER TABLE prompt_versions
    ADD COLUMN note TEXT,
    ADD COLUMN restored_from INT;
