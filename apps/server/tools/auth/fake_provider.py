#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""A deterministic local OAuth provider for the `auth-oauth` differential test.

It answers the two **self-hostable** providers' shapes (Google's and GitHub's
hardcoded endpoints are served by `tools/auth/oauth_https_proxy.py` instead):

| Provider | Endpoints |
| --- | --- |
| GitLab | `POST /oauth/token`, `GET /oauth/authorize`, `GET /api/v4/user` |
| Gitea | `POST /login/oauth/access_token`, `GET /login/oauth/authorize`, `GET /api/v1/user`, `GET /api/v1/user/emails` |

The `code` query/form parameter selects the case, so one server serves the
whole scenario: `gitlab-signup`, `gitlab-signin`, `gitlab-unverified`,
`gitlab-token-not-json`, `gitlab-userinfo-not-object`, `gitlab-token-error`,
`gitlab-userinfo-error`, `gitlab-token-no-access-token`, and the Gitea
equivalents. Every response is fixed; the access token is `access-<code>` so a
differing token in the database is a visible diff.

`http://127.0.0.1:<port>/avatar.png` exists for the IDP-sync case: the pinned
avatar fetch in `plane/utils/url_security.py` rejects loopback, so Plane falls
back to storing the URL. Both backends must take the same fallback.

Run it from `apps/server/`:

    apps/api/.venv/bin/python tools/auth/fake_provider.py --port 18400

