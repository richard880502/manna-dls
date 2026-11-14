"""Shortlist representative worksheets for few-shot use and build the de-identified few-shot block.

Selection criteria (all computed from the inventory; see docs/LLM_VALIDATION.md §少量代表性範例):
  hard filters
    - reading-based worksheet (課文閱讀理解 / 綜合講義), instructions in English
    - not truncated when classified, 4-9 sections (a complete but compact structure)
    - has a warm-up, and ends with an 分析/比較/創造 task
  score
    + distinct information functions (max 5)           + distinct answer formats (max 4)
    + scaffold types present (verbal / instructional / procedural, max 3)
    + 1 if it has a closing reflection / exit ticket   + 1 if it shows learning objectives
  diversity
    - at most 2 per publisher+book, at most 1 per lesson, at most 2 per "series" (same designer's
      template, approximated by the file-name pattern), so one template cannot dominate

The few-shot block contains only the activity skeleton (activity kind, information function,
student-task labels, answer formats, scaffolds) and a one-sentence design note written by the
model -- no worksheet text, no titles, no teacher or school names.

Outputs (local): $MANNA_DATA/worksheets/fewshot_shortlist.json, fewshot_for_prompt.txt
usage: python select_fewshot.py [--k 6]
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import re
from collections import Counter
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
HIGHER = {"分析資訊", "比較資訊", "創造資訊"}


def series_key(name: str) -> str:
    n = re.sub(r"^\d{4}_(\d\d_)?", "", name)
    for pat, key in [(r"課文深度閱讀", "deep-reading"), (r"課文閱讀講義", "reading-handout"), (r"^LT B\dU\d", "lt-bu"),
                     (r"^Lungteng|^龍騰 B\d ?L\d+ 課文講義", "lt-handout"), (r"^114[-_]1", "114-1"), (r"^SM ?B\d", "sm-b"),
                     (r"提問單", "question-sheet")]:
        if re.search(pat, n):
            return key
    return re.sub(r"[^A-Za-z一-鿿]", "", n)[:6]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--k", type=int, default=6)
    a = ap.parse_args()
    ws = DATA / "worksheets"
    files = {r["id"]: r for r in csv.DictReader(open(ws / "files.csv", encoding="utf-8"))}
    rows = {}
    for line in open(ws / "inventory.jsonl"):
        r = json.loads(line)
        if r.get("ok"):
            rows[r["id"]] = r
    cands = []
    for wid, r in sorted(rows.items()):
        l = r["label"]
        seq = l["task_sequence"]
        if l["worksheet_type"] not in ("課文閱讀理解", "綜合講義") or l["instruction_language"] != "英文":
            continue
        if r["truncated"] or not 4 <= len(seq) <= 9 or not l["has_warm_up"]:
            continue
        if not (set(seq[-1]["info_functions"]) & HIGHER):
            continue
        funcs = {f for t in seq for f in t["info_functions"]}
        fmts = {f for t in seq for f in t["answer_formats"]}
        scaf = sum(bool(l[k]) for k in ("verbal_scaffolds", "instructional_scaffolds", "procedural_scaffolds"))
        score = len(funcs) + min(len(fmts), 4) + scaf + l["has_exit_ticket_or_closing_reflection"] + l["has_learning_objectives"]
        cands.append({"id": wid, "score": score, "publisher": l["publisher"], "book": l["book"], "lesson": l["lesson"],
                      "series": series_key(files[wid]["file"]), "n_sections": len(seq), "functions": sorted(funcs),
                      "formats": sorted(fmts), "closing": l["has_exit_ticket_or_closing_reflection"],
                      "objectives": l["has_learning_objectives"]})
    cands.sort(key=lambda c: (-c["score"], c["id"]))
    picked, per_book, per_series, lessons = [], Counter(), Counter(), set()
    for c in cands:
        book = (c["publisher"], c["book"])
        lesson = (c["publisher"], c["book"], c["lesson"])
        if per_book[book] >= 2 or per_series[c["series"]] >= 2 or (c["lesson"] and lesson in lessons):
            continue
        picked.append(c)
        per_book[book] += 1
        per_series[c["series"]] += 1
        lessons.add(lesson)
        if len(picked) == a.k:
            break
    (ws / "fewshot_shortlist.json").write_text(json.dumps({"eligible": len(cands), "picked": picked}, ensure_ascii=False, indent=1))

    blocks = []
    for i, c in enumerate(picked[:3], 1):          # 3 examples in the prompt; the rest are alternates
        l = rows[c["id"]]["label"]
        lines = [f"範例 {i}（{l['worksheet_type']}，共 {len(l['task_sequence'])} 個活動區塊）"]
        for j, t in enumerate(l["task_sequence"], 1):
            lines.append(f"  {j}. {t['activity_kind']}｜功能：{'、'.join(t['info_functions']) or '（語言練習）'}"
                         f"｜學生任務：{'、'.join(t['student_tasks']) or '—'}｜作答方式：{'、'.join(dict.fromkeys(t['answer_formats'])) or '—'}")
        lines.append(f"  鷹架：語言（{'、'.join(l['verbal_scaffolds'])}）；教學（{'、'.join(l['instructional_scaffolds'])}）；"
                     f"程序（{'、'.join(l['procedural_scaffolds'])}）")
        note = re.sub(r"\$\\?\w*arrow\$", "→", l["design_notes"])
        lines.append(f"  設計手法：{note}")
        blocks.append("\n".join(lines))
    (ws / "fewshot_for_prompt.txt").write_text("\n\n".join(blocks) + "\n")
    print("eligible", len(cands), "picked", [(c["id"], c["score"], c["publisher"], c["book"], c["lesson"], c["series"]) for c in picked])
    print((ws / "fewshot_for_prompt.txt").read_text())


if __name__ == "__main__":
    main()
