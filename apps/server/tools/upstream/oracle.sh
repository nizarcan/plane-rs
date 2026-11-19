#!/usr/bin/env bash
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
#
# Fetch and locate the Django oracle: `apps/api` exported from the upstream
# commit pinned in UPSTREAM.toml, with its virtualenv built from uv.lock.
#
#   apps/server/tools/upstream/oracle.sh fetch        # idempotent
#   apps/server/tools/upstream/oracle.sh fetch --force
#   apps/server/tools/upstream/oracle.sh check        # exit 1 when stale/missing
#   apps/server/tools/upstream/oracle.sh env          # shell assignments
#   apps/server/tools/upstream/oracle.sh api-dir|python|root|commit
#
# The cache lives under the home directory because the Docker VM only mounts
# `$HOME` (the harness starts Django natively and mounts the cache into the dev
# containers):
#
#   ${PLANE_RS_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/plane-rs}/
#     git/plane.git/                 bare mirror, fetched by commit sha
#     oracle/<commit>/               the materialised upstream tree
#       apps/api/                    exactly the path the citations use
#       .oracle-stamp
#
# `PLANE_RS_ORACLE_DIR` overrides `<cache>/oracle/<commit>` (it must still
# contain `apps/api/`); `PLANE_RS_CACHE_DIR` overrides the cache root.
#
# Network is used only by `git fetch` (the pinned commit and later refs for
# tools/upstream/diff.py) and by the pinned uv's package download for the
# virtualenv. A commit already present in this checkout is exported from the
# local object database without touching the network.
#
# Sourced, the file only defines functions (used by dev/lib.sh); run directly,
# it is the CLI. Every command is safe to run from any working directory.

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -euo pipefail
fi

ORACLE_TOOL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORACLE_SERVER_DIR="$(cd "$ORACLE_TOOL_DIR/../.." && pwd)"
ORACLE_REPO_ROOT="$(cd "$ORACLE_SERVER_DIR/../.." && pwd)"
ORACLE_PY="$ORACLE_TOOL_DIR/oracle.py"
ORACLE_HOST_PYTHON="${PLANE_RS_HOST_PYTHON:-python3}"

oracle_note() {
  printf '==> %s\n' "$*" >&2
}

oracle_die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

# oracle_meta <field>: commit, describe, repository, subdir, uv, django, python
oracle_meta() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" meta | sed -n "s/^$1=//p"
}

oracle_root() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" root
}

oracle_api_dir() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" api-dir
}

oracle_python() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" python
}

oracle_commit() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" commit
}

# Print the cache errors and the fix, without exiting.
oracle_problems() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" check "$@"
}

oracle_require() {
  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" require >/dev/null
}

# oracle_acquire_lock <directory>
#
# A mkdir lock with a stale-holder check, so two parallel CI jobs sharing the
# cache cannot extract into the same tree. mkdir is atomic on every filesystem
# we use and needs no flock (macOS has none in the base system).
oracle_acquire_lock() {
  local lock="$1.pid" waited=0 owner
  while ! mkdir "$lock" 2>/dev/null; do
    owner="$(cat "$lock/pid" 2>/dev/null || true)"
    if [[ -n "$owner" ]] && ! kill -0 "$owner" 2>/dev/null; then
      oracle_note "removing stale oracle lock from pid $owner"
      rm -rf "$lock"
      continue
    fi
    ((waited < 300)) || oracle_die "timed out waiting for the oracle lock $lock"
    sleep 2
    waited=$((waited + 2))
  done
  echo "$$" >"$lock/pid"
}

oracle_release_lock() {
  rm -rf "$1.pid"
}

# oracle_git_dir: the bare mirror used to export commits.
oracle_git_dir() {
  local cache
  cache="$("$ORACLE_HOST_PYTHON" "$ORACLE_PY" root)"
  # <cache>/oracle/<commit> -> <cache>/git/plane.git, unless the caller
  # overrode the oracle directory wholesale.
  if [[ -n "${PLANE_RS_ORACLE_DIR:-}" ]]; then
    cache="${PLANE_RS_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/plane-rs}"
  else
    cache="$(dirname "$(dirname "$cache")")"
  fi
  printf '%s/git/plane.git\n' "$cache"
}

