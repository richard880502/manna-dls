"""Deterministic CEEC vocabulary-level analysis of worksheet text (spec §9.6 subset, no LLM).

Reports
  * level distribution (tokens and distinct words) with explicit denominators,
  * words above a target level with their positions,
  * and, kept OUT of the distribution: proper nouns, unknown words, multiword / hyphenated items,
    appendix words (numbers, days, countries ...), and words listed at more than one level.

Lemmatisation is rule-based (no model): exact match -> irregular table -> inflection rules
(-s, -es, -ies, -ed, -ing, doubled consonant, -er/-est, possessive). A second, clearly labelled pass
handles the derivations that the list's own editorial principles say are deliberately NOT listed
(regular -ing/-ed adjectives are covered by inflection; re-, non-, un-, -less, -ly, -ness): these are
reported as match="derived" with the base word's level, never silently merged.

Limits (see docs/LLM_VALIDATION.md §詞彙分析): no POS disambiguation, no word-sense disambiguation
(e.g. "capital" is listed at two levels -> lowest level is used and the word is flagged), phrasal
verbs / idioms are not detected unless given via --phrases, sentence-initial unknown capitalised
words cannot be told apart from proper nouns.

usage:
  python analyze.py worksheet.txt --target 4 [--vocab $MANNA_DATA/kb/ceec_vocab.csv] [--json out.json]
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from irregular import CONTRACTION_PARTS, IRREGULAR  # noqa: E402

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
TOKEN = re.compile(r"[A-Za-z]+(?:[’'][A-Za-z]+)*(?:-[A-Za-z]+(?:[’'][A-Za-z]+)*)*")
VOWELS = "aeiou"


class Vocab:
    def __init__(self, path: Path):
        self.levels: dict[str, set[int]] = defaultdict(set)   # lowercased word -> levels
        self.pos: dict[str, set[str]] = defaultdict(set)
        self.appendix: dict[str, str] = {}
        self.case_sensitive: set[str] = set()                  # e.g. "I", "TV"
        with open(path, encoding="utf-8") as f:
            for r in csv.DictReader(f):
                w = r["word"].replace("’", "'")
                if r["kind"].startswith("appendix"):
                    self.appendix[w.lower()] = r["kind"].split(":", 1)[1]
                    continue
                self.levels[w.lower()].add(int(r["level"]))
                self.pos[w.lower()].add(r["pos"])
        self.version = "大考中心 高中英文參考詞彙表（111學年度起適用）"

    def level(self, w: str):
        lv = self.levels.get(w)
        return (min(lv), len(lv) > 1) if lv else None


def inflection_candidates(w: str, comparatives: bool = True) -> list[str]:
    """Possible base forms of an inflected word, most specific first."""
    c: list[str] = []
    if w in IRREGULAR:
        c += IRREGULAR[w]
    if w.endswith("ies") and len(w) > 4:
        c.append(w[:-3] + "y")
    if w.endswith("ves") and len(w) > 4:
        c += [w[:-3] + "f", w[:-3] + "fe"]
    if w.endswith("es") and len(w) > 3:
        c += [w[:-2], w[:-1]]
    if w.endswith("s") and not w.endswith("ss") and len(w) > 2:
        c.append(w[:-1])
    if w.endswith("ied") and len(w) > 4:
        c.append(w[:-3] + "y")
    if w.endswith("ed") and len(w) > 3:
        stem = w[:-2]
        c += [stem, stem + "e"]
        if len(stem) > 2 and stem[-1] == stem[-2] and stem[-1] not in VOWELS:
            c.append(stem[:-1])
    if w.endswith("ing") and len(w) > 4:
        stem = w[:-3]
        c += [stem, stem + "e"]
        if len(stem) > 2 and stem[-1] == stem[-2] and stem[-1] not in VOWELS:
            c.append(stem[:-1])
        if stem.endswith("y"):
            c.append(stem[:-1] + "ie")       # dying -> die
    for suf in ("er", "est") if comparatives else ():
        if w.endswith(suf) and len(w) > len(suf) + 2:
            stem = w[: -len(suf)]
            c += [stem, stem + "e"]
            if stem.endswith("i"):
                c.append(stem[:-1] + "y")
            if len(stem) > 2 and stem[-1] == stem[-2] and stem[-1] not in VOWELS:
                c.append(stem[:-1])
    return list(dict.fromkeys(c))


def derivation_candidates(w: str) -> list[tuple[str, str]]:
    """(base, rule) for regular derivations the list does not enumerate."""
    c: list[tuple[str, str]] = []
    for pre in ("re", "non", "un"):
        if w.startswith(pre) and len(w) >= len(pre) + 3:
            c.append((w[len(pre):].lstrip("-"), f"{pre}-"))
    if w.endswith("less") and len(w) > 6:
        c.append((w[:-4], "-less"))
    if w.endswith("ness") and len(w) > 6:
        c += [(w[:-4], "-ness"), (w[:-5] + "y", "-ness")] if w[-5] == "i" else [(w[:-4], "-ness")]
    if w.endswith("ly") and len(w) > 4:
        c.append((w[:-2], "-ly"))
        if w.endswith("ily"):
            c.append((w[:-3] + "y", "-ly"))
        if w.endswith("ally"):
            c.append((w[:-4], "-ly"))
        if w.endswith("bly") or w.endswith("ply") or w.endswith("tly"):
            c.append((w[:-1] + "e", "-ly"))
    return c


def lookup(v: Vocab, w: str):
    """-> (base, level, multi_level, match) or None. w is lowercased."""
    exact = v.level(w)
    infl = None
    hits = [(b, v.level(b)) for b in inflection_candidates(w) if v.level(b)]
    if hits:
        # "used" -> us / use: prefer the candidate whose listed POS fits the suffix (verb for -ed/-ing)
        want = "v." if w.endswith(("ed", "ing")) else None
        fit = [h for h in hits if want and any(want in p for p in v.pos.get(h[0], ()))]
        b, hit = (fit or hits)[0]
        infl = (b, hit[0], hit[1], "inflection")
    verb_bases = inflection_candidates(w, comparatives=False)
    if exact and infl and infl[1] < exact[0] and infl[0] in verb_bases:
        # e.g. "broke" (adj., L4) vs. past tense of "break" (L1); "learned" (adj., L4) vs. "learn".
        # Without POS tagging the two readings cannot be told apart: use the lower level and say so.
        return infl[0], infl[1], infl[2], f"inflection|ambiguous(also listed as '{w}' L{exact[0]})"
    if exact:
        return w, exact[0], exact[1], "exact"
    if infl:
        return infl
    for b, rule in derivation_candidates(w):
        for cand in [b] + inflection_candidates(b):
            hit = v.level(cand)
            if hit:
                return cand, hit[0], hit[1], f"derived({rule})"
    return None


def split_compound(v: Vocab, w: str):
    """Closed compounds are mostly not listed (editorial principle 18): classroom -> class + room."""
    for i in range(3, len(w) - 2):
        a, b = w[:i], w[i:]
        ha, hb = v.level(a), lookup_simple(v, b)
        if ha and hb:
            return [(a, ha[0]), (b, hb)]
    return None


def lookup_simple(v: Vocab, w: str):
    hit = v.level(w)
    if hit:
        return hit[0]
    for b in inflection_candidates(w):
        hit = v.level(b)
        if hit:
            return hit[0]
    return None


def sentence_starts(text: str) -> set[int]:
    starts = {m.end() for m in re.finditer(r"(?:^|[.!?:;]\s+|\n\s*|[\"“(]\s*|\d[.)]\s+|[-•*]\s+)", text)}
    return starts


def analyze(text: str, vocab: Vocab, target: int, phrases: list[str] | None = None) -> dict:
    line_starts = [0] + [m.end() for m in re.finditer(r"\n", text)]

    def pos_of(off: int):
        import bisect
        ln = bisect.bisect_right(line_starts, off) - 1
        return ln + 1, off - line_starts[ln] + 1

    def ctx(a: int, b: int) -> str:
        return text[max(0, a - 30): b + 30].replace("\n", " ").strip()

    starts = sentence_starts(text)
    masked = text
    multiword = []
    for ph in phrases or []:
        for m in re.finditer(r"\b" + re.escape(ph) + r"\b", text, flags=re.I):
            line, col = pos_of(m.start())
            multiword.append({"item": m.group(0), "kind": "phrase(list)", "line": line, "col": col})
            masked = masked[: m.start()] + " " * (m.end() - m.start()) + masked[m.end():]

    tokens = []          # counted tokens
    proper, unknown, appendix, multi_level = [], [], [], {}
    for m in TOKEN.finditer(masked):
        raw = m.group(0).replace("’", "'")
        line, col = pos_of(m.start())
        where = {"line": line, "col": col}
        low = raw.lower()
        if low in CONTRACTION_PARTS or len(raw) == 1 and low not in ("a", "i"):
            continue
        # contractions: don't -> do ; teacher's -> teacher ; I'm -> I
        base_raw = re.sub(r"(n't|'s|'re|'ve|'ll|'d|'m|')$", "", low) if "'" in low else low
        if base_raw in ("ca", "wo", "sha"):
            base_raw = {"ca": "can", "wo": "will", "sha": "shall"}[base_raw]
        if "-" in base_raw and raw[0].isupper() and not any(lookup(vocab, p) for p in base_raw.split("-") if p):
            proper.append({"word": raw, **where})
            continue
        if "-" in base_raw:
            parts = [p for p in base_raw.split("-") if p]
            hit = lookup(vocab, base_raw.replace("-", "")) or lookup(vocab, base_raw)
            comp = [(p, (lookup(vocab, p) or (None, None))[1]) for p in parts]
            multiword.append({"item": raw, "kind": "hyphenated", **where,
                              "component_levels": comp,
                              "listed_as_single_word_level": hit[1] if hit and hit[3] == "exact" else None})
            continue
        sentence_initial = m.start() in starts
        is_cap = raw[0].isupper()
        if low in vocab.appendix and (is_cap or vocab.appendix[low] in ("Cardinal Numbers", "Ordinal Numbers", "Seasons")):
            hit0 = lookup(vocab, base_raw)
            if not hit0 or is_cap and not sentence_initial:
                appendix.append({"word": raw, "category": vocab.appendix[low], **where})
                continue
        hit = lookup(vocab, base_raw)
        if is_cap and not sentence_initial and (hit is None or raw.isupper() and len(raw) > 1 and hit[3] != "exact"):
            proper.append({"word": raw, **where})
            continue
        if is_cap and not sentence_initial and hit and hit[3] != "exact":
            # capitalised mid-sentence and only reachable through rules -> treat as a name
            proper.append({"word": raw, **where})
            continue
        if hit is None and not is_cap:
            comp = split_compound(vocab, base_raw)
            if comp:
                multiword.append({"item": raw, "kind": "compound", **where, "component_levels": comp,
                                  "listed_as_single_word_level": None})
                continue
        if hit is None:
            unknown.append({"word": raw, **where, "note": "句首大寫，可能是專有名詞" if is_cap else "未收錄或無法還原詞形",
                            "context": ctx(m.start(), m.end())})
            continue
        base, level, multi, match = hit
        if multi:
            multi_level[base] = sorted(vocab.levels[base])
        tokens.append({"word": raw, "base": base, "level": level, "match": match, **where,
                       "start": m.start(), "end": m.end()})

    proper_set = {x["word"] for x in proper}
    for x in [u for u in unknown if u["word"] in proper_set]:
        unknown.remove(x)
        proper.append({k: x[k] for k in ("word", "line", "col")})
    n = len(tokens)
    types = {}
    for t in tokens:
        types.setdefault(t["base"], t["level"])
    tok_by = Counter(t["level"] for t in tokens)
    typ_by = Counter(types.values())
    dist = [{"level": lv, "tokens": tok_by.get(lv, 0), "token_ratio": round(tok_by.get(lv, 0) / n, 4) if n else 0,
             "types": typ_by.get(lv, 0), "type_ratio": round(typ_by.get(lv, 0) / len(types), 4) if types else 0}
            for lv in range(1, 7)]
    above = defaultdict(list)
    for t in tokens:
        if t["level"] > target:
            above[(t["base"], t["level"])].append(
                {"form": t["word"], "line": t["line"], "col": t["col"], "match": t["match"],
                 "context": ctx(t["start"], t["end"])})
    above_list = [{"base": b, "level": lv, "count": len(v), "positions": v}
                  for (b, lv), v in sorted(above.items(), key=lambda kv: (-kv[0][1], kv[0][0]))]

    def uniq(items, key="word"):
        g = defaultdict(list)
        for x in items:
            g[x[key]].append({k: v for k, v in x.items() if k != key})
        return [{key: k, "count": len(v), "occurrences": v[:20]} for k, v in sorted(g.items(), key=lambda kv: kv[0].lower())]

    return {
        "wordlist_version": vocab.version,
        "target_level": target,
        "basis": {
            "counted_tokens": n,
            "counted_types": len(types),
            "denominator_note": "比例的分母＝可對應到詞彙表的詞（含可還原的詞形變化與規則衍生詞）；"
                                "專有名詞、未收錄詞、連字號複合詞、片語、附錄詞彙不計入分母，另列於下。",
            "excluded_tokens": {"proper_nouns": len(proper), "unknown": len(unknown),
                                "multiword": len(multiword), "appendix": len(appendix)},
            "match_types": dict(Counter(re.sub(r"[(|].*", "", t["match"]) for t in tokens)),
        },
        "level_distribution": dist,
        "above_target": above_list,
        "ambiguous_forms": sorted({(t["word"].lower(), t["base"], t["level"], t["match"]) for t in tokens
                                   if "ambiguous" in t["match"]}),
        "derived_matches": sorted({(t["word"].lower(), t["base"], t["level"], t["match"]) for t in tokens
                                   if t["match"].startswith("derived")}),
        "multi_level_words": [{"base": b, "levels": lv, "used_level": min(lv)} for b, lv in sorted(multi_level.items())],
        "proper_nouns": uniq(proper),
        "unknown": uniq(unknown),
        "multiword": uniq(multiword, "item"),
        "appendix_words": uniq(appendix),
        "disclaimer": "本分析為備課輔助，依公開詞彙表以規則比對，不是大考中心的官方評定、命題預測或品質保證。",
    }


def word_levels(words: list[str], vocab: Vocab) -> dict[str, dict]:
    """Level lookup for a list of words/phrases (used by the warm-up step: levels come from data)."""
    out = {}
    for w in words:
        key = w.strip()
        if " " in key or "-" in key:
            out[key] = {"level": None, "status": "multiword"}
            continue
        hit = lookup(vocab, key.lower().replace("’", "'"))
        if hit is None:
            out[key] = {"level": None, "status": "proper_or_unlisted" if key[:1].isupper() else "unlisted"}
        else:
            out[key] = {"level": hit[1], "status": hit[3] if not hit[2] else hit[3] + "+multi_level", "base": hit[0]}
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("text_file")
    ap.add_argument("--target", type=int, default=4)
    ap.add_argument("--vocab", default=str(DATA / "kb" / "ceec_vocab.csv"))
    ap.add_argument("--phrases", help="optional file with one multiword expression per line")
    ap.add_argument("--json")
    a = ap.parse_args()
    vocab = Vocab(Path(a.vocab))
    phrases = [l.strip() for l in open(a.phrases)] if a.phrases else None
    res = analyze(Path(a.text_file).read_text(), vocab, a.target, phrases)
    if a.json:
        Path(a.json).write_text(json.dumps(res, ensure_ascii=False, indent=1))
    b = res["basis"]
    print(f"詞彙表：{res['wordlist_version']}　目標級別：{a.target}")
    print(f"計入 {b['counted_tokens']} 詞次／{b['counted_types']} 個不同詞；未計入 {b['excluded_tokens']}")
    for d in res["level_distribution"]:
        print(f"  第{d['level']}級  詞次 {d['tokens']:4d} ({d['token_ratio']:.1%})   不同詞 {d['types']:4d} ({d['type_ratio']:.1%})")
    print(f"超出第{a.target}級：", ", ".join(f"{x['base']}(L{x['level']})@" + "/".join(f"{p['line']}:{p['col']}" for p in x["positions"][:3]) for x in res["above_target"][:40]))
    print("專有名詞：", ", ".join(x["word"] for x in res["proper_nouns"][:40]))
    print("未收錄／無法判定：", ", ".join(x["word"] for x in res["unknown"][:40]))
    print("複合詞／片語：", ", ".join(x["item"] for x in res["multiword"][:30]))
    print("規則衍生（推定）：", ", ".join(f"{w}→{b}" for w, b, _, _ in res["derived_matches"][:30]))
    print("詞形歧義（取較低級）：", ", ".join(f"{w}→{b}(L{lv})" for w, b, lv, _ in res["ambiguous_forms"]))
    print("多級別詞：", ", ".join(f"{x['base']}{x['levels']}" for x in res["multi_level_words"]))


if __name__ == "__main__":
    main()
