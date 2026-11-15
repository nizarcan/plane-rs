#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Show the state of the dev stack and verify that every service is healthy.
#
# Exits 0 when all four containers are healthy, 1 otherwise, so it can gate
# other scripts:
#
#   apps/server/dev/status.sh || apps/server/dev/up.sh

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

require_cmd docker

printf 'compose project: %s\n' "$PLANE_COMPOSE_PROJECT"
compose ps

services=(postgres valkey rabbitmq minio)
failed=0

printf '\n%-10s %-10s\n' "SERVICE" "HEALTH"
for service in "${services[@]}"; do
  health="$(service_health "$service")"
  printf '%-10s %-10s\n' "$service" "$health"
  [[ "$health" == "healthy" ]] || failed=1
done

if command -v psql >/dev/null 2>&1; then
  if ! psql_dev postgres -tAc 'SELECT 1' >/dev/null 2>&1; then
    printf '\npostgres: not accepting connections\n'
    failed=1
  fi
fi

printf '\nendpoints (127.0.0.1 only):\n'
printf '  postgres    postgresql://%s:%s@%s:%s/%s\n' \
  "$PLANE_PG_USER" "$PLANE_PG_PASSWORD" "$PLANE_PG_HOST" "$PLANE_PG_PORT" "$PLANE_DB"
printf '  valkey      redis://%s:%s/%s\n' "$PLANE_VALKEY_HOST" "$PLANE_VALKEY_PORT" "$PLANE_REDIS_DB"
printf '  rabbitmq    amqp://%s:***@%s:%s/%s  (management http://%s:%s)\n' \
  "$PLANE_RABBITMQ_USER" "$PLANE_RABBITMQ_HOST" "$PLANE_RABBITMQ_PORT" "$PLANE_MQ_VHOST" \
  "$PLANE_RABBITMQ_HOST" "$PLANE_RABBITMQ_MGMT_PORT"
printf '  minio       http://%s:%s  (console http://%s:%s, bucket %s)\n' \
  "$PLANE_MINIO_HOST" "$PLANE_MINIO_PORT" "$PLANE_MINIO_HOST" "$PLANE_MINIO_CONSOLE_PORT" "$PLANE_MINIO_BUCKET"

if ((failed)); then
  printf '\nstack is NOT healthy\n' >&2
  exit 1
fi

printf '\nstack is healthy\n'
