# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the `ipaddress.ip_network(x, strict=False)` corpus for `plane-core`.

`apps/api/plane/settings/common.py:60-69` parses every `WEBHOOK_ALLOWED_IPS`
entry with `ipaddress.ip_network(_cidr, strict=False)` and logs a WARNING for
each entry that raises `ValueError`. `plane-core`'s settings parser has to make
the same accept/reject decision for the same input, including the forms that a
plain CIDR parser does not accept:

* a bare address (`10.0.0.5` -> `/32`, `::1` -> `/128`);
* host bits set (`10.0.0.5/8` -> `10.0.0.0/8`);
* IPv4 netmask (`10.0.0.0/255.0.0.0`) and hostmask (`10.0.0.0/0.255.255.255`)
  notation (IPv6 accepts only a prefix length);
* an IPv6 scope id (`fe80::1%eth0`), which `ipaddress` accepts and an `IpNet`
  cannot represent (membership tests compare the 128-bit value only).

Usage (from the repository root; any CPython 3.12 works, no Django needed):

    apps/api/.venv/bin/python apps/server/tools/net/ip_network_corpus.py
    apps/api/.venv/bin/python apps/server/tools/net/ip_network_corpus.py --check

``--check`` regenerates into memory and fails if the committed file differs.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import random
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
DEFAULT_OUT = (
    REPO_ROOT
    / "apps"
    / "server"
    / "crates"
    / "plane-core"
    / "tests"
    / "golden"
    / "ip_network.jsonl"
)

# Hand-picked cases: every accepted form, every rejected one, and the places
# where CPython is stricter than a plain CIDR parser (leading zeros, signs,
# interior whitespace, mixed netmasks, more than one '/', IPv6 netmasks).
CURATED = [
    # Bare addresses (no prefix supplied).
    "10.0.0.5",
    "127.0.0.1",
    "0.0.0.0",
    "255.255.255.255",
    "::1",
    "::",
    "2001:db8::1",
    "fe80::1",
    # Prefix lengths, including both ends and host bits set.
    "10.0.0.0/8",
    "10.0.0.5/8",
    "192.168.1.7/24",
    "172.16.0.0/12",
    "0.0.0.0/0",
    "255.255.255.255/32",
    "8.8.8.8/31",
    "1.2.3.4/30",
    "1.2.3.4/1",
    "1.2.3.4/08",
    "1.2.3.4/008",
    "1.2.3.4/0000000000000000000008",
    "1.2.3.4/032",
    "1.2.3.4/00",
    "1.2.3.4/33",
    "1.2.3.4/-1",
    "1.2.3.4/+8",
    "1.2.3.4/",
    "1.2.3.4//8",
    "10.0.0.0/255.0.0.0/8",
    "/8",
    # IPv4 netmask and hostmask notation.
    "10.0.0.0/255.0.0.0",
    "10.0.0.0/0.255.255.255",
    "1.2.3.4/255.255.255.0",
    "1.2.3.4/255.255.255.255",
    "1.2.3.4/0.0.0.0",
    "1.2.3.4/0.0.0.1",
    "1.2.3.4/0.0.0.3",
    "1.2.3.4/0.0.0.255",
    "1.2.3.4/0.0.0.254",
    "1.2.3.4/128.0.0.0",
    "1.2.3.4/192.0.0.0",
    "1.2.3.4/255.255.255.1",
    "1.2.3.4/255.0.255.0",
    "1.2.3.4/0.255.0.255",
    "1.2.3.4/255.255.255.256",
    "1.2.3.4/1.2.3.4",
    "1.2.3.4/0xFF",
    # Octet validation (`_parse_octet`, ipaddress.py:1214).
    "1.2.3",
    "1.2.3.4.5",
    "1.2.3.256",
    "1.2.3.1000",
    "1.2.3.04",
    "1.2.3.00",
    "01.2.3.4",
    "000.0.0.0",
    "1.2.3.",
    ".1.2.3",
    "..1.2",
    "1..2.3",
    # Whitespace: the settings loop strips each entry before calling
    # `ip_network`, and CPython itself rejects interior/edge whitespace.
    " 1.2.3.4/8",
    "1.2.3.4/8 ",
    "1.2.3.4/ 8",
    "1.2.3.4\n",
    "1.2.3.4\t/8",
    "10.0.0.0/8\n",
    "\n10.0.0.0/8",
    # IPv6 basics.
    "::/0",
    "::/128",
    "2001:db8::/32",
    "2001:DB8::/32",
    "2001:0db8:0000:0000:0000:0000:0000:0000/32",
    "2001:db8::1/64",
    "fe80::1/10",
    "ff02::1/16",
    "1:2:3:4:5:6:7:8/64",
    "1:2:3:4:5:6:7::/64",
    "::2:3:4:5:6:7:8/64",
    "1::8",
    "1::",
    "::8",
    "2001:db8:0:0:1:0:0:1/128",
    "2001:db8::1/127",
    "2001:db8::/0128",
    "2001:db8::/000000000000000000000000000000128",
    "2001:db8::/129",
    "2001:db8::/-1",
    "2001:db8::/",
    "2001:db8::/ 128",
    "2001:db8:: /64",
    # IPv6 netmask/hostmask notation is NOT accepted (unlike IPv4).
    "2001:db8::/ffff:ffff::",
    "2001:db8::/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
    "2001:db8::/::",
    "2001:db8::/255.255.255.0",
    # Embedded IPv4 suffix and IPv4-mapped addresses.
    "::ffff:10.0.0.0/104",
    "::ffff:10.0.0.5/104",
    "::ffff:1.2.3.4/120",
    "::ffff:1.2.3.4/96",
    "::FFFF:1.2.3.4/128",
    "::ffff:0:0/96",
    "::ffff:0:0/0",
    "::1.2.3.4",
    "::1.2.3.4/128",
    "::1.2.3.4/129",
    "::1.2.3.4/255.255.255.0",
    "64:ff9b::192.0.2.33/96",
    "64:ff9b::/96",
    "1:2:3:4:5:6:1.2.3.4/128",
    "1:2:3:4:5:6:1.2.3.4/64",
    "::1.2.3.256",
    "::01.2.3.4",
    "::1.2.3.4.5",
    "::1.2.3.",
    # Malformed IPv6 structure (`_ip_int_from_string`, ipaddress.py:1652).
    "1:2:3:4:5:6:7:8:9/128",
    "1:2:3:4:5:6:7/128",
    "1::2::3",
    ":::1",
    "1:::2",
    ":1:2:3:4:5:6:7",
    "1:2:3:4:5:6:7:",
    "1:2:3:4:5:6:7:8:",
    "1:2:3:4:5:6:7:8::",
    "::1::",
    "12345::",
    "::gggg",
    "::12345",
    "1:2:3:4:5:6:7:8:9:a:b",
    # Scope ids are accepted on the IPv6 address and dropped by the Rust
    # conversion (an IpNet has no scope field); membership tests ignore them.
    "fe80::1%eth0/64",
    "fe80::%eth0/64",
    "::1%lo",
    "::1%lo/128",
    "::1%eth0/0",
    "%eth0",
    "fe80::1%/64",
    "fe80::1%eth0%x/64",
    "::1/64%eth0",
    "::1%25eth0/128",
    # Non-ASCII digits and characters (CPython checks `isascii()`).
    "１２７.0.0.1",
    "1.2.3.٤",
    "::١",
    "1.2.3.4/١",
    "1.2.3.4/１２",
    "1.2.3.4/①",
    "１::1",
    "::１",
    # Empty and non-IP input.
    "",
    " ",
    "abc",
    "localhost",
    "10.0.0.0/8,192.168.0.0/16",
    "10.0.0.0 /8",
    "10.0.0.0/8 ",
]


