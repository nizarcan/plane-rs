#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Phase 8 final benchmark: the Phase 5 read matrix and background-service
# footprint at the cutover head, plus what Phase 5 could not show — a live job
# queue, task throughput, and the whole-stack total.
#
#   * five read scenarios, concurrency 1, one repetition, durations that give
#     every route at least 50 sequential requests on Django (Phase 5's method
#     and durations, unchanged);
#   * `reads-mixed` at concurrency 8, one 20 s window;
#   * one write mix (`scenarios/bench/writes-mixed.yaml`): create/update of a
#     work item and a comment, concurrency 1, 60 s per target. Deletes are not
#     in the loop: one static URL per template can only be deleted once per
#     target start (see the scenario's header);
#   * `plane-worker` with its `LISTEN` connection up and four claimed echo
#     jobs (`PLANE_RS_JOB_CONCURRENCY=4`), `plane-beat` after a real schedule
#     tick has fired, `plane-api` idle;
#   * a 10,000-job `recent_visited_task` drain on `plane-worker` vs
#     `celery worker -c 4`, wall time and cgroup CPU, both paused while the
#     queue is filled so the window starts from an empty consumer;
#   * the whole-stack total against `RUST_REWRITE.md`'s Python budget.
#
# Isolation (Phase 8 namespace):
#   containers  plane-rs-bench-p8-{django,api,stats,worker,beat,migrator,
#               celery,celery-beat}
#   databases   bench_p8_{django,api,jobs} (clones of plane_fix_tpl)
#   Redis DBs   10 (Django) and 11 (Rust API/worker/beat) only
#   vhosts      bench_p8_{django,api,jobs}
#   host ports  127.0.0.1:18310 (Django), 127.0.0.1:18360 (plane-api)
#   limits      --cpus 2 --memory 512m --memory-swap 512m for the HTTP targets
#               and the Rust background services; the Celery containers get
#               --memory 1g because their Linux prefork set is larger than the
#               Rust in-flight set (recorded in BENCH.md).
#
#   apps/server/tools/bench/run-phase8.sh
#   apps/server/tools/bench/run-phase8.sh --no-build --scenarios reads-mixed
#   apps/server/tools/bench/run-phase8.sh --verify-only
#   apps/server/tools/bench/run-phase8.sh --down
#
# Secrets (SECRET_KEY) are read through dev/django-env.sh and written to
# apps/server/dev/run/, which is gitignored; they are never printed.

set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

DJANGO_IMAGE="plane-rs-bench-django:latest"
API_IMAGE="plane-rs-bench-p8-api:latest"
WORKER_IMAGE="plane-rs-bench-p8-worker:latest"
BEAT_IMAGE="plane-rs-bench-p8-beat:latest"
MIGRATOR_IMAGE="plane-rs-bench-p8-migrator:latest"

DJANGO_CONTAINER="plane-rs-bench-p8-django"
API_CONTAINER="plane-rs-bench-p8-api"
STATS_CONTAINER="plane-rs-bench-p8-stats"
WORKER_CONTAINER="plane-rs-bench-p8-worker"
BEAT_CONTAINER="plane-rs-bench-p8-beat"
MIGRATOR_CONTAINER="plane-rs-bench-p8-migrator"
CELERY_CONTAINER="plane-rs-bench-p8-celery"
CELERY_SOLO_CONTAINER="plane-rs-bench-p8-celery-solo"
CELERY_BEAT_CONTAINER="plane-rs-bench-p8-celery-beat"

DJANGO_DB="bench_p8_django"
API_DB="bench_p8_api"
JOBS_DB="bench_p8_jobs"
DJANGO_REDIS_DB=10
API_REDIS_DB=11
JOBS_REDIS_DB=11
DJANGO_VHOST="bench_p8_django"
API_VHOST="bench_p8_api"
JOBS_VHOST="bench_p8_jobs"
DJANGO_PORT=18310
API_PORT=18360
NETWORK="plane-rs-dev_default"
SCENARIOS=(reads-users-me reads-project reads-issues reads-cycles reads-mixed)
WRITE_SCENARIO="writes-mixed"
DIFFTEST="$PLANE_SERVER_DIR/target/release/difftest"
SYSTEM_PYTHON="$(command -v python3)"
COMMIT="6bff715d1d"
BENCH_DATE="2026-11-19"
LATENCY_DURATION=30
MIXED_LATENCY_DURATION=70
WRITE_DURATION=60
LOAD_DURATION=20
LOAD_CONCURRENCY=8
DRAIN_JOBS=10000
WORKER_CONCURRENCY=4

usage() {
  cat <<'EOF'
usage: run-phase8.sh [--keep] [--no-build] [--verify-only] [--down] [--tag NAME]
                     [--scenarios LIST] [--skip-jobs] [--skip-drain]

  --keep          leave the containers and databases running at the end
  --no-build      reuse the images and target/release/difftest
  --verify-only   start, run the differential verification, stop
  --down          remove what an earlier run left behind (containers, databases,
                  vhosts; the images stay)
  --tag NAME      report directory under target/bench (default phase8-<timestamp>)
  --scenarios LIST comma-separated subset of the five reads-* scenarios
  --skip-jobs     skip the worker/beat live-queue sample
  --skip-drain    skip the Rust-vs-Celery task throughput measurement
  --jobs-only     only the background-service and throughput measurements
                  (for development; needs a provisioned stack)
  --load-only     only the reads-mixed concurrency-8 window on fresh clones
EOF
}

keep=0
skip_build=0
verify_only=0
down_only=0
skip_jobs=0
skip_drain=0
jobs_only=0
load_only=0
scenarios_csv=""
tag="phase8-$(date +%Y%m%d-%H%M%S)"
while (($# > 0)); do
  case "$1" in
    --keep) keep=1 ;;
    --no-build) skip_build=1 ;;
    --verify-only) verify_only=1 ;;
    --down) down_only=1 ;;
    --skip-jobs) skip_jobs=1 ;;
    --skip-drain) skip_drain=1 ;;
    --jobs-only) jobs_only=1 ;;
    --load-only) load_only=1 ;;
    --scenarios)
      shift
      scenarios_csv="${1:-}"
      [[ -n "$scenarios_csv" ]] || die "--scenarios needs a comma-separated list"
      ;;
    --tag)
      shift
      tag="${1:-}"
      [[ -n "$tag" ]] || die "--tag needs a value"
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown argument '$1'" ;;
  esac
  shift
done
if [[ -n "$scenarios_csv" ]]; then
  IFS=',' read -r -a SCENARIOS <<<"$scenarios_csv"
fi

out="$PLANE_SERVER_DIR/target/bench/$tag"
run_dir="$PLANE_DEV_DIR/run"
mkdir -p "$out" "$run_dir"
cd "$PLANE_SERVER_DIR"

# ---------------------------------------------------------------------------
# Guard rails: only ever remove containers this script created.
# ---------------------------------------------------------------------------
remove_container() {
  local name="$1"
  [[ "$name" == plane-rs-bench-p8-* ]] || die "refusing to remove container '$name'"
  if docker container inspect "$name" >/dev/null 2>&1; then
    docker rm -f "$name" >/dev/null
  fi
}

rabbit_api() {
  curl -fsS --max-time 10 -u "$PLANE_RABBITMQ_USER:$PLANE_RABBITMQ_PASSWORD" "$@"
}

