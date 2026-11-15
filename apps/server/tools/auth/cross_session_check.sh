#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Phase 2 exit check: sessions and CSRF tokens created by one backend must be
# accepted by the other.
#
#   apps/server/tools/auth/cross_session_check.sh          # build, run, clean up
#   KEEP=1 apps/server/tools/auth/cross_session_check.sh   # keep the database
#
# What it does, on ONE shared database clone (`t_auth_x` by default):
#
#   1. starts Django (18010) and a `plane-api` built with `--features
#      test-endpoints` (18110) against the same database, `SECRET_KEY` and
#      Redis/vhost;
#   2. (a) bootstraps the instance admin and a user through Django's real
#      `/auth/` form flow, then calls Rust's `GET /_plane_rs/test/whoami` with
#      Django's `session-id` cookie and compares user ids;
#   3. (b) logs the same user in through Rust's `POST /_plane_rs/test/login`,
#      calls Django's `GET /api/users/me/` with Rust's cookie, signs out through
#      Django's `POST /auth/sign-out/`, and shows Rust reports anonymous and
#      deletes the stale cookie;
#   4. CSRF interop: a Django-issued masked token + cookie is accepted by Rust's
#      CSRF check, and a Rust-issued one is accepted by Django's `POST
#      /auth/sign-in/` (302, not the failure page);
#   5. measures p50/p95 of 1000 `/_plane_rs/test/whoami` requests with a valid
#      session and 1000 `GET /` requests with no session (release build).
#
# Every assertion prints `ok:`; a failure prints `FAIL:` and exits non-zero.
# The instance admin cookie path (`admin-session-id`) is covered by the unit
# test `middleware::session::tests::admin_paths_use_the_admin_cookie`, because
# `/_plane_rs/` is not an `instances` path.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../dev" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_DEV_DIR/lib.sh"

DB="${DB:-t_auth_x}"
DJANGO_PORT="${DJANGO_PORT:-18010}"
RUST_PORT="${RUST_PORT:-18110}"
REDIS_DB="${REDIS_DB:-13}"
MQ_VHOST="${MQ_VHOST:-$DB}"
RUST_BIN="$PLANE_SERVER_DIR/target/auth/release/plane-api"
LOG_DIR="$PLANE_SERVER_DIR/target/auth/logs"
EMAIL="${EMAIL:-auth-check-user@example.com}"
ADMIN_EMAIL="${ADMIN_EMAIL:-auth-check-admin@example.com}"
PASSWORD="${PASSWORD:-Diff-Test-Passw0rd-2026!}"
JAR_DIR="$(mktemp -d "${TMPDIR:-/tmp}/auth-check.XXXXXX")"

django_pid=""
rust_pid=""

note() { printf '==> %s\n' "$*"; }
ok() { printf 'ok: %s\n' "$*"; }
fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  local status=$?
  [[ -n "$rust_pid" ]] && kill "$rust_pid" 2>/dev/null || true
  [[ -n "$django_pid" ]] && kill "$django_pid" 2>/dev/null || true
  wait 2>/dev/null || true
  if [[ "${KEEP:-0}" != "1" ]]; then
    "$PLANE_DEV_DIR/dropdb.sh" "$DB" >/dev/null 2>&1 || true
    "$PLANE_DEV_DIR/mq-vhost.sh" --delete "$MQ_VHOST" >/dev/null 2>&1 || true
    rm -rf "$JAR_DIR"
  else
    note "KEEP=1: database '$DB', vhost '$MQ_VHOST' and jars in $JAR_DIR kept"
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

wait_http() {
  local url=$1 name=$2
  for _ in $(seq 1 240); do
    if curl -sf -o /dev/null "$url"; then
      return 0
    fi
    sleep 0.25
  done
  fail "$name did not answer on $url"
}

json_field() {
  # json_field <file> <pointer>
  python3 -c 'import json,sys; v=json.load(open(sys.argv[1]));
for p in sys.argv[2].strip("/").split("/"):
    v = v[int(p)] if isinstance(v, list) else v[p]
print(v)' "$1" "$2"
}

