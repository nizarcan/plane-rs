#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""An intercepting HTTPS forward proxy for the hardcoded Google/GitHub endpoints.

`fake_provider.py` covers GitLab and Gitea because Plane builds their URLs from
`GITLAB_HOST`/`GITEA_HOST`. Google's and GitHub's URLs are class constants
(`provider/oauth/google.py:22-23`, `github.py:24-26`), so the only way to run
their **callbacks** end to end is to intercept TLS:

* the target process gets `HTTPS_PROXY=http://127.0.0.1:<port>`; both
  `requests` (Python) and `plane-net`'s unguarded preset (D9) read it;
* the proxy answers `CONNECT`, then terminates TLS with a CA generated into
  `--ca-dir` at startup, and the target trusts **only** that CA through
  `REQUESTS_CA_BUNDLE`/`CURL_CA_BUNDLE`/`SSL_CERT_FILE`. A request that
  bypasses the proxy therefore fails certificate verification against the real
  provider, and the scenario cannot silently pass off the machine;
* the inner requests are answered from a fixed table keyed by the OAuth `code`
  (token endpoint) and by the bearer token it issued (`access-<code>`) for the
  userinfo/emails/membership calls, exactly like `fake_provider.py`.

Every connection and request is written to `--log` as one JSON object per line:
`connect`, `request` (with `status`) and `blocked` (a CONNECT to a host outside
the allowlist, or an absolute-form request to one). `verify_mitm_log.py` asserts
the exact call inventory, so a provider request that never reached the proxy is
a wrapper failure rather than a silently different login.

The certificate and keys live under a gitignored directory
(`apps/server/target/oauth-mitm/` by default in the wrapper); nothing is
committed. `--check` runs a self-test through the proxy with the generated CA.

    apps/api/.venv/bin/python tools/auth/oauth_https_proxy.py \
        --port 18401 --ca-dir target/oauth-mitm --log target/oauth-mitm/requests.jsonl
