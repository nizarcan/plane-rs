#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Drop a database created with mkdb.sh. Idempotent: a missing database is not
# an error. Forcibly terminates connections so a running django-serve.sh does
# not keep the database alive.
#
#   apps/server/dev/dropdb.sh t_task_1
#
# The shared template (`plane_tpl`), the default database (`plane`) and the
# Postgres built-ins are protected. Rebuild the template with
# `bootstrap.sh --force` and drop `plane` with an explicit SQL statement if you
# really need to.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

name="${1:-}"
if [[ -z "$name" ]]; then
  die "usage: $(basename "$0") <name>"
fi
[[ $# -eq 1 ]] || die "unexpected arguments: ${*:2}"

case "$name" in
  "$PLANE_TEMPLATE_DB")
    die "'$PLANE_TEMPLATE_DB' is the shared template; rebuild it with bootstrap.sh --force instead"
    ;;
  plane | postgres | template0 | template1)
    die "refusing to drop the protected database '$name'"
    ;;
esac

validate_db_name "$name"

if [[ "$(psql_dev postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '$name'")" != "1" ]]; then
  note "database '$name' does not exist; nothing to do"
  exit 0
fi

note "dropping database '$name'"
# WITH (FORCE) terminates existing sessions (PostgreSQL 13+; the dev server is 15).
psql_dev postgres -c "DROP DATABASE IF EXISTS \"$name\" WITH (FORCE)"
note "database '$name' dropped"
