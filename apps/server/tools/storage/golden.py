#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Record boto3's presigned artefacts exactly as Plane configures boto3.

The Rust `plane-storage` crate must emit byte-identical presigned POST policies
and presigned GET URLs. Byte equality is checked by

    cargo test -p plane-storage --test golden

so this script is the oracle: it calls the *real* `S3Storage` wrapper from
`apps/api/plane/settings/storage.py` (not a reimplementation of it) with the
pinned virtualenv's boto3, under a frozen clock, and writes every URL, field
map and policy document into `crates/plane-storage/tests/golden/presign.json`.

Run it from `apps/server` with the Django virtualenv:

    apps/api/.venv/bin/python tools/storage/golden.py

Options:

    --out PATH     write somewhere else (default: the crate's golden file)
    --only TEXT    run only cases whose name contains TEXT (debugging aid)

No network is used: boto3 presigning is local, and every credential the cases
need is set in the environment (the explicit pair, or the env chain when a case
exercises `AWS_SESSION_TOKEN`).

What is recorded, per case:

    kind      "post" | "get" | "error"
    env       the object-storage variables, `null` = unset
    request   `null` for `S3Storage()` (worker tasks) or
              {"scheme": ..., "host": ...} for `S3Storage(request=...)`
    input     the arguments Plane passes to the wrapper
    expected  {"url", "fields"} for POST; {"url"} for GET;
              {"error": "<Python exception class>"} for error cases

`fields` is a list of [name, value] pairs, not an object, because the order is
part of the contract (DRF renders the dict in insertion order).

The frozen instant is 2026-11-16T12:34:56Z, chosen to make the seven-day
expiry land on the same date as the session start of the rewrite.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import sys
import urllib.parse
from pathlib import Path

# Importing `plane` builds a Celery app, which builds a Redis client at import
# time (apps/api/plane/__init__.py -> plane.celery -> plane.settings.redis).
# The client never connects; a syntactically valid URL is enough.
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:56379/15")

import django
from django.conf import settings as django_settings
from freezegun import freeze_time

# The constants `storage.py` reads out of Django settings. They are copied here
# instead of loading `plane.settings.common`, which needs every env var Django
# reads and would abort on the first malformed one. The assertion below fails
# loudly if the source of truth changes.
_INLINE_SAFE = frozenset(["image/jpeg", "image/jpg", "image/png", "image/gif", "image/webp"])
_REPO_ROOT = Path(__file__).resolve().parents[4]
_COMMON_PY = _REPO_ROOT / "apps" / "api" / "plane" / "settings" / "common.py"
_INLINE_LINE = (
    "# The only MIME types ever served inline (S3Storage.generate_presigned_url pins the\n"
    "# type on the response). Everything else, including SVG and HTML, downloads.\n"
    'INLINE_SAFE_MIME_TYPES: frozenset[str] = frozenset(["image/jpeg", "image/jpg", '
    '"image/png", "image/gif", "image/webp"])'
)

django_settings.configure(
    DEBUG=False,
    INLINE_SAFE_MIME_TYPES=_INLINE_SAFE,
    FILE_SIZE_LIMIT=5242880,
    REDIS_SSL=False,
    REDIS_URL=os.environ["REDIS_URL"],
)
django.setup()

# The virtualenv is not installed as a package: `plane` is imported from the
# Django source tree, with the interpreter that owns its dependencies.
sys.path.insert(0, str(_REPO_ROOT / "apps" / "api"))

from plane.settings.storage import S3Storage  # noqa: E402

FROZEN_NOW = "2026-11-16 12:34:56"
FROZEN_NOW_ISO = "2026-11-16T12:34:56Z"

# Every environment variable the wrapper or boto3's credential chain reads in
# these cases. `None` means "unset" and is what the generator writes before
# each case, so one case cannot leak into the next.
STORAGE_ENV = [
    "USE_MINIO",
    "MINIO_ENDPOINT_SSL",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_S3_BUCKET_NAME",
    "AWS_REGION",
    "AWS_DEFAULT_REGION",
    "AWS_S3_ENDPOINT_URL",
    "MINIO_ENDPOINT_URL",
    "SIGNED_URL_EXPIRATION",
    "WEB_URL",
]

ACCESS_KEY = "AKID"
SECRET_KEY = "SECRET"


class FakeRequest:
    """`HttpRequest` from Django, reduced to the two attributes storage.py reads."""

    def __init__(self, scheme: str, host: str):
        self.scheme = scheme
        self._host = host

    def get_host(self) -> str:
        return self._host


def env(**overrides) -> dict:
    """A full storage environment: explicit credentials, MinIO bucket, unset rest."""
    base = {name: None for name in STORAGE_ENV}
    base["AWS_ACCESS_KEY_ID"] = ACCESS_KEY
    base["AWS_SECRET_ACCESS_KEY"] = SECRET_KEY
    base["AWS_S3_BUCKET_NAME"] = "uploads"
    base.update(overrides)
    return base


# ---------------------------------------------------------------------------
# Endpoint configurations
# ---------------------------------------------------------------------------

# MinIO through the request host: the shape every /api/ upload and download
# endpoint produces (storage.py:42-56). `AWS_S3_ENDPOINT_URL` is set to the
# worker endpoint on purpose: it must be ignored when a request is present.
MINIO_API_HTTP = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
    MINIO_ENDPOINT_URL="http://127.0.0.1:59000",
    WEB_URL="http://localhost:3000",
)
MINIO_API_HTTPS = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
    WEB_URL="https://app.plane.example",
)
MINIO_API_SSL_ENV = env(
    USE_MINIO="1",
    MINIO_ENDPOINT_SSL="1",
    AWS_REGION="us-east-1",
    WEB_URL="https://app.plane.example",
)
MINIO_WORKER = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
    WEB_URL="http://localhost:3000",
)
MINIO_WORKER_EMPTY_REGION = env(
    USE_MINIO="1",
    AWS_REGION="",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
)
MINIO_WORKER_NO_REGION = env(
    USE_MINIO="1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
)
MINIO_WORKER_TRAILING_SLASH = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000/",
)
MINIO_WORKER_PATH = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="https://example.com/base",
)
MINIO_WORKER_HTTPS_PORT = env(
    USE_MINIO="1",
    AWS_REGION="eu-central-1",
    AWS_S3_ENDPOINT_URL="https://s3.example.com:9443",
)
MINIO_WORKER_IP = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://10.0.0.5:9000/",
)
MINIO_WORKER_DEFAULT_PORT = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://example.com:80",
)
MINIO_WORKER_PATH_TRAILING = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="https://example.com/base/",
)
MINIO_WORKER_UPPER_ENDPOINT = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://EXAMPLE.com:9000",
)
MINIO_WORKER_IPV6 = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://[::1]:9000",
)

