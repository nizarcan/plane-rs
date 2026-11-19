#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Build the rich read fixture template `plane_fix_tpl`.
#
#   apps/server/dev/bootstrap-fixture.sh            build it if it does not exist
#   apps/server/dev/bootstrap-fixture.sh --force    drop and rebuild it
#
# Steps:
#   1. clone the empty template `plane_tpl` into `plane_fix_tpl`;
#   2. run tools/fixtures/build.py through the Django ORM (save hooks run as in
#      production), pinning every timestamp to one `--base-time`;
#   3. create one signed session row per fixture user plus the instance
#      admin's cookie (the dev SECRET_KEY from dev/.env.local signs them);
#   4. VACUUM ANALYZE;
#   5. mark the database as a template and close it to connections.
#
# The manifest `scenarios/fixtures/fix_tpl.json` lists every id, slug and
# session value a read scenario may reference.
#
# The base time is recorded inside the fixture (cycle windows, timestamps,
# date anchors) and in the manifest. Scenarios must not depend on the exact day
# a "current" cycle is read: the window is -20..+20 days around the base time.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

FIXTURE_DB="plane_fix_tpl"
BASE_TEMPLATE="$PLANE_TEMPLATE_DB"
MANIFEST="$PLANE_SERVER_DIR/scenarios/fixtures/fix_tpl.json"
BUILD_SCRIPT="$PLANE_SERVER_DIR/tools/fixtures/build.py"

usage() {
  cat <<'EOF'
usage: bootstrap-fixture.sh [--force] [--base-time <RFC3339>]

  (no options)  build plane_fix_tpl if it does not exist, otherwise do nothing
  --force       drop and rebuild plane_fix_tpl
  --base-time   instant every timestamp is derived from (default: now,
                truncated to the hour). Recorded in the manifest.
EOF
}

force=0
base_time=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --force) force=1 ;;
    --base-time)
      shift
      [[ $# -gt 0 ]] || die "--base-time needs a value"
      base_time="$1"
      ;;
    --base-time=*) base_time="${1#--base-time=}" ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown argument '$1'" ;;
  esac
  shift
done

require_cmd docker
[[ -x "$PLANE_DJANGO_PYTHON" ]] ||
  die "Django oracle not ready at $PLANE_DJANGO_PYTHON; run apps/server/tools/upstream/oracle.sh fetch"
[[ -f "$PLANE_DEV_ENV_FILE" ]] ||
  die "$PLANE_DEV_ENV_FILE is missing; run dev/bootstrap.sh first so SECRET_KEY exists"
[[ -f "$BUILD_SCRIPT" ]] || die "fixture builder not found at $BUILD_SCRIPT"

if [[ -z "$base_time" ]]; then
  base_time="$(python3 - <<'PY'
from datetime import datetime, timezone
print(datetime.now(timezone.utc).replace(minute=0, second=0, microsecond=0).isoformat().replace("+00:00", "Z"))
PY
)"
fi

if ! "$PLANE_DEV_DIR/up.sh" >/dev/null 2>&1; then
  die "the dev stack did not come up; run apps/server/dev/up.sh to see why"
fi

