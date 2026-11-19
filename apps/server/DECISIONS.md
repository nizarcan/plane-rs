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
- 2026-11-16, Phase 3 memory study (`BENCH.md`, "Phase 3: memory after sustained
  load"): no leak. With mimalloc defaults, memory plateaus at ~89 MiB after 1.08M
  requests and stays flat while idle; with `MIMALLOC_PURGE_DELAY=0` it plateaus at
  51 MiB at the same throughput and p95. **Kept mimalloc, with `MIMALLOC_PURGE_DELAY=0`
  as the production image default.** jemalloc and the system allocator weren't measured
  (scope cut, and no measured need). Revisit only if a later phase moves the plateau.

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

## D5a. Page cache (`cache_page`): same behaviour, separate namespace (decided)

- One Plane view uses Django's page cache: `TimezoneEndpoint.get`
  (`plane/app/views/timezone/base.py:23`, `cache_page(60 * 60 * 2)`), which is
  `CacheMiddleware` with `page_timeout=7200`.
- Rust reproduces everything a client can observe: the entry lives for 7200 s,
  a miss carries `Expires`
  (`http_date(now + 7200)`) and `Cache-Control: max-age=7200`
  (`django/utils/cache.py:patch_response_headers`), a hit serves the stored
  status, headers and body unchanged plus
  `Age = max(0, max_age - (expires - now))`
  (`django/middleware/cache.py:FetchFromCacheMiddleware.process_request`),
  `GET`/`HEAD` only, `200` only, and the same store-suppressing guards
  (`private`/`no-cache`/`no-store`, `Vary: *`, a cookie with `Vary: Cookie`).
  Entries are shared across replicas through the same Redis.
- Rust does **not** use Django's keys. Django pickles a
  `rest_framework.response.Response` object under
  `:1:views.decorators.cache.cache_page.<prefix>.<method>.<md5(uri)>.<md5(headers)>.<lang>.<tz>`
  (plus a `cache_header..` registry key); Rust writes
  `:1:plane_rs.cache_page.<method>.<md5(uri)>.<md5(headers)>.<lang>.<tz>` in a
  private binary format (`plane_cache::page`). Routing the endpoint back to
  Django therefore never makes Django `pickle.loads` a Rust-written value, and
  the reverse is impossible because Rust never reads Django's keys.
- The differential harness bridges the two: page-cache keys are matched by
  presence and 10-second TTL bucket only, their values are not compared, and
  Django's `cache_header..` registry key (which has no Rust counterpart) is
  dropped. `Age` is compared with a one-second tolerance (the two targets store
  and observe at their own wall-clock instants); one side sending it and the
  other not is always a difference.

## D6. Object storage: an in-house SigV4 signer on `reqwest`, not `aws-sdk-s3` or `rusty-s3` (decided 2026-11-16)

- Built as `apps/server/crates/plane-storage`. The contract is not "a working S3
  client" but "the exact bytes boto3 1.34.96 produces when
  `plane/settings/storage.py` configures it": the presigned POST URL, the form
  field list and its order, the JSON policy document and its base64 encoding, the
  presigned GET URL (including its unusual parameter order) and the `head_object`
  metadata dict.
- The oracle is `tools/storage/golden.py`: it imports the real `S3Storage`
  wrapper from `apps/api/plane/settings/storage.py`, patches the environment per
  case, freezes `datetime`/`time` at 2026-11-16T12:34:56Z and records every
  artefact. `crates/plane-storage/tests/golden.rs` compares **byte for byte**:
  URL strings, the ordered `(field, value)` list, the base64 policy and the
  decoded policy document. 281 cases (269 with output, 12 error cases) cover
  endpoint styles (AWS global/regional, MinIO via the request host, MinIO worker
  endpoint, custom domain used by exports, http/https, ports, endpoint paths,
  trailing slashes), regions (unset, empty, us-east-1, eu-central-1, ap-south-1,
  cn-north-1, us-gov-west-1, `AWS_DEFAULT_REGION` fallback), bucket names that
  change boto3's addressing style, keys and filenames with spaces, quotes,
  `%`, `+`, `#`, `?`, `&`, `=`, `${filename}`, emoji and CJK, inline-safe and
  attachment MIME types, six expiries, and the error paths. Result: 0 mismatches.
- Why not `rusty-s3` 0.11 (spiked against the same case):
  - It has no presigned POST policy at all (half of Plane's surface).
  - Its `sign()` writes the query string sorted by parameter name. boto3 writes
    the operation parameters in serialization order and then the `X-Amz-*`
    parameters (`botocore/auth.py:702-800`). For Plane's exact inputs the
    signature is identical but the URL bytes are not:
    `...?response-content-disposition=...&X-Amz-Algorithm=...` (boto3) vs
    `...?X-Amz-Algorithm=...&response-content-disposition=...` (rusty-s3).
    Matching would require rewriting the URL after signing and reimplementing
    the POST signer anyway, i.e. two signing implementations rather than one.
  - It requires `jiff` and `url` for functionality already present, and its
    header/path handling is narrower than botocore's.
- Why not `aws-sdk-s3`: the original reasons (build time, binary size, memory)
  stand, and the SDK's presign output is not boto3's either, so the golden test
  would still have to be satisfied by hand.
- Measurements (2026-11-16; macOS arm64; Homebrew rustc 1.99.1; release profile
  `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`, `panic = "abort"`;
  tiny binary that constructs the client and presigns one GET):

  | binary | size | peak RSS | `cargo tree` normal lines |
  | --- | --- | --- | --- |
  | empty (`println!`) | 302,560 B | 1.42 MiB | 1 |
  | `plane-storage` + `reqwest` | 3,619,968 B | 7.27 MiB | 286 |
  | `rusty-s3` 0.11 + `reqwest` | 3,421,056 B | 7.08 MiB | 255 |

  `reqwest` + rustls + aws-lc-rs account for almost all of it; the in-house
  signer is about 0.2 MiB larger than rusty-s3. Peak RSS moved by ±0.2 MiB
  between runs of the *same* binary, so the memory difference is noise. Presign
  cost with cached credentials (50,000 iterations, release; per-op `Instant`):
  GET p50 10.25 µs / p95 12.71 µs / p99 13.17 µs; POST p50 7.75 µs / p95
  9.62 µs; with `Utc::now()` per call, GET p50 10.50 µs. The GET cost includes
  the `_host_from_url` parse (lowercase/default-port) that boto3 performs too.
  Run the benchmark with
  `cargo run --release -p plane-storage --example bench_presign`.
- Transport: `reqwest` 0.13.5, `default-features = false, features = ["rustls"]`
  (aws-lc-rs, satisfying D9). Signing is always `s3v4`; Plane always passes
  `Config(signature_version="s3v4")`, so s3/sigv2 is not implemented. One
  attempt per operation: botocore's legacy retry mode retries up to 5 times.
  This is the only deliberate transport difference; the wrapper's error
  swallowing is reproduced (`head_object`, `copy_object`, `delete_objects`,
  `upload_file` turn an S3 error response into `None`/`false` after logging;
  transport errors propagate). Two settings that Django only reads when the
  first request needs storage (`AWS_REGION`'s validity and
  `int(SIGNED_URL_EXPIRATION)`) are validated when `Storage::new` runs, so
  `plane-api` refuses to start instead of 500ing every upload; that follows the
  ENV.md rule for malformed settings.
- Credentials: the explicit env pair, exactly as `S3Storage` passes it — which
  means `AWS_SESSION_TOKEN` is ignored when `AWS_ACCESS_KEY_ID` and
  `AWS_SECRET_ACCESS_KEY` are set, a real Plane bug reproduced and pinned by the
  golden cases `post/get_explicit_ignores_env_token` — plus the boto3 chain's
  container (`AWS_CONTAINER_CREDENTIALS_*`) and IMDSv2 providers, lazily
  resolved, cached, refreshed 15 minutes before `Expiration`
  (`botocore/credentials.py:61`). Shared credential/config files are not ported
  (PROGRESS open question 3); the env provider of the chain is effectively dead
  for Plane because it reads the same variables `S3Storage` already passes.
  IMDSv1 fallback is not implemented (IMDSv2 is available on every supported
  EC2 instance; an IMDSv1-only endpoint would be a behaviour change).
- Deliberately not reproduced, with reasons:
  - `upload_fileobj`'s 8 MiB multipart threshold: a single `PutObject` produces
    the same bytes, `Content-Type` and ACL, and only the ETag's `-N` suffix
    differs. No caller reads an upload's ETag.
  - bucket ARNs, S3 Access Points and the `us-iso-*` partitions: they require
    `endpoint_url`, which is used verbatim.
  - the shared mutable `extra_args={}` default in `S3Storage.upload_file`
    (`storage.py:190`): the only caller always passes a `ContentType`, so the
    cross-call leak cannot be observed.
- Behaviour that surprised while porting, all pinned by a golden case:
  - Plane calls `minio_endpoint_ssl` only through the request branch: worker
    tasks (`storage_metadata_task`, `copy_s3_object`, exports) ignore it
    (`storage.py:42-56`).
  - With `AWS_REGION=""` boto3 signs an empty credential scope
    (`AKID/20261116//s3/aws4_request`) and MinIO accepts it; with no
    `endpoint_url` the same setting crashes client construction with
    `https://s3..amazonaws.com` (`botocore/endpoint.py:400`).
  - Without `endpoint_url`, every `aws`-partition region uses the **global**
    `s3.amazonaws.com` host for DNS-label buckets; a bucket with `.`, `_`,
    uppercase or length <3 falls back to path style, and the fallback host is
    global or regional depending on whether the bucket is a valid *ruleset* host
    label (`botocore/handlers.py:1129-1162`, `botocore/utils.py:1344`).
  - `bucket` and `key` appear twice in the POST policy: Plane supplies them and
    boto3 appends them again (`storage.py:74-85`, `signers.py:828-838`).
  - Plane checks `object_name.startswith("${filename}")` while boto3 checks
    `key.endswith("${filename}")` (`storage.py:81`, `signers.py:833`), so a key
    that starts with the placeholder but does not end with it gets a truncated
    `starts-with` prefix (`"${filen"` for `"${filename}-suffix"`).
  - The policy document is Python `json.dumps` with default separators and
    `ensure_ascii=True`: `", "`/`": "`, and non-ASCII escaped as lowercase
    `\uXXXX` with surrogate pairs for astral characters.
  - `Content-Disposition` is double-encoded: `quote(filename)` first
    (`storage.py:111`) and then the query serializer escapes the `%` again
    (`botocore/signers.py:653-663`), so a space in a filename appears as
    `%2520`.
  - The export presign endpoint is
    `AWS_S3_URL_PROTOCOL//(<custom domain with the literal "/uploads" removed>)/`
    (`export_task.py:69`): for a bucket named anything but `uploads` the bucket
    appears twice (`https://example.com/store/store/<key>`), and the presign
    client is built without `region_name`, so the scope is us-east-1 even when
    `AWS_REGION` says otherwise.


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
- Implemented by `crates/plane-net` (modules `pyurl`, `ipaddr`, `ssrf`, `dns`, `client`,
  `preset`, `tls`, `webhook`), with `tools/net/ssrf_corpus.py` as the oracle. Versions:
  `reqwest` 0.13.5 (`default-features = false`, features `rustls`, `gzip`, `deflate`),
  `rustls` 0.23.46, `webpki-roots` 1.0.12, `idna` 1.1.0, `ipnet` 2.12.2, `base64` 0.22.1.
  Direct deps rather than workspace entries because the domain agent did not own the
  workspace manifest; hoist them unchanged.
- **Pinning.** Python rewrites the request URL to the validated IP literal and passes the
  name as `Host` plus a urllib3 pool-key `server_hostname`. reqwest has no per-request SNI
  override, so `plane-net` does the equivalent one layer lower: the URL keeps the
  hostname, the client is built with a `reqwest::dns::Resolve` implementation that
  answers exactly one name with exactly the validated `SocketAddr` and **fails every
  other name**, and the `Host` header is set explicitly to Python's value. hyper connects
  to the pinned address, rustls verifies the certificate against the URL host. There is
  no code path that can resolve a second time: `tests/live_https.rs` proves it with a
  flip-flop resolver (one lookup; a rebind to a second address is unreachable). A
  `reqwest::Client` is built per hop, as Python builds a `requests.Session` per hop; the
  TLS config is shared through an `Arc`. Measured marginal cost of the dependency
  (release, macOS arm64, `examples/cost.rs` 3,340,416 B / 7.9 MiB max RSS vs
  `examples/minimal.rs` 302,496 B / 1.4 MiB): **+3.0 MiB binary, +6.5 MiB peak RSS**;
  p50 **0.46 µs** to split the URL, **0.54 µs** to resolve+validate a hop, **0.83 µs** for
  the create-time webhook policy, **0.38 µs** to build a pinned client (20 000 iterations,
  run-to-run variance about 30%). A real binary that already links tokio/rustls for
  `plane-db` pays less than the isolated example.
- **Redirects.** `reqwest`'s redirect policy is always `none()`. The webhook returns a 3xx
  unchanged (`allow_redirects=False`); the link crawler, favicon and avatar paths follow
  at most five hops manually, re-splitting, re-resolving, re-validating and re-pinning
  every hop, exactly like `pinned_fetch_following_redirects`. A `Location` that fails
  validation is a `ValueError`-class failure, not a retryable transport error.
- **Proxies.** Pinned presets use `ClientBuilder::no_proxy()` and never read `HTTP_PROXY`
  (`session.trust_env = False` plus `proxies={"http": None, "https": None}` in Python): a
  CONNECT tunnel would resolve the name at the proxy and bypass pinning. Unguarded call
  sites (OAuth token/userinfo, Unsplash, GitHub releases, live conversion, the LLM call)
  keep env-proxy behaviour, which reqwest reads from `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`
  /`NO_PROXY` with the `system-proxy` feature deliberately **off** (requests reads env
  only, not macOS/Windows system settings).
- **TLS trust.** `webpki-roots` (Mozilla store, the equivalent of the certifi bundle) for
  pinned calls, whose `trust_env=False` also ignores `REQUESTS_CA_BUNDLE`; for unguarded
  calls `REQUESTS_CA_BUNDLE`, then `CURL_CA_BUNDLE`, else `webpki-roots`. The provider is
  constructed explicitly (`aws_lc_rs::default_provider`) and preconfigured `ClientConfig`s
  are shared, so no process default is installed and `plane-db`'s rustls setup is
  unaffected. ALPN is `http/1.1` only (urllib3 2.x), so no h2 negotiation.
- **Timeouts.** `requests`' `timeout=` bounds connect and each read, not the whole
  request. Presets map it to `connect_timeout` + `read_timeout`; the total `timeout()` is
  never set. The link crawler really uses one second (`safe_get`'s default), the favicon
  HEAD two seconds, the avatar ten, webhook delivery and the GitHub release check thirty
  and ten, and the OAuth/Unsplash/live-conversion calls no timeout (the OpenAI SDK call is
  600 s with two retries).
- **Response bodies.** `requests`' text decoding rules: charset parameter, else
  `ISO-8859-1` for a media type containing `text`, else UTF-8 for `application/json`, else
  `apparent_encoding` — which is the literal `"utf-8"` because neither `chardet` nor
  `charset_normalizer` is installed in `apps/api`. Only the avatar path caps the body
  (`DATA_UPLOAD_MAX_MEMORY_SIZE`, 5 MiB by default) because only it reads incrementally;
  webhook delivery and link unfurling read everything, as they do in Python. The knob
  exists per request for a future decision to cap them.
- **Golden corpus.** `tools/net/ssrf_corpus.py` (run with `apps/api/.venv/bin/python`,
  CPython **3.12.13**) produces `crates/plane-net/tests/golden/ssrf_urls.jsonl` (**2 400**
  cases: URL split, `validate_url`, the pinning resolution and `Host` header, and
  `WebhookSerializer._validate_webhook_url`) and `ssrf_ips.jsonl` (**6 835** cases with
  every `ipaddress` property, including a seeded random sample). Both DNS sides are stubbed from the same table. `--check`
  fails on a stale file and belongs in CI. The tables are CPython 3.12's (the pinned
  interpreter), and the corpus records the version that produced it: `ipaddress`'s
  classification and IPv6 scope-id rules have changed between releases, which is why a
  partial re-implementation is not acceptable here. Partial ports of `ipaddress` are
  exactly how SSRF bypasses happen, so the whole property set is golden-tested rather
  than trusted.
- **IDNA.** CPython only IDNA-encodes **non-ASCII** names, with the IDNA2003 `idna` codec
  (`ß` → `ss`). `plane-net` applies UTS #46 transitional mapping and then `idna`'s
  non-transitional algorithm, which agrees on the corpus (including `straße`/`faß`) but is
  not a complete nameprep implementation; a name where the two disagree is a documented
  divergence, not a silent one. `_checknetloc`'s NFKC guard is implemented with the
  19-character table derived from CPython 3.12.13 instead of a normalisation crate.
- **Known differences, all recorded in the modules:**
  - Python's pinning rewrites the URL to the IP, so a non-ASCII `Host` header raises
    `UnicodeEncodeError` there; `plane-net` keeps the name and fails the same request at
    header encoding. Both land in a caller's generic exception branch.
  - Transport error strings differ (hyper/rustls vs urllib3), which changes
    `webhook_logs.response_body` on delivery failures. Unavoidable across HTTP stacks.
  - `str(response.headers)` ordering depends on `http::HeaderMap` iteration order. The
    live test asserts the observed order matches the wire; if that ever changes, the
    webhook log's header string is the only affected column.
  - Unguarded redirect chains do not carry cookies between hops (Python's `Session`
    does), because each hop builds a new client. No Plane call site depends on it.
  - A `Content-Type` parameter without `=` is ignored; Python stores `True` and crashes
    with `AttributeError` if it is `charset`.
- **Follow-up outside this crate:** `plane-core`'s `parse_networks` uses
  `IpNet::from_str`, which rejects a bare address, while `ipaddress.ip_network("10.0.0.5",
  strict=False)` accepts it as `/32`. `WEBHOOK_ALLOWED_IPS=10.0.0.5` therefore warns and is
  ignored by the Rust config today. `SsrfPolicy::parse_cidr_list` in `plane-net` has the
  Python behaviour; the settings parser should use the same rule or the two will disagree.


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

## D16. Output derived from iterating a Python `set` of strings is compared as unordered (decided 2026-11-16)

- Some Django responses contain lists built with `list(some_set)` where the set holds
  `str` values, for example the group id arrays of multi-value grouped issue pages
  (`plane/utils/paginator.py:347,352,579-602`: `result_group_mapping[...].add(str(group_id))`)
  and the `counts` keys of the v1 project summary (`plane/api/views/project.py:733-739`).
  CPython randomises `str` hashing per process (`PYTHONHASHSEED` is not set by the
  production image or entrypoints), so Django's order for these values changes on every
  restart. It is not a contract any client can rely on.
- Rust emits these values in ascending order (stable and cheap) and does not try to
  reproduce one particular seed's order. The differential harness compares exactly these
  fields as multisets, through path-scoped rules (`scenarios/normalize.yaml`,
  `unordered_json_paths`) that still fail on a missing, extra or duplicated element.
  Everything else, including group key order where Django builds it from an ordered dict
  or an **ordered** queryset, stays an exact comparison.
- Three shapes are covered, each named by a JSON-document path (and, for the ones that
  only one endpoint produces, a request glob):
  - `mode: array` — the `label_ids` / `assignee_ids` / `module_ids` arrays of a grouped
    or sub-grouped page, inside a group's (or a sub-group's) `results`;
  - `mode: object_keys` — the group keys of a single m2m-grouped page
    (`paginator.py:355-365`, built by iterating that same set) and the sub-group cell
    keys (`paginator.py:511-518`, whose query has no `ORDER BY`); the key set and every
    value are still compared exactly;
  - `mode: parent_key` — the raw group column of a row that the page copies into every
    group the issue belongs to (`paginator.py:355`), whose value is whichever of the
    issue's rows the database returned first; it is canonicalised to the group the row is
    filed under, because the membership array carries the real data.
- The scenarios keep `PYTHONHASHSEED=0` for Django so Django-vs-Django runs stay
  reproducible; Rust never depends on it.

## D17. Presigned S3 URLs are compared by verifying each signature, not by bytes (decided 2026-11-16)

- Django (boto3) and Rust (`plane-storage`, D6) sign presigned URLs at the current second.
  The two targets of a differential run answer at slightly different times, so a step
  that straddles a second boundary differs in `X-Amz-Date`, and therefore in
  `X-Amz-Signature`. Nothing else in the URL changes. Observed on the asset routes: up to 2
  of 13 presign steps per run, with Rust on the earlier second because it answers faster.
- The harness compares these URLs in a `presigned_url` mode: everything except those two
  query parameters must be byte-identical (scheme, host, path, parameter order and
  encoding, `X-Amz-Expires`, credential scope). It then recomputes each side's SigV4
  signature with the dev stack credentials and that side's own `X-Amz-Date`. The step
  fails if either signature is invalid, if a date falls outside its request's time
  window, or if the dates differ by more than 2 seconds. Masking the signature without
  recomputing it is not allowed, because it would hide signing bugs.
- The orchestrator checked this by hand before the rule existed, with
  `/tmp/orch-scripts/sigv4check.py`: on the asset merge commit both sides' signatures
  were valid for their own dates, one second apart.

## D18. `Content-Length` on 204/304 is not part of the contract (decided 2026-11-16)

- Django (uvicorn) sends `Content-Length: 0` on a 204. hyper omits it, as RFC 9110
  §8.6 requires: a server must not send `Content-Length` on 204, nor on 304 other
  than the representation's length.
- Clients never see Django's header. Plane is served through Caddy (`apps/proxy`, and
  the coexistence proxy in `deploy/caddy`), and Go's `net/http` strips `Content-Length`
  from 204 responses. Checked by the orchestrator: a `caddy:2` `reverse_proxy` in front
  of an upstream that sends `204` plus `Content-Length: 0` returns no `Content-Length`.
