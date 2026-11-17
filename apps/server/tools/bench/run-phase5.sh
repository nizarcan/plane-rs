#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Phase 5 re-measurement: the Phase 3 read matrix on a quiet stack, plus the
# background services' container footprint.
#
# This is `run-phase3.sh` with a distinct namespace and the Phase 5 budget:
#
#   * five read scenarios, concurrency 1, one repetition ("50 sequential
#     requests per route": the durations below give every route at least 50
#     requests on the slowest target, `reads-mixed` 70 s for its weight-1
#     routes, the rest 30 s);
#   * `reads-mixed` at concurrency 8, one 20 s window (the harness's unit);
#   * `plane-worker` and `plane-beat` in containers with the same `--cpus 2`,
#     `--memory 512m` limits, sampled idle for 30 s.
#
#   apps/server/tools/bench/run-phase5.sh
#   apps/server/tools/bench/run-phase5.sh --no-build --scenarios reads-mixed
#   apps/server/tools/bench/run-phase5.sh --verify-only
#   apps/server/tools/bench/run-phase5.sh --down
#
# Isolation: containers are named `plane-rs-bench-p5-*` on the dev network and
# publish 127.0.0.1:18310 (Django) and 127.0.0.1:18360 (plane-api). Databases
# `bench_p5_django`, `bench_p5_api`, `bench_p5_jobs` are clones of
# `plane_fix_tpl`; Redis DBs 10-12 and RabbitMQ vhosts `bench_p5_*` are this
# run's. Nothing else on the VM is touched.
#
# Secrets (SECRET_KEY) are read through dev/django-env.sh and written to
# apps/server/dev/run/, which is gitignored; they are never printed.

set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

DJANGO_IMAGE="plane-rs-bench-django:latest"
API_IMAGE="plane-rs-bench-api:latest"
WORKER_IMAGE="plane-rs-bench-worker:latest"
BEAT_IMAGE="plane-rs-bench-beat:latest"
MIGRATOR_IMAGE="plane-rs-bench-migrator:latest"

DJANGO_CONTAINER="plane-rs-bench-p5-django"
API_CONTAINER="plane-rs-bench-p5-api"
STATS_CONTAINER="plane-rs-bench-p5-stats"
WORKER_CONTAINER="plane-rs-bench-p5-worker"
BEAT_CONTAINER="plane-rs-bench-p5-beat"
MIGRATOR_CONTAINER="plane-rs-bench-p5-migrator"

DJANGO_DB="bench_p5_django"
API_DB="bench_p5_api"
JOBS_DB="bench_p5_jobs"
DJANGO_REDIS_DB=10
API_REDIS_DB=11
JOBS_REDIS_DB=12
DJANGO_VHOST="bench_p5_django"
API_VHOST="bench_p5_api"
JOBS_VHOST="bench_p5_jobs"
DJANGO_PORT=18310
API_PORT=18360
NETWORK="plane-rs-dev_default"
SCENARIOS=(reads-users-me reads-project reads-issues reads-cycles reads-mixed)
DIFFTEST="$PLANE_SERVER_DIR/target/release/difftest"
SYSTEM_PYTHON="$(command -v python3)"
COMMIT="2a80b3f1f5"
BENCH_DATE="2026-11-17"
LATENCY_DURATION=30
MIXED_LATENCY_DURATION=70
LOAD_DURATION=20
LOAD_CONCURRENCY=8

usage() {
  cat <<'EOF'
usage: run-phase5.sh [--keep] [--no-build] [--verify-only] [--down] [--tag NAME]
                     [--scenarios LIST] [--skip-jobs]

  --keep          leave the containers and databases running at the end
  --no-build      reuse the images and target/release/difftest
  --verify-only   start, run the differential verification, stop
  --down          remove what an earlier run left behind
  --tag NAME      report directory under target/bench (default phase5-<timestamp>)
  --scenarios LIST comma-separated subset of the five reads-* scenarios
  --skip-jobs     skip the plane-worker/plane-beat idle measurement
EOF
}