flush_redis() {
  compose exec -T valkey valkey-cli -n "$1" flushdb >/dev/null
}

drop_database() {
  psql_dev plane -c "DROP DATABASE IF EXISTS \"$1\" WITH (FORCE)" >/dev/null
}

create_database() {
  psql_dev plane -c "CREATE DATABASE \"$1\" TEMPLATE plane_fix_tpl" >/dev/null
}

create_vhost() {
  local vhost="$1" ttl_ms="$2"
  rabbit_api -X PUT "$MGMT/api/vhosts/$vhost" >/dev/null
  rabbit_api -X PUT -H 'content-type: application/json' \
    -d '{"configure":".*","write":".*","read":".*"}' \
    "$MGMT/api/permissions/$vhost/$PLANE_RABBITMQ_USER" >/dev/null
  # No consumer drains the HTTP targets' vhosts during the measured windows.
  # 10 minutes is long enough for the Celery throughput drain (enqueued while
  # the worker is paused) and short enough that the read matrix's messages do
  # not accumulate.
  rabbit_api -X PUT -H 'content-type: application/json' \
    -d "{\"pattern\":\".*\",\"definition\":{\"message-ttl\":$ttl_ms},\"priority\":0,\"apply-to\":\"queues\"}" \
    "$MGMT/api/policies/$vhost/bench-ttl" >/dev/null
}

delete_vhost() {
  rabbit_api -X DELETE "$MGMT/api/vhosts/$1" >/dev/null 2>&1 || true
}

stop_sampler() {
  local phase="$1" pid
  if [[ -f "$out/samples-$phase.pid" ]]; then
    pid="$(cat "$out/samples-$phase.pid")"
    # Only kill the pipeline this script started. The recorded pid is the
    # background group; the exact log path also names its stamp.py and
    # `docker logs --follow` processes.
    kill "$pid" 2>/dev/null || true
    pkill -f "stamp.py $out/samples-$phase.log" 2>/dev/null || true
    pkill -f "docker logs --follow $STATS_CONTAINER" 2>/dev/null || true
    rm -f "$out/samples-$phase.pid"
  fi
  remove_container "$STATS_CONTAINER"
}

stop_all_samplers() {
  local pidfile
  for pidfile in "$out"/samples-*.pid; do
    [[ -f "$pidfile" ]] || continue
    stop_sampler "$(basename "$pidfile" .pid | sed -E 's/^samples-//')"
  done
  # A crashed sampler may have lost its pid file; the path is unique to this
  # run, so matching it cannot touch another agent's process.
  pkill -f "stamp.py $out/samples-" 2>/dev/null || true
  pkill -f "docker logs --follow $STATS_CONTAINER" 2>/dev/null || true
  remove_container "$STATS_CONTAINER" || true
}

pg_load_pid=""
stop_pg_load() {
  touch "$out/pg-load.stop"
  if [[ -n "${pg_load_pid:-}" ]] && kill -0 "$pg_load_pid" 2>/dev/null; then
    kill "$pg_load_pid" 2>/dev/null || true
  fi
  pkill -f "pg-load.sh $out/pg-load.log" 2>/dev/null || true
}

cleanup() {
  note "stopping bench containers"
  stop_all_samplers
  stop_pg_load
  for name in "$MIGRATOR_CONTAINER" "$WORKER_CONTAINER" "$BEAT_CONTAINER" \
    "$CELERY_CONTAINER" "$CELERY_SOLO_CONTAINER" "$CELERY_BEAT_CONTAINER" \
    "$DJANGO_CONTAINER" "$API_CONTAINER"; do
    remove_container "$name"
  done
  note "dropping bench databases, vhosts and Redis DBs"
  for database in "$DJANGO_DB" "$API_DB" "$JOBS_DB" "bench_p8_inspect"; do
    drop_database "$database" || true
  done
  for vhost in "$DJANGO_VHOST" "$API_VHOST" "$JOBS_VHOST"; do
    delete_vhost "$vhost"
  done
  for redis_db in "$DJANGO_REDIS_DB" "$API_REDIS_DB"; do
    flush_redis "$redis_db" || true
  done
  rm -f "$run_dir/bench-p8-django.env" "$run_dir/bench-p8-api.env" "$run_dir/bench-p8-jobs.env"
}

MGMT="http://127.0.0.1:$PLANE_RABBITMQ_MGMT_PORT"

on_exit() {
  local status=$?
  trap - EXIT INT TERM
  # A verify DIFF or a failed wait must not leave a log follower holding the
  # caller's stdout pipe open, and must not leave this run's containers behind.
  stop_all_samplers
  stop_pg_load
  if ((keep == 0)); then
    cleanup
  fi
  exit "$status"
}
trap on_exit EXIT INT TERM

if ((down_only == 1)); then
  cleanup
  note "done"
  exit 0
fi

# ---------------------------------------------------------------------------
# Images
# ---------------------------------------------------------------------------
image_size() {
  docker image inspect --format '{{.Size}}' "$1"
}

image_disk_size() {
  docker image ls --format '{{.Size}}' "$1"
}

preflight() {
  [[ -n "$(docker network ls --filter "name=^${NETWORK}$" --format '{{.Name}}')" ]] ||
    die "docker network $NETWORK is missing; start the dev stack with apps/server/dev/up.sh"
  local host_free_kib vm_available_mib
  host_free_kib="$(df -k "$PLANE_REPO_ROOT" | awk 'NR==2 {print $4}')"
  vm_available_mib="$(docker run --rm alpine:3.20 sh -c \
    "awk '/MemAvailable/{print int(\$2 / 1024)}' /proc/meminfo" 2>/dev/null || echo 0)"
  note "free space: host $((host_free_kib / 1024 / 1024)) GiB, Docker VM ${vm_available_mib} MiB memory"
  ((host_free_kib >= 20 * 1024 * 1024)) ||
    die "the host filesystem holding the repo has $((host_free_kib / 1024 / 1024)) GiB free; keep at least 20 GiB"
  if ((skip_build == 0)) && ((vm_available_mib < 700)); then
    die "the Docker VM has ${vm_available_mib} MiB available and the fat-LTO release build needs ~700+ MiB"
  fi
}

build_images() {
  if ((skip_build == 1)); then
    for image in "$DJANGO_IMAGE" "$API_IMAGE"; do
      docker image inspect "$image" >/dev/null || die "$image is missing"
    done
    if ((skip_jobs == 0 || skip_drain == 0)); then
      for image in "$WORKER_IMAGE" "$BEAT_IMAGE" "$MIGRATOR_IMAGE"; do
        docker image inspect "$image" >/dev/null || die "$image is missing"
      done
    fi
    [[ -x "$DIFFTEST" ]] || die "$DIFFTEST is missing"
    return
  fi
  if ! docker image inspect "$DJANGO_IMAGE" >/dev/null 2>&1; then
    die "$DJANGO_IMAGE is missing; build it from apps/api/Dockerfile.api first"
  fi
  note "building $API_IMAGE (multi-stage, release profile)"
  docker build --progress=plain \
    --build-arg CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" \
    -f "$PLANE_SERVER_DIR/deploy/docker/Dockerfile.plane-api" \
    -t "$API_IMAGE" "$PLANE_REPO_ROOT" 2>&1 | tee "$out/build-api-image.log"
  note "building $WORKER_IMAGE, $BEAT_IMAGE, $MIGRATOR_IMAGE"
  local target image
  for target in worker beat migrator; do
    case "$target" in
      worker) image="$WORKER_IMAGE" ;;
      beat) image="$BEAT_IMAGE" ;;
      migrator) image="$MIGRATOR_IMAGE" ;;
    esac
    docker build --progress=plain \
      --build-arg CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" \
      --target "$target" \
      -f "$PLANE_SERVER_DIR/deploy/docker/Dockerfile.plane-jobs" \
      -t "$image" "$PLANE_REPO_ROOT" 2>&1 | tee "$out/build-$target-image.log"
  done
  note "building difftest (release)"
  cargo build --release --manifest-path "$PLANE_SERVER_DIR/crates/plane-testkit/Cargo.toml" \
    --bin difftest
}

