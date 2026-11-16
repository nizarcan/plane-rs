#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Phase 3 benchmark: authenticated read paths, Django vs plane-api, in Linux
# containers on the dev stack's Docker VM. This is the script BENCH.md
# ("Phase 3 — Linux containers") documents; it builds both images, provisions a
# database clone, Redis index and RabbitMQ vhost per target, starts the two
# containers on the dev network, verifies that every scenario's responses are
# identical with `difftest run`, then measures with `difftest bench` in attach
# mode while a cgroup sidecar samples the containers' memory and CPU.
#
#   apps/server/tools/bench/run-phase3.sh              # everything, then clean up
#   apps/server/tools/bench/run-phase3.sh --keep       # leave the containers up
#   apps/server/tools/bench/run-phase3.sh --no-build   # reuse images and difftest
#   apps/server/tools/bench/run-phase3.sh --verify-only
#   apps/server/tools/bench/run-phase3.sh --down       # remove what an earlier run left
#
# Isolation: the containers are named plane-rs-bench-*, use the databases
# bench_p3_django / bench_p3_api (clones of the fixture template), Redis DBs 8
# and 9, and RabbitMQ vhosts bench_p3_django / bench_p3_api. Nothing else on the
# VM is touched. The containers publish 127.0.0.1:18810 (Django) and
# 127.0.0.1:18860 (plane-api).
#
# The secrets this run needs (SECRET_KEY) are read through dev/django-env.sh and
# written to apps/server/dev/run/, which is gitignored; they are never printed.

set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

DJANGO_IMAGE="plane-rs-bench-django:latest"
API_IMAGE="plane-rs-bench-api:latest"
DJANGO_CONTAINER="plane-rs-bench-django"
API_CONTAINER="plane-rs-bench-api"
STATS_CONTAINER="plane-rs-bench-stats"
DJANGO_DB="bench_p3_django"
API_DB="bench_p3_api"
DJANGO_REDIS_DB=8
API_REDIS_DB=9
DJANGO_VHOST="bench_p3_django"
API_VHOST="bench_p3_api"
DJANGO_PORT=18810
API_PORT=18860
NETWORK="plane-rs-dev_default"
MGMT="http://127.0.0.1:$PLANE_RABBITMQ_MGMT_PORT"
SCENARIOS=(reads-users-me reads-project reads-issues reads-cycles reads-mixed)
DIFFTEST="$PLANE_SERVER_DIR/target/release/difftest"
SYSTEM_PYTHON="$(command -v python3)"

usage() {
  cat <<'EOF'
usage: run-phase3.sh [--keep] [--no-build] [--verify-only] [--down] [--tag NAME]

  --keep         leave the bench containers and databases running at the end
  --no-build     reuse the two images and target/release/difftest
  --verify-only  build/start, run the differential verification, stop
  --down         remove the bench containers, databases, vhosts and Redis DBs
  --scenarios LIST  comma-separated subset of the five reads-* scenarios
                    (default: all of them)
  --tag NAME        report directory under target/bench (default phase3-<timestamp>)
EOF
}

keep=0
skip_build=0
verify_only=0
down_only=0
scenarios_csv=""
tag="phase3-$(date +%Y%m%d-%H%M%S)"
while (($# > 0)); do
  case "$1" in
    --keep) keep=1 ;;
    --no-build) skip_build=1 ;;
    --verify-only) verify_only=1 ;;
    --down) down_only=1 ;;
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
  local database="$1"
  compose exec -T valkey valkey-cli -n "$database" flushdb >/dev/null
}

drop_database() {
  local database="$1"
  # FORCE (PostgreSQL 13+) closes a connection a stopped container has not
  # released yet, so a rerun never fails on its own leftovers.
  psql_dev plane -c "DROP DATABASE IF EXISTS \"$database\" WITH (FORCE)" >/dev/null
}

