#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate `scenarios/policy-probe.yaml`.

The scenario drives **every** non-`/api/v1/` route method through the Rust DRF
runtime (built with `--features policy-probe`) and against Django, for every
fixture user. The harness's pass-through rule decides what is compared:

* a B response with `X-Plane-RS-Probe: handler` means the Rust policy let the
  request through; if Django denied it (401/403/405/406/415/429, including the
  `allow_permission` 403 body) the step is a DIFF, otherwise it is recorded as
  `reached-handler` and not compared;
* every other step is compared exactly, including its database, Redis and
  Celery diffs.

Ordering and isolation (why a permitted write cannot cascade):

1. **Read step first** (GET, one per user), then HEAD and OPTIONS, then the
   "missing object" variants, then all write steps. A write can therefore never
   change an object a **compared** step reads.
2. All writers are in the final block, so a mutation of A's database cannot
   reach a later compared read.
3. A **DELETE** step names a `pk`/object id that does not exist in the fixture.
   The permission classes still run (role-based deletes pass, creator-based
   deletes deny), but when the policy passes, Django's handler answers 404
   instead of deleting a row later steps still need. A successful delete is
   deliberately not exercised: any row it removed would make a later
   `creator`/membership check answer differently on A and B.
4. A POST/PUT/PATCH with an empty body cannot remove a row, and the columns the
   permission checks read (`created_by`, roles, membership rows) are never
   written by these routes with an empty payload. It may create rows; creation
   does not change any fixture id a later step references.
5. Plain `django.views.View` writes are CSRF-rejected on both backends before
   the view runs, because the scenario never sends a CSRF token.

The fixture (`gen_fixture.py`) supplies the users, sessions and objects; it is
applied to both target databases before the baseline snapshot.

Usage (from `apps/server`):

    apps/api/.venv/bin/python tools/policy/gen_policy_scenario.py
