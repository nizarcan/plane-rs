# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Route ownership overrides for the contract runner.

``ROUTES.md`` is the source of truth for which backend answers a request, but a
domain agent needs to exercise its write paths *before* flipping the table. The
two environment variables below are set by ``run.sh`` from ``--assume-rust`` and
``--methods``:

* ``PLANE_RS_CONTRACT_ASSUME_RUST`` — route orders (``routes.json``'s ``order``),
  comma or space separated. Every unsafe method (``POST``/``PUT``/``PATCH``/
  ``DELETE``) that route registers is treated as Rust-owned; a route with no
  unsafe method is reported and ignored. A route whose cell already claims
  methods (``rust(GET)``) keeps them: the cell only grows, so a GET that is
  already served by Rust stays there and a read-only route is not accidentally
  sent to an unported handler.
* ``PLANE_RS_CONTRACT_METHODS`` — narrows every plan cluster to these HTTP
  methods, so only requests that use them reach Rust. In ``dry_routing.py`` this
  also selects the files whose write calls match.

Both are read here so ``pytest_plane_contract.py`` and ``dry_routing.py`` apply
exactly the same override to the same plan.
"""

from __future__ import annotations

import dataclasses
import os

UNSAFE_METHODS = ("POST", "PUT", "PATCH", "DELETE")
METHOD_ORDER = ("GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS")


def _split(spec: str) -> list[str]:
    return [item.strip() for item in spec.replace(",", " ").split() if item.strip()]


def assume_rust_orders() -> list[int]:
    """``--assume-rust 223,224``: route orders to treat as write-owned by Rust."""
    orders: list[int] = []
    for item in _split(os.environ.get("PLANE_RS_CONTRACT_ASSUME_RUST", "")):
        try:
            orders.append(int(item))
        except ValueError:
            raise SystemExit(
                f"error: --assume-rust expects route orders, got {item!r}"
            ) from None
    return orders


def methods_filter() -> frozenset[str]:
    """``--methods POST,PATCH``: only plan clusters answering these methods."""
    return frozenset(item.upper() for item in _split(os.environ.get("PLANE_RS_CONTRACT_METHODS", "")))


def _merged_cell(current: str, unsafe: set[str]) -> str:
    """Add ``unsafe`` to a cell, keeping whatever Rust already owns.

    ``django`` becomes ``rust(POST,PATCH)``; ``rust(GET)`` becomes
    ``rust(GET,POST,PATCH)``; a whole-row ``rust`` stays whole-row.
    """
    text = current.strip()
    if text == "rust":
        return text
    existing: set[str] = set()
    if text.startswith("rust(") and text.endswith(")"):
        existing = {item.strip().upper() for item in text[5:-1].split(",") if item.strip()}
    methods = existing | unsafe
    ordered = [name for name in METHOD_ORDER if name in methods]
    return f"rust({','.join(ordered)})"


def override_owners(
    routes: list[dict],
    owners: dict[tuple[str, str, str], tuple[str, str]],
    orders: list[int],
) -> list[str]:
    """Point the given route orders' unsafe methods at Rust.

    Returns human-readable warnings (unknown order, route without an unsafe
    method). ``owners`` is mutated in place: its value is the ``(owner, status)``
    pair ``gen_caddy.build_plan`` reads, and the owner cell uses the
    ``rust(METHODS)`` form ``parse_owner_cell`` accepts.
    """
    warnings: list[str] = []
    wanted = set(orders)
    seen: set[int] = set()
    for route in routes:
        order = route.get("order")
        if not isinstance(order, int):
            continue
        seen.add(order)
        if order not in wanted:
            continue
        key = (route["group"], route["path"], route["view"])
        declared = {name.lower() for name in (route.get("methods") or {})}
        unsafe = declared & {name.lower() for name in UNSAFE_METHODS}
        if not unsafe:
            warnings.append(
                f"--assume-rust {order}: {route['path']} registers no unsafe method; ignored"
            )
            continue
        cell = _merged_cell(owners.get(key, ("django", ""))[0], {name.upper() for name in unsafe})
        owners[key] = (cell, "assumed")
    for order in sorted(wanted - seen):
        warnings.append(f"--assume-rust {order}: no route with that order")
    return warnings


def filter_methods(plan: list, methods: frozenset[str]) -> list:
    """Restrict every plan entry to ``methods``, dropping empty ones.

    Narrowing the set (rather than only dropping entries) matters for a
    ``rust(GET,POST)`` cluster: ``--methods POST`` must not let a file's GET
    call select it, and the proxy must not answer a GET the run did not ask
    about. The plugin's plan entries are ``RustRoute`` objects; ``dry_routing``'s
    are ``(group, methods, regex)`` tuples, so both shapes are handled.
    """
    if not methods:
        return plan
    kept = []
    for entry in plan:
        if hasattr(entry, "methods"):
            remaining = entry.methods & methods
            if remaining:
                kept.append(dataclasses.replace(entry, methods=frozenset(remaining)))
        else:
            remaining = entry[1] & methods
            if remaining:
                kept.append((entry[0], frozenset(remaining), entry[2]))
    return kept
