#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Record the four OAuth providers' observable behaviour, without a network.

The golden file `crates/plane-auth/tests/golden/oauth_claims.json` is produced
by the **real** provider classes in `apps/api/plane/authentication/provider/
oauth/`: the script patches only

* `get_configuration_value`, so no database row is needed, and
* `requests.get`/`requests.post`, so no socket is opened.

Everything else — the auth URL, the token request's form and headers, the
`set_token_data` mapping and the claim rules (`verified_email`, `confirmed_at`,
primary+verified email, the organisation membership check) — runs in Python.

Run it from `apps/server/` with the dev environment, then commit the JSON:

    apps/api/.venv/bin/python tools/auth/record_oauth_claims.py

The Rust test `crates/plane-auth/src/auth_flow/providers/tests.rs` replays every
case against `plane_auth::auth_flow::providers` and fails on a mismatch.
"""

import json
import os
import sys
from datetime import datetime
from pathlib import Path

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO / "apps" / "api"))

import django  # noqa: E402

django.setup()

import requests  # noqa: E402

from plane.authentication.adapter.error import AuthenticationException  # noqa: E402
from plane.authentication.provider.oauth import gitea as gitea_module  # noqa: E402
from plane.authentication.provider.oauth import github as github_module  # noqa: E402
from plane.authentication.provider.oauth import gitlab as gitlab_module  # noqa: E402
from plane.authentication.provider.oauth import google as google_module  # noqa: E402

OUT = REPO / "apps" / "server" / "crates" / "plane-auth" / "tests" / "golden" / "oauth_claims.json"

HOST = "plane.example.test"


class FakeRequest:
    """The two attributes the providers read (`request.is_secure()`,
    `request.get_host()`)."""

    def is_secure(self):
        return False

    def get_host(self):
        return HOST


class FakeResponse:
    def __init__(self, status_code=200, payload=None, body=None):
        self.status_code = status_code
        self._payload = payload
        self._body = body

    @property
    def ok(self):
        return self.status_code < 400

    def raise_for_status(self):
        if self.status_code >= 400:
            raise requests.HTTPError(f"{self.status_code}")

    def json(self):
        if self._body is not None:
            # Reproduce requests' own exception: `JSONDecodeError` subclasses
            # `RequestException` in 2.33, which the providers catch.
            response = requests.Response()
            response._content = self._body.encode()
            response.status_code = self.status_code
            response.encoding = "utf-8"
            return response.json()
        return self._payload


def patch_config(module, values):
    """`get_configuration_value` returns `values` by key, so the environment
    does not decide the case."""
    module.get_configuration_value = lambda keys: tuple(values.get(key["key"]) for key in keys)


def patch_requests(answer):
    """`requests.get`/`requests.post` answer from `answer(method, url, kwargs)`."""
    captured = {}

    def post(url, data=None, headers=None, **kwargs):
        captured["post"] = {
            "url": url,
            "data": list(data.items()) if isinstance(data, dict) else data,
            "headers": headers or {},
        }
        return answer("POST", url, captured["post"])

    def get(url, headers=None, **kwargs):
        captured["get"] = {"url": url, "headers": headers or {}}
        return answer("GET", url, captured["get"])

    requests.post = post
    requests.get = get
    return captured


def datetime_json(value):
    if value is None:
        return None
    return value.astimezone(google_module.pytz.utc).isoformat()


def token_data_json(provider):
    expires = provider.token_data.get("access_token_expired_at")
    refresh = provider.token_data.get("refresh_token_expired_at")
    return {
        "access_token": provider.token_data.get("access_token"),
        "refresh_token": provider.token_data.get("refresh_token"),
        "access_token_expired_at": datetime_json(expires),
        "refresh_token_expired_at": datetime_json(refresh),
        "id_token": provider.token_data.get("id_token"),
    }


def error_json(error):
    return {
        "error_code": error.error_code,
        "error_message": error.error_message,
        "payload": error.payload,
    }


def run(case, build, action):
    """Run one case, turning an `AuthenticationException` into data and any
    other exception into `{"exception": <type name>}` (the 500 cases)."""
    record = dict(case)
    try:
        provider, captured = build()
        record["result"] = action(provider, captured)
    except AuthenticationException as error:
        record["result"] = {"authentication_exception": error_json(error)}
    except Exception as error:  # noqa: BLE001 — the point is the 500 cases
        record["result"] = {"exception": type(error).__name__}
    return record


def provider_case(module, config, state, kind):
    def build():
        patch_config(module, config)
        captured = patch_requests(lambda method, url, seen: RESPONSES[kind])
        provider = CLASSES[module](
            request=FakeRequest(),
            code=CODE,
            state=state,
        )
        return provider, captured

    return build


RESPONSES = {}
CODE = "fixture-code"


def build_auth_url_cases():
    cases = []

    def auth_url(module, config, state, kind="token-ok"):
        def action(provider, _captured):
            return {"auth_url": provider.get_auth_url()}

        return run(
            {
                "kind": "auth_url",
                "provider": module.__name__.rsplit(".", 1)[-1],
                "config": config,
                "state": state,
            },
            provider_case(module, config, state, kind),
            action,
        )

    google = {"GOOGLE_CLIENT_ID": "google-client", "GOOGLE_CLIENT_SECRET": "google-secret"}
    cases.append(auth_url(google_module, google, "0123456789abcdef0123456789abcdef"))
    cases.append(auth_url(google_module, {"GOOGLE_CLIENT_ID": "", "GOOGLE_CLIENT_SECRET": "s"}, "s"))
    github = {"GITHUB_CLIENT_ID": "github-client", "GITHUB_CLIENT_SECRET": "github-secret"}
    cases.append(auth_url(github_module, github, "0123456789abcdef0123456789abcdef"))
    github_org = dict(github, GITHUB_ORGANIZATION_ID="acme")
    cases.append(auth_url(github_module, github_org, "0123456789abcdef0123456789abcdef"))
    gitlab = {
        "GITLAB_CLIENT_ID": "gitlab-client",
        "GITLAB_CLIENT_SECRET": "gitlab-secret",
        "GITLAB_HOST": "http://127.0.0.1:18400",
    }
    cases.append(auth_url(gitlab_module, gitlab, "0123456789abcdef0123456789abcdef"))
    cases.append(auth_url(gitlab_module, dict(gitlab, GITLAB_HOST=""), "s"))
    gitea = {
        "GITEA_CLIENT_ID": "gitea-client",
        "GITEA_CLIENT_SECRET": "gitea-secret",
        "GITEA_HOST": "http://127.0.0.1:18400/",
    }
    cases.append(auth_url(gitea_module, gitea, "0123456789abcdef0123456789abcdef"))
    cases.append(auth_url(gitea_module, dict(gitea, GITEA_HOST="gitea.example.test"), "s"))
    return cases


TOKEN_RESPONSES = {
    "token-ok": FakeResponse(
        200,
        {
            "access_token": "access-ok",
            "refresh_token": "refresh-ok",
            "expires_in": 3600,
            "refresh_token_expired_at": 1700000000,
            "id_token": "id-token-ok",
        },
    ),
    "token-minimal": FakeResponse(200, {"access_token": "access-min"}),
    "token-zero-expiry": FakeResponse(200, {"access_token": "a", "expires_in": 0}),
    "token-string-expiry": FakeResponse(200, {"access_token": "a", "expires_in": "3600"}),
    "token-gitlab": FakeResponse(
        200,
        {
            "access_token": "gitlab-access",
            "refresh_token": "gitlab-refresh",
            "created_at": 1700000000,
            "expires_in": 7200,
        },
    ),
    "token-gitlab-no-created": FakeResponse(200, {"access_token": "a", "expires_in": 7200}),
    "token-gitea": FakeResponse(200, {"access_token": "gitea-access", "expires_in": 3600}),
    "token-error": FakeResponse(401, {"error": "invalid_grant"}),
    "token-not-json": FakeResponse(200, body="not json"),
}


def build_token_cases():
    cases = []
    google = {"GOOGLE_CLIENT_ID": "google-client", "GOOGLE_CLIENT_SECRET": "google-secret"}
    github = {"GITHUB_CLIENT_ID": "github-client", "GITHUB_CLIENT_SECRET": "github-secret"}
    gitlab = {
        "GITLAB_CLIENT_ID": "gitlab-client",
        "GITLAB_CLIENT_SECRET": "gitlab-secret",
        "GITLAB_HOST": "http://127.0.0.1:18400",
    }
    gitea = {
        "GITEA_CLIENT_ID": "gitea-client",
        "GITEA_CLIENT_SECRET": "gitea-secret",
        "GITEA_HOST": "http://127.0.0.1:18400",
    }
    for kind in ("token-ok", "token-minimal", "token-zero-expiry", "token-string-expiry"):
        cases.append(
            token_case(google_module, google, kind, "token_response.get")
        )
    cases.append(token_case(github_module, github, "token-ok", "token_response.get"))
    for kind in ("token-gitlab", "token-gitlab-no-created"):
        cases.append(token_case(gitlab_module, gitlab, kind, "token_response.get"))
    cases.append(token_case(gitea_module, gitea, "token-gitea", "token_response.get"))
    cases.append(token_case(gitea_module, gitea, "token-error", "token_response.get"))
    cases.append(token_case(gitea_module, gitea, "token-not-json", "token_response.get"))
    return cases


def token_case(module, config, kind, _label):
    def build():
        patch_config(module, config)
        captured = patch_requests(lambda method, url, seen: RESPONSES[kind])
        provider = CLASSES[module](request=FakeRequest(), code=CODE, state=None)
        return provider, captured

    def action(provider, captured):
        provider.set_token_data()
        record = {"token_request": captured["post"]}
        record.update(token_data_json(provider))
        return record

    response = RESPONSES[kind]
    case = {
        "kind": "token",
        "provider": module.__name__.rsplit(".", 1)[-1],
        "config": config,
        "response": kind,
        "status": response.status_code,
        "body_is_json": response._body is None,
        "token_response": response._payload,
    }
    return run(case, build, action)


def userinfo_case(module, config, payload, extra=None, org=None):
    """`payload` is the userinfo body; `extra` maps a URL suffix to a
    `FakeResponse` for the provider's additional calls."""

    token_kind = {
        gitlab_module: "token-gitlab",
        gitea_module: "token-gitea",
    }.get(module, "token-ok")

    def answer(method, url, _captured):
        if method == "POST":
            return RESPONSES[token_kind]
        for suffix, response in (extra or {}).items():
            if url.endswith(suffix):
                return response
        return FakeResponse(200, payload)

    def build():
        patch_config(module, config)
        captured = patch_requests(answer)
        provider = CLASSES[module](
            request=FakeRequest(),
            code=CODE,
            state=None,
        )
        if org is not None:
            provider.organization_id = org
        return provider, captured

    def action(provider, _captured):
        # `set_user_data` reads `self.token_data` for the extra bearer headers.
        provider.set_token_data()
        provider.get_user_response = lambda: payload
        provider.set_user_data()
        return {"user_data": provider.user_data}

    case = {
        "kind": "user_data",
        "provider": module.__name__.rsplit(".", 1)[-1],
        "config": config,
        "userinfo": payload,
        "extra": {
            suffix: {
                "status": response.status_code,
                "body_is_json": response._body is None,
                "payload": response._payload,
            }
            for suffix, response in (extra or {}).items()
        },
        "organization_id": org,
    }
    return run(case, build, action)


