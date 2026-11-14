"""Extract item-level records (one learning-performance / learning-content code per record) from the
108 curriculum PDFs.

Approach: pdfplumber table extraction (the 學習重點 tables are ruled, so cells come out clean, while
plain text extraction interleaves the vertical category labels with the item text). Each cell is
split on item codes; text before the first code in a cell is a continuation of the previous item
(a cell broken across pages). The FIRST occurrence of a code is its definition; later occurrences
(核心素養對應表、議題融入附錄) are only used as a cross-check (see `xcheck_mismatch`).

Outputs (under $MANNA_DATA/kb/):
  curriculum_items.json / .csv   all records, full metadata (incl. source SHA-256)
  knowledge_import.csv           columns of POST /api/admin/knowledge/import, importable tracks only
  knowledge_import_comprehensive.csv   same columns; school_track=comprehensive is NOT yet accepted
                                       by the API (general|vocational|both) -> kept separate
  curriculum_report.json         counts + validation

usage: python ingest_curriculum.py <pdf_dir>
"""
from __future__ import annotations

import csv
import hashlib
import json
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import pdfplumber

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))

ROMAN = {"Ⅰ": "I", "Ⅱ": "II", "Ⅲ": "III", "Ⅳ": "IV", "Ⅴ": "V", "Ⅵ": "VI", "Ⅶ": "VII"}

DOCS = [
    # (filename substring, track, doc key, item-code prefix, source_version)
    ("國民中小學暨普通型高中", "general", "lang", "108-G", "108課綱 語文領域-英語文（國中小暨普通型高中）發布版"),
    ("技術型高中_語文領域", "vocational", "lang", "108-V", "108課綱 語文領域-英語文（技術型高中）"),
    ("綜合型高中_語文領域", "comprehensive", "lang", "108-C", "108課綱 語文領域-英語文（綜合型高中）"),
    ("技術型高中_外語群", "vocational", "group", "", "108課綱 技術型高中外語群 110年8月版"),
]

PERF_NAMES = {
    "1": "語言能力（聽）", "2": "語言能力（說）", "3": "語言能力（讀）", "4": "語言能力（寫）",
    "5": "語言能力（聽說讀寫綜合應用能力）", "6": "學習興趣與態度", "7": "學習方法與策略",
    "8": "文化理解", "9": "邏輯思考、判斷與創造力",
}
CONTENT_NAMES = {
    "Aa": "語言知識／字母", "Ab": "語言知識／語音", "Ac": "語言知識／字詞", "Ad": "語言知識／句構",
    "Ae": "語言知識／篇章", "B": "溝通功能", "C": "文化與習俗", "D": "思考能力",
}

# 語文領域: optional marks, strand (1-9 | Aa-Ae | B-D), stage (roman, unicode or ascii), number
LANG_CODE = re.compile(r"([*＊◎]{0,3})\s*((?:[1-9])|(?:A[a-e])|(?:[B-D]))\s?-\s?(Ⅱ|Ⅲ|Ⅳ|Ⅴ|V|IV|III|II)\s?-\s?(\d{1,2})(?!\d)")
# 外語群: 外語-專-商概-1 / 外語-技-英語 I-A-a
GROUP_CODE = re.compile(
    r"外語-(專|實|技)-\s*([一-鿿]{2})\s*((?:[IVX]+)|(?:[ⅠⅡⅢⅣⅤⅥⅦ]))?\s*-\s*(\d{1,2}|[A-Z]-[a-z])(?![A-Za-z0-9：:])"
)
ENGLISH_GROUP_SUBJECTS = {"英語", "職場"}

CJK = r"　-〿一-鿿＀-￯"


def clean(text: str) -> str:
    """Join wrapped lines; drop spaces that pdf justification inserted between CJK characters."""
    t = text.replace("\n", "")
    t = re.sub(rf"(?<=[{CJK}])\s+(?=[{CJK}])", "", t)
    t = re.sub(r"\s+", " ", t).strip()
    return t


