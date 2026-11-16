#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# End-to-end worker proof for the Rust publisher:
#
#   1. creates a disposable database (cloned from plane_tpl) and RabbitMQ vhost;
#   2. starts a real Plane Celery worker (`celery -A plane worker -l info -c 1
#      -Q celery`, with `-P solo` on this host — see the note below) against
#      them;
#   3. writes the minimum user/workspace/project rows the tasks need;
#   4. publishes `recent_visited_task` and `process_logs` from Rust;
#   5. waits for the worker to execute them and shows the rows that appeared in
#      `user_recent_visits` and `api_activity_logs`;
#   6. stops the worker and drops the database and vhost.
#
# Nothing outside the dev stack is touched. Use `--keep` to leave the database,
# vhost and worker log for inspection.
#
#   apps/server/tools/celery/live_worker.sh

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../dev" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_DEV_DIR/lib.sh"

REPO_ROOT="$(cd "$PLANE_DEV_DIR/../../.." && pwd)"
SERVER_DIR="$REPO_ROOT/apps/server"
TOOLS_DIR="$SERVER_DIR/tools/celery"
API_DIR="$REPO_ROOT/apps/api"
PYTHON="$API_DIR/.venv/bin/python"

SUFFIX="$$"
DB="t_celery_w${SUFFIX}"
VHOST="t_celery_w${SUFFIX}"
REDIS_DB=15
WORK="$(mktemp -d "${TMPDIR:-/tmp}/celery-worker.XXXXXX")"
LOG="$WORK/worker.log"
KEEP=0
[[ "${1:-}" == "--keep" ]] && KEEP=1

export CARGO_TARGET_DIR="$SERVER_DIR/target/jobs"
WORKER_PID=""

cleanup() {
  status=$?
  if [[ -n "$WORKER_PID" ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill -TERM "$WORKER_PID" 2>/dev/null || true
    for _ in $(seq 1 40); do
      kill -0 "$WORKER_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -KILL "$WORKER_PID" 2>/dev/null || true
    wait "$WORKER_PID" 2>/dev/null || true
  fi
  if ((KEEP)); then
    note "keeping $DB, $VHOST and $LOG (--keep)"
  else
    "$PLANE_DEV_DIR/dropdb.sh" "$DB" >/dev/null 2>&1 || true
    "$PLANE_DEV_DIR/mq-vhost.sh" --delete "$VHOST" >/dev/null 2>&1 || true
    rm -rf "$WORK"
  fi
  exit "$status"
}
trap cleanup EXIT

[[ -x "$PYTHON" ]] || die "missing $PYTHON; the Django venv is required"
"$PLANE_DEV_DIR/status.sh" >/dev/null || die "dev stack is not healthy"

note "creating database $DB and vhost $VHOST"
"$PLANE_DEV_DIR/mkdb.sh" "$DB" --replace >/dev/null
"$PLANE_DEV_DIR/mq-vhost.sh" "$VHOST" >/dev/null

note "writing the demo fixture"
PLANE_DB="$DB" "$PLANE_DEV_DIR/django.sh" shell <"$TOOLS_DIR/fixture.py" >/dev/null

note "building the Rust publisher"
cd "$SERVER_DIR"
cargo build -q -p plane-jobs --features test-cases --example publish_case
PUBLISH="$CARGO_TARGET_DIR/debug/examples/publish_case"
URL="amqp://$PLANE_RABBITMQ_USER:$PLANE_RABBITMQ_PASSWORD@$PLANE_RABBITMQ_HOST:$PLANE_RABBITMQ_PORT/$VHOST"

note "starting the Plane Celery worker (log: $LOG)"
cd "$API_DIR"
# `django-env.sh` exports exactly what the API/worker containers get, with
# PLANE_DB/PLANE_MQ_VHOST selecting this run's clone and vhost.
export PLANE_DB="$DB" PLANE_MQ_VHOST="$VHOST" PLANE_REDIS_DB="$REDIS_DB"
# shellcheck disable=SC1091
source "$PLANE_DEV_DIR/django-env.sh"
# The command is the one from TASKS.md §1.4 with `-P solo` added: this host's
# Python uses billiard's spawn start method, and the prefork child hangs before
# the pool reports ready (it consumes but never dispatches), so tasks would sit
# in the pool queue forever. The solo pool runs the same worker, consumer,
# serialiser and task code, which is what this check exercises.
.venv/bin/celery -A plane worker -l info -c 1 -Q celery -P solo >"$LOG" 2>&1 &
WORKER_PID=$!

ready=0
for _ in $(seq 1 240); do
  if ! kill -0 "$WORKER_PID" 2>/dev/null; then
    tail -40 "$LOG" >&2
    die "the worker exited during startup"
  fi
  # The "ready." line is swallowed by Plane's custom JSON logging, so ask the
  # broker instead: the worker is ready when the queue has a consumer.
  consumers="$(
    curl -fsS -u "$PLANE_RABBITMQ_USER:$PLANE_RABBITMQ_PASSWORD" \
      "http://$PLANE_RABBITMQ_HOST:$PLANE_RABBITMQ_MGMT_PORT/api/queues/$VHOST/celery" 2>/dev/null |
      python3 -c 'import json,sys; print(json.load(sys.stdin).get("consumers", 0))' 2>/dev/null ||
      echo 0
  )"
  if [[ "${consumers:-0}" -ge 1 ]]; then
    ready=1
    break
  fi
  sleep 0.25
done
((ready)) || {
  tail -40 "$LOG" >&2
  die "the worker did not start consuming from the celery queue"
}
note "worker consuming from $VHOST/celery"

note "publishing recent_visited_task and process_logs from Rust"
"$PUBLISH" --url "$URL" --case recent_visited_project
"$PUBLISH" --url "$URL" --case process_logs

recent_query=$(
  cat <<'SQL'
SELECT count(*) FROM user_recent_visits
WHERE user_id = '0d1e2f30-4456-4789-9abc-def012345678'::uuid
  AND workspace_id = '6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60'::uuid
  AND project_id = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'::uuid
  AND entity_name = 'project'
SQL
)
logs_query="SELECT count(*) FROM api_activity_logs WHERE token_identifier = 'b3f1c0d2e3a4'"

recent=0
logs=0
for _ in $(seq 1 120); do
  recent="$(psql_dev "$DB" -tAc "$recent_query" | tr -d '[:space:]')"
  logs="$(psql_dev "$DB" -tAc "$logs_query" | tr -d '[:space:]')"
  if [[ "${recent:-0}" -ge 1 && "${logs:-0}" -ge 1 ]]; then
    break
  fi
  sleep 0.25
done

[[ "${recent:-0}" -ge 1 && "${logs:-0}" -ge 1 ]] || {
  tail -80 "$LOG" >&2
  die "tasks did not produce rows (user_recent_visits=$recent api_activity_logs=$logs)"
}

note "rows written by the worker"
psql_dev "$DB" -c "SELECT entity_name, entity_identifier, user_id, project_id, visited_at FROM user_recent_visits"
psql_dev "$DB" -c "SELECT token_identifier, path, method, response_code FROM api_activity_logs"
note "worker task log"
grep -Ei "received|succeeded" "$LOG" | tail -8

note "stopping the worker"
kill -TERM "$WORKER_PID"
wait "$WORKER_PID" 2>/dev/null || true
WORKER_PID=""

note "worker demo passed"
