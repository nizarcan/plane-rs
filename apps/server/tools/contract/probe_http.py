# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Synthetic probes for the cross-cutting HTTP contract.

Everything here runs Django in-process with ``plane.settings.production`` and a
dummy ``DATABASE_URL``/``REDIS_URL``.  Requests that would need Postgres are not
made (see the notes in HTTP_CONTRACT.md).  The Redis cache is replaced with
``LocMemCache`` for the probes that reach a DRF throttle, because the throttle
runs before the permission check and would otherwise raise
``ConnectionInterrupted`` on a dead Redis; nothing else about the throttle
changes (the cache key and value format is verified separately).

Two details of the Django test client that matter:

* ``Client(enforce_csrf_checks=True)`` is required to exercise
  ``CsrfViewMiddleware``; the default client sets
  ``request._dont_enforce_csrf_checks``.
* Middleware instances are created once per handler on the first request, and
  several middlewares cache settings in ``__init__`` (``SecurityMiddleware``,
  ``WhiteNoiseMiddleware``).  Every ``override_settings`` that changes such a
  setting therefore gets a fresh ``Client`` created inside the context.

Run from ``apps/api``:

    .venv/bin/python ../server/tools/contract/probe_http.py
"""

import gzip
import os
import sys

# Keep the tree clean: importing probe_urls must not create __pycache__ (the
# repository ignores *.pyc but not __pycache__/ under apps/server).
sys.dont_write_bytecode = True

API_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", "api"))
TOOLS_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path[:0] = [API_DIR, TOOLS_DIR]

SECRET_KEY = "contract-test-secret-key-not-for-deployment"
os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ["SECRET_KEY"] = SECRET_KEY
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.test import Client, override_settings  # noqa: E402

LOCMEM = {"default": {"BACKEND": "django.core.cache.backends.locmem.LocMemCache"}}


def client(**kwargs):
    return Client(raise_request_exception=False, **kwargs)


def _body(response):
    if getattr(response, "streaming", False):
        return b"".join(response.streaming_content)
    return response.content


def show(label, response, body_repr_limit=400):
    print("### %s" % label)
    print("status: %d" % response.status_code)
    for key, value in response.headers.items():
        print("  %s: %s" % (key, value))
    for line in response.cookies.output().split("\r\n"):
        if line.strip():
            print("  Set-Cookie(raw): %s" % line)
    body = _body(response)
    if len(body) > body_repr_limit:
        print("body(%d bytes, first %d): %r" % (len(body), body_repr_limit, body[:body_repr_limit]))
    else:
        print("body(%d bytes): %r" % (len(body), body))
    print()


def guarded(label, fn):
    try:
        fn()
    except Exception as exc:  # noqa: BLE001 - a probe that errors is a data point
        print("### %s" % label)
        print("RAISED %s: %s" % (type(exc).__name__, exc))
        print()


def section(title):
    print()
    print("=" * 78)
    print("== %s" % title)
    print("=" * 78)
    print()


def main():
    print("Django", django.get_version())
    from django.conf import settings as dj_settings

    print("DEBUG =", dj_settings.DEBUG, "(production.py: int(os.environ['DEBUG'], 0) == 1)")
    print("DATABASE_URL =", os.environ.get("DATABASE_URL"))
    print("REDIS_URL    =", os.environ.get("REDIS_URL"))
    print()
    with override_settings(CACHES=LOCMEM):
        run_all()
    return 0


def run_all():
    c = client()

    section("1. Plain Django views (no DRF, no DB)")
    show("GET /  (plane/web/views.py health_check -> JsonResponse)", c.get("/"))
    show("GET /robots.txt", c.get("/robots.txt"))
    show("GET /definitely-not-a-route/  (handler404 = custom_404_view)", c.get("/definitely-not-a-route/"))
    show("HEAD /definitely-not-a-route/", c.head("/definitely-not-a-route/"))
    show("GET /api/  (include prefix resolves to no pattern)", c.get("/api/"))
    show("GET /api  (APPEND_SLASH: '/api/' does not resolve either)", c.get("/api"))
    show("HEAD /  (plain function view answers HEAD)", c.head("/"))
    show("OPTIONS /  (plain function view answers OPTIONS with its normal body)", c.options("/"))

    # Function views do not restrict methods, but an unsafe method on a
    # non-exempt view is checked by CsrfViewMiddleware first (middleware #5
    # runs before the view). With no token that is the 200 HTML failure page of
    # §7.5, not the health body.
    csrf_client = client(enforce_csrf_checks=True)
    show(
        "POST / with no CSRF token (CsrfViewMiddleware -> 200 HTML csrf_failure, not the health body)",
        csrf_client.post("/", data=b'{"ping": true}', content_type="application/json"),
    )
    show(
        "PUT / with no CSRF token (same)",
        csrf_client.put("/", data=b"", content_type="text/plain"),
    )
    show("DELETE / with no CSRF token (same)", csrf_client.delete("/"))
    token_response = csrf_client.get("/auth/get-csrf-token/")
    token = token_response.json()["csrf_token"]
    cookie = token_response.cookies["csrftoken"].value
    show(
        "POST / with a valid X-CSRFToken and its cookie (passes CSRF; the view answers POST)",
        csrf_client.post(
            "/",
            data=b'{"ping": true}',
            content_type="application/json",
            HTTP_X_CSRFTOKEN=token,
            cookies={"csrftoken": cookie},
        ),
    )

    section("2. APPEND_SLASH (CommonMiddleware)")
    show("GET /api/users/me  (no trailing slash -> 301 to /api/users/me/)", c.get("/api/users/me"))
    show(
        "POST /api/users/me  (DEBUG=0 -> 301, body dropped)",
        c.post("/api/users/me", data=b"{}", content_type="application/json"),
    )
    show("GET /api/users/me?x=1&y=2  (query string preserved)", c.get("/api/users/me?x=1&y=2"))
    with override_settings(DEBUG=True):
        guarded(
            "POST /api/users/me with DEBUG=True (RuntimeError -> 500 technical page)",
            lambda: show(
                "POST /api/users/me DEBUG=1 (body truncated)",
                client().post("/api/users/me", data=b"{}", content_type="application/json"),
                400,
            ),
        )

    section("3. CORS: allow-all (CORS_ALLOW_ALL_ORIGINS=True, CORS_ALLOW_CREDENTIALS=True)")
    show("GET / with Origin (simple request)", c.get("/", HTTP_ORIGIN="http://localhost:3000"))
    show("GET / without Origin (Vary: origin still added)", c.get("/"))
    show(
        "OPTIONS /api/users/me/ with Origin + Access-Control-Request-Method (preflight)",
        c.options(
            "/api/users/me/",
            HTTP_ORIGIN="http://localhost:3000",
            HTTP_ACCESS_CONTROL_REQUEST_METHOD="POST",
            HTTP_ACCESS_CONTROL_REQUEST_HEADERS="content-type, x-api-key",
        ),
    )
    show(
        "OPTIONS /api/users/me/ with Origin, NO Access-Control-Request-Method (not a preflight -> view runs)",
        c.options("/api/users/me/", HTTP_ORIGIN="http://localhost:3000"),
    )
    show(
        "OPTIONS /definitely-not-a-route/ with Origin + ACRM (preflight answers before routing)",
        c.options(
            "/definitely-not-a-route/",
            HTTP_ORIGIN="http://localhost:3000",
            HTTP_ACCESS_CONTROL_REQUEST_METHOD="GET",
        ),
    )

    section("4. CORS: allow-list (CORS_ALLOW_ALL_ORIGINS=False, CORS_ALLOWED_ORIGINS=[...])")
    allowed = ["http://localhost:3000", "https://app.plane.so"]
    with override_settings(CORS_ALLOW_ALL_ORIGINS=False, CORS_ALLOWED_ORIGINS=allowed, CACHES=LOCMEM):
        c2 = client()
        show("GET / with allowed Origin", c2.get("/", HTTP_ORIGIN="http://localhost:3000"))
        show("GET / with disallowed Origin (Vary: origin, no ACAO)", c2.get("/", HTTP_ORIGIN="http://evil.example"))
        show(
            "OPTIONS preflight with disallowed Origin",
            c2.options("/api/users/me/", HTTP_ORIGIN="http://evil.example", HTTP_ACCESS_CONTROL_REQUEST_METHOD="POST"),
        )

    section("5. Security headers (SecurityMiddleware, XFrameOptionsMiddleware)")
    show("GET / baseline security headers", c.get("/"))
    with override_settings(
        SECURE_SSL_REDIRECT=True,
        SECURE_REDIRECT_EXEMPT=["^robots\\.txt$"],
        SECURE_HSTS_SECONDS=31536000,
        SECURE_HSTS_INCLUDE_SUBDOMAINS=True,
        SECURE_HSTS_PRELOAD=True,
        CACHES=LOCMEM,
    ):
        c2 = client()
        show("GET / over plain http with SECURE_SSL_REDIRECT=True -> 301", c2.get("/"))
        show("GET /robots.txt (SECURE_REDIRECT_EXEMPT) -> no redirect", c2.get("/robots.txt"))
        show(
            "GET / with X-Forwarded-Proto: https (SECURE_PROXY_SSL_HEADER) -> HSTS",
            c2.get("/", HTTP_X_FORWARDED_PROTO="https"),
        )

    section("6. DRF: unauthenticated /api/, /api/v1/, /api/public/ (no DB touched)")
    show("GET /api/users/me/  (BaseSessionAuthentication, IsAuthenticated)", c.get("/api/users/me/"))
    show("DELETE /api/users/me/  (401 before 405: permission runs first)", c.delete("/api/users/me/"))
    show("GET /api/v1/users/me/  (APIKeyAuthentication, no X-Api-Key)", c.get("/api/v1/users/me/"))
    show(
        "POST /api/v1/users/me/ with a bogus X-Api-Key (AuthenticationFailed 401)",
        c.post("/api/v1/users/me/", data=b"{}", content_type="application/json", HTTP_X_API_KEY="nope"),
    )
    show("GET /api/public/  (plane.space, no matching pattern)", c.get("/api/public/"))

    section("7. Origin check used by /api/ writes (BaseSessionAuthentication.enforce_csrf)")
    from django.test import RequestFactory

    from plane.authentication.session import BaseSessionAuthentication, is_allowed_origin

    rf = RequestFactory()
    print("is_allowed_origin() decisions (settings.WEB_URL is unset in this probe):")
    cases = [
        ("no Origin header", {}),
        ("Origin == request host", {"HTTP_ORIGIN": "http://testserver"}),
        ("Origin has a port, Host does not", {"HTTP_ORIGIN": "http://testserver:80"}),
        ("Origin 'null'", {"HTTP_ORIGIN": "null"}),
        ("Origin cross-site", {"HTTP_ORIGIN": "http://evil.example"}),
    ]
    for label, extra in cases:
        request = rf.get("/api/users/me/", **extra)
        print("  %-36s -> %s" % (label, is_allowed_origin(request)))
    print()
    auth = BaseSessionAuthentication()
    for method, extra in [
        ("GET", {"HTTP_ORIGIN": "http://evil.example"}),
        ("POST", {}),
        ("POST", {"HTTP_ORIGIN": "http://evil.example"}),
        ("POST", {"HTTP_ORIGIN": "http://testserver"}),
    ]:
        request = rf.generic(method, "/api/x/", **extra)
        try:
            auth.enforce_csrf(request)
            outcome = "allowed"
        except Exception as exc:  # noqa: BLE001
            outcome = "%s: %s" % (type(exc).__name__, exc)
        print("  enforce_csrf %-5s %-40r -> %s" % (method, extra, outcome))
    print()

    section("8. CSRF token endpoint and the CSRF cookie")
    show("GET /auth/get-csrf-token/", c.get("/auth/get-csrf-token/"))
    r = c.get("/auth/get-csrf-token/")
    print("  token length:", len(r.json()["csrf_token"]))
    print()
    with override_settings(CSRF_COOKIE_SECURE=True, COOKIE_DOMAIN=".plane.so", CSRF_COOKIE_DOMAIN=".plane.so", CACHES=LOCMEM):
        show("GET /auth/get-csrf-token/ with CSRF_COOKIE_SECURE + COOKIE_DOMAIN", client().get("/auth/get-csrf-token/"))
    with override_settings(CSRF_COOKIE_HTTPONLY=False, CACHES=LOCMEM):
        show("GET /auth/get-csrf-token/ with CSRF_COOKIE_HTTPONLY=False", client().get("/auth/get-csrf-token/"))

    section("9. CsrfViewMiddleware on a plain Django View (POST /auth/magic-sign-in/)")
    csrf_client = client(enforce_csrf_checks=True)
    show(
        "POST /auth/magic-sign-in/ with no CSRF cookie (rejected)",
        csrf_client.post("/auth/magic-sign-in/", data={"email": "a@b.c", "code": "1"}),
    )
    show(
        "POST /auth/magic-sign-in/ with Origin (rejected on the origin check)",
        csrf_client.post(
            "/auth/magic-sign-in/",
            data={"email": "a@b.c", "code": "1"},
            HTTP_ORIGIN="http://evil.example",
        ),
    )
    r = csrf_client.get("/auth/get-csrf-token/")
    cookie_token = r.cookies["csrftoken"].value
    header_token = r.json()["csrf_token"]
    print("  csrftoken cookie value (first 8):", cookie_token[:8], "len", len(cookie_token))
    print("  JSON csrf_token  (first 8):", header_token[:8], "len", len(header_token))
    print()
    show(
        "POST /auth/magic-sign-in/ with matching X-CSRFToken and cookie (passes CSRF, then 500 on Redis)",
        csrf_client.post(
            "/auth/magic-sign-in/",
            data={"email": "a@b.c", "code": "1"},
            HTTP_X_CSRFTOKEN=header_token,
            cookies={"csrftoken": cookie_token},
        ),
    )

    section("10. gzip (GZipMiddleware)")
    show("GET /  (16 bytes < 200 -> no gzip, no Vary)", c.get("/", HTTP_ACCEPT_ENCODING="gzip"))
    show(
        "GET /definitely-not-a-route/ (28 bytes -> no gzip, Vary: Accept-Encoding added anyway)",
        c.get("/definitely-not-a-route/", HTTP_ACCEPT_ENCODING="gzip"),
    )
    r = c.get("/auth/get-csrf-token/", HTTP_ACCEPT_ENCODING="gzip")
    print("### GET /auth/get-csrf-token/ with Accept-Encoding: gzip (81-byte body)")
    print("  Content-Encoding:", r.get("Content-Encoding"))
    print("  Vary:", r.get("Vary"))
    print("  Content-Length:", r.get("Content-Length"))
    print()

    section("11. Request body limits (RequestBodySizeLimitMiddleware, DATA_UPLOAD_MAX_MEMORY_SIZE)")
    with override_settings(DATA_UPLOAD_MAX_MEMORY_SIZE=100, CACHES=LOCMEM):
        c2 = client()
        show(
            "POST / with a 200-byte body and DATA_UPLOAD_MAX_MEMORY_SIZE=100 -> 413",
            c2.post("/", data=b"x" * 200, content_type="text/plain"),
        )
        show(
            "POST /api/users/me/ with a 200-byte body -> 413, before authentication",
            c2.post("/api/users/me/", data=b"x" * 200, content_type="application/json"),
        )

    section("12. ALLOWED_HOSTS")
    with override_settings(ALLOWED_HOSTS=["plane.example.com"], CACHES=LOCMEM):
        c2 = client()
        show(
            "GET / with Host: testserver under ALLOWED_HOSTS=['plane.example.com'] -> 400",
            c2.get("/", HTTP_HOST="testserver"),
        )
        show("GET / with Host: plane.example.com -> 200", c2.get("/", HTTP_HOST="plane.example.com"))

    section("12b. WhiteNoise and /static/")
    show(
        "GET /static/css/style.css with DEBUG=0 and no collected-static dir -> falls through to 404",
        c.get("/static/css/style.css"),
    )
    show("GET /static/ -> falls through to 404", c.get("/static/"))
    with override_settings(DEBUG=True, CACHES=LOCMEM):
        c2 = client()
        show(
            "GET /static/css/style.css with DEBUG=True (WHITENOISE_AUTOREFRESH/USE_FINDERS default to DEBUG)",
            c2.get("/static/css/style.css"),
        )
        show(
            "GET /static/css/style.css with If-None-Match",
            c2.get("/static/css/style.css", HTTP_IF_NONE_MATCH=c2.get("/static/css/style.css").get("ETag")),
        )
        show(
            "GET /static/logos/Logo.png with Range: bytes=0-9",
            c2.get("/static/logos/Logo.png", HTTP_RANGE="bytes=0-9"),
        )

    section("13. Synthetic DRF views (DRF contract in isolation)")
    import probe_urls  # noqa: F401

    with override_settings(ROOT_URLCONF="probe_urls", CACHES=LOCMEM):
        synthetic()


def synthetic():
    c = client()

    show("GET /t/plain/ (APIView, AllowAny, single JSONRenderer)", c.get("/t/plain/"))
    show(
        "POST /t/plain/ (405 MethodNotAllowed + Allow header)",
        c.post("/t/plain/", data=b"{}", content_type="application/json"),
    )
    show("GET /t/plain/?format=json", c.get("/t/plain/?format=json"))
    show("GET /t/plain/?format=xml (404 from filter_renderers)", c.get("/t/plain/?format=xml"))
    show("OPTIONS /t/plain/ (SimpleMetadata)", c.options("/t/plain/"))
    show("GET /t/plain/ with Accept: text/html (406 NotAcceptable)", c.get("/t/plain/", HTTP_ACCEPT="text/html"))
    show("OPTIONS /t/generic/ (SimpleMetadata with actions)", c.options("/t/generic/"))

    show("GET /t/permission/ (PermissionDenied 403)", c.get("/t/permission/"))
    show("GET /t/notfound/ (Http404 -> NotFound 404)", c.get("/t/notfound/"))
    show(
        "GET /t/auth-none/ no credentials (NotAuthenticated, no authenticate_header -> Plane forces 401)",
        c.get("/t/auth-none/"),
    )
    show(
        "GET /t/auth-none/ with X-Fail: raise (AuthenticationFailed, no authenticate_header -> 403)",
        c.get("/t/auth-none/", HTTP_X_FAIL="raise"),
    )
    show(
        "GET /t/auth-header/ with X-Fail: raise (authenticate_header present -> 401 + WWW-Authenticate)",
        c.get("/t/auth-header/", HTTP_X_FAIL="raise"),
    )
    show("GET /t/boom/ (unhandled exception -> Django 500 handler)", c.get("/t/boom/"))

    show("POST /t/echo/ valid JSON", c.post("/t/echo/", data=b'{"a":1}', content_type="application/json"))
    show("POST /t/echo/ malformed JSON", c.post("/t/echo/", data=b'{"a":', content_type="application/json"))
    show("POST /t/echo/ empty body with Content-Type: application/json", c.post("/t/echo/", data=b"", content_type="application/json"))
    show("POST /t/echo/ JSON null", c.post("/t/echo/", data=b"null", content_type="application/json"))
    show("POST /t/echo/ JSON string body", c.post("/t/echo/", data=b'"hello"', content_type="application/json"))
    show("POST /t/echo/ JSON NaN (STRICT_JSON=True)", c.post("/t/echo/", data=b'{"x": NaN}', content_type="application/json"))
    show(
        "POST /t/echo/ text/plain (415 UnsupportedMediaType)",
        c.post("/t/echo/", data=b"x", content_type="text/plain"),
    )
    show(
        "POST /t/echo/ application/json; charset=latin-1",
        c.post("/t/echo/", data=b'{"a":1}', content_type="application/json; charset=latin-1"),
    )
    show(
        "POST /t/echo/ application/json; charset=bz2 (non-text codec rejected)",
        c.generic("POST", "/t/echo/", data=b'{"a":1}', content_type="application/json; charset=bz2"),
    )
    show(
        "POST /t/echo/ no Content-Type at all (stream None -> empty data)",
        c.generic("POST", "/t/echo/", data=b'{"a":1}'),
    )
    show(
        "POST /t/echo/ empty Content-Type header with a body",
        c.generic("POST", "/t/echo/", data=b'{"a":1}', content_type=""),
    )
    show(
        "GET /t/apikey/ with X-Api-Key: probe-key (ApiKeyRateThrottle headers)",
        c.get("/t/apikey/", HTTP_X_API_KEY="probe-key"),
    )
    show(
        "GET /t/apikey/ with no X-Api-Key (throttle key None, no headers)",
        c.get("/t/apikey/"),
    )

    show(
        "POST /t/validation/ (serializer ValidationError)",
        c.post("/t/validation/", data=b"{}", content_type="application/json"),
    )
    show(
        "POST /t/validation-string/ (bare ValidationError('boom'))",
        c.post("/t/validation-string/", data=b"{}", content_type="application/json"),
    )
    show(
        "POST /t/parse-error/ (explicit ParseError)",
        c.post("/t/parse-error/", data=b"{}", content_type="application/json"),
    )

    show("GET /t/throttled/ (rate 1/minute, first call)", c.get("/t/throttled/"))
    show("GET /t/throttled/ (second call -> 429)", c.get("/t/throttled/"))

    with override_settings(DEBUG=True):
        show(
            "GET /t/boom/ with DEBUG=True (DEBUG_PROPAGATE_EXCEPTIONS off -> technical 500 page)",
            client().get("/t/boom/"),
            300,
        )


if __name__ == "__main__":
    sys.exit(main())
