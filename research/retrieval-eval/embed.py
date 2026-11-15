"""Embeds texts through an OpenAI-compatible /embeddings endpoint, with an on-disk cache.

The API key never passes through this script. `EMBED_CMD` is a shell command that reads an
embeddings request body (JSON) on stdin and writes the response on stdout, e.g.

    EMBED_CMD='curl -s -m 120 -H "Authorization: Bearer $EMBEDDING_API_KEY" \
        -H "content-type: application/json" -d @- https://portal.genai.nchc.org.tw/api/v1/embeddings'

Vectors are cached per (model, text) in `$EVAL_CACHE` (default ~/manna-data/kb/eval-cache), outside
the repository: the curriculum texts and anything derived from them are not committed.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

MODEL = os.environ.get("EMBEDDING_MODEL", "embeddinggemma-2")
CACHE_DIR = Path(os.environ.get("EVAL_CACHE", "~/manna-data/kb/eval-cache")).expanduser()
BATCH = 64


def _key(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _cache_path() -> Path:
    return CACHE_DIR / f"{MODEL.replace('/', '_')}.jsonl"


def _load() -> dict[str, list[float]]:
    out: dict[str, list[float]] = {}
    path = _cache_path()
    if path.exists():
        with path.open(encoding="utf-8") as f:
            for line in f:
                row = json.loads(line)
                out[row["k"]] = row["v"]
    return out


def _call(texts: list[str]) -> list[list[float]]:
    cmd = os.environ.get("EMBED_CMD")
    if not cmd:
        sys.exit("EMBED_CMD is not set and some texts are not cached; see embed.py")
    body = json.dumps({"model": MODEL, "input": texts}, ensure_ascii=False)
    proc = None
    for attempt in range(4):
        proc = subprocess.run(cmd, shell=True, input=body.encode("utf-8"), capture_output=True)
        try:
            data = json.loads(proc.stdout)["data"]
            data.sort(key=lambda d: d["index"])
            assert len(data) == len(texts)
            return [d["embedding"] for d in data]
        except Exception:  # noqa: BLE001 - retried, then reported without the response body
            time.sleep(2 * (attempt + 1))
    sys.exit(f"embedding request failed after retries (exit {proc.returncode if proc else '?'})")


def embed(texts: list[str]) -> list[list[float]]:
    """Vectors for `texts`, in order. Only uncached texts are sent."""
    cache = _load()
    missing = [t for t in dict.fromkeys(texts) if _key(t) not in cache]
    if missing:
        CACHE_DIR.mkdir(parents=True, exist_ok=True)
        with _cache_path().open("a", encoding="utf-8") as f:
            for i in range(0, len(missing), BATCH):
                chunk = missing[i : i + BATCH]
                for text, vec in zip(chunk, _call(chunk)):
                    cache[_key(text)] = vec
                    f.write(json.dumps({"k": _key(text), "v": vec}) + "\n")
                print(f"  embedded {min(i + BATCH, len(missing))}/{len(missing)}", file=sys.stderr)
    return [cache[_key(t)] for t in texts]
