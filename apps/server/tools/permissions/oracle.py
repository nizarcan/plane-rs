# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Evaluate the real Django permission classes against the fixture.

Usage (from the repository root):

    PLANE_DB=t_perms_fixture apps/server/tools/permissions/run.sh \\
        apps/api/.venv/bin/python apps/server/tools/permissions/oracle.py

The oracle is the specification for ``crates/plane-permissions``: it imports
the *actual* classes from the dotted paths in ``routes.json`` and the actual
``allow_permission`` decorators, builds DRF ``Request`` objects with
``APIRequestFactory``, and writes the result matrix to
``crates/plane-permissions/tests/golden/matrix.json``. The Rust integration
test replays the same cases and asserts equality; the oracle must never be
changed to match Rust.

Cases
-----
``class``
    One permission class from ``permission_classes`` against one URL-kwargs
    context, for every user and every HTTP method.
``spec``
    One distinct ``allow_permission`` spec found in ``routes.json`` (roles,
    level, creator, model) against the contexts in ``common.SPEC_BASE_CONTEXTS``
    (plus the creator contexts when ``creator=True``), every user and method.
``helper``
    ``issue_hidden_from_guest`` on the issue contexts.
``dynamic``
    A ``get_permissions`` override: the resolved class list for one
    (view, method, action). Independent of the user.
``check``
    DRF's ``APIView.check_permissions`` loop over one distinct
    ``permission_classes`` list, which pins the order and the
    401-vs-403 decision (``NotAuthenticated`` when no authenticator succeeded).
