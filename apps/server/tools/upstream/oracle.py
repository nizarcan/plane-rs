#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Locate (and describe) the Django oracle at the pinned upstream commit.

`apps/api` is no longer part of this repository. Every parity proof -- the
differential harness, the inventory and corpus generators, the schema and boot
checks, the contract suite -- runs against *the oracle*: `apps/api` exported
from the upstream commit recorded in `UPSTREAM.toml` and materialised by
`tools/upstream/oracle.sh`.

Layout of the cache (under the home directory on purpose: the Docker VM only
mounts `$HOME`):

    ${PLANE_RS_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/plane-rs}/
        git/plane.git/                 bare mirror used to export commits
        oracle/<commit>/
            .oracle-stamp              commit, tree object, uv, creation time
            apps/api/                  the upstream tree at the pin
                .venv/                 uv environment built from uv.lock
                manage.py
                plane/

`PLANE_RS_ORACLE_DIR` overrides the `<cache>/oracle/<commit>` directory
exactly; the directory still has to contain `apps/api/`, so a citation such as
`apps/api/plane/app/views/issue/base.py` resolves inside it. `PLANE_RS_CACHE_DIR`
overrides only the cache root (the bare mirror and the default oracle location).

Python callers use the functions below; shell callers use the CLI:

    python3 apps/server/tools/upstream/oracle.py api-dir
    python3 apps/server/tools/upstream/oracle.py python
    python3 apps/server/tools/upstream/oracle.py check     # exit 1 if stale
    eval "$(python3 apps/server/tools/upstream/oracle.py shell-env)"

The module imports nothing outside the standard library and works with the
host's Python 3.11+ (it needs `tomllib`); the Django-side scripts themselves
must still run under the oracle's own interpreter, `<api-dir>/.venv/bin/python`.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve()
SERVER_DIR = HERE.parents[2]  # apps/server
REPO_ROOT = HERE.parents[4]  # repository root

UPSTREAM_FILE = REPO_ROOT / "UPSTREAM.toml"
STAMP_NAME = ".oracle-stamp"


def upstream() -> dict:
    """The parsed `UPSTREAM.toml`."""
    with UPSTREAM_FILE.open("rb") as handle:
        return tomllib.load(handle)


def pinned_commit() -> str:
    """The commit `UPSTREAM.toml` records (never overridden)."""
    return upstream()["upstream"]["commit"]


def upstream_commit() -> str:
    """The commit whose oracle is wanted.

    `PLANE_RS_UPSTREAM_COMMIT` selects another commit for one process, which is
    how `tools/upstream/diff.py` materialises the *target* oracle while the
    repository still pins the old one. Everything else reads the file.
    """
    return os.environ.get("PLANE_RS_UPSTREAM_COMMIT") or pinned_commit()


def is_pinned() -> bool:
    return upstream_commit() == pinned_commit()


def upstream_repository() -> str:
    return upstream()["upstream"]["repository"]


def upstream_subdir() -> str:
    return upstream()["oracle"]["subdir"]


def cache_root() -> Path:
    """Where the bare mirror and the default oracle tree live."""
    explicit = os.environ.get("PLANE_RS_CACHE_DIR")
    if explicit:
        return Path(explicit).expanduser()
    xdg = os.environ.get("XDG_CACHE_HOME")
    base = Path(xdg).expanduser() if xdg else Path.home() / ".cache"
    return base / "plane-rs"


def oracle_root() -> Path:
    """The materialised upstream tree root (contains `apps/api`)."""
    explicit = os.environ.get("PLANE_RS_ORACLE_DIR")
    if explicit:
        return Path(explicit).expanduser()
    return cache_root() / "oracle" / upstream_commit()


def api_dir() -> Path:
    """`apps/api` inside the oracle: what used to be `apps/api` in this tree."""
    return oracle_root() / upstream_subdir()


def python() -> Path:
    """The oracle's virtualenv interpreter (Django and the pinned libraries)."""
    return api_dir() / ".venv" / "bin" / "python"


def manage_py() -> Path:
    return api_dir() / upstream()["oracle"]["manage"]


def stamp_path() -> Path:
    return oracle_root() / STAMP_NAME


def production_spec(name: str) -> str:
    """The pinned requirement for `name` in the oracle's production group.

    The oracle venv is synced with the default (dev, test) groups, which is
    everything the corpus generators and pytest need; gunicorn lives in the
    production group only, and `dev/django-serve.sh` needs it. Read the exact
    pin from the oracle's own pyproject instead of repeating a version here.
    """
    pyproject = api_dir() / "pyproject.toml"
    if not pyproject.is_file():
        raise SystemExit(f"{pyproject} is missing; run oracle.sh fetch")
    with pyproject.open("rb") as handle:
        data = tomllib.load(handle)
    for spec in data.get("dependency-groups", {}).get("production", []):
        if spec.lower().startswith(name.lower()):
            return spec
    raise SystemExit(f"the oracle's production group does not pin {name}")


def display_path(path: Path) -> str:
    """Render a path inside the oracle as it is cited in this repository.

    `.../oracle/<commit>/apps/api/plane/x.py` becomes `apps/api/plane/x.py`, so
    generated inventories keep the same file references as before the oracle
    moved out of the tree.
    """
    path = Path(path)
    try:
        return path.resolve().relative_to(oracle_root().resolve()).as_posix()
    except ValueError:
        pass
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return str(path)