def ascii_roman(s: str) -> str:
    for k, v in ROMAN.items():
        s = s.replace(k, v)
    return s


def iter_cells(pdf_path: Path):
    """Yields (page_no, column_key, cell_text) for every non-empty table cell, in reading order.
    column_key is the cell's left edge (rounded), so a column can be followed across pages."""
    with pdfplumber.open(pdf_path) as doc:
        for pn, page in enumerate(doc.pages, 1):
            for table in page.find_tables():
                texts = table.extract()
                for row, row_text in zip(table.rows, texts):
                    for bbox, cell in zip(row.cells, row_text):
                        if bbox and cell and cell.strip():
                            yield pn, round(bbox[0] / 12), cell


def iter_group_lines(pdf_path: Path):
    """外語群: code and description sit in separate (sometimes merged) cells, so tables are not
    usable. Rebuild visual lines from word boxes instead and yield (page, [(x0, text), ...])."""
    with pdfplumber.open(pdf_path) as doc:
        for pn, page in enumerate(doc.pages, 1):
            words = sorted(page.extract_words(), key=lambda w: (round(w["top"]), w["x0"]))
            lines: list[list[dict]] = []
            for w in words:
                if lines and abs(lines[-1][0]["top"] - w["top"]) <= 3:
                    lines[-1].append(w)
                else:
                    lines.append([w])
            for ln in lines:
                ln.sort(key=lambda w: w["x0"])
                yield pn, ln


def split_cell(cell: str, regex: re.Pattern):
    """-> (leading_text_without_code, [(match, text_after_code)])"""
    ms = list(regex.finditer(cell))
    if not ms:
        return cell, []
    out = []
    for i, m in enumerate(ms):
        end = ms[i + 1].start() if i + 1 < len(ms) else len(cell)
        out.append((m, cell[m.end():end]))
    return cell[: ms[0].start()], out


HEADER = re.compile(r"學習階段|學習表現|學習內容|學習重點|主題|項目|類別|核心素養|^[A-D]\.|^[a-e]\.")


def base_record(track, pdf_path, version, sha, pn, text):
    return {"text": text, "school_track": track, "source_doc": pdf_path.name,
            "source_version": version, "page": pn, "page_end": pn, "source_sha256": sha}


def parse_lang(pdf_path: Path, track: str, prefix: str, version: str, sha: str):
    items: dict[str, dict] = {}
    later: dict[str, list[str]] = defaultdict(list)
    open_by_col: dict[int, str] = {}      # column -> last item (possibly continued on next page)
    seen_col_on_page: set[tuple[int, int]] = set()
    for pn, col, cell in iter_cells(pdf_path):
        lead, parts = split_cell(cell, LANG_CODE)
        first_in_col = (pn, col) not in seen_col_on_page
        if HEADER.search(cell.strip().split("\n")[0]) and not parts:
            continue  # header / label cell: does not consume the "first cell of the column" slot
        seen_col_on_page.add((pn, col))
        code0 = open_by_col.get(col)
        if lead.strip() and first_in_col and code0 and items[code0]["page_end"] == pn - 1 \
                and not items[code0]["text"].endswith("。") and len(clean(lead)) < 80:
            items[code0]["text"] = clean(items[code0]["text"] + lead)
            items[code0]["page_end"] = pn
            items[code0]["joined_across_pages"] = True
        for m, tail in parts:
            text = clean(tail)
            marks, strand, stage, num = m.group(1), m.group(2), m.group(3), m.group(4)
            stage_ascii = ascii_roman(stage)
            official = f"{strand}-{stage}-{num}"
            code = f"{prefix}-{strand}-{stage_ascii}-{num}"
            is_perf = strand[0].isdigit()
            if not text:
                continue
            if code in items:
                later[code].append(text)
                continue
            rec = {
                "item_code": code, "official_code": official,
                "kind": "learning_performance" if is_perf else "learning_content",
                "strand": strand, "strand_name": (PERF_NAMES if is_perf else CONTENT_NAMES)[strand],
                "stage": stage_ascii, "advanced": "*" in marks or "＊" in marks,
                "recurring": "◎" in marks, "subject": "英語文", "joined_across_pages": False,
            }
            rec.update(base_record(track, pdf_path, version, sha, pn, text))
            items[code] = rec
            open_by_col[col] = code
    norm = lambda s: re.sub(r"[\s，。、,.()（）]", "", s)
    for code, rec in items.items():
        d = norm(rec["text"])
        rec["relisted"] = len(later.get(code, []))
        # appendix tables re-list items; the next row's text can be glued on there, so only flag
        # when NO re-listing agrees with the definition
        agree = [t for t in later.get(code, []) if norm(t) == d]
        rec["xcheck_agree"] = len(agree)
        rec["xcheck_mismatch"] = bool(later.get(code)) and not agree
    return list(items.values())


