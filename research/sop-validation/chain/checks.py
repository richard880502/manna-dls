"""Automatic checks for every step of the lesson-SOP chain (the Python twin of what would be added
to api/src/checks.rs). Each check returns a `Check`:

    name    stable identifier (reported per step in docs/LLM_VALIDATION.md)
    hard    True  -> a failure rejects the output (retry / do not show to the teacher)
            False -> quality signal only (logged, shown as a hint)
    n, ok   item-level counts, so rates can be reported per quotation / per code / per item
    detail  first few offending values

Schema validity itself is checked by `schema_check` (jsonschema) before these run.
"""
from __future__ import annotations

import re
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path

import jsonschema

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from labels import ANSWER_FORMATS, INFO_FUNCTIONS, SCAFFOLD_TYPES, STUDENT_TASKS, VERSIONS  # noqa: E402

CJK = re.compile(r"[㐀-鿿　-〿＀-￯]")


@dataclass
class Check:
    name: str
    hard: bool
    n: int
    ok: int
    detail: list = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return self.ok == self.n

    def to_dict(self):
        d = asdict(self)
        d["passed"] = self.passed
        d["detail"] = [str(x)[:160] for x in self.detail[:5]]
        return d


def each(name: str, hard: bool, items, pred) -> Check:
    """One check over many items; pred(item) -> True or an offending description."""
    items = list(items)
    bad = []
    for it in items:
        r = pred(it)
        if r is not True:
            bad.append(r if r not in (False, None) else it)
    return Check(name, hard, len(items), len(items) - len(bad), bad)


def one(name: str, hard: bool, cond: bool, detail="") -> Check:
    return Check(name, hard, 1, 1 if cond else 0, [] if cond else [detail])


# ------------------------------------------------------------------ text helpers
def norm(s: str) -> str:
    """Whitespace / typographic normalisation used for verbatim comparison (case is kept)."""
    s = s.replace("’", "'").replace("‘", "'").replace("“", '"').replace("”", '"')
    s = s.replace("—", "-").replace("–", "-").replace(" ", " ")
    return re.sub(r"\s+", " ", s).strip()


def is_verbatim(quote: str, source: str) -> bool:
    q = norm(quote).strip(" \"'")
    q = q.rstrip(".,;:!?")
    return len(q) >= 3 and q in norm(source)


def has_cjk(s: str) -> bool:
    return bool(CJK.search(s or ""))


def english_only(s: str) -> bool:
    return not has_cjk(s)


MARKUP = re.compile(r"\$|\\[a-zA-Z]+|[\r\x08\x0c]|ightarrow|\*\*")


def no_markup(s: str) -> bool:
    """LaTeX / Markdown leftovers such as "$\\rightarrow$" (which JSON-decodes to CR + "ightarrow$")."""
    return not MARKUP.search(s or "")


def words(s: str) -> int:
    return len(re.findall(r"[A-Za-z0-9'’-]+", s))


def schema_check(output, schema) -> Check:
    try:
        jsonschema.validate(output, schema)
        return one("schema_valid", True, True)
    except jsonschema.ValidationError as e:
        return one("schema_valid", True, False, f"{list(e.absolute_path)}: {e.message}")


