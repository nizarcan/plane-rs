#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Select the contract test files that reach a Rust-owned route, without running them.

A dry collection/routing pass: parse every `test_*_app.py` module under
`plane/tests/contract`, recover the URL template of each test-client call from
the AST (`client.get(f"/api/workspaces/{slug}/projects/{pk}/")`, a module-level
`URL = "/api/…"` constant, `.format()` on one, …), generate concrete sample
paths for the placeholders, and match them against the proxy plan the runner
and `gen_caddy.py` share. A file with at least one match is a file the mixed
run must cover; everything else cannot reach Rust and is out of scope.

The method is part of the match (`client.post(URL)` only selects a file when a
cluster owns POST for that path), so a file that only mutates a Rust-owned path
is not selected for a GET-only port.

Usage::

    apps/api/.venv/bin/python apps/server/tools/contract/dry_routing.py
    apps/api/.venv/bin/python apps/server/tools/contract/dry_routing.py --quiet
    apps/api/.venv/bin/python apps/server/tools/contract/dry_routing.py --dir api

Exit code 0 always; the selection is printed one file per line (stderr carries
the per-file detail), so it can be piped into the runner.
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
SERVER_DIR = TOOLS_DIR.parents[1]
REPO_ROOT = SERVER_DIR.parents[1]

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()
CONTRACT_DIR = API_DIR / "plane" / "tests" / "contract"
PROXY_DIR = SERVER_DIR / "tools" / "proxy"

METHODS = ("get", "post", "put", "patch", "delete", "head", "options")
UUID_SAMPLE = "01234567-89ab-cdef-0123-456789abcdef"
STR_SAMPLE = "sample"
INT_SAMPLE = "7"


def load_plan() -> list[tuple[str, frozenset[str], re.Pattern[str]]]:
    if str(PROXY_DIR) not in sys.path:
        sys.path.insert(0, str(PROXY_DIR))
    import gen_caddy

    routes_module = gen_caddy.load_inventory_module()
    _inventory, routes = gen_caddy.load_inventory(gen_caddy.DEFAULT_ROUTES_JSON)
    owners = gen_caddy.load_owners(gen_caddy.DEFAULT_OWNERS_FILE)
    sys.path.insert(0, str(TOOLS_DIR))
    import owner_override

    orders = owner_override.assume_rust_orders()
    if orders:
        for warning in owner_override.override_owners(routes, owners, orders):
            print(f"dry-routing: {warning}", file=sys.stderr)
    groups = gen_caddy.load_groups(gen_caddy.DEFAULT_GROUPS_FILE, routes)
    plans = gen_caddy.build_plan(routes, owners, groups, routes_module)
    plan = []
    for group_plan in plans:
        for cluster in group_plan.clusters:
            for regex in cluster.regexes:
                plan.append((group_plan.group.name, frozenset(cluster.methods), re.compile(regex)))
    return owner_override.filter_methods(plan, owner_override.methods_filter())


class Resolver:
    """Resolve URL templates through module constants and local assignments.

    Contract tests often build the URL in a helper (`_url(...)`) and pass the
    local variable to `client.get`; resolving names keeps the dry pass from
    missing those files. A name with several assignments yields every template,
    which can only over-select.
    """

    def __init__(self, path: Path) -> None:
        self.assignments: dict[str, list[ast.AST]] = {}
        self.returns: dict[str, list[ast.AST]] = {}
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.Assign):
                for target in node.targets:
                    for name in _target_names(target):
                        self.assignments.setdefault(name, []).append(node.value)
            elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                self.returns.setdefault(node.name, []).extend(
                    statement.value
                    for statement in ast.walk(node)
                    if isinstance(statement, ast.Return) and statement.value is not None
                )

    def templates(self, node: ast.AST, depth: int = 0) -> list[str]:
        if depth > 4:
            return []
        literal = self._literal(node)
        if literal is not None:
            return [literal]
        if isinstance(node, ast.Name):
            return [
                template
                for value in self.assignments.get(node.id, [])
                for template in self.templates(value, depth + 1)
            ]
        if isinstance(node, ast.Attribute):
            return [
                template
                for value in self.assignments.get(f"self.{node.attr}", [])
                for template in self.templates(value, depth + 1)
            ]
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            left = self.templates(node.left, depth + 1)
            right = self.templates(node.right, depth + 1)
            if not left and not right:
                return []
            if not left:
                return right
            head = left[0].rstrip("/")
            return [head + "/{}"]
        if isinstance(node, ast.Call):
            # `URL.format(...)`, `self._url(...)`, `_url(...)`: the value is the
            # receiver's template or the helper's `return` expression.
            if isinstance(node.func, ast.Attribute):
                if node.func.attr == "format":
                    return self.templates(node.func.value, depth + 1)
                helper = self.returns.get(node.func.attr)
            elif isinstance(node.func, ast.Name):
                helper = self.returns.get(node.func.id)
            else:
                helper = None
            if helper:
                return [
                    template
                    for value in helper
                    for template in self.templates(value, depth + 1)
                ]
            if node.args:
                return self.templates(node.args[0], depth + 1)
        return []

    def _literal(self, node: ast.AST) -> str | None:
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            value = node.value
            if not value.startswith("/api"):
                return None
            # `.format(...)` placeholders and already-braced templates both
            # become `{}`: only the shape matters for the match.
            return re.sub(r"\{[^}]*\}", "{}", value)
        if isinstance(node, ast.JoinedStr):
            parts: list[str] = []
            for value in node.values:
                if isinstance(value, ast.Constant) and isinstance(value.value, str):
                    parts.append(value.value)
                else:
                    parts.append("{}")
            text = "".join(parts)
            return re.sub(r"\{[^}]*\}", "{}", text) if text.startswith("/api") else None
        return None