"""

import argparse
import json
import os
import ssl
import sys
import threading
from datetime import UTC, datetime, timedelta
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

# The four hosts Plane's Google/GitHub providers call. `accounts.google.com`
# and `github.com`'s authorize URL are never fetched by the server (they are
# redirect targets), but they are in the allowlist so the certificate covers
# them and a mistake is still intercepted rather than leaked.
ALLOWED_HOSTS = frozenset(
    {
        "oauth2.googleapis.com",
        "www.googleapis.com",
        "accounts.google.com",
        "github.com",
        "api.github.com",
    }
)

CA_CERT_NAME = "ca.pem"
CA_KEY_NAME = "ca.key"
LEAF_CERT_NAME = "leaf.pem"
LEAF_KEY_NAME = "leaf.key"

# `fake_provider.py`'s avatar. The pinned avatar fetch rejects loopback before
# any connection, so both targets fall back to storing the URL (the same branch
# the GitLab/Gitea scenario pins).
FAKE_PROVIDER_PORT = 18400
AVATAR_URL = f"http://127.0.0.1:{FAKE_PROVIDER_PORT}/avatar.png"

SIGNIN_GOOGLE_ID = "google-admin-account"
SIGNIN_GOOGLE_EMAIL = "fix.admin@example.com"
SIGNIN_GITHUB_ID = "github-member-account"
SIGNIN_GITHUB_EMAIL = "fix.member@example.com"

# GitHub organization membership: everyone is a member except this login, so
# one scenario covers both branches with GITHUB_ORGANIZATION_ID set.
NONMEMBER_LOGIN = "gh-outsider"


def google_user(case):
    """The `/oauth2/v2/userinfo` body for a case, or `None` for a 403."""
    if case == "google-signin":
        return {
            "id": SIGNIN_GOOGLE_ID,
            "email": SIGNIN_GOOGLE_EMAIL,
            "verified_email": True,
            "given_name": "Ada Grace",
            "family_name": "Lovelace",
            "picture": AVATAR_URL,
        }
    if case == "google-unverified":
        return {
            "id": "google-unverified-account",
            "email": "oauth.google.unverified@example.com",
            "verified_email": False,
            "given_name": "Una",
            "family_name": "Verified",
            "picture": None,
        }
    if case == "google-unverified-absent":
        # `google.py:108-112` fails closed on an absent claim.
        return {
            "id": "google-unverified-absent-account",
            "email": "oauth.google.unverified.absent@example.com",
            "given_name": "Nora",
            "family_name": "Claim",
            "picture": None,
        }
    if case == "google-userinfo-not-object":
        return []
    if case == "google-userinfo-error":
        return None
    if case == "google-space-signin":
        return {
            "id": SIGNIN_GOOGLE_ID,
            "email": SIGNIN_GOOGLE_EMAIL,
            "verified_email": True,
            "given_name": "Ada Grace",
            "family_name": "Lovelace",
            "picture": AVATAR_URL,
        }
    if case == "google-space-signup":
        return {
            "id": "google-space-account",
            "email": "oauth.google.space@example.com",
            "verified_email": True,
            "given_name": "Sage",
            "family_name": "Space",
            "picture": AVATAR_URL,
        }
    if case == "google-space-unverified":
        return {
            "id": "google-space-unverified-account",
            "email": "oauth.google.space.unverified@example.com",
            "verified_email": False,
            "given_name": "Una",
            "family_name": "Space",
            "picture": None,
        }
    # `google-signup` and every other case create this user.
    return {
        "id": "google-signup-account",
        "email": "oauth.google.signup@example.com",
        "verified_email": True,
        "given_name": "Gina",
        "family_name": "Signup",
        "picture": AVATAR_URL,
    }


def github_user(case):
    """The `/user` body for a case, or `None` for a 500."""
    if case == "github-signin":
        return {
            "id": SIGNIN_GITHUB_ID,
            "login": "gh-member",
            "name": "Mia Member",
            "avatar_url": AVATAR_URL,
        }
    if case == "github-space-signin":
        return {
            "id": SIGNIN_GITHUB_ID,
            "login": "gh-member",
            "name": "Mia Member",
            "avatar_url": AVATAR_URL,
        }
    if case == "github-outsider":
        return {
            "id": "github-outsider-account",
            "login": NONMEMBER_LOGIN,
            "name": "Oscar Outsider",
            "avatar_url": AVATAR_URL,
        }
    if case == "github-unverified":
        return {
            "id": "github-unverified-account",
            "login": "gh-unverified",
            "name": "Una Verified",
            "avatar_url": None,
        }
    if case == "github-primary-unverified":
        return {
            "id": "github-primary-unverified-account",
            "login": "gh-primary-unverified",
            "name": "Prim Ary",
            "avatar_url": None,
        }
    if case == "github-secondary-first":
        return {
            "id": "github-secondary-first-account",
            "login": "gh-secondary-first",
            "name": "Sid Secondary",
            "avatar_url": AVATAR_URL,
        }
    if case == "github-emails-not-list":
        return {
            "id": "github-emails-not-list-account",
            "login": "gh-emails-not-list",
            "name": "Lena List",
            "avatar_url": None,
        }
    if case == "github-emails-not-json":
        return {
            "id": "github-emails-not-json-account",
            "login": "gh-emails-not-json",
            "name": "Jon Json",
            "avatar_url": None,
        }
    if case == "github-userinfo-error":
        return None
    if case == "github-space-signup":
        return {
            "id": "github-space-account",
            "login": "gh-space",
            "name": "Sage Space",
            "avatar_url": AVATAR_URL,
        }
    if case == "github-space-unverified":
        return {
            "id": "github-space-unverified-account",
            "login": "gh-space-unverified",
            "name": "Una Space",
            "avatar_url": None,
        }
    # `github-signup` and every other case create this user.
    return {
        "id": "github-signup-account",
        "login": "gh-signup",
        "name": "Gina Signup",
        "avatar_url": AVATAR_URL,
    }


def github_emails(case):
    """`(status, body, content_type)` for `/user/emails`."""
    if case == "github-signin" or case == "github-space-signin":
        return (
            200,
            [
                {"email": "other@example.com", "primary": False, "verified": True},
                {"email": SIGNIN_GITHUB_EMAIL, "primary": True, "verified": True},
            ],
            "application/json",
        )
    if case == "github-unverified":
        # Only an unverified primary address: `github.py:120-132` rejects it.
        return (
            200,
            [
                {
                    "email": "oauth.github.unverified@example.com",
                    "primary": True,
                    "verified": False,
                }
            ],
            "application/json",
        )
    if case == "github-primary-unverified":
        # A verified *secondary* address must not rescue an unverified primary.
        return (
            200,
            [
                {"email": "unverified.primary@example.com", "primary": True, "verified": False},
                {"email": "verified.secondary@example.com", "primary": False, "verified": True},
            ],
            "application/json",
        )
    if case == "github-secondary-first":
        # The primary entry comes second: the `next()` must still pick it, not
        # the first verified address.
        return (
            200,
            [
                {"email": "verified.secondary@example.com", "primary": False, "verified": True},
                {
                    "email": "oauth.github.signup@example.com",
                    "primary": True,
                    "verified": True,
                },
            ],
            "application/json",
        )
    if case == "github-emails-not-list":
        # A JSON object instead of a list is the provider's own error.
        return (200, {"message": "unexpected"}, "application/json")
    if case == "github-emails-not-json":
        return (200, "<html>not json</html>", "text/html")
    if case == "github-space-unverified":
        return (
            200,
            [
                {
                    "email": "oauth.github.space.unverified@example.com",
                    "primary": True,
                    "verified": False,
                }
            ],
            "application/json",
        )
    if case == "github-space-signup":
        return (
            200,
            [{"email": "oauth.github.space@example.com", "primary": True, "verified": True}],
            "application/json",
        )
    return (
        200,
        [{"email": "oauth.github.signup@example.com", "primary": True, "verified": True}],
        "application/json",
    )


def _case_from_bearer(headers):
    token = headers.get("Authorization", "").removeprefix("Bearer ").strip()
    return token.removeprefix("access-") if token.startswith("access-") else ""


class Log:
    """One JSON object per line, flushed, guarded by a lock."""

    def __init__(self, path):
        self._lock = threading.Lock()
        self._file = open(path, "w", buffering=1, encoding="utf-8")

    def write(self, record):
        with self._lock:
            self._file.write(json.dumps(record, sort_keys=True) + "\n")
            self._file.flush()

    def close(self):
        with self._lock:
            self._file.close()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def __init__(self, request, client_address, server, origin_host=None):
        # `origin_host` must exist before `handle()` runs, and the base
        # `__init__` runs it, so set it on the instance first.
        self.origin_host = origin_host
        super().__init__(request, client_address, server)

    def log_message(self, fmt, *args):  # noqa: A003
        sys.stderr.write("oauth-mitm: " + (fmt % args) + "\n")

    # -- helpers -----------------------------------------------------------

    @property
    def log(self):
        return self.server.log

    @property
    def tls(self):
        return self.server.tls

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
        self.wfile.flush()
        return status

    def _record(self, status, method, host, path, query, case):
        self.log.write(
            {
                "kind": "request",
                "method": method,
                "host": host,
                "path": path,
                "query": query,
                "case": case,
                "status": status,
            }
        )

    # -- CONNECT -----------------------------------------------------------

    def do_CONNECT(self):  # noqa: N802
        authority = self.path
        host, _, port = authority.rpartition(":")
        port = int(port) if port.isdigit() else 443
        if host not in ALLOWED_HOSTS:
            self.log.write(
                {"kind": "blocked", "method": "CONNECT", "host": host, "path": authority}
            )
            self.send_response(403)
            self.send_header("Content-Length", "0")
            self.end_headers()
            self.close_connection = True
            return
        self.log.write({"kind": "connect", "host": host, "port": port})
        self.send_response(200, "Connection established")
        self.end_headers()
        self.wfile.flush()
        # Hand the socket to a second handler that speaks HTTP over TLS. The
        # outer handler must not read another request: the tunnel owns the
        # connection now.
        try:
            tls_conn = self.tls.wrap_socket(self.connection, server_side=True)
        except (ssl.SSLError, OSError) as error:
            self.log.write({"kind": "tls-error", "host": host, "error": str(error)})
            self.close_connection = True
            return
        inner = Handler(tls_conn, self.client_address, self.server, origin_host=host)
        try:
            inner.handle()
        finally:
            inner.close_connection = True
            self.close_connection = True

    # -- inner requests ----------------------------------------------------

    def _origin(self):
        host = getattr(self, "origin_host", None)
        if host is None:
            split = urlsplit(self.path)
            if split.scheme or split.netloc:
                # An absolute-form request means HTTP_PROXY sent a plain-http
                # URL here, which NO_PROXY was supposed to keep direct.
                host = split.hostname or ""
                self.log.write(
                    {
                        "kind": "blocked",
                        "method": self.command,
                        "host": host,
                        "path": self.path,
                    }
                )
                self._send(502, {"message": "absolute-form request refused"})
                return None
            host = self.headers.get("Host", "")
        return host

    def do_GET(self):  # noqa: N802
        self._handle("GET")

    def do_POST(self):  # noqa: N802
        self._handle("POST")

    def _handle(self, method):
        if getattr(self, "origin_host", None) is None and self.path == "/__health":
            self._send(200, {"ok": True})
            return
        host = self._origin()
        if host is None:
            self.close_connection = True
            return
        split = urlsplit(self.path)
        path = split.path
        query = parse_qs(split.query)
        case = query.get("code", [""])[0]
        status = self._dispatch(method, host, path, query, case)
        self._record(
            status,
            method,
            host,
            path,
            split.query,
            getattr(self, "last_case", "") or case or _case_from_bearer(self.headers),
        )

    def _dispatch(self, method, host, path, query, case):
        self.last_case = case
        if method == "POST":
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length).decode() if length else ""
            form = parse_qs(body)
            case = form["code"][0] if form.get("code") else ""
            self.last_case = case
            if host == "oauth2.googleapis.com" and path == "/token":
                if case == "google-token-error":
                    return self._send(400, {"error": "invalid_grant"})
                if case == "google-token-not-json":
                    return self._send(200, "<html>not json</html>", "text/html")
                return self._send(
                    200,
                    {
                        "access_token": f"access-{case or 'default'}",
                        "token_type": "Bearer",
                        "refresh_token": f"refresh-{case or 'default'}",
                        "expires_in": 7200,
                        "id_token": f"id-{case or 'default'}",
                    },
                )
            if host == "github.com" and path == "/login/oauth/access_token":
                if case == "github-token-error":
                    return self._send(401, {"error": "bad_verification_code"})
                if case == "github-token-not-json":
                    return self._send(200, "<html>not json</html>", "text/html")
                return self._send(
                    200,
                    {
                        "access_token": f"access-{case or 'default'}",
                        "token_type": "bearer",
                        "scope": "read:user,user:email",
                    },
                )
            return self._send(404, {"message": "not found"})

        # GET
        if host == "www.googleapis.com" and path == "/oauth2/v2/userinfo":
            case = _case_from_bearer(self.headers)
            self.last_case = case
            user = google_user(case)
            if user is None:
                return self._send(403, {"error": "forbidden"})
            return self._send(200, user)
        if host == "api.github.com" and path == "/user":
            case = _case_from_bearer(self.headers)
            self.last_case = case
            user = github_user(case)
            if user is None:
                return self._send(500, {"message": "server error"})
            return self._send(200, user)
        if host == "api.github.com" and path == "/user/emails":
            case = _case_from_bearer(self.headers)
            self.last_case = case
            status, payload, content_type = github_emails(case)
            return self._send(status, payload, content_type)
        if host == "api.github.com" and path.startswith("/orgs/"):
            parts = path.split("/")
            # /orgs/<org>/memberships/<login>
            if len(parts) == 5 and parts[3] == "memberships":
                login = parts[4]
                if login == NONMEMBER_LOGIN:
                    return self._send(404, {"message": "Not Found"})
                return self._send(
                    200,
                    {
                        "url": f"https://api.github.com/orgs/{parts[2]}/memberships/{login}",
                        "state": "active",
                        "role": "member",
                    },
                )
            return self._send(404, {"message": "Not Found"})
        return self._send(404, {"message": "not found"})


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def handle_error(self, request, client_address):
        # A client that closes a keep-alive tunnel (or the wrapper stopping the
        # proxy) makes an in-flight `readline` fail; that is not an error worth
        # a traceback. Real failures still print.
        import traceback

        exception = sys.exc_info()[1]
        if isinstance(
            exception,
            (BrokenPipeError, ConnectionResetError, ssl.SSLError, ValueError, OSError),
        ):
            return
        traceback.print_exc()


def generate_ca(ca_dir):
    """Write a fresh CA and one leaf covering every provider host."""
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

    directory = Path(ca_dir)
    directory.mkdir(parents=True, exist_ok=True)
    now = datetime.now(UTC)
    ca_key = ec.generate_private_key(ec.SECP256R1())
    ca_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Plane OAuth MITM CA")])
    ca_cert = (
        x509.CertificateBuilder()
        .subject_name(ca_name)
        .issuer_name(ca_name)
        .public_key(ca_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(hours=1))
        .not_valid_after(now + timedelta(days=7))
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=False,
                key_encipherment=False,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=True,
                crl_sign=True,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
        .sign(ca_key, hashes.SHA256())
    )
    leaf_key = ec.generate_private_key(ec.SECP256R1())
    leaf_cert = (
        x509.CertificateBuilder()
        .subject_name(
            x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "plane-oauth-mitm")])
        )
        .issuer_name(ca_name)
        .public_key(leaf_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(hours=1))
        .not_valid_after(now + timedelta(days=7))
        .add_extension(
            x509.SubjectAlternativeName([x509.DNSName(host) for host in ALLOWED_HOSTS]),
            critical=False,
        )
        .add_extension(
            x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False
        )
        .sign(ca_key, hashes.SHA256())
    )

    def write(name, data, private):
        path = directory / name
        path.write_bytes(data)
        if private:
            os.chmod(path, 0o600)

    write(
        CA_CERT_NAME,
        ca_cert.public_bytes(serialization.Encoding.PEM),
        private=False,
    )
    write(
        CA_KEY_NAME,
        ca_key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        ),
        private=True,
    )
    write(LEAF_CERT_NAME, leaf_cert.public_bytes(serialization.Encoding.PEM), private=False)
    write(
        LEAF_KEY_NAME,
        leaf_key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        ),
        private=True,
    )
    return directory / CA_CERT_NAME, directory / LEAF_CERT_NAME, directory / LEAF_KEY_NAME


def tls_context(leaf_cert, leaf_key):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(certfile=str(leaf_cert), keyfile=str(leaf_key))
    return context


def serve(port, ca_dir, log_path):
    ca_cert, leaf_cert, leaf_key = generate_ca(ca_dir)
    server = Server(("127.0.0.1", port), Handler)
    server.log = Log(log_path)
    server.tls = tls_context(leaf_cert, leaf_key)
    return server, ca_cert


def check(port, ca_dir, log_path):
    """Fetch one Google userinfo body through the proxy, trusting only the CA."""
    import urllib.request

    server, ca_cert = serve(port, ca_dir, log_path)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    proxy = f"http://127.0.0.1:{port}"
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({"https": proxy}),
        urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=str(ca_cert))),
    )
    request = urllib.request.Request(
        "https://www.googleapis.com/oauth2/v2/userinfo",
        headers={"Authorization": "Bearer access-google-signup"},
    )
    with opener.open(request, timeout=5) as response:
        payload = json.loads(response.read())
    print("check:", response.status, payload["id"], payload["email"])
    assert payload["id"] == "google-signup-account", payload
    server.shutdown()
    server.server_close()
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=18401)
    parser.add_argument(
        "--ca-dir",
        default="target/oauth-mitm",
        help="directory the CA and leaf certificate are (re)generated into",
    )
    parser.add_argument(
        "--log",
        default="target/oauth-mitm/requests.jsonl",
        help="JSONL request log, truncated on startup",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="bind, fetch one provider body through the proxy and exit",
    )
    args = parser.parse_args()
    Path(args.log).parent.mkdir(parents=True, exist_ok=True)
    if args.check:
        return check(args.port, args.ca_dir, args.log)
    server, ca_cert = serve(args.port, args.ca_dir, args.log)
    print(
        f"oauth-mitm proxy listening on http://127.0.0.1:{args.port} "
        f"(CA {ca_cert})",
        file=sys.stderr,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.log.close()
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