create_database() {
  local database="$1"
  psql_dev plane -c "CREATE DATABASE \"$database\" TEMPLATE plane_fix_tpl" >/dev/null
}

create_vhost() {
  local vhost="$1"
  rabbit_api -X PUT "$MGMT/api/vhosts/$vhost" >/dev/null
  rabbit_api -X PUT -H 'content-type: application/json' \
    -d '{"configure":".*","write":".*","read":".*"}' \
    "$MGMT/api/permissions/$vhost/$PLANE_RABBITMQ_USER" >/dev/null
  # Every measured read of a project, cycle, module or issue publishes
  # recent_visited_task. With no worker in this stack the `celery` queue would
  # grow by tens of thousands of messages per run and eventually take the
  # broker past its 320 MB container limit, which would perturb the numbers and
  # possibly the whole VM. A 2 s message TTL keeps the queue (nearly) empty and
  # is scoped to this vhost.
  rabbit_api -X PUT -H 'content-type: application/json' \
    -d '{"pattern":".*","definition":{"message-ttl":2000},"priority":0,"apply-to":"queues"}' \
    "$MGMT/api/policies/$vhost/bench-ttl" >/dev/null
}

delete_vhost() {
  local vhost="$1"
  rabbit_api -X DELETE "$MGMT/api/vhosts/$vhost" >/dev/null 2>&1 || true
}

cleanup() {
  note "stopping bench containers"
  remove_container "$STATS_CONTAINER"
  remove_container "$DJANGO_CONTAINER"
  remove_container "$API_CONTAINER"
  note "dropping bench databases and broker vhosts"
  drop_database "$DJANGO_DB" || true
  drop_database "$API_DB" || true
  delete_vhost "$DJANGO_VHOST"
  delete_vhost "$API_VHOST"
  flush_redis "$DJANGO_REDIS_DB" || true
  flush_redis "$API_REDIS_DB" || true
  rm -f "$run_dir/bench-django.env" "$run_dir/bench-api.env"
}

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

preflight() {
  [[ -n "$(docker network ls --filter "name=^${NETWORK}$" --format '{{.Name}}')" ]] ||
    die "docker network $NETWORK is missing; start the dev stack with apps/server/dev/up.sh"

  # The VM is shared with the owner's containers, and a Rust release build with
  # fat LTO needs more memory than it usually has free. Refuse to start a build
  # that is likely to be OOM-killed (and to fill the host's disk while doing it);
  # `--no-build` skips this check, PLANE_BENCH_FORCE=1 overrides it.
  local host_free_mib vm_available_mib min_free_mib
  min_free_mib="${PLANE_BENCH_MIN_FREE_MIB:-20480}"
  host_free_mib="$(df -k "$PLANE_REPO_ROOT" | awk 'NR==2 {print int($4 / 1024)}')"
  vm_available_mib="$(docker run --rm alpine:3.20 sh -c \
    "awk '/MemAvailable/{print int(\$2 / 1024)}' /proc/meminfo" 2>/dev/null || echo 0)"
  note "free memory: host ${host_free_mib} MiB, Docker VM ${vm_available_mib} MiB"
  if [[ "${PLANE_BENCH_FORCE:-0}" == 1 ]]; then
    return
  fi
  ((host_free_mib >= min_free_mib)) ||
    die "the host filesystem holding the repo has ${host_free_mib} MiB free; the VM's disk grows while images build. Keep at least ${min_free_mib} MiB (PLANE_BENCH_MIN_FREE_MIB) or set PLANE_BENCH_FORCE=1"
  if ((skip_build == 0)) && ((vm_available_mib < 700)); then
    die "the Docker VM has ${vm_available_mib} MiB available and the fat-LTO release build needs ~700+ MiB (it needed a 4 GiB swap file once). Keep the dev stack up, stop other work, or set PLANE_BENCH_FORCE=1"
  fi
}

