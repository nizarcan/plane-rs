# Decisions

Architecture and dependency decisions for the Rust backend, with the reason for each.
The brief (`RUST_REWRITE.md`) recommends a stack; where this file departs from it, the
reason is performance or memory, and the departure must not change observable behaviour.
Each entry has a status: **decided**, **provisional** (pending a measurement or a spike),
or **owner** (left to the owner, default assumed).

Versions are the latest stable on crates.io on 2026-11-15. Pin minor versions in the
workspace `Cargo.toml` and upgrade deliberately.

## D1. Postgres driver: `tokio-postgres` + `deadpool-postgres` instead of `sqlx` (decided)

- `tokio-postgres` 0.7.18, `deadpool-postgres` 0.14.2, `postgres-types` with
  `with-chrono-0_4`, `with-uuid-1`, `with-serde_json-1`.
- Why: lower per-query and per-row overhead than sqlx, query pipelining on a single
  connection (useful for the many independent count/aggregate queries the Django views
  run), smaller dependency tree and binary. Every read path that matters (issue lists,
  grouped pagination, filters) needs dynamic SQL anyway, which sqlx's compile-time macros
  do not cover.
- What we give up, and how we get it back: sqlx checks queries against the schema at
  build time. Instead, every static SQL statement in `plane-db` is registered in a query
  catalog, and a `schema_check` test prepares each one against a Django-migrated database
  and compares the result column names and types with the Rust row type. The test runs
  in CI against the template database.
- Access result columns by index in hot paths, not by name.
- Amended 2026-11-16: `prepare_cached` only for the static statements in the query
  catalogs. Dynamic SQL (filters, ordering, grouping) uses unnamed statements
  (`Client::prepare` / `query` with SQL text). deadpool's statement cache is unbounded
  per connection and never deallocates: replaying ~800 distinct filter shapes cost about
  0.4 MiB of Postgres backend memory each and got the dev backend OOM-killed. If a
  dynamic shape turns out to be hot, parameterise it into a fixed shape first, then
  cache it.

## D2. JSON output: `serde_json` with a DRF-compatible formatter (decided)

- `serde_json` 1.0.154 with a custom `Formatter` in `plane-core` that reproduces DRF's
  `JSONRenderer`: compact separators, `ensure_ascii=False`, U+2028 and U+2029 written as the escapes `\u2028` and `\u2029`, `allow_nan=False`, and Python `float.__repr__` number formatting (for example
  `1e-05`, `1e+16`, `100.0`).
- Datetimes, dates, decimals and UUIDs are formatted the way DRF fields and DRF's
  `JSONEncoder` format them (for example a UTC `datetime` renders as `isoformat()` with
  `+00:00` replaced by `Z`, and microseconds only when non-zero). Shared formatting
  helpers in `plane-core`; never format a datetime ad hoc in a handler.
- Responses are written straight into a pre-sized `BytesMut`.
- `sonic-rs` is a later option if profiling shows serialisation in the top costs.
- Some Django views use `JsonResponse` (Django's encoder, different separators and
  `ensure_ascii=True`). Those routes use a second formatter. `HTTP_CONTRACT.md` lists
  which routes use which.

## D3. Global allocator: `mimalloc` (provisional)

- `mimalloc` 0.1.52 in every binary. Measure against `tikv-jemallocator` 0.7.0 and the
  system allocator in `BENCH.md` before Phase 8. Lowest idle RSS at equal p95 wins.

## D4. HTTP: `axum` 0.8 + `tower` 0.5 + `tower-http` 0.7 on `hyper` 1 (decided, as the brief)

- Django middleware maps to tower layers in the same order as `MIDDLEWARE` in
  `plane/settings/common.py`. Ordering matters, for example CORS headers on error
  responses and gzip after everything else.
- Response compression: gzip only, with the same thresholds as Django's
  `GZipMiddleware`, so headers match. No brotli or zstd.

