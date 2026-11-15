#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# The local coexistence proxy: one Caddy container on 127.0.0.1:18080 that
# sends Rust-owned routes to plane-api on 18100 and everything else to the
# Django instance on 18000, the frontend dev servers and MinIO.
#
#   apps/server/dev/proxy.sh up       # generate the snippet, start the container
#   apps/server/dev/proxy.sh reload   # regenerate the snippet, reload Caddy
#   apps/server/dev/proxy.sh status   # container state, flags, live probe
#   apps/server/dev/proxy.sh down     # stop and remove the container
#
# The service lives in the compose "proxy" profile, so `up.sh` does not start
# it and `down.sh` does not stop it. Only this script manages it.
#
# The Caddy container mounts apps/server read-only at /srv/plane and runs
# /srv/plane/dev/Caddyfile.dev, which imports the generated snippet from
# dev/run/rust-routes.caddy (gitignored). `reload` rewrites that copy and asks
# Caddy to reload its configuration in place; the container keeps running.
#
# The dev proxy never writes the committed deploy/caddy/rust-routes.caddy: that
# file changes only when someone runs gen_caddy.py deliberately (or CI's
# --check). This keeps PLANE_RS_OWNERS_FILE / PLANE_RS_GROUPS_FILE experiments
# out of `git status`.
#
# Flags. PLANE_RS_GROUP_<NAME> is read by Caddy per request from the container
# environment (see the generated file), so a flag change means recreating the
# container: export the variable and run `proxy.sh up`. `up` copies every
# exported PLANE_RS_GROUP_* variable plus PLANE_RS_API_UPSTREAM into the
# gitignored dev/run/proxy.env, which compose passes to the container. Unset
# flags mean `rust` for groups that have Rust-owned routes, and any value other
# than `rust` sends that group back to Django.
#
# Local overrides (testing only, never committed):
#   PLANE_RS_OWNERS_FILE=/tmp/ROUTES.md   parse Owner cells from another file
#   PLANE_RS_GROUPS_FILE=/tmp/groups.yaml use another route-groups.yaml
#   PLANE_RS_API_UPSTREAM=host:port       Rust upstream (default the dev port)
#
# The Django instance must already be running on 18000 (django-serve.sh) and
# the Rust server on 18100 (bins/plane-api) for the proxied requests to
# succeed; `up` and `status` only prove that Caddy itself is serving.

set -euo pipefail

PLANE_DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "$PLANE_DEV_DIR/lib.sh"

PROXY_SERVICE="proxy"
PROXY_CONTAINER="plane-rs-dev-proxy"
PROXY_PORT=18080
PROXY_CONFIG_IN_CONTAINER="/srv/plane/dev/Caddyfile.dev"
PROXY_ENV_FILE="$PLANE_DEV_RUN_DIR/proxy.env"
# The snippet Caddy imports. It lives under the gitignored dev/run/ so an
# experiment here never touches the committed deploy/caddy/rust-routes.caddy.
PROXY_SNIPPET="$PLANE_DEV_RUN_DIR/rust-routes.caddy"
DEFAULT_RS_UPSTREAM="host.docker.internal:18100"

usage() {
  cat <<'EOF'
usage: proxy.sh <up|down|reload|status>

  up       generate dev/run/rust-routes.caddy, then create/start the
           plane-rs-dev-proxy container with the flags exported right now
  reload   regenerate the snippet and reload Caddy without restarting it
  status   print container state, the flags it was started with, and a probe
  down     stop and remove the container (the generated snippet is kept)
EOF
}

generate_snippet() {
  [[ -x "$PLANE_DJANGO_PYTHON" ]] ||
    die "python not found at $PLANE_DJANGO_PYTHON (the orchestrator creates apps/api/.venv)"
  local args=(--out "$PROXY_SNIPPET")
  if [[ -n "${PLANE_RS_OWNERS_FILE:-}" ]]; then
    args+=(--owners-file "$PLANE_RS_OWNERS_FILE")
  fi
  if [[ -n "${PLANE_RS_GROUPS_FILE:-}" ]]; then
    args+=(--groups "$PLANE_RS_GROUPS_FILE")
  fi
  "$PLANE_DJANGO_PYTHON" "$PLANE_SERVER_DIR/tools/proxy/gen_caddy.py" "${args[@]}"
}

