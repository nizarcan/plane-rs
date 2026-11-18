#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# The environment Django needs to run against apps/server/dev/compose.yml.
#
# Usage:
#   source apps/server/dev/django-env.sh      # exports into the current shell
#   eval "$(apps/server/dev/django-env.sh)"   # same, in a subshell-friendly way
#   apps/server/dev/django-env.sh             # prints `export K=V` lines
#
# Inputs (environment variables, all optional):
#   PLANE_DB         Postgres database name                 (default: plane)
#   PLANE_REDIS_DB   Redis database index, 0-15             (default: 0)
#   PLANE_MQ_VHOST   RabbitMQ vhost                         (default: plane)
#
# SECRET_KEY comes from apps/server/dev/.env.local, which lib.sh generates on
# first use. It is stable on purpose: InstanceConfiguration rows in the
# template database are encrypted with it, so rotating the key requires
# `bootstrap.sh --force`.
#
# DJANGO_SETTINGS_MODULE matches what apps/api/Dockerfile.api serves and what
# apps/api/manage.py, plane/wsgi.py, plane/asgi.py and plane/celery.py default
# to: plane.settings.production. The stack runs in that mode (DEBUG=0) so the
# dev server behaves like the real one.
#
# When executed this fails fast. When sourced it deliberately leaves the
# caller's shell options alone: the caller is expected to have set
# `set -euo pipefail` itself.

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -euo pipefail
fi

# shellcheck source=lib.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

[[ "$PLANE_REDIS_DB" =~ ^([0-9]|1[0-5])$ ]] ||
  die "PLANE_REDIS_DB must be an integer 0-15 (valkey has 16 logical databases), got '$PLANE_REDIS_DB'"
[[ -n "$PLANE_DB" ]] || die "PLANE_DB must not be empty"

# Everything Django reads, in one place. Each entry is KEY=VALUE; the value is
# exported verbatim. Keep this list in sync with the table in dev/README.md.
_plane_env_pairs=(
  # Which settings module and mode. production.py is the module the container
  # entrypoint serves; DEBUG stays off so caching, cookie and error behaviour
  # match a real deployment.
  "DJANGO_SETTINGS_MODULE=plane.settings.production"
  "DEBUG=0"
  "IS_TEST=0"

  # Django's signing key: sessions, password-reset tokens, CSRF, and the
  # instance-configuration encryption key (plane/license/utils/encryption.py).
  "SECRET_KEY=$SECRET_KEY"

  # System of record. common.py parses DATABASE_URL with dj-database-url.
  "DATABASE_URL=postgresql://$PLANE_PG_USER:$PLANE_PG_PASSWORD@$PLANE_PG_HOST:$PLANE_PG_PORT/$PLANE_DB"
  # No replica in dev; explicitly off so a stale ENABLE_READ_REPLICA in the
  # caller's shell cannot install the read-replica router.
  "ENABLE_READ_REPLICA=0"

  # django-redis cache/rate-limit store. The path component selects the Redis
  # logical database, which is the per-agent isolation knob.
  "REDIS_URL=redis://$PLANE_VALKEY_HOST:$PLANE_VALKEY_PORT/$PLANE_REDIS_DB"

  # Celery broker (common.py builds CELERY_BROKER_URL from these). AMQP_URL is
  # unset below because, when present, it wins over all of them.
  "RABBITMQ_HOST=$PLANE_RABBITMQ_HOST"
  "RABBITMQ_PORT=$PLANE_RABBITMQ_PORT"
  "RABBITMQ_USER=$PLANE_RABBITMQ_USER"
  "RABBITMQ_PASSWORD=$PLANE_RABBITMQ_PASSWORD"
  "RABBITMQ_VHOST=$PLANE_MQ_VHOST"

  # Public URL of the dev proxy. Used for the S3 custom domain when MinIO is on
  # (common.py: AWS_S3_CUSTOM_DOMAIN) and for the Secure-cookie decision.
  "WEB_URL=http://127.0.0.1:18080"
  # Dev only: accept any Host header, as common.py's default does.
  "ALLOWED_HOSTS=*"

  # Object storage. USE_MINIO selects the path-style MinIO client in
  # plane/settings/storage.py; the endpoint and credentials are also what
  # `manage.py create_bucket` uses. MinIO ignores the region but SigV4 needs
  # one to sign with.
  "USE_MINIO=1"
  "MINIO_ENDPOINT_SSL=0"
  "AWS_ACCESS_KEY_ID=$PLANE_MINIO_ACCESS_KEY"
  "AWS_SECRET_ACCESS_KEY=$PLANE_MINIO_SECRET_KEY"
  "AWS_S3_BUCKET_NAME=$PLANE_MINIO_BUCKET"
  "AWS_S3_ENDPOINT_URL=http://$PLANE_MINIO_HOST:$PLANE_MINIO_PORT"
  "AWS_REGION=us-east-1"
  "SIGNED_URL_EXPIRATION=3600"

  # Read by bin/docker-entrypoint-api.sh; django-serve.sh honours it too.
  "GUNICORN_WORKERS=1"

  # Match the image: no .pyc files in the (tracked) apps/api tree, unbuffered
  # output so logs stream while serving.
  "PYTHONDONTWRITEBYTECODE=1"
  "PYTHONUNBUFFERED=1"

  # Off by default in common.py; pinned so a caller's shell cannot turn schema
  # generation on and slow every management command down. A scenario that
  # deliberately exercises the three drf-spectacular routes sets it in its
  # `target_env` (`scenarios/read/openapi.yaml`); sourcing this file must not
  # clobber that, or the Django target would 404 the routes the Rust target
  # serves.
  "ENABLE_DRF_SPECTACULAR=${ENABLE_DRF_SPECTACULAR:-0}"

  # Pass the isolation knobs through to child processes for readability.
  "PLANE_DB=$PLANE_DB"
  "PLANE_REDIS_DB=$PLANE_REDIS_DB"
  "PLANE_MQ_VHOST=$PLANE_MQ_VHOST"
)

# Variables that must not leak in from the caller's shell.
#   AMQP_URL        would override RABBITMQ_* and point Celery at another broker
#   SCOUT_MONITOR   production.py adds scout_apm; any non-empty string is truthy
#   SCOUT_KEY       ditto
_plane_env_unset=(
  "AMQP_URL"
  "SCOUT_MONITOR"
  "SCOUT_KEY"
)

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  # Executed: print shell-evaluable assignments for `eval "$(django-env.sh)"`.
  for _key in "${_plane_env_unset[@]}"; do
    printf 'unset %s\n' "$_key"
  done
  for _pair in "${_plane_env_pairs[@]}"; do
    _key="${_pair%%=*}"
    _value="${_pair#*=}"
    printf 'export %s=%q\n' "$_key" "$_value"
  done
else
  # Sourced: mutate the calling shell directly.
  for _key in "${_plane_env_unset[@]}"; do
    unset "$_key"
  done
  for _pair in "${_plane_env_pairs[@]}"; do
    export "$_pair"
  done
fi

unset _key _value _pair
