#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""A fake SMTP server that records what a backend sent.

`plane/license/api/views/configuration.py:80-171` sends one email through
Django's SMTP backend. A differential scenario for that route compares two
things the HTTP response cannot show: the **message bytes** the backend put on
the wire (header order, encoded words, folding, transfer encoding) and the
**envelope** (`MAIL FROM:`/`RCPT TO:`). This listener records both, per
transaction, as JSON lines:

    {"time": "...", "peer": "127.0.0.1:54321",
     "ehlo": "1.0.0.127.in-addr.arpa", "auth": null,
     "mail_from": "<team@mailer.plane.so>",
     "rcpt_to": ["<receiver@example.com>"],
     "data": "Content-Type: text/plain; charset=\"utf-8\"\\r\\n...",
     "transcript": [["connect", "220 fake-smtp ready"], ["EHLO ...", "250-..."], ...]}

`data` is the message exactly as received (CRLF preserved, dot-unstuffed,
without the terminating `.<CRLF>`), so a scenario can compare it byte for byte
after masking `Date:`/`Message-ID:` with the scenario's normaliser.
`mail_from`/`rcpt_to` hold the **addresses only** (`<a@b>`, brackets kept) and
`mail_params`/`rcpt_params` the ESMTP parameters after them: smtplib appends
`size=<len>` when the server advertises SIZE (`smtplib.py:865-867`) while lettre
does not, so the parameters are recorded but are not part of the envelope a
scenario compares.

Run it (ports 18410-18419 are reserved for this listener):

    apps/api/.venv/bin/python apps/server/tools/mail/fake_smtp.py \\
        --port 18410 --output /tmp/mail-a.jsonl

Then point a backend at it with the instance configuration the scenario writes
(`EMAIL_HOST=127.0.0.1`, `EMAIL_PORT=18410`, `EMAIL_USE_TLS=0`,
`EMAIL_USE_SSL=0`, `EMAIL_FROM=...`) and POST
`/api/instances/email-credentials-check/`.

Failure modes exist so the endpoint's branches can be exercised:
`--greeting 554` (SMTPConnectError), `--auth` with a wrong password
(SMTPAuthenticationError), `--reject-sender` (SMTPSenderRefused),
`--reject-recipient` (SMTPRecipientsRefused), `--data-error` (SMTPDataError),
`--drop-after-greeting` (SMTPServerDisconnected) and `--delay 5`
(TimeoutError). Every reply and the whole transcript are recorded either way.