# ---------------------------------------------------------------------------
# Environment files: dev/django-env.sh with the container hostnames.
# ---------------------------------------------------------------------------
ENV_KEYS=(
  DJANGO_SETTINGS_MODULE DEBUG IS_TEST SECRET_KEY DATABASE_URL ENABLE_READ_REPLICA
  REDIS_URL RABBITMQ_HOST RABBITMQ_PORT RABBITMQ_USER RABBITMQ_PASSWORD RABBITMQ_VHOST
  WEB_URL ALLOWED_HOSTS USE_MINIO MINIO_ENDPOINT_SSL AWS_ACCESS_KEY_ID
  AWS_SECRET_ACCESS_KEY AWS_S3_BUCKET_NAME AWS_S3_ENDPOINT_URL AWS_REGION
  SIGNED_URL_EXPIRATION GUNICORN_WORKERS PYTHONDONTWRITEBYTECODE PYTHONUNBUFFERED
  ENABLE_DRF_SPECTACULAR PLANE_DB PLANE_REDIS_DB PLANE_MQ_VHOST
  PORT PYTHONHASHSEED PLANE_RS_WORKER_THREADS
)

write_env() {
  local file="$1" database="$2" redis_db="$3" vhost="$4" port="$5"
  (
    export PLANE_DB="$database" PLANE_REDIS_DB="$redis_db" PLANE_MQ_VHOST="$vhost"
    eval "$("$PLANE_DEV_DIR/django-env.sh")"
    export DATABASE_URL="postgresql://$PLANE_PG_USER:$PLANE_PG_PASSWORD@postgres:5432/$database"
    export REDIS_URL="redis://valkey:6379/$redis_db"
    export RABBITMQ_HOST="rabbitmq"
    export RABBITMQ_PORT="5672"
    export AWS_S3_ENDPOINT_URL="http://minio:9000"
    export PORT="$port"
    export PYTHONHASHSEED="0"
    export PLANE_RS_WORKER_THREADS="2"
    umask 077
    : >"$file"
    local key
    for key in "${ENV_KEYS[@]}"; do
      [[ -n "${!key+x}" ]] || continue
      printf '%s=%s\n' "$key" "${!key}" >>"$file"
    done
  )
}

# ---------------------------------------------------------------------------
# Provisioning
# ---------------------------------------------------------------------------
provision() {
  note "recreating $DJANGO_DB, $API_DB and $JOBS_DB from plane_fix_tpl"
  for database in "$DJANGO_DB" "$API_DB" "$JOBS_DB"; do
    drop_database "$database"
    create_database "$database"
  done

  note "flushing Redis DBs $DJANGO_REDIS_DB and $API_REDIS_DB"
  for redis_db in "$DJANGO_REDIS_DB" "$API_REDIS_DB"; do
    flush_redis "$redis_db"
  done

  note "creating RabbitMQ vhosts (10 min message TTL)"
  create_vhost "$DJANGO_VHOST" 600000
  create_vhost "$API_VHOST" 2000
  create_vhost "$JOBS_VHOST" 2000

  note "writing environment files under dev/run/"
  write_env "$run_dir/bench-p8-django.env" "$DJANGO_DB" "$DJANGO_REDIS_DB" "$DJANGO_VHOST" 8000
  write_env "$run_dir/bench-p8-api.env" "$API_DB" "$API_REDIS_DB" "$API_VHOST" 8000
  write_env "$run_dir/bench-p8-jobs.env" "$JOBS_DB" "$JOBS_REDIS_DB" "$JOBS_VHOST" 8000
  # The harness's Rust target starts from the cloned fixture, like
  # `dev/django-serve.sh`, which begins after the entrypoint's boot steps. The
  # default would run `register_instance` (an outbound GitHub lookup) and
  # `clear_cache` before the listener opens; BENCH.md records the choice.
  printf 'PLANE_RS_BOOT_STEPS=skip\n' >>"$run_dir/bench-p8-api.env"
  printf 'PLANE_RS_JOB_CONCURRENCY=%s\n' "$WORKER_CONCURRENCY" >>"$run_dir/bench-p8-jobs.env"
  printf 'PLANE_RS_JOB_POLL_MS=100\n' >>"$run_dir/bench-p8-jobs.env"
}

wait_http() {
  local url="$1" deadline=$((SECONDS + 120))
  until curl -fsS -o /dev/null --max-time 2 "$url"; do
    ((SECONDS < deadline)) || die "timed out waiting for $url"
    sleep 0.5
  done
}

wait_log() {
  local name="$1" pattern="$2"
  local timeout="${3:-60}"
  local deadline=$((SECONDS + timeout))
  until docker logs "$name" >"$out/$name.log" 2>&1 && grep -q "$pattern" "$out/$name.log"; do
    ((SECONDS < deadline)) || die "$name never printed $pattern"
    sleep 0.5
  done
}

try_wait_log() {
  local name="$1" pattern="$2"
  local timeout="${3:-20}"
  local deadline=$((SECONDS + timeout))
  until docker logs "$name" >"$out/$name.log" 2>&1 && grep -q "$pattern" "$out/$name.log"; do
    ((SECONDS < deadline)) || return 1
    sleep 0.5
  done
}

wait_log_count() {
  local name="$1" pattern="$2" expected="$3"
  local timeout="${4:-60}"
  local deadline=$((SECONDS + timeout)) count
  while :; do
    count="$(docker logs "$name" 2>&1 | grep -c "$pattern" || true)"
    ((count >= expected)) && return 0
    ((SECONDS < deadline)) || {
      note "timed out waiting for $expected '$pattern' lines in $name (last $count)"
      return 1
    }
    sleep 0.5
  done
}

wait_sql_count() {
  local db="$1" sql="$2" expected="$3"
  local timeout="${4:-240}"
  local deadline=$((SECONDS + timeout)) count
  while :; do
    count="$(psql_dev "$db" -tAc "$sql" | tr -d '[:space:]')"
    [[ "$count" == "$expected" ]] && return 0
    ((SECONDS < deadline)) || {
      note "timed out waiting for $expected rows (last $count): $sql"
      return 1
    }
    sleep 0.2
  done
}

migrate_jobs_schema() {
  local database="$1" env_file="$2" log_file="$3"
  note "creating the plane_jobs schema in $database with $MIGRATOR_IMAGE"
  docker run --rm --name "$MIGRATOR_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 --memory 512m --memory-swap 512m \
    --env-file "$env_file" \
    "$MIGRATOR_IMAGE" 2>&1 | tee "$out/$log_file"
}

