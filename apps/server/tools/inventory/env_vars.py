# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Inventory of every environment variable read by the Django backend.

This is a Phase 0 inventory tool for the Rust rewrite (see RUST_REWRITE.md). It
scans the Python source with an AST (never a regex, so commented-out reads and
string literals do not show up) plus the entrypoint shell scripts and the API
Dockerfile, and writes ``apps/server/tools/inventory/env_vars.json``.

What counts as a read:

  Python  ``os.environ.get("K"[, d])``   ``os.getenv("K"[, d])``
          ``os.environ.setdefault("K", d)``   ``environ.get("K"[, d])``
          ``os.environ["K"]``   ``environ["K"]``
          ``"K" in os.environ``   ``"K" not in os.environ``
  Shell   ``$K``   ``${K}``   ``${K:-d}``   ``${K-d}``   ``${K:=d}``
          ``${K:?msg}``   ``${K:+alt}``
  Docker  ``ENV K=v`` / ``ENV K v``   ``ARG K[=v]``

Excluded: ``plane/tests``, any ``migrations`` directory, ``__pycache__``.

Usage::

    python apps/server/tools/inventory/env_vars.py            # write the JSON
    python apps/server/tools/inventory/env_vars.py --print    # also print a table
    python apps/server/tools/inventory/env_vars.py --check-md apps/server/ENV.md

``--check-md`` exits non-zero when a variable in the JSON is not mentioned in the
given Markdown file (as ``NAME`` somewhere in the text).
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve()
REPO_ROOT = HERE.parents[4]  # repository root
API_DIR = REPO_ROOT / "apps" / "api"
PLANE_DIR = API_DIR / "plane"
OUT_PATH = HERE.parent / "env_vars.json"

SKIP_DIR_NAMES = {"tests", "migrations", "__pycache__", "node_modules"}

# Shell parameter expansions: ${VAR}, ${VAR:-d}, ${VAR-d}, ${VAR:=d}, ${VAR=d},
# ${VAR:?m}, ${VAR:??}, ${VAR:+a}, and the bare $VAR form.
SHELL_BRACED = re.compile(
    r"\$\{(?P<name>[A-Za-z_][A-Za-z0-9_]*)(?:(?P<op>:[-=?+]?|[-=?+])(?P<default>[^}]*))?\}"
)
SHELL_BARE = re.compile(r"(?<!\\)\$(?P<name>[A-Za-z_][A-Za-z0-9_]*)")

# Names that the shell itself owns; never deployment configuration.
SHELL_SPECIAL = {
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "@",
    "*",
    "#",
    "?",
    "-",
    "$",
    "!",
    "_",
    "IFS",
    "PATH",
    "HOME",
    "PWD",
    "OLDPWD",
    "SHELL",
    "USER",
    "RANDOM",
    "SECONDS",
    "LINENO",
    "BASH_SOURCE",
    "FUNCNAME",
    "REPLY",
}

ENVON = ("os", "environ")


def _attr_chain(node: ast.AST) -> list[str] | None:
    """Return the dotted-name chain of an expression, e.g. ['os','environ','get']."""
    parts: list[str] = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    if isinstance(node, ast.Name):
        parts.append(node.id)
        return list(reversed(parts))
    return None


def _const_str(node: ast.AST | None) -> str | None:
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def _is_environ(node: ast.AST) -> bool:
    """True for ``os.environ`` or a bare ``environ`` imported from os."""
    chain = _attr_chain(node)
    return chain in (["os", "environ"], ["environ"])