"""

from __future__ import annotations

import json
import sys
from collections import OrderedDict
from pathlib import Path

import common

HERE = Path(__file__).resolve().parent
SERVER_DIR = HERE.parents[1]
ROUTES_JSON = SERVER_DIR / "routes.json"
GOLDEN = SERVER_DIR / "crates" / "plane-permissions" / "tests" / "golden"
MATRIX_JSON = GOLDEN / "matrix.json"


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


# ---------------------------------------------------------------------------
# routes.json: the distinct classes and allow_permission specs
# ---------------------------------------------------------------------------


def load_routes() -> list[dict]:
    payload = json.loads(ROUTES_JSON.read_text())
    return payload["routes"]


def distinct_classes(routes: list[dict]) -> list[str]:
    seen: dict[str, None] = OrderedDict()
    for route in routes:
        for path in route.get("permission_classes") or []:
            seen.setdefault(path, None)
    return list(seen)


def distinct_specs(routes: list[dict]) -> list[dict]:
    seen: dict[tuple, dict] = OrderedDict()
    for route in routes:
        for specs in (route.get("method_permissions") or {}).values():
            for spec in specs:
                key = (
                    spec["decorator"],
                    tuple(spec["allowed_roles"]),
                    spec["level"],
                    bool(spec["creator"]),
                    spec["model"],
                )
                seen.setdefault(
                    key,
                    {
                        "roles": [common.SPEC_ROLES[name] for name in spec["allowed_roles"]],
                        "level": spec["level"],
                        "creator": bool(spec["creator"]),
                        "model": spec["model"],
                    },
                )
    return list(seen.values())


def distinct_permission_lists(routes: list[dict]) -> list[list[str]]:
    seen: dict[tuple, None] = OrderedDict()
    for route in routes:
        paths = route.get("permission_classes") or []
        if paths:
            seen.setdefault(tuple(paths), None)
    return [list(key) for key in seen]


# ---------------------------------------------------------------------------
# Contexts
# ---------------------------------------------------------------------------


def route_for_path(routes: list[dict], path: str) -> dict:
    """The DRF route that serves ``path`` (the one Django resolves)."""
    candidates = [route for route in routes if route["path"] == path]
    if not candidates:
        die(f"no route for context path {path!r}")
    for route in candidates:
        if route.get("drf_options"):
            return route
    return candidates[0]


def fill_path(template: str, values: dict[str, str]) -> str:
    filled = template
    while "{" in filled:
        start = filled.index("{")
        end = filled.index("}", start)
        name = filled[start + 1 : end]
        if name not in values:
            die(f"context path {template!r} needs {name!r}")
        filled = filled[:start] + str(values[name]) + filled[end + 1 :]
    return filled


def build_contexts(routes: list[dict]) -> dict[str, dict]:
    from django.urls import resolve

    contexts = {}
    for name, spec in common.CONTEXTS.items():
        route = route_for_path(routes, spec["route"])
        values = dict(spec.get("extra") or {})
        values.update(spec["kwargs"])
        path = fill_path(spec["route"], values)
        match = resolve(path)
        contexts[name] = {
            "kwargs": spec["kwargs"],
            "path": path,
            "url_name": match.url_name,
            "view": route["view"],
        }
    return contexts


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------


def main() -> None:
    import django
    from django.conf import settings

    try:
        django.setup()
    except Exception as error:  # pragma: no cover - environment mistake
        die(f"django.setup() failed ({error}); run through tools/permissions/run.sh")

    db_name = settings.DATABASES["default"]["NAME"]
    if not str(db_name).startswith("t_"):
        die(f"refusing to read database {db_name!r}: expected a t_* clone")

    from django.contrib.auth.models import AnonymousUser
    from django.utils.module_loading import import_string
    from rest_framework.exceptions import NotAuthenticated, PermissionDenied
    from rest_framework.request import Request
    from rest_framework.test import APIRequestFactory
    from rest_framework.views import APIView

    class CheckView(APIView):
        """Bare DRF view for ``check`` cases.

        The permission classes are real; this scaffolding only supplies the
        three view attributes Plane's classes read, copied verbatim from
        ``plane/api/views/base.py:138-149`` (identical in
        ``plane/app/views/base.py`` and ``plane/space/views/base.py``).
        """

        permission_classes = []

        def __init__(self, kwargs):
            super().__init__()
            self.kwargs = kwargs

        @property
        def workspace_slug(self):
            return self.kwargs.get("slug", None)

        @property
        def project_id(self):
            project_id = self.kwargs.get("project_id", None)
            if project_id:
                return project_id
            from django.urls import resolve

            if resolve(self.request.path_info).url_name == "project":
                return self.kwargs.get("pk", None)
            return None

        @property
        def project_identifier(self):
            return self.kwargs.get("project_identifier", None)

    from plane.app.permissions import ROLE as APP_ROLE
    from plane.app.permissions import allow_permission
    from plane.app.permissions.base import issue_hidden_from_guest
    from plane.db.models import User

    routes = load_routes()
    classes = distinct_classes(routes)
    specs = distinct_specs(routes)
    class_lists = distinct_permission_lists(routes)
    contexts = build_contexts(routes)

    users = {name: spec for name, spec in common.USERS.items() if spec["anonymous"] or spec["id"]}
    user_objects = {
        name: AnonymousUser() if spec["anonymous"] else User.objects.get(id=spec["id"])
        for name, spec in users.items()
    }

    factory = APIRequestFactory()

    # Requests are immutable for the checks (they only read ``method`` and
    # ``user``), so one per (context, method, user) is enough. The
    # ``authenticators`` tuple is non-empty because every DRF route in
    # routes.json configures one; that is what makes DRF answer 401 rather
    # than 403 for an anonymous caller.
    requests: dict[tuple[str, str, str], Request] = {}

    def make_request(context: str, method: str, user: str) -> Request:
        cache_key = (context, method, user)
        cached = requests.get(cache_key)
        if cached is not None:
            return cached
        django_request = factory.generic(method, contexts[context]["path"])
        request = Request(django_request, authenticators=(object(),))
        request.user = user_objects[user]
        # ``successful_authenticator`` reads ``_authenticator`` and would
        # otherwise call ``_authenticate()`` (an anonymous user has none). A
        # real request has been through ``initial()`` before a permission check,
        # so the attribute is always present by then.
        request._authenticator = object() if user_objects[user].is_authenticated else None
        requests[cache_key] = request
        return request

    view_classes = {name: import_string(context["view"]) for name, context in contexts.items()}
    views = {}

    def make_view(context: str, request: Request, action: str | None = None):
        if action is None:
            cached = views.get(context)
            if cached is not None:
                return cached
        view = view_classes[context]()
        view.request = request
        view.kwargs = dict(contexts[context]["kwargs"])
        if action is not None:
            view.action = action
        if action is None:
            views[context] = view
        return view

    # --- interned result details -------------------------------------------
    details: dict[str, list[dict]] = {}

    def intern(tag: str, detail: dict) -> str:
        bucket = details.setdefault(tag, [])
        if detail not in bucket:
            bucket.append(detail)
        return tag

    def record_exception(kind: str, subject, context: str, user: str, method: str, error: Exception) -> None:
        results[key(kind, subject, context, user, method, None)] = intern(
            f"exception:{type(error).__name__}",
            {"exception": type(error).__name__, "message": str(error)},
        )

    results: dict[str, object] = {}

    def key(kind: str, subject, context: str, user: str, method: str | None, action: str | None) -> str:
        return "|".join([kind, str(subject), context, user, method or "-", action or "-"])

    # --- get_permissions overrides -----------------------------------------
    dynamic = []
    for entry in common.DYNAMIC_VIEWS:
        owner_view = import_string(entry["view"])
        for method, action in entry["pairs"]:
            request = make_request(entry["context"], method, "user_admin")
            view = owner_view()
            view.request = request
            view.kwargs = dict(contexts[entry["context"]]["kwargs"])
            # DRF's ViewSetMixin sets ``action`` from the router's map; an
            # unmapped method (OPTIONS) leaves it None.
            view.action = action
            resolved = [
                type(permission).__module__ + "." + type(permission).__qualname__
                for permission in view.get_permissions()
            ]
            dynamic.append(
                {"view": entry["view"], "method": method, "action": action, "classes": resolved}
            )

    # The classes a `get_permissions` override resolves to are part of the
    # surface too (`ProjectAdminPermission` has no static route at all), so the
    # golden class list is the union.
    for entry in dynamic:
        for path in entry["classes"]:
            if path not in classes:
                classes.append(path)

    # --- class matrix -------------------------------------------------------
    permission_instances: dict[str, object] = {}

    def permission_instance(path: str):
        if path not in permission_instances:
            permission_instances[path] = import_string(path)()
        return permission_instances[path]

    for path in classes:
        if path not in common.CLASS_CONTEXTS:
            die(f"no matrix contexts for permission class {path!r} (add it to common.CLASS_CONTEXTS)")
    # An app/utils twin can be unused by every route (only the utils
    # `WorkspaceOwnerPermission` is: the app copy has no route), but it is still
    # evaluated so the two copies are pinned. A path with no sibling of the same
    # class name is a typo.
    used_names = {path.rsplit(".", 1)[-1] for path in classes}
    for path in common.CLASS_CONTEXTS:
        if path not in classes and path.rsplit(".", 1)[-1] not in used_names:
            die(f"matrix contexts declared for {path!r}, which is not a permission class in routes.json")

    # Every view whose `get_permissions` overrides the class attribute must be
    # modelled by `dynamic_permission_classes` in the Rust crate.
    recorded_dynamic = {
        route["dynamic_owners"]["permissions"]
        for route in routes
        if route.get("dynamic_permissions") and route.get("dynamic_owners", {}).get("permissions")
    }
    modelled_dynamic = {entry["view"] for entry in common.DYNAMIC_VIEWS}
    if recorded_dynamic != modelled_dynamic:
        die(
            "dynamic permission owners disagree with common.DYNAMIC_VIEWS: "
            f"recorded {sorted(recorded_dynamic)}, modelled {sorted(modelled_dynamic)}"
        )

    for path, context_names in common.CLASS_CONTEXTS.items():
        permission = permission_instance(path)
        for context in context_names:
            for user in users:
                for method in common.METHODS:
                    request = make_request(context, method, user)
                    view = make_view(context, request)
                    try:
                        value = permission.has_permission(request, view)
                        results[key("class", path, context, user, method, None)] = bool(value)
                    except Exception as error:  # noqa: BLE001 - the oracle records everything
                        record_exception("class", path, context, user, method, error)

    # --- allow_permission matrix -------------------------------------------
    def dummy(instance, request, **kwargs):
        return "ALLOWED"

    decorated = []
    for spec in specs:
        roles = [APP_ROLE(role) for role in spec["roles"]] if spec["roles"] else []
        model = import_string(spec["model"]) if spec["model"] else None
        decorated.append(
            allow_permission(roles, level=spec["level"], creator=spec["creator"], model=model)(dummy)
        )

    for index, spec in enumerate(specs):
        context_names = list(common.SPEC_BASE_CONTEXTS)
        if spec["creator"]:
            context_names += common.SPEC_CREATOR_CONTEXTS
        for context in context_names:
            kwargs = contexts[context]["kwargs"]
            for user in users:
                for method in common.METHODS:
                    request = make_request(context, method, user)
                    case = key("spec", index, context, user, method, None)
                    try:
                        value = decorated[index](None, request, **kwargs)
                        if value == "ALLOWED":
                            results[case] = True
                        else:
                            results[case] = intern(
                                "deny", {"status": value.status_code, "body": value.data}
                            )
                    except Exception as error:  # noqa: BLE001
                        results[case] = intern(
                            f"exception:{type(error).__name__}",
                            {"exception": type(error).__name__, "message": str(error)},
                        )

    # --- issue_hidden_from_guest -------------------------------------------
    for context in common.HELPER_CONTEXTS:
        for user in users:
            for method in common.METHODS:
                request = make_request(context, method, user)
                kwargs = contexts[context]["kwargs"]
                case = key("helper", "issue_hidden_from_guest", context, user, method, None)
                try:
                    hidden = issue_hidden_from_guest(
                        request, kwargs["slug"], kwargs["project_id"], kwargs["pk"]
                    )
                    results[case] = bool(hidden)
                except Exception as error:  # noqa: BLE001
                    results[case] = intern(
                        f"exception:{type(error).__name__}",
                        {"exception": type(error).__name__, "message": str(error)},
                    )

    # --- APIView.check_permissions -----------------------------------------
    list_imports = [[import_string(path) for path in class_paths] for class_paths in class_lists]
    for list_index, class_paths in enumerate(class_lists):
        context = "blank"
        for route in routes:
            if (route.get("permission_classes") or []) == class_paths:
                params = set((route.get("params") or {}).keys())
                if "slug" in params:
                    context = "ws"
                elif "project_id" in params:
                    context = "proj"
                elif "workspace_id" in params:
                    context = "ws_id"
                break
        for user in users:
            for method in common.METHODS:
                request = make_request(context, method, user)
                view = CheckView(dict(contexts[context]["kwargs"]))
                view.request = request
                view.permission_classes = list_imports[list_index]
                case = key("check", list_index, context, user, method, None)
                try:
                    APIView.check_permissions(view, request)
                    results[case] = True
                except NotAuthenticated:
                    results[case] = intern(
                        "not_authenticated",
                        {
                            "exception": "NotAuthenticated",
                            "status": 401,
                            "detail": "Authentication credentials were not provided.",
                        },
                    )
                except PermissionDenied as error:
                    results[case] = intern(
                        "permission_denied",
                        {"exception": "PermissionDenied", "status": 403, "detail": str(error.detail)},
                    )
                except Exception as error:  # noqa: BLE001
                    record_exception("check", list_index, context, user, method, error)

    # ----------------------------------------------------------------------
    # Write the golden matrix
    # ----------------------------------------------------------------------
    GOLDEN.mkdir(parents=True, exist_ok=True)
    payload = {
        "database": db_name,
        "generated_by": "apps/server/tools/permissions/oracle.py",
        "users": {
            name: {
                "id": str(spec["id"]) if spec["id"] is not None else None,
                "anonymous": spec["anonymous"],
            }
            for name, spec in users.items()
        },
        "contexts": {
            name: {
                "kwargs": {k: str(v) for k, v in context["kwargs"].items()},
                "url_name": context["url_name"],
                "view": context["view"],
                "view_project_identifier": (
                    getattr(make_view(name, make_request(name, "GET", "anonymous")), "project_identifier", None)
                    if hasattr(view_classes[name], "project_identifier")
                    else None
                ),
            }
            for name, context in contexts.items()
        },
        "methods": common.METHODS,
        "classes": classes,
        "class_contexts": common.CLASS_CONTEXTS,
        "specs": specs,
        "spec_base_contexts": common.SPEC_BASE_CONTEXTS,
        "spec_creator_contexts": common.SPEC_CREATOR_CONTEXTS,
        "helper_contexts": common.HELPER_CONTEXTS,
        "class_lists": class_lists,
        "dynamic": dynamic,
        "details": details,
        "results": results,
    }
    MATRIX_JSON.write_text(json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n")

    # ----------------------------------------------------------------------
    # Summary
    # ----------------------------------------------------------------------
    counts: dict[str, int] = {}
    for case_key in results:
        kind = case_key.split("|", 1)[0]
        counts[kind] = counts.get(kind, 0) + 1
    print(f"matrix written to {MATRIX_JSON} ({MATRIX_JSON.stat().st_size / 1_000_000:.1f} MB)")
    for kind in sorted(counts):
        print(f"  {kind}: {counts[kind]}")
    print(f"  total: {len(results)}")
    print(f"  distinct detail tags: {len(details)}")
    for tag, bucket in sorted(details.items()):
        print(f"    {tag}: {json.dumps(bucket, sort_keys=True)}")


if __name__ == "__main__":
    main()