# ------------------------------------------------------------------ (a) passage analysis
def check_analysis(out: dict, paragraphs: list[str]) -> list[Check]:
    n = len(paragraphs)
    nos = [p["paragraph_no"] for p in out["paragraphs"]]
    feats = [(p["paragraph_no"], f) for p in out["paragraphs"] for f in p["writing_features"]]

    def para(no):
        return paragraphs[no - 1] if 1 <= no <= n else ""

    return [
        one("paragraphs_one_to_one", True, nos == list(range(1, n + 1)), f"got {nos}, expected 1..{n}"),
        each("each_paragraph_has_feature", True, out["paragraphs"],
             lambda p: len(p["writing_features"]) >= 1 or f"P{p['paragraph_no']} has none"),
        each("evidence_verbatim", True, feats,
             lambda x: is_verbatim(x[1]["evidence"], para(x[0])) or f"P{x[0]}: {x[1]['evidence']}"),
        each("evidence_length_6_30_words", False, feats,
             lambda x: 6 <= words(x[1]["evidence"]) <= 30 or f"{words(x[1]['evidence'])}w: {x[1]['evidence']}"),
        each("focus_evidence_verbatim", True, out["language_focus"],
             lambda f: is_verbatim(f["evidence"], para(f["paragraph_no"])) or f"P{f['paragraph_no']}: {f['evidence']}"),
        one("language_focus_3_6", False, 3 <= len(out["language_focus"]) <= 6, str(len(out["language_focus"]))),
        each("zh_fields_in_chinese", True,
             [out["main_idea_zh"], out["structure_zh"]] + [p["function_zh"] for p in out["paragraphs"]]
             + [p["summary_zh"] for p in out["paragraphs"]] + [f["explanation_zh"] for _, f in feats],
             lambda s: has_cjk(s) or s),
        each("no_markup_artifacts", False, [out["main_idea_zh"], out["structure_zh"]] + [p["function_zh"] for p in out["paragraphs"]],
             lambda s: no_markup(s) or s),
    ]


# ------------------------------------------------------------------ (b) objectives
def _bigrams(s: str) -> set[str]:
    s = re.sub(r"[^一-鿿]", "", s)
    return {s[i:i + 2] for i in range(len(s) - 1)}


def check_objectives(out: dict, allowed: dict[str, str], n_paragraphs: int) -> list[Check]:
    """allowed: item_code -> curriculum text (exactly the items shown to the model)."""
    objs = out["objectives"]
    links = [(o["objective_id"], l) for o in objs for l in o["curriculum_links"]]
    types = {o["siop_type"] for o in objs}

    def copied(o):
        ob = _bigrams(o["objective_zh"])
        if not ob:
            return True
        worst = max((len(ob & _bigrams(t)) / len(ob) for t in allowed.values()), default=0)
        return worst < 0.8 or f"{o['objective_id']} overlaps a curriculum item {worst:.0%}"

    return [
        one("both_siop_types_present", True, types == {"內容目標", "語言目標"}, str(types)),
        one("objective_count_3_5", False, 3 <= len(objs) <= 5, str(len(objs))),
        each("cited_codes_exist", True, links, lambda x: x[1]["item_code"] in allowed or f"{x[0]}: {x[1]['item_code']}"),
        each("link_status_consistent", True, objs,
             lambda o: (o["link_status"] == "linked") == (len(o["curriculum_links"]) > 0) or o["objective_id"]),
        each("links_1_to_3", False, [o for o in objs if o["link_status"] == "linked"],
             lambda o: 1 <= len(o["curriculum_links"]) <= 3 or o["objective_id"]),
        each("objective_not_copied_from_curriculum", True, objs, copied),
        each("objective_zh_in_chinese", True, objs,
             lambda o: (has_cjk(o["objective_zh"]) and has_cjk(o["reason_zh"])) or o["objective_id"]),
        each("student_facing_english_i_can", True, objs,
             lambda o: (english_only(o["student_facing_en"]) and o["student_facing_en"].strip().lower().startswith("i can"))
             or o["student_facing_en"]),
        each("paragraph_refs_valid", True, objs,
             lambda o: all(1 <= r <= n_paragraphs for r in o["paragraph_refs"]) or f"{o['objective_id']}: {o['paragraph_refs']}"),
        one("objective_ids_sequential", True, [o["objective_id"] for o in objs] == [f"O{i}" for i in range(1, len(objs) + 1)],
            str([o["objective_id"] for o in objs])),
        each("no_markup_artifacts", False, [o["objective_zh"] for o in objs] + [o["student_facing_en"] for o in objs],
             lambda s: no_markup(s) or s),
    ]


# ------------------------------------------------------------------ (c) paragraph tasks
def _item_wellformed(it: dict):
    f, opts, ans = it["answer_format"], it["options"], it["answer_en"]
    if f == "選擇題":
        return (len(opts) == 4 and ans in opts and len(set(opts)) == 4) or f"{it['item_id']}: MCQ options/answer"
    if f == "是非題":
        return ans in ("True", "False") or f"{it['item_id']}: T/F answer={ans}"
    if f == "填空題":
        return "___" in it["prompt_en"] or f"{it['item_id']}: no blank"
    if f == "配對題":
        return len(opts) >= 3 or f"{it['item_id']}: matching needs options"
    return True