def scan_python(path: Path) -> list[dict]:
    source = path.read_text(encoding="utf-8")
    try:
        tree = ast.parse(source, filename=str(path))
    except SyntaxError as exc:  # pragma: no cover - the tree is known-good
        print(f"warning: cannot parse {path}: {exc}", file=sys.stderr)
        return []

    rel = str(path.relative_to(REPO_ROOT))
    found: list[dict] = []

    def default_segment(node: ast.AST | None) -> str | None:
        if node is None:
            return None
        return ast.get_source_segment(source, node)

    def record(name: str | None, key_node: ast.AST, line: int, kind: str, default: str | None, has_default: bool):
        found.append(
            {
                "file": rel,
                "line": line,
                "kind": kind,
                "name": name,
                "key_expr": ast.get_source_segment(source, key_node) or "",
                "has_default": has_default,
                "default": default,
                "dynamic": name is None,
            }
        )

    for node in ast.walk(tree):
        # os.environ.get(...) / os.getenv(...) / environ.get(...) / os.environ.setdefault(...)
        if isinstance(node, ast.Call):
            chain = _attr_chain(node.func)
            method = chain[-1] if chain else None
            receiver = chain[:-1] if chain else []
            if method == "get" and (receiver in (["os", "environ"], ["environ"])):
                kind = ".".join(receiver) + ".get"
            elif method == "getenv" and receiver == ["os"]:
                kind = "os.getenv"
            elif method == "setdefault" and receiver in (["os", "environ"], ["environ"]):
                kind = ".".join(receiver) + ".setdefault"
            else:
                kind = ""
            if kind:
                key_node = node.args[0] if node.args else None
                if key_node is None:
                    continue
                has_default = len(node.args) > 1
                default = default_segment(node.args[1]) if has_default else None
                record(_const_str(key_node), key_node, node.lineno, kind, default, has_default)
                continue

        # os.environ["K"] / environ["K"]
        if isinstance(node, ast.Subscript) and _is_environ(node.value):
            record(
                _const_str(node.slice),
                node.slice,
                node.lineno,
                "os.environ[...]",
                None,
                False,
            )
            continue

        # "K" in os.environ / "K" not in os.environ
        if isinstance(node, ast.Compare):
            operands = [node.left, *node.comparators]
            for index, op in enumerate(node.ops):
                if isinstance(op, (ast.In, ast.NotIn)) and _is_environ(operands[index + 1]):
                    key_node = operands[index]
                    record(
                        _const_str(key_node),
                        key_node,
                        node.lineno,
                        "in os.environ" if isinstance(op, ast.In) else "not in os.environ",
                        None,
                        False,
                    )

    return found


def scan_shell(path: Path) -> list[dict]:
    text = path.read_text(encoding="utf-8")
    rel = str(path.relative_to(REPO_ROOT))
    found: list[dict] = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        stripped = line.lstrip()
        if stripped.startswith("#"):
            continue
        consumed: list[tuple[int, int]] = []
        for match in SHELL_BRACED.finditer(line):
            name = match.group("name")
            if name in SHELL_SPECIAL:
                continue
            consumed.append(match.span())
            op = match.group("op") or ""
            default = match.group("default") if op in (":-", "-", ":=", "=") else None
            found.append(
                {
                    "file": rel,
                    "line": lineno,
                    "kind": "${...}",
                    "name": name,
                    "key_expr": match.group(0),
                    "has_default": default is not None,
                    "default": default,
                    "dynamic": False,
                    "expansion_op": op,
                }
            )
        for match in SHELL_BARE.finditer(line):
            if any(start <= match.start() < end for start, end in consumed):
                continue
            name = match.group("name")
            if name in SHELL_SPECIAL:
                continue
            found.append(
                {
                    "file": rel,
                    "line": lineno,
                    "kind": "$VAR",
                    "name": name,
                    "key_expr": match.group(0),
                    "has_default": False,
                    "default": None,
                    "dynamic": False,
                }
            )
    return found


def scan_dockerfile(path: Path) -> list[dict]:
    text = path.read_text(encoding="utf-8")
    rel = str(path.relative_to(REPO_ROOT))
    found: list[dict] = []
    continuation = ""
    for lineno, raw in enumerate(text.splitlines(), start=1):
        line = raw
        if continuation:
            line = continuation + line
        if line.rstrip().endswith("\\"):
            continuation = line.rstrip()[:-1]
            continue
        continuation = ""
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        match = re.match(r"^(ENV|ARG)\s+(.*)$", stripped)
        if not match:
            continue
        directive, rest = match.group(1), match.group(2)
        # "ENV K=v" or "ENV K v" (legacy). Multiple assignments per line are legal.
        for assignment in re.finditer(r"([A-Za-z_][A-Za-z0-9_]*)(?:=(\S*))?", rest):
            name = assignment.group(1)
            value = assignment.group(2)
            found.append(
                {
                    "file": rel,
                    "line": lineno,
                    "kind": directive,
                    "name": name,
                    "key_expr": stripped,
                    "has_default": value is not None,
                    "default": value,
                    "dynamic": False,
                }
            )
            if directive == "ENV" and "=" not in rest:
                break
    return found