## D5. Redis: `redis` 1.8 with a multiplexed `ConnectionManager` (decided)

- `fred` is not maintained (last release 2025-02). `redis` with `tokio-comp` and
  `connection-manager`.
- Throttle and cache keys must be readable and writable by both backends during
  coexistence: django-redis key format (`:<version>:<key>`) and its value serialisation
  (pickle, except integers). Use `serde-pickle` 1.2 for the values only where both
  backends must share them (throttle histories). Rust does not cache responses itself;
  it does invalidate the Django cache keys a Django write would invalidate.

## D6. Object storage: a small SigV4 client (`rusty-s3` + `reqwest`) instead of `aws-sdk-s3` (provisional)

- `aws-sdk-s3` costs build time, binary size and memory for a handful of operations
  (presigned POST and GET, HEAD, DELETE, COPY, GET or PUT of export files).
- `rusty-s3` 0.11 does sans-IO SigV4 signing. If it lacks presigned POST policies, sign
  the POST policy in `plane-storage` (a short SigV4 routine with test vectors from AWS
  documentation and from boto3 output). Must work with MinIO path-style addressing.
- Decide when the asset routes are ported (Phase 5), with a spike and test vectors.

## D7. Celery publishing during coexistence: `lapin` 4.12 behind a cargo feature (decided)

- Only the coexistence build enables the `celery` feature. After cutover it is removed
  and RabbitMQ leaves the deployment.

## D8. Job queue: Postgres `SKIP LOCKED` in schema `plane_jobs`, woken by `LISTEN/NOTIFY` (owner, default assumed)

## D9. Outbound HTTP: `reqwest` 0.13 with rustls (decided)

- SSRF protection as `plane/utils/url_security.py` does it: resolve, validate every
  address, then connect to the validated address (no second DNS lookup), through a
  custom resolver.
- One rustls crypto provider for the whole binary (`aws-lc-rs`, the reqwest default),
  never two.

## D10. Password hashing: `pbkdf2` with SHA-256 via `aws-lc-rs` (provisional)

- Same algorithm, iteration count and salt format as the pinned Django version (verify
  in Django 5.2.17 source). Hash and verify on `spawn_blocking`.

## D11. Time: `chrono` 0.4 + `chrono-tz` (decided)

- Best `postgres-types` integration. Datetime output goes through the D2 helpers.

## D12. Email templates: `minijinja` 3 (decided)

- Templates in `apps/api/templates` are ported to minijinja syntax. Each one gets a
  golden test against Django's rendered output for fixed contexts.

## D13. HTML sanitisation: `ammonia` 4 (provisional)

- Python uses `nh3` 0.2.18, which wraps ammonia. Pin the ammonia version that nh3
  0.2.18 bundles, or prove equivalence on a corpus, and configure it exactly as
  `plane/utils/content_validator.py` and `html_processor.py` do.

## D14. Telemetry: `tracing` + `tracing-subscriber` JSON logs; OpenTelemetry behind a cargo feature (decided)

- Log field names follow the Django JSON log format where operators depend on them.
- The OTLP exporter is compiled in only with the `otel` feature, so deployments that do
  not export traces do not pay for it.

## D15. Testing the contract (decided)

- The differential harness (`plane-testkit`, binary `difftest`) is the primary check.
- JSON is compared after parsing with `serde_json` `preserve_order` and
  `arbitrary_precision` (key order and number lexemes kept), normalising only the fields
  listed in the harness's normalisation table. Each side is also checked to be
  byte-identical to its own re-serialisation through the D2 formatter, so structural
  equality implies byte equality.
- Each target gets its own database (cloned from one template), its own Redis DB index
  and its own RabbitMQ vhost. Enqueued Celery messages are drained and compared as part
  of the side effects.

## Open owner decisions

See "Open decisions for the owner" in `RUST_REWRITE.md`. Defaults are assumed until the
owner says otherwise.