- So the harness ignores `Content-Length` only when **both** sides return 204 (or both
  304) and the header is absent on one side and `0` (204) on the other. Any other
  `Content-Length` difference is still a diff. Rust handlers don't add the header back.

## D19. `zxcvbn` is vendored with two documented changes (decided 2026-11-17)

- Plane's `is_password_strong` scores the first and last 128 characters with Python
  `zxcvbn==4.4.28`, which does not truncate. The Rust `zxcvbn` 3.1.1 crate truncates at
  100 characters, and it keeps one-character l33t matches that Python drops
  (`zxcvbn/matching.py:244`).
- `apps/server/vendor/zxcvbn/` is upstream 3.1.1 with exactly those two changes (cap 128,
  `len(token) > 1`), wired in with `[patch.crates-io]`; `VENDORED.md` has the diff.
- Evidence: a corpus of 742 passwords (1484 windows) recorded from Python. Score and
  policy both agree on every case; `tests/zxcvbn_corpus.rs` requires 0 differences.
- Upgrading the crate means re-applying the two changes and re-running the corpus.

## D20. Scope includes Phase 8 (decided by the owner 2026-11-17)

- The work continues past Phase 7 into Phase 8 (cutover): every Celery task and schedule
  owned by Rust, Django and RabbitMQ removed from the deployment, and `plane-migrator`
  owning the schema from a baseline equal to the last Django migration.
