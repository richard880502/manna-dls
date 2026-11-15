"""Template-extraction probe: worksheet text -> teacher template -> new-lesson worksheet.
Run on manna with OPENAI_COMPAT_API_KEY in env (never printed). Writes results/ next to this file."""
import json, os, sys, time, urllib.request, collections

BASE = "https://portal.genai.nchc.org.tw/api/v1"
MODEL = "gemma-4-31B-it"
KEY = os.environ["OPENAI_COMPAT_API_KEY"]
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "results"); os.makedirs(OUT, exist_ok=True)

STUDENT_TASKS = ["理解內容","字彙練習","文法／句型練習","比較／分類","推論／預測","評估／提出理由","蒐集資料","整理／分析資料","討論／分享","小組共同完成","寫作","製作成果","成果發表","學習反思","自我評量","同儕互評"]
ANSWER_FORMATS = ["填空題","選擇題","是非題","配對題","簡答題","表格整理","圖像組織","段落寫作"]
GENERATION = ["加入暖身活動","顯示學習目標","提供引導問題","顯示作答示例","提供字詞表","提供句型框架"]
SCAFFOLDS = ["改述或重述","思考放聲","提示與追問","透過上下文解釋詞義","引導問題","字詞表","句型框架","作答示例","圖像組織","部分完成的大綱或內容","調整文本","標示關鍵概念或字彙","清楚的分步指示及完成順序","個別或小組指導安排","策略性配對或分組","逐步減少支持"]

def arr(enum=None, item=None):
    return {"type": "array", "items": item or ({"type": "string", "enum": enum} if enum else {"type": "string"})}
def obj(props):  # keep written order: the gateway emits fields in schema order
    return {"type": "object", "properties": props, "required": list(props), "additionalProperties": False}
S = {"type": "string"}; I = {"type": "integer"}; B = {"type": "boolean"}

TEMPLATE_SCHEMA = obj({
    "worksheet_kind": S,
    "instruction_language": {"type": "string", "enum": ["英文", "中文", "中英並用"]},
    "header_fields": arr(),
    "shows_objectives": B,
    "includes_passage_text": B,
    "outline": arr(),
    "sections": arr(item=obj({
        "title_pattern": S,
        "purpose": S,
        "block_kind": {"type": "string", "enum": ["warmup", "activity", "exit_ticket"]},
        "student_tasks": arr(STUDENT_TASKS),
        "answer_formats": arr(ANSWER_FORMATS),
        "item_count": I,
        "text_scope": S,
        "scaffolds": arr(SCAFFOLDS),
        "instruction_style": S,
        "item_patterns": arr(),
    })),
    "generation_options": arr(GENERATION),
    "style_notes": arr(),
})

EXTRACT_SYS = """你是協助高中英文教師的備課助手。教師上傳一份自己做過的學習單（文字是從 DOCX/PDF 抽出來的，表格已被攤平，可能有斷行或順序錯亂）。
請抽出這份學習單的「設計版型」，讓系統之後能用同樣的設計，為「另一課的新教材」產生一份相似的學習單。

規則：
1. 描述的是設計，不是這一課的內容。title_pattern、item_patterns 要寫成可套用到其他課的通用樣式，把課文專屬的字詞換成 [段落編號]、[關鍵字]、[人物] 這類佔位符。例如「Paragraph 3: Coherence」→「Paragraph [n]: [寫作概念]」；「What do they put on the altars?」→「What do [人物] do/put [地點]?（依段落細節提問）」。
2. 先在 outline 依出現順序列出學習單上「每一個」主要區塊的標題（從開頭到最後一題，包含最後的寫作、反思或 Wrap up；通常 3–10 個）。先從頭到尾看完全文再寫 outline，不要漏掉後半段。
   接著 sections 必須與 outline 一一對應、數量相同、順序相同。
3. student_tasks、answer_formats、scaffolds、generation_options 只能從給定的選項中選；不確定就少選，不要猜。
4. item_count 是該區塊大約有幾題或幾格要學生作答。
5. text_scope 說明這個區塊針對課文哪裡（例如「逐段」「全文」「第 1 段」「課文外」）。
6. instruction_style 描述指示語的寫法（語言、語氣、長短，例如「英文祈使句，一句話」）。
7. 不要寫出任何人名、學校、班級、出版社、教師或學生資料。
8. style_notes 寫 3–6 條這位老師的設計習慣（例如「每段先問細節再問寫作手法」「句型練習都附中文提示」），也要是可套用到其他課的寫法。
所有說明文字用繁體中文。"""

