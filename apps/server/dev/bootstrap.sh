#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Build the `plane_tpl` template database: a migrated schema plus exactly the
# rows the real API entrypoint creates on a fresh instance, and nothing else.
#
#   apps/server/dev/bootstrap.sh            build it if it does not exist
#   apps/server/dev/bootstrap.sh --force    drop and rebuild it
#
# The steps mirror apps/api/bin/docker-entrypoint-api.sh:
#   migrate, register_instance <fixed signature>, configure_instance,
#   create_bucket.
# `clear_cache` and `collectstatic` are deliberately skipped: they touch the
# shared Redis and the static-files tree, not the database, and django-serve.sh
# does not need them.
#
# The template is marked `IS_TEMPLATE true` and `ALLOW_CONNECTIONS false`, so
# `CREATE DATABASE ... TEMPLATE plane_tpl` is reproducible and nothing can
# accidentally write to it. Every cloned database therefore starts with one
# instance row and no users or workspaces.
#
# The encrypted InstanceConfiguration values are keyed off SECRET_KEY from
# .env.local. If that key is regenerated, rebuild the template with --force.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

usage() {
  cat <<'EOF'
usage: bootstrap.sh [--force]

  (no options)  build plane_tpl if it does not exist, otherwise do nothing
  --force       drop and rebuild plane_tpl
EOF
}

force=0
for arg in "$@"; do
  case "$arg" in
    --force) force=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown argument '$arg'" ;;
  esac
done

require_cmd docker
[[ -x "$PLANE_DJANGO_PYTHON" ]] ||
  die "Django virtualenv not found at $PLANE_DJANGO_PYTHON (the orchestrator creates it with uv)"

# Fixed for every bootstrap so a rebuilt template is comparable. The command
# does not persist it (register_instance only validates that it is non-empty),
# but passing the same value keeps invocations deterministic.
MACHINE_SIGNATURE="plane-rs-dev-template"

database_exists() {
  [[ "$(psql_dev postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '$1'")" == "1" ]]
}

terminate_template_connections() {
  psql_dev postgres -tAc \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
     WHERE datname = '$PLANE_TEMPLATE_DB' AND pid <> pg_backend_pid()" >/dev/null
}

# --- prerequisites ---------------------------------------------------------

# up.sh is idempotent and waits for healthchecks; the Celery publish inside
# register_instance and the bucket creation both need the stack running.
if ! "$PLANE_DEV_DIR/up.sh" >/dev/null 2>&1; then
  die "the dev stack did not come up; run apps/server/dev/up.sh to see why"
fi

if database_exists "$PLANE_TEMPLATE_DB"; then
  if ((force == 0)); then
    note "template database '$PLANE_TEMPLATE_DB' already exists; nothing to do (use --force to rebuild)"
    exit 0
  fi
  note "dropping existing template database '$PLANE_TEMPLATE_DB'"
  # Postgres refuses to drop a database while it is marked as a template, and
  # ALLOW_CONNECTIONS false would block the check query, so undo both first.
  psql_dev postgres -c "ALTER DATABASE \"$PLANE_TEMPLATE_DB\" IS_TEMPLATE false"
  psql_dev postgres -c "ALTER DATABASE \"$PLANE_TEMPLATE_DB\" ALLOW_CONNECTIONS true"
  terminate_template_connections
  # FORCE evicts any session that slipped in between the two statements.
  psql_dev postgres -c "DROP DATABASE IF EXISTS \"$PLANE_TEMPLATE_DB\" WITH (FORCE)"
fi

# --- build ----------------------------------------------------------------

note "creating database '$PLANE_TEMPLATE_DB'"
psql_dev postgres -c "CREATE DATABASE \"$PLANE_TEMPLATE_DB\""

note "running Django migrations"
PLANE_DB="$PLANE_TEMPLATE_DB" "$PLANE_DEV_DIR/django.sh" migrate --noinput

note "registering the instance (fixed dev signature: $MACHINE_SIGNATURE)"
PLANE_DB="$PLANE_TEMPLATE_DB" "$PLANE_DEV_DIR/django.sh" register_instance "$MACHINE_SIGNATURE"

note "loading instance configuration from the environment"
PLANE_DB="$PLANE_TEMPLATE_DB" "$PLANE_DEV_DIR/django.sh" configure_instance

note "creating the object-storage bucket"
PLANE_DB="$PLANE_TEMPLATE_DB" "$PLANE_DEV_DIR/django.sh" create_bucket

# --- verify the contents ---------------------------------------------------

migrations="$(psql_dev "$PLANE_TEMPLATE_DB" -tAc 'SELECT count(*) FROM django_migrations')"
instances="$(psql_dev "$PLANE_TEMPLATE_DB" -tAc 'SELECT count(*) FROM instances')"
configurations="$(psql_dev "$PLANE_TEMPLATE_DB" -tAc 'SELECT count(*) FROM instance_configurations')"
users="$(psql_dev "$PLANE_TEMPLATE_DB" -tAc 'SELECT count(*) FROM users')"
workspaces="$(psql_dev "$PLANE_TEMPLATE_DB" -tAc 'SELECT count(*) FROM workspaces')"

[[ "$instances" == "1" ]] ||
  die "expected exactly 1 row in instances, found $instances; the template is not usable"
[[ "$configurations" -gt 0 ]] ||
  die "instance_configurations is empty; configure_instance did not run"
[[ "$users" == "0" ]] ||
  die "expected no users in the template, found $users (bootstrap must not seed data)"
[[ "$workspaces" == "0" ]] ||
  die "expected no workspaces in the template, found $workspaces (bootstrap must not seed data)"

# --- freeze as a template --------------------------------------------------

terminate_template_connections

note "marking '$PLANE_TEMPLATE_DB' as a template and closing it to connections"
psql_dev postgres -c "ALTER DATABASE \"$PLANE_TEMPLATE_DB\" IS_TEMPLATE true"
psql_dev postgres -c "ALTER DATABASE \"$PLANE_TEMPLATE_DB\" ALLOW_CONNECTIONS false"

# With ALLOW_CONNECTIONS false no new session can appear, but check anyway:
# CREATE DATABASE ... TEMPLATE fails if one exists.
remaining="$(psql_dev postgres -tAc \
  "SELECT count(*) FROM pg_stat_activity WHERE datname = '$PLANE_TEMPLATE_DB'")"
[[ "$remaining" == "0" ]] ||
  die "still $remaining connection(s) to '$PLANE_TEMPLATE_DB'; refusing to leave a busy template"

flags="$(psql_dev postgres -tAc \
  "SELECT datistemplate::text || '/' || datallowconn::text FROM pg_database WHERE datname = '$PLANE_TEMPLATE_DB'")"
[[ "$flags" == "true/false" ]] ||
  die "expected '$PLANE_TEMPLATE_DB' to be datistemplate=true and datallowconn=false, got $flags"

cat <<EOF

Template '$PLANE_TEMPLATE_DB' is ready.
  migrations             $migrations
  instances              $instances
  instance_configurations $configurations
  users                  $users
  workspaces             $workspaces
  datistemplate/allowconn $flags

Clone it for your own work:
  apps/server/dev/mkdb.sh t_<task>_<n>
  PLANE_DB=t_<task>_<n> apps/server/dev/django-serve.sh 18000
Drop it when done:
  apps/server/dev/dropdb.sh t_<task>_<n>
EOF
