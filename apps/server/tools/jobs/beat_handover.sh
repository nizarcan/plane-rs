#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Phase 4 handover proof against a real Celery beat (DatabaseScheduler):
#
#   apps/server/tools/jobs/beat_handover.sh
#
# Creates t_rsjobs_handover and the rsjobs_handover vhost, inserts a 10 s
# PeriodicTask through Django's ORM (so the post_save tracker bump happens),
# and starts the real Celery beat. The handover request is deliberately made
# 1 s before a tick, so the quiet-window path has to wait. It then counts each
# 10 s tick window from start to finish and asserts Celery + plane-beat fired
# exactly one job per window, with none fired by both or neither.
# Under two minutes; removes the database and vhost on exit unless KEEP=1.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER="$(cd "$HERE/../.." && pwd)"
REPO="$(cd "$SERVER/../.." && pwd)"
cd "$SERVER"

# The side that still runs Django (the Celery beat) is the oracle at the
# UPSTREAM.toml pin, not a tree in this checkout.
# shellcheck source=../upstream/oracle.sh
source "$SERVER/tools/upstream/oracle.sh"
API_DIR="$(oracle_api_dir)"
API_PYTHON="$(oracle_python)"

PG_URL="${PLANE_JOBS_PG_URL:-postgresql://plane:plane@127.0.0.1:55432}"
RABBIT_HTTP="${PLANE_JOBS_RABBIT_HTTP:-http://127.0.0.1:55673}"
TEST_DB="t_rsjobs_handover"
VHOST="rsjobs_handover"
TASK="plane.jobs.handover_probe"
PROBE_NAME="rsjobs-handover-probe"
INTERVAL=10
GAP=3
MARGIN=3
TICKS=5
BIN="$SERVER/target/debug"
KEEP="${KEEP:-0}"

log() { printf '==> %s\n' "$*"; }

