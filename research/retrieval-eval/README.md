# Retrieval evaluation (keyword vs hybrid)

Harness behind `docs/RETRIEVAL_EVAL.md`.

| File | What |
|---|---|
| `queries.json` | 31 labelled queries (general 15, vocational 16; zh / en / mixed). Labels are the developer's reading of the official texts, **not** a teacher's judgement. |
| `evaluate.py` | Ports of the keyword scorer (`api/src/retrieval.rs`), the embedded text (`api/src/kb_embeddings.rs`) and the fusion (`retrieval::fuse`); recall@8/20/150 and MRR. |
| `embed.py` | Embedding calls with an on-disk cache outside the repo (`~/manna-data/kb/eval-cache`). |
| `results.json` | Per-query metrics of the last run (no curriculum text). |

The curriculum CSV (`~/manna-data/kb/import/curriculum_import.csv`) and the cached vectors stay
out of the repo.

```bash
# EMBED_CMD reads an embeddings request on stdin and prints the response; the API key never
# passes through these scripts. Only needed for texts that are not cached yet.
EMBED_CMD='curl -s -m 120 -H "Authorization: Bearer $EMBEDDING_API_KEY" -H "content-type: application/json" \
  -d @- https://portal.genai.nchc.org.tw/api/v1/embeddings' python3 evaluate.py --sweep
python3 evaluate.py --examples --json results.json
python3 evaluate.py --dump rankings.json   # rankings + similarities, to compare with the Rust code
```

The port was checked against `retrieval::rank` and `retrieval::rank_hybrid` once (a throwaway
Rust test fed `--dump` output): identical keyword rankings and identical hybrid top-20 for all
31 queries. Re-check after changing either side.
