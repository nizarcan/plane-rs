#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the Rust/Django coexistence proxy snippet from route ownership.

The inputs are all checked in:

* ``apps/server/routes.json`` -- the route inventory written by
  ``apps/server/tools/inventory/routes.py``; its ``path`` templates and
  ``django_pattern`` converters are the source of the URL regexes.
* ``apps/server/ROUTES.md`` -- the hand-edited Owner column: ``django``,
  ``rust`` or ``rust(GET,HEAD)``.
* ``apps/server/deploy/route-groups.yaml`` -- which routes form a group, so a
  group can be moved to Rust or sent back with one flag.

The output is ``apps/server/deploy/caddy/rust-routes.caddy``, a Caddyfile
fragment imported by ``deploy/caddy/Caddyfile.coexist`` (production shape) and
``dev/Caddyfile.dev`` (local proxy). It contains one named matcher and one
``handle`` block per group that has at least one Rust-owned route+method, and
nothing but a comment for groups that are entirely Django-owned.

Usage::

    apps/api/.venv/bin/python apps/server/tools/proxy/gen_caddy.py
    apps/api/.venv/bin/python apps/server/tools/proxy/gen_caddy.py --check
    apps/api/.venv/bin/python apps/server/tools/proxy/gen_caddy.py --explain

``--check`` regenerates in memory and exits 1 when the committed file is stale;
CI runs it. ``--explain`` prints the route -> group assignment and the flag for
every group.

Why a per-request environment lookup
------------------------------------
Caddy has two environment mechanisms. ``{$NAME}`` is substituted by the
Caddyfile adapter *before parsing*, so it is baked into the loaded config:
flipping a group would mean regenerating and reloading the config, and the
generated file would differ per deployment. ``{env.NAME}`` is a placeholder
resolved from the Caddy process environment *at request time* (it is a global
placeholder; Caddy's CEL matcher rewrites it into a ``ph(req, "env.NAME")``
call). The generated file therefore stays environment-independent, the same
bytes are valid in every deployment, and a deployment flips a group by setting
``PLANE_RS_GROUP_<NAME>`` and restarting/relaunching the proxy with the subset
of routes it wants Rust to serve. A process environment cannot change while the
process runs, so a flag change is a container (or reload) event, not a request
event; per-request lookup is what makes the *same config* serve both states.