keep=0
skip_build=0
verify_only=0
down_only=0
skip_jobs=0
scenarios_csv=""
tag="phase5-$(date +%Y%m%d-%H%M%S)"
while (($# > 0)); do
  case "$1" in
    --keep) keep=1 ;;
    --no-build) skip_build=1 ;;
    --verify-only) verify_only=1 ;;
    --down) down_only=1 ;;
    --skip-jobs) skip_jobs=1 ;;
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
  [[ "$name" == plane-rs-bench-* ]] || die "refusing to remove container '$name'"
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
  local vhost="$1"
  rabbit_api -X PUT "$MGMT/api/vhosts/$vhost" >/dev/null
  rabbit_api -X PUT -H 'content-type: application/json' \
    -d '{"configure":".*","write":".*","read":".*"}' \
    "$MGMT/api/permissions/$vhost/$PLANE_RABBITMQ_USER" >/dev/null
  # Same 2 s TTL as Phase 3: the read paths publish `recent_visited_task` and
  # no consumer drains these vhosts, so the queue must not grow during a run.
  rabbit_api -X PUT -H 'content-type: application/json' \
    -d '{"pattern":".*","definition":{"message-ttl":2000},"priority":0,"apply-to":"queues"}' \
    "$MGMT/api/policies/$vhost/bench-ttl" >/dev/null
}

delete_vhost() {
  rabbit_api -X DELETE "$MGMT/api/vhosts/$1" >/dev/null 2>&1 || true
}

stop_sampler() {
  local phase="$1" pid
  if [[ -f "$out/samples-$phase.pid" ]]; then
    pid="$(cat "$out/samples-$phase.pid")"
    # Only kill the pipeline this script started; a stale pid file from a
    # crashed run can name an unrelated process after a reboot.
    if ps -p "$pid" -o command= 2>/dev/null | grep -q "stamp.py"; then
      kill "$pid" 2>/dev/null || true
    fi
    rm -f "$out/samples-$phase.pid"
  fi
  remove_container "$STATS_CONTAINER"
}

cleanup() {
  note "stopping bench containers"
  stop_sampler api || true
  stop_sampler worker || true
  stop_sampler beat || true
  remove_container "$MIGRATOR_CONTAINER"
  remove_container "$WORKER_CONTAINER"
  remove_container "$BEAT_CONTAINER"
  remove_container "$DJANGO_CONTAINER"
  remove_container "$API_CONTAINER"
  note "dropping bench databases, vhosts and Redis DBs"
  for database in "$DJANGO_DB" "$API_DB" "$JOBS_DB"; do
    drop_database "$database" || true
  done
  for vhost in "$DJANGO_VHOST" "$API_VHOST" "$JOBS_VHOST"; do
    delete_vhost "$vhost"
  done
  for redis_db in "$DJANGO_REDIS_DB" "$API_REDIS_DB" "$JOBS_REDIS_DB"; do
    flush_redis "$redis_db" || true
  done
  rm -f "$run_dir/bench-django.env" "$run_dir/bench-api.env" "$run_dir/bench-jobs.env"
}

MGMT="http://127.0.0.1:$PLANE_RABBITMQ_MGMT_PORT"

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
  local host_free_kib vm_available_mib min_free_kib
  min_free_kib=$((30 * 1024 * 1024))
  host_free_kib="$(df -k "$PLANE_REPO_ROOT" | awk 'NR==2 {print $4}')"
  vm_available_mib="$(docker run --rm alpine:3.20 sh -c \
    "awk '/MemAvailable/{print int(\$2 / 1024)}' /proc/meminfo" 2>/dev/null || echo 0)"
  note "free space: host $((host_free_kib / 1024 / 1024)) GiB, Docker VM ${vm_available_mib} MiB memory"
  ((host_free_kib >= min_free_kib)) ||
    die "the host filesystem holding the repo has $((host_free_kib / 1024 / 1024)) GiB free; keep at least 30 GiB"
  if ((skip_build == 0)) && ((vm_available_mib < 700)); then
    die "the Docker VM has ${vm_available_mib} MiB available and the fat-LTO release build needs ~700+ MiB"
  fi
}

