-- Phase 2.10 (docs/API.md「掃描檔與 OCR」): scanned PDFs, photos of pages and PDFs with a broken
-- text layer are read by the chandra-ocr-2 model in the background.
--
-- text_source      where `text` came from: the file (or pasted) or OCR.
-- ocr_status       none | queued | running | done | failed. While a run is open the material's
--                  text_status stays what it was.
-- ocr_*_pages      progress of the current run (pages read, pages to read).
-- ocr_error        why the run failed; for a finished run, a note (pages failed or skipped).
-- ocr_pages        the raw result: [{ page, blocks: [{ label, text, bbox }], pictures, error? }].
-- ocr_started_at   when the current run started; it also identifies the run (a run taken over
--                  after a restart gets a new one, and the old job stops writing).
ALTER TABLE materials
    ADD COLUMN text_source TEXT NOT NULL DEFAULT 'file'
        CHECK (text_source IN ('file', 'ocr')),
    ADD COLUMN ocr_status TEXT NOT NULL DEFAULT 'none'
        CHECK (ocr_status IN ('none', 'queued', 'running', 'done', 'failed')),
    ADD COLUMN ocr_done_pages INT NOT NULL DEFAULT 0,
    ADD COLUMN ocr_total_pages INT NOT NULL DEFAULT 0,
    ADD COLUMN ocr_error TEXT,
    ADD COLUMN ocr_pages JSONB,
    ADD COLUMN ocr_started_at TIMESTAMPTZ;

-- Startup recovery and the stale check look for open runs only.
CREATE INDEX materials_ocr_open_idx ON materials (ocr_status)
    WHERE ocr_status IN ('queued', 'running');
