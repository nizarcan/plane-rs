#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The "the published API docs are still accurate" check (PUBLIC_INSTANCES §4).

Three of §4's four checks are mechanical and live here; the fourth (the v1
contract suite) is `tools/contract/run.sh`.

1. **The goldens are current.** Re-render through Django and compare with
   `deploy/openapi/` byte for byte. This is what `capture.py --check` does; it is
   called here so CI has one entry point.
2. **The document describes what the Rust router serves.** Parse the committed
   YAML's path+method set and compare it with the v1 route table in
   `routes.json`. The OpenAPI preprocessing hook filters the Django resolver
   (`plane/utils/openapi/hooks.py:14-24`), so the comparison must apply the same
   filter from the Rust side:

   * only group `v1` (paths under `/api/v1/`);
   * no `PUT` (`method.upper() != "PUT"`);
   * no path containing `server` (case-insensitive);
   * the two DRF router api-root views are not in the document
     (`rest_framework.routers.APIRootView`; drf-spectacular skips them);
   * `.{format}`-suffix routes collapse onto their base path, which is itself
     in `routes.json`, so the suffixed rows are dropped.
3. The `Content-Disposition`/`Content-Type` header contract is recorded in
   `deploy/openapi/manifest.json` by `capture.py` and served from Rust; the
   differential scenario `scenarios/read/openapi.yaml` compares the wire.

Run from the repository root (or `apps/api`):

    apps/api/.venv/bin/python apps/server/tools/openapi/check.py --check
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve()
SERVER_ROOT = HERE.parents[2]  # apps/server
REPO_ROOT = HERE.parents[4]  # repository root


def load_capture():
    spec = importlib.util.spec_from_file_location("openapi_capture", HERE.with_name("capture.py"))
    if spec is None or spec.loader is None:  # pragma: no cover - safety net
        raise SystemExit("cannot load tools/openapi/capture.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def golden_failures() -> list[str]:
    """Check 1: the committed artefacts equal what Django produces now."""
    capture = load_capture()
    bodies, manifest = capture.render()
    bodies["manifest.json"] = capture.manifest_bytes(manifest)
    failures = []
    for name, body in bodies.items():
        path = capture.OUT_DIR / name
        existing = path.read_bytes() if path.exists() else None
        if existing != body:
            failures.append(
                f"deploy/openapi/{name} is stale: committed "
                f"{len(existing) if existing is not None else 0} bytes, Django now "
                f"produces {len(body)}"
            )
    return failures


def documented_pairs(document: dict) -> set[tuple[str, str]]:
    pairs = set()
    for path, operations in document["paths"].items():
        for method in operations:
            pairs.add((path, method.upper()))
    return pairs


def served_pairs(routes: dict) -> set[tuple[str, str]]:
    """The v1 route table after the OpenAPI preprocessing hook's filters."""
    pairs = set()
    for route in routes["routes"]:
        if route["group"] != "v1":
            continue
        if route["view"] == "rest_framework.routers.APIRootView":
            continue
        if "server" in route["path"].lower() or ".{format}" in route["path"]:
            continue
        for method in route["methods"]:
            if method.upper() == "PUT":
                continue
            pairs.add((route["path"], method.upper()))
    return pairs


def coverage_failures() -> list[str]:
    """Check 2: path+method set equality between the document and routes.json."""
    import yaml

    document = yaml.safe_load((SERVER_ROOT / "deploy" / "openapi" / "v1.yaml").read_text())
    routes = json.loads((SERVER_ROOT / "routes.json").read_text())
    documented = documented_pairs(document)
    served = served_pairs(routes)
    failures = []
    for path, method in sorted(documented - served):
        failures.append(f"{method} {path} is documented but not in the v1 route table")
    for path, method in sorted(served - documented):
        failures.append(f"{method} {path} is in the v1 route table but not documented")
    if not failures:
        print(f"ok    document path+method set: {len(documented)} pairs == routes.json v1")
    return failures


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="CI mode: verify the goldens and the route coverage (the default behaviour)",
    )
    parser.parse_args(argv)

    failures = golden_failures() + coverage_failures()
    if failures:
        for failure in failures:
            print(f"FAIL  {failure}", file=sys.stderr)
        print(
            "The published OpenAPI contract no longer matches the code. Regenerate the "
            "goldens with `apps/api/.venv/bin/python apps/server/tools/openapi/capture.py` "
            "and review the diff against RUST_REWRITE.md's Phase 7 exit condition.",
            file=sys.stderr,
        )
        return 1
    print("ok    OpenAPI goldens and route coverage")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
