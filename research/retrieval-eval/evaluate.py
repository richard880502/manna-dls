"""Keyword-only vs vector vs hybrid retrieval over the 108 curriculum items.

    python3 evaluate.py [--csv ~/manna-data/kb/import/curriculum_import.csv] [--sweep] [--examples]

The tokenizer, keyword scorer, embedded text and fusion are line-by-line ports of
`api/src/retrieval.rs` and `api/src/kb_embeddings.rs`. The port was checked once against the Rust
code (`--dump`, see README.md): identical keyword and hybrid rankings for every query. Embeddings come from `embed.py` (cached).
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import os
from pathlib import Path

from embed import embed

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------- port of retrieval.rs

EN_STOPWORDS = set(
    """the and for are but not you all any can had her was one our out has have him his how its
    may new now see two did get let put say she too use with this that from they will would there
    their what about like into than then them these those some could other been were your more
    most such each very just over should does doing done here while after before because being
    both own same few via per etc""".split()
)
CJK_STOP_BIGRAMS = set(
    """學生 老師 教師 我們 這個 一個 可以 什麼 如何 進行 本課 課程 這節 節課 希望 能夠 以及 或是 還是
    使用 他們 自己 然後 因為 所以 的是 了解 一下 示範""".split()
)


def is_cjk(c: str) -> bool:
    o = ord(c)
    return (
        0x3400 <= o <= 0x4DBF
        or 0x4E00 <= o <= 0x9FFF
        or 0xF900 <= o <= 0xFAFF
        or 0x20000 <= o <= 0x2EBEF
        or 0x30000 <= o <= 0x3134F
    )


def stem_en(w: str) -> str:
    if len(w) > 4 and w.endswith("ies"):
        return w[:-3] + "y"
    if len(w) > 3 and w.endswith("s") and not w.endswith("ss"):
        return w[:-1]
    return w


def tokenize(text: str) -> list[str]:
    seen: dict[str, None] = {}
    word = ""
    run: list[str] = []

    def flush_word() -> None:
        nonlocal word
        if len(word) >= 3 and word not in EN_STOPWORDS:
            seen.setdefault(stem_en(word))
        word = ""

    def flush_cjk() -> None:
        for a, b in zip(run, run[1:]):
            if a + b not in CJK_STOP_BIGRAMS:
                seen.setdefault(a + b)
        run.clear()

    for c in text:
        if c.isascii() and c.isalnum():
            flush_cjk()
            word += c.lower()
        elif is_cjk(c):
            flush_word()
            run.append(c)
        else:
            flush_word()
            flush_cjk()
    flush_word()
    flush_cjk()
    return list(seen)


def keyword_rank(query: str, items: list[dict]) -> list[tuple[str, int]]:
    """(item_code, score) with score > 0, best first; ties by item_code (as `rank`)."""
    q = tokenize(query)
    out = []
    for it in items:
        title = it["_title_tokens"]
        every = it["_all_tokens"]
        matched = sum(1 for t in q if t in every)
        if matched:
            out.append((it["item_code"], matched + sum(1 for t in q if t in title)))
    out.sort(key=lambda p: (-p[1], p[0]))
    return out


# ---------------------------------------------------------------- port of kb_embeddings.rs (embed_text)

KIND_LABEL = {"learning_performance": "學習表現", "learning_content": "學習內容"}
QUERY_CHARS = 2000


def embed_text(it: dict, recipe: str) -> str:
    kind = KIND_LABEL.get(it["item_kind"], "")
    if recipe == "content":
        return it["content"]
    if recipe == "context":  # shipped: subject, kind and strand names, then the official text
        head = "｜".join(p for p in (it["subject"], kind, it["strand_name"]) if p)
        return f"{head}：{it['content']}" if head else f"{it['title']}：{it['content']}"
    if recipe == "code":
        head = "｜".join(p for p in (it["official_code"], it["subject"], kind, it["strand_name"]) if p)
        return f"{head}：{it['content']}"
    if recipe == "gemma":
        return f"title: {it['strand_name'] or 'none'} | text: {it['content']}"
    raise ValueError(recipe)


def query_text(q: str, recipe: str) -> str:
    q = q[:QUERY_CHARS]
    return f"task: search result | query: {q}" if recipe == "gemma" else q


def cosine(a: list[float], b: list[float]) -> float:
    dot = sum(x * y for x, y in zip(a, b))
    na = math.sqrt(sum(x * x for x in a))
    nb = math.sqrt(sum(x * x for x in b))
    return dot / (na * nb) if na and nb else 0.0


def fuse(
    keyword: list[tuple[str, int]],
    vector: list[tuple[str, float]],
    *,
    k: float = 60.0,
    w_kw: float = 1.0,
    w_vec: float = 1.0,
    scale: str = "relative",
) -> list[str]:
    """Weighted reciprocal rank fusion (port of `retrieval::fuse`; defaults = shipped values).

    fused = w_kw * (score / best score) / (k + keyword rank) + w_vec / (k + vector rank)

    Keyword ranks are competition ranks: items with the same keyword score share the rank of the
    first of them. `scale="none"` drops the (score / best score) factor (plain RRF), kept only
    for the comparison in --sweep. Ties in the fused score: higher similarity, then item_code.
    """
    score: dict[str, float] = {}
    best = max(keyword[0][1], 1) if keyword else 1
    rank = 0
    prev = None
    for i, (code, s) in enumerate(keyword):
        if s != prev:
            rank, prev = i + 1, s
        factor = s / best if scale == "relative" else 1.0
        score[code] = score.get(code, 0.0) + w_kw * factor / (k + rank)
    sims = dict(vector)
    for i, (code, _) in enumerate(vector):
        score[code] = score.get(code, 0.0) + w_vec / (k + i + 1)
    return sorted(score, key=lambda c: (-score[c], -sims.get(c, float("-inf")), c))


# ---------------------------------------------------------------- data

SENIOR_HIGH = "V"
ENGLISH_SUBJECTS = {"英語文", "英語", "職場"}
PREFIX = {"general": "108-G-", "vocational": "108-V-", "comprehensive": "108-C-"}


def load_items(path: Path) -> list[dict]:
    with path.open(encoding="utf-8-sig") as f:
        rows = list(csv.DictReader(f))
    for r in rows:
        r["_title_tokens"] = set(tokenize(r["title"]))
        r["_all_tokens"] = r["_title_tokens"] | set(tokenize(r["content"]))
    return rows


def in_default_scope(it: dict, track: str) -> bool:
    return (
        it["category"] == "curriculum"
        and it["school_track"] in (track, "both")
        and it["stage"] == SENIOR_HIGH
        and it["subject"] in ENGLISH_SUBJECTS
    )


def full_code(track: str, code: str) -> str:
    return code if code.startswith("外語-") else PREFIX[track] + code


# ---------------------------------------------------------------- metrics

KS = (8, 20, 150)  # RAG_TOP_K, curriculum search default limit, SOP objectives CURRICULUM_LIMIT


def metrics(ranked: list[str], relevant: set[str]) -> dict[str, float]:
    out = {f"recall@{k}": len(relevant & set(ranked[:k])) / len(relevant) for k in KS}
    first = next((i + 1 for i, c in enumerate(ranked) if c in relevant), None)
    out["mrr"] = 1.0 / first if first else 0.0
    return out


def mean(rows: list[dict[str, float]]) -> dict[str, float]:
    return {k: sum(r[k] for r in rows) / len(rows) for k in rows[0]} if rows else {}


def fmt(m: dict[str, float]) -> str:
    return "  ".join(f"{k} {v:.3f}" for k, v in m.items())


# ---------------------------------------------------------------- main

def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv", default="~/manna-data/kb/import/curriculum_import.csv")
    ap.add_argument("--recipe", default="context", choices=["content", "context", "code", "gemma"])
    ap.add_argument("--sweep", action="store_true", help="compare recipes and fusion settings")
    ap.add_argument("--examples", action="store_true", help="per-query results")
    ap.add_argument("--dump", help="write keyword / vector / hybrid rankings to this JSON file (port check)")
    ap.add_argument("--json", help="write the summary to this file")
    args = ap.parse_args()

    items = load_items(Path(os.path.expanduser(args.csv)))
    queries = json.loads((HERE / "queries.json").read_text(encoding="utf-8"))["queries"]
    by_code = {it["item_code"]: it for it in items}
    for q in queries:
        q["rel"] = {full_code(q["track"], c) for c in q["relevant"]}
        q["cands"] = [it for it in items if in_default_scope(it, q["track"])]
        missing = q["rel"] - {it["item_code"] for it in q["cands"]}
        assert not missing, f"{q['id']}: labels not in scope: {missing}"
        q["kw"] = keyword_rank(q["query"], q["cands"])

    def vector_lists(recipe: str) -> dict[str, list[tuple[str, float]]]:
        scope = [it for it in items if it["stage"] == SENIOR_HIGH and it["subject"] in ENGLISH_SUBJECTS]
        doc_vecs = dict(zip((it["item_code"] for it in scope), embed([embed_text(it, recipe) for it in scope])))
        q_vecs = embed([query_text(q["query"], recipe) for q in queries])
        out = {}
        for q, qv in zip(queries, q_vecs):
            sims = [(it["item_code"], cosine(qv, doc_vecs[it["item_code"]])) for it in q["cands"]]
            sims.sort(key=lambda p: (-p[1], p[0]))
            out[q["id"]] = sims
        return out

    def run(vec: dict[str, list[tuple[str, float]]], method: str, **params) -> dict[str, list[str]]:
        out = {}
        for q in queries:
            if method == "keyword":
                out[q["id"]] = [c for c, _ in q["kw"]]
            elif method == "vector":
                out[q["id"]] = [c for c, _ in vec[q["id"]]]
            else:
                out[q["id"]] = fuse(q["kw"], vec[q["id"]], **params)
        return out

    def summarize(ranked: dict[str, list[str]], subset=lambda q: True) -> dict[str, float]:
        return mean([metrics(ranked[q["id"]], q["rel"]) for q in queries if subset(q)])

    groups = {
        "all": lambda q: True,
        "general": lambda q: q["track"] == "general",
        "vocational": lambda q: q["track"] == "vocational",
        "zh": lambda q: q["lang"] == "zh",
        "en": lambda q: q["lang"] == "en",
        "mixed": lambda q: q["lang"] == "mixed",
    }

    if args.sweep:
        print(f"{len(queries)} queries\n== embedded text recipe (vector only / hybrid default)")
        for recipe in ("content", "context", "code", "gemma"):
            vec = vector_lists(recipe)
            print(f"{recipe:8} vector  {fmt(summarize(run(vec, 'vector')))}")
            print(f"{recipe:8} hybrid  {fmt(summarize(run(vec, 'hybrid')))}")
        vec = vector_lists(args.recipe)
        print(f"\n== fusion sweep (recipe {args.recipe})")
        print(f"keyword                              {fmt(summarize(run(vec, 'keyword')))}")
        print(f"vector                               {fmt(summarize(run(vec, 'vector')))}")
        for scale in ("none", "relative"):
            for k in (20.0, 60.0):
                for w_kw in (1.0, 0.5, 0.3):
                    m = summarize(run(vec, "hybrid", k=k, w_kw=w_kw, scale=scale))
                    print(f"rrf scale={scale:8} k={k:<4} kw={w_kw} vec=1.0  {fmt(m)}")
        print("\n== can a similarity floor tell an off-topic query from a relevant one?")
        rel_sims, top8_sims = [], []
        for q in queries:
            sims = dict(vec[q["id"]])
            rel_sims += [sims[c] for c in q["rel"]]
            top8_sims += [s for _, s in vec[q["id"]][:8]]
        rel_sims.sort()
        top8_sims.sort()
        pct = lambda xs, p: xs[int(p * (len(xs) - 1))]  # noqa: E731
        print(
            "labelled items: min %.3f p10 %.3f median %.3f | the 8 nearest of each query: min %.3f median %.3f"
            % (rel_sims[0], pct(rel_sims, 0.1), pct(rel_sims, 0.5), top8_sims[0], pct(top8_sims, 0.5))
        )
        off_topic = ["量子力學的薛丁格方程式", "微積分的極限與連續", "股票技術分析與K線圖", "今天天氣很好",
                     "How to bake sourdough bread at home", "The mitochondria is the powerhouse of the cell"]
        scope = [it for it in items if in_default_scope(it, "general")]
        docs = embed([embed_text(it, args.recipe) for it in scope])
        for text, qv in zip(off_topic, embed([query_text(t, args.recipe) for t in off_topic])):
            print("off-topic %-48s nearest %.3f" % (text, max(cosine(qv, d) for d in docs)))
        return

    vec = vector_lists(args.recipe)
    results = {m: run(vec, m) for m in ("keyword", "vector", "hybrid")}
    if args.dump:
        Path(args.dump).write_text(
            json.dumps(
                [
                    {"id": q["id"], "track": q["track"], "query": q["query"],
                     "similarities": dict(vec[q["id"]]),
                     "keyword": results["keyword"][q["id"]], "hybrid": results["hybrid"][q["id"]]}
                    for q in queries
                ],
                ensure_ascii=False,
            ),
            encoding="utf-8",
        )
    summary = {
        g: {m: summarize(results[m], subset) for m in results}
        for g, subset in groups.items()
    }
    counts = {g: sum(1 for q in queries if subset(q)) for g, subset in groups.items()}
    for g in groups:
        print(f"== {g} ({counts[g]} queries)")
        for m in results:
            print(f"  {m:8} {fmt(summary[g][m])}")

    per_query = []
    for q in queries:
        row = {"id": q["id"], "track": q["track"], "lang": q["lang"], "kind": q["kind"]}
        for m in results:
            row[m] = metrics(results[m][q["id"]], q["rel"])
        per_query.append(row)
    if args.examples:
        print("\n== per query: recall@8 keyword -> hybrid (vector), top 8 of each")
        for q, row in zip(queries, per_query):
            print(
                f"{q['id']} [{q['lang']}/{q['kind']}] r@8 {row['keyword']['recall@8']:.2f} -> "
                f"{row['hybrid']['recall@8']:.2f} ({row['vector']['recall@8']:.2f})  "
                f"r@20 {row['keyword']['recall@20']:.2f} -> {row['hybrid']['recall@20']:.2f}  "
                f"mrr {row['keyword']['mrr']:.2f} -> {row['hybrid']['mrr']:.2f}"
            )
            print("   Q:", q["query"].split("\n")[0][:70])
            for m in ("keyword", "hybrid"):
                top = results[m][q["id"]][:8]
                print(f"   {m:7}:", " ".join(("+" if c in q["rel"] else "-") + short(c) for c in top))
            missed = [c for c in q["rel"] if c not in results["hybrid"][q["id"]][:8]]
            if missed:
                print("   hybrid misses @8:", "; ".join(f"{short(c)} {by_code[c]['content'][:24]}" for c in missed))
    if args.json:
        Path(args.json).write_text(
            json.dumps({"counts": counts, "summary": summary, "per_query": per_query}, ensure_ascii=False, indent=1),
            encoding="utf-8",
        )


def short(code: str) -> str:
    for p in ("108-G-", "108-V-", "108-C-", "外語-技-"):
        if code.startswith(p):
            return code[len(p) :]
    return code


if __name__ == "__main__":
    main()