# oracle_archive_source <commit>: print the git directory that has <commit>.
oracle_archive_source() {
  local commit="$1" git_dir
  if git -C "$ORACLE_REPO_ROOT" cat-file -e "$commit^{commit}" 2>/dev/null; then
    printf '%s\n' "$ORACLE_REPO_ROOT"
    return 0
  fi
  git_dir="$(oracle_git_dir)"
  if [[ -d "$git_dir" ]] && git -C "$git_dir" cat-file -e "$commit^{commit}" 2>/dev/null; then
    printf '%s\n' "$git_dir"
    return 0
  fi
  return 1
}

# oracle_fetch_commit <commit> [url]: make <commit> available in the mirror.
oracle_fetch_commit() {
  local commit="$1" url="${2:-}" git_dir
  git_dir="$(oracle_git_dir)"
  if [[ ! -d "$git_dir" ]]; then
    mkdir -p "$(dirname "$git_dir")"
    git init --bare -q "$git_dir"
  fi
  if git -C "$git_dir" cat-file -e "$commit^{commit}" 2>/dev/null; then
    return 0
  fi
  [[ -n "$url" ]] || url="$(oracle_meta repository)"
  oracle_note "fetching $commit from $url"
  git -C "$git_dir" fetch --no-tags --depth=1 "$url" "$commit"
}

oracle_export() {
  local commit="$1" source="$2" subdir="$3" destination="$4"
  mkdir -p "$destination"
  git -C "$source" archive --format=tar "$commit" -- "$subdir" |
    tar -xf - -C "$destination"
}

