"""Extract the CEEC 高中英文參考詞彙表（111學年度起適用）into word -> level records.

The list is printed twice (依級別排序 pp.13-64 of the PDF, 依字母排序 pp.65-115), three columns per
page. Plain text extraction interleaves the columns, so words are assigned to columns by x position
and each column is read as a token stream:

  alphabetical section:  HEADWORD... POS... LEVEL(1-6)        <- primary source of levels
  by-level section:      HEADWORD... POS...   (level = running "第N級" heading)   <- cross-check

Outputs (under $MANNA_DATA/kb/):
  ceec_vocab_entries.csv   one row per printed entry: headword (as printed), pos, level, page
  ceec_vocab.csv           one row per usable word form: word, level, pos, headword, kind, ...
                           (a/an -> a, an; agree(ment) -> agree v. + agreement n.; shoe(s) -> shoe)
  ceec_vocab_report.json   counts and cross-check between the two sections

usage: python ingest_vocab.py <pdf>
"""
from __future__ import annotations

import csv
import hashlib
import json
import os
import re
import sys
from collections import Counter
from pathlib import Path

import pdfplumber

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
VERSION = "大考中心 高中英文參考詞彙表（111學年度起適用），109年7月"
COLS = [(55, 226), (226, 392), (392, 570)]
POS_ATOM = r"(?:n|v|adj|adv|prep|conj|pron|art|aux|int|interj|det|num|abbr|pl)\."
# the last atom may lack its period in the source ("calm v./adj./n")
POS_TOKEN = re.compile(rf"^\(?{POS_ATOM}\)?(?:/\(?{POS_ATOM}\)?)*(?:/(?:n|v|adj|adv))?/?$")
LEVEL_HEAD = {"第一級": 1, "第二級": 2, "第三級": 3, "第四級": 4, "第五級": 5, "第六級": 6}
CJK = re.compile(r"[一-鿿]")


def column_tokens(page, y0=55, y1=800):
    """-> list (per column) of token lists; headers, footers, letter headings removed."""
    words = [w for w in page.extract_words() if y0 < w["top"] < y1]
    out = []
    for x0, x1 in COLS:
        col = sorted((w for w in words if x0 <= w["x0"] < x1), key=lambda w: (round(w["top"]), w["x0"]))
        lines: list[list[dict]] = []
        for w in col:
            if lines and abs(lines[-1][0]["top"] - w["top"]) <= 3:
                lines[-1].append(w)
            else:
                lines.append([w])
        toks = []
        for ln in lines:
            texts = [w["text"] for w in ln]
            if len(texts) == 1 and re.fullmatch(r"[A-Z]", texts[0]):
                continue  # alphabet heading (the entry "I (me, ...) pron. 1" has several tokens)
            if len(texts) == 1 and texts[0] in LEVEL_HEAD:
                toks.append(texts[0])   # level heading sits inline in the column flow
                continue
            if any(CJK.search(t) for t in texts):
                continue
            toks += texts
        out.append(toks)
    return out


def parse_stream(tokens, with_level: bool):
    """Yield (headword, pos, level|heading). An entry = non-POS tokens, POS tokens[, digit]."""
    head: list[str] = []
    pos: list[str] = []
    for t in tokens:
        if t in LEVEL_HEAD:
            if head:                      # flush the entry before the heading
                yield (" ".join(head), "".join(pos), None)
                head, pos = [], []
            yield ("__LEVEL__", "", LEVEL_HEAD[t])
            continue
        if with_level and re.fullmatch(r"[1-6]", t) and head:
            yield (" ".join(head), "".join(pos), int(t))
            head, pos = [], []
        elif POS_TOKEN.match(t):
            pos.append(t)
        else:
            if pos and not with_level:
                yield (" ".join(head), "".join(pos), None)
                head, pos = [], []
            elif pos and with_level:
                # a new headword started but no level digit was seen: keep and flag
                yield (" ".join(head), "".join(pos), None)
                head, pos = [], []
            head.append(t)
    if head:
        yield (" ".join(head), "".join(pos), None)


