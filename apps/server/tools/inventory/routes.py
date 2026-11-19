#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the route inventory from Django's live URL resolver.

Django is the specification for the Rust rewrite (see RUST_REWRITE.md). This
script walks ``django.urls.get_resolver()`` depth first, in resolution order,
and writes two artefacts:

* ``apps/server/routes.json`` -- the machine-readable inventory.
* ``apps/server/ROUTES.md``   -- the human-readable ownership table.

Both are deterministic: no timestamps and no unordered sets are written. The
only inputs are the Python code in ``apps/api``, the installed Django/DRF
versions and the environment (``ENABLE_DRF_SPECTACULAR`` changes the routes).

It expects to run under the already-synced ``apps/api/.venv``::

    cd apps/api
    DJANGO_SETTINGS_MODULE=plane.settings.production \
    SECRET_KEY=inventory \
    DATABASE_URL=postgresql://x:x@127.0.0.1:1/x \
    REDIS_URL=redis://127.0.0.1:1/0 \
    .venv/bin/python ../server/tools/inventory/routes.py

The defaults below make the shorter form work too, as long as the interpreter
is ``apps/api/.venv/bin/python``. Nothing in here opens a database or Redis
connection: URL resolution only imports modules and reads settings.

``ROUTES.md`` may be hand-edited in exactly two columns, ``Owner`` and
``Status``. Regeneration keeps those values for every route it already knows,
keyed on ``(group, path, view)``. Everything else is overwritten.

The ``Owner`` cell syntax is:

* ``django`` -- every method of the row is served by Django.
* ``rust`` -- every method of the row is served by Rust. That includes the
  implicit ``HEAD`` (for a ``GET``) and ``OPTIONS`` (for any DRF view) that
  Django answers without an explicitly registered handler.
* ``rust(GET,HEAD)`` -- only the listed methods are served by Rust; the rest of
  the row stays with Django. Methods are uppercase, comma separated, and must
  be methods the row has (``routes.json`` ``methods`` plus ``implicit_methods``).

Anything else is rejected, both here (before writing either artefact) and by
``apps/server/tools/proxy/gen_caddy.py``, which turns these cells into the
coexistence proxy configuration.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import uuid
from collections import OrderedDict, defaultdict
from enum import Enum
from itertools import combinations
from pathlib import Path
from typing import Any, Iterable

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
REPO_ROOT = SCRIPT_PATH.parents[4]

# Django is not in this tree: the oracle is `apps/api` at the UPSTREAM.toml pin,
# materialised by tools/upstream/oracle.sh. Its path is never hard-coded here.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()

HTTP_METHODS = ("get", "post", "put", "patch", "delete", "head", "options", "trace")
HTTP_METHODS_UPPER = tuple(method.upper() for method in HTTP_METHODS)

# Owner cell grammar, see the module docstring. ``rust(GET,HEAD)`` is the only
# form that carries a method list; ``django(GET)`` is not a thing.
OWNER_ENGINES = ("django", "rust")
OWNER_CELL_RE = re.compile(
    r"^(?P<engine>[a-z]+)(?:\((?P<methods>[^()]*)\))?$"
)

# Numbers quoted in RUST_REWRITE.md ("What exists today").
BRIEF_COUNTS = {
    "app": 234,
    "v1": 75,
    "public": 25,
    "auth": 37,
    "instances": 14,
}

GROUP_ORDER = ("app", "v1", "public", "auth", "instances", "web", "schema")
GROUP_PREFIX = {
    "app": "/api/",
    "v1": "/api/v1/",
    "public": "/api/public/",
    "auth": "/auth/",
    "instances": "/api/instances/",
    "web": "(root)",
    "schema": "/api/schema/",
}
GROUP_TITLE = {
    "app": "app (`/api/`)",
    "v1": "v1 (`/api/v1/`)",
    "public": "public (`/api/public/`)",
    "auth": "auth (`/auth/`)",
    "instances": "instances (`/api/instances/`)",
    "web": "web (`plane.web`)",
    "schema": "schema (drf-spectacular)",
}

ROUTE_TABLE_COLUMNS = ("#", "Path", "Methods", "View", "Permissions", "Owner", "Status")

# Plane's base classes, in the order they win an MRO scan. The value is the
# `base_family` string `plane-drf` keys its per-family behaviour on.
BASE_FAMILIES = {
    ("plane.app.views.base", "BaseViewSet"): "app.BaseViewSet",
    ("plane.app.views.base", "BaseAPIView"): "app.BaseAPIView",
    ("plane.api.views.base", "BaseViewSet"): "api.BaseViewSet",
    ("plane.api.views.base", "BaseAPIView"): "api.BaseAPIView",
    ("plane.space.views.base", "BaseViewSet"): "space.BaseViewSet",
    ("plane.space.views.base", "BaseAPIView"): "space.BaseAPIView",
    ("plane.license.api.views.base", "BaseAPIView"): "license.BaseAPIView",
}

# The hooks `plane-drf` must be told about when a concrete view overrides them:
# each one changes the request lifecycle the runtime reproduces.
LIFECYCLE_HOOKS = (
    "get_permissions",
    "get_authenticators",
    "get_throttles",
    "initial",
    "dispatch",
    "handle_exception",
    "finalize_response",
    "options",
)


# ---------------------------------------------------------------------------
# Django bootstrap
# ---------------------------------------------------------------------------


def setup_django() -> Any:
    """Import Django with production settings, without touching a service."""
    if str(API_DIR) not in sys.path:
        sys.path.insert(0, str(API_DIR))

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    os.environ.setdefault("SECRET_KEY", "inventory")
    os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
    os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")

    import django

    django.setup()
    return django


# ---------------------------------------------------------------------------
# Dotted names and small helpers
# ---------------------------------------------------------------------------


def dotted_callable(fn: Any) -> str:
    module = getattr(fn, "__module__", None) or "?"
    qualname = getattr(fn, "__qualname__", None) or getattr(fn, "__name__", None) or repr(fn)
    return f"{module}.{qualname}"


def dotted_class(cls: type) -> str:
    return f"{cls.__module__}.{cls.__qualname__}"


def short_name(dotted: str) -> str:
    return dotted.rsplit(".", 1)[-1]


def find_owner(cls: type, attribute: str) -> type | None:
    """Return the class in the MRO that first defines ``attribute``."""
    for klass in cls.__mro__:
        if attribute in vars(klass):
            return klass
    return None


def closure_env(fn: Any) -> dict[str, Any]:
    """Read a function's closure cells into a name -> value mapping."""
    env: dict[str, Any] = {}
    code = getattr(fn, "__code__", None)
    closure = getattr(fn, "__closure__", None) or ()
    if code is None:
        return env
    for name, cell in zip(code.co_freevars, closure):
        try:
            env[name] = cell.cell_contents
        except ValueError:  # cell is empty
            env[name] = None
    return env


def wrapper_chain(fn: Any) -> Iterable[Any]:
    """Yield ``fn`` and every wrapper reachable through ``__wrapped__``."""
    seen: set[int] = set()
    current = fn
    while current is not None and id(current) not in seen:
        seen.add(id(current))
        yield current
        current = getattr(current, "__wrapped__", None)


def role_name(role: Any) -> str:
    if isinstance(role, Enum):
        return role.name
    return str(role)


def dedupe(values: Iterable[str]) -> list[str]:
    return list(OrderedDict.fromkeys(values))


# ---------------------------------------------------------------------------
# URL pattern normalisation
# ---------------------------------------------------------------------------

ROUTE_PARAM_RE = re.compile(r"<(?:(?P<converter>[^>:]+):)?(?P<name>[^>]+)>")
REGEX_PARAM_RE = re.compile(r"\(\?P<([A-Za-z_]\w*)>([^()]*)\)")
PURE_PARAM_SEGMENT_RE = re.compile(r"^\{([^}]+)\}$")


def normalise_route_pattern(pattern: Any) -> str:
    """``workspaces/<str:slug>/`` -> ``workspaces/{slug}/``.

    Django also accepts a bare ``<name>`` (the default ``str`` converter); the
    project uses it for the password-reset tokens and estimate point ids.
    ``drf_format_suffix`` is special: its converter regex is ``\\.[a-z0-9]+/?``,
    so the literal dot belongs in the template (``.{format}``).
    """

    def replace(match: re.Match[str]) -> str:
        converter = match.group("converter")
        name = match.group("name")
        if converter == "drf_format_suffix":
            return ".{" + name + "}"
        return "{" + name + "}"

    return ROUTE_PARAM_RE.sub(replace, str(pattern))


def normalise_regex_pattern(source: str) -> tuple[str, OrderedDict[str, str], bool]:
    """Turn the small regexes DRF's router emits into a path template.

    Returns ``(template, params, optional_trailing_slash)``. ``params`` maps
    the group name to the regex body, because a regex group has no converter.
    """
    optional_trailing_slash = bool(re.search(r"/\?\$$", source))

    body = source
    if body.startswith("^"):
        body = body[1:]
    if body.endswith("$"):
        body = body[:-1]
    if body.endswith("/?"):
        body = body[:-2]

    params: OrderedDict[str, str] = OrderedDict()

    def replace(match: re.Match[str]) -> str:
        name, expression = match.group(1), match.group(2)
        params[name] = expression
        return "{" + name + "}"

    body = REGEX_PARAM_RE.sub(replace, body)
    body = body.replace(r"\.", ".").replace(r"\/", "/")
    return body, params, optional_trailing_slash


def converter_name(converter: Any) -> str:
    from django.urls.converters import get_converters

    registered = {type(value): name for name, value in get_converters().items()}
    name = registered.get(type(converter))
    if name is not None:
        return name
    class_name = type(converter).__name__
    if class_name == "FormatSuffixConverter":
        return "drf_format_suffix"
    return class_name


def group_for(path: str) -> str:
    if path.startswith("/api/schema/"):
        return "schema"
    if path.startswith("/api/public/"):
        return "public"
    if path.startswith("/api/instances/"):
        return "instances"
    if path.startswith("/api/v1/"):
        return "v1"
    if path.startswith("/api/"):
        return "app"
    if path.startswith("/auth/"):
        return "auth"
    return "web"


def segments(path: str) -> list[str]:
    if path == "/":
        return []
    return path.strip("/").split("/")


def segment_kinds(path: str) -> list[tuple[str, str]]:
    """Classify each segment as literal, param or mixed.

    ``mixed`` means the segment contains a ``{name}`` capture but also literal
    text, for example ``{project_identifier}-{issue_identifier}``.
    """
    result: list[tuple[str, str]] = []
    for segment in segments(path):
        match = PURE_PARAM_SEGMENT_RE.match(segment)
        if match:
            result.append(("param", match.group(1)))
        elif "{" in segment or "}" in segment:
            result.append(("mixed", segment))
        else:
            result.append(("literal", segment))
    return result