jar_value() {
  # jar_value <jar> <cookie-name>
  awk -v name="$2" '$6 == name { print $7 }' "$1" | tail -1
}

status_of() {
  # status_of <url> <method> <jar> [extra curl args...]
  local url=$1 method=$2 jar=$3
  shift 3
  curl -sS -o /dev/null -w '%{http_code}' -X "$method" -b "$jar" -c "$jar" "$@" "$url"
}

# ---------------------------------------------------------------------------
# 0. Build and start both backends on one database.
# ---------------------------------------------------------------------------
note "building plane-api with --features test-endpoints (release)"
(cd "$PLANE_SERVER_DIR" && CARGO_TARGET_DIR="$PLANE_SERVER_DIR/target/auth" \
  cargo build --release -p plane-api --features test-endpoints >/dev/null)
[[ -x "$RUST_BIN" ]] || fail "missing $RUST_BIN"

"$PLANE_DEV_DIR/mkdb.sh" "$DB" --replace >/dev/null
"$PLANE_DEV_DIR/mq-vhost.sh" "$MQ_VHOST" >/dev/null
mkdir -p "$LOG_DIR"

note "starting Django on :$DJANGO_PORT (db=$DB redis-db=$REDIS_DB vhost=$MQ_VHOST)"
(
  cd "$PLANE_SERVER_DIR"
  PLANE_DB="$DB" PLANE_REDIS_DB="$REDIS_DB" PLANE_MQ_VHOST="$MQ_VHOST" \
    "$PLANE_DEV_DIR/django-serve.sh" "$DJANGO_PORT"
) >"$LOG_DIR/django-auth.log" 2>&1 &
django_pid=$!
wait_http "http://127.0.0.1:$DJANGO_PORT/robots.txt" "Django"

note "starting plane-api on :$RUST_PORT (same database and SECRET_KEY)"
(
  cd "$PLANE_SERVER_DIR"
  # shellcheck disable=SC1090
  eval "$(PLANE_DB="$DB" PLANE_REDIS_DB="$REDIS_DB" PLANE_MQ_VHOST="$MQ_VHOST" "$PLANE_DEV_DIR/django-env.sh")"
  PLANE_RS_BIND="127.0.0.1:$RUST_PORT" PLANE_RS_LOG_FORMAT=json "$RUST_BIN"
) >"$LOG_DIR/rust-auth.log" 2>&1 &
rust_pid=$!
wait_http "http://127.0.0.1:$RUST_PORT/robots.txt" "plane-api"

DJANGO="http://127.0.0.1:$DJANGO_PORT"
RUST="http://127.0.0.1:$RUST_PORT"

# ---------------------------------------------------------------------------
# 1. Django creates the instance admin and a user, exactly like
#    scenarios/basic-session.yaml (CSRF token, form posts).
# ---------------------------------------------------------------------------
ADMIN_JAR="$JAR_DIR/admin.jar"
admin_csrf=$(curl -sS -c "$ADMIN_JAR" "$DJANGO/auth/get-csrf-token/" | python3 -c 'import json,sys; print(json.load(sys.stdin)["csrf_token"])')
status=$(status_of "$DJANGO/api/instances/admins/sign-up/" POST "$ADMIN_JAR" \
  --data-urlencode "email=$ADMIN_EMAIL" \
  --data-urlencode "password=$PASSWORD" \
  --data-urlencode "first_name=Auth" \
  --data-urlencode "last_name=Admin" \
  --data-urlencode "company_name=Auth Check Inc" \
  --data-urlencode "is_telemetry_enabled=True" \
  --data-urlencode "csrfmiddlewaretoken=$admin_csrf")
[[ "$status" == "302" ]] || fail "admin sign-up answered $status, expected 302"
[[ -n "$(jar_value "$ADMIN_JAR" admin-session-id)" ]] || fail "admin sign-up did not set admin-session-id"
ok "instance admin created; admin-session-id cookie set"

