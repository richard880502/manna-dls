"""Print a compact per-step log of one run: attempts, latency, tokens, failed checks.

  python runner/show.py <run> [passage ...] [--soft]
"""
import json
import os
import sys
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    soft = "--soft" in sys.argv
    run, passages = args[0], args[1:]
    for f in sorted((DATA / "runs" / "chain" / run).glob("*/*.json")):
        r = json.loads(f.read_text())
        if passages and r["passage"] not in passages:
            continue
        if "attempts" not in r:
            bad = [(c["name"], c["detail"]) for c in r["checks"] if not c["passed"]]
            print(r["passage"], r["step"], "PASS" if r["pass_final"] else "FAIL", bad)
            continue
        if r.get("reused_from"):
            print(r["passage"], r["step"], "reused from", r["reused_from"])
            continue
        for a in r["attempts"]:
            print(r["passage"], r["step"], f"a{a['attempt']}", f"{a['latency_s']}s", a["prompt_tokens"], a["completion_tokens"],
                  "PASS" if a["hard_pass"] else "FAIL", a["error"] or "", "soft:" + ",".join(a["soft_fail"]) if a["soft_fail"] else "")
            for c in a["checks"]:
                if not c["passed"] and (c["hard"] or soft):
                    print("     ", "HARD" if c["hard"] else "soft", c["name"], f"{c['ok']}/{c['n']}", c["detail"][:3])


if __name__ == "__main__":
    main()