Or let `tools/auth/run-oauth-scenario.sh` start it and run `difftest`.
"""

import argparse
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlsplit

# A single transparent 1×1 PNG.
PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4"
    "890000000d49444154789c6360000002000154a24f5f0000000049454e44ae42"
    "6082"
)

# The fixture user the sign-in cases resolve to. It exists in `plane_fix_tpl`
# (`scenarios/write/auth-oauth-fixture.sql` seeds the `accounts` row).
SIGNIN_GITLAB_EMAIL = "fix.member@example.com"
SIGNIN_GITLAB_ID = 201
SIGNIN_GITEA_EMAIL = "fix.member2@example.com"
SIGNIN_GITEA_ID = 401


def gitlab_user(code):
    if code == "gitlab-signin":
        return {
            "id": SIGNIN_GITLAB_ID,
            "username": "fix.member",
            "name": "Mia Member",
            "email": SIGNIN_GITLAB_EMAIL,
            "avatar_url": "http://127.0.0.1:{port}/avatar.png",
            "confirmed_at": "2026-01-02T03:04:05Z",
        }
    if code == "gitlab-unverified":
        return {
            "id": 202,
            "username": "unverified",
            "name": " Una Verified ",
            "email": "OAuth.GitLab.Unverified@Example.COM",
            "avatar_url": None,
            "confirmed_at": None,
        }
    if code == "gitlab-userinfo-error":
        return None
    if code == "gitlab-userinfo-not-object":
        return []
    # `gitlab-signup` and any other case create this user.
    return {
        "id": 101,
        "username": "oauth-signup",
        "name": "OAuth Signup",
        "email": "oauth.gitlab.signup@example.com",
        "avatar_url": "http://127.0.0.1:{port}/avatar.png",
        "confirmed_at": "2026-01-02T03:04:05Z",
    }


def gitea_user(code):
    if code == "gitea-signin":
        return {
            "id": SIGNIN_GITEA_ID,
            "login": "fix.member2",
            "full_name": "Marco Member",
            "email": "",
            "avatar_url": "http://127.0.0.1:{port}/avatar.png",
        }
    if code == "gitea-signup":
        return {
            "id": 301,
            "login": "gitea-signup",
            "full_name": "Gitea Signup",
            "email": "",
            "avatar_url": "",
        }
    if code == "gitea-signup-b":
        return {
            "id": 302,
            "login": "gitea-signup-b",
            "full_name": "",
            "email": "",
            "avatar_url": "",
        }
    if code == "gitea-missing-id":
        return {
            "login": "no-id",
            "full_name": "No Id",
            "email": "",
            "avatar_url": "",
        }
    return {
        "id": 303,
        "login": "gitea-generic",
        "full_name": "Gitea Generic",
        "email": "",
        "avatar_url": "",
    }


def gitea_emails(code):
    if code == "gitea-emails-unverified":
        return [{"email": "oauth.gitea.unverified@example.com", "primary": True, "verified": False}]
    if code == "gitea-verified-secondary":
        return [
            {"email": "oauth.gitea.secondary@example.com", "primary": False, "verified": True}
        ]
    if code == "gitea-no-emails":
        return []
    if code == "gitea-emails-error":
        return None
    if code == "gitea-signin":
        return [
            {"email": "other@example.com", "primary": False, "verified": True},
            {"email": SIGNIN_GITEA_EMAIL, "primary": True, "verified": True},
        ]
    if code == "gitea-signup-b":
        return [{"email": "oauth.gitea.signup.b@example.com", "primary": True, "verified": True}]
    return [{"email": "oauth.gitea.signup@example.com", "primary": True, "verified": True}]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    # `BaseHTTPRequestHandler` logs every request to stderr; the harness reads
    # it for debugging only.
    def log_message(self, fmt, *args):  # noqa: A003
        sys.stderr.write("fake-provider: " + (fmt % args) + "\n")

    def _send(self, status, payload, content_type="application/json"):
        if isinstance(payload, (dict, list)):
            body = json.dumps(payload).encode()
        elif payload is None:
            body = b""
        elif isinstance(payload, bytes):
            body = payload
        else:
            body = str(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _port(self):
        return self.server.server_address[1]

    def _mode(self):
        """The case the token endpoint issued: the access token is
        `access-<code>`, and the userinfo calls carry it as a bearer token (the
        provider sends no other identifier)."""
        authorization = self.headers.get("Authorization", "")
        token = authorization.removeprefix("Bearer ").strip()
        if token.startswith("access-"):
            return token[len("access-"):]
        return ""

    def _code_from_form(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length).decode()
        return parse_qs(body).get("code", [""])[0], parse_qs(body)

    def do_GET(self):  # noqa: N802
        split = urlsplit(self.path)
        query = parse_qs(split.query)
        code = query.get("code", [""])[0]

        if split.path == "/avatar.png":
            return self._send(200, PNG, "image/png")

        if split.path == "/oauth/authorize":
            redirect_uri = query.get("redirect_uri", [""])[0]
            state = query.get("state", [""])[0]
            location = f"{redirect_uri}?{urlencode({'code': code, 'state': state})}"
            self.send_response(302)
            self.send_header("Location", location)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None

        if split.path == "/login/oauth/authorize":
            redirect_uri = query.get("redirect_uri", [""])[0]
            state = query.get("state", [""])[0]
            location = f"{redirect_uri}?{urlencode({'code': code, 'state': state})}"
            self.send_response(302)
            self.send_header("Location", location)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None

        if split.path == "/api/v4/user":
            user = gitlab_user(self._mode())
            if user is None:
                return self._send(403, {"message": "403 Forbidden"})
            return self._send(200, render_port(user, self._port()))

        if split.path == "/api/v1/user":
            return self._send(200, render_port(gitea_user(self._mode()), self._port()))

        if split.path == "/api/v1/user/emails":
            emails = gitea_emails(self._mode())
            if emails is None:
                return self._send(403, {"message": "forbidden"})
            return self._send(200, emails)

        return self._send(404, {"message": "not found"})

    def do_POST(self):  # noqa: N802
        split = urlsplit(self.path)
        code, form = self._code_from_form()
        if split.path == "/oauth/token":
            if code == "gitlab-token-error":
                return self._send(401, {"error": "invalid_grant"})
            if code == "gitlab-token-not-json":
                return self._send(200, "<html>not json</html>", "text/html")
            response = {
                "access_token": f"access-{code or 'default'}",
                "token_type": "bearer",
                "refresh_token": f"refresh-{code or 'default'}",
                "created_at": 1700000000,
                "expires_in": 7200,
            }
            if code == "gitlab-token-no-access-token":
                response.pop("access_token")
            return self._send(200, response)
        if split.path == "/login/oauth/access_token":
            if code == "gitea-token-not-json":
                return self._send(200, "not json", "text/plain")
            return self._send(
                200,
                {
                    "access_token": f"access-{code or 'default'}",
                    "token_type": "bearer",
                    "refresh_token": f"refresh-{code or 'default'}",
                    "expires_in": 3600,
                },
            )
        return self._send(404, {"message": "not found"})


def render_port(value, port):
    """Replace the `{port}` placeholder in the avatar URLs."""
    if isinstance(value, dict):
        return {key: render_port(item, port) for key, item in value.items()}
    if isinstance(value, list):
        return [render_port(item, port) for item in value]
    if isinstance(value, str):
        return value.replace("{port}", str(port))
    return value


def serve(port):
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    return server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=18400)
    parser.add_argument(
        "--check",
        action="store_true",
        help="bind, answer /api/v1/user once and exit (smoke test)",
    )
    args = parser.parse_args()
    server = serve(args.port)
    if args.check:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        import urllib.request

        with urllib.request.urlopen(
            f"http://127.0.0.1:{args.port}/api/v1/user?code=gitea-signup"
        ) as response:
            print(response.status, response.read().decode())
        server.shutdown()
        return
    print(f"fake provider listening on http://127.0.0.1:{args.port}", file=sys.stderr)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