- Phase 9 (the live server) is **out of scope**.
- Phase 8 is planned read-only after Phase 7's sets are under way. Until then, tasks stay
  Celery-owned and are published byte-identically.

## D21. The Phase 6 browser walk is shared with the owner (decided 2026-11-17)

- The orchestrator runs the mixed backend and the web/space/admin frontends locally
  behind the dev proxy, and walks the email, magic-link and password flows with the
  in-app browser.
- The owner tests Google and GitLab sign-in with real provider credentials and reports
  back. GitHub and Gitea rely on the differential scenarios (intercept proxy and fake
  provider).
- Amended 2026-11-18: after the orchestrator's walk passed, the owner chose to skip the
  real Google/GitLab sign-in and accepted the OAuth flows as working on the differential
  evidence (`auth-oauth.yaml` 57/57, `auth-oauth-google-github.yaml` 52/52 with every
  provider call intercepted). Phase 6 is closed.

## D22. Disk floor lowered to 20 GB for the build-heavy phases (decided by the owner 2026-11-18)

- Each agent build takes about 3.3 GB with `CARGO_PROFILE_DEV_DEBUG=0` /
  `CARGO_PROFILE_TEST_DEBUG=0` (4.3–5.3 GB with debug info). With about 32 GB free, the
  previous 30 GB floor allowed one build at a time.
