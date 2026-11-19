#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Record `instance_config_variables` for `plane-bootstrap::configure_instance`.

`manage.py configure_instance` iterates
`plane/utils/instance_config_variables/core.py` (`instance_config_variables`,
`core.py:8-263`) and writes one `InstanceConfiguration` row per entry: the key,
the category, `is_encrypted`, and `os.environ.get(key, default)` as the value.
The Rust boot step iterates the same table, embedded from

    apps/server/crates/plane-bootstrap/assets/instance_config_variables.json

This script parses the Python source with `ast` (no import, so the ambient
environment cannot leak into the committed defaults) and writes or checks the
JSON. Every `value` must be `os.environ.get("<KEY>")` or
`os.environ.get("<KEY>", <literal default>)`; anything else is an error, because
the Rust side would have to guess.

    apps/api/.venv/bin/python tools/boot/gen_instance_config.py --write
    apps/api/.venv/bin/python tools/boot/gen_instance_config.py --check
"""

import argparse
import ast
import json
import sys
from pathlib import Path

SERVER_DIR = Path(__file__).resolve().parent.parent.parent
API_DIR = SERVER_DIR.parent.parent / "apps" / "api"
SOURCES = [
    API_DIR / "plane" / "utils" / "instance_config_variables" / "core.py",
    API_DIR / "plane" / "utils" / "instance_config_variables" / "extended.py",
]
GOLDEN = SERVER_DIR / "crates" / "plane-bootstrap" / "assets" / "instance_config_variables.json"


def literal(node):
    try:
        return ast.literal_eval(node)
    except (ValueError, SyntaxError) as error:
        raise SystemExit(f"not a literal: {ast.dump(node)} ({error})")


def env_get(node, where):
    if not isinstance(node, ast.Call) or not node.args:
        raise SystemExit(f"{where}: `value` is not os.environ.get(...): {ast.dump(node)}")
    function = node.func
    if not (
        isinstance(function, ast.Attribute)
        and function.attr == "get"
        and isinstance(function.value, ast.Attribute)
        and function.value.attr == "environ"
        and isinstance(function.value.value, ast.Name)
        and function.value.value.id == "os"
    ):
        raise SystemExit(f"{where}: `value` is not os.environ.get(...)")
    key = literal(node.args[0])
    default = literal(node.args[1]) if len(node.args) > 1 else None
    if not isinstance(key, str):
        raise SystemExit(f"{where}: environment key is not a string")
    return {"key": key, "default": default}


def collect():
    variables = []
    for source in SOURCES:
        tree = ast.parse(source.read_text(encoding="utf-8"), filename=str(source))
        for node in tree.body:
            if not isinstance(node, ast.Assign):
                continue
            if not any(
                isinstance(target, ast.Name)
                and target.id.endswith("_config_variables")
                for target in node.targets
            ):
                continue
            if not isinstance(node.value, ast.List):
                continue
            # `core_config_variables` is the aggregate (`*authentication_…`,
            # …); its entries are collected from the concrete lists above.
            if any(isinstance(item, ast.Starred) for item in node.value.elts):
                continue
            for item in node.value.elts:
                if not isinstance(item, ast.Dict):
                    raise SystemExit(f"{source}: a config entry is not a dict")
                fields = {}
                for key_node, value_node in zip(item.keys, item.values):
                    fields[literal(key_node)] = value_node
                where = f"{source.name}:{item.lineno}"
                missing = {"key", "value", "category", "is_encrypted"} - fields.keys()
                if missing:
                    raise SystemExit(f"{where}: missing {sorted(missing)}")
                parsed = env_get(fields["value"], where)
                key = literal(fields["key"])
                if key != parsed["key"]:
                    raise SystemExit(
                        f"{where}: key {key!r} but os.environ.get({parsed['key']!r})"
                    )
                variables.append(
                    {
                        "key": key,
                        "category": literal(fields["category"]),
                        "is_encrypted": literal(fields["is_encrypted"]),
                        "default": parsed["default"],
                    }
                )
    keys = [entry["key"] for entry in variables]
    duplicates = sorted({key for key in keys if keys.count(key) > 1})
    if duplicates:
        raise SystemExit(f"duplicate keys: {duplicates}")
    return {"variables": variables}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--write", action="store_true")
    group.add_argument("--check", action="store_true")
    args = parser.parse_args()

    collected = collect()
    rendered = json.dumps(collected, indent=2, sort_keys=False) + "\n"
    if args.write:
        GOLDEN.parent.mkdir(parents=True, exist_ok=True)
        GOLDEN.write_text(rendered, encoding="utf-8")
        print(f"wrote {GOLDEN} ({len(collected['variables'])} variables)")
        return 0

    if not GOLDEN.exists():
        print(f"FAIL: {GOLDEN} is missing; run --write")
        return 1
    if GOLDEN.read_text(encoding="utf-8") != rendered:
        print(f"FAIL: {GOLDEN} is stale; run --write")
        return 1
    print(f"PASS: {len(collected['variables'])} instance configuration variables")
    return 0


if __name__ == "__main__":
    sys.exit(main())
