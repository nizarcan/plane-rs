#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Keep the embedded `/static/` files byte-identical to Django's.

`plane-api` serves `/static/*` after cutover (`plans/CUTOVER.md` §3, §5); the
eight files Django collects from `apps/api/plane/static` (36 KiB, referenced by
`templates/emails/invitations/project_invitation.html:230-280`) are embedded
into `plane-bootstrap` with `include_bytes!`, because the distroless image ships
no files. The copies live under `crates/plane-bootstrap/assets/static/` so the
Docker build context needs no `apps/api` files beyond the one template
`csrf.rs` already embeds.

    apps/api/.venv/bin/python tools/boot/static_files.py --write
    apps/api/.venv/bin/python tools/boot/static_files.py --check
"""

import argparse
import filecmp
import shutil
import sys
from pathlib import Path

SERVER_DIR = Path(__file__).resolve().parent.parent.parent
REPO_ROOT = SERVER_DIR.parent.parent

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

SOURCE = _oracle_api_dir() / "plane" / "static"
TARGET = SERVER_DIR / "crates" / "plane-bootstrap" / "assets" / "static"


def source_files() -> list[Path]:
    if not SOURCE.is_dir():
        raise SystemExit(f"{SOURCE} does not exist")
    return sorted(path for path in SOURCE.rglob("*") if path.is_file())


def target_files() -> list[Path]:
    if not TARGET.is_dir():
        return []
    return sorted(path for path in TARGET.rglob("*") if path.is_file())


def relative(path: Path, root: Path) -> Path:
    return path.relative_to(root)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--write", action="store_true")
    group.add_argument("--check", action="store_true")
    args = parser.parse_args()

    sources = source_files()
    if args.write:
        if TARGET.exists():
            shutil.rmtree(TARGET)
        for path in sources:
            destination = TARGET / relative(path, SOURCE)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
        print(f"wrote {len(sources)} files to {TARGET}")
        return 0

    targets = target_files()
    expected = {relative(path, SOURCE) for path in sources}
    actual = {relative(path, TARGET) for path in targets}
    ok = True
    if expected != actual:
        ok = False
        for missing in sorted(expected - actual):
            print(f"FAIL: missing {missing}")
        for extra in sorted(actual - expected):
            print(f"FAIL: unexpected {extra}")
    for path in sources:
        copy = TARGET / relative(path, SOURCE)
        if copy.exists() and not filecmp.cmp(path, copy, shallow=False):
            ok = False
            print(f"FAIL: {relative(path, SOURCE)} differs from Django's copy")
    if not ok:
        print("Run tools/boot/static_files.py --write and commit the result.")
        return 1
    print(f"PASS: {len(sources)} static files are byte-identical to apps/api/plane/static")
    return 0


if __name__ == "__main__":
    sys.exit(main())
