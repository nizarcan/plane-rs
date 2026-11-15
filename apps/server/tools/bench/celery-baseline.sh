#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Idle-memory baseline for the Django background processes that plane-worker
# and plane-beat will replace.
#
# The processes are started exactly the way the production entrypoints start
# them, minus the one-off setup steps:
#
#   apps/api/bin/docker-entrypoint-worker.sh:
#     python manage.py wait_for_db; python manage.py wait_for_migrations
#     celery -A plane worker -l info
#   apps/api/bin/docker-entrypoint-beat.sh:
#     python manage.py wait_for_db; python manage.py wait_for_migrations
#     celery -A plane beat -l info
#
# Neither passes --concurrency, so Celery's default applies: one worker process
# per CPU. RUST_REWRITE.md's deployed instance used concurrency 4, so both the
# default and --concurrency 4 are measured.
#
# Each service gets its own database clone, RabbitMQ vhost and Redis DB index,
# waits for its "ready" line, settles, and is sampled for RSS. Everything it
# created is removed afterwards unless --keep is given.
#
#   apps/server/tools/bench/celery-baseline.sh
#   apps/server/tools/bench/celery-baseline.sh --concurrency 4 --keep

set -euo pipefail

PLANE_TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_TOOLS_DIR/../../dev/lib.sh"

usage() {
  cat <<'EOF'
usage: celery-baseline.sh [options]

  --concurrency N   pass `-c N` to `celery worker` (default: Celery's own
                    default, one process per CPU). Beat is not affected.
  --settle SECONDS  idle time before sampling (default: 15)
  --samples N       samples per service, 250 ms apart (default: 20)
  --keep            keep the database clone and vhost afterwards
EOF
}

concurrency=""
settle=15
samples=20
keep=0
while (($# > 0)); do
  case "$1" in
    --concurrency)
      shift
      concurrency="${1:-}"
      [[ "$concurrency" =~ ^[0-9]+$ ]] || die "--concurrency needs a number"
      ;;
    --settle)
      shift
      settle="${1:-}"
      [[ "$settle" =~ ^[0-9]+$ ]] || die "--settle needs a number"
      ;;
    --samples)
      shift
      samples="${1:-}"
      [[ "$samples" =~ ^[0-9]+$ ]] || die "--samples needs a number"
      ;;
    --keep) keep=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown argument '$1'" ;;
  esac
  shift
done

run_id="cb$(date +%H%M%S)"
worker_db="bench_celery_w_$run_id"
worker_vhost="$worker_db"
beat_db="bench_celery_b_$run_id"
beat_vhost="$beat_db"
redis_db=4
celery_bin="$PLANE_API_DIR/.venv/bin/celery"
[[ -x "$celery_bin" ]] || die "celery not found at $celery_bin (see dev/README.md)"

cleanup() {
  stop_group "$worker_pid" || true
  stop_group "$beat_pid" || true
  if ((keep == 0)); then
    "$PLANE_DEV_DIR/dropdb.sh" "$worker_db" >/dev/null 2>&1 || true
    "$PLANE_DEV_DIR/dropdb.sh" "$beat_db" >/dev/null 2>&1 || true
    "$PLANE_DEV_DIR/mq-vhost.sh" --delete "$worker_vhost" >/dev/null 2>&1 || true
    "$PLANE_DEV_DIR/mq-vhost.sh" --delete "$beat_vhost" >/dev/null 2>&1 || true
  fi
}

worker_pid=""
beat_pid=""
trap cleanup EXIT INT TERM

# kill -TERM the whole process group of a background job started with `set -m`.
stop_group() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill -0 "$pid" 2>/dev/null || return 0
  kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 60); do
    kill -0 "$pid" 2>/dev/null || return 0
    sleep 0.25
  done
  kill -KILL -- "-$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

# sum_rss_kib <pgid>: prints "<sum KiB> <processes>"
sum_rss_kib() {
  ps -A -o pgid=,rss= | awk -v group="$1" '$1 == group { sum += $2; count += 1 } END { printf "%d %d\n", sum, count }'
}

median() {
  sort -n | awk '{ values[NR] = $1 } END {
    if (NR == 0) { print 0; exit }
    if (NR % 2) { print values[(NR + 1) / 2] } else { print (values[NR / 2] + values[NR / 2 + 1]) / 2 }
  }'
}

