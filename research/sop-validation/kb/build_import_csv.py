"""Build the two CSV files the knowledge-base admin page uploads, from the extraction outputs.

Inputs (written by ingest_curriculum.py / ingest_vocab.py under $MANNA_DATA/kb/):
  curriculum_items.csv, ceec_vocab.csv, ceec_vocab_report.json

Outputs (under $MANNA_DATA/kb/import/; never committed, licensing unconfirmed):
  curriculum_import.csv   POST /api/admin/knowledge/import   one row per curriculum item
  vocabulary_import.csv   POST /api/admin/vocabulary/import  one row per matchable word form
  import_manifest.json    row counts and SHA-256 of both files

All extracted curriculum items are exported (every stage and subject): the knowledge base keeps
the complete set, and retrieval scopes itself to senior-high English items by default (see
api/src/retrieval.rs). Nothing is imported by this script.

usage: python build_import_csv.py
"""
from __future__ import annotations

import csv
import hashlib
import json
import os
from collections import Counter
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
KB = DATA / "kb"
OUT = KB / "import"

CURRICULUM_COLS = [
    "item_code", "category", "school_track", "title", "content", "source_doc", "source_version",
    "source_page", "official_code", "item_kind", "strand", "strand_name", "stage", "subject",
    "advanced", "recurring", "source_sha256",
]
VOCABULARY_COLS = [
    "word", "headword", "level", "pos", "kind", "appendix_category", "list_version", "source_doc",
    "source_page", "source_sha256",
]
TRACKS = {"general", "vocational", "comprehensive"}
KINDS = {"learning_performance": "學習表現", "learning_content": "學習內容"}


def truthy(s: str) -> bool:
    return s.strip().lower() == "true"


def curriculum_rows() -> list[dict]:
    out = []
    with open(KB / "curriculum_items.csv", encoding="utf-8") as f:
        for r in csv.DictReader(f):
            advanced, recurring = truthy(r["advanced"]), truthy(r["recurring"])
            marks = ("*" if advanced else "") + ("◎" if recurring else "")
            page = r["page"] if r["page_end"] in ("", r["page"]) else f"{r['page']}-{r['page_end']}"
            out.append({
                "item_code": r["item_code"],
                "category": "curriculum",
                "school_track": r["school_track"],
                # the title is what a teacher sees next to the text: marks, official code, kind, strand
                "title": f"{marks}{r['official_code']} {KINDS[r['kind']]}｜{r['strand_name']}",
                "content": r["text"],
                "source_doc": r["source_doc"],
                "source_version": r["source_version"],
                "source_page": page,
                "official_code": r["official_code"],
                "item_kind": r["kind"],
                "strand": r["strand"],
                "strand_name": r["strand_name"],
                "stage": r["stage"],
                "subject": r["subject"],
                "advanced": "true" if advanced else "false",
                "recurring": "true" if recurring else "false",
                "source_sha256": r["source_sha256"],
            })
    codes = Counter(r["item_code"] for r in out)
    assert not [c for c, n in codes.items() if n > 1], "duplicate item_code"
    for r in out:
        assert r["school_track"] in TRACKS, r
        assert r["item_code"] and r["content"] and r["official_code"], r
        assert len(r["source_sha256"]) == 64, r
    return out


def vocabulary_rows() -> list[dict]:
    report = json.loads((KB / "ceec_vocab_report.json").read_text(encoding="utf-8"))
    common = {
        "list_version": report["version"],
        "source_doc": report["source"],
        "source_sha256": report["sha256"],
    }
    appendix_page = str(report["sections_start_page"]["appendix"])
    out = []
    with open(KB / "ceec_vocab.csv", encoding="utf-8") as f:
        for r in csv.DictReader(f):
            if r["kind"].startswith("appendix:"):
                row = {"word": r["word"], "headword": r["headword"], "level": "", "pos": "",
                       "kind": "appendix", "appendix_category": r["kind"].split(":", 1)[1],
                       "source_page": appendix_page}
            else:
                assert r["kind"] in ("base", "variant", "derived") and r["level"] in "123456", r
                row = {"word": r["word"], "headword": r["headword"], "level": r["level"],
                       "pos": r["pos"], "kind": r["kind"], "appendix_category": "",
                       "source_page": r["page"]}
            out.append({**row, **common})
    keys = Counter((r["word"], r["headword"], r["pos"], r["kind"], r["appendix_category"]) for r in out)
    assert not [k for k, n in keys.items() if n > 1], "duplicate word form"
    return out


def write(name: str, cols: list[str], rows: list[dict]) -> dict:
    path = OUT / name
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=cols)
        w.writeheader()
        w.writerows(rows)
    return {"path": str(path), "rows": len(rows),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    cur, voc = curriculum_rows(), vocabulary_rows()
    manifest = {
        "curriculum_import.csv": {
            **write("curriculum_import.csv", CURRICULUM_COLS, cur),
            "by_track": dict(Counter(r["school_track"] for r in cur)),
            "by_kind": dict(Counter(r["item_kind"] for r in cur)),
            "by_stage": dict(Counter(r["stage"] for r in cur)),
            "by_subject": dict(Counter(r["subject"] for r in cur)),
        },
        "vocabulary_import.csv": {
            **write("vocabulary_import.csv", VOCABULARY_COLS, voc),
            "entries": len({(r["headword"], r["level"]) for r in voc if r["kind"] != "appendix"}),
            "by_level": dict(sorted(Counter(r["level"] or "appendix" for r in voc).items())),
        },
    }
    (OUT / "import_manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1), encoding="utf-8")
    print(json.dumps(manifest, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
