#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run a differential scenario with `tools/auth/fake_provider.py` listening on
# 127.0.0.1:18400. The OAuth fixture points GITLAB_HOST/GITEA_HOST there, so the
# server must be up before `difftest` starts the targets and stay up for the
# whole run.
#
#   tools/auth/run-oauth-scenario.sh run --scenario scenarios/write/auth-oauth.yaml \
#       --targets /tmp/targets-authc.yaml --a django --b rust \
#       --a-redis-db 7 --b-redis-db 8
#
# Every argument is passed to `difftest` unchanged. The port is fixed because
# the scenario's SQL fixture and the recorded `Location` URLs name it; a busy
# port fails loudly rather than silently testing nothing.

set -euo pipefail

SERVER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PORT="${FAKE_PROVIDER_PORT:-18400}"
PYTHON="${PLANE_PYTHON:-$SERVER_DIR/../../apps/api/.venv/bin/python}"

if [[ ! -x "$PYTHON" ]]; then
  echo "run-oauth-scenario: no interpreter at $PYTHON (set PLANE_PYTHON)" >&2
  exit 1
fi

if lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "run-oauth-scenario: 127.0.0.1:$PORT is already in use" >&2
  exit 1
fi

"$PYTHON" "$SERVER_DIR/tools/auth/fake_provider.py" --port "$PORT" &
provider_pid=$!
trap 'kill "$provider_pid" 2>/dev/null || true; wait "$provider_pid" 2>/dev/null || true' EXIT

# Wait for the listener (a bind failure would otherwise surface as two 500s).
for _ in $(seq 1 100); do
  if "$PYTHON" - "$PORT" <<'PY' >/dev/null 2>&1
import socket
import sys

port = int(sys.argv[1])
with socket.create_connection(("127.0.0.1", port), timeout=0.2):
    pass
PY
  then
    break
  fi
  sleep 0.05
done

if ! kill -0 "$provider_pid" 2>/dev/null; then
  echo "run-oauth-scenario: the fake provider failed to start" >&2
  exit 1
fi

cd "$SERVER_DIR"
./target/debug/difftest "$@"
