# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Record the exact HTTP request the pinned `openai` SDK sends for Plane's call.

`plane/app/views/external/base.py:123-145` builds `OpenAI(api_key=...)` and calls
`chat.completions.create(model=..., messages=[{"role": "user", "content": ...}])`.
The Rust port has to put the same bytes on the wire, so this script runs the
*real* SDK against a local capture server and writes everything that arrives into
`crates/plane-http-app/tests/golden/openai_wire.json`.

Run it from `apps/server` with the Django virtualenv:

    apps/api/.venv/bin/python tools/ai/openai_wire.py

Options:

    --out PATH    write somewhere else (default: the crate's golden file)
    --print       print the recorded requests instead of writing the file

No network is used: the client's `base_url` points at a `127.0.0.1` listener, and
`max_retries=0` means each case makes exactly one connection. The key and the
organization/project ids are fake. Never point this at api.openai.com.

What is recorded, per case:

    name      the case
    env       OPENAI_ORG_ID / OPENAI_PROJECT_ID as the SDK sees them (`null` = unset)
    base_url  the `base_url` argument, with `{port}` for the capture server
    request   {"method", "target", "headers", "body", "body_bytes"}

`target` is the request-target from the request line (path + query), which is
what the SDK's base-URL handling produces. `headers` is the ordered list of
`[name, value]` pairs exactly as they arrived, including the ones httpx adds
(`Host`, `Accept-Encoding`, `Content-Length`, `Connection`), because the Rust
test's job is to know which of them it owns and which the transport owns.
`body` is the body decoded as UTF-8 (it is always UTF-8 for this call) with the
exact bytes preserved, so the Rust test can compare a `String`.

The case list is the surface `get_llm_config` (`external/base.py:76-120`) can
produce: the three providers' model lists (`:42-73`), the gemini model prefix
(`:128-129`), a non-ASCII prompt so the body's escaping is pinned, and the
organization/project header pair (`_client.py:122-127,175-180`). Base URLs
cover a bare origin, a `/v1` suffix and a path prefix, because `OPENAI_BASE_URL`
is a deployment setting.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import socket
import sys
import threading
from pathlib import Path

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
DEFAULT_OUT = SERVER_DIR / "crates/plane-http-app/tests/golden/openai_wire.json"

# A fake key that is recorded with the request. Never a real credential.
API_KEY = "wire-golden-key"

# A syntactically valid chat completion. The SDK parses it into its pydantic
# models before `chat_completion.choices[0].message.content` is read
# (`external/base.py:135`), so every field it requires has to be present.
REPLY = {
    "id": "chatcmpl-wire-golden",
    "object": "chat.completion",
    "created": 1700000000,
    "model": "gpt-4o-mini",
    "choices": [
        {
            "index": 0,
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop",
        }
    ],
    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
}

# The cases. `task` and `prompt` are the two strings `get_llm_response` joins
# with "\n" before sending (`external/base.py:125`).
CASES: list[dict] = [
    {
        "name": "openai-default-model",
        "env": {},
        "base_url": "http://127.0.0.1:{port}",
        "model": "gpt-4o-mini",
        "task": "Summarise the issue",
        "prompt": "The issue is about labels.",
    },
    {
        "name": "openai-v1-suffix",
        "env": {},
        "base_url": "http://127.0.0.1:{port}/v1",
        "model": "gpt-4o",
        "task": "t",
        "prompt": "",
    },
    {
        "name": "openai-path-prefix-trailing-slash",
        "env": {},
        "base_url": "http://127.0.0.1:{port}/proxy/openai/",
        "model": "gpt-3.5-turbo",
        "task": "t",
        "prompt": "p",
    },
    {
        "name": "openai-organization-and-project",
        "env": {"OPENAI_ORG_ID": "org-wire", "OPENAI_PROJECT_ID": "proj-wire"},
        "base_url": "http://127.0.0.1:{port}",
        "model": "o1-mini",
        "task": "t",
        "prompt": "p",
    },
    {
        "name": "openai-empty-prompt-newline",
        "env": {},
        "base_url": "http://127.0.0.1:{port}",
        "model": "o1-preview",
        "task": "line one\nline two",
        "prompt": "",
    },
    {
        "name": "openai-non-ascii",
        "env": {},
        "base_url": "http://127.0.0.1:{port}",
        "model": "gpt-4o-mini",
        "task": "Résume le ticket → 世界",
        "prompt": "emoji 🚀 and a tab\tand a \"quote\" and a \\backslash",
    },
    {
        "name": "anthropic-default-model",
        "env": {},
        "base_url": "http://127.0.0.1:{port}",
        "model": "claude-3-sonnet-20240229",
        "task": "t",
        "prompt": "p",
    },
    {
        "name": "gemini-prefixed-model",
        "env": {},
        "base_url": "http://127.0.0.1:{port}",
        # `get_llm_response` prepends "gemini/" for the gemini provider
        # (`external/base.py:128-129`); the SDK sees this model string.
        "model": "gemini/gemini-pro",
        "task": "t",
        "prompt": "p",
    },
]


def read_request(conn: socket.socket) -> bytes:
    """Read one HTTP/1.1 request: the head plus `Content-Length` body bytes."""
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = conn.recv(65536)
        if not chunk:
            break
        data += chunk
    head, _, body = data.partition(b"\r\n\r\n")
    length = 0
    for line in head.split(b"\r\n")[1:]:
        name, _, value = line.partition(b":")
        if name.strip().lower() == b"content-length":
            length = int(value.strip())
    while len(body) < length:
        chunk = conn.recv(65536)
        if not chunk:
            break
        body += chunk
    return head + b"\r\n\r\n" + body


def serve_once(listener: socket.socket) -> tuple[threading.Thread, list[bytes]]:
    """Accept one connection, capture the request and answer with `REPLY`.

    Returns the worker thread and the one-element list it appends the raw
    request to (the thread writes it, the caller reads it after `join`).
    """

    captured: list[bytes] = []

    def run() -> None:
        conn, _ = listener.accept()
        with conn:
            raw = read_request(conn)
            body = json.dumps(REPLY).encode()
            conn.sendall(
                b"HTTP/1.1 200 OK\r\n"
                b"Content-Type: application/json\r\n"
                + f"Content-Length: {len(body)}\r\n".encode()
                + b"Connection: close\r\n\r\n"
                + body
            )
            captured.append(raw)

    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return thread, captured


def parse_request(raw: bytes) -> dict:
    head, _, body = raw.partition(b"\r\n\r\n")
    lines = head.split(b"\r\n")
    method, target, _ = lines[0].decode().split(" ")
    headers = []
    for line in lines[1:]:
        name, _, value = line.decode("latin-1").partition(":")
        if name:
            headers.append([name.strip(), value.strip()])
    return {
        "method": method,
        "target": target,
        "headers": headers,
        "body": body.decode("utf-8"),
        "body_bytes": len(body),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--print", dest="show", action="store_true")
    args = parser.parse_args()

    # Imported here so `--help` works without the venv's site-packages.
    import openai

    saved = {name: os.environ.get(name) for name in ("OPENAI_ORG_ID", "OPENAI_PROJECT_ID")}
    cases = []
    try:
        for case in CASES:
            for name, value in saved.items():
                if name in case["env"]:
                    os.environ[name] = case["env"][name]
                elif value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value

            listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            port = listener.getsockname()[1]
            thread, holder = serve_once(listener)
            try:
                client = openai.OpenAI(
                    api_key=API_KEY,
                    base_url=case["base_url"].format(port=port),
                    max_retries=0,
                )
                client.chat.completions.create(
                    model=case["model"],
                    messages=[
                        {
                            "role": "user",
                            "content": case["task"] + "\n" + case["prompt"],
                        }
                    ],
                )
            finally:
                thread.join(timeout=10)
                listener.close()
            if not holder:
                raise SystemExit(f"case {case['name']}: the SDK sent no request")
            request = parse_request(holder[0])
            cases.append(
                {
                    "name": case["name"],
                    "env": case["env"],
                    "base_url": case["base_url"],
                    "model": case["model"],
                    "task": case["task"],
                    "prompt": case["prompt"],
                    "request": request,
                }
            )
            print(f"recorded {case['name']}: {request['method']} {request['target']}", file=sys.stderr)
    finally:
        for name, value in saved.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

    document = {
        "generated_by": "apps/server/tools/ai/openai_wire.py",
        "sdk": {
            "openai": openai.__version__,
            "python": platform.python_version(),
            "platform_system": platform.system(),
            "platform_machine": platform.machine(),
        },
        "api_key": API_KEY,
        "why": (
            "The SDK's request bytes are part of the ported contract: the URL "
            "join, the header set and the JSON body. Recorded against a local "
            "listener; api.openai.com is never contacted."
        ),
        "cases": cases,
    }

    if args.show:
        print(json.dumps(document, indent=2, ensure_ascii=False))
        return 0
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