build_images() {
  if ((skip_build == 1)); then
    docker image inspect "$DJANGO_IMAGE" >/dev/null || die "$DJANGO_IMAGE is missing"
    docker image inspect "$API_IMAGE" >/dev/null || die "$API_IMAGE is missing"
    [[ -x "$DIFFTEST" ]] || die "$DIFFTEST is missing"
    return
  fi
  note "building $API_IMAGE (multi-stage, release profile)"
  docker build --progress=plain \
    -f "$PLANE_SERVER_DIR/deploy/docker/Dockerfile.plane-api" \
    -t "$API_IMAGE" "$PLANE_REPO_ROOT" 2>&1 | tee "$out/build-api-image.log"
  note "building $DJANGO_IMAGE (apps/api/Dockerfile.api, unchanged)"
  docker build --progress=plain \
    -f "$PLANE_API_DIR/Dockerfile.api" \
    -t "$DJANGO_IMAGE" "$PLANE_API_DIR" 2>&1 | tee "$out/build-django-image.log"
  note "building difftest (release)"
  cargo build --release --manifest-path "$PLANE_SERVER_DIR/crates/plane-testkit/Cargo.toml" \
    --bin difftest
  # The images are built; the build cache is disposable and on this host it is
  # multiple gigabytes. Keep the disk headroom for the measurement run.
  note "pruning the Docker build cache"
  docker builder prune -f >/dev/null
}

# ---------------------------------------------------------------------------
# Environment: dev/django-env.sh with the container hostnames substituted.
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
    # Same names as `dev/django-env.sh`, pointed at the compose service names
    # on plane-rs-dev_default instead of the host's published ports.
    export DATABASE_URL="postgresql://$PLANE_PG_USER:$PLANE_PG_PASSWORD@postgres:5432/$database"
    export REDIS_URL="redis://valkey:6379/$redis_db"
    export RABBITMQ_HOST="rabbitmq"
    export RABBITMQ_PORT="5672"
    export AWS_S3_ENDPOINT_URL="http://minio:9000"
    # The production contract shared with the Django entrypoint.
    export PORT="$port"
    # Deterministic JSON key order for the Python responses assembled from a
    # `set` (scenarios/read/README.md); plane-api ignores it.
    export PYTHONHASHSEED="0"
    # One tokio worker thread per CPU the container is allowed to use (the
    # `--cpus 2` quota below). Without the pin the server would size its pool
    # from the VM's CPU count, so the comparison would change when the VM is
    # resized; on the original 2-vCPU VM tokio's default was exactly two.
    # gunicorn stays at its production default of one worker.
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
  note "recreating $DJANGO_DB and $API_DB from plane_fix_tpl"
  drop_database "$DJANGO_DB"
  drop_database "$API_DB"
  create_database "$DJANGO_DB"
  create_database "$API_DB"

  note "flushing Redis DBs $DJANGO_REDIS_DB and $API_REDIS_DB"
  flush_redis "$DJANGO_REDIS_DB"
  flush_redis "$API_REDIS_DB"

  note "creating RabbitMQ vhosts with a 2 s message TTL"
  create_vhost "$DJANGO_VHOST"
  create_vhost "$API_VHOST"

  note "writing environment files under dev/run/"
  write_env "$run_dir/bench-django.env" "$DJANGO_DB" "$DJANGO_REDIS_DB" "$DJANGO_VHOST" 8000
  write_env "$run_dir/bench-api.env" "$API_DB" "$API_REDIS_DB" "$API_VHOST" 8000
}

wait_http() {
  local url="$1" deadline=$((SECONDS + 120))
  until curl -fsS -o /dev/null --max-time 2 "$url"; do
    ((SECONDS < deadline)) || die "timed out waiting for $url"
    sleep 0.5
  done
}