AWS_EAST = env(AWS_REGION="us-east-1")
AWS_EU = env(AWS_REGION="eu-central-1")
AWS_AP = env(AWS_REGION="ap-south-1")
AWS_NO_REGION = env()
AWS_DEFAULT_REGION_ENV = env(AWS_DEFAULT_REGION="eu-west-1")
AWS_CN = env(AWS_REGION="cn-north-1")
AWS_GOV = env(AWS_REGION="us-gov-west-1")

AWS_EU_DOT_BUCKET = env(AWS_REGION="eu-central-1", AWS_S3_BUCKET_NAME="my.bucket")
AWS_EU_UNDERSCORE_BUCKET = env(AWS_REGION="eu-central-1", AWS_S3_BUCKET_NAME="My_Bucket")
AWS_EU_SHORT_BUCKET = env(AWS_REGION="eu-central-1", AWS_S3_BUCKET_NAME="ab")
AWS_EU_UPPER_BUCKET = env(AWS_REGION="eu-central-1", AWS_S3_BUCKET_NAME="MyBucket")
AWS_EAST_DOT_BUCKET = env(AWS_REGION="us-east-1", AWS_S3_BUCKET_NAME="my.bucket")

# `apps/api/plane/bgtasks/export_task.py:50-80`: exports build their own boto3
# client. In the MinIO branch the *presign* client is pointed at
# AWS_S3_URL_PROTOCOL + (AWS_S3_CUSTOM_DOMAIN with the literal "/uploads"
# stripped) + "/", and no region is passed (so the scope is us-east-1). These
# cases say `via: export`; the generator builds the client with that code.
EXPORT_MINIO_DOMAIN = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
    WEB_URL="https://example.com/plane",
)
EXPORT_MINIO_LOCAL = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
    WEB_URL="http://localhost:3000",
)
EXPORT_MINIO_OTHER_BUCKET = env(
    USE_MINIO="1",
    AWS_REGION="us-east-1",
    AWS_S3_BUCKET_NAME="store",
    AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000",
    WEB_URL="https://example.com",
)
EXPORT_NON_MINIO_ENDPOINT = env(
    AWS_REGION="eu-central-1",
    AWS_S3_ENDPOINT_URL="https://s3.example.com",
)
EXPORT_AWS_NO_ENDPOINT = env(AWS_REGION="eu-central-1")


