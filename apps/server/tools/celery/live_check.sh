#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Live proof of the Celery publisher:
#
#   * publishes one case from Python and one from Rust to a fresh RabbitMQ
#     vhost, with the identity pinned identically;
#   * drains each message twice — with Python kombu and with the differential
#     harness's own decoder (plane-testkit) — and checks all four decodings
#     agree and that the Python/Rust body bytes are identical;
#   * verifies the broker topology the first publish created (a durable
#     `celery` direct exchange, a durable `celery` queue with no arguments, and
#     the `celery -> celery` binding) on a vhost that started empty;
#   * runs the crate's live tests against the same vhost.
#
# The vhost is deleted on exit. Nothing else on the dev stack is touched.
#
#   apps/server/tools/celery/live_check.sh

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../dev" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_DEV_DIR/lib.sh"

REPO_ROOT="$(cd "$PLANE_DEV_DIR/../../.." && pwd)"
SERVER_DIR="$REPO_ROOT/apps/server"
TOOLS_DIR="$SERVER_DIR/tools/celery"
PYTHON="$PLANE_DJANGO_PYTHON"
VHOST="t_celery_live_$$"
URL="amqp://$PLANE_RABBITMQ_USER:$PLANE_RABBITMQ_PASSWORD@$PLANE_RABBITMQ_HOST:$PLANE_RABBITMQ_PORT/$VHOST"
MGMT="http://$PLANE_RABBITMQ_HOST:$PLANE_RABBITMQ_MGMT_PORT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/celery-live.XXXXXX")"

export CARGO_TARGET_DIR="$SERVER_DIR/target/jobs"
export PLANE_RABBITMQ_USER PLANE_RABBITMQ_PASSWORD

cleanup() {
  status=$?
  "$PLANE_DEV_DIR/mq-vhost.sh" --delete "$VHOST" >/dev/null 2>&1 || true
  rm -rf "$WORK"
  exit "$status"
}
trap cleanup EXIT

[[ -x "$PYTHON" ]] || die "missing $PYTHON; the Django venv is required"

"$PLANE_DEV_DIR/status.sh" >/dev/null || die "dev stack is not healthy"
"$PLANE_DEV_DIR/mq-vhost.sh" "$VHOST" >/dev/null
note "vhost $VHOST ready"

cd "$SERVER_DIR"

note "building the Rust publisher cases and the harness drain tool"
cargo build -q -p plane-jobs --features test-cases --example publish_case
cargo build -q --manifest-path "$TOOLS_DIR/drain/Cargo.toml"
PUBLISH="$CARGO_TARGET_DIR/debug/examples/publish_case"
DRAIN="$CARGO_TARGET_DIR/debug/celery-drain"

# ---------------------------------------------------------------------------
# A fresh vhost has no celery entities.
# ---------------------------------------------------------------------------

python3 - "$MGMT" "$VHOST" <<'PY'
import json, os, sys, urllib.request
base, vhost = sys.argv[1], sys.argv[2]
import base64
credentials = f"{os.environ['PLANE_RABBITMQ_USER']}:{os.environ['PLANE_RABBITMQ_PASSWORD']}"
auth = "Basic " + base64.b64encode(credentials.encode()).decode()
def get(path):
    request = urllib.request.Request(base + path)
    request.add_header("Authorization", auth)
    with urllib.request.urlopen(request) as response:
        return json.load(response)
exchanges = {item["name"] for item in get(f"/api/exchanges/{vhost}")}
queues = {item["name"] for item in get(f"/api/queues/{vhost}")}
assert "celery" not in exchanges, exchanges
assert "celery" not in queues, queues
print("fresh vhost: no celery exchange or queue")
PY

# ---------------------------------------------------------------------------
# Automated live tests first; they drain what they publish.
# ---------------------------------------------------------------------------

note "running the crate's live tests (PLANE_RS_TEST_AMQP_URL=$URL)"
PLANE_RS_TEST_AMQP_URL="$URL" cargo test -q -p plane-jobs --all-features \
  celery::tests::live -- --test-threads=1

# ---------------------------------------------------------------------------
# Per-case cross-decoding.
# ---------------------------------------------------------------------------

CASES=(
  recent_visited_project
  process_logs
  issue_activity_view
  sync_issue_version_continue
  webhook_send_task
)

for case in "${CASES[@]}"; do
  case_dir="$WORK/$case"
  mkdir -p "$case_dir"

  # A drain consumes the message, so publish the same pinned message once per
  # decoder. Both copies are byte-identical; the comparison checks that.
  "$PYTHON" "$TOOLS_DIR/live_publish.py" --url "$URL" --case "$case" >/dev/null
  "$PYTHON" "$TOOLS_DIR/drain.py" --url "$URL" >"$case_dir/py_kombu.json"
  "$PYTHON" "$TOOLS_DIR/live_publish.py" --url "$URL" --case "$case" >/dev/null
  "$DRAIN" "$URL" >"$case_dir/py_testkit.json"

  "$PUBLISH" --url "$URL" --case "$case" >/dev/null
  "$PYTHON" "$TOOLS_DIR/drain.py" --url "$URL" >"$case_dir/rs_kombu.json"
  "$PUBLISH" --url "$URL" --case "$case" >/dev/null
  "$DRAIN" "$URL" >"$case_dir/rs_testkit.json"

  "$PYTHON" "$TOOLS_DIR/compare_live.py" --case "$case" --dir "$case_dir"
done

# ---------------------------------------------------------------------------
# Topology created by the first publish (same as kombu's declare=[queue]).
# ---------------------------------------------------------------------------

python3 - "$MGMT" "$VHOST" <<'PY'
import base64, json, os, sys, urllib.request
base, vhost = sys.argv[1], sys.argv[2]
credentials = f"{os.environ['PLANE_RABBITMQ_USER']}:{os.environ['PLANE_RABBITMQ_PASSWORD']}"
auth = "Basic " + base64.b64encode(credentials.encode()).decode()
def get(path):
    request = urllib.request.Request(base + path)
    request.add_header("Authorization", auth)
    with urllib.request.urlopen(request) as response:
        return json.load(response)
exchanges = {item["name"]: item for item in get(f"/api/exchanges/{vhost}")}
queues = {item["name"]: item for item in get(f"/api/queues/{vhost}")}
bindings = get(f"/api/bindings/{vhost}")
assert exchanges["celery"]["type"] == "direct", exchanges["celery"]
assert exchanges["celery"]["durable"] is True
assert exchanges["celery"]["auto_delete"] is False
assert exchanges["celery"]["arguments"] == {}
queue = queues["celery"]
assert queue["durable"] is True
assert queue["auto_delete"] is False
assert queue["exclusive"] is False
assert queue["arguments"] == {}, queue["arguments"]
assert any(
    binding["source"] == "celery"
    and binding["destination"] == "celery"
    and binding["destination_type"] == "queue"
    and binding["routing_key"] == "celery"
    for binding in bindings
), bindings
print("topology: exchange celery(direct,durable) -> queue celery(durable,no args) rk=celery")
PY

note "live check passed"
