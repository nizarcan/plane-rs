#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run a Django management command against the dev stack.
#
#   apps/server/dev/django.sh migrate
#   PLANE_DB=t_task_1 apps/server/dev/django.sh showmigrations --plan
#
# The environment comes from django-env.sh. The current directory is switched
# to apps/api so `manage.py` and the `plane` package resolve.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"
# shellcheck source=django-env.sh
source "$PLANE_DEV_DIR/django-env.sh"

[[ $# -ge 1 ]] || die "usage: $(basename "$0") <manage.py args...>"
[[ -x "$PLANE_DJANGO_PYTHON" ]] ||
  die "Django virtualenv not found at $PLANE_DJANGO_PYTHON (the orchestrator creates it with uv)"

cd "$PLANE_API_DIR"
exec "$PLANE_DJANGO_PYTHON" manage.py "$@"
