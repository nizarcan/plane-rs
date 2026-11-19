# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the golden corpus for `plane-net` from the real Python code.

The oracle is the Django code that is about to be replaced, run on the
repository's own virtualenv (Python 3.12.13 at the time of writing; the task
brief says 3.12.16 -- always re-run this script with the pinned interpreter and
regenerate rather than trusting the recorded tables):

* ``plane/utils/ip_address.py`` -- ``is_blocked_ip``, ``resolve_and_validate``,
  ``validate_url``;
* ``plane/utils/url_security.py`` -- ``_split_target`` and the ``Host`` header
  construction of ``_request_to_ip``;
* ``plane/app/serializers/webhook.py`` -- ``_validate_webhook_url`` including
  the ``WEBHOOK_DISALLOWED_DOMAINS`` / request-host loop-back guard (imported
  with Django configured; no database or network access happens).

DNS is stubbed on the Python side so the corpus is deterministic: every case
carries the address set the resolver returns (or a resolution error), and the
Rust test replays the same table. The stub mimics ``getaddrinfo`` faithfully:

* a numeric literal resolves to itself (``ipaddress.ip_address`` parses it, and
  the textual form is canonicalised the way libc prints it);
* anything else is IDNA-encoded with the ``idna`` codec exactly as CPython's
  ``socket.getaddrinfo`` does (IDNA2003), then looked up in the case's table;
* an ``idna`` failure propagates as ``UnicodeError``, which the Python code
  turns into ``ValueError("Hostname could not be resolved")``.

Usage (from the repository root):

    apps/api/.venv/bin/python apps/server/tools/net/ssrf_corpus.py
    apps/api/.venv/bin/python apps/server/tools/net/ssrf_corpus.py --check

