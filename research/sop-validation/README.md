# Lesson-SOP / worksheet-generator LLM validation

Research code for `docs/LLM_VALIDATION.md`: can the available model (`gemma-4-31B-it`, NCHC GenAI,
OpenAI-compatible) support the lesson-prep SOP and worksheet generator described in the product
spec `學習單產生器_教師選項與SOP整合規格_20261114`?

Nothing here is wired into the web UI or the API. Prompts, schemas and checks are shaped so they
can later move into `api/src/ai/` (prompt versions in the DB, strict JSON-schema output, automatic
checks with one repair attempt).

## What must stay out of the repo

The curriculum PDFs, the CEEC vocabulary list and the ~270 teacher worksheets are copyrighted.
Every script reads them from a path you pass in and writes **only** to `$MANNA_DATA`
(default `~/manna-data`): extracted text, CSV/JSON, model outputs, call logs. The repo holds code,
prompts, schemas, the de-identified design rules, one original test passage and the report.

## Layout

| Path | What |
|---|---|
| `llm.py` | OpenAI-compatible client. Default transport pipes the request over ssh to the host that holds the key (the key never reaches this machine); `MANNA_LLM_TRANSPORT=http` + `OPENAI_COMPAT_API_KEY` for a direct call. Always sends `max_tokens`. Logs every call to `$MANNA_DATA/runs/*.jsonl`. |
| `probe_limits.py` | Context-window / output-length probe. |
| `labels.py` | The fixed label sets of spec §2, §3, §5, §8 (single source for schemas and checks). |
| `kb/dump_pdf_text.py` | Per-page text of the knowledge-base PDFs (to find scanned pages). |
| `kb/ingest_curriculum.py` | 108 curriculum PDFs → item-level records + CSV in the `/api/admin/knowledge/import` column format. |
| `kb/ingest_vocab.py` | CEEC 高中英文參考詞彙表 → headword/level/POS entries and an expanded word-form CSV. |
| `kb/build_import_csv.py` | The two extraction outputs → the CSV files the 知識庫管理 page uploads (`$MANNA_DATA/kb/import/`), with row counts and SHA-256 in `import_manifest.json`. Imports nothing itself. |
| `worksheets/extract_text.py` | DOCX/PDF worksheet examples → text + `files.csv` (ids `W001…`, SHA-256). |
| `worksheets/classify.py` | LLM-built structured inventory (one call per worksheet, resumable). |
| `worksheets/inventory_stats.py` | Aggregated, de-identified statistics + LLM-vs-regex agreement. |
| `worksheets/sample_for_review.py` | Seeded sample for manual checking of the inventory. |
| `worksheets/select_fewshot.py` | Few-shot shortlist + de-identified few-shot block (activity skeletons only). |
| `rules/design_rules.json` | Structured design rules with their evidence. `rules/design_rules_for_prompt.md` is the prompt form. |
| `prompts/*.md` | One file per step: system text, `=====USER=====`, user template with `{{placeholders}}` (same rendering as `api/src/prompts.rs`). |
| `chain/schemas.py`, `schemas/*.json` | Strict JSON schemas (generated: `python chain/schemas.py`). |
| `chain/checks.py` | Automatic checks per step (hard = reject, soft = quality signal). |
| `vocab/analyze.py` | Deterministic CEEC level analysis of worksheet text (no LLM). |
| `runner/run_chain.py` | Runs steps (a)–(g) on the validation passages; one repair attempt per step. |
| `runner/report.py`, `runner/show.py` | Result tables / per-step log. |
| `runner/call_stats.py` | Totals over the call logs (failures by kind, cache hits, longest output). |
| `runner/ablation_codes.py` | Step (b) without the curriculum list (hallucinated-code rate). |
| `passages/manifest.json` | Validation passages: three are pointers (worksheet id + line numbers + hash) into the local worksheet extraction, one is an original passage stored here. |

## Rerun

```bash
uv venv ~/manna-data/.venv --python 3.12
uv pip install --python ~/manna-data/.venv/bin/python pypdf pdfplumber python-docx jsonschema pymupdf
PY=~/manna-data/.venv/bin/python
cd research/sop-validation

# 1. knowledge base (no LLM)
$PY kb/dump_pdf_text.py   <dir with the curriculum + vocabulary PDFs>
$PY kb/ingest_curriculum.py <same dir>          # -> $MANNA_DATA/kb/curriculum_items.{json,csv}, knowledge_import*.csv
$PY kb/ingest_vocab.py "<dir>/高中英文參考詞彙表(111學年度起適用).pdf"   # -> ceec_vocab_entries.csv, ceec_vocab.csv
$PY kb/build_import_csv.py                      # -> import/curriculum_import.csv, import/vocabulary_import.csv (upload on the 知識庫管理 page)

# 2. worksheets
$PY worksheets/extract_text.py <dir with the worksheet examples>
$PY worksheets/classify.py --workers 3          # ~270 calls, resumable; --from-log rebuilds from the call log
$PY worksheets/inventory_stats.py
$PY worksheets/select_fewshot.py --k 8

# 3. chain evaluation (9 calls per passage per run, plus repairs)
$PY probe_limits.py
$PY runner/run_chain.py --run r1
$PY runner/run_chain.py --run r2
$PY runner/run_chain.py --run r1-rules --variant rules --reuse r1 --passages P2 P4
$PY runner/run_chain.py --run r2-rules --variant rules --reuse r2 --passages P2 P4
$PY runner/run_chain.py --run r1 --retry-failed-g   # regenerate only worksheet versions that returned no JSON
$PY runner/run_chain.py --run r1 --rescore          # no API calls: recompute checks from the call log after editing checks.py
$PY runner/report.py r1 r2
$PY runner/report.py r1-rules r2-rules --compare r1 r2
$PY runner/ablation_codes.py --base r1 --repeat 2

# 4. vocabulary analysis of any text
$PY vocab/analyze.py passages/original_repair_club.txt --target 3
$PY runner/worksheet_text.py r1 P2 一般版 > /tmp/sheet.txt && $PY vocab/analyze.py /tmp/sheet.txt --target 4
```

The knowledge-base CSVs are **not** imported anywhere by these scripts (they are uploaded on the
知識庫管理 page, see `docs/API.md`「Phase 2.0：知識庫與詞彙」). Importing
`knowledge_import.csv` through `POST /api/admin/knowledge/import` is a separate, deliberate step
(see the report for what has to be decided first).
