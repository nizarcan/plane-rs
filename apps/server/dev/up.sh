#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Start the dev stack and wait until every healthcheck passes.
#
# Idempotent: running it against an already healthy stack only re-ensures the
# RabbitMQ vhost and returns.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

require_cmd docker
docker compose version >/dev/null 2>&1 || die "docker compose v2 is required"

note "starting $PLANE_COMPOSE_PROJECT (waiting for healthchecks)"
compose up -d --wait --wait-timeout 240

# The broker creates RABBITMQ_DEFAULT_VHOST on first boot only. When the
# container is recreated without its state (no volume), that is enough; when it
# is reused, this makes the vhost exist anyway. The command is idempotent and
# also restores the `plane` permissions if the vhost already exists.
"$PLANE_DEV_DIR/mq-vhost.sh" "$PLANE_MQ_VHOST" >/dev/null

note "all services are healthy"
# Status goes to stderr so callers can use up.sh's stdout for their own output.
compose ps >&2
