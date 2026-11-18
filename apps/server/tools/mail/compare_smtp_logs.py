#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Compare what two backends sent to `fake_smtp.py`.

`tools/mail/run-smtp-scenario.sh <mode>` starts one listener per target, each
writing its JSONL transcript, and then calls this script. It compares every
field the endpoint's branch depends on:

* the EHLO/HELO argument (both backends send `socket.getfqdn()`), the envelope
  (`mail_from` plus its `size=` parameter, `rcpt_to` and its parameters), the
  authenticated user, and the message bytes with `Date:` masked and the
  `Message-ID` local part masked **but its domain compared** (also
  `socket.getfqdn()`, and the same value the EHLO argument must have);
* the whole client/server transcript: each client command as sent (verb, its
  argument, MAIL/RCPT parameters) with the AUTH payloads masked, and each server
  reply code. This is what distinguishes the branches: EHLO vs the HELO
  fallback, `AUTH PLAIN` then `AUTH LOGIN` after a 535, the `RSET` smtplib sends
  after a refused MAIL/RCPT/DATA, `QUIT` from `send_messages`' `finally`, and
  **no** QUIT when the connection failed while opening.

Exit code 0 when the two logs agree, 1 otherwise. `--expect` names a mode so the
report can say what the run was supposed to produce.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

DATE = re.compile(rb"(?m)^Date:[^\r\n]*(?:\r?\n[ \t][^\r\n]*)*")
# The header value with its folding whitespace (continuations included); a
# long Message-ID is folded after the header name (`email/header.py`).
MESSAGE_ID = re.compile(rb"(?m)^Message-ID:([^\r\n]*(?:\r?\n[ \t][^\r\n]*)*)")
# Client commands whose line is compared; anything else a client sends is an
# AUTH challenge response and is masked.
COMMANDS = {"EHLO", "HELO", "AUTH", "MAIL", "RCPT", "DATA", "RSET", "QUIT", "NOOP", "STARTTLS"}
# `MAIL FROM:<a@b> size=<len(msg)>`: the length moves with the Message-ID's
# random decimal digits, so only the parameter's presence is compared.
SIZE = re.compile(r"size=\d+")


def unfold(value: bytes) -> bytes:
    return value.replace(b"\r\n", b"").replace(b"\n", b"").strip()


def normalise_data(data: str | None) -> tuple[str | None, str | None]:
    """The message bytes with volatile headers masked, and the Message-ID domain."""
    if data is None:
        return None, None
    raw = data.encode("latin-1", "replace")
    domain = None
    match = MESSAGE_ID.search(raw)
    if match:
        value = unfold(match.group(1))
        _, at, tail = value.rpartition(b"@")
        if at:
            domain = tail.rstrip(b">").decode("latin-1")
    raw = MESSAGE_ID.sub(b"Message-ID: <MESSAGE-ID>", raw)
    raw = DATE.sub(b"Date: <DATE>", raw)
    return raw.decode("latin-1"), domain


def normalise_transcript(transcript: list) -> list[str]:
    """Every command and reply, in order, with credentials masked."""
    result: list[str] = []
    for entry in transcript:
        if not isinstance(entry, list) or len(entry) != 2:
            continue
        side, line = entry
        text = str(line)
        if side == "client":
            verb, _, rest = text.partition(" ")
            verb = verb.upper()
            if text.startswith("<"):
                # `<data: N line(s)>` and `<eof>`: the payload marker and the
                # connection ending, both worth comparing.
                result.append(f"client {text}")
            elif verb == "AUTH":
                # `AUTH PLAIN <base64>` / `AUTH LOGIN <base64>`: compare the
                # mechanism, never the credentials.
                mechanism = rest.partition(" ")[0].strip()
                result.append(f"client AUTH {mechanism}".strip())
            elif verb in COMMANDS:
                # The `size=` parameter counts the Message-ID's random digits,
                # so its value is not comparable; its presence is.
                result.append(f"client {SIZE.sub('size=<size>', text)}")
            else:
                # A base64 AUTH challenge response.
                result.append("client <response>")
        else:
            # A multiline EHLO reply is one entry per feature line; keep only
            # the code so the feature list (client-independent) is not noise.
            result.append(f"server {text.partition(' ')[0]}")
    return result


def normalise(record: dict) -> dict:
    data, message_id_domain = normalise_data(record.get("data"))
    mail_params = record.get("mail_params")
    if isinstance(mail_params, str):
        mail_params = SIZE.sub("size=<size>", mail_params)
    return {
        "ehlo": record.get("ehlo"),
        "mail_from": record.get("mail_from"),
        "mail_params": mail_params,
        "rcpt_to": record.get("rcpt_to"),
        "rcpt_params": record.get("rcpt_params"),
        "auth": record.get("auth"),
        "data": data,
        "message_id_domain": message_id_domain,
        "transcript": normalise_transcript(record.get("transcript") or []),
    }


def load(path: pathlib.Path) -> list[dict]:
    records = []
    if not path.exists():
        return records
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        records.append(normalise(json.loads(line)))
    return records


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--a", required=True, type=pathlib.Path)
    parser.add_argument("--b", required=True, type=pathlib.Path)
    parser.add_argument("--expect", default="")
    args = parser.parse_args()

    a = load(args.a)
    b = load(args.b)
    failures: list[str] = []
    if len(a) != len(b):
        failures.append(f"transaction count differs: A {len(a)}, B {len(b)}")
    fields = (
        "ehlo",
        "mail_from",
        "mail_params",
        "rcpt_to",
        "rcpt_params",
        "auth",
        "data",
        "message_id_domain",
        "transcript",
    )
    for index, (left, right) in enumerate(zip(a, b)):
        for field in fields:
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
        f"({len(a)} transaction(s), ehlo/envelope/message/transcript equal)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