def mixed_segment_notes(path: str) -> list[str]:
    notes: list[str] = []
    for segment in segments(path):
        if "{" not in segment:
            continue
        if PURE_PARAM_SEGMENT_RE.match(segment):
            continue
        captures = re.findall(r"\{[^}]+\}", segment)
        if len(captures) > 1:
            notes.append(f"mixed segment {segment!r}: {len(captures)} parameters in one segment")
        else:
            notes.append(f"mixed segment {segment!r}: literal text plus a parameter")
    return notes


# ---------------------------------------------------------------------------
# View introspection
# ---------------------------------------------------------------------------


def view_class(callback: Any) -> type | None:
    """DRF's ``as_view`` sets ``cls``; Django's sets ``view_class``."""
    cls = getattr(callback, "cls", None)
    if cls is None:
        cls = getattr(callback, "view_class", None)
    return cls


def kind_of(cls: type | None) -> str:
    if cls is None:
        return "function"

    from django.views import View
    from rest_framework.views import APIView
    from rest_framework.viewsets import ViewSetMixin

    if issubclass(cls, ViewSetMixin):
        return "ViewSet"
    if issubclass(cls, APIView):
        from rest_framework.generics import GenericAPIView

        if issubclass(cls, GenericAPIView):
            return "GenericAPIView"
        return "APIView"
    if issubclass(cls, View):
        return "View"
    return "function"


def is_drf(kind: str) -> bool:
    return kind in ("APIView", "GenericAPIView", "ViewSet")


def init_kwargs(callback: Any) -> dict[str, Any]:
    kwargs = getattr(callback, "initkwargs", None)
    if kwargs is None:
        kwargs = getattr(callback, "view_initkwargs", None)
    return dict(kwargs or {})


def function_allowed_methods(callback: Any) -> list[str] | None:
    """Read Django's ``require_http_methods`` family from the wrapper chain."""
    for wrapper in wrapper_chain(callback):
        code = getattr(wrapper, "__code__", None)
        if code is None or "request_method_list" not in code.co_freevars:
            continue
        methods = closure_env(wrapper).get("request_method_list")
        if methods:
            return [str(method).lower() for method in methods]
    return None


def effective_methods(
    callback: Any, cls: type | None, kind: str
) -> tuple[OrderedDict[str, str], list[str], list[str] | None]:
    """Return ``(method -> handler/action, implicit methods, http_method_names)``."""
    kwargs = init_kwargs(callback)
    override = kwargs.get("http_method_names")

    if kind == "ViewSet":
        allowed = {str(m).lower() for m in (override or HTTP_METHODS)}
        actions = getattr(callback, "actions", None) or {}
        methods = OrderedDict(
            (str(method).lower(), action)
            for method, action in actions.items()
            if str(method).lower() in allowed
        )
    elif cls is not None:
        allowed = {str(m).lower() for m in (override or getattr(cls, "http_method_names", HTTP_METHODS))}
        methods = OrderedDict()
        for method in HTTP_METHODS:
            owner = find_owner(cls, method)
            if owner is None:
                continue
            # APIView defines ``options``; Django's View defines ``options`` and
            # copies ``get`` onto ``head``. Neither is an implemented handler.
            if method in ("head", "options") and (
                (owner.__module__ == "rest_framework.views" and owner.__name__ == "APIView")
                or (owner.__module__ == "django.views.generic.base" and owner.__name__ == "View")
            ):
                continue
            if method not in allowed:
                continue
            methods[method] = method
    else:
        declared = function_allowed_methods(callback)
        names = declared if declared is not None else list(HTTP_METHODS)
        handler_name = getattr(callback, "__name__", "view")
        methods = OrderedDict((method, handler_name) for method in names)

    implicit: list[str] = []
    if kind in ("APIView", "GenericAPIView", "ViewSet", "View"):
        implicit.append("options")
        if "get" in methods:
            implicit.insert(0, "head")

    return methods, implicit, list(override) if override else None


def normalise_allow_permission(env: dict[str, Any]) -> dict[str, Any]:
    roles = env.get("allowed_roles")
    if not isinstance(roles, (list, tuple, set, frozenset)):
        return {
            "decorator": "allow_permission",
            "unknown": True,
            "reason": "allowed_roles is not a sequence",
        }

    model = env.get("model")
    if isinstance(model, type):
        model_path: str | None = dotted_class(model)
    elif model is None:
        model_path = None
    else:
        model_path = str(model)

    level = env.get("level")
    if not isinstance(level, str):
        return {
            "decorator": "allow_permission",
            "unknown": True,
            "reason": "level is not a string",
        }

    return {
        "decorator": "allow_permission",
        "allowed_roles": dedupe(role_name(role) for role in roles),
        "level": level,
        "creator": bool(env.get("creator")),
        "model": model_path,
    }


