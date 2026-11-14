"""JSON schemas for the lesson-SOP prompt chain.

Same rules as api/src/ai/schemas.rs: every object closes its properties (additionalProperties:false)
and lists all of them in `required`; no minLength/minItems/numeric constraints (those are enforced
by the check functions instead). Enums come from labels.py so the spec labels stay in one place.

`python schemas.py` writes schemas/<key>.json for use outside Python.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from labels import ANSWER_FORMATS, INFO_FUNCTIONS, SCAFFOLD_TYPES, SIOP_OBJECTIVE_TYPES, STUDENT_TASKS  # noqa: E402

S = {"type": "string"}
I = {"type": "integer"}
B = {"type": "boolean"}


def enum(values):
    return {"type": "string", "enum": list(values)}


def arr(items):
    return {"type": "array", "items": items}


def nullable(s):
    return {"anyOf": [s, {"type": "null"}]}


def obj(**props):
    return {"type": "object", "properties": props, "required": list(props), "additionalProperties": False}


TEXT_TYPES = ["記敘文", "描寫文", "說明文", "論說文", "討論文", "其他"]

# (a) 逐段教材分析
PASSAGE_ANALYSIS = obj(
    text_type=enum(TEXT_TYPES),
    main_idea_zh=S,
    structure_zh=S,
    paragraphs=arr(obj(
        paragraph_no=I,
        function_zh=S,
        summary_zh=S,
        writing_features=arr(obj(feature_zh=S, evidence=S, explanation_zh=S)),
    )),
    language_focus=arr(obj(
        kind=enum(["字彙", "句型", "篇章標記"]),
        item=S,
        paragraph_no=I,
        evidence=S,
        note_zh=S,
    )),
)

# (b) SIOP 內容目標／語言目標 + 108 課綱連結
OBJECTIVES = obj(
    objectives=arr(obj(
        objective_id=S,
        siop_type=enum(SIOP_OBJECTIVE_TYPES),
        objective_zh=S,
        student_facing_en=S,
        reason_zh=S,
        paragraph_refs=arr(I),
        link_status=enum(["linked", "unlinked"]),
        curriculum_links=arr(obj(item_code=S, why_zh=S)),
    )),
)

# (c) 逐段問題／任務
PARAGRAPH_TASKS = obj(
    blocks=arr(obj(
        block_id=S,
        paragraph_nos=arr(I),
        title_en=S,
        info_function=enum(INFO_FUNCTIONS),
        student_tasks=arr(enum(STUDENT_TASKS)),
        objective_refs=arr(S),
        instructions_en=S,
        items=arr(obj(
            item_id=S,
            answer_format=enum(ANSWER_FORMATS),
            prompt_en=S,
            options=arr(S),
            answer_en=S,
            evidence=nullable(S),
        )),
        design_reason_zh=S,
    )),
)

# (d) 暖身：背景知識 + 關鍵字彙（級別由程式從詞彙表帶入，模型不輸出級別）
WARMUP = obj(
    background_activities=arr(obj(
        title_en=S,
        instructions_en=S,
        prompts_en=arr(S),
        student_tasks=arr(enum(STUDENT_TASKS)),
        answer_format=enum(ANSWER_FORMATS),
        purpose_zh=S,
    )),
    key_vocabulary=arr(obj(
        word=S,
        paragraph_no=I,
        context_sentence=S,
        student_friendly_definition_en=S,
        why_zh=S,
    )),
    vocabulary_activity=obj(
        title_en=S,
        instructions_en=S,
        answer_format=enum(ANSWER_FORMATS),
        items=arr(obj(prompt_en=S, answer_en=S)),
    ),
)

# (e) 三種鷹架
SCAFFOLDS = obj(
    scaffolds=arr(obj(
        target=S,                                   # block_id from (c) or "warm_up"
        verbal=arr(obj(kind=enum(SCAFFOLD_TYPES["語言鷹架"]), content_en=S)),
        instructional=arr(obj(kind=enum(SCAFFOLD_TYPES["教學鷹架"]), content_en=S)),
        procedural=arr(obj(kind=enum(SCAFFOLD_TYPES["程序鷹架"]), steps_en=arr(S))),
        fading=obj(basic_zh=S, standard_zh=S, advanced_zh=S),
    )),
)

# (f) Exit ticket
EXIT_TICKET = obj(
    estimated_minutes=I,
    items=arr(obj(
        item_id=S,
        purpose=enum(["形成性評量", "學生反思", "蒐集問題"]),
        prompt_en=S,
        answer_format=enum(ANSWER_FORMATS),
        student_tasks=arr(enum(STUDENT_TASKS)),
        objective_refs=arr(S),
        sample_answer_en=nullable(S),
    )),
)

# (g) 學習單組裝（每個版本一次呼叫）。學生版 schema 沒有任何答案欄位；答案只在 teacher。
SECTION_KINDS = ["learning_objectives", "warm_up", "main_task", "exit_ticket"]
WORKSHEET = obj(
    version=enum(["基礎版", "一般版", "進階版"]),
    title_en=S,
    student=obj(
        sections=arr(obj(
            kind=enum(SECTION_KINDS),
            source_id=nullable(S),                  # block_id of (c) for main_task
            heading_en=S,
            instructions_en=S,
            supports_en=arr(S),                     # word bank / sentence frames / worked example / steps
            items=arr(obj(
                item_id=S,
                answer_format=enum(ANSWER_FORMATS),
                prompt_en=S,
                options=arr(S),
            )),
        )),
    ),
    teacher=obj(
        answers=arr(obj(item_id=S, answer_en=S, evidence=nullable(S))),
        teaching_notes_zh=S,
        version_note_zh=S,
    ),
)

ALL = {
    "sop_passage_analysis": PASSAGE_ANALYSIS,
    "sop_objectives": OBJECTIVES,
    "sop_paragraph_tasks": PARAGRAPH_TASKS,
    "sop_warmup": WARMUP,
    "sop_scaffolds": SCAFFOLDS,
    "sop_exit_ticket": EXIT_TICKET,
    "sop_worksheet": WORKSHEET,
}


def assert_strict(v, path="$"):
    """Mirror of api/src/ai/schemas.rs::assert_strict."""
    if isinstance(v, dict):
        if v.get("type") == "object":
            assert v.get("additionalProperties") is False, path
            assert set(v["properties"]) == set(v["required"]), path
        for bad in ("minLength", "maxLength", "minimum", "maximum", "minItems"):
            assert bad not in v, f"{path} uses {bad}"
        for k, c in v.items():
            assert_strict(c, f"{path}.{k}")
    elif isinstance(v, list):
        for c in v:
            assert_strict(c, path)


if __name__ == "__main__":
    out = Path(__file__).resolve().parent.parent / "schemas"
    out.mkdir(exist_ok=True)
    for key, schema in ALL.items():
        assert_strict(schema, key)
        (out / f"{key}.json").write_text(json.dumps(schema, ensure_ascii=False, indent=2) + "\n")
        print("wrote", key)