build_images() {
  if ((skip_build == 1)); then
    for image in "$DJANGO_IMAGE" "$API_IMAGE"; do
      docker image inspect "$image" >/dev/null || die "$image is missing"
    done
    if ((skip_jobs == 0)); then
      for image in "$WORKER_IMAGE" "$BEAT_IMAGE" "$MIGRATOR_IMAGE"; do
        docker image inspect "$image" >/dev/null || die "$image is missing"
      done
    fi
    [[ -x "$DIFFTEST" ]] || die "$DIFFTEST is missing"
    return
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

  note "flushing Redis DBs $DJANGO_REDIS_DB, $API_REDIS_DB, $JOBS_REDIS_DB"
  for redis_db in "$DJANGO_REDIS_DB" "$API_REDIS_DB" "$JOBS_REDIS_DB"; do
    flush_redis "$redis_db"
  done

  note "creating RabbitMQ vhosts with a 2 s message TTL"
  for vhost in "$DJANGO_VHOST" "$API_VHOST" "$JOBS_VHOST"; do
    create_vhost "$vhost"
  done

  note "writing environment files under dev/run/"
  write_env "$run_dir/bench-django.env" "$DJANGO_DB" "$DJANGO_REDIS_DB" "$DJANGO_VHOST" 8000
  write_env "$run_dir/bench-api.env" "$API_DB" "$API_REDIS_DB" "$API_VHOST" 8000
  write_env "$run_dir/bench-jobs.env" "$JOBS_DB" "$JOBS_REDIS_DB" "$JOBS_VHOST" 8000
}

wait_http() {
  local url="$1" deadline=$((SECONDS + 120))
  until curl -fsS -o /dev/null --max-time 2 "$url"; do
    ((SECONDS < deadline)) || die "timed out waiting for $url"
    sleep 0.5
  done
}

wait_log() {
  local name="$1" pattern="$2" deadline=$((SECONDS + 60))
  until docker logs "$name" >"$out/$name.log" 2>&1 && grep -q "$pattern" "$out/$name.log"; do
    ((SECONDS < deadline)) || die "$name never printed $pattern"
    sleep 0.5
  done
}

start_containers() {
  note "starting $DJANGO_CONTAINER on 127.0.0.1:$DJANGO_PORT and $API_CONTAINER on 127.0.0.1:$API_PORT"
  # Same commands as run-phase3.sh (`apps/api/bin/docker-entrypoint-api.sh`'s
  # gunicorn line, access log to /dev/null) with this run's names and ports.
  docker run -d --name "$DJANGO_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 \
    --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-django.env" \
    -p "127.0.0.1:$DJANGO_PORT:8000" \
    "$DJANGO_IMAGE" \
    /bin/bash -c 'python manage.py collectstatic --noinput >/dev/null 2>&1 && exec gunicorn -w "$GUNICORN_WORKERS" -k uvicorn.workers.UvicornWorker plane.asgi:application --bind 0.0.0.0:"${PORT:-8000}" --max-requests 1200 --max-requests-jitter 1000 --access-logfile /dev/null' \
    >/dev/null

  docker run -d --name "$API_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 \
    --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-api.env" \
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
  (docker logs --follow "$STATS_CONTAINER" 2>/dev/null \
    | "$SYSTEM_PYTHON" "$TOOLS_DIR/stamp.py" "$out/samples-$phase.log" >/dev/null) &
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
  note "verifying scenario responses: Django vs plane-api"
  local scenario verify_file
  for scenario in "${SCENARIOS[@]}"; do
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
      create_vhost "$vhost"
    fi
  done
}

# ---------------------------------------------------------------------------
# Measurement
# ---------------------------------------------------------------------------
duration_for() {
  case "$1" in
    reads-mixed) printf '%s' "$MIXED_LATENCY_DURATION" ;;
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
# plane-worker / plane-beat container idle memory
# ---------------------------------------------------------------------------
migrate_jobs_schema() {
  note "creating the plane_jobs schema in $JOBS_DB with $MIGRATOR_IMAGE"
  docker run --rm --name "$MIGRATOR_CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 --memory 512m --memory-swap 512m \
    --env-file "$run_dir/bench-jobs.env" \
    "$MIGRATOR_IMAGE" 2>&1 | tee "$out/jobs-migrator.log"
}