- The floor is now **20 GB free** (`df -h /`): up to 3 builds in parallel. No new build is
  launched below 20 GB, and agents are told to build without debug info and to delete
  `target/debug/incremental`.

## D23. Outbound mail: `lettre` 0.11.24 for SMTP, with the message bytes written in-house (decided 2026-11-18)

- The brief names `lettre` (`RUST_REWRITE.md:119`). Only one Plane endpoint
  sends mail from the API process: `EmailCredentialCheckEndpoint`
  (`plane/license/api/views/configuration.py:80-171`). Every other plane e-mail
  (magic links, invitations, notifications) goes through a Celery task and stays
  on Django until Phase 8.
- `lettre` is pinned with `default-features = false, features =
  ["smtp-transport", "tokio1-rustls", "aws-lc-rs", "webpki-roots"]`. The default
  feature set would add `native-tls` (a second TLS stack) and `ring` (a second
  rustls crypto provider, which D9 forbids). `rustls-tls` also forces `ring`, so
  the port uses `tokio1-rustls` + `aws-lc-rs` + `webpki-roots`; both crates are
  already pinned in the workspace (`rustls` 0.23.46, `webpki-roots` 1.0.12).
- The message bytes are written in-house rather than with lettre's `Message`
  builder. The contract is Django's
  `EmailMultiAlternatives.message().as_bytes(linesep="\r\n")` byte for byte —
  header order, RFC 2047 base64 encoded words, Python's 78-column folding and
  the 7bit/8bit/quoted-printable body choice — and lettre's builder formats
  differently. `crates/plane-http-app/src/instances/mail.rs` reproduces
  `django/core/mail/message.py` and `email/header.py`; the corpus at
  `crates/plane-http-app/tests/golden/email_messages.json` is recorded from the
  real Django by `tools/mail/email_message_corpus.py` (18 messages, 7 envelope
  addresses) and compared exactly.
