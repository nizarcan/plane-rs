#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run a Python file against the dev Django environment, from any directory.
#
#   apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py
#   PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
#     apps/server/tools/query/oracle.py
#
# The database is the `PLANE_DB` variable the dev scripts already use
# (apps/server/dev/django-env.sh), so this never touches a database other than
# the one the caller names. The interpreter is the oracle's (`tools/upstream/oracle.sh`), called
# directly so concurrent agents never trigger a `uv sync`.
set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../dev" && pwd)"
# shellcheck source=../../dev/lib.sh
source "$PLANE_DEV_DIR/lib.sh"
# shellcheck source=../../dev/django-env.sh
source "$PLANE_DEV_DIR/django-env.sh"

[[ $# -ge 1 ]] || {
  echo "usage: $(basename "$0") <script.py> [args...]" >&2
  exit 2
}

[[ -x "$PLANE_DJANGO_PYTHON" ]] || {
  echo "Django oracle not ready at $PLANE_DJANGO_PYTHON; run tools/upstream/oracle.sh fetch" >&2
  exit 1
}

# `plane` is a package in the oracle's apps/api; put it on the path instead of
# changing directory, so relative paths in the caller's arguments keep working.
export PYTHONPATH="$PLANE_API_DIR${PYTHONPATH:+:$PYTHONPATH}"

exec "$PLANE_DJANGO_PYTHON" "$@"
