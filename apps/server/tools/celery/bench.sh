#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Publish 1,000 messages and report p50/p99, throughput and RSS.
#
#   apps/server/tools/celery/bench.sh [count]
#
# Builds the release example, creates a disposable vhost, runs the benchmark,
# and deletes the vhost.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../dev" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_DEV_DIR/lib.sh"

REPO_ROOT="$(cd "$PLANE_DEV_DIR/../../.." && pwd)"
SERVER_DIR="$REPO_ROOT/apps/server"
VHOST="t_celery_bench_$$"
COUNT="${1:-1000}"

export CARGO_TARGET_DIR="$SERVER_DIR/target/jobs"

cleanup() {
  status=$?
  "$PLANE_DEV_DIR/mq-vhost.sh" --delete "$VHOST" >/dev/null 2>&1 || true
  exit "$status"
}
trap cleanup EXIT

"$PLANE_DEV_DIR/status.sh" >/dev/null || die "dev stack is not healthy"
"$PLANE_DEV_DIR/mq-vhost.sh" "$VHOST" >/dev/null

note "building the release benchmark"
cd "$SERVER_DIR"
cargo build -q --release -p plane-jobs --example publish_bench

URL="amqp://$PLANE_RABBITMQ_USER:$PLANE_RABBITMQ_PASSWORD@$PLANE_RABBITMQ_HOST:$PLANE_RABBITMQ_PORT/$VHOST"
note "publishing $COUNT messages to $VHOST"
"$CARGO_TARGET_DIR/release/examples/publish_bench" --url "$URL" --count "$COUNT"