- Connection semantics follow `django.core.mail.backends.smtp.EmailBackend`:
  plaintext, STARTTLS (`EMAIL_USE_TLS=1`) or implicit TLS (`EMAIL_USE_SSL=1`);
  TLS and SSL together are an error Django answers with a 500; authentication
  only when **both** username and password are non-empty; one connection per
  send; no timeout, because Plane leaves `EMAIL_TIMEOUT` unset.
- **Amended 2026-11-18 (merge `e09b5f87ad`): the session is driven command by
  command**, not with `AsyncTransport::send_raw` (which hid which command
  failed). The transport uses lettre's `AsyncNetworkStream` (public, marked
  deprecated; `#[allow(deprecated)]` is scoped to `instances/mail.rs` with the
  reason) and reproduces `smtplib.SMTP`'s dialogue: `ehlo`/`helo` fallback,
  `STARTTLS` then a fresh EHLO, `AUTH` in smtplib's `PLAIN`-then-`LOGIN` order
  with its retry after a 535, `MAIL FROM` with `size=` when SIZE is advertised,
  one `RCPT TO` per recipient, `DATA`, `RSET` after a refused MAIL/RCPT/DATA
  (skipped after a 421, which closes), and `QUIT` from `send_messages`'
  `finally`, but no QUIT when opening failed (`connect`, `STARTTLS`, `AUTH`).
  Each reply therefore maps to Django's own branch: `SMTPSenderRefused`,
  `SMTPRecipientsRefused`, `SMTPDataError`, `SMTPConnectError`,
  `SMTPServerDisconnected`, `SMTPAuthenticationError`, `SMTPHeloError`.
- `socket.getfqdn()` (Django's `DNS_NAME`, used for EHLO/HELO and every
  `Message-ID` domain) is ported: `gethostname` 1.1.0 (already a `plane-jobs`
  dependency) then `getaddrinfo`/`getnameinfo` through **`dns-lookup` 4.0.2**
  (safe wrappers; `libc` was already in the graph), with CPython
  `setipaddr`'s zeroed hints, cached once per process. Known difference: on a
  platform whose canonical name is single-label and whose `/etc/hosts` holds a
  dotted alias, Python returns the alias (`gethostbyaddr`'s alias list has no
  safe binding) and the port returns the canonical name. glibc and macOS
  libinfo otherwise match.
- Remaining differences: CRAM-MD5 is not implemented (a server that offers only
  CRAM-MD5 gets the generic 400 where Python may succeed); TLS trust is
  `webpki-roots`, not the system store (D9).
- `tools/mail/fake_smtp.py` is the harness listener: one JSONL record per
  connection (envelope, message bytes, full transcript including DATA's reply
  and QUIT), with switches for every failure branch the endpoint distinguishes
  (`quit-error` and `delay` included). `tools/mail/run-smtp-scenario.sh` +
  `compare_smtp_logs.py` compare both backends' transcripts, masking only
  `Date:` and the Message-ID local part. The scenario wrapper uses ports 18410/18411;
  `difftest task` binds `127.0.0.1:0` and reads the chosen port from `--port-file`, so
  concurrent harness runs can't deliver into each other's listener (amended with set B).

## D24. E-mail templates: minijinja 3.1.0 over embedded copies of Django's templates (decided 2026-11-19)

- `crates/plane-mail` owns outbound mail for the API and the worker: the D23 message
  bytes and SMTP session (moved from `plane-http-app::instances::mail`, which now
  re-exports it), `generate_plain_text_from_html`, `get_email_configuration`, and
  template rendering.
- Templates are checked in under `crates/plane-mail/templates/` and embedded with
  `include_str!`, so a distroless image needs no files. Only ported templates are
  registered; an unknown name is an explicit error, never a blank e-mail.