def collect() -> dict:
    occurrences: list[dict] = []

    python_files = sorted(
        path
        for path in PLANE_DIR.rglob("*.py")
        if not SKIP_DIR_NAMES.intersection(part for part in path.relative_to(PLANE_DIR).parts[:-1])
    )
    for path in python_files:
        occurrences.extend(scan_python(path))

    for path in sorted((API_DIR / "bin").glob("*.sh")):
        occurrences.extend(scan_shell(path))

    for name in ("Dockerfile.api",):
        path = API_DIR / name
        if path.exists():
            occurrences.extend(scan_dockerfile(path))

    variables: dict[str, list[dict]] = {}
    dynamic: list[dict] = []
    for item in occurrences:
        if item["name"] is None:
            dynamic.append({k: v for k, v in item.items() if k != "name"})
            continue
        variables.setdefault(item["name"], []).append(item)

    for items in variables.values():
        items.sort(key=lambda item: (item["file"], item["line"]))

    return {
        "generated_by": "apps/server/tools/inventory/env_vars.py",
        "scanned": {
            "python_root": str(PLANE_DIR.relative_to(REPO_ROOT)),
            "python_files": len(python_files),
            "shell_globs": [str((API_DIR / "bin").relative_to(REPO_ROOT) / "*.sh")],
            "dockerfiles": [str((API_DIR / "Dockerfile.api").relative_to(REPO_ROOT))],
            "excluded_dirs": sorted(SKIP_DIR_NAMES),
        },
        "counts": {
            "distinct_names": len(variables),
            "occurrences": sum(len(items) for items in variables.values()),
            "dynamic_reads": len(dynamic),
        },
        "variables": dict(sorted(variables.items())),
        "dynamic_reads": dynamic,
    }


def check_markdown(data: dict, markdown_path: Path) -> int:
    text = markdown_path.read_text(encoding="utf-8")
    # A variable counts as documented when its name appears as a whole word
    # (inside a table cell, backticks or a code block all work).
    patterns = {name: re.compile(rf"(?<![A-Za-z0-9_]){re.escape(name)}(?![A-Za-z0-9_])") for name in data["variables"]}
    missing = [name for name, pattern in patterns.items() if not pattern.search(text)]
    extra = sorted(
        {
            match
            for match in re.findall(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b", text)
            if match not in data["variables"]
        }
    )
    print(f"env_vars.json distinct names: {data['counts']['distinct_names']}")
    print(f"occurrences: {data['counts']['occurrences']}, dynamic reads: {data['counts']['dynamic_reads']}")
    print(f"documented in {markdown_path.name}: {len(patterns) - len(missing)}/{len(patterns)}")
    if missing:
        print(f"MISSING ({len(missing)}):")
        for name in missing:
            print(f"  - {name}")
    else:
        print("all variables from env_vars.json appear in the document")
    if extra:
        print(f"noted in the document but absent from the scan ({len(extra)}): {', '.join(extra)}")
    return 1 if missing else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--print", dest="do_print", action="store_true", help="print a summary table")
    parser.add_argument("--check-md", metavar="PATH", help="verify every scanned name appears in PATH")
    args = parser.parse_args()

    data = collect()
    OUT_PATH.write_text(json.dumps(data, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(f"wrote {OUT_PATH.relative_to(REPO_ROOT)}")
    print(f"distinct names: {data['counts']['distinct_names']}")
    print(f"occurrences: {data['counts']['occurrences']}")
    print(f"dynamic reads: {data['counts']['dynamic_reads']}")

    if args.do_print:
        for name, items in data["variables"].items():
            locations = ", ".join(f"{item['file']}:{item['line']}" for item in items[:3])
            more = "" if len(items) <= 3 else f" (+{len(items) - 3})"
            print(f"{name:38s} {locations}{more}")
        if data["dynamic_reads"]:
            print("\ndynamic reads (key not a literal):")
            for item in data["dynamic_reads"]:
                print(f"  {item['file']}:{item['line']} {item['kind']}({item['key_expr']})")

    if args.check_md:
        print()
        return check_markdown(data, Path(args.check_md))
    return 0


if __name__ == "__main__":
    sys.exit(main())