start_containers() {
  note "starting $DJANGO_CONTAINER on 127.0.0.1:$DJANGO_PORT and $API_CONTAINER on 127.0.0.1:$API_PORT"
  # Same command as run-phase5.sh (`apps/api/bin/docker-entrypoint-api.sh`'s
  # gunicorn line, access log to /dev/null) with this run's names and ports.
  docker run -d --name "$DJANGO_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 \
    --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-p8-django.env" \
    -p "127.0.0.1:$DJANGO_PORT:8000" \
    "$DJANGO_IMAGE" \
    /bin/bash -c 'python manage.py collectstatic --noinput >/dev/null 2>&1 && exec gunicorn -w "$GUNICORN_WORKERS" -k uvicorn.workers.UvicornWorker plane.asgi:application --bind 0.0.0.0:"${PORT:-8000}" --max-requests 1200 --max-requests-jitter 1000 --access-logfile /dev/null' \
    >/dev/null

  docker run -d --name "$API_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 \
    --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-p8-api.env" \
    -p "127.0.0.1:$API_PORT:8000" \
    "$API_IMAGE" \
    >/dev/null

  wait_http "http://127.0.0.1:$DJANGO_PORT/robots.txt"
  wait_http "http://127.0.0.1:$API_PORT/"
  note "both targets answer"
}

# ---------------------------------------------------------------------------
# cgroup sampler: `TARGETS=name:<container id>[,...]`.
# ---------------------------------------------------------------------------
start_sampler() {
  local phase="$1" targets="$2"
  remove_container "$STATS_CONTAINER"
  note "starting the cgroup sampler ($targets)"
  docker run -d --name "$STATS_CONTAINER" \
    --network none \
    --memory 64m \
    --log-driver local --log-opt max-size=50m \
    -v /sys/fs/cgroup:/host/cgroup:ro \
    -v "$TOOLS_DIR/cgroup-sampler.py:/sampler.py:ro" \
    -e "TARGETS=$targets" \
    -e "INTERVAL=0.25" \
    "$DJANGO_BASE_PYTHON" python3 /sampler.py >/dev/null
  # The whole group's stdio goes to /dev/null: a follower that outlives this
  # script must never hold the caller's stdout pipe open.
  {
    docker logs --follow "$STATS_CONTAINER" 2>/dev/null \
      | "$SYSTEM_PYTHON" "$TOOLS_DIR/stamp.py" "$out/samples-$phase.log" >/dev/null
  } >/dev/null 2>&1 &
  echo $! >"$out/samples-$phase.pid"
  sleep 1
}

container_id() {
  docker inspect --format '{{.Id}}' "$1"
}

mark() {
  local file="$1" label="$2"
  printf '%s EVENT %s\n' "$("$SYSTEM_PYTHON" -c 'import time; print(f"{time.time():.6f}")')" "$label" \
    >>"$file"
}

load_note() {
  local load
  load="$(uptime | awk -F'load averages: ' '{print $2}' | awk '{print $1}')"
  printf '%s %s load1=%s\n' "$("$SYSTEM_PYTHON" -c 'import time; print(f"{time.time():.6f}")')" "$1" "$load" \
    >>"$out/load.log"
}

# ---------------------------------------------------------------------------
# Verification
# ---------------------------------------------------------------------------
verify() {
  local scenarios=("$@") scenario verify_file
  note "verifying scenario responses: Django vs plane-api"
  for scenario in "${scenarios[@]}"; do
    verify_file="$out/verify-$scenario.yaml"
    "$PLANE_DJANGO_PYTHON" "$TOOLS_DIR/bench-to-diff.py" \
      "scenarios/bench/$scenario.yaml" "$verify_file"
    if "$DIFFTEST" run --scenario "$verify_file" \
      --a-url "http://127.0.0.1:$DJANGO_PORT" --b-url "http://127.0.0.1:$API_PORT" \
      --a-db "$DJANGO_DB" --b-db "$API_DB" \
      --a-redis-db "$DJANGO_REDIS_DB" --b-redis-db "$API_REDIS_DB" \
      --a-vhost "$DJANGO_VHOST" --b-vhost "$API_VHOST" \
      --report "$out/verify-$scenario.json" >"$out/verify-$scenario.log" 2>&1; then
      note "  $scenario: identical"
    else
      note "  $scenario: DIFF (see $out/verify-$scenario.log)"
      return 1
    fi
  done
}

check_targets() {
  local name
  for name in "$DJANGO_CONTAINER" "$API_CONTAINER" "$STATS_CONTAINER"; do
    [[ "$(docker inspect --format '{{.State.Running}}' "$name" 2>/dev/null)" == "true" ]] ||
      die "$name is not running"
  done
  for vhost in "$DJANGO_VHOST" "$API_VHOST"; do
    if ! rabbit_api -o /dev/null "$MGMT/api/vhosts/$vhost" >/dev/null 2>&1; then
      note "WARNING: RabbitMQ vhost $vhost disappeared; recreating it"
      create_vhost "$vhost" 600000
    fi
  done
}

# ---------------------------------------------------------------------------
# Measurement
# ---------------------------------------------------------------------------
duration_for() {
  case "$1" in
    reads-mixed) printf '%s' "$MIXED_LATENCY_DURATION" ;;
    "$WRITE_SCENARIO") printf '%s' "$WRITE_DURATION" ;;
    *) printf '%s' "$LATENCY_DURATION" ;;
  esac
}

measure_latency() {
  local scenario="$1" target="$2" spec_target="$3" port="$4" database="$5"
  local duration
  duration="$(duration_for "$scenario")"
  note "latency $scenario on $target: concurrency 1, ${duration}s window, one repetition"
  check_targets
  load_note "before $scenario/$target latency (duration $duration)"
  : >"$out/events-$scenario-$target.log"
  "$DIFFTEST" bench \
    --scenario "scenarios/bench/$scenario.yaml" \
    --target "$spec_target" \
    --url "http://127.0.0.1:$port" --db "$database" \
    --concurrency 1 --duration "$duration" --warmup 5 --settle 10 --repetitions 1 \
    --headroom-duration 0 \
    --report "$out/report-$scenario-$target.json" \
    2>&1 | "$SYSTEM_PYTHON" "$TOOLS_DIR/stamp.py" "$out/events-$scenario-$target.log"
  load_note "after $scenario/$target latency (duration $duration)"
  "$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" summarize \
    --samples "$out/samples-api.log" \
    --events "$out/events-$scenario-$target.log" \
    --report "$out/report-$scenario-$target.json" \
    --target "$target" \
    --duration "$duration" --warmup 5 \
    --out "$out/row-$scenario-$target.json"
}