cleanup() {
  local status=$?
  for pid in "${BEAT_PID:-}" "${CELERY_PID:-}"; do
    [[ -n "$pid" ]] && kill -TERM "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  if [[ "$KEEP" != "1" ]]; then
    psql "$PG_URL/postgres" -q -c "DROP DATABASE IF EXISTS \"$TEST_DB\" WITH (FORCE)" >/dev/null 2>&1 || true
    /usr/bin/curl -s -u plane:plane -X DELETE "$RABBIT_HTTP/api/vhosts/$VHOST" -o /dev/null || true
  else
    log "kept database $TEST_DB and vhost $VHOST (KEEP=1)"
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

log "building plane-migrator and plane-beat"
cargo build -q -p plane-migrator -p plane-beat

log "creating $TEST_DB and vhost $VHOST"
psql "$PG_URL/postgres" -q -c "DROP DATABASE IF EXISTS \"$TEST_DB\" WITH (FORCE)" >/dev/null
psql "$PG_URL/postgres" -q -c "CREATE DATABASE \"$TEST_DB\" TEMPLATE plane_fix_tpl" >/dev/null
/usr/bin/curl -s -u plane:plane -X PUT "$RABBIT_HTTP/api/vhosts/$VHOST" \
  -H 'content-type: application/json' -d '{}' -o /dev/null -w ''

export PLANE_DB="$TEST_DB"
export PLANE_REDIS_DB=5
export PLANE_MQ_VHOST="$VHOST"
export PLANE_RS_JOBS_HANDOVER_GAP_SECONDS="$GAP"
export PLANE_RS_JOBS_HANDOVER_MARGIN_SECONDS="$MARGIN"
export PLANE_RS_BEAT_POLL_MS=100
set -a
# shellcheck source=../../dev/django-env.sh
source "$SERVER/dev/django-env.sh"
set +a

log "migrating plane_jobs"
"$BIN/plane-migrator" >/dev/null

ANCHOR=$(date +%s)
log "inserting a ${INTERVAL}s PeriodicTask anchored at $ANCHOR through Django's ORM"
(
  cd "$API_DIR"
  "$API_PYTHON" - "$ANCHOR" "$INTERVAL" <<'PY'
import sys
from datetime import datetime, timezone

import django
django.setup()
from django_celery_beat.models import IntervalSchedule, PeriodicTask

anchor = datetime.fromtimestamp(int(sys.argv[1]), tz=timezone.utc)
interval = int(sys.argv[2])
schedule, _ = IntervalSchedule.objects.get_or_create(
    every=interval, period=IntervalSchedule.SECONDS
)
task, _ = PeriodicTask.objects.update_or_create(
    name="rsjobs-handover-probe",
    defaults={
        "task": "plane.jobs.handover_probe",
        "interval": schedule,
        "enabled": True,
        "last_run_at": anchor,
        "args": "[]",
        "kwargs": "{}",
    },
)
print("inserted", task.name, task.task, "anchor", task.last_run_at.isoformat())
PY
)

probe_count() {
  python3 - "$RABBIT_HTTP" "$VHOST" "$TASK" <<'PY'
import json
import sys
import urllib.error
import urllib.request

url, vhost, task = sys.argv[1], sys.argv[2], sys.argv[3]
body = json.dumps({"count": 200, "ackmode": "ack_requeue_true", "encoding": "auto"}).encode()
request = urllib.request.Request(
    f"{url}/api/queues/{vhost}/celery/get",
    data=body,
    headers={
        "content-type": "application/json",
        "authorization": "Basic cGxhbmU6cGxhbmU=",
    },
    method="POST",
)
try:
    with urllib.request.urlopen(request, timeout=5) as response:
        messages = json.load(response)
except urllib.error.HTTPError as error:
    if error.code == 404:
        print(0)
        sys.exit(0)
    raise
count = 0
for message in messages:
    headers = (message.get("properties") or {}).get("headers") or {}
    if headers.get("task") == task:
        count += 1
print(count)
PY
}

log "starting the real Celery beat (DatabaseScheduler)"
(
  cd "$API_DIR"
  exec "$API_DIR/.venv/bin/celery" -A plane beat -l info \
    --schedule "$SERVER/target/rsjobs-handover-celerybeat-schedule"
) >"$SERVER/target/jobs-celery-beat.log" 2>&1 &
CELERY_PID=$!

log "waiting for the first probe tick"
FIRST_COUNT=0
FIRST_TS=0
for _ in $(seq 1 20); do
  sleep 1
  FIRST_COUNT=$(probe_count)
  if [[ "$FIRST_COUNT" -ge 1 ]]; then
    FIRST_TS=$(date +%s)
    break
  fi
done
[[ "$FIRST_COUNT" -ge 1 ]] || { echo "FAIL: Celery never published the probe" >&2; tail -20 "$SERVER/target/jobs-celery-beat.log" >&2; exit 1; }

# The next tick is ANCHOR+2*INTERVAL. Trigger the handover request one second
# before it, which is not a quiet window: the waiting path must be exercised.
HANDOVER_REQUEST=$((ANCHOR + 2 * INTERVAL - 1))
NOW=$(date +%s)
if [[ "$HANDOVER_REQUEST" -gt "$NOW" ]]; then
  sleep $((HANDOVER_REQUEST - NOW))
fi
HANDOVER_REQ_TS=$(date +%s)
log "handover request at +$((HANDOVER_REQ_TS - ANCHOR))s (tick is at +$((2 * INTERVAL))s)"

psql "$PG_URL/$TEST_DB" -q <<SQL
INSERT INTO plane_jobs.schedules
    (name, task, owner, kind, interval_seconds, enabled, last_run_at)
VALUES ('$PROBE_NAME', '$TASK', 'rust', 'interval', $INTERVAL, true,
        to_timestamp($ANCHOR))
ON CONFLICT (name) DO UPDATE SET
    task = EXCLUDED.task, owner = 'rust', kind = 'interval',
    interval_seconds = EXCLUDED.interval_seconds, enabled = true,
    last_run_at = EXCLUDED.last_run_at, handover_at = NULL;
SQL

"$BIN/plane-beat" >"$SERVER/target/jobs-handover-beat.log" 2>&1 &
BEAT_PID=$!

# Watch Celery and plane-beat until the fifth tick window has closed.
declare -A CELERY_TICK=()
FIRST_TICK=$(( (FIRST_TS - ANCHOR + INTERVAL / 2) / INTERVAL ))
CELERY_TICK[$FIRST_TICK]=$FIRST_COUNT
LAST_COUNT=$FIRST_COUNT
LAST_CELERY_TS=$FIRST_TS
END=$((ANCHOR + TICKS * INTERVAL + 5))
while [[ "$(date +%s)" -lt "$END" ]]; do
  COUNT=$(probe_count)
  NOW=$(date +%s)
  if [[ "$COUNT" -gt "$LAST_COUNT" ]]; then
    K=$(( (NOW - ANCHOR + INTERVAL / 2) / INTERVAL ))
    CELERY_TICK[$K]=$(( ${CELERY_TICK[$K]:-0} + COUNT - LAST_COUNT ))
    LAST_CELERY_TS=$NOW
    LAST_COUNT=$COUNT
  fi
  sleep 1
done

declare -A PLANE_TICK=()
while IFS='|' read -r k c; do
  [[ -n "$k" ]] && PLANE_TICK[$k]=$c
done < <(psql "$PG_URL/$TEST_DB" -tA -F'|' -c \
  "SELECT floor(extract(epoch FROM (created_at - to_timestamp($ANCHOR))) / $INTERVAL)::int AS k, count(*) \
   FROM plane_jobs.jobs WHERE task = '$TASK' GROUP BY k ORDER BY k")

HANDOVER_AT=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT extract(epoch FROM handover_at)::bigint FROM plane_jobs.schedules WHERE name = '$PROBE_NAME'")
DJANGO_ENABLED=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT enabled FROM django_celery_beat_periodictask WHERE name = '$PROBE_NAME'")
TRACKER_AFTER=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT last_update > to_timestamp($HANDOVER_REQ_TS) FROM django_celery_beat_periodictasks WHERE ident = 1")
PLANE_JOBS=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT count(*) FROM plane_jobs.jobs WHERE task = '$TASK'")
PLANE_TICKS=$(psql "$PG_URL/$TEST_DB" -tA -c \
  "SELECT count(*) FROM plane_jobs.ticks WHERE schedule_name = '$PROBE_NAME'")

CELERY_STOP_DELAY=$((LAST_CELERY_TS - HANDOVER_REQ_TS))
[[ "$CELERY_STOP_DELAY" -lt 0 ]] && CELERY_STOP_DELAY=0
WAITED=$((HANDOVER_AT - HANDOVER_REQ_TS))

log "handover happened +${WAITED}s after the request (non-quiet request waited)"
log "Celery last publish was +${CELERY_STOP_DELAY}s after the request (margin ${MARGIN}s)"
printf 'tick  celery  plane  total\n'
FAILED=0
for k in $(seq 1 "$TICKS"); do
  c=${CELERY_TICK[$k]:-0}
  p=${PLANE_TICK[$k]:-0}
  total=$((c + p))
  printf '%4d  %6d  %5d  %5d\n' "$k" "$c" "$p" "$total"
  [[ "$total" -eq 1 ]] || FAILED=1
done

log "django-enabled=$DJANGO_ENABLED tracker-bumped=$TRACKER_AFTER plane jobs=$PLANE_JOBS ticks=$PLANE_TICKS"
[[ "$WAITED" -ge 1 ]] || { echo "FAIL: handover did not wait for the quiet window" >&2; FAILED=1; }
[[ "$CELERY_STOP_DELAY" -le "$MARGIN" ]] || { echo "FAIL: Celery stopped after the margin" >&2; FAILED=1; }
[[ "$FAILED" -eq 0 ]] || { echo "FAIL: a tick was fired twice or not at all" >&2; exit 1; }
[[ "$DJANGO_ENABLED" == "f" ]] || { echo "FAIL: Django row is still enabled" >&2; exit 1; }
[[ "$TRACKER_AFTER" == "t" ]] || { echo "FAIL: tracker was not bumped" >&2; exit 1; }
[[ "$PLANE_JOBS" == "$PLANE_TICKS" ]] || { echo "FAIL: plane ticks and jobs differ" >&2; exit 1; }

log "PASS: exactly one run per tick, handover waited for a quiet window"
