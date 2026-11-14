"""Build the structured worksheet inventory with the LLM (one call per worksheet).

Input : $MANNA_DATA/worksheets/text/<id>.txt, files.csv   (from extract_text.py)
Output: $MANNA_DATA/worksheets/inventory.jsonl             (one JSON per worksheet, resumable)
        $MANNA_DATA/worksheets/inventory.csv               (flat view)

The model sees at most HEAD+TAIL characters of each worksheet (long handouts are truncated in the
middle; `truncated` is recorded). Deterministic regex signals are stored next to the model's labels
so the two can be compared (see inventory_stats.py).

usage: python classify.py [--limit N] [--workers 3]
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import re
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from labels import ANSWER_FORMATS, INFO_FUNCTIONS, STUDENT_TASKS  # noqa: E402
from llm import chat  # noqa: E402

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
HEAD, TAIL = 11000, 4000

WORKSHEET_TYPES = ["課文閱讀理解", "字彙", "文法句型", "寫作", "聽力口說", "綜合講義", "專題成果", "反思評量", "其他"]
ACTIVITY_KINDS = ["暖身", "字彙", "閱讀理解", "文本結構分析", "文法句型", "討論", "寫作", "口說發表", "聽力",
                  "摘要", "延伸應用", "反思自評", "Exit ticket", "其他"]
PUBLISHERS = ["三民", "龍騰", "翰林", "南一", "東大", "泰宇", "遠東", "其他", "無法判斷"]
VERBAL = ["引導問題", "字詞表", "句型框架", "作答示例", "透過上下文解釋詞義", "提示與追問"]
INSTRUCTIONAL = ["圖像組織", "部分完成的大綱或內容", "調整文本", "標示關鍵概念或字彙"]
PROCEDURAL = ["清楚的分步指示及完成順序", "個別或小組指導安排", "策略性配對或分組", "逐步減少支持"]


def enum_array(values):
    return {"type": "array", "items": {"type": "string", "enum": values}}


SCHEMA = {
    "type": "object",
    "properties": {
        "publisher": {"type": "string", "enum": PUBLISHERS},
        "book": {"anyOf": [{"type": "string"}, {"type": "null"}]},
        "lesson": {"anyOf": [{"type": "string"}, {"type": "null"}]},
        "lesson_title": {"anyOf": [{"type": "string"}, {"type": "null"}]},
        "worksheet_type": {"type": "string", "enum": WORKSHEET_TYPES},
        "contains_reading_passage": {"type": "boolean"},
        "instruction_language": {"type": "string", "enum": ["英文", "中文", "中英混合"]},
        "task_sequence": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "section_title": {"type": "string"},
                    "activity_kind": {"type": "string", "enum": ACTIVITY_KINDS},
                    "info_functions": enum_array(INFO_FUNCTIONS),
                    "student_tasks": enum_array(STUDENT_TASKS),
                    "answer_formats": enum_array(ANSWER_FORMATS),
                },
                "required": ["section_title", "activity_kind", "info_functions", "student_tasks", "answer_formats"],
                "additionalProperties": False,
            },
        },
        "verbal_scaffolds": enum_array(VERBAL),
        "instructional_scaffolds": enum_array(INSTRUCTIONAL),
        "procedural_scaffolds": enum_array(PROCEDURAL),
        "has_warm_up": {"type": "boolean"},
        "has_exit_ticket_or_closing_reflection": {"type": "boolean"},
        "has_learning_objectives": {"type": "boolean"},
        "has_answer_key": {"type": "boolean"},
        "has_differentiation": {"type": "boolean"},
        "design_notes": {"type": "string"},
    },
    "required": ["publisher", "book", "lesson", "lesson_title", "worksheet_type", "contains_reading_passage",
                 "instruction_language", "task_sequence", "verbal_scaffolds", "instructional_scaffolds",
                 "procedural_scaffolds", "has_warm_up", "has_exit_ticket_or_closing_reflection",
                 "has_learning_objectives", "has_answer_key", "has_differentiation", "design_notes"],
    "additionalProperties": False,
}

SYSTEM = """你是協助研究團隊整理高中英文教師自製學習單的分析助理。你會讀到一份學習單擷取出的純文字（版面、圖片與表格線已遺失，表格列以「 | 」分隔，過長的文件中段會被省略），請只依據文字內容，如實標註這份學習單的設計特徵。