def parse_group(pdf_path: Path, track: str, version: str, sha: str):
    items: dict[str, dict] = {}
    cur = None        # code of the item still collecting continuation lines
    cur_x = cur_top = cur_page = None
    for pn, ln in iter_group_lines(pdf_path):
        idx = next((i for i, w in enumerate(ln) if w["text"].startswith("外語-")), None)
        top = ln[0]["top"]
        if idx is None:
            if cur and pn == cur_page and top - cur_top < 22:
                cont = "".join(w["text"] for w in ln if w["x0"] > cur_x + 45)
                if cont:
                    items[cur]["text"] = clean(items[cur]["text"] + cont)
                    cur_top = top
            else:
                cur = None
            continue
        rest = " ".join(w["text"] for w in ln[idx:])
        m = GROUP_CODE.match(rest)
        if not m:
            cur = None
            continue
        text = clean(rest[m.end():])
        if not text or text.startswith(("：", ":")):
            cur = None
            continue
        cat, subj, lvl, tailcode = m.group(1), m.group(2), ascii_roman(m.group(3) or ""), m.group(4)
        code = f"外語-{cat}-{subj}{lvl}-{tailcode}"
        if code in items:
            cur = None
            continue
        rec = {
            "item_code": code, "official_code": code,
            "kind": "learning_performance" if tailcode.isdigit() else "learning_content",
            "strand": f"{subj}{lvl}",
            "strand_name": {"專": "專業科目", "實": "實習科目", "技": "技能領域"}[cat] + f"／{subj}{lvl}",
            "stage": "V", "advanced": False, "recurring": False, "subject": subj,
            "joined_across_pages": False, "relisted": 0, "xcheck_agree": 0, "xcheck_mismatch": False,
        }
        rec.update(base_record(track, pdf_path, version, sha, pn, text))
        items[code] = rec
        cur, cur_x, cur_top, cur_page = code, ln[idx]["x0"], top, pn
    return list(items.values())


def parse_doc(pdf_path, track, kind, prefix, version, sha):
    if kind == "lang":
        return parse_lang(pdf_path, track, prefix, version, sha)
    return parse_group(pdf_path, track, version, sha)


def regex_census(pdf_path: Path, kind: str) -> set[str]:
    """All codes that appear anywhere in the plain text (independent of table extraction)."""
    regex = LANG_CODE if kind == "lang" else GROUP_CODE
    found = set()
    with pdfplumber.open(pdf_path) as doc:
        for page in doc.pages:
            for m in regex.finditer(page.extract_text() or ""):
                if kind == "lang":
                    found.add(f"{m.group(2)}-{ascii_roman(m.group(3))}-{m.group(4)}")
                else:
                    found.add(f"外語-{m.group(1)}-{m.group(2)}{ascii_roman(m.group(3) or '')}-{m.group(4)}")
    return found


def numbering_gaps(items: list[dict]) -> list[str]:
    """Performance/content numbers should be consecutive 1..n within strand+stage."""
    groups = defaultdict(list)
    for r in items:
        tail = r["official_code"].rsplit("-", 1)[1]
        if tail.isdigit():
            groups[(r["school_track"], r["source_doc"], r["strand"], r["stage"])].append(int(tail))
    gaps = []
    for k, nums in groups.items():
        missing = sorted(set(range(1, max(nums) + 1)) - set(nums))
        if missing:
            gaps.append(f"{k[2]}-{k[3]} ({k[0]}): missing {missing}")
    return gaps