def extract_method_decorators(
    fn: Any,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Extract permission and throttle decorators from a handler's wrappers."""
    if fn is None:
        return ([{"unknown": True, "reason": "handler not found"}], [])

    permissions: list[dict[str, Any]] = []
    throttles: list[dict[str, Any]] = []

    for wrapper in wrapper_chain(fn):
        code = getattr(wrapper, "__code__", None)
        if code is None:
            continue
        freevars = set(code.co_freevars)

        if {"allowed_roles", "level", "creator", "model"} <= freevars:
            permissions.append(normalise_allow_permission(closure_env(wrapper)))
        elif "allowed_roles" in freevars or any(
            "permission" in name.lower() or "role" in name.lower() for name in freevars
        ):
            permissions.append(
                {
                    "decorator": dotted_callable(wrapper),
                    "unknown": True,
                    "reason": "unrecognised permission decorator",
                }
            )

        if {"is_app", "is_space", "is_admin", "post_method"} <= freevars:
            env = closure_env(wrapper)
            throttles.append(
                {
                    "decorator": "throttle_auth_redirect",
                    "is_app": bool(env.get("is_app")),
                    "is_space": bool(env.get("is_space")),
                    "is_admin": bool(env.get("is_admin")),
                }
            )

    return (permissions, throttles)


def handler_for(cls: type | None, kind: str, method: str, action: str) -> Any:
    if cls is None:
        return None
    if kind == "ViewSet":
        return getattr(cls, action, None)
    return getattr(cls, method, None)


def is_project_implementation(cls: type, attribute: str) -> bool:
    """True when ``attribute`` comes from this repository, not from DRF."""
    owner = find_owner(cls, attribute)
    return owner is not None and not owner.__module__.startswith("rest_framework")


def viewset_guard(
    cls: type | None,
    kind: str,
    methods: OrderedDict[str, str],
    dynamic_permissions: bool,
) -> dict[str, Any]:
    """Statically evaluate ``plane.app.views.base.BaseViewSet.initial``.

    That guard refuses an action served straight from a DRF mixin when the
    viewset has no restrictive permission class and no project implementation
    of the action. The route still exists, but an authenticated request gets
    405. Only the app ``BaseViewSet`` carries this guard; the v1 and space
    bases do not.
    """
    if kind != "ViewSet" or cls is None or not hasattr(cls, "_resolved_action_is_authorized"):
        return {"status": "not-applicable", "refused_methods": []}
    if dynamic_permissions:
        # get_permissions() can return a restrictive class per action; a static
        # read of permission_classes would guess. Leave it to a runtime test.
        return {"status": "dynamic", "refused_methods": []}

    try:
        from plane.app.views.base import _MIXIN_PROVIDED_ACTIONS, _NON_AUTHORIZING_PERMISSIONS
    except Exception:  # pragma: no cover - the app module is always importable here
        return {"status": "unknown", "refused_methods": []}

    permissions = getattr(cls, "permission_classes", ())
    non_authorizing = all(permission in _NON_AUTHORIZING_PERMISSIONS for permission in permissions)

    refused: list[dict[str, str]] = []
    for method, action in methods.items():
        if action not in _MIXIN_PROVIDED_ACTIONS:
            continue
        if not non_authorizing:
            continue
        if action == "create" and is_project_implementation(cls, "perform_create"):
            continue
        if action == "partial_update" and is_project_implementation(cls, "update"):
            continue
        if is_project_implementation(cls, action):
            continue
        owner = find_owner(cls, action)
        refused.append(
            {
                "method": method,
                "action": action,
                "handler_owner": dotted_class(owner) if owner is not None else "",
            }
        )
    return {"status": "evaluated", "refused_methods": refused}


# ---------------------------------------------------------------------------
# Phase 2 introspection: base family, lifecycle hooks, OPTIONS metadata
# ---------------------------------------------------------------------------


def base_family(cls: type | None, kind: str) -> str:
    """Which of Plane's base classes (or which plain base) a view derives from.

    Values are the strings `plane-drf` matches on: `app.BaseViewSet`,
    `app.BaseAPIView`, `api.BaseViewSet`, `api.BaseAPIView`, `space.BaseViewSet`,
    `space.BaseAPIView`, `license.BaseAPIView`, `drf.ViewSet`, `drf.APIView`,
    `django.View` and `function`.
    """
    if cls is None:
        return "function"
    for klass in cls.__mro__:
        family = BASE_FAMILIES.get((klass.__module__, klass.__qualname__))
        if family is not None:
            return family
    if is_drf(kind):
        return "drf.ViewSet" if kind == "ViewSet" else "drf.APIView"
    from django.views import View

    if isinstance(cls, type) and issubclass(cls, View):
        return "django.View"
    return "function"


def family_base(cls: type | None, family: str) -> type | None:
    """The class `family` names, or `None` for a non-class view."""
    if cls is None:
        return None
    wanted = {
        "app.BaseViewSet": ("plane.app.views.base", "BaseViewSet"),
        "app.BaseAPIView": ("plane.app.views.base", "BaseAPIView"),
        "api.BaseViewSet": ("plane.api.views.base", "BaseViewSet"),
        "api.BaseAPIView": ("plane.api.views.base", "BaseAPIView"),
        "space.BaseViewSet": ("plane.space.views.base", "BaseViewSet"),
        "space.BaseAPIView": ("plane.space.views.base", "BaseAPIView"),
        "license.BaseAPIView": ("plane.license.api.views.base", "BaseAPIView"),
        "drf.ViewSet": ("rest_framework.viewsets", "ViewSet"),
        "drf.APIView": ("rest_framework.views", "APIView"),
        "django.View": ("django.views.generic.base", "View"),
    }.get(family)
    if wanted is None:
        return None
    for klass in cls.__mro__:
        if (klass.__module__, klass.__qualname__) == wanted:
            return klass
    return None


def lifecycle_overrides(cls: type | None, family: str) -> dict[str, str | None]:
    """Hooks a concrete view overrides *below* the family base class.

    `None` means the runtime's family implementation is what Django runs. A
    dotted class name means the concrete view (or a mixin between it and the
    family base) replaces that step and `plane-drf` must be given an explicit
    implementation at registration time.
    """
    overrides: dict[str, str | None] = {hook: None for hook in LIFECYCLE_HOOKS}
    if cls is None:
        return overrides
    base = family_base(cls, family)
    mro = cls.__mro__
    stop = mro.index(base) if base is not None else 0
    for hook in LIFECYCLE_HOOKS:
        for index, klass in enumerate(mro[:stop]):
            if hook in vars(klass):
                overrides[hook] = dotted_class(klass)
                break
    return overrides


def effective_http_method_names(cls: type | None, callback: Any) -> list[str] | None:
    """The effective `http_method_names`, or `None` for a function view."""
    if cls is None:
        return None
    override = init_kwargs(callback).get("http_method_names")
    names = override or getattr(cls, "http_method_names", HTTP_METHODS)
    return [str(method).lower() for method in names]


def class_list_attribute(cls: type | None, kind: str, attribute: str) -> list[str] | None:
    """A DRF class attribute resolved through `api_settings`, or `None`."""
    if cls is None or not is_drf(kind):
        return None
    from rest_framework.settings import api_settings

    defaults = {
        "renderer_classes": api_settings.DEFAULT_RENDERER_CLASSES,
        "parser_classes": api_settings.DEFAULT_PARSER_CLASSES,
        "authentication_classes": api_settings.DEFAULT_AUTHENTICATION_CLASSES,
        "throttle_classes": api_settings.DEFAULT_THROTTLE_CLASSES,
        "permission_classes": api_settings.DEFAULT_PERMISSION_CLASSES,
    }
    value = getattr(cls, attribute, defaults.get(attribute))
    if value is None:
        return None
    return dedupe(dotted_class(item) for item in value)


def class_scalar_attribute(cls: type | None, kind: str, attribute: str) -> str | None:
    """`content_negotiation_class` / `metadata_class` as a dotted path."""
    if cls is None or not is_drf(kind):
        return None
    from rest_framework.settings import api_settings

    defaults = {
        "content_negotiation_class": api_settings.DEFAULT_CONTENT_NEGOTIATION_CLASS,
        "metadata_class": api_settings.DEFAULT_METADATA_CLASS,
    }
    value = getattr(cls, attribute, defaults.get(attribute))
    if value is None:
        return None
    return dotted_class(value)


def view_metadata_instance(cls: type | None, kind: str, callback: Any) -> Any | None:
    """An instance of the view, built the way `as_view` builds it.

    `get_view_name`/`get_view_description` are instance methods; ViewSets get
    their `suffix` from the router's `as_view` initkwargs, so those matter.
    """
    if cls is None:
        return None
    initkwargs = init_kwargs(callback) if is_drf(kind) or kind == "View" else {}
    try:
        return cls(**initkwargs)
    except Exception:
        try:
            return cls()
        except Exception:
            return None


def view_names(cls: type | None, kind: str, callback: Any) -> dict[str, str | None]:
    """`get_view_name()` and `get_view_description()` exactly as DRF returns them."""
    if cls is None or not is_drf(kind):
        return {"name": None, "description": None}
    from rest_framework.views import get_view_description, get_view_name

    instance = view_metadata_instance(cls, kind, callback)
    if instance is None:
        return {"name": None, "description": None}
    try:
        name = get_view_name(instance)
    except Exception:
        name = None
    try:
        description = get_view_description(instance)
    except Exception:
        description = None
    return {"name": name, "description": description}


def allowed_methods(route_methods_map: dict[str, str], implicit: list[str], names: list[str] | None) -> list[str]:
    """`[m.upper() for m in http_method_names if hasattr(view, m)]`.

    A bound ViewSet action exists for every registered method, `head` exists
    whenever `get` does, and `APIView` always defines `options`.
    """
    if names is None:
        return []
    present = set(route_methods_map) | set(implicit)
    return [name.upper() for name in names if name in present]


def options_actions(
    cls: type | None,
    kind: str,
    callback: Any,
    methods: dict[str, str],
    implicit: list[str],
    names: list[str] | None,
) -> dict[str, Any]:
    """What `SimpleMetadata.determine_actions` returns for a permitted caller.

    The two runtime-dependent branches — `check_permissions(clone_request(...))`
    and, for `PUT`, `get_object()` — are bypassed here so the serializer field
    map can be recorded statically; `plane-drf` performs the permission check
    itself at request time. The `get_object()` branch cannot be reproduced
    without a database, so a route whose `PUT` is allowed records
    `get_object_required: true` and the runtime treats its PUT actions as
    runtime-dependent (see ROUTES.md, "OPTIONS metadata").

    A serializer that cannot be instantiated without a request raises inside
    `determine_actions`, so Django answers that OPTIONS with the family 500;
    the exact exception is recorded in `error`.
    """
    # `actions_json` holds each field map as compact `json.dumps` text, not as a
    # nested object: the Rust policy reader must preserve the serializer's field
    # order byte for byte, and `serde_json` without `preserve_order` would sort
    # it. The runtime splices the text into the metadata body.
    result: dict[str, Any] = {
        "attempts": [],
        "actions_json": {},
        "error": None,
        "get_object_required": False,
    }
    if cls is None or not is_drf(kind) or not hasattr(cls, "get_serializer"):
        return result
    from rest_framework.metadata import SimpleMetadata

    attempts = [method for method in ("POST", "PUT") if method in allowed_methods(methods, implicit, names)]
    result["attempts"] = attempts
    if not attempts:
        return result
    instance = view_metadata_instance(cls, kind, callback)
    if instance is None:
        result["error"] = "could not instantiate the view"
        return result
    metadata = SimpleMetadata()
    # `initial()` sets these before `options()` runs; serializers read
    # `self.action` (via `get_serializer_class`) and `format_kwarg`.
    instance.format_kwarg = None
    instance.action = "metadata"
    instance.request = None
    for method in attempts:
        if method == "PUT":
            # `determine_actions` calls `view.get_object()` for PUT. Recorded as
            # runtime-dependent rather than guessed.
            result["get_object_required"] = True
        try:
            serializer = instance.get_serializer()
            result["actions_json"][method] = json.dumps(
                metadata.get_serializer_info(serializer),
                ensure_ascii=False,
                separators=(",", ":"),
            )
        except Exception as exc:  # noqa: BLE001 - the exception text is recorded
            if result["error"] is None:
                result["error"] = f"{type(exc).__name__}: {exc}"
    return result


def guard_actions(
    cls: type | None,
    kind: str,
    methods: dict[str, str],
) -> dict[str, dict[str, Any]]:
    """Per action: who implements it, for the runtime `BaseViewSet` guard.

    `BaseViewSet.initial` refuses a mixin-served action when the *effective*
    permissions authorize nothing. `routes.json` cannot always decide that
    statically (`get_permissions` may be overridden), so the runtime evaluates
    the rule with the permissions it resolved for the request. These fields are
    the action half of the rule and do not depend on permissions.
    """
    if cls is None or kind != "ViewSet" or not hasattr(cls, "_resolved_action_is_authorized"):
        return {}
    from plane.app.views.base import _MIXIN_PROVIDED_ACTIONS

    actions: dict[str, dict[str, Any]] = {}
    for action in dedupe(methods.values()):
        owner = find_owner(cls, action)
        actions[action] = {
            "owner": dotted_class(owner) if owner is not None else "",
            "mixin_provided": action in _MIXIN_PROVIDED_ACTIONS,
            "project_implemented": is_project_implementation(cls, action),
            "project_implements_create": action == "create"
            and is_project_implementation(cls, "perform_create"),
            "project_implements_update": action == "partial_update"
            and is_project_implementation(cls, "update"),
        }
    return actions


def defined_methods(cls: type | None, kind: str, names: list[str] | None) -> list[str] | None:
    """For a plain `django.View`: the method handlers the class itself defines."""
    if cls is None or kind != "View" or names is None:
        return None
    from django.views.generic.base import View

    defined: list[str] = []
    for name in names:
        owner = find_owner(cls, name)
        if owner is None or owner is View:
            continue
        defined.append(name)
    return defined


def timezone_mixin(cls: type | None) -> bool:
    if cls is None:
        return False
    return any(
        klass.__qualname__ == "TimezoneMixin" and klass.__module__.startswith("plane.")
        for klass in cls.__mro__
    )


def use_read_replica(cls: type | None, callback: Any) -> bool | None:
    """`getattr(view, "use_read_replica", None)`, for functions and classes."""
    target = cls if cls is not None else callback
    value = getattr(target, "use_read_replica", None)
    return bool(value) if value is not None else None


def security_classes(
    cls: type | None, kind: str
) -> tuple[
    list[str],
    list[str],
    list[str],
    dict[str, bool],
    dict[str, str | None],
]:
    """Resolve DRF authentication/permission/throttle classes as on the class."""
    from rest_framework.settings import api_settings

    if cls is None:
        return ([], [], [], {"authenticators": False, "permissions": False, "throttles": False}, {})

    if not is_drf(kind):
        # A plain django.views.View never runs DRF's initial(), so any
        # authentication_classes/permission_classes it declares are dead code.
        return ([], [], [], {"authenticators": False, "permissions": False, "throttles": False}, {})

    defaults: dict[str, Iterable[type]] = {
        "authentication_classes": api_settings.DEFAULT_AUTHENTICATION_CLASSES,
        "permission_classes": api_settings.DEFAULT_PERMISSION_CLASSES,
        "throttle_classes": api_settings.DEFAULT_THROTTLE_CLASSES,
    }

    def resolve(attribute: str) -> list[str]:
        value = getattr(cls, attribute, defaults[attribute])
        return dedupe(dotted_class(item) for item in value)

    def dynamic(getter: str) -> tuple[bool, str | None]:
        owner = find_owner(cls, getter)
        if owner is None:
            return (False, None)
        if owner.__module__ == "rest_framework.views" and owner.__name__ == "APIView":
            return (False, dotted_class(owner))
        if not is_drf(kind):
            return (False, dotted_class(owner))
        return (True, dotted_class(owner))

    dynamic_flags: dict[str, bool] = {}
    dynamic_owners: dict[str, str | None] = {}
    for label, getter in (
        ("authenticators", "get_authenticators"),
        ("permissions", "get_permissions"),
        ("throttles", "get_throttles"),
    ):
        flag, owner = dynamic(getter)
        dynamic_flags[label] = flag
        if flag and owner:
            dynamic_owners[label] = owner

    return (
        resolve("authentication_classes"),
        resolve("permission_classes"),
        resolve("throttle_classes"),
        dynamic_flags,
        dynamic_owners,
    )


# ---------------------------------------------------------------------------
# Collection
# ---------------------------------------------------------------------------


def collect_routes() -> tuple[list[dict[str, Any]], str, str, bool]:
    import django
    import rest_framework
    from django.conf import settings
    from django.urls import URLResolver, get_resolver
    from django.urls.resolvers import RoutePattern

    routes: list[dict[str, Any]] = []

    def pattern_parts(
        pattern: Any, django_leaf: str
    ) -> tuple[str, OrderedDict[str, str], bool]:
        """Normalised leaf template, the parameters it captures, optional slash."""
        if isinstance(pattern, RoutePattern):
            leaf = normalise_route_pattern(pattern)
            params: OrderedDict[str, str] = OrderedDict(
                (name, converter_name(converter))
                for name, converter in pattern.converters.items()
            )
            optional_trailing_slash = bool(
                re.search(r"<drf_format_suffix:[^>]+>$", str(pattern))
            )
            return leaf, params, optional_trailing_slash
        return normalise_regex_pattern(django_leaf)

    def walk(
        resolver: Any,
        prefix_path: str,
        prefix_django: str,
        prefix_params: OrderedDict[str, str],
    ) -> None:
        for entry in resolver.url_patterns:
            pattern = entry.pattern
            django_leaf = str(pattern)

            if isinstance(entry, URLResolver):
                leaf, leaf_params, _ = pattern_parts(pattern, django_leaf)
                # A converter declared on the include belongs to every route
                # below it: `path("api/v1/workspaces/<str:slug>/", include(...))`
                # gives each router-generated leaf a `slug` parameter even
                # though the leaf pattern itself captures nothing. Carry them
                # down in URL order (outermost first) so `params` describes the
                # full pattern, not just the leaf.
                inherited = OrderedDict(prefix_params)
                inherited.update(leaf_params)
                walk(entry, prefix_path + leaf, prefix_django + django_leaf, inherited)
                continue

            leaf, leaf_params, optional_trailing_slash = pattern_parts(
                pattern, django_leaf
            )
            params: OrderedDict[str, str] = OrderedDict(prefix_params)
            params.update(leaf_params)
            pattern_type = "path" if isinstance(pattern, RoutePattern) else "regex"

            raw_path = prefix_path + leaf
            path = "/" + raw_path.lstrip("/")
            if path == "":
                path = "/"
            django_pattern = prefix_django + django_leaf

            callback = entry.callback
            cls = view_class(callback)
            kind = kind_of(cls)
            methods, implicit_methods, http_method_names = effective_methods(callback, cls, kind)
            view = dotted_class(cls) if cls is not None else dotted_callable(callback)

            method_permissions: OrderedDict[str, list[dict[str, Any]]] = OrderedDict()
            method_throttles: OrderedDict[str, list[dict[str, Any]]] = OrderedDict()
            for method, action in methods.items():
                handler = (
                    callback
                    if cls is None
                    else handler_for(cls, kind, method, action)
                )
                permissions, throttles = extract_method_decorators(handler)
                method_permissions[method] = permissions
                method_throttles[method] = throttles

            (
                authentication_classes,
                permission_classes,
                throttle_classes,
                dynamic_flags,
                dynamic_owners,
            ) = security_classes(cls, kind)

            guard = viewset_guard(cls, kind, methods, dynamic_flags["permissions"])

            family = base_family(cls, kind)
            method_names = effective_http_method_names(cls, callback)
            renderer_classes = class_list_attribute(cls, kind, "renderer_classes")
            parser_classes = class_list_attribute(cls, kind, "parser_classes")
            content_negotiation_class = class_scalar_attribute(
                cls, kind, "content_negotiation_class"
            )
            metadata_class = class_scalar_attribute(cls, kind, "metadata_class")
            names = view_names(cls, kind, callback)
            lifecycle = lifecycle_overrides(cls, family)
            options = options_actions(
                cls, kind, callback, methods, implicit_methods, method_names
            )
            defined = defined_methods(cls, kind, method_names)

            notes: list[str] = []
            if cls is not None and not is_drf(kind):
                dead_declarations = [
                    attribute
                    for attribute in (
                        "authentication_classes",
                        "permission_classes",
                        "throttle_classes",
                    )
                    if getattr(cls, attribute, None)
                ]
                if dead_declarations:
                    notes.append(
                        "non-DRF view declares "
                        + ", ".join(dead_declarations)
                        + "; DRF does not enforce them"
                    )
            if pattern_type == "regex":
                notes.append("regex route")
            if optional_trailing_slash:
                notes.append("optional trailing slash")
            notes.extend(mixed_segment_notes(path))

            route = OrderedDict(
                [
                    ("order", len(routes)),
                    # ---- Phase 2 (plane-drf) policy introspection ----
                    ("base_family", family),
                    ("use_read_replica", use_read_replica(cls, callback)),
                    ("timezone_mixin", timezone_mixin(cls)),
                    ("http_method_names", method_names),
                    ("renderer_classes", renderer_classes),
                    ("parser_classes", parser_classes),
                    ("content_negotiation_class", content_negotiation_class),
                    ("metadata_class", metadata_class),
                    ("view_name", names["name"]),
                    ("view_description", names["description"]),
                    ("lifecycle_overrides", lifecycle),
                    ("defined_methods", defined),
                    ("guard_actions", guard_actions(cls, kind, methods)),
                    ("options_metadata", options),
                    ("group", group_for(path)),
                    ("django_pattern", django_pattern),
                    ("path", path),
                    ("params", params),
                    ("name", entry.name),
                    ("view", view),
                    ("kind", kind),
                    ("methods", methods),
                    ("viewset_guard", guard),
                    ("method_permissions", method_permissions),
                    ("method_throttles", method_throttles),
                    ("authentication_classes", authentication_classes),
                    ("permission_classes", permission_classes),
                    ("throttle_classes", throttle_classes),
                    ("dynamic_authenticators", dynamic_flags["authenticators"]),
                    ("dynamic_permissions", dynamic_flags["permissions"]),
                    ("dynamic_throttles", dynamic_flags["throttles"]),
                    ("dynamic_owners", dynamic_owners),
                    ("csrf_exempt", bool(getattr(callback, "csrf_exempt", False))),
                    ("drf_options", is_drf(kind)),
                    ("implicit_methods", implicit_methods),
                    ("http_method_names_override", http_method_names),
                    ("pattern_type", pattern_type),
                    ("trailing_slash", path.endswith("/")),
                    (
                        "trailing_slash_form",
                        "optional"
                        if optional_trailing_slash
                        else ("present" if path.endswith("/") else "absent"),
                    ),
                    ("notes", notes),
                ]
            )
            routes.append(route)

    walk(get_resolver(), "", "", OrderedDict())

    # Annotate duplicate normalised paths; Django serves only the first.
    by_path: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for route in routes:
        by_path[(route["group"], route["path"])].append(route)
    for entries in by_path.values():
        if len(entries) < 2:
            continue
        for route in entries:
            route["notes"].append(f"path registered {len(entries)} times")
        for route in entries[1:]:
            route["notes"].append("unreachable: Django uses the first registration")

    return (
        routes,
        django.get_version(),
        rest_framework.VERSION,
        bool(settings.ENABLE_DRF_SPECTACULAR),
    )


# ---------------------------------------------------------------------------
# bytes / ordering helpers for the JSON payload
# ---------------------------------------------------------------------------


def git_commit(override: str | None = None) -> str:
    if override:
        return override
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    return result.stdout.strip() or "unknown"


def write_json(payload: dict[str, Any], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, indent=2, ensure_ascii=False)
        handle.write("\n")


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


# ---------------------------------------------------------------------------
# ROUTES.md ownership preservation
# ---------------------------------------------------------------------------

OWNERSHIP_GROUP_RE = re.compile(r"^##\s+(app|v1|public|auth|instances|web|schema)\b")
TABLE_SPLIT_RE = re.compile(r"(?<!\\)\|")


def strip_cell(cell: str) -> str:
    value = cell.strip().replace("\\|", "|")
    if value.startswith("`") and value.endswith("`") and len(value) >= 2:
        value = value[1:-1]
    return value


def parse_ownership(markdown: str) -> dict[tuple[str, str, str], tuple[str, str]]:
    """Read Owner/Status from an existing ROUTES.md.

    Keyed on ``(group, path, view)``. When duplicate routes share a key (the
    DRF api-root is registered twice) the first row wins, which is also the
    route Django serves.
    """
    ownership: dict[tuple[str, str, str], tuple[str, str]] = {}
    current_group: str | None = None

    for line in markdown.splitlines():
        heading = OWNERSHIP_GROUP_RE.match(line)
        if heading:
            current_group = heading.group(1)
            continue
        if line.startswith("## "):
            current_group = None
            continue
        if current_group is None or not line.startswith("|"):
            continue

        cells = TABLE_SPLIT_RE.split(line)
        cells = [cell for cell in cells]
        if cells and cells[0].strip() == "":
            cells = cells[1:]
        if cells and cells[-1].strip() == "":
            cells = cells[:-1]
        if len(cells) != len(ROUTE_TABLE_COLUMNS):
            continue
        if not cells[0].strip().isdigit():
            continue

        path = strip_cell(cells[1])
        view = strip_cell(cells[3])
        owner = strip_cell(cells[5])
        status = strip_cell(cells[6])
        if not path or not view or not owner or not status:
            continue

        key = (current_group, path, view)
        ownership.setdefault(key, (owner, status))

    return ownership


def markdown_escape(value: str) -> str:
    return value.replace("|", "\\|")


class OwnerCellError(ValueError):
    """An Owner cell does not follow the documented syntax or the route."""


def parse_owner_cell(cell: str) -> tuple[str, tuple[str, ...] | None]:
    """Parse an Owner cell into ``(engine, methods)``.

    ``methods`` is ``None`` for a whole-row owner and a tuple of uppercase
    method names for a ``rust(GET,HEAD)`` split. Raises :class:`OwnerCellError`
    on anything else.
    """
    text = cell.strip()
    if not text:
        raise OwnerCellError("owner cell is empty")

    match = OWNER_CELL_RE.match(text)
    if match is None:
        raise OwnerCellError(
            f"unknown owner syntax {cell!r}; expected django, rust or rust(GET,HEAD)"
        )

    engine = match.group("engine")
    if engine not in OWNER_ENGINES:
        raise OwnerCellError(
            f"unknown owner {engine!r}; expected one of {', '.join(OWNER_ENGINES)}"
        )

    method_list = match.group("methods")
    if engine == "django":
        if method_list is not None:
            raise OwnerCellError(
                "django cannot carry a method list; use rust(GET,HEAD) to move a "
                "subset to Rust"
            )
        return ("django", None)
    if method_list is None:
        return ("rust", None)

    names = [name.strip() for name in method_list.split(",")]
    if not names or any(not name for name in names):
        raise OwnerCellError(
            f"malformed method list in {cell!r}; expected rust(GET,HEAD)"
        )

    methods: list[str] = []
    for name in names:
        if name not in HTTP_METHODS_UPPER:
            raise OwnerCellError(
                f"unknown method {name!r} in {cell!r}; expected uppercase "
                f"methods from {', '.join(HTTP_METHODS_UPPER)}"
            )
        if name in methods:
            raise OwnerCellError(f"method {name!r} is listed twice in {cell!r}")
        methods.append(name)
    return ("rust", tuple(methods))


def route_methods(route: dict[str, Any]) -> set[str]:
    """Every method the route answers, as lowercase names.

    ``methods`` holds the explicitly registered handlers; ``implicit_methods``
    holds the ``HEAD`` Django copies from a ``GET`` and the ``OPTIONS`` DRF
    answers for every DRF view.
    """
    methods = {str(method).lower() for method in route.get("methods", {})}
    methods |= {str(method).lower() for method in route.get("implicit_methods", [])}
    return methods


def validate_owner_cell(cell: str, route: dict[str, Any]) -> tuple[str, tuple[str, ...] | None]:
    """Parse ``cell`` and check its methods against ``route``."""
    engine, methods = parse_owner_cell(cell)
    if engine == "django" or methods is None:
        if engine == "rust" and not route_methods(route):
            raise OwnerCellError(
                "route registers no methods, so a whole-row rust owner would "
                "produce no proxy rule"
            )
        return (engine, methods)

    available = route_methods(route)
    missing = [name for name in methods if name.lower() not in available]
    if missing:
        raise OwnerCellError(
            f"method(s) {', '.join(missing)} are not registered for this route "
            f"(it answers {', '.join(sorted(m.upper() for m in available)) or 'nothing'})"
        )
    return (engine, methods)


def validate_ownership(
    routes: list[dict[str, Any]],
    ownership: dict[tuple[str, str, str], tuple[str, str]],
) -> list[str]:
    """Return one message per route whose preserved Owner cell is invalid."""
    errors: list[str] = []
    for route in routes:
        key = (route["group"], route["path"], route["view"])
        entry = ownership.get(key)
        if entry is None:
            continue
        owner, _status = entry
        try:
            validate_owner_cell(owner, route)
        except OwnerCellError as exc:
            errors.append(f"{route['path']} ({route['view']}): {exc}")
    return errors


def code(value: str) -> str:
    return "`" + markdown_escape(value) + "`"


# ---------------------------------------------------------------------------
# ROUTES.md rendering
# ---------------------------------------------------------------------------


def render_methods(route: dict[str, Any]) -> str:
    methods: dict[str, str] = route["methods"]
    if not methods:
        return "-"
    refused = {item["method"] for item in route.get("viewset_guard", {}).get("refused_methods", [])}
    actions = list(methods.values())
    if (
        len(methods) == len(HTTP_METHODS)
        and len(set(actions)) == 1
        and actions[0] == route["view"].rsplit(".", 1)[-1]
    ):
        return f"ALL ({actions[0]})"
    parts: list[str] = []
    for method, action in methods.items():
        label = method.upper() if action == method else f"{method.upper()}:{action}"
        if method in refused:
            label += " [405]"
        parts.append(label)
    return ", ".join(parts)


def render_allow(entry: dict[str, Any]) -> str:
    if entry.get("unknown"):
        return "UNKNOWN"
    roles = ",".join(str(role) for role in entry["allowed_roles"]) or "-"
    extra = [str(entry["level"])]
    if entry.get("creator"):
        model = entry.get("model")
        extra.append("creator=" + short_name(model) if model else "creator")
    return f"allow({roles};{';'.join(extra)})"


def render_permissions(route: dict[str, Any]) -> str:
    parts: list[str] = []
    permission_classes = [short_name(item) for item in route["permission_classes"]]
    if permission_classes:
        rendered = "+".join(permission_classes)
        if route["dynamic_permissions"]:
            rendered += " (dynamic)"
        parts.append(rendered)
    elif route["dynamic_permissions"]:
        parts.append("dynamic")

    grouped: OrderedDict[str, list[str]] = OrderedDict()
    for method, entries in route["method_permissions"].items():
        for entry in entries:
            signature = render_allow(entry)
            grouped.setdefault(signature, []).append(method.upper())
    for signature, methods in grouped.items():
        parts.append(f"{','.join(methods)} {signature}")

    return "; ".join(parts) if parts else "-"


def render_route_table(
    routes: list[dict[str, Any]],
    ownership: dict[tuple[str, str, str], tuple[str, str]],
) -> list[str]:
    lines: list[str] = []
    lines.append("| " + " | ".join(ROUTE_TABLE_COLUMNS) + " |")
    lines.append("|" + "|".join(["---"] * len(ROUTE_TABLE_COLUMNS)) + "|")
    for route in routes:
        key = (route["group"], route["path"], route["view"])
        owner, status = ownership.get(key, ("django", "not-started"))
        lines.append(
            "| "
            + " | ".join(
                [
                    str(route["order"]),
                    code(route["path"]),
                    render_methods(route),
                    code(route["view"]),
                    markdown_escape(render_permissions(route)),
                    markdown_escape(owner),
                    markdown_escape(status),
                ]
            )
            + " |"
        )
    return lines


def group_counts(routes: list[dict[str, Any]]) -> OrderedDict[str, dict[str, int]]:
    counts: OrderedDict[str, dict[str, int]] = OrderedDict(
        (group, {"endpoints": 0, "method_pairs": 0}) for group in GROUP_ORDER
    )
    for route in routes:
        counts[route["group"]]["endpoints"] += 1
        counts[route["group"]]["method_pairs"] += len(route["methods"])
    return counts


def render_summary(routes: list[dict[str, Any]]) -> list[str]:
    counts = group_counts(routes)
    lines: list[str] = []
    lines.append("## Summary")
    lines.append("")
    lines.append("| Group | Prefix | Endpoints | Brief | Difference | Endpoint × method |")
    lines.append("| --- | --- | ---: | ---: | ---: | ---: |")
    total_endpoints = 0
    total_pairs = 0
    total_brief = 0
    for group in GROUP_ORDER:
        endpoints = counts[group]["endpoints"]
        pairs = counts[group]["method_pairs"]
        total_endpoints += endpoints
        total_pairs += pairs
        brief = BRIEF_COUNTS.get(group)
        if brief is None:
            brief_cell = "—"
            difference_cell = "—"
        else:
            total_brief += brief
            brief_cell = str(brief)
            difference_cell = f"{endpoints - brief:+d}" if endpoints != brief else "0"
        if endpoints == 0 and group == "schema":
            continue
        lines.append(
            f"| {group} | {GROUP_PREFIX[group]} | {endpoints} | {brief_cell} | "
            f"{difference_cell} | {pairs} |"
        )
    lines.append(
        f"| **total** | | **{total_endpoints}** | **{total_brief}** | "
        f"**{total_endpoints - total_brief:+d}** | **{total_pairs}** |"
    )
    lines.append("")
    lines.append("The brief's counts are from `RUST_REWRITE.md` (\"What exists today\").")
    lines.append(
        "They were produced by counting `path()` calls, which is not the same as counting "
        "resolver entries. Differences:"
    )
    lines.append("")
    lines.append("- **app**: 234 `path()` calls, 234 resolver entries. No difference.")
    v1_total = counts["v1"]["endpoints"]
    v1_router = sum(
        1
        for route in routes
        if route["group"] == "v1"
        and (route["pattern_type"] == "regex" or route["name"] == "api-root")
    )
    v1_plain = v1_total - v1_router
    lines.append(
        f"- **v1**: {v1_total} resolver entries = {v1_plain} plain `path()` routes + "
        f"{v1_router} entries generated by the two `include(router.urls)` expansions "
        "(list/detail, format-suffix variants and two api-roots per router). The brief's "
        "75 is the number of `path()` calls in `plane/api/urls/*.py`; that count includes "
        "3 paths in `plane/api/urls/schema.py` and 3 in `plane/api/urls/estimate.py` which "
        "`plane/api/urls/__init__.py` never imports, and counts each router include as one "
        f"entry. Mounted `path()` calls: {v1_plain + 2}; {v1_plain + 2} − 2 + {v1_router} = {v1_total}."
    )
    lines.append("- **public**: 25 resolver entries, 25 in the brief. No difference.")
    lines.append("- **auth**: 37 resolver entries, 37 in the brief. No difference.")
    lines.append("- **instances**: 14 resolver entries, 14 in the brief. No difference.")
    lines.append(
        "- **web**: 2 resolver entries (`robots.txt` and the health check at `/`). The "
        "brief's table does not list `plane.web`."
    )
    if counts["schema"]["endpoints"]:
        lines.append(
            "- **schema**: present only because `ENABLE_DRF_SPECTACULAR=1`; the brief's "
            "table does not list these routes. With the default (`0`) they do not exist."
        )
    else:
        lines.append(
            "- **schema**: 0 entries in this run. `ENABLE_DRF_SPECTACULAR=1` adds "
            "`/api/schema/`, `/api/schema/swagger-ui/` and `/api/schema/redoc/`."
        )
    lines.append("")
    lines.append(
        "`Endpoint × method` counts one pair per HTTP method each resolver entry registers "
        "(ViewSet actions, implemented APIView handlers, and the methods a Django `View` "
        "or function view accepts). It is the number of method/path pairs the Rust router "
        "must account for, including pairs that Django answers with 405 because of the "
        "`BaseViewSet` guard (hazard (g))."
    )
    lines.append("")
    return lines


def render_hazards(routes: list[dict[str, Any]]) -> list[str]:
    lines: list[str] = []
    lines.append("## Routing hazards")
    lines.append("")
    lines.append(
        "Everything in this section is computed from `routes.json`. Path templates use "
        "`{name}` for a captured segment. `matchit` is the router under axum 0.8 "
        "(matchit 0.8): static segments are tried before parameters, and a parameter only "
        "matches the text its Django converter would accept if the handler validates it."
    )
    lines.append("")

    # (a) parameter vs literal at one segment position.
    candidates: list[dict[str, Any]] = []
    seen_pairs: set[tuple[str, str]] = set()
    for first, second in combinations(routes, 2):
        if first["group"] != second["group"]:
            continue
        first_kinds = segment_kinds(first["path"])
        second_kinds = segment_kinds(second["path"])
        if len(first_kinds) != len(second_kinds):
            continue
        if any(kind == "mixed" for kind, _ in first_kinds + second_kinds):
            continue

        differences = [
            index
            for index in range(len(first_kinds))
            if first_kinds[index][0] != second_kinds[index][0]
        ]
        if len(differences) != 1:
            continue
        index = differences[0]
        if {first_kinds[index][0], second_kinds[index][0]} != {"param", "literal"}:
            continue

        compatible = True
        for position in range(len(first_kinds)):
            if position == index:
                continue
            first_kind, first_value = first_kinds[position]
            second_kind, second_value = second_kinds[position]
            if first_kind == "literal" and second_kind == "literal":
                if first_value != second_value:
                    compatible = False
                    break
            elif first_kind == "param" and second_kind == "literal":
                if not literal_matches_converter(second_value, first["params"].get(first_value, "str")):
                    compatible = False
                    break
            elif first_kind == "literal" and second_kind == "param":
                if not literal_matches_converter(first_value, second["params"].get(second_value, "str")):
                    compatible = False
                    break
        if not compatible:
            continue

        key = (first["path"], second["path"])
        if key in seen_pairs:
            continue
        seen_pairs.add(key)

        first_kind, first_value = first_kinds[index]
        second_kind, second_value = second_kinds[index]
        if first_kind == "param":
            param_route, literal_route = first, second
            param_value, literal_value = first_value, second_value
        else:
            param_route, literal_route = second, first
            param_value, literal_value = second_value, first_value

        trailing_compatible = (
            first["trailing_slash_form"] == "optional"
            or second["trailing_slash_form"] == "optional"
            or first["trailing_slash"] == second["trailing_slash"]
        )
        overlap = trailing_compatible and literal_matches_converter(
            literal_value, param_route["params"].get(param_value, "str")
        )

        # Django resolves in order: with an overlap the earlier route wins.
        # Without an overlap the earlier parameter route cannot match the
        # literal URL at all, so Django falls through to the literal route.
        django_winner = first if overlap else literal_route
        matchit_winner = literal_route  # static beats a parameter in matchit.
        same = django_winner["order"] == matchit_winner["order"]

        candidates.append(
            {
                "first": first,
                "second": second,
                "position": index,
                "param_route": param_route,
                "literal_route": literal_route,
                "param_value": param_value,
                "literal_value": literal_value,
                "overlap": overlap,
                "django_winner": django_winner,
                "matchit_winner": matchit_winner,
                "same": same,
            }
        )

    disagreements = [candidate for candidate in candidates if not candidate["same"]]
    full_candidates = sum(
        1
        for first, second in combinations(routes, 2)
        if pair_can_overlap(first, second)
        and any(
            {first_kind, second_kind} == {"param", "literal"}
            for (first_kind, _), (second_kind, _) in zip(
                segment_kinds(first["path"]), segment_kinds(second["path"])
            )
        )
    )
    lines.append("### (a) Parameter vs literal at the same position")
    lines.append("")
    lines.append(
        f"Found {len(candidates)} single-segment candidates where a parameter and a literal "
        f"compete; {len(disagreements)} disagree with matchit."
    )
    lines.append("")
    lines.append(
        "The table lists every pair whose templates are identical except at one segment, "
        "where one side has a parameter and the other a literal. A full pairwise scan that "
        "also requires a concrete URL to match both templates (every segment compatible "
        f"with its converter, and the same trailing-slash form) finds {full_candidates} "
        f"pair{'s' if full_candidates != 1 else ''}. In "
        "particular the brief's example — `issues/{pk}/` next to `issues/bulk-delete/` — "
        "does not occur here: the bulk-delete route is "
        "`bulk-delete-issues/` at a different position, and `{pk}` is `<uuid:pk>`, whose "
        "converter rejects `bulk-delete`."
    )
    lines.append("")
    if candidates:
        lines.append(
            "| First (order) | Second (order) | Position | Parameter | Literal | "
            "Common URL? | Django picks | matchit picks | Same? |"
        )
        lines.append("| --- | --- | ---: | --- | --- | --- | --- | --- | --- |")
        for candidate in candidates:
            first = candidate["first"]
            second = candidate["second"]
            lines.append(
                "| "
                + " | ".join(
                    [
                        f"#{first['order']} {code(first['path'])}",
                        f"#{second['order']} {code(second['path'])}",
                        str(candidate["position"]),
                        f"{{{candidate['param_value']}}}",
                        code(candidate["literal_value"]),
                        "yes" if candidate["overlap"] else "no",
                        f"#{candidate['django_winner']['order']}",
                        f"#{candidate['matchit_winner']['order']}",
                        "yes" if candidate["same"] else "**NO**",
                    ]
                )
                + " |"
            )
        lines.append("")
        lines.append(
            "\"Common URL?\" means a concrete URL can match both templates: the literal must "
            "satisfy the parameter's Django converter, and the trailing-slash form must be "
            "compatible. When it is `no`, Django's earlier parameter route rejects the "
            "literal value and falls through to the literal route, which is also matchit's "
            "choice — so the outcomes agree. A disagreement needs a common URL, the "
            "parameter route registered first, and a `str`-like converter that accepts the "
            "literal. None exist in this tree because Plane uses `<uuid:...>` wherever a "
            "literal sibling could otherwise be shadowed."
        )
        lines.append("")
        if disagreements:
            lines.append("**Disagreements:**")
            lines.append("")
            for candidate in disagreements:
                lines.append(
                    f"- {code(candidate['param_route']['path'])} (order "
                    f"{candidate['param_route']['order']}) is tried before "
                    f"{code(candidate['literal_route']['path'])} (order "
                    f"{candidate['literal_route']['order']}). Django matches the parameter; "
                    "matchit matches the static segment."
                )
            lines.append("")
        else:
            lines.append(
                "No candidate changes the winner: Django and matchit agree on every pair in "
                "this tree. The Rust router still has to validate `uuid` and `int` "
                "parameters and return 404 when they do not parse, because matchit's "
                "parameter accepts any segment while Django's converter rejects it "
                "(see (c))."
            )
            lines.append("")
    else:
        lines.append(
            "None. No two resolver entries differ at exactly one segment with a parameter "
            "on one side and a literal on the other."
        )
        lines.append("")

    # (b) duplicate normalised paths.
    by_path: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for route in routes:
        by_path[(route["group"], route["path"])].append(route)
    duplicates = [entries for entries in by_path.values() if len(entries) > 1]

    lines.append("### (b) Identical normalised paths registered more than once")
    lines.append("")
    lines.append(
        f"Found {len(duplicates)} paths with more than one resolver entry. Django resolves "
        "by path only and tries entries in order, so the first entry serves every HTTP "
        "method; later entries are unreachable, they do not serve the methods the first "
        "entry lacks."
    )
    lines.append("")
    if duplicates:
        lines.append("| Path | Entries (order, view, methods) | First wins |")
        lines.append("| --- | --- | --- |")
        for entries in duplicates:
            entries = sorted(entries, key=lambda route: route["order"])
            description = "; ".join(
                f"#{route['order']} {short_name(route['view'])} [{render_methods(route)}]"
                for route in entries
            )
            lines.append(
                f"| {code(entries[0]['path'])} | {markdown_escape(description)} | "
                f"#{entries[0]['order']} {short_name(entries[0]['view'])} |"
            )
        lines.append("")
        lines.append(
            "Both duplicates are the DRF `DefaultRouter` api-root, included once by "
            "`plane/api/urls/invite.py` and once by `plane/api/urls/sticky.py`. The first "
            "registration (the invitations router) answers `GET /api/v1/workspaces/{slug}/` "
            "and its `.<format>` variant, so its `api_root_dict` — invitations only — is "
            "what clients see. Port that behaviour as it is; a Rust router that merged the "
            "two routers would change the response."
        )
        lines.append("")
    else:
        lines.append("None.")
        lines.append("")

    # (c) non-standard converters and regex routes.
    standard_converters = {"str", "uuid", "int", "slug", "path"}
    non_standard = [
        route
        for route in routes
        if route["pattern_type"] == "regex"
        or any(converter not in standard_converters for converter in route["params"].values())
    ]
    regex_routes = [route for route in routes if route["pattern_type"] == "regex"]

    lines.append("### (c) Non-standard converters and regex routes")
    lines.append("")
    lines.append(
        f"Found {len(non_standard)} entries: {len(regex_routes)} regex patterns and "
        f"{len(non_standard) - len(regex_routes)} entries with a custom converter. "
        "`str`, `uuid`, `int`, `slug` and `path` are the standard converters; none of the "
        "entries here uses `int`, `slug` or `path`."
    )
    lines.append("")
    if non_standard:
        lines.append("| # | Path | Converter(s) | Pattern type | View |")
        lines.append("| ---: | --- | --- | --- | --- |")
        for route in non_standard:
            converters = ", ".join(
                f"{name}:{'regex ' + expression if route['pattern_type'] == 'regex' else expression}"
                for name, expression in route["params"].items()
            )
            lines.append(
                f"| {route['order']} | {code(route['path'])} | "
                f"{markdown_escape(converters) or '—'} | {route['pattern_type']} | "
                f"{code(short_name(route['view']))} |"
            )
        lines.append("")
    lines.append(
        "The regex entries are generated by `DefaultRouter`: the list/detail routes use "
        "`(?P<pk>[^/.]+)` and the format variants add `\\.(?P<format>[a-z0-9]+)` with an "
        "optional trailing slash. `drf_format_suffix` is DRF's `FormatSuffixConverter`, "
        "whose regex is `\\.[a-z0-9]+/?`; the dot is part of the match, which is why the "
        "template is `.{format}`, not `{format}`. Django rejects `/workspaces/{slug}/json` "
        "and resolves `/workspaces/{slug}/.json` (with or without a trailing slash). "
        "matchit has no converters: a `{pk}` segment matches "
        "`not-a-uuid` and `not a uuid`, where Django's `<uuid:pk>` returns 404 (and a slash "
        "is a segment separator in both). Every UUID, int, slug and regex parameter must "
        "therefore be validated in the Rust handler before it reaches the database, and a "
        "parse failure must produce Django's 404, not a handler error."
    )
    lines.append("")

    # (d) routes without a trailing slash.
    no_slash = [
        route for route in routes if route["path"] != "/" and not route["trailing_slash"]
    ]
    lines.append("### (d) Routes without a trailing slash")
    lines.append("")
    lines.append(
        f"Found {len(no_slash)} entries. The `absent` rows match only without the trailing "
        "slash; the `optional` rows match both forms. (`CommonMiddleware`'s "
        "`APPEND_SLASH` can append a slash only when nothing matches and the slashed form "
        "exists, so it does not help the `absent` routes.)"
    )
    lines.append("")
    if no_slash:
        lines.append("| # | Path | Slash form | View |")
        lines.append("| ---: | --- | --- | --- |")
        for route in no_slash:
            lines.append(
                f"| {route['order']} | {code(route['path'])} | "
                f"{route['trailing_slash_form']} | {code(short_name(route['view']))} |"
            )
        lines.append("")
        if any(route["trailing_slash_form"] == "optional" for route in no_slash):
            lines.append(
                "The `optional` rows are DRF format-suffix routes: four regex patterns and "
                "two `<drf_format_suffix:format>` path patterns. They accept both "
                "`/invitations.json` and `/invitations.json/` (and `.json` for the "
                "api-root). The path template shows the literal dot because Django requires "
                "it; the optional slash is not part of the template."
            )
            lines.append("")
    else:
        lines.append("None.")
        lines.append("")

    # (e) trailing-slash variants.
    slash_variants: list[tuple[str, list[dict[str, Any]]]] = []
    by_base: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for route in routes:
        by_base[(route["group"], route["path"].rstrip("/") or "/")].append(route)
    for entries in by_base.values():
        forms = {route["trailing_slash"] for route in entries}
        if len(forms) > 1:
            slash_variants.append((entries[0]["path"], entries))

    lines.append("### (e) Patterns that differ only by a trailing slash")
    lines.append("")
    lines.append(
        f"Found {len(slash_variants)} pairs of normalised paths that are identical once the "
        "trailing slash is removed and register both forms."
    )
    lines.append("")
    if slash_variants:
        lines.append("| Base path | Forms (order, trailing slash) |")
        lines.append("| --- | --- |")
        for base, entries in slash_variants:
            description = "; ".join(
                f"#{route['order']} {route['trailing_slash_form']}" for route in entries
            )
            lines.append(f"| {code(base)} | {description} |")
        lines.append("")
    else:
        lines.append(
            "None. The DRF format-suffix regexes accept an optional trailing slash, but no "
            "second entry registers the other form as a separate path."
        )
        lines.append("")

    # (f) segments matchit cannot express.
    two_param_segments: list[dict[str, Any]] = []
    prefixed_param_segments: list[dict[str, Any]] = []
    for route in routes:
        for segment in segments(route["path"]):
            if "{" not in segment or PURE_PARAM_SEGMENT_RE.match(segment):
                continue
            captures = re.findall(r"\{[^}]+\}", segment)
            if len(captures) > 1:
                two_param_segments.append(route)
            else:
                prefixed_param_segments.append(route)
            break

    lines.append("### (f) Segments matchit cannot express as-is")
    lines.append("")
    lines.append(
        "matchit allows at most one named parameter per segment. A segment with two "
        "captures such as `{project_identifier}-{issue_identifier}` is rejected by "
        "`Router::insert` (`InsertError::InvalidParamSegment`), so axum needs two routes, "
        "a regex layer in front of the router, or a custom fallback."
    )
    lines.append("")
    if two_param_segments:
        lines.append("| # | Path | Segment | View |")
        lines.append("| ---: | --- | --- | --- |")
        for route in two_param_segments:
            for segment in segments(route["path"]):
                if len(re.findall(r"\{[^}]+\}", segment)) > 1:
                    lines.append(
                        f"| {route['order']} | {code(route['path'])} | "
                        f"{code(segment)} | {code(short_name(route['view']))} |"
                    )
                    break
        lines.append("")
    else:
        lines.append("No segment contains more than one parameter.")
        lines.append("")
    if prefixed_param_segments:
        lines.append(
            f"One-parameter segments with literal text ({len(prefixed_param_segments)} entries, "
            "e.g. "
            + ", ".join(
                code(segment)
                for route in prefixed_param_segments[:3]
                for segment in segments(route["path"])
                if "{" in segment and not PURE_PARAM_SEGMENT_RE.match(segment)
            )
            + ") are supported by matchit as prefix/suffix parameters; they are not a "
            "hazard, but they match as prefix parameters with static priority rules that "
            "differ from Django's regex order."
        )
        lines.append("")

    # (g) ViewSet actions the app BaseViewSet guard turns into 405.
    guard_routes = [
        route
        for route in routes
        if route.get("viewset_guard", {}).get("refused_methods")
    ]
    guard_dynamic = [
        route
        for route in routes
        if route.get("viewset_guard", {}).get("status") == "dynamic"
    ]
    refused_count = sum(len(route["viewset_guard"]["refused_methods"]) for route in guard_routes)

    lines.append("### (g) ViewSet actions registered but refused by the BaseViewSet guard")
    lines.append("")
    lines.append(
        f"`plane/app/views/base.py:BaseViewSet.initial` refuses an action that is served "
        f"straight from a DRF mixin when the viewset has only `IsAuthenticated`/`AllowAny` "
        f"and does not implement the action itself. The route is still registered for that "
        f"method, but an authenticated request gets 405. Found {refused_count} such method(s) "
        f"across {len(guard_routes)} routes; {len(guard_dynamic)} viewset route(s) override "
        "`get_permissions` and could not be decided statically."
    )
    lines.append("")
    if guard_routes:
        lines.append("| # | Path | Method | Action | Handler owner |")
        lines.append("| ---: | --- | --- | --- | --- |")
        for route in guard_routes:
            for item in route["viewset_guard"]["refused_methods"]:
                lines.append(
                    f"| {route['order']} | {code(route['path'])} | {item['method'].upper()} | "
                    f"{item['action']} | {code(short_name(item['handler_owner'])) if item['handler_owner'] else '—'} |"
                )
        lines.append("")
        lines.append(
            "The methods column marks these with `[405]`. The action map still contains "
            "them because `routes.json` records what the URL entry registers; the Rust "
            "router must return Django's 405 for these method/path pairs rather than run "
            "the inherited mixin handler. `OPTIONS` still succeeds (the guard skips it)."
        )
        lines.append("")
    else:
        lines.append("None.")
        lines.append("")
    if guard_dynamic:
        lines.append(
            "Dynamic `get_permissions` was not evaluated: "
            + ", ".join(
                f"#{route['order']} {code(short_name(route['view']))}" for route in guard_dynamic
            )
            + ". Check these at runtime; the guard only refuses when `get_permissions()` "
            "returns nothing stronger than `IsAuthenticated` or `AllowAny`."
        )
        lines.append("")

    # /api/ prefix shadowing, requested separately by the brief.
    app_routes = [route for route in routes if route["group"] == "app"]
    reserved = {"public", "instances", "v1", "schema"}
    shadowing = []
    for route in app_routes:
        parts = route["path"].split("/")
        # "/api/<first>/..." -> parts[0]="", parts[1]="api", parts[2]="<first>"
        if len(parts) > 2 and parts[2] in reserved:
            shadowing.append(route)

    lines.append("### `/api/` prefix shadowing")
    lines.append("")
    lines.append(
        "`plane/urls.py` registers `api/` (the app resolver) before `api/public/`, "
        "`api/instances/` and `api/v1/`. A resolver include is tried and, if its subtree "
        "does not match, Django falls through to the next top-level pattern, so this is "
        "only a hazard if an app route's first segment is one of the reserved prefixes. "
        f"Checked all {len(app_routes)} app routes; {len(shadowing)} begin with "
        "`public/`, `instances/`, `v1/` or `schema/`."
    )
    lines.append("")
    if shadowing:
        lines.append("| # | Path | View |")
        lines.append("| ---: | --- | --- |")
        for route in shadowing:
            lines.append(
                f"| {route['order']} | {code(route['path'])} | {code(short_name(route['view']))} |"
            )
        lines.append("")
        lines.append(
            "**These routes shadow the reserved prefix.** Django reaches them before the "
            "matching top-level include, so they are effective app routes; any Rust router "
            "that mounts the groups independently must not assume the top-level order."
        )
        lines.append("")
    else:
        first_segments = sorted(
            {
                route["path"].split("/")[2]
                for route in app_routes
                if route["path"].startswith("/api/") and len(route["path"].split("/")) > 2
            }
        )
        lines.append(
            "None. The app router's first segments are "
            + ", ".join(code(segment) for segment in first_segments)
            + "; no app route can be reached under `public/`, `instances/`, `v1/` or "
            "`schema/`. The top-level order in `plane/urls.py` is therefore safe, and the "
            "groups can be mounted independently in the Rust router."
        )
        lines.append("")

    return lines


def literal_matches_converter(literal: str, converter: str) -> bool:
    """Would a Django converter accept this literal segment?"""
    if converter in ("str", "path"):
        return True
    if converter == "uuid":
        try:
            uuid.UUID(literal)
            return True
        except (ValueError, AttributeError):
            return False
    if converter == "int":
        return bool(re.fullmatch(r"[0-9]+", literal))
    if converter == "slug":
        return bool(re.fullmatch(r"[-a-zA-Z0-9_]+", literal))
    if converter == "drf_format_suffix":
        return bool(re.fullmatch(r"\.[a-z0-9]+", literal))
    # A regex route stores the group body here. Full-match it; if the regex is
    # not expressible, err on the side of overlap rather than hiding a hazard.
    try:
        return re.fullmatch(converter, literal) is not None
    except re.error:
        return True


def pair_can_overlap(first: dict[str, Any], second: dict[str, Any]) -> bool:
    """Could one concrete URL match both templates?

    Requires the same trailing-slash form, the same segment count, no mixed
    segments, equal literals everywhere both sides are literal, and every
    literal accepted by the converter on the other side. Used to keep hazard
    (a) honest: templates that cannot produce a common URL are not competing.
    """
    if first["group"] != second["group"]:
        return False
    if first["trailing_slash_form"] != second["trailing_slash_form"] and "optional" not in (
        first["trailing_slash_form"],
        second["trailing_slash_form"],
    ):
        return False
    first_kinds = segment_kinds(first["path"])
    second_kinds = segment_kinds(second["path"])
    if len(first_kinds) != len(second_kinds):
        return False
    if any(kind == "mixed" for kind, _ in first_kinds + second_kinds):
        return False
    for (first_kind, first_value), (second_kind, second_value) in zip(first_kinds, second_kinds):
        if first_kind == "literal" and second_kind == "literal":
            if first_value != second_value:
                return False
        elif first_kind == "param" and second_kind == "literal":
            if not literal_matches_converter(second_value, first["params"].get(first_value, "str")):
                return False
        elif first_kind == "literal" and second_kind == "param":
            if not literal_matches_converter(first_value, second["params"].get(second_value, "str")):
                return False
    return True


def render_markdown(payload: dict[str, Any], ownership: dict[tuple[str, str, str], tuple[str, str]]) -> str:
    generated = payload["generated_from"]
    routes: list[dict[str, Any]] = payload["routes"]

    lines: list[str] = []
    lines.append("# Route inventory")
    lines.append("")
    lines.append(
        "Generated from Django's URL resolver by "
        "`apps/server/tools/inventory/routes.py`. Do not edit anything except the "
        "**Owner** and **Status** columns; regeneration preserves those two values for "
        "every route it already knows, keyed on `(group, path, view)`, and overwrites "
        "everything else. Status starts at `not-started`; use values such as "
        "`in-progress`, `verified` or `blocked` by hand."
    )
    lines.append("")
    lines.append(
        "Owner cells use one of three forms. `django` keeps every method of the row on "
        "Django. `rust` moves every method of the row to Rust, including the implicit "
        "`HEAD` Django answers for a `GET` and the `OPTIONS` DRF answers for any DRF "
        "view. `rust(GET,HEAD)` moves only the listed methods; the other methods of the "
        "same path stay on Django. Method names are uppercase and must be methods the "
        "row answers. Anything else is rejected by this generator and by "
        "`apps/server/tools/proxy/gen_caddy.py`, which turns these cells into "
        "`apps/server/deploy/caddy/rust-routes.caddy`: one named matcher and one "
        "`handle` block per route group, gated by `PLANE_RS_GROUP_<NAME>` (`rust` by "
        "default, `django` to send the group back)."
    )
    lines.append("")
    lines.append("Regenerate:")
    lines.append("")
    lines.append("```sh")
    lines.append("cd apps/api")
    lines.append("DJANGO_SETTINGS_MODULE=plane.settings.production \\")
    lines.append("SECRET_KEY=inventory \\")
    lines.append("DATABASE_URL=postgresql://x:x@127.0.0.1:1/x \\")
    lines.append("REDIS_URL=redis://127.0.0.1:1/0 \\")
    lines.append(
        f"ENABLE_DRF_SPECTACULAR={1 if generated['enable_drf_spectacular'] else 0} \\"
    )
    lines.append(".venv/bin/python ../server/tools/inventory/routes.py")
    lines.append("```")
    lines.append("")
    lines.append(
        f"Generated from commit `{generated['commit']}` with Django "
        f"{generated['django_version']} and DRF {generated['drf_version']} "
        f"(`ENABLE_DRF_SPECTACULAR={'1' if generated['enable_drf_spectacular'] else '0'}`). "
        "`routes.json` is the machine-readable form of the same data."
    )
    lines.append("")
    lines.append(
        "`order` is the 0-based index in Django's first-match order. `methods` is the "
        "ViewSet action map for a ViewSet, the implemented handlers for an `APIView`/"
        "`GenericAPIView`, and the methods the class or decorators accept for a Django "
        "`View` or function view. `head` and `options` are omitted unless explicitly "
        "implemented; `drf_options` records that DRF answers `OPTIONS` on every DRF view "
        "even when it is not listed. A `[405]` after a method means the method is registered "
        "but `BaseViewSet.initial` refuses it at runtime — see hazard (g)."
    )
    lines.append("")
    lines.append(
        "`permissions` shows the class-level permission classes (short names) and, after "
        "`;`, the per-method `allow_permission` decorator arguments as "
        "`METHODS allow(ROLES;LEVEL;creator=Model)`. `(dynamic)` means the class overrides "
        "`get_permissions`; `routes.json` also carries `dynamic_authenticators` and "
        "`dynamic_throttles` with the owning class. A `-` in the role list is an empty "
        "`allowed_roles`; only the `creator` branch can allow such a request. Every DRF "
        "view is `csrf_exempt` at Django's middleware layer (`routes.json` records that "
        "flag), but DRF's `SessionAuthentication` still enforces CSRF for "
        "session-authenticated requests. Plain `django.views.View` and function entries "
        "have no DRF security classes: they run under Django's `AuthenticationMiddleware` "
        "and inline checks, and a `permission_classes` attribute declared on such a class "
        "is dead code (flagged in the route's `notes`)."
    )
    lines.append("")
    lines.append(
        "With `ENABLE_DRF_SPECTACULAR=1` Django adds `/api/schema/`, "
        "`/api/schema/swagger-ui/` and `/api/schema/redoc/`; this file is generated from "
        "the default configuration unless the regeneration command sets the flag. With "
        "`DEBUG=1` and `debug_toolbar` installed, `__debug__/` routes are prepended; the "
        "production settings used here default to `DEBUG=0`, so they are absent."
    )
    lines.append("")
    lines.append("## Phase 2 policy fields (`plane-drf`)")
    lines.append("")
    lines.append(
        "`routes.json` also records what `plane-drf` needs to run each route exactly. "
        "These fields are generated from the view class, never typed by hand:"
    )
    lines.append("")
    lines.append("| Field | Meaning |")
    lines.append("| --- | --- |")
    lines.append("| `base_family` | which base class the view derives from: `app.BaseViewSet`, `app.BaseAPIView`, `api.BaseViewSet`, `api.BaseAPIView`, `space.BaseViewSet`, `space.BaseAPIView`, `license.BaseAPIView`, `drf.ViewSet`, `drf.APIView`, `django.View` or `function` |")
    lines.append("| `use_read_replica` | the class/function attribute the read replica router reads (`null` when it has none) |")
    lines.append("| `timezone_mixin` | the MRO contains a `TimezoneMixin` |")
    lines.append("| `http_method_names` | the effective list, in Django's order |")
    lines.append("| `renderer_classes`, `parser_classes`, `content_negotiation_class`, `metadata_class` | the DRF classes Django resolves through `api_settings` |")
    lines.append("| `view_name`, `view_description` | `get_view_name()` / `get_view_description()` on an instance built with the route's `as_view` initkwargs (so a ViewSet's `List`/`Instance` suffix is included) |")
    lines.append("| `lifecycle_overrides` | the hooks (`get_permissions`, `get_authenticators`, `get_throttles`, `initial`, `dispatch`, `handle_exception`, `finalize_response`, `options`) a concrete view overrides below its family base class, with the owning class; a non-null value without an implementation is a startup error |")
    lines.append("| `defined_methods` | for a plain `django.views.View`, the method handlers the class defines |")
    lines.append("| `guard_actions` | per ViewSet action: the implementing class and whether it is a DRF mixin method, so the runtime re-evaluates `BaseViewSet.initial`'s 405 rule with the permissions `get_permissions` actually resolved |")
    lines.append("| `options_metadata` | `attempts` (`{'PUT','POST'} & allowed_methods`), `actions_json` (the serializer field map per attempt as compact `json.dumps` text, order preserved), `error` (the exception `determine_actions` raised) and `get_object_required` |")
    lines.append("")
    lines.append("### OPTIONS metadata gaps")
    lines.append("")
    lines.append(
        "`actions_json` is recorded for a **permitted** caller, with "
        "`check_permissions` and `get_object()` bypassed so no database is needed. At "
        "request time `plane-drf` re-runs the permission check for the attempt's method "
        "and splices the recorded text into the body. Two gaps follow:"
    )
    lines.append("")
    lines.append(
        "* a `PUT` action also requires `view.get_object()` to succeed on Django; the "
        "runtime cannot run a view's `get_object()` generically, so a route with "
        "`get_object_required: true` may include `PUT` actions where Django omits them "
        "(the differential probe's OPTIONS steps avoid those routes);"
    )
    lines.append(
        "* CPython iterates `{'PUT','POST'}` in hash order, which is randomised per "
        "process, so the key order of Django's `actions` object is not stable either; "
        "the runtime writes `POST` before `PUT` and the recorded text follows that order."
    )
    lines.append("")
    lines.append("### Policy probe coverage")
    lines.append("")
    lines.append(
        "`scenarios/policy-probe.yaml` (generated by `tools/policy/gen_policy_scenario.py`) "
        "runs every route method outside `/api/v1/` through the Rust runtime for the "
        "fixture users. Four gaps are deliberate:"
    )
    lines.append("")
    lines.append(
        "* **`/api/v1/`** cannot be registered until Phase 4's Celery publisher exists: "
        "`APITokenLogMiddleware` would enqueue `plane.bgtasks.logger_task.process_logs`. "
        "The policy for those routes is generated and loaded, and the registry refuses "
        "their registration with an explicit error."
    )
    lines.append(
        "* **Write steps are omitted** for views whose handler re-implements an "
        "authorization check with the same body `allow_permission` returns "
        "(`{\"error\": \"You don't have the required permissions.\"}`; "
        "`asset/v2.py`, `asset/base.py`, "
        "`workspace/draft.py`, `project/base.py`). A 403 there cannot be attributed to "
        "the policy, so the probe does not weaken its rule; those 46 write method pairs "
        "keep the `django` owner until Phase 3 ports their handlers."
    )
    lines.append(
        "* **`DELETE /api/users/me/`** deactivates the caller and their memberships, and "
        "**`DELETE /api/workspaces/{slug}/`** deletes the workspace. They run only with "
        "callers that cannot succeed plus one admin against a disposable object, so a "
        "permitted delete cannot change a later step."
    )
    lines.append(
        "* **No `HEAD` steps**: the harness cannot decompress a `HEAD` response that "
        "carries `Content-Encoding: gzip` and an empty body. `HEAD` falls back to the "
        "`GET` handler (`View.setup`), which the `GET` steps exercise."
    )
    lines.append("")

    lines.extend(render_summary(routes))
    lines.append("")

    for group in GROUP_ORDER:
        group_routes = [route for route in routes if route["group"] == group]
        if not group_routes and group == "schema":
            continue
        lines.append(f"## {GROUP_TITLE[group]}")
        lines.append("")
        if group_routes:
            lines.extend(render_route_table(group_routes, ownership))
        else:
            lines.append("None.")
        lines.append("")

    lines.extend(render_hazards(routes))
    lines.append("")
    lines.append("## Unknown permission decorators")
    lines.append("")
    unknown: list[tuple[dict[str, Any], str]] = []
    for route in routes:
        for method, entries in route["method_permissions"].items():
            for entry in entries:
                if entry.get("unknown"):
                    unknown.append((route, method))
    if unknown:
        lines.append(
            f"{len(unknown)} method(s) carry a permission decorator whose arguments could "
            "not be extracted. Each is marked `UNKNOWN` in the tables above."
        )
        lines.append("")
        lines.append("| # | Path | Method | View |")
        lines.append("| ---: | --- | --- | --- |")
        for route, method in unknown:
            lines.append(
                f"| {route['order']} | {code(route['path'])} | {method.upper()} | "
                f"{code(short_name(route['view']))} |"
            )
        lines.append("")
    else:
        lines.append(
            "None. Every `allow_permission` wrapper's arguments were read from its closure."
        )
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def build_payload(commit_override: str | None = None) -> dict[str, Any]:
    routes, django_version, drf_version, spectacular = collect_routes()
    return OrderedDict(
        [
            (
                "generated_from",
                OrderedDict(
                    [
                        ("commit", git_commit(commit_override)),
                        ("django_version", django_version),
                        ("drf_version", drf_version),
                        ("settings_module", os.environ.get("DJANGO_SETTINGS_MODULE")),
                        ("enable_drf_spectacular", spectacular),
                    ]
                ),
            ),
            ("routes", routes),
        ]
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=SERVER_DIR,
        help="directory for routes.json and ROUTES.md (default: apps/server)",
    )
    parser.add_argument(
        "--print-hazards",
        action="store_true",
        help="print the hazard summary after writing",
    )
    parser.add_argument(
        "--commit",
        default=None,
        help=(
            "record this commit in generated_from instead of `git rev-parse HEAD`; "
            "useful for diffing two runs while other commits land"
        ),
    )
    args = parser.parse_args(argv)

    out_dir: Path = args.out_dir.resolve()
    json_path = out_dir / "routes.json"
    markdown_path = out_dir / "ROUTES.md"

    setup_django()

    payload = build_payload(args.commit)
    routes = payload["routes"]

    # Preserve Owner/Status from the existing markdown before overwriting it.
    ownership: dict[tuple[str, str, str], tuple[str, str]] = {}
    if markdown_path.exists():
        ownership = parse_ownership(markdown_path.read_text(encoding="utf-8"))
        errors = validate_ownership(routes, ownership)
        if errors:
            for error in errors:
                print(f"error: invalid Owner cell: {error}", file=sys.stderr)
            print(
                f"refusing to write {json_path.name} or {markdown_path.name}",
                file=sys.stderr,
            )
            return 2

    write_json(payload, json_path)
    # Render from the JSON we just wrote: ROUTES.md is derived from routes.json.
    payload = read_json(json_path)
    markdown = render_markdown(payload, ownership)
    markdown_path.write_text(markdown, encoding="utf-8", newline="\n")

    counts = group_counts(routes)
    unknown = sum(
        1
        for route in routes
        for entries in route["method_permissions"].values()
        for entry in entries
        if entry.get("unknown")
    )
    refused = sum(
        len(route.get("viewset_guard", {}).get("refused_methods", [])) for route in routes
    )
    print(f"wrote {json_path} ({len(routes)} routes)")
    print(f"wrote {markdown_path}")
    for group in GROUP_ORDER:
        if counts[group]["endpoints"] == 0 and group == "schema":
            continue
        print(
            f"  {group:9s} {counts[group]['endpoints']:4d} endpoints "
            f"{counts[group]['method_pairs']:4d} endpoint x method pairs"
        )
    print(f"  unknown permission decorators: {unknown}")
    print(f"  methods refused by the BaseViewSet guard: {refused}")

    if args.print_hazards:
        hazard_lines = render_hazards(routes)
        # Only the counts and disagreement lines, not the whole tables.
        for line in hazard_lines:
            if line.startswith("###") or "Found " in line or "No candidate" in line:
                print(line)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