def _target_names(target: ast.AST) -> list[str]:
    if isinstance(target, ast.Name):
        return [target.id]
    if isinstance(target, ast.Attribute):
        return [f"self.{target.attr}"]
    if isinstance(target, (ast.Tuple, ast.List)):
        names: list[str] = []
        for element in target.elts:
            names.extend(_target_names(element))
        return names
    return []


def _samples(template: str) -> list[str]:
    placeholders = template.count("{}")
    if placeholders == 0:
        return [template]
    values = [UUID_SAMPLE, STR_SAMPLE, INT_SAMPLE]
    samples = [
        template.replace("{}", UUID_SAMPLE),
        template.replace("{}", STR_SAMPLE),
        template.replace("{}", INT_SAMPLE),
    ]
    if placeholders <= 4:
        for index in range(placeholders):
            parts = template.split("{}")
            rebuilt = parts[0]
            for position, part in enumerate(parts[1:]):
                rebuilt += (values[(index + 1) % len(values)] if position == index else STR_SAMPLE) + part
            samples.append(rebuilt)
    return samples


def calls_in(path: Path) -> list[tuple[str, str, int]]:
    """`(method, template, line)` for every test-client call with a URL."""
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    calls: list[tuple[str, str, int]] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Attribute):
            continue
        method = node.func.attr
        if method not in METHODS or not node.args:
            continue
        template = _template(node.args[0])
        if template is not None:
            calls.append((method.upper(), template, node.lineno))
    return calls


def file_matches(path: Path, plan: list[tuple[str, frozenset[str], re.Pattern[str]]]) -> list[tuple[str, str, str]]:
    """`(method, template, group)` for every call that a plan cluster owns."""
    matches: list[tuple[str, str, str]] = []
    resolver = Resolver(path)
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    seen: set[tuple[str, str]] = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Attribute):
            continue
        method = node.func.attr
        if method not in METHODS or not node.args:
            continue
        for template in resolver.templates(node.args[0]):
            if (method.upper(), template) in seen:
                continue
            seen.add((method.upper(), template))
            matched = False
            for url in _samples(template):
                if matched:
                    break
                for group, methods, pattern in plan:
                    if method.upper() in methods and pattern.fullmatch(url):
                        matches.append((method.upper(), template, group))
                        matched = True
                        break
    return matches


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--dir",
        default="app",
        choices=("app", "api", "all"),
        help="which contract subdirectory to scan (default: app)",
    )
    parser.add_argument("--quiet", action="store_true", help="print only the selected paths")
    args = parser.parse_args(argv)

    plan = load_plan()
    subdirs = ("app", "api") if args.dir == "all" else (args.dir,)
    selected: list[Path] = []
    for subdir in subdirs:
        for path in sorted((CONTRACT_DIR / subdir).glob("test_*.py")):
            matches = file_matches(path, plan)
            if matches:
                selected.append(path)
                if not args.quiet:
                    detail = ", ".join(f"{method} {template} -> {group}" for method, template, group in matches[:4])
                    print(f"# {path.relative_to(CONTRACT_DIR)}: {len(matches)} rust-owned call(s): {detail}", file=sys.stderr)
    for path in selected:
        print(path.relative_to(API_DIR))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
