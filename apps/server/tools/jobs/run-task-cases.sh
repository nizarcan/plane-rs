#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run a list of `difftest task` cases and print one PASS/FAIL line per case.
#
# Usage:
#   tools/jobs/run-task-cases.sh [options]
#
# Options (all also settable from the environment):
#   --difftest PATH      difftest binary            (DIFFTEST)
#   --fixtures DIR       fixture directory          (FIXTURES, default: next to this script)
#   --list FILE          case list                  (TASK_CASES, default: tools/jobs/task-cases.txt)
#   --set c|d|a3|a1|a2|b|all  which set column to run (TASK_CASE_SET, default: all)
#   --a-db NAME          Django database            (DIFFTEST_A_DB, default: t_p8c_task_a)
#   --b-db NAME          Rust database              (DIFFTEST_B_DB, default: t_p8c_task_b)
#   --a-redis-db N       Django Redis index         (DIFFTEST_A_REDIS_DB, default: 5)
#   --b-redis-db N       Rust Redis index           (DIFFTEST_B_REDIS_DB, default: 6)
#   --vhost NAME         RabbitMQ vhost             (DIFFTEST_VHOST, default: p8c_task)
#   --s3-bucket-a NAME   Django bucket (p8*)        (DIFFTEST_S3_BUCKET_A)
#   --s3-bucket-b NAME   Rust bucket (p8*)          (DIFFTEST_S3_BUCKET_B)
#                        Any pair works: the harness replaces each side's own
#                        bucket name with <BUCKET> before comparing, so no case
#                        file hard-codes an agent's prefix.
#   --live-url URL       LIVE_BASE_URL; {{http}}    (DIFFTEST_LIVE_URL)
#                        expands to the case's fake HTTP server
#   --worker PATH        plane-worker binary        (DIFFTEST_WORKER)
#   --list-only          print the selected cases and exit
#
# The case list has one case per line, whitespace separated:
#
#   <set> <task> <kwargs file> <setup sql or -> <case file or -> [extra flags...]
#
# `#` starts a comment; blank lines are ignored. Every path is relative to
# `--fixtures`. Extra flags are passed to `difftest task` verbatim (they name
# behaviour the harness only knows after set D's merge, e.g.
# `--expect-django-failure`).
#
# The script never creates databases, buckets or vhosts itself: `difftest task`
# owns and removes them. Point it at resources you own (`t_<set>_*`, Redis DBs 5
# and 6, a `p8*` bucket pair). Exit status is 0 only when every selected case
# passed.
#
# A case with an `http` block (`set-c-crawl-link`, `set-c-copy-s3`, the webhook
# delivery cases) needs a worker built with the `difftest` feature, because the
# fake server is on loopback and the SSRF escape is compile-time only (D26).
# `difftest task` probes `plane-worker --difftest-feature` first and refuses the
# case with a build command in the message; a default-build worker cannot turn
# that case into a silent database diff.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

DIFFTEST="${DIFFTEST:-$SERVER_DIR/target/debug/difftest}"
FIXTURES="${FIXTURES:-$SCRIPT_DIR/fixtures}"
TASK_CASES="${TASK_CASES:-$SCRIPT_DIR/task-cases.txt}"
TASK_CASE_SET="${TASK_CASE_SET:-all}"
DIFFTEST_A_DB="${DIFFTEST_A_DB:-t_p8c_task_a}"
DIFFTEST_B_DB="${DIFFTEST_B_DB:-t_p8c_task_b}"
DIFFTEST_A_REDIS_DB="${DIFFTEST_A_REDIS_DB:-5}"
DIFFTEST_B_REDIS_DB="${DIFFTEST_B_REDIS_DB:-6}"
DIFFTEST_VHOST="${DIFFTEST_VHOST:-p8c_task}"
DIFFTEST_S3_BUCKET_A="${DIFFTEST_S3_BUCKET_A:-}"
DIFFTEST_S3_BUCKET_B="${DIFFTEST_S3_BUCKET_B:-}"
DIFFTEST_LIVE_URL="${DIFFTEST_LIVE_URL:-}"
DIFFTEST_WORKER="${DIFFTEST_WORKER:-}"
LIST_ONLY=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --difftest) DIFFTEST="$2"; shift 2 ;;
    --fixtures) FIXTURES="$2"; shift 2 ;;
    --list) TASK_CASES="$2"; shift 2 ;;
    --set) TASK_CASE_SET="$2"; shift 2 ;;
    --a-db) DIFFTEST_A_DB="$2"; shift 2 ;;
    --b-db) DIFFTEST_B_DB="$2"; shift 2 ;;
    --a-redis-db) DIFFTEST_A_REDIS_DB="$2"; shift 2 ;;
    --b-redis-db) DIFFTEST_B_REDIS_DB="$2"; shift 2 ;;
    --vhost) DIFFTEST_VHOST="$2"; shift 2 ;;
    --s3-bucket-a) DIFFTEST_S3_BUCKET_A="$2"; shift 2 ;;
    --s3-bucket-b) DIFFTEST_S3_BUCKET_B="$2"; shift 2 ;;
    --live-url) DIFFTEST_LIVE_URL="$2"; shift 2 ;;
    --worker) DIFFTEST_WORKER="$2"; shift 2 ;;
    --list-only) LIST_ONLY=1; shift ;;
    -h|--help) sed -n '5,45p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "error: unknown argument $1" >&2; exit 2 ;;
  esac