def expand(headword: str, pos: str):
    """Printed headword -> [(word, pos, kind)]. kind: base | variant | derived."""
    base_pos = "/".join(p for p in pos.split("/") if p and not p.startswith("("))
    par_pos = "/".join(p.strip("()") for p in pos.split("/") if p.startswith("("))
    out = []
    m0 = re.fullmatch(r"(\S+) \(([^)]*,[^)]*)\)", headword)   # I (me, my, mine, myself)
    if m0:
        return [(m0.group(1), pos, "base")] + [(w.strip(), pos, "variant") for w in m0.group(2).split(",")]
    for i, variant in enumerate(headword.split("/")):
        variant = variant.strip()
        if not variant:
            continue
        kind = "base" if i == 0 else "variant"
        m = re.fullmatch(r"(.*?)\(([a-z]+)\)(.*)", variant)
        if m and not variant.startswith("("):
            pre, mid, post = m.groups()
            if not post and len(mid) > 4 and mid[:3] == pre[:3]:   # argue(argument): respelt
                out.append((pre, base_pos or pos, kind))
                out.append((mid, par_pos or base_pos, "derived"))
            elif mid in ("s", "es") and not post:         # shoe(s): usually plural
                out.append((pre, base_pos or pos, kind))
            elif not post and len(mid) > 2:               # agree(ment)
                out.append((pre, base_pos or pos, kind))
                out.append((pre + mid, par_pos or base_pos, "derived"))
            else:                                         # colo(u)r, judg(e)ment
                out.append((pre + post, base_pos or pos, kind))
                out.append((pre + mid + post, base_pos or pos, "variant"))
        else:
            m2 = re.fullmatch(r"(\S+) \((\S+)\)", variant)   # argue (argument)
            if m2:
                out.append((m2.group(1), base_pos or pos, kind))
                out.append((m2.group(2), par_pos or base_pos, "derived"))
            else:
                out.append((re.sub(r"^\(the\)\s*", "", variant), base_pos or pos, kind))
    return [(w.strip(), p, k) for w, p, k in out if w.strip()]


