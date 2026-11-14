"""Extract text from the teacher worksheet examples (DOCX / PDF) into $MANNA_DATA/worksheets/.

Outputs:
  text/<id>.txt        extracted text (DOCX: paragraphs and table cells in document order,
                       table rows as " | "-joined lines; PDF: PyMuPDF page text)
  files.csv            id, file name, format, sha256, pages, chars, cjk ratio, images, tables,
                       text_quality (ok | sparse | scanned | error)

IDs are W001.. in sorted file-name order (the numeric file-name prefixes come from two overlapping
numbering series, so they are not unique). files.csv keeps the SHA-256, which is the stable key.

The source files are copyrighted teaching materials: nothing written here may be committed.

usage: python extract_text.py <worksheet_dir>
"""
from __future__ import annotations

import csv
import hashlib
import os
import re
import sys
from pathlib import Path

import docx
import fitz  # PyMuPDF
from docx.oxml.ns import qn

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
CJK = re.compile(r"[一-鿿]")


def docx_text(path: Path):
    d = docx.Document(str(path))
    lines, n_tables = [], 0

    def para_text(p):
        return "".join(t.text or "" for t in p.iter(qn("w:t")))

    def walk(parent):
        nonlocal n_tables
        for child in parent.iterchildren():
            if child.tag == qn("w:p"):
                t = para_text(child)
                if t.strip():
                    lines.append(t)
            elif child.tag == qn("w:tbl"):
                n_tables += 1
                for tr in child.iter(qn("w:tr")):
                    cells = []
                    for tc in tr.iterchildren(qn("w:tc")):
                        cells.append(" ".join(para_text(p) for p in tc.iter(qn("w:p")) if para_text(p).strip()))
                    if any(c.strip() for c in cells):
                        lines.append(" | ".join(cells))
            elif child.tag == qn("w:sdt"):
                for c in child.iterchildren(qn("w:sdtContent")):
                    walk(c)

    walk(d.element.body)
    # text boxes are common in worksheets and are not part of the body flow
    boxes = []
    for txbx in d.element.body.iter(qn("w:txbxContent")):
        for p in txbx.iter(qn("w:p")):
            t = para_text(p)
            if t.strip():
                boxes.append(t)
    if boxes:
        lines.append("[TEXTBOXES]")
        lines += list(dict.fromkeys(boxes))
    n_images = len(d.inline_shapes)
    return "\n".join(lines), None, n_images, n_tables


def pdf_text(path: Path):
    doc = fitz.open(str(path))
    parts, n_images = [], 0
    for i, page in enumerate(doc, 1):
        parts.append(f"[PAGE {i}]\n" + page.get_text("text"))
        n_images += len(page.get_images())
    return "\n".join(parts), len(doc), n_images, 0


def main(src: str) -> None:
    out = DATA / "worksheets"
    (out / "text").mkdir(parents=True, exist_ok=True)
    rows = []
    for path in sorted(Path(src).iterdir()):
        ext = path.suffix.lower()
        if ext not in (".docx", ".pdf"):
            continue
        wid = f"W{len(rows) + 1:03d}"
        sha = hashlib.sha256(path.read_bytes()).hexdigest()
        try:
            text, pages, n_img, n_tab = docx_text(path) if ext == ".docx" else pdf_text(path)
            err = ""
        except Exception as e:  # noqa: BLE001
            text, pages, n_img, n_tab, err = "", None, 0, 0, f"{type(e).__name__}: {e}"[:200]
        body = re.sub(r"\[PAGE \d+\]", "", text)
        chars = len(body.strip())
        if err:
            quality = "error"
        elif ext == ".pdf" and pages and chars / pages < 80:
            quality = "scanned"
        elif chars < 300:
            quality = "sparse"
        else:
            quality = "ok"
        (out / "text" / f"{wid}.txt").write_text(text)
        rows.append({
            "id": wid, "file": path.name, "format": ext[1:], "sha256": sha, "pages": pages or "",
            "chars": chars, "cjk_ratio": round(len(CJK.findall(body)) / max(chars, 1), 3),
            "images": n_img, "tables": n_tab, "text_quality": quality, "error": err,
        })
    with open(out / "files.csv", "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)
    from collections import Counter
    print("files", len(rows), dict(Counter(r["format"] for r in rows)))
    print("quality", dict(Counter((r["format"], r["text_quality"]) for r in rows)))
    print("duplicate sha256:", len(rows) - len({r["sha256"] for r in rows}))
    print("duplicate ids:", len(rows) - len({r["id"] for r in rows}))


if __name__ == "__main__":
    main(sys.argv[1])
