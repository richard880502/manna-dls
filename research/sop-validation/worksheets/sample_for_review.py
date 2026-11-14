"""Print a seeded random sample of worksheets (model labels + head/tail of the text) for manual
review of the inventory. Output contains worksheet text: read it locally, never commit it.

usage: python sample_for_review.py [--n 16] [--seed 20261114] [--offset 0 --count 8]
"""
import argparse
import json
import os
import random
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=16)
    ap.add_argument("--seed", type=int, default=20261114)
    ap.add_argument("--offset", type=int, default=0)
    ap.add_argument("--count", type=int, default=8)
    ap.add_argument("--head", type=int, default=1300)
    ap.add_argument("--tail", type=int, default=600)
    a = ap.parse_args()
    ws = DATA / "worksheets"
    rows = {}
    for line in open(ws / "inventory.jsonl"):
        r = json.loads(line)
        if r.get("ok"):
            rows[r["id"]] = r
    ids = sorted(rows)
    random.seed(a.seed)
    sample = sorted(random.sample(ids, a.n))
    print("sample ids:", sample)
    for wid in sample[a.offset: a.offset + a.count]:
        l = rows[wid]["label"]
        text = (ws / "text" / f"{wid}.txt").read_text()
        seq = [(t["section_title"][:22], t["activity_kind"], "/".join(t["info_functions"]), "/".join(t["answer_formats"])) for t in l["task_sequence"]]
        print("=" * 100)
        print(wid, "| chars", len(text), "| LLM:", l["publisher"], l["book"], l["lesson"], "|", l["worksheet_type"],
              "| warm", l["has_warm_up"], "| closing", l["has_exit_ticket_or_closing_reflection"], "| obj", l["has_learning_objectives"],
              "| key", l["has_answer_key"], "| diff", l["has_differentiation"], "| passage", l["contains_reading_passage"], "| lang", l["instruction_language"])
        print("  seq:", seq)
        print("  scaffolds:", l["verbal_scaffolds"], l["instructional_scaffolds"], l["procedural_scaffolds"])
        print("  rx:", {k: v for k, v in rows[wid]["signals"].items() if v})
        print("--- head ---")
        print(" ".join(text[: a.head].split()))
        print("--- tail ---")
        print(" ".join(text[-a.tail:].split()))


if __name__ == "__main__":
    main()