def read_stamp() -> dict[str, str]:
    """The stamp's key=value lines, or an empty dict when there is none."""
    try:
        text = stamp_path().read_text(encoding="utf-8")
    except OSError:
        return {}
    stamp: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        stamp[key.strip()] = value.strip()
    return stamp


def problems(check_venv: bool = True) -> list[str]:
    """Everything that makes the cached oracle not the pinned one."""
    found: list[str] = []
    expected = upstream_commit()
    root = oracle_root()
    if not root.is_dir():
        found.append(f"no oracle at {root}")
        return found
    stamp = read_stamp()
    if not stamp:
        found.append(f"{root}/{STAMP_NAME} is missing")
    else:
        if stamp.get("commit") != expected:
            expected_name = "PLANE_RS_UPSTREAM_COMMIT" if not is_pinned() else "UPSTREAM.toml"
            found.append(
                f"{STAMP_NAME} says commit {stamp.get('commit', '?')}, "
                f"{expected_name} asks for {expected}"
            )
    for required in (api_dir() / "manage.py", api_dir() / "pyproject.toml", api_dir() / "uv.lock"):
        if not required.is_file():
            found.append(f"{required} is missing")
    if check_venv:
        interpreter = python()
        if not interpreter.is_file():
            found.append(f"{interpreter} is missing (run oracle.sh fetch)")
        else:
            # A target commit may legitimately pin another Django; its oracle
            # is still useful for a diff, so only the pin is version-checked,
            # but both must actually import Django.
            wanted = upstream()["upstream"]["django"]
            try:
                result = subprocess.run(
                    [str(interpreter), "-c", "import django; print(django.get_version())"],
                    capture_output=True,
                    text=True,
                    check=True,
                )
            except (OSError, subprocess.CalledProcessError) as error:
                found.append(f"{interpreter} cannot import Django: {error}")
            else:
                actual = result.stdout.strip()
                if is_pinned() and actual != wanted:
                    found.append(
                        f"the oracle venv has Django {actual}, UPSTREAM.toml pins {wanted}"
                    )
    return found


def require(check_venv: bool = True) -> Path:
    """Return `api_dir()`, or exit with the exact command that fixes it."""
    found = problems(check_venv=check_venv)
    if found:
        detail = "\n  ".join(found)
        raise SystemExit(
            f"the Django oracle for {upstream_commit()} is not usable:\n  {detail}\n"
            "run: apps/server/tools/upstream/oracle.sh fetch"
        )
    return api_dir()


def _print_env() -> None:
    values = {
        "PLANE_RS_ORACLE_ROOT": oracle_root(),
        "PLANE_RS_API_DIR": api_dir(),
        "PLANE_RS_DJANGO_PYTHON": python(),
        "PLANE_RS_ORACLE_COMMIT": upstream_commit(),
        "PLANE_RS_ORACLE_READY": "1" if not problems(check_venv=False) else "0",
    }
    for key, value in values.items():
        print(f"{key}={shlex.quote(str(value))}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("root", help="print the oracle root (contains apps/api)")
    sub.add_parser("api-dir", help="print the materialised apps/api")
    sub.add_parser("python", help="print the oracle virtualenv interpreter")
    sub.add_parser("commit", help="print the requested upstream commit")
    sub.add_parser("pinned-commit", help="print the commit UPSTREAM.toml records")
    sub.add_parser("describe", help="print the pinned git describe")
    sub.add_parser("meta", help="print key=value pins for shell callers")
    sub.add_parser("shell-env", help="print shell assignments for eval")
    check = sub.add_parser("check", help="verify the cache matches the pin")
    check.add_argument(
        "--no-venv",
        action="store_true",
        help="skip the virtualenv/Django version check (locate only)",
    )
    sub.add_parser("require", help="print apps/api or exit non-zero")
    spec = sub.add_parser(
        "production-spec", help="print a pinned production-group requirement"
    )
    spec.add_argument("name", help="package name, for example gunicorn")
    args = parser.parse_args(argv)

    if args.command == "root":
        print(oracle_root())
    elif args.command == "api-dir":
        print(api_dir())
    elif args.command == "python":
        print(python())
    elif args.command == "commit":
        print(upstream_commit())
    elif args.command == "pinned-commit":
        print(pinned_commit())
    elif args.command == "describe":
        print(upstream()["upstream"]["describe"])
    elif args.command == "meta":
        data = upstream()
        print(f"commit={data['upstream']['commit']}")
        print(f"describe={data['upstream']['describe']}")
        print(f"repository={data['upstream']['repository']}")
        print(f"subdir={data['oracle']['subdir']}")
        print(f"uv={data['upstream']['uv']}")
        print(f"django={data['upstream']['django']}")
        print(f"python={data['upstream']['python']}")
    elif args.command == "shell-env":
        _print_env()
    elif args.command == "check":
        found = problems(check_venv=not args.no_venv)
        if found:
            for problem in found:
                print(f"FAIL: {problem}", file=sys.stderr)
            print("run: apps/server/tools/upstream/oracle.sh fetch", file=sys.stderr)
            return 1
        label = upstream()["upstream"]["describe"] if is_pinned() else "target"
        print(f"ok: oracle at {oracle_root()} is {label} ({upstream_commit()})")
    elif args.command == "require":
        print(require())
    elif args.command == "production-spec":
        print(production_spec(args.name))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
