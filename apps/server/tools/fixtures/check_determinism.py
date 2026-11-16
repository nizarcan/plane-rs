#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prove the read fixture is deterministic.

Builds `tools/fixtures/build.py` twice into two scratch clones of `plane_tpl`
with the **same** `--base-time`, then compares every table in `public` row by
row. Row counts are printed for every table; any difference is printed with the
offending column and the first few mismatching keys.

A column may be declared volatile with `--ignore table.column`; the report
lists what was ignored and why. Nothing is ignored by default: the fixture
pins passwords, tokens, session signatures and every timestamp, so a clean
comparison is the expected result.

Usage (the environment is the one the dev scripts use):

    eval "$(apps/server/dev/django-env.sh)"
    apps/api/.venv/bin/python apps/server/tools/fixtures/check_determinism.py

Exit code 0 only when the two builds match (and both were built with the same
base time).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.parse
from datetime import datetime, timezone as dt_timezone
from pathlib import Path

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
REPO_ROOT = SCRIPT.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"
BUILD_SCRIPT = SERVER_DIR / "tools" / "fixtures" / "build.py"
BASE_TEMPLATE = "plane_tpl"


def split_url(url: str) -> dict:
    parts = urllib.parse.urlparse(url)
    return {
        "host": parts.hostname or "127.0.0.1",
        "port": parts.port or 5432,
        "user": urllib.parse.unquote(parts.username or "plane"),
        "password": urllib.parse.unquote(parts.password or ""),
        "database": urllib.parse.unquote(parts.path.lstrip("/") or "postgres"),
    }


def dsn(info: dict, database: str | None = None) -> str:
    return (
        f"host={info['host']} port={info['port']} user={info['user']} "
        f"password={info['password']} dbname={database or info['database']}"
    )


def admin_connection(info: dict):
    import psycopg

    return psycopg.connect(dsn(info, "postgres"), autocommit=True)


def create_database(admin, name: str) -> None:
    with admin.cursor() as cursor:
        cursor.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')
        cursor.execute(f'CREATE DATABASE "{name}" TEMPLATE "{BASE_TEMPLATE}"')


def drop_database(admin, name: str) -> None:
    with admin.cursor() as cursor:
        cursor.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


def database_url(base: str, name: str) -> str:
    parts = urllib.parse.urlparse(base)
    return urllib.parse.urlunparse(parts._replace(path="/" + name))


def build(name: str, base_time: str, url_template: str) -> None:
    env = dict(os.environ)
    env["DATABASE_URL"] = database_url(url_template, name)
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    result = subprocess.run(
        [
            sys.executable,
            str(BUILD_SCRIPT),
            "--database",
            name,
            "--base-time",
            base_time,
            "--no-manifest",
        ],
        cwd=str(API_DIR),
        env=env,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"build into {name} failed:\n{result.stderr[-4000:]}")


def list_tables(info: dict, database: str) -> list[str]:
    import psycopg

    with psycopg.connect(dsn(info, database)) as connection:
        with connection.cursor() as cursor:
            cursor.execute(
                "SELECT table_name FROM information_schema.tables "
                "WHERE table_schema = 'public' AND table_type = 'BASE TABLE' "
                "ORDER BY table_name"
            )
            return [row[0] for row in cursor.fetchall()]


def table_rows(info: dict, database: str, table: str, ignore: set[str]) -> list[dict]:
    """Every row as a dict, in a stable order.

    Ordering by the whole row (as text) makes the comparison independent of the
    physical order, which is what a rebuilt table may change.
    """
    import psycopg
    from psycopg.rows import dict_row

    with psycopg.connect(dsn(info, database), row_factory=dict_row) as connection:
        with connection.cursor() as cursor:
            cursor.execute(f'SELECT * FROM "{table}"')
            rows = cursor.fetchall()
    for value in (row for row in rows):
        for column in list(value):
            if f"{table}.{column}" in ignore:
                del value[column]
            else:
                value[column] = normalise(value[column])
    return sorted(rows, key=lambda row: json.dumps(row, sort_keys=True, default=str))


def normalise(value):
    if isinstance(value, bytes):
        return value.hex()
    if isinstance(value, datetime):
        return value.astimezone(dt_timezone.utc).isoformat()
    if isinstance(value, (dict, list)):
        return value
    return value