sample_service() {
  local service="$1" image="$2" container="$3"
  note "starting $container ($image), settling 30 s, then sampling idle for 5 s"
  docker run -d --name "$container" \
    --network "$NETWORK" \
    --cpus 2 --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    --env-file "$run_dir/bench-jobs.env" \
    "$image" >/dev/null
  wait_log "$container" '"starting"' 60
  : >"$out/events-$service.log"
  : >"$out/samples-$service.log"
  start_sampler "$service" "$service:$(container_id "$container")"
  load_note "before $service idle sample"
  sleep 30
  mark "$out/events-$service.log" "$service:start"
  sleep 5
  mark "$out/events-$service.log" "$service:run-end"
  stop_sampler "$service"
  load_note "after $service idle sample"
  "$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" idle \
    --samples "$out/samples-$service.log" \
    --events "$out/events-$service.log" \
    --out "$out/idle-$service.json"
  docker stop -t 10 "$container" >/dev/null
  docker rm "$container" >/dev/null
}

# ---------------------------------------------------------------------------
# Tables
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
  for scenario in "${SCENARIOS[@]}"; do
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
start_containers

{
  printf '# Phase 5 run %s\n' "$tag"
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
  printf 'bench_container_memory_mib: 512\n'
  printf 'latency_duration_seconds: reads-mixed %s, others %s\n' "$MIXED_LATENCY_DURATION" "$LATENCY_DURATION"
  printf 'load: reads-mixed at concurrency %s for %s s\n' "$LOAD_CONCURRENCY" "$LOAD_DURATION"
} >"$out/environment.txt"

: >"$out/events.log"
: >"$out/load.log"
: >"$out/pg-load.log"
: >"$out/samples-api.log"
rm -f "$out/pg-load.stop"
"$PLANE_SERVER_DIR/tools/bench/pg-load.sh" "$out/pg-load.log" "$out/pg-load.stop" 5 >/dev/null 2>&1 &
pg_load_pid=$!
load_note "run start"
uptime >>"$out/load.log"

start_sampler api "django:$(container_id "$DJANGO_CONTAINER"),api:$(container_id "$API_CONTAINER")"
note "settling 30 s before the fresh-container idle sample"
sleep 30
mark "$out/events.log" "django:start"
mark "$out/events.log" "api:start"

verify
if ((verify_only == 1)); then
  stop_sampler api
  touch "$out/pg-load.stop"
  wait "$pg_load_pid" 2>/dev/null || true
  if ((keep == 0)); then cleanup; fi
  note "wrote $out"
  exit 0
fi

for scenario in "${SCENARIOS[@]}"; do
  measure_latency "$scenario" django django "$DJANGO_PORT" "$DJANGO_DB"
  measure_latency "$scenario" api rust "$API_PORT" "$API_DB"
done
measure_load django django "$DJANGO_PORT" "$DJANGO_DB"
measure_load api rust "$API_PORT" "$API_DB"

note "settling 30 s before the final container idle sample"
sleep 30
mark "$out/events.log" "django:run-end"
mark "$out/events.log" "api:run-end"
stop_sampler api
load_note "API matrix end"
uptime >>"$out/load.log"

if ((skip_jobs == 0)); then
  migrate_jobs_schema
  sample_service worker "$WORKER_IMAGE" "$WORKER_CONTAINER"
  sample_service beat "$BEAT_IMAGE" "$BEAT_CONTAINER"
fi

touch "$out/pg-load.stop"
wait "$pg_load_pid" 2>/dev/null || true
"$SYSTEM_PYTHON" "$TOOLS_DIR/phase3.py" idle --samples "$out/samples-api.log" \
  --events "$out/events.log" --out "$out/idle-api.json"
load_note "run end"
uptime >>"$out/load.log"
render_tables

if ((keep == 0)); then
  cleanup
else
  note "--keep: containers and databases left running"
fi
note "wrote $out"
