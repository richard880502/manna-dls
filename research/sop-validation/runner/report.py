"""Aggregate chain runs into per-step tables (Markdown on stdout, JSON under $MANNA_DATA/runs/).

  python runner/report.py r1 r2                 # pooled table for the main runs
  python runner/report.py r1-rules --compare r1 # (c)/(g) with rules+few-shot vs. zero-shot on the same passages
"""
from __future__ import annotations

import argparse
import json
import os
import statistics as st
from collections import defaultdict
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
STEP_NAMES = {"a": "(a) 逐段教材分析", "b": "(b) SIOP 目標＋課綱連結", "c": "(c) 逐段問題／任務", "d": "(d) 暖身",
              "e": "(e) 三種鷹架", "f": "(f) Exit ticket", "g": "(g) 學習單組裝（每版一次）", "g-versions": "(g) 跨版本一致性"}


def load(run: str, passages=None, include_reused=False):
    recs = []
    for f in sorted((DATA / "runs" / "chain" / run).glob("*/*.json")):
        r = json.loads(f.read_text())
        if passages and r["passage"] not in passages:
            continue
        if r.get("reused_from") and not include_reused:
            continue
        recs.append(r)
    return recs


def step_of(r):
    return "g" if r["step"].startswith("g-") and r["step"] != "g-versions" else r["step"]


def aggregate(recs):
    by = defaultdict(list)
    for r in recs:
        by[step_of(r)].append(r)
    table, checks = [], {}
    for step in STEP_NAMES:
        rs = by.get(step, [])
        if not rs:
            continue
        if step == "g-versions":
            cs = defaultdict(lambda: [0, 0])
            for r in rs:
                for c in r["checks"]:
                    cs[c["name"]][0] += c["passed"]
                    cs[c["name"]][1] += 1
            checks[step] = {k: {"runs_passed": v[0], "runs": v[1], "hard": True} for k, v in cs.items()}
            table.append({"step": step, "calls": len(rs), "pass_at_1": None, "pass_final": sum(r["pass_final"] for r in rs),
                          "n": len(rs)})
            continue
        first = [r["attempts"][0] for r in rs]
        lat = [a["latency_s"] for r in rs for a in r["attempts"] if a["api_ok"]]
        ct = [a["completion_tokens"] for r in rs for a in r["attempts"] if a["api_ok"] and a["completion_tokens"]]
        pt = [a["prompt_tokens"] for r in rs for a in r["attempts"] if a["api_ok"] and a["prompt_tokens"]]
        table.append({
            "step": step, "n": len(rs),
            "api_json_ok_at_1": sum(a["api_ok"] for a in first),
            "schema_ok_at_1": sum(bool(a["checks"]) and a["checks"][0]["passed"] for a in first),
            "pass_at_1": sum(r["pass_at_1"] for r in rs), "pass_final": sum(r["pass_final"] for r in rs),
            "latency_median_s": round(st.median(lat), 1) if lat else None, "latency_max_s": max(lat) if lat else None,
            "prompt_tokens_median": int(st.median(pt)) if pt else None,
            "completion_tokens_median": int(st.median(ct)) if ct else None,
            "completion_tokens_max": max(ct) if ct else None,
        })
        cs = defaultdict(lambda: {"runs_passed": 0, "runs": 0, "items_ok": 0, "items": 0, "hard": None, "examples": []})
        for a in first:
            for c in a["checks"]:
                d = cs[c["name"]]
                d["runs"] += 1
                d["runs_passed"] += c["passed"]
                d["items"] += c["n"]
                d["items_ok"] += c["ok"]
                d["hard"] = c["hard"]
                if not c["passed"] and len(d["examples"]) < 3:
                    d["examples"] += c["detail"][:1]
        checks[step] = dict(cs)
    return table, checks


def md_table(table):
    out = ["| 步驟 | 次數 | JSON/Schema 有效（第 1 次） | 硬性檢查全過（第 1 次） | 修復 1 次後通過 | 延遲中位數／最大 (s) | 輸入／輸出 tokens 中位數（輸出最大） |",
           "|---|---|---|---|---|---|---|"]
    for t in table:
        if t["step"] == "g-versions":
            out.append(f"| {STEP_NAMES[t['step']]} | {t['n']} | – | – | {t['pass_final']}/{t['n']} | – | – |")
            continue
        out.append(f"| {STEP_NAMES[t['step']]} | {t['n']} | {t['schema_ok_at_1']}/{t['n']} | {t['pass_at_1']}/{t['n']} | "
                   f"{t['pass_final']}/{t['n']} | {t['latency_median_s']}／{t['latency_max_s']} | "
                   f"{t['prompt_tokens_median']}／{t['completion_tokens_median']}（{t['completion_tokens_max']}） |")
    return "\n".join(out)


def md_checks(checks):
    out = ["| 步驟 | 檢查 | 類型 | 第 1 次通過（次） | 項目層級通過率 | 失敗例 |", "|---|---|---|---|---|---|"]
    for step, cs in checks.items():
        for name, d in cs.items():
            items = f"{d['items_ok']}/{d['items']}" if d.get("items") else "–"
            ex = "；".join(e.replace("|", "/").replace("\n", " ")[:90] for e in d.get("examples", []))
            out.append(f"| {step} | `{name}` | {'硬' if d['hard'] else '軟'} | {d['runs_passed']}/{d['runs']} | {items} | {ex} |")
    return "\n".join(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("runs", nargs="+")
    ap.add_argument("--compare", nargs="*", help="baseline run ids (restricted to the same passages)")
    ap.add_argument("--out", default="report")
    a = ap.parse_args()
    recs = [r for run in a.runs for r in load(run)]
    table, checks = aggregate(recs)
    result = {"runs": a.runs, "table": table, "checks": checks}
    print(f"## runs: {', '.join(a.runs)}\n")
    print(md_table(table), "\n")
    print(md_checks(checks), "\n")
    if a.compare:
        passages = {r["passage"] for r in recs}
        steps = {step_of(r) for r in recs}
        base = [r for run in a.compare for r in load(run, passages) if step_of(r) in steps]
        bt, bc = aggregate(base)
        result["baseline"] = {"runs": a.compare, "table": bt, "checks": bc}
        print(f"## baseline (same passages {sorted(passages)}): {', '.join(a.compare)}\n")
        print(md_table(bt), "\n")
        print(md_checks(bc), "\n")
    (DATA / "runs" / f"{a.out}.json").write_text(json.dumps(result, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