WORKSHEET_SCHEMA = obj({
    "title": S,
    "objectives": arr(),
    "sections": arr(item=obj({
        "title": S,
        "instructions": S,
        "scaffold": S,
        "items": arr(item=obj({
            "answer_format": {"type": "string", "enum": ANSWER_FORMATS},
            "prompt": S,
            "options": arr(),
            "answer": S,
        })),
    })),
})

GEN_SYS = """你是協助高中英文教師的備課助手。教師提供了自己學習單的「設計版型」和一篇新教材。
請完全依照版型，為新教材產生一份學習單：
1. 區塊數量與順序、每個區塊的作答方式、題數、指示語寫法、鷹架，都照版型。
2. 題目內容全部依據新教材，答案必須能在新教材找到根據；不要出現版型以外、也不要出現舊課文的內容。
3. item_patterns 是題型樣式，請把佔位符換成新教材的內容。
4. 新教材沒有適合某區塊的內容時，仍保留該區塊並以最接近的方式改寫，不要刪除。
4b. outline 中 per_paragraph 為 true 的區塊，要對新教材的每一段各做一次（標題寫出段落編號）。
5. 不要寫出任何真實人名、學校或學生資料。
學生看到的題目與指示語依版型的 instruction_language；scaffold 欄位寫給學生的提示（可空字串）。"""

def call(system, user, schema, name, max_tokens, temperature):
    body = {"model": MODEL, "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "response_format": {"type": "json_schema", "json_schema": {"name": name, "strict": True, "schema": schema}},
            "max_tokens": max_tokens, "temperature": temperature}
    req = urllib.request.Request(BASE + "/chat/completions", data=json.dumps(body, ensure_ascii=False).encode(),
                                 headers={"Authorization": "Bearer " + KEY, "Content-Type": "application/json"})
    t = time.time()
    with urllib.request.urlopen(req, timeout=300) as r:
        resp = json.load(r)
    ch = resp["choices"][0]
    return ch["message"]["content"], ch.get("finish_reason"), time.time() - t, resp.get("usage", {})

def run(kind, key, system, user, schema, max_tokens, temperature):
    rec = run1(kind, key, system, user, schema, max_tokens, temperature)
    if "data" not in rec:  # runaway or parse error: retry once with a changed request
        rec = run1(kind, key + "_retry", system, user, schema, max_tokens, min(temperature + 0.4, 1.0))
    return rec

def run1(kind, key, system, user, schema, max_tokens, temperature):
    path = os.path.join(OUT, f"{kind}__{key}.json")
    if os.path.exists(path):
        return json.load(open(path))
    rec = {"key": key, "temperature": temperature}
    try:
        content, finish, secs, usage = call(system, user, schema, kind, max_tokens, temperature)
        rec.update(finish=finish, secs=round(secs, 1), usage=usage)
        try:
            rec["data"] = json.loads(content)
        except Exception as e:
            rec["parse_error"] = str(e); rec["raw_tail"] = content[-300:]
    except Exception as e:
        rec["error"] = repr(e)[:300]
    json.dump(rec, open(path, "w"), ensure_ascii=False, indent=1)
    print(kind, key, rec.get("finish"), rec.get("secs"), "ERR" if ("error" in rec or "parse_error" in rec) else "ok", flush=True)
    return rec

def main():
    mode = sys.argv[1]
    if mode == "extract":
        for fn in sorted(os.listdir(os.path.join(HERE, "text"))):
            text = open(os.path.join(HERE, "text", fn)).read()
            stem = fn[:4]
            for rep, temp in ((1, 0.2), (2, 0.5)):
                run("template", f"v2_{stem}_r{rep}", EXTRACT_SYS, "以下是學習單文字：\n\n" + text, TEMPLATE_SCHEMA, 4000, temp)
    elif mode == "generate":
        passages = json.load(open(os.path.join(HERE, "passages.json")))
        pairs = json.load(open(os.path.join(HERE, "pairs.json")))  # [[template_key, passage_key], ...]
        for tkey, pkey in pairs:
            t = json.load(open(os.path.join(OUT, f"template__{tkey}.json")))["data"]
            p = passages[pkey]
            user = "設計版型：\n" + json.dumps(t, ensure_ascii=False, indent=1) + f"\n\n新教材（{p['title']}）：\n\n" + p["text"]
            run("worksheet", f"{tkey}__{pkey}", GEN_SYS, user, WORKSHEET_SCHEMA, 8000, 0.3)

