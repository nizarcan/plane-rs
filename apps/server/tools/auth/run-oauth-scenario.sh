#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run a differential scenario with the OAuth fakes up:
#
#   * `tools/auth/fake_provider.py` on 127.0.0.1:18400, for GitLab and Gitea
#     (the fixture points `GITLAB_HOST`/`GITEA_HOST` there);
#   * `tools/auth/oauth_https_proxy.py` on 127.0.0.1:18401, an intercepting
#     HTTPS forward proxy for the hardcoded Google/GitHub endpoints. It
#     generates a CA into `apps/server/target/oauth-mitm/` at startup; the
#     scenario's `target_env` points `REQUESTS_CA_BUNDLE`/`CURL_CA_BUNDLE`/
#     `SSL_CERT_FILE` at it and its proxies at 18401.
#
#   tools/auth/run-oauth-scenario.sh run \
#       --scenario scenarios/write/auth-oauth-google-github.yaml \
#       --targets /tmp/targets-mitm.yaml --a django --b rust \
#       --a-redis-db 9 --b-redis-db 10
#
# Every argument is passed to `difftest` unchanged. If the scenario has a
# sibling `<name>.calls.json` (the expected provider-call inventory), the log
# is verified after the run: a request that bypassed the proxy, went to a
# non-provider host, or a missing/extra provider call fails the wrapper. The
# CA's private keys never leave the gitignored `target/oauth-mitm/` directory.
#
# The ports are fixed because the scenario's SQL fixture, `target_env` and the
# recorded `Location` URLs name them; a busy port fails loudly rather than
# silently testing nothing.

set -euo pipefail

SERVER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROVIDER_PORT="${FAKE_PROVIDER_PORT:-18400}"
MITM_PORT="${OAUTH_MITM_PORT:-18401}"
CA_DIR="${OAUTH_MITM_CA_DIR:-$SERVER_DIR/target/oauth-mitm}"
MITM_LOG="$CA_DIR/requests.jsonl"
# shellcheck source=../upstream/oracle.sh
source "$SERVER_DIR/tools/upstream/oracle.sh"
PYTHON="${PLANE_PYTHON:-$(oracle_python)}"

if [[ ! -x "$PYTHON" ]]; then
  echo "run-oauth-scenario: no interpreter at $PYTHON (set PLANE_PYTHON)" >&2
  exit 1
fi

for port in "$PROVIDER_PORT" "$MITM_PORT"; do
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "run-oauth-scenario: 127.0.0.1:$port is already in use" >&2
    exit 1
  fi
done

# The expected-call inventory lives next to the scenario file.
EXPECTED=""
absolute() {
  (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd)" "$(basename "$1")")
}
previous=""
for argument in "$@"; do
  candidate=""
  if [[ "$previous" == "--scenario" ]]; then
    candidate="${argument%.yaml}.calls.json"
  elif [[ "$argument" == --scenario=* ]]; then
    candidate="${argument#--scenario=}"
    candidate="${candidate%.yaml}.calls.json"
  fi
  if [[ -n "$candidate" && -f "$candidate" ]]; then
    EXPECTED="$(absolute "$candidate")"
  fi
  previous="$argument"
done

wait_for_port() {
  local port="$1"
  for _ in $(seq 1 200); do
    if "$PYTHON" - "$port" <<'PY' >/dev/null 2>&1
import socket
import sys

port = int(sys.argv[1])
with socket.create_connection(("127.0.0.1", port), timeout=0.2):
    pass
PY
    then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

provider_pid=""
proxy_pid=""
cleanup() {
  if [[ -n "$proxy_pid" ]]; then
    kill "$proxy_pid" 2>/dev/null || true
    wait "$proxy_pid" 2>/dev/null || true
  fi
  if [[ -n "$provider_pid" ]]; then
    kill "$provider_pid" 2>/dev/null || true
    wait "$provider_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

mkdir -p "$CA_DIR"
rm -f "$MITM_LOG"

"$PYTHON" "$SERVER_DIR/tools/auth/fake_provider.py" --port "$PROVIDER_PORT" &
provider_pid=$!

"$PYTHON" "$SERVER_DIR/tools/auth/oauth_https_proxy.py" \
  --port "$MITM_PORT" --ca-dir "$CA_DIR" --log "$MITM_LOG" &
proxy_pid=$!

wait_for_port "$PROVIDER_PORT" || {
  echo "run-oauth-scenario: the fake provider did not listen on $PROVIDER_PORT" >&2
  exit 1
}
wait_for_port "$MITM_PORT" || {
  echo "run-oauth-scenario: the OAuth MITM proxy did not listen on $MITM_PORT" >&2
  exit 1
}
for _ in $(seq 1 100); do
  [[ -f "$CA_DIR/ca.pem" ]] && break
  sleep 0.02
done
if [[ ! -f "$CA_DIR/ca.pem" ]]; then
  echo "run-oauth-scenario: the MITM proxy did not write $CA_DIR/ca.pem" >&2
  exit 1
fi

if ! kill -0 "$provider_pid" 2>/dev/null; then
  echo "run-oauth-scenario: the fake provider failed to start" >&2
  exit 1
fi
if ! kill -0 "$proxy_pid" 2>/dev/null; then
  echo "run-oauth-scenario: the OAuth MITM proxy failed to start" >&2
  exit 1
fi

cd "$SERVER_DIR"
status=0
./target/debug/difftest "$@" || status=$?

if [[ -n "$EXPECTED" ]]; then
  verify_status=0
  "$PYTHON" "$SERVER_DIR/tools/auth/verify_mitm_log.py" \
    --log "$MITM_LOG" --expected "$EXPECTED" || verify_status=$?
  if [[ "$verify_status" -ne 0 ]]; then
    exit "$verify_status"
  fi
fi

exit "$status"