def build_user_data_cases():
    cases = []
    google = {"GOOGLE_CLIENT_ID": "google-client", "GOOGLE_CLIENT_SECRET": "google-secret"}
    for verified in (True, False, None, 1):
        payload = {
            "id": 1001,
            "email": "ada@example.com",
            "verified_email": verified,
            "given_name": "Ada",
            "family_name": "Lovelace",
            "picture": "https://example.test/ada.png",
        }
        cases.append(userinfo_case(google_module, google, payload))

    gitlab = {
        "GITLAB_CLIENT_ID": "gitlab-client",
        "GITLAB_CLIENT_SECRET": "gitlab-secret",
        "GITLAB_HOST": "http://127.0.0.1:18400",
    }
    for confirmed in (
        "2026-01-01T00:00:00Z",
        None,
        "",
    ):
        payload = {
            "id": 2001,
            "email": "ada@example.com",
            "confirmed_at": confirmed,
            "name": "Ada Lovelace",
            "avatar_url": "https://example.test/ada.png",
        }
        cases.append(userinfo_case(gitlab_module, gitlab, payload))

    github = {"GITHUB_CLIENT_ID": "github-client", "GITHUB_CLIENT_SECRET": "github-secret"}
    userinfo = {
        "id": 3001,
        "login": "ada",
        "name": "Ada Lovelace",
        "avatar_url": "https://example.test/ada.png",
    }
    email_sets = {
        "emails-primary-verified": [
            {"email": "second@example.com", "primary": False, "verified": True},
            {"email": "ada@example.com", "primary": True, "verified": True},
        ],
        "emails-secondary-only": [
            {"email": "second@example.com", "primary": False, "verified": True}
        ],
        "emails-unverified": [
            {"email": "ada@example.com", "primary": True, "verified": False}
        ],
        "emails-empty": [],
        "emails-not-list": {"email": "ada@example.com"},
        "emails-missing-key": [{"primary": True, "verified": True}],
        "emails-string-entry": ["ada@example.com"],
    }
    for name, payload in email_sets.items():
        cases.append(
            userinfo_case(
                github_module,
                github,
                userinfo,
                extra={"/user/emails": FakeResponse(200, payload)},
            )
        )
    cases.append(
        userinfo_case(
            github_module,
            github,
            userinfo,
            extra={"/user/emails": FakeResponse(403, {"message": "Bad credentials"})},
        )
    )
    # `response.json()`'s decode error is a RequestException in requests 2.33,
    # so this is the provider error rather than a 500.
    cases.append(
        userinfo_case(
            github_module,
            github,
            userinfo,
            extra={"/user/emails": FakeResponse(200, body="not json")},
        )
    )
    # The organisation membership check (`github.py:139-152`).
    cases.append(
        userinfo_case(
            github_module,
            dict(github, GITHUB_ORGANIZATION_ID="acme"),
            userinfo,
            extra={
                "/memberships/ada": FakeResponse(404, {"message": "Not Found"}),
                "/user/emails": FakeResponse(
                    200, [{"email": "ada@example.com", "primary": True, "verified": True}]
                ),
            },
            org="acme",
        )
    )
    cases.append(
        userinfo_case(
            github_module,
            dict(github, GITHUB_ORGANIZATION_ID="acme"),
            userinfo,
            extra={
                "/memberships/ada": FakeResponse(200, {"state": "active"}),
                "/user/emails": FakeResponse(
                    200, [{"email": "ada@example.com", "primary": True, "verified": True}]
                ),
            },
            org="acme",
        )
    )

    gitea = {
        "GITEA_CLIENT_ID": "gitea-client",
        "GITEA_CLIENT_SECRET": "gitea-secret",
        "GITEA_HOST": "http://127.0.0.1:18400",
    }
    gitea_user = {
        "id": 4001,
        "login": "ada",
        "full_name": "Ada Lovelace",
        "email": "",
        "avatar_url": "https://example.test/ada.png",
    }
    gitea_emails = {
        "gitea-primary-verified": [
            {"email": "second@example.com", "primary": False, "verified": True},
            {"email": "ada@example.com", "primary": True, "verified": True},
        ],
        "gitea-secondary-only": [
            {"email": "second@example.com", "primary": False, "verified": True}
        ],
        "gitea-unverified": [{"email": "ada@example.com", "primary": True, "verified": False}],
        "gitea-empty": [],
        "gitea-not-list": {"email": "ada@example.com"},
        "gitea-string": "abc",
        "gitea-missing-key": [{"primary": True, "verified": True}],
    }
    for name, payload in gitea_emails.items():
        cases.append(
            userinfo_case(
                gitea_module,
                gitea,
                gitea_user,
                extra={"/api/v1/user/emails": FakeResponse(200, payload)},
            )
        )
    cases.append(
        userinfo_case(
            gitea_module,
            gitea,
            gitea_user,
            extra={"/api/v1/user/emails": FakeResponse(403, {"message": "forbidden"})},
        )
    )
    cases.append(
        userinfo_case(
            gitea_module,
            gitea,
            gitea_user,
            extra={"/api/v1/user/emails": FakeResponse(200, body="not json")},
        )
    )
    # `full_name` falls back to `login`, then to `None`.
    for payload in (
        {"id": 4002, "login": "ada", "full_name": "", "avatar_url": None},
        {"id": 4003, "avatar_url": None},
        {"login": "", "full_name": ""},
    ):
        cases.append(
            userinfo_case(
                gitea_module,
                gitea,
                payload,
                extra={
                    "/api/v1/user/emails": FakeResponse(
                        200, [{"email": "ada@example.com", "primary": True, "verified": True}]
                    )
                },
            )
        )
    return cases


CLASSES = {
    google_module: google_module.GoogleOAuthProvider,
    github_module: github_module.GitHubOAuthProvider,
    gitlab_module: gitlab_module.GitLabOAuthProvider,
    gitea_module: gitea_module.GiteaOAuthProvider,
}

RESPONSES.update(TOKEN_RESPONSES)


def main():
    cases = []
    cases.extend(build_auth_url_cases())
    cases.extend(build_token_cases())
    cases.extend(build_user_data_cases())
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps({"cases": cases}, indent=2) + "\n")
    print(f"wrote {len(cases)} cases to {OUT}")


if __name__ == "__main__":
    main()