measure_load() {
  local scenario="reads-mixed" target="$1" spec_target="$2" port="$3" database="$4"
  note "load $scenario on $target: concurrency $LOAD_CONCURRENCY, ${LOAD_DURATION}s window, one repetition"
  check_targets
  load_note "before $scenario/$target load (concurrency $LOAD_CONCURRENCY)"
  : >"$out/events-load-$scenario-$target.log"
  "$DIFFTEST" bench \
    --scenario "scenarios/bench/$scenario.yaml" \
    --target "$spec_target" \
    --url "http://127.0.0.1:$port" --db "$database" \
    --concurrency "$LOAD_CONCURRENCY" --duration "$LOAD_DURATION" --warmup 5 \
    --settle 10 --repetitions 1 \
    --headroom-duration 0 \
    --report "$out/report-load-$scenario-$target.json" \
    2>&1 | "$SYSTEM_PYTHON" "$TOOLS_DIR/stamp.py" "$out/events-load-$scenario-$target.log"
  load_note "after $scenario/$target load (concurrency $LOAD_CONCURRENCY)"
  "$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" summarize \
    --samples "$out/samples-api.log" \
    --events "$out/events-load-$scenario-$target.log" \
    --report "$out/report-load-$scenario-$target.json" \
    --target "$target" \
    --duration "$LOAD_DURATION" --warmup 5 \
    --out "$out/row-load-$scenario-$target.json"
}

# ---------------------------------------------------------------------------
# plane-worker with a live queue, plane-beat after a tick
# ---------------------------------------------------------------------------
run_as_jobs() {
  docker run -d --name "$1" \
    --network "$NETWORK" \
    --cpus 2 --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-p8-jobs.env" \
    "${@:2}" >/dev/null
}

sample_worker_live() {
  note "starting $WORKER_CONTAINER and waiting for its LISTEN connection"
  run_as_jobs "$WORKER_CONTAINER" "$WORKER_IMAGE"
  # Phase 8 finding: at this head the listener task never announces a LISTEN
  # (see BENCH.md); it deadlocks on `batch_execute` before the connection
  # future is polled, and the worker drains through its 1 s poll fallback. The
  # measurement records the state instead of failing, so the worker's live-queue
  # footprint is still measured; the exact connection list is kept below.
  if ! try_wait_log "$WORKER_CONTAINER" '"job listener connected"' 20; then
    note "  no 'job listener connected' line in 20 s (recorded as a finding)"
  fi

  psql_dev "$JOBS_DB" -c \
    "SELECT pid, state, wait_event_type, wait_event, left(coalesce(query, ''), 70) AS query
       FROM pg_stat_activity WHERE datname = current_database() ORDER BY pid" \
    >"$out/worker-connections.txt"
  psql_dev "$JOBS_DB" -tAc \
    "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND query ILIKE 'LISTEN%'" \
    >"$out/worker-listen.txt"

  note "sampling the worker idle with the listener up, then with $WORKER_CONCURRENCY claimed jobs"
  start_sampler worker "worker:$(container_id "$WORKER_CONTAINER")"
  : >"$out/events-worker.log"
  sleep 10
  mark "$out/events-worker.log" "worker:idle-start"
  sleep 5
  mark "$out/events-worker.log" "worker:idle-end"

  psql_dev "$JOBS_DB" -c \
    "INSERT INTO plane_jobs.jobs (id, task, payload, state)
     SELECT gen_random_uuid(), 'plane.jobs.echo', json_build_object('sleep_ms', 20000), 'queued'
     FROM generate_series(1, $WORKER_CONCURRENCY)" >/dev/null
  sleep 2
  psql_dev "$JOBS_DB" -c \
    "SELECT state, count(*) AS jobs, min(locked_by) AS worker FROM plane_jobs.jobs GROUP BY state ORDER BY state" \
    >"$out/worker-claims.txt"
  mark "$out/events-worker.log" "worker:live-start"
  sleep 5
  mark "$out/events-worker.log" "worker:live-end"
  psql_dev "$JOBS_DB" -tAc \
    "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND query ILIKE 'LISTEN%'" \
    >>"$out/worker-listen.txt"

  wait_sql_count "$JOBS_DB" "SELECT count(*) FROM plane_jobs.jobs WHERE state = 'done'" "$WORKER_CONCURRENCY" 60 || true
  stop_sampler worker
  docker logs "$WORKER_CONTAINER" >"$out/worker.log" 2>&1 || true
  grep -c '"echo"' "$out/worker.log" >"$out/worker-echo-count.txt" || true
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-worker.log" \
    --events "$out/events-worker.log" --target worker \
    --start worker:idle-start --end worker:idle-end --out "$out/idle-worker.json"
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-worker.log" \
    --events "$out/events-worker.log" --target worker \
    --start worker:live-start --end worker:live-end --out "$out/live-worker.json"
  docker stop -t 10 "$WORKER_CONTAINER" >/dev/null
  docker rm "$WORKER_CONTAINER" >/dev/null
}

sample_beat_tick() {
  note "starting $BEAT_CONTAINER once to seed the schedule, then marking it handed over and due"
  run_as_jobs "$BEAT_CONTAINER" "$BEAT_IMAGE"
  wait_log "$BEAT_CONTAINER" '"starting"' 120
  docker stop -t 10 "$BEAT_CONTAINER" >/dev/null
  docker rm "$BEAT_CONTAINER" >/dev/null
  # All Rust-owned entries are crontabs (the only interval row is
  # push-instance-metrics, Celery-owned). Backdating every Rust row by a day
  # makes each schedule's latest due tick strictly newer than `last_run_at`, so
  # the next start fires one queued job per real entry instead of waiting for a
  # five-minute or daily boundary. The jobs stay queued (the worker is not
  # running during this sample).
  psql_dev "$JOBS_DB" -c \
    "UPDATE plane_jobs.schedules
        SET handover_at = coalesce(handover_at, now()),
            last_run_at = now() - interval '1 day',
            updated_at = now()
      WHERE owner = 'rust'" >/dev/null

  note "restarting $BEAT_CONTAINER and waiting for a real schedule tick"
  run_as_jobs "$BEAT_CONTAINER" "$BEAT_IMAGE"
  wait_log "$BEAT_CONTAINER" 'fired schedule' 120
  psql_dev "$JOBS_DB" -c \
    "SELECT schedule_name, count(*) AS ticks FROM plane_jobs.ticks GROUP BY schedule_name ORDER BY schedule_name" \
    >"$out/beat-ticks.txt"
  psql_dev "$JOBS_DB" -tAc "SELECT count(*) FROM plane_jobs.ticks" >"$out/beat-tick-count.txt"
  docker logs "$BEAT_CONTAINER" >"$out/beat.log" 2>&1 || true

  start_sampler beat "beat:$(container_id "$BEAT_CONTAINER")"
  : >"$out/events-beat.log"
  sleep 10
  mark "$out/events-beat.log" "beat:idle-start"
  sleep 5
  mark "$out/events-beat.log" "beat:idle-end"
  stop_sampler beat
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-beat.log" \
    --events "$out/events-beat.log" --target beat \
    --start beat:idle-start --end beat:idle-end --out "$out/idle-beat.json"
  docker stop -t 10 "$BEAT_CONTAINER" >/dev/null
  docker rm "$BEAT_CONTAINER" >/dev/null
}

