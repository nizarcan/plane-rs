# Environment variables and instance configuration

This is the configuration specification for the Rust port. `plane-core`'s config
loader is written from this document: every name, default and parsing rule here
was read out of `apps/api` (Django is the specification) and, where marked,
verified by running the Python code.

Three layers exist, in increasing precedence:

1. **Environment variables** — the 131 names in
   `apps/server/tools/inventory/env_vars.json`, produced by
   `apps/server/tools/inventory/env_vars.py` from an AST scan of
   `apps/api/plane/**/*.py` (tests and migrations excluded) plus
   `apps/api/bin/*.sh` and `apps/api/Dockerfile.api`.
2. **Derived settings** — values Django computes from several environment
   variables (`SESSION_COOKIE_SECURE`, `CELERY_BROKER_URL`, `CORS_*`,
   `AWS_S3_CUSTOM_DOMAIN`, `NUM_PROXIES`, …). §2 gives the exact logic.
3. **Instance configuration in the database** — rows in
   `instance_configurations` that *override* the environment for 36 keys, with 7
   of them encrypted with a key derived from `SECRET_KEY`. §3 gives the table
   shape, precedence and the exact crypto.

Rules for the port:

- Existing deployments must not need to change a single variable or default.
- Where the same variable is read with different defaults in different places
  (§5), the Rust config must be able to reproduce *each* call site. Do not
  collapse them into one value.
- Anything the Rust services need that Django does not have must be prefixed
  `PLANE_RS_` and added to §6.

---

## 0. How to read this document

**Notation used in the "Default" column**

| Notation | Meaning |
| --- | --- |
| `—` | Not configuration at all (a shell local, a build argument, a dead setting): there is no default to reproduce. |
| `""` | Default is the empty string (or, for `os.environ.get("X", "")`, an unset variable yields `""`). |
| `unset` | Read with no default: absent gives `None`, which then behaves as a distinct case (see below). |

**Unset versus empty string.** These are different values in Python and the
difference is observable:

- `os.environ.get("X")` → `None` when unset, `""` when set to empty.
- `os.environ.get("X", d)` → `d` when unset, `""` when set to empty.
- `os.environ.get("X") or d` → `d` for **both** unset and empty.
- `os.environ["X"]` → raises `KeyError` when unset.
- `bool(env)`, `if env:` and `x == "1"` treat `""` exactly like unset, but
  `int(env)` does not (`int("")` raises `ValueError`).

Every table below calls out which of these applies. For the Rust loader, model
an environment value as `Option<String>` where `Some("")` is possible, and
apply the stated default rule explicitly.

**Role codes.** Django loads one settings module for every process, so in
practice every role *reads* every variable. The **Roles** column lists the roles
that actually *use* the value and would misbehave without it:

| Code | Process | Entrypoint |
| --- | --- | --- |
| `api` | gunicorn + uvicorn workers serving `/api/`, `/auth/`, `/static/` | `bin/docker-entrypoint-api.sh` |
| `worker` | `celery -A plane worker` | `bin/docker-entrypoint-worker.sh` |
| `beat` | `celery -A plane beat` (DatabaseScheduler) | `bin/docker-entrypoint-beat.sh` |
| `migrator` | `manage.py migrate` | `bin/docker-entrypoint-migrator.sh` |
| `entry` | the shell entrypoint scripts themselves, before Python starts | — |
| `all` | all four processes | — |

**Python parsing helpers referenced below**

- `int(x)` — Python `int()` on a string: strips leading/trailing ASCII
  whitespace, accepts an optional `+`/`-`, leading zeros and underscores
  between digits. `int("")` and `int("abc")` raise `ValueError`, which at
  settings-import time aborts the process. Rust must decide between "error at
  startup" (Django's behaviour) and "fall back"; where Django crashes, the port
  should fail loudly at startup with the variable name in the message.
- `== "1"` — exact string comparison, **no trimming, no case folding**. `" 1"`,
  `"true"`, `"01"`, `"yes"` are all false.
- `x or d` — Python truthiness: `None` and `""` are falsy, `"0"` is truthy.
- `.split(",")` — keeps empty fields (`"a,,b"` → `["a", "", "b"]`);
  `"".split(",")` → `[""]`, not `[]`.
- `is_valid_url(u)` (`plane/utils/url.py:56`) — `urlparse(u)`, true iff both
  `scheme` and `netloc` are non-empty. `"localhost:3000"` parses as scheme
  `localhost` with empty netloc → invalid. `TypeError` → false.

**A note on import-time failure.** `manage.py`, `asgi.py`, `wsgi.py` and
`celery.py` all default `DJANGO_SETTINGS_MODULE` to `plane.settings.production`,
and the `plane` package's `__init__.py` imports `plane.celery`, which builds a
Redis client at import time. Consequences, verified by running
`env -u REDIS_URL .venv/bin/python manage.py check`:

- `REDIS_URL` is effectively **required by every role, including `migrator`**
  (unset → `AttributeError: 'NoneType' object has no attribute 'startswith'`
  before any management command runs).
- Any malformed `int(...)` setting (e.g. `DEBUG=true`, `FILE_SIZE_LIMIT=""`)
  aborts every role at startup.

The Rust port does not inherit that coupling; `plane-api`, `plane-worker` and
`plane-beat` need Redis, `plane-migrator` does not. Keep the *accepted values*
identical, drop the accidental coupling; it is recorded as a deliberate
behaviour change here and in the session report, and needs no new variable.

---

## 1. Variable reference

