#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Compare what two backends sent to `fake_smtp.py`.

`tools/mail/run-smtp-scenario.sh <mode>` starts one listener per target, each
writing its JSONL transcript, and then calls this script. The comparison keeps
what the endpoint's branch depends on and masks what is client metadata:

* the envelope (`mail_from`, `rcpt_to`), the authenticated user and the message
  bytes with `Date:`/`Message-ID:` masked;
* the sequence of **client verbs** and **server reply codes**, so a failure
  that never reached MAIL (authentication, greeting, a dropped connection) is
  still distinguished. The EHLO argument, AUTH payload and MAIL/RCPT addresses
  are client- or message-level and already covered by the fields above.

Exit code 0 when the two logs agree, 1 otherwise. `--expect` names a mode so the
report can say what the run was supposed to produce.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

DATE = re.compile(rb"(?m)^Date: .*$")
# `EmailMessage.message()` folds a long header at 78 columns, so the
# Message-ID can own continuation lines. Django composes it from
# `socket.getfqdn()` (`make_msgid(domain=DNS_NAME)`); the Rust transport uses
# the process HOSTNAME (DECISIONS.md D23's libc note), so the whole header —
# continuations included — is masked.
MESSAGE_ID = re.compile(rb"(?m)^Message-ID: .*(?:\r?\n[ \t].*)*")


def normalise_data(data: str | None) -> str | None:
    if data is None:
        return None
    raw = data.encode("latin-1", "replace")
    raw = DATE.sub(b"Date: <DATE>", raw)
    raw = MESSAGE_ID.sub(b"Message-ID: <MESSAGE-ID>", raw)
    return raw.decode("latin-1")


def normalise_transcript(transcript: list, keep: bool) -> list[str]:
    """Client verbs and server reply codes, in order.

    `smtplib` sends `RSET` after a refused DATA and `lettre` does not; the
    `RSET` and its reply are dropped so the command flow that matters compares.
    """
    if not keep:
        return []
    result: list[str] = []
    skip_next_server = False
    for entry in transcript:
        if not isinstance(entry, list) or len(entry) != 2:
            continue
        side, line = entry
        text = str(line)
        if side == "client":
            verb = text.split(" ", 1)[0].upper()
            if verb == "RSET":
                skip_next_server = True
                continue
            result.append(f"client {verb}")
        else:
            if skip_next_server:
                skip_next_server = False
                continue
            code = text.split(" ", 1)[0]
            # A multiline EHLO reply is one entry per feature line; keep only
            # the code so the feature list (client-independent) is not noise.
            result.append(f"server {code}")
    return result


def normalise(record: dict, keep_transcript: bool) -> dict:
    transcript = record.get("transcript") or []
    clients = [
        str(entry[1])
        for entry in transcript
        if isinstance(entry, list) and len(entry) == 2 and entry[0] == "client"
    ]
    # The Rust transport probes a 554 with a second connection that sends only
    # `QUIT` (`mail.rs::probe_greeting`); it carries no backend behaviour.
    probe = [command.strip().upper() for command in clients] == ["QUIT"]
    return {
        "probe": probe,
        "mail_from": record.get("mail_from"),
        "rcpt_to": record.get("rcpt_to"),
        "auth": record.get("auth"),
        "data": normalise_data(record.get("data")),
        "transcript": normalise_transcript(transcript, keep_transcript),
    }


def load(path: pathlib.Path, keep_transcript: bool) -> list[dict]:
    records = []
    if not path.exists():
        return records
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        record = normalise(json.loads(line), keep_transcript)
        if record.pop("probe"):
            continue
        records.append(record)
    return records


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--a", required=True, type=pathlib.Path)
    parser.add_argument("--b", required=True, type=pathlib.Path)
    parser.add_argument("--expect", default="")
    parser.add_argument(
        "--no-transcript",
        action="store_true",
        help="compare envelope, auth and message bytes only (AUTH mechanism retries differ)",
    )
    args = parser.parse_args()

    a = load(args.a, not args.no_transcript)
    b = load(args.b, not args.no_transcript)
    failures: list[str] = []
    if len(a) != len(b):
        failures.append(f"transaction count differs: A {len(a)}, B {len(b)}")
    for index, (left, right) in enumerate(zip(a, b)):
        for field in ("mail_from", "rcpt_to", "auth", "data", "transcript"):
            if left[field] != right[field]:
                failures.append(
                    f"transaction {index}: {field} differs\n  A: {left[field]!r}\n  B: {right[field]!r}"
                )
    if failures:
        print(f"smtp-compare ({args.expect or 'mode'}): FAIL")
        for failure in failures:
            print(" ", failure)
        return 1
    print(
        f"smtp-compare ({args.expect or 'mode'}): ok "
        f"({len(a)} transaction(s), envelope/message/transcript equal)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