# ---------------------------------------------------------------------------
# Task throughput: plane-worker vs celery worker, the same 10,000
# `recent_visited_task` jobs, both paused while the queue is filled.
# ---------------------------------------------------------------------------
drain_rust() {
  note "throughput: $DRAIN_JOBS recent_visited_task jobs through $WORKER_CONTAINER"
  psql_dev "$JOBS_DB" -c "DELETE FROM plane_jobs.jobs" >/dev/null
  run_as_jobs "$WORKER_CONTAINER" "$WORKER_IMAGE"
  try_wait_log "$WORKER_CONTAINER" '"job listener connected"' 20 ||
    note "  no 'job listener connected' line (Phase 8 finding; poll fallback is used)"
  start_sampler drain-rust "worker:$(container_id "$WORKER_CONTAINER")"
  : >"$out/events-drain-rust.log"
  mark "$out/events-drain-rust.log" "drain-start"
  psql_dev "$JOBS_DB" -c \
    "INSERT INTO plane_jobs.jobs (id, task, payload, state)
     SELECT gen_random_uuid(),
            'plane.bgtasks.recent_visited_task.recent_visited_task',
            json_build_object(
              'args', '[]'::json,
              'kwargs', json_build_object(
                'slug', 'fix-workspace',
                'entity_name', 'project',
                'entity_identifier', 'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
                'project_id', 'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
                'user_id', 'd44ae570-3de4-4413-af08-f686bf5b7235')),
            'queued'
     FROM generate_series(1, $DRAIN_JOBS)" >/dev/null
  if wait_sql_count "$JOBS_DB" \
    "SELECT count(*) FROM plane_jobs.jobs WHERE state IN ('done', 'dead')" "$DRAIN_JOBS" 300; then
    note "  drained in time"
  else
    note "  drain did not finish; recording what was done"
  fi
  mark "$out/events-drain-rust.log" "drain-end"
  stop_sampler drain-rust
  psql_dev "$JOBS_DB" -c \
    "SELECT state, count(*) AS jobs FROM plane_jobs.jobs GROUP BY state ORDER BY state" \
    >"$out/drain-rust-states.txt"
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-drain-rust.log" \
    --events "$out/events-drain-rust.log" --target worker \
    --start drain-start --end drain-end --out "$out/drain-rust.json"
  docker stop -t 30 "$WORKER_CONTAINER" >/dev/null
  docker rm "$WORKER_CONTAINER" >/dev/null
}

enqueue_celery_jobs() {
  local count="$1"
  (
    cd "$PLANE_API_DIR"
    export PLANE_DB="$DJANGO_DB" PLANE_REDIS_DB="$DJANGO_REDIS_DB" PLANE_MQ_VHOST="$DJANGO_VHOST"
    eval "$("$PLANE_DEV_DIR/django-env.sh")"
    "$PLANE_DJANGO_PYTHON" - "$count" <<'PY'
import os
import sys
import time

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
import django

django.setup()

from plane.bgtasks.recent_visited_task import recent_visited_task

count = int(sys.argv[1])
args = (
    "project",
    "b0063033-54f9-49d8-b5f3-6b7a1551c6a6",
    "d44ae570-3de4-4413-af08-f686bf5b7235",
    "b0063033-54f9-49d8-b5f3-6b7a1551c6a6",
    "fix-workspace",
)
started = time.time()
for _ in range(count):
    recent_visited_task.delay(*args)
print(f"enqueued {count} in {time.time() - started:.2f}s")
PY
  )
}

celery_processed() {
  local total
  total="$(
    cd "$PLANE_API_DIR"
    export PLANE_DB="$DJANGO_DB" PLANE_REDIS_DB="$DJANGO_REDIS_DB" PLANE_MQ_VHOST="$DJANGO_VHOST"
    eval "$("$PLANE_DEV_DIR/django-env.sh")"
    "$PLANE_API_DIR/.venv/bin/celery" -A plane inspect stats --json --timeout 10 2>/dev/null |
      "$SYSTEM_PYTHON" -c '
import json
import sys

task = "plane.bgtasks.recent_visited_task.recent_visited_task"
try:
    payload = json.load(sys.stdin)
except Exception:
    print(0)
    raise SystemExit(0)
total = 0
for worker in payload.values():
    total += int(worker.get("total", {}).get(task, 0))
print(total)
'
  )" || total=0
  printf '%s' "${total:-0}"
}

wait_celery_processed() {
  local expected="$1"
  local timeout="${2:-300}"
  local deadline=$((SECONDS + timeout)) done_count
  while :; do
    done_count="$(celery_processed)"
    [[ "$done_count" -ge "$expected" ]] && return 0
    ((SECONDS < deadline)) || {
      note "timed out waiting for $expected Celery tasks (last $done_count)"
      return 1
    }
    sleep 5
  done
}

sample_celery_worker_idle() {
  note "starting $CELERY_CONTAINER (celery -c 4, prefork) for its idle footprint"
  # The write matrix published to this vhost. Recreate it empty so nothing is
  # consumed during the idle window.
  delete_vhost "$DJANGO_VHOST"
  create_vhost "$DJANGO_VHOST" 600000
  docker run -d --name "$CELERY_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 --memory 1g --memory-swap 1g \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-p8-django.env" \
    "$DJANGO_IMAGE" \
    /bin/bash -c 'exec celery -A plane worker -l info -c 4' >/dev/null
  wait_log "$CELERY_CONTAINER" 'mingle: all alone' 240
  start_sampler celery "celery:$(container_id "$CELERY_CONTAINER")"
  : >"$out/events-celery.log"
  sleep 10
  mark "$out/events-celery.log" "celery:idle-start"
  sleep 5
  mark "$out/events-celery.log" "celery:idle-end"
  stop_sampler celery
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-celery.log" \
    --events "$out/events-celery.log" --target celery \
    --start celery:idle-start --end celery:idle-end --out "$out/idle-celery.json"
  docker stop -t 30 "$CELERY_CONTAINER" >/dev/null
  docker rm "$CELERY_CONTAINER" >/dev/null
}

drain_celery() {
  note "throughput: $DRAIN_JOBS recent_visited_task messages through celery worker (-P solo)"
  # The prefork worker's children do not execute a single task in this
  # environment (Phase 6 recorded the same prefork hang; `-P solo` works).
  # The drain therefore uses one slot, a lower bound on Celery's concurrency-4
  # throughput, and is labelled as such in BENCH.md.
  delete_vhost "$DJANGO_VHOST"
  create_vhost "$DJANGO_VHOST" 600000
  docker run -d --name "$CELERY_SOLO_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 --memory 1g --memory-swap 1g \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-p8-django.env" \
    "$DJANGO_IMAGE" \
    /bin/bash -c 'exec celery -A plane worker -l info -P solo' >/dev/null
  wait_log "$CELERY_SOLO_CONTAINER" 'mingle: all alone' 240
  rabbit_api -X DELETE "$MGMT/api/queues/$DJANGO_VHOST/celery/contents" >/dev/null 2>&1 || true
  start_sampler drain-celery "celery-solo:$(container_id "$CELERY_SOLO_CONTAINER")"
  : >"$out/events-drain-celery.log"
  mark "$out/events-drain-celery.log" "drain-start"
  local enqueue_start=$SECONDS
  enqueue_celery_jobs "$DRAIN_JOBS" | tee "$out/celery-enqueue.log"
  note "enqueue took $((SECONDS - enqueue_start)) s"
  if wait_celery_processed "$DRAIN_JOBS" 300; then
    note "  drained in time"
  else
    note "  drain did not finish; recording what was processed"
  fi
  mark "$out/events-drain-celery.log" "drain-end"
  stop_sampler drain-celery
  docker logs "$CELERY_SOLO_CONTAINER" >"$out/celery.log" 2>&1 || true
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-drain-celery.log" \
    --events "$out/events-drain-celery.log" --target celery-solo \
    --start drain-start --end drain-end --out "$out/drain-celery.json"
  docker stop -t 30 "$CELERY_SOLO_CONTAINER" >/dev/null
  docker rm "$CELERY_SOLO_CONTAINER" >/dev/null
}