USER_JAR="$JAR_DIR/user.jar"
user_csrf=$(curl -sS -c "$USER_JAR" "$DJANGO/auth/get-csrf-token/" | python3 -c 'import json,sys; print(json.load(sys.stdin)["csrf_token"])')
status=$(status_of "$DJANGO/auth/sign-up/" POST "$USER_JAR" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=$PASSWORD" \
  --data-urlencode "csrfmiddlewaretoken=$user_csrf")
[[ "$status" == "302" ]] || fail "user sign-up answered $status, expected 302"
[[ -n "$(jar_value "$USER_JAR" session-id)" ]] || fail "user sign-up did not set session-id"
ok "user created; Django session-id cookie set"

status=$(status_of "$DJANGO/api/users/me/" GET "$USER_JAR")
[[ "$status" == "200" ]] || fail "Django /api/users/me/ answered $status"
curl -sS -b "$USER_JAR" "$DJANGO/api/users/me/" >"$JAR_DIR/me.json"
django_user_id=$(json_field "$JAR_DIR/me.json" "/id")
ok "Django reports user $django_user_id"

# ---------------------------------------------------------------------------
# 2. (a) Django's session cookie works on Rust.
# ---------------------------------------------------------------------------
curl -sS -b "$USER_JAR" "$RUST/_plane_rs/test/whoami" >"$JAR_DIR/whoami_a.json"
rust_user_id=$(json_field "$JAR_DIR/whoami_a.json" "/user_id")
[[ "$rust_user_id" == "$django_user_id" ]] ||
  fail "Rust whoami returned '$rust_user_id' for Django's session, expected '$django_user_id'"
[[ "$(json_field "$JAR_DIR/whoami_a.json" "/session_key_present")" == "True" ]] ||
  fail "Rust whoami did not see the session cookie"
ok "Django session accepted by Rust: whoami = $rust_user_id"

# ---------------------------------------------------------------------------
# 3. (b) Rust's session works on Django, and Django's sign-out kills it.
# ---------------------------------------------------------------------------
RUST_JAR="$JAR_DIR/rust.jar"
rust_csrf=$(curl -sS -c "$RUST_JAR" "$RUST/_plane_rs/test/csrf-token" | python3 -c 'import json,sys; print(json.load(sys.stdin)["csrf_token"])')
[[ -n "$(jar_value "$RUST_JAR" csrftoken)" ]] || fail "Rust did not set the csrftoken cookie"

# The endpoint is CSRF-protected like Django's form views: no token is the
# 200 HTML failure page, not a 403, and it does not rotate the CSRF cookie.
status=$(curl -sS -o "$JAR_DIR/csrf_fail.html" -w '%{http_code}' \
  -b "$RUST_JAR" -c "$RUST_JAR" -H 'Content-Type: application/json' \
  -X POST --data-binary "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" \
  "$RUST/_plane_rs/test/login")
[[ "$status" == "200" ]] || fail "unprotected Rust login answered $status"
grep -q "CSRF Verification Failed" "$JAR_DIR/csrf_fail.html" ||
  fail "a Rust login without a CSRF token did not return the failure page"
ok "Rust login without a CSRF token returns Django's failure page"

# A successful login rotates the CSRF secret (Django's `login` calls
# `rotate_token`), so this must be one request: reuse of the old token would
# fail exactly as it does in Django.
status=$(curl -sS -o "$JAR_DIR/login.json" -w '%{http_code}' \
  -b "$RUST_JAR" -c "$RUST_JAR" -H 'Content-Type: application/json' \
  -H "X-CSRFToken: $rust_csrf" -X POST \
  --data-binary "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" \
  "$RUST/_plane_rs/test/login")
[[ "$status" == "200" ]] || fail "Rust login answered $status, expected 200"
[[ "$(json_field "$JAR_DIR/login.json" "/user_id")" == "$django_user_id" ]] ||
  fail "Rust login returned a different user"
[[ -n "$(jar_value "$RUST_JAR" session-id)" ]] || fail "Rust login did not set session-id"
rust_session=$(jar_value "$RUST_JAR" session-id)
ok "Rust login created session ${rust_session:0:8}… for $django_user_id"

# The row must carry Plane's `create_model_instance` columns and the signed
# payload must decode with Django's own SessionStore.
row=$(psql_dev "$DB" -tAc "SELECT user_id || '|' || coalesce(device_info->>'domain', '') \
  FROM sessions WHERE session_key = '$rust_session'")
[[ "$row" == "$django_user_id|http://127.0.0.1:18080" ]] ||
  fail "Rust's sessions row has user_id/device_info '$row'"
session_data=$(psql_dev "$DB" -tAc "SELECT session_data FROM sessions WHERE session_key = '$rust_session'")
(
  cd "$PLANE_API_DIR"
  DJANGO_SETTINGS_MODULE=plane.settings.production DEBUG=0 IS_TEST=0 \
    SECRET_KEY="$SECRET_KEY" DATABASE_URL="postgresql://x:x@127.0.0.1:1/x" \
    REDIS_URL="redis://127.0.0.1:1/0" \
    "$PLANE_DJANGO_PYTHON" - "$session_data" "$django_user_id" <<'PY'
import sys

import django

django.setup()
from django.contrib.auth import BACKEND_SESSION_KEY, SESSION_KEY  # noqa: E402
from plane.db.models.session import SessionStore  # noqa: E402

data = SessionStore().decode(sys.argv[1])
assert data[SESSION_KEY] == sys.argv[2], data
assert data[BACKEND_SESSION_KEY] == "django.contrib.auth.backends.ModelBackend", data
assert set(data["device_info"]) == {"user_agent", "ip_address", "domain"}, data
assert len(data["_auth_user_hash"]) == 64, data
print("django decoded Rust's session payload")
PY
)
ok "Django decodes Rust's session payload; user_id/device_info columns are correct"

status=$(status_of "$DJANGO/api/users/me/" GET "$RUST_JAR")
[[ "$status" == "200" ]] || fail "Django /api/users/me/ with Rust's session answered $status"
curl -sS -b "$RUST_JAR" "$DJANGO/api/users/me/" >"$JAR_DIR/me_rust.json"
[[ "$(json_field "$JAR_DIR/me_rust.json" "/id")" == "$django_user_id" ]] ||
  fail "Django resolved a different user from Rust's session"
ok "Rust session accepted by Django: GET /api/users/me/ = $django_user_id"

# Django's sign-out is a plain View, so it needs a CSRF token. The cookie's
# 32-character secret is accepted directly in the header.
csrf_secret=$(jar_value "$RUST_JAR" csrftoken)
# Deliberately no `-c` on the sign-out call: the jar keeps the stale cookie so
# the next step can prove Rust deletes it.
status=$(curl -sS -o /dev/null -w '%{http_code}' -b "$RUST_JAR" -X POST \
  -H "X-CSRFToken: $csrf_secret" "$DJANGO/auth/sign-out/")
[[ "$status" == "302" ]] || fail "Django sign-out answered $status, expected 302"
[[ "$(jar_value "$RUST_JAR" session-id)" == "$rust_session" ]] ||
  fail "the jar lost the session cookie before the Rust check"
remaining=$(psql_dev "$DB" -tAc "SELECT count(*) FROM sessions WHERE session_key = '$rust_session'")
[[ "$remaining" == "0" ]] || fail "Django's sign-out left $remaining session rows"
ok "Django sign-out deleted session ${rust_session:0:8}… from the database"

curl -sS -i -b "$RUST_JAR" "$RUST/_plane_rs/test/whoami" >"$JAR_DIR/whoami_b.txt"
grep -q '"user_id":null' "$JAR_DIR/whoami_b.txt" ||
  fail "Rust whoami still reports a user after Django's sign-out"
grep -qi '^set-cookie: session-id=""' "$JAR_DIR/whoami_b.txt" ||
  fail "Rust did not delete the stale session cookie"
ok "Rust reports anonymous and deletes the stale cookie after Django's sign-out"

# ---------------------------------------------------------------------------
# 4. CSRF interop in both directions.
# ---------------------------------------------------------------------------
# Django -> Rust: a masked token and its cookie from /auth/get-csrf-token/.
DJ_CSRF_JAR="$JAR_DIR/django-csrf.jar"
django_token=$(curl -sS -c "$DJ_CSRF_JAR" "$DJANGO/auth/get-csrf-token/" | python3 -c 'import json,sys; print(json.load(sys.stdin)["csrf_token"])')
[[ ${#django_token} -eq 64 ]] || fail "Django's csrf token is not 64 characters"
status=$(status_of "$RUST/_plane_rs/test/login" POST "$DJ_CSRF_JAR" \
  -H 'Content-Type: application/json' -H "X-CSRFToken: $django_token" \
  --data-binary "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}")
[[ "$status" == "200" ]] || fail "Rust rejected Django's CSRF token ($status)"
ok "Django-issued masked CSRF token accepted by Rust"

# Rust -> Django: a masked token and its cookie from get_token.
RS_CSRF_JAR="$JAR_DIR/rust-csrf.jar"
rust_token=$(curl -sS -c "$RS_CSRF_JAR" "$RUST/_plane_rs/test/csrf-token" | python3 -c 'import json,sys; print(json.load(sys.stdin)["csrf_token"])')
[[ ${#rust_token} -eq 64 ]] || fail "Rust's csrf token is not 64 characters"
status=$(status_of "$DJANGO/auth/sign-in/" POST "$RS_CSRF_JAR" \
  --data-urlencode "email=$EMAIL" \
  --data-urlencode "password=$PASSWORD" \
  --data-urlencode "csrfmiddlewaretoken=$rust_token")
[[ "$status" == "302" ]] || fail "Django rejected Rust's CSRF token ($status)"
ok "Rust-issued masked CSRF token accepted by Django's POST /auth/sign-in/"

# ---------------------------------------------------------------------------
# 5. Cost of the session + user pipeline (release build).
# ---------------------------------------------------------------------------
note "measuring 1000 whoami requests with a valid session and 1000 GET /"
BENCH_JAR="$JAR_DIR/bench.jar"
bench_token=$(curl -sS -c "$BENCH_JAR" "$RUST/_plane_rs/test/csrf-token" | python3 -c 'import json,sys; print(json.load(sys.stdin)["csrf_token"])')
curl -sS -o /dev/null -b "$BENCH_JAR" -c "$BENCH_JAR" -H 'Content-Type: application/json' \
  -H "X-CSRFToken: $bench_token" -X POST "$RUST/_plane_rs/test/login" \
  --data-binary "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}"
[[ -n "$(jar_value "$BENCH_JAR" session-id)" ]] || fail "bench login failed"
python3 - "$BENCH_JAR" "$RUST" <<'PY'
import http.client, statistics, sys, time
jar, base = sys.argv[1], sys.argv[2]
host = base.split("//", 1)[1]
session = None
for line in open(jar):
    parts = line.split("\t")
    if len(parts) >= 7 and parts[5] == "session-id":
        session = parts[6].strip()

def run(path, cookie, count=1000):
    conn = http.client.HTTPConnection(host, timeout=10)
    headers = {"Host": host}
    if cookie:
        headers["Cookie"] = f"session-id={cookie}"
    samples = []
    for _ in range(count):
        start = time.perf_counter()
        conn.request("GET", path, headers=headers)
        response = conn.getresponse()
        response.read()
        samples.append((time.perf_counter() - start) * 1000.0)
        if response.status != 200:
            raise SystemExit(f"{path} answered {response.status}")
    conn.close()
    return samples

def report(label, samples):
    samples.sort()
    p50 = statistics.median(samples)
    p95 = samples[int(len(samples) * 0.95) - 1]
    print(f"{label}: p50 {p50:.3f} ms, p95 {p95:.3f} ms (n={len(samples)})")
    return p50, p95

session_p50, session_p95 = report("GET /_plane_rs/test/whoami (valid session)", run("/_plane_rs/test/whoami", session))
plain_p50, plain_p95 = report("GET / (no session, no logger)", run("/", None))
print(f"session+user overhead: p50 {session_p50 - plain_p50:.3f} ms, p95 {session_p95 - plain_p95:.3f} ms")
PY

note "all checks passed"
