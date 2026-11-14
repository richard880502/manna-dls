"""Ablation for step (b): what happens when the model is NOT given the curriculum items and has to
cite 108-curriculum codes from memory? (This is the failure mode the retrieval + `cited_codes_exist`
guardrail exists to prevent.)

For every passage with a step-(a) output in the baseline run, the objectives prompt is sent without
the candidate list; the model must return the official code and its recollection of the item text.
Each citation is scored against the extracted curriculum items of the general track:

  code_exists           the code is a real item code
  text_matches          recalled text is essentially the real text of that code (CJK-bigram overlap >= 0.8)

usage: python runner/ablation_codes.py --base r1 [--repeat 2]
"""
from __future__ import annotations

import argparse
import copy
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "chain"))
sys.path.insert(0, str(ROOT / "runner"))

import schemas as SC  # noqa: E402
from common import DATA, analysis_summary, load_passages, load_prompt, numbered, render  # noqa: E402
from llm import chat  # noqa: E402

ROMAN = {"Ⅰ": "I", "Ⅱ": "II", "Ⅲ": "III", "Ⅳ": "IV", "Ⅴ": "V"}


def norm_code(c: str) -> str:
    for k, v in ROMAN.items():
        c = c.replace(k, v)
    c = re.sub(r"\s", "", c).replace("108-G-", "").lstrip("*◎")
    return c


def bigrams(s: str) -> set[str]:
    s = re.sub(r"[^一-鿿]", "", s)
    return {s[i:i + 2] for i in range(len(s) - 1)}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--repeat", type=int, default=1)
    a = ap.parse_args()
    items = json.loads((DATA / "kb" / "curriculum_items.json").read_text())
    real = {norm_code(r["official_code"]): r["text"] for r in items if r["school_track"] == "general"}
    schema = copy.deepcopy(SC.OBJECTIVES)
    link = schema["properties"]["objectives"]["items"]["properties"]["curriculum_links"]["items"]
    link["properties"]["item_text_zh"] = {"type": "string"}
    link["required"].append("item_text_zh")
    system, user_t = load_prompt("sop_objectives")
    system += "\n\n【本次特別說明】這次沒有提供課綱條目清單。請直接寫出你認為對應的 108 課綱（普通型高中英語文，第五學習階段）學習表現或學習內容的官方代碼（例如 3-Ⅴ-7、Ae-Ⅴ-9 這類格式），並在 item_text_zh 寫出該條目的條文原文。規則 5、6 中「只能從清單選擇」的限制本次不適用。"
    rows = []
    for p in load_passages():
        f = DATA / "runs" / "chain" / a.base / p["id"] / "a.json"
        if not f.exists():
            continue
        an = json.loads(f.read_text())["output"]
        vars = {"school_track": "普通型高中", "grade": p["grade"], "student_info": p["student_info"], "minutes": "50",
                "passage_numbered": numbered(p["paragraphs"]), "analysis_summary": analysis_summary(an),
                "curriculum_items": "（本次未提供清單）"}
        for k in range(a.repeat):
            r = chat(system, render(user_t, vars), schema, "sop_objectives_nolist", tag=f"ablation:{p['id']}:{k}",
                     log="ablation.jsonl", temperature=0.2, max_tokens=6000)
            if not r["ok"]:
                print(p["id"], k, "error", r["error"])
                continue
            for o in r["json"]["objectives"]:
                for l in o["curriculum_links"]:
                    code = norm_code(l["item_code"])
                    exists = code in real
                    overlap = None
                    if exists:
                        rb, mb = bigrams(real[code]), bigrams(l["item_text_zh"])
                        overlap = round(len(rb & mb) / max(len(rb | mb), 1), 2)
                    rows.append({"passage": p["id"], "rep": k, "cited": l["item_code"], "code_exists": exists,
                                 "recalled_text": l["item_text_zh"], "real_text": real.get(code), "text_overlap": overlap,
                                 "text_matches": bool(exists and overlap is not None and overlap >= 0.8)})
    n = len(rows)
    res = {"citations": n, "code_exists": sum(r["code_exists"] for r in rows),
           "text_matches": sum(r["text_matches"] for r in rows), "rows": rows}
    (DATA / "runs" / "ablation_codes.json").write_text(json.dumps(res, ensure_ascii=False, indent=1))
    print(json.dumps({k: v for k, v in res.items() if k != "rows"}, ensure_ascii=False))
    for r in rows[:12]:
        print(r["cited"], r["code_exists"], r["text_overlap"], "|", r["recalled_text"][:40], "|", (r["real_text"] or "")[:40])


if __name__ == "__main__":
    main()
