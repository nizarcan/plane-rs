#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Record what else is connected to the shared dev Postgres while a benchmark
# runs. The VM's Postgres is used by several agents' differential runs at once;
# a benchmark's database queries compete with them, so a row's numbers are only
# comparable when this log shows no other `client backend`.
#
#   tools/bench/pg-load.sh <out.log> <stop-file> [interval-seconds]
#
# One line per sample: `<host epoch> <database>=<connections> ...` for every
# non-null client-backend database. `run-phase3.sh` starts it before the first
# measurement and removes the stop file after the last one.
set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

out="${1:-}"
stop="${2:-}"
interval="${3:-5}"
[[ -n "$out" && -n "$stop" ]] || die "usage: pg-load.sh <out.log> <stop-file> [interval]"

while [[ ! -f "$stop" ]]; do
  printf '%s ' "$("$PLANE_DJANGO_PYTHON" -c 'import time; print(f"{time.time():.6f}")')" >>"$out"
  psql_dev plane -Atc "
    SELECT string_agg(datname || '=' || n, ' ' ORDER BY datname)
      FROM (SELECT datname, count(*) AS n
              FROM pg_stat_activity
             WHERE backend_type = 'client backend' AND datname IS NOT NULL
             GROUP BY datname) AS s
  " >>"$out" 2>/dev/null || printf 'unavailable\n' >>"$out"
  sleep "$interval"
done
