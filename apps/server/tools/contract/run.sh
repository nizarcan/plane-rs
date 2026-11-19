#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Run the upstream contract suite (apps/api/plane/tests/contract) through the
# mixed-backend plugin in apps/server/tools/contract.
#
#   ./run.sh --backend django          # baseline: everything in-process
#   ./run.sh --backend mixed           # ROUTES.md owners decide per request
#   ./run.sh --backend mixed -- --create-db -x
#
# A domain agent whose write methods are not flipped in ROUTES.md yet can run
# them against Rust anyway. `--assume-rust` takes route orders (`routes.json`'s
# `order` column, also ROUTES.md's first column) and points their unsafe methods
# (`POST`/`PUT`/`PATCH`/`DELETE`) at `plane-api`; `--methods` narrows the plan
# to those verbs, so only requests that use them reach Rust:
#
#   ./run.sh --assume-rust 223,224 --methods POST,PATCH,DELETE
#
# Everything before `--` is the runner's own; everything after it goes to
# pytest unchanged. PLANE_RS_CONTRACT_* variables override the defaults below.
#
# The run uses the shared dev stack (Postgres 55432, Valkey 56379, RabbitMQ
# 55672/55673): database `t_rsct_*`, Redis DB 15, vhost `rsct*`. It never starts
# or stops a container; if a service is down, `dev/status.sh` says so.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
SERVER_DIR="$REPO_ROOT/apps/server"

backend="${PLANE_RS_CONTRACT_BACKEND:-mixed}"
build=1
smoke=0
assume_rust=""
methods=""
declare -a pytest_args=()

while (($#)); do
  case "$1" in
    --backend)
      backend="${2:?--backend needs django or mixed}"
      shift 2
      ;;
    --backend=*)
      backend="${1#*=}"
      shift
      ;;
    --assume-rust)
      assume_rust="${2:?--assume-rust needs route orders (e.g. 223,224)}"
      shift 2
      ;;
    --assume-rust=*)
      assume_rust="${1#*=}"
      shift
      ;;
    --methods)
      methods="${2:?--methods needs a comma-separated list (e.g. POST,PATCH)}"
      shift 2
      ;;
    --methods=*)
      methods="${1#*=}"
      shift
      ;;
    --smoke)
      smoke=1
      shift
      ;;
    -h | --help)
      sed -n '6,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    --)
      shift
      pytest_args+=("$@")
      break
      ;;
    *)
      pytest_args+=("$1")
      shift
      ;;
  esac
done

case "$backend" in
  django | mixed) ;;
  *)
    echo "error: --backend must be django or mixed (got '$backend')" >&2
    exit 2
    ;;
esac

# The dev environment (SECRET_KEY, DATABASE_URL, REDIS_URL, RABBITMQ_*,
# WEB_URL, AWS_*) is defined in one place; source it rather than duplicating it.
contract_db="${PLANE_RS_CONTRACT_DB:-t_rsct_contract}"
export PLANE_DB="$contract_db"
export PLANE_REDIS_DB="${PLANE_RS_CONTRACT_REDIS_DB:-15}"
# One vhost per contract database: separate DBs can run side by side without
# deleting each other's broker state. An explicit value still wins.
export PLANE_RS_CONTRACT_VHOST="${PLANE_RS_CONTRACT_VHOST:-$contract_db}"
export PLANE_MQ_VHOST="$PLANE_RS_CONTRACT_VHOST"
# shellcheck source=../../dev/django-env.sh
source "$SERVER_DIR/dev/django-env.sh"

# `/magic-generate/` refuses to work without an SMTP host: the provider raises
# SMTP_NOT_CONFIGURED when `EMAIL_HOST` is empty
# (plane/authentication/provider/credentials/magic_code.py:57-72), and the tests'
# empty `instance_configurations` table makes `get_configuration_value` fall
# back to the environment (plane/license/utils/instance_value.py:19-33). Without
# it the 15 magic-link tests in plane/tests/contract/app/test_authentication.py
# fail on a 400.
#
# This is exported here, not in dev/django-env.sh, because the differential
# harness uses that file too. No test can deliver real mail: the ones that reach
# the send path mock `magic_link.delay`, the test settings use the locmem email
# backend (plane/settings/test.py:12), and 127.0.0.1 has no SMTP listener.
export EMAIL_HOST=127.0.0.1

# The contract suite must run under the test settings (DEBUG, locmem email,
# plane.tests installed); django-env.sh sets the production module.
export DJANGO_SETTINGS_MODULE=plane.settings.test
export PLANE_RS_CONTRACT_BACKEND="$backend"
if [[ -n "$assume_rust" ]]; then
  export PLANE_RS_CONTRACT_ASSUME_RUST="$assume_rust"
fi
if [[ -n "$methods" ]]; then
  export PLANE_RS_CONTRACT_METHODS="$methods"
fi
export PYTHONPATH="$SERVER_DIR/tools/contract${PYTHONPATH:+:$PYTHONPATH}"

if [[ -n "$assume_rust" || -n "$methods" ]]; then
  printf 'contract: backend=%s assume-rust=%s methods=%s\n' \
    "$backend" "${assume_rust:-none}" "${methods:-all}" >&2
fi

if [[ "$backend" == "mixed" && "$build" == "1" ]]; then
  (cd "$SERVER_DIR" && cargo build -p plane-api -p plane-migrator)
fi

# The default is the whole contract directory; `--smoke` selects the plugin's
# own replay test instead; a path given on the command line replaces either
# (pytest would otherwise collect the directory *and* the file).
declare -a targets=("plane/tests/contract")
declare -a config_args=()
if [[ "$smoke" == "1" ]]; then
  targets=("$SERVER_DIR/tools/contract/smoke")
  config_args=(-c "$REPO_ROOT/apps/api/pytest.ini")
fi
for arg in "${pytest_args[@]:-}"; do
  case "$arg" in
    plane | plane/* | *.py | *::*)
      if [[ "$smoke" == "0" ]]; then
        targets=()
      fi
      break
      ;;
  esac
done

# Bash 3.2 (the macOS /bin/bash) expands an empty array under `set -u`
# differently from bash 5, so assemble the argv explicitly.
declare -a argv=()
if ((${#config_args[@]})); then
  argv+=("${config_args[@]}")
fi
if ((${#targets[@]})); then
  argv+=("${targets[@]}")
fi
argv+=(-p pytest_plane_contract)
if ((${#pytest_args[@]})); then
  argv+=("${pytest_args[@]}")
fi

cd "$REPO_ROOT/apps/api"
exec .venv/bin/python -m pytest "${argv[@]}"
