#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Clone the `plane_tpl` template database for isolated work.
#
#   apps/server/dev/mkdb.sh t_task_1
#   apps/server/dev/mkdb.sh t_task_1 --replace   # drop and re-clone
#
# The clone contains the migrated schema, one instance row and no users or
# workspaces. Use a unique name per agent/run: `t_<task>_<n>`, lowercase.
# Remember to pick a matching Redis DB index and RabbitMQ vhost as well; see
# dev/README.md ("Isolation for concurrent agents").

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

usage() {
  cat <<'EOF'
usage: mkdb.sh <name> [--replace]

  <name>       database to create, cloned from plane_tpl (e.g. t_task_1)
  --replace    drop an existing database with that name first
EOF
}

name=""
replace=0
for arg in "$@"; do
  case "$arg" in
    --replace) replace=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      die "unknown option '$arg'"
      ;;
    *)
      [[ -z "$name" ]] || die "only one database name may be given (got '$name' and '$arg')"
      name="$arg"
      ;;
  esac
done

if [[ -z "$name" ]]; then
  usage >&2
  exit 2
fi

validate_db_name "$name"

if [[ "$(psql_dev postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '$PLANE_TEMPLATE_DB'")" != "1" ]]; then
  die "template database '$PLANE_TEMPLATE_DB' does not exist; run apps/server/dev/bootstrap.sh first"
fi

terminate_connections() {
  psql_dev postgres -tAc \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
     WHERE datname = '$1' AND pid <> pg_backend_pid()" >/dev/null
}

if [[ "$(psql_dev postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '$name'")" == "1" ]]; then
  if ((replace == 0)); then
    die "database '$name' already exists (use --replace to drop and re-clone it)"
  fi
  note "dropping existing database '$name'"
  terminate_connections "$name"
  psql_dev postgres -c "DROP DATABASE IF EXISTS \"$name\" WITH (FORCE)"
fi

# CREATE DATABASE ... TEMPLATE fails if any session is attached to the
# template. It is ALLOW_CONNECTIONS false, so this is only a belt-and-braces
# guard against a pre-existing session from an older bootstrap.
terminate_connections "$PLANE_TEMPLATE_DB"

note "cloning '$PLANE_TEMPLATE_DB' -> '$name'"
psql_dev postgres -c "CREATE DATABASE \"$name\" TEMPLATE \"$PLANE_TEMPLATE_DB\""

note "database '$name' is ready"
printf '%s\n' \
  "  postgresql://$PLANE_PG_USER:$PLANE_PG_PASSWORD@$PLANE_PG_HOST:$PLANE_PG_PORT/$name" \
  "  PLANE_DB=$name apps/server/dev/django-serve.sh 18000" >&2
