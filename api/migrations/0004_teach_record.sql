-- Manna DLS Phase 1.2: Teach & Record (課中書籤) and post-lesson review (課後補寫).
-- Spec §4.3, §3 (lesson_runs, record_events, record_event_revisions), §6.4, §7.1; contract in
-- docs/API.md「Phase 1.2」. The three tables were reserved (and never written) in 0001; this
-- migration fills in the columns and rules they need.

-- ---------------------------------------------------------------- lesson runs
ALTER TABLE lesson_runs
    ALTER COLUMN started_at SET DEFAULT now(),
    -- The plan version in use when the teacher pressed 開始本節課 (research trace).
    ADD COLUMN lesson_version_id   BIGINT REFERENCES lesson_versions (id),
    -- 完成課後補寫. Never required (§6.4): it only records that the teacher pressed it.
    ADD COLUMN review_completed_at TIMESTAMPTZ,
    ADD COLUMN created_at          TIMESTAMPTZ NOT NULL DEFAULT now();
-- One open run per lesson at a time.
CREATE UNIQUE INDEX lesson_runs_one_open_idx ON lesson_runs (lesson_id) WHERE ended_at IS NULL;
CREATE INDEX lesson_runs_lesson_idx ON lesson_runs (lesson_id);

-- ---------------------------------------------------------------- phone as remote (QR pairing)
-- A pairing code is shown as a QR code on the teacher's screen. It is random, stored only as a
-- SHA-256 hash, valid for 10 minutes, single use and revocable.
CREATE TABLE run_pairings (
    id             BIGSERIAL PRIMARY KEY,
    lesson_run_id  BIGINT NOT NULL REFERENCES lesson_runs (id),
    code_hash      TEXT NOT NULL UNIQUE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,
    used_at        TIMESTAMPTZ,
    revoked_at     TIMESTAMPTZ
);
CREATE INDEX run_pairings_run_idx ON run_pairings (lesson_run_id);

-- A paired phone/tablet. Its token (hash only) can add bookmarks to this one run and undo its
-- own, until the run ends (plus a late-sync window for bookmarks tapped before the end).
CREATE TABLE run_remotes (
    id             BIGSERIAL PRIMARY KEY,
    lesson_run_id  BIGINT NOT NULL REFERENCES lesson_runs (id),
    pairing_id     BIGINT NOT NULL UNIQUE REFERENCES run_pairings (id),
    token_hash     TEXT NOT NULL UNIQUE,
    device_type    TEXT NOT NULL CHECK (device_type IN ('phone', 'tablet', 'desktop')),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at   TIMESTAMPTZ,
    revoked_at     TIMESTAMPTZ
);
CREATE INDEX run_remotes_run_idx ON run_remotes (lesson_run_id);

-- ---------------------------------------------------------------- bookmarks
-- record_events keeps the latest state of each bookmark; device_event_id and device_ts come
-- from the device and are kept as sent. icap_cues holds the observable student behaviours
-- (API field `behaviors`).
ALTER TABLE record_events
    ALTER COLUMN device_type DROP DEFAULT,
    ADD COLUMN received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN via         TEXT NOT NULL DEFAULT 'console' CHECK (via IN ('console', 'pip', 'remote')),
    ADD COLUMN remote_id   BIGINT REFERENCES run_remotes (id),
    ADD CONSTRAINT record_events_device_type_check
        CHECK (device_type IN ('phone', 'tablet', 'desktop')),
    ADD CONSTRAINT record_events_handling_check
        CHECK (handling IN ('re_explain', 'add_example', 'adjust_questions', 'change_activity',
                            'adjust_time', 'defer')),
    ADD CONSTRAINT record_events_icap_cues_check
        CHECK (icap_cues <@ ARRAY['listening', 'note_taking', 'answering', 'asking', 'explaining',
                                  'discussing', 'probing', 'co_completing', 'other']::TEXT[]);
CREATE INDEX record_events_run_idx ON record_events (lesson_run_id, device_ts);

-- Every undo / edit / delete / review append a revision with the before and after values; the
-- original bookmark is the first revision's before_value (or the row itself if never changed).
-- client_op_id makes retried offline operations idempotent.
ALTER TABLE record_event_revisions
    DROP CONSTRAINT record_event_revisions_change_type_check,
    ADD CONSTRAINT record_event_revisions_change_type_check
        CHECK (change_type IN ('undo', 'edit', 'delete', 'review')),
    ADD COLUMN actor        TEXT NOT NULL DEFAULT 'teacher' CHECK (actor IN ('teacher', 'remote')),
    ADD COLUMN client_op_id TEXT,
    ADD COLUMN device_ts    TIMESTAMPTZ;      -- device time of the change, when the device sent it
CREATE INDEX record_event_revisions_event_idx ON record_event_revisions (record_event_id);
CREATE UNIQUE INDEX record_event_revisions_op_idx
    ON record_event_revisions (record_event_id, client_op_id) WHERE client_op_id IS NOT NULL;

-- ---------------------------------------------------------------- scaffold
INSERT INTO scaffolds (scaffold_code, version, name, category, process_stage, professional_task,
                       support_form, support_provider, use_level, adaptation_rule, evidence) VALUES
('SC-RECORD', 1, '課中書籤與課後補寫', 'low_burden_evidence',
 '課中（Teach & Record）與課後補寫',
 '在不打斷教學的情況下留下課堂證據，課後再補上脈絡',
 '兩顆大按鈕（順利／卡住，固定圖示）一按即記下裝置時間與好壞；可加一句話（數秒未動作自動收起）；手機掃 QR Code 當遙控器，電腦可開子母畫面小視窗；課後依時間列出書籤，旁列教案步驟，選填活動歸屬、學生可觀察行為、處理方式與補寫',
 'AI', 'individual',
 '課中不呼叫 AI、不錄音、不拍照；書籤先存裝置端、恢復連線立即同步；加一句話未動作自動收起、不擋下一次標記；全部可略過，沒有書籤或未補寫都不阻擋完成（§6.4）',
 'lesson_runs、record_events（裝置端 ID 與時間戳、裝置類型、來源）、record_event_revisions（復原、修改、刪除、補寫）、scaffold_events（顯示、使用、QR 遙控、子母畫面）');
