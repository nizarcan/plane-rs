#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run a command with the Django environment of the dev stack pointed at the
# permissions fixture database.
#
#   apps/server/tools/permissions/run.sh apps/api/.venv/bin/python \
#       apps/server/tools/permissions/oracle.py
#
# PLANE_DB defaults to t_perms_fixture; set it to another t_* clone to work on
# a separate copy. Everything else (SECRET_KEY, DATABASE_URL, ...) comes from
# apps/server/dev/django-env.sh, which never points at a non-dev database.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
server_dir="$(cd "$here/../.." && pwd)"

export PLANE_DB="${PLANE_DB:-t_perms_fixture}"
# `plane` is a source-tree package, not an installed distribution: Django's
# settings and models import it from apps/api. manage.py works because it lives
# there; these scripts are run from the repository root, so say it explicitly.
export PYTHONPATH="$server_dir/../../apps/api${PYTHONPATH:+:$PYTHONPATH}"

# django-env.sh prints shell assignments on stdout and progress on stderr.
eval "$("$server_dir/dev/django-env.sh")"

exec "$@"
