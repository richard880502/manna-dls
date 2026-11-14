"""Dump per-page text of every knowledge-base PDF to $MANNA_DATA/kb/text/<stem>.txt.

Pages are separated by a form feed + "=== page N ===" line so later parsers can recover page
numbers. Also writes kb/text/_summary.json (pages, chars per page, SHA-256) so scanned / empty
pages are visible.

usage: python dump_pdf_text.py <pdf_dir>
"""
import hashlib
import json
import os
import sys
from pathlib import Path

import pdfplumber

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def main(src: str) -> None:
    out = DATA / "kb" / "text"
    out.mkdir(parents=True, exist_ok=True)
    summary = {}
    for pdf in sorted(Path(src).glob("*.pdf")):
        sha = hashlib.sha256(pdf.read_bytes()).hexdigest()
        chars = []
        with pdfplumber.open(pdf) as doc, open(out / (pdf.stem + ".txt"), "w") as f:
            for i, page in enumerate(doc.pages, 1):
                text = page.extract_text() or ""
                chars.append(len(text))
                f.write(f"\f=== page {i} ===\n{text}\n")
        summary[pdf.name] = {
            "sha256": sha,
            "pages": len(chars),
            "empty_pages": [i + 1 for i, c in enumerate(chars) if c < 20],
            "total_chars": sum(chars),
        }
        print(pdf.name, summary[pdf.name]["pages"], "pages", sum(chars), "chars",
              "empty:", summary[pdf.name]["empty_pages"])
    (out / "_summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main(sys.argv[1])
