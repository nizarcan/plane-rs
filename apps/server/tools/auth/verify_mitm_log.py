#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Assert that every provider call of an OAuth scenario was intercepted.

`oauth_https_proxy.py` writes one JSON object per line for every connection and
request it sees. This tool compares that log with the scenario's expected call
inventory (`scenarios/write/<name>.calls.json`):

* no `blocked` line, and no request to a host outside the proxy's allowlist --
  a request that tried to leave the machine;
* the exact number of calls per `METHOD host path` -- a provider call that
  never reached the proxy means the target bypassed it (it would also have
  failed TLS verification, because the targets trust only the generated CA, but
  a missing call must fail the wrapper even if the scenario still produced a
  matching error redirect);
* no unexpected call.

    apps/api/.venv/bin/python tools/auth/verify_mitm_log.py \
        --log target/oauth-mitm/requests.jsonl \
        --expected scenarios/write/auth-oauth-google-github.calls.json
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

ALLOWED_HOSTS = frozenset(
    {
        "oauth2.googleapis.com",
        "www.googleapis.com",
        "accounts.google.com",
        "github.com",
        "api.github.com",
    }
)


def load_log(path):
    records = []
    with open(path, encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            line = line.strip()
            if not line:
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as error:
                raise SystemExit(f"{path}:{line_number}: not JSON: {error}") from error
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True)
    parser.add_argument("--expected", required=True)
    args = parser.parse_args()

    log_path = Path(args.log)
    if not log_path.is_file():
        raise SystemExit(f"verify-mitm: no proxy log at {log_path}")
    records = load_log(log_path)
    expected = json.loads(Path(args.expected).read_text(encoding="utf-8"))

    failures = []

    blocked = [record for record in records if record.get("kind") == "blocked"]
    for record in blocked:
        failures.append(
            f"blocked {record.get('method')} {record.get('host')}{record.get('path')}: "
            "a request tried to leave the machine"
        )
    for record in records:
        if record.get("kind") == "request" and record.get("host") not in ALLOWED_HOSTS:
            failures.append(f"request to a non-provider host: {record}")
        if record.get("kind") == "tls-error":
            failures.append(f"TLS handshake failed for {record.get('host')}: {record.get('error')}")

    observed = Counter(
        (record["method"], record["host"], record["path"])
        for record in records
        if record.get("kind") == "request"
    )
    wanted = Counter(
        (entry["method"], entry["host"], entry["path"]) for entry in expected
        for _ in range(entry["count"])
    )
    for key, count in sorted(wanted.items()):
        actual = observed.get(key, 0)
        if actual != count:
            failures.append(
                f"{key[0]} {key[1]}{key[2]}: expected {count} call(s), saw {actual}"
            )
    for key, count in sorted(observed.items()):
        if key not in wanted:
            failures.append(f"unexpected {key[0]} {key[1]}{key[2]}: {count} call(s)")

    print(f"oauth-mitm: {sum(observed.values())} provider request(s) intercepted")
    for (method, host, path), count in sorted(observed.items()):
        print(f"  {count:>2}x {method:<4} {host}{path}")

    if failures:
        print("\nverify-mitm: FAILED", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("verify-mitm: every expected provider call was intercepted")
    return 0


if __name__ == "__main__":
    sys.exit(main())