def mutated_inputs() -> list[str]:
    """Seeded single/double character mutations of valid forms.

    The structural IPv6 rules (where `::` may appear, how the part count works)
    are the easiest to get subtly wrong, so the inputs are mutations of valid
    addresses instead of characters drawn at random. The seed is fixed, and the
    corpus records the exact result of each input.
    """
    rng = random.Random(20261116)
    bases = [
        "10.0.0.5/8",
        "255.255.255.255/255.255.255.0",
        "1.2.3.4/0.0.15.255",
        "::1/64",
        "2001:db8::abcd/32",
        "::ffff:1.2.3.4/120",
        "fe80::1%eth0/64",
        "1:2:3:4:5:6:7:8",
        "1::2:3:4:5:6:7",
        "::",
        "::1",
        "1.2.3.4",
    ]
    alphabet = "0123456789abcdefABCDEF:./% -_g\t\né١"
    out = []
    for _ in range(400):
        value = list(rng.choice(bases))
        for _ in range(rng.randint(1, 4)):
            action = rng.random()
            if action < 0.4 and value:
                value[rng.randrange(len(value))] = rng.choice(alphabet)
            elif action < 0.7:
                value.insert(rng.randrange(len(value) + 1), rng.choice(alphabet))
            elif value:
                value.pop(rng.randrange(len(value)))
        out.append("".join(value))
    return out


def inputs() -> list[str]:
    """Curated cases, exhaustive prefix sweeps and seeded mutations."""
    values = list(CURATED)
    for prefix in range(0, 34):
        values.append(f"10.1.2.3/{prefix}")
    for prefix in range(0, 131):
        values.append(f"2001:db8::abcd/{prefix}")
    values.extend(mutated_inputs())
    deduplicated = []
    seen = set()
    for value in values:
        if value not in seen:
            seen.add(value)
            deduplicated.append(value)
    return deduplicated


def rows() -> list[dict]:
    out = []
    for index, value in enumerate(inputs(), start=1):
        row = {"id": f"n{index:04d}", "input": value}
        try:
            network = ipaddress.ip_network(value, strict=False)
        except ValueError as exc:
            row["ok"] = False
            # Documentation only; the Rust test asserts accept/reject and the
            # network address, not the exact message.
            row["error"] = str(exc)
        else:
            row["ok"] = True
            row["version"] = network.version
            row["prefix"] = network.prefixlen
            # Unambiguous and formatting-independent: the network address as a
            # lowercase hex integer ("a000000" for 10.0.0.0), compared with the
            # Rust address formatted the same way.
            row["address_hex"] = format(int(network.network_address), "x")
            row["network"] = str(network)
        out.append(row)
    return out


def render() -> str:
    return "".join(
        json.dumps(row, sort_keys=True, ensure_ascii=False) + "\n" for row in rows()
    )


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the committed corpus differs from a fresh run",
    )
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)

    text = render()
    if args.check:
        if not args.out.exists():
            print(f"{args.out}: missing", file=sys.stderr)
            return 1
        if args.out.read_text(encoding="utf-8") != text:
            print(f"{args.out}: stale (re-run this script)", file=sys.stderr)
            return 1
        print(f"corpus fresh: {len(text.splitlines())} cases")
        return 0

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8")
    python_version = ".".join(str(part) for part in sys.version_info[:3])
    print(
        f"wrote {len(text.splitlines())} cases to {args.out} "
        f"(python {python_version}, {sys.executable})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
