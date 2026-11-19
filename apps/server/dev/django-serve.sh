#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Serve Django the way apps/api/bin/docker-entrypoint-api.sh does, but without
# the one-off setup steps (wait_for_db, register_instance, configure_instance,
# create_bucket, collectstatic) and with a single worker:
#
#   gunicorn -w 1 -k uvicorn.workers.UvicornWorker plane.asgi:application \
#     --bind 127.0.0.1:<port> --access-logfile -
#
# Runs in the foreground. Pick a port from the 18000-18099 range reserved for
# Django instances in apps/server/AGENTS.md, and a database that is yours:
#
#   PLANE_DB=t_task_1 apps/server/dev/django-serve.sh 18000
#
# Stop it with Ctrl-C (or kill the process group if it was backgrounded).

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"
# shellcheck source=django-env.sh
source "$PLANE_DEV_DIR/django-env.sh"

port="${1:-}"
if [[ -z "$port" ]]; then
  die "usage: $(basename "$0") <port>   (Django instances use 18000-18099)"
fi
shift || true
[[ $# -eq 0 ]] || die "unexpected arguments: $*"
[[ "$port" =~ ^[0-9]{1,5}$ ]] || die "port must be a number, got '$port'"
((port >= 1 && port <= 65535)) || die "port must be between 1 and 65535, got '$port'"

[[ -x "$PLANE_DJANGO_PYTHON" ]] ||
  die "Django oracle not ready at $PLANE_DJANGO_PYTHON; run apps/server/tools/upstream/oracle.sh fetch"
[[ -x "$PLANE_GUNICORN" ]] ||
  die "gunicorn not found at $PLANE_GUNICORN; re-run apps/server/tools/upstream/oracle.sh fetch --force"

cd "$PLANE_API_DIR"
note "serving plane.asgi:application on http://127.0.0.1:$port"
note "database=$PLANE_DB redis-db=$PLANE_REDIS_DB rabbitmq-vhost=$PLANE_MQ_VHOST settings=$DJANGO_SETTINGS_MODULE"

exec "$PLANE_GUNICORN" \
  --workers "${GUNICORN_WORKERS:-1}" \
  --worker-class uvicorn.workers.UvicornWorker \
  --bind "127.0.0.1:$port" \
  --access-logfile - \
  plane.asgi:application