"""

from __future__ import annotations

import argparse
import json
import uuid
from pathlib import Path

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
REPO_ROOT = SCRIPT.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"
ROUTES = SERVER_DIR / "routes.json"
FIXTURE = SERVER_DIR / "scenarios" / "policy-fixture.json"
OUT = SERVER_DIR / "scenarios" / "policy-probe.yaml"

# The users, in the order their steps are emitted. `anon` has no cookie;
# `inactive` has a session whose user is inactive, so Django's `get_user`
# returns Anonymous and DRF's `_not_authenticated` replaces it (401).
USERS = [
    "anon",
    "nonmember",
    "ws_guest",
    "ws_member",
    "ws_admin",
    "pj_guest",
    "pj_member",
    "pj_admin",
    "inactive",
]

READ_METHODS = ("get",)
WRITE_METHODS = ("post", "put", "patch", "delete")
ALL_METHODS = ("get", "post", "put", "patch", "delete")


_AMBIGUOUS: dict[str, bool] = {}


def ambiguous_view(view: str) -> bool:
    """Whether the view's *handler* can emit an `allow_permission`-shaped 403.

    `plane/app/permissions/base.py` returns
    `{"error": "You don't have the required permissions."}`; fifteen handler
    sites reuse the exact string in their own authorization checks (asset,
    draft and project views). On those routes a 403 with that body cannot be
    attributed to the policy, so the probe would have to weaken its DIFF rule.
    Instead the generator does not emit write steps for them: the gap is
    explicit in the scenario header and in the Phase 2 report. Read methods on
    the same views are safe (their handlers use different bodies).
    """
    cached = _AMBIGUOUS.get(view)
    if cached is not None:
        return cached
    module = view.rsplit(".", 1)[0]
    path = API_DIR.joinpath(*module.split(".")).with_suffix(".py")
    ambiguous = False
    if path.is_file():
        text = path.read_text(encoding="utf-8")
        ambiguous = "You don't have the required permissions." in text
    _AMBIGUOUS[view] = ambiguous
    return ambiguous


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--name", default="policy-probe")
    parser.add_argument(
        "--limit",
        type=int,
        default=0,
        help="only emit the first N route entries (for a smoke run)",
    )
    args = parser.parse_args()

    routes = json.loads(ROUTES.read_text())["routes"]
    fixture = json.loads(FIXTURE.read_text())
    manifest = dict(fixture["objects"])
    manifest["projects"] = fixture["projects"]
    manifest["creator_objects"] = fixture["creator_objects"]
    manifest["throwaway_workspace_slug"] = fixture["throwaway_workspace_slug"]
    manifest["throwaway_project"] = fixture["throwaway_project"]
    manifest["throwaway_leave_workspace_slug"] = fixture["throwaway_leave_workspace_slug"]
    manifest["throwaway_leave_project"] = fixture["throwaway_leave_project"]
    sessions = fixture["session_keys"]
    users = {name: data for name, data in fixture["users"].items()}

    routes = [route for route in routes if route["group"] != "v1"]
    if args.limit:
        routes = routes[: args.limit]

    vars_: dict[str, str] = {
        "fixture_sql": "{repo}/apps/server/scenarios/policy-fixture.sql",
        "workspace_slug": fixture["workspace"]["slug"],
        "nonexistent": manifest["nonexistent"],
        "missing_slug": "no-such-workspace",
    }
    for name, data in users.items():
        # Fixture users are `policy_<role>`; the scenario calls them `<role>`.
        vars_[f"user_{name.removeprefix('policy_')}"] = data["id"]
    for name, key in sessions.items():
        vars_[f"session_{name}"] = key
    for name, value in manifest.items():
        if isinstance(value, str):
            vars_[name] = value
    for key, project in manifest["projects"].items():
        vars_[f"project_{key}"] = project["id"]
        vars_[f"project_{key}_identifier"] = project["identifier"]

    steps: list[dict] = []
    seen_names: set[str] = set()

    def add(
        name: str,
        method: str,
        path: str,
        *,
        user: str,
        kind: str,
        view: str = "",
        headers: dict[str, str] | None = None,
        query: str | None = None,
        ip: str | None = None,
    ) -> None:
        unique = name
        index = 2
        while unique in seen_names:
            unique = f"{name}-{index}"
            index += 1
        seen_names.add(unique)
        step: dict = {"name": unique, "method": method, "path": path}
        if query:
            step["query"] = query
        step["headers"] = step_headers(path, user, kind, ip)
        if headers:
            step["headers"].update(headers)
        steps.append(step)

    def step_headers(path: str, user: str, kind: str, ip: str | None) -> dict[str, str]:
        # Every request gets its own X-Forwarded-For so the anonymous throttle
        # (30/minute, one bucket per identity) never interferes. The throttle
        # block at the end deliberately reuses one address to trip it.
        headers = {
            "X-Forwarded-For": ip or f"198.51.100.{(len(steps) % 250) + 1}",
            "Accept": "application/json",
        }
        if user == "anon":
            return headers
        key = sessions.get(user)
        if key is None:
            return headers
        # `"instances" in request.path` selects the admin cookie (HTTP_CONTRACT
        # §5.1), a substring test, so `/api/instances/` needs the other name.
        cookie = "admin-session-id" if "instances" in path else "session-id"
        headers["Cookie"] = f"{cookie}={key}"
        if kind == "write":
            headers["Origin"] = "http://127.0.0.1:18080"
        return headers

    def substitute(route: dict, *, method: str, user: str) -> str:
        path = route["path"]
        creator = None
        for entry in route["method_permissions"].get(method, []):
            if entry.get("model"):
                creator = entry["model"]
                break
        values = param_values(route, method, manifest, creator)
        for name, value in values.items():
            path = path.replace("{" + name + "}", value)
        return path

    # ---- read block -----------------------------------------------------
    for route in routes:
        if "get" not in route["methods"]:
            continue
        for user in USERS:
            add(
                f"read-{route['order']}-{user}",
                "GET",
                substitute(route, method="get", user=user),
                user=user,
                kind="read",
                view=route["view"],
            )
        # HEAD is implicit for a GET (`View.setup` copies `get` onto `head`).
        # It is not a `routes.json` method, and the harness cannot decompress a
        # HEAD's gzip body (the header is present, the body is not), so HEAD is
        # covered by the unit tests and `skeleton.yaml` rather than here.

    # ---- OPTIONS (DRF metadata, plain View.options, or the function view) --
    for route in routes:
        add(
            f"options-{route['order']}-anon",
            "OPTIONS",
            substitute(route, method="get" if "get" in route["methods"] else method_of(route), user="anon"),
            user="anon",
            kind="read",
            view=route["view"],
        )
        # `determine_actions` calls `get_object()` for PUT; those routes are
        # left to the unit tests (the recorded `actions` cannot know whether the
        # object exists). A POST attempt is reproduced exactly.
        # A serializer field named like a volatile JSON key (`token`,
        # `username`, `background_color`, …) is a descriptor object in the
        # metadata body, and the normalisation rule for that key expects a
        # string. Rather than weaken the rule, do not compare those OPTIONS
        # bodies; the anonymous step still checks 401 and the metadata shape is
        # unit-tested.
        volatile_fields = ("token", "username", "background_color", "csrf_token", "epoch")
        has_volatile_field = any(
            f'"{name}":' in text
            for text in route["options_metadata"]["actions_json"].values()
            for name in volatile_fields
        )
        if "PUT" not in route["options_metadata"]["attempts"] and not has_volatile_field:
            add(
                f"options-{route['order']}-admin",
                "OPTIONS",
                substitute(route, method="get" if "get" in route["methods"] else method_of(route), user="ws_admin"),
                user="ws_admin",
                kind="read",
                view=route["view"],
            )

    # ---- missing-object variants ---------------------------------------
    for route in routes:
        if "get" not in route["methods"]:
            continue
        if "slug" in route["params"]:
            path = substitute(route, method="get", user="ws_admin").replace(
                "/workspaces/" + vars_["workspace_slug"] + "/", "/workspaces/{{missing_slug}}/"
            )
            add(
                f"missing-slug-{route['order']}",
                "GET",
                path,
                user="ws_admin",
                kind="read",
                view=route["view"],
            )
        if "project_id" in route["params"]:
            path = substitute(route, method="get", user="ws_admin").replace(
                vars_["project_alpha"], "{{nonexistent}}"
            )
            add(
                f"missing-project-{route['order']}",
                "GET",
                path,
                user="ws_admin",
                kind="read",
                view=route["view"],
            )

    # ---- negotiation errors --------------------------------------------
    add(
        "not-acceptable",
        "GET",
        "/api/users/me/",
        user="ws_admin",
        kind="read",
        headers={"Accept": "text/xml"},
    )
    add(
        "format-404",
        "GET",
        "/api/users/me/",
        user="ws_admin",
        kind="read",
        query="format=xml",
    )
    add(
        "format-json",
        "GET",
        "/api/users/me/",
        user="ws_admin",
        kind="read",
        query="format=json",
    )

    # ---- wrong-method 405 ----------------------------------------------
    for route in routes:
        methods = route["methods"]
        if "get" in methods and "patch" in methods and "delete" not in methods:
            add(
                f"wrong-method-{route['order']}",
                "DELETE",
                substitute(route, method="patch", user="ws_admin"),
                user="ws_admin",
                kind="read",
                view=route["view"],
            )

    # ---- write block (last; see the module docstring) -------------------
    for route in routes:
        for method in WRITE_METHODS:
            if method not in route["methods"]:
                continue
            # Two routes can destroy shared state no later step can rebuild:
            # `DELETE /api/users/me/` deactivates the acting user and their
            # memberships, and `DELETE /api/workspaces/{slug}/` deletes the
            # workspace. They are exercised only with callers that cannot
            # succeed (anonymous, a non-member) and with an admin against a
            # disposable object created for this purpose.
            if ambiguous_view(route["view"]):
                # The handler duplicates the policy check with the same body;
                # a 403 cannot be attributed. Skipped, not weakened.
                continue
            deactivate = method == "delete" and route["path"] == "/api/users/me/"
            delete_workspace = (
                method == "delete" and route["path"] == "/api/workspaces/{slug}/"
            )
            # `members/leave/` deactivates the caller's own membership; the
            # later steps' membership checks would then differ. Exercise the
            # permitted path against the throwaway workspace/project only.
            leave_project = (
                method == "post"
                and route["path"].endswith("/members/leave/")
                and "/projects/" in route["path"]
            )
            leave_workspace = (
                method == "post"
                and route["path"] == "/api/workspaces/{slug}/members/leave/"
            )
            if leave_project or leave_workspace:
                slug = "{{workspace_slug}}"
                project = "{{project_alpha}}"
                if leave_project:
                    slug = "{{throwaway_leave_workspace_slug}}"
                    project = "{{throwaway_leave_project}}"
                else:
                    slug = "{{throwaway_leave_workspace_slug}}"
                disposable = (
                    "disposable_project_leave" if leave_project else "disposable_workspace_leave"
                )
                for user in ("anon", "nonmember", disposable):
                    path = substitute(route, method=method, user=user)
                    path = path.replace("{{workspace_slug}}", slug)
                    path = path.replace("{{project_alpha}}", project)
                    add(
                        f"write-{route['order']}-{method}-{user}",
                        method.upper(),
                        path,
                        user=user,
                        kind="write",
                        view=route["view"],
                    )
                continue
            if deactivate:
                for user in ("anon", "ws_admin"):
                    add(
                        f"write-{route['order']}-{method}-{user}",
                        method.upper(),
                        substitute(route, method=method, user=user),
                        user=user,
                        kind="write",
                        view=route["view"],
                    )
                # `policy_disposable` is used by no other step, so its
                # successful deactivation cannot cascade.
                add(
                    f"write-{route['order']}-{method}-disposable",
                    method.upper(),
                    substitute(route, method=method, user="ws_admin"),
                    user="disposable",
                    kind="write",
                    view=route["view"],
                )
                continue
            if delete_workspace:
                for user in ("anon", "nonmember", "ws_admin"):
                    # `substitute` writes the `{{workspace_slug}}` template
                    # placeholder, not its value; replace the placeholder.
                    path = substitute(route, method=method, user=user).replace(
                        "/workspaces/{{workspace_slug}}/",
                        "/workspaces/{{throwaway_workspace_slug}}/",
                    )
                    add(
                        f"write-{route['order']}-{method}-{user}",
                        method.upper(),
                        path,
                        user=user,
                        kind="write",
                        view=route["view"],
                    )
                continue
            for user in USERS:
                path = substitute(route, method=method, user=user)
                if method == "delete":
                    # Never remove a row a later step needs. The permission
                    # checks still run against the missing id.
                    path = path.replace(manifest["cycle"], "{{nonexistent}}")
                    path = path.replace(manifest["issue"], "{{nonexistent}}")
                    path = path.replace(manifest["issue_comment"], "{{nonexistent}}")
                    for key in (
                        "module",
                        "view",
                        "page",
                        "label",
                        "state",
                        "intake",
                        "estimate",
                        "estimate_point",
                        "sticky",
                        "draft",
                        "file_asset",
                    ):
                        path = path.replace(manifest[key], "{{nonexistent}}")
                add(
                    f"write-{route['order']}-{method}-{user}",
                    method.upper(),
                    path,
                    user=user,
                    kind="write",
                    view=route["view"],
                )

    # ---- throttle block -------------------------------------------------
    # One address per class, reused deliberately, after every other step.
    asset = manifest["file_asset"]
    for index in range(31):
        add(
            f"throttle-anon-{index}",
            "GET",
            f"/api/assets/v2/static/{asset}/",
            user="anon",
            kind="read",
            ip="203.0.113.250",
        )
    for index in range(12):
        add(
            f"throttle-authentication-{index}",
            "POST",
            "/auth/email-check/",
            user="anon",
            kind="read",
            ip="203.0.113.251",
        )
    code_route = next(
        (
            route
            for route in routes
            if "generate_email_verification_code" in route["methods"].values()
        ),
        None,
    )
    if code_route is not None:
        for user in ("ws_admin",):
            for index in range(5):
                add(
                    f"throttle-email-{index}",
                    "POST",
                    substitute(code_route, method="post", user=user),
                    user=user,
                    kind="read",
                    view=code_route["view"],
                    ip="203.0.113.252",
                )
    lines = [
        "# Copyright (c) 2023-present Plane Software, Inc. and contributors",
        "# SPDX-License-Identifier: AGPL-3.0-only",
        "# See the LICENSE file for details.",
        "#",
        "# GENERATED by apps/server/tools/policy/gen_policy_scenario.py. Do not edit.",
        "#",
        "# Run with the policy-probe build:",
        "#   cargo build -p plane-api --features policy-probe",
        f"#   ./target/debug/difftest run --scenario {args.out.resolve().relative_to(SERVER_DIR)} --a django --b rust",
        "#",
        "# Write steps are omitted for views whose handler re-implements an",
        "# authorization check with the same 403 body `allow_permission` returns",
        "# (asset, draft and project views); a 403 there cannot be attributed to",
        "# the policy. See tools/policy/gen_policy_scenario.py:ambiguous_view.",
        f"name: {args.name}",
        "template_db: plane_tpl",
        "redis_db: [11, 12]",
        "vars:",
    ]
    for key in sorted(vars_):
        lines.append(f"  {key}: {yaml_scalar(vars_[key])}")
    lines.append("steps:")
    for step in steps:
        lines.append(f"  - name: {yaml_scalar(step['name'])}")
        lines.append(f"    method: {step['method']}")
        lines.append(f"    path: {yaml_scalar(step['path'])}")
        if "query" in step:
            lines.append(f"    query: {yaml_scalar(step['query'])}")
        lines.append("    headers:")
        for key in sorted(step["headers"]):
            lines.append(f"      {key}: {yaml_scalar(step['headers'][key])}")
    args.out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {args.out} ({len(steps)} steps, {len(routes)} routes)")
    return 0


def method_of(route: dict) -> str:
    for method in ALL_METHODS:
        if method in route["methods"]:
            return method
    return "get"


def param_values(
    route: dict, method: str, manifest: dict, creator: str | None
) -> dict[str, str]:
    """What each URL kwarg points at.

    The probe handler never sees these, so they exist to make the **permission**
    checks realistic: the workspace slug and project id decide membership, and a
    `creator=Model` decorator needs a row of that model whose `created_by` is the
    requesting user (the fixture creates every creator object as `ws_admin`).
    """
    values: dict[str, str] = {
        "slug": "{{workspace_slug}}",
        "project_id": "{{project_alpha}}",
        "project_identifier": "{{project_alpha_identifier}}",
        "issue_identifier": "1",
        "anchor": manifest["anchor_ok"],
        "reaction_code": "thumbs_up",
        "asset_key": "policy/probe.txt",
        "estimate_point_id": manifest["estimate_point"],
        "key": "1",
        "token": "probe-token",
        "uidb64": "cHJvYmU",
        "workspace_id": manifest["projects"]["alpha"]["id"],
    }
    named = {
        "issue_id": manifest["issue"],
        "work_item_id": manifest["issue"],
        "comment_id": manifest["issue_comment"],
        "cycle_id": manifest["cycle"],
        "module_id": manifest["module"],
        "view_id": manifest["view"],
        "page_id": manifest["page"],
        "label_id": manifest["label"],
        "state_id": manifest["state"],
        "intake_id": manifest["intake"],
        "estimate_id": manifest["estimate"],
        "estimate_point_id": manifest["estimate_point"],
        "asset_id": manifest["file_asset"],
        "draft_id": manifest["draft"],
        "favorite_id": manifest["cycle"],
        "member_id": manifest["member"],
        "subscriber_id": manifest["member"],
        "user_id": "{{user_ws_admin}}",
        "webhook_id": manifest["nonexistent"],
        "analytic_id": manifest["nonexistent"],
        "entity_id": manifest["issue"],
        "pk": manifest["nonexistent"],
    }
    for param in route["params"]:
        if param in named:
            values[param] = named[param]
    if creator is not None:
        creator_id = manifest["creator_objects"].get(creator)
        if creator_id:
            for candidate in ("pk", "issue_id", "comment_id", "view_id", "cycle_id", "module_id", "asset_id", "draft_id"):
                if candidate in route["params"]:
                    values[candidate] = creator_id
                    break
    if method == "delete":
        for candidate in ("pk", "issue_id", "comment_id", "cycle_id", "module_id", "view_id", "page_id",
                          "label_id", "state_id", "intake_id", "estimate_id", "estimate_point_id",
                          "asset_id", "draft_id", "sticky_id"):
            if candidate in route["params"]:
                values[candidate] = manifest["nonexistent"]
                break
    # A stable but nonexistent identifier: both backends must produce the same
    # 404/membership answer.
    for param in route["params"]:
        values.setdefault(param, manifest["nonexistent"])
    return values


def yaml_scalar(value: str) -> str:
    if value == "" or any(character in value for character in ":#{}[],&*?|-<>=!%@`\"'\n"):
        return json.dumps(value)
    return value


if __name__ == "__main__":
    raise SystemExit(main())
