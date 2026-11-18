# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run `apps/api/plane/tests/contract` against a mixed Django + plane-api backend.

The contract suite is the HTTP surface oracle of the migration: `plane/tests/
contract` asserts what the API answers, and the brief says a route group is not
done until its contract tests pass on Rust (`RUST_REWRITE.md`, Verification §2).
This pytest plugin lets the suite run **unchanged** while every request whose
method + path is `rust` in `ROUTES.md` goes over real HTTP to a `plane-api`
instance and everything else stays in-process in Django, exactly as today.

## Routing

The route plan is not re-derived here: the plugin imports
`apps/server/tools/proxy/gen_caddy.py` and asks it for the same `GroupPlan`
list the proxy generator renders (`load_inventory`, `load_owners`,
`assign_groups`, `build_plan`). A request is Rust-owned when a plan cluster
matches its method and its path matches the cluster's anchored regex, and the
group's `PLANE_RS_GROUP_<NAME>` flag is unset, empty or `rust` — the same
expression the generated Caddyfile evaluates per request. Runner and proxy
therefore cannot disagree about which paths belong to Rust.

## Interception

`django.test.client.Client.request` is the single method every client call
funnels through: `Client.get/post/...` → `RequestFactory.generic` →
`Client.request`, and DRF's `APIClient` adds its `force_authenticate` user and
`credentials()` into the same call. The plugin wraps that method. For a
Rust-owned request it composes a real HTTP request from the WSGI environment
the client built (method, path, query, headers, body, cookies) and converts the
wire response back into a Django `HttpResponse` with the attributes the suite
uses: `status_code`, `content`, `.json()`, `.data`, headers and cookies. For
every other request it calls the original method untouched, so Django sees the
same request it always did.

## Authentication

`force_authenticate(user=...)` is a test-only shortcut DRF implements by
attaching `_force_auth_user` to the request; no cookie is involved. Rust has no
such hook, so for a Rust-bound request the plugin materialises the forced user
as a **real Django session** in the test database (the same keys
`django.contrib.auth.login` writes: `_auth_user_id`, `_auth_user_backend`,
`_auth_user_hash`) and sends it as the `session-id` cookie. Sessions created by
`client.force_login()`/`client.login()` already live in the client's cookie jar
and are forwarded as-is. `credentials(HTTP_X_API_KEY=...)` becomes the real
`X-API-Key` header. Unsafe methods additionally get a CSRF cookie and masked
`X-CSRFToken` header (no Rust route is unsafe today, but the plumbing is there).

A forced user that is **inactive** cannot be reproduced over HTTP: Django's
session backend (and therefore Rust) refuses to authenticate a deactivated
user, while `force_authenticate` bypasses the backend entirely. Such a request
is served in-process and reported as a runner limitation instead of being
silently sent with the wrong identity.

## Database visibility

`plane-api` is a second process, so it can only see committed rows. pytest-django
normally wraps each test in a transaction plus savepoints and rolls it back.
The plugin makes every test transactional (`pytest.mark.django_db(transaction=True)`),
which commits the ORM writes the test makes before the request and flushes the
tables afterwards — the same trade-off Django's `TransactionTestCase` makes.
`django_db_modify_db_settings` pins the test database name (`t_rsct_*`) before
pytest-django creates it, and the plugin starts one `plane-api` per session
against that database once `django_db_setup` has run. With `--reuse-db` the
database survives between runs (fast iterations, stale rows possible); pass
`--create-db` for a fresh one.

## Side effects

Tests that patch a Celery task's `.delay` assert on the Python call. A Rust
handler publishes a real AMQP message instead, so after every Rust-bound
request the plugin drains the run's own RabbitMQ vhost and, for each message
whose task's `.delay` is currently a `Mock` (`unittest.mock.patch("…delay")`
replaces the attribute on the shared task object), calls that mock with the
message's args and kwargs, with kombu's UUID envelopes restored to
`uuid.UUID`. Messages for tasks that are not mocked are dropped: Django's own
request would only have published them too, and nothing in the test consumes
them. Cache *state* assertions need no special handling — Django and Rust share
the same Redis database, and Rust performs the same cache writes a Django
request would. A test that patches a cache method itself
(`patch.object(cache, "incr", side_effect=…)`) wants an in-process Python call
Rust cannot make; `_local_mock_reason` detects that, like the patched storage
wrappers, OAuth provider classes and throttle rates, and serves the request
from Django.

## Settings

`plane-api` reads [`ENV_SETTINGS`] — `APP_BASE_URL`, `WEB_URL`, the other base
URLs, the CORS/CSRF origin lists and the session/CSRF cookie settings — from
its environment when it starts. A test that changes one in-process (pytest's
`settings` fixture, `override_settings`, `modify_settings`, or an in-place list
edit) cannot reach that process, so the request is served by Django and
reported. The value is compared with a deep copy taken in
`pytest_sessionstart`, before any test ran; a setting that ends up equal on
both sides (including a list reassigned to an equal list) never fires, and a
setting outside the list is never looked at.

## Modes and reporting

`PLANE_RS_CONTRACT_BACKEND=django` runs everything in-process through the same
wrapper (the routing decision is simply "never Rust"), which is the baseline
that proves the plugin does not change upstream results. `mixed` is the
porting check. Both modes force transactional tests, so the baseline is
comparable. At the end the plugin prints the Rust-touching tests with their
outcome, the Django-only counts, and writes a JSON report.

## Usage