oracle_write_stamp() {
  local root="$1" commit="$2" source="$3" tree="$4" uv="$5" django="$6"
  {
    printf '# Generated by apps/server/tools/upstream/oracle.sh. Do not edit.\n'
    printf 'commit=%s\n' "$commit"
    printf 'source=%s\n' "$source"
    printf 'tree=%s\n' "$tree"
    printf 'uv=%s\n' "$uv"
    printf 'django=%s\n' "$django"
    printf 'created=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } >"$root/.oracle-stamp.tmp"
  mv "$root/.oracle-stamp.tmp" "$root/.oracle-stamp"
}

oracle_build_venv() {
  local api="$1" uv="$2" django="$3" gunicorn
  local uv_command=("uvx" "--from" "uv==$uv" "uv")
  command -v uvx >/dev/null 2>&1 ||
    oracle_die "uvx not found; install uv (https://docs.astral.sh/uv/) or set PLANE_RS_HOST_PYTHON"

  oracle_note "syncing the oracle venv with uv $uv (dev and test groups)"
  (cd "$api" && env -u UV_PROJECT_ENVIRONMENT "${uv_command[@]}" sync --locked)

  # gunicorn is only in the oracle's production group, which the dev venv
  # deliberately does not install (psycopg-c builds from source). django-serve.sh
  # needs it, so install the same pin, without dependencies.
  gunicorn="$("$ORACLE_HOST_PYTHON" "$ORACLE_PY" production-spec gunicorn)"
  oracle_note "installing $gunicorn into the oracle venv"
  "${uv_command[@]}" pip install --python "$api/.venv/bin/python" --no-deps "$gunicorn" >/dev/null

  local actual
  actual="$("$api/.venv/bin/python" -c 'import django; print(django.get_version())')"
  # A target commit (diff.py) may pin another Django; only the pinned tree is
  # required to match UPSTREAM.toml. The stamp records what was built.
  if [[ -n "$django" && "$actual" != "$django" ]]; then
    oracle_die "the oracle venv has Django $actual, UPSTREAM.toml pins $django"
  fi
  ORACLE_ACTUAL_DJANGO="$actual"
  [[ -x "$api/.venv/bin/gunicorn" ]] ||
    oracle_die "gunicorn is missing from the oracle venv at $api/.venv"
}

oracle_fetch() {
  local force=0
  while (($#)); do
    case "$1" in
      --force) force=1 ;;
      -h | --help)
        sed -n '6,44p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        return 0
        ;;
      *) oracle_die "unknown argument: $1" ;;
    esac
    shift
  done

  local commit pinned subdir uv django root api source tmp tree
  commit="$(oracle_commit)"
  pinned="$("$ORACLE_HOST_PYTHON" "$ORACLE_PY" pinned-commit)"
  subdir="$(oracle_meta subdir)"
  uv="$(oracle_meta uv)"
  django="$(oracle_meta django)"
  if [[ "$commit" != "$pinned" ]]; then
    # The target of a diff: its own lock decides the Django version.
    django=""
  fi
  root="$(oracle_root)"
  api="$root/$subdir"

  if ((force == 0)) && [[ -f "$root/.oracle-stamp" ]] &&
    grep -qx "commit=$commit" "$root/.oracle-stamp" &&
    [[ -f "$api/manage.py" && -x "$api/.venv/bin/python" ]]; then
    oracle_note "oracle already at $root ($(oracle_meta describe))"
    "$ORACLE_HOST_PYTHON" "$ORACLE_PY" check
    return 0
  fi

  mkdir -p "$(dirname "$root")"
  oracle_acquire_lock "$root"
  # shellcheck disable=SC2064
  trap "oracle_release_lock '$root'" RETURN

  if ((force == 0)) && [[ -f "$root/.oracle-stamp" ]] &&
    grep -qx "commit=$commit" "$root/.oracle-stamp" &&
    [[ -f "$api/manage.py" && -x "$api/.venv/bin/python" ]]; then
    oracle_note "oracle already at $root ($(oracle_meta describe))"
    oracle_release_lock "$root"
    trap - RETURN
    return 0
  fi

  if source="$(oracle_archive_source "$commit")"; then
    oracle_note "exporting $commit from $source"
  else
    oracle_fetch_commit "$commit"
    source="$(oracle_git_dir)"
    oracle_note "exporting $commit from $source"
  fi

  tmp="$(mktemp -d "$(dirname "$root")/.oracle-tmp.XXXXXX")"
  oracle_export "$commit" "$source" "$subdir" "$tmp"
  [[ -f "$tmp/$subdir/manage.py" ]] ||
    { rm -rf "$tmp"; oracle_die "$commit:$subdir has no manage.py"; }
  tree="$(git -C "$source" rev-parse "$commit:$subdir")"

  oracle_note "replacing $root"
  rm -rf "$root"
  mv "$tmp" "$root"
  api="$root/$subdir"

  # The uv that syncs the venv must be the one the extracted commit pins, not
  # a guess in UPSTREAM.toml; the file only names it so CI can show it.
  local pinned_uv
  pinned_uv="$(sed -n 's/^required-version *= *"==\{0,1\}\([^"]*\)".*/\1/p' "$api/pyproject.toml")"
  [[ -n "$pinned_uv" ]] ||
    oracle_die "$api/pyproject.toml has no [tool.uv] required-version"
  [[ "$pinned_uv" == "$uv" ]] ||
    oracle_die "$commit pins uv $pinned_uv, UPSTREAM.toml says $uv"

  ORACLE_ACTUAL_DJANGO=""
  oracle_build_venv "$api" "$uv" "$django"
  oracle_write_stamp "$root" "$commit" "$source" "$tree" "$uv" "$ORACLE_ACTUAL_DJANGO"

  "$ORACLE_HOST_PYTHON" "$ORACLE_PY" check
  oracle_release_lock "$root"
  trap - RETURN
}

# The CLI. Sourced, nothing below runs.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  command="${1:-}"
  shift || true
  case "$command" in
    fetch)
      oracle_fetch "$@"
      ;;
    check)
      oracle_problems "$@"
      ;;
    api-dir)
      oracle_api_dir
      ;;
    python)
      oracle_python
      ;;
    root)
      oracle_root
      ;;
    commit)
      oracle_commit
      ;;
    env)
      "$ORACLE_HOST_PYTHON" "$ORACLE_PY" shell-env
      ;;
    "" | -h | --help)
      sed -n '6,44p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      ;;
    *)
      oracle_die "unknown command: $command (try --help)"
      ;;
  esac
fi
