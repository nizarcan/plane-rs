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

## Open owner decisions

See "Open decisions for the owner" in `RUST_REWRITE.md`. Defaults are assumed until the
owner says otherwise.