### 1.1 Core and security

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `SECRET_KEY` | unset/empty → `get_random_secret_key()` | `os.environ.get("SECRET_KEY") or get_random_secret_key()` — **empty counts as unset**. Random value is 50 chars from `abcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*(-_=+)`, generated per process. No trimming. | Django `SECRET_KEY`; signing key for sessions, password-reset tokens, CSRF; PBKDF2 input for the instance-config Fernet key | all | Logs a **critical** message (does not stop) when the value is one of two known-bad strings: `60gp0byfz2dvffa45cxl20p1scy9xbpf6d8c5y0geejgkyp1b5` (old public default) or `change-this-key-on-deployment` (template placeholder) — `plane/settings/common.py:38`. The random fallback means an unset key makes sessions and *encrypted instance config* unreadable across processes/restarts. `configure_instance` refuses to run without it (§3.3). Rust: make it required; a random fallback silently breaks decryption. |
| `DEBUG` | `"0"` in `common.py`, `0` in `production.py`, `"1"` in `local.py` | `common.py:52` `int(os.environ.get("DEBUG","0"))`; `production.py:12` `int(os.environ.get("DEBUG",0)) == 1` (bool); `local.py:11` `os.environ.get("DEBUG","1") == "1"` | `settings.DEBUG` | all | Production is the default settings module. `DEBUG=1` selects `logs/plane-debug.log` and DEBUG-level `plane.exception`/api loggers (`production.py:31`). `DEBUG=true` crashes (`int`) in common/production but is `False` in local. |
| `ALLOWED_HOSTS` | `"*"` | `os.environ.get("ALLOWED_HOSTS","*").split(",")` — no strip; **set-but-empty gives `[""]`**, which makes every request fail host validation (Django's own check only tests for an empty *list*) | Django `ALLOWED_HOSTS` | api | Django host matching rules apply (`*`, `.example.com` wildcards). |
| `CORS_ALLOWED_ORIGINS` | `""` | Split on `,`, strip each entry, drop empties. Non-empty list → `CORS_ALLOWED_ORIGINS`; empty → `CORS_ALLOW_ALL_ORIGINS=True` (§2.3) | `CORS_ALLOWED_ORIGINS`, `CSRF_TRUSTED_ORIGINS`, `SESSION_COOKIE_SECURE` | api | `CORS_ALLOW_CREDENTIALS=True` is unconditional. `CORS_ALLOW_HEADERS` = django-cors-headers defaults + `X-API-Key`. |
| `COOKIE_DOMAIN` | `None` | Passed through unchanged; empty string stays `""` (Django then emits no `Domain` attribute) | `SESSION_COOKIE_DOMAIN`, `CSRF_COOKIE_DOMAIN` | api | |
| `WEBHOOK_ALLOWED_IPS` | `""` | Split on `,`, strip, skip empties, `ipaddress.ip_network(cidr, strict=False)`; invalid entries are logged at WARNING and skipped (never fatal) | `settings.WEBHOOK_ALLOWED_IPS` → SSRF allowlist | api, worker | Host bits set are accepted and masked (`strict=False`): `"10.0.0.5/8"` becomes `10.0.0.0/8`. IPv4 and IPv6 both allowed. |
| `WEBHOOK_ALLOWED_HOSTS` | `""` | Split on `,`; keep entries whose `strip()` is non-empty; value = `strip().rstrip(".").lower()` | `settings.WEBHOOK_ALLOWED_HOSTS` | api, worker | Exact host match (not suffix); also bypasses `WEBHOOK_DISALLOWED_DOMAINS`. |
| `WEBHOOK_DISALLOWED_DOMAINS` | `""` | Same normalisation as `WEBHOOK_ALLOWED_HOSTS` | `settings.WEBHOOK_DISALLOWED_DOMAINS` | api | Rejects the domain and all subdomains; the request's own host is appended at validation time (`plane/app/serializers/webhook.py:49`). |
| `SKIP_ENV_VAR` | `"1"` | `os.environ.get("SKIP_ENV_VAR","1") == "1"` | `settings.SKIP_ENV_VAR` | all | **Default true** = instance-config rows in the DB win over environment variables. Setting anything other than the exact string `1` (`0`, `false`, `no`) flips to environment-first. See §3.4. |

### 1.2 Django runtime

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `DJANGO_SETTINGS_MODULE` | `plane.settings.production` | `os.environ.setdefault(...)` in `asgi.py:13`, `wsgi.py:16`, `celery.py:20`; `manage.py` uses `setdefault` too | Django settings module | all | `plane.settings.local` only via `--settings=plane.settings.local` (`docker-compose-local.yml`, `docker-entrypoint-api-local.sh`). `plane.settings.test` for tests. Rust has no equivalent. |
| `ENABLE_DRF_SPECTACULAR` | `"0"` | `== "1"` | Adds `drf_spectacular` to `INSTALLED_APPS`, sets `DEFAULT_SCHEMA_CLASS` and imports `SPECTACULAR_SETTINGS` | api | Only affects the OpenAPI schema endpoint. |
| `APP_VERSION` | `False` (absent) | `os.environ.get("APP_VERSION", False)`; truthy → used as the current version, else read `package.json` `version`, else `v0.1.0` | `Instance.current_version` | entry (`register_instance`) | Never set by any compose file; the image ships `package.json`. |

### 1.3 Database

Primary connection is either a single URL or five discrete variables; the URL
wins whenever it is non-empty.

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `DATABASE_URL` | unset | `if bool(os.environ.get("DATABASE_URL")):` → `dj_database_url.config()` (default env name `DATABASE_URL`). Set-but-empty falls back to `POSTGRES_*`. Parsing: `urlsplit`; scheme must be a registered postgres scheme (`postgres`, `postgresql`, `pgsql`, `postgis`, `redshift`, `timescale`, `timescalegis`) — unknown scheme raises at startup. `USER`, `PASSWORD`, `HOST`, `NAME` (path minus leading `/`) are percent-decoded; `PORT` is `urlsplit().port` (int) or `""`. Query string becomes Django `OPTIONS` with `_parse_value` per item: all-digits → int, `true`/`false` (case-insensitive) → bool, else string; repeated keys → list. Then `CONN_MAX_AGE=0`, `CONN_HEALTH_CHECKS=false`, `DISABLE_SERVER_SIDE_CURSORS=false`. For postgres, a `currentSchema` option is replaced by `options=-c search_path=<schema>`. Empty `OPTIONS` is dropped. Invalid port → `ParseError`. | `DATABASES["default"]` | all, entry | Example: `postgresql://plane:plane@plane-db:5432/plane?sslmode=require` → `OPTIONS={"sslmode":"require"}`. |
| `POSTGRES_DB` | unset | Raw string → `NAME` | `DATABASES["default"]["NAME"]` | all, entry | Only used when `DATABASE_URL` is falsy. |
| `POSTGRES_USER` | unset | Raw string → `USER` | `DATABASES["default"]["USER"]` | all, entry | |
| `POSTGRES_PASSWORD` | unset | Raw string → `PASSWORD` | `DATABASES["default"]["PASSWORD"]` | all, entry | |
| `POSTGRES_HOST` | unset | Raw string → `HOST` | `DATABASES["default"]["HOST"]` | all, entry | Empty/unset `HOST` makes libpq use its default (unix socket or `PGHOST`). |
| `POSTGRES_PORT` | `"5432"` | Passed as a **string** to Django; libpq parses it | `DATABASES["default"]["PORT"]` | all, entry | |
| `ENABLE_READ_REPLICA` | `"0"` | `== "1"` exactly. When enabled, adds alias `replica`, installs `ReadReplicaRouter` and appends `ReadReplicaRoutingMiddleware` to `MIDDLEWARE` (`common.py:228-245`) | `DATABASES["replica"]`, `DATABASE_ROUTERS`, `MIDDLEWARE` | api | Routing: writes always primary; migrations only primary (`allow_migrate`); reads go to the replica only for GET/HEAD/OPTIONS on views with `use_read_replica = True`, everything else primary (`plane/middleware/db_routing.py`, `plane/utils/core/dbrouters.py`). A replica that is configured but unreachable is an error, never a silent fallback. |
| `DATABASE_READ_REPLICA_URL` | unset | Non-empty → `dj_database_url.parse(...)` (same rules as `DATABASE_URL`); set-but-empty falls through to the discrete replica variables | `DATABASES["replica"]` | api | |
| `POSTGRES_READ_REPLICA_DB` | unset | Raw → `NAME` | `DATABASES["replica"]["NAME"]` | api | |
| `POSTGRES_READ_REPLICA_USER` | unset | Raw → `USER` | `DATABASES["replica"]["USER"]` | api | |
| `POSTGRES_READ_REPLICA_PASSWORD` | unset | Raw → `PASSWORD` | `DATABASES["replica"]["PASSWORD"]` | api | |
| `POSTGRES_READ_REPLICA_HOST` | unset | Raw → `HOST` | `DATABASES["replica"]["HOST"]` | api | |
| `POSTGRES_READ_REPLICA_PORT` | `"5432"` | Passed as string | `DATABASES["replica"]["PORT"]` | api | |

### 1.4 Redis

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `REDIS_URL` | unset (`None`) | Raw string, used as `CACHES["default"]["LOCATION"]` and `redis.Redis.from_url(url, db=0)`. redis-py rules: scheme must be `redis://`, `rediss://` or `unix://` (else `ValueError`); username/password percent-decoded; path is the DB index (`/2` → db 2) **unless** `?db=` is given; query args are applied as connection kwargs and override explicit ones (`db`, `socket_timeout`, `retry_on_timeout`, `ssl_check_hostname`, …; booleans accept `True/False/Yes/No`; a bad value is a `ValueError`). | `CACHES`, `settings.REDIS_URL`, raw client (`plane/settings/redis.py`) | all | Required at import by every role (§0). Unset is fatal at import. Cache key format is django-redis `:<version>:<key>` (`DECISIONS.md` D5). |
| `REDIS_SSL` | derived | `os.environ.get("REDIS_URL") and "rediss" in os.environ.get("REDIS_URL")` — substring test anywhere in the URL, **not** a scheme check. Value is `None`, `""`, `True` or `False`. | Chooses whether `CONNECTION_POOL_KWARGS` are set on the cache | api, worker | Divergence to reproduce: `plane/settings/redis.py:9` uses the stricter `startswith("rediss://")` to decide whether to pass TLS kwargs to the raw client. A password containing `rediss` turns the cache SSL on but not the raw client. |
| `REDIS_SSL_CERT_REQS` | `""` (then `"required"`) | `(url_ssl_cert_reqs or env).strip().lower() or "required"` — a `ssl_cert_reqs` query parameter on `REDIS_URL` wins (`parse_qs`, blank values ignored); the result is lower-cased; empty → `"required"`. Passed to redis-py as `ssl_cert_reqs` (`required`/`optional`/`none`/`cert_required`). | `CACHES["default"]["OPTIONS"]["CONNECTION_POOL_KWARGS"]["ssl_cert_reqs"]` | api, worker | |
| `REDIS_SSL_CA_CERTS` | unset | Truthy → included verbatim as `ssl_ca_certs` (a filesystem path). Read via `os.environ[...]` but only after a truthiness check, so never a `KeyError`. | `...["CONNECTION_POOL_KWARGS"]["ssl_ca_certs"]` | api, worker | Path inside the container; must be mounted into every role. |

### 1.5 RabbitMQ and Celery

**Historical at the Phase 8 cutover.** The six broker names and
`METRICS_PUSH_INTERVAL_MINUTES` below are no longer set by any shipped
deployment (§4.1); the Rust loader still accepts them so an old environment
file keeps loading unchanged, exactly as it keeps accepting every other
Django-era name. Kept here because Django is the specification for the loader.

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `AMQP_URL` | unset | Truthy → `CELERY_BROKER_URL` verbatim (no parsing, no normalisation) | `CELERY_BROKER_URL` | api, worker, beat | Takes precedence over all `RABBITMQ_*`. |
| `RABBITMQ_HOST` | `"localhost"` | Raw string | Composed broker URL | api, worker, beat | Only used when `AMQP_URL` is falsy. |
| `RABBITMQ_PORT` | `"5672"` | Raw string | Composed broker URL | api, worker, beat | |
| `RABBITMQ_USER` | `"guest"` | Raw string, **not URL-encoded** when composed | Composed broker URL | api, worker, beat | A `@`, `:` or `/` in the user/password breaks the URL. |
| `RABBITMQ_PASSWORD` | `"guest"` | Raw string, not URL-encoded | Composed broker URL | api, worker, beat | |
| `RABBITMQ_VHOST` | `"/"` | Raw string, not URL-encoded; the composed URL always inserts one `/` before it, so the default produces `.../` + `/` = `...//` (AMQP's encoding of vhost `/`) | Composed broker URL | api, worker, beat | A vhost without a leading slash (`plane`) yields `.../plane`. |
| `METRICS_PUSH_INTERVAL_MINUTES` | `"360"` | `int(raw)`; valid iff `0 < value <= 10_000_000`, otherwise `360`. `ValueError`/`OverflowError` → `360` | `celery.py` beat schedule `push-instance-metrics` (`schedule(run_every=timedelta(minutes=...))`) | beat | Used by the scheduler only. |

`CELERY_BROKER_URL` is the only broker setting; `CELERY_TIMEZONE="UTC"`,
`CELERY_TASK_SERIALIZER="json"`, `CELERY_RESULT_SERIALIZER="json"`,
`CELERY_ACCEPT_CONTENT=["application/json"]`, and the scheduler is
`django_celery_beat.schedulers.DatabaseScheduler` (it reads and writes the
`django_celery_beat_*` tables). See §2.4.

### 1.6 Object storage (S3 / MinIO)

Two independent code paths read these: `plane/settings/common.py` (Django
settings, used by `plane.settings.storage.S3Storage` callers that go through
`settings` and by `exporter_expired_task`) and `plane/settings/storage.py`
(`S3Storage.__init__`, which re-reads the environment **directly**, with
different defaults — see §5).

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `USE_MINIO` | `0` | `common.py:329` `int(os.environ.get("USE_MINIO",0)) == 1`; `storage.py:42` `os.environ.get("USE_MINIO") == "1"` — **different parsers** | `settings.USE_MINIO`; MinIO branch in `S3Storage`; `exporter_expired_task` client construction | api, worker | `USE_MINIO="01"`, `" 1"` or `"+1"` is true for `settings.USE_MINIO` but false for `S3Storage`. `create_bucket` ignores the flag and always uses `AWS_S3_ENDPOINT_URL`. |
| `AWS_ACCESS_KEY_ID` | unset (`None`) | Raw string; no fallback to boto's credential chain at the settings level (`common.py:333` comment) | `settings.AWS_ACCESS_KEY_ID`; `S3Storage`; `create_bucket`/`update_bucket` | api, worker, entry | `entry` only `create_bucket`, which always passes it to boto3. |
| `AWS_SECRET_ACCESS_KEY` | unset (`None`) | Raw string. When `USE_MINIO` is on and the value is `secret-key`, `common.py:337` logs a **critical** placeholder-credentials warning | `settings.AWS_SECRET_ACCESS_KEY`; `S3Storage`; bucket commands | api, worker, entry | |
| `AWS_S3_BUCKET_NAME` | `"uploads"` | `common.py:343` → `settings.AWS_STORAGE_BUCKET_NAME` with default `"uploads"`; `storage.py:34` and both bucket commands read the env var with **no default** | `AWS_STORAGE_BUCKET_NAME`, presign bucket, bucket creation | api, worker, entry | See §5: an unset bucket name means `S3Storage` signs for bucket `None` even though the setting says `uploads`. |
| `AWS_REGION` | `""` (settings) / unset (storage) | Raw string; empty is passed to boto3 as `region_name=""` | `settings.AWS_REGION`, boto3 `region_name` | api, worker, entry | |
| `AWS_S3_ENDPOINT_URL` | `None` | `os.environ.get("AWS_S3_ENDPOINT_URL", None) or os.environ.get("MINIO_ENDPOINT_URL", None)` — first truthy wins, empty counts as absent | `settings.AWS_S3_ENDPOINT_URL`; `S3Storage`; bucket commands | api, worker, entry | Same expression in `storage.py:38`. |
| `MINIO_ENDPOINT_URL` | `None` | Fallback for `AWS_S3_ENDPOINT_URL` (see above) | same as above | api, worker, entry | |
| `MINIO_ENDPOINT_SSL` | unset | `== "1"` exactly | Chooses `https` vs the request scheme for the MinIO endpoint when `S3Storage` was constructed with a `request` | api | Only consulted inside the `USE_MINIO` branch and only when `request` is not `None`; otherwise the endpoint URL is used as-is (`storage.py:42-56`). Every api call site passes `request=`, the worker's two call sites (`storage_metadata_task`, `copy_s3_object`) do not, so in practice this is an api-only variable. |
| `SIGNED_URL_EXPIRATION` | `"3600"` | `int(os.environ.get("SIGNED_URL_EXPIRATION","3600"))` — seconds | Default expiry for presigned GET (and POST) URLs | api, worker | `int("")` crashes at the first presign. Used as `ExpiresIn` for both presigned POST and GET. |

MinIO endpoint choice, which the presigner must reproduce exactly: inside the
`USE_MINIO` branch, when `S3Storage` is constructed **with a request** (every api
upload/presign call site does `S3Storage(request=request)`), the endpoint is
`f"{protocol}://{request.get_host()}"` where `protocol` is `https` if
`MINIO_ENDPOINT_SSL == "1"` else the request's own scheme. `AWS_S3_ENDPOINT_URL`
is **ignored** in that case, so a browser-visible presigned URL has the shape
`https://<the host the client used>/<bucket>/<key>`, which the proxy routes to
MinIO (§4). Presigned URLs generated without a request (worker tasks) use
`AWS_S3_ENDPOINT_URL`.

**boto3's own credential chain.** When `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY` are unset, Plane passes `None` to boto3, which then
resolves credentials through its standard chain: the `AWS_*` environment
variables (`AWS_SESSION_TOKEN`, `AWS_PROFILE`, `AWS_SHARED_CREDENTIALS_FILE`,
`AWS_CONFIG_FILE`, `AWS_DEFAULT_REGION`), shared config files, and the
container/EC2 metadata endpoints. Those names are read by boto3, not by Plane,
so they are not in `env_vars.json`. If the Rust S3 client does not implement
`AWS_PROFILE`/instance roles, deployments that rely on them are a behaviour
change and must be recorded as an open question in the report.

The S3 client always uses `Config(signature_version="s3v4")`. Presigned POSTconditions are: `bucket`, `["content-length-range", 1, file_size]`,
`{"Content-Type": file_type}`, plus either `["starts-with", "$key", prefix]`
when the object name starts with `${filename}` or an exact `key` condition
otherwise. `S3Storage.url()` returns the bare key. Presigned GET pins
`ResponseContentType` only for the MIME types in `INLINE_SAFE_MIME_TYPES`, and
always sets `ResponseContentDisposition` (`inline` or `attachment` with an
RFC 5987 `filename*=UTF-8''…`).

**`AWS_S3_CUSTOM_DOMAIN` / `AWS_S3_URL_PROTOCOL`** are derived, not read (§2.5).

### 1.7 Email (SMTP)

These are *not* Django email settings: `EMAIL_HOST`, `EMAIL_HOST_USER`,
`EMAIL_HOST_PASSWORD`, `EMAIL_PORT`, `EMAIL_USE_TLS`, `EMAIL_USE_SSL` and
`EMAIL_FROM` are read through `get_configuration_value()` /
`get_email_configuration()` (`plane/license/utils/instance_value.py:42`) on
every send, so the database row (§3) overrides the environment by default.

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `EMAIL_HOST` | unset (`None`) in the senders; `""` in the instance/admin views and in the seed | Raw string. "SMTP is configured" is decided by `bool(EMAIL_HOST)`, **not** by `ENABLE_SMTP` | `get_connection(host=…)`; `data["is_smtp_configured"]`; `/auth/*/email-check/` (`smtp_configured`) | api, worker | With `SKIP_ENV_VAR=1` (default) and a seeded row, the row's value (possibly `""`) wins. |
| `EMAIL_HOST_USER` | unset (`None`) | Raw string | `get_connection(username=…)` | worker (sends), api (credential check) | |
| `EMAIL_HOST_PASSWORD` | unset (`None`) | Raw string; **encrypted at rest** when seeded | `get_connection(password=…)` | worker, api | |
| `EMAIL_PORT` | `587` (int) when unset in `get_email_configuration`; `"587"` when seeded from the environment | `int(EMAIL_PORT)` at every call site — `int("")` or a non-numeric value raises and fails the task/request | `get_connection(port=…)` | worker, api | Seeded value is a string; the fallback is an int. Both end up in `int()`. |
| `EMAIL_USE_TLS` | `"1"` | `== "1"` | `get_connection(use_tls=…)` | worker, api | |
| `EMAIL_USE_SSL` | `"0"` | `== "1"` | `get_connection(use_ssl=…)` | worker, api | TLS and SSL are independent flags; setting both is left to Django (which raises). |
| `EMAIL_FROM` | `""` when seeded (`core.py:198`); `"Team Plane <team@mailer.plane.so>"` in `get_email_configuration` (`instance_value.py:56`) | Raw string. When empty, Django's `EmailMessage` substitutes `settings.DEFAULT_FROM_EMAIL`, which Plane never overrides → `webmaster@localhost` | `from_email=` on every outgoing mail | worker, api | Because seeding writes `""` when the env var is unset, a fresh install that never sets `EMAIL_FROM` sends from `webmaster@localhost`. Port this as-is. |
| `EMAIL_BACKEND` | `"django.core.mail.backends.console.EmailBackend"` | Raw string; only read in `plane/settings/local.py:21` | `settings.EMAIL_BACKEND` | api-local | Production hardcodes `django.core.mail.backends.smtp.EmailBackend` (`common.py:325`); tests use `locmem`. Not used by the Rust port. |
| `ENABLE_SMTP` | `"0"` | Seeded only; never read for behaviour. `DELETE /api/instances/configurations/disable-email-feature/` sets it to `"0"` and blanks the five `EMAIL_*` rows | `instance_configurations` row | — | A dead flag in this repository: SMTP enablement is inferred from `EMAIL_HOST`. |

### 1.8 URLs, paths and cookies

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `WEB_URL` | unset (`None`) | Raw string, no validation and no trailing-slash normalisation | `settings.WEB_URL`; `base_host()` fallback (`plane/utils/host.py:25`); `SESSION_COOKIE_SECURE`; MinIO custom domain (§2.5); telemetry `domain` attribute | api, worker | `base_host()` raises `ImproperlyConfigured("APP_BASE_URL or WEB_URL is not set")` when both are unset. Emails built from `base_host` therefore require one of them. |
| `APP_BASE_URL` | unset (`None`) | Read raw, then **validated**: falsy stays `None`, otherwise `is_valid_url(value)` must be true or the setting becomes `None` (invalid values are silently dropped, the env var is untouched). | `settings.APP_BASE_URL`; `base_host(is_app=True)`; `SESSION_COOKIE_SECURE` | api | The check only requires a non-empty scheme and netloc, and it does not normalise: a value like `" http://x"` or `"http://x "` passes and is stored verbatim (Python's `urlparse` tolerates the surrounding whitespace), while `"localhost:3000"` (scheme `localhost`, empty netloc) is nulled. |
| `APP_BASE_PATH` | `"/"` | Raw string | `settings.APP_BASE_PATH` | — | Read and stored but **never used**: `base_host(is_app=True)` returns `APP_BASE_URL` or the base origin without appending a path. Keep accepting it. |
| `ADMIN_BASE_URL` | unset (`None`) | Same validate-or-null rule as `APP_BASE_URL` | `settings.ADMIN_BASE_URL`; `base_host(is_admin=True)` | api | The admin frontend lives on its own origin. |
| `ADMIN_BASE_PATH` | `"/god-mode/"` | Raw string. `base_host` ensures a leading and trailing `/`; a non-string falls back to `/god-mode/` | `settings.ADMIN_BASE_PATH` | api | |
| `SPACE_BASE_URL` | unset (`None`) | Same validate-or-null rule | `settings.SPACE_BASE_URL`; `base_host(is_space=True)` | api | |
| `SPACE_BASE_PATH` | `"/spaces/"` | Same normalisation as `ADMIN_BASE_PATH` | `settings.SPACE_BASE_PATH` | api | |
| `LIVE_BASE_URL` | unset (`None`) | Same validate-or-null rule | `settings.LIVE_BASE_URL`; `settings.LIVE_URL` | api | |
| `LIVE_BASE_PATH` | `"/live/"` | Raw string, fed to `urljoin(LIVE_BASE_URL, LIVE_BASE_PATH)`; `LIVE_URL` is `None` when `LIVE_BASE_URL` is unset | `settings.LIVE_BASE_PATH`, `settings.LIVE_URL` | api | A path without a leading `/` replaces the last segment of the base URL (standard `urljoin`). |
| `INSTANCE_CHANGELOG_URL` | `""` (settings) / the Plane changelog URL via `ENV` in `Dockerfile.api:10` | Raw string | `settings.INSTANCE_CHANGELOG_URL`, exposed as `config.instance_changelog_url` | api | The image sets a non-empty default, so the effective default in containers is not `""`. `Dockerfile.plane-api` bakes in the same value at cutover. |
| `SESSION_COOKIE_NAME` | `"session-id"` | Raw string | `settings.SESSION_COOKIE_NAME` | api | The admin session uses the hardcoded `admin-session-id` (`common.py:431`), not configurable. |
| `SESSION_COOKIE_AGE` | `604800` | `int(os.environ.get("SESSION_COOKIE_AGE",604800))` — seconds; also the stored-session expiry | `settings.SESSION_COOKIE_AGE` | api | `SESSION_ENGINE = plane.db.models.session` (custom DB-backed sessions). |
| `ADMIN_SESSION_COOKIE_AGE` | `3600` | `int(...)` seconds | `settings.ADMIN_SESSION_COOKIE_AGE` | api | |
| `SESSION_SAVE_EVERY_REQUEST` | `"0"` | `== "1"` | `settings.SESSION_SAVE_EVERY_REQUEST` | api | When true every request writes a `sessions` row and resets the cookie expiry. |
| `SESSION_COOKIE_SECURE` | derived | See §2.1 | `settings.SESSION_COOKIE_SECURE` and `CSRF_COOKIE_SECURE` | api | Never read directly. |

### 1.9 Authentication and OAuth

All of these are read **through** `get_configuration_value()`, so under the
default `SKIP_ENV_VAR=1` the database row wins and the environment value is only
the seed/fallback. See §3.

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `ENABLE_SIGNUP` | `"1"` in the seed and in `Adapter.__check_signup`; **`"0"` in `license/api/views/instance.py:68`** | `== "0"` means disabled outside the instance endpoint; the instance endpoint reports it as `== "1"` (boolean) | signup gate in `plane/authentication/adapter/base.py:99`; `config.enable_signup` | api | Divergent defaults, see §5. Disabled signup still allows invited emails. |
| `ENABLE_EMAIL_PASSWORD` | `"1"` (seed and instance view); unset (`None`) in `authentication/provider/credentials/email.py:30` | Disabled iff `== "0"`. When the value is `None` authentication is **enabled** (None != "0") | `EmailProvider` gate; `config.is_email_password_enabled` | api | |
| `ENABLE_MAGIC_LINK_LOGIN` | `"0"` in the seed; `"1"` at every read site (`magic_code.py:62`, `auth/views/{app,space}/check.py:49`, `license/api/views/instance.py:97`) | Disabled iff `== "0"`; the instance endpoint reports `== "1"` | magic-link availability; `config.is_magic_login_enabled` | api | Divergent defaults, see §5. Because the seed writes `"0"`, a fresh install has magic links **off** even though the code's fallback default is on. |
| `IS_GOOGLE_ENABLED` | `"0"` | `== "1"` in the instance view | Google button visibility (`config.is_google_enabled`) | api | Provider construction does not check this flag; it checks that the client id/secret exist. |
| `GOOGLE_CLIENT_ID` | unset (`None`) | Raw string; provider raises `GOOGLE_NOT_CONFIGURED` unless both id and secret are truthy | Google OAuth | api | |
| `GOOGLE_CLIENT_SECRET` | unset (`None`) | Raw string; **encrypted at rest** once seeded | Google OAuth | api | |
| `ENABLE_GOOGLE_SYNC` | `"0"` | `== "1"` (`Adapter.check_sync_enabled`, key chosen by provider name) | Refresh profile data from the provider on login | api | |
| `IS_GITHUB_ENABLED` | `"0"` | `== "1"` | `config.is_github_enabled` | api | |
| `GITHUB_CLIENT_ID` | unset (`None`) | Raw string | GitHub OAuth | api | |
| `GITHUB_CLIENT_SECRET` | unset (`None`) | Raw string; encrypted at rest | GitHub OAuth | api | |
| `GITHUB_ORGANIZATION_ID` | unset (`None`) | Raw string; truthy adds the `read:org` scope and enables org membership checks | GitHub OAuth | api | |
| `ENABLE_GITHUB_SYNC` | `"0"` | `== "1"` | profile sync on login | api | |
| `GITHUB_APP_NAME` | `""` | Raw string; only looked up in the DB by `/api/instances/`, **never seeded** | `config.github_app_name` (`str(...)`) | api | With no row it falls back to the env var / `""`. |
| `GITLAB_CLIENT_ID` | unset (`None`) | Raw string | GitLab OAuth | api | |
| `GITLAB_CLIENT_SECRET` | unset (`None`) | Raw string; encrypted at rest | GitLab OAuth | api | |
| `GITLAB_HOST` | `"https://gitlab.com"` at the provider (`gitlab.py:38`); unset (`None`) in the seed | Raw string; the provider appends `/oauth/token` and `/api/v4/user` | GitLab OAuth | api | Divergent defaults, see §5. |
| `ENABLE_GITLAB_SYNC` | `"0"` | `== "1"` | profile sync | api | |
| `GITEA_CLIENT_ID` | unset (`None`) | Raw string | Gitea OAuth | api | |
| `GITEA_CLIENT_SECRET` | unset (`None`) | Raw string; encrypted at rest | Gitea OAuth | api | |
| `GITEA_HOST` | unset (`None`) | Raw string; scheme must be `http`/`https` (else `GITEA_NOT_CONFIGURED`); trailing slashes stripped | Gitea OAuth | api | |
| `ENABLE_GITEA_SYNC` | `"0"` | `== "1"` | profile sync | api | |
| `SLACK_CLIENT_ID` | unset (`None`) | Raw string; only looked up in the DB by `/api/instances/`, never seeded | `config.slack_client_id` | api | Exposed to the frontend as-is (may be `null`). |

### 1.10 Rate limits and client IP

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `API_KEY_RATE_LIMIT` | `"60/minute"` | DRF `SimpleRateThrottle` format: `"<n>/<period>"`, `period[0]` selects `s`=1, `m`=60, `h`=3600, `d`=86400 seconds; `n` is `int()`. A malformed value raises on the first API-key request (`ValueError` from `parse_rate`). The empty string is **falsy**, so DRF falls back to `THROTTLE_RATES["api_key"]`, which Plane does not define → `ImproperlyConfigured`. | `plane/api/rate_limit.py:14` (`scope="api_key"`, key = the `X-Api-Key` header; `plane/api/views/base.py:128` copies `X-RateLimit-Remaining`/`X-RateLimit-Reset` from the request to the response) | api | Applies to `/api/v1/` only. |
| `AUTHENTICATION_RATE_LIMIT` | `"10/minute"` | Same format; empty string → same `ImproperlyConfigured` path (`scope="authentication"`) | `AuthenticationThrottle` (`plane/authentication/rate_limit.py:29`), keyed on the client IP for **every** caller (signed in or not) | api | Applies to all of `/auth/`. Throttle state is in Redis under DRF's `throttle_authentication_<ip>` (django-redis adds the `:1:` prefix); the API-key throttle uses `api_key:<key>`. Both backends must read and write the same keys during coexistence (`DECISIONS.md` D5). |
| `NUM_PROXIES` | `""` | `raw = os.environ.get("NUM_PROXIES","").strip()`; if `raw.isdecimal() and len(raw) <= 3` → `REST_FRAMEWORK["NUM_PROXIES"] = int(raw)`; else if `raw` is non-empty → WARNING and the setting is left unset (`None`). `isdecimal()` is Unicode-aware (`"٣"` is decimal and `int()` accepts it). | `REST_FRAMEWORK["NUM_PROXIES"]` → DRF `get_ident` | api | `None` (unset) means DRF joins **all** `X-Forwarded-For` entries with whitespace removed; `0` means `REMOTE_ADDR`; `n` means the n-th address from the right. This is separate from `plane.utils.ip_address.get_client_ip`, which unconditionally takes the **first** `X-Forwarded-For` entry (used for logging and `last_login_ip`). |

§2.6 has the exact client-IP pseudo-code.

### 1.11 Logging, telemetry, OTLP

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `OTLP_ENDPOINT` | `"https://telemetry.plane.so"` | Raw string. Two derived forms (`plane/utils/otlp_endpoints.py`): gRPC `host:port` (`:46-53`; 443 for `https`, otherwise 4317) and HTTP `<url>/v1/metrics` (`:56-59`, §2.7) | OTLP metric exporter target | worker | Only the `push_instance_metrics` task uses it. There is no OTLP tracing anywhere in this repository. The Rust handler (`plane-tasks/src/telemetry_metrics.rs`) reads the same variable for both transports and points at the same collector. |
| `OTLP_METRICS_PROTOCOL` | `"grpc"` | `(os.environ.get("OTLP_METRICS_PROTOCOL") or "grpc").strip().lower()`; `"grpc"` → gRPC exporter, **anything else** → HTTP exporter | Exporter class | worker | Empty string behaves as `grpc` (`or`). Both branches are ported: gRPC over `h2` with the OTLP `MetricsService/Export` method, HTTP as a `POST` to `<endpoint>/v1/metrics`. |
| `OTEL_EXPORTER_OTLP_METRICS_INSECURE` | `""` | `os.environ.get(...,"").lower() == "true"` — no strip, so `" true"` is false | gRPC `insecure=` flag | worker | Only read in the gRPC branch. `telemetry_metrics.py:55` passes it explicitly, so the endpoint's scheme is not consulted (`get_otlp_grpc_endpoint` strips it anyway). The Rust handler reads the same variable; secure uses the pinned certifi roots with ALPN `h2`. |
| `OTEL_EXPORTER_OTLP_METRICS_TIMEOUT` | unset → `OTEL_EXPORTER_OTLP_TIMEOUT` → `10` | `float(raw)`; the exporter's deadline for the whole retry loop, and the per-RPC deadline for gRPC | Both transports | worker | The Rust handler reproduces the precedence and falls back to 10 on a malformed value. |
| `OTEL_EXPORTER_OTLP_TIMEOUT` | `10` | Same, for both signals | Both transports | worker | |
| `OTEL_EXPORTER_OTLP_METRICS_HEADERS` | unset → `OTEL_EXPORTER_OTLP_HEADERS` | Comma-separated `name=value`; values URL-decoded, names lower-cased (`opentelemetry/util/re.py:parse_env_headers(liberal=True)`) | HTTP request headers / gRPC metadata | worker | Metrics-specific wins over the global when non-empty. The Rust handler reproduces the parser and sends them on both transports. |
| `OTEL_EXPORTER_OTLP_HEADERS` | unset | Same | Both transports | worker | Global fallback. |
| `OTEL_EXPORTER_OTLP_METRICS_COMPRESSION` | unset → `OTEL_EXPORTER_OTLP_COMPRESSION` → none | `gzip` for both; HTTP also accepts `deflate`; `none`. HTTP: an invalid value warns and sends uncompressed. gRPC: an invalid value raises in the exporter, so the task sends nothing | Both transports | worker | gRPC uses `grpc-encoding: gzip` and the compressed frame bit; HTTP `Content-Encoding: gzip`/`deflate`. Python's HTTP gzip header embeds the current mtime, so `difftest task` decompresses before comparing. |
| `OTEL_EXPORTER_OTLP_COMPRESSION` | unset | Same | Both transports | worker | The gRPC mixin treats a metrics value of `none` as "not provided" and re-reads this variable; the Rust handler reproduces that quirk. |
| `SERVICE_NAME` | `"plane-ce-api"` | Raw string | OTLP resource `service.name` | worker | The default is a leftover from the api image; the worker (which runs the metric push) uses it unchanged. |
| `OTEL_RESOURCE_ATTRIBUTES` | unset | Comma-separated `key=value`; a value is `urllib.parse.unquote`d and surrounding whitespace stripped; a pair without `=` is logged and skipped | OTLP resource attributes, merged after the SDK defaults and before `SERVICE_NAME`/`instance_id`/`plane.instance.type` | worker | Python's `OTELResourceDetector` runs last, so it can override the random `service.instance.id`. The Rust handler reproduces the merge and the order. |
| `OTEL_SERVICE_NAME` | unset | Non-empty → `service.name` in the detector's resource | OTLP resource `service.name` | worker | Overridden by `SERVICE_NAME` for this task (the task's own dict is merged last). |
| `SCOUT_MONITOR` | `False` (bool) | `os.environ.get("SCOUT_MONITOR", False)` — any non-empty string is truthy, including `"0"` and `"false"` | Scout APM (`scout_apm.django` is in `INSTALLED_APPS` in production) | api, worker, beat | Only present in `production.py:21`. Scout is part of the production image (`scout-apm==3.5.3`); if the flag is off it should be inert. Recommendation for the port: drop it (report it as an open question) unless the owner wants Scout kept. |
| `SCOUT_KEY` | `""` | Raw string | Scout APM key | api, worker, beat | |

Not ported, and not set by any deployment: `OTEL_EXPORTER_OTLP_*_CERTIFICATE`,
`OTEL_EXPORTER_OTLP_*_CLIENT_KEY`, `OTEL_EXPORTER_OTLP_*_CLIENT_CERTIFICATE`,
`OTEL_PYTHON_EXPORTER_OTLP_*_CREDENTIAL_PROVIDER`,
`OTEL_PYTHON_EXPORTER_OTLP_GRPC_RETRYABLE_ERROR_CODES`, gRPC's
`google.rpc.retryinfo-bin` trailer, and the HTTP exporter's 64 MiB request-size
cap. A TLS client certificate would also be a second trust configuration
(`D9`).

Logging itself is not configurable by environment: `production.py` writes JSON
to stdout (`pythonjsonlogger`, format `"%(levelname)s %(asctime)s %(module)s
%(name)s %(message)s"`) and additionally to
`logs/plane-error.log` (or `plane-debug.log` when `DEBUG`) with 1 MiB rotation,
5 backups. The loggers and levels are fixed in code (`plane.api.request`,
`plane.api`, `plane.worker`, `plane.exception`, `plane.external`,
`plane.authentication`, `plane.migrations`).

### 1.12 Limits and retention

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `FILE_SIZE_LIMIT` | `5242880` | `int(os.environ.get("FILE_SIZE_LIMIT",5242880))` — **no validation**; used twice (`settings.FILE_SIZE_LIMIT` and `settings.DATA_UPLOAD_MAX_MEMORY_SIZE`) | Max upload size in presigned POSTs and model validators; Django's request-body limit; avatar download cap; `config.file_size_limit` (as `float(...)`) | api, worker, proxy | `int("")` or `int("5MB")` aborts every Python role at startup. The proxy (`Caddyfile.ce`) uses the same variable with its own default (`5242880`). |
| `HARD_DELETE_AFTER_DAYS` | `60` | `int(os.environ.get("HARD_DELETE_AFTER_DAYS",60))` — no validation; negative values put the cutoff in the future and delete everything | `settings.HARD_DELETE_AFTER_DAYS`, nightly `hard_delete` task | worker (beat schedules it) | |
| `API_ACTIVITY_LOG_RETENTION_DAYS` | `14` | `_retention_days(...)`: unset → default; non-integer → default; negative → default; `0` allowed | `delete_api_logs` | worker | API activity logs hold payloads, hence the shorter window. |
| `WEBHOOK_LOG_RETENTION_DAYS` | `14` | same helper | `delete_webhook_logs` | worker | |
| `EMAIL_LOG_RETENTION_DAYS` | `7` | same helper | `delete_email_notification_logs` | worker | |
| `UNUPLOADED_ASSET_DELETE_DAYS` | `"7"` | `int(os.environ.get("UNUPLOADED_ASSET_DELETE_DAYS","7"))` — no validation; a negative value deletes **every** unuploaded asset (cutoff in the future) | `delete_unuploaded_file_asset` | worker | |

The `_retention_days` helper is documented in §2.8.

### 1.13 AI and third-party integrations

| Variable | Default | Parsing and semantics | Feeds | Roles | Notes |
| --- | --- | --- | --- | --- | --- |
| `LLM_API_KEY` | unset (`None`) in `get_llm_config`; `""` in the instance view and seed | Raw string; **encrypted at rest** once seeded. Falsy → the endpoint logs and returns "not configured" | `plane/app/views/external/base.py:76` (`get_llm_config`) | api | The provider is chosen separately; an unknown provider also disables the feature. |
| `LLM_PROVIDER` | `"openai"` | `provider_key.lower()` looked up in `{openai, anthropic, gemini}`; unknown → disabled and logged | provider class selection | api | |
| `LLM_MODEL` | `"gpt-4o-mini"` (seed) / unset (`None`) in `get_llm_config` | Raw string; falsy → the provider's `default_model` | model name in the provider request | api | Divergent defaults, see §5. |
| `GPT_ENGINE` | `"gpt-3.5-turbo"` | Seeded only; never read | `instance_configurations` row | — | Deprecated; dead in CE. |
| `UNSPLASH_ACCESS_KEY` | unset (`None`) in `settings`/`get_llm` sites; `""` in the instance view and seed | Raw string; **encrypted at rest** once seeded. Falsy → the Unsplash endpoint returns `[]` | `plane/app/views/external/base.py:215` (`UnsplashEndpoint`), `config.has_unsplash_configured` | api | |
| `GITHUB_ACCESS_TOKEN` | `False` (bool) | Raw string; declared but **never used anywhere** | `settings.GITHUB_ACCESS_TOKEN` | — | Dead setting; keep accepting it for compatibility, no behaviour. |
| `ANALYTICS_SECRET_KEY` | `False` (bool) | Raw string; never used | `settings.ANALYTICS_SECRET_KEY` | — | Dead setting. |
| `ANALYTICS_BASE_API` | `False` (bool) | Raw string; never used | `settings.ANALYTICS_BASE_API` | — | Dead setting. |
| `IS_TEST` | `"0"` | `== "1"` | `Instance.is_test` | entry (`register_instance`) | Marks an instance created during tests. |

### 1.14 Image, entrypoint and deployment-only variables

These appear in `env_vars.json` because the scan covers the entrypoint scripts
and `Dockerfile.api`. Most are build-time or shell-local and must **not** be
part of the Rust config.

| Variable | Default | Where | Meaning |
| --- | --- | --- | --- |
| `GUNICORN_WORKERS` | — | `bin/docker-entrypoint-api.sh:38` | `-w "$GUNICORN_WORKERS"` for gunicorn. Compose default `1` (`deployments/cli/community/docker-compose.yml`), `.env.example` `2`. **Removed from the shipped deployments at cutover** (§4.1); the Rust equivalent is `PLANE_RS_WORKER_THREADS` (§6), which counts tokio threads rather than processes. |
| `PORT` | `${PORT:-8000}` | `bin/docker-entrypoint-api.sh:38` | gunicorn `--bind 0.0.0.0:${PORT:-8000}`. |
| `HOSTNAME` | shell | both api entrypoints (`:18`) | `hostname` command output used to build `MACHINE_SIGNATURE`. Shell-local. |
| `MAC_ADDRESS` | shell | both api entrypoints (`:18`) | First MAC from `ip link`. Shell-local. |
| `CPU_INFO` | shell | both api entrypoints (`:18`) | `cat /proc/cpuinfo`. Shell-local. |
| `MEMORY_INFO` | shell | both api entrypoints (`:18`) | `free -h`. Shell-local. |
| `DISK_INFO` | shell | both api entrypoints (`:18`) | `df -h`. Shell-local. |
| `SIGNATURE` | shell | both api entrypoints (`:21`) | sha256 of the concatenation above. Shell-local. |
| `MACHINE_SIGNATURE` | shell | both api entrypoints (`:24`) | Exported, then passed as an argv to `register_instance`. Shell-local. |
| `PYTHONDONTWRITEBYTECODE` | `1` | `Dockerfile.api:7` | Image build/runtime; irrelevant to Rust. |
| `PYTHONUNBUFFERED` | `1` | `Dockerfile.api:8` | Same. |
| `PIP_DISABLE_PIP_VERSION_CHECK` | `1` | `Dockerfile.api:9` | Same. |
| `UV_PROJECT_ENVIRONMENT` | `/usr/local` | `Dockerfile.api:13` | uv installs into the image Python. Build-time. |
| `UV_PYTHON_DOWNLOADS` | `never` | `Dockerfile.api:14` | Build-time. |
| `UV_COMPILE_BYTECODE` | `1` | `Dockerfile.api:15` | Build-time. |
| `UV_NO_CACHE` | `1` | `Dockerfile.api:16` | Build-time. |
| `GRPC_DEFAULT_SSL_ROOTS_FILE_PATH` | `/etc/ssl/certs/ca-certificates.crt` | `Dockerfile.api:19` | Forces grpcio's OTLP exporter to use the system CA bundle. Rust: the OTLP/tonic client should use the system store; add an override under the `PLANE_RS_` rule (§6) only if a deployment needs it. |
| `APK_SECURITY_PATCH` | `2026-10-26` | `Dockerfile.api:24` | Build `ARG` used only to bust the apk cache. Build-time. |
| `MIMALLOC_PURGE_DELAY` | `0` | `apps/server/deploy/docker/Dockerfile.plane-api` (`runtime` stage) | mimalloc's purge delay in milliseconds: how long a freed segment is kept before being returned to the OS (and 10x that for a whole arena). mimalloc's own default is `10`; `0` purges immediately and `-1` never purges. Not read by Plane code. `0` lowers the post-load plateau from 89.1 to 51.4 MiB after 1M requests at concurrency 32 (`BENCH.md`, "Phase 3: memory after sustained load"). |

---

## 2. Derived settings

Everything in this section is computed from the variables above. Rust must
reproduce the logic exactly, including the falsy/empty distinctions.

### 2.1 Cookie security (`common.py:406`)

```
_cookies_secure(web_url, cors_allowed_origins, app_base_url=None):
    if (web_url or app_base_url or "").strip().lower().startswith("https://"):
        return True
    return len(cors_allowed_origins) > 0
           and not any("http:" in origin for origin in cors_allowed_origins)

SESSION_COOKIE_SECURE = _cookies_secure(env("WEB_URL"),                 # raw env, not settings.WEB_URL
                                        cors_allowed_origins,           # the stripped list from CORS_ALLOWED_ORIGINS
                                        app_base_url = env("APP_BASE_URL"))   # raw env, not the validated setting
CSRF_COOKIE_SECURE     = SESSION_COOKIE_SECURE
SESSION_COOKIE_DOMAIN  = env("COOKIE_DOMAIN") or None
CSRF_COOKIE_DOMAIN     = env("COOKIE_DOMAIN") or None
CSRF_TRUSTED_ORIGINS   = cors_allowed_origins      # possibly []
```

Notes:

- `web_url or app_base_url` means an **empty** `WEB_URL` falls through to
  `APP_BASE_URL`.
- The raw environment values are used here, so an `APP_BASE_URL` that
  `is_valid_url` later rejects still makes cookies `Secure` if it starts with
  `https://`.
- `"http:" in origin` is a substring test: an `https://` origin does not match
  (`"https:"`), but any origin whose text contains `http:` does (e.g.
  `https://x/p?u=http:`), flipping the result to insecure.
- With no CORS origins configured, cookies are `Secure` only via the
  `https://` prefix test. Plain-HTTP installs therefore get non-`Secure`
  cookies.

### 2.2 Allowed hosts (`common.py:95`)

```
ALLOWED_HOSTS = env("ALLOWED_HOSTS", "*").split(",")     # no strip, no filtering
```

Set-but-empty → `[""]` (all requests rejected). Unset → `["*"]`.

### 2.3 CORS and CSRF trusted origins (`common.py:191`)

```
raw = env("CORS_ALLOWED_ORIGINS", "")
origins = [o.strip() for o in raw.split(",") if o.strip()]

if len(origins) > 0:
    CORS_ALLOWED_ORIGINS = origins
    CORS_ALLOW_ALL_ORIGINS = <unset: django-cors-headers default False>
else:
    CORS_ALLOWED_ORIGINS = <unset>
    CORS_ALLOW_ALL_ORIGINS = True

CORS_ALLOW_CREDENTIALS = True
CORS_ALLOW_HEADERS = django_cors_headers.default_headers + ["X-API-Key"]
CSRF_TRUSTED_ORIGINS = origins            # always the list, possibly empty
```

With `CORS_ALLOW_ALL_ORIGINS = True`, django-cors-headers reflects the request
`Origin` and, because credentials are allowed, adds
`Access-Control-Allow-Credentials: true`; it also adds `Vary: Origin`. Origins
are compared exactly (scheme + host + port, no trailing slash).

### 2.4 Celery broker URL (`common.py:354`)

```
RABBITMQ_HOST     = env("RABBITMQ_HOST", "localhost")
RABBITMQ_PORT     = env("RABBITMQ_PORT", "5672")
RABBITMQ_USER     = env("RABBITMQ_USER", "guest")
RABBITMQ_PASSWORD = env("RABBITMQ_PASSWORD", "guest")
RABBITMQ_VHOST    = env("RABBITMQ_VHOST", "/")
AMQP_URL          = env("AMQP_URL")              # None when unset

if AMQP_URL:                                     # empty string counts as unset
    CELERY_BROKER_URL = AMQP_URL
else:
    CELERY_BROKER_URL = f"amqp://{RABBITMQ_USER}:{RABBITMQ_PASSWORD}@{RABBITMQ_HOST}:{RABBITMQ_PORT}/{RABBITMQ_VHOST}"
```

No percent-encoding is applied to any component. With all defaults the result
is `amqp://guest:guest@localhost:5672//` (vhost `/`). Rust must parse this URL
into (host, port, user, password, vhost) for `lapin`; note that the vhost is
everything after the third `/`, so `//` means `/`.

**At the cutover the derivation is dead code in a deployment** (§4.1): no
shipped environment sets `AMQP_URL` or `RABBITMQ_*`, every task is
`plane_jobs`-owned, and no `plane-mq` container exists. `plane-core` keeps the
parser because the loader must accept a Django-era environment unchanged.

### 2.5 MinIO custom domain (`common.py:348`)

```
AWS_S3_ENDPOINT_URL = env("AWS_S3_ENDPOINT_URL") or env("MINIO_ENDPOINT_URL")   # None if both unset
USE_MINIO           = int(env("USE_MINIO", 0)) == 1

if AWS_S3_ENDPOINT_URL and USE_MINIO:
    parsed = urlparse(env("WEB_URL", "http://localhost"))   # note: raw env, and "" is NOT replaced
    AWS_S3_CUSTOM_DOMAIN = f"{parsed.netloc}/{AWS_STORAGE_BUCKET_NAME}"
    AWS_S3_URL_PROTOCOL  = f"{parsed.scheme}:"
```

Edge cases to reproduce:

- `WEB_URL` unset → `http://localhost` → custom domain `localhost/uploads`,
  protocol `http:`.
- `WEB_URL=""` (set but empty) → `urlparse("")` → custom domain
  `/uploads`, protocol `:` (a single colon). This is what the shipped compose
  defaults avoid by always setting `WEB_URL`.
- `WEB_URL` with a path (`https://example.com/plane`) → the path is ignored
  (`netloc` only).

### 2.6 Client IP (`NUM_PROXIES`)

DRF throttle identity (`rest_framework/throttling.py`, used by every throttle):

```
xff        = header "X-Forwarded-For"          # None when absent
remote     = request peer address              # REMOTE_ADDR
num_proxies = REST_FRAMEWORK["NUM_PROXIES"]    # None when unset/invalid

if num_proxies is None:
    ident = "".join(xff.split()) if xff else remote      # all XFF entries, whitespace removed
elif num_proxies == 0 or xff is None:
    ident = remote
else:
    addrs = xff.split(",")
    ident = addrs[-min(num_proxies, len(addrs))].strip()
```

Everything else that asks for a client IP uses the *other* helper
(`plane/utils/ip_address.py:get_client_ip`): first `X-Forwarded-For` entry if
present, else `REMOTE_ADDR`, **no** honouring of `NUM_PROXIES`. That value ends
up in `users.last_login_ip` and the request logs. Both behaviours must be
ported; they are not interchangeable.

### 2.7 OTLP endpoints (`plane/utils/otlp_endpoints.py`)

```
_DEFAULT = "https://telemetry.plane.so"

grpc_endpoint(url):
    if "://" not in url: url = "//" + url
    p = urlparse(url)
    host = p.hostname or "telemetry.plane.so"
    port = p.port if p.port is not None
           else (443 if p.scheme == "https" else 4317)
    return f"{host}:{port}"

http_metrics_url(url) = url.rstrip("/") + "/v1/metrics"

OTLP_ENDPOINT = env("OTLP_ENDPOINT", _DEFAULT)
protocol      = (env("OTLP_METRICS_PROTOCOL") or "grpc").strip().lower()
endpoint      = grpc_endpoint(OTLP_ENDPOINT) if protocol == "grpc" else http_metrics_url(OTLP_ENDPOINT)
insecure      = (env("OTEL_EXPORTER_OTLP_METRICS_INSECURE", "")).lower() == "true"
service_name  = env("SERVICE_NAME", "plane-ce-api")
```

`urlparse` raises `ValueError` for a non-numeric port (`"host:abc"`), which
propagates out of the Celery task. The Rust port should treat an unparseable
endpoint as "telemetry misconfigured" and log, not panic.

### 2.8 Retention helper (`common.py:475`)

```
_retention_days(var, default):
    raw = env(var)                  # None when unset
    if raw is None:            return default
    try: days = int(raw)
    except ValueError:         return default
    return days if days >= 0 else default

API_ACTIVITY_LOG_RETENTION_DAYS = _retention_days("API_ACTIVITY_LOG_RETENTION_DAYS", 14)
WEBHOOK_LOG_RETENTION_DAYS      = _retention_days("WEBHOOK_LOG_RETENTION_DAYS", 14)
EMAIL_LOG_RETENTION_DAYS        = _retention_days("EMAIL_LOG_RETENTION_DAYS", 7)
```

`""` is a `ValueError` → default. `"0"` is accepted. `-1` → default.

### 2.9 Session and CSRF constants that are not configurable

For completeness, because the Rust port must match them: `SESSION_COOKIE_HTTPONLY
= True`, `CSRF_COOKIE_HTTPONLY = True`, `CSRF_FAILURE_VIEW =
plane.authentication.views.common.csrf_failure`, `SESSION_ENGINE =
plane.db.models.session`, `ADMIN_SESSION_COOKIE_NAME = "admin-session-id"`,
`SITE_ID = 1`, `PASSWORD_RESET_TIMEOUT = 3600`, `TIME_ZONE = "UTC"`,
`USE_TZ = True`.

---

## 3. Instance configuration (database overrides)

### 3.1 Table

Model: `plane/license/models/instance.py:72` (`InstanceConfiguration`).
Table: **`instance_configurations`** (verified against
`plane/license/migrations/0001_initial.py`). Columns:

| Column | Type / constraints | Notes |
| --- | --- | --- |
| `id` | `uuid` PK, default `uuid4` | |
| `created_at` | `timestamptz NOT NULL` | `auto_now_add` |
| `updated_at` | `timestamptz NOT NULL` | `auto_now` |
| `created_by_id` | `uuid NULL` FK `users.id` `ON DELETE SET NULL` | set from the request user via `crum`; NULL for CLI writes |
| `updated_by_id` | `uuid NULL` FK `users.id` `ON DELETE SET NULL` | same |
| `deleted_at` | `timestamptz NULL` | soft delete |
| `key` | `varchar(100) NOT NULL UNIQUE` | unique index covers soft-deleted rows |
| `value` | `text NULL` | plaintext, or a Fernet token when `is_encrypted` |
| `category` | `text NOT NULL` | one of the constants below |
| `is_encrypted` | `boolean NOT NULL DEFAULT false` | |

Reads go through the default manager, which filters `deleted_at IS NULL`
(`SoftDeletionManager`). `configure_instance` also uses that manager, so a
soft-deleted key is invisible to it but still occupies the unique index — an
`IntegrityError` follows (§3.7).

### 3.2 Config keys

36 keys, from `plane/utils/instance_config_variables/core.py` (the `extended`
list is empty; commercial editions add to it). "Seeded from" is the environment
variable whose value is written on first seed, with the exact default used when
that variable is unset.

| Key | Category | Encrypted | Seeded from (default) | Read by |
| --- | --- | --- | --- | --- |
| `ENABLE_SIGNUP` | AUTHENTICATION | no | `ENABLE_SIGNUP` (`"1"`) | `adapter/base.py:103`, instance endpoint (`"0"` fallback) |
| `ENABLE_EMAIL_PASSWORD` | AUTHENTICATION | no | `ENABLE_EMAIL_PASSWORD` (`"1"`) | `credentials/email.py:30`, instance endpoint |
| `ENABLE_MAGIC_LINK_LOGIN` | AUTHENTICATION | no | `ENABLE_MAGIC_LINK_LOGIN` (`"0"`) | `credentials/magic_code.py:62`, both `check.py`, instance endpoint |
| `DISABLE_WORKSPACE_CREATION` | WORKSPACE_MANAGEMENT | no | `DISABLE_WORKSPACE_CREATION` (`"0"`) | `workspace/base.py:88`, instance endpoint |
| `IS_GOOGLE_ENABLED` | GOOGLE | no | `IS_GOOGLE_ENABLED` (`"0"`) | instance endpoint |
| `GOOGLE_CLIENT_ID` | GOOGLE | no | `GOOGLE_CLIENT_ID` (`None`) | `provider/oauth/google.py:32` |
| `GOOGLE_CLIENT_SECRET` | GOOGLE | **yes** | `GOOGLE_CLIENT_SECRET` (`None`) | `provider/oauth/google.py:36` |
| `ENABLE_GOOGLE_SYNC` | GOOGLE | no | `ENABLE_GOOGLE_SYNC` (`"0"`) | `adapter/base.py:124` |
| `IS_GITHUB_ENABLED` | GITHUB | no | `IS_GITHUB_ENABLED` (`"0"`) | instance endpoint |
| `GITHUB_CLIENT_ID` | GITHUB | no | `GITHUB_CLIENT_ID` (`None`) | `provider/oauth/github.py:37` |
| `GITHUB_CLIENT_SECRET` | GITHUB | **yes** | `GITHUB_CLIENT_SECRET` (`None`) | `provider/oauth/github.py:41` |
| `GITHUB_ORGANIZATION_ID` | GITHUB | no | `GITHUB_ORGANIZATION_ID` (`None`) | `provider/oauth/github.py:45` |
| `ENABLE_GITHUB_SYNC` | GITHUB | no | `ENABLE_GITHUB_SYNC` (`"0"`) | `adapter/base.py:124` |
| `IS_GITLAB_ENABLED` | GITLAB | no | `IS_GITLAB_ENABLED` (`"0"`) | instance endpoint |
| `GITLAB_HOST` | GITLAB | no | `GITLAB_HOST` (`None`) | `provider/oauth/gitlab.py:38` |
| `GITLAB_CLIENT_ID` | GITLAB | no | `GITLAB_CLIENT_ID` (`None`) | `provider/oauth/gitlab.py:30` |
| `GITLAB_CLIENT_SECRET` | GITLAB | **yes** | `GITLAB_CLIENT_SECRET` (`None`) | `provider/oauth/gitlab.py:34` |
| `ENABLE_GITLAB_SYNC` | GITLAB | no | `ENABLE_GITLAB_SYNC` (`"0"`) | `adapter/base.py:124` |
| `IS_GITEA_ENABLED` | GITEA | no | `IS_GITEA_ENABLED` (`"0"`) | instance endpoint |
| `GITEA_HOST` | GITEA | no | `GITEA_HOST` (`None`) | `provider/oauth/gitea.py:37` |
| `GITEA_CLIENT_ID` | GITEA | no | `GITEA_CLIENT_ID` (`None`) | `provider/oauth/gitea.py:29` |
| `GITEA_CLIENT_SECRET` | GITEA | **yes** | `GITEA_CLIENT_SECRET` (`None`) | `provider/oauth/gitea.py:33` |
| `ENABLE_GITEA_SYNC` | GITEA | no | `ENABLE_GITEA_SYNC` (`"0"`) | `adapter/base.py:124` |
| `ENABLE_SMTP` | SMTP | no | `ENABLE_SMTP` (`"0"`) | nothing (written by the disable-email endpoint) |
| `EMAIL_HOST` | SMTP | no | `EMAIL_HOST` (`""`) | every mail sender |
| `EMAIL_HOST_USER` | SMTP | no | `EMAIL_HOST_USER` (`""`) | every mail sender |
| `EMAIL_HOST_PASSWORD` | SMTP | **yes** | `EMAIL_HOST_PASSWORD` (`""`) | every mail sender |
| `EMAIL_PORT` | SMTP | no | `EMAIL_PORT` (`"587"`) | every mail sender (`int()`) |
| `EMAIL_FROM` | SMTP | no | `EMAIL_FROM` (`""`) | `from_email` on every mail |
| `EMAIL_USE_TLS` | SMTP | no | `EMAIL_USE_TLS` (`"1"`) | `get_connection(use_tls=…)` |
| `EMAIL_USE_SSL` | SMTP | no | `EMAIL_USE_SSL` (`"0"`) | `get_connection(use_ssl=…)` |
| `LLM_API_KEY` | AI | **yes** | `LLM_API_KEY` (`None`) | `app/views/external/base.py:85` |
| `LLM_PROVIDER` | AI | no | `LLM_PROVIDER` (`"openai"`) | `app/views/external/base.py:89` |
| `LLM_MODEL` | AI | no | `LLM_MODEL` (`"gpt-4o-mini"`) | `app/views/external/base.py:93` |
| `GPT_ENGINE` | AI | no | `GPT_ENGINE` (`"gpt-3.5-turbo"`) | nothing (deprecated) |
| `UNSPLASH_ACCESS_KEY` | UNSPLASH | **yes** | `UNSPLASH_ACCESS_KEY` (`""`) | `app/views/external/base.py:221`, instance endpoint |

Two keys are *looked up* through `get_configuration_value` but are **not**
seeded: `GITHUB_APP_NAME` and `SLACK_CLIENT_ID` (both in
`license/api/views/instance.py`). They fall back to the environment/`""`/`None`
unless an operator inserts rows by hand. Port the lookup, not a seed.

A fourth kind of value exists: `FILE_SIZE_LIMIT` is read from the environment
directly (not through the DB) by the instance endpoint, so it can never be
overridden from the admin UI.

### 3.3 Seeding: `manage.py configure_instance`

`plane/license/management/commands/configure_instance.py`. Run by the **api**
entrypoint only (`bin/docker-entrypoint-api.sh`, `docker-entrypoint-api-local.sh`),
after `register_instance`.

```
for key in ["SECRET_KEY"]:
    if not os.environ.get(key):            # empty string also fails
        raise CommandError(f"{key} env variable is required.")

for item in instance_config_variables:     # the 36 keys, in list order
    obj, created = InstanceConfiguration.objects.get_or_create(key=item["key"])
    if created:
        obj.category     = item["category"]
        obj.is_encrypted = item.get("is_encrypted", False)
        obj.value        = encrypt_data(item["value"]) if obj.is_encrypted else item["value"]
        obj.save()
        print("loaded with value from environment variable")
    else:
        print("configuration already exists")     # WARNING; nothing is written
```

Facts the Rust port must reproduce:

- **Seed-only.** An existing row is never updated, not even `category`,
  `is_encrypted` or `updated_at`. Environment changes after the first boot have
  no effect on these keys until an operator edits the row through
  `PATCH /api/instances/configurations/` (§3.6).
- One `SELECT` + one `INSERT`/no-op per key, 36 times. The Rust equivalent can
  do a single `INSERT ... ON CONFLICT (key) DO NOTHING`, but note the
  soft-delete interaction below.
- `SECRET_KEY` is mandatory: an empty or missing value aborts the api
  entrypoint before the server starts.
- Seeding writes the *raw* environment value, which may be `None` (for
  `GOOGLE_CLIENT_ID` and friends). A NULL value is a legitimate seed result and
  is later returned as `NULL` (not the default) by the reader (§3.4).
- Encrypted keys go through `encrypt_data`, so a `None` value becomes `""` (see
  §3.5).

### 3.4 Reading: `get_configuration_value`

`plane/license/utils/instance_value.py:17`. This is the only reader; 60
references across 28 files use it (or `get_email_configuration`, which wraps it).

```
get_configuration_value(keys):            # keys: [{"key": str, "default": Any}, ...]
    out = []
    if settings.SKIP_ENV_VAR:             # default True
        rows = SELECT key, value, is_encrypted FROM instance_configurations
               WHERE deleted_at IS NULL   # evaluated per key in Python, not SQL
        for k in keys:
            for row in rows:
                if row.key == k["key"]:
                    if row.is_encrypted:
                        out.append(decrypt_data(row.value))
                    else:
                        out.append(row.value)
                    break
            else:
                out.append(k["default"])
    else:
        for k in keys:
            out.append(os.environ.get(k["key"], k["default"]))
    return tuple(out)
```

Precedence rules, in order of importance:

1. `SKIP_ENV_VAR=1` (the default) → the **database value wins whenever a row
   exists**, even when the row's value is `NULL` or `""`. The `default`
   argument is used *only* when no row exists for the key.
2. `SKIP_ENV_VAR` set to anything else → the environment wins; a set-but-empty
   environment variable wins over the default (`os.environ.get` returns `""`).
3. The default is whatever the call site passed — and those differ per call
   site (§5, e.g. `ENABLE_SIGNUP` `"1"` vs `"0"`, `GITLAB_HOST` `None` vs
   `"https://gitlab.com"`, `LLM_MODEL` `"gpt-4o-mini"` vs `None`). The Rust
   reader must therefore take the default from the caller, not from a central
   table, or keep a per-call-site default map.
4. Order and count of the returned tuple follow the input list. A key repeated
   in one call returns the same value twice.
5. Values are returned **unparsed and untrimmed** (`"1 "` does not equal `"1"`).
   Each caller applies its own `== "1"` / `int()` / `bool()`.
6. Encryption failures degrade to `""` (§3.5), which is falsy for every caller —
   e.g. a `GOOGLE_CLIENT_SECRET` that cannot be decrypted makes Google OAuth
   report "not configured" rather than error.

Performance note for the Rust port: the Python function fetches the whole table
once per call and scans it in Python. It is called several times per request
(instance config endpoint, every auth flow, every email send). Cache the table
in memory (invalidate on `PATCH` and `DELETE /api/instances/configurations/`)
and keep a per-key row map; do not issue one query per key.

### 3.5 Encryption of secret values

`plane/license/utils/encryption.py` — the whole scheme:

```
derive_key(secret_key):
    dk = PBKDF2-HMAC-SHA256(password = secret_key.encode("utf-8"),
                            salt     = b"salt",          # literal 4 ASCII bytes
                            iterations = 100000,
                            dklen    = 32)
    return base64.urlsafe_b64encode(dk)                  # 44 chars, with '=' padding

encrypt_data(data):
    if not data: return ""                               # None or "" -> ""
    f = Fernet(derive_key(settings.SECRET_KEY))
    return f.encrypt(data.encode("utf-8")).decode("ascii")

decrypt_data(token):
    if not token: return ""                              # None or "" -> ""
    f = Fernet(derive_key(settings.SECRET_KEY))
    try:
        return f.decrypt(token.encode("ascii")).decode("utf-8")
    except Exception:                                    # InvalidToken, bad base64, bad UTF-8, ...
        log_exception(e)                                 # logger "plane.exception"
        return ""
```

Details that matter for a compatible Rust implementation:

- **Fernet**, not a raw AES scheme (`cryptography==50.0.0`, which follows the
  Fernet spec). Key = the 32 decoded PBKDF2 bytes: the **first 16 are the HMAC
  key**, the **last 16 are the AES key**.
- Token = `urlsafe_base64( 0x80 || timestamp_u64_be || IV(16) || ciphertext || HMAC(32) )`
  with standard `=` padding. `timestamp` is `int(time.time())` (seconds, UTC,
  big-endian). IV is 16 random bytes. Ciphertext is AES-128-CBC with PKCS7
  padding. `HMAC` is the full 32-byte HMAC-SHA256 over the preceding bytes
  (version, timestamp, IV, ciphertext); 32 bytes is the whole digest, so nothing
  is truncated in practice.
- **Decryption does not enforce freshness**: `decrypt()` is called with `ttl =
  None`, so a token with any timestamp is accepted. Do not add a TTL.
- Failure modes that must yield `""` rather than an error: `InvalidToken`
  (wrong key/HMAC, bad padding, wrong version byte, short token), invalid
  base64, non-UTF-8 plaintext, missing `SECRET_KEY` (which in Python is a random
  key, i.e. guaranteed failure).
- `log_exception` logs at ERROR via the `plane.exception` logger, which in
  production also writes `logs/plane-error.log`. The Rust port should log a
  warning/error with the config key but must return `""`.
- 100 000 PBKDF2 iterations cost a few milliseconds. Python pays it on *every*
  encrypt/decrypt call. Derive the key once at startup in Rust and cache it.
- The key depends on `SECRET_KEY` exactly as Django resolved it. If a
  deployment runs without `SECRET_KEY`, each Python process uses a different
  random key and stored secrets are permanently unreadable. `configure_instance`
  refuses to run in that case, so this only bites when a value was seeded by one
  process and read by another with a different key. Rust must therefore load
  `SECRET_KEY` the same way (`or get_random_secret_key()` is *not* acceptable —
  make it required, and log the same critical warnings for the two placeholder
  values).

**Worked example / test vector.** Generated by
`apps/server/tools/inventory/encryption_vector.py`, which calls the repository's
own `encrypt_data`/`decrypt_data` with `time.time` and `os.urandom` patched so
the token is deterministic (the script also prints one real token):

```
secret_key   = "plane-rs-fernet-vector-not-a-real-secret"
plaintext    = "postgres://plane:plane@plane-db:5432/plane"
timestamp    = 1700000000
iv_hex       = 000102030405060708090a0b0c0d0e0f

PBKDF2-HMAC-SHA256(secret_key, b"salt", 100000, 32)
             = 568e01f6db3be234edee8749b1b45bf8d7fb18327c7979707948a9fc749d53a4
fernet_key   = "Vo4B9ts74jTt7odJsbRb-Nf7GDJ8eXlweUip_HSdU6Q="
hmac_key     = 568e01f6db3be234edee8749b1b45bf8
aes_key      = d7fb18327c7979707948a9fc749d53a4

token        = "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD_SnVf1mrlHhHb4c5e-aA8GMdLq0PUT_Pk9CZUuJwCqdkAkvnySvjAzQmocDM0BPwT_NuJmtB5ybzGqAEwvR6aTjQMEBapMa1RrzA6bVtWdw"
```

Decoding the token gives 105 bytes, field by field (this is also the layout a
Rust test should assert against):

```
version      1 byte   80
timestamp    8 bytes  000000006553f100              = 1700000000 (big-endian u64)
IV          16 bytes  000102030405060708090a0b0c0d0e0f
ciphertext  48 bytes  f4a755fd66ae51e11dbe1ce5ef9a03c18c74bab43d44ff3e4f42654b89c02a9d90092f9f24af8c0cd09a870333404fc1
HMAC-SHA256 32 bytes  3fcdb899ad079c9bcc6a80130bd1e9a4e340c1016a931ad51af303a6d5b56770
```

(Concatenating those five fields and base64-url-encoding reproduces the token
above byte for byte; the HMAC verifies with the first 16 bytes of the PBKDF2
output as the key. Both were re-checked independently of the repository code.)

`decrypt_data(token)` with the same `SECRET_KEY` returns the plaintext; with a
different `SECRET_KEY`, and after flipping the last four base64 characters, it
returns `""` (exceptions swallowed). The Rust test should assert both the
decrypt of this exact token and the encrypt-with-fixed-IV reproduction.

### 3.6 Writing: the admin endpoints

`plane/license/api/views/configuration.py`, mounted at `/api/instances/`.

- `GET /configurations/` — all non-deleted rows, ordered `-created_at`,
  serialised with every model field (`id`, `created_at`, `updated_at`,
  `created_by`, `updated_by`, `deleted_at`, `key`, `value`, `category`,
  `is_encrypted`). `value` is **decrypted** for `is_encrypted` rows when
  `value is not None` (decryption failure → `""`). Response is cached for
  2 hours (`cache_response(60*60*2, user=False)`).
- `PATCH /configurations/` — `InstanceConfiguration.objects.filter(key__in =
  request.data.keys())`; **existing rows only, no creation**. For each row:
  `value = "" if request.data[key] is None else str(request.data[key]).strip()`,
  encrypted again when `is_encrypted`. Then `bulk_update(["value"])`. Invalidates
  the caches for `/api/instances/configurations/` and `/api/instances/`.
  `category` and `is_encrypted` cannot be changed over the API.
- `DELETE /configurations/disable-email-feature/` — sets `ENABLE_SMTP="0"` and
  blanks the values of `EMAIL_HOST`, `EMAIL_HOST_USER`, `EMAIL_HOST_PASSWORD`,
  `EMAIL_PORT`, `EMAIL_FROM` (raw `""`, even for the encrypted
  `EMAIL_HOST_PASSWORD` row, which then decrypts to `""`).

Any Rust write path that updates configuration must invalidate the same caches
(and, once Rust caches the table, its own cache).

### 3.7 Edge cases and suspected upstream bugs (ported as-is)

- **Soft-deleted keys break re-seeding.** The reader ignores rows with
  `deleted_at` set; `get_or_create` also ignores them, but the unique index on
  `key` does not, so `configure_instance` raises `IntegrityError` after any
  soft delete of a seeded key. There is no API that soft-deletes these rows, so
  this needs manual SQL to hit.
- **`ENABLE_MAGIC_LINK_LOGIN` seed default is `"0"` while every reader defaults
  to `"1"`.** A fresh install therefore has magic links disabled even though
  the code's fallback says enabled (`core.py:23` vs `magic_code.py:62`).
- **`ENABLE_SIGNUP` seed/reader defaults `"1"`, instance endpoint `"0"`.** The
  `/api/instances/` payload disagrees with actual signup behaviour in the
  pre-seed state only; after seeding both read the same row, so this is
  unobservable in practice but must be kept for byte-identical responses on an
  unseeded DB.
- **`EMAIL_FROM` seed default is `""`**, so mails from a fresh install use
  Django's `webmaster@localhost`, not the `Team Plane <team@mailer.plane.so>`
  default of `get_email_configuration`.
- **`GITLAB_HOST` seed default is `None`** while the provider defaults to
  `https://gitlab.com`; after seeding, GitLab OAuth reports "not configured"
  unless the operator sets a host.
- **`ENABLE_SMTP` and `GPT_ENGINE` are written but never read.** SMTP is
  considered configured iff `EMAIL_HOST` is non-empty.
- **The reader queries the whole table for every call**, and the instance
  endpoint's GET response is cached for two hours, so a PATCH can return stale
  data from `/api/instances/` if the cache invalidation path is missed.

---

## 4. Proxy and deployment variables

Not read by Python, but they configure how traffic reaches the API and the
proxy's own body limit. Listed so the Rust deployment stays compatible.

**`apps/proxy/Caddyfile.ce`** (Caddy substitutes `{$VAR}` and `{$VAR:default}`):

| Variable | Default in Caddyfile | Effect relevant to the API |
| --- | --- | --- |
| `FILE_SIZE_LIMIT` | none (compose always sets it) | `request_body { max_size {$FILE_SIZE_LIMIT} }` — the proxy rejects bodies above this before they reach `plane-api:8000`. Must equal the API's `FILE_SIZE_LIMIT`. |
| `BUCKET_NAME` | none | Routes `/{BUCKET_NAME}/*` to `plane-minio:9000` so presigned MinIO URLs work. Must equal `AWS_S3_BUCKET_NAME`. |
| `SITE_ADDRESS` | none | Caddy site block address (e.g. `:80`, `example.com`). |
| `CERT_EMAIL` | empty | ACME account email. |
| `CERT_ACME_CA` | `https://acme-v02.api.letsencrypt.org/directory` | ACME directory. |
| `CERT_ACME_DNS` | empty | DNS challenge provider line. |
| `TRUSTED_PROXIES` | `0.0.0.0/0` | Which peers may set `X-Forwarded-For`/`X-Real-IP`; Caddy then sets `client_ip_headers X-Forwarded-For X-Real-IP`. This is what makes `NUM_PROXIES` meaningful. |

Routing fixed in the Caddyfile: `/api/*`, `/auth/*`, `/static/*` →
`plane-api:8000`; `/spaces/*` → `space:3000`; `/god-mode/*` → `admin:3000`;
`/live/*` → `live:3000`; `/{BUCKET_NAME}*` → `plane-minio:9000`; everything
else → `web:3000`. `max_header_size 25MB`. The AIO Caddyfile
(`Caddyfile.aio.ce`) proxies the same three prefixes to `localhost:3004` and
gained the `/static/*` handle at cutover (it never had one, so invitation-email
logos used to 404).

**`docker-compose.yml`** (repo root): `FILE_SIZE_LIMIT` and `BUCKET_NAME` are
the only variables the proxy service gets; `plane-minio` takes
`AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` as `MINIO_ROOT_USER`/`_PASSWORD`;
`plane-db` takes `POSTGRES_USER`/`_DB`/`_PASSWORD` and `PGDATA`;
`plane-api`/`plane-worker`/`plane-beat`/`migrator` all load `apps/api/.env` via
`env_file` and wait for the migrator's `service_completed_successfully`. There
is no `plane-mq` service and no `rabbitmq_data` volume.

**`deployments/cli/community/docker-compose.yml`** (the published stack) is
where the defaults live. Relevant groups:

- api roles (`plane-api`, `plane-worker`, `plane-beat`, `migrator`): `WEB_URL` (`http://localhost`),
  `DEBUG` (`0`), `CORS_ALLOWED_ORIGINS`, `USE_MINIO` (`1`),
  `DATABASE_URL` (`postgresql://plane:plane@plane-db/plane`), `SECRET_KEY`,
  `API_KEY_RATE_LIMIT` (`60/minute`), `AUTHENTICATION_RATE_LIMIT` (`10/minute`),
  `NUM_PROXIES` (`""`), `MINIO_ENDPOINT_SSL` (`0`), `LIVE_SERVER_SECRET_KEY`,
  `WEBHOOK_ALLOWED_IPS` (`""`), `WEBHOOK_ALLOWED_HOSTS` (`""`), the job-queue
  group (`PLANE_RS_JOB_CONCURRENCY` `4`, `PLANE_RS_JOB_LEASE_SECONDS` `60`,
  `PLANE_RS_JOB_DRAIN_SECONDS` `30`, `PLANE_RS_JOB_POLL_MS` `1000`, all §6),
  plus the DB, Redis, MinIO and S3 groups. `plane-worker` gets the same
  environment as `plane-api`: it decrypts the instance configuration with
  `SECRET_KEY` and the task handlers read the database, Redis, S3, `WEB_URL`,
  `LIVE_URL` and the webhook allowlists.
- DB group: `PGHOST`, `PGDATABASE`, `POSTGRES_USER`, `POSTGRES_PASSWORD`,
  `POSTGRES_DB`, `POSTGRES_PORT` (`5432`), `PGDATA`. (`PGHOST`/`PGDATABASE` are
  libpq variables; Django only reads the `POSTGRES_*` ones and `DATABASE_URL`.)
- Redis group: `REDIS_HOST`, `REDIS_PORT`, `REDIS_URL` (`redis://plane-redis:6379/`),
  `REDIS_SSL_CERT_REQS` (`required`), `REDIS_SSL_CA_CERTS` (`""`).
- MinIO/S3 group: `AWS_REGION` (`""`), `AWS_ACCESS_KEY_ID` (`access-key`),
  `AWS_SECRET_ACCESS_KEY` (`secret-key`), `AWS_S3_ENDPOINT_URL`
  (`http://plane-minio:9000`), `AWS_S3_BUCKET_NAME` (`uploads`).
- `deployments/cli/community/variables.env` and
  `deployments/aio/community/variables.env` also define `APP_DOMAIN`,
  `APP_RELEASE`, `WEB_REPLICAS`, `SPACE_REPLICAS`, `ADMIN_REPLICAS`,
  `API_REPLICAS`, `WORKER_REPLICAS`, `BEAT_WORKER_REPLICAS`, `LIVE_REPLICAS`,
  `LISTEN_HTTP_PORT`, `LISTEN_HTTPS_PORT`, `API_BASE_URL`,
  `LIVE_SERVER_SECRET_KEY` — deployment tooling, not API configuration.
- `deployments/aio/community/variables.env` additionally has `BUCKET_NAME` (the
  all-in-one proxy uses it) and `USE_MINIO=0` by default.

`apps/api/.env.example` and `.env.example` (repo root) are the developer-facing
lists; they define no default that the code does not already have, except
`GUNICORN_WORKERS` (`2`) and the `ADMIN_/SPACE_/APP_/LIVE_BASE_*` example URLs.
Both files still carry names that **nothing reads** and that the port should
keep accepting silently: `DOCKERIZED`, `OPENAI_API_KEY`, `OPENAI_API_BASE`
(root `.env.example` marks all three deprecated), plus `REDIS_HOST`,
`REDIS_PORT` and `RABBITMQ_*`-adjacent tooling names in the root file that are
only used for compose variable interpolation. `LIVE_SERVER_SECRET_KEY` is read
by `apps/live` (Hocuspocus auth middleware), not by the API.

### 4.1 Removal at the Phase 8 cutover

The shipped deployments (`docker-compose.yml`, `deployments/cli/community/`,
`deployments/aio/community/`) no longer set these names:

| Removed name(s) | Why | What replaces it |
| --- | --- | --- |
| `AMQP_URL`, `RABBITMQ_HOST`, `RABBITMQ_PORT`, `RABBITMQ_USER`, `RABBITMQ_PASSWORD`, `RABBITMQ_VHOST` | The broker is gone (`plans/CUTOVER.md` §2). | The Postgres-backed `plane_jobs` queue. `plane-core` still parses the six names (Django is the specification) but nothing publishes to Celery after cutover. |
| `CELERY_BROKER_URL`, `CELERY_TIMEZONE`, `CELERY_TASK_SERIALIZER`, `CELERY_RESULT_SERIALIZER`, `CELERY_ACCEPT_CONTENT` | Derived Celery settings (`settings/common.py:354-387`), never environment inputs. | No Rust equivalent; §2.4 documents the historical derivation only. |
| `GUNICORN_WORKERS` | There is no gunicorn. | `PLANE_RS_WORKER_THREADS` (§6) sizes the tokio runtime; unset keeps one thread per core. |
| `METRICS_PUSH_INTERVAL_MINUTES` | Shipped deployments no longer set it, but the task and schedule are now Rust-owned (`push_instance_metrics`, D34 amendment) and the loader still reads it. | `plane_jobs.schedules`' `push-instance-metrics` `interval_seconds` (60 × the value; default 360). |

Everything else keeps its name and default. `INSTANCE_CHANGELOG_URL` is baked
into `Dockerfile.plane-api` as `https://sites.plane.so/pages/691ef037bcfe416a902e48cb55f59891/`,
the value `Dockerfile.api:10` gave the Python image; a deployment can still
override it. The worker images read the same environment file as the api image.

---

## 5. Variables read with differing defaults or parsers

The Rust config must reproduce each call site. File:line from
`env_vars.json`; "effective difference" says whether the difference is
observable.

| Variable | Call sites and defaults | Effective difference |
| --- | --- | --- |
| `ENABLE_MAGIC_LINK_LOGIN` | `utils/instance_config_variables/core.py:23` `"0"`; `authentication/provider/credentials/magic_code.py:62` `"1"`; `authentication/views/app/check.py:49` `"1"`; `authentication/views/space/check.py:49` `"1"`; `license/api/views/instance.py:97` `"1"` | **Yes, before the first seed.** Every reader falls back to `"1"` (enabled) while `configure_instance` would write `"0"` (disabled). A worker started against a database the api has not yet seeded therefore enables magic links that a later seed disables. |
| `ENABLE_SIGNUP` | `utils/instance_config_variables/core.py:11` `"1"`; `authentication/adapter/base.py:104` `"1"`; `license/api/views/instance.py:68` `"0"` | **Yes, on an unseeded DB.** The instance endpoint reports signup disabled while the actual gate allows it. |
| `ENABLE_EMAIL_PASSWORD` | `core.py:17` `"1"`; `license/api/views/instance.py:101` `"1"`; `authentication/provider/credentials/email.py:30` *no default* (`None`) | Only differs from the others when a caller passes no default: `None != "0"`, so auth stays enabled — same outcome as `"1"`. |
| `EMAIL_FROM` | `license/utils/instance_value.py:56` `"Team Plane <team@mailer.plane.so>"`; `utils/instance_config_variables/core.py:198` `""` | **Yes.** Seeding stores `""`, which shadows the nicer default → mail is sent from `webmaster@localhost`. |
| `EMAIL_HOST` | `core.py:174` `""`; `instance_value.py:45` no default; `magic_code.py:59` no default; `views/app/check.py:46` `""`; `views/space/check.py:46` `""`; `views/app/password_management.py:62` no default; `views/space/password_management.py:64` no default; `license/api/views/instance.py:94` `""` | No observable difference (`bool()` / `not` of `None` and `""` agree). |
| `EMAIL_HOST_USER` | `core.py:180` `""`; `instance_value.py:46` no default; `views/space/password_management.py:67` no default | No observable difference. |
| `EMAIL_HOST_PASSWORD` | `core.py:186` `""`; `instance_value.py:49` no default; `views/space/password_management.py:71` no default | No observable difference. |
| `EMAIL_PORT` | `instance_value.py:51` `587` (int); `core.py:192` `"587"` (string) | No observable difference: when no row exists the fallback is the int `587` (or the env string), and every call site applies `int()`. A `None` default would crash `int(None)`, so the two defaults are not interchangeable in the Rust port — keep the int fallback. |
| `GITLAB_HOST` | `provider/oauth/gitlab.py:38` `"https://gitlab.com"`; `core.py:108` no default (`None`) | **Yes.** After seeding with `GITLAB_HOST` unset the row value is `NULL` → provider sees `None` → `GITLAB_NOT_CONFIGURED`, whereas the unseeded fallback would have used gitlab.com. |
| `LLM_MODEL` | `app/views/external/base.py:93` no default (`None` → provider's `default_model`); `core.py:231` `"gpt-4o-mini"` | **Yes.** Seeding pins `gpt-4o-mini`; an operator who later clears the value gets `None` → provider default (which may differ per provider). |
| `LLM_API_KEY` | `app/views/external/base.py:85` `None`; `license/api/views/instance.py:113` `""`; `core.py:219` no default | No observable difference (`bool()`); encrypted seed of `None` becomes `""`. |
| `UNSPLASH_ACCESS_KEY` | `settings/common.py:392` no default; `app/views/external/base.py:221` no default; `license/api/views/instance.py:109` `""`; `core.py:247` `""` | No observable difference. |
| `AWS_S3_BUCKET_NAME` | `settings/common.py:343` `"uploads"`; `settings/storage.py:34` no default; `db/management/commands/create_bucket.py:29,38` no default; `update_bucket.py:140` no default | **Yes.** `settings.AWS_STORAGE_BUCKET_NAME` is `uploads` when unset, but `S3Storage` signs requests for bucket `None` and `create_bucket`/`update_bucket` fail. Reproduce both. |
| `AWS_REGION` | `settings/common.py:344` `""`; `settings/storage.py:36` no default (`None`); `create_bucket.py:25`, `update_bucket.py:24` no default | Barely: boto3 treats `""` and `None` the same (no region). Keep both. |
| `AWS_S3_ENDPOINT_URL` / `MINIO_ENDPOINT_URL` | `common.py:348` `None` + `MINIO_ENDPOINT_URL` fallback; `storage.py:38` same expression; bucket commands read only `AWS_S3_ENDPOINT_URL` (no MinIO fallback) | **Yes for the bucket commands:** with only `MINIO_ENDPOINT_URL` set, `create_bucket` talks to AWS's default endpoint, not MinIO. |
| `USE_MINIO` | `settings/common.py:329` `int(...) == 1`; `settings/storage.py:42` `== "1"` | **Yes.** `"01"`, `" 1"`, `"+1"`, `"1\n"` are MinIO for the settings and not for `S3Storage`. |
| `WEB_URL` | `settings/common.py:350` `"http://localhost"` (MinIO custom domain); `common.py:421` no default (cookie security); `common.py:470` no default (`settings.WEB_URL = None`); `license/bgtasks/telemetry_metrics.py:128` `""` | **Yes.** `settings.WEB_URL` is `None` when unset, but the MinIO custom-domain computation uses `http://localhost` and the telemetry `domain` attribute uses `""`. Three different "unset" meanings. |
| `DEBUG` | `common.py:52` `"0"` and `int()`; `production.py:12` `0` and `int(...) == 1`; `local.py:11` `"1"` and `== "1"` | Only across settings modules: production requires an `int()`-parseable value, local accepts any non-`"1"` string as false. |
| `INSTANCE_CHANGELOG_URL` | `settings/common.py:503` `""`; `Dockerfile.api:10` sets the changelog URL as an image `ENV` | **Yes in containers:** the image default is non-empty, so "unset" never happens there. |
| `SECRET_KEY` | `settings/common.py:33` `os.environ.get(...) or get_random_secret_key()`; `license/management/commands/configure_instance.py:25` `os.environ.get("SECRET_KEY")` truthiness | Compatible, but note: settings accepts empty (random), `configure_instance` refuses empty. |
| `APP_VERSION` | `register_instance.py:29` `False`; `:30` no default (second read inside the same `if`) | No difference (both falsy). |
| `FILE_SIZE_LIMIT` | `settings/common.py:389,403` `int(...)` default `5242880`; `license/api/views/instance.py:142` `float(...)` default `5242880` | Same default, different conversion: the API payload is a JSON float (`5242880.0`), the setting an int. |
| `APP_BASE_URL` | `settings/common.py:421` no default (cookie security, raw env); `common.py:456` `None` + validation | **Yes, subtly:** cookies can be `Secure` from a `WEB_URL`/`APP_BASE_URL` that the settings then reject as invalid (validation is not applied for the cookie decision). |
| `SESSION_COOKIE_AGE`, `ADMIN_SESSION_COOKIE_AGE`, `SIGNED_URL_EXPIRATION`, `HARD_DELETE_AFTER_DAYS`, `NUM_PROXIES`, `METRICS_PUSH_INTERVAL_MINUTES`, `UNUPLOADED_ASSET_DELETE_DAYS` | single call site each | No divergence, but all use `int()` with **no** error handling, unlike the `_retention_days` helper — replicate that split. |

---

## 6. Variables the Rust services will need that Django does not have

Added in Phase 1 (`plane-api`). Every name is prefixed `PLANE_RS_`; unset means
"use the default", so an existing deployment's environment keeps working
unchanged.

| Variable | Default | Parsing | Binaries | Why no existing name works |
| --- | --- | --- | --- | --- |
| `PLANE_RS_BIND` | `0.0.0.0:${PORT:-8000}` | A `host:port` parsed into a `SocketAddr`. Invalid → startup error naming the variable. When unset, `PORT` is read with shell `${PORT:-8000}` semantics (unset and empty both give 8000) and the host is `0.0.0.0`. | `plane-api` (and the other HTTP binaries later) | Django's bind address is gunicorn's `--bind 0.0.0.0:${PORT:-8000}` (`apps/api/bin/docker-entrypoint-api.sh:38`), not a setting. Reusing `PORT` alone would fix the host to `0.0.0.0`, and `GUNICORN_WORKERS` is a process count, not an address. |
| `PLANE_RS_WORKER_THREADS` | unset (tokio's default: one per available core) | Python `int()` semantics (`ENV.md` §0); `0` means automatic; `1`–`4096` accepted; anything else is a startup error. | `plane-api` | `GUNICORN_WORKERS` counts gunicorn *processes*, each with its own event loop, memory and connection pool. tokio has one runtime with N worker threads; mapping the variables would change memory use and the number of database connections, not just the concurrency. |
| `PLANE_RS_DB_POOL_SIZE` | `16` | Positive integer, at most 1000; `0` and negatives are startup errors. | `plane-api` (and any later role that opens pools) | Django has no pool-size setting: it opens one connection per thread with `CONN_MAX_AGE=0` and closes it at the end of the request. `deadpool-postgres` needs an explicit maximum, and 16 covers one connection per core plus headroom without the ~760 MiB of Python workers. |
| `PLANE_RS_LOG_FORMAT` | `json` | One of `json`, `pretty`, `compact` (exact, lower-case). Anything else is a startup error naming the variable. | all `plane-*` binaries | Django's log format is hard-coded per settings module (`plane/settings/production.py:55-67` always writes JSON). A developer running a Rust binary locally needs a readable format without introducing a second Django settings module or an unprefixed general-purpose variable such as `RUST_LOG`. |
| `PLANE_RS_JOB_CONCURRENCY` | `4` | Positive integer; invalid or zero falls back to 4. | `plane-worker` | Celery's `worker_concurrency` defaults to the CPU count and has no environment variable in Plane; the Postgres worker has in-flight handlers rather than prefork slots. |
| `PLANE_RS_JOB_LEASE_SECONDS` | `60` | Positive integer; invalid or zero falls back to 60. | `plane-worker` | Celery acks before running and has no visibility timeout. A leased queue must choose one; 60 s is longer than any Phase 4 handler. |
| `PLANE_RS_JOB_DRAIN_SECONDS` | `30` | Non-negative integer; invalid falls back to 30. | `plane-worker` | Celery has no graceful-drain setting; SIGTERM drops prefork children. |
| `PLANE_RS_JOB_POLL_MS` | `1000` | Positive integer, minimum 10 ms. | `plane-worker` | This is the LISTEN/NOTIFY fallback, not a broker poll; Celery has no equivalent. |
| `PLANE_RS_BEAT_POLL_MS` | `1000` | Positive integer, minimum 10 ms. | `plane-beat` | `django_celery_beat` uses `beat_max_loop_interval` (5 min default) internally; the Postgres scheduler polls its own table. |
| `PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS` | unset | Positive integer; unset disables the test entry. | `plane-beat` (test builds) | Test-only time compression for `tools/jobs/beat_exactly_once.sh`; no production equivalent. |
| `PLANE_RS_JOBS_HANDOVER_GAP_SECONDS` | `30` | Positive integer, minimum 0. | `plane-beat` | Minimum age of the previous due tick at handover; `DatabaseScheduler`'s loop is 5 s, so 30 s is 6× and proves Celery fired it. |
| `PLANE_RS_JOBS_HANDOVER_MARGIN_SECONDS` | `15` | Positive integer, minimum 1. | `plane-beat` | Minimum distance to the next due tick at handover; 15 s is 3× the 5 s loop, enough for the tracker bump to be observed. |
| `PLANE_RS_JOB_ONCE_JOBS` | `0` | Non-negative integer; `0` means all runnable jobs, otherwise the maximum `plane-worker --once` processes. | `plane-worker` (test) | `difftest task` must not execute a handler's published job as a second job of the same run; the published row is the compared effect. A deployment never passes `--once`. |
| `PLANE_RS_JOBS_RUST_TASKS` | unset (all Celery) | Comma-separated task names; whitespace trimmed; empty entries ignored. | `plane-api`, `plane-worker`, `plane-beat` (test/dev) | Phase 4 exit-check override. The production ownership table is `crates/plane-jobs/src/ownership.rs` and `TASKS.md` §10; this variable must not be used in a deployment. |
| `PLANE_RS_GITHUB_RELEASES_URL` | `https://api.github.com/repos/makeplane/plane/releases/latest` | Absolute URL; no validation beyond reqwest's (a malformed value falls back like any lookup failure). | `plane-api` boot, `plane-admin register-instance` | `register_instance` (`plane/license/management/commands/register_instance.py:42`) hard-codes GitHub; the differential harness and the boot comparison must not reach the internet. Tests point it at a local fake server. A deployment leaves it unset. |
| `PLANE_RS_DIFFTEST_METRICS_NS` | unset | Unix nanoseconds; unset (or a non-numeric value) uses the system clock. Honoured only in a `difftest`-feature build. | `plane-worker` (test) | Freezes `NumberDataPoint.time_unix_nano` so `difftest task` can compare the OTLP protobuf body byte for byte; the Django runner patches the OTel SDK's `time_ns` with the same value. A deployment never sets it. |
| `PLANE_RS_BOOT_STEPS` | `all` | Exactly `all` or `skip` (case-sensitive). Anything else, including an empty value, is a startup error naming the variable. | `plane-api` | `all` runs `register_instance`, `configure_instance`, `create_bucket` and `clear_cache` before the listener opens, like `apps/api/bin/docker-entrypoint-api.sh:24-33`. `skip` is for the differential harness only: its Django target is `dev/django-serve.sh`, which starts after the entrypoint's setup steps, so the Rust target must skip the same steps to begin from the cloned fixture (D34). The default runs every step, so a deployment cannot lose them by omission; `plane-api` logs a warning whenever it skips. Boot-step parity is proven separately by `tools/boot/compare_boot.py`. |

`difftest task` points both sides' OTLP exporter at a loopback recorder: for
HTTP with `--telemetry-url '{{http}}'` (it sets `OTLP_ENDPOINT`), for gRPC with
a case's `grpc` block (one h2c server per side, `OTEL_EXPORTER_OTLP_METRICS_INSECURE=true`).
It freezes the two values the exporter normally generates so the protobuf
bodies compare byte for byte:
`OTEL_RESOURCE_ATTRIBUTES=service.instance.id=…` and
`PLANE_RS_DIFFTEST_METRICS_NS=…` (the Django runner patches the OTel SDK's
`time_ns` with the same value). No deployment sets any of them.

`PLANE_RS_BIND`'s `PORT` fallback keeps the container contract: compose sets
`PORT=8000` and the Django entrypoint uses it; `plane-api` now does too.

`PLANE_RS_LOG_FORMAT` only changes the *logging* subscriber, never a response:
the HTTP contract does not include log lines.

Any further variable must be prefixed `PLANE_RS_` and documented here before
code reads it: name, default, parsing, which binary needs it, and why none of
the existing names above can express it. Adding an unprefixed variable, or
reusing a Django name with different semantics, is not allowed — it would
silently change the meaning of an existing deployment's environment.

---

## 7. How this document was produced and verified

**Inventory.** `apps/server/tools/inventory/env_vars.py` walks
`apps/api/plane/**/*.py` with `ast` (skipping `tests`, `migrations`,
`__pycache__`) and recognises `os.environ.get`, `os.getenv`,
`os.environ.setdefault`, `environ.get`, `os.environ[...]`, `"X" in os.environ`,
`"X" not in os.environ`, plus `$VAR`/`${VAR…}` in `apps/api/bin/*.sh` and
`ENV`/`ARG` lines in `apps/api/Dockerfile.api`. Output:
`apps/server/tools/inventory/env_vars.json` (name → every occurrence with file,
line, kind, whether a default was supplied, and the default's source text).

```
$ python3 apps/server/tools/inventory/env_vars.py
wrote apps/server/tools/inventory/env_vars.json
distinct names: 131
occurrences: 221
dynamic reads: 4
```

The four dynamic reads (a non-literal key, resolved by hand and covered above):
`authentication/adapter/base.py:132` (`os.environ.get(config_key)` — one of
`ENABLE_{GOOGLE,GITHUB,GITLAB,GITEA}_SYNC`),
`license/management/commands/configure_instance.py:25` (`os.environ.get(item)` —
`SECRET_KEY`),
`license/utils/instance_value.py:37` (`os.environ.get(key.get("key"))` — the 36
config keys),
`settings/common.py:481` (`os.environ.get(env_var)` — the three retention
variables).

**Coverage check.** `python3 apps/server/tools/inventory/env_vars.py --check-md
apps/server/ENV.md` asserts every scanned name occurs in this file as a whole
word:

```
$ python3 apps/server/tools/inventory/env_vars.py --check-md apps/server/ENV.md
wrote apps/server/tools/inventory/env_vars.json
distinct names: 131
occurrences: 221
dynamic reads: 4

env_vars.json distinct names: 131
occurrences: 221, dynamic reads: 4
documented in ENV.md: 131/131
all variables from env_vars.json appear in the document
```

The command also lists names that appear here but not in the scan; they are the
deployment/proxy/tooling names from §4 (`PGHOST`, `PGDATABASE`, `LISTEN_HTTP_PORT`,
`CERT_*`, `BUCKET_NAME`, …), derived settings (`CELERY_BROKER_URL`,
`SESSION_COOKIE_SECURE`, `CORS_ALLOW_ALL_ORIGINS`, …) and the
`*_NOT_CONFIGURED`/`*_DISABLED` error codes quoted from the Python.

**Encryption.** `apps/api/.venv/bin/python
apps/server/tools/inventory/encryption_vector.py` imports
`plane.license.utils.encryption` (with a minimal `settings.configure`) and
prints the derivation, the deterministic token, the round-trip, and the two
failure cases. The vector in §3.5 is its output.

**Python sources read for this document** (all under `apps/api/`):
`plane/settings/{common,production,local,storage,redis,test}.py`,
`plane/utils/instance_config_variables/{core,extended,__init__}.py`,
`plane/license/models/instance.py`,
`plane/license/migrations/0001_initial.py`,
`plane/license/management/commands/{configure_instance,register_instance}.py`,
`plane/license/utils/{encryption,instance_value}.py`,
`plane/license/api/views/{configuration,instance}.py`,
`plane/license/api/serializers/configuration.py`,
`plane/license/bgtasks/telemetry_metrics.py`,
`plane/utils/{otlp_endpoints,url,host,ip_address}.py`,
`plane/utils/core/dbrouters.py`, `plane/middleware/{db_routing,request_body_size}.py`,
`plane/authentication/{adapter/base,rate_limit}.py`,
`plane/authentication/provider/oauth/{google,github,gitlab,gitea}.py`,
`plane/authentication/provider/credentials/{email,magic_code}.py`,
`plane/authentication/views/{app,space}/{check,password_management}.py`,
`plane/api/rate_limit.py`, `plane/app/views/{external/base,workspace/base}.py`,
`plane/app/serializers/webhook.py`,
`plane/bgtasks/{file_asset_task,forgot_password_task,exporter_expired_task}.py`,
`plane/db/management/commands/{create_bucket,update_bucket,clear_cache,wait_for_db,wait_for_migrations}.py`,
`plane/db/mixins.py`, `plane/celery.py`, `plane/{asgi,wsgi,__init__}.py`,
`manage.py`, `bin/*.sh`, `Dockerfile.api`, `.env.example`,
`apps/proxy/Caddyfile.ce`, `docker-compose*.yml`, `deployments/**`.
