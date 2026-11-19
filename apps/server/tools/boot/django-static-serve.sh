#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Serve Django with a collected static root, for `scenarios/boot/static.yaml`.
#
# The dev stack never runs `collectstatic` (`dev/django-serve.sh` says so), so
# WhiteNoise has no `STATIC_ROOT` and every `/static/*` request 404s. This
# wrapper builds the collected tree under `${TMPDIR:-/tmp}/p8boot-static-root`
# and starts gunicorn with an ASGI shim that points Django's `STATIC_ROOT`
# there. Nothing under `apps/api` is written (the default `STATIC_ROOT` lives in
# the source tree and is not gitignored).
#
#   tools/boot/django-static-serve.sh <port>
#
# `tools/boot/static_files.py --check` keeps the Rust copies byte-identical.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER="$(cd "$HERE/../.." && pwd)"
REPO="$(cd "$SERVER/../.." && pwd)"
API="$REPO/apps/api"

port="${1:-}"
[[ -n "$port" ]] || { echo "usage: $(basename "$0") <port>" >&2; exit 2; }

TMP="${TMPDIR:-/tmp}"
SHIM_DIR="$TMP/p8boot-static-settings"
STATIC_ROOT="$TMP/p8boot-static-root"
mkdir -p "$SHIM_DIR"
export P8BOOT_STATIC_ROOT="$STATIC_ROOT"

# The shim patches `STATIC_ROOT` after `django.setup()` (so `plane/__init__.py`
# is not imported while Django's settings are still loading) and re-exports the
# real ASGI application. Gunicorn loads it by name.
cat >"$SHIM_DIR/p8boot_asgi.py" <<'PY'
import os

import django

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
django.setup()

from django.conf import settings  # noqa: E402

settings.STATIC_ROOT = os.environ["P8BOOT_STATIC_ROOT"]

from plane.asgi import application  # noqa: E402,F401
PY

set -a
# shellcheck disable=SC1090
. "$SERVER/dev/django-env.sh"
set +a
export PYTHONPATH="$SHIM_DIR${PYTHONPATH:+:$PYTHONPATH}"

cd "$API"
if [[ ! -f "$STATIC_ROOT/staticfiles.json" ]]; then
  "$API/.venv/bin/python" - <<'PY'
import os

import django

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
django.setup()

from django.conf import settings  # noqa: E402

settings.STATIC_ROOT = os.environ["P8BOOT_STATIC_ROOT"]
from django.core.management import call_command  # noqa: E402

call_command("collectstatic", interactive=False, verbosity=0)
PY
  # WhiteNoise reports the collected file's mtime; pin it to the instant the
  # embedded copies use (`plane-bootstrap/src/static_files.rs`), so
  # `Last-Modified` is byte-comparable.
  "$API/.venv/bin/python" - "$STATIC_ROOT" <<'PY'
import os
import sys

for root, _, files in os.walk(sys.argv[1]):
    for name in files:
        os.utime(os.path.join(root, name), (946684800, 946684800))
PY
fi

exec "$API/.venv/bin/gunicorn" \
  --workers "${GUNICORN_WORKERS:-1}" \
  --worker-class uvicorn.workers.UvicornWorker \
  --bind "127.0.0.1:$port" \
  --access-logfile - \
  p8boot_asgi:application
