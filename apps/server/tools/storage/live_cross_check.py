#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Live cross-check: boto3's presigned artefacts against plane-storage's.

This is the live companion of `cargo test -p plane-storage --test golden` (which
proves byte equality under a frozen clock with no network). Here, against the
dev MinIO:

1. boto3 and Rust presign a POST for the same key, content type and size at the
   same frozen instant; the URLs, field maps and policies must be identical.
2. Both uploads happen through `requests` (multipart form, file last), and the
   two stored objects must have identical `Content-Type`, `Content-Length`,
   `ETag` and user metadata.
3. Both generate a presigned GET for an inline-safe image with a non-ASCII
   filename at the same instant; the URLs must be identical and both must be
   accepted by MinIO, returning the pinned `Content-Type`.
4. The export download URL (`plane/bgtasks/export_task.py:66-80`) is compared
   the same way, then fetched.
5. Everything under the run prefix is deleted.

Usage (from `apps/server`):

    ../../apps/api/.venv/bin/python tools/storage/live_cross_check.py

Environment:

    PLANE_RS_TEST_S3_URL          default http://127.0.0.1:59000
    PLANE_RS_TEST_S3_ACCESS_KEY   default access-key
    PLANE_RS_TEST_S3_SECRET_KEY   default secret-key
    PLANE_RS_TEST_S3_BUCKET       default uploads
    PLANE_RS_TEST_S3_REGION       default us-east-1
    CARGO_TARGET_DIR              default target/storage (relative to apps/server)
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlsplit

import boto3
import requests
from botocore.client import Config
from freezegun import freeze_time

SERVER_DIR = Path(__file__).resolve().parents[2]  # apps/server
URL = os.environ.get("PLANE_RS_TEST_S3_URL", "http://127.0.0.1:59000")
ACCESS_KEY = os.environ.get("PLANE_RS_TEST_S3_ACCESS_KEY", "access-key")
SECRET_KEY = os.environ.get("PLANE_RS_TEST_S3_SECRET_KEY", "secret-key")
BUCKET = os.environ.get("PLANE_RS_TEST_S3_BUCKET", "uploads")
REGION = os.environ.get("PLANE_RS_TEST_S3_REGION", "us-east-1")
CARGO_TARGET_DIR = os.environ.get("CARGO_TARGET_DIR", "target/storage")

CREDS = {
    "aws_access_key_id": ACCESS_KEY,
    "aws_secret_access_key": SECRET_KEY,
}


def boto3_client(**overrides):
    kwargs = dict(
        endpoint_url=URL,
        region_name=REGION,
        config=Config(signature_version="s3v4"),
        **CREDS,
    )
    kwargs.update(overrides)
    return boto3.client("s3", **kwargs)


def rust_env() -> dict:
    env = dict(os.environ)
    env.update(
        {
            "AWS_ACCESS_KEY_ID": ACCESS_KEY,
            "AWS_SECRET_ACCESS_KEY": SECRET_KEY,
            "AWS_S3_BUCKET_NAME": BUCKET,
            "AWS_S3_ENDPOINT_URL": URL,
            "AWS_REGION": REGION,
            "CARGO_TARGET_DIR": CARGO_TARGET_DIR,
            "WEB_URL": URL,
            # `Settings::from_source(Role::Worker)` requires Redis, exactly as
            # every Python role does at import time. Nothing connects to it.
            "REDIS_URL": "redis://127.0.0.1:56379/15",
            "SECRET_KEY": "live-cross-check",
        }
    )
    env.pop("USE_MINIO", None)
    env.pop("MINIO_ENDPOINT_SSL", None)
    env.pop("SIGNED_URL_EXPIRATION", None)
    return env


def build_example() -> Path:
    subprocess.run(
        ["cargo", "build", "--quiet", "--example", "presign", "-p", "plane-storage"],
        cwd=SERVER_DIR,
        env=rust_env(),
        check=True,
    )
    binary = SERVER_DIR / CARGO_TARGET_DIR / "debug" / "examples" / "presign"
    if not binary.exists():
        raise SystemExit(f"example binary not found at {binary}")
    return binary