# ---------------------------------------------------------------------------
# Operations
# ---------------------------------------------------------------------------


def post(key, file_type, file_size, expiration=None) -> dict:
    return {
        "kind": "post",
        "input": {
            "key": key,
            "file_type": file_type,
            "file_size": file_size,
            "expiration": expiration,
        },
    }


def get(key, disposition, filename, content_type, expiration=None) -> dict:
    return {
        "kind": "get",
        "input": {
            "key": key,
            "disposition": disposition,
            "filename": filename,
            "content_type": content_type,
            "expiration": expiration,
        },
    }


PLAIN_LIST = []
PLAIN_LIST.append(
    ("post_plain", MINIO_API_HTTP, "http:localhost:18080", post("ws/0f8f-file name.png", "image/png", 5242880))
)
PLAIN_LIST.append(
    (
        "get_plain",
        MINIO_API_HTTP,
        "http:localhost:18080",
        get("ws/0f8f-file name.png", "attachment", "file name.png", "image/png"),
    )
)

# Every endpoint configuration gets one POST and one GET with the same inputs,
# so a URL difference can only come from endpoint resolution.
ENDPOINT_MATRIX = [
    ("minio_api_http", MINIO_API_HTTP, "http:localhost:18080"),
    ("minio_api_https", MINIO_API_HTTPS, "https:plane.example.com"),
    ("minio_api_ssl_env", MINIO_API_SSL_ENV, "http:plane.example.com"),
    ("minio_api_ssl_env_https_req", MINIO_API_SSL_ENV, "https:plane.example.com"),
    ("minio_api_upper_host", MINIO_API_HTTP, "https:Plane.Example.COM"),
    ("minio_worker", MINIO_WORKER, None),
    ("minio_worker_empty_region", MINIO_WORKER_EMPTY_REGION, None),
    ("minio_worker_no_region", MINIO_WORKER_NO_REGION, None),
    ("minio_worker_trailing_slash", MINIO_WORKER_TRAILING_SLASH, None),
    ("minio_worker_path", MINIO_WORKER_PATH, None),
    ("minio_worker_https_port", MINIO_WORKER_HTTPS_PORT, None),
    ("minio_worker_ip", MINIO_WORKER_IP, None),
    ("minio_worker_default_port", MINIO_WORKER_DEFAULT_PORT, None),
    ("minio_worker_path_trailing", MINIO_WORKER_PATH_TRAILING, None),
    ("minio_worker_ipv6", MINIO_WORKER_IPV6, None),
    ("minio_worker_upper_endpoint", MINIO_WORKER_UPPER_ENDPOINT, None),
    ("aws_east", AWS_EAST, "http:localhost:18080"),
    ("aws_eu", AWS_EU, "http:localhost:18080"),
    ("aws_ap", AWS_AP, None),
    ("aws_no_region", AWS_NO_REGION, None),
    ("aws_default_region_env", AWS_DEFAULT_REGION_ENV, None),
    ("aws_cn", AWS_CN, None),
    ("aws_gov", AWS_GOV, None),
    ("aws_eu_dot_bucket", AWS_EU_DOT_BUCKET, None),
    ("aws_eu_underscore_bucket", AWS_EU_UNDERSCORE_BUCKET, None),
    ("aws_eu_short_bucket", AWS_EU_SHORT_BUCKET, None),
    ("aws_eu_upper_bucket", AWS_EU_UPPER_BUCKET, None),
    ("aws_east_dot_bucket", AWS_EAST_DOT_BUCKET, None),
]