- Rendering uses a Django-compatible formatter: `{{ x }}` is Python `str(x)` (`None` →
  `None`, `True` → `True`) then `html.escape(quote=True)`, five characters only (not
  `/`, which minijinja's own escaper would emit as `&#x2f;`).
- Oracle: `tools/mail/email_template_corpus.py` records Django's `render_to_string` +
  `generate_plain_text_from_html`; goldens are compared byte for byte. Each task set
  that ports an e-mail adds its templates and corpus cases.

## D25. Per-task cutover: ownership, producer table, payload equivalence (decided 2026-11-19)

- A task moves to Rust by adding it to `plane_jobs::ownership::RUST_TASKS` and
  `policy::PRODUCERS`, registering its handler in `crates/plane-tasks`, and adding it
  to the testkit's `RUST_OWNED_TASKS` mirror. Never dual-publish.
- `PRODUCERS` is the safety rule: a unit test rejects any Rust-owned task with a
  Python (`Celery`/`CeleryBeat`) producer, so a task only moves once every call site
  that enqueues it is Rust.
- `Job::from_message` is the only queue representation of a call site, so the queue
  payload is provably the Celery `(args, kwargs)`.
  Every producer passes `Job::from_message(&message)`; `Job::new(task)` queues an empty
  payload once the task is Rust-owned (found at 18 call sites in sets A3 and C).
- UUID types survive the queue: `Job::from_message` records which arguments were
  `uuid.UUID` objects (`uuid_args`/`uuid_kwargs`, only when non-empty; values stay
  strings, so handlers see what Python's task sees). A handler that re-publishes a
  still Celery-owned task (`issue_activity` → `notifications`) re-creates kombu's UUID
  envelope from the hint. A UUID-vs-string difference on the wire is never normalised.
- Scenario sweeps drop A's Celery delivery for Rust-owned tasks; handler correctness is
  proven by `difftest task` (A = Django `task.apply()`, B = `plane-worker --once`),
  which compares the task's effects.
- **A handler reproduces every side effect of the Python task, including the tasks it
  publishes**, even when the published task is a no-op for that model: while Celery
  still runs, the message is observable. (P0's `recent_visited_task` skipped the
  `soft_delete_related_objects` publish that `SoftDeleteModel.delete` makes; that is
  corrected in the next task set.)

## D26. `difftest task` compares SQL, S3 and outbound HTTP per task case (decided 2026-11-19)

- A case file (`tools/jobs/fixtures/*.case.json`) lists `sql` queries (columns cast to
  text, compared in order), `s3` seeds and capture, `http` scripted responses, and
  narrow `normalize` regexes.
- S3: two run-owned buckets are created, seeded, read and deleted
  (`plane-testkit/src/s3.rs`, a path-style SigV4 client for the five operations it
  needs; it never touches `uploads`). Every object's key, bytes, `Content-Type` and ACL
  grants are compared, because the exports bucket is public-read.
- HTTP: a recorder on `127.0.0.1:0` stands in for the remote, `{{http}}` is substituted
  into the kwargs, and method, path, query, `User-Agent` and body are compared per side
  in order. No traffic leaves the machine.
- The crawl task's SSRF guard blocks loopback, so loopback is opened only for a
  differential run: `PLANE_DIFFTEST_ALLOW_LOOPBACK=1` patches Django's `is_blocked_ip`
  in `tools/jobs/run_django_task.py`, and the Rust side's switch is compiled only
  with a non-default `difftest` cargo feature, so a release worker can't read it.
- Outgoing messages and jobs a task publishes are compared too (D25).
- A `process_logs` delivery's `log_data.response_body` is the step's response body, so the
  step's response-body rules (D16 key-set objects, D17 presigned URLs) are applied to it
  when it is JSON; every other field, and a non-JSON body, compares exactly.
  Both sides' `(task, args, kwargs)` are compared as Python values (A's kombu UUID
  envelope decoded by the shared `python_value`), and a still Celery-owned delivery is
  additionally compared as its raw wire JSON, `epoch` → `<EPOCH>` (amended with set C
  part 4).
- A UUID absent from that clone's before-snapshot is `<NEW-UUID>` in the diff; a
  fixture UUID stays literal, so writing the wrong existing row is still caught.
- Each table's inserted/deleted/updated rows are compared as multisets after
  normalisation (amended with set A3): both sides mint independent uuid4 primary keys,
  so primary-key order is not an effect.
- SMTP: a case with `"smtp": {}` starts one `tools/mail/fake_smtp.py` per side and points
  that clone's `EMAIL_*` instance configuration at it; envelopes and message bytes are
  compared, masking only `Date:`, the `Message-ID` and the random MIME boundary.
- ZIP objects (and the XLSX inside the export ZIP) are decompressed recursively and
  compared entry by entry (D27).

## D27. The issue-export xlsx is written in-house (`plane-xlsx`) (decided 2026-11-19)

- openpyxl 3.1.2 writes nine ZIP parts in a fixed order (`docProps/app.xml`,
  `docProps/core.xml`, `xl/theme/theme1.xml`, `xl/worksheets/sheet1.xml`,
  `xl/styles.xml`, `_rels/.rels`, `xl/workbook.xml`, `xl/_rels/workbook.xml.rels`,
  `[Content_Types].xml`), with `inlineStr` cells and no `sharedStrings.xml`. No
  crates.io writer (`rust_xlsxwriter`, `umya-spreadsheet`) emits that part set and
  order, so none can be byte-compatible.
- `plane-xlsx` reproduces it with an injected clock. The bytes depend on time twice: each
  ZIP entry's DOS timestamp (`time.localtime()`) and `docProps/core.xml`
  (`datetime.utcnow()`), and `export_task.py:29-36` wraps the workbook in a second ZIP.
  `difftest task` decompresses both sides and normalises only those two timestamp
  families; raw archive bytes are comparable only at a frozen clock.

## D28. Queue provisioning in the harness, transport comparison, and beat handover (decided 2026-11-19)

- `difftest run` (managed mode) and `tools/contract` provision every Rust target's
  database with `plane-migrator` before starting it, like compose's
  `service_completed_successfully` dependency (`plans/CUTOVER.md` §3). Without it, every
  Rust route that publishes a Rust-owned task 500s (found at `90724f9c00`).
- `plane_jobs.*` is removed from the generic DB diff. A Rust-owned task's delivery is
  compared as `(task, args, kwargs)` between A's Celery message (kombu envelope decoded)
  and B's `plane_jobs.jobs` row, with object key order canonicalised (jsonb reorders
  keys). A delivery missing on either side fails the step; nothing is dropped.
- `difftest task` diffs the whole schema, not a per-table list, so every side effect a
  handler owes (D25) is checked. That is how P0's missing `visited_at` bump
  (`auto_now`, `plane/db/models/recent_visit.py:30`) was found.
- Schedules listed in `plane_jobs::beat::RUST_SCHEDULES` are flipped to `owner='rust'`
  at `plane-beat` startup; the quiet-window handover then disables the
  `django_celery_beat_periodictask` row. A unit test fails if a listed entry's task is
  not Rust-owned. Rollback is removing the name from the list and re-enabling the
  Django row.
- The contract plugin replays the `plane_jobs.jobs` rows a Rust request inserted (those
  created after a pre-request `created_at` watermark, each once, never deleted) into a
  patched `.delay` mock through the same `_replay_call` as RabbitMQ messages, so a test asserting `.delay` keeps
  working when its task is Rust-owned. Contract baseline from set C: 984 passed /
  2 failed (the two legacy file-asset POST mocks).
- The beat handover's quiet window is timing-sensitive: `tools/jobs/beat_handover.sh`
  failed three times under several parallel cargo builds and passed 3/3 on a quieter
  host. The cutover runbook hands schedules over on an idle host and checks the first
  Rust ticks.
- The pipeline logs the cause of every 500 it renders at target `plane.exception`
  (no secrets), as Django's `log_exception` does.

## D29. `copy_s3_objects_of_description_and_assets` reproduces `str(BeautifulSoup(html, "html.parser"))` (decided 2026-11-19)

- The task rewrites `image-component@src` and stores the bs4-serialised HTML, so
  `crates/plane-tasks/src/bs4.rs` ports CPython 3.12's `html.parser` (tokenizer incl.
  RAWTEXT/RCDATA and close branches) and bs4 4.12.3's `HTMLParserTreeBuilder` with the
  `"minimal"` formatter: attributes sorted and re-quoted, `class`-like values re-joined,
  void tags as `<br/>`, doctype suffix, whitespace-only text collapsed outside
  `pre`/`textarea`, comments/PIs/CDATA rendered. `strip_tags` is `feed()` only (EOF
  leftovers dropped).
- Oracle: `tools/inventory/bs4_corpus.py` records the cases (editor HTML plus malformed
  input) from the pinned venv, and `bs4_entities.py` the entity table; both have
  `--check`.
