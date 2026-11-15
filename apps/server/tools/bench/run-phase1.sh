#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run the Phase 1 benchmark matrix from BENCH.md and write one JSON report and
# one Markdown table per (scenario, target configuration) into
# apps/server/target/bench/phase1-<tag>/.
#
#   apps/server/tools/bench/run-phase1.sh
#   apps/server/tools/bench/run-phase1.sh --no-build      # reuse the binaries
#
# One measurement runs at a time on purpose: the machine is shared, and two
# concurrent benchmarks would measure each other. Load averages before and
# after every invocation are recorded in the JSON report.
#
# Matrix: scenarios health and static-responses, targets
#   rust (default tokio threads), rust with --rust-threads 1, django with 1
#   gunicorn worker, each at concurrency 1, 8 and 64, 20 s measured after a 5 s
#   warm-up and a 10 s settle, three repetitions (the report shows medians).
# The client-headroom measurement (the same loop against a trivial local
# server) is done once, for 20 s per concurrency, on the first invocation.

set -euo pipefail

PLANE_TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_TOOLS_DIR/../../dev/lib.sh"

usage() {
  cat <<'EOF'
usage: run-phase1.sh [--no-build] [--tag <name>]

  --no-build   do not rebuild plane-api and difftest (use target/release)
  --tag NAME   directory name under target/bench (default: phase1-<timestamp>)
EOF
}

skip_build=0
tag="phase1-$(date +%Y%m%d-%H%M%S)"
while (($# > 0)); do
  case "$1" in
    --no-build) skip_build=1 ;;
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

out="$PLANE_SERVER_DIR/target/bench/$tag"
mkdir -p "$out"
# The bench CLI resolves its default paths (`scenarios/targets.yaml`) relative to
# apps/server; run from there so the script works from any directory.
cd "$PLANE_SERVER_DIR"

if ((skip_build == 0)); then
  note "building plane-api (release)"
  (cd "$PLANE_SERVER_DIR" && cargo build --release -p plane-api)
  note "building difftest (release)"
  cargo build --release --manifest-path "$PLANE_SERVER_DIR/crates/plane-testkit/Cargo.toml" \
    --bin difftest
fi

difftest="$PLANE_SERVER_DIR/target/release/difftest"
[[ -x "$difftest" ]] || die "difftest not built: $difftest"
[[ -x "$PLANE_SERVER_DIR/target/release/plane-api" ]] ||
  die "plane-api release binary is missing; run without --no-build"

measure() {
  local label="$1" scenario="$2" target="$3"
  shift 3
  note "measuring $label"
  "$difftest" bench \
    --scenario "$PLANE_SERVER_DIR/scenarios/bench/$scenario.yaml" \
    --target "$target" \
    --release \
    --concurrency 1,8,64 \
    --duration 20 \
    --warmup 5 \
    --settle 10 \
    --repetitions 3 \
    --report "$out/$label.json" \
    --markdown "$out/$label.md" \
    "$@"
}

for scenario in health static-responses; do
  # The first invocation also measures the client's own ceiling, for the same
  # 20 s window as the real measurements.
  measure "$scenario-rust-default" "$scenario" rust --headroom-duration 20
  measure "$scenario-rust-1t" "$scenario" rust --rust-threads 1 --headroom-duration 0
  measure "$scenario-django-1w" "$scenario" django --django-workers 1 --headroom-duration 0
done

# shellcheck disable=SC2012,SC2035
cat "$out"/*.md >"$out/summary.md"
note "wrote $out/summary.md"
ls -1 "$out"