IMPORT_COLS = ["item_code", "category", "school_track", "title", "content", "source_doc",
               "source_version", "source_page"]


def import_row(r: dict) -> dict:
    marks = ("*" if r["advanced"] else "") + ("◎" if r["recurring"] else "")
    kind = "學習表現" if r["kind"] == "learning_performance" else "學習內容"
    return {
        "item_code": r["item_code"],
        "category": "curriculum",
        "school_track": r["school_track"],
        "title": f"{marks}{r['official_code']} {kind}｜{r['strand_name']}",
        "content": r["text"],
        "source_doc": r["source_doc"],
        "source_version": r["source_version"],
        "source_page": str(r["page"]),
    }


def main(src: str) -> None:
    out = DATA / "kb"
    out.mkdir(parents=True, exist_ok=True)
    all_items, report = [], {}
    for needle, track, kind, prefix, version in DOCS:
        pdf = next(p for p in Path(src).glob("*.pdf") if needle in p.name)
        sha = hashlib.sha256(pdf.read_bytes()).hexdigest()
        items = parse_doc(pdf, track, kind, prefix, version, sha)
        census = regex_census(pdf, kind)
        got = {ascii_roman(r["official_code"]) for r in items}
        report[pdf.name] = {
            "sha256": sha,
            "track": track,
            "items": len(items),
            "by_kind": dict(Counter(r["kind"] for r in items)),
            "by_stage": dict(Counter(r["stage"] for r in items)),
            "by_subject": dict(Counter(r["subject"] for r in items)),
            "codes_in_text_not_extracted": sorted(census - got),
            "codes_extracted_not_in_text": sorted(got - census),
            "numbering_gaps": numbering_gaps(items),
            "relisted_and_identical": sum(1 for r in items if r["xcheck_agree"]),
            "relisted_but_no_identical_copy": [r["official_code"] for r in items if r["xcheck_mismatch"]],
            "joined_across_pages": [r["official_code"] for r in items if r["joined_across_pages"]],
            "not_ending_with_period": [r["official_code"] for r in items
                                       if kind == "lang" and not r["text"].endswith("。")],
            "short_text(<6 chars)": [r["official_code"] for r in items if len(r["text"]) < 6],
        }
        all_items += items
    (out / "curriculum_items.json").write_text(json.dumps(all_items, ensure_ascii=False, indent=1))
    cols = ["item_code", "official_code", "kind", "strand", "strand_name", "stage", "advanced",
            "recurring", "subject", "text", "school_track", "source_doc", "source_version", "page",
            "page_end", "source_sha256"]
    with open(out / "curriculum_items.csv", "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(all_items)

    # Import-compatible: senior-high stage only; English-related subjects of the 外語群 only.
    def importable(r):
        return r["stage"] == "V" and (r["subject"] == "英語文" or r["subject"] in ENGLISH_GROUP_SUBJECTS)

    sel = [r for r in all_items if importable(r)]
    for name, rows in [
        ("knowledge_import.csv", [r for r in sel if r["school_track"] in ("general", "vocational")]),
        ("knowledge_import_comprehensive.csv", [r for r in sel if r["school_track"] == "comprehensive"]),
    ]:
        with open(out / name, "w", newline="", encoding="utf-8") as f:
            w = csv.DictWriter(f, fieldnames=IMPORT_COLS)
            w.writeheader()
            w.writerows(import_row(r) for r in rows)
        report[name] = {"rows": len(rows), "by_track": dict(Counter(r["school_track"] for r in rows))}
    codes = [r["item_code"] for r in all_items]
    report["_total"] = {"items": len(all_items), "duplicate_item_codes": [c for c, n in Counter(codes).items() if n > 1]}
    (out / "curriculum_report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1))
    print(json.dumps(report, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main(sys.argv[1])