def main(pdf_path: str) -> None:
    pdf = Path(pdf_path)
    sha = hashlib.sha256(pdf.read_bytes()).hexdigest()
    alpha, bylevel, problems = [], [], []
    with pdfplumber.open(pdf) as doc:
        starts = {"level": None, "alpha": None, "appendix": None}
        for i, page in enumerate(doc.pages, 1):
            head = (page.extract_text() or "")[:40]
            if i > 12 and "依級別排序" in head and starts["level"] is None:
                starts["level"] = i
            if i > 12 and "依字母排序" in head and starts["alpha"] is None:
                starts["alpha"] = i
            if i > 12 and head.strip().startswith("附錄"):
                starts["appendix"] = i
        level = None
        for i in range(starts["level"], starts["alpha"]):
            for toks in column_tokens(doc.pages[i - 1]):
                for hw, pos, lv in parse_stream(toks, with_level=False):
                    if hw == "__LEVEL__":
                        level = lv
                    elif pos:
                        bylevel.append({"headword": hw, "pos": pos, "level": level, "page": i})
                    else:
                        problems.append({"section": "level", "page": i, "text": hw})
        for i in range(starts["alpha"], starts["appendix"]):
            for toks in column_tokens(doc.pages[i - 1]):
                for hw, pos, lv in parse_stream(toks, with_level=True):
                    # wrap artefacts: "x/ y" and a 1-2 letter tail split off the headword
                    hw = re.sub(r"(?<=[a-z]) (?=[a-z]{1,2}$)", "", hw.replace("/ ", "/"))
                    if pos and lv:
                        alpha.append({"headword": hw, "pos": pos, "level": lv, "page": i})
                    else:
                        problems.append({"section": "alpha", "page": i, "text": f"{hw} {pos} {lv}"})
        appendix_text = "\n".join((doc.pages[i - 1].extract_text() or "") for i in range(starts["appendix"], len(doc.pages) + 1))

    # appendix: unlevelled supplementary words (numbers, days, months, countries ...)
    appendix = []
    cat = None
    buf = ""
    def flush():
        nonlocal buf
        for item in re.split(r",\s*", buf.replace("-\n", "-").replace("\n", " ")):
            item = item.strip()
            if item and item != "etc.":
                for v in item.split("/"):
                    v = re.sub(r"^\(the\)\s*", "", v.strip())
                    if v:
                        appendix.append({"word": v, "category": cat})
        buf = ""
    for line in appendix_text.split("\n"):
        s = line.strip()
        if not s or CJK.search(s) or re.fullmatch(r"\d+", s):
            continue
        if "," not in s and re.match(r"^[A-Z]", s) and len(s.split()) <= 6 and not buf.rstrip().endswith(","):
            flush()
            cat = s
        else:
            buf += s + "\n"
    flush()

    a = {(e["headword"], e["pos"]): e["level"] for e in alpha}
    b = {(e["headword"], e["pos"]): e["level"] for e in bylevel}
    disagree = [{"entry": k, "alpha": a[k], "bylevel": b[k]} for k in a if k in b and a[k] != b[k]]
    only_alpha = sorted(f"{k[0]} {k[1]}" for k in a if k not in b)
    only_level = sorted(f"{k[0]} {k[1]}" for k in b if k not in a)

    out = DATA / "kb"
    out.mkdir(parents=True, exist_ok=True)
    with open(out / "ceec_vocab_entries.csv", "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=["headword", "pos", "level", "page", "source_doc", "source_version", "source_sha256"])
        w.writeheader()
        for e in alpha:
            w.writerow({**e, "source_doc": pdf.name, "source_version": VERSION, "source_sha256": sha})
    forms, seen = [], {}
    for e in alpha:
        for word, pos, kind in expand(e["headword"], e["pos"]):
            key = word.lower()
            # homographs listed twice (e.g. different POS at different levels): keep all rows; the
            # analysis script uses the LOWEST level and reports the ambiguity
            forms.append({"word": word, "level": e["level"], "pos": pos, "headword": e["headword"],
                          "kind": kind, "page": e["page"]})
            seen.setdefault(key, set()).add(e["level"])
    for x in appendix:
        forms.append({"word": x["word"], "level": "", "pos": "", "headword": x["word"],
                      "kind": "appendix:" + x["category"], "page": ""})
    with open(out / "ceec_vocab.csv", "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=["word", "level", "pos", "headword", "kind", "page"])
        w.writeheader()
        w.writerows(forms)
    report = {
        "source": pdf.name, "sha256": sha, "version": VERSION, "sections_start_page": starts,
        "entries_alpha": len(alpha), "entries_bylevel": len(bylevel),
        "entries_by_level_alpha": dict(sorted(Counter(e["level"] for e in alpha).items())),
        "entries_by_level_bylevel": dict(sorted(Counter(e["level"] for e in bylevel).items())),
        "word_forms": len(forms) - len(appendix), "appendix_words": len(appendix),
        "appendix_categories": dict(Counter(x["category"] for x in appendix)),
        "multi_level_words": sorted(k for k, v in seen.items() if len(v) > 1),
        "level_disagreements_between_sections": disagree,
        "only_in_alpha": only_alpha, "only_in_bylevel": only_level,
        "unparsed": problems,
    }
    (out / "ceec_vocab_report.json").write_text(json.dumps(report, ensure_ascii=False, indent=1))
    brief = {k: (v if not isinstance(v, list) else (len(v), v[:25])) for k, v in report.items()}
    print(json.dumps(brief, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main(sys.argv[1])