def describe_difference(left: list[dict], right: list[dict], table: str) -> list[str]:
    lines: list[str] = []
    if len(left) != len(right):
        lines.append(f"  row count differs: {len(left)} vs {len(right)}")
    left_keys = [json.dumps(row, sort_keys=True, default=str) for row in left]
    right_keys = [json.dumps(row, sort_keys=True, default=str) for row in right]
    only_left = sorted(set(left_keys) - set(right_keys))
    only_right = sorted(set(right_keys) - set(left_keys))
    for text in only_left[:3]:
        lines.append(f"  only in A: {text[:400]}")
    for text in only_right[:3]:
        lines.append(f"  only in B: {text[:400]}")
    # Column-level attribution for the first mismatching pair.
    left_by_id = {row.get("id", row.get("session_key", row.get("id"))): row for row in left}
    right_by_id = {row.get("id", row.get("session_key", row.get("id"))): row for row in right}
    for key in sorted(set(left_by_id) & set(right_by_id), key=str):
        a, b = left_by_id[key], right_by_id[key]
        if a == b:
            continue
        for column in sorted(set(a) | set(b)):
            if a.get(column) != b.get(column):
                lines.append(f"  {table}.{column} on {key}: {a.get(column)!r} vs {b.get(column)!r}")
        break
    return lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base-time", default=None, help="base time both builds use (default: now, truncated to the hour)")
    parser.add_argument("--a", default="t_fixdet_a", help="first scratch database (default t_fixdet_a)")
    parser.add_argument("--b", default="t_fixdet_b", help="second scratch database (default t_fixdet_b)")
    parser.add_argument(
        "--ignore",
        action="append",
        default=[],
        metavar="TABLE.COLUMN",
        help="column to exclude from the comparison (repeatable; record why in the fixture docstring)",
    )
    parser.add_argument("--json", type=Path, default=None, help="write the per-table report as JSON")
    parser.add_argument("--keep", action="store_true", help="keep the scratch databases")
    args = parser.parse_args()

    url = os.environ.get("DATABASE_URL")
    if not url:
        raise SystemExit(
            "DATABASE_URL is not set; run under the dev environment:\n"
            '  eval "$(apps/server/dev/django-env.sh)"'
        )
    info = split_url(url)
    base_time = args.base_time or datetime.now(dt_timezone.utc).replace(minute=0, second=0, microsecond=0).isoformat()
    base_time = base_time.replace("+00:00", "Z")
    ignore = set(args.ignore)

    admin = admin_connection(info)
    try:
        if not _template_exists(admin):
            raise SystemExit(f"{BASE_TEMPLATE} does not exist; run apps/server/dev/bootstrap.sh first")
        print(f"building {args.a} at {base_time}", file=sys.stderr)
        create_database(admin, args.a)
        build(args.a, base_time, url)
        print(f"building {args.b} at {base_time}", file=sys.stderr)
        create_database(admin, args.b)
        build(args.b, base_time, url)

        tables = list_tables(info, args.a)
        report: dict = {"base_time": base_time, "ignored": sorted(ignore), "tables": {}, "ok": True}
        failures: list[str] = []
        for table in tables:
            left = table_rows(info, args.a, table, ignore)
            right = table_rows(info, args.b, table, ignore)
            differ = left != right
            report["tables"][table] = {"rows_a": len(left), "rows_b": len(right), "equal": not differ}
            print(f"{table:42s} {len(left):7d} {'ok' if not differ else 'DIFF'}")
            if differ:
                report["ok"] = False
                failures.append(table)
                for line in describe_difference(left, right, table):
                    print(line)
        if args.json:
            args.json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            print(f"wrote {args.json}", file=sys.stderr)
        total = sum(entry["rows_a"] for entry in report["tables"].values())
        print(
            f"\n{len(tables)} tables, {total} rows; "
            + ("deterministic" if report["ok"] else f"{len(failures)} table(s) differ: {', '.join(failures)}")
        )
        return 0 if report["ok"] else 1
    finally:
        if not args.keep:
            drop_database(admin, args.a)
            drop_database(admin, args.b)
        admin.close()


def _template_exists(admin) -> bool:
    with admin.cursor() as cursor:
        cursor.execute("SELECT 1 FROM pg_database WHERE datname = %s", [BASE_TEMPLATE])
        return cursor.fetchone() is not None


if __name__ == "__main__":
    raise SystemExit(main())
