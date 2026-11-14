"""Aggregate the LLM-built worksheet inventory into de-identified statistics, and compare the
model's labels with the deterministic regex signals.

Input : $MANNA_DATA/worksheets/inventory.jsonl
Output: $MANNA_DATA/worksheets/inventory.csv        (flat, one row per worksheet; local only)
        $MANNA_DATA/worksheets/inventory_stats.json (aggregates only; safe to quote in the report)

usage: python inventory_stats.py
"""
from __future__ import annotations

import csv
import json
import os
import statistics as st
from collections import Counter
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def pct(n, d):
    return round(100 * n / d, 1) if d else 0.0


def main() -> None:
    ws = DATA / "worksheets"
    rows = {}
    for line in open(ws / "inventory.jsonl"):
        r = json.loads(line)
        if r.get("ok") and r.get("label"):
            rows[r["id"]] = r
    rows = [rows[k] for k in sorted(rows)]
    n = len(rows)
    L = [r["label"] for r in rows]

    seqs = [[t["activity_kind"] for t in l["task_sequence"]] for l in L]
    dedup = [[k for i, k in enumerate(s) if i == 0 or s[i - 1] != k] for s in seqs]
    first = Counter(s[0] for s in seqs if s)
    last = Counter(s[-1] for s in seqs if s)
    bigrams = Counter((a, b) for s in dedup for a, b in zip(s, s[1:]))
    funcs_per_ws = [sorted({f for t in l["task_sequence"] for f in t["info_functions"]}) for l in L]
    fmts_per_ws = [sorted({f for t in l["task_sequence"] for f in t["answer_formats"]}) for l in L]
    tasks_per_ws = [sorted({f for t in l["task_sequence"] for f in t["student_tasks"]}) for l in L]

    def share(list_of_sets):
        c = Counter(x for s in list_of_sets for x in s)
        return {k: {"worksheets": v, "pct": pct(v, n)} for k, v in c.most_common()}

    # progression of information functions inside reading worksheets: position of first occurrence
    order = {"發現資訊": [], "整理資訊": [], "分析資訊": [], "比較資訊": [], "創造資訊": []}
    for l in L:
        seq = l["task_sequence"]
        for f in order:
            idx = [i for i, t in enumerate(seq) if f in t["info_functions"]]
            if idx and len(seq) > 1:
                order[f].append(idx[0] / (len(seq) - 1))

    reading = [l for l in L if l["worksheet_type"] in ("課文閱讀理解", "綜合講義")]
    by_type = {}
    for t in sorted({l["worksheet_type"] for l in L}):
        sub = [l for l in L if l["worksheet_type"] == t]
        by_type[t] = {
            "n": len(sub),
            "has_warm_up_pct": pct(sum(l["has_warm_up"] for l in sub), len(sub)),
            "has_closing_pct": pct(sum(l["has_exit_ticket_or_closing_reflection"] for l in sub), len(sub)),
            "median_sections": st.median(len(l["task_sequence"]) for l in sub),
            "any_higher_order_pct": pct(sum(any(f in ("分析資訊", "比較資訊", "創造資訊") for t_ in l["task_sequence"] for f in t_["info_functions"]) for l in sub), len(sub)),
        }

    # agreement between LLM labels and regex signals (neither is ground truth)
    def agree(label_key, rx_key):
        tp = sum(1 for r in rows if r["label"][label_key] and r["signals"][rx_key])
        fp = sum(1 for r in rows if r["label"][label_key] and not r["signals"][rx_key])
        fn = sum(1 for r in rows if not r["label"][label_key] and r["signals"][rx_key])
        tn = n - tp - fp - fn
        return {"llm_yes_rx_yes": tp, "llm_yes_rx_no": fp, "llm_no_rx_yes": fn, "llm_no_rx_no": tn,
                "agreement_pct": pct(tp + tn, n)}

    stats = {
        "worksheets_classified": n,
        "truncated_for_model": sum(r["truncated"] for r in rows),
        "latency_s_median": st.median(r["latency_s"] for r in rows),
        "prompt_tokens_median": st.median(r["usage"].get("prompt_tokens", 0) for r in rows),
        "completion_tokens_median": st.median(r["usage"].get("completion_tokens", 0) for r in rows),
        "publisher": dict(Counter(l["publisher"] for l in L).most_common()),
        "worksheet_type": dict(Counter(l["worksheet_type"] for l in L).most_common()),
        "instruction_language": dict(Counter(l["instruction_language"] for l in L).most_common()),
        "contains_reading_passage_pct": pct(sum(l["contains_reading_passage"] for l in L), n),
        "has_warm_up_pct": pct(sum(l["has_warm_up"] for l in L), n),
        "has_exit_or_closing_reflection_pct": pct(sum(l["has_exit_ticket_or_closing_reflection"] for l in L), n),
        "has_learning_objectives_pct": pct(sum(l["has_learning_objectives"] for l in L), n),
        "has_answer_key_pct": pct(sum(l["has_answer_key"] for l in L), n),
        "has_differentiation_pct": pct(sum(l["has_differentiation"] for l in L), n),
        "sections_per_worksheet": {"median": st.median(len(s) for s in seqs), "p25": st.quantiles([len(s) for s in seqs], n=4)[0],
                                   "p75": st.quantiles([len(s) for s in seqs], n=4)[2]},
        "first_activity": dict(first.most_common(8)),
        "last_activity": dict(last.most_common(8)),
        "common_transitions": [{"from": a, "to": b, "count": c} for (a, b), c in bigrams.most_common(15)],
        "info_functions": share(funcs_per_ws),
        "distinct_info_functions_per_worksheet": dict(sorted(Counter(len(f) for f in funcs_per_ws).items())),
        "info_function_relative_position_median(0=start,1=end)": {k: round(st.median(v), 2) for k, v in order.items() if v},
        "answer_formats": share(fmts_per_ws),
        "distinct_answer_formats_per_worksheet_median": st.median(len(f) for f in fmts_per_ws),
        "student_tasks": share(tasks_per_ws),
        "verbal_scaffolds": share([l["verbal_scaffolds"] for l in L]),
        "instructional_scaffolds": share([l["instructional_scaffolds"] for l in L]),
        "procedural_scaffolds": share([l["procedural_scaffolds"] for l in L]),
        "by_type": by_type,
        "reading_worksheets": {
            "n": len(reading),
            "has_warm_up_pct": pct(sum(l["has_warm_up"] for l in reading), len(reading)),
            "has_closing_pct": pct(sum(l["has_exit_ticket_or_closing_reflection"] for l in reading), len(reading)),
            "ends_with_create_pct": pct(sum(bool(l["task_sequence"]) and "創造資訊" in l["task_sequence"][-1]["info_functions"] for l in reading), len(reading)),
            "starts_with_discover_or_warmup_pct": pct(sum(bool(l["task_sequence"]) and (l["task_sequence"][0]["activity_kind"] == "暖身" or "發現資訊" in l["task_sequence"][0]["info_functions"]) for l in reading), len(reading)),
        },
        "llm_vs_regex": {
            "warm_up": agree("has_warm_up", "rx_warm_up"),
            "closing_reflection": agree("has_exit_ticket_or_closing_reflection", "rx_exit"),
            "learning_objectives": agree("has_learning_objectives", "rx_objectives"),
            "answer_key": agree("has_answer_key", "rx_answer_key"),
        },
    }
    (ws / "inventory_stats.json").write_text(json.dumps(stats, ensure_ascii=False, indent=1))

    with open(ws / "inventory.csv", "w", newline="", encoding="utf-8") as f:
        cols = ["id", "format", "chars", "truncated", "publisher", "book", "lesson", "lesson_title", "worksheet_type",
                "contains_reading_passage", "instruction_language", "n_sections", "activity_sequence", "info_functions",
                "student_tasks", "answer_formats", "verbal_scaffolds", "instructional_scaffolds", "procedural_scaffolds",
                "has_warm_up", "has_exit_ticket_or_closing_reflection", "has_learning_objectives", "has_answer_key",
                "has_differentiation", "design_notes"]
        w = csv.DictWriter(f, fieldnames=cols)
        w.writeheader()
        for r, s, fu, fm, tk in zip(rows, seqs, funcs_per_ws, fmts_per_ws, tasks_per_ws):
            l = r["label"]
            w.writerow({**{k: l.get(k) for k in cols if k in l}, "id": r["id"], "format": r["format"], "chars": r["chars"],
                        "truncated": r["truncated"], "n_sections": len(s), "activity_sequence": " > ".join(s),
                        "info_functions": "、".join(fu), "student_tasks": "、".join(tk), "answer_formats": "、".join(fm),
                        "verbal_scaffolds": "、".join(l["verbal_scaffolds"]),
                        "instructional_scaffolds": "、".join(l["instructional_scaffolds"]),
                        "procedural_scaffolds": "、".join(l["procedural_scaffolds"])})
    print(json.dumps(stats, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