def check_tasks(out: dict, paragraphs: list[str], objective_ids: list[str], item_budget: int | None = None) -> list[Check]:
    blocks = out["blocks"]
    items = [it for b in blocks for it in b["items"]]
    n = len(paragraphs)
    passage = "\n".join(paragraphs)
    covered = {p for b in blocks for p in b["paragraph_nos"]}
    # a closing whole-text block (covers the first and the last paragraph) may follow the per-paragraph blocks
    seq = blocks[:-1] if blocks and blocks[-1]["paragraph_nos"] and {1, n} <= set(blocks[-1]["paragraph_nos"]) else blocks
    firsts = [min(b["paragraph_nos"]) if b["paragraph_nos"] else 0 for b in seq]
    funcs = [b["info_function"] for b in blocks]
    strings = [s for b in blocks for s in [b["title_en"], b["instructions_en"]]] + \
              [s for it in items for s in [it["prompt_en"], it["answer_en"], *it["options"]]]
    grounded = [it for it in items if it["evidence"] is not None]

    def leaks(it):
        a = norm(it["answer_en"]).lower()
        return not (words(a) >= 4 and a in norm(it["prompt_en"]).lower()) or f"{it['item_id']}: answer inside prompt"

    checks = [
        each("labels_from_spec_sets", True, blocks,
             lambda b: (b["info_function"] in INFO_FUNCTIONS and all(t in STUDENT_TASKS for t in b["student_tasks"])
                        and all(it["answer_format"] in ANSWER_FORMATS for it in b["items"])) or b["block_id"]),
        one("all_paragraphs_covered", True, covered == set(range(1, n + 1)), f"covered {sorted(covered)} of 1..{n}"),
        one("blocks_in_paragraph_order", True, firsts == sorted(firsts), str(firsts)),
        one("at_least_3_info_functions", True, len(set(funcs)) >= 3, str(funcs)),
        one("last_block_is_higher_order", False, bool(funcs) and funcs[-1] in ("分析資訊", "比較資訊", "創造資訊"), str(funcs[-1:])),
        each("evidence_verbatim", True, grounded, lambda it: is_verbatim(it["evidence"], passage) or f"{it['item_id']}: {it['evidence']}"),
        each("closed_items_have_evidence", False,
             [it for it in items if it["answer_format"] in ("選擇題", "是非題", "填空題", "配對題")],
             lambda it: it["evidence"] is not None or it["item_id"]),
        each("english_only", True, strings, lambda s: english_only(s) or s),
        each("items_wellformed", True, items, _item_wellformed),
        each("answer_not_in_prompt", True, items, leaks),
        each("objective_refs_valid", True, blocks,
             lambda b: (len(b["objective_refs"]) > 0 and all(r in objective_ids for r in b["objective_refs"])) or f"{b['block_id']}: {b['objective_refs']}"),
        one("ids_wellformed_unique", True,
            [b["block_id"] for b in blocks] == [f"B{i}" for i in range(1, len(blocks) + 1)]
            and len({it["item_id"] for it in items}) == len(items)
            and all(it["item_id"].startswith(b["block_id"] + "-") for b in blocks for it in b["items"]),
            "block/item ids"),
        each("student_tasks_not_empty", False, blocks, lambda b: len(b["student_tasks"]) > 0 or b["block_id"]),
        one("answer_format_variety_3", False, len({it["answer_format"] for it in items}) >= 3,
            str(sorted({it["answer_format"] for it in items}))),
    ]
    checks.append(each("no_markup_artifacts", False, strings, lambda s: no_markup(s) or s))
    if item_budget:
        checks.append(one("item_count_within_budget", False, 0.5 * item_budget <= len(items) <= 1.5 * item_budget,
                          f"{len(items)} items vs budget {item_budget}"))
    return checks