The listener never talks to the network beyond `--bind` (default 127.0.0.1) and
never relays: a completed DATA is accepted and recorded, nothing more.
"""

from __future__ import annotations

import argparse
import base64
import json
import pathlib
import socketserver
import threading
import time


class Recorder:
    """Append-only JSONL writer, shared by the handler threads."""

    def __init__(self, path: pathlib.Path | None):
        self.path = path
        self.lock = threading.Lock()
        if path is not None:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("", encoding="utf-8")

    def write(self, record: dict) -> None:
        line = json.dumps(record, ensure_ascii=False)
        with self.lock:
            if self.path is not None:
                with self.path.open("a", encoding="utf-8") as handle:
                    handle.write(line + "\n")
            print(line, flush=True)


class Options:
    def __init__(self, args: argparse.Namespace):
        self.greeting = args.greeting
        self.auth = args.auth
        self.user = args.user
        self.password = args.password
        self.reject_sender = args.reject_sender
        self.reject_recipient = args.reject_recipient
        self.data_error = args.data_error
        self.drop_after_greeting = args.drop_after_greeting
        self.delay = args.delay
        self.quit_error = args.quit_error


class Handler(socketserver.StreamRequestHandler):
    options: Options
    recorder: Recorder

    @staticmethod
    def split_envelope(tail: str) -> tuple[str, str]:
        """`FROM:<a@b> size=42` -> (`<a@b>`, `size=42`)."""
        _, _, rest = tail.partition(":")
        rest = rest.strip()
        start = rest.find("<")
        end = rest.find(">", start + 1) if start >= 0 else -1
        if start < 0 or end < 0:
            return rest, ""
        return rest[start : end + 1], rest[end + 1 :].strip()

    def reply(self, line: str, transcript: list) -> None:
        transcript.append(["server", line])
        self.wfile.write((line + "\r\n").encode("ascii", "replace"))
        self.wfile.flush()

    def handle(self) -> None:
        transcript: list = []
        record = {
            "time": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "peer": "%s:%s" % self.client_address[:2],
            "ehlo": None,
            "auth": None,
            "mail_from": None,
            "rcpt_to": [],
            "data": None,
            "transcript": transcript,
        }
        try:
            if self.options.delay:
                time.sleep(self.options.delay)
            self.reply(f"{self.options.greeting} fake-smtp ready", transcript)
            if self.options.drop_after_greeting:
                return
            state = {"from": None, "rcpt": [], "auth_user": None}
            while True:
                raw = self.rfile.readline()
                if not raw:
                    transcript.append(["client", "<eof>"])
                    break
                line = raw.decode("latin-1").rstrip("\r\n")
                transcript.append(["client", line])
                verb, _, rest = line.partition(" ")
                verb = verb.upper()
                if verb in ("EHLO", "HELO"):
                    record["ehlo"] = rest
                    if verb == "EHLO":
                        features = ["250-fake-smtp", "250-SIZE 10485760", "250-8BITMIME"]
                        if self.options.auth != "none":
                            features.append("250-AUTH PLAIN LOGIN")
                        features.append("250 HELP")
                        for feature in features:
                            transcript.append(["server", feature])
                            self.wfile.write((feature + "\r\n").encode("ascii"))
                        self.wfile.flush()
                    else:
                        self.reply("250 fake-smtp", transcript)
                elif verb == "AUTH":
                    mechanism, _, initial = rest.partition(" ")
                    credential = self.read_auth(mechanism.upper(), initial, transcript)
                    if credential is None:
                        self.reply("535 5.7.8 authentication failed", transcript)
                        continue
                    user, password = credential
                    record["auth"] = user
                    if (
                        self.options.auth == "none"
                        or user != self.options.user
                        or password != self.options.password
                    ):
                        self.reply("535 5.7.8 authentication failed", transcript)
                    else:
                        state["auth_user"] = user
                        self.reply("235 2.7.0 authentication successful", transcript)
                elif verb == "MAIL":
                    if self.options.auth != "none" and state["auth_user"] is None:
                        self.reply("530 5.7.0 authentication required", transcript)
                        continue
                    if self.options.reject_sender:
                        self.reply("550 5.1.7 sender refused", transcript)
                        continue
                    address, params = self.split_envelope(rest)
                    state["from"] = address
                    record["mail_from"] = address
                    record["mail_params"] = params
                    self.reply("250 2.1.0 ok", transcript)
                elif verb == "RCPT":
                    address, params = self.split_envelope(rest)
                    if self.options.reject_recipient:
                        state["rcpt"].append(address)
                        self.reply("550 5.1.1 recipient refused", transcript)
                        continue
                    state["rcpt"].append(address)
                    record["rcpt_to"] = list(state["rcpt"])
                    record["rcpt_params"] = params
                    self.reply("250 2.1.5 ok", transcript)
                elif verb == "DATA":
                    self.reply("354 end data with <CR><LF>.<CR><LF>", transcript)
                    data = self.read_data(transcript)
                    if self.options.data_error:
                        self.reply("554 5.0.0 data refused", transcript)
                        continue
                    record["data"] = data
                    self.recorder.write(record)
                    self.reply("250 2.0.0 ok: queued", transcript)
                    # A second message on the same connection would append to
                    # the same record; Django opens one connection per send.
                elif verb == "RSET":
                    state = {"from": None, "rcpt": [], "auth_user": state["auth_user"]}
                    self.reply("250 2.0.0 ok", transcript)
                elif verb == "NOOP":
                    self.reply("250 2.0.0 ok", transcript)
                elif verb == "QUIT":
                    if self.options.quit_error:
                        self.reply("554 5.0.0 quit refused", transcript)
                    else:
                        self.reply("221 2.0.0 bye", transcript)
                    break
                else:
                    self.reply("500 5.5.2 command not recognized", transcript)
        except (BrokenPipeError, ConnectionResetError):
            transcript.append(["server", "<connection lost>"])
        finally:
            if record["data"] is None and (record["mail_from"] or transcript):
                # A failed transaction is worth recording too: the scenario
                # compares the reply that produced the endpoint's branch.
                self.recorder.write(record)

    def read_auth(self, mechanism: str, initial: str, transcript: list):
        if mechanism == "PLAIN":
            payload = initial or self.challenge("", transcript)
            try:
                decoded = base64.b64decode(payload + "===").decode("utf-8")
            except Exception:  # noqa: BLE001 - a malformed AUTH is a failure reply
                return None
            _, user, password = decoded.split("\x00")
            return user, password
        if mechanism == "LOGIN":
            payload = initial or self.challenge("VXNlcm5hbWU6", transcript)
            user = base64.b64decode(payload + "===").decode("utf-8")
            payload = self.challenge("UGFzc3dvcmQ6", transcript)
            password = base64.b64decode(payload + "===").decode("utf-8")
            return user, password
        self.reply("504 5.5.4 unrecognized authentication type", transcript)
        return None

    def challenge(self, prompt: str, transcript: list) -> str:
        transcript.append(["server", f"334 {prompt}"])
        self.wfile.write(f"334 {prompt}\r\n".encode("ascii"))
        self.wfile.flush()
        line = self.rfile.readline().decode("latin-1").rstrip("\r\n")
        transcript.append(["client", line])
        return line

    def read_data(self, transcript: list) -> str:
        """Read until the `.` terminator, undoing dot-stuffing."""
        lines = []
        while True:
            raw = self.rfile.readline()
            if not raw:
                break
            line = raw.decode("latin-1")
            if line in ("\r\n", "\n"):
                lines.append("")
                continue
            if line.rstrip("\r\n") == ".":
                break
            if line.startswith(".."):
                line = line[1:]
            lines.append(line.rstrip("\r\n"))
        transcript.append(["client", f"<data: {len(lines)} line(s)>"])
        # The CRLF before the terminating `.` is part of the end-of-data
        # sequence, not of the message (RFC 5321 §4.1.1.4).
        return "\r\n".join(lines)


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def serve(args: argparse.Namespace) -> int:
    Handler.options = Options(args)
    Handler.recorder = Recorder(pathlib.Path(args.output) if args.output else None)
    server = Server((args.bind, args.port), Handler)
    print(f"fake-smtp listening on {args.bind}:{args.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


def selftest(args: argparse.Namespace) -> int:
    """Send one message with the real `smtplib` and check the recording."""
    import smtplib
    import tempfile

    with tempfile.TemporaryDirectory() as directory:
        output = pathlib.Path(directory) / "mail.jsonl"
        args.port = 0
        args.output = str(output)
        Handler.options = Options(args)
        Handler.recorder = Recorder(output)
        server = Server((args.bind, 0), Handler)
        port = server.server_address[1]
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            connection = smtplib.SMTP("127.0.0.1", port, timeout=5)
            connection.sendmail(
                "Team Plane <team@mailer.plane.so>",
                ["receiver@example.com"],
                b"Subject: selftest\r\n\r\nbody",
            )
            connection.quit()
        finally:
            server.shutdown()
            server.server_close()
        records = [
            json.loads(line) for line in output.read_text(encoding="utf-8").splitlines()
        ]
    assert len(records) == 1, records
    record = records[0]
    assert record["mail_from"] == "<team@mailer.plane.so>", record
    assert record["mail_params"].startswith("size="), record
    assert record["rcpt_to"] == ["<receiver@example.com>"], record
    assert record["data"] == "Subject: selftest\r\n\r\nbody", record["data"]
    print("fake-smtp selftest: ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18410)
    parser.add_argument("--output", help="JSONL file to append transactions to")
    parser.add_argument(
        "--greeting",
        type=int,
        default=220,
        help="greeting reply code (554 produces SMTPConnectError in Django)",
    )
    parser.add_argument(
        "--auth", choices=("none", "required"), default="none", help="require AUTH"
    )
    parser.add_argument("--user", default="plane")
    parser.add_argument("--password", default="plane")
    parser.add_argument("--reject-sender", action="store_true")
    parser.add_argument("--reject-recipient", action="store_true")
    parser.add_argument("--data-error", action="store_true")
    parser.add_argument("--drop-after-greeting", action="store_true")
    parser.add_argument("--quit-error", action="store_true")
    parser.add_argument("--delay", type=float, default=0.0, help="seconds before greeting")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()
    return selftest(args) if args.selftest else serve(args)


if __name__ == "__main__":
    raise SystemExit(main())
