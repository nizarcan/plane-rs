# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the P0 auth-flow golden corpora from the real Python.

Run from `apps/api` with the venv interpreter:

    cd apps/api
    .venv/bin/python ../server/tools/auth/record_corpora.py

Writes:

* `crates/plane-auth/tests/golden/redirect.json` — `validate_next_path`,
  `get_safe_redirect_url`, `get_allowed_hosts`, Django's
  `url_has_allowed_host_and_scheme`, Plane's space redirect and
  `urllib.parse.urljoin` (the Gitea shape).
* `crates/plane-auth/tests/golden/reset_tokens.json` — Django 5.2.17's
  `PasswordResetTokenGenerator` and `urlsafe_base64_{encode,decode}` at fixed
  keys, users and timestamps.
* `tools/auth/zxcvbn_corpus.json` — `plane.authentication.utils.password.
  is_password_strong` for 512 passwords (first/last 128-character windows).

Nothing here is imported by the product; `--check` is not implemented because
the corpora pin CPython 3.12.13 / Django 5.2.17, which the venv already fixes.
"""

import datetime
import importlib.util
import json
import os
import random
import string
import sys
import urllib.parse

import django
from django.conf import settings
from django.utils.http import (
    url_has_allowed_host_and_scheme,
    urlsafe_base64_decode,
    urlsafe_base64_encode,
)

HERE = os.path.dirname(os.path.abspath(__file__))
SERVER = os.path.dirname(os.path.dirname(HERE))
REPO = os.path.dirname(os.path.dirname(SERVER))
GOLDEN = os.path.join(SERVER, "crates", "plane-auth", "tests", "golden")


def configure():
    settings.configure(
        DEBUG=False,
        USE_TZ=True,
        SECRET_KEY="record-corpora-" + "x" * 40,
        WEB_URL="http://localhost:3000",
        APP_BASE_URL="http://localhost:3000",
        ADMIN_BASE_URL="http://localhost:3001",
        SPACE_BASE_URL="http://localhost:3002",
        PASSWORD_RESET_TIMEOUT=60 * 60 * 24 * 3,
    )
    django.setup()


def load_path_validator():
    path = os.path.join(REPO, "apps", "api", "plane", "utils", "path_validator.py")
    spec = importlib.util.spec_from_file_location("plane_path_validator", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def outcome(function, *args):
    try:
        return {"value": function(*args)}
    except Exception as error:  # noqa: BLE001 - the exception is the oracle
        return {"exception": f"{type(error).__name__}: {error}"}


# ---------------------------------------------------------------------------
# Redirect corpus
# ---------------------------------------------------------------------------

URL_SETTINGS = [
    # The dev stack.
    ("http://localhost:3000", "http://localhost:3000", "http://localhost:3001", "http://localhost:3002"),
    # Nothing configured: the empty-allow-list quirk.
    (None, None, None, None),
    # One host, the others unset.
    ("https://app.plane.example", None, None, None),
    # Distinct origins with ports.
    ("http://web:80", "http://app:8080", "http://admin:9000", "http://space:7000"),
    # Duplicates: `get_allowed_hosts` dedupes in order.
    ("https://plane.example", "https://plane.example", "https://plane.example/", "https://plane.example/admin"),
    # A value that is not a URL: urlparse().netloc is empty, so it adds nothing.
    ("not a url", "/relative", "http://[bad", "mailto:a@b.com"),
    # Trailing slash / path components: the netloc ignores them.
    ("https://example.com/", "https://example.com/a/b", "https://example.com:8443", "https://space.example/x?y=1"),
    # A host that is a suffix of another, and a bracketed IPv6 URL.
    ("https://a.example.com", "https://example.com", "http://[::1]:3000", "https://xn--bcher-kva.example"),
]

NEXT_PATHS = [
    None,
    "",
    "/",
    "/my-workspace/",
    "/my-workspace/issues/",
    "/my-workspace?foo=bar",
    "/my-workspace#frag",
    "/?a=1&b=2",
    "onboarding",
    "my-workspace",
    "invitations",
    "create-workspace",
    "//evil.com",
    "///evil.com",
    "////evil.com",
    "/\\evil.com",
    "/\\/evil.com",
    "\\\\evil.com",
    "/%2e%2e/etc",
    "/%2E%2E/etc",
    "/..",
    "/a/../b",
    "/a/%2f%2fb",
    "/a/%5c%5cb",
    "/javascript:alert(1)",
    "/data:text/html,x",
    "/vbscript:msgbox(1)",
    "/file:///etc/passwd",
    "/ftp://example.com",
    "/<script>alert(1)</script>",
    "/<iframe>",
    "/<object>",
    "/<embed>",
    "/<form>",
    "/x onload=alert(1)",
    "/x onerror=alert(1)",
    "/x onclick=alert(1)",
    "/JavaScript:alert(1)",
    "/DATA:text/html,x",
    "http://localhost:3000/ok",
    "http://localhost:3000/ok?x=1",
    "https://app.plane.example/deep/path",
    "http://evil.com/ok",
    "http://localhost:3000@evil.com/ok",
    "http://evil.com@localhost:3000/ok",
    "http://localhost:3000/ok;params",
    "http://localhost:3000/a;b?c=d",
    "http://localhost:3000/ok#frag",
    "HTTP://LOCALHOST:3000/ok",
    "mailto:a@b.com",
    "tel:+1234",
    "javascript:alert(1)",
    "data:text/html,x",
    "ftp://example.com/x",
    "//localhost:3000/ok",
    "/\t/\t/evil.com",
    "/\r/evil.com",
    "/\n/evil.com",
    "/a\tb",
    "/\x00abc",
    "/\x1fabc",
    "/ space",
    "/%",
    "/?next=http://evil.com",
    "/?x=1&y=2#z",
    "/a" * 250,
    "/" + "a" * 500,
    "/" + "a" * 501,
    "http://localhost:3000/" + "a" * 600,
    "/\\\t/evil.com",
    "/\u00e9t\u00e9",
    "/\u2028x",
    "/\U0001f600",
    "http://[::1]:3000/x",
    "http://[::1]/x",
    "http://[127.0.0.1]/x",
    "http://[bad]/x",
    "http://[::1/x",
    "http://host:bad/x",
    "//[::1]/x",
    "////[::1]/x",
    "/ok?q=%00",
    "/ok?q=<script>",
    "/ok#<script>",
    "/ok?next=/a/../b",
]

PARAMS = [
    [],
    [("error_code", 5000), ("error_message", "INSTANCE_NOT_CONFIGURED")],
    [("error_code", 5005), ("error_message", "INVALID_EMAIL"), ("email", "a@b.com")],
    [("error_code", 5070), ("error_message", "REQUIRED_EMAIL_PASSWORD_SIGN_IN"), ("email", "None")],
    [("email", "with space+and&amp@example.com"), ("error_code", 5021), ("error_message", "PASSWORD_TOO_WEAK")],
    [("next_path", "/already"), ("error_code", 5900), ("error_message", "RATE_LIMIT_EXCEEDED")],
    [("unicode", "é"), ("emoji", "😀"), ("error_code", 5010), ("error_message", "EMAIL_REQUIRED")],
]


def redirect_corpus(pv):
    cases = []
    for web, app, admin, space in URL_SETTINGS:
        settings.WEB_URL = web
        settings.APP_BASE_URL = app
        settings.ADMIN_BASE_URL = admin
        settings.SPACE_BASE_URL = space
        allowed_outcome = outcome(pv.get_allowed_hosts)
        allowed = allowed_outcome.get("value", [])
        for base in [app, web, "http://localhost:3000/", "http://localhost:3000/base", "https://app.plane.example", ""]:
            if base is None:
                continue
            for next_path in NEXT_PATHS:
                for params in PARAMS:
                    if next_path not in (None, "", "/", "/my-workspace/", "//evil.com", "onboarding", "http://localhost:3000/ok"):
                        # The cross product is already large; keep one params
                        # shape for the exotic paths.
                        if params is not PARAMS[1]:
                            continue
                    param_pairs = [[key, str(value) if not isinstance(value, str) else value] for key, value in params]
                    param_dict = dict(params)
                    validated = outcome(pv.validate_next_path, next_path)
                    redirect = outcome(pv.get_safe_redirect_url, base, next_path, param_dict)
                    # Space's flow: `base.rstrip("/") + validate_next_path(...)`
                    # and a plain host check.
                    validated_path = validated.get("value", "")
                    space_url = f"{base.rstrip('/')}{validated_path}"
                    space_allowed = (
                        url_has_allowed_host_and_scheme(space_url, allowed_hosts=allowed)
                        if "value" in allowed_outcome
                        else None
                    )
                    cases.append(
                        {
                            "web_url": web,
                            "app_base_url": app,
                            "admin_base_url": admin,
                            "space_base_url": space,
                            "base_url": base,
                            "next_path": next_path,
                            "params": param_pairs,
                            "allowed_hosts": allowed_outcome,
                            "validated": validated,
                            "redirect": redirect,
                            "space_url": space_url,
                            "space_allowed": space_allowed,
                        }
                    )

    # Restore the dev settings for any import that runs afterwards.
    settings.WEB_URL = "http://localhost:3000"
    settings.APP_BASE_URL = "http://localhost:3000"
    settings.ADMIN_BASE_URL = "http://localhost:3001"
    settings.SPACE_BASE_URL = "http://localhost:3002"

    url_checks = []
    host_lists = [
        [],
        ["localhost:3000"],
        ["localhost:3000", "localhost:3001", "localhost:3002"],
        ["localhost:3000", ".example.com"],
        ["EXAMPLE.com"],
    ]
    urls = [
        "",
        "/",
        "/path",
        "http://localhost:3000/",
        "http://localhost:3000",
        "https://localhost:3000/",
        "http://localhost:3001/x",
        "http://localhost:4000/x",
        "//localhost:3000/x",
        "///localhost:3000/x",
        "////localhost:3000/x",
        "http:///localhost:3000/x",
        "https:///x",
        "http://evil.com/x",
        "http://evil.com@localhost:3000/x",
        "http://localhost:3000@evil.com/x",
        "http://localhost:3000/x?y=1",
        "http://localhost:3000/x#f",
        "javascript:alert(1)",
        "javascript:/x",
        "data:,x",
        "ftp://localhost:3000/x",
        "mailto:a@b.com",
        "http://localhost:3000/x\\y",
        "\\\\localhost:3000\\x",
        "http://localhost:3000\\@evil.com/x",
        " http://localhost:3000/x ",
        "\thttp://localhost:3000/x",
        "\x00http://localhost:3000/x",
        "http://localhost:3000/x\n",
        "http://localhost:3000/" + "a" * 2100,
        "http://localhost:3000/" + "a" * 2000,
        "http://[::1]:3000/x",
        "http://[::1]/x",
        "http://[127.0.0.1]/x",
        "http://[bad]/x",
        "http://[::1/x",
        "http://host:bad/x",
        "http://example.com/x",
        "http://sub.example.com/x",
        "https://example.com/x",
        "http://badexample.com/x",
        "\u00e9http://localhost:3000/x",
        "http://localhost:3000/\u00e9",
        "http://exa\u2100mple.com/x",
    ]
    for url in urls:
        for hosts in host_lists:
            url_checks.append(
                {
                    "url": url,
                    "allowed_hosts": hosts,
                    "allow": url_has_allowed_host_and_scheme(url, allowed_hosts=hosts),
                    "allow_require_https": url_has_allowed_host_and_scheme(url, allowed_hosts=hosts, require_https=True),
                }
            )

    # `urljoin`: the Gitea call sites only.
    joins = []
    bases = [
        "http://localhost:3000",
        "http://localhost:3000/",
        "http://localhost:3000/app",
        "http://localhost:3000/app/",
        "http://localhost:3000/app?x=1",
        "https://app.plane.example",
        "https://app.plane.example/base/",
        "http://localhost:3000/#frag",
        "/relative/base",
        "?x=1",
        "",
        "not a url",
    ]
    relatives = [
        "?error_code=5005&error_message=INVALID_EMAIL",
        "?",
        "?a=1&a=2&b=",
        "?" + "x" * 300,
        "onboarding",
        "my-workspace",
        "invitations",
        "/my-workspace/",
        "/",
        "",
        "#frag",
        "/a?b=c#d",
        "../up",
        "//other.example/x",
        "http://other.example/x",
        "https://other.example",
        "?next_path=%2Fa&error_code=5000",
        "\\evil",
        "a b",
    ]
    for base in bases:
        for relative in relatives:
            joins.append(
                {
                    "base": base,
                    "relative": relative,
                    "result": outcome(urllib.parse.urljoin, base, relative),
                }
            )

    return {
        "python": sys.version.split()[0],
        "django": django.get_version(),
        "cases": cases,
        "url_checks": url_checks,
        "joins": joins,
    }


# ---------------------------------------------------------------------------
# Reset-token corpus
# ---------------------------------------------------------------------------


class FakeUser:
    def __init__(self, pk, password, last_login=None, email=""):
        self.pk = pk
        self.password = password
        self.last_login = last_login
        self.email = email

    def get_email_field_name(self):
        return "email"


class FixedNowGenerator:
    """`PasswordResetTokenGenerator` with `_now` pinned."""

    def __init__(self, generator_class, now):
        self.generator = generator_class()
        self.now = now

    def __getattr__(self, name):
        return getattr(self.generator, name)


def reset_corpus():
    from django.contrib.auth.tokens import PasswordResetTokenGenerator

    secrets = [
        "secret",
        "django-insecure-" + "k" * 50,
        "kéy-😀",
    ]
    passwords = [
        "pbkdf2_sha256$1000000$abcdefgh$0123456789abcdef",
        "pbkdf2_sha256$260000$salt$hash",
        "!",
        "",
    ]
    emails = ["a@b.com", "", "MiXeD@Example.COM"]
    timestamps = [0, 1, 35, 36, 1295, 1777777777, 2147483647]
    logins = [
        None,
        datetime.datetime(2023, 1, 2, 3, 4, 5, 123456, tzinfo=datetime.timezone.utc),
        datetime.datetime(2020, 12, 31, 23, 59, 59, tzinfo=datetime.timezone.utc),
    ]

    tokens = []
    for secret in secrets:
        generator = PasswordResetTokenGenerator()
        generator.secret = secret
        for pk in [1, 42, "a1b2c3d4-0000-0000-0000-000000000000"]:
            for password in passwords:
                for email in emails:
                    for last_login in logins:
                        for ts in timestamps:
                            user = FakeUser(pk, password, last_login, email)
                            value = generator._make_token_with_timestamp(user, ts, secret)
                            tokens.append(
                                {
                                    "secret": secret,
                                    "pk": str(pk),
                                    "password": password,
                                    "email": email,
                                    "last_login": None if last_login is None else last_login.isoformat(),
                                    "timestamp": ts,
                                    "token": value,
                                }
                            )

    checks = []
    nows = [
        datetime.datetime(2001, 1, 1, 0, 0, 0),
        datetime.datetime(2023, 1, 1, 0, 0, 0),
        datetime.datetime(2030, 5, 6, 7, 8, 9),
    ]
    for secret in secrets[:2]:
        for now in nows:
            generator = PasswordResetTokenGenerator()
            generator.secret = secret
            generator._now = lambda now=now: now
            for password in passwords[:2]:
                user = FakeUser(42, password, logins[1], emails[0])
                token = generator.make_token(user)
                ts = generator._num_seconds(now)
                variants = [
                    token,
                    token.upper(),
                    token.split("-")[0],
                    token + "-extra",
                    "",
                    "x-y",
                    f"{ts + 259200}-{token.split('-')[1]}",
                    f"{ts + 259201}-{token.split('-')[1]}",
                    f"{ts}-{token.split('-')[1]}",
                ]
                for candidate in variants:
                    checks.append(
                        {
                            "secret": secret,
                            "pk": "42",
                            "password": password,
                            "email": emails[0],
                            "last_login": logins[1].isoformat(),
                            "now": now.isoformat(),
                            "token": candidate,
                            "valid": generator.check_token(user, candidate),
                        }
                    )

    base64_cases = []
    payloads = ["", "1", "42", "a1b2c3d4-0000-0000-0000-000000000000", "é😀", "x" * 100]
    for payload in payloads:
        encoded = urlsafe_base64_encode(payload.encode())
        decoded = outcome(lambda value=encoded: urlsafe_base64_decode(value).decode())
        base64_cases.append({"input": payload, "encoded": encoded, "decoded": decoded})
    for value in ["", "a", "ab", "abc", "abcd", "ab=c", "a b", "!!", "abcde", "-_8="]:
        base64_cases.append(
            {
                "input": None,
                "encoded": value,
                "decoded": outcome(lambda value=value: urlsafe_base64_decode(value).decode()),
            }
        )

    return {
        "python": sys.version.split()[0],
        "django": django.get_version(),
        "timeout": settings.PASSWORD_RESET_TIMEOUT,
        "tokens": tokens,
        "checks": checks,
        "base64": base64_cases,
    }


# ---------------------------------------------------------------------------
# zxcvbn corpus
# ---------------------------------------------------------------------------


def zxcvbn_corpus():
    path = os.path.join(REPO, "apps", "api", "plane", "authentication", "utils", "password.py")
    spec = importlib.util.spec_from_file_location("plane_password", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    is_password_strong = module.is_password_strong

    weak = [
        "password", "Password1", "Password123", "password123!", "123456", "12345678",
        "qwerty", "qwertyuiop", "abc123", "letmein", "welcome", "monkey", "dragon",
        "iloveyou", "admin", "administrator", "root", "toor", "changeme", "secret",
        "pl@ne", "plane", "plane123", "makeplane", "test", "test1234", "guest",
        "aaaaaaaa", "aaaaaaaaaaaaaaaa", "11111111", "00000000", "asdfghjkl",
        "zxcvbnm", "qazwsx", "1q2w3e4r", "1qaz2wsx", "trustno1", "sunshine",
        "princess", "football", "baseball", "superman", "starwars", "master",
        "shadow", "michael", "jennifer", "jordan", "harley", "ranger",
        "abcdef", "abcdefgh", "abcd1234", "a1b2c3d4", "passw0rd", "p@ssw0rd",
        "correcthorsebatterystaple", "Tr0ub4dor&3", "hunter2", "letmein1",
        "qwerty123", "1q2w3e4r5t", "zaq12wsx", "qweasdzxc", "123qweasd",
    ]
    weak += [
        "1" * 32, "a" * 32, "ab" * 32, "abc" * 20, "password" * 4,
        "qwerty" * 5, "1234567890" * 3, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaA1!", "password1!", "P@ssword", "P@ssw0rd!", "Welcome1",
        "Summer2024!", "Winter2024!", "Company123!", "Plane@123",
    ]

    # Fixture and contract-test passwords.
    fixture = [
        "password", "password123", "test-password", "Str0ng!Passw0rd",
        "fixture-password", "admin", "Admin1234!", "demo1234", "user1234",
        "plane", "plane@123", "Plane1234!", "x", "1", "12", "123",
    ]

    rng = random.Random(20261117)
    random_cases = []
    alphabets = [
        string.ascii_letters + string.digits,
        string.ascii_letters + string.digits + string.punctuation,
        string.ascii_lowercase,
        string.digits,
        string.printable[:94],
    ]
    lengths = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 20, 24, 32, 40, 48,
               56, 64, 80, 96, 100, 120, 127, 128, 129, 150, 200, 256, 300]
    for length in lengths:
        for alphabet in alphabets:
            random_cases.append("".join(rng.choice(alphabet) for _ in range(length)))
    for _ in range(60):
        length = rng.randrange(1, 260)
        random_cases.append("".join(rng.choice(string.printable[:94]) for _ in range(length)))

    unicode_cases = [
        "пароль", "Пароль123", "密码", "密码123456", "パスワード", "パスワード123",
        "비밀번호", "كلمةالسر", "motdepasse", "MotDePasse123!", "contraseña",
        "Contraseña123!", "senha", "Senha123!", "passwort", "Passwort123!",
        "😀😀😀😀😀😀😀😀", "😀a😀b😀c😀d", "é" * 20, "é" * 129,
        "Ω" * 40, "日本語のパスワード", "密码密码密码密码密码密码密码密码",
        "üñïçøðé", "pässwörd", "pässwörd123!", "🔥🔥🔥🔥🔥🔥🔥🔥🔥🔥",
        "\u2028\u2029", "a\u0000b", "\u0000" * 10,
    ]

    long_cases = []
    for length in [128, 129, 130, 150, 200, 256, 300, 400]:
        long_cases.append("a" * length)
        long_cases.append("1" * length)
        long_cases.append(("ab" * (length // 2 + 1))[:length])
        long_cases.append(("password" * (length // 8 + 1))[:length])
        long_cases.append(("xY9!" * (length // 4 + 1))[:length])
        long_cases.append("".join(rng.choice(string.ascii_letters + string.digits) for _ in range(length)))
    # Windows differ: only the last 128 characters are strong.
    long_cases.append("a" * 200 + "xY9!xY9!xY9!xY9!xY9!xY9!xY9!xY9!")
    long_cases.append("xY9!xY9!xY9!xY9!xY9!xY9!xY9!xY9!" + "a" * 200)
    long_cases.append("a" * 300 + "Tr0ub4dor&3xY9!" * 10)
    long_cases.append("Tr0ub4dor&3xY9!" * 10 + "a" * 300)
    long_cases.append("😀" * 200)
    long_cases.append("密码" * 200)

    words = [
        "correct", "horse", "battery", "staple", "coffee", "guitar", "summer",
        "winter", "spring", "autumn", "london", "paris", "tokyo", "berlin",
        "alice", "bob", "carol", "dave", "eve", "frank", "grace", "heidi",
        "ivan", "judy", "mallory", "oscar", "peggy", "trent", "victor", "walter",
        "company", "project", "workspace", "issue", "cycle", "module", "plane",
        "backend", "frontend", "server", "client", "database", "redis", "postgres",
    ]
    composed = []
    for _ in range(150):
        count = rng.randrange(1, 5)
        pieces = [rng.choice(words) for _ in range(count)]
        separator = rng.choice(["", "-", "_", " ", ".", "@"])
        value = separator.join(pieces)
        if rng.random() < 0.4:
            value += str(rng.randrange(0, 3000))
        if rng.random() < 0.3:
            value = value.capitalize()
        if rng.random() < 0.2:
            value += rng.choice(["!", "!!", "?", "#", "$", "&", "*"])
        composed.append(value)

    all_cases = weak + fixture + random_cases + unicode_cases + long_cases + composed
    seen = set()
    unique = []
    for password in all_cases:
        if password in seen:
            continue
        seen.add(password)
        unique.append(password)
    assert len(unique) >= 500, len(unique)

    cases = []
    for password in unique:
        first = password[:128]
        last = password[-128:]
        first_score = __import__("zxcvbn").zxcvbn(first)["score"]
        last_score = __import__("zxcvbn").zxcvbn(last)["score"]
        cases.append(
            {
                "password": password,
                "first": first,
                "last": last,
                "first_score": first_score,
                "last_score": last_score,
                "strong": is_password_strong(password),
            }
        )

    return {
        "python": sys.version.split()[0],
        "zxcvbn": __import__("importlib.metadata", fromlist=["version"]).version("zxcvbn"),
        "cases": cases,
    }


def main():
    configure()
    pv = load_path_validator()
    os.makedirs(GOLDEN, exist_ok=True)

    redirect = redirect_corpus(pv)
    with open(os.path.join(GOLDEN, "redirect.json"), "w", encoding="utf-8") as handle:
        # Compact: the corpus is mostly long hostile paths repeated per base.
        json.dump(redirect, handle, ensure_ascii=False, separators=(",", ":"))
        handle.write("\n")
    print(f"redirect: {len(redirect['cases'])} cases, {len(redirect['url_checks'])} url checks, {len(redirect['joins'])} joins")

    reset = reset_corpus()
    with open(os.path.join(GOLDEN, "reset_tokens.json"), "w", encoding="utf-8") as handle:
        json.dump(reset, handle, ensure_ascii=False, separators=(",", ":"))
        handle.write("\n")
    print(f"reset tokens: {len(reset['tokens'])} tokens, {len(reset['checks'])} checks, {len(reset['base64'])} base64")

    zx = zxcvbn_corpus()
    with open(os.path.join(HERE, "zxcvbn_corpus.json"), "w", encoding="utf-8") as handle:
        json.dump(zx, handle, ensure_ascii=False, indent=1)
        handle.write("\n")
    print(f"zxcvbn: {len(zx['cases'])} passwords")


if __name__ == "__main__":
    main()
