#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Prove `plane-migrator` equals Django, and that its guards refuse.

`plans/CUTOVER.md` §4, "Verify the dump":

1. **two databases, same empty state** — A is `manage.py migrate`, B is
   `plane-migrator`. `pg_dump --schema-only` must agree after normalising only
   the dump header and `search_path` lines, and `information_schema.columns`,
   `pg_indexes` and `pg_constraint` must agree row for row.
2. **the upgrade path** — a fresh clone of `plane_tpl` (a Django-migrated
   snapshot with rows): `plane-migrator` matches the baseline, adds only
   `plane_jobs`, and leaves `public` byte-identical.
3. **every refusal** — one missing migration row names Django's migrator; one
   extra row is a downgrade refusal; a non-empty `public` without
   `django_migrations` is refused; a rerun is idempotent.

    apps/api/.venv/bin/python tools/schema/verify_migrator.py \\
        --database-url postgresql://plane:plane@127.0.0.1:55432/plane \\
        --dump-url-template 'postgresql://plane:plane@127.0.0.1:5432/<db>' \\
        --pg-dump 'docker exec plane-rs-dev-postgres-1 pg_dump'

The script creates and drops exactly the four `t_p8boot_schema_*` databases (or
the names given with `--prefix`). Pass `--keep` to leave them for inspection.
"""

import argparse
import os
import shlex
import subprocess
import sys
from pathlib import Path

import psycopg

from pg_dump_hint import hint as pg_dump_hint

SERVER_DIR = Path(__file__).resolve().parent.parent.parent
REPO_ROOT = SERVER_DIR.parent.parent
API_DIR = REPO_ROOT / "apps" / "api"
MIGRATOR = SERVER_DIR / "target" / "debug" / "plane-migrator"


def url_with_database(url: str, database: str) -> str:
    base, _, _ = url.rpartition("/")
    return f"{base}/{database}"


def admin_connection(url: str, autocommit: bool = True):
    return psycopg.connect(url_with_database(url, "postgres"), autocommit=autocommit)


def drop_database(url: str, name: str) -> None:
    with admin_connection(url) as connection, connection.cursor() as cursor:
        cursor.execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity "
            "WHERE datname = %s AND pid <> pg_backend_pid()",
            (name,),
        )
        cursor.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


def create_database(url: str, name: str, template: str | None = None) -> None:
    drop_database(url, name)
    with admin_connection(url) as connection, connection.cursor() as cursor:
        if template:
            cursor.execute(f'CREATE DATABASE "{name}" TEMPLATE "{template}"')
        else:
            cursor.execute(f'CREATE DATABASE "{name}"')


def run_migrate(url: str, name: str) -> None:
    env = dict(os.environ)
    env["DATABASE_URL"] = url_with_database(url, name)
    env.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    env.setdefault("SECRET_KEY", "verify-migrator")
    env.setdefault("ENABLE_READ_REPLICA", "0")
    env.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    result = subprocess.run(
        [sys.executable, "manage.py", "migrate", "--noinput"],
        cwd=API_DIR,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"manage.py migrate failed:\n{result.stdout}")


def run_migrator(url: str, name: str) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env["DATABASE_URL"] = url_with_database(url, name)
    env.setdefault("SECRET_KEY", "verify-migrator")
    return subprocess.run(
        [str(MIGRATOR)],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def dump(url: str, name: str, pg_dump: str, template: str) -> str:
    dump_url = template.replace("<db>", name)
    command = shlex.split(pg_dump) + [
        "--schema-only",
        "--no-owner",
        "--no-privileges",
        "--schema=public",
        "--dbname",
        dump_url,
    ]
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if result.returncode != 0:
        detail = pg_dump_hint(result.stderr, dump_url_flag="--dump-url-template")
        raise SystemExit(
            f"pg_dump failed ({result.returncode}):\n{result.stderr}"
            + (f"{detail}\n" if detail else "")
        )
    return result.stdout


def normalise(dump_text: str) -> str:
    """Only the dump header and `search_path` lines, as CUTOVER.md §4 says.

    Both dumps come from the same `pg_dump` against the same server, so the two
    version lines and the `search_path` reset are the only known noise. Every
    structural line, including pg_dump's `-- Name:` comments, is compared.
    """
    kept = []
    for line in dump_text.splitlines():
        if line.startswith("-- Dumped from database version"):
            continue
        if line.startswith("-- Dumped by pg_dump version"):
            continue
        if line.startswith("SELECT pg_catalog.set_config('search_path'"):
            continue
        if line.startswith("SET search_path"):
            continue
        if line.startswith("\\restrict") or line.startswith("\\unrestrict"):
            continue
        kept.append(line)
    return "\n".join(kept)


def compare_dumps(a: str, b: str) -> bool:
    """Return True when equal; print a unified diff and the first difference."""
    if a == b:
        return True
    import difflib

    diff = list(
        difflib.unified_diff(
            a.splitlines(), b.splitlines(), fromfile="A django", tofile="B plane-migrator", n=1
        )
    )
    print("FAIL: dumps differ")
    print("\n".join(diff[:200]))
    return False


CATALOGUE_QUERIES = {
    "information_schema.columns": (
        "SELECT table_schema, table_name, column_name, data_type, udt_name, "
        "is_nullable, column_default, character_maximum_length, numeric_precision, "
        "numeric_scale, is_identity, identity_generation, is_generated "
        "FROM information_schema.columns WHERE table_schema = 'public' "
        "ORDER BY table_name, ordinal_position"
    ),
    "pg_indexes": (
        "SELECT schemaname, tablename, indexname, indexdef FROM pg_indexes "
        "WHERE schemaname = 'public' ORDER BY tablename, indexname"
    ),
    "pg_constraint": (
        "SELECT n.nspname, c.conname, c.contype, c.condeferrable, c.condeferred, "
        "pg_get_constraintdef(c.oid, true) FROM pg_constraint c "
        "JOIN pg_namespace n ON n.oid = c.connamespace "
        "WHERE n.nspname = 'public' ORDER BY c.conname"
    ),
    "sequences": (
        "SELECT schemaname, sequencename, data_type, start_value, min_value, max_value, "
        "increment_by, cycle FROM pg_sequences WHERE schemaname = 'public' "
        "ORDER BY sequencename"
    ),
}


def catalogue(url: str, name: str) -> dict[str, list[tuple]]:
    connection_url = url_with_database(url, name)
    result = {}
    with psycopg.connect(connection_url) as connection:
        with connection.cursor() as cursor:
            for key, query in CATALOGUE_QUERIES.items():
                cursor.execute(query)
                result[key] = cursor.fetchall()
    return result


def compare_catalogues(a: dict, b: dict) -> bool:
    ok = True
    for key in CATALOGUE_QUERIES:
        if a[key] == b[key]:
            print(f"PASS: {key} matches ({len(a[key])} rows)")
        else:
            ok = False
            print(f"FAIL: {key} differs ({len(a[key])} vs {len(b[key])} rows)")
            for left, right in zip(a[key], b[key]):
                if left != right:
                    print(f"  A: {left}\n  B: {right}")
                    break
    return ok


def marker(url: str, name: str) -> str | None:
    with psycopg.connect(url_with_database(url, name)) as connection:
        with connection.cursor() as cursor:
            cursor.execute(
                "SELECT value FROM plane_jobs.meta WHERE key = 'django_baseline'"
            )
            row = cursor.fetchone()
    return row[0] if row else None


def expect_refusal(label: str, result: subprocess.CompletedProcess, needle: str) -> bool:
    output = result.stdout
    if result.returncode == 0:
        print(f"FAIL: {label}: migrator exited 0")
        print(output)
        return False
    if needle not in output:
        print(f"FAIL: {label}: output does not name {needle!r}")
        print(output)
        return False
    print(f"PASS: {label} refused and named {needle!r}")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--database-url",
        default=os.environ.get(
            "DATABASE_URL", "postgresql://plane:plane@127.0.0.1:55432/plane"
        ),
    )
    parser.add_argument(
        "--dump-url-template",
        default="postgresql://plane:plane@127.0.0.1:5432/<db>",
        help="URL as seen by pg_dump; <db> is replaced",
    )
    parser.add_argument(
        "--pg-dump", default="pg_dump", help="pg_dump command (see tools/schema/baseline.py)"
    )
    parser.add_argument("--prefix", default="t_p8boot_schema")
    parser.add_argument("--template-db", default="plane_tpl")
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()

    if not MIGRATOR.exists():
        raise SystemExit(f"{MIGRATOR} does not exist; run `cargo build -p plane-migrator`")

    url = args.database_url
    fresh_django = f"{args.prefix}_a"
    fresh_rust = f"{args.prefix}_b"
    upgrade = f"{args.prefix}_upg"
    refusal = f"{args.prefix}_ref"
    ok = True
    try:
        print(f"== fresh Django: {fresh_django}")
        create_database(url, fresh_django)
        run_migrate(url, fresh_django)

        print(f"== fresh plane-migrator: {fresh_rust}")
        create_database(url, fresh_rust)
        first = run_migrator(url, fresh_rust)
        print(first.stdout.strip())
        if first.returncode != 0:
            print("FAIL: fresh install failed")
            return 1
        if "fresh install" not in first.stdout:
            print("FAIL: fresh install did not report a fresh install")
            ok = False
        second = run_migrator(url, fresh_rust)
        print(second.stdout.strip())
        if second.returncode != 0 or "already up to date" not in second.stdout:
            print("FAIL: rerun was not idempotent")
            ok = False
        else:
            print("PASS: rerun is idempotent")

        print("== dump and catalogue comparison")
        dump_a = normalise(dump(url, fresh_django, args.pg_dump, args.dump_url_template))
        dump_b = normalise(dump(url, fresh_rust, args.pg_dump, args.dump_url_template))
        ok = compare_dumps(dump_a, dump_b) and ok
        if dump_a == dump_b:
            print(f"PASS: pg_dump --schema-only is identical ({len(dump_a.splitlines())} lines)")
        ok = compare_catalogues(
            catalogue(url, fresh_django), catalogue(url, fresh_rust)
        ) and ok

        print(f"== upgrade path: {upgrade} cloned from {args.template_db}")
        create_database(url, upgrade, template=args.template_db)
        before = normalise(dump(url, upgrade, args.pg_dump, args.dump_url_template))
        result = run_migrator(url, upgrade)
        print(result.stdout.strip())
        if result.returncode != 0 or "baseline matched" not in result.stdout:
            print("FAIL: upgrade rejected a Django-migrated database")
            ok = False
        else:
            print("PASS: upgrade accepted a Django-migrated snapshot")
        after = normalise(dump(url, upgrade, args.pg_dump, args.dump_url_template))
        if before == after:
            print("PASS: upgrade left the public schema byte-identical")
        else:
            print("FAIL: upgrade changed the public schema")
            ok = compare_dumps(before, after) and ok
        expected = (
            "db:0125_reset_project_deploy_board_identity,"
            "license:0006_instance_is_current_version_deprecated"
        )
        if marker(url, upgrade) == expected:
            print("PASS: django_baseline marker is set")
        else:
            print(f"FAIL: marker is {marker(url, upgrade)!r}")
            ok = False

        print(f"== refusal: one missing migration row ({refusal})")
        create_database(url, refusal, template=args.template_db)
        with psycopg.connect(url_with_database(url, refusal)) as connection:
            with connection.cursor() as cursor:
                cursor.execute(
                    "DELETE FROM django_migrations "
                    "WHERE app = 'db' AND name = '0125_reset_project_deploy_board_identity'"
                )
        ok = expect_refusal(
            "older database", run_migrator(url, refusal), "docker-entrypoint-migrator.sh"
        ) and ok

        print(f"== refusal: one extra migration row ({refusal})")
        create_database(url, refusal, template=args.template_db)
        with psycopg.connect(url_with_database(url, refusal)) as connection:
            with connection.cursor() as cursor:
                cursor.execute(
                    "INSERT INTO django_migrations (app, name, applied) "
                    "VALUES ('db', '9999_future_migration', now())"
                )
        ok = expect_refusal(
            "newer database", run_migrator(url, refusal), "Refusing to downgrade"
        ) and ok

        print(f"== refusal: non-empty public without django_migrations ({refusal})")
        create_database(url, refusal)
        with psycopg.connect(url_with_database(url, refusal)) as connection:
            with connection.cursor() as cursor:
                cursor.execute("CREATE TABLE public.stray (id integer)")
        ok = expect_refusal(
            "non-empty public", run_migrator(url, refusal), "refusing to guess"
        ) and ok

        print(f"== refusal: older and newer than baseline use distinct messages")
        print(f"== marker after refusals: {marker(url, fresh_rust)}")
    finally:
        if args.keep:
            print(f"kept {fresh_django}, {fresh_rust}, {upgrade}, {refusal}")
        else:
            for name in (fresh_django, fresh_rust, upgrade, refusal):
                drop_database(url, name)

    if not ok:
        return 1
    print("PASS: plane-migrator equals Django and every guard refuses")
    return 0


if __name__ == "__main__":
    sys.exit(main())
