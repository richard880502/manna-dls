"""Totals over the call logs in $MANNA_DATA/runs/*.jsonl: calls, failures by kind, cache hits,
latency and output size. usage: python runner/call_stats.py"""
import json
import os
from collections import Counter
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def main():
    total = Counter()
    for f in sorted((DATA / "runs").glob("*.jsonl")):
        c = Counter()
        max_out = 0
        for line in open(f):
            r = json.loads(line)
            c["calls"] += 1
            err = r.get("error") or ""
            if r["ok"]:
                c["ok"] += 1
                max_out = max(max_out, (r.get("usage") or {}).get("completion_tokens") or 0)
            elif err == "max_tokens":
                c["max_tokens_cached_retry" if r["latency_s"] < 3 else "max_tokens_runaway"] += 1
            elif err.startswith("transport"):
                c["timeout_or_empty"] += 1
            elif err.startswith("invalid_json"):
                c["invalid_json"] += 1
            else:
                c["api_error"] += 1
        c["max_completion_tokens_ok"] = max_out
        print(f.name, dict(c))
        for k, v in c.items():
            if k == "max_completion_tokens_ok":
                total[k] = max(total[k], v)
            else:
                total[k] += v
    print("TOTAL", dict(total))


if __name__ == "__main__":
    main()
