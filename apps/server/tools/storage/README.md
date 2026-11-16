# plane-storage tools

Python helpers for `crates/plane-storage`. Both run with the Django virtualenv
(`apps/api/.venv`), not the host Python.

## `golden.py` — the presigning oracle

Calls the real `S3Storage` wrapper (`apps/api/plane/settings/storage.py`) with
boto3 1.34.96 under a frozen clock (2026-11-16T12:34:56Z) and writes
`crates/plane-storage/tests/golden/presign.json`: 281 cases covering every
endpoint style, region, bucket-shape and filename shape Plane can produce, plus
the error paths. No network is used.

```sh
cd apps/server
../../apps/api/.venv/bin/python tools/storage/golden.py
CARGO_TARGET_DIR=target/storage cargo test -p plane-storage --test golden
```

`--only TEXT` runs a subset while debugging. The script refuses to run if
`INLINE_SAFE_MIME_TYPES` in `plane/settings/common.py` no longer matches the
literal it configures, and it runs every case twice to catch
nondeterminism.

## `live_cross_check.py` — boto3 vs Rust against MinIO

Requires the dev MinIO (`127.0.0.1:59000`, bucket `uploads`). It:

1. presigns the same POST with boto3 and Rust at the same frozen instant and
   compares URL, fields and policy;
2. uploads through both and compares the stored objects' `Content-Type`,
   `Content-Length`, `ETag` and metadata;
3. compares and exercises a presigned GET with an inline-safe type and a
   non-ASCII filename;
4. compares the export download URL (`export_task.py:66-80`);
5. deletes everything under its `t_storage/cross-*` prefix.

```sh
cd apps/server
CARGO_TARGET_DIR=target/storage ../../apps/api/.venv/bin/python tools/storage/live_cross_check.py
```

The Rust side is driven by `crates/plane-storage/examples/presign.rs`, which
prints the same artefacts as JSON. This is the live companion of the offline
golden test; the crate's own live suite is
`cargo test -p plane-storage --test live_minio` (gated on
`PLANE_RS_TEST_S3_URL`).

## `rusty_s3_spike/`

The D6 decision artefact: rusty-s3 0.11 signing the `get_plain` golden case.
Not a workspace member; see its README for the recorded diff.