start_containers() {
  note "starting $DJANGO_CONTAINER on 127.0.0.1:$DJANGO_PORT and $API_CONTAINER on 127.0.0.1:$API_PORT"
  # `apps/api/bin/docker-entrypoint-api.sh` runs wait_for_db, register_instance,
  # configure_instance, create_bucket, clear_cache and collectstatic before the
  # final exec. Registration and bucket creation talk to the network and have
  # no effect on the read paths being measured, so they are skipped; the
  # database, Redis and bucket already exist. `collectstatic` is kept because
  # WhiteNoise's `add_files(STATIC_ROOT)` raises at startup when the directory
  # is missing, and the image does not run it at build time. The final command is
  # the entrypoint's gunicorn line, byte for byte, except that the access log
  # goes to /dev/null: the formatting and write are still paid, but the Docker
  # log driver does not become the bottleneck at thousands of requests per
  # second.
  #
  # `--cpus 2` and `--memory 512m` are explicit on both containers so the
  # comparison does not depend on how many CPUs the VM is given (the benchmark
  # VM is resized between runs) — two cores of quota each, one gunicorn worker
  # vs two tokio threads, 512 MB of hard memory each.
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

start_sampler() {
  if docker container inspect "$STATS_CONTAINER" >/dev/null 2>&1; then
    docker rm -f "$STATS_CONTAINER" >/dev/null
  fi
  local django_id api_id
  django_id="$(docker inspect --format '{{.Id}}' "$DJANGO_CONTAINER")"
  api_id="$(docker inspect --format '{{.Id}}' "$API_CONTAINER")"
  note "starting the cgroup sampler sidecar"
  docker run -d --name "$STATS_CONTAINER" \
    --network none \
    --memory 64m \
    --log-driver local --log-opt max-size=50m \
    -v /sys/fs/cgroup:/host/cgroup:ro \
    -v "$TOOLS_DIR/cgroup-sampler.py:/sampler.py:ro" \
    -e "TARGETS=django:$django_id,api:$api_id" \
    -e "INTERVAL=0.25" \
    "$DJANGO_BASE_PYTHON" python3 /sampler.py >/dev/null
  # The lines are stamped on the *host* as they arrive. Docker's own
  # `--timestamps` comes from the VM's clock, which drifts after a VM restart
  # (it was ~14 minutes ahead of the host on 2026-11-16); the bench's `conc`
  # lines are stamped with the host clock, and the two streams must share one.
  # `stamp.py` writes `<host epoch> <line>` to samples.log and echoes the line,
  # which is discarded here.
  (docker logs --follow "$STATS_CONTAINER" 2>/dev/null \
    | "$SYSTEM_PYTHON" "$TOOLS_DIR/stamp.py" "$out/samples.log" >/dev/null) &
  echo $! >"$out/samples.pid"
  sleep 1
}

stop_sampler() {
  if [[ -f "$out/samples.pid" ]]; then
    kill "$(cat "$out/samples.pid")" 2>/dev/null || true
    rm -f "$out/samples.pid"
  fi
  remove_container "$STATS_CONTAINER"
}

mark() {
  printf '%s EVENT %s\n' "$("$SYSTEM_PYTHON" -c 'import time; print(f"{time.time():.6f}")')" "$1" \
    >>"$out/events.log"
}

# ---------------------------------------------------------------------------
# Verification and measurement
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

# The broker is shared with other agents and has already been recreated out
# from under a run once (2026-11-16 09:26 UTC, which invalidated two
# scenarios). Check that the bench containers and both vhosts are still there
# before each measurement; recreate a missing vhost with the same 2 s TTL
# policy rather than losing the rest of the run.
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

measure() {
  # `target` is the label used in the report and the sampler (django/api);
  # `spec_target` is the entry in scenarios/targets.yaml, which `difftest bench`
  # insists on even in attach mode (`django` for both kinds of Django target,
  # `rust` for plane-api). Nothing from that file is started here.
  local scenario="$1" target="$2" spec_target="$3" port="$4" database="$5"
  note "measuring $scenario on $target"
  check_targets
  # stamp.py appends; a rerun of the same tag must not pair new measurements
  # with the previous run's window lines.
  : >"$out/events-$scenario-$target.log"
  "$DIFFTEST" bench \
    --scenario "scenarios/bench/$scenario.yaml" \
    --target "$spec_target" \
    --url "http://127.0.0.1:$port" --db "$database" \
    --concurrency 1,8,32 --duration 20 --warmup 5 --settle 10 --repetitions 3 \
    --report "$out/report-$scenario-$target.json" \
    2>&1 | "$SYSTEM_PYTHON" "$TOOLS_DIR/stamp.py" "$out/events-$scenario-$target.log"
}

summarize() {
  $SYSTEM_PYTHON "$TOOLS_DIR/phase3.py" summarize \
    --samples "$out/samples.log" \
    --events "$out/events-$1-$2.log" \
    --report "$out/report-$1-$2.json" \
    --target "$2" \
    --duration 20 --warmup 5 \
    --out "$out/row-$1-$2.json"
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
  printf '# Phase 3 run %s\n' "$tag"
  printf 'date: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'kernel: %s\n' "$(docker run --rm alpine:3.20 uname -r 2>/dev/null || echo unknown)"
  printf 'django_image_bytes: %s\n' "$(image_size "$DJANGO_IMAGE")"
  printf 'api_image_bytes: %s\n' "$(image_size "$API_IMAGE")"
  printf 'rustc: %s\n' "$(docker run --rm rust:1.99.1-slim-bookworm rustc --version 2>/dev/null || echo unknown)"
  printf 'python_stack: %s\n' "$(docker run --rm --entrypoint python "$DJANGO_IMAGE" -c 'import django, rest_framework, gunicorn, uvicorn; print(f"Django {django.get_version()}, DRF {rest_framework.VERSION}, gunicorn {gunicorn.__version__}, uvicorn {uvicorn.__version__}")' 2>/dev/null || echo unknown)"
  printf 'vm_cpus: %s\n' "$(docker run --rm alpine:3.20 nproc 2>/dev/null || echo unknown)"
  printf 'vm_memory: %s\n' "$(docker run --rm alpine:3.20 sh -c 'grep MemTotal /proc/meminfo' 2>/dev/null || echo unknown)"
  printf 'bench_container_cpus: 2\n'
  printf 'bench_container_memory_mib: 512\n'
} >"$out/environment.txt"

: >"$out/events.log"
: >"$out/samples.log"
: >"$out/pg-load.log"
rm -f "$out/pg-load.stop"
"$PLANE_SERVER_DIR/tools/bench/pg-load.sh" "$out/pg-load.log" "$out/pg-load.stop" 5   >/dev/null 2>&1 &
pg_load_pid=$!
start_sampler

note "settling 30 s before the container idle sample"
sleep 30
mark "django:start"
mark "api:start"

verify
if ((verify_only == 1)); then
  stop_sampler
  if ((keep == 0)); then cleanup; fi
  note "wrote $out"
  exit 0
fi

for scenario in "${SCENARIOS[@]}"; do
  measure "$scenario" django django "$DJANGO_PORT" "$DJANGO_DB"
  summarize "$scenario" django
  measure "$scenario" api rust "$API_PORT" "$API_DB"
  summarize "$scenario" api
done

note "settling 30 s before the final container idle sample"
sleep 30
mark "django:run-end"
mark "api:run-end"

stop_sampler
touch "$out/pg-load.stop"
wait "$pg_load_pid" 2>/dev/null || true
$SYSTEM_PYTHON "$TOOLS_DIR/phase3.py" idle --samples "$out/samples.log" \
  --events "$out/events.log" --out "$out/idle.json"
$SYSTEM_PYTHON "$TOOLS_DIR/phase3.py" table --rows "$out" \
  --out "$out/table.md" --ratios "$out/ratios.md"

if ((keep == 0)); then
  cleanup
else
  note "--keep: containers and databases left running"
fi
note "wrote $out"