database_exists() {
  [[ "$(psql_dev postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '$1'")" == "1" ]]
}

terminate_connections() {
  psql_dev postgres -tAc \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
     WHERE datname = '$1' AND pid <> pg_backend_pid()" >/dev/null
}

database_exists "$BASE_TEMPLATE" ||
  die "$BASE_TEMPLATE does not exist; run apps/server/dev/bootstrap.sh first"

if database_exists "$FIXTURE_DB"; then
  if ((force == 0)); then
    note "fixture database '$FIXTURE_DB' already exists; nothing to do (use --force to rebuild)"
    exit 0
  fi
  note "dropping existing fixture database '$FIXTURE_DB'"
  psql_dev postgres -c "ALTER DATABASE \"$FIXTURE_DB\" IS_TEMPLATE false"
  psql_dev postgres -c "ALTER DATABASE \"$FIXTURE_DB\" ALLOW_CONNECTIONS true"
  terminate_connections "$FIXTURE_DB"
  psql_dev postgres -c "DROP DATABASE IF EXISTS \"$FIXTURE_DB\" WITH (FORCE)"
fi

note "cloning '$BASE_TEMPLATE' into '$FIXTURE_DB'"
terminate_connections "$BASE_TEMPLATE"
psql_dev postgres -c "CREATE DATABASE \"$FIXTURE_DB\" TEMPLATE \"$BASE_TEMPLATE\""

# Anything that fails after this point must not leave a broken template
# behind: drop the half-built database instead of leaving something that
# looks usable.
cleanup_on_error() {
  local status=$?
  if ((status != 0)) && database_exists "$FIXTURE_DB"; then
    note "build failed; dropping the partial database '$FIXTURE_DB'"
    psql_dev postgres -c "ALTER DATABASE \"$FIXTURE_DB\" ALLOW_CONNECTIONS true" >/dev/null 2>&1 || true
    terminate_connections "$FIXTURE_DB" || true
    psql_dev postgres -c "DROP DATABASE IF EXISTS \"$FIXTURE_DB\" WITH (FORCE)" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup_on_error EXIT

note "building fixture rows (base time $base_time)"
(
  cd "$PLANE_API_DIR"
  export PLANE_DB="$FIXTURE_DB"
  export PLANE_REDIS_DB="${PLANE_REDIS_DB:-9}"
  export PLANE_MQ_VHOST="${PLANE_MQ_VHOST:-plane}"
  # shellcheck disable=SC1090
  source "$PLANE_DEV_DIR/django-env.sh"
  exec "$PLANE_DJANGO_PYTHON" "$BUILD_SCRIPT" \
    --database "$FIXTURE_DB" --base-time "$base_time" --manifest "$MANIFEST"
)

users="$(psql_dev "$FIXTURE_DB" -tAc 'SELECT count(*) FROM users')"
sessions="$(psql_dev "$FIXTURE_DB" -tAc 'SELECT count(*) FROM sessions')"
issues="$(psql_dev "$FIXTURE_DB" -tAc 'SELECT count(*) FROM issues')"

note "vacuum analyze"
psql_dev "$FIXTURE_DB" -c "VACUUM ANALYZE" >/dev/null

terminate_connections "$FIXTURE_DB"

note "marking '$FIXTURE_DB' as a template and closing it to connections"
psql_dev postgres -c "ALTER DATABASE \"$FIXTURE_DB\" IS_TEMPLATE true"
psql_dev postgres -c "ALTER DATABASE \"$FIXTURE_DB\" ALLOW_CONNECTIONS false"

remaining="$(psql_dev postgres -tAc \
  "SELECT count(*) FROM pg_stat_activity WHERE datname = '$FIXTURE_DB'")"
[[ "$remaining" == "0" ]] ||
  die "still $remaining connection(s) to '$FIXTURE_DB'; refusing to leave a busy template"

flags="$(psql_dev postgres -tAc \
  "SELECT datistemplate::text || '/' || datallowconn::text FROM pg_database WHERE datname = '$FIXTURE_DB'")"
[[ "$flags" == "true/false" ]] ||
  die "expected '$FIXTURE_DB' to be datistemplate=true and datallowconn=false, got $flags"

trap - EXIT

cat <<EOF

Fixture template '$FIXTURE_DB' is ready.
  base time        $base_time
  users            $users
  sessions         $sessions
  issues           $issues
  manifest         $MANIFEST
  datistemplate/allowconn $flags

Read scenarios are generated from the manifest and the route inventory:
  "$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/scenarios/gen_read.py

Run one group against Django only:
  cd apps/server
  ./target/debug/difftest run --scenario scenarios/read/issues.yaml --a django --b django
EOF
