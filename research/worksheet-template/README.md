# Worksheet template probe

Scripts behind `docs/WORKSHEET_TEMPLATE_PROBE.md`. No data is committed here: the example worksheets are copyrighted and include teacher names, so they stay with the research team.

- `extract_main.rs` + `extract_Cargo.toml`: a small binary that turns DOCX/PDF/TXT worksheets into plain text with the platform's own extractor (`manna_api::materials::extract`), so the probe sees exactly what an upload would. To build it, put them in a folder as `src/main.rs` and `Cargo.toml` and fix the `path` to `api/`.
- `probe.py`: calls NCHC `gemma-4-31B-it` (OpenAI-compatible, json_schema output).
  - Modes: `extract` (v1/v2, one call), `extract3` (outline, then details), `extract4` (structured outline with `per_paragraph`, then details), `generate` (one call per worksheet), `generate2` (one call per template section).
  - It reads `text/*.txt`, `passages.json` and `pairs.json` next to it and writes `results/`.
  - Run it on the manna host with `OPENAI_COMPAT_API_KEY` in the environment, read from the Coolify `.env` without printing it.