The upstream (``{$PLANE_RS_API_UPSTREAM:plane-api:8000}``) is config-time on
purpose: it is a deployment address, and the adapter's default syntax is the
normal way to give it one.
"""

from __future__ import annotations

import argparse
import difflib
import importlib.util
import json
import re
import sys
from collections import OrderedDict, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

import yaml

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
REPO_ROOT = SCRIPT_PATH.parents[4]
DEFAULT_ROUTES_JSON = SERVER_DIR / "routes.json"
DEFAULT_OWNERS_FILE = SERVER_DIR / "ROUTES.md"
DEFAULT_GROUPS_FILE = SERVER_DIR / "deploy" / "route-groups.yaml"
DEFAULT_OUT = SERVER_DIR / "deploy" / "caddy" / "rust-routes.caddy"

UPSTREAM_PLACEHOLDER = "{$PLANE_RS_API_UPSTREAM:plane-api:8000}"
UPSTREAM_ENV = "PLANE_RS_API_UPSTREAM"
FLAG_PREFIX = "PLANE_RS_GROUP_"

# Django 5.2's converters, copied verbatim from
# apps/api/.venv/lib/python3.12/site-packages/django/urls/converters.py so the
# proxy matches exactly what Django's resolver accepts. In particular uuid is
# lowercase-only.
DJANGO_CONVERTERS = OrderedDict(
    [
        ("int", "[0-9]+"),
        ("path", ".+"),
        ("slug", "[-a-zA-Z0-9_]+"),
        ("str", "[^/]+"),
        (
            "uuid",
            "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        ),
    ]
)
# DRF's FormatSuffixConverter (rest_framework/urlpatterns.py): the regex
# includes the dot and an optional trailing slash, which is why routes.json's
# template is `.format` rather than `{format}` for these entries.
DRF_FORMAT_SUFFIX = r"\.[a-z0-9]+/?"

METHOD_ORDER = ("GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH", "DELETE", "TRACE")

# The marker routes.py writes for a duplicate registration that Django never
# reaches. Marking one as rust would set up a rule for a URL Django answers
# with the earlier route, so the generator refuses it.
UNREACHABLE_MARKER = "unreachable: Django uses the first registration"

ROUTE_PARAM_RE = re.compile(r"\{([^}]+)\}")
DJANGO_PARAM_RE = re.compile(
    r"(?<!\(\?P)<(?:(?P<converter>[^>:]+):)?(?P<name>[^>]+)>"
)
GROUP_NAME_RE = re.compile(r"^[a-z][a-z0-9-]*$")


# ---------------------------------------------------------------------------
# Inventory and ownership inputs
# ---------------------------------------------------------------------------


def load_inventory(path: Path) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    routes = payload.get("routes")
    if not isinstance(routes, list) or not routes:
        raise SystemExit(f"{path}: no routes")
    return payload, routes


def load_inventory_module() -> Any:
    """Import apps/server/tools/inventory/routes.py without a package."""
    path = SCRIPT_PATH.parents[1] / "inventory" / "routes.py"
    spec = importlib.util.spec_from_file_location("plane_inventory_routes", path)
    if spec is None or spec.loader is None:  # pragma: no cover - broken checkout
        raise SystemExit(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_owners(path: Path) -> dict[tuple[str, str, str], tuple[str, str]]:
    routes_module = load_inventory_module()
    return routes_module.parse_ownership(path.read_text(encoding="utf-8"))


# ---------------------------------------------------------------------------
# Route groups
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Group:
    name: str
    description: str
    routes: tuple[tuple[str, str], ...] = ()
    match: tuple[str, ...] = ()

    @property
    def flag(self) -> str:
        return FLAG_PREFIX + self.name.replace("-", "_").upper()


def path_segments(path: str) -> list[str]:
    if path == "/":
        return []
    return path.strip("/").split("/")


def glob_matches(pattern: str, path: str) -> bool:
    """Match a route template against a group glob.

    ``*`` matches one segment, a trailing ``**`` matches zero or more trailing
    segments, and everything else is literal.
    """
    pattern_segments = path_segments(pattern)
    path_segments_ = path_segments(path)
    for index, segment in enumerate(pattern_segments):
        if segment == "**":
            return index == len(pattern_segments) - 1 and len(path_segments_) >= index
        if index >= len(path_segments_):
            return False
        if segment not in ("*", path_segments_[index]):
            return False
    return len(path_segments_) == len(pattern_segments)


def selector_score(pattern: str) -> tuple[int, int, int, int]:
    """How specific a glob is: more literal segments, fewer wildcards, longer."""
    segments = path_segments(pattern)
    literals = sum(1 for segment in segments if segment not in ("*", "**"))
    return (
        literals,
        -segments.count("**"),
        -segments.count("*"),
        len(pattern),
    )


def load_groups(path: Path, routes: list[dict[str, Any]]) -> list[Group]:
    with path.open(encoding="utf-8") as handle:
        payload = yaml.safe_load(handle)
    if not isinstance(payload, dict) or payload.get("version") != 1:
        raise SystemExit(f"{path}: expected a mapping with version: 1")

    raw_groups = payload.get("groups")
    if not isinstance(raw_groups, list) or not raw_groups:
        raise SystemExit(f"{path}: expected a non-empty groups list")

    known_names: set[str] = set()
    known_flags: dict[str, str] = {}
    known_route_keys = {(route["group"], route["path"]) for route in routes}
    groups: list[Group] = []

    for index, entry in enumerate(raw_groups):
        where = f"{path}: groups[{index}]"
        if not isinstance(entry, dict):
            raise SystemExit(f"{where}: expected a mapping")
        unknown = set(entry) - {"name", "description", "routes", "match"}
        if unknown:
            raise SystemExit(f"{where}: unknown key(s) {', '.join(sorted(unknown))}")

        name = entry.get("name")
        if not isinstance(name, str) or not GROUP_NAME_RE.match(name):
            raise SystemExit(
                f"{where}: name must be lowercase letters, digits and hyphens, "
                f"starting with a letter; got {name!r}"
            )
        if name in known_names:
            raise SystemExit(f"{where}: duplicate group name {name!r}")
        known_names.add(name)

        description = entry.get("description", "")
        if not isinstance(description, str):
            raise SystemExit(f"{where}: description must be a string")

        flag = FLAG_PREFIX + name.replace("-", "_").upper()
        if flag in known_flags:
            raise SystemExit(
                f"{where}: flag {flag} collides with group {known_flags[flag]!r}"
            )
        known_flags[flag] = name

        exact_entries = entry.get("routes") or []
        if not isinstance(exact_entries, list):
            raise SystemExit(f"{where}: routes must be a list")
        exact: list[tuple[str, str]] = []
        for item in exact_entries:
            if (
                not isinstance(item, dict)
                or set(item) != {"group", "path"}
                or not isinstance(item["group"], str)
                or not isinstance(item["path"], str)
            ):
                raise SystemExit(
                    f"{where}: each routes entry must be {{group: ..., path: ...}}"
                )
            key = (item["group"], item["path"])
            if key not in known_route_keys:
                raise SystemExit(
                    f"{where}: {item['path']!r} is not a {item['group']} route in "
                    "routes.json"
                )
            if key in exact:
                raise SystemExit(f"{where}: duplicate routes entry {key}")
            exact.append(key)

        patterns = entry.get("match") or []
        if not isinstance(patterns, list):
            raise SystemExit(f"{where}: match must be a list")
        for pattern in patterns:
            if not isinstance(pattern, str) or not pattern.startswith("/"):
                raise SystemExit(f"{where}: match patterns must be paths starting with /")
            segments = path_segments(pattern)
            for position, segment in enumerate(segments):
                if segment == "**" and position != len(segments) - 1:
                    raise SystemExit(
                        f"{where}: '**' is only allowed as the last segment of {pattern!r}"
                    )
                if "/" in segment:
                    raise SystemExit(f"{where}: malformed pattern {pattern!r}")

        if not exact and not patterns:
            raise SystemExit(f"{where}: group {name!r} selects no routes")

        groups.append(
            Group(
                name=name,
                description=description,
                routes=tuple(exact),
                match=tuple(patterns),
            )
        )

    return groups


def assign_groups(
    routes: list[dict[str, Any]], groups: list[Group]
) -> tuple[dict[int, Group], dict[str, list[dict[str, Any]]]]:
    """Assign every route to exactly one group.

    Exact selectors win. Otherwise the most specific matching glob wins. A tie
    between two groups, or no match at all, is an error: the proxy must never
    guess which flag owns a path.
    """
    errors: list[str] = []

    exact_owner: dict[tuple[str, str], Group] = {}
    for group in groups:
        for key in group.routes:
            if key in exact_owner:
                errors.append(
                    f"route {key[1]} ({key[0]}) is listed exactly by both "
                    f"{exact_owner[key].name!r} and {group.name!r}"
                )
                continue
            exact_owner[key] = group

    by_order: dict[int, Group] = {}
    by_group: dict[str, list[dict[str, Any]]] = defaultdict(list)

    for route in routes:
        key = (route["group"], route["path"])
        candidates: list[tuple[tuple[int, int, int, int], Group, str]] = []
        if key in exact_owner:
            candidates.append(((10**6, 0, 0, 0), exact_owner[key], "exact"))
        for group in groups:
            for pattern in group.match:
                if glob_matches(pattern, route["path"]):
                    candidates.append((selector_score(pattern), group, pattern))

        if not candidates:
            errors.append(
                f"route {route['path']} ({route['group']}) belongs to no group in "
                "route-groups.yaml"
            )
            continue

        candidates.sort(key=lambda item: item[0], reverse=True)
        best_score = candidates[0][0]
        winners = [item for item in candidates if item[0] == best_score]
        winner_groups = {item[1].name for item in winners}
        if len(winner_groups) > 1:
            detail = ", ".join(
                f"{group.name} via {pattern!r}" for _, group, pattern in winners
            )
            errors.append(
                f"route {route['path']} ({route['group']}) is matched equally well "
                f"by {detail}; make one selector more specific or drop one"
            )
            continue

        group = winners[0][1]
        by_order[route["order"]] = group
        by_group[group.name].append(route)

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)

    return by_order, by_group


# ---------------------------------------------------------------------------
# URL regexes
# ---------------------------------------------------------------------------


def escape_literal(text: str) -> str:
    """Escape literal path text for RE2 (used by Caddy's path_regexp)."""
    return re.sub(r"([\\.+*?()|\[\]{}^$])", r"\\\1", text)


def route_converters(route: dict[str, Any]) -> dict[str, str]:
    """Map each ``{name}`` in the route template to its converter or regex group.

    ``routes.json`` carries leaf parameters in ``params``. Parameters that come
    from an enclosing ``URLResolver`` (for example the ``<str:slug>`` prefix of
    the ``/api/v1/workspaces/<str:slug>/`` include) are not repeated there, so
    they are read from ``django_pattern``, which is the full Django pattern.
    """
    converters: dict[str, str] = dict(route.get("params") or {})

    for match in DJANGO_PARAM_RE.finditer(route["django_pattern"]):
        name = match.group("name")
        converter = match.group("converter") or "str"
        known = converters.get(name)
        if known is not None and known != converter:
            raise SystemExit(
                f"route {route['path']}: {name!r} is {known!r} in routes.json but "
                f"{converter!r} in django_pattern"
            )
        converters.setdefault(name, converter)

    for name in ROUTE_PARAM_RE.findall(route["path"]):
        if name not in converters:
            raise SystemExit(
                f"route {route['path']}: no converter for parameter {name!r} in "
                "routes.json or django_pattern"
            )

    for name, converter in converters.items():
        if converter in DJANGO_CONVERTERS or converter == "drf_format_suffix":
            continue
        if route.get("pattern_type") == "regex":
            # DefaultRouter regex groups store their body here, e.g. `[^/.]+`.
            continue
        raise SystemExit(
            f"route {route['path']}: unknown converter {converter!r} for {name!r}; "
            "add its Django regex to DJANGO_CONVERTERS"
        )

    return converters


def route_regex(route: dict[str, Any]) -> str:
    """Anchored RE2 regex for one route template, with Django's converters."""
    converters = route_converters(route)
    template = route["path"]
    pieces: list[str] = []
    cursor = 0
    last_converter: str | None = None

    for match in ROUTE_PARAM_RE.finditer(template):
        literal = template[cursor : match.start()]
        name = match.group(1)
        converter = converters[name]

        if converter == "drf_format_suffix":
            # The dot is part of the converter's regex, not of the literal.
            if literal.endswith("."):
                literal = literal[:-1]
            expression = DRF_FORMAT_SUFFIX
        else:
            expression = DJANGO_CONVERTERS.get(converter, converter)

        pieces.append(escape_literal(literal))
        pieces.append(expression)
        cursor = match.end()
        last_converter = converter

    pieces.append(escape_literal(template[cursor:]))

    trailing = ""
    if route.get("trailing_slash_form") == "optional" and last_converter != "drf_format_suffix":
        # DRF's regex routes carry the optional slash in their leaf pattern;
        # routes.json records that as trailing_slash_form=optional and strips
        # it from the template.
        trailing = "/?"

    regex = "^" + "".join(pieces) + trailing + "$"
    if '"' in regex or "\n" in regex or "`" in regex:
        raise SystemExit(
            f"route {route['path']}: regex {regex!r} cannot be embedded in a "
            "Caddyfile CEL raw string"
        )
    return regex


# ---------------------------------------------------------------------------
# Plan
# ---------------------------------------------------------------------------


@dataclass
class Cluster:
    methods: tuple[str, ...]
    regexes: list[str] = field(default_factory=list)
    routes: list[str] = field(default_factory=list)


@dataclass
class GroupPlan:
    group: Group
    route_count: int
    method_pairs: int
    clusters: list[Cluster] = field(default_factory=list)

    @property
    def emitted(self) -> bool:
        return bool(self.clusters)


def sorted_methods(methods: Iterable[str]) -> tuple[str, ...]:
    def key(method: str) -> tuple[int, str]:
        try:
            return (METHOD_ORDER.index(method), method)
        except ValueError:  # pragma: no cover - validated by routes.py
            return (len(METHOD_ORDER), method)

    return tuple(sorted(methods, key=key))


def build_plan(
    routes: list[dict[str, Any]],
    owners: dict[tuple[str, str, str], tuple[str, str]],
    groups: list[Group],
    routes_module: Any,
) -> list[GroupPlan]:
    _by_order, by_group = assign_groups(routes, groups)

    plans: list[GroupPlan] = []
    for group in groups:
        group_routes = by_group.get(group.name, [])
        plan = GroupPlan(
            group=group,
            route_count=len(group_routes),
            method_pairs=sum(len(routes_module.route_methods(route)) for route in group_routes),
        )
        clusters: OrderedDict[tuple[str, ...], Cluster] = OrderedDict()

        for route in group_routes:
            key = (route["group"], route["path"], route["view"])
            owner_cell, _status = owners.get(key, ("django", "not-started"))
            try:
                engine, declared = routes_module.validate_owner_cell(owner_cell, route)
            except routes_module.OwnerCellError as exc:
                raise SystemExit(f"{route['path']}: {exc}") from None

            if engine == "django":
                continue
            if declared is None:
                methods = routes_module.route_methods(route)
            else:
                methods = {method.lower() for method in declared}

            if not methods:  # pragma: no cover - validate_owner_cell rejects this
                continue
            if UNREACHABLE_MARKER in route.get("notes", []):
                # A duplicate registration of a path Django already serves from
                # an earlier entry. It shares (group, path, view) with that
                # entry, so the Owner cell cannot distinguish them; the
                # reachable twin carries the rule. Skip it rather than emit a
                # second identical regex (or refuse a row the user legitimately
                # marked rust).
                continue

            signature = sorted_methods(method.upper() for method in methods)
            cluster = clusters.setdefault(signature, Cluster(methods=signature))
            regex = route_regex(route)
            if regex not in cluster.regexes:
                cluster.regexes.append(regex)
            cluster.routes.append(route["path"])

        plan.clusters = list(clusters.values())
        plans.append(plan)

    return plans


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def render_flags_table(plans: list[GroupPlan]) -> list[str]:
    lines = [
        "| Group | Flag | Routes | Methods | Emitted |",
        "| --- | --- | ---: | ---: | --- |",
    ]
    for plan in plans:
        flag = f"`{plan.group.flag}`"
        lines.append(
            f"| {plan.group.name} | {flag} | {plan.route_count} | "
            f"{plan.method_pairs} | {'yes' if plan.emitted else 'no'} |"
        )
    return lines


def render_expression(plan: GroupPlan) -> list[str]:
    flag = plan.group.flag
    lines = [
        "\t\t("
        f'{{env.{flag}}} == "" ? "rust" : {{env.{flag}}}'
        ') == "rust"',
        "\t\t&& (",
    ]
    terms: list[str] = []
    for cluster in plan.clusters:
        methods = ", ".join(f'"{method}"' for method in cluster.methods)
        paths = " || ".join(f'path_regexp(r"{regex}")' for regex in cluster.regexes)
        terms.append(f"\t\t\t(method({methods}) && ({paths}))")
    lines.append("\n\t\t\t|| ".join(terms))
    lines.append("\t\t)")
    return lines


def render_snippet(plans: list[GroupPlan], inventory: dict[str, Any]) -> str:
    generated = inventory.get("generated_from", {})
    lines: list[str] = []
    lines.append("# Copyright (c) 2023-present Plane Software, Inc. and contributors")
    lines.append("# SPDX-License-Identifier: AGPL-3.0-only")
    lines.append("# See the LICENSE file for details.")
    lines.append("#")
    lines.append("# GENERATED FILE -- DO NOT EDIT.")
    lines.append("#")
    lines.append("# Written by apps/server/tools/proxy/gen_caddy.py from")
    lines.append("#   apps/server/routes.json            (path templates and converters)")
    lines.append("#   apps/server/ROUTES.md              (Owner column)")
    lines.append("#   apps/server/deploy/route-groups.yaml (group membership)")
    lines.append(f"# Route inventory generated from commit {generated.get('commit', 'unknown')}.")
    lines.append("#")
    lines.append("# What this file is")
    lines.append("# ----------------")
    lines.append("# A Caddyfile fragment, imported inside the site block by")
    lines.append("# deploy/caddy/Caddyfile.coexist and by dev/Caddyfile.dev. For every route")
    lines.append("# group that has at least one Rust-owned route it defines")
    lines.append("#")
    lines.append("#   @plane_rs_<group>  a named matcher: exact path(s) and exact method(s)")
    lines.append("#   handle @plane_rs_<group> { reverse_proxy <Rust upstream> }")
    lines.append("#")
    lines.append("# `handle` is used instead of a bare `reverse_proxy` because handle blocks")
    lines.append("# are mutually exclusive and are not reordered by the Caddyfile adapter.")
    lines.append("# Both Caddyfiles that import this fragment wrap their production routing in")
    lines.append("# one fallback `handle`, in the same mutually-exclusive group, so a Rust")
    lines.append("# handle that matches ends routing there: the fallback is skipped and the")
    lines.append("# request is proxied exactly once. A bare `reverse_proxy` with a named")
    lines.append("# matcher would be sorted after the single-path directives (Caddy sorts")
    lines.append("# same-named directives by path-matcher specificity) and would let the")
    lines.append("# request continue down the chain, reaching Django as well.")
    lines.append("#")
    lines.append("# Path matching")
    lines.append("# -------------")
    lines.append("# Patterns are anchored ^...$ RE2 regexes built from the Django converter")
    lines.append("# regexes (see DJANGO_CONVERTERS in gen_caddy.py; uuid is lowercase-only),")
    lines.append("# DRF's `.format` suffix regex, and DRF's optional trailing slash. Caddy's")
    lines.append("# path_regexp matches the URI-decoded, cleaned path (it runs against")
    lines.append("# r.URL.Path and merges doubled slashes / resolves dot segments); Django's")
    lines.append("# resolver also works on the decoded PATH_INFO, so the two agree. The")
    lines.append("# cleaning means a request like /api//instances/ is matched here as")
    lines.append("# /api/instances/ where Django would 404; Rust must 404 such paths itself")
    lines.append("# (its router will not have them).")
    lines.append("#")
    lines.append("# Method matching")
    lines.append("# ---------------")
    lines.append("# A whole-row `rust` includes the implicit HEAD Django answers for a GET")
    lines.append("# and the OPTIONS DRF answers for every DRF view; `rust(GET,HEAD)` moves")
    lines.append("# only the listed methods and leaves the rest of the row on Django.")
    lines.append("# Clusters are OR-ed as (method(...) && path_regexp(...)), so a path that is")
    lines.append("# Rust-owned for GET and Django-owned for POST is split correctly even when")
    lines.append("# other routes in the same group have a different method set.")
    lines.append("#")
    lines.append("# Flags")
    lines.append("# -----")
    lines.append("# Each group is controlled by PLANE_RS_GROUP_<NAME>. The value is read per")
    lines.append("# request with the {env.*} placeholder, not substituted at config load with")
    lines.append("# {$...}: the generated file is therefore identical in every environment,")
    lines.append("# and a deployment flips a group by setting the variable and restarting (or")
    lines.append("# reloading) the proxy. `rust` -- or unset -- sends the group to Rust;")
    lines.append("# `django` (or any other value) sends every method back to Django. A group")
    lines.append("# with no Rust-owned route has no matcher, so its flag has no effect until")
    lines.append("# a route in it is marked rust.")
    lines.append("#")
    lines.append(f"# Rust upstream: {UPSTREAM_PLACEHOLDER} (env {UPSTREAM_ENV},")
    lines.append("# overridden by Caddyfile.dev to reach the host).")
    lines.append("#")
    lines.append("# Groups")
    lines.append("# ------")
    lines.extend(f"# {line}" for line in render_flags_table(plans))
    lines.append("")

    emitted = [plan for plan in plans if plan.emitted]
    if not emitted:
        lines.append("# No group has a Rust-owned route yet: every Owner cell in ROUTES.md is")
        lines.append("# `django`, so this fragment adds no matcher and the site block routes")
        lines.append("# exactly as apps/proxy/Caddyfile.ce does.")
        lines.append("")
        return "\n".join(lines)

    for plan in emitted:
        lines.append("# " + "-" * 74)
        lines.append(f"# {plan.group.name}: {plan.group.description}")
        lines.append(
            f"# {plan.route_count} route(s), {plan.method_pairs} endpoint x method "
            f"pair(s); PLANE_RS_GROUP_{plan.group.name.replace('-', '_').upper()}=django "
            "sends them back."
        )
        lines.append("# " + "-" * 74)
        lines.append(f"@plane_rs_{plan.group.name.replace('-', '_')} {{")
        lines.append("\texpression <<CEL")
        lines.extend(render_expression(plan))
        lines.append("\tCEL")
        lines.append("}")
        lines.append(f"handle @plane_rs_{plan.group.name.replace('-', '_')} {{")
        lines.append(f"\treverse_proxy {UPSTREAM_PLACEHOLDER}")
        lines.append("}")
        lines.append("")

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def explain(
    routes: list[dict[str, Any]],
    owners: dict[tuple[str, str, str], tuple[str, str]],
    groups: list[Group],
    routes_module: Any,
) -> None:
    by_order, by_group = assign_groups(routes, groups)
    for route in routes:
        group = by_order[route["order"]]
        key = (route["group"], route["path"], route["view"])
        owner_cell, _status = owners.get(key, ("django", "not-started"))
        try:
            engine, declared = routes_module.validate_owner_cell(owner_cell, route)
        except routes_module.OwnerCellError as exc:  # pragma: no cover - validated earlier
            print(f"{route['order']:4d} {group.name:14s} {route['path']}  ERROR: {exc}")
            continue
        methods = (
            sorted_methods(m.upper() for m in routes_module.route_methods(route))
            if engine == "rust" and declared is None
            else tuple(declared or ())
        )
        target = "rust(" + ",".join(methods) + ")" if methods else "django"
        print(f"{route['order']:4d} {group.name:14s} {target:24s} {route['path']}")
    print()
    for group in groups:
        group_routes = by_group.get(group.name, [])
        pairs = sum(len(routes_module.route_methods(route)) for route in group_routes)
        rust_pairs = 0
        for route in group_routes:
            key = (route["group"], route["path"], route["view"])
            owner_cell, _status = owners.get(key, ("django", "not-started"))
            engine, declared = routes_module.parse_owner_cell(owner_cell)
            if engine == "django":
                continue
            rust_pairs += (
                len(routes_module.route_methods(route))
                if declared is None
                else len(declared)
            )
        print(
            f"{group.flag:32s} {group.name:14s} {len(group_routes):4d} routes, "
            f"{pairs:4d} pairs, {rust_pairs:4d} on rust"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Generate apps/server/deploy/caddy/rust-routes.caddy from "
            "routes.json, the Owner column of ROUTES.md and route-groups.yaml."
        )
    )
    parser.add_argument(
        "--routes-json",
        type=Path,
        default=DEFAULT_ROUTES_JSON,
        help=f"route inventory (default: {DEFAULT_ROUTES_JSON.relative_to(REPO_ROOT)})",
    )
    parser.add_argument(
        "--owners-file",
        type=Path,
        default=DEFAULT_OWNERS_FILE,
        help=(
            "Markdown file with the Owner column (default: "
            f"{DEFAULT_OWNERS_FILE.relative_to(REPO_ROOT)})"
        ),
    )
    parser.add_argument(
        "--groups",
        type=Path,
        default=DEFAULT_GROUPS_FILE,
        help=f"group map (default: {DEFAULT_GROUPS_FILE.relative_to(REPO_ROOT)})",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help=f"output file (default: {DEFAULT_OUT.relative_to(REPO_ROOT)})",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail instead of writing when the committed output is stale",
    )
    parser.add_argument(
        "--explain",
        action="store_true",
        help="print the route -> group assignment and the flags, then exit",
    )
    args = parser.parse_args(argv)

    routes_module = load_inventory_module()
    inventory, routes = load_inventory(args.routes_json)
    owners = load_owners(args.owners_file)
    groups = load_groups(args.groups, routes)

    # routes.py owns the Owner grammar; fail with the same messages it uses.
    owner_errors = routes_module.validate_ownership(routes, owners)
    if owner_errors:
        for error in owner_errors:
            print(f"error: invalid Owner cell: {error}", file=sys.stderr)
        return 2

    plans = build_plan(routes, owners, groups, routes_module)

    if args.explain:
        explain(routes, owners, groups, routes_module)
        return 0

    text = render_snippet(plans, inventory)
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else ""
        if current == text:
            print(f"{display_path(args.out)} is up to date")
            return 0
        diff = difflib.unified_diff(
            current.splitlines(keepends=True),
            text.splitlines(keepends=True),
            fromfile=str(args.out),
            tofile=str(args.out) + " (regenerated)",
        )
        sys.stderr.writelines(diff)
        print(
            f"error: {display_path(args.out)} is stale; run "
            "apps/server/tools/proxy/gen_caddy.py and commit the result",
            file=sys.stderr,
        )
        return 1

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    emitted = [plan for plan in plans if plan.emitted]
    print(
        f"wrote {display_path(args.out)}: "
        f"{len(emitted)} of {len(plans)} group(s) on Rust, "
        f"{sum(plan.route_count for plan in plans)} routes assigned"
    )
    for plan in plans:
        state = "rust" if plan.emitted else "django"
        print(
            f"  {plan.group.name:14s} {state:6s} {plan.route_count:4d} routes "
            f"{plan.method_pairs:4d} pairs  {plan.group.flag}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
