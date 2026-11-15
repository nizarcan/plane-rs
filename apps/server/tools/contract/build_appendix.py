# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Regenerate the appendix of HTTP_CONTRACT.md from the probe scripts.

Usage, from apps/api:

    .venv/bin/python ../server/tools/contract/build_appendix.py

It runs every probe with the pinned venv, strips Django's JSON log lines and the
whitenoise startup warning (which are environment noise, not contract), and
rewrites everything after the ``<!-- APPENDIX -->`` marker in
``apps/server/HTTP_CONTRACT.md``.
"""

import os
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
DOC = HERE.parent.parent / "HTTP_CONTRACT.md"
MARKER = "<!-- APPENDIX -->"
PYTHON = ".venv/bin/python"

SECTIONS = [
    ("A.1", "probe_http.py", "status, headers and raw body bytes"),
    ("A.2", "probe_session_cookie.py", "Set-Cookie for every session branch"),
    ("A.3", "probe_throttles.py", "throttle identities, cache keys, pickle values"),
    ("A.4", "probe_json_renderer.py", "exact JSON bytes"),
    ("B.1", "session_codec.py", "session payload test vector"),
    ("B.2", "csrf_token_vector.py", "CSRF masked-token test vector"),
]


def clean(text):
    out = []
    skip_warning = False
    for line in text.splitlines():
        if line.startswith('{"levelname"'):
            continue
        if line.startswith(" " * 0) and "UserWarning: No directory at:" in line:
            skip_warning = True
            continue
        if skip_warning and line.strip() == "mw_instance = middleware(adapted_handler)":
            skip_warning = False
            continue
        skip_warning = False
        out.append(line)
    # collapse runs of blank lines introduced by the filtering
    collapsed = []
    for line in out:
        if line == "" and collapsed and collapsed[-1] == "":
            continue
        collapsed.append(line)
    return "\n".join(collapsed).strip("\n")


def run(script):
    env = dict(os.environ)
    env.pop("DEBUG", None)
    proc = subprocess.run(
        [PYTHON, str(HERE / script)],
        capture_output=True,
        text=True,
        cwd=str(HERE.parent.parent.parent / "api"),
        env=env,
    )
    if proc.returncode != 0:
        raise SystemExit("%s failed (%d):\n%s" % (script, proc.returncode, proc.stderr[-4000:]))
    return clean(proc.stdout)


BANNER = re.compile(r"^== (\w+)\. (.*)$")


def split_banners(text):
    """Split probe_http.py output on its `== N. title ==` banners."""
    blocks = []
    current = None
    body = []
    preamble = []
    for line in text.splitlines():
        match = BANNER.match(line)
        if match:
            if current is not None:
                blocks.append((current[0], current[1], "\n".join(body)))
            elif body:
                preamble = body
            current = (match.group(1), match.group(2))
            body = []
            continue
        if line.startswith("=" * 40):
            continue
        body.append(line)
    if current is not None:
        blocks.append((current[0], current[1], "\n".join(body)))
    if preamble:
        blocks.insert(0, ("0", "preamble", "\n".join(preamble)))
    return blocks


def main():
    body = DOC.read_text()
    if MARKER in body:
        head = body.split(MARKER)[0].rstrip() + "\n"
    elif "\n# Appendix\n" in body:
        head = body.split("\n# Appendix\n")[0].rstrip() + "\n"
    else:
        raise SystemExit("no appendix anchor found in %s" % DOC)

    parts = [
        "",
        "---",
        "",
        "# Appendix",
        "",
        "Captured output of the scripts in `apps/server/tools/contract/`, run from",
        "`apps/api` with `DJANGO_SETTINGS_MODULE=plane.settings.production`,",
        "`SECRET_KEY=contract-test-secret-key-not-for-deployment`,",
        "`DATABASE_URL=postgresql://x:x@127.0.0.1:1/x`, `REDIS_URL=redis://127.0.0.1:1/0`.",
        "Django's JSON log lines and the whitenoise startup warning are stripped; nothing",
        "else is edited. Bodies longer than the script's preview limit are marked as such,",
        "and `b'...'` is Python `repr` of the exact bytes.",
        "",
        "Reproduce with:",
        "",
        "```sh",
        "cd apps/api",
        "for s in probe_http probe_session_cookie probe_throttles probe_json_renderer \\",
        "         session_codec csrf_token_vector; do",
        "  .venv/bin/python ../server/tools/contract/$s.py",
        "done",
        "```",
        "",
        "or regenerate this appendix with",
        "`.venv/bin/python ../server/tools/contract/build_appendix.py`.",
        "",
    ]

    for number, script, title in SECTIONS:
        output = run(script)
        if script == "probe_http.py":
            parts.append("## %s `%s` — %s" % (number, script, title))
            parts.append("")
            parts.append(
                "Split by the script's own `== N. …` banners; the sub-section numbers are"
                " the ones cited as `§A.1.N` in the body."
            )
            parts.append("")
            for sub, sub_title, block in split_banners(output):
                parts.append("### %s.%s %s" % (number, sub, sub_title))
                parts.append("")
                parts.append("```text")
                parts.append(block.strip("\n"))
                parts.append("```")
                parts.append("")
            continue
        parts.append("## %s `%s` — %s" % (number, script, title))
        parts.append("")
        parts.append("```text")
        parts.append(output)
        parts.append("```")
        parts.append("")

    DOC.write_text(head + "\n" + MARKER + "\n" + "\n".join(parts))
    print("wrote", DOC)
    return 0


if __name__ == "__main__":
    sys.exit(main())
