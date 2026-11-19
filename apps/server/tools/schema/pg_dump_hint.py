#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""The flag that fixes a failed `pg_dump`, chosen from its stderr.

`tools/schema/baseline.py` and `tools/schema/verify_migrator.py` dump through a
possibly-remote client (`docker exec <container> pg_dump`). Two failures have
different fixes and used to get the same advice:

* a **server version mismatch** (the host's pg_dump 14 against the stack's
  server 15) is fixed with `--pg-dump`;
* a **connection failure** (the in-container client was given the host port
  instead of the container-network port) is fixed with `--dump-url` /
  `--dump-url-template`, the URL handed to pg_dump.

``hint(stderr, dump_url_flag="--dump-url")`` returns the sentence to append,
or an empty string for a failure neither pattern explains (the caller still
prints pg_dump's own stderr in full).
"""

# pg_dump 14/15/16 print either the first or the second line for a mismatch;
# both contain "server version mismatch".
VERSION_MARKERS = (
    "server version mismatch",
    "aborting because of server version mismatch",
)

# libpq's connection errors, from pg_dump: refused/reset TCP, unresolvable host,
# timeout, or a missing socket. Authentication errors are connection errors for
# this purpose too: the URL, not the client binary, names the credentials.
CONNECTION_MARKERS = (
    "connection to server",
    "could not connect",
    "connection refused",
    "could not translate host name",
    "no route to host",
    "timeout expired",
    "no such file or directory",
    "password authentication failed",
)


def hint(stderr: str, *, dump_url_flag: str) -> str:
    """Return the actionable sentence for `stderr`, or `""` if unrecognised."""
    text = stderr.lower()
    if any(marker in text for marker in VERSION_MARKERS):
        return "Pass --pg-dump with a client whose major version matches the server."
    if any(marker in text for marker in CONNECTION_MARKERS):
        return (
            f"This is a connection failure, not a version mismatch. {dump_url_flag} is "
            "the URL pg_dump connects to; with a 'docker exec <container> pg_dump' "
            "client it must name the server on the container network (for the dev "
            "stack: ...:5432/<db>), not the host port."
        )
    return ""