標註規則：
1. 只標註文字中看得到的證據；看不出來就選「無法判斷」、null、false 或空陣列，不要猜測。
2. publisher / book / lesson：從檔名或內文判斷教科書版本（三民 SM、龍騰 LT/Lungteng、翰林 HL、南一、東大、泰宇、遠東），冊次寫成「B1」這類格式，課次寫成「L5」或「U11」這類格式。
3. task_sequence：依出現順序列出主要活動區塊（最多 12 個）；section_title 用學習單上的原標題（不超過 10 個字詞），沒有標題就用 3-6 個字描述。
4. info_functions（資訊處理功能）：發現資訊＝從文本找出明示訊息；整理資訊＝分類、排序、填表、畫圖整理；分析資訊＝推論、找原因、判斷作者意圖或評估；比較資訊＝比較異同；創造資訊＝產出新的句子、段落、作品或個人觀點。純字彙或文法機械練習不屬於任何一項，請給空陣列。
5. student_tasks 與 answer_formats 只能使用提供的標籤；沒有合適標籤就給空陣列。
6. 鷹架：語言鷹架（引導問題、字詞表、句型框架、作答示例、透過上下文解釋詞義、提示與追問）；教學鷹架（圖像組織、部分完成的大綱或內容、調整文本、標示關鍵概念或字彙）；程序鷹架（清楚的分步指示及完成順序、個別或小組指導安排、策略性配對或分組、逐步減少支持）。
7. has_warm_up：主要任務之前有喚起背景知識、預測、看圖/看標題討論或先備字彙的活動才算 true。
8. has_exit_ticket_or_closing_reflection：學習單末端有 Exit ticket、學習反思、自我評量或「今天學到什麼／還有什麼問題」這類收尾才算 true。
9. has_differentiation：同一份學習單明確區分難度（如基礎/進階、挑戰題、A/B 版）才算 true。
10. design_notes：用 1-2 句繁體中文說明這份學習單最值得借鏡的設計手法；不要抄錄學習單原文，不要出現教師姓名或校名。"""


def regex_signals(text: str) -> dict:
    t = text.lower()
    return {
        "rx_warm_up": bool(re.search(r"warm[- ]?up|before (you )?read|pre-?reading|lead[- ]?in|暖身|讀前", t)),
        "rx_exit": bool(re.search(r"exit ticket|reflection|self[- ]?(assessment|evaluation|check)|rating time|反思|自評|自我評量|學到什麼|省思", t)),
        "rx_blank": len(re.findall(r"_{4,}", text)),
        "rx_choice": len(re.findall(r"\(A\)", text)),
        "rx_tf": bool(re.search(r"\bT\s*/\s*F\b|true or false|true/false|是非", t)),
        "rx_matching": bool(re.search(r"\bmatch(ing)?\b|配對|配合題", t)),
        "rx_objectives": bool(re.search(r"learning (objective|goal|target)s?|學習目標|i can ", t)),
        "rx_answer_key": bool(re.search(r"answer key|參考答案|解答|教師版", t)),
        "rx_group": bool(re.search(r"partner|group|pair|小組|分組|同伴", t)),
        "rx_graphic": bool(re.search(r"mind ?map|venn|graphic organizer|timeline|flow ?chart|心智圖|圖像組織|時間軸", t)),
    }


def classify(row: dict) -> dict:
    text = (DATA / "worksheets" / "text" / f"{row['id']}.txt").read_text()
    truncated = len(text) > HEAD + TAIL
    shown = text if not truncated else text[:HEAD] + "\n\n[…中段省略…]\n\n" + text[-TAIL:]
    user = f"檔名：{row['file']}\n格式：{row['format']}　全文字數：{len(text)}\n\n=== 學習單文字開始 ===\n{shown}\n=== 學習單文字結束 ==="
    r = chat(SYSTEM, user, SCHEMA, "worksheet_inventory", tag=f"classify:{row['id']}", log="classify.jsonl",
             temperature=0, max_tokens=4000)   # cap: one worksheet sent the model into runaway output
    if r["error"] == "max_tokens":
        # runaway repetition inside an enum array at temperature 0: retry once with sampling
        r = chat(SYSTEM, user, SCHEMA, "worksheet_inventory", tag=f"classify:{row['id']}:t0.5", log="classify.jsonl",
                 temperature=0.5, max_tokens=4000)
    return {"id": row["id"], "format": row["format"], "chars": len(text), "truncated": truncated,
            "ok": r["ok"], "error": r["error"], "latency_s": r["latency_s"], "usage": r["usage"],
            "label": r["json"], "signals": regex_signals(text)}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--from-log", action="store_true", help="rebuild missing results from runs/classify.jsonl")
    a = ap.parse_args()
    ws = DATA / "worksheets"
    rows = list(csv.DictReader(open(ws / "files.csv", encoding="utf-8")))
    out = ws / "inventory.jsonl"
    done = set()
    if out.exists():
        done = {json.loads(l)["id"] for l in open(out) if json.loads(l).get("ok")}
    todo = [r for r in rows if r["id"] not in done]
    if a.limit:
        todo = todo[: a.limit]
    if a.from_log:
        # rebuild results from the call log (e.g. after an interrupted run); no API calls
        by_id = {r["id"]: r for r in todo}
        with open(out, "a") as f:
            for line in open(DATA / "runs" / "classify.jsonl"):
                c = json.loads(line)
                wid = c["tag"].split(":")[-1]
                if c["ok"] and wid in by_id:
                    row = by_id.pop(wid)
                    text = (ws / "text" / f"{wid}.txt").read_text()
                    f.write(json.dumps({"id": wid, "format": row["format"], "chars": len(text),
                                        "truncated": len(text) > HEAD + TAIL, "ok": True, "error": None,
                                        "latency_s": c["latency_s"], "usage": c["usage"], "label": c["json"],
                                        "signals": regex_signals(text)}, ensure_ascii=False) + "\n")
        print("recovered; still missing:", sorted(by_id))
        return
    with ThreadPoolExecutor(a.workers) as ex, open(out, "a") as f:
        # as_completed: one slow / hanging call must not hold back writing the others
        for i, fut in enumerate(as_completed([ex.submit(classify, r) for r in todo]), 1):
            res = fut.result()
            f.write(json.dumps(res, ensure_ascii=False) + "\n")
            f.flush()
            print(i, "/", len(todo), res["id"], "ok" if res["ok"] else res["error"], res["latency_s"], flush=True)


if __name__ == "__main__":
    main()
