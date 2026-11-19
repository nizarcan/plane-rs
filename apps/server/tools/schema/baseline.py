#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Generate or check `plane-migrator`'s Django baseline.

The baseline is the schema Django creates at the pinned upstream commit
(`plans/CUTOVER.md` §4): a fresh database migrated by the venv's Django, dumped
with

    pg_dump --schema-only --no-owner --no-privileges --schema=public

plus the (app, name) rows of `django_migrations` the same `migrate` inserted.
Two committed artefacts:

    apps/server/deploy/schema/baseline.sql
    apps/server/deploy/schema/django_migrations.txt

`plane-migrator` embeds both with `include_str!`; `--check` re-migrates a fresh
database and fails if either differs, so a Django migration added after the pin
(or a hand edit of the dump) is caught in CI.

Usage (from apps/server, with the API virtualenv's Python):

    # regenerate the committed files
    apps/api/.venv/bin/python tools/schema/baseline.py --write \\
        --database-url postgresql://plane:plane@127.0.0.1:55432/t_p8boot_schema \\
        --pg-dump "docker exec plane-rs-dev-postgres-1 pg_dump"

    # verify (CI)
    uv run --no-sync python ../server/tools/schema/baseline.py --check \\
        --database-url postgresql://plane:plane@127.0.0.1:5432/t_baseline_check \\
        --pg-dump "docker exec <postgres-container> pg_dump"

`pg_dump` must be the client of the server's major version: PostgreSQL refuses
a dump whose server is newer than the client, and the host's `pg_dump` 14 is
older than the stack's server 15. The default is `pg_dump` from `PATH`; the dev
stack and CI use the one inside the postgres container.

The database is created and dropped by exact name (default
`t_schema_baseline_check`); it is never cloned from a template and no other
database is touched. Use `--database` to pick another name, as concurrent agents
must.
"""

import argparse
import difflib
import os
import shlex
import subprocess
import sys
from pathlib import Path

import psycopg

from pg_dump_hint import hint as pg_dump_hint

SERVER_DIR = Path(__file__).resolve().parent.parent.parent
REPO_ROOT = SERVER_DIR.parent.parent

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()
BASELINE_SQL = SERVER_DIR / "deploy" / "schema" / "baseline.sql"
BASELINE_MIGRATIONS = SERVER_DIR / "deploy" / "schema" / "django_migrations.txt"

DEFAULT_DATABASE_NAME = "t_schema_baseline_check"
DEFAULT_DEV_URL = "postgresql://plane:plane@127.0.0.1:55432"


def database_url(args) -> str:
    if args.database_url:
        return args.database_url
    if os.environ.get("DATABASE_URL"):
        return os.environ["DATABASE_URL"]
    return f"{DEFAULT_DEV_URL}/{args.database}"


def server_url(url: str, database: str) -> str:
    """The same server and credentials, with `database` as the database name."""
    base, _, _ = url.rpartition("/")
    return f"{base}/{database}"


def admin_url(url: str) -> str:
    return server_url(url, "postgres")


def run(command, *, env=None, cwd=None, capture=True):
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        check=False,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        text=capture,
    )
    if result.returncode != 0:
        stderr = result.stderr if capture else ""
        raise SystemExit(
            f"command failed ({result.returncode}): {' '.join(command)}\n{stderr}"
        )
    return result.stdout if capture else None


def create_database(url: str, name: str) -> None:
    with psycopg.connect(admin_url(url), autocommit=True) as connection:
        with connection.cursor() as cursor:
            cursor.execute(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity "
                "WHERE datname = %s AND pid <> pg_backend_pid()",
                (name,),
            )
            cursor.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')
            cursor.execute(f'CREATE DATABASE "{name}"')


def drop_database(url: str, name: str) -> None:
    with psycopg.connect(admin_url(url), autocommit=True) as connection:
        with connection.cursor() as cursor:
            cursor.execute(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity "
                "WHERE datname = %s AND pid <> pg_backend_pid()",
                (name,),
            )
            cursor.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


def migrate(url: str) -> None:
    env = dict(os.environ)
    env["DATABASE_URL"] = url
    env.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    env.setdefault("SECRET_KEY", "baseline-generator")
    env.setdefault("ENABLE_READ_REPLICA", "0")
    env.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    env.setdefault("IS_TEST", "0")
    env.setdefault("DEBUG", "0")
    env.setdefault("PYTHONDONTWRITEBYTECODE", "1")
    run(
        [sys.executable, "manage.py", "migrate", "--noinput"],
        env=env,
        cwd=str(API_DIR),
        capture=False,
    )


def dump(url: str, pg_dump: str) -> bytes:
    command = shlex.split(pg_dump) + [
        "--schema-only",
        "--no-owner",
        "--no-privileges",
        "--schema=public",
        "--dbname",
        url,
    ]
    result = subprocess.run(
        command, check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if result.returncode != 0:
        stderr = result.stderr.decode("utf-8", "replace")
        detail = pg_dump_hint(stderr, dump_url_flag="--dump-url")
        raise SystemExit(
            f"pg_dump failed ({result.returncode}): {' '.join(command)}\n{stderr}"
            + (f"{detail}\n" if detail else "")
        )
    return result.stdout


def migrations(url: str) -> str:
    with psycopg.connect(url) as connection:
        with connection.cursor() as cursor:
            cursor.execute("SELECT app, name FROM django_migrations ORDER BY id")
            rows = cursor.fetchall()
    return "".join(f"{app}\t{name}\n" for app, name in rows)


def compare(name: str, expected: bytes, actual: bytes, path: Path) -> bool:
    if expected == actual:
        return True
    expected_text = expected.decode("utf-8", "replace").splitlines(keepends=True)
    actual_text = actual.decode("utf-8", "replace").splitlines(keepends=True)
    diff = "".join(
        difflib.unified_diff(
            expected_text,
            actual_text,
            fromfile=f"{name} (committed)",
            tofile=f"{name} (fresh migrate)",
            n=2,
        )
    )
    print(f"FAIL: {name} is stale ({path})")
    print(diff[:8000])
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true", help="compare against the committed baseline")
    group.add_argument("--write", action="store_true", help="regenerate the committed baseline")
    parser.add_argument("--database-url", help="URL of a database on the target server")
    parser.add_argument(
        "--database",
        default=DEFAULT_DATABASE_NAME,
        help=f"scratch database to create and drop (default {DEFAULT_DATABASE_NAME})",
    )
    parser.add_argument(
        "--pg-dump",
        default="pg_dump",
        help="pg_dump command, e.g. 'docker exec plane-rs-dev-postgres-1 pg_dump'",
    )
    parser.add_argument(
        "--dump-url",
        help="URL as seen by pg_dump (default: the scratch URL); with a "
        "'docker exec <container> pg_dump' client, name the server on the "
        "container network, e.g. postgresql://plane:plane@127.0.0.1:5432/<db>",
    )
    parser.add_argument("--keep", action="store_true", help="do not drop the scratch database")
    args = parser.parse_args()

    url = database_url(args)
    scratch = args.database
    create_database(url, scratch)
    scratch_url = server_url(url, scratch)
    try:
        migrate(scratch_url)
        dump_url = args.dump_url.replace("<db>", scratch) if args.dump_url else scratch_url
        dumped = dump(dump_url, args.pg_dump)
        applied = migrations(scratch_url)
    finally:
        if not args.keep:
            drop_database(url, scratch)

    if args.write:
        BASELINE_SQL.parent.mkdir(parents=True, exist_ok=True)
        BASELINE_SQL.write_bytes(dumped)
        BASELINE_MIGRATIONS.write_text(applied, encoding="utf-8")
        count = applied.count("\n")
        print(f"wrote {BASELINE_SQL} ({len(dumped)} bytes) and {BASELINE_MIGRATIONS} ({count} rows)")
        return 0

    ok = compare(
        "baseline.sql", BASELINE_SQL.read_bytes(), dumped, BASELINE_SQL
    )
    ok = compare(
        "django_migrations.txt",
        BASELINE_MIGRATIONS.read_bytes(),
        applied.encode(),
        BASELINE_MIGRATIONS,
    ) and ok
    if not ok:
        return 1
    print(
        f"PASS: baseline matches Django ({len(dumped)} bytes, {applied.count(chr(10))} migrations)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