# ------------------------------------------------------------------ (d) warm-up
def check_warmup(out: dict, paragraphs: list[str], candidates: dict[str, int]) -> list[Check]:
    """candidates: base word -> CEEC level (from data). The model never outputs a level."""
    kv = out["key_vocabulary"]
    n = len(paragraphs)
    chosen = {v["word"] for v in kv}
    act = out["vocabulary_activity"]
    strings = [s for a in out["background_activities"] for s in [a["title_en"], a["instructions_en"], *a["prompts_en"]]] + \
              [v["student_friendly_definition_en"] for v in kv] + [act["title_en"], act["instructions_en"]] + \
              [s for it in act["items"] for s in [it["prompt_en"], it["answer_en"]]]

    def ctx_ok(v):
        p = paragraphs[v["paragraph_no"] - 1] if 1 <= v["paragraph_no"] <= n else ""
        stem = v["word"][: max(3, len(v["word"]) - 3)].lower()
        return (is_verbatim(v["context_sentence"], p) and stem in v["context_sentence"].lower()) or f"{v['word']} P{v['paragraph_no']}: {v['context_sentence']}"

    return [
        each("vocab_only_from_candidates", True, kv, lambda v: v["word"] in candidates or v["word"]),
        each("vocab_level_resolved_from_data", True, kv, lambda v: isinstance(candidates.get(v["word"]), int) or v["word"]),
        one("key_vocab_count_5_8", False, 5 <= len(kv) <= 8, str(len(kv))),
        each("context_sentence_verbatim", True, kv, ctx_ok),
        each("definition_simple_no_target_word", False, kv,
             lambda v: (v["word"].lower() not in v["student_friendly_definition_en"].lower()
                        and words(v["student_friendly_definition_en"]) <= 15) or f"{v['word']}: {v['student_friendly_definition_en']}"),
        each("english_only", True, strings, lambda s: english_only(s) or s),
        one("background_1_2_activities", True, 1 <= len(out["background_activities"]) <= 2, str(len(out["background_activities"]))),
        each("labels_from_spec_sets", True, out["background_activities"],
             lambda a: (all(t in STUDENT_TASKS for t in a["student_tasks"]) and a["answer_format"] in ANSWER_FORMATS) or a["title_en"]),
        each("activity_answers_are_key_words", True, act["items"],
             lambda it: any(w.lower() in it["answer_en"].lower() for w in chosen) or f"{it['answer_en']}"),
        one("activity_4_8_items", False, 4 <= len(act["items"]) <= 8, str(len(act["items"]))),
        each("zh_fields_in_chinese", True, [a["purpose_zh"] for a in out["background_activities"]] + [v["why_zh"] for v in kv],
             lambda s: has_cjk(s) or s),
    ]


# ------------------------------------------------------------------ (e) scaffolds
def check_scaffolds(out: dict, block_ids: list[str], task_answers: list[str]) -> list[Check]:
    sc = out["scaffolds"]
    targets = [s["target"] for s in sc]
    proc = [(s["target"], p) for s in sc for p in s["procedural"]]
    contents = [c["content_en"] for s in sc for c in s["verbal"] + s["instructional"]] + [st for _, p in proc for st in p["steps_en"]]
    long_answers = [norm(a).lower() for a in task_answers if words(a) >= 4]

    def leak(c):
        lc = norm(c).lower()
        return not any(a in lc for a in long_answers) or c

    return [
        one("targets_match_blocks_and_warmup", True, sorted(targets) == sorted(["warm_up"] + block_ids),
            f"got {targets}"),
        each("each_target_has_procedural", True, sc, lambda s: len(s["procedural"]) >= 1 or s["target"]),
        each("procedural_2_5_steps", False, proc, lambda x: 2 <= len(x[1]["steps_en"]) <= 5 or f"{x[0]}: {len(x[1]['steps_en'])}"),
        each("kinds_from_spec_sets", True, sc,
             lambda s: (all(v["kind"] in SCAFFOLD_TYPES["語言鷹架"] for v in s["verbal"])
                        and all(v["kind"] in SCAFFOLD_TYPES["教學鷹架"] for v in s["instructional"])
                        and all(v["kind"] in SCAFFOLD_TYPES["程序鷹架"] for v in s["procedural"])) or s["target"]),
        each("at_most_2_verbal_2_instructional", False, sc,
             lambda s: (len(s["verbal"]) <= 2 and len(s["instructional"]) <= 2) or s["target"]),
        each("english_only", True, contents, lambda c: english_only(c) or c),
        each("no_answer_leak", True, contents, leak),
        each("fading_notes_in_chinese", True, sc,
             lambda s: all(has_cjk(s["fading"][k]) for k in ("basic_zh", "standard_zh", "advanced_zh")) or s["target"]),
    ]


