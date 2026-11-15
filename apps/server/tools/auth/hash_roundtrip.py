#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Phase 2 exit check: password hashes round-trip between Rust and Django.

Runs three comparisons against Django 5.2.17 in `apps/api/.venv`:

1. 100 random passwords hashed by Rust (`pbkdf2_sha256`) are verified with
   Django's `check_password`.
2. 100 passwords hashed by Django (`make_password`) are verified by Rust.
3. `must_update` agrees on hashes whose algorithm, iteration count or salt is
   outdated, and unusable passwords behave identically — in both directions.

The Rust side is `crates/plane-auth/examples/hash_roundtrip.rs`, built in
release mode. Passwords include non-ASCII (Latin-1, CJK, astral emoji) because
Django hashes the UTF-8 encoding of the `str`, and the JSON/session paths are
where an encoding mistake would show up.

    apps/server/tools/auth/hash_roundtrip.py

Exit code 0 only when every count matches.
"""

from __future__ import annotations

import importlib.util
import json
import os
import random
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve()
SERVER_DIR = HERE.parents[2]
REPO_ROOT = SERVER_DIR.parents[1]
API_DIR = REPO_ROOT / "apps" / "api"
VENV_PYTHON = API_DIR / ".venv" / "bin" / "python"
COUNT = 100

# Django lives in apps/api/.venv. Re-exec with that interpreter when this was
# started with the system python3, so `tools/auth/hash_roundtrip.py` works with
# either one.
if importlib.util.find_spec("django") is None and VENV_PYTHON.exists():
    os.execv(str(VENV_PYTHON), [str(VENV_PYTHON), str(HERE), *sys.argv[1:]])

# Django needs a settings module, but this script never touches a database or
# Redis: the URLs point at a closed port on purpose (the same trick the
# contract probes in tools/contract use).
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("SECRET_KEY", "hash-roundtrip-check-not-for-deployment")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
os.environ.pop("SCOUT_MONITOR", None)
os.environ.pop("SCOUT_KEY", None)
sys.path.insert(0, str(API_DIR))

import django  # noqa: E402

django.setup()

from django.contrib.auth.hashers import (  # noqa: E402
    PBKDF2PasswordHasher,
    PBKDF2SHA1PasswordHasher,
    check_password as django_check_password,
    is_password_usable as django_is_usable,
    make_password as django_make_password,
    verify_password as django_verify_password,
)

# The same character classes the Rust example uses: ASCII, Latin-1, CJK,
# astral emoji, whitespace, quotes and symbols.
ALPHABET = (
    "abZ09!@#$%^&*()-_=+[]{};:,.<>/ ?|~`'\"\\ "
    "éüñßøåç—€日本語あ🚀🎉𝄞👩‍🔐"
)


def rust(*args: str) -> list[str]:
    """Run the Rust example and return its stdout lines."""
    command = [
        "cargo",
        "run",
        "--quiet",
        "--release",
        "-p",
        "plane-auth",
        "--example",
        "hash_roundtrip",
        "--",
        *args,
    ]
    result = subprocess.run(
        command,
        cwd=SERVER_DIR,
        env={**os.environ, "CARGO_TARGET_DIR": str(SERVER_DIR / "target" / "auth")},
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit(f"cargo run failed with {result.returncode}")
    return [line for line in result.stdout.splitlines() if line.strip()]


def section(title: str) -> None:
    print(f"\n== {title}")


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def random_passwords(count: int, seed: int) -> list[str]:
    rng = random.Random(seed)
    return ["".join(rng.choice(ALPHABET) for _ in range(rng.randint(1, 40))) for _ in range(count)]


def rust_to_django() -> tuple[int, float]:
    section(f"Rust -> Django: {COUNT} Rust hashes verified by Django")
    lines = rust("gen", "--count", str(COUNT), "--seed", "7")
    if len(lines) != COUNT:
        fail(f"Rust generated {len(lines)} hashes, expected {COUNT}")
    started = time.perf_counter()
    correct = 0
    for index, line in enumerate(lines):
        entry = json.loads(line)
        if django_check_password(entry["password"], entry["hash"]):
            correct += 1
        else:
            fail(f"Rust hash {index} does not verify in Django")
    elapsed = time.perf_counter() - started
    print(f"rust->django: {correct}/{COUNT} verified")
    return correct, elapsed


def django_to_rust() -> int:
    section(f"Django -> Rust: {COUNT} Django hashes verified by Rust")
    entries = []
    for password in random_passwords(COUNT, seed=11):
        entries.append({"password": password, "hash": django_make_password(password)})
    with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as handle:
        path = handle.name
        for entry in entries:
            handle.write(json.dumps(entry) + "\n")
    try:
        output = rust("verify", "--input", path)
    finally:
        os.unlink(path)
    if len(output) != 1:
        fail(f"Rust verify printed {len(output)} lines")
    summary = json.loads(output[0])
    if summary["errors"]:
        fail(f"Rust verify errors: {summary['errors'][:3]}")
    if summary["correct"] != COUNT:
        fail(f"Rust verified {summary['correct']}/{COUNT} Django hashes")
    print(f"django->rust: {summary['correct']}/{COUNT} verified")
    return summary["correct"]


def rust_fixtures_checked_by_django() -> dict[str, int]:
    section("must_update / unusable: Rust fixtures judged by Django")
    lines = rust("fixtures")
    counts = {"correct": 0, "must_update": 0, "unusable": 0, "checked": 0}
    for line in lines:
        entry = json.loads(line)
        password, encoded = entry["password"], entry["hash"]
        is_correct, must_update = django_verify_password(password, encoded)
        if is_correct != entry["expect_correct"]:
            fail(f"{entry['kind']}: Django says is_correct={is_correct}")
        if must_update != entry["expect_must_update"]:
            fail(f"{entry['kind']}: Django says must_update={must_update}")
        counts["checked"] += 1
        counts["correct"] += int(is_correct)
        counts["must_update"] += int(must_update)
        counts["unusable"] += int(not django_is_usable(encoded))
    print(
        f"django judged {counts['checked']} Rust fixtures: "
        f"{counts['correct']} correct, {counts['must_update']} must_update, "
        f"{counts['unusable']} unusable"
    )
    return counts


def django_fixtures_checked_by_rust() -> dict[str, int]:
    section("must_update / unusable: Django fixtures judged by Rust")
    hasher = PBKDF2PasswordHasher()
    sha1 = PBKDF2SHA1PasswordHasher()
    password = "Fixture-Passw0rd-2026!"
    salt = "0123456789abcdefghijkl"
    fixtures = [
        # Django encodes with a different iteration count.
        {
            "kind": "old_iterations",
            "hash": hasher.encode(password, salt, iterations=600_000),
            "expect_correct": True,
            "expect_must_update": True,
            "check_must_update": True,
        },
        {
            "kind": "pbkdf2_sha1",
            "hash": sha1.encode(password, salt),
            "expect_correct": True,
            "expect_must_update": True,
            "check_must_update": True,
        },
        {
            "kind": "fresh",
            "hash": hasher.encode(password, salt),
            "expect_correct": True,
            "expect_must_update": False,
            "check_must_update": True,
        },
        {
            "kind": "unusable",
            "hash": django_make_password(None),
            "expect_correct": False,
            "expect_must_update": False,
            "check_must_update": True,
        },
        {
            "kind": "unknown_algorithm",
            "hash": f"md5${salt}$0123456789abcdef0123456789abcdef",
            "expect_correct": False,
            "expect_must_update": False,
            "check_must_update": True,
        },
    ]
    with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as handle:
        path = handle.name
        for fixture in fixtures:
            handle.write(json.dumps({"password": password, **fixture}) + "\n")
    try:
        output = rust("verify", "--input", path)
    finally:
        os.unlink(path)
    summary = json.loads(output[0])
    if summary["errors"]:
        fail(f"Rust disagreed with Django's fixtures: {summary['errors']}")
    print(
        f"rust judged {summary['checked']} Django fixtures: "
        f"{summary['correct']} correct, {summary['must_update_true']} must_update, "
        f"{summary['unusable']} unusable"
    )
    return summary


def main() -> int:
    print(f"Django {django.get_version()} at {API_DIR}")
    rust_correct, django_elapsed = rust_to_django()
    django_correct = django_to_rust()
    rust_fixtures = rust_fixtures_checked_by_django()
    django_fixtures = django_fixtures_checked_by_rust()

    section("summary")
    print(f"rust -> django:   {rust_correct}/{COUNT} verified ({django_elapsed:.1f} s in Django)")
    print(f"django -> rust:   {django_correct}/{COUNT} verified")
    print(
        "must_update:      "
        f"old-iteration {'ok' if rust_fixtures['must_update'] >= 2 else 'FAIL'}, "
        f"pbkdf2_sha1 {'ok' if django_fixtures['must_update_true'] == 2 else 'FAIL'}"
    )
    print(
        "unusable:         "
        f"{rust_fixtures['unusable']} Rust fixture(s), {django_fixtures['unusable']} Django fixture(s)"
    )
    if rust_correct != COUNT or django_correct != COUNT:
        fail("hash round-trip counts do not match")
    print("all hash round-trip checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