def run_example(binary: Path, *arguments: str, extra_env: dict | None = None) -> dict:
    env = rust_env()
    if extra_env:
        env.update(extra_env)
    completed = subprocess.run(
        [str(binary), *arguments],
        cwd=SERVER_DIR,
        env=env,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise SystemExit(f"rust example failed ({completed.returncode}):\n{completed.stderr[-2000:]}")
    return json.loads(completed.stdout.strip().splitlines()[-1])


def upload_post(artifact: dict, filename: str, content_type: str, body: bytes) -> None:
    fields = {name: value for name, value in artifact["fields"] if value is not None}
    response = requests.post(
        artifact["url"],
        data=fields,
        files={"file": (filename, body, content_type)},
        timeout=30,
    )
    if response.status_code not in (200, 201, 204):
        raise SystemExit(f"presigned POST to {artifact['url']} failed: {response.status_code} {response.text[:400]}")


def normalise_fields(fields) -> list[list[str | None]]:
    # Rust prints Option<String> as null; Python's None must compare equal.
    return [[name, value] for name, value in fields]


def head(client, key: str) -> dict:
    response = client.head_object(Bucket=BUCKET, Key=key)
    return {
        "ContentType": response.get("ContentType"),
        "ContentLength": response.get("ContentLength"),
        "ETag": response.get("ETag"),
        "Metadata": response.get("Metadata", {}),
    }


def main() -> int:
    binary = build_example()
    client = boto3_client()
    run = uuid.uuid4().hex
    prefix = f"t_storage/cross-{run}"
    python_key = f"{prefix}/python.txt"
    rust_key = f"{prefix}/rust.txt"
    get_key = f"{prefix}/inline image.png"
    body = b"cross-check payload\n" * 10
    body = body + b"the quick brown fox jumps over the lazy dog\n"
    content_type = "text/plain"

    failures: list[str] = []
    created: list[str] = []

    try:
        # Pin one instant so a second boundary cannot make the two URLs differ.
        now = datetime.now(timezone.utc).replace(microsecond=0)
        now_iso = now.strftime("%Y-%m-%dT%H:%M:%SZ")

        print("1. presigned POST, boto3 vs plane-storage")
        with freeze_time(now_iso):
            py_post = client.generate_presigned_post(
                Bucket=BUCKET,
                Key=python_key,
                Fields={"Content-Type": content_type},
                Conditions=[
                    {"bucket": BUCKET},
                    ["content-length-range", 1, len(body)],
                    {"Content-Type": content_type},
                    {"key": python_key},
                ],
                ExpiresIn=300,
            )
            py_post = {
                "url": py_post["url"],
                "fields": [[name, value] for name, value in py_post["fields"].items()],
            }
        rust_post = run_example(
            binary,
            "post",
            python_key,
            content_type,
            str(len(body)),
            "300",
            now_iso,
        )
        if rust_post["url"] != py_post["url"]:
            failures.append(f"POST url differs:\n  python {py_post['url']}\n  rust   {rust_post['url']}")
        else:
            print(f"   url identical: {py_post['url']}")
        if normalise_fields(rust_post["fields"]) != normalise_fields(py_post["fields"]):
            failures.append(f"POST fields differ:\n  python {py_post['fields']}\n  rust   {rust_post['fields']}")
        else:
            print(f"   {len(py_post['fields'])} fields identical, policy {len(rust_post['policy'])} bytes")

        # Upload through both artefacts. The Rust artefact signs the Python key
        # on purpose: the policy is bound to the key, so this also proves the
        # Rust policy is accepted for the key boto3 signed.
        upload_post(py_post, "python.txt", content_type, body)
        created.append(python_key)
        upload_post(rust_post, "python.txt", content_type, body)
        # same key, same bytes: the second upload overwrites; use a second key
        # for the stored-object comparison so both objects exist side by side.
        with freeze_time(now_iso):
            rust_key_post = client.generate_presigned_post(
                Bucket=BUCKET,
                Key=rust_key,
                Fields={"Content-Type": content_type},
                Conditions=[
                    {"bucket": BUCKET},
                    ["content-length-range", 1, len(body)],
                    {"Content-Type": content_type},
                    {"key": rust_key},
                ],
                ExpiresIn=300,
            )
        rust_key_post = {
            "url": rust_key_post["url"],
            "fields": [[name, value] for name, value in rust_key_post["fields"].items()],
        }
        rust_for_rust_key = run_example(
            binary,
            "post",
            rust_key,
            content_type,
            str(len(body)),
            "300",
            now_iso,
        )
        if rust_for_rust_key["fields"] != rust_key_post["fields"]:
            failures.append("POST fields differ for the second key")
        upload_post(rust_for_rust_key, "rust.txt", content_type, body)
        created.append(rust_key)

        print("2. stored objects")
        python_head = head(client, python_key)
        rust_head = head(client, rust_key)
        for field in ("ContentType", "ContentLength", "ETag", "Metadata"):
            if python_head[field] != rust_head[field]:
                failures.append(f"stored {field} differs: python {python_head[field]!r} rust {rust_head[field]!r}")
        if not failures:
            print(f"   identical metadata: {json.dumps(python_head, sort_keys=True)}")

        print("3. presigned GET, boto3 vs plane-storage")
        # The object is text/plain; pinning image/png exercises the inline path
        # (`INLINE_SAFE_MIME_TYPES`) and MinIO returns the overridden type.
        with freeze_time(now_iso):
            py_get = client.generate_presigned_url(
                "get_object",
                Params={
                    "Bucket": BUCKET,
                    "Key": get_key,
                    "ResponseContentType": "image/png",
                    "ResponseContentDisposition": "inline; filename*=UTF-8''%C3%A9%20%E5%90%8D.png",
                },
                ExpiresIn=3600,
            )
        rust_get = run_example(
            binary,
            "get",
            get_key,
            "inline",
            "é 名.png",
            "image/png",
            "3600",
            now_iso,
        )["url"]
        if rust_get != py_get:
            failures.append(f"GET url differs:\n  python {py_get}\n  rust   {rust_get}")
        else:
            print(f"   url identical: {py_get}")

        # Upload the object the GET points at, then fetch with both URLs.
        client.put_object(Bucket=BUCKET, Key=get_key, Body=b"\x89PNG fake", ContentType="image/png")
        created.append(get_key)
        for label, url in (("python", py_get), ("rust", rust_get)):
            response = requests.get(url, timeout=30)
            if response.status_code != 200:
                failures.append(f"{label} GET was rejected: {response.status_code} {response.text[:200]}")
            elif response.headers.get("Content-Type") != "image/png":
                failures.append(f"{label} GET content-type {response.headers.get('Content-Type')!r}")
            elif response.content != b"\x89PNG fake":
                failures.append(f"{label} GET body differs")
            else:
                print(f"   {label} presigned GET accepted by MinIO ({len(response.content)} bytes)")

        print("4. export download URL (custom-domain endpoint)")
        # `plane/settings/common.py:348-352` + `export_task.py:66-80`.
        parsed = urlsplit(URL)
        custom_domain = f"{parsed.netloc}/{BUCKET}"
        export_endpoint = f"{parsed.scheme}://{custom_domain.replace('/uploads', '')}/"
        export_client = boto3.client(
            "s3",
            endpoint_url=export_endpoint,
            config=Config(signature_version="s3v4"),
            **CREDS,
        )
        expires = 7 * 24 * 60 * 60
        with freeze_time(now_iso):
            py_export = export_client.generate_presigned_url(
                "get_object",
                Params={"Bucket": BUCKET, "Key": python_key},
                ExpiresIn=expires,
            )
        rust_export = run_example(
            binary,
            "export-get",
            python_key,
            str(expires),
            now_iso,
            extra_env={"USE_MINIO": "1", "WEB_URL": URL},
        )["url"]
        if rust_export != py_export:
            failures.append(f"export URL differs:\n  python {py_export}\n  rust   {rust_export}")
        else:
            print(f"   url identical: {py_export}")
            response = requests.get(rust_export, timeout=30)
            if response.status_code != 200 or response.content != body:
                failures.append(f"export GET was rejected: {response.status_code}")
            else:
                print("   export URL accepted by MinIO")
    finally:
        if created:
            client.delete_objects(
                Bucket=BUCKET,
                Delete={"Objects": [{"Key": key} for key in created]},
            )
            for key in created:
                try:
                    client.head_object(Bucket=BUCKET, Key=key)
                    failures.append(f"cleanup failed: {key} still exists")
                except client.exceptions.ClientError:
                    pass
            print(f"deleted {len(created)} objects under {prefix}/")

    if failures:
        print("\nFAIL")
        for failure in failures:
            print(f" - {failure}")
        return 1
    print("\nPASS: boto3 and plane-storage produced identical artefacts and stored objects")
    return 0


if __name__ == "__main__":
    sys.exit(main())