# ------------------------------------------------------------------ (f) exit ticket
def check_exit(out: dict, objective_ids: list[str], exit_minutes: int, task_prompts: list[str]) -> list[Check]:
    items = out["items"]
    formative = [i for i in items if i["purpose"] == "形成性評量"]
    prompts = {norm(p).lower() for p in task_prompts}
    return [
        one("item_count_2_4", True, 2 <= len(items) <= 4, str(len(items))),
        one("has_formative_item", True, len(formative) >= 1),
        one("has_reflection_or_question_item", True, any(i["purpose"] in ("學生反思", "蒐集問題") for i in items)),
        each("formative_linked_to_objective", True, formative,
             lambda i: (len(i["objective_refs"]) > 0 and all(r in objective_ids for r in i["objective_refs"])) or f"{i['item_id']}: {i['objective_refs']}"),
        each("formative_has_sample_answer", True, formative, lambda i: bool(i["sample_answer_en"]) or i["item_id"]),
        each("labels_from_spec_sets", True, items,
             lambda i: (i["answer_format"] in ANSWER_FORMATS and all(t in STUDENT_TASKS for t in i["student_tasks"])) or i["item_id"]),
        each("english_only", True, [i["prompt_en"] for i in items] + [i["sample_answer_en"] or "" for i in items],
             lambda s: english_only(s) or s),
        each("not_a_repeat_of_main_task", True, items, lambda i: norm(i["prompt_en"]).lower() not in prompts or i["item_id"]),
        one("fits_time", False, out["estimated_minutes"] <= exit_minutes + 2, str(out["estimated_minutes"])),
        one("ids_sequential", True, [i["item_id"] for i in items] == [f"E{k}" for k in range(1, len(items) + 1)],
            str([i["item_id"] for i in items])),
    ]


# ------------------------------------------------------------------ (g) worksheet
ANSWER_MARKERS = re.compile(r"\b(answer key|answers?\s*:|correct answer|\(correct\)|✓|✔)", re.I)


def student_strings(ws: dict) -> list[str]:
    out = [ws["title_en"]]
    for s in ws["student"]["sections"]:
        out += [s["heading_en"], s["instructions_en"], *s["supports_en"]]
        for it in s["items"]:
            out += [it["prompt_en"], *it["options"]]
    return out