sample_celery_beat() {
  note "starting $CELERY_BEAT_CONTAINER and sampling its idle memory"
  docker run -d --name "$CELERY_BEAT_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 --memory 1g --memory-swap 1g \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-p8-django.env" \
    "$DJANGO_IMAGE" \
    /bin/bash -c 'exec celery -A plane beat -l info' >/dev/null
  wait_log "$CELERY_BEAT_CONTAINER" 'celery beat v' 180
  local rabbit_id
  rabbit_id="$(container_id plane-rs-dev-rabbitmq-1)"
  start_sampler celery-beat "celery-beat:$(container_id "$CELERY_BEAT_CONTAINER"),rabbitmq:$rabbit_id"
  : >"$out/events-celery-beat.log"
  sleep 15
  mark "$out/events-celery-beat.log" "celery-beat:start"
  sleep 5
  mark "$out/events-celery-beat.log" "celery-beat:end"
  stop_sampler celery-beat
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-celery-beat.log" \
    --events "$out/events-celery-beat.log" --target celery-beat \
    --start celery-beat:start --end celery-beat:end --out "$out/idle-celery-beat.json"
  "$SYSTEM_PYTHON" "$TOOLS_DIR/window.py" --samples "$out/samples-celery-beat.log" \
    --events "$out/events-celery-beat.log" --target rabbitmq \
    --start celery-beat:start --end celery-beat:end --out "$out/idle-rabbitmq.json"
  docker stop -t 30 "$CELERY_BEAT_CONTAINER" >/dev/null
  docker rm "$CELERY_BEAT_CONTAINER" >/dev/null
}

# ---------------------------------------------------------------------------
# Tables and totals
# ---------------------------------------------------------------------------
merge_report() {
  local report="$1" row="$2" target="$3"
  "$SYSTEM_PYTHON" - "$report" "$row" "$out/merged-$(basename "$report" .json).json" <<'PY'
import json
import sys

report_path, row_path, out_path = sys.argv[1:4]
with open(report_path, "r", encoding="utf-8") as handle:
    report = json.load(handle)
with open(row_path, "r", encoding="utf-8") as handle:
    row = json.load(handle)
container = {median["concurrency"]: median for median in row["medians"]}
for median in report["medians"]:
    source = container[median["concurrency"]]
    median["idle_rss_bytes"] = source["idle_rss_bytes"]
    median["peak_rss_bytes"] = source["peak_rss_bytes"]
    median["cpu_secs"] = source["cpu_secs"]
with open(out_path, "w", encoding="utf-8") as handle:
    json.dump(report, handle, indent=2)
PY
}