if sys.argv[1] in ("extract", "generate"): main()

# ---- v3: two calls. A = outline + header facts; B = section details with a fixed count.
OUTLINE_SCHEMA = obj({
    "worksheet_kind": S,
    "instruction_language": {"type": "string", "enum": ["英文", "中文", "中英並用"]},
    "header_fields": arr(),
    "shows_objectives": B,
    "includes_passage_text": B,
    "outline": arr(),
})
SECTION = TEMPLATE_SCHEMA["properties"]["sections"]["items"]
def details_schema(n):
    return obj({
        "sections": {"type": "array", "items": SECTION, "minItems": n, "maxItems": n},
        "generation_options": arr(GENERATION),
        "style_notes": arr(),
    })
OUTLINE_SYS = EXTRACT_SYS.split("規則：")[0] + """這一步只要列出大綱。
1. outline：依出現順序列出學習單上「每一個」主要區塊的標題（從開頭到最後一題，包含最後的寫作、反思或 Wrap up；通常 3–10 個）。先從頭到尾看完全文再寫，不要漏掉後半段。同一份學習單重複出現的學生版／教師版只算一次。
2. 標題寫成可套用到其他課的通用樣式，把課文專屬字詞換成 [佔位符]。
3. 不要寫出任何人名、學校、班級、出版社、教師或學生資料。"""
import re
DETAIL_SYS = re.sub(r"\n2\. .*?\n3\. ", "\n2. sections 必須與給定的大綱一一對應：數量相同、順序相同，每個大綱項目一個 section。\n3. ", EXTRACT_SYS, flags=re.S)

def extract_v3():
    for fn in sorted(os.listdir(os.path.join(HERE, "text"))):
        text = open(os.path.join(HERE, "text", fn)).read()
        stem = fn[:4]
        for rep, temp in ((1, 0.2), (2, 0.5)):
            a = run("outline", f"v3_{stem}_r{rep}", OUTLINE_SYS, "以下是學習單文字：\n\n" + text, OUTLINE_SCHEMA, 2000, temp)
            if "data" not in a: continue
            ol = a["data"]["outline"]
            user = "大綱（共 %d 項，sections 要逐項對應）：\n" % len(ol) + "\n".join(f"{i+1}. {t}" for i, t in enumerate(ol)) + "\n\n以下是學習單文字：\n\n" + text
            b = run("details", f"v3_{stem}_r{rep}", DETAIL_SYS, user, details_schema(len(ol)), 5000, temp)
            if "data" in b:
                tpl = dict(a["data"]); tpl.update(b["data"])
                json.dump({"key": f"v3_{stem}_r{rep}", "data": tpl}, open(os.path.join(OUT, f"template__v3_{stem}_r{rep}.json"), "w"), ensure_ascii=False, indent=1)

if sys.argv[1] == "extract3":
    extract_v3()

# ---- v4: structured outline (per-paragraph blocks listed once; sub-steps not separate)
OUTLINE4_SCHEMA = obj({
    "worksheet_kind": S,
    "instruction_language": {"type": "string", "enum": ["英文", "中文", "中英並用"]},
    "header_fields": arr(),
    "shows_objectives": B,
    "includes_passage_text": B,
    "outline": arr(item=obj({"title_pattern": S, "per_paragraph": B})),
})
OUTLINE4_SYS = EXTRACT_SYS.split("規則：")[0] + """這一步只要列出大綱。
1. outline：依出現順序列出學習單的主要區塊（從開頭到最後一題，包含最後的寫作、反思、評量或 Wrap up）。先從頭到尾看完全文再寫，不要漏掉後半段。
2. 一個區塊裡的小步驟（Step 1/2/3、小題、同一個表格的各欄）不要另外列；它們屬於同一個區塊。
3. 針對課文「每一段」重複出現的同一組活動只列一次，per_paragraph 設為 true，標題用 [n] 表示段落。其他區塊 per_paragraph 為 false。
4. 學生版與教師版（或答案版）重複出現時只算一次。
5. 標題寫成可套用到其他課的通用樣式，把課文專屬字詞換成 [佔位符]。
6. 不要寫出任何人名、學校、班級、出版社、教師或學生資料。"""

