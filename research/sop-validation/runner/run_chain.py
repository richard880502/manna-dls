"""Run the lesson-SOP prompt chain (a)-(g) on the validation passages and record, per step,
automatic-check results, latency and token usage.

  python runner/run_chain.py --run r1                        # zero-shot, all passages
  python runner/run_chain.py --run r1-rules --variant rules --reuse r1 --passages P2 P4
        # (c)/(e)/(g) regenerated WITH design rules + few-shot; (a)(b)(d)(f) copied from run r1

Outputs: $MANNA_DATA/runs/chain/<run>/<passage>/<step>.json and results.jsonl (one line per step).
Every step gets one automatic repair attempt: if a hard check fails, the failing checks are fed
back and the model is asked again (attempt 2). pass@1 and pass@2 are both recorded.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / "chain"))
sys.path.insert(0, str(ROOT / "runner"))

import checks as C  # noqa: E402
import schemas as SC  # noqa: E402
from common import (DATA, analysis_summary, curriculum_items, format_candidates, format_curriculum,  # noqa: E402
                    load_passages, load_prompt, numbered, objectives_detail, objectives_summary, render,
                    tasks_detail, tasks_summary, vocab_candidates, warmup_summary)
from labels import ANSWER_FORMATS, SCAFFOLD_TYPES, STUDENT_TASKS, VERSIONS  # noqa: E402
from llm import chat  # noqa: E402

REPLAY: dict[str, dict] | None = None     # tag -> logged call (set by --rescore)
MINUTES = 50
EXIT_MINUTES = 5
ITEM_BUDGET = 12
SETTINGS = ["加入暖身活動", "顯示學習目標", "提供引導問題", "顯示作答示例", "提供字詞表", "提供句型框架",
            "產生基礎版／一般版／進階版（三版皆選）"]
NONE_SELECTED = "（教師未勾選，請依教材與目標判斷）"


def run_step(run_dir: Path, pid: str, step: str, key: str, vars: dict, check_fn, log: str, max_attempts: int = 2,
             schema_key: str | None = None) -> dict:
    """Call the model, validate, optionally repair once. Returns the stored record."""
    system, user_t = load_prompt(key)
    system = render(system, vars)
    user = render(user_t, vars)
    schema = SC.ALL[schema_key or key]
    attempts = []
    output = None
    prior = None
    for attempt in range(1, max_attempts + 1):
        tag = f"{run_dir.name}:{pid}:{step}:a{attempt}"
        if REPLAY is not None:
            if tag not in REPLAY:
                # stricter checks than at run time and no logged repair attempt: counts as not repaired
                attempts.append({"attempt": attempt, "api_ok": False, "error": "no logged repair attempt", "latency_s": 0,
                                 "prompt_tokens": None, "completion_tokens": None, "api_attempts": 0, "checks": [],
                                 "hard_pass": False, "soft_fail": []})
                break
            r = REPLAY[tag]
        else:
            r = chat(system, user, schema, key, tag=tag, log=log, temperature=0.2, prior=prior)
        rec = {"attempt": attempt, "api_ok": r["ok"], "error": r["error"], "latency_s": r["latency_s"],
               "prompt_tokens": r["usage"].get("prompt_tokens"), "completion_tokens": r["usage"].get("completion_tokens"),
               "api_attempts": r["attempts"]}
        if r["ok"]:
            sch = C.schema_check(r["json"], schema)
            results = [sch] + (check_fn(r["json"]) if sch.passed else [])
            rec["checks"] = [c.to_dict() for c in results]
            rec["hard_pass"] = all(c.passed for c in results if c.hard)
            rec["soft_fail"] = [c.name for c in results if not c.hard and not c.passed]
            output = r["json"]
        else:
            rec["checks"], rec["hard_pass"], rec["soft_fail"] = [], False, []
        attempts.append(rec)
        if rec["hard_pass"]:
            break
        failed = [c for c in rec["checks"] if c["hard"] and not c["passed"]]
        feedback = "\n".join(f"- {c['name']}：{'; '.join(c['detail'])}" for c in failed) or f"- {rec['error']}"
        if output is None:
            continue                                   # API / JSON failure: plain retry
        prior = [{"role": "assistant", "content": json.dumps(output, ensure_ascii=False)},
                 {"role": "user", "content": "你上一次的輸出未通過下列自動檢查。請只修正這些問題，其餘內容維持不變，"
                                             "重新輸出完整的 JSON：\n" + feedback}]
    if REPLAY is not None:
        # keep the output that downstream steps actually saw at run time (= last logged attempt)
        logged = [REPLAY[t] for t in (f"{run_dir.name}:{pid}:{step}:a{k}" for k in (2, 1)) if t in REPLAY and REPLAY[t]["json"] is not None]
        output = logged[0]["json"] if logged else output
    record = {"run": run_dir.name, "passage": pid, "step": step, "key": key, "attempts": attempts,
              "pass_at_1": attempts[0]["hard_pass"], "pass_final": attempts[-1]["hard_pass"], "output": output}
    d = run_dir / pid
    d.mkdir(parents=True, exist_ok=True)
    (d / f"{step}.json").write_text(json.dumps(record, ensure_ascii=False, indent=1))
    return record


def load_step(run_dir: Path, pid: str, step: str):
    p = run_dir / pid / f"{step}.json"
    return json.loads(p.read_text()) if p.exists() else None


def chain(p: dict, run_dir: Path, variant: str, reuse_dir: Path | None, steps: set[str]) -> None:
    pid, paras = p["id"], p["paragraphs"]
    log = f"chain-{run_dir.name}.jsonl"
    base = {
        "school_track": "普通型高中", "grade": p["grade"], "student_info": p["student_info"], "minutes": str(MINUTES),
        "passage_numbered": numbered(paras),
        "student_task_labels": "、".join(STUDENT_TASKS), "answer_format_labels": "、".join(ANSWER_FORMATS),
        "design_rules": "", "few_shot": "",
    }
    if variant == "rules":
        rules = (ROOT / "rules" / "design_rules_for_prompt.md").read_text().strip()
        base["design_rules"] = "\n教師學習單設計規則（由教師自製學習單歸納，請遵循）：\n" + rules
        fs = DATA / "worksheets" / "fewshot_for_prompt.txt"
        if fs.exists():
            base["few_shot"] = "\n代表性範例（教師自製學習單的活動骨架，僅供參考設計方式；不可照抄內容，題目必須依本教材重新設計）：\n" + fs.read_text().strip()

    def get(step, fn):
        """Run `fn` unless the step is reused from another run or not requested."""
        if reuse_dir is not None and step in ("a", "b", "d", "f"):
            rec = load_step(reuse_dir, pid, step)
            if rec:
                (run_dir / pid).mkdir(parents=True, exist_ok=True)
                (run_dir / pid / f"{step}.json").write_text(json.dumps({**rec, "reused_from": reuse_dir.name}, ensure_ascii=False, indent=1))
                return rec
        if step[0] not in steps:
            return load_step(run_dir, pid, step)
        return fn()

    t0 = time.time()
    # (a)
    ra = get("a", lambda: run_step(run_dir, pid, "a", "sop_passage_analysis", base, lambda o: C.check_analysis(o, paras), log))
    if not ra or ra["output"] is None:
        return
    a = ra["output"]
    # (b)
    items = curriculum_items("general")
    allowed = {c: v["content"] for c, v in items.items()}
    vb = {**base, "analysis_summary": analysis_summary(a), "curriculum_items": format_curriculum(items)}
    rb = get("b", lambda: run_step(run_dir, pid, "b", "sop_objectives", vb, lambda o: C.check_objectives(o, allowed, len(paras)), log))
    if not rb or rb["output"] is None:
        return
    o = rb["output"]
    oids = [x["objective_id"] for x in o["objectives"]]
    # (c)
    vc = {**base, "analysis_summary": analysis_summary(a), "objectives_summary": objectives_summary(o),
          "item_budget": str(ITEM_BUDGET), "selected_student_tasks": NONE_SELECTED, "selected_answer_formats": NONE_SELECTED}
    rc = get("c", lambda: run_step(run_dir, pid, "c", "sop_paragraph_tasks", vc, lambda t: C.check_tasks(t, paras, oids, ITEM_BUDGET), log))
    if not rc or rc["output"] is None:
        return
    t = rc["output"]
    block_ids = [b["block_id"] for b in t["blocks"]]
    # (d) vocabulary candidates + levels come from the CEEC data, never from the model
    cands = vocab_candidates(paras, p["vocab_min_level"])
    cand_levels = {c["word"]: c["level"] for c in cands}
    vd = {**base, "objectives_summary": objectives_summary(o), "tasks_summary": tasks_summary(t), "vocab_candidates": format_candidates(cands)}
    rd = get("d", lambda: run_step(run_dir, pid, "d", "sop_warmup", vd, lambda w: C.check_warmup(w, paras, cand_levels), log))
    if not rd or rd["output"] is None:
        return
    w = rd["output"]
    for v in w["key_vocabulary"]:                      # join the level from data for downstream display
        v["ceec_level"] = cand_levels.get(v["word"])
    # (e)
    ve = {**base, "objectives_summary": objectives_summary(o), "warmup_summary": warmup_summary(w), "tasks_detail": tasks_detail(t),
          "verbal_kinds": "、".join(SCAFFOLD_TYPES["語言鷹架"]), "instructional_kinds": "、".join(SCAFFOLD_TYPES["教學鷹架"]),
          "procedural_kinds": "、".join(SCAFFOLD_TYPES["程序鷹架"])}
    answers = [it["answer_en"] for b in t["blocks"] for it in b["items"]]
    re_ = get("e", lambda: run_step(run_dir, pid, "e", "sop_scaffolds", ve, lambda s: C.check_scaffolds(s, block_ids, answers), log))
    # (f)
    vf = {**base, "objectives_summary": objectives_summary(o), "tasks_summary": tasks_summary(t), "exit_minutes": str(EXIT_MINUTES)}
    prompts = [it["prompt_en"] for b in t["blocks"] for it in b["items"]]
    rf = get("f", lambda: run_step(run_dir, pid, "f", "sop_exit_ticket", vf, lambda x: C.check_exit(x, oids, EXIT_MINUTES, prompts), log))
    if not re_ or re_["output"] is None or not rf or rf["output"] is None:
        return
    # (g) one call per version
    known = {it["item_id"] for b in t["blocks"] for it in b["items"]}
    passage = "\n".join(paras)
    sheets = {}
    if "g" in steps:
        for ver in VERSIONS:
            vg = {**base, "version": ver, "generation_settings": "、".join(SETTINGS),
                  "objectives_section_rule": "learning_objectives（顯示學習目標，放在最前面）→ ",
                  "objectives_detail": objectives_detail(o), "warmup_json": json.dumps(w, ensure_ascii=False),
                  "tasks_json": json.dumps(t, ensure_ascii=False), "scaffolds_json": json.dumps(re_["output"], ensure_ascii=False),
                  "exit_json": json.dumps(rf["output"], ensure_ascii=False)}
            rg = run_step(run_dir, pid, f"g-{ver}", "sop_worksheet", vg,
                          lambda s, ver=ver: C.check_worksheet(s, ver, block_ids, known, passage, True), log)
            if rg["output"] is not None:
                sheets[ver] = rg["output"]
        if sheets:
            cv = C.check_versions(sheets)
            (run_dir / pid / "g-versions.json").write_text(json.dumps(
                {"run": run_dir.name, "passage": pid, "step": "g-versions", "checks": [c.to_dict() for c in cv],
                 "pass_final": all(c.passed for c in cv if c.hard), "versions": list(sheets)}, ensure_ascii=False, indent=1))
    print(f"{pid} done in {time.time() - t0:.0f}s", flush=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--variant", choices=["zero", "rules"], default="zero")
    ap.add_argument("--reuse", help="run id to copy steps a,b,d,f from")
    ap.add_argument("--passages", nargs="*")
    ap.add_argument("--steps", default="abcdefg")
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--rescore", action="store_true",
                    help="no API calls: recompute all checks from the call log of this run (after changing checks.py)")
    a = ap.parse_args()
    if a.rescore:
        global REPLAY
        REPLAY = {}
        for line in open(DATA / "runs" / f"chain-{a.run}.jsonl"):
            c = json.loads(line)
            REPLAY[c["tag"]] = c       # later lines (API-level retries) overwrite earlier ones
    run_dir = DATA / "runs" / "chain" / a.run
    run_dir.mkdir(parents=True, exist_ok=True)
    if not a.rescore:
        (run_dir / "config.json").write_text(json.dumps(vars(a), ensure_ascii=False))
    else:
        cfg = json.loads((run_dir / "config.json").read_text())
        a.variant, a.reuse = cfg.get("variant", a.variant), cfg.get("reuse", a.reuse)
    ps = [p for p in load_passages() if not a.passages or p["id"] in a.passages]
    reuse = DATA / "runs" / "chain" / a.reuse if a.reuse else None
    with ThreadPoolExecutor(a.workers) as ex:
        list(ex.map(lambda p: chain(p, run_dir, a.variant, reuse, set(a.steps)), ps))


if __name__ == "__main__":
    main()
