#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Create or delete a RabbitMQ vhost for an isolated agent, with the `plane`
# user granted full configure/write/read permissions.
#
#   apps/server/dev/mq-vhost.sh t_task_1
#   apps/server/dev/mq-vhost.sh --delete t_task_1
#
# Creation is idempotent: an existing vhost gets its permissions re-applied.
# Deletion is idempotent too: a missing vhost is not an error. The shared
# default vhost `plane` is protected unless --force is also given.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

usage() {
  cat <<'EOF'
usage: mq-vhost.sh [--delete] [--force] <vhost>

  <vhost>              create the vhost if needed and grant `plane` full access
  --delete             delete the vhost instead
  --force              allow deleting the shared `plane` vhost
EOF
}

delete=0
force=0
vhost=""
for arg in "$@"; do
  case "$arg" in
    --delete) delete=1 ;;
    --force) force=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      die "unknown option '$arg'"
      ;;
    *)
      [[ -z "$vhost" ]] || die "only one vhost may be given (got '$vhost' and '$arg')"
      vhost="$arg"
      ;;
  esac
done

[[ -n "$vhost" ]] || {
  usage >&2
  exit 2
}
[[ "$vhost" != *$'\n'* ]] || die "vhost must not contain a newline"

require_cmd docker
[[ "$(service_health rabbitmq)" == "healthy" ]] ||
  die "rabbitmq is not healthy; run apps/server/dev/up.sh first"

rabbitmqctl() {
  compose exec -T rabbitmq rabbitmqctl "$@"
}

vhost_exists() {
  # `-q` (--quiet) suppresses the `Listing vhosts ...` banner that rabbitmqctl
  # 3.13 writes to *stdout* even with `--no-table-headers`, so the listing is
  # exactly one bare vhost name per line; `--no-table-headers` drops the `name`
  # column header.
  #
  # The listing is captured first instead of piped straight into `grep -q`:
  # with `set -o pipefail`, `grep -q` exiting after the first match can leave
  # rabbitmqctl with a SIGPIPE (141), and a transient CLI failure would then be
  # indistinguishable from "the vhost does not exist" -- which makes creation
  # and deletion silently non-idempotent. Fail loudly instead.
  local listing
  listing="$(rabbitmqctl -q list_vhosts --no-table-headers)" ||
    die "rabbitmqctl list_vhosts failed; cannot tell whether vhost '$1' exists"
  grep -Fxq -- "$1" <<<"$listing"
}

if ((delete)); then
  if [[ "$vhost" == "plane" && "$force" -eq 0 ]]; then
    die "refusing to delete the shared default vhost 'plane' (use --force if you really mean it)"
  fi
  if vhost_exists "$vhost"; then
    rabbitmqctl delete_vhost "$vhost" >/dev/null
    note "deleted RabbitMQ vhost '$vhost'"
  else
    note "RabbitMQ vhost '$vhost' does not exist; nothing to delete"
  fi
  exit 0
fi

if vhost_exists "$vhost"; then
  note "RabbitMQ vhost '$vhost' already exists"
else
  rabbitmqctl add_vhost "$vhost" >/dev/null
  note "created RabbitMQ vhost '$vhost'"
fi

# .* / .* / .* = configure / write / read on every resource in the vhost.
rabbitmqctl set_permissions -p "$vhost" "$PLANE_RABBITMQ_USER" '.*' '.*' '.*' >/dev/null
note "granted user '$PLANE_RABBITMQ_USER' full permissions on '$vhost'"