# Input variants, each run against a couple of endpoint configurations.
POST_VARIANTS = [
    ("unicode", post("ws/é 名前.png", "application/pdf", 1, 60)),
    ("quotes", post('ws/"quoted" & <x>.png', "text/plain", 10, 300)),
    ("dollar_filename_exact", post("${filename}", "image/jpeg", 5, 60)),
    ("dollar_filename_prefix", post("pre/${filename}", "image/jpeg", 7, 60)),
    ("dollar_filename_suffix", post("${filename}-suffix", "image/jpeg", 9, 60)),
    ("null_type", post("ws/null-type.bin", None, 2, 30)),
    ("expiry_7d", post("ws/expiry.png", "image/webp", 4096, 604800)),
    ("expiry_1", post("ws/expiry-one.png", "image/gif", 4096, 1)),
    ("expiry_zero", post("ws/expiry-zero.png", "image/gif", 4096, 0)),
    ("percent_parens", post("ws/100% (1)+x#y.png", "image/gif", 33, None)),
    ("emoji", post("ws/🎉 party.png", "image/png", 12, None)),
    ("long_key", post("ws/" + "x" * 300 + ".png", "image/png", 10, None)),
    ("empty_key", post("", "image/png", 10, None)),
    ("slash_key", post("a//b/../c/", "image/png", 10, None)),
]

GET_VARIANTS = [
    ("attach_pdf", get("ws/0f8f-report.pdf", "attachment", "report.pdf", "application/pdf")),
    ("inline_png", get("ws/0f8f-pic.png", "inline", "pic name.png", "image/png")),
    ("inline_png_params", get("ws/0f8f-pic.png", "inline", "pic name.png", "IMAGE/PNG ; charset=binary")),
    ("inline_jpeg_upper", get("ws/0f8f-pic.jpg", "inline", "pic.jpg", "image/JPEG")),
    ("inline_svg", get("ws/0f8f-image.svg", "inline", "image.svg", "image/svg+xml")),
    ("inline_no_type", get("ws/0f8f-blob", "inline", "blob", None)),
    ("attachment_with_inline_type", get("ws/0f8f-pic.png", "attachment", "pic name.png", "image/png")),
    ("unknown_disposition", get("ws/0f8f-pic.png", "foo", "pic name.png", "image/png")),
    ("filename_quotes", get("ws/0f8f-file.bin", "attachment", 'my "file"; name.bin', "application/octet-stream")),
    ("filename_unicode", get("ws/0f8f-file.bin", "attachment", "é 名前 100% (1).bin", "application/octet-stream")),
    ("filename_slash", get("ws/0f8f-file.bin", "attachment", "folder/name.bin", "application/octet-stream")),
    ("key_unicode", get("ws/é 名前 #1+2%&=?.png", "attachment", "é 名前 #1+2%&=?.png", "image/png")),
    ("key_dotdot", get("ws/../a//b.png", "attachment", "b.png", "image/png")),
    ("key_emoji", get("ws/🎉 party.png", "attachment", "🎉 party.png", "image/png")),
    ("expiry_7d", get("ws/expiry.png", "attachment", "expiry.png", "image/png", 604800)),
    ("expiry_1", get("ws/expiry-one.png", "attachment", "expiry-one.png", "image/png", 1)),
    ("expiry_negative", get("ws/expiry-neg.png", "attachment", "expiry-neg.png", "image/png", -5)),
    ("key_trailing_slash", get("ws/dir/", "attachment", "dir", "image/png")),
    ("key_double_slash", get("ws//double.png", "attachment", "double.png", "image/png")),
    ("key_percent", get("ws/a%2Fb.png", "attachment", "a%2Fb.png", "image/png")),
]

