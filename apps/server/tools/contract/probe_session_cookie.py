# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Session cookie emission: what ``SessionMiddleware.process_response`` writes.

The database write is stubbed (``SessionStore.save``) so this runs without
Postgres; every branch of the middleware's logic and every cookie attribute is
still the real code from
``plane/authentication/middleware/session.py``.

Run from ``apps/api``:

    .venv/bin/python ../server/tools/contract/probe_session_cookie.py
"""

import os
import sys
import time

API_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", "api"))
sys.path.insert(0, API_DIR)

os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ["SECRET_KEY"] = "contract-test-secret-key-not-for-deployment"
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.http import HttpResponse  # noqa: E402
from django.test import override_settings  # noqa: E402

from plane.authentication.middleware.session import SessionMiddleware  # noqa: E402
from plane.db.models.session import SessionStore  # noqa: E402

FAKE_KEY = "k" * 128


def _stub_save(self, must_create=False):
    if self._session_key is None:
        self._session_key = FAKE_KEY
    self.modified = False


SessionStore.save = _stub_save


class FakeRequest:
    def __init__(self, path, cookies=None):
        self.path = path
        self.COOKIES = cookies or {}
        self.META = {}
        self.session = None


def make_session(data, key=None, accessed=True, modified=True):
    store = SessionStore(key)
    store._session_cache = dict(data)
    store.accessed = accessed
    store.modified = modified
    return store


def probe(label, path, data, key=None, cookies=None, status=200, accessed=True, modified=True):
    mw = SessionMiddleware(lambda request: HttpResponse())
    request = FakeRequest(path, cookies)
    request.session = make_session(data, key=key, accessed=accessed, modified=modified)
    response = HttpResponse(status=status)
    response = mw.process_response(request, response)
    print("### %s" % label)
    print("  path=%r cookie_key=%r" % (path, key))
    print("  session data=%r" % (data,))
    print("  status=%d accessed=%s modified=%s" % (status, accessed, modified))
    print("  Vary =", response.get("Vary"))
    out = response.cookies.output().strip()
    print("  Set-Cookie =", out if out else "(none)")

    # What http.cookies emits, attribute order is `sorted(self.items())`.
    for name, morsel in response.cookies.items():
        print("  morsel[%s] attrs = %r" % (name, sorted(morsel.items())))
    print()


def main():
    print("Django", django.get_version())
    print("now = %d" % int(time.time()))
    print()

    alice = {
        "_auth_user_id": "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        "backend": "django.contrib.auth.backends.ModelBackend",
        "_auth_user_hash": "9f2f" * 16,
        "device_info": {"user_agent": "probe", "ip_address": "203.0.113.7", "domain": "http://localhost/"},
    }

    print("Defaults: SESSION_COOKIE_NAME=session-id, SESSION_COOKIE_AGE=604800,")
    print("          ADMIN_SESSION_COOKIE_NAME=admin-session-id, ADMIN_SESSION_COOKIE_AGE=3600,")
    print("          SESSION_COOKIE_HTTPONLY=True, SESSION_COOKIE_SAMESITE=Lax,")
    print("          SESSION_COOKIE_PATH=/, SESSION_COOKIE_DOMAIN=None, SESSION_COOKIE_SECURE=False,")
    print("          SESSION_SAVE_EVERY_REQUEST=False")
    print()

    probe("1. new session, modified, no cookie in the request", "/api/users/me/", alice)
    probe("2. new session on an admin path (substring 'instances')", "/api/instances/users/", alice)
    probe("3. path that merely contains 'instances' also gets the admin cookie", "/api/workspaces/acme/instances/", alice)
    probe("4. session accessed but not modified", "/api/users/me/", alice, accessed=True, modified=False)
    probe("5. session neither accessed nor modified", "/api/users/me/", alice, accessed=False, modified=False)
    probe(
        "6. flushed session (session_key None + empty data) with a cookie -> delete cookie",
        "/api/users/me/",
        {},
        key=None,
        cookies={"session-id": "some-old-key"},
    )
    probe(
        "7. flushed admin session with the admin cookie -> delete admin cookie",
        "/api/instances/",
        {},
        key=None,
        cookies={"admin-session-id": "some-old-key"},
    )
    probe(
        "7b. session_key present but payload empty -> NOT a delete, cookie re-set",
        "/api/users/me/",
        {},
        key=FAKE_KEY,
        cookies={"session-id": FAKE_KEY},
    )
    probe("8. response status 500 -> no save, no cookie", "/api/users/me/", alice, status=500)
    probe("9. browser-close session (_session_expiry = 0)", "/api/users/me/", {**alice, "_session_expiry": 0})
    probe("10. custom expiry (_session_expiry = 100)", "/api/users/me/", {**alice, "_session_expiry": 100})
    probe("11. custom expiry as an ISO timestamp", "/api/users/me/", {**alice, "_session_expiry": "2099-01-01T00:00:00+00:00"})

    with override_settings(SESSION_COOKIE_SECURE=True):
        probe("12. SESSION_COOKIE_SECURE=True -> Secure attribute", "/api/users/me/", alice)
    with override_settings(COOKIE_DOMAIN=".plane.so", SESSION_COOKIE_DOMAIN=".plane.so"):
        probe("13. COOKIE_DOMAIN=.plane.so -> Domain attribute", "/api/users/me/", alice)
    with override_settings(SESSION_SAVE_EVERY_REQUEST=True):
        probe(
            "14. SESSION_SAVE_EVERY_REQUEST=True, not modified -> cookie refreshed",
            "/api/users/me/",
            alice,
            accessed=True,
            modified=False,
        )
    with override_settings(SESSION_COOKIE_NAME="plane-session", ADMIN_SESSION_COOKIE_NAME="plane-admin"):
        probe("15. renamed cookies", "/api/users/me/", alice)
        probe("16. renamed admin cookie", "/api/instances/x/", alice)

    print("Cookie attribute order is http.cookies.Morsel.OutputString():")
    print("  `sorted(self.items())`, i.e. alphabetical by lower-case attribute name:")
    print("  comment < domain < expires < httponly < max-age < path < samesite < secure < version")
    print()
    print("Deletion is HttpResponse.delete_cookie():")
    print("  value='', max-age=0, expires='Thu, 01 Jan 1970 00:00:00 GMT', path=SESSION_COOKIE_PATH,")
    print("  domain=SESSION_COOKIE_DOMAIN, samesite=SESSION_COOKIE_SAMESITE, no httponly.")
    print("  patch_vary_headers(response, ('Cookie',)) is applied in both branches.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
