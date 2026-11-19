#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run the SMTP credential-check scenario with one fake SMTP listener per target
# and compare what each backend put on the wire.
#
#   tools/mail/run-smtp-scenario.sh success \
#       --scenario scenarios/write/instances-email.yaml \
#       --targets /tmp/targets-instc.yaml --a django --b rust \
#       --a-redis-db 7 --b-redis-db 8
#
# Modes (one listener pair each, ports 18410/18411; 18412-18419 are free for a
# second concurrent agent):
#
#   success       EHLO, AUTH PLAIN with plane/plane, MAIL, RCPT, DATA
#   auth          AUTH PLAIN then AUTH LOGIN with a wrong password -> retried,
#                 then SMTPAuthenticationError
#   connect       greeting 554 -> SMTPConnectError
#   sender        MAIL refused -> SMTPSenderRefused
#   recipient     RCPT refused -> SMTPRecipientsRefused
#   disconnected  the server drops after its greeting -> SMTPServerDisconnected
#   data-error    the DATA terminator is refused -> SMTPDataError (generic 400)
#   quit-error    the QUIT reply is refused (still a 200: `quit()` ignores it)
#
# The scenario's `smtp_port`/`smtp_password` are per-target variables, so each
# backend talks to its own listener and the two JSONL files can be compared.
# Everything else is passed to `difftest` unchanged (managed mode, `--a-var`
# values included). Exit code is the difftest code, or 1 if the two SMTP logs
# disagree.

set -euo pipefail

SERVER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../upstream/oracle.sh
source "$SERVER_DIR/tools/upstream/oracle.sh"
PYTHON="${PLANE_PYTHON:-$(oracle_python)}"
BASE_PORT="${FAKE_SMTP_PORT:-18410}"
LOG_DIR="$SERVER_DIR/target/smtp-scenarios"

if [[ $# -lt 1 ]]; then
  echo "usage: run-smtp-scenario.sh <mode> <difftest run args...>" >&2
  exit 2
fi

mode="$1"
shift

case "$mode" in
  success) options=(--auth required); password="plane" ;;
  auth) options=(--auth required); password="wrong-password" ;;
  connect) options=(--greeting 554 --auth none); password="" ;;
  sender) options=(--reject-sender --auth none); password="" ;;
  recipient) options=(--reject-recipient --auth none); password="" ;;
  disconnected) options=(--drop-after-greeting --auth none); password="" ;;
  data-error) options=(--data-error --auth none); password="" ;;
  quit-error) options=(--quit-error --auth none); password="" ;;
  # A 5 s pause before the greeting: `EMAIL_TIMEOUT` is unset, so both
  # backends wait (Django's socket has no timeout either).
  delay) options=(--delay 5 --auth none); password="" ;;
  *)
    echo "run-smtp-scenario: unknown mode '$mode'" >&2
    exit 2
    ;;
esac

if [[ ! -x "$PYTHON" ]]; then
  echo "run-smtp-scenario: no interpreter at $PYTHON (set PLANE_PYTHON)" >&2
  exit 1
fi

a_port="$BASE_PORT"
b_port="$((BASE_PORT + 1))"
for port in "$a_port" "$b_port"; do
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "run-smtp-scenario: 127.0.0.1:$port is already in use" >&2
    exit 1
  fi
done

mkdir -p "$LOG_DIR"
a_log="$LOG_DIR/${mode}-a.jsonl"
b_log="$LOG_DIR/${mode}-b.jsonl"
a_out="$LOG_DIR/${mode}-a.log"
b_out="$LOG_DIR/${mode}-b.log"

a_pid=""
b_pid=""
cleanup() {
  if [[ -n "$a_pid" ]]; then
    kill "$a_pid" 2>/dev/null || true
    wait "$a_pid" 2>/dev/null || true
  fi
  if [[ -n "$b_pid" ]]; then
    kill "$b_pid" 2>/dev/null || true
    wait "$b_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

"$PYTHON" "$SERVER_DIR/tools/mail/fake_smtp.py" \
  --port "$a_port" --output "$a_log" ${options[@]+"${options[@]}"} >"$a_out" 2>&1 &
a_pid=$!
"$PYTHON" "$SERVER_DIR/tools/mail/fake_smtp.py" \
  --port "$b_port" --output "$b_log" ${options[@]+"${options[@]}"} >"$b_out" 2>&1 &
b_pid=$!

# Wait for the listener's own "listening" line instead of opening a probe
# connection: the fake server records every connection, and a readiness probe
# would appear in the transcript as a greeting-only transaction.
wait_listening() {
  local log="$1"
  for _ in $(seq 1 200); do
    if grep -q "fake-smtp listening on" "$log" 2>/dev/null; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

wait_listening "$a_out" || {
  echo "run-smtp-scenario: listener A did not start on $a_port" >&2
  exit 1
}
wait_listening "$b_out" || {
  echo "run-smtp-scenario: listener B did not start on $b_port" >&2
  exit 1
}

status=0
"$SERVER_DIR/target/debug/difftest" run \
  --a-var "smtp_port=$a_port" --b-var "smtp_port=$b_port" \
  --a-var "smtp_password=$password" --b-var "smtp_password=$password" \
  "$@" || status=$?

# Give the listeners a moment to flush the last transaction before reading.
sleep 0.2
compare=0
"$PYTHON" "$SERVER_DIR/tools/mail/compare_smtp_logs.py" \
  --a "$a_log" --b "$b_log" --expect "$mode" || compare=$?

if ((compare != 0)); then
  exit 1
fi
exit "$status"