render_tables() {
  local reports=() scenario target report row
  for scenario in "${SCENARIOS[@]}" "$WRITE_SCENARIO"; do
    for target in django api; do
      report="$out/report-$scenario-$target.json"
      row="$out/row-$scenario-$target.json"
      [[ -f "$report" && -f "$row" ]] || continue
      merge_report "$report" "$row" "$target"
      reports+=("$out/merged-$(basename "$report" .json).json")
    done
  done
  for target in django api; do
    report="$out/report-load-reads-mixed-$target.json"
    row="$out/row-load-reads-mixed-$target.json"
    [[ -f "$report" && -f "$row" ]] || continue
    merge_report "$report" "$row" "$target"
    reports+=("$out/merged-$(basename "$report" .json).json")
  done
  if ((${#reports[@]} > 0)); then
    "$SYSTEM_PYTHON" "$TOOLS_DIR/summarize.py" \
      --commit "$COMMIT" --date "$BENCH_DATE" "${reports[@]}" >"$out/summarize.md"
    note "wrote $out/summarize.md"
  fi
  "$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" table --rows "$out" \
    --out "$out/table.md" --ratios "$out/ratios.md" >/dev/null
  note "wrote $out/table.md and $out/ratios.md"
}

render_totals() {
  "$SYSTEM_PYTHON" - "$out" <<'PY'
import json
import os
import sys

out = sys.argv[1]
MIB = 1024.0 * 1024.0


def read(name):
    path = os.path.join(out, name)
    if not os.path.exists(path):
        return None
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


api = read("idle-api.json")
worker = read("idle-worker.json")
beat = read("idle-beat.json")
celery = read("idle-celery.json")
celery_beat = read("idle-celery-beat.json")
rabbit = read("idle-rabbitmq.json")
drain_rust = read("drain-rust.json")
drain_celery = read("drain-celery.json")


def mib(value):
    return f"{value / MIB:.1f}" if value is not None else "—"


def idle_of(entry, key="median_rss_bytes"):
    return entry.get(key) if entry else None

rows = [
    ("rust", "plane-api (fresh idle)", (api or {}).get("api", {}).get("start", {}).get("rss_bytes")),
    ("rust", "plane-worker (LISTEN up)", idle_of(worker)),
    ("rust", "plane-beat (after a tick)", idle_of(beat)),
    ("python", "django api (fresh idle)", (api or {}).get("django", {}).get("start", {}).get("rss_bytes")),
    ("python", "celery worker -c 4", idle_of(celery)),
    ("python", "celery beat", idle_of(celery_beat)),
    ("python", "rabbitmq (dev container)", idle_of(rabbit)),
]
lines = ["| stack | service | idle (cache-excluded, MiB) |", "| --- | --- | --- |"]
for stack, service, value in rows:
    lines.append(f"| {stack} | {service} | {mib(value)} |")

rust_total = sum(
    value
    for value in [
        (api or {}).get("api", {}).get("start", {}).get("rss_bytes"),
        idle_of(worker),
        idle_of(beat),
    ]
    if value is not None
)
python_total = sum(
    value
    for value in [
        (api or {}).get("django", {}).get("start", {}).get("rss_bytes"),
        idle_of(celery),
        idle_of(celery_beat),
        idle_of(rabbit),
    ]
    if value is not None
)
lines.append(f"| rust | **total** | **{mib(rust_total)}** |")
lines.append(f"| python | **total** | **{mib(python_total)}** |")

with open(os.path.join(out, "stack.md"), "w", encoding="utf-8") as handle:
    handle.write("\n".join(lines) + "\n")
print("\n".join(lines))

if drain_rust and drain_celery:
    jobs = 10000
    lines = [
        "| drain | jobs | wall s | jobs/s | CPU s | CPU per job ms | requests/s at CPU 1 |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for name, entry in [("plane-worker", drain_rust), ("celery -c 4", drain_celery)]:
        wall = entry["duration_secs"]
        cpu = entry["cpu_secs"]
        lines.append(
            "| {} | {} | {:.2f} | {:.0f} | {:.2f} | {:.3f} | {:.0f} |".format(
                name,
                jobs,
                wall,
                jobs / wall if wall else 0,
                cpu,
                cpu * 1000.0 / jobs if jobs else 0,
                jobs / cpu if cpu else 0,
            )
        )
    with open(os.path.join(out, "throughput.md"), "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")
    print("\n".join(lines))
PY
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
preflight
build_images
DJANGO_BASE_PYTHON="$(grep -m1 '^FROM python:' "$PLANE_API_DIR/Dockerfile.api" |
  sed -E 's/^FROM ([^ ]+).*/\1/')"
[[ -n "$DJANGO_BASE_PYTHON" ]] || die "could not read the Python base image from Dockerfile.api"

cleanup
provision
if ((jobs_only == 1)); then
  migrate_jobs_schema "$JOBS_DB" "$run_dir/bench-p8-jobs.env" "migrator-jobs.log"
  sample_worker_live
  sample_beat_tick
  drain_rust
  sample_celery_worker_idle
  drain_celery
  sample_celery_beat
  render_totals
  if ((keep == 0)); then
    cleanup
  fi
  note "wrote $out"
  exit 0
fi
migrate_jobs_schema "$API_DB" "$run_dir/bench-p8-api.env" "migrator-api.log"
if ((skip_jobs == 0 || skip_drain == 0)) && ((load_only == 0)); then
  migrate_jobs_schema "$JOBS_DB" "$run_dir/bench-p8-jobs.env" "migrator-jobs.log"
fi
start_containers

if ((load_only == 1)); then
  : >"$out/events.log"
  : >"$out/load.log"
  start_sampler api "django:$(container_id "$DJANGO_CONTAINER"),api:$(container_id "$API_CONTAINER")"
  sleep 30
  mark "$out/events.log" "django:start"
  mark "$out/events.log" "api:start"
  verify reads-mixed
  measure_load django django "$DJANGO_PORT" "$DJANGO_DB"
  measure_load api rust "$API_PORT" "$API_DB"
  sleep 30
  mark "$out/events.log" "django:run-end"
  mark "$out/events.log" "api:run-end"
  stop_sampler api
  "$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" idle --samples "$out/samples-api.log" \
    --events "$out/events.log" --out "$out/idle-api.json"
  render_tables
  if ((keep == 0)); then
    cleanup
  fi
  note "wrote $out"
  exit 0
fi

{
  printf '# Phase 8 run %s\n' "$tag"
  printf 'date: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'commit: %s\n' "$COMMIT"
  printf 'kernel: %s\n' "$(docker run --rm alpine:3.20 uname -r 2>/dev/null || echo unknown)"
  printf 'django_image_bytes: %s (%s on disk)\n' "$(image_size "$DJANGO_IMAGE")" "$(image_disk_size "$DJANGO_IMAGE")"
  printf 'api_image_bytes: %s (%s on disk)\n' "$(image_size "$API_IMAGE")" "$(image_disk_size "$API_IMAGE")"
  printf 'worker_image_bytes: %s (%s on disk)\n' "$(image_size "$WORKER_IMAGE")" "$(image_disk_size "$WORKER_IMAGE")"
  printf 'beat_image_bytes: %s (%s on disk)\n' "$(image_size "$BEAT_IMAGE")" "$(image_disk_size "$BEAT_IMAGE")"
  printf 'migrator_image_bytes: %s (%s on disk)\n' "$(image_size "$MIGRATOR_IMAGE")" "$(image_disk_size "$MIGRATOR_IMAGE")"
  printf 'rustc: %s\n' "$(docker run --rm rust:1.99.1-slim-bookworm rustc --version 2>/dev/null || echo unknown)"
  printf 'python_stack: %s\n' "$(docker run --rm --entrypoint python "$DJANGO_IMAGE" -c 'import django, rest_framework, gunicorn, uvicorn; print(f"Django {django.get_version()}, DRF {rest_framework.VERSION}, gunicorn {gunicorn.__version__}, uvicorn {uvicorn.__version__}")' 2>/dev/null || echo unknown)"
  printf 'vm_cpus: %s\n' "$(docker run --rm alpine:3.20 nproc 2>/dev/null || echo unknown)"
  printf 'vm_memory: %s\n' "$(docker run --rm alpine:3.20 sh -c 'grep MemTotal /proc/meminfo' 2>/dev/null || echo unknown)"
  printf 'bench_container_cpus: 2\n'
  printf 'bench_container_memory_mib: 512 (HTTP targets and Rust background services), 1024 (Celery)\n'
  printf 'latency_duration_seconds: reads-mixed %s, writes-mixed %s, others %s\n' \
    "$MIXED_LATENCY_DURATION" "$WRITE_DURATION" "$LATENCY_DURATION"
  printf 'load: reads-mixed at concurrency %s for %s s\n' "$LOAD_CONCURRENCY" "$LOAD_DURATION"
  printf 'drain_jobs: %s, worker concurrency %s\n' "$DRAIN_JOBS" "$WORKER_CONCURRENCY"
} >"$out/environment.txt"

: >"$out/events.log"
: >"$out/load.log"
: >"$out/pg-load.log"
: >"$out/samples-api.log"
rm -f "$out/pg-load.stop"
"$PLANE_SERVER_DIR/tools/bench/pg-load.sh" "$out/pg-load.log" "$out/pg-load.stop" 5 >/dev/null 2>&1 &
pg_load_pid=$!
disown "$pg_load_pid" 2>/dev/null || true
load_note "run start"
uptime >>"$out/load.log"

start_sampler api "django:$(container_id "$DJANGO_CONTAINER"),api:$(container_id "$API_CONTAINER")"
note "settling 30 s before the fresh-container idle sample"
sleep 30
mark "$out/events.log" "django:start"
mark "$out/events.log" "api:start"

verify "${SCENARIOS[@]}" "$WRITE_SCENARIO"
if ((verify_only == 1)); then
  stop_sampler api
  stop_pg_load
  if ((keep == 0)); then cleanup; fi
  note "wrote $out"
  exit 0
fi

for scenario in "${SCENARIOS[@]}"; do
  measure_latency "$scenario" django django "$DJANGO_PORT" "$DJANGO_DB"
  measure_latency "$scenario" api rust "$API_PORT" "$API_DB"
done
# The load window runs before the write mix: the write mix adds activity rows
# to the issue `reads-mixed` reads its history from, and that made the
# concurrency-8 window incomparable with the latency rows in the first
# Phase 8 run (BENCH.md records the re-measurement).
measure_load django django "$DJANGO_PORT" "$DJANGO_DB"
measure_load api rust "$API_PORT" "$API_DB"
measure_latency "$WRITE_SCENARIO" django django "$DJANGO_PORT" "$DJANGO_DB"
measure_latency "$WRITE_SCENARIO" api rust "$API_PORT" "$API_DB"

note "settling 30 s before the final container idle sample"
sleep 30
mark "$out/events.log" "django:run-end"
mark "$out/events.log" "api:run-end"
stop_sampler api
load_note "API matrix end"
uptime >>"$out/load.log"

if ((skip_jobs == 0)); then
  sample_worker_live
  sample_beat_tick
fi
if ((skip_drain == 0)); then
  drain_rust
  sample_celery_worker_idle
  drain_celery
  sample_celery_beat
fi

stop_pg_load
"$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" idle --samples "$out/samples-api.log" \
  --events "$out/events.log" --out "$out/idle-api.json"
load_note "run end"
uptime >>"$out/load.log"
render_tables
render_totals

if ((keep == 0)); then
  cleanup
else
  note "--keep: containers and databases left running"
fi
note "wrote $out"
