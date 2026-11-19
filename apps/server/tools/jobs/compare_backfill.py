#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Compare the DB effect of Django's backfill chain and `plane-admin`.

Drops the columns that legitimately differ (the uuid4 primary key and the
three clock columns) and compares every other column as text, in a
deterministic order. Run after `tools/jobs/run_backfill_chain.py` on A and
`plane-admin` on B.

    tools/jobs/compare_backfill.py --table issue_versions \
        --a-db t_p8a3_bf_a --b-db t_p8a3_bf_b
"""

import argparse
import sys

import psycopg

IGNORED = {"id", "created_at", "updated_at", "last_saved_at"}


def rows(database, table):
    with psycopg.connect(
        host="127.0.0.1", port=55432, user="plane", password="plane", dbname=database
    ) as connection:
        with connection.cursor() as cursor:
            cursor.execute(f"SELECT * FROM {table}")
            columns = [column.name for column in cursor.description]
            keep = [index for index, name in enumerate(columns) if name not in IGNORED]
            values = [
                tuple("" if row[index] is None else str(row[index]) for index in keep)
                for row in cursor.fetchall()
            ]
    return [columns[index] for index in keep], sorted(values)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--table", required=True)
    parser.add_argument("--a-db", required=True)
    parser.add_argument("--b-db", required=True)
    args = parser.parse_args()

    columns_a, rows_a = rows(args.a_db, args.table)
    columns_b, rows_b = rows(args.b_db, args.table)
    if columns_a != columns_b:
        print(f"column sets differ: {columns_a} vs {columns_b}")
        return 1
    if rows_a != rows_b:
        print(f"FAIL: {args.table} differs ({len(rows_a)} vs {len(rows_b)} rows)")
        for index in range(max(len(rows_a), len(rows_b))):
            left = rows_a[index] if index < len(rows_a) else None
            right = rows_b[index] if index < len(rows_b) else None
            if left != right:
                print(f"  django: {dict(zip(columns_a, left)) if left else None}")
                print(f"  rust:   {dict(zip(columns_b, right)) if right else None}")
        return 1
    print(f"PASS: {args.table} has the same {len(rows_a)} rows on both sides")
    return 0


if __name__ == "__main__":
    sys.exit(main())
