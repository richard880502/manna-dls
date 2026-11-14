"""Probe the model's context window and maximum output (a handful of calls).

1. oversized prompt -> the API error normally states the context limit
2. growing prompts (binary-ish steps) until rejection, if (1) is not informative
3. long-output request with a large max_tokens -> finish_reason / completion_tokens

usage: python probe_limits.py
"""
import json

from llm import chat

FILLER = "The quick brown fox jumps over the lazy dog near the quiet river bank. "  # ~15 tokens


def main():
    res = {}
    r = chat("", FILLER * 40000 + "\nSay OK.", tag="probe-context-600k", log="probe.jsonl", retries=0, max_tokens=8)
    res["oversized"] = {"ok": r["ok"], "error": (r["error"] or "")[:500], "usage": r["usage"]}
    print(json.dumps(res["oversized"], ensure_ascii=False))
    for n in (2000, 8000, 16000):   # ~30k, ~120k, ~240k tokens
        r = chat("", FILLER * n + "\nSay OK.", tag=f"probe-context-{n}", log="probe.jsonl", retries=0, max_tokens=8)
        res[f"filler_x{n}"] = {"ok": r["ok"], "error": (r["error"] or "")[:300], "usage": r["usage"], "latency_s": r["latency_s"]}
        print(n, json.dumps(res[f"filler_x{n}"], ensure_ascii=False))
        if not r["ok"]:
            break
    r = chat("", "Count from 1 to 20000, one number per line, no other text. Do not stop early.",
             tag="probe-output", log="probe.jsonl", retries=0, max_tokens=100000, temperature=0)
    res["long_output"] = {"ok": r["ok"], "error": (r["error"] or "")[:300], "usage": r["usage"],
                          "finish_reason": r["finish_reason"], "latency_s": r["latency_s"],
                          "last_line": r["text"].strip().split("\n")[-1][:40]}
    print(json.dumps(res["long_output"], ensure_ascii=False))


if __name__ == "__main__":
    main()
