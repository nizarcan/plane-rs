#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the route-ownership -> Caddy config generator.

Run with either::

    apps/api/.venv/bin/python -m pytest apps/server/tools/proxy/test_gen_caddy.py
    apps/api/.venv/bin/python -m unittest apps/server/tools/proxy/test_gen_caddy.py

The central test walks every route in ``routes.json``, marks that one route as
``rust``, generates the snippet, and checks that the generated regex for the
route's group matches a concrete sample URL built from the route's converters
and does not match any sample URL of any other (reachable) route. That is the
property the proxy relies on: the group's matcher selects exactly the paths the
Owner column moved to Rust.
"""

from __future__ import annotations

import contextlib
import io
import re
import sys
import tempfile
import unittest
from pathlib import Path

PROXY_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(PROXY_DIR))

import gen_caddy  # noqa: E402  (path manipulation must come first)

UUID_SAMPLE = "01234567-89ab-cdef-0123-456789abcdef"
STR_SAMPLE = "sample"
INT_SAMPLE = "7"
FORMAT_SAMPLE = "json"


def sample_for(converter: str, name: str) -> str:
    """A concrete value for a ``{name}`` parameter, matching its converter."""
    fixed = {
        "str": STR_SAMPLE,
        "slug": STR_SAMPLE,
        "int": INT_SAMPLE,
        "uuid": UUID_SAMPLE,
        "path": STR_SAMPLE,
        "drf_format_suffix": FORMAT_SAMPLE,
        "[a-z0-9]+": FORMAT_SAMPLE,
        "[^/.]+": STR_SAMPLE,
    }
    if converter in fixed:
        return fixed[converter]
    for candidate in (STR_SAMPLE, INT_SAMPLE, FORMAT_SAMPLE, UUID_SAMPLE):
        if re.fullmatch(converter, candidate):
            return candidate
    raise AssertionError(f"no sample value for converter {converter!r} of {name!r}")


def route_samples(route: dict) -> list[str]:
    """Every concrete URL form a route accepts (the optional slash has two)."""
    converters = gen_caddy.route_converters(route)
    rendered = re.sub(
        r"\{([^}]+)\}",
        lambda match: sample_for(converters[match.group(1)], match.group(1)),
        route["path"],
    )
    form = route.get("trailing_slash_form")
    if form == "optional":
        if rendered.endswith("/"):
            return [rendered, rendered[:-1]]
        return [rendered, rendered + "/"]
    return [rendered]


def is_unreachable(route: dict) -> bool:
    return gen_caddy.UNREACHABLE_MARKER in route.get("notes", [])


class GeneratorTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.routes_module = gen_caddy.load_inventory_module()
        cls.inventory, cls.routes = gen_caddy.load_inventory(gen_caddy.DEFAULT_ROUTES_JSON)
        cls.groups = gen_caddy.load_groups(gen_caddy.DEFAULT_GROUPS_FILE, cls.routes)
        cls.committed_owners = gen_caddy.load_owners(gen_caddy.DEFAULT_OWNERS_FILE)
        cls.by_order = {route["order"]: route for route in cls.routes}

    def owners_with(self, *marked: tuple[dict, str]) -> dict:
        owners = dict(self.committed_owners)
        for route, cell in marked:
            key = (route["group"], route["path"], route["view"])
            self.assertIn(key, owners, f"{route['path']} is missing from ROUTES.md")
            owners[key] = (cell, owners[key][1])
        return owners

    def plan_for(self, owners: dict) -> list[gen_caddy.GroupPlan]:
        return gen_caddy.build_plan(self.routes, owners, self.groups, self.routes_module)

    def group_plan(self, plans: list, name: str) -> gen_caddy.GroupPlan:
        for plan in plans:
            if plan.group.name == name:
                return plan
        raise AssertionError(f"group {name!r} is missing from the plan")

    def clusters_matching(self, plan: gen_caddy.GroupPlan, url: str) -> list:
        return [
            cluster
            for cluster in plan.clusters
            if any(re.fullmatch(regex, url) for regex in cluster.regexes)
        ]


class TestInventoryParams(GeneratorTestCase):
    """`routes.py` must describe the whole pattern, not just the leaf."""

    def test_every_path_parameter_has_a_converter(self) -> None:
        for route in self.routes:
            for name in re.findall(r"\{([^}]+)\}", route["path"]):
                self.assertIn(
                    name,
                    route["params"],
                    f"{route['path']} has no converter for {name!r} "
                    f"(django_pattern {route['django_pattern']!r})",
                )

    def test_parameters_are_ordered_as_they_appear_in_the_path(self) -> None:
        for route in self.routes:
            in_path = [
                name
                for name in re.findall(r"\{([^}]+)\}", route["path"])
                if name in route["params"]
            ]
            self.assertEqual(
                [name for name in route["params"] if name in in_path],
                in_path,
                f"{route['path']}: `params` order must follow the URL",
            )

    def test_enclosing_include_converters_are_inherited(self) -> None:
        # The invitations and stickies routers are included under
        # `api/v1/workspaces/<str:slug>/`, so Django captures `slug` in the
        # enclosing resolver and the router-generated leaf captures nothing.
        # Before routes.py carried converters down the walk these 12 rows had
        # `params: {}` (or only the leaf's own groups) even though their
        # `django_pattern` contains `<str:slug>`.
        by_path = {route["path"]: route for route in self.routes}
        self.assertEqual(
            list(by_path["/api/v1/workspaces/{slug}/invitations/"]["params"].items()),
            [("slug", "str")],
        )
        self.assertEqual(
            list(by_path["/api/v1/workspaces/{slug}/stickies/"]["params"].items()),
            [("slug", "str")],
        )
        self.assertEqual(
            list(
                by_path["/api/v1/workspaces/{slug}/invitations/{pk}.{format}"][
                    "params"
                ].items()
            ),
            [("slug", "str"), ("pk", "[^/.]+"), ("format", "[a-z0-9]+")],
        )
        include = "api/v1/workspaces/<str:slug>/"
        inherited = [
            route
            for route in self.routes
            if route["django_pattern"].startswith(include)
            and (
                route["django_pattern"] == include
                or route["django_pattern"][len(include) :].startswith("^")
                or route["django_pattern"][len(include) :].startswith(
                    "<drf_format_suffix:"
                )
            )
        ]
        self.assertEqual(len(inherited), 12, [route["path"] for route in inherited])
        for route in inherited:
            self.assertEqual(route["params"].get("slug"), "str", route["path"])


class TestProductionPaths(GeneratorTestCase):
    """Only paths Caddyfile.ce sends to the API may be routed to Rust."""

    def test_api_matchers_come_from_the_production_caddyfile(self) -> None:
        self.assertEqual(
            gen_caddy.api_path_matchers(),
            ("/api/*", "/auth/*", "/static/*"),
        )

    def test_the_parser_ignores_the_frontend_and_bucket_upstreams(self) -> None:
        matchers = gen_caddy.api_path_matchers()
        for matcher in matchers:
            self.assertTrue(matcher.startswith("/"))
        # `/` (web), `/uploads/*` (MinIO in dev) and the frontend paths are not
        # in the list even though Caddyfile.ce reverse-proxies them.
        self.assertNotIn("/*", matchers)
        self.assertFalse(any("BUCKET" in matcher for matcher in matchers))

    def test_non_api_paths_are_refused(self) -> None:
        for order in (389, 390):  # `/robots.txt` (web) and `/` (health)
            route = self.by_order[order]
            self.assertIsNotNone(route, order)
            self.assertFalse(gen_caddy.is_api_path(route["path"], gen_caddy.api_path_matchers()))
            owners = self.owners_with((route, "rust"))
            with self.subTest(path=route["path"]):
                with self.assertRaises(SystemExit) as raised:
                    self.plan_for(owners)
                message = str(raised.exception)
                self.assertIn(route["path"], message)
                self.assertIn("api:8000", message)
                self.assertIn("hijack", message)

    def test_a_method_split_outside_the_api_is_refused_too(self) -> None:
        route = self.by_order[389]  # /robots.txt
        with self.assertRaises(SystemExit) as raised:
            self.plan_for(self.owners_with((route, "rust(TRACE)")))
        self.assertIn("/robots.txt", str(raised.exception))

    def test_a_custom_caddyfile_is_used_instead_of_the_committed_one(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            caddyfile = Path(tmp) / "Caddyfile.ce"
            caddyfile.write_text(
                "{\n\treverse_proxy /v2/* api:8000\n\treverse_proxy /* web:3000\n}\n",
                encoding="utf-8",
            )
            self.assertEqual(gen_caddy.api_path_matchers(caddyfile), ("/v2/*",))
            self.assertFalse(gen_caddy.is_api_path("/api/instances/", ("/v2/*",)))
            self.assertTrue(gen_caddy.is_api_path("/v2/instances/", ("/v2/*",)))

    def test_a_caddyfile_without_an_api_upstream_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            caddyfile = Path(tmp) / "Caddyfile.ce"
            caddyfile.write_text("{\n\treverse_proxy /* web:3000\n}\n", encoding="utf-8")
            with self.assertRaises(SystemExit) as raised:
                gen_caddy.api_path_matchers(caddyfile)
            self.assertIn("no `reverse_proxy", str(raised.exception))

    def test_path_matching(self) -> None:
        matchers = ("/api/*", "/auth/*", "/static/*")
        self.assertTrue(gen_caddy.is_api_path("/api/instances/", matchers))
        self.assertTrue(gen_caddy.is_api_path("/api/", matchers))
        self.assertTrue(gen_caddy.is_api_path("/auth/sign-in/", matchers))
        self.assertTrue(gen_caddy.is_api_path("/static/css/style.css", matchers))
        self.assertFalse(gen_caddy.is_api_path("/", matchers))
        self.assertFalse(gen_caddy.is_api_path("/robots.txt", matchers))
        # No `/apifoo` false positive: the prefix keeps its trailing slash.
        self.assertFalse(gen_caddy.is_api_path("/apifoo/", matchers))
        # `/*` means everything, including `/`.
        self.assertTrue(gen_caddy.is_api_path("/", ("/*",)))


class TestRouteGroups(GeneratorTestCase):
    def test_every_route_belongs_to_exactly_one_group(self) -> None:
        by_order, by_group = gen_caddy.assign_groups(self.routes, self.groups)
        self.assertEqual(len(by_order), len(self.routes))
        self.assertEqual(set(by_order), {route["order"] for route in self.routes})
        assigned = [route["order"] for routes in by_group.values() for route in routes]
        self.assertEqual(sorted(assigned), sorted(by_order))

    def test_every_group_selects_something(self) -> None:
        _by_order, by_group = gen_caddy.assign_groups(self.routes, self.groups)
        for group in self.groups:
            self.assertIn(group.name, by_group, f"group {group.name!r} selects no routes")

    def test_exact_selectors_exist_in_the_inventory(self) -> None:
        keys = {(route["group"], route["path"]) for route in self.routes}
        for group in self.groups:
            for key in group.routes:
                self.assertIn(key, keys, f"{group.name} selects unknown route {key}")

    def test_specific_nested_routes_win_over_the_project_umbrella(self) -> None:
        by_order, _by_group = gen_caddy.assign_groups(self.routes, self.groups)
        expected = {
            "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/": "states",
            "/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/": "labels",
            "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/": "issues",
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/": "cycles",
            "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/": "modules",
            "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/": "pages",
            "/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/": "intake",
            "/api/workspaces/{slug}/projects/{project_id}/views/{pk}/": "views",
            "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/": "estimates",
            "/api/workspaces/{slug}/projects/{pk}/": "projects",
            "/api/workspaces/{slug}/members/{pk}/": "workspaces",
            "/api/workspaces/{slug}/users/notifications/{pk}/": "notifications",
            "/api/users/me/notification-preferences/": "notifications",
            "/api/users/me/": "users-me",
            "/api/workspaces/{slug}/file-assets/": "assets",
            "/api/workspaces/{slug}/stickies/{pk}/": "stickies",
            "/api/workspaces/{slug}/views/{pk}/": "views",
        }
        routes_by_path = {route["path"]: route for route in self.routes}
        for path, group_name in expected.items():
            route = routes_by_path[path]
            self.assertEqual(by_order[route["order"]].name, group_name, path)

    def test_glob_semantics(self) -> None:
        self.assertTrue(gen_caddy.glob_matches("/api/*/", "/api/x/"))
        self.assertFalse(gen_caddy.glob_matches("/api/*/", "/api/x/y/"))
        self.assertTrue(gen_caddy.glob_matches("/api/**", "/api/"))
        self.assertTrue(gen_caddy.glob_matches("/api/**", "/api/x/y/z/"))
        self.assertFalse(gen_caddy.glob_matches("/api/*/issues/**", "/api/x/modules/y/"))
        self.assertFalse(gen_caddy.glob_matches("/api/**", "/other/x/"))
        # `**` matches zero or more trailing segments, so the parent itself is
        # less specific than a pattern naming the child.
        self.assertGreater(
            gen_caddy.selector_score("/api/workspaces/*/projects/*/states/**"),
            gen_caddy.selector_score("/api/workspaces/*/projects/**"),
        )

    def test_uncovered_route_is_an_error(self) -> None:
        groups = [
            group
            for group in self.groups
            if group.name not in ("health", "web")
        ]
        with self.assertRaises(SystemExit):
            gen_caddy.assign_groups(self.routes, groups)

    def test_ambiguous_pattern_is_an_error(self) -> None:
        groups = [
            gen_caddy.Group(name="a", description="", match=("/api/workspaces/*/issues/",)),
            gen_caddy.Group(name="b", description="", match=("/api/workspaces/*/issues/",)),
        ]
        with self.assertRaises(SystemExit):
            gen_caddy.assign_groups(self.routes, groups)


class TestOwnerCells(GeneratorTestCase):
    def test_committed_owners_are_all_django(self) -> None:
        values = {owner for owner, _status in self.committed_owners.values()}
        self.assertEqual(values, {"django"})

    def test_valid_cells(self) -> None:
        route = self.by_order[259]  # /api/instances/ -- GET, PATCH
        self.assertEqual(self.routes_module.parse_owner_cell("django"), ("django", None))
        self.assertEqual(self.routes_module.parse_owner_cell("rust"), ("rust", None))
        self.assertEqual(self.routes_module.parse_owner_cell("rust(GET)"), ("rust", ("GET",)))
        self.assertEqual(
            self.routes_module.parse_owner_cell("rust(GET,HEAD)"),
            ("rust", ("GET", "HEAD")),
        )
        self.assertEqual(
            self.routes_module.parse_owner_cell("rust( GET , PATCH )"),
            ("rust", ("GET", "PATCH")),
        )
        self.assertEqual(
            self.routes_module.validate_owner_cell("rust(GET)", route), ("rust", ("GET",))
        )
        self.assertEqual(
            self.routes_module.validate_owner_cell("rust(HEAD,OPTIONS)", route),
            ("rust", ("HEAD", "OPTIONS")),
        )

    def test_invalid_cells(self) -> None:
        invalid = [
            "",
            " ",
            "golang",
            "Rust",
            "RUST",
            "rust()",
            "rust(GET,)",
            "rust(,GET)",
            "rust(GET,,HEAD)",
            "rust(get)",
            "rust(Get)",
            "rust(GET,FOO)",
            "rust(GET HEAD)",
            "rust(GET",
            "rust GET",
            "django(GET)",
            "django()",
        ]
        for cell in invalid:
            with self.subTest(cell=cell):
                with self.assertRaises(self.routes_module.OwnerCellError):
                    self.routes_module.parse_owner_cell(cell)

    def test_methods_the_route_does_not_have_are_rejected(self) -> None:
        route = self.by_order[259]  # GET, PATCH only
        for cell in ("rust(POST)", "rust(DELETE)", "rust(TRACE)", "rust(GET,POST)"):
            with self.subTest(cell=cell):
                with self.assertRaises(self.routes_module.OwnerCellError):
                    self.routes_module.validate_owner_cell(cell, route)

    def test_implicit_methods_count_as_methods_the_route_has(self) -> None:
        # DRF answers HEAD for every GET and OPTIONS for every APIView.
        route = self.by_order[259]
        self.assertEqual(
            self.routes_module.validate_owner_cell("rust(HEAD,OPTIONS)", route),
            ("rust", ("HEAD", "OPTIONS")),
        )

    def test_unreachable_duplicate_is_skipped(self) -> None:
        # The duplicate DRF api-root shares (group, path, view) with the entry
        # Django serves, so an Owner cell cannot distinguish them. Marking the
        # row rust must produce one rule for the reachable entry, not two.
        unreachable = [route for route in self.routes if is_unreachable(route)]
        self.assertTrue(unreachable, "the inventory should still have the duplicate api-root")
        route = unreachable[0]
        owners = self.owners_with((route, "rust"))
        plan = self.group_plan(
            gen_caddy.build_plan(self.routes, owners, self.groups, self.routes_module),
            "v1",
        )
        regexes = [regex for cluster in plan.clusters for regex in cluster.regexes]
        self.assertEqual(regexes.count("^/api/v1/workspaces/[^/]+/$"), 1)
        # It has the same URL as the entry Django serves, so the generated
        # regex matches it too; there is just no separate rule for it.
        for url in route_samples(route):
            self.assertTrue(any(re.fullmatch(regex, url) for regex in regexes))


class TestRegexes(GeneratorTestCase):
    def regex(self, path: str) -> str:
        for route in self.routes:
            if route["path"] == path:
                return gen_caddy.route_regex(route)
        raise AssertionError(f"unknown path {path}")

    def test_uuid_converter_is_lowercase_only(self) -> None:
        regex = self.regex("/api/instances/admins/{pk}/")
        self.assertEqual(
            regex,
            "^/api/instances/admins/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-"
            "[0-9a-f]{4}-[0-9a-f]{12}/$",
        )
        self.assertIsNotNone(re.fullmatch(regex, f"/api/instances/admins/{UUID_SAMPLE}/"))
        self.assertIsNone(
            re.fullmatch(regex, "/api/instances/admins/01234567-89AB-CDEF-0123-456789ABCDEF/")
        )

    def test_str_converter_does_not_cross_a_slash(self) -> None:
        regex = self.regex("/api/workspaces/{slug}/")
        self.assertIsNotNone(re.fullmatch(regex, "/api/workspaces/acme/"))
        self.assertIsNone(re.fullmatch(regex, "/api/workspaces/acme/more/"))

    def test_drf_format_suffix_path_route(self) -> None:
        regex = self.regex("/api/v1/workspaces/{slug}/.{format}")
        self.assertEqual(regex, "^/api/v1/workspaces/[^/]+/\\.[a-z0-9]+/?$")
        self.assertIsNotNone(re.fullmatch(regex, "/api/v1/workspaces/acme/.json"))
        self.assertIsNotNone(re.fullmatch(regex, "/api/v1/workspaces/acme/.json/"))
        self.assertIsNone(re.fullmatch(regex, "/api/v1/workspaces/acme/json"))

    def test_drf_format_suffix_regex_route(self) -> None:
        regex = self.regex("/api/v1/workspaces/{slug}/invitations/{pk}.{format}")
        self.assertEqual(
            regex,
            "^/api/v1/workspaces/[^/]+/invitations/[^/.]+\\.[a-z0-9]+/?$",
        )
        self.assertIsNotNone(
            re.fullmatch(regex, "/api/v1/workspaces/acme/invitations/abcd.json")
        )
        self.assertIsNotNone(
            re.fullmatch(regex, "/api/v1/workspaces/acme/invitations/abcd.json/")
        )
        self.assertIsNone(
            re.fullmatch(regex, "/api/v1/workspaces/acme/invitations/abcd/ef.json")
        )

    def test_robots_txt_has_no_trailing_slash(self) -> None:
        regex = self.regex("/robots.txt")
        self.assertEqual(regex, "^/robots\\.txt$")
        self.assertIsNotNone(re.fullmatch(regex, "/robots.txt"))
        self.assertIsNone(re.fullmatch(regex, "/robots.txt/"))

    def test_catch_all_routes(self) -> None:
        self.assertEqual(self.regex("/"), "^/$")


class TestSampleUrls(GeneratorTestCase):
    """The per-route property the proxy depends on."""

    def test_every_route_matches_its_own_group_and_nothing_else(self) -> None:
        matchers = gen_caddy.api_path_matchers()
        reachable = [route for route in self.routes if not is_unreachable(route)]
        routable = [
            route for route in reachable if gen_caddy.is_api_path(route["path"], matchers)
        ]
        # The only entries the production proxy never sends to the API: the
        # health check at `/` and `robots.txt`, both served by the web app.
        routable_orders = {route["order"] for route in routable}
        self.assertEqual(
            sorted(
                route["path"]
                for route in reachable
                if route["order"] not in routable_orders
            ),
            ["/", "/robots.txt"],
        )
        samples = {route["order"]: route_samples(route) for route in routable}
        by_order, _by_group = gen_caddy.assign_groups(self.routes, self.groups)

        for route in routable:
            group_name = by_order[route["order"]].name
            owners = self.owners_with((route, "rust"))
            plan = self.group_plan(self.plan_for(owners), group_name)
            regexes = [
                regex for cluster in plan.clusters for regex in cluster.regexes
            ]
            self.assertTrue(regexes, f"{route['path']} produced no matcher")
            expected_methods = {
                method.upper() for method in self.routes_module.route_methods(route)
            }
            matched_methods = {
                method for cluster in plan.clusters for method in cluster.methods
            }
            self.assertEqual(
                matched_methods, expected_methods, f"{route['path']} method set"
            )

            for url in samples[route["order"]]:
                self.assertTrue(
                    any(re.fullmatch(regex, url) for regex in regexes),
                    f"{route['path']}: own regex {regexes} does not match {url!r}",
                )

            for other in routable:
                if other["order"] == route["order"]:
                    continue
                for url in samples[other["order"]]:
                    self.assertFalse(
                        any(re.fullmatch(regex, url) for regex in regexes),
                        f"{route['path']}: regex also matches {other['path']} "
                        f"via {url!r}",
                    )

    def test_unreachable_duplicate_shares_its_twin_regex(self) -> None:
        unreachable = [route for route in self.routes if is_unreachable(route)]
        for route in unreachable:
            twin = next(
                other
                for other in self.routes
                if other["group"] == route["group"]
                and other["path"] == route["path"]
                and not is_unreachable(other)
            )
            self.assertEqual(
                gen_caddy.route_regex(route), gen_caddy.route_regex(twin), route["path"]
            )


class TestMethodSplits(GeneratorTestCase):
    def test_single_route_split(self) -> None:
        route = self.by_order[259]  # /api/instances/ -- GET, PATCH
        plan = self.group_plan(self.plan_for(self.owners_with((route, "rust(GET)"))), "instances")
        self.assertEqual(len(plan.clusters), 1)
        cluster = plan.clusters[0]
        self.assertEqual(cluster.methods, ("GET",))
        self.assertEqual(cluster.regexes, ["^/api/instances/$"])
        # The PATCH half stays on Django and POST was never implemented.
        for method in ("POST", "PATCH", "DELETE"):
            self.assertNotIn(method, cluster.methods)

    def test_two_routes_in_one_group_with_different_method_sets(self) -> None:
        first = self.by_order[259]  # /api/instances/ GET, PATCH
        second = self.by_order[272]  # /api/instances/workspaces/ GET, POST
        owners = self.owners_with((first, "rust(GET)"), (second, "rust(POST)"))
        plan = self.group_plan(self.plan_for(owners), "instances")

        self.assertEqual(len(plan.clusters), 2)
        by_methods = {cluster.methods: cluster for cluster in plan.clusters}
        self.assertEqual(set(by_methods), {("GET",), ("POST",)})

        get_cluster = by_methods[("GET",)]
        post_cluster = by_methods[("POST",)]
        self.assertEqual(get_cluster.regexes, ["^/api/instances/$"])
        self.assertEqual(post_cluster.regexes, ["^/api/instances/workspaces/$"])

        # GET on the workspaces route is Django's, POST on /api/instances/ too.
        self.assertEqual(self.clusters_matching(plan, "/api/instances/workspaces/"), [post_cluster])
        self.assertEqual(self.clusters_matching(plan, "/api/instances/"), [get_cluster])

    def test_whole_row_and_split_coexist(self) -> None:
        whole = self.by_order[259]  # /api/instances/ GET, PATCH
        split = self.by_order[272]  # /api/instances/workspaces/ GET, POST
        owners = self.owners_with((whole, "rust"), (split, "rust(POST)"))
        plan = self.group_plan(self.plan_for(owners), "instances")

        self.assertEqual(len(plan.clusters), 2)
        methods = {cluster.methods for cluster in plan.clusters}
        self.assertEqual(methods, {("GET", "HEAD", "OPTIONS", "PATCH"), ("POST",)})
        get_cluster = next(c for c in plan.clusters if "GET" in c.methods)
        post_cluster = next(c for c in plan.clusters if c.methods == ("POST",))
        self.assertEqual(get_cluster.regexes, ["^/api/instances/$"])
        self.assertEqual(post_cluster.regexes, ["^/api/instances/workspaces/$"])

    def test_method_split_can_cover_a_route_django_never_implements(self) -> None:
        # `/api/instances/` registers GET and PATCH; HEAD is implicit, and a
        # split may name only one of the methods the row answers.
        route = self.by_order[259]
        plan = self.group_plan(self.plan_for(self.owners_with((route, "rust(PATCH)"))), "instances")
        self.assertEqual(plan.clusters[0].methods, ("PATCH",))
        self.assertIn('method("PATCH")', "\n".join(gen_caddy.render_expression(plan)))


class TestRendering(GeneratorTestCase):
    def render(self, owners: dict, inventory: dict | None = None) -> str:
        plans = self.plan_for(owners)
        return gen_caddy.render_snippet(plans, inventory or self.inventory)

    def test_committed_snippet_is_current(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(
                gen_caddy.main(["--check"]),
                0,
                "apps/server/deploy/caddy/rust-routes.caddy is stale",
            )

    def test_empty_when_everything_is_django(self) -> None:
        text = self.render(self.committed_owners)
        self.assertNotRegex(text, r"(?m)^@plane_rs_")
        self.assertNotRegex(text, r"(?m)^handle @plane_rs_")
        self.assertIn("adds no matcher", text)

    def test_matcher_and_handle_shape(self) -> None:
        route = self.by_order[259]
        text = self.render(self.owners_with((route, "rust(GET)")))
        self.assertIn("@plane_rs_instances {", text)
        self.assertIn("expression <<CEL", text)
        self.assertIn(
            '({env.PLANE_RS_GROUP_INSTANCES} == "" ? "rust" : '
            '{env.PLANE_RS_GROUP_INSTANCES}) == "rust"',
            text,
        )
        self.assertIn('(method("GET") && (path_regexp(r"^/api/instances/$")))', text)
        self.assertIn("handle @plane_rs_instances {", text)
        self.assertIn(
            "reverse_proxy {$PLANE_RS_API_UPSTREAM:plane-api:8000}", text
        )
        # The explanation of when the flag is read is part of the file.
        self.assertIn("read per", text)
        self.assertIn("{env.*}", text)
        self.assertIn("{$...}", text)

    def test_check_detects_a_stale_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "rust-routes.caddy"
            argv = ["--out", str(out)]
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gen_caddy.main(argv), 0)
                self.assertEqual(gen_caddy.main([*argv, "--check"]), 0)
            out.write_text(out.read_text(encoding="utf-8") + "# stale\n", encoding="utf-8")
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(gen_caddy.main([*argv, "--check"]), 1)

    def test_invalid_owner_cell_fails_generation(self) -> None:
        route = self.by_order[259]
        owners = self.owners_with((route, "heroku"))
        with self.assertRaises(SystemExit):
            self.plan_for(owners)


if __name__ == "__main__":
    unittest.main(verbosity=2)
