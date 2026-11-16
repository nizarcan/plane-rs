#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Start and stop the single `plane-api` container the memory study drives, and
# its cgroup sampler. See BENCH.md ("Phase 3: memory after sustained load").
#
#   tools/bench/memstudy.sh up [--variant mimalloc] [--pool N]
#                              [--threads N] [--env KEY=VALUE]...
#   tools/bench/memstudy.sh down
#   tools/bench/memstudy.sh stats <outdir>      # start streaming samples.log
#   tools/bench/memstudy.sh stats-stop
#
# `--variant mimalloc` (the default) runs the production image's binary; pass
# `--env MIMALLOC_PURGE_DELAY=0` for the lower plateau. The `system` and
# `jemalloc` variants need the temporary study patch (a cargo feature per
# allocator plus the `runtime-all` Docker target), which is not merged; see
# BENCH.md, "Phase 3: memory after sustained load". This script only ever
# touches containers named plane-rs-mem-*.
#
# Ports 18300-18399, Redis DB 4, database t_rsmem_api, vhost rsmem_api, and the
# container names plane-rs-mem-* are this study's; nothing else is touched.
set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

IMAGE="plane-rs-mem-api:latest"
NETWORK="plane-rs-dev_default"
CONTAINER="plane-rs-mem-api"
STATS="plane-rs-mem-stats"
PORT=18360
ENV_FILE="$PLANE_DEV_DIR/run/memstudy-api.env"
BASE_PYTHON="python:3.12.12-alpine"

usage() {
  sed -n '6,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

remove() {
  local name="$1"
  [[ "$name" == plane-rs-mem-* ]] || die "refusing to remove '$name'"
  if docker container inspect "$name" >/dev/null 2>&1; then
    docker rm -f "$name" >/dev/null
  fi
}

down() {
  remove "$STATS"
  remove "$CONTAINER"
  rm -f "$PLANE_DEV_DIR/run/memstudy-samples.pid"
}

up() {
  local variant="mimalloc" pool="" threads="2"
  local -a overrides=()
  while (($# > 0)); do
    case "$1" in
      --variant) shift; variant="${1:?--variant needs a value}" ;;
      --pool) shift; pool="${1:?--pool needs a value}" ;;
      --threads) shift; threads="${1:?--threads needs a value}" ;;
      --env) shift; overrides+=("${1:?--env needs KEY=VALUE}") ;;
      *) die "unknown argument '$1'" ;;
    esac
    shift
  done
  [[ -f "$ENV_FILE" ]] || die "$ENV_FILE is missing"
  local command=()
  case "$variant" in
    mimalloc) ;;
    system | jemalloc)
      die "--variant $variant needs the temporary study patch (cargo allocator features + the runtime-all image target, see BENCH.md); the production image ships the mimalloc binary only"
      ;;
    *) die "unknown variant '$variant'" ;;
  esac

  remove "$STATS"
  remove "$CONTAINER"

  local -a env_args=(--env-file "$ENV_FILE")
  env_args+=(-e "PLANE_RS_WORKER_THREADS=$threads")
  [[ -n "$pool" ]] && env_args+=(-e "PLANE_RS_DB_POOL_SIZE=$pool")
  local pair
  for pair in "${overrides[@]}"; do
    env_args+=(-e "$pair")
  done

  note "starting $CONTAINER ($variant, pool=${pool:-default}, threads=$threads)"
  docker run -d --name "$CONTAINER" \
    --network "$NETWORK" \
    --cpus 2 --memory 512m --memory-swap 512m \
    --log-driver local --log-opt max-size=20m \
    "${env_args[@]}" \
    -p "127.0.0.1:$PORT:8000" \
    "$IMAGE" "${command[@]}" >/dev/null

  local deadline=$((SECONDS + 120))
  until curl -fsS -o /dev/null --max-time 2 "http://127.0.0.1:$PORT/"; do
    ((SECONDS < deadline)) || die "timed out waiting for $CONTAINER"
    sleep 0.5
  done
  note "healthy; cgroup: $(docker inspect --format '{{.Id}}' "$CONTAINER")"
}

stats() {
  local out="${1:?usage: memstudy.sh stats <outdir>}"
  mkdir -p "$out"
  : >"$out/samples.log"
  local id postgres_id
  id="$(docker inspect --format '{{.Id}}' "$CONTAINER")"
  # The shared dev Postgres is sampled too: if the API's growth is server-side
  # prepared statements, its container grows with it.
  postgres_id="$(docker inspect --format '{{.Id}}' plane-rs-dev-postgres-1)"
  docker run -d --name "$STATS" \
    --network none --memory 64m \
    --log-driver local --log-opt max-size=50m \
    -v /sys/fs/cgroup:/host/cgroup:ro \
    -v "$TOOLS_DIR/cgroup-sampler.py:/sampler.py:ro" \
    -e "TARGETS=api:$id,db:$postgres_id" -e INTERVAL=1 \
    "$BASE_PYTHON" python3 /sampler.py >/dev/null
  # Host-stamped: the VM's clock drifts after a restart (see BENCH.md).
  (docker logs --follow "$STATS" 2>/dev/null \
    | "$PLANE_DJANGO_PYTHON" "$TOOLS_DIR/stamp.py" "$out/samples.log" >/dev/null) &
  echo $! >"$PLANE_DEV_DIR/run/memstudy-samples.pid"
  note "sampling into $out/samples.log"
}

stats_stop() {
  if [[ -f "$PLANE_DEV_DIR/run/memstudy-samples.pid" ]]; then
    kill "$(cat "$PLANE_DEV_DIR/run/memstudy-samples.pid")" 2>/dev/null || true
    rm -f "$PLANE_DEV_DIR/run/memstudy-samples.pid"
  fi
  remove "$STATS"
  note "sampler stopped"
}

case "${1:-}" in
  up) shift; up "$@" ;;
  down) shift; down ;;
  stats) shift; stats "$@" ;;
  stats-stop) shift; stats_stop ;;
  -h | --help | "") usage ;;
  *) die "unknown command '$1'" ;;
esac
