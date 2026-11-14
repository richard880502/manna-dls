"""Minimal OpenAI-compatible client for the NCHC model used in this validation.

Two transports:
  * ssh  (default): the request body is piped over ssh to the production host, where the API key
    is read into a shell variable and passed to curl. The key never reaches this machine and is
    never printed or logged.
  * http: set MANNA_LLM_TRANSPORT=http and OPENAI_COMPAT_API_KEY in the environment (for reruns
    from a machine that legitimately holds a key).

Every call is appended to a JSONL log under $MANNA_DATA/runs/ (request, response, latency, usage).
The request body mirrors api/src/ai/openai.rs::build_body (system + user, strict json_schema).
"""
from __future__ import annotations

import json
import os
import subprocess
import time
import urllib.request
from pathlib import Path

DATA = Path(os.environ.get("MANNA_DATA", str(Path.home() / "manna-data")))
BASE = os.environ.get("OPENAI_COMPAT_BASE_URL", "https://portal.genai.nchc.org.tw/api/v1")
MODEL = os.environ.get("OPENAI_COMPAT_MODEL", "gemma-4-31B-it")
SSH_HOST = os.environ.get("MANNA_LLM_SSH_HOST", "manna")
ENV_FILE = "/data/coolify/applications/zij0ommqsyg3tr1vqqcwoxtj/.env"
TIMEOUT = int(os.environ.get("MANNA_LLM_TIMEOUT", "200"))
DEFAULT_MAX_TOKENS = int(os.environ.get("MANNA_LLM_MAX_TOKENS", "9000"))

_REMOTE = (
    'K=$(sudo sed -n "s/^OPENAI_COMPAT_API_KEY=\\"\\{0,1\\}\\([^\\"]*\\)\\"\\{0,1\\}$/\\1/p" '
    + ENV_FILE
    + '); curl -s -m %d -H "Authorization: Bearer $K" -H "content-type: application/json" '
    "-d @- %s/chat/completions"
)


def _post(body: dict) -> str:
    raw = json.dumps(body, ensure_ascii=False).encode()
    if os.environ.get("MANNA_LLM_TRANSPORT") == "http":
        req = urllib.request.Request(
            BASE + "/chat/completions",
            data=raw,
            headers={
                "content-type": "application/json",
                "Authorization": "Bearer " + os.environ["OPENAI_COMPAT_API_KEY"],
            },
        )
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            return r.read().decode()
    p = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", SSH_HOST, _REMOTE % (TIMEOUT, BASE)],
        input=raw,
        capture_output=True,
        timeout=TIMEOUT + 30,
    )
    return p.stdout.decode(errors="replace")


def chat(
    system: str,
    user: str,
    schema: dict | None = None,
    schema_name: str = "out",
    *,
    max_tokens: int | None = None,
    temperature: float | None = 0.2,
    tag: str = "",
    log: str = "calls.jsonl",
    retries: int = 2,
    prior: list[dict] | None = None,
) -> dict:
    """Returns {ok, json|None, text, error, latency_s, usage, finish_reason, attempts}."""
    messages = []
    if system:
        messages.append({"role": "system", "content": system})
    messages.append({"role": "user", "content": user})
    # repair turn: [..., user, assistant(previous output), user(check feedback)]
    messages += prior or []
    body: dict = {"model": MODEL, "messages": messages}
    if schema is not None:
        body["response_format"] = {
            "type": "json_schema",
            "json_schema": {"name": schema_name, "schema": schema, "strict": True},
        }
    # Always cap the output. Without a cap, a structured-output request can run away (the model keeps
    # emitting array items until the HTTP timeout) and the abandoned generation keeps the shared
    # backend busy for later calls.
    body["max_tokens"] = max_tokens or DEFAULT_MAX_TOKENS
    if temperature is not None:
        body["temperature"] = temperature
    out: dict = {}
    for attempt in range(1, retries + 2):
        if attempt > 1:
            # The gateway caches identical requests (an identical retry comes back in <1 s with the
            # same failure), and runaway output tends to repeat at the same temperature: change it.
            body["temperature"] = round((temperature or 0.0) + 0.15 * (attempt - 1), 2)
        t0 = time.time()
        err, text, parsed, usage, finish = None, "", None, {}, None
        try:
            resp = _post(body)
            v = json.loads(resp)
            if "choices" not in v:
                err = "api_error: " + json.dumps(v, ensure_ascii=False)[:600]
            else:
                ch = v["choices"][0]
                finish = ch.get("finish_reason")
                text = ch["message"].get("content") or ""
                usage = v.get("usage") or {}
                if finish == "length":
                    err = "max_tokens"
                elif schema is not None:
                    try:
                        parsed = json.loads(text.strip())
                    except Exception as e:  # noqa: BLE001
                        err = f"invalid_json: {e}"
        except Exception as e:  # noqa: BLE001
            err = f"transport: {type(e).__name__}: {str(e)[:300]}"
        out = {
            "ok": err is None,
            "json": parsed,
            "text": text,
            "error": err,
            "latency_s": round(time.time() - t0, 2),
            "usage": usage,
            "finish_reason": finish,
            "attempts": attempt,
        }
        (DATA / "runs").mkdir(parents=True, exist_ok=True)
        with open(DATA / "runs" / log, "a") as f:
            f.write(json.dumps({"tag": tag, "ts": time.time(), "schema_name": schema_name,
                                "system": system, "user": user, **out}, ensure_ascii=False) + "\n")
        if err is None or (err == "max_tokens" and attempt >= 2):
            break                      # max_tokens (runaway output) gets exactly one retry
        time.sleep(5 * attempt)  # back off on transport / API errors
    return out


if __name__ == "__main__":
    r = chat("", "Reply with a JSON greeting.", {
        "type": "object", "properties": {"greeting": {"type": "string"}},
        "required": ["greeting"], "additionalProperties": False}, tag="probe")
    print(json.dumps({k: r[k] for k in ("ok", "json", "error", "latency_s", "usage")}, ensure_ascii=False))
