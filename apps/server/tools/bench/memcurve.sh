#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Drive a long `reads-mixed` load at one concurrency level and record where the
# target's memory is at cumulative request checkpoints. Used by BENCH.md
# ("Phase 3: memory after sustained load").
#
#   tools/bench/memcurve.sh <outdir> <port> <database> [target-requests...]
#
# Default checkpoints: 50000 200000 500000 1000000. The chunk length for each
# checkpoint is computed from the previous chunk's measured rps, so the
# checkpoints are reached regardless of how fast the target is.
#
# The target must already be running (attach mode: the process is not restarted,
# so the curve is one process's). The script expects a cgroup sampler sidecar
# whose stdout is streamed into `<outdir>/samples.log` by the caller; it writes
# `<outdir>/events.log` (host-stamped bench output) and `<outdir>/markers.log`
# (`<epoch> <label> <cumulative-requests>`).
set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

out="${1:-}"
port="${2:-}"
database="${3:-}"
shift 3 || true
if [[ -z "$out" || -z "$port" || -z "$database" ]]; then
  die "usage: memcurve.sh <outdir> <port> <database> [target-requests...]"
fi
targets=("$@")
if ((${#targets[@]} == 0)); then
  targets=(50000 200000 500000 1000000)
fi

difftest="$PLANE_SERVER_DIR/target/release/difftest"
[[ -x "$difftest" ]] || die "difftest is missing: $difftest"
mkdir -p "$out"
: >"$out/events.log"
: >"$out/markers.log"

mark() {
  "$PLANE_DJANGO_PYTHON" - "$out/markers.log" "$1" "$2" <<'PY'
import sys, time
path, label, requests = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, "a", encoding="utf-8") as handle:
    handle.write(f"{time.time():.6f} {label} {requests}\n")
PY
}

mark start 0
cumulative=0
rps=1000
previous=0
for target in "${targets[@]}"; do
  remaining=$((target - cumulative))
  if ((remaining <= 0)); then
    continue
  fi
  duration=$("$PLANE_DJANGO_PYTHON" -c "print(max(10, min(900, int($remaining / $rps))))")
  note "chunk to ${target} requests: ${remaining} more at ~${rps} rps -> ${duration}s"
  : >"$out/chunk-$target.events"
  "$difftest" bench \
    --scenario "$PLANE_SERVER_DIR/scenarios/bench/reads-mixed.yaml" \
    --target rust \
    --url "http://127.0.0.1:$port" --db "$database" \
    --concurrency 32 --duration "$duration" --warmup 0 --settle 0 --repetitions 1 \
    --report "$out/chunk-$target.json" \
    2>&1 | "$PLANE_DJANGO_PYTHON" "$TOOLS_DIR/stamp.py" "$out/chunk-$target.events" |
    grep -E '^  conc|^repetition' || true
  read -r requests rps_value < <("$PLANE_DJANGO_PYTHON" - "$out/chunk-$target.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
measurement = report["measurements"][-1]
print(measurement["requests"], f"{measurement['rps']:.1f}")
PY
)
  cumulative=$((cumulative + requests))
  rps="$rps_value"
  note "  reached ${cumulative} requests (${rps} rps)"
  mark checkpoint "$cumulative"
done

idle_seconds="${MEMCURVE_IDLE_SECONDS:-300}"
note "${idle_seconds}s idle"
sleep "$idle_seconds"
mark idle-after-load "$cumulative"
note "done: ${cumulative} requests, markers in $out/markers.log"