done

[[ -x "$DIFFTEST" ]] || { echo "error: difftest binary not executable: $DIFFTEST" >&2; exit 2; }
[[ -f "$TASK_CASES" ]] || { echo "error: case list not found: $TASK_CASES" >&2; exit 2; }
[[ -d "$FIXTURES" ]] || { echo "error: fixture directory not found: $FIXTURES" >&2; exit 2; }
case "$TASK_CASE_SET" in
  c|d|a3|a1|a2|b|all) ;;
  *) echo "error: --set must be c, d, a3, a1, a2, b or all" >&2; exit 2 ;;
esac

pass=0
fail=0
skipped=0
failed=()

run_case() {
  local set="$1" task="$2" kwargs="$3" setup="$4" case_file="$5"
  shift 5
  local extra=("$@")

  if [[ "$TASK_CASE_SET" != "all" && "$TASK_CASE_SET" != "$set" ]]; then
    return
  fi
  if (( LIST_ONLY )); then
    echo "set-$set $task $kwargs $setup $case_file ${extra[*]:-}"
    return
  fi

  local args=("$DIFFTEST" task --task "$task"
    --kwargs "$FIXTURES/$kwargs"
    --a-db "$DIFFTEST_A_DB" --b-db "$DIFFTEST_B_DB"
    --a-redis-db "$DIFFTEST_A_REDIS_DB" --b-redis-db "$DIFFTEST_B_REDIS_DB"
    --vhost "$DIFFTEST_VHOST")
  if [[ "$setup" != "-" ]]; then
    args+=(--setup "$FIXTURES/$setup")
  fi
  if [[ "$case_file" != "-" ]]; then
    args+=(--case "$FIXTURES/$case_file")
  fi
  if [[ -n "$DIFFTEST_S3_BUCKET_A" ]]; then
    args+=(--s3-bucket-a "$DIFFTEST_S3_BUCKET_A")
  fi
  if [[ -n "$DIFFTEST_S3_BUCKET_B" ]]; then
    args+=(--s3-bucket-b "$DIFFTEST_S3_BUCKET_B")
  fi
  if [[ -n "$DIFFTEST_LIVE_URL" ]]; then
    args+=(--live-url "$DIFFTEST_LIVE_URL")
  fi
  if [[ -n "$DIFFTEST_WORKER" ]]; then
    args+=(--worker "$DIFFTEST_WORKER")
  fi
  args+=("${extra[@]}")

  local missing=0
  for path in "$kwargs" "$setup" "$case_file"; do
    if [[ "$path" != "-" && ! -f "$FIXTURES/$path" ]]; then
      echo "FAIL  set-$set $task $kwargs: missing fixture $path"
      fail=$((fail + 1))
      failed+=("$task $kwargs (missing $path)")
      missing=1
      break
    fi
  done
  (( missing )) && return

  local output
  output=$("${args[@]}" 2>&1)
  local status=$?
  if (( status == 0 )); then
    pass=$((pass + 1))
    echo "PASS  set-$set $task $kwargs"
  else
    fail=$((fail + 1))
    failed+=("$task $kwargs")
    echo "FAIL  set-$set $task $kwargs"
    echo "$output" | tail -20 | sed 's/^/      /'
  fi
}

while IFS= read -r line || [[ -n "$line" ]]; do
  line="${line%%#*}"
  # shellcheck disable=SC2086
  set -- $line
  [[ $# -eq 0 ]] && continue
  if [[ $# -lt 5 ]]; then
    echo "error: malformed case line: $line" >&2
    exit 2
  fi
  run_case "$@"
done < "$TASK_CASES"

if (( LIST_ONLY )); then
  exit 0
fi

total=$((pass + fail))
echo "TOTAL pass=$pass fail=$fail (set=$TASK_CASE_SET)"
if (( fail > 0 )); then
  printf '  failed: %s\n' "${failed[@]}"
  exit 1
fi
exit 0