sample_service() {
  local label="$1" pgid="$2"
  local values=() sum processes
  for _ in $(seq 1 "$samples"); do
    read -r sum processes < <(sum_rss_kib "$pgid")
    values+=("$sum")
    sleep 0.25
  done
  local median_kib min_kib max_kib
  median_kib="$(printf '%s\n' "${values[@]}" | median)"
  min_kib="$(printf '%s\n' "${values[@]}" | sort -n | head -1)"
  max_kib="$(printf '%s\n' "${values[@]}" | sort -n | tail -1)"
  read -r _ processes < <(sum_rss_kib "$pgid")
  awk -v label="$label" -v median="$median_kib" -v min="$min_kib" -v max="$max_kib" \
    -v procs="$processes" 'BEGIN {
      printf "%-8s processes %-3s idle RSS median %7.1f MiB (min %7.1f, max %7.1f)\n",
        label, procs, median / 1024, min / 1024, max / 1024
    }'
}

wait_for_log() {
  local file="$1" pattern="$2" timeout="$3"
  for _ in $(seq 1 "$timeout"); do
    grep -q "$pattern" "$file" 2>/dev/null && return 0
    sleep 1
  done
  return 1
}

cd "$PLANE_API_DIR"

# --- worker -----------------------------------------------------------------
note "worker: cloning $worker_db and creating vhost $worker_vhost"
"$PLANE_DEV_DIR/mkdb.sh" "$worker_db" --replace
"$PLANE_DEV_DIR/mq-vhost.sh" "$worker_vhost"

export PLANE_DB="$worker_db"
export PLANE_REDIS_DB="$redis_db"
export PLANE_MQ_VHOST="$worker_vhost"
eval "$("$PLANE_DEV_DIR/django-env.sh")"

worker_log="$PLANE_DEV_DIR/logs/celery-worker-$run_id.log"
worker_args=(-A plane worker -l info)
if [[ -n "$concurrency" ]]; then
  worker_args+=(-c "$concurrency")
fi
note "starting: celery ${worker_args[*]}"
set -m
"$celery_bin" "${worker_args[@]}" >"$worker_log" 2>&1 &
worker_pid=$!
set +m

# Celery 5.5 on this stack does not print its `celery@host ready.` line (the
# consumer is up: `celery inspect ping` answers pong), so readiness is taken
# from the mingle line, which is emitted after the pool has started and every
# child has imported the app. That is also when the idle RSS is final.
if ! wait_for_log "$worker_log" "mingle: all alone" 180; then
  tail -40 "$worker_log" >&2
  die "the worker never finished starting"
fi
worker_concurrency="$(grep -o 'concurrency: [0-9]*' "$worker_log" | head -1 | awk '{print $2}')"
worker_concurrency="${worker_concurrency:-unknown}"
note "worker started (concurrency $worker_concurrency); settling ${settle}s"
sleep "$settle"
sample_service "worker" "$worker_pid"
stop_group "$worker_pid"
worker_pid=""

# --- beat -------------------------------------------------------------------
note "beat: cloning $beat_db and creating vhost $beat_vhost"
"$PLANE_DEV_DIR/mkdb.sh" "$beat_db" --replace
"$PLANE_DEV_DIR/mq-vhost.sh" "$beat_vhost"

export PLANE_DB="$beat_db"
export PLANE_REDIS_DB="$redis_db"
export PLANE_MQ_VHOST="$beat_vhost"
eval "$("$PLANE_DEV_DIR/django-env.sh")"

beat_log="$PLANE_DEV_DIR/logs/celery-beat-$run_id.log"
note "starting: celery -A plane beat -l info"
set -m
"$celery_bin" -A plane beat -l info >"$beat_log" 2>&1 &
beat_pid=$!
set +m

# Celery 5.5 prints "celery beat v5.5.3 ... is starting." rather than the old
# "beat: Starting..." line.
if ! wait_for_log "$beat_log" "is starting\." 120; then
  tail -40 "$beat_log" >&2
  die "beat never started"
fi
note "beat started; settling ${settle}s"
sleep "$settle"
sample_service "beat" "$beat_pid"
stop_group "$beat_pid"
beat_pid=""

if ((keep == 1)); then
  note "--keep: left $worker_db, $beat_db and their vhosts"
fi
note "done (logs: $worker_log, $beat_log)"
