#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Phase 4 exit check: two plane-beat replicas fire every test tick exactly once,
# and the survivor keeps firing after the other is killed.
#
#   apps/server/tools/jobs/beat_exactly_once.sh
#
# Creates and drops its own database (t_rsjobs_beat) and uses Redis DB 5 and
# the shared dev RabbitMQ `plane` vhost only for configuration; no Celery
# message is published by this script.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER="$(cd "$HERE/../.." && pwd)"
cd "$SERVER"

PG_URL="${PLANE_JOBS_PG_URL:-postgresql://plane:plane@127.0.0.1:55432}"
TEST_DB="t_rsjobs_beat"
KEEP="${KEEP:-0}"
TOTAL_BEFORE_KILL="${TOTAL_BEFORE_KILL:-50}"
AFTER_KILL="${AFTER_KILL:-10}"
BIN="$SERVER/target/debug"

log() { printf '==> %s\n' "$*"; }

cleanup() {
  local status=$?
  for pid in "${WORKER_PID:-}" "${BEAT_B_PID:-}" "${BEAT_A_PID:-}"; do
    [[ -n "$pid" ]] && kill -TERM "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  if [[ "$KEEP" != "1" ]]; then
    psql "$PG_URL/postgres" -q -c "DROP DATABASE IF EXISTS \"$TEST_DB\" WITH (FORCE)" >/dev/null 2>&1 || true
  else
    log "kept database $TEST_DB (KEEP=1)"
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

log "building plane-migrator, plane-worker and plane-beat"
cargo build -q -p plane-migrator -p plane-worker -p plane-beat

log "creating $TEST_DB from plane_fix_tpl"
psql "$PG_URL/postgres" -q -c "DROP DATABASE IF EXISTS \"$TEST_DB\" WITH (FORCE)" >/dev/null
psql "$PG_URL/postgres" -q -c "CREATE DATABASE \"$TEST_DB\" TEMPLATE plane_fix_tpl" >/dev/null

export PLANE_DB="$TEST_DB"
export PLANE_REDIS_DB=3
export PLANE_MQ_VHOST=plane
export PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS=2
export PLANE_RS_BEAT_POLL_MS=100
export PLANE_RS_JOB_POLL_MS=100
export PLANE_RS_JOB_CONCURRENCY=2
export PLANE_RS_JOB_DRAIN_SECONDS=5
set -a
# shellcheck source=../..//dev/django-env.sh
source "$SERVER/dev/django-env.sh"
set +a

log "migrating plane_jobs"
"$BIN/plane-migrator"

log "starting two beat replicas and one worker"
"$BIN/plane-beat" >"$SERVER/target/jobs-beat-a.log" 2>&1 &
BEAT_A_PID=$!
"$BIN/plane-beat" >"$SERVER/target/jobs-beat-b.log" 2>&1 &
BEAT_B_PID=$!
"$BIN/plane-worker" >"$SERVER/target/jobs-worker.log" 2>&1 &
WORKER_PID=$!

sleep 2
tick_count() {
  psql "$PG_URL/$TEST_DB" -tA -c "SELECT count(*) FROM plane_jobs.ticks WHERE schedule_name = 'plane.jobs.test-tick'"
}

log "letting both replicas run for ${TOTAL_BEFORE_KILL}s"
sleep "$TOTAL_BEFORE_KILL"
BEFORE=$(tick_count)

log "killing replica A mid-run"
kill -TERM "$BEAT_A_PID"
wait "$BEAT_A_PID" 2>/dev/null || true
BEAT_A_PID=""
STOPPED=$(tick_count)

log "letting replica B continue for ${AFTER_KILL}s"
sleep "$AFTER_KILL"
AFTER=$(tick_count)

TICKS=$(psql "$PG_URL/$TEST_DB" -tA -c "SELECT count(*) FROM plane_jobs.ticks")
JOBS=$(psql "$PG_URL/$TEST_DB" -tA -c "SELECT count(*) FROM plane_jobs.jobs")
DONE=$(psql "$PG_URL/$TEST_DB" -tA -c "SELECT count(*) FROM plane_jobs.jobs WHERE state = 'done'")
DUPES=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT count(*) FROM (SELECT schedule_name, tick_at FROM plane_jobs.ticks GROUP BY 1, 2 HAVING count(*) > 1) d")
GAPS=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT count(*) FROM (SELECT tick_at, lag(tick_at) OVER (ORDER BY tick_at) AS prev FROM plane_jobs.ticks) g WHERE prev IS NOT NULL AND tick_at - prev > interval '3 seconds'")

log "ticks: total=$TICKS before-kill=$BEFORE stopped=$STOPPED after-kill=$AFTER"
log "jobs: total=$JOBS done=$DONE duplicate-ticks=$DUPES gaps=$GAPS"

[[ "$BEFORE" -ge 10 ]] || { echo "FAIL: too few ticks before the kill ($BEFORE)" >&2; exit 1; }
[[ "$AFTER" -gt "$STOPPED" ]] || { echo "FAIL: survivor stopped firing ($STOPPED -> $AFTER)" >&2; exit 1; }
[[ "$DUPES" == "0" ]] || { echo "FAIL: duplicate ticks" >&2; exit 1; }
[[ "$GAPS" == "0" ]] || { echo "FAIL: gap between ticks > 3s" >&2; exit 1; }
[[ "$JOBS" == "$TICKS" ]] || { echo "FAIL: ticks and jobs differ ($TICKS vs $JOBS)" >&2; exit 1; }
[[ "$DONE" == "$TICKS" ]] || { echo "FAIL: not every tick was processed ($DONE/$TICKS done)" >&2; exit 1; }

log "PASS: exactly one worker run per tick, two replicas, survivor continued"
exit 0