write_env_file() {
  mkdir -p "$PLANE_DEV_RUN_DIR"
  (
    umask 077
    {
      printf 'PLANE_RS_API_UPSTREAM=%s\n' "${PLANE_RS_API_UPSTREAM:-$DEFAULT_RS_UPSTREAM}"
      # Every PLANE_RS_GROUP_* exported in the caller's shell, sorted so the
      # file is deterministic and an unchanged set does not recreate the
      # container. `env` output is one KEY=VALUE per line.
      env | grep -E '^PLANE_RS_GROUP_[A-Z0-9_]+=' | sort || true
    } >"$PROXY_ENV_FILE"
  )
}

container_running() {
  [[ "$(docker inspect --format '{{.State.Running}}' "$PROXY_CONTAINER" 2>/dev/null || true)" == "true" ]]
}

# Any HTTP status (including a 502 from a down frontend) proves Caddy is
# listening and routing.
probe() {
  local code=""
  for _ in $(seq 1 30); do
    code="$(curl -s -o /dev/null --max-time 1 -w '%{http_code}' "http://127.0.0.1:$PROXY_PORT/" || true)"
    if [[ "$code" =~ ^[1-5][0-9][0-9]$ ]]; then
      printf '%s\n' "$code"
      return 0
    fi
    sleep 0.2
  done
  return 1
}

cmd_up() {
  generate_snippet
  write_env_file
  note "starting $PROXY_CONTAINER on 127.0.0.1:$PROXY_PORT"
  compose --profile "$PROXY_SERVICE" up -d --wait "$PROXY_SERVICE"
  local code
  if code="$(probe)"; then
    note "proxy is serving (GET / -> $code)"
  else
    die "proxy container started but $PROXY_PORT did not answer; see: docker logs $PROXY_CONTAINER"
  fi
  print_flags
}

cmd_down() {
  note "removing $PROXY_CONTAINER"
  compose --profile "$PROXY_SERVICE" rm --stop --force "$PROXY_SERVICE"
}

cmd_reload() {
  generate_snippet
  container_running || die "$PROXY_CONTAINER is not running; run: $(basename "$0") up"
  note "reloading Caddy from $PROXY_CONFIG_IN_CONTAINER"
  docker exec "$PROXY_CONTAINER" \
    caddy reload --config "$PROXY_CONFIG_IN_CONTAINER" --adapter caddyfile
  local code
  if code="$(probe)"; then
    note "reload done (GET / -> $code)"
  else
    die "reload finished but $PROXY_PORT did not answer; check the container log"
  fi
}

print_flags() {
  local upstream flags
  upstream="$(docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' \
    "$PROXY_CONTAINER" 2>/dev/null | sed -n 's/^PLANE_RS_API_UPSTREAM=//p')"
  flags="$(docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' \
    "$PROXY_CONTAINER" 2>/dev/null | sed -n 's/^PLANE_RS_GROUP_//p' | sort)"
  printf 'upstream: %s\n' "${upstream:-unknown}"
  if [[ -n "$flags" ]]; then
    printf 'flags (read per request; unset group = rust):\n'
    while IFS= read -r flag; do
      printf '  %s\n' "$flag"
    done <<<"$flags"
  else
    printf 'flags: none set (every group with Rust-owned routes is on Rust)\n'
  fi
}

cmd_status() {
  compose --profile "$PROXY_SERVICE" ps "$PROXY_SERVICE" || true
  if container_running; then
    local code
    code="$(probe || true)"
    printf 'container: running\n'
    printf 'probe:     GET http://127.0.0.1:%s/ -> %s\n' "$PROXY_PORT" "${code:-no response}"
    print_flags
  else
    printf 'container: not running (start it with: %s up)\n' "$(basename "$0")"
  fi
}

command="${1:-}"
case "$command" in
  up) cmd_up ;;
  down) cmd_down ;;
  reload) cmd_reload ;;
  status) cmd_status ;;
  -h | --help | help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