`apps/server/tools/contract/run.sh --backend django` (baseline) then
`…/run.sh --backend mixed`. The README has the details and the limitations.
"""

from __future__ import annotations

import base64
import copy
import importlib
import json
import os
import re
import subprocess
import sys
import time
import unittest.mock
import urllib.error
import urllib.parse
import urllib.request
import uuid
from dataclasses import dataclass, field
from functools import partial
from pathlib import Path
from typing import Any

import pytest

TOOLS_DIR = Path(__file__).resolve().parent
SERVER_DIR = TOOLS_DIR.parents[1]
REPO_ROOT = SERVER_DIR.parents[1]
PROXY_DIR = SERVER_DIR / "tools" / "proxy"
PLANE_API_BINARY = SERVER_DIR / "target" / "debug" / "plane-api"
DEV_ENV_SCRIPT = SERVER_DIR / "dev" / "django-env.sh"

BACKENDS = ("django", "mixed")
DEFAULT_DB = "t_rsct_contract"
DEFAULT_PORT = 18200
DEFAULT_REDIS_DB = 15
DEFAULT_VHOST = "rsct_contract"
DB_PREFIX = "t_rsct"

RABBIT_HTTP = os.environ.get("PLANE_RS_CONTRACT_RABBIT_HTTP", "http://127.0.0.1:55673")
RABBIT_USER = os.environ.get("PLANE_RS_CONTRACT_RABBIT_USER", "plane")
RABBIT_PASSWORD = os.environ.get("PLANE_RS_CONTRACT_RABBIT_PASSWORD", "plane")

# Headers `requests`/urllib may add that must not leak into a comparison with
# Django's in-process request.
_HOP_BY_HOP = frozenset({"connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailers", "transfer-encoding", "upgrade"})


@dataclass(frozen=True)
class RustRoute:
    """One (method set, regex) cluster of the proxy plan."""

    group: str
    flag: str
    methods: frozenset[str]
    pattern: re.Pattern[str]
    source: str


@dataclass
class RequestRecord:
    method: str
    path: str
    backend: str
    status: int | None = None
    group: str | None = None
    error: str | None = None


@dataclass
class TestRecord:
    nodeid: str
    outcome: str = "unknown"
    rust: list[RequestRecord] = field(default_factory=list)
    django_requests: int = 0
    replayed: list[str] = field(default_factory=list)
    limitations: list[str] = field(default_factory=list)

    @property
    def touched_rust(self) -> bool:
        return bool(self.rust)


class State:
    """Session-wide state of the runner (the plugin is a singleton module)."""

    def __init__(self) -> None:
        self.backend: str = "mixed"
        self.routes: list[RustRoute] = []
        self.server: PlaneApi | None = None
        self.session: Any = None  # requests.Session
        self.rabbit: RabbitAdmin | None = None
        self.db_name: str = DEFAULT_DB
        self.redis_db: int = DEFAULT_REDIS_DB
        self.vhost: str = DEFAULT_VHOST
        self.port: int = DEFAULT_PORT
        self.report_path: Path | None = None
        self.records: dict[str, TestRecord] = {}
        self.current: TestRecord | None = None
        self.session_keys: dict[str, str] = {}
        self.replay_count: int = 0
        self.settings_baseline: dict[str, Any] | None = None


STATE = State()


# ---------------------------------------------------------------------------
# Route plan (the proxy generator's own data)
# ---------------------------------------------------------------------------


def _load_plan() -> list[RustRoute]:
    if str(TOOLS_DIR) not in sys.path:
        sys.path.insert(0, str(TOOLS_DIR))
    if str(PROXY_DIR) not in sys.path:
        sys.path.insert(0, str(PROXY_DIR))
    import gen_caddy
    import owner_override

    routes_module = gen_caddy.load_inventory_module()
    _inventory, routes = gen_caddy.load_inventory(gen_caddy.DEFAULT_ROUTES_JSON)
    owners = gen_caddy.load_owners(gen_caddy.DEFAULT_OWNERS_FILE)
    orders = owner_override.assume_rust_orders()
    if orders:
        for warning in owner_override.override_owners(routes, owners, orders):
            print(f"contract: {warning}", file=sys.stderr)
    groups = gen_caddy.load_groups(gen_caddy.DEFAULT_GROUPS_FILE, routes)
    plans = gen_caddy.build_plan(routes, owners, groups, routes_module)

    plan: list[RustRoute] = []
    for group_plan in plans:
        for cluster in group_plan.clusters:
            for regex in cluster.regexes:
                plan.append(
                    RustRoute(
                        group=group_plan.group.name,
                        flag=group_plan.group.flag,
                        methods=frozenset(cluster.methods),
                        pattern=re.compile(regex),
                        source=regex,
                    )
                )
    return owner_override.filter_methods(plan, owner_override.methods_filter())


def _rust_route(method: str, path: str) -> RustRoute | None:
    """The plan entry that owns this request, or None for Django."""
    if STATE.backend != "mixed":
        return None
    for route in STATE.routes:
        if method not in route.methods:
            continue
        # `({env.FLAG} == "" ? "rust" : {env.FLAG}) == "rust"`
        flag = os.environ.get(route.flag, "")
        if flag and flag != "rust":
            continue
        if route.pattern.fullmatch(path):
            return route
    return None


# ---------------------------------------------------------------------------
# plane-api process
# ---------------------------------------------------------------------------


class PlaneApi:
    """One `plane-api` process, started and stopped by PID."""

    def __init__(self, port: int, database: str, redis_db: int, vhost: str) -> None:
        self.port = port
        self.database = database
        self.redis_db = redis_db
        self.vhost = vhost
        self.process: subprocess.Popen[bytes] | None = None
        self.log_path = SERVER_DIR / "target" / f"contract-plane-api-{os.getpid()}.log"

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    def _environment(self) -> dict[str, str]:
        if not DEV_ENV_SCRIPT.exists():
            raise RuntimeError(f"missing {DEV_ENV_SCRIPT}")
        base = dict(os.environ)
        base.update(
            {
                "PLANE_DB": self.database,
                "PLANE_REDIS_DB": str(self.redis_db),
                "PLANE_MQ_VHOST": self.vhost,
            }
        )
        completed = subprocess.run(
            ["bash", "-c", f'source "{DEV_ENV_SCRIPT}" >/dev/null && env -0'],
            env=base,
            capture_output=True,
        )
        if completed.returncode != 0:
            raise RuntimeError(
                f"cannot read the dev environment from {DEV_ENV_SCRIPT}:\n"
                f"{completed.stderr.decode(errors='replace')}"
            )
        env: dict[str, str] = {}
        for entry in completed.stdout.split(b"\0"):
            if not entry:
                continue
            key, _, value = entry.partition(b"=")
            env[key.decode()] = value.decode()
        env["PLANE_RS_BIND"] = f"127.0.0.1:{self.port}"
        return env

    def start(self) -> None:
        if not PLANE_API_BINARY.exists():
            raise RuntimeError(
                f"{PLANE_API_BINARY} does not exist; run `cargo build -p plane-api` "
                "(tools/contract/run.sh does it)"
            )
        if _port_in_use(self.port):
            raise RuntimeError(
                f"port {self.port} is already in use; stop the process that owns it "
                "or set PLANE_RS_CONTRACT_PORT"
            )
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        log = self.log_path.open("wb")
        self.process = subprocess.Popen(
            [str(PLANE_API_BINARY)],
            cwd=str(REPO_ROOT),
            env=self._environment(),
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        self._wait_healthy(log)

    def _wait_healthy(self, log: Any) -> None:
        deadline = time.monotonic() + 60
        url = self.base_url + "/"
        while time.monotonic() < deadline:
            assert self.process is not None
            if self.process.poll() is not None:
                raise RuntimeError(
                    f"plane-api exited during startup (pid {self.process.pid}); "
                    f"log {self.log_path}:\n{_tail(self.log_path)}"
                )
            try:
                with urllib.request.urlopen(url, timeout=1) as response:  # noqa: S310 - loopback
                    if response.status == 200:
                        return
            except (urllib.error.URLError, ConnectionError, TimeoutError, OSError):
                pass
            time.sleep(0.2)
        raise RuntimeError(
            f"plane-api did not become healthy on {self.base_url}; "
            f"log {self.log_path}:\n{_tail(self.log_path)}"
        )

    def stop(self) -> None:
        process = self.process
        if process is None or process.poll() is not None:
            return
        pid = process.pid
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
        print(f"\n[plane-contract] stopped plane-api pid {pid}", file=sys.stderr)


# Modules whose storage wrapper a contract test replaces with a `Mock`, and
# whose calls the test then asserts on. `plane-storage` implements the signer
# in Rust, so there is no Python call to observe: such a request is served by
# Django (and reported) instead of failing on an assertion Rust cannot satisfy.
STORAGE_MOCK_TARGETS: tuple[tuple[str, str], ...] = (
    ("plane.settings.storage", "boto3"),
    ("plane.app.views.asset.v2", "S3Storage"),
    ("plane.app.views.issue.attachment", "S3Storage"),
    ("plane.api.views.asset", "S3Storage"),
    ("plane.api.views.issue", "S3Storage"),
    ("plane.space.views.asset", "S3Storage"),
    ("plane.authentication.adapter.base", "S3Storage"),
)

# The auth contract tests that patch a symbol inside Django's process:
# `user_activation_email` is a Celery task the adapter calls (the test asserts
# the call) and `omnimatch` is zxcvbn's matcher (the test asserts the scored
# window's length). A Rust process runs its own code and is not reachable by
# `unittest.mock`.
AUTH_MOCK_TARGETS: tuple[tuple[str, str], ...] = (
    ("plane.authentication.adapter.base", "user_activation_email"),
    ("zxcvbn.matching", "omnimatch"),
)

# The OAuth callback view modules import their provider class into the module
# namespace (`views/app/github.py:13`, `views/space/gitea.py:14`, …) and
# instantiate it there. `test_oauth_callback_state_app.py::TestOAuthCallbackState`
# patches that attribute, either to assert `provider.assert_not_called()` or to
# script the login with
# `provider.return_value.authenticate.return_value = create_user`. A Rust
# handler builds its own provider, so a Mock installed here is unreachable and
# the scripted-login cases have no user; those requests are served by Django.
# Every callback view of both apps is listed, not only the five the test uses
# today, so a new parametrisation needs no runner change.
AUTH_PROVIDER_MOCK_TARGETS: tuple[tuple[str, str], ...] = (
    ("plane.authentication.views.app.github", "GitHubOAuthProvider"),
    ("plane.authentication.views.app.gitlab", "GitLabOAuthProvider"),
    ("plane.authentication.views.app.google", "GoogleOAuthProvider"),
    ("plane.authentication.views.app.gitea", "GiteaOAuthProvider"),
    ("plane.authentication.views.space.github", "GitHubOAuthProvider"),
    ("plane.authentication.views.space.gitlab", "GitLabOAuthProvider"),
    ("plane.authentication.views.space.google", "GoogleOAuthProvider"),
    ("plane.authentication.views.space.gitea", "GiteaOAuthProvider"),
)

# `test_user_email_change_app.py::…::test_code_invalidated_after_attempt_is_counted_is_not_compared`
# wraps `cache.incr` in a `side_effect` that deletes the code and its counter
# between the view's attempt increment and its code read
# (`plane/app/views/user/base.py:221-222`). The check is the cache API surface a
# test can patch on the shared object: a `Mock` there means the test expects
# its own Python call to run in-process.
CACHE_MOCK_METHODS: tuple[str, ...] = (
    "add",
    "clear",
    "decr",
    "delete",
    "delete_many",
    "delete_pattern",
    "get",
    "get_many",
    "get_or_set",
    "has_key",
    "incr",
    "set",
    "set_many",
    "touch",
)

# `patch.object(AuthenticationThrottle, "rate", "2/minute")` assigns a plain
# string, not a `Mock`, so it is detected by comparing the class attribute with
# the value its module read from the runner's environment at import time.
AUTH_PATCHED_RATES: tuple[tuple[str, str, str, str], ...] = (
    (
        "plane.authentication.rate_limit",
        "AuthenticationThrottle",
        "rate",
        "AUTHENTICATION_RATE_LIMIT",
    ),
)

# Django settings that `plane-api` reads from its environment **once, at
# startup** and that a rust-owned route's response depends on. The suite
# overrides `settings.APP_BASE_URL` with pytest's `settings` fixture
# (`plane/tests/contract/app/test_oauth_callback_state_app.py:107-108`), and a
# future test can override any of the others the same way or through
# `override_settings`/`modify_settings`. A separate process cannot see an
# in-process change, so such a request has to be served by Django.
#
# Only settings whose value the two processes derive identically from the same
# environment belong here. `DATABASE_URL`, `REDIS_URL`, `SECRET_KEY` and the
# email settings are deliberately absent: the first two are per-process
# plumbing, `SECRET_KEY` must not be printed in a report, and the email
# credentials are read through `get_configuration_value` (DB first, env
# fallback), so an env value is not the whole story. `ALLOWED_HOSTS` is absent
# too: Django's `setup_test_environment()` appends `"testserver"` to it
# (`django/test/utils.py`), so it always differs from the value `plane-api`
# read even though no test touched it. `plane-core/config.rs` reads every name
# below (`UrlSettings`, `CookieSettings`, `CorsSettings`).
ENV_SETTINGS: tuple[str, ...] = (
    "APP_BASE_URL",
    "WEB_URL",
    "ADMIN_BASE_URL",
    "SPACE_BASE_URL",
    "LIVE_BASE_URL",
    "CORS_ALLOWED_ORIGINS",
    "CSRF_TRUSTED_ORIGINS",
    "SESSION_COOKIE_NAME",
    "SESSION_COOKIE_AGE",
    "SESSION_COOKIE_SECURE",
    "SESSION_COOKIE_DOMAIN",
    "CSRF_COOKIE_SECURE",
    "CSRF_COOKIE_DOMAIN",
)

# `django-cors-headers` defines no default for `CORS_ALLOWED_ORIGINS` and Plane
# only assigns the setting when `CORS_ALLOWED_ORIGINS` is non-empty
# (`plane/settings/common.py:191-197`), so it is absent from `settings` in the
# dev environment while its effective value is the empty allow-list. Comparing
# `[]` with the attribute's absence keeps an override to `[]` silent.
SETTING_DEFAULTS: dict[str, Any] = {"CORS_ALLOWED_ORIGINS": []}

_MISSING = object()

# Tests that assert an exception is raised *inside* Django's process. They wrap
# the client call in `try/except` (or `pytest.raises`) and accept either a 4xx
# response or the exception itself; Django's test client re-raises it, while a
# separate Rust process can only answer with the 500 page the exception produces
# over HTTP. The two magic-generate tests are the auth case: an empty or invalid
# address raises `ValidationError` from `views/app/magic.py:52-53`, outside the
# view's `except AuthenticationException`, and DRF's handler re-raises it.
IN_PROCESS_EXCEPTION_TESTS: tuple[tuple[str, str], ...] = (
    (
        "test_authentication.py::TestMagicLinkGenerate::test_without_data",
        "asserts the empty-email `ValidationError` propagates in-process "
        "(`views/app/magic.py:52-53`)",
    ),
    (
        "test_authentication.py::TestMagicLinkGenerate::test_email_validity",
        "asserts the invalid-email `ValidationError` propagates in-process "
        "(`views/app/magic.py:52-53`)",
    ),
)


def _local_mock_reason() -> str | None:
    """A `unittest.mock` replacement that only an in-process Django view can satisfy.

    The asset routes presign through the storage wrapper. Their contract tests
    assert on the *mock's* call (`s3_client.generate_presigned_url.call_args`,
    `S3Storage.return_value.generate_presigned_url.assert_called_once()`), which
    a Rust handler (own signer, different process) can never produce. Such a
    request is served by Django and reported, rather than failing on an
    assertion about an object Rust does not use.

    The auth routes have the same shape of test: `user_activation_email.delay`
    is mocked (the test asserts the mock was called), `zxcvbn.matching.omnimatch`
    is wrapped (the test asserts the argument length) and
    `AuthenticationThrottle.rate` is patched to a small rate so a test does not
    have to fire ten requests
    (`plane/tests/contract/app/test_authentication.py::TestAuthenticationThrottle`).
    Django reads the class attribute per request; `plane-api` reads
    `AUTHENTICATION_RATE_LIMIT` once at startup, so a patch in this process
    cannot reach it. The differential scenario
    `scenarios/write/auth-magic-throttle.yaml` covers the behaviour on Rust
    instead.

    The check looks at the effect (the module attribute is a `Mock`, or the class
    attribute differs from the value the module read at import time) rather than
    `unittest.mock._patch._active_patches`: the context-manager form of `patch`
    does not register there.

    Further families are detected by their effect:

    * An OAuth provider class on a callback view module
      (`[AUTH_PROVIDER_MOCK_TARGETS]`). The view instantiates the module
      attribute, so the mock is only used in-process; the test then asserts on
      it (`provider.assert_not_called()`) or scripts the login through it
      (`test_oauth_callback_state_app.py::TestOAuthCallbackState`). None of
      these tests can pass against Rust, and `assert_not_called` can pass
      vacuously there, so they must not be sent.
    * A method of the shared cache object (`[CACHE_MOCK_METHODS]`).
      `test_user_email_change_app.py::TestUserEmailChangeVerification::
      test_code_invalidated_after_attempt_is_counted_is_not_compared` replaces
      `cache.incr` with a `side_effect` that deletes the code and its counter
      between the attempt increment and the code read
      (`plane/app/views/user/base.py:221-222`); the effect only exists in
      Django's process. `django.core.cache.cache` is a
      `ConnectionProxy`, so `patch.object(cache, …)` resolves the real method
      through `getattr(cache, …)` but installs the mock on the backend instance
      through `setattr`, where the proxy still exposes it for the block's
      duration (the Django view imports the same proxy,
      `plane/app/views/user/base.py:19`).
    * A setting in [`ENV_SETTINGS`] whose value differs from the snapshot taken
      at session start (`_settings_override_reason`). `plane-api` reads its
      settings from the environment once, so `settings.APP_BASE_URL = …` (the
      `settings` fixture) or `override_settings(...)` changes only Django's
      answer; the request must be served in-process.
    """
    for module_name, attribute in STORAGE_MOCK_TARGETS + AUTH_MOCK_TARGETS:
        module = sys.modules.get(module_name)
        if module is None:
            continue
        value = getattr(module, attribute, None)
        if isinstance(value, unittest.mock.Mock):
            return (
                f"asserts on an in-process `{module_name}.{attribute}` mock; "
                "served by Django"
            )
    for module_name, class_name in AUTH_PROVIDER_MOCK_TARGETS:
        module = sys.modules.get(module_name)
        if module is None:
            continue
        if isinstance(getattr(module, class_name, None), unittest.mock.Mock):
            return (
                f"replaces the in-process OAuth provider `{module_name}.{class_name}` "
                "with a mock (the test asserts on it or scripts its login); served by Django"
            )
    cache_reason = _cache_mock_reason()
    if cache_reason is not None:
        return cache_reason
    settings_reason = _settings_override_reason()
    if settings_reason is not None:
        return settings_reason
    for module_name, class_name, attribute, env_name in AUTH_PATCHED_RATES:
        module = sys.modules.get(module_name)
        if module is None:
            continue
        cls = getattr(module, class_name, None)
        if cls is None:
            continue
        expected = os.environ.get(env_name, "10/minute")
        current = getattr(cls, attribute, None)
        if current != expected:
            return (
                f"patches `{module_name}.{class_name}.{attribute}` to {current!r} per test; "
                f"the Rust process read {expected!r} at startup — served by Django"
            )
    return _in_process_exception_reason()


def _cache_mock_reason() -> str | None:
    """A `Mock` installed on the shared Django cache object by a test patch.

    `django.core.cache.cache` is a `django.utils.connection.ConnectionProxy`, not
    the backend itself. `patch.object(cache, "incr", side_effect=…)` asks the
    proxy for `incr` (which delegates to the backend's real bound method for the
    saved original) and assigns the mock back through the proxy's `__setattr__`,
    which sets it on the backend instance. While the block is active,
    `getattr(cache, "incr")` therefore returns the `Mock`; on exit `patch` puts
    the original bound method back as an instance attribute, so the check sees a
    real method again and an unrelated test does not match. The views import the
    same proxy (`plane/app/views/user/base.py:19`), so this object is the whole
    surface a test can patch.
    """
    module = sys.modules.get("django.core.cache")
    if module is None:
        return None
    cache = getattr(module, "cache", None)
    if cache is None:
        return None
    for attribute in CACHE_MOCK_METHODS:
        try:
            value = getattr(cache, attribute, None)
        except Exception:  # a closed or misconfigured backend must not break routing
            continue
        if isinstance(value, unittest.mock.Mock):
            return (
                f"patches `django.core.cache.cache.{attribute}` in-process to observe "
                "or steer a cache call the view makes itself; served by Django"
            )
    return None


def _in_process_exception_reason() -> str | None:
    """Whether this test expects an exception to propagate inside Django.

    `DjangoTestClient` re-raises a view's uncaught exception; a request answered
    by `plane-api` can only return the 500 page that exception produces. See
    [`IN_PROCESS_EXCEPTION_TESTS`].
    """
    if STATE.current is None:
        return None
    nodeid = STATE.current.nodeid
    for suffix, why in IN_PROCESS_EXCEPTION_TESTS:
        if nodeid.endswith(suffix):
            return f"{why}; served by Django"
    return None


def _settings_value(name: str) -> Any:
    """`settings.<name>` right now, as a detached copy (`_MISSING` if absent)."""
    from django.conf import settings

    value = getattr(settings, name, _MISSING)
    if value is _MISSING:
        value = SETTING_DEFAULTS.get(name, _MISSING)
    if value is _MISSING:
        return _MISSING
    try:
        # Lists and dicts are mutable; an in-place edit
        # (`settings.CSRF_TRUSTED_ORIGINS.append(…)`) must not change the value
        # it is compared against.
        return copy.deepcopy(value)
    except Exception:  # a setting that cannot be copied is compared in place
        return value


def _snapshot_env_settings() -> None:
    """Record the settings `plane-api` read at startup, once, before any test.

    `plane-api` resolves [`ENV_SETTINGS`] from its environment when the process
    starts. The runner copies Django's values at the equivalent point of the
    session: `pytest_sessionstart`, after pytest-django has configured the
    settings module and before collection, so no test can have overridden
    anything yet. A deep copy is stored so a test that mutates a list setting
    in place (`CORS_ALLOWED_ORIGINS`, `CSRF_TRUSTED_ORIGINS`) is still visible
    as a difference.
    """
    STATE.settings_baseline = {name: _settings_value(name) for name in ENV_SETTINGS}


def _settings_override_reason() -> str | None:
    """A setting the test changed in-process that `plane-api` read at startup.

    `plane-api` is a separate process, so `settings.APP_BASE_URL = "…"` in a
    test (`plane/tests/contract/app/test_oauth_callback_state_app.py:107-108`)
    or `override_settings(...)` changes Django's answer only. The check is
    effect-based: the current value of every setting in [`ENV_SETTINGS`] is
    compared with the deep copy taken in `pytest_sessionstart`, before any test
    ran. Values that are equal — including a list reassigned to an equal list,
    or a lazily built value that resolves to the same result — never fire, and
    an unrelated setting (anything outside the list) never fires.
    """
    if STATE.settings_baseline is None:
        # Direct use without `pytest_sessionstart` (the smoke tests do call the
        # detector directly); any test override is still ahead of this point.
        _snapshot_env_settings()
    baseline = STATE.settings_baseline or {}
    for name in ENV_SETTINGS:
        original = baseline.get(name, _MISSING)
        current = _settings_value(name)
        if current is _MISSING and original is _MISSING:
            continue
        if current == original:
            continue
        return (
            f"overrides `settings.{name}` in-process ({current!r}); "
            f"plane-api read {original!r} at startup — served by Django"
        )
    return None


def _debug() -> bool:
    return bool(os.environ.get("PLANE_RS_CONTRACT_DEBUG"))


def _task_is_eager(task: Any) -> bool:
    """Whether the task's app runs `.delay()` in-process (`task_always_eager`).

    A test that flips Celery into eager mode asserts on the rows the task
    writes, not on a published message, and the drain/replay step has to run the
    task for a Rust-bound request the same way `apply_async` does for a
    Django-bound one.
    """
    app = getattr(task, "app", None)
    conf = getattr(app, "conf", None)
    return bool(getattr(conf, "task_always_eager", False))


def _tail(path: Path, lines: int = 40) -> str:
    try:
        return "\n".join(path.read_text(errors="replace").splitlines()[-lines:])
    except OSError:
        return "<no log>"


def _port_in_use(port: int) -> bool:
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(0.2)
        return sock.connect_ex(("127.0.0.1", port)) == 0


# ---------------------------------------------------------------------------
# RabbitMQ: drain the run's own vhost, replay into patched `.delay` mocks
# ---------------------------------------------------------------------------


class RabbitAdmin:
    def __init__(self, base: str, user: str, password: str) -> None:
        self.base = base.rstrip("/")
        token = base64.b64encode(f"{user}:{password}".encode()).decode()
        self.authorization = f"Basic {token}"

    def _request(
        self, method: str, path: str, payload: dict[str, Any] | None = None
    ) -> tuple[int, Any]:
        data = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request(
            self.base + urllib.parse.quote(path, safe="/"),
            data=data,
            method=method,
        )
        request.add_header("Authorization", self.authorization)
        if data is not None:
            request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=5) as response:  # noqa: S310 - loopback
                body = response.read()
                return response.status, json.loads(body) if body else None
        except urllib.error.HTTPError as error:
            body = error.read()
            try:
                parsed = json.loads(body) if body else None
            except ValueError:
                parsed = None
            return error.code, parsed

    def ensure_vhost(self, vhost: str) -> None:
        status, _ = self._request("PUT", f"/api/vhosts/{vhost}", {})
        if status not in (201, 204):
            raise RuntimeError(f"cannot create RabbitMQ vhost {vhost!r}: HTTP {status}")
        status, _ = self._request(
            "PUT",
            f"/api/permissions/{vhost}/{RABBIT_USER}",
            {"configure": ".*", "write": ".*", "read": ".*"},
        )
        if status not in (201, 204):
            raise RuntimeError(f"cannot grant permissions on vhost {vhost!r}: HTTP {status}")

    def delete_vhost(self, vhost: str) -> None:
        self._request("DELETE", f"/api/vhosts/{vhost}")

    def queues(self, vhost: str) -> list[str]:
        status, payload = self._request("GET", f"/api/queues/{vhost}")
        if status == 404:
            return []
        if status != 200 or not isinstance(payload, list):
            raise RuntimeError(f"cannot list queues in vhost {vhost!r}: HTTP {status}")
        names = [item.get("name") for item in payload if isinstance(item, dict)]
        return sorted(name for name in names if isinstance(name, str))

    def get(self, vhost: str, queue: str, count: int = 100) -> list[dict[str, Any]]:
        status, payload = self._request(
            "POST",
            f"/api/queues/{vhost}/{queue}/get",
            {
                "count": count,
                "ackmode": "ack_requeue_false",
                "encoding": "auto",
                "truncate": 100_000,
            },
        )
        if status == 404:
            return []
        if status != 200 or not isinstance(payload, list):
            raise RuntimeError(
                f"cannot read queue {queue!r} in vhost {vhost!r}: HTTP {status}"
            )
        return [item for item in payload if isinstance(item, dict)]


def _decode_kombu(value: Any) -> Any:
    """Restore kombu's JSON envelopes (`uuid`, `decimal`, `bytes`) to Python objects."""
    if isinstance(value, dict):
        marker = value.get("__type__")
        inner = value.get("__value__")
        if marker == "uuid" and isinstance(inner, dict) and isinstance(inner.get("hex"), str):
            return uuid.UUID(hex=inner["hex"])
        if marker == "decimal" and isinstance(inner, str):
            from decimal import Decimal

            return Decimal(inner)
        if marker == "bytes" and isinstance(inner, str):
            return base64.b64decode(inner)
        return {key: _decode_kombu(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_decode_kombu(item) for item in value]
    return value


def _drain_and_replay(admin: RabbitAdmin, record: TestRecord) -> None:
    """Replay every published task into a patched `.delay` mock, if there is one."""
    messages: list[dict[str, Any]] = []
    queues = admin.queues(STATE.vhost)
    if _debug():
        print(f"[plane-contract] draining queues {queues} in {STATE.vhost}", file=sys.stderr)
    for queue in queues:
        drained = admin.get(STATE.vhost, queue)
        if _debug() and drained:
            print(f"[plane-contract] {queue}: {len(drained)} message(s)", file=sys.stderr)
        messages.extend(drained)
    for message in messages:
        # Celery's body is `[args, kwargs, embed]` (kombu's `dumps` of the
        # protocol-2 message); the task name is in the AMQP headers, which the
        # management API exposes under `properties`.
        body = message.get("payload")
        if isinstance(body, str):
            try:
                body = json.loads(body)
            except ValueError:
                continue
        properties = message.get("properties") or {}
        headers = properties.get("headers") or {}
        task_name = headers.get("task")
        if not isinstance(task_name, str):
            continue
        if isinstance(body, dict):  # a hand-built message, not Celery's shape
            args = body.get("args") or []
            kwargs = body.get("kwargs") or {}
        elif isinstance(body, list) and len(body) >= 2:
            args = body[0] or []
            kwargs = body[1] or {}
        else:
            continue
        module_name, _, attribute = task_name.rpartition(".")
        try:
            task = getattr(importlib.import_module(module_name), attribute)
        except (ImportError, AttributeError):
            record.limitations.append(f"cannot resolve task {task_name!r} for replay")
            continue
        delay = getattr(task, "delay", None)
        if _debug():
            print(
                f"[plane-contract] task {task_name!r} delay is "
                f"{type(delay).__name__} ({'mock' if isinstance(delay, unittest.mock.Mock) else 'real'})",
                file=sys.stderr,
            )
        if not isinstance(delay, unittest.mock.Mock):
            # A test may patch the task at the call site
            # (`with mock.patch("module.view.issue_activity") as activity:`)
            # instead of patching `.delay` on the shared task object. The view
            # module's attribute is then a `Mock` named after the task; replay
            # into its `.delay` so the test sees the Rust message. Without
            # this, a mixed-backend run leaves `call_args` as `None`.
            call_sites = _call_site_mocks(attribute)
            if call_sites:
                for call_site in call_sites:
                    call_site.delay(*_decode_kombu(args), **_decode_kombu(kwargs))
                STATE.replay_count += 1
                record.replayed.append(task_name)
                continue
            if _task_is_eager(task):
                # `task_always_eager=True`: the test expects the task's *ORM
                # writes*, not a published message, and Django's own request
                # runs it in-process (`Task.apply` inside `apply_async`). Run
                # the same call here so a Rust-bound request produces the same
                # rows; without it such a test can never observe them.
                if _debug():
                    print(
                        f"[plane-contract] replaying {task_name!r} eagerly",
                        file=sys.stderr,
                    )
                delay(*_decode_kombu(args), **_decode_kombu(kwargs))
                STATE.replay_count += 1
                record.replayed.append(task_name)
                continue
            # Not patched and not eager: Django would only have published the
            # message too, and nothing in the test consumes it.
            continue
        delay(*_decode_kombu(args), **_decode_kombu(kwargs))
        STATE.replay_count += 1
        record.replayed.append(task_name)


def _call_site_mocks(attribute: str) -> list[unittest.mock.Mock]:
    """Every active `mock.patch("…<attribute>")` whose target is the task.

    A view module may be patched both by an autouse fixture and by the test
    that asserts on the call, so all matching attribute mocks receive the
    replay; the assertion then sees the message whichever module it patched.
    """
    found: list[unittest.mock.Mock] = []
    seen: set[int] = set()
    for module in list(sys.modules.values()):
        if module is None:
            continue
        try:
            value = getattr(module, attribute, None)
        except Exception:
            continue
        if (
            isinstance(value, unittest.mock.Mock)
            and getattr(value, "_mock_name", None) == attribute
            and id(value) not in seen
        ):
            seen.add(id(value))
            found.append(value)
    if _debug() and found:
        print(
            f"[plane-contract] call-site mocks for {attribute!r}: {len(found)}",
            file=sys.stderr,
        )
    return found


# ---------------------------------------------------------------------------
# Sessions and CSRF for Rust-bound requests
# ---------------------------------------------------------------------------


def _forced_user(client: Any) -> Any:
    handler = getattr(client, "handler", None)
    user = getattr(handler, "_force_user", None)
    if user is None:
        return None
    if not getattr(user, "is_active", True):
        return None
    return user


def _session_key(user: Any) -> str:
    """Create (once per test) the Django session `force_authenticate` stands in for."""
    cache_key = str(user.pk)
    cached = STATE.session_keys.get(cache_key)
    if cached is not None:
        return cached

    from django.conf import settings
    from django.contrib.auth import BACKEND_SESSION_KEY, HASH_SESSION_KEY, SESSION_KEY
    from django.utils.module_loading import import_string

    store_class = import_string(settings.SESSION_ENGINE + ".SessionStore")
    store = store_class()
    store[SESSION_KEY] = user._meta.pk.value_to_string(user)
    store[BACKEND_SESSION_KEY] = settings.AUTHENTICATION_BACKENDS[0]
    store[HASH_SESSION_KEY] = user.get_session_auth_hash()
    store.create()
    STATE.session_keys[cache_key] = store.session_key
    return store.session_key


def _csrf_pair() -> tuple[str, str]:
    from django.middleware.csrf import _get_new_csrf_string, _mask_cipher_secret

    secret = _get_new_csrf_string()
    return secret, _mask_cipher_secret(secret)


# ---------------------------------------------------------------------------
# The HTTP request / response bridge
# ---------------------------------------------------------------------------


def _header_name(wsgi_key: str) -> str:
    return "-".join(part.capitalize() for part in wsgi_key[5:].split("_"))


def _quote_path(path: str) -> str:
    """Percent-encode a decoded path for the wire, leaving existing escapes alone."""
    return urllib.parse.quote(path, safe="/%:@!$&'()*+,;=-._~")


def _cookie_header(client: Any) -> str | None:
    cookies = getattr(client, "cookies", None)
    if not cookies:
        return None
    return "; ".join(f"{morsel.key}={morsel.coded_value}" for morsel in cookies.values())


def _build_request(
    client: Any, environ: dict[str, Any], route: RustRoute
) -> tuple[str, str, str, dict[str, str], bytes | None, list[str]]:
    """Turn the WSGI environment Django's test client built into an HTTP request."""
    method = environ["REQUEST_METHOD"].upper()
    path = environ["PATH_INFO"]
    query = environ.get("QUERY_STRING") or ""
    url = STATE.server.base_url + _quote_path(path)
    if query:
        url += "?" + query

    headers: dict[str, str] = {}
    for key, value in environ.items():
        if key.startswith("HTTP_"):
            name = _header_name(key)
            if name.lower() not in _HOP_BY_HOP:
                headers[name] = value
        elif key == "CONTENT_TYPE":
            headers["Content-Type"] = value
        elif key == "CONTENT_LENGTH":
            headers["Content-Length"] = value
    headers.setdefault("Host", "testserver")

    cookie = headers.get("Cookie")
    if cookie is None:
        cookie = _cookie_header(client)
    limitations: list[str] = []
    forced = _forced_user(client)
    if forced is not None:
        session_cookie = f"session-id={_session_key(forced)}"
        cookie = f"{cookie}; {session_cookie}" if cookie else session_cookie
    elif getattr(getattr(client, "handler", None), "_force_user", None) is not None:
        limitations.append("inactive forced user: served by Django (cannot authenticate over HTTP)")
    if cookie:
        headers["Cookie"] = cookie
    if method not in ("GET", "HEAD", "OPTIONS", "TRACE"):
        secret, token = _csrf_pair()
        headers["X-CSRFToken"] = token
        cookie = headers.get("Cookie")
        csrf_cookie = f"csrftoken={secret}"
        headers["Cookie"] = f"{cookie}; {csrf_cookie}" if cookie else csrf_cookie

    body: bytes | None = None
    stream = environ.get("wsgi.input")
    content_length = environ.get("CONTENT_LENGTH")
    if stream is not None and content_length:
        body = stream.read(int(content_length))
    return method, path, url, headers, body, limitations


def _parse_json_body(content: bytes, content_type: str) -> Any:
    if not content or "json" not in content_type.lower():
        return None
    try:
        return json.loads(content)
    except ValueError:
        return None


def _build_response(
    client: Any, environ: dict[str, Any], wire: Any, route: RustRoute
) -> Any:
    from django.http import HttpResponse
    from django.http.response import ResponseHeaders

    response = HttpResponse(content=wire.content, status=wire.status_code)
    response.headers = ResponseHeaders({})
    cookies = []
    for name, value in wire.headers.items():
        if name.lower() in _HOP_BY_HOP or name.lower() == "content-length":
            continue
        if name.lower() == "set-cookie":
            cookies.append(value)
            continue
        response.headers[name] = value
    for value in cookies:
        response.cookies.load(value)
        if "set-cookie" not in {key.lower() for key in response.headers}:
            response.headers["Set-Cookie"] = value

    response.client = client
    response.request = environ
    response.templates = []
    response.context = None
    response.resolver_match = None
    response.wsgi_request = None
    response.json = partial(client._parse_json, response)
    response.data = _parse_json_body(response.content, response.headers.get("Content-Type", ""))
    # `HttpResponseRedirectBase.url` is `property(lambda self: self["Location"])`
    # (`django/http/response.py:190-205`). A Django `HttpResponse` built here has
    # no such attribute, and the tests read it: `response.url` on a redirect, and
    # `Client._handle_redirects`/`_follow_redirect` follow it for `follow=True`
    # (`django/test/client.py:958-997,1372-1397`).
    location = response.headers.get("Location")
    if location is not None:
        response.url = location
    response.was_rust = True
    response.rust_group = route.group
    return response


def _dispatch_to_rust(client: Any, environ: dict[str, Any], route: RustRoute) -> Any:
    if STATE.session is None or STATE.server is None:
        raise RuntimeError("the plane-api HTTP session is not initialised")
    method, path, url, headers, body, limitations = _build_request(client, environ, route)
    record = STATE.current
    entry = RequestRecord(method=method, path=path, backend="rust", group=route.group)
    if record is not None:
        record.rust.append(entry)
        record.limitations.extend(limitations)
    try:
        wire = STATE.session.request(
            method,
            url,
            headers=headers,
            data=body,
            allow_redirects=False,
            timeout=120,
        )
    except Exception as error:  # noqa: BLE001 - surfaced as a test failure
        entry.error = repr(error)
        raise AssertionError(
            f"plane-api request failed: {method} {path}: {error!r}\n"
            f"log {STATE.server.log_path}:\n{_tail(STATE.server.log_path)}"
        ) from error
    entry.status = wire.status_code
    if STATE.rabbit is not None and record is not None:
        _drain_and_replay(STATE.rabbit, record)
    response = _build_response(client, environ, wire, route)
    # `Client.request`'s "Update persistent cookie data"
    # (`django/test/client.py:1104-1106`): `self.cookies.update(response.cookies)`.
    # The Rust branch returns from the patched `request` before that line, so
    # without this the client's jar — and therefore `response.client.session` /
    # `response.client.cookies` for the next step — never sees the target's
    # `session-id`/`csrftoken` cookies.
    if response.cookies:
        client.cookies.update(response.cookies)
    return response


# ---------------------------------------------------------------------------
# Plugin: routing decision wrapper
# ---------------------------------------------------------------------------

_ORIGINAL_REQUEST: Any = None


def _install_client_patch() -> None:
    """Wrap `django.test.client.Client.request` once per session."""
    global _ORIGINAL_REQUEST
    if _ORIGINAL_REQUEST is not None:
        return

    import django.test.client

    original = django.test.client.Client.request

    def request(self: Any, **environ: Any) -> Any:
        merged = {**getattr(self, "defaults", {}), **environ}
        method = merged.get("REQUEST_METHOD")
        path = merged.get("PATH_INFO")
        if isinstance(method, str) and isinstance(path, str):
            route = _rust_route(method.upper(), path)
            if route is not None:
                reason = _local_mock_reason()
                if reason is None:
                    return _dispatch_to_rust(self, merged, route)
                if STATE.current is not None:
                    STATE.current.limitations.append(reason)
                    STATE.current.django_requests += 1
            record = STATE.current
            if record is not None:
                record.django_requests += 1
        return original(self, **environ)

    django.test.client.Client.request = request  # type: ignore[method-assign]
    _ORIGINAL_REQUEST = original


# ---------------------------------------------------------------------------
# pytest hooks
# ---------------------------------------------------------------------------


def pytest_addoption(parser: pytest.Parser) -> None:
    group = parser.getgroup("plane-contract")
    group.addoption(
        "--plane-contract-backend",
        action="store",
        default=None,
        choices=BACKENDS,
        help="django: everything in-process; mixed: ROUTES.md owners decide (default: $PLANE_RS_CONTRACT_BACKEND or mixed)",
    )


def pytest_configure(config: pytest.Config) -> None:
    STATE.records = {}
    STATE.settings_baseline = None
    backend = config.getoption("plane_contract_backend") or os.environ.get(
        "PLANE_RS_CONTRACT_BACKEND", "mixed"
    )
    if backend not in BACKENDS:
        raise pytest.UsageError(
            f"unknown PLANE_RS_CONTRACT_BACKEND {backend!r}; expected one of {', '.join(BACKENDS)}"
        )
    STATE.backend = backend
    STATE.db_name = os.environ.get("PLANE_RS_CONTRACT_DB", DEFAULT_DB)
    if not STATE.db_name.startswith(DB_PREFIX):
        raise pytest.UsageError(
            f"PLANE_RS_CONTRACT_DB {STATE.db_name!r} must start with {DB_PREFIX!r}"
        )
    STATE.port = int(os.environ.get("PLANE_RS_CONTRACT_PORT", DEFAULT_PORT))
    STATE.redis_db = int(os.environ.get("PLANE_RS_CONTRACT_REDIS_DB", DEFAULT_REDIS_DB))
    STATE.vhost = os.environ.get("PLANE_RS_CONTRACT_VHOST", DEFAULT_VHOST)
    report = os.environ.get("PLANE_RS_CONTRACT_REPORT")
    STATE.report_path = Path(report) if report else SERVER_DIR / "target" / f"contract-{backend}.json"
    if backend == "mixed":
        try:
            STATE.routes = _load_plan()
        except SystemExit as error:  # gen_caddy's error path
            raise pytest.UsageError(f"cannot build the proxy plan: {error}") from error
        if not STATE.routes:
            raise pytest.UsageError(
                "the proxy plan is empty: no route in ROUTES.md is owned by rust"
            )
    _install_client_patch()


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Every DB test must commit, so the second process can see its rows."""
    del config
    for item in items:
        marker = item.get_closest_marker("django_db")
        if marker is not None:
            marker.kwargs["transaction"] = True
        elif "db" in item.fixturenames:
            item.fixturenames.append("transactional_db")


def pytest_sessionstart(session: pytest.Session) -> None:
    """Pin the test database name before pytest-django creates it.

    This runs after pytest-django has configured settings (`pytest_configure`
    may still be ahead of that) and before collection, so the `TEST["NAME"]`
    pytest-django reads in `django_db_setup` is already ours. A fixture with
    the same name as pytest-django's `django_db_modify_db_settings` would be
    shadowed by the `-p` plugin's definition, which is why this is a hook.
    """
    del session
    from django.conf import settings

    settings.DATABASES["default"].setdefault("TEST", {})
    settings.DATABASES["default"]["TEST"]["NAME"] = STATE.db_name
    # Django is configured and no test has run, which is the same point in the
    # session at which `plane-api` resolves its own settings from the
    # environment. `_settings_override_reason` compares against this snapshot.
    _snapshot_env_settings()


@pytest.fixture(scope="session")
def plane_contract_server(django_db_setup: None) -> Any:  # noqa: ARG001, F811
    """Prepare the run's broker vhost and, in mixed mode, one `plane-api`."""
    import requests

    # Django's own views publish to the broker on the paths the suite exercises
    # (no eager mode in Plane's settings), so the vhost has to exist in both
    # modes; upstream would have a fresh one from its compose stack.
    STATE.rabbit = RabbitAdmin(RABBIT_HTTP, RABBIT_USER, RABBIT_PASSWORD)
    STATE.rabbit.ensure_vhost(STATE.vhost)
    try:
        if STATE.backend == "mixed":
            STATE.session = requests.Session()
            STATE.session.trust_env = False
            STATE.server = PlaneApi(STATE.port, STATE.db_name, STATE.redis_db, STATE.vhost)
            STATE.server.start()
            print(
                f"\n[plane-contract] plane-api pid {STATE.server.process.pid} on "
                f"{STATE.server.base_url}, db {STATE.db_name}, redis {STATE.redis_db}, "
                f"vhost {STATE.vhost}",
                file=sys.stderr,
            )
        yield STATE.server
    finally:
        if STATE.server is not None:
            STATE.server.stop()
            STATE.server = None
        if STATE.session is not None:
            STATE.session.close()
            STATE.session = None
        STATE.rabbit.delete_vhost(STATE.vhost)
        STATE.rabbit = None


@pytest.fixture(autouse=True)
def _plane_contract_state(plane_contract_server: Any, request: pytest.FixtureRequest) -> Any:
    """Reset the per-test record and sessions; drain leftovers before each test."""
    del plane_contract_server
    STATE.session_keys = {}
    if STATE.current is not None:
        STATE.records[STATE.current.nodeid] = STATE.current
    _ = request
    yield
    if STATE.current is not None:
        STATE.records[STATE.current.nodeid] = STATE.current
        STATE.current = None


def pytest_runtest_setup(item: pytest.Item) -> None:
    STATE.current = TestRecord(nodeid=item.nodeid)
    STATE.session_keys = {}
    if STATE.backend == "mixed" and STATE.rabbit is not None:
        # Leftovers can only come from an earlier request that failed mid-flight;
        # drop them rather than attributing them to this test.
        for queue in STATE.rabbit.queues(STATE.vhost):
            STATE.rabbit.get(STATE.vhost, queue)


def pytest_runtest_logreport(report: pytest.TestReport) -> None:
    record = STATE.records.get(report.nodeid)
    if record is None:
        record = TestRecord(nodeid=report.nodeid)
        STATE.records[report.nodeid] = record
    if report.when == "call":
        record.outcome = "passed" if report.passed else "failed"
    elif report.when == "setup" and report.failed:
        record.outcome = "error"
    elif report.when == "teardown" and report.failed and record.outcome == "unknown":
        record.outcome = "error"


def pytest_terminal_summary(
    terminalreporter: Any, exitstatus: int, config: pytest.Config
) -> None:
    del exitstatus
    records = sorted(STATE.records.values(), key=lambda record: record.nodeid)
    rust_tests = [record for record in records if record.touched_rust]
    django_tests = [record for record in records if not record.touched_rust]
    rust_requests = sum(len(record.rust) for record in rust_tests)
    django_requests = sum(record.django_requests for record in records)

    def counts(records: list[TestRecord]) -> dict[str, int]:
        result = {"passed": 0, "failed": 0, "error": 0, "skipped": 0, "unknown": 0}
        for record in records:
            result[record.outcome] = result.get(record.outcome, 0) + 1
        return result

    rust_counts = counts(rust_tests)
    groups: dict[str, int] = {}
    for record in rust_tests:
        for request in record.rust:
            if request.group:
                groups[request.group] = groups.get(request.group, 0) + 1

    writer = terminalreporter.write_line
    writer("")
    writer("=" * 78)
    writer(f"plane contract runner: backend={STATE.backend}")
    writer("=" * 78)
    if STATE.backend == "mixed":
        writer(
            f"plan: {len(STATE.routes)} route cluster(s) on rust; "
            f"port {STATE.port}, db {STATE.db_name}, redis {STATE.redis_db}, vhost {STATE.vhost}"
        )
    writer(
        f"requests: {rust_requests + django_requests} total, {rust_requests} to rust, "
        f"{django_requests} to django"
    )
    writer(
        f"tests touching rust: {len(rust_tests)} "
        f"(passed {rust_counts['passed']}, failed {rust_counts['failed']}, "
        f"error {rust_counts['error']}, skipped {rust_counts['skipped']})"
    )
    if groups:
        writer("  groups: " + ", ".join(f"{name}={count}" for name, count in sorted(groups.items())))
    if STATE.replay_count:
        writer(f"celery replays into patched mocks: {STATE.replay_count}")
    writer(
        f"tests django-only: {len(django_tests)} "
        f"({counts(django_tests)['passed']} passed, {counts(django_tests)['failed']} failed, "
        f"{counts(django_tests)['error']} error, {counts(django_tests)['skipped']} skipped)"
    )
    limited = [record for record in records if record.limitations]
    if limited:
        writer("")
        writer("runner limitations (served by Django despite a rust-owned route):")
        for record in limited:
            for limitation in sorted(set(record.limitations)):
                writer(f"  {record.nodeid}")
                writer(f"          {limitation}")
    if rust_tests:
        writer("")
        writer("tests that touched rust:")
        for record in rust_tests:
            methods = ", ".join(
                sorted({f"{request.method} {request.path}" for request in record.rust})
            )
            writer(f"  {record.outcome:<7} {record.nodeid}")
            writer(f"          rust: {methods}")
            for limitation in sorted(set(record.limitations)):
                writer(f"          limitation: {limitation}")
    failures = [record for record in rust_tests if record.outcome in ("failed", "error")]
    if failures:
        writer("")
        writer("FAILURES INVOLVING RUST:")
        for record in failures:
            writer(f"  {record.outcome:<7} {record.nodeid}")
            for request in record.rust:
                writer(
                    f"          {request.method} {request.path} -> {request.status}"
                    + (f" ({request.error})" if request.error else "")
                )

    if STATE.report_path is not None:
        STATE.report_path.parent.mkdir(parents=True, exist_ok=True)
        STATE.report_path.write_text(
            json.dumps(
                {
                    "backend": STATE.backend,
                    "db": STATE.db_name,
                    "redis_db": STATE.redis_db,
                    "vhost": STATE.vhost,
                    "routes": len(STATE.routes),
                    "rust_requests": rust_requests,
                    "django_requests": django_requests,
                    "celery_replays": STATE.replay_count,
                    "tests": [
                        {
                            "nodeid": record.nodeid,
                            "outcome": record.outcome,
                            "rust_requests": [
                                {
                                    "method": request.method,
                                    "path": request.path,
                                    "status": request.status,
                                    "group": request.group,
                                    "error": request.error,
                                }
                                for request in record.rust
                            ],
                            "django_requests": record.django_requests,
                            "limitations": sorted(set(record.limitations)),
                        }
                        for record in records
                    ],
                },
                indent=1,
            )
            + "\n",
            encoding="utf-8",
        )
        writer("")
        writer(f"report: {STATE.report_path}")