``--check`` regenerates into memory and fails if the committed files differ,
which is what CI runs.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
import random
import socket
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(REPO_ROOT / "apps" / "server" / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_ROOT = _oracle_api_dir()
CORPUS_DIR = REPO_ROOT / "apps" / "server" / "crates" / "plane-net" / "tests" / "golden"

# The import of `plane` registers Celery and constructs a Redis client from
# settings. Nothing connects, but the URL has to parse. Never a real endpoint.
os.environ.setdefault("REDIS_URL", "redis://localhost:56379/0")
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.test")
sys.path.insert(0, str(API_ROOT))

import django  # noqa: E402

django.setup()

import logging  # noqa: E402

# `WebhookSerializer._validate_webhook_url` logs every rejection at WARNING;
# the corpus calls it thousands of times on purpose.
logging.getLogger("plane.app.serializers.webhook").setLevel(logging.CRITICAL)
logging.disable(logging.CRITICAL)

from unittest.mock import MagicMock  # noqa: E402

from django.conf import settings  # noqa: E402

import plane.utils.ip_address as ip_address_module  # noqa: E402
from plane.app.serializers.webhook import WebhookSerializer  # noqa: E402
from plane.utils.ip_address import is_blocked_ip, resolve_and_validate, validate_url  # noqa: E402
from plane.utils.url_security import _split_target  # noqa: E402

# ---------------------------------------------------------------------------
# Stub resolver
# ---------------------------------------------------------------------------

DEFAULT_PUBLIC = ["93.184.216.34"]


class DnsCase:
    """One deterministic answer set for every lookup in a case."""

    def __init__(self, hosts=None, default=None, error=False):
        self.hosts = hosts or {}
        self.default = default
        self.error = error

    def to_json(self):
        # The Rust side represents addresses as `IpAddr` and prints them
        # canonically; a non-canonical table entry would compare unequal for a
        # reason unrelated to the port. Refuse to emit one.
        for key, ips in self.hosts.items():
            for ip in ips:
                assert ip == str(ipaddress.ip_address(ip)), (key, ip)
        payload = {"hosts": self.hosts}
        if self.default is not None:
            for ip in self.default:
                assert ip == str(ipaddress.ip_address(ip)), ip
            payload["default"] = self.default
        if self.error:
            payload["error"] = True
        return payload


_ACTIVE_DNS = DnsCase(default=DEFAULT_PUBLIC)


def _idna(host):
    # CPython's socket module only runs the `idna` codec on *non-ASCII* names;
    # ASCII names go to the resolver unchanged (DNS matching is case-insensitive)
    # and the URL parser has already lowercased everything before a '%' zone.
    if host.isascii():
        return host
    return host.encode("idna").decode("ascii")


def fake_getaddrinfo(host, port=None, family=0, type=0, proto=0, flags=0):
    case = _ACTIVE_DNS
    if case.error:
        raise socket.gaierror(-2, "Name or service not known")
    # Numeric literal: getaddrinfo answers with the address itself.
    try:
        literal = ipaddress.ip_address(host)
    except ValueError:
        literal = None
    if literal is not None:
        ips = [str(literal)]
    else:
        key = _idna(host)
        if key in case.hosts:
            ips = case.hosts[key]
        elif case.default is not None:
            ips = case.default
        else:
            raise socket.gaierror(-2, "Name or service not known")
    out = []
    for ip in ips:
        af = socket.AF_INET6 if ":" in ip else socket.AF_INET
        out.append((af, socket.SOCK_STREAM, 6, "", (ip, 0)))
    return out


ip_address_module.socket.getaddrinfo = fake_getaddrinfo


def set_dns(case):
    global _ACTIVE_DNS
    _ACTIVE_DNS = case


# ---------------------------------------------------------------------------
# Host / address pools
# ---------------------------------------------------------------------------

# Addresses at the boundary of every range that Python 3.12's `ipaddress`
# classifies specially, plus the embedded-IPv4 transition formats. Sampled
# programmatically into network / network+1 / mid / broadcast-1 / broadcast.
INTERESTING_V4_NETWORKS = [
    "0.0.0.0/8", "10.0.0.0/8", "100.64.0.0/10", "127.0.0.0/8", "169.254.0.0/16",
    "172.16.0.0/12", "192.0.0.0/24", "192.0.0.170/31", "192.0.2.0/24",
    "192.88.99.0/24", "192.168.0.0/16", "198.18.0.0/15", "198.51.100.0/24",
    "203.0.113.0/24", "224.0.0.0/4", "240.0.0.0/4", "255.255.255.255/32",
    "1.0.0.0/8", "8.8.8.0/24", "93.184.216.0/24", "151.101.0.0/16",
]

INTERESTING_V6_NETWORKS = [
    "::/128", "::1/128", "::ffff:0:0/96", "64:ff9b::/96", "64:ff9b:1::/48",
    "100::/64", "2001::/32", "2001::/23", "2001:1::1/128", "2001:1::2/128",
    "2001:2::/48", "2001:3::/32", "2001:4:112::/48", "2001:20::/28",
    "2001:30::/28", "2001:db8::/32", "2002::/16", "3fff::/20", "4000::/3",
    "fc00::/7", "fe80::/10", "fec0::/10", "ff00::/8", "2001:4860::/32",
    "2606:4700::/32",
]

# Hand-picked addresses with interesting embedded IPv4 or textual form.
EXTRA_IPS = [
    "8.8.8.8", "8.8.4.4", "1.1.1.1", "9.9.9.9", "93.184.216.34",
    "151.101.1.69", "185.199.108.153", "45.33.32.156", "203.0.113.7",
    "100.64.0.1", "100.127.255.254", "100.128.0.0", "192.0.0.9", "192.0.0.10",
    "192.0.0.8", "192.0.0.11", "169.254.169.254", "169.254.0.1",
    "255.255.255.255", "255.255.255.254", "224.0.0.1", "239.255.255.250",
    "::ffff:127.0.0.1", "::ffff:169.254.169.254", "::ffff:10.0.0.1",
    "::ffff:8.8.8.8", "::ffff:255.255.255.255", "::ffff:100.64.0.1",
    "64:ff9b::7f00:1", "64:ff9b::a9fe:a9fe", "64:ff9b::808:808",
    "64:ff9b::ffff:ffff", "64:ff9b:1::7f00:1", "64:ff9b:1:0100::1",
    "64:ff9b:1::", "2002:7f00:1::", "2002:a00:1::", "2002:c0a8:101::",
    "2002:808:808::", "2002::", "2002:ffff:ffff::",
    "2001:0:4136:e378:8000:63bf:3fff:fdd2",  # Teredo, client 192.0.2.45
    "2001:0:aaaa:bbbb:0:0:808:808",          # Teredo, client 8.8.8.8
    "2001::1", "2001::", "2001:1::1", "2001:1::2", "2001:2::", "2001:3::1",
    "2001:4:112::1", "2001:20::1", "2001:30::1", "2001:db8::1", "2001:db8::",
    "3fff::1", "3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff", "4000::1",
    "100::1", "100::ffff:ffff:ffff:ffff", "fc00::1", "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
    "fe80::1", "fe80::1%eth0", "fe80::1%1", "febf::1", "fec0::1", "feff::1",
    "ff00::1", "ff02::1", "ff02::1:2", "ffff::1", "::", "::1", "::2",
    "2606:4700:4700::1111", "2001:4860:4860::8888", "2a00:1450:4001:80f::200e",
    "::1.2.3.4", "::ffff:0:1.2.3.4", "0:0:0:0:0:ffff:7f00:1",
]


def _sample_network(cidr):
    net = ipaddress.ip_network(cidr)
    first, last = int(net.network_address), int(net.broadcast_address)
    picks = {first, last}
    if last - first >= 1:
        picks.add(first + 1)
        picks.add(last - 1)
    picks.add(first + (last - first) // 2)
    cls = ipaddress.IPv4Address if net.version == 4 else ipaddress.IPv6Address
    return [str(cls(value)) for value in sorted(picks)]


EDGE_IPS = []
for _cidr in INTERESTING_V4_NETWORKS + INTERESTING_V6_NETWORKS:
    EDGE_IPS.extend(_sample_network(_cidr))
EDGE_IPS.extend(EXTRA_IPS)
# De-duplicate while keeping the curated order.
_seen = set()
EDGE_IPS = [ip for ip in EDGE_IPS if not (ip in _seen or _seen.add(ip))]

PUBLIC_IPS = [
    "93.184.216.34", "151.101.1.69", "8.8.8.8", "1.1.1.1", "185.199.108.153",
    "2606:4700:4700::1111", "2001:4860:4860::8888", "2a00:1450:4001:80f::200e",
]

HOSTS = [
    # ordinary public names
    "example.com", "hooks.example.com", "sub.domain.example.co.uk",
    "silo", "silo.namespace.svc.cluster.local", "myplane.example.com",
    "localhost", "localhost.localdomain", "metadata.google.internal",
    # case / trailing dots / whitespace-ish forms
    "Example.COM", "example.com.", "EXAMPLE.com.", ".example.com", "example..com",
    # IDNA forms. CPython's getaddrinfo encodes non-ASCII names with the IDNA2003
    # codec (nameprep + punycode), which maps 'ß' to 'ss' and case-folds. The
    # Rust resolver must reach the same table key for these; see the crate docs
    # for the one documented divergence (names IDNA2003 accepts and UTS46
    # rejects).
    "münchen.example", "bücher.example", "xn--mnchen-3ya.example",
    "例子.example", "xn--fsqu00a.example", "straße.example",
    "STRASSE.example", "faß.de", "fass.de", "x--an.example",
    # userinfo / port / IPv6 literal shapes, used as hosts only where valid
    "user@example.com", "example.com@evil.example",
    # empty-ish
    "", ".", "..", "-", "_", "exa mple.com", "exa\tmple.com",
]

# Textual spellings of addresses: what a resolver may accept, and what the URL
# parser does with them before resolution. Each maps to the canonical address a
# resolver would hand back.
OBFUSCATED_HOSTS = {
    "0x7f.1": "127.0.0.1",
    "0x7f.0.0.1": "127.0.0.1",
    "0X7F.1": "127.0.0.1",
    "127.1": "127.0.0.1",
    "127.0.1": "127.0.0.1",
    "127.0.0.1": "127.0.0.1",
    "0177.0.0.1": "127.0.0.1",
    "017700000001": "127.0.0.1",
    "2130706433": "127.0.0.1",
    "0x7f000001": "127.0.0.1",
    "0x7f.0x0.0x0.0x1": "127.0.0.1",
    "00.00.00.00": "0.0.0.0",
    "0": "0.0.0.0",
    "0x0": "0.0.0.0",
    "4294967295": "255.255.255.255",
    "0xffffffff": "255.255.255.255",
    "0255.0255.0255.0255": "255.255.255.255",
    "1.2.3.4": "1.2.3.4",
    "1.2.3.004": "1.2.3.4",
    "1.2.3.4.5": None,
    "256.1.1.1": None,
    "1.2.3.256": None,
    "1.2.3.-1": None,
    "1.2.3.+4": None,
    "1..2.3": None,
    "1.2.3.": None,
    ".1.2.3": None,
    "1.2.3": None,
}

SCHEMES = [
    "http", "https", "HTTP", "HTTPS", "Http", "ftp", "file", "gopher",
    "javascript", "data", "ws", "wss", "http+unix", "", "1http", "ht~tp",
]

BAD_PORTS = ["0", "1", "80", "443", "8080", "65535", "65536", "99999", "-1", "+80", "abc", "8 0", ""]

ALLOWED_IP_SETS = {
    "none": [],
    "loopback": ["127.0.0.0/8"],
    "rfc1918": ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"],
    "cgnat_and_link": ["100.64.0.0/10", "169.254.0.0/16"],
    "mixed_family": ["192.168.1.0/24", "2001:db8::/32"],
    "mapped": ["::ffff:0:0/96"],
    "everywhere": ["0.0.0.0/0", "::/0"],
    "v6_ula": ["fc00::/7"],
}

ALLOWED_HOST_SETS = {
    "none": [],
    "silo": ["silo"],
    "cluster": ["silo.namespace.svc.cluster.local"],
    "case_dot": ["SILO.Namespace.Svc.Cluster.Local."],
    "public": ["example.com"],
    "subdomain_only": ["internal.example.com"],
}

DISALLOWED_DOMAIN_SETS = {
    "none": [],
    "plane": ["myplane.example.com"],
    "parent": ["example.com"],
    "several": ["example.com", "metadata.google.internal", "silo"],
}


def networks(spec):
    return [ipaddress.ip_network(entry) for entry in spec]


# ---------------------------------------------------------------------------
# Case construction
# ---------------------------------------------------------------------------

URL_CASES = []


def add_case(url, dns=None, allowed_ips="none", allowed_hosts="none",
             disallowed_domains=None, request_host=None, tags=None):
    URL_CASES.append({
        "url": url,
        "dns": dns or DnsCase(default=DEFAULT_PUBLIC),
        "allowed_ips": allowed_ips,
        "allowed_hosts": allowed_hosts,
        "disallowed_domains": disallowed_domains,
        "request_host": request_host,
        "tags": tags or [],
    })


def build_url_cases():
    # (a) every edge address as an IPv4/IPv6 literal host.
    for ip in EDGE_IPS:
        host = f"[{ip}]" if ":" in ip and "%" not in ip else ip
        add_case(f"http://{host}/", tags=["literal"])
        add_case(f"https://{host}:8443/cb", tags=["literal", "port"])

    # (b) the same addresses inside a URL with userinfo, a custom port and a
    # query string: exercises auth extraction and path/query reassembly. The
    # allow-list is varied here rather than in the literal battery above.
    allowlist_cycle = ["none", "loopback", "rfc1918", "mixed_family", "mapped", "everywhere"]
    for index, ip in enumerate(EDGE_IPS):
        host = f"[{ip}]" if ":" in ip and "%" not in ip else ip
        add_case(
            f"https://user:p%40ss@{host}:8443/a/b?x=1&y=2",
            allowed_ips=allowlist_cycle[index % len(allowlist_cycle)],
            tags=["literal", "userinfo", "query"],
        )
        if index % 2 == 0:
            add_case(
                f"http://{host}:8080/p?q=%2f",
                allowed_ips=allowlist_cycle[(index + 3) % len(allowlist_cycle)],
                tags=["literal", "allowlist"],
            )

    # (c) obfuscated / encoded numeric hosts. The resolver answers with the
    # canonical address a libc resolver would produce for that spelling.
    for host, answer in OBFUSCATED_HOSTS.items():
        dns = DnsCase(default=[answer] if answer else None, error=answer is None)
        add_case(f"http://{host}/", dns=dns, tags=["obfuscated"])
        add_case(f"http://{host}:8080/x", dns=dns, tags=["obfuscated", "port"])

    # (d) userinfo tricks.
    for url in [
        "http://evil.com@127.0.0.1/",
        "http://127.0.0.1@evil.com/",
        "http://user:pass@example.com/",
        "http://user:@example.com/",
        "http://:pass@example.com/",
        "http://@example.com/",
        "http://a@b@example.com/",
        "http://example.com@/",
        "http://user%40host@example.com/",
        "http://user:p@ss@example.com/",
        "http://example.com\\@127.0.0.1/",
        "http://example.com%2f@127.0.0.1/",
        "https://user:pass@[::1]:8443/",
    ]:
        add_case(url, tags=["userinfo"])
        add_case(url, allowed_hosts="public", tags=["userinfo", "allowed_host"])

    # (e) hostnames with resolver answers: single public, single private,
    # dual-stack (public+private fails closed), empty answer, resolver error.
    dns_shapes = {
        "public": DnsCase(default=["93.184.216.34"]),
        "public6": DnsCase(default=["2606:4700:4700::1111"]),
        "dual_public": DnsCase(default=["93.184.216.34", "2606:4700:4700::1111"]),
        "private": DnsCase(default=["10.1.2.3"]),
        "private6": DnsCase(default=["fc00::1"]),
        "mixed": DnsCase(default=["93.184.216.34", "127.0.0.1"]),
        "mixed6": DnsCase(default=["2606:4700:4700::1111", "::1"]),
        "empty": DnsCase(default=[]),
        "error": DnsCase(error=True),
        "metadata": DnsCase(default=["169.254.169.254"]),
        "cgnat": DnsCase(default=["100.64.0.1"]),
        "mapped": DnsCase(default=["::ffff:127.0.0.1"]),
        "nat64": DnsCase(default=["64:ff9b::7f00:1"]),
        "six2four": DnsCase(default=["2002:7f00:1::"]),
    }
    for host in HOSTS:
        for shape, dns in dns_shapes.items():
            add_case(f"https://{host}/hook", dns=dns, tags=["hostname", shape])
    # (e2) IDNA: the resolver table is keyed on the IDNA2003 encoding the real
    # getaddrinfo uses, so a different normalisation in the Rust resolver
    # changes the *verdict*, not merely the spelling: 'straße.example' must
    # reach the 'strasse.example' entry (IDNA2003 maps ß -> ss), not a fresh
    # punycode label.
    for host in [
        "münchen.example", "bücher.example", "例子.example", "straße.example",
        "STRASSE.example", "faß.de", "fass.de", "xn--mnchen-3ya.example",
    ]:
        key = host.lower() if host.isascii() else host.encode("idna").decode("ascii")
        for answer in (["10.1.2.3"], ["169.254.169.254"], ["93.184.216.34"]):
            add_case(
                f"https://{host}/hook",
                dns=DnsCase(hosts={key: answer}),
                tags=["idna"],
            )
    for host in ["example.com", "myplane.example.com", "silo"]:
        for shape in ("public", "private", "mixed", "error", "empty"):
            add_case(
                f"http://{host}/hook",
                dns=dns_shapes[shape],
                allowed_hosts="silo" if host == "silo" else "none",
                tags=["hostname", shape, "allowed_hosts"],
            )

    # (f) allow-list hits and misses, IPv4 and IPv6, both families mixed.
    for name, ips in ALLOWED_IP_SETS.items():
        for host_ip in [
            "10.0.0.5", "10.255.255.255", "172.16.0.1", "192.168.1.50",
            "127.0.0.1", "100.64.0.1", "169.254.169.254", "8.8.8.8",
            "::1", "fc00::1", "2001:db8::1", "::ffff:127.0.0.1",
            "64:ff9b::a9fe:a9fe", "2606:4700:4700::1111",
        ]:
            host = f"[{host_ip}]" if ":" in host_ip else host_ip
            add_case(f"http://{host}/x", allowed_ips=name, tags=["allowlist", name])
        for host in ["internal.example.com", "silo", "example.com"]:
            add_case(
                f"http://{host}/x",
                dns=DnsCase(default=["10.1.2.3"]),
                allowed_ips=name,
                tags=["allowlist", name, "hostname"],
            )

    # (f2) bracketed and zoned IPv6 forms. Zoned literals are rejected by
    # `urlsplit` (the zone is not allowed in a bracketed host on 3.12) and
    # unbalanced brackets are a parse error, so these mostly exercise the
    # error messages; a few are valid and must resolve to the literal.
    for host in [
        "[::1]", "[::]", "[fe80::1%25eth0]", "[fe80::1%eth0]", "[fe80::1]",
        "[::ffff:127.0.0.1]", "[::1]:8080", "[::1]:", "[::1]x",
        "[::1", "::1]", "[]", "[[]]", "[::1]]", "[[::1]",
        "[127.0.0.1]", "[1.2.3.4]", "[v1.x]", "[v1]", "[vzz.z]", "[v1.2.3]",
        "[::gggg]", "[12345::1]", "[1:2:3:4:5:6:7:8:9]", "[::1.2.3.4]",
    ]:
        add_case(f"http://{host}/x", tags=["bracket"])
        add_case(f"https://user:pw@{host}/x?q=1", tags=["bracket", "userinfo"])

    # (f3) hostname × port × path matrix: reassembly of path/query and the
    # Host header's default-port elision.
    for host in ["example.com", "hooks.example.com", "münchen.example"]:
        for port in ["", ":80", ":443", ":8080", ":0"]:
            for path in ["", "/", "/x", "/x/y", "/x?y=1", "/x?y=1&z=2", "/?q="]:
                add_case(f"http://{host}{port}{path}", tags=["matrix"])
                add_case(f"https://{host}{port}{path}", tags=["matrix"])

    # (g) scheme and parse failures.
    for scheme in SCHEMES:
        for shape in [
            f"{scheme}://example.com/x",
            f"{scheme}://127.0.0.1/x",
            f"{scheme}://[::1]/x",
            f"{scheme}:/example.com/x",
            f"{scheme}:example.com/x",
        ]:
            add_case(shape, tags=["scheme"])
    for url in [
        "example.com", "example.com/x", "//example.com/x", "///example.com/x",
        "/relative/path", "?q=1", "#frag", "", " ", "\t", "\n",
        " http://example.com/x", "\thttp://example.com/x\n",
        "http://", "http:///x", "http://?", "http://#", "https://:@/x",
        "http://exa mple.com/x", "http://exa\tmple.com/x",
        "http://example.com/x#frag", "http://example.com/x?q=1#frag",
        "http://example.com/x;params?q=1",
        "http://[::1]/x", "http://[::1]:8080/x", "http://[::1]x/",
        "http://[::1", "http://::1]/x", "http://[]/x", "http://[127.0.0.1]/x",
        "http://[::ffff:127.0.0.1]/x", "http://[::zzz]/x", "http://[v1.x]/x",
        "http://[vzz.x]/x", "http://[v1]/x", "http://[[::1]]/x",
        "http://user@[::1]:99999/x", "http://[::1]:abc/x",
        "http://example.com:abc/x", "http://example.com:99999/x",
        "http://example.com:-1/x", "http://example.com:+80/x",
        "http://example.com: 80/x", "http://example.com:80:90/x",
        "http://example.com./x", "http://example.com../x", "http://.example.com/x",
        "http://example.com\\x/", "http://example.com/x\\y",
        "http://münchen.example/x", "http://xn--mnchen-3ya.example/x",
        "http://straße.example/x", "http://faß.de/x",
        "http://example.com/%zz", "http://example.com/%2f%2e%2e/",
        "http://example.com/a%00b", "http://example.com/ä",
        "HTTP://EXAMPLE.COM/Path?Q=1",
        "hTtPs://Example.Com:443/",
        "http://example.com:80/", "https://example.com:443/",
        "https://example.com:80/", "http://example.com:443/",
        "http://0.0.0.0/", "http://0/", "http://0x0/",
        "http://999.999.999.999/", "http://1.2.3.4.5/",
        "http://user:pass@/x", "http://@/x", "http://@:80/x",
        "http://example.com@127.0.0.1:0/",
        "http://example.com:65536/", "http://example.com:0/",
        "http://example.com/?" , "http://example.com/x?",
        "http://example.com/x?", "http://example.com/x??y",
    ]:
        add_case(url, tags=["parse"])
        add_case(url, allowed_hosts="public", tags=["parse", "allowed_host"])

    # (h) ports on a resolvable name.
    for port in BAD_PORTS:
        add_case(f"http://example.com:{port}/x", tags=["port"])
        add_case(f"https://[::1]:{port}/x", tags=["port", "ipv6"])

    # (i) redirect-looking locations (the corpus only validates them; the
    # redirect follower itself is exercised by the live test).
    for url in [
        "https://other.example/page", "//other.example/page", "/relative",
        "../up", "?only=query", "#only-frag", "http://169.254.169.254/latest/",
        "https://example.com/./a/../b", "https://example.com//double//slash",
    ]:
        add_case(url, tags=["redirect-shape"])

    # (j) webhook create/update policy (`_validate_webhook_url`): SSRF check,
    # allowed-host bypass, disallowed domains including the request host and
    # its subdomains.
    # These cases are produced separately below but share the URL list shape.


def build_webhook_cases():
    cases = []
    hosts = [
        "example.com", "hooks.example.com", "sub.hooks.example.com",
        "myplane.example.com", "app.myplane.example.com", "myplane.example.com.",
        "MYPLANE.example.com", "notmyplane.example.com",
        "example.com.evil.example", "evil.example", "silo",
        "metadata.google.internal", "localhost", "127.0.0.1",
        "münchen.example", "xn--mnchen-3ya.example",
    ]
    for host in hosts:
        for disallowed in DISALLOWED_DOMAIN_SETS:
            for allowed_hosts in ("none", "silo", "public"):
                cases.append({
                    "url": f"https://{host}/hook",
                    "dns": DnsCase(default=["93.184.216.34"]),
                    "allowed_ips": "none",
                    "allowed_hosts": allowed_hosts,
                    "disallowed_domains": disallowed,
                    "request_host": "myplane.example.com",
                    "tags": ["webhook", disallowed, allowed_hosts],
                })
    # The request-host loop-back guard with an explicit port, case and trailing
    # dot in `request.get_host()`.
    for request_host in ["myplane.example.com", "MYPLANE.example.com.",
                         "myplane.example.com:18000", "[::1]:8000"]:
        for host in ["myplane.example.com", "app.myplane.example.com", "example.com"]:
            cases.append({
                "url": f"https://{host}/hook",
                "dns": DnsCase(default=["93.184.216.34"]),
                "allowed_ips": "none",
                "allowed_hosts": "none",
                "disallowed_domains": "plane",
                "request_host": request_host,
                "tags": ["webhook", "request_host"],
            })
    # SSRF rejection at create/update time, with and without allow-lists.
    for ip in [
        "127.0.0.1", "10.1.2.3", "169.254.169.254", "100.64.0.1", "0.0.0.0",
        "224.0.0.1", "::1", "::ffff:127.0.0.1", "64:ff9b::a9fe:a9fe",
        "2002:7f00:1::", "8.8.8.8", "93.184.216.34",
    ]:
        host = f"[{ip}]" if ":" in ip else ip
        for allowed_ips in ("none", "loopback", "cgnat_and_link"):
            cases.append({
                "url": f"https://{host}/hook",
                "dns": DnsCase(default=["93.184.216.34"]),
                "allowed_ips": allowed_ips,
                "allowed_hosts": "none",
                "disallowed_domains": "none",
                "request_host": "myplane.example.com",
                "tags": ["webhook", "ssrf", allowed_ips],
            })
    # Hostname shapes: private answer, dual-stack, error, empty.
    for shape, dns in {
        "private": DnsCase(default=["10.1.2.3"]),
        "mixed": DnsCase(default=["93.184.216.34", "127.0.0.1"]),
        "error": DnsCase(error=True),
        "empty": DnsCase(default=[]),
    }.items():
        for host in ["example.com", "silo", "allowed.internal"]:
            for allowed_hosts in ("none", "silo"):
                cases.append({
                    "url": f"https://{host}/hook",
                    "dns": dns,
                    "allowed_ips": "none",
                    "allowed_hosts": allowed_hosts,
                    "disallowed_domains": "parent",
                    "request_host": "myplane.example.com",
                    "tags": ["webhook", shape, allowed_hosts],
                })
    return cases


def build_webhook_only_cases():
    """Extra `_validate_webhook_url` inputs that are not valid URLs at all."""
    cases = []
    for url in [
        "not a url", "ftp://example.com/x", "file:///etc/passwd",
        "http://", "javascript:alert(1)", "http://exa mple.com/",
        "https://example.com:99999/hook",
    ]:
        cases.append({
            "url": url,
            "dns": DnsCase(default=["93.184.216.34"]),
            "allowed_ips": "none",
            "allowed_hosts": "none",
            "disallowed_domains": "none",
            "request_host": "myplane.example.com",
            "tags": ["webhook", "invalid"],
        })
    return cases


def run_url_case(case):
    set_dns(case["dns"])
    expectations = {}

    allowed_ips = networks(ALLOWED_IP_SETS[case["allowed_ips"]])
    allowed_hosts = ALLOWED_HOST_SETS[case["allowed_hosts"]]
    disallowed_domains = DISALLOWED_DOMAIN_SETS[case["disallowed_domains"] or "none"]
    request_host = case["request_host"] or "myplane.example.com"

    # 1. `_split_target`: the pinned request's view of the URL.
    try:
        scheme, hostname, port, path, auth = _split_target(case["url"])
    except ValueError as exc:
        expectations["split"] = {"error": str(exc), "error_type": "ValueError"}
    else:
        expectations["split"] = {
            "ok": {
                "scheme": scheme,
                "hostname": hostname,
                "port": port,
                "path": path,
                "auth": list(auth) if auth is not None else None,
            }
        }

    # 2. `validate_url` at webhook create time.
    try:
        validate_url(
            case["url"],
            allowed_ips=allowed_ips if allowed_ips else [],
            allowed_hosts=allowed_hosts if allowed_hosts else [],
        )
    except ValueError as exc:
        expectations["validate"] = {"error": str(exc), "error_type": "ValueError"}
    else:
        expectations["validate"] = {"ok": None}

    # 3. The pinning path: `_fetch_validated_hop` resolves even for trusted
    # hosts (`require_safe=not trusted`) and returns the addresses to connect
    # to, plus the Host header `_request_to_ip` would send.
    split = expectations["split"]
    if "ok" in split:
        hostname = split["ok"]["hostname"]
        normalized = hostname.rstrip(".").lower()
        trusted = bool(allowed_hosts) and normalized in {
            (entry or "").rstrip(".").lower() for entry in allowed_hosts if entry
        }
        try:
            resolved = resolve_and_validate(
                hostname,
                allowed_ips=allowed_ips if allowed_ips else [],
                require_safe=not trusted,
            )
        except ValueError as exc:
            expectations["resolve"] = {"error": str(exc), "error_type": "ValueError"}
        else:
            default_port = 443 if split["ok"]["scheme"] == "https" else 80
            port = split["ok"]["port"]
            label = f"[{hostname}]" if ":" in hostname else hostname
            expectations["resolve"] = {
                "ok": {
                    "ips": resolved,
                    "trusted": trusted,
                    "host_header": label if port == default_port else f"{label}:{port}",
                }
            }
    else:
        expectations["resolve"] = dict(split)

    # 4. The webhook serializer's create/update guard.
    request = MagicMock()
    request.get_host.return_value = request_host
    serializer = WebhookSerializer(context={"request": request})
    old_disallowed = settings.WEBHOOK_DISALLOWED_DOMAINS
    old_allowed_hosts = settings.WEBHOOK_ALLOWED_HOSTS
    old_allowed_ips = settings.WEBHOOK_ALLOWED_IPS
    settings.WEBHOOK_DISALLOWED_DOMAINS = disallowed_domains
    settings.WEBHOOK_ALLOWED_HOSTS = allowed_hosts
    settings.WEBHOOK_ALLOWED_IPS = allowed_ips
    try:
        serializer._validate_webhook_url(case["url"])
    except Exception as exc:  # serializers.ValidationError
        detail = getattr(exc, "detail", None)
        if isinstance(detail, dict) and "url" in detail:
            value = detail["url"]
            message = str(value[0]) if isinstance(value, list) else str(value)
        else:
            message = str(exc)
        expectations["webhook"] = {"error": message, "error_type": type(exc).__name__}
    else:
        expectations["webhook"] = {"ok": None}
    finally:
        settings.WEBHOOK_DISALLOWED_DOMAINS = old_disallowed
        settings.WEBHOOK_ALLOWED_HOSTS = old_allowed_hosts
        settings.WEBHOOK_ALLOWED_IPS = old_allowed_ips

    resolved = {
        "allowed_ips": [str(network) for network in allowed_ips],
        "allowed_hosts": list(allowed_hosts),
        "disallowed_domains": list(disallowed_domains),
        "request_host": request_host,
    }
    return expectations, resolved


# ---------------------------------------------------------------------------
# IP classification cases
# ---------------------------------------------------------------------------

IP_CASES = []


def build_ip_cases():
    texts = []
    # Systematic sweeps across the octets that decide each boundary, so a table
    # typo (an off-by-one prefix) cannot survive.
    sweeps = []
    sweeps += [f"10.{n}.0.1" for n in range(0, 256, 1)]
    sweeps += [f"127.{n}.0.1" for n in range(0, 256, 1)]
    sweeps += [f"192.168.{n}.1" for n in range(0, 256, 1)]
    sweeps += [f"169.254.{n}.1" for n in range(0, 256, 1)]
    sweeps += [f"100.{n}.0.1" for n in range(60, 132)]
    sweeps += [f"172.{n}.0.1" for n in range(12, 36)]
    sweeps += [f"198.{n}.0.1" for n in range(14, 24)]
    sweeps += [f"224.{n}.0.1" for n in range(0, 256, 16)]
    sweeps += [f"240.{n}.0.1" for n in range(0, 256, 16)]
    sweeps += [f"192.0.0.{n}" for n in range(0, 256, 8)]
    sweeps += [f"203.0.113.{n}" for n in range(0, 256, 32)]
    texts.extend(sweeps)
    for ip in EDGE_IPS:
        texts.append(ip)
        # Alternate textual spellings that must parse to the same address.
        addr = ipaddress.ip_address(ip.split("%")[0])
        if addr.version == 6:
            texts.append(addr.exploded)
            texts.append(str(addr).upper())
            if addr.ipv4_mapped is not None:
                texts.append(f"::FFFF:{addr.ipv4_mapped}")
        else:
            texts.append(str(addr))
            texts.append(f"{int(addr):d}")
    # IPv6 prefix boundaries: the first and last address of every /16 that
    # contains a special range, plus the transition formats.
    texts.extend(
        f"{prefix}::{suffix}"
        for prefix in (
            "2001", "2002", "3ffe", "3fff", "4000", "5f00", "64", "fc00", "fd00",
            "fe80", "febf", "fec0", "feff", "ff00", "ff02", "ffff", "0100",
        )
        for suffix in ("", "1", "ffff")
    )
    # A seeded random sample, so a mistyped prefix in a table that the sweeps
    # above happen not to bisect still fails. Deterministic: same seed, same
    # corpus.
    rng = random.Random(20261116)
    for _ in range(4_000):
        texts.append(str(ipaddress.IPv4Address(rng.getrandbits(32))))
    prefixes = [
        0x0000, 0x0064, 0x0100, 0x2001, 0x2002, 0x3FFF, 0x2606, 0x2A00, 0x4000,
        0x6000, 0x8000, 0xA000, 0xC000, 0xE000, 0xFC00, 0xFE00, 0xFE80, 0xFF00,
    ]
    for _ in range(1_000):
        prefix = rng.choice(prefixes)
        texts.append(str(ipaddress.IPv6Address((prefix << 112) | rng.getrandbits(112))))
    texts.extend([
        "1.2.3", "1.2.3.4.5", "256.1.1.1", "1.2.3.-1", "1.2.3.a", "1..2.3",
        "1.2.3.", ".1.2.3", "", " ", "abc", "0x7f.1", "127.1", "0177.0.0.1",
        "::zzz", "::1::2", "1:2:3:4:5:6:7:8:9", "1::2::3", ":::",
        "::1.2.3", "1.2.3.4::", ":::", ":", "1.2.3.04", "+1.2.3.4",
        "::ffff:127.0.0.1%eth0", "1.2.3.4%eth0", "fe80::1%", "fe80::1%eth0",
        "2001:db8::1%25eth0", "::1/128", "10.0.0.0/8", "1.2.3.4 ",
        " 1.2.3.4", "1.2.3.4\n", "０.０.０.０",
    ])
    seen = set()
    for text in texts:
        if text in seen:
            continue
        seen.add(text)
        try:
            addr = ipaddress.ip_address(text)
        except ValueError as exc:
            IP_CASES.append({
                "input": text,
                "expect": {"error": str(exc), "error_type": "ValueError"},
            })
            continue
        IP_CASES.append({
            "input": text,
            "expect": {
                "ok": {
                    "canonical": str(addr),
                    "version": addr.version,
                    "is_blocked": is_blocked_ip(addr),
                    "is_private": addr.is_private,
                    "is_loopback": addr.is_loopback,
                    "is_reserved": addr.is_reserved,
                    "is_link_local": addr.is_link_local,
                    "is_multicast": addr.is_multicast,
                    "is_unspecified": addr.is_unspecified,
                }
            },
        })


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

def dump_jsonl(path, rows):
    path.parent.mkdir(parents=True, exist_ok=True)
    text = "".join(json.dumps(row, sort_keys=True, ensure_ascii=False) + "\n" for row in rows)
    return text


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true",
                        help="fail if the committed corpus differs from a fresh run")
    parser.add_argument("--out", default=str(CORPUS_DIR))
    args = parser.parse_args(argv)

    build_url_cases()
    build_ip_cases()
    webhook_cases = build_webhook_cases() + build_webhook_only_cases()

    url_rows = []
    for index, case in enumerate(URL_CASES, start=1):
        expectations, resolved = run_url_case(case)
        row = {
            "id": f"u{index:05d}",
            "url": case["url"],
            "dns": case["dns"].to_json(),
            **resolved,
            "tags": case["tags"],
            "expect": expectations,
        }
        url_rows.append(row)
    for index, case in enumerate(webhook_cases, start=len(url_rows) + 1):
        expectations, resolved = run_url_case(case)
        row = {
            "id": f"u{index:05d}",
            "url": case["url"],
            "dns": case["dns"].to_json(),
            **resolved,
            "tags": case["tags"],
            "expect": expectations,
        }
        url_rows.append(row)

    ip_rows = [{"id": f"i{i:05d}", **row} for i, row in enumerate(IP_CASES, start=1)]

    out = Path(args.out)
    url_text = dump_jsonl(out / "ssrf_urls.jsonl", url_rows)
    ip_text = dump_jsonl(out / "ssrf_ips.jsonl", ip_rows)

    if args.check:
        failures = []
        for name, fresh in (("ssrf_urls.jsonl", url_text), ("ssrf_ips.jsonl", ip_text)):
            path = out / name
            if not path.exists():
                failures.append(f"{name}: missing")
            elif path.read_text(encoding="utf-8") != fresh:
                failures.append(f"{name}: stale (re-run this script)")
        if failures:
            print("\n".join(failures), file=sys.stderr)
            return 1
        print(f"corpus fresh: {len(url_rows)} URL cases, {len(ip_rows)} IP cases")
        return 0

    (out / "ssrf_urls.jsonl").write_text(url_text, encoding="utf-8")
    (out / "ssrf_ips.jsonl").write_text(ip_text, encoding="utf-8")
    python_version = ".".join(str(part) for part in sys.version_info[:3])
    print(f"wrote {len(url_rows)} URL cases, {len(ip_rows)} IP cases "
          f"(python {python_version}, {sys.executable})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
