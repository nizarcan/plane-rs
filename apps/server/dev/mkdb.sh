#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Clone a template database for isolated work.
#
#   apps/server/dev/mkdb.sh t_task_1
#   apps/server/dev/mkdb.sh t_task_1 --replace   # drop and re-clone
#   apps/server/dev/mkdb.sh t_task_1 --template plane_fix_tpl
#
# The default `plane_tpl` contains the migrated schema, one instance row and no
# users or workspaces. `plane_fix_tpl` is the rich deterministic fixture built
# by `apps/server/dev/bootstrap-fixture.sh` (users, workspaces, issues, …);
# `--template` accepts only those two names. Use a unique name per agent/run:
# `t_<task>_<n>`, lowercase. Remember to pick a matching Redis DB index and
# RabbitMQ vhost as well; see dev/README.md ("Isolation for concurrent agents").

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

usage() {
  cat <<'EOF'
usage: mkdb.sh <name> [--replace] [--template <name>]

  <name>              database to create (e.g. t_task_1)
  --replace           drop an existing database with that name first
  --template <name>   template to clone: plane_tpl (default) or plane_fix_tpl
EOF
}

name=""
replace=0
template=""
while (($#)); do
  case "$1" in
    --replace) replace=1; shift ;;
    --template)
      [[ $# -ge 2 ]] || die "--template needs a name (plane_tpl or plane_fix_tpl)"
      template="$2"
      shift 2
      ;;
    --template=*)
      template="${1#*=}"
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      die "unknown option '$1'"
      ;;
    *)
      [[ -z "$name" ]] || die "only one database name may be given (got '$name' and '$1')"
      name="$1"
      shift
      ;;
  esac
done

if [[ -z "$name" ]]; then
  usage >&2
  exit 2
fi

validate_db_name "$name"

if [[ -n "$template" ]]; then
  case "$template" in
    plane_tpl | plane_fix_tpl) PLANE_TEMPLATE_DB="$template" ;;
    *) die "--template must be plane_tpl or plane_fix_tpl (got '$template')" ;;
  esac
fi

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
# template. Both templates are created with ALLOW_CONNECTIONS false, so no
# session can be attached and `pg_terminate_backend` has nothing to do; skip it
# there (and in particular never touch `plane_fix_tpl`, which other agents
# clone concurrently). A template that does allow connections keeps the guard.
if [[ "$(psql_dev postgres -tAc "SELECT datallowconn FROM pg_database WHERE datname = '$PLANE_TEMPLATE_DB'")" == "t" ]]; then
  terminate_connections "$PLANE_TEMPLATE_DB"
fi

note "cloning '$PLANE_TEMPLATE_DB' -> '$name'"
psql_dev postgres -c "CREATE DATABASE \"$name\" TEMPLATE \"$PLANE_TEMPLATE_DB\""

note "database '$name' is ready"
printf '%s\n' \
  "  postgresql://$PLANE_PG_USER:$PLANE_PG_PASSWORD@$PLANE_PG_HOST:$PLANE_PG_PORT/$name" \
  "  PLANE_DB=$name apps/server/dev/django-serve.sh 18000" >&2
