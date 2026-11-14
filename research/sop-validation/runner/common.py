"""Shared helpers for the chain runner: prompt loading/rendering, passages, curriculum items,
vocabulary candidates and the compact summaries passed between steps."""
from __future__ import annotations

import csv
import hashlib
import json
import os
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "vocab"))
from analyze import Vocab, analyze  # noqa: E402

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))


def load_prompt(key: str, variant_dir: str = "prompts") -> tuple[str, str]:
    text = (ROOT / variant_dir / f"{key}.md").read_text()
    system, user = text.split("=====USER=====\n", 1)
    return system.strip(), user.strip()


def render(template: str, vars: dict[str, str]) -> str:
    """Same semantics as api/src/prompts.rs::render: unknown placeholders stay visible, substituted
    values are not re-scanned."""
    def sub(m):
        name = m.group(1).strip()
        return vars[name] if name in vars else m.group(0)
    return re.sub(r"\{\{(.*?)\}\}", sub, template)


# ------------------------------------------------------------------ passages
def load_passages() -> list[dict]:
    """passages/manifest.json -> [{id, paragraphs, ...}]. Worksheet-derived passages are read from
    the local extraction (never stored in the repo); the SHA-256 prefix guards against a different
    file set producing different W### ids."""
    manifest = json.loads((ROOT / "passages" / "manifest.json").read_text())
    files = {}
    fcsv = DATA / "worksheets" / "files.csv"
    if fcsv.exists():
        files = {r["id"]: r for r in csv.DictReader(open(fcsv, encoding="utf-8"))}
    out = []
    for p in manifest:
        if p["source"] == "original":
            paras = [x.strip() for x in (ROOT / p["file"]).read_text().split("\n\n") if x.strip()]
        else:
            row = files.get(p["worksheet_id"])
            if not row or not row["sha256"].startswith(p["sha256_prefix"]):
                print(f"skip {p['id']}: worksheet {p['worksheet_id']} not available locally", file=sys.stderr)
                continue
            lines = (DATA / "worksheets" / "text" / f"{p['worksheet_id']}.txt").read_text().split("\n")
            paras = [lines[i].strip() for i in p["lines"]]
        out.append({**p, "paragraphs": paras, "words": sum(len(x.split()) for x in paras)})
    return out


def numbered(paragraphs: list[str]) -> str:
    return "\n\n".join(f"[P{i}] {p}" for i, p in enumerate(paragraphs, 1))


# ------------------------------------------------------------------ curriculum items (KB)
def curriculum_items(track: str = "general") -> dict[str, dict]:
    """item_code -> {title, content} for the items the model may cite (senior-high stage of the track)."""
    path = DATA / "kb" / "knowledge_import.csv"
    items = {}
    for r in csv.DictReader(open(path, encoding="utf-8")):
        if r["school_track"] == track and r["item_code"].startswith("108-"):
            items[r["item_code"]] = {"title": r["title"], "content": r["content"], "page": r["source_page"],
                                     "source_doc": r["source_doc"], "source_version": r["source_version"]}
    return items


def format_curriculum(items: dict[str, dict]) -> str:
    return "\n".join(f"{c}｜{v['title'].split(' ', 1)[1] if ' ' in v['title'] else v['title']}｜{v['content']}" for c, v in items.items())


# ------------------------------------------------------------------ vocabulary candidates (data, not model)
_VOCAB = None


def vocab() -> Vocab:
    global _VOCAB
    if _VOCAB is None:
        _VOCAB = Vocab(DATA / "kb" / "ceec_vocab.csv")
    return _VOCAB


def vocab_candidates(paragraphs: list[str], min_level: int, limit: int = 45) -> list[dict]:
    """Words of the passage at or above `min_level` in the CEEC list, with level and paragraphs."""
    seen: dict[str, dict] = {}
    for i, p in enumerate(paragraphs, 1):
        res = analyze(p, vocab(), target=min_level - 1)
        for x in res["above_target"]:
            if any(pos["match"].startswith("derived") for pos in x["positions"]):
                continue
            d = seen.setdefault(x["base"], {"word": x["base"], "level": x["level"], "paragraphs": [], "forms": set()})
            d["paragraphs"].append(i)
            d["forms"].update(pos["form"] for pos in x["positions"])
    cands = sorted(seen.values(), key=lambda d: (-d["level"], d["paragraphs"][0]))[:limit]
    return sorted(cands, key=lambda d: d["paragraphs"][0])


def format_candidates(cands: list[dict]) -> str:
    return "\n".join(f"{c['word']}｜第{c['level']}級｜" + "、".join(f"P{p}" for p in sorted(set(c["paragraphs"]))) for c in cands)


# ------------------------------------------------------------------ summaries between steps
def analysis_summary(a: dict) -> str:
    lines = [f"文體：{a['text_type']}", f"主旨：{a['main_idea_zh']}", f"結構：{a['structure_zh']}"]
    for p in a["paragraphs"]:
        feats = "；".join(f["feature_zh"] for f in p["writing_features"])
        lines.append(f"P{p['paragraph_no']}　功能：{p['function_zh']}　摘要：{p['summary_zh']}　寫作特色：{feats}")
    lines.append("語言重點：" + "；".join(f"{f['kind']}「{f['item']}」(P{f['paragraph_no']})" for f in a["language_focus"]))
    return "\n".join(lines)


def objectives_summary(o: dict) -> str:
    return "\n".join(f"{x['objective_id']}［{x['siop_type']}］{x['objective_zh']}（{x['student_facing_en']}）" for x in o["objectives"])


def objectives_detail(o: dict) -> str:
    return json.dumps([{k: x[k] for k in ("objective_id", "siop_type", "student_facing_en")} for x in o["objectives"]], ensure_ascii=False)


def tasks_summary(t: dict) -> str:
    out = []
    for b in t["blocks"]:
        fm = Counter(it["answer_format"] for it in b["items"])
        out.append(f"{b['block_id']}（P{'、P'.join(map(str, b['paragraph_nos']))}）{b['info_function']}｜{b['title_en']}｜"
                   + "、".join(f"{k}×{v}" for k, v in fm.items()))
        out += [f"  - {it['item_id']} {it['prompt_en']}" for it in b["items"]]
    return "\n".join(out)


def tasks_detail(t: dict) -> str:
    slim = [{"block_id": b["block_id"], "paragraph_nos": b["paragraph_nos"], "info_function": b["info_function"],
             "title_en": b["title_en"], "instructions_en": b["instructions_en"],
             "items": [{k: it[k] for k in ("item_id", "answer_format", "prompt_en", "options", "answer_en")} for it in b["items"]]}
            for b in t["blocks"]]
    return json.dumps(slim, ensure_ascii=False, indent=1)


def warmup_summary(w: dict) -> str:
    return json.dumps({"background_activities": [{k: a[k] for k in ("title_en", "instructions_en", "prompts_en")} for a in w["background_activities"]],
                       "key_vocabulary": [v["word"] for v in w["key_vocabulary"]],
                       "vocabulary_activity": w["vocabulary_activity"]["title_en"]}, ensure_ascii=False)


def sha(s: str) -> str:
    return hashlib.sha256(s.encode()).hexdigest()[:12]