- `difftest task` gains `--live-url` (the task's `convert-document` POST) and per-case
  HTTP-body normalisers, and compares S3 listings sorted after normalisation, so a
  fresh uuid key is a one-shot value.

## D30. `track_page_version` is an upstream no-op; `soft_delete_related_objects` moves with it (decided 2026-11-19)

- `page_version_task.py:55,63` read `page.description`, which `Page` doesn't define
  (it has `description_json`). Every changed description raises `AttributeError`, is
  logged, and writes nothing; the create branch, the 600 s update branch, the
  20-version trim and its `soft_delete_related_objects` publish (`:72-74`) are
  unreachable. Verified with the real task (10 `difftest task` cases: no DB effect, no
  message). The Rust handler ports the bug.
- That unreachable line was task 9's only Python producer, so
  `soft_delete_related_objects` is Rust-owned from set A3 on. `TASKS.md` and
  `plans/CUTOVER.md`'s description of task 31 describe the intent, not the behaviour.

## D31. The command-only version backfills live in `plane-admin` (decided 2026-11-19)

- `sync_issue_version` / `sync_issue_description_version` and their `schedule_*` tasks
  (21/22/25/26) have no HTTP, beat or task producer, so they get no `plane_jobs` row and
  no `PRODUCERS` entry. `bins/plane-admin` runs the batch loop in-process:
  `--batch-size` replaces `input()`, `--countdown` is the chain's `eta` delay, each batch
  keeps the task's transaction and ordering, and the loop stops at `end_offset >= total`.
- Proven row for row against the Python command and task chain on cloned fixture DBs
  (`tools/jobs/run_backfill_chain.py`, `compare_backfill.py`). `reset_password`,
  `activate_user` and `test_email` join `plane-admin` later (`plans/CUTOVER.md` §3).
- The Python tasks stay registered in the Celery worker while it exists; no Rust path
  publishes them.

## D32. `notifications` moves with set A2; `difftest task` runs exactly one job (decided 2026-11-19)

- Its only producer is the Rust `issue_activity` handler's `notification=True`
  re-publish (`issue_activities_task.py:1632-1645`), now a Rust→Rust enqueue through
  `dispatch`. The `uuid_kwargs` hint (D25) keeps the one observable type difference:
  `issue_detail.id` is a string, so a `uuid.UUID` `issue_id` (from the automation task)
  never equals it and no per-subscriber activity notification is written
  (`notification_task.py:323`). That branch is proven by a unit test and a manual
  two-database comparison; a chained `difftest task` case (run `issue_activity`, then
  the job it queues) is a follow-up.
- The handler ports the Python body and its bugs (mention extraction with bs4, the
  wrong e-mail receiver at `:576`/`:625`, `actor` always `"None"`, the leaked-variable
  `UnboundLocalError` at `:559-567`, preference `get()` aborts, no transaction so
  partial effects commit).
- `difftest task` runs `plane-worker --once` with `PLANE_RS_JOB_ONCE_JOBS=1`: only the
  harness-inserted job executes, so a Rust-owned task the handler publishes stays a
  `plane_jobs` row and is compared as `(task, args, kwargs)` (D28). A default `--once`
  still drains everything.

## D33. Webhook delivery: one unavoidable difference in `WebhookLog.response_headers` (decided 2026-11-19)

- `crates/plane-tasks/src/webhook` ports `webhook_task` (model activity, fan-out,
  `get_model_data` per entity, the `requests` JSON body and HMAC `X-Plane-Signature`,
  a `WebhookLog` row per attempt with Python `repr()` columns, retry 0–600 s × 5,
  deactivation and its e-mail). Retries are proven attempt by attempt: `difftest task`
  runs one attempt through Celery's own tracer (`request.retries = n - 1`) and the
  matching job attempt on the Rust side.
- `webhook_logs.response_headers` is `str(requests.headers)`, whose keys keep the
  remote's wire spelling. The Rust client can't recover it: `http::HeaderName` is
  lower-case by construction, reqwest 0.13.5 has no response-side case preservation,
  and hyper 1.12.0's `preserve_header_case` stores a `pub(crate)` `HeaderCaseMap` that
  can't be read. Recovering it would need a second HTTP/1 parser, outside D9. Rust
  writes canonical title case, so the column differs only for non-canonical spellings
  (nginx `ETag`, Cloudflare `cf-*`, custom lower-case names); values, order, merging and
  every decision the task makes are identical. Also: `response_body` for a transport
  error is reqwest's `Display` text, not `requests`' message (D9's accepted error-text
  difference).

## D34. `plane-migrator` owns the Django baseline; the boot steps move into `plane-bootstrap` (decided 2026-11-19)

- `deploy/schema/baseline.sql` is a `pg_dump --schema-only --no-owner --no-privileges
  --schema=public` of a database Django migrated at the pin (`db:0125_…`,
  `license:0006_…`), with `django_migrations.txt` (167 rows).
  `tools/schema/baseline.py --check` re-migrates and compares; CI runs it.
- Fresh install: apply the dump and the rows in one transaction (only after proving
  `public` empty), then `plane_jobs::migrate` and `meta('django_baseline', …)`. Upgrade:
  `django_migrations` must equal the baseline exactly; missing rows → refuse and name
  `apps/api/bin/docker-entrypoint-migrator.sh`; extra rows → refuse as a downgrade; a
  non-empty `public` without the table → refuse. One advisory lock; reruns are
  idempotent. Proven by `tools/schema/verify_migrator.py` (dump, columns, indexes,
  constraints, sequences identical; upgrade; every refusal).
- `crates/plane-bootstrap` holds `wait_for_db`, `wait_for_migrations` (reads
  `django_baseline` and `plane_jobs::queue::SCHEMA_VERSION`), `register_instance`,
  `configure_instance`, `create_bucket`, `clear_cache` (FLUSHDB) and the eight embedded
  `/static/` files. `plane-api` runs them in entrypoint order; worker and beat only
  wait (only the migrator migrates). `plane-admin` gains `reset-password`,
  `activate-user`, `test-email` and the four boot steps. Proven by
  `tools/boot/compare_boot.py` and `scenarios/boot/static.yaml`.
- `push_instance_metrics` is not ported (open owner question) and sits behind one
  `PUSH_INSTANCE_METRICS_ENABLED = false` switch; Django enqueues it once per boot.
- The differential harness boots neither side through the entrypoint's setup (amended
  after the `d9eedf323e` verification): `dev/django-serve.sh` starts after it, and
  `scenarios/targets.yaml` sets `PLANE_RS_BOOT_STEPS=skip` on the Rust target only, so
  `plane-api` still waits for the database and migrations but skips `register_instance`,
  `configure_instance`, `create_bucket` and `clear_cache`. The default is `all`, and a skip
  logs a warning (`ENV.md` §6). Boot parity is `compare_boot.py`'s and
  `scenarios/boot/static.yaml`'s job (the latter names its Django command in the scenario's
  `target_commands`). No normaliser covers `GET /api/instances/` boot fields.
- Deliberate boot differences (not HTTP contract): `wait_for_db` really retries
  (Django's loop never opens a connection, `wait_for_db.py:17-22`); `create_bucket`
  branches on the HTTP status, so a non-numeric S3 error code (`AccessDenied`) is logged
  where Django's `int(code)` raises out of the command (`create_bucket.py:37`);
  `test-email`'s 30 s timeout bounds the connect only, where Python bounds every
  socket operation.

## D35. Queue payloads are `json`, so dict key order survives between tasks (decided 2026-11-19)

- `plane_jobs.jobs.payload` was `jsonb`, which sorts object keys, so a Python dict's
  insertion order was lost between Rust tasks. It is user-visible: the actor and comment
  order in `send_email_notification`'s e-mail, the webhook request body (which had
  needed a per-serializer reorder).