# Error cases: boto3 raises before or during presigning. Plane does not catch
# any of these (it catches only ClientError, which a local presign cannot
# raise), so each one is a 500 on the Django side.
ERROR_CASES = [
    (
        "error_bucket_unset_post",
        env(AWS_S3_BUCKET_NAME=None, AWS_REGION="us-east-1"),
        None,
        post("x.png", "image/png", 10),
    ),
    (
        "error_bucket_unset_get",
        env(AWS_S3_BUCKET_NAME=None, AWS_REGION="us-east-1"),
        None,
        get("x.png", "attachment", "x.png", "image/png"),
    ),
    (
        "error_bucket_empty_post",
        env(AWS_S3_BUCKET_NAME="", AWS_REGION="us-east-1"),
        None,
        post("x.png", "image/png", 10),
    ),
    (
        "error_bucket_empty_get",
        env(AWS_S3_BUCKET_NAME="", AWS_REGION="us-east-1"),
        None,
        get("x.png", "attachment", "x.png", "image/png"),
    ),
    ("error_empty_key_get", AWS_EAST, None, get("", "attachment", "x.png", "image/png")),
    ("error_empty_region_no_endpoint", env(AWS_REGION=""), None, post("x.png", "image/png", 10)),
    (
        "error_bad_endpoint",
        env(AWS_REGION="us-east-1", AWS_S3_ENDPOINT_URL="localhost:9000"),
        None,
        post("x.png", "image/png", 10),
    ),
    (
        "error_bad_region_underscore",
        env(AWS_REGION="bad_region", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"),
        None,
        post("x.png", "image/png", 10),
    ),
    (
        "error_bad_region_all_digits",
        env(AWS_REGION="123", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"),
        None,
        post("x.png", "image/png", 10),
    ),
    (
        "error_bad_region_leading_dash",
        env(AWS_REGION="-eu", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"),
        None,
        get("x.png", "attachment", "x.png", "image/png"),
    ),
    (
        "error_partial_credentials_post",
        env(AWS_SECRET_ACCESS_KEY=None, AWS_REGION="us-east-1", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"),
        None,
        post("x.png", "image/png", 10),
    ),
    (
        "error_partial_credentials_get",
        env(AWS_SECRET_ACCESS_KEY=None, AWS_REGION="us-east-1", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"),
        None,
        get("x.png", "attachment", "x.png", "image/png"),
    ),
]


def build_cases() -> list[dict]:
    cases: list[dict] = []
    for name, storage_env, request, operation in PLAIN_LIST:
        cases.append({"name": name, "env": storage_env, "request": request, **operation})
    for cfg_name, storage_env, request in ENDPOINT_MATRIX:
        cases.append(
            {
                "name": f"post_endpoint_{cfg_name}",
                "env": storage_env,
                "request": request,
                **post("ws/0f8f-file name.png", "image/png", 5242880),
            }
        )
        cases.append(
            {
                "name": f"get_endpoint_{cfg_name}",
                "env": storage_env,
                "request": request,
                **get("ws/0f8f-report.pdf", "attachment", "report.pdf", "application/pdf"),
            }
        )
    variant_matrix = [
        ("minio_api", MINIO_API_HTTP, "http:localhost:18080"),
        ("minio_worker", MINIO_WORKER, None),
        ("aws_eu", AWS_EU, None),
        ("aws_no_region", AWS_NO_REGION, None),
        ("minio_worker_path", MINIO_WORKER_PATH, None),
        ("minio_worker_https_port", MINIO_WORKER_HTTPS_PORT, None),
    ]
    for cfg_name, storage_env, request in variant_matrix:
        for variant_name, operation in POST_VARIANTS:
            cases.append(
                {
                    "name": f"post_{cfg_name}_{variant_name}",
                    "env": storage_env,
                    "request": request,
                    **operation,
                }
            )
        for variant_name, operation in GET_VARIANTS:
            cases.append(
                {
                    "name": f"get_{cfg_name}_{variant_name}",
                    "env": storage_env,
                    "request": request,
                    **operation,
                }
            )
    # export_task.py:50-80 (`via: export`). The Rust helper derives the same
    # endpoint from WEB_URL/AWS_S3_CUSTOM_DOMAIN; the golden holds the URL boto3
    # produced with that endpoint.
    export_matrix = [
        ("minio_domain", EXPORT_MINIO_DOMAIN),
        ("minio_local", EXPORT_MINIO_LOCAL),
        ("minio_other_bucket", EXPORT_MINIO_OTHER_BUCKET),
        ("non_minio_endpoint", EXPORT_NON_MINIO_ENDPOINT),
        ("aws_no_endpoint", EXPORT_AWS_NO_ENDPOINT),
    ]
    for cfg_name, storage_env in export_matrix:
        cases.append(
            {
                "name": f"export_get_{cfg_name}",
                "env": storage_env,
                "request": None,
                "via": "export",
                **get(
                    "3f9a1c2e-dead-beef-0000-4b1d/export-example-abc123-2026-11-16.zip",
                    "attachment",
                    "ignored.zip",
                    "application/zip",
                    7 * 24 * 60 * 60,
                ),
            }
        )
    # Explicit credentials win over the whole boto3 chain, including the token:
    # storage.py:49-56 never passes aws_session_token, so a deployment with
    # temporary env credentials signs requests without X-Amz-Security-Token.
    # (storage.py:49-56 never passes aws_session_token).
    cases.append(
        {
            "name": "post_explicit_ignores_env_token",
            "env": env(
                AWS_REGION="us-east-1", AWS_SESSION_TOKEN="SESSIONTOKEN", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"
            ),
            "request": None,
            **post("ws/token.png", "image/png", 10),
        }
    )
    cases.append(
        {
            "name": "get_explicit_ignores_env_token",
            "env": env(
                AWS_REGION="us-east-1", AWS_SESSION_TOKEN="SESSIONTOKEN", AWS_S3_ENDPOINT_URL="http://127.0.0.1:59000"
            ),
            "request": None,
            **get("ws/token.png", "attachment", "token.png", "image/png"),
        }
    )
    for name, storage_env, request, operation in ERROR_CASES:
        cases.append({"name": name, "env": storage_env, "request": request, **operation})
    return cases


# boto3 reads these itself (not Plane); point them at a path that cannot exist so
# a developer's profile files cannot change the recorded bytes. Profile support
# is deliberately out of scope (PROGRESS.md open question 3).
BOTO3_ISOLATION = {
    "AWS_CONFIG_FILE": "/nonexistent-plane-storage-golden",
    "AWS_SHARED_CREDENTIALS_FILE": "/nonexistent-plane-storage-golden",
}


def apply_env(storage_env: dict) -> None:
    for name, value in BOTO3_ISOLATION.items():
        os.environ[name] = value
    for name in STORAGE_ENV:
        value = storage_env.get(name)
        if value is None:
            os.environ.pop(name, None)
        else:
            os.environ[name] = value


def _common_storage_settings(storage_env: dict) -> tuple[bool, str]:
    """`plane/settings/common.py:329-352` for the two values export_task uses."""
    use_minio = int(storage_env.get("USE_MINIO") or 0) == 1
    endpoint = storage_env.get("AWS_S3_ENDPOINT_URL") or storage_env.get("MINIO_ENDPOINT_URL")
    if not endpoint or not use_minio:
        return use_minio, ""
    web_url = storage_env.get("WEB_URL")
    if web_url is None:
        web_url = "http://localhost"
    parsed = urllib.parse.urlsplit(web_url)
    return use_minio, f"{parsed.netloc}/{storage_env.get('AWS_S3_BUCKET_NAME', 'uploads')}"


def run_export_presign(storage_env: dict, key: str, expires_in: int) -> str:
    """`upload_to_s3` in apps/api/plane/bgtasks/export_task.py:43-113.

    Only the presign half is used here; the upload half is exercised live.
    """
    import boto3

    use_minio, custom_domain = _common_storage_settings(storage_env)
    bucket = storage_env.get("AWS_S3_BUCKET_NAME", "uploads")
    if use_minio:
        web_url = storage_env.get("WEB_URL")
        if web_url is None:
            web_url = "http://localhost"
        protocol = f"{urllib.parse.urlsplit(web_url).scheme}:"
        presign_s3 = boto3.client(
            "s3",
            endpoint_url=f"{protocol}//{custom_domain.replace('/uploads', '')}/",
            aws_access_key_id=storage_env.get("AWS_ACCESS_KEY_ID"),
            aws_secret_access_key=storage_env.get("AWS_SECRET_ACCESS_KEY"),
            config=boto3.session.Config(signature_version="s3v4"),
        )
    elif storage_env.get("AWS_S3_ENDPOINT_URL"):
        presign_s3 = boto3.client(
            "s3",
            endpoint_url=storage_env["AWS_S3_ENDPOINT_URL"],
            aws_access_key_id=storage_env.get("AWS_ACCESS_KEY_ID"),
            aws_secret_access_key=storage_env.get("AWS_SECRET_ACCESS_KEY"),
            config=boto3.session.Config(signature_version="s3v4"),
        )
    else:
        presign_s3 = boto3.client(
            "s3",
            region_name=storage_env.get("AWS_REGION", ""),
            aws_access_key_id=storage_env.get("AWS_ACCESS_KEY_ID"),
            aws_secret_access_key=storage_env.get("AWS_SECRET_ACCESS_KEY"),
            config=boto3.session.Config(signature_version="s3v4"),
        )
    return presign_s3.generate_presigned_url(
        "get_object",
        Params={"Bucket": bucket, "Key": key},
        ExpiresIn=expires_in,
    )


def run_case(case: dict) -> dict:
    apply_env(case["env"])
    request = case["request"]
    fake = None
    if request is not None:
        scheme, host = request.split(":", 1)
        fake = FakeRequest(scheme, host)
    expected: dict = {}
    try:
        inp = case["input"]
        if case.get("via") == "export":
            expected["url"] = run_export_presign(case["env"], inp["key"], inp["expiration"])
        elif case["kind"] == "post":
            storage = S3Storage(request=fake)
            deadline = inp["expiration"]
            result = storage.generate_presigned_post(
                object_name=inp["key"],
                file_type=inp["file_type"],
                file_size=inp["file_size"],
                expiration=deadline,
            )
            expected["url"] = result["url"]
            expected["fields"] = [[name, value] for name, value in result["fields"].items()]
            expected["policy"] = base64.b64decode(result["fields"]["policy"]).decode("utf-8")
        else:
            storage = S3Storage(request=fake)
            url = storage.generate_presigned_url(
                object_name=inp["key"],
                expiration=inp["expiration"],
                disposition=inp["disposition"],
                filename=inp["filename"],
                content_type=inp["content_type"],
            )
            expected["url"] = url
    except Exception as exc:  # noqa: BLE001 - the exception class is the evidence
        expected = {"error": type(exc).__name__, "message": str(exc)}
    finally:
        for name in STORAGE_ENV:
            os.environ.pop(name, None)
    return {"name": case["name"], **case, "expected": expected}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    default_out = _REPO_ROOT / "apps" / "server" / "crates" / "plane-storage" / "tests" / "golden" / "presign.json"
    parser.add_argument("--out", type=Path, default=default_out)
    parser.add_argument("--only", default=None)
    args = parser.parse_args()

    source = _COMMON_PY.read_text()
    if _INLINE_LINE not in source:
        print(
            "apps/api/plane/settings/common.py changed: INLINE_SAFE_MIME_TYPES is no longer "
            "the literal this generator configures. Update _INLINE_SAFE (and plane-storage).",
            file=sys.stderr,
        )
        return 1

    cases = build_cases()
    if args.only:
        cases = [case for case in cases if args.only in case["name"]]
    if not cases:
        print("no cases selected", file=sys.stderr)
        return 1

    frozen = freeze_time(FROZEN_NOW)
    frozen.start()
    # `_get_content_disposition` generates `uuid.uuid4().hex` when the filename is
    # missing or not a non-empty string. No golden case exercises that path (the
    # URL would change on every run); the Rust unit test covers its shape.
    try:
        recorded = [run_case(case) for case in cases]
    finally:
        frozen.stop()

    # Determinism guard: every case must survive a second pass byte for byte.
    frozen.start()
    try:
        again = [run_case(case) for case in cases]
    finally:
        frozen.stop()
    for first, second in zip(recorded, again):
        if first != second:
            print(f"non-deterministic case: {first['name']}", file=sys.stderr)
            print(json.dumps([first, second], indent=2, ensure_ascii=False), file=sys.stderr)
            return 1

    import boto3
    import botocore

    document = {
        "generated_by": "apps/server/tools/storage/golden.py",
        "frozen_now": FROZEN_NOW_ISO,
        "boto3": boto3.__version__,
        "botocore": botocore.__version__,
        "credentials": {"access_key_id": ACCESS_KEY, "secret_access_key": SECRET_KEY},
        "case_count": len(recorded),
        "cases": recorded,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    successes = sum(1 for case in recorded if "error" not in case["expected"])
    print(f"wrote {len(recorded)} cases ({successes} with output, {len(recorded) - successes} errors) to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
