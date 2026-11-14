"""Flatten a generated worksheet (step g output) into plain text: student version by default,
teacher version with --teacher. Used to feed vocab/analyze.py and for eyeballing results.

  python runner/worksheet_text.py <run> <passage> <基礎版|一般版|進階版> [--teacher] > sheet.txt
"""
import json
import os
import sys
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def flatten(ws: dict, teacher: bool = False) -> str:
    out = [ws["title_en"], ""]
    for s in ws["student"]["sections"]:
        out += [s["heading_en"], s["instructions_en"]]
        out += s["supports_en"]
        for it in s["items"]:
            out.append(f"{it['item_id']}. {it['prompt_en']}")
            out += [f"   - {o}" for o in it["options"]]
        out.append("")
    if teacher:
        out.append("ANSWER KEY (teacher version only)")
        for a in ws["teacher"]["answers"]:
            out.append(f"{a['item_id']}: {a['answer_en']}" + (f"  [evidence: {a['evidence']}]" if a["evidence"] else ""))
        out += ["", ws["teacher"]["teaching_notes_zh"], ws["teacher"]["version_note_zh"]]
    return "\n".join(out)


if __name__ == "__main__":
    run, pid, ver = sys.argv[1:4]
    rec = json.loads((DATA / "runs" / "chain" / run / pid / f"g-{ver}.json").read_text())
    print(flatten(rec["output"], "--teacher" in sys.argv))