- The column is now `json` (exact text). `Job::from_message` builds the ordered text
  from the typed `TaskArg`s; `queue::claim` reads `payload::text` and decodes it into
  `plane_jobs::payload::Payload`, which holds the ordered `pyjson` value beside the
  usual `serde_json` view (handlers keep reading the latter through `Deref`; handlers
  whose output depends on order read `Payload::py*`). The rest of the workspace's
  `serde_json` is unchanged. The webhook delivery's input reorder is removed.
- `plane_jobs::queue::SCHEMA_VERSION` is `"3"`; `migrate` converts an existing `jsonb`
  column in place (`USING payload::json`), and rows queued before that keep jsonb's
  order. A Django → Rust upgrade creates the table fresh, so no queued row is affected.
- Proven by `p8b-en-send-order` (two actors in non-uuid order, SMTP bytes equal) and
  `p8b-send-order` (unsorted `event_data`/`activity`, recorder body and
  `webhook_logs.request_body` equal).

## D36. Cutover deployment: Rust services only, Postgres queue, static from `plane-api` (decided 2026-11-19)

- The shipped stacks (root and CLI compose, AIO supervisor) run `plane-api`
  (`Dockerfile.plane-api`) and `plane-worker`/`plane-beat`/`plane-migrator`
  (`Dockerfile.plane-jobs`). API, worker and beat depend on a completed migrator
  (`service_completed_successfully`); Swarm and the Helm chart (separate repo, changes
  documented in `deployments/kubernetes/community/README.md`) run it to completion first.
- RabbitMQ, `rabbitmq_data`, `AMQP_URL`, `RABBITMQ_*`, `CELERY_*`, `GUNICORN_WORKERS` and
  `METRICS_PUSH_INTERVAL_MINUTES` leave the deployment (`ENV.md` §4.1); the Rust loader still
  accepts the old names. `PLANE_RS_JOB_*` is the queue's tuning surface. Worker and beat use
  the API's env file. `INSTANCE_CHANGELOG_URL` is baked into the API image.
- `Caddyfile.ce` routes `/api/*`, `/auth/*`, `/static/*` to `plane-api:8000`;
  `Caddyfile.aio.ce` gains `/static/*` (deliberate fix: AIO never served it and the
  invitation-e-mail logos 404ed). The coexistence route groups (`deploy/caddy/*`,
  `route-groups.yaml`, `PLANE_RS_GROUP_*`) are dev/harness-only.
- `build-branch.yml` publishes `makeplane/plane-{api,worker,beat,migrator}` instead of
  `plane-backend`; the build action has no `target` input, so `Dockerfile.plane-jobs`'s last
  stage builds the binary named by `JOBS_BINARY`.
- `tools/cutover/exit_check.py all` is the Phase 8 exit check: fresh install and upgrade from
  a restored Django-migrated dump, no Python process in any container, a scripted API walk
  (sign-in → workspace → project → work item → comment → upload → export → invite e-mail →
  webhook delivery), `/static`, `git grep apps/api/bin` clean in deployment files, and it fails
  on any Celery dispatch or `plane.exception` line. The read-only difftest scenarios can't
  target that stack (they need the dev fixture and dev-signed sessions).
- `docker-compose-local.yml` / `docker-compose-test.yml` are developer stacks and still run
  Django; they are not deployment files.

## D37. The job listener drives its connection while it subscribes (decided 2026-11-19)

- `crates/plane-jobs/src/listener.rs::listen_once` polls `Connection::poll_message`
  concurrently with the `LISTEN` request (`tokio::select!`). tokio-postgres writes a request
  only while its `Connection` is polled, so awaiting `LISTEN` first (the code until
  `398c2f05c0`) never sent the subscription, and the worker ran on its poll fallback alone.
  Spawning the `Connection` future isn't an alternative: it discards notifications.
- The forwarding loop starts with the request, so no notification falls between
  subscribing and forwarding. Reconnect and its 1 s backoff are unchanged.
- `the_listener_subscribes_and_wakes_the_worker_before_the_poll_interval` (DB-backed, in
  `plane-jobs/tests/jobs.rs`) requires a `LISTEN plane_jobs_jobs` session and a claim within
  2 s at a 60 s poll; it fails on the old code. Enqueue-to-claim at the default 1 s poll:
  411 ms median (973 max) before, 13 ms (24 max) after; worker idle +0.04 MiB (BENCH.md).
- No other direct tokio-postgres connection has the pattern: all are spawned at once or come
  from `deadpool_postgres`, which spawns its own connection task.

## D38. The Helm chart and the published images live in this repository (decided 2026-11-20)

- Nine images, `ghcr.io/nizarcan/plane-rs-{api,worker,beat,migrator,web,space,admin,live,proxy}`,
  published by `.github/workflows/build-branch.yml` on push to `main`, tags `v*` and dispatch:
  `linux/amd64` + `linux/arm64` built natively, pushed with the built-in `GITHUB_TOKEN`
  (`packages: write`). Every deployment file pulls them (`IMAGE_REGISTRY` replaces
  `DOCKERHUB_USER`). Django-only workflows (`migration-check`, `pull-request-build-lint-api`,
  `check-version`, the upstream `feature-deployment`) are removed.
- `charts/plane-rs` replaces the external `plane-ce` chart: the four Rust services and four
  frontends, ingress paths identical to `Caddyfile.ce`, optional bundled PostgreSQL / Valkey /
  MinIO, an `existingSecret` per secret group (app, database, object storage), no RabbitMQ.
  `chart-ci.yml` lints, renders every fixture and runs `kubeconform -strict`.
- The migrator and bucket Jobs are content-addressed (`<fullname>-migrator-<8 hex>` over the
  image and every pod-template input), never `.Release.Revision`: ArgoCD renders with `helm
  template` (revision always 1) and Job pod templates are immutable. The regular Job carries no
  ArgoCD `Sync` hook (that would re-run the migration on every sync); with an external database
  and an existing Secret it is a Helm `pre-install,pre-upgrade` hook (ArgoCD PreSync). With the
  bundled PostgreSQL a `pg_isready` init container waits for the StatefulSet. `wait_for_migrations`
  holds api/worker/beat in every shape.
- Beat is a singleton (render fails on `replicas != 1`). Probes: api `GET /` (answered without
  the database; startup window 5 min for the boot steps), web `/`, space `/spaces/`, admin
  `/god-mode/`, live TCP; worker, beat and the migrator have none (no HTTP, distroless).

## Open owner decisions

See "Open decisions for the owner" in `RUST_REWRITE.md`. Defaults are assumed until the
owner says otherwise.
