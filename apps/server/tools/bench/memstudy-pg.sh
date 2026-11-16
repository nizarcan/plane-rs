#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# A private Postgres for the memory study, cloned from the shared fixture.
#
# The shared dev Postgres carries several agents' differential runs; their
# prepared statements are indistinguishable from ours in the container's
# memory, so the server-side half of the attribution cannot be measured there.
# This script starts `plane-rs-mem-pg` (postgres:15.7-alpine, 1 GiB) on the dev
# network as `postgres-mem`, dumps a clone of `plane_fix_tpl` and restores it
# into `t_rsmem_api`. Only containers and databases named plane-rs-mem-*/t_rsmem_*
# are created or dropped.
#
#   tools/bench/memstudy-pg.sh up
#   tools/bench/memstudy-pg.sh down
set -euo pipefail

TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$TOOLS_DIR/../../dev/lib.sh"

CONTAINER="plane-rs-mem-pg"
NETWORK="plane-rs-dev_default"
TARGET_DB="t_rsmem_api"
DUMP_DB="t_rsmem_dump"
DUMP_FILE="/tmp/rsmem-fixture.dump"

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

remove_container() {
  [[ "$1" == plane-rs-mem-* ]] || die "refusing to remove '$1'"
  if docker container inspect "$1" >/dev/null 2>&1; then
    docker rm -f "$1" >/dev/null
  fi
}

down() {
  remove_container "$CONTAINER"
  psql_dev plane -c "DROP DATABASE IF EXISTS \"$DUMP_DB\" WITH (FORCE)" >/dev/null 2>&1 || true
  rm -f "$DUMP_FILE"
  printf '==> %s removed\n' "$CONTAINER" >&2
}

up() {
  remove_container "$CONTAINER"
  printf '==> starting %s (private, not the shared dev Postgres)\n' "$CONTAINER" >&2
  docker run -d --name "$CONTAINER" \
    --network "$NETWORK" --network-alias postgres-mem \
    --memory 1g --memory-swap 1g \
    -e POSTGRES_USER=plane -e POSTGRES_PASSWORD=plane -e POSTGRES_DB=plane \
    postgres:15.7-alpine \
    -c fsync=off -c synchronous_commit=off -c full_page_writes=off \
    -c shared_buffers=96MB -c max_connections=100 >/dev/null

  local deadline=$((SECONDS + 60))
  until docker exec "$CONTAINER" pg_isready -U plane -d plane >/dev/null 2>&1; do
    ((SECONDS < deadline)) || die "timed out waiting for $CONTAINER"
    sleep 0.5
  done

  printf '==> dumping a clone of plane_fix_tpl from the shared dev Postgres\n' >&2
  psql_dev plane -c "DROP DATABASE IF EXISTS \"$DUMP_DB\" WITH (FORCE)" >/dev/null
  psql_dev plane -c "CREATE DATABASE \"$DUMP_DB\" TEMPLATE plane_fix_tpl" >/dev/null
  # `pg_dump` runs inside the private container: it is the same 15.7 binary as
  # the server, and the host's client may be older than 15 (pg_dump refuses to
  # dump a newer server). The container reaches the shared server as `postgres`
  # on plane-rs-dev_default.
  docker exec -e PGPASSWORD="$PLANE_PG_PASSWORD" "$CONTAINER" \
    pg_dump -h postgres -p 5432 -U plane -Fc -f "$DUMP_FILE" "$DUMP_DB"
  psql_dev plane -c "DROP DATABASE \"$DUMP_DB\" WITH (FORCE)" >/dev/null

  printf '==> restoring into %s\n' "$TARGET_DB" >&2
  docker exec "$CONTAINER" psql -q -U plane -d plane -c "CREATE DATABASE \"$TARGET_DB\""
  docker exec "$CONTAINER" pg_restore -U plane -d "$TARGET_DB" --no-owner --no-privileges \
    "$DUMP_FILE"
  docker exec "$CONTAINER" rm -f "$DUMP_FILE"
  printf '==> %s ready: postgresql://plane:plane@postgres-mem:5432/%s\n' "$CONTAINER" "$TARGET_DB" >&2
}

case "${1:-}" in
  up) up ;;
  down) down ;;
  *) die "usage: memstudy-pg.sh up|down" ;;
esac