def check_worksheet(ws: dict, version: str, block_ids: list[str], known_item_ids: set[str], passage: str,
                    show_objectives: bool) -> list[Check]:
    secs = ws["student"]["sections"]
    kinds = [s["kind"] for s in secs]
    main = [s for s in secs if s["kind"] == "main_task"]
    s_items = [(s, it) for s in secs for it in s["items"]]
    s_ids = [it["item_id"] for _, it in s_items]
    answers = {a["item_id"]: a for a in ws["teacher"]["answers"]}
    fmt = {it["item_id"]: it["answer_format"] for _, it in s_items}
    opts = {it["item_id"]: it["options"] for _, it in s_items}
    s_text = [norm(x).lower() for x in student_strings(ws)]
    # per-section student text without the options of closed items (options legitimately contain the key)
    def section_text_without_options(s):
        return norm(" ".join([s["heading_en"], s["instructions_en"], *s["supports_en"]] + [it["prompt_en"] for it in s["items"]])).lower()
    sec_of = {it["item_id"]: section_text_without_options(s) for s, it in s_items}

    def leaked(a):
        ans = norm(a["answer_en"]).lower().rstrip(".")
        iid = a["item_id"]
        if iid not in fmt or fmt[iid] in ("是非題",):
            return True
        if fmt[iid] in ("選擇題", "配對題") and words(ans) < 4:
            return True
        if words(ans) >= 4 or (fmt[iid] == "填空題" and len(ans) >= 5):
            return ans not in sec_of.get(iid, "") or f"{iid}: '{a['answer_en']}' appears in student section"
        return True

    first_main = kinds.index("main_task") if "main_task" in kinds else len(kinds)
    order_ok = (kinds.count("warm_up") == 1 and kinds.count("exit_ticket") == 1
                and kinds.index("warm_up") < first_main and kinds[-1] == "exit_ticket"
                and all(k != "main_task" for k in kinds[: kinds.index("warm_up")]))
    if show_objectives:
        order_ok = order_ok and kinds[0] == "learning_objectives"

    def mcq_ok(x):
        sec, it = x
        if it["answer_format"] != "選擇題":
            return True
        a = answers.get(it["item_id"])
        if sec["kind"] != "main_task":      # warm-up polls / exit-ticket self-ratings have no single key
            return len(it["options"]) >= 2 or f"{it['item_id']}: fewer than 2 options"
        return (a is not None and len(it["options"]) == 4 and a["answer_en"] in it["options"]) or f"{it['item_id']}"

    return [
        one("version_matches_request", True, ws["version"] == version and version in VERSIONS, ws["version"]),
        one("section_order_warmup_first_exit_last", True, order_ok, str(kinds)),
        one("same_core_blocks_in_order", True, [s["source_id"] for s in main] == block_ids,
            f"{[s['source_id'] for s in main]} vs {block_ids}"),
        each("student_version_english_only", True, student_strings(ws), lambda s: english_only(s) or s),
        each("no_answer_text_in_student_version", True, ws["teacher"]["answers"], leaked),
        each("no_answer_markers_in_student_version", True, s_text, lambda s: not ANSWER_MARKERS.search(s) or s),
        each("every_student_item_has_teacher_answer", True, s_ids, lambda i: i in answers or i),
        each("no_orphan_teacher_answers", False, list(answers), lambda i: i in s_ids or i),
        one("student_item_ids_unique", True, len(set(s_ids)) == len(s_ids), "duplicate item ids"),
        each("main_item_ids_from_design", True,
             [it["item_id"] for s in main for it in s["items"]],
             lambda i: i in known_item_ids or (version == "進階版" and i.endswith("-X")) or i),
        each("mcq_wellformed_and_keyed", True, s_items, mcq_ok),
        each("teacher_evidence_verbatim", True, [a for a in ws["teacher"]["answers"] if a["evidence"]],
             lambda a: is_verbatim(a["evidence"], passage) or f"{a['item_id']}: {a['evidence']}"),
        one("teacher_notes_in_chinese", True, has_cjk(ws["teacher"]["teaching_notes_zh"]) and has_cjk(ws["teacher"]["version_note_zh"])),
        each("main_sections_not_empty", True, main, lambda s: len(s["items"]) >= 1 or s["source_id"]),
        each("no_markup_artifacts", False, student_strings(ws), lambda s: no_markup(s) or s),
    ]


def check_versions(sheets: dict[str, dict]) -> list[Check]:
    """Cross-version check (spec §5.4): same objectives and core tasks; support decreases."""
    def supports(ws):
        return sum(len(s["supports_en"]) for s in ws["student"]["sections"])

    def blocks(ws):
        return [s["source_id"] for s in ws["student"]["sections"] if s["kind"] == "main_task"]

    have = [v for v in VERSIONS if v in sheets]
    sup = [supports(sheets[v]) for v in have]
    bl = [blocks(sheets[v]) for v in have]
    texts = [" ".join(it["prompt_en"] for s in sheets[v]["student"]["sections"] for it in s["items"]) + " ".join(
        x for s in sheets[v]["student"]["sections"] for x in s["supports_en"]) for v in have]
    return [
        one("versions_share_core_blocks", True, all(b == bl[0] for b in bl), str(bl)),
        one("support_decreases_basic_to_advanced", True, sup == sorted(sup, reverse=True) and (len(sup) < 2 or sup[0] > sup[-1]),
            f"supports per version {dict(zip(have, sup))}"),
        one("versions_actually_differ", True, len(set(texts)) == len(texts), "two versions are identical"),
    ]
