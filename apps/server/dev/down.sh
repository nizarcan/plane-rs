#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Stop the dev stack.
#
#   down.sh            stop and remove the containers; keep the named volumes
#                      (plane_tpl and cloned databases survive)
#   down.sh --purge    also remove the named volumes: the next `up.sh` starts
#                      from an empty Postgres and an empty object store, and
#                      `bootstrap.sh` must rebuild the template

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

usage() {
  cat <<'EOF'
usage: down.sh [--purge]

  (no options)  stop and remove the dev containers, keep the named volumes
  --purge       also remove the named volumes (drops plane_tpl and all data)
EOF
}

purge=0
for arg in "$@"; do
  case "$arg" in
    --purge) purge=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) die "unknown argument '$arg'" ;;
  esac
done

require_cmd docker

if ((purge)); then
  note "stopping $PLANE_COMPOSE_PROJECT and removing volumes"
  compose down --volumes --remove-orphans
else
  note "stopping $PLANE_COMPOSE_PROJECT (volumes kept; use --purge to remove them)"
  compose down --remove-orphans
fi