def extract_v4():
    for fn in sorted(os.listdir(os.path.join(HERE, "text"))):
        text = open(os.path.join(HERE, "text", fn)).read()
        stem = fn[:4]
        for rep, temp in ((1, 0.2), (2, 0.5)):
            a = run("outline", f"v4_{stem}_r{rep}", OUTLINE4_SYS, "以下是學習單文字：\n\n" + text, OUTLINE4_SCHEMA, 2000, temp)
            if "data" not in a: continue
            ol = a["data"]["outline"]
            lines = [f"{i+1}. {o['title_pattern']}" + ("（逐段重複，只寫一個 section，text_scope 寫「逐段」）" if o["per_paragraph"] else "") for i, o in enumerate(ol)]
            user = "大綱（共 %d 項，sections 要逐項對應）：\n" % len(ol) + "\n".join(lines) + "\n\n以下是學習單文字：\n\n" + text
            b = run("details", f"v4_{stem}_r{rep}", DETAIL_SYS, user, details_schema(len(ol)), 5000, temp)
            if "data" in b:
                tpl = dict(a["data"]); tpl.update(b["data"])
                json.dump({"key": f"v4_{stem}_r{rep}", "data": tpl}, open(os.path.join(OUT, f"template__v4_{stem}_r{rep}.json"), "w"), ensure_ascii=False, indent=1)

if sys.argv[1] == "extract4":
    extract_v4()

# ---- gen2: one call per template section (like the production generator's step split)
SECTION_OUT = obj({
    "title": S,
    "instructions": S,
    "scaffold": S,
    "items": {"type": "array", "items": WORKSHEET_SCHEMA["properties"]["sections"]["items"]["properties"]["items"]["items"], "maxItems": 12},
})
GEN2_SYS = GEN_SYS + """
這次只產生「一個區塊」：給定的目標區塊。題數照版型的 item_count（最多 12 題）。"""

def generate2():
    passages = json.load(open(os.path.join(HERE, "passages.json")))
    pairs = json.load(open(os.path.join(HERE, "pairs.json")))
    for tkey, pkey in pairs:
        t = json.load(open(os.path.join(OUT, f"template__{tkey}.json")))["data"]
        p = passages[pkey]
        paras = p["text"].split("\n\n")
        targets = []
        for i, sec in enumerate(t["sections"]):
            per = t["outline"][i]["per_paragraph"] if i < len(t["outline"]) else False
            if per:
                targets += [(i, sec, n + 1) for n in range(len(paras))]
            else:
                targets.append((i, sec, None))
        out, done = [], []
        t0 = time.time()
        for j, (i, sec, para) in enumerate(targets):
            head = {k: t[k] for k in ("worksheet_kind", "instruction_language", "style_notes")}
            user = ("版型整體：\n" + json.dumps(head, ensure_ascii=False) +
                    "\n\n目標區塊（第 %d 個，共 %d 個）：\n" % (j + 1, len(targets)) + json.dumps(sec, ensure_ascii=False) +
                    (f"\n\n這個區塊只針對新教材第 {para} 段。" if para else "") +
                    "\n\n已完成的區塊標題：" + ("、".join(done) or "（無）") +
                    f"\n\n新教材（{p['title']}，共 {len(paras)} 段）：\n\n" + "\n\n".join(f"[第 {n+1} 段] {x}" for n, x in enumerate(paras)))
            rec = run("section", f"{tkey}__{pkey}__{j+1:02d}", GEN2_SYS, user, SECTION_OUT, 2500, 0.3)
            if "data" in rec:
                out.append(rec["data"]); done.append(rec["data"]["title"])
            else:
                out.append(None)
        json.dump({"key": f"{tkey}__{pkey}", "secs": round(time.time() - t0, 1), "data": {"sections": out}},
                  open(os.path.join(OUT, f"worksheet2__{tkey}__{pkey}.json"), "w"), ensure_ascii=False, indent=1)
        print("worksheet2", tkey, pkey, len(targets), "sections", sum(x is None for x in out), "failed", round(time.time() - t0, 1), "s", flush=True)

if sys.argv[1] == "generate2":
    generate2()
