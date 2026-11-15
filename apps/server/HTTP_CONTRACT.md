# HTTP contract

Cross-cutting HTTP behaviour every Rust route must reproduce. This is the
specification the `plane-api` skeleton (tower layers, error type, JSON renderer,
extractors) is written from.

Everything here was read from the pinned source trees and then confirmed by
running Django in-process. Nothing is from memory. Where a claim could not be
confirmed without a database or Redis it says so.

## 0. Versions and how to read the citations

| Component | Version |
| --- | --- |
| Python | 3.12.13 |
| Django | 5.2.17 |
| djangorestframework | 3.18.3 |
| django-cors-headers | 4.9.0 |
| whitenoise | 6.12.0 |
| django-crum | 0.7.9 |
| django-redis | 7.0.0 |
| redis-py | 5.0.4 |
| psycopg | 3.3.4 |

Citation prefixes:

* `api:` — `apps/api/…` (the Django project; the specification).
* `py:` — `apps/api/.venv/lib/python3.12/site-packages/…` (a pinned dependency).

Example: `py:django/middleware/csrf.py:59` is
`apps/api/.venv/lib/python3.12/site-packages/django/middleware/csrf.py` line 59.

Two settings files matter. `api:plane/settings/common.py` is the base;
`api:plane/settings/production.py:8` does `from .common import *` and is the module
the containers run — `DJANGO_SETTINGS_MODULE` defaults to `plane.settings.production`
in `api:plane/wsgi.py:16`, `api:plane/asgi.py:13` and `api:manage.py:10`. `DEBUG` is
`int(os.environ["DEBUG"]) == 1` (`api:plane/settings/production.py:12`), so `DEBUG=0`
unless configured.

The probes are in `apps/server/tools/contract/`; their captured output is in
Appendix A, the two test vectors in Appendix B.

---

## 1. The middleware stack

`api:plane/settings/common.py:122-136`:

| # | Middleware | Kind |
| --- | --- | --- |
| 0 | `corsheaders.middleware.CorsMiddleware` | plain class, custom `__call__` |
| 1 | `django.middleware.security.SecurityMiddleware` | `MiddlewareMixin` |
| 2 | `whitenoise.middleware.WhiteNoiseMiddleware` | custom `__call__` |
| 3 | `plane.authentication.middleware.session.SessionMiddleware` | `MiddlewareMixin` |
| 4 | `django.middleware.common.CommonMiddleware` | `MiddlewareMixin` |
| 5 | `django.middleware.csrf.CsrfViewMiddleware` | `MiddlewareMixin` |
| 6 | `django.contrib.auth.middleware.AuthenticationMiddleware` | `MiddlewareMixin` |
| 7 | `django.middleware.clickjacking.XFrameOptionsMiddleware` | `MiddlewareMixin` |
| 8 | `crum.CurrentRequestUserMiddleware` | plain class, custom `__call__` |
| 9 | `django.middleware.gzip.GZipMiddleware` | `MiddlewareMixin` |
| 10 | `plane.middleware.request_body_size.RequestBodySizeLimitMiddleware` | plain class |
| 11 | `plane.middleware.logger.APITokenLogMiddleware` | plain class |
| 12 | `plane.middleware.logger.RequestLoggerMiddleware` | plain class |

`api:plane/settings/common.py:245` appends a 13th entry, only when
`ENABLE_READ_REPLICA=1`: `plane.middleware.db_routing.ReadReplicaRoutingMiddleware`.

`production.py` adds no middleware. It adds `scout_apm.django` to `INSTALLED_APPS`
(`api:plane/settings/production.py:17`), which is APM instrumentation and does not
change request or response semantics; it also sets
`SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")`
(`api:plane/settings/production.py:15`).

Ordering rules that the tower stack must reproduce:

1. **Request phase runs 0→12, response phase runs 12→0.** `MiddlewareMixin.__call__`
   (`py:django/utils/deprecation.py:56-70`) is
   `process_request` → next → `process_response`. `process_view` hooks are called by
   the handler after all `process_request` hooks, in the same 0→12 order.
2. **Each middleware is individually wrapped in `convert_exception_to_response`**
   (`py:django/core/handlers/base.py:61`, `py:django/core/handlers/exception.py:24-60`).
   Consequence: an exception raised inside middleware *i*'s `process_request` is
   turned into a response by the wrapper *around* middleware *i*, so middleware *i*'s
   own `process_response` is skipped, every middleware with a higher index never runs
   at all, and middlewares `0..i-1` still get to process the response. The
   `ALLOWED_HOSTS` 400 (raised in `CommonMiddleware.process_request`, #4) therefore has
   no `Content-Length` (set by #4), no `X-Frame-Options` (set by #7) and no
   `Vary: Cookie` (set by #3, but the session was never accessed because #12 never
   ran). The probe in §A.1.12 shows exactly that.
3. **`CorsMiddleware` short-circuits preflights before anything else runs**
   (`py:corsheaders/middleware.py:51-57`). A CORS preflight touches no session, no
   CSRF, no view, no database.
4. **`plane`'s three middlewares (#10-#12) sit below GZip**, i.e. closer to the view.
   On the response path #12→#10 run first and #9 GZip runs last, so the API-activity
   log records the *uncompressed* body, and the 413 that #10 can produce is still seen
   by GZip — it is 111 bytes, below the 200-byte threshold, so it is never compressed.

### 1.1 `Vary: Cookie` appears on almost everything

The `Vary` header is built by `patch_vary_headers`
(`py:django/utils/cache.py:62-84`), which appends and joins with `", "`, keeping
existing order. In practice:

* `SessionMiddleware.process_response` adds `Cookie` when `request.session.accessed`
  (`api:plane/authentication/middleware/session.py:55-56`).
* `CsrfViewMiddleware.process_response` adds `Cookie` whenever it (re)sets the CSRF
  cookie (`py:django/middleware/csrf.py:269`).
* `GZipMiddleware` adds `Accept-Encoding` for any response not shorter than 200
  bytes, **before** it checks the request's `Accept-Encoding`
  (`py:django/middleware/gzip.py:27`).
* `CorsMiddleware` adds `origin` whenever CORS is enabled for the path
  (`py:corsheaders/middleware.py:94`), which with the default
  `CORS_URLS_REGEX = r"^.*$"` is every request.

`request.session.accessed` becomes true for any request that `RequestLoggerMiddleware`
logs, because the logger reads `request.user.is_authenticated`
(`api:plane/middleware/logger.py:56-58`), which evaluates Django's `SimpleLazyObject`
and loads the session (`py:django/contrib/auth/middleware.py:40`,
`api:plane/authentication/middleware/session.py:22-27`). The only route that
skips logging is `GET /` (`api:plane/middleware/logger.py:35-36`), so `GET /` is the
one response without `Vary: Cookie`; `HEAD /` and `OPTIONS /` do get it.

DRF's `Request._not_authenticated()` replaces `request._request.user` with a plain
`AnonymousUser` (`py:rest_framework/request.py:400-410`). After that the logger no
longer touches the session. That is why `/api/v1/` responses (API-key
authentication, which returns `None` and then replaces the user) carry
`Vary: origin` but **not** `Vary: Cookie`, while `/api/` responses
(`BaseSessionAuthentication`, which reads `request._request.user` itself) carry
both. Observed in §A.1.6.

---

## 2. CORS — `corsheaders.middleware.CorsMiddleware`

Settings (`api:plane/settings/common.py:189-199`):

```python
CORS_ALLOW_CREDENTIALS = True
cors_origins_raw = os.environ.get("CORS_ALLOWED_ORIGINS", "")
cors_allowed_origins = [origin.strip() for origin in cors_origins_raw.split(",") if origin.strip()]
if cors_allowed_origins:
    CORS_ALLOWED_ORIGINS = cors_allowed_origins
else:
    CORS_ALLOW_ALL_ORIGINS = True
CORS_ALLOW_HEADERS = [*default_headers, "X-API-Key"]
```

Not set, so the package defaults apply
(`py:corsheaders/conf.py`):

| Setting | Value with Plane's config |
| --- | --- |
| `CORS_URLS_REGEX` | `r"^.*$"` (`conf.py:68`) → CORS on every path |
| `CORS_ALLOW_METHODS` | `("DELETE", "GET", "OPTIONS", "PATCH", "POST", "PUT")` (`py:corsheaders/defaults.py:14-20`) |
| `CORS_ALLOW_HEADERS` | `["accept", "authorization", "content-type", "user-agent", "x-csrftoken", "x-requested-with", "X-API-Key"]` — note the literal `x-` spellings and the capitalised `X-API-Key` |
| `CORS_PREFLIGHT_MAX_AGE` | `86400` (`conf.py:35`) |
| `CORS_EXPOSE_HEADERS` | `()` → header not sent |
| `CORS_ALLOW_PRIVATE_NETWORK` | `False` |
| `CORS_ALLOWED_ORIGIN_REGEXES` | `()` |
| `CORS_ALLOW_CREDENTIALS` | `True` |

### 2.1 Algorithm (`py:corsheaders/middleware.py`)

Request phase:

1. `_cors_enabled = re.match(CORS_URLS_REGEX, request.path_info)` (path only, no
   query string) — `middleware.py:152-155`.
2. If enabled **and** `request.method == "OPTIONS"` **and** the
   `access-control-request-method` header is present → return
   `HttpResponse(headers={"content-length": "0"})` — `middleware.py:73-79`.
   Status 200, empty body, and **`Content-Type: text/html; charset=utf-8`** because
   `HttpResponse` fills in a default content type. All other middleware is skipped.
   This happens even for paths that do not resolve (probe §A.1.3).

Response phase (`middleware.py:81-137`), applied to every response that reaches it:

```
if not enabled: return response
patch_vary_headers(response, ("origin",))          # always, even without an Origin header
origin = request.headers["origin"]
if not origin: return response
if not allow_all and origin not in allow-list and no signal: return response
Access-Control-Allow-Origin: "*"   if CORS_ALLOW_ALL_ORIGINS and not CORS_ALLOW_CREDENTIALS
                              else  the request Origin verbatim
Access-Control-Allow-Credentials: true            (since CORS_ALLOW_CREDENTIALS)
if expose headers: Access-Control-Expose-Headers: ", ".join(...)
if request.method == "OPTIONS":                   # note: method, not "was a preflight"
    Access-Control-Allow-Headers: ", ".join(CORS_ALLOW_HEADERS)
    Access-Control-Allow-Methods: ", ".join(CORS_ALLOW_METHODS)
    if CORS_PREFLIGHT_MAX_AGE: Access-Control-Max-Age: str(86400)
```

Consequences to reproduce exactly:

* With Plane's default config (`CORS_ALLOW_ALL_ORIGINS=True` **and**
  `CORS_ALLOW_CREDENTIALS=True`) the value of `Access-Control-Allow-Origin` is the
  **request's Origin echoed back**, never `*`.
* `Vary: origin` is added even when the request has no `Origin` header and even when
  the origin is rejected.
* `Access-Control-Allow-Headers`, `-Methods` and `-Max-Age` are emitted on **every**
  `OPTIONS` response that has an acceptable `Origin` — including a non-preflight
  `OPTIONS` that reached the view and returned 401 (probe §A.1.3, third block).
* A preflight from a **disallowed** origin still gets a 200 with
  `Vary: origin` and **no** `Access-Control-*` headers (§A.1.4).
* Header case: the three `access-control-*` headers are set with lower-case names
  (`middleware.py:16-24`); Django preserves the given case, so they go on the wire
  lower-case while everything else is `Title-Case`. HTTP header names are
  case-insensitive, but a byte-level differential test must normalise or match this.

---

## 3. `SecurityMiddleware`

Settings resolved with Plane's configuration
(`py:django/conf/global_settings.py:660-670`, nothing overridden in Plane):

| Setting | Value | Effect |
| --- | --- | --- |
| `SECURE_CONTENT_TYPE_NOSNIFF` | `True` | `X-Content-Type-Options: nosniff` via `headers.setdefault` (`py:django/middleware/security.py:46-47`) |
| `SECURE_REFERRER_POLICY` | `"same-origin"` | `Referrer-Policy: same-origin` via `setdefault`; a comma-separated setting is split, stripped and re-joined with `","` (`security.py:49-59`) |
| `SECURE_CROSS_ORIGIN_OPENER_POLICY` | `"same-origin"` | `Cross-Origin-Opener-Policy: same-origin` via `setdefault` (`security.py:61-65`) |
| `SECURE_HSTS_SECONDS` | `0` | no `Strict-Transport-Security` unless configured |
| `SECURE_HSTS_INCLUDE_SUBDOMAINS` | `False` | |
| `SECURE_HSTS_PRELOAD` | `False` | |
| `SECURE_SSL_REDIRECT` | `False` | no redirect (TLS terminates at Caddy) |
| `SECURE_SSL_HOST` | `None` | |
| `SECURE_REDIRECT_EXEMPT` | `[]` | |

Behaviour when an operator changes them (all confirmed in §A.1.5):

* HSTS is emitted only when `SECURE_HSTS_SECONDS != 0` **and**
  `request.is_secure()` **and** the response has no `Strict-Transport-Security`
  already. Format: `max-age=<seconds>` then `"; includeSubDomains"` then
  `"; preload"`, in that order.
* SSL redirect is `HttpResponsePermanentRedirect("https://%s%s" % (host, full_path))`
  → 301, `Content-Type: text/html; charset=utf-8`, empty body, no
  `X-Frame-Options`. The exemption regex is matched against
  `request.path.lstrip("/")`. It is a **301, not 302/308**, so a POST body is
  silently dropped.
* `request.is_secure()` uses `SECURE_PROXY_SSL_HEADER`, so the API behind Caddy sees
  `X-Forwarded-Proto: https` as secure.

The three `setdefault` calls mean an inner middleware or the view can pre-empt them,
which nothing in Plane does.

---

## 4. WhiteNoise and `/static/`

Settings (`api:plane/settings/common.py:304-307`, `:331`):

```python
STATIC_URL  = "/static/"
STATIC_ROOT = <BASE_DIR>/static-assets/collected-static   # BASE_DIR = apps/api/plane
STATICFILES_DIRS = (<BASE_DIR>/static,)
STORAGES = {"staticfiles": {"BACKEND": "whitenoise.storage.CompressedManifestStaticFilesStorage"}}
```

`WhiteNoiseMiddleware.__init__` (`py:whitenoise/middleware.py:36-114`) resolves,
with `DEBUG=0`:

| Setting | Value | Source |
| --- | --- | --- |
| `WHITENOISE_AUTOREFRESH` | `settings.DEBUG` → `False` | `middleware.py:39-42` |
| `WHITENOISE_MAX_AGE` | `60` when not `DEBUG` | `middleware.py:43-49` |
| `WHITENOISE_ALLOW_ALL_ORIGINS` | `True` | `middleware.py:50-53` |
| `WHITENOISE_CHARSET` | `"utf-8"` | `middleware.py:54-57` |
| `WHITENOISE_USE_FINDERS` | `settings.DEBUG` → `False` | `middleware.py:87-90` |
| `WHITENOISE_STATIC_PREFIX` | `"/static/"` (from `STATIC_URL`) | `middleware.py:92-100` |

At import it indexes `STATIC_ROOT`; nothing else is served. `plane/static/` (8
files: `css/style.css`, `js/script.js`, `humans.txt`, five `logos/*.png`, three of
which are zero bytes) is only used by `collectstatic`.

**Responses** (`py:whitenoise/base.py:206-237`, `py:whitenoise/responders.py:79-231`):

* `Content-Type` from `mimetypes`, with `; charset="utf-8"` (quoted — the
  `wsgiref` quoting) appended for `text/*`.
* `Cache-Control: max-age=60, public`, or
  `max-age=315360000, public, immutable` when `immutable_file_test` recognises a
  hashed filename — i.e. when `CompressedManifestStaticFilesStorage.url(name)` maps
  the un-hashed name back to the same basename (`middleware.py:164-180`).
  `FOREVER = 10 * 365 * 24 * 60 * 60 = 315360000` (`base.py:24`).
* `Access-Control-Allow-Origin: *`.
* `Last-Modified: <RFC 9110 date>` and `ETag: "<mtime_hex>-<size_hex>"`
  (`responders.py:166-183`).
* `Content-Length`.
* `Vary: Accept-Encoding` only when a `.gz`/`.br` alternative exists — which, with
  `CompressedManifestStaticFilesStorage`, it does after `collectstatic`.
* `If-None-Match`/`If-Modified-Since` → **304** with only
  `Cache-Control, Content-Location, Date, ETag, Expires, Vary`
  (`responders.py:30-38`, `185-198`).
* `Range: bytes=a-b` → **206** with `Content-Range: bytes a-b/size` and a sliced
  body; unsatisfiable → 416; other methods → **405 with `Allow: GET, HEAD`**
  (`responders.py:24-28`).
* `WhiteNoiseFileResponse` (a `FileResponse`) deliberately clears the headers Django
  would add, including `Content-Disposition` (`py:whitenoise/middleware.py:18-27`,
  `125-134`).

Because `plane/static-assets/collected-static/` does not exist in this repository,
Django currently serves nothing under `/static/`: the request falls through to the
URLconf and returns the custom 404 JSON (probe §A.1.12b). `DEBUG=1` flips
`USE_FINDERS`/`AUTOREFRESH` on, which serves straight from `plane/static/` (same
probe).

**Does `plane-api` have to serve `/static/*`?** Caddy sends `/static/*` to
`api:8000` (`apps/proxy/Caddyfile.ce:19`). No frontend in this repository requests a
`/static/...` asset (grepped `apps/web`, `apps/admin`, `apps/space`, `packages`), and
the only files involved are the eight above. So `/static/*` is either (a) served by
`plane-api` with the headers above plus the manifest-driven immutable caching, or
(b) left routed to Django. It must not be dropped: the 404-vs-200 difference is
observable. Recommendation for the skeleton: implement a `tower-http` static-file
layer on `STATIC_ROOT`/`plane/static` with `max-age=60`, `Access-Control-Allow-Origin:
*`, `ETag`, `Last-Modified`, `Vary: Accept-Encoding` and 304/206/405 handling as
above, and keep `/static/*` on Django until then.

---

## 5. Sessions

### 5.1 `plane`'s `SessionMiddleware`

`api:plane/authentication/middleware/session.py`. It is Django's stock
`SessionMiddleware` with three changes.

**Cookie selection is a substring test on the path** (`session.py:22-27`, `43-44`):

```python
if "instances" in request.path:
    session_key = request.COOKIES.get(settings.ADMIN_SESSION_COOKIE_NAME)  # "admin-session-id"
else:
    session_key = request.COOKIES.get(settings.SESSION_COOKIE_NAME)        # "session-id"
```

Not a prefix test: `/api/workspaces/acme/instances/` reads and writes
`admin-session-id` (§A.2 case 3). Reproduce the substring test.

**The admin cookie has its own Max-Age** (`session.py:62-66`): on a path containing
`instances` the cookie's `Max-Age` is `ADMIN_SESSION_COOKIE_AGE` (default 3600) and
`Expires = now + 3600`, regardless of the session's own `_session_expiry`. The
database row's `expire_date` still comes from the session data.

**Nothing else differs.** The save rule (`session.py:46-92`):

```
if session_is_empty() and the cookie was sent:
    delete_cookie(name, path, domain, samesite)      # no httponly, no secure
elif (modified or SESSION_SAVE_EVERY_REQUEST) and not empty:
    if get_expire_at_browser_close(): max_age = expires = None
    else: max_age = ADMIN_SESSION_COOKIE_AGE if admin_path else session.get_expiry_age()
          expires = http_date(time.time() + max_age)
    if response.status_code < 500:
        session.save()                # may raise UpdateError -> SessionInterrupted (500)
        set_cookie(cookie_name, session.session_key,
                   max_age=max_age, expires=expires,
                   domain=SESSION_COOKIE_DOMAIN, path=SESSION_COOKIE_PATH,
                   secure=SESSION_COOKIE_SECURE or None,
                   httponly=SESSION_COOKIE_HTTPONLY or None,
                   samesite=SESSION_COOKIE_SAMESITE)
patch_vary_headers(response, ("Cookie",)) if the cookie was deleted, or if the session
was accessed
```

`session.is_empty()` is `not session_key and not session_data`
(`py:django/contrib/sessions/backends/base.py:185-190`), so:

* a session row that no longer exists (expired, or the user signed out) sets the
  store's key back to `None` (`py:django/contrib/sessions/backends/db.py:32-42`,
  `54-56`) and the **response deletes the cookie**;
* a session that has a key but an empty payload is *re-saved*, not deleted
  (§A.2 case 7b).

Note `get_expiry_age()` returns `SESSION_COOKIE_AGE` when the payload has no
`_session_expiry`, and `int(_session_expiry - now)` when it does
(`base.py:265-293`), so an explicit `set_expiry(datetime)` produces a shrinking
`Max-Age`; `set_expiry(0)` produces a browser-close cookie with no `Max-Age`/`Expires`
at all. `SESSION_EXPIRE_AT_BROWSER_CLOSE` (default `False`) only matters when the
payload has no `_session_expiry`.

Settings (`api:plane/settings/common.py:419-432`):

| Setting | Value |
| --- | --- |
| `SESSION_ENGINE` | `plane.db.models.session` (`SessionStore` subclass of the DB backend) |
| `SESSION_COOKIE_NAME` | `session-id` (`SESSION_COOKIE_NAME` env) |
| `SESSION_COOKIE_AGE` | `604800` |
| `SESSION_COOKIE_HTTPONLY` | `True` |
| `SESSION_COOKIE_SAMESITE` | `Lax` (Django default; not overridden) |
| `SESSION_COOKIE_PATH` | `/` (Django default) |
| `SESSION_COOKIE_DOMAIN` | `COOKIE_DOMAIN` env or `None` |
| `SESSION_COOKIE_SECURE` | `_cookies_secure(WEB_URL, cors_allowed_origins, APP_BASE_URL)` — true when `WEB_URL`/`APP_BASE_URL` starts with `https://`, or when `CORS_ALLOWED_ORIGINS` is non-empty and every entry is https (`common.py:406-417`) |
| `SESSION_SAVE_EVERY_REQUEST` | `SESSION_SAVE_EVERY_REQUEST=1` env, default `False` |
| `ADMIN_SESSION_COOKIE_NAME` | `admin-session-id` |
| `ADMIN_SESSION_COOKIE_AGE` | `3600` (`ADMIN_SESSION_COOKIE_AGE` env) |

`Set-Cookie` attribute order is Python's `http.cookies.Morsel.OutputString`, which
emits `sorted(self.items())` — alphabetical by lower-case attribute name:
`comment < domain < expires < httponly < max-age < path < samesite < secure`. Observed
values (§A.2):

```
session-id=<128 chars>; expires=Sun, 22 Nov 2026 17:26:57 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
admin-session-id=<128>; expires=Sun, 15 Nov 2026 18:26:57 GMT; HttpOnly; Max-Age=3600; Path=/; SameSite=Lax
session-id=<128>; Domain=.plane.so; expires=…; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
session-id=<128>; expires=…; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax; Secure
session-id=<128>; HttpOnly; Path=/; SameSite=Lax                      # browser-close
session-id=""; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax   # delete
```

The delete cookie's value is the **quoted empty string** `""` (that is what
`SimpleCookie` does with an empty value), it has no `HttpOnly` and no `Secure`, and
`expires` is the literal `Thu, 01 Jan 1970 00:00:00 GMT`.

### 5.2 The database row

`api:plane/db/models/session.py`:

| Column | Value |
| --- | --- |
| `session_key` | `get_random_string(128, "abcdefghijklmnopqrstuvwxyz0123456789")`, retried while a row with that key exists (`session.py:35-43`) |
| `session_data` | the signed payload of §5.3 |
| `expire_date` | `now + get_expiry_age()` at save time |
| `user_id` | `session_data["_auth_user_id"]`, `CharField(max_length=50)`, nullable, indexed (`session.py:45-51`) |
| `device_info` | `session_data["device_info"]` when it is a dict, else `NULL` (`session.py:53-55`) |

`user_id` is the user UUID as a string. `device_info` after login is
`{"user_agent": …, "ip_address": …, "domain": …}`
(`api:plane/authentication/utils/login.py:21-26`).

Load is `SELECT … WHERE session_key = ? AND expire_date > now()`; a miss clears the
key (see above). Save is `INSERT` when the key is new, `UPDATE … force_update` when it
exists; a `DatabaseError` on update becomes `UpdateError`, which the middleware turns
into `SessionInterrupted` (a 500).

Login writes these keys in this order (`py:django/contrib/auth/__init__.py`, then
`user_login`): `_auth_user_id`, `backend`, `_auth_user_hash`, `_session_expiry` (only
for the admin flow, `login.py:19`), `device_info`. `request.session.save()` is called
explicitly at `login.py:27`, so a row always exists after login even if the
middleware would not have saved.

### 5.3 `session_data` encoding — byte-compatible spec

`SessionBase.encode` (`py:django/contrib/sessions/backends/base.py:122-129`):

```python
signing.dumps(session_dict,
              salt=self.key_salt,
              serializer=django.contrib.sessions.serializers.JSONSerializer,
              compress=True)
```

`key_salt` is `"django.contrib.sessions." + type(self).__qualname__`. Plane's store is
`SessionStore` at module level, so

```
key_salt = "django.contrib.sessions.SessionStore"
```

(Confirmed: `SessionStore().key_salt` in §B.1.)

The JSON serializer is Django's `JSONSerializer`
(`py:django/core/signing.py:142-152`): `json.dumps(obj, separators=(",", ":"))`
then `.encode("latin-1")`. `ensure_ascii` is **not** disabled, so every non-ASCII
character becomes `\uXXXX` (astral characters as a surrogate pair) and the byte
stream is pure ASCII. Key order is dict insertion order; **no sorting**.

`signing.dumps` builds a `TimestampSigner(key=None, salt=key_salt)`
(`signing.py:155-176`, `201-221`, `239-301`):

```
1. data     = json.dumps(dict, separators=(",", ":")).encode("latin-1")
2. z        = zlib.compress(data)                  # default level 6 / Z_DEFAULT_COMPRESSION
   is_compressed = len(z) < len(data) - 1
   payload  = z if is_compressed else data
3. b64bytes = urlsafe_b64encode(payload).rstrip(b"=")
   b64str   = ("." + b64bytes.decode()) if is_compressed else b64bytes.decode()
4. ts       = b62_encode(int(time.time()))          # alphabet 0-9A-Za-z, no padding
5. value    = b64str + ":" + ts
6. digest   = HMAC-SHA256(key = sha256(b"django.contrib.sessions.SessionStoresigner"
                                      + SECRET_KEY.encode()).digest(),
                          msg = value.encode("ascii")).digest()
   sig      = urlsafe_b64encode(digest).rstrip(b"=")
7. session_data = value + ":" + sig.decode()
```

Notes that bite:

* The HMAC key is **`sha256(salt_bytes + SECRET_KEY_bytes)`**, not `SECRET_KEY`
  itself: `salted_hmac` derives it (`py:django/utils/crypto.py:19-45`).
* The salt given to `salted_hmac` is `signer_salt + "signer"`, where `signer_salt` is
  the `salt=` passed to `Signer` — concatenated **without a separator**
  (`signing.py:225`). Hence the literal `"django.contrib.sessions.SessionStoresigner"`.
  The module docstring's `django.core.signing` example is *not* what sessions use.
* `zlib.compress` is the **zlib** container (header `0x78 0x9c` at default level), not
  gzip and not raw deflate. Rust: `flate2::write::ZlibEncoder` with
  `Compression::default()` (level 6) reproduces it; the exact byte equality of the
  compressed stream is *not* required for Django to accept it, only for a
  byte-for-byte differential test of a write.
* Base64 is URL-safe with **all trailing `=` stripped**; decoding re-pads with
  `b"=" * (-len(s) % 4)` (`signing.py:89-95`).
* Splitting is `rsplit(":", 1)` twice (`signing.py:230-237`, `286-292`), so the
  payload itself may not contain `:`. Base64url never does; the b62 timestamp
  never does.
* Verification compares against `[SECRET_KEY, *SECRET_KEY_FALLBACKS]`
  (`signing.py:234-237`); `SECRET_KEY_FALLBACKS` is `[]` in Plane.
* `max_age` is not passed by the session backend, so an old signature does **not**
  expire at the *signing* layer; expiry is enforced solely by the row's
  `expire_date`.
* Decode failures are silent: `BadSignature` logs `"Session data corrupted"` to
  `django.security.SuspiciousSession` and returns `{}`; any other exception returns
  `{}` (`base.py:131-143`). A tampered or truncated `session_data` means "anonymous",
  never an error response.

---

## 6. `CommonMiddleware`

`py:django/middleware/common.py`. Settings with Plane's config:

| Setting | Value |
| --- | --- |
| `APPEND_SLASH` | `True` (global default; not overridden) |
| `PREPEND_WWW` | `False` |
| `DISALLOWED_USER_AGENTS` | `[]` (compiled regex list, empty) |

### 6.1 `APPEND_SLASH`

Request phase (`common.py:47-60`) does nothing for the slash case; the redirect comes
from the response phase (`common.py:100-108`):

```
if response.status_code == 404 and should_redirect_with_slash(request):
    return HttpResponsePermanentRedirect(get_full_path_with_slash(request))
```

`should_redirect_with_slash` (`common.py:62-74`) requires all of:

* `APPEND_SLASH` is true;
* `request.path_info` does not end with `/`;
* `is_valid_path(path_info)` is **false** (this is checked in the response phase, i.e.
  after the 404);
* `is_valid_path(path_info + "/")` is **true**;
* the resolved view's `should_append_slash` attribute is true (absent on every Plane
  view).

So the rule is exactly: *the request 404'd, and its path plus one slash resolves*.
Plane's URLconf has no pattern without a trailing slash except `robots.txt` and the
empty string (`api:plane/web/urls.py:8`), so this fires for e.g. `/api/users/me`
(probes §A.1.2).

Response: `HttpResponsePermanentRedirect` → **301**, `Location` = the full path with a
single slash appended before the query string (`escape_leading_slashes` applied, so no
scheme-relative URL), `Content-Type: text/html; charset=utf-8`, **empty body**, no
`X-Frame-Options`, and `Content-Length` is *not* set (the redirect is returned before
the `Content-Length` line at `common.py:110-113`).

Method handling: the redirect is PROTOCOL-INDEPENDENT, so **`POST`/`PUT`/`PATCH` /
`DELETE` get a 301 and their body is discarded** — unless `DEBUG=1`, in which case
`get_full_path_with_slash` raises `RuntimeError` and the client sees Django's
technical 500 page (`common.py:76-98`; probe §A.1.2, fourth block, 74074-byte body).
The Rust port should reproduce the `DEBUG=0` behaviour (301); the `DEBUG=1` page is a
development affordance and need not be reproduced.

`PREPEND_WWW` is off, so nothing happens; if it were on it would be a 301 to
`{scheme}://www.{host}{path}` in the *request* phase.

`DISALLOWED_USER_AGENTS` is empty. If entries were configured, a matching
`User-Agent` raises `PermissionDenied("Forbidden user agent")`, which under `DEBUG=0`
renders the default 403 HTML page (see §11).

### 6.2 `Content-Length`

Response phase (`common.py:110-115`): for any non-streaming response that does not
already have `Content-Length`, set `Content-Length: str(len(response.content))`.

Consequences:

* Every ordinary response carries `Content-Length`. Streaming responses
  (`HttpResponse` subclasses with `streaming=True`, i.e. `StreamingHttpResponse` and
  `FileResponse`/WhiteNoise) do not get one from here.
* `HEAD` responses keep the length of the body they would have had: the probe shows
  `HEAD /definitely-not-a-route/` → `Content-Length: 28`, body 0 bytes (§A.1.1).
* Responses produced by a middleware above #4 (the 400 from a disallowed
  `Host`) never reach this code and therefore have **no** `Content-Length`, because
  the exception aborted #4's `__call__`.

---

## 7. CSRF

There are **two unrelated mechanisms**, and confusing them is the most likely porting
error in this area.

### 7.1 Django's `CsrfViewMiddleware` — only plain Django views

**Every DRF view is CSRF-exempt.** `APIView.as_view` returns `csrf_exempt(view)`
(`py:rest_framework/views.py:143-149`). `plane` itself never uses
`django.views.decorators.csrf.csrf_exempt` (grep: no hits), so the *only* routes that
Django's CSRF middleware checks are the `django.views.View` subclasses in
`plane/authentication/views/` — all of which are form POSTs that redirect:

| Path | View class | Base | Method |
| --- | --- | --- | --- |
| `/auth/sign-in/` | `SignInAuthEndpoint` (`api:plane/authentication/views/app/email.py:27`) | `View` | POST |
| `/auth/sign-up/` | `SignUpAuthEndpoint` (`…/email.py:140`) | `View` | POST |
| `/auth/magic-sign-in/` | `MagicSignInEndpoint` (`api:plane/authentication/views/app/magic.py:64`) | `View` | POST |
| `/auth/magic-sign-up/` | `MagicSignUpEndpoint` (`…/magic.py:136`) | `View` | POST |
| `/auth/reset-password/<uidb64>/<token>/` | `ResetPasswordEndpoint` (`…/password_management.py:99`) | `View` | POST |
| `/auth/sign-out/` | `SignOutAuthEndpoint` (`…/signout.py:16`) | `View` | POST |
| `/auth/google/`, `/auth/google/callback/`, `/auth/github/…`, `/auth/gitlab/…`, `/auth/gitea/…` | `*OauthInitiate*`, `*Callback*` | `View` | GET |
| `/auth/spaces/…` | the `*Space*` equivalents | `View` | POST/GET |

Everything else — `/api/`, `/api/v1/`, `/api/public/`, `/api/instances/`, `/auth/get-csrf-token/`,
`/auth/email-check/`, `/auth/forgot-password/`, `/auth/change-password/`, `/auth/set-password/`,
`/auth/magic-generate/` — is a DRF `APIView`, therefore exempt.

Safe methods (`GET`, `HEAD`, `OPTIONS`, `TRACE`) are always accepted
(`py:django/middleware/csrf.py:424-425`), so the OAuth initiate/callback GETs never
hit a token check.

### 7.2 The masked token

`py:django/middleware/csrf.py:44-81`:

```
CSRF_SECRET_LENGTH = 32
CSRF_TOKEN_LENGTH  = 2 * 32 = 64
CSRF_ALLOWED_CHARS = string.ascii_letters + string.digits
                   = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
```

Masking: take a fresh 32-character random `mask` over that alphabet; for each `i`,
`cipher[i] = CHARS[(CHARS.index(secret[i]) + CHARS.index(mask[i])) % 62]`; the token is
`mask + cipher`. Unmasking is the inverse with Python's negative-index wrap:
`secret[i] = CHARS[CHARS.index(token[32+i]) - CHARS.index(mask[i])]`. Not base64, not
hex, and case matters.

Acceptance (`csrf.py:130-157`): a token is accepted when its length is 32 or 64, all
characters are in the 62-character alphabet, and after unmasking (only for length 64)
it equals the secret in constant time. A 32-character token in `X-CSRFToken` is
therefore also accepted.

The cookie stores the **unmasked** 32-character secret; the JSON body of
`/auth/get-csrf-token/` and the value a client puts in the header are the **masked**
64-character form (`csrf.py:96-114`, `253-269`). The probe in §A.1.9 shows a 32-char
cookie and a 64-char JSON token in the same response.

### 7.3 `get_token` and the cookie

`GET /auth/get-csrf-token/` (`api:plane/authentication/views/common.py:29-36`) calls
`django.middleware.csrf.get_token(request)` and returns
`{"csrf_token": "<64 chars>"}`. `get_token` sets
`request.META["CSRF_COOKIE"]`/`["CSRF_COOKIE_NEEDS_UPDATE"]`, and
`CsrfViewMiddleware.process_response` then writes the cookie and `Vary: Cookie`
(`csrf.py:471-483`). `get_token` also marks the cookie for refresh even when the
client already has one, so the expiry slides on every call.

Cookie settings (`api:plane/settings/common.py:435-439`):

| Setting | Value |
| --- | --- |
| `CSRF_COOKIE_NAME` | `csrftoken` |
| `CSRF_COOKIE_AGE` | `31449600` (52 weeks) |
| `CSRF_COOKIE_PATH` | `/` |
| `CSRF_COOKIE_DOMAIN` | `COOKIE_DOMAIN` env or `None` |
| `CSRF_COOKIE_SECURE` | `= SESSION_COOKIE_SECURE` |
| `CSRF_COOKIE_HTTPONLY` | **`True`** — JavaScript cannot read the cookie; it must use the JSON endpoint |
| `CSRF_COOKIE_SAMESITE` | `Lax` (default) |
| `CSRF_HEADER_NAME` | `HTTP_X_CSRFTOKEN` → header `X-CSRFToken` |
| `CSRF_USE_SESSIONS` | `False` |
| `CSRF_TRUSTED_ORIGINS` | `= cors_allowed_origins` (the stripped `CORS_ALLOWED_ORIGINS` list, therefore **empty** in the default allow-all configuration) |
| `CSRF_FAILURE_VIEW` | `plane.authentication.views.common.csrf_failure` |

Observed `Set-Cookie` values (§A.1.8):

```
csrftoken=<32 chars>; expires=Sun, 14 Nov 2027 17:33:57 GMT; HttpOnly; Max-Age=31449600; Path=/; SameSite=Lax
csrftoken=<32>; Domain=.plane.so; expires=…; HttpOnly; Max-Age=31449600; Path=/; SameSite=Lax; Secure
csrftoken=<32>; expires=…; Max-Age=31449600; Path=/; SameSite=Lax        # CSRF_COOKIE_HTTPONLY=False
```

### 7.4 Origin and Referer checks (plain Django views, unsafe methods only)

`py:django/middleware/csrf.py:414-469`:

1. `request.csrf_processing_done` → skip.
2. The view is `csrf_exempt` → skip. (All DRF views.)
3. `GET`/`HEAD`/`OPTIONS`/`TRACE` → accept.
4. `Origin` present → `_origin_verified` (`csrf.py:271-295`): accept when
   `Origin == ("https" if request.is_secure() else "http") + "://" + request.get_host()`,
   or the Origin is in `allowed_origins_exact`, or it matches a
   `*`-subdomain pattern in `CSRF_TRUSTED_ORIGINS`. Otherwise reject with
   `"Origin checking failed - <origin> does not match any trusted origins."`
5. No `Origin` **and** `request.is_secure()` → `_check_referer`
   (`csrf.py:297-340`): missing → `"Referer checking failed - no Referer."`;
   unparseable or without scheme/host → `"… Referer is malformed."`; scheme not
   `https` → `"… Referer is insecure while host is secure."`; netloc must be a
   subdomain-match of a `CSRF_TRUSTED_ORIGINS` host, else of `CSRF_COOKIE_DOMAIN`
   (or of `request.get_host()` when the domain is unset), else
   `"… Referer is %s does not match any trusted origins."`
6. `_check_token` (`csrf.py:349-399`): cookie secret required
   (`"CSRF cookie not set."`); the token comes from `POST["csrfmiddlewaretoken"]` or
   `X-CSRFToken` (`"CSRF token missing."`); length/charset
   (`"CSRF token from the 'X-Csrftoken' HTTP header has incorrect length."` /
   `"… has invalid characters."`); mismatch →
   `"… is incorrect."`; a malformed cookie →
   `"CSRF cookie has incorrect length."` / `"… has invalid characters."`.

All reason strings are in Appendix B.2 verbatim.

### 7.5 The failure response is `200 OK` with HTML

`CSRF_FAILURE_VIEW` (`api:plane/authentication/views/common.py:39-45`) is:

```python
def csrf_failure(request, reason=""):
    return render(request, "csrf_failure.html",
                  {"reason": reason, "root_url": base_host(request=request)})
```

`render()` uses the default status **200**. `CsrfViewMiddleware._reject` returns that
response unchanged (`csrf.py:209-219`), after logging
`"Forbidden (<reason>): <path>"` to `django.security.csrf`.

So a CSRF rejection is **HTTP 200**, `Content-Type: text/html; charset=utf-8`, with
`templates/csrf_failure.html` (1732 bytes) as the body. It passes through the
remaining response middleware, so it also gets `Vary: Cookie`, `Vary: origin`,
`Vary: Accept-Encoding` (body ≥ 200 bytes), `X-Frame-Options: DENY` and, if CORS
matched, the `Access-Control-*` headers. Probe §A.1.9 has the exact output.

`base_host()` returns `settings.WEB_URL or settings.APP_BASE_URL` for the app flow,
and those are unset by default, so the rendered anchor is
`<a href="None" class="btn-primary">` — Django renders `None` as `"None"`. With
`WEB_URL` set it is that URL. Port the template verbatim, including this.

### 7.6 The CSRF check that `/api/` actually uses: `BaseSessionAuthentication`

`api:plane/authentication/session.py:56-63`:

```python
class BaseSessionAuthentication(SessionAuthentication):
    def enforce_csrf(self, request):
        if request.method in SAFE_METHODS:      # GET, HEAD, OPTIONS, TRACE
            return
        if not is_allowed_origin(request):
            raise PermissionDenied("CSRF Failed: origin not allowed")
```

No token, no cookie: **an origin check only**, and only for authenticated callers
(`SessionAuthentication.authenticate` calls `enforce_csrf` only after
`request._request.user` is authenticated, `py:rest_framework/authentication.py:117-133`).

`is_allowed_origin` (`api:plane/authentication/session.py:38-53`):

* no `Origin` header → allowed (non-browser clients such as the live server);
* `Origin` that does not parse to `scheme://host[:port]` → denied (includes the
  opaque `"null"`);
* `origin.split("://", 1)[1] == request.get_host().lower()` → allowed. Only
  host[:port] is compared, and the scheme is *not* compared, so an `https` page
  talking to the same host over `http` is accepted;
* otherwise allowed when the normalised origin is in `allowed_origins()`
  (`session.py:26-35`): `WEB_URL`, `APP_BASE_URL`, `ADMIN_BASE_URL`,
  `SPACE_BASE_URL`, `LIVE_BASE_URL`, `CORS_ALLOWED_ORIGINS`,
  `CSRF_TRUSTED_ORIGINS`, normalised to lower-case `scheme://netloc`, cached in an
  unbounded `lru_cache`.

Failure → `PermissionDenied("CSRF Failed: origin not allowed")` → **403**
`{"detail":"CSRF Failed: origin not allowed"}`, with the DRF `Allow` header.

`api:plane/app/views/base.py:74`, `:284`, `api:plane/space/views/base.py:69`, `:159`
and `api:plane/license/api/views/base.py:47` all use `BaseSessionAuthentication`, i.e.
all of `/api/`, `/api/public/`, `/api/instances/`. `/api/v1/` uses
`APIKeyAuthentication` and does no CSRF or origin check at all
(`api:plane/api/views/base.py:53`, `:165`).

---

## 8. Authentication, `crum`, `XFrameOptions`, `GZip`

### 8.1 `AuthenticationMiddleware`

`py:django/contrib/auth/middleware.py:30-41`: sets
`request.user = SimpleLazyObject(lambda: get_user(request))` and `request.auser`. It
requires `request.session` to exist. `get_user` reads
`request.session[SESSION_KEY]` and `[BACKEND_SESSION_KEY]`
(`py:django/contrib/auth/__init__.py`), and verifies `_auth_user_hash` against
`user.get_session_auth_hash()`; a mismatch or a missing user yields `AnonymousUser`.
Nothing is written to the session and no cookie is set here.

### 8.2 `crum.CurrentRequestUserMiddleware`

`py:crum/__init__.py:86-111`: sets a thread-local current request before the view and
clears it afterwards (also on exception). Plane's models read
`crum.get_current_user()` to fill `created_by`/`updated_by`. It sets no headers and
writes nothing by itself. In Rust this becomes an explicit request-scoped task-local
or an argument threaded through the write paths.

### 8.3 `XFrameOptionsMiddleware`

`py:django/middleware/clickjacking.py:25-48`: if the response has no
`X-Frame-Options` and is not `xframe_options_exempt`, set
`X-Frame-Options: <X_FRAME_OPTIONS or "DENY">.upper()`.

`X_FRAME_OPTIONS` is not configured, so the value is **`DENY`**. It appears on every
response that reaches #7 — including DRF error responses and the Django 500 page — but
not on: the CORS preflight (§2), responses produced above #7 (the `ALLOWED_HOSTS` 400,
§6.1), or `SECURE_SSL_REDIRECT` redirects (§3).

### 8.4 `GZipMiddleware`

`py:django/middleware/gzip.py:16-74`, in order:

1. If the response is not streaming **and** `len(response.content) < 200` → return
   unchanged (no `Vary`, no compression). 200 is inclusive-excluded.
2. If `Content-Encoding` is already set → return unchanged (no `Vary` either).
3. `patch_vary_headers(response, ("Accept-Encoding",))` — **unconditionally**, before
   the client's `Accept-Encoding` is examined.
4. If `re.search(r"\bgzip\b", request.META["HTTP_ACCEPT_ENCODING"])` fails → return
   (but `Vary: Accept-Encoding` has already been added).
5. Streaming: replace `streaming_content` with the gzip of each chunk, delete
   `Content-Length`.
6. Non-streaming: compress with `gzip.compress(data, compresslevel=6, mtime=0)`
   (`py:django/utils/text.py:351-353`); **if the compressed body is not shorter,
   return the original unchanged — with `Vary: Accept-Encoding` already added and no
   `Content-Encoding`**. Otherwise replace the body and set `Content-Length`.
7. If `ETag` starts with `"` → prefix `W/` (weak validator).
8. Set `Content-Encoding: gzip`.

Settings: `max_random_bytes = 100` (`gzip.py:16`). Django 5.2's `compress_string`
therefore also names the gzip member after 100 random bytes
(`text.py:355-372`: `header[3] = gzip.FNAME`, `filename = <random> + \x00`). The
compressed bytes are **not** reproducible byte-for-byte — the mtime is fixed to 0 but
the gzip `FNAME` is random. `Content-Encoding`, `Vary`, `Content-Length`,
`ETag` weakening and the "compressed must be strictly shorter" rule must match; the
payload need not. The Rust side may emit a plain gzip header, but a byte comparison of
gzipped bodies must be done after decompression.

Note that `Accept-Encoding: gzip;q=0` **does** gzip: the regex only looks for the word
`gzip`. There is no `identity;q=0` handling and no 406.

---

## 9. Plane's own middlewares

### 9.1 `RequestBodySizeLimitMiddleware`

`api:plane/middleware/request_body_size.py:18-31`:

```python
try:
    _ = request.body
except RequestDataTooBig:
    return JsonResponse({"error": "REQUEST_BODY_TOO_LARGE",
                         "detail": "The size of the request body exceeds the maximum allowed size."},
                        status=413)
return self.get_response(request)
```

`request.body` reads the whole body into memory and raises `RequestDataTooBig`
(`= SuspiciousOperation`) when the content length exceeds
`DATA_UPLOAD_MAX_MEMORY_SIZE`, which Plane sets **equal to `FILE_SIZE_LIMIT`**
(`api:plane/settings/common.py:389`, `:403`, both `int(os.environ["FILE_SIZE_LIMIT"]
or 5242880)`).

So the 413 fires before authentication, before DRF parsing, and before the API-token
log (this middleware is #10, the logger #11). Body (probe §A.1.11, 111 bytes,
`JsonResponse` → `ensure_ascii=True`, `", "`/`": "` separators):

```json
{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}
```

with `Content-Type: application/json`, `Cache-Control` unaffected, `X-Frame-Options`,
the security headers, `Vary: origin`. GZip does not compress it (111 < 200).

Two further notes:

* A body rejected mid-read by Django's `request.POST`/multipart parsing (rather than
  by `request.body`) surfaces as a plain `SuspiciousOperation` and becomes the
  **400** page of §11, not 413.
* Caddy front-runs this: `apps/proxy/Caddyfile.ce:2-4` applies
  `request_body { max_size {$FILE_SIZE_LIMIT} }` globally, so in a real deployment a
  body over the limit is rejected by Caddy before reaching the API. Both layers exist;
  the API's 413 still has to be implemented for direct connections.

`FILE_SIZE_LIMIT` is separately used by the asset presigning views
(`api:plane/app/views/asset/v2.py:155-159` and friends) and by the model validators
(`api:plane/db/models/asset.py:28`, `api:plane/db/models/issue.py:394`).

### 9.2 `APITokenLogMiddleware`

`api:plane/middleware/logger.py:80-163`. For **every** request:

1. Read `request.body` (already done by #10, so it is cached).
2. Call the view.
3. If `X-Api-Key` is present, build

```python
{
  "token_identifier": hmac.new(SECRET_KEY.encode(), api_key.encode(), sha256).hexdigest(),
  "path": request.path,                       # no query string
  "method": request.method,
  "query_params": request.META["QUERY_STRING"],
  "headers": str({k: "[REDACTED]" if k.lower() in {"x-api-key","authorization","cookie"} else v
                  for k, v in request.headers.items()}),
  "body": _safe_decode_body(request_body) if request_body else None,
  "response_body": _safe_decode_body(response.content) if response.content else None,
  "response_code": response.status_code,
  "ip_address": get_client_ip(request),
  "user_agent": request.META.get("HTTP_USER_AGENT"),
}
```

`_safe_decode_body` (`logger.py:94-114`) returns `None` for `None`/`b""`,
`"[Binary Content]"` for bodies starting `\x89PNG`, `\xff\xd8\xff` or `%PDF`, the UTF-8
text otherwise, `"[Could not decode content]"` on a `UnicodeDecodeError`.

`headers` is Python's `dict.__repr__` of the lower-cased header mapping — single
quotes, `", "` separators, and the order Django stores them in. Not JSON.

Then `process_logs.delay(log_data=log_data)` (`api:plane/bgtasks/logger_task.py:33-40`)
— a **Celery task**, not a synchronous write; the handler
`plane.bgtasks.logger_task.log_to_postgres` does
`APIActivityLog.objects.create(**log_data)`. Failures (including the broker being
unreachable) are swallowed by `except Exception: log_exception(e)`, so logging never
breaks a request.

Table `api_activity_logs` (`api:plane/db/models/api.py:49-74`): `token_identifier`
(255), `path` (255), `method` (10), `query_params` (text), `headers` (text), `body`
(text), `response_code` (positive int), `response_body` (text), `ip_address`
(`GenericIPAddressField`, nullable), `user_agent` (512), plus `BaseModel`
`created_at/updated_at/created_by/updated_by/deleted_at`. `ip_address` is
`get_client_ip`, which takes the **first** `X-Forwarded-For` entry and does *not*
consult `NUM_PROXIES` (`api:plane/utils/ip_address.py:199-205`).

Note the log is written for **any** request carrying `X-Api-Key`, whatever the
response — including 401/403/404 and any `/api/` route that ignores the header.

### 9.3 `RequestLoggerMiddleware`

`api:plane/middleware/logger.py:26-77`. Logs to the `plane.api.request` logger
(JSON via `python-json-logger`, `api:plane/settings/production.py:63-67`) unless the
request is `GET /`:

```
message: "<METHOD> <full_path_with_query> <status>"
extra:   path, method, status_code, duration_ms (int), remote_addr (get_client_ip),
         user_agent, user_id (None unless authenticated)
```

It writes no database rows and sets no headers, but it *is* what makes
`request.session.accessed` true (§1.1), so it changes the `Vary` header of nearly
every response. That side effect is part of the contract.

`log.exception`/`log.warning` from `plane.utils.exception_logger` writes to the
`plane.exception` logger, which under `DEBUG=0` also writes to
`apps/api/plane/logs/plane-error.log`.

### 9.4 `ReadReplicaRoutingMiddleware` (only with `ENABLE_READ_REPLICA=1`)

`api:plane/middleware/db_routing.py`, appended **last** (`#13`,
`api:plane/settings/common.py:245`): for `POST/PUT/PATCH/DELETE` it sets
"primary" before the view; for `GET/HEAD/OPTIONS` its `process_view` sets
"replica" iff the resolved view has `use_read_replica` truthy (checked on the
function, on `.view_class`, then on `.cls`) and always clears the flag afterwards.
With no replica configured, `DATABASE_ROUTERS` is not set either
(`common.py:228-245`), so the whole feature is inert. `ReadReplicaControlMixin`
defaults `use_read_replica = True` (`api:plane/utils/core/mixins/view.py:13-29`); the
base view classes set `False` (`api:plane/app/views/base.py:80`,
`api:plane/api/views/base.py:57`, `:169`, `api:plane/space/views/base.py`) and **59
view classes opt back in** with `use_read_replica = True` — for example
`api:plane/app/views/user/base.py:67`, `api:plane/app/views/workspace/base.py:179`,
`api:plane/app/views/project/base.py:52`, `api:plane/app/views/issue/activity.py:27`,
`api:plane/app/views/notification/base.py:202`. Those are the GET routes that will read
from a replica when an operator enables one, so the Rust equivalent needs the same
per-route opt-in list, not a global switch.

---

## 10. The DRF request/response layer

### 10.1 Renderer

`REST_FRAMEWORK["DEFAULT_RENDERER_CLASSES"] = ("rest_framework.renderers.JSONRenderer",)`
(`api:plane/settings/common.py:147`) — exactly one renderer, so:

* no content negotiation on `Accept` except that it must match `application/json`
  (`*/*` matches); a request that only accepts something else gets
  **406** `{"detail":"Could not satisfy the request Accept header."}`
  (`py:rest_framework/negotiation.py:91`);
* DRF never adds `Vary: Accept` (`py:rest_framework/views.py:163-164` adds it only
  when there is more than one renderer);
* `?format=xml` raises `Http404` inside `filter_renderers`
  (`negotiation.py:93-102`) → **404** `{"detail":"Not found."}` with the `Allow`
  header.

`JSONRenderer.render` (`py:rest_framework/renderers.py:53-107`):

```python
ensure_ascii = not UNICODE_JSON = False
compact      = COMPACT_JSON   = True
strict       = STRICT_JSON    = True
charset      = None                      # so no "; charset=" on the header
separators   = (",", ":")                # SHORT_SEPARATORS
if data is None: return b""              # and Response then deletes Content-Type
ret = json.dumps(data, cls=JSONEncoder, indent=None, ensure_ascii=False,
                 allow_nan=False, separators=(",", ":"))
ret = ret.replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
return ret.encode("utf-8")
```

Exact header value: **`Content-Type: application/json`** — no charset, no space
(`py:rest_framework/response.py:64-72`: when `renderer.charset is None` the header is
just the media type; and when the rendered body is empty the `Content-Type` header is
deleted, `response.py:82-84`).

Body is UTF-8, compact, unsorted, with raw non-ASCII bytes, `\u2028`/`\u2029` escaped,
`NaN`/`Infinity` rejected with `ValueError` (`py:rest_framework/utils/json.py:12-37`).

### 10.2 `JSONEncoder` (`py:rest_framework/utils/encoders.py:18-70`)

| Python type | Encoded as |
| --- | --- |
| `Promise` (lazy `ugettext`) | `force_str` |
| `datetime` | `isoformat()`, then a trailing `+00:00` replaced by `Z`. **All six microsecond digits are kept** (`…T12:34:56.789012Z`) |
| `date` | `isoformat()` |
| `time` | raises `ValueError` if aware, else `isoformat()` (6 digits) |
| `timedelta` | `str(obj.total_seconds())` — e.g. `90061.5` |
| `Decimal` | `float(obj)` — e.g. `1.1`, `1000.0`, `1e-06` |
| `UUID` | `str(obj)` |
| IP addresses/networks/interfaces | `str(obj)` |
| `QuerySet` | `tuple(obj)` |
| `bytes` | `obj.decode()` (UTF-8; raises on invalid) |
| objects with `tolist` | `obj.tolist()` |
| objects with `__getitem__` | `list(obj)`/`dict(obj)` |
| other iterables (generators, sets, frozensets) | `tuple(...)` |

Note `datetime` microsecond truncation is *not* applied here — that is Django's
`DjangoJSONEncoder`. DRF keeps all six digits.

### 10.3 Serializer field formatting

With `COERCE_DECIMAL_TO_STRING=True`, `DATE_FORMAT`/`DATETIME_FORMAT`/`TIME_FORMAT`
= `ISO_8601` (all DRF defaults, not overridden in Plane):

| Field | `to_representation` | Example |
| --- | --- | --- |
| `DateTimeField` | `enforce_timezone` → `isoformat()`, `+00:00` → `Z`; microseconds only when non-zero (`py:rest_framework/fields.py:1216-1232`) | `"2026-11-15T12:34:56.120000Z"`, `"2026-11-15T12:34:56Z"` |
| `DateField` | `isoformat()` (`fields.py:1278-1299`) | `"2026-11-15"` |
| `TimeField` | `isoformat()` (`fields.py:1341-1361`) | `"12:34:56.789012"` |
| `DurationField` | `duration_string()` → `"HH:MM:SS[.ffffff]"`. `DURATION_FORMAT` is `"django"`, **not** ISO 8601, unlike the three above (`py:rest_framework/settings.py:113`, `fields.py:1409-1420`) | `"02:03:00"` |
| `DecimalField` | `quantize` then `f"{quantized:f}"` — trailing zeros kept (`fields.py:1107-1145`) | `"1.1000"` |
| `FloatField` | Python float → §10.4 | `1e-05` |
| `BooleanField` | `true`/`false` | |

`enforce_timezone` converts an aware value to the *current* timezone, which
`TimezoneMixin.initial` sets to `request.user.user_timezone` for authenticated
callers (`api:plane/app/views/base.py:40-45`, `api:plane/api/views/base.py:44-49`).
So the same UTC instant can serialise with a non-UTC offset. Rust must activate the
user's timezone per request the same way (chrono `TimeZone` from the IANA name), and
serialise with the same `isoformat()` shape.

### 10.4 Floats

Both encoders use CPython's `float.__repr__` (shortest round-trip). `ryu`/`serde_json`
agrees for most values but **not** for the boundary cases: Python switches to
exponent form for `|x| < 1e-4` and `|x| >= 1e16`, writes a two-digit exponent with a
sign, and always writes a decimal point for integral values. Measured (§A.4):

| Value | Python/DRF output |
| --- | --- |
| `0.0` | `0.0` |
| `-0.0` | `-0.0` |
| `1.0` | `1.0` |
| `0.1 + 0.2` | `0.30000000000000004` |
| `1/3` | `0.3333333333333333` |
| `1e-4` | `0.0001` |
| `1e-5` | `1e-05` |
| `1.234567890123e-5` | `1.234567890123e-05` |
| `1e-7` | `1e-07` |
| `1e15` | `1000000000000000.0` |
| `1e16` | `1e+16` |
| `1234567890123456.0` | `1234567890123456.0` |
| `12345678901234567.0` | `1.2345678901234568e+16` |
| `1e21` | `1e+21` |
| `5e-324` | `5e-324` |
| `1.7976931348623157e308` | `1.7976931348623157e+308` |
| `-1.5e-9` | `-1.5e-09` |
| `100.0` | `100.0` |

`serde_json`'s default float formatter (`ryu`) writes `1e-5` as `1e-5` and `1e16` as
`1e16`, `1.0` as `1.0`. The D2 formatter in `plane-core` must implement the Python
rules above (`{:?}`-style shortest digits plus Python's exponent thresholds, `e+NN`/
`e-NN` with at least two exponent digits, and a mandatory `.0` for integral values
below the exponent threshold). `allow_nan` must be off.

### 10.5 Parsers

Default `DEFAULT_PARSER_CLASSES` (`py:rest_framework/parsers.py`, DRF defaults, not
overridden): `JSONParser`, `FormParser`, `MultiPartParser`.

Dispatch (`py:rest_framework/request.py:330-384`):

* If `Content-Length` is 0/absent → stream is `None` → **no parsing**, `request.data`
  is `{}` for a non-form content type (or an empty `QueryDict` for a form type).
* Else `select_parser` picks the first parser whose `media_type` matches the
  `Content-Type` (`py:rest_framework/negotiation.py:29-41`), refusing a
  `Content-Type` longer than 256 characters with a 415. No match → 415.
* `JSONParser` (`parsers.py:71-91`) decodes with the charset from `Content-Type`
  (default `utf-8`), rejecting non-text codecs such as `bz2`
  (`parsers.py:27-46`), then `json.load` with `parse_constant` raising on
  `NaN`/`Infinity`; any `ValueError` becomes
  `ParseError('JSON parse error - %s' % str(exc))`.
* `BaseAPIView.initial` then rejects a successfully parsed non-`dict`/non-`list` body
  on `POST`/`PUT`/`PATCH` with
  `ParseError("Request body must be a JSON object or array.")`
  (`api:plane/api/views/base.py:34-35`, `:59-62`, and `:171-174` for viewsets).

Observed messages and statuses (§A.1.13):

| Case | Status | Body |
| --- | --- | --- |
| `{"a":` | 400 | `{"detail":"JSON parse error - Expecting value: line 1 column 6 (char 5)"}` |
| empty body, `Content-Type: application/json` | 200 | parsed as `{}` (no error) |
| `null` | 400 | `{"detail":"Request body must be a JSON object or array."}` |
| `"hello"` | 400 | same |
| `{"x": NaN}` | 400 | `{"detail":"JSON parse error - Out of range float values are not JSON compliant: 'NaN'"}` |
| `Content-Type: text/plain` | 415 | `{"detail":"Unsupported media type \"text/plain\" in request."}` |
| `Content-Type: ""` with a body | 415 | `{"detail":"Unsupported media type \"\" in request."}` |
| `charset=latin-1` | 200 | parsed |
| `charset=bz2` | 400 | `{"detail":"Unsupported charset \"bz2\" in request Content-Type header."}` |

The `JSON parse error - …` suffix is CPython's `json.JSONDecodeError` message
(`"Expecting value: line L column C (char N)"`, `"Expecting ',' delimiter: …"`,
`"Extra data: …"`, `"Unterminated string starting at: …"`, `"Invalid control
character …"`, …). To be byte-identical the Rust parser must produce the same
wording; otherwise record the divergence in `TASKS.md` and let the differential
harness normalise it. Do not silently invent a different shape.

### 10.6 Exceptions

`EXCEPTION_HANDLER = "plane.authentication.adapter.exception.auth_exception_handler"`
(`api:plane/settings/common.py:149`), which is DRF's handler plus two patches
(`api:plane/authentication/adapter/exception.py:17-34`):

```python
response = rest_framework.views.exception_handler(exc, context)
if isinstance(exc, NotAuthenticated):
    response.status_code = 401                     # DRF would have used 403 here
if isinstance(exc, Throttled):
    exc = AuthenticationException(RATE_LIMIT_EXCEEDED, "RATE_LIMIT_EXCEEDED")
    response.data = exc.get_error_dict()           # {"error_code": 5900, "error_message": ...}
    response.status_code = 429
return response
```

The base handler (`py:rest_framework/views.py:72-102`) maps `Http404` →
`NotFound`, `PermissionDenied` → `PermissionDenied`, then for any `APIException`
builds `headers = {}`, adds `WWW-Authenticate` from `exc.auth_header` and
`Retry-After` from `exc.wait`, and wraps a scalar `detail` as `{"detail": …}`.

The 401-vs-403 decision happens before that, in `APIView.handle_exception`
(`views.py:453-477`):

```python
if isinstance(exc, (NotAuthenticated, AuthenticationFailed)):
    auth_header = self.get_authenticate_header(self.request)   # authenticators[0].authenticate_header(request)
    if auth_header: exc.auth_header = auth_header
    else:           exc.status_code = 403
```

Both Plane authenticators return `None` from `authenticate_header`:
`SessionAuthentication` inherits `BaseAuthentication.authenticate_header` → `pass`
(`py:rest_framework/authentication.py:44-50`), and `APIKeyAuthentication` extends
`BaseAuthentication` without overriding it
(`api:plane/api/middleware/api_authentication.py:17-52`). Therefore:

| Situation | Mechanism | Status | Body | Extra headers |
| --- | --- | --- | --- | --- |
| no credentials at all | authenticator returns `None` → `permission_denied` raises `NotAuthenticated` (`views.py:174-180`) | **401** (forced by Plane's handler) | `{"detail":"Authentication credentials were not provided."}` | none |
| bad API key | `APIKeyAuthentication` raises `AuthenticationFailed("Given API token is not valid")`; no `authenticate_header` → 403 | **403** | `{"detail":"Given API token is not valid"}` | none |
| `AuthenticationFailed` anywhere an authenticator defines `authenticate_header` (none does in Plane today; the probe uses a synthetic one) | `auth_header` truthy → keep 401 | **401** | `{"detail":"<detail>"}` | `WWW-Authenticate: <value>` |
| `IsAuthenticated` fails for an authenticated user | `PermissionDenied` | 403 | `{"detail":"You do not have permission to perform this action."}` | |
| origin check on `/api/` writes | `PermissionDenied("CSRF Failed: origin not allowed")` | 403 | `{"detail":"CSRF Failed: origin not allowed"}` | |
| `Http404` inside a DRF view | `NotFound` | 404 | `{"detail":"Not found."}` | |
| unmatched URL (no DRF) | Django `handler404` | 404 | `{"error": "Page not found."}` | (Django `JsonResponse`) |
| wrong method | `MethodNotAllowed` | 405 | `{"detail":"Method \"<METHOD>\" not allowed."}` | `Allow` |
| throttled | `Throttled`, then Plane's override | 429 | `{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}` | `Retry-After: <ceil(wait)>` |
| serializer errors | `ValidationError` | 400 | `{"field": ["This field is required."]}` | |
| bare `ValidationError("boom")` | `ValidationError` | 400 | `["boom"]` (a JSON **array**) | |
| `ParseError` | | 400 | `{"detail":"<message>"}` | |
| `UnsupportedMediaType` | | 415 | `{"detail":"Unsupported media type \"<ct>\" in request."}` | |
| unhandled exception | re-raised by DRF, caught by Django | 500 | Django's HTML 500 page (§11) | |

Note that a 429 replaces the body but keeps the headers the base handler already
computed — including `Retry-After`. `Throttled` sets `wait = math.ceil(wait)`
(`py:rest_framework/exceptions.py:233-244`), so the header is an integer number of
seconds.

Note also that the `Allow` header is present on **every** DRF response, including
401/403/404/406/429, because `APIView.dispatch` copies
`default_response_headers` onto the response (`views.py:157-165`, `448-449`).
Its contents are `[m.upper() for m in self.http_method_names if hasattr(self, m)]`
with `http_method_names = ["get","post","put","patch","delete","head","options","trace"]`,
so the order is that list's order — e.g. `Allow: GET, PATCH, DELETE, HEAD, OPTIONS`
for `/api/users/me/`, `Allow: GET, POST, HEAD, OPTIONS` for a view with both, never
alphabetical.

### 10.7 `OPTIONS` and metadata

`APIView.options` (`views.py:531-538`) returns
`Response(SimpleMetadata().determine_metadata(request, self), status=200)`
**after** the normal `initial()` pipeline, so it is authenticated and permission-checked
like any other method. Consequences:

* Anonymous `OPTIONS /api/users/me/` → **401**, not a metadata body (§A.1.3).
* Authenticated `OPTIONS` → 200 with
  `{"name": …, "description": "", "renders": ["application/json"], "parses": ["application/json","application/x-www-form-urlencoded","multipart/form-data"]}`
  and, for views with `get_serializer`, an `"actions": {"POST": {...}}` /
  `{"PUT": {...}}` object (`py:rest_framework/metadata.py:59-70`, `99-152`).
* A CORS preflight `OPTIONS` never reaches it (§2).
* `OPTIONS` on a plain Django function view runs the view itself:
  `OPTIONS /` returns 200 with `{"status": "OK"}` (§A.1.1).

`SimpleMetadata` invokes `view.get_serializer()` and `view.check_permissions` for the
`actions` block; that may hit the database. Port it per-view or omit `actions` for
views whose `OPTIONS` is not depended on — but do not return a different shape for a
view that has one today.

---

## 11. Responses produced outside DRF

### 11.1 `404` — `handler404`

`api:plane/urls.py:16` sets `handler404 = "plane.app.views.error_404.custom_404_view"`;
`api:plane/app/views/error_404.py:9-10`:

```python
def custom_404_view(request, exception=None):
    return JsonResponse({"error": "Page not found."}, status=404)
```

Body is **28 bytes**: `{"error": "Page not found."}`. `JsonResponse` uses
`DjangoJSONEncoder` with `json.dumps` defaults: `ensure_ascii=True`, separators
`", "` and `": "`, `allow_nan=True`. Status 404,
`Content-Type: application/json`. Resolved via
`resolver.resolve_error_handler(404)` (`py:django/core/handlers/exception.py:63-70`,
`162-170`), so it also covers an `Http404` raised outside a DRF view.

### 11.2 `500` — Django's default

No `handler500` is configured and there is no `templates/500.html`
(`apps/api/templates/` holds `admin/`, `base.html`, `csrf_failure.html`, `emails/`).
So `py:django/views/defaults.py:82-99` renders `ERROR_PAGE_TEMPLATE`:

```
\n<!doctype html>\n<html lang="en">\n<head>\n  <title>Server Error (500)</title>\n</head>\n<body>\n  <h1>Server Error (500)</h1><p></p>\n</body>\n</html>\n
```

**145 bytes**, `Content-Type: text/html; charset=utf-8`, status 500. The body is
identical for every unhandled exception — no stack trace, no JSON. A DRF view that
raises something DRF does not handle returns this, because `raise_uncaught_exception`
re-raises (`py:rest_framework/views.py:479-485`) and Django's handler catches it
(`py:django/core/handlers/exception.py:139-151`, `173-186`). There is no `500.html`
anywhere in the project (`TEMPLATES[0]["DIRS"]` is the single `apps/api/templates/`
directory and `APP_DIRS` adds no app template with that name).

**Except**: `BaseAPIView.handle_exception` / `BaseViewSet.handle_exception` in
`plane/app/views/base.py:200-239` and `api:plane/api/views/base.py:72-109`,
`183-242` catch *everything* that escapes DRF and return JSON:

| Exception | Status | Body |
| --- | --- | --- |
| `IntegrityError` | 400 | `{"error":"The payload is not valid"}` |
| `django.core.exceptions.ValidationError` | 400 | `{"error":"Please provide valid detail"}` |
| `ObjectDoesNotExist` | 404 | `{"error":"The required resource does not exist."}` (`BaseViewSet` says `"The required object does not exist."` … see below) |
| `KeyError` | 400 | `{"error":"The required key does not exist."}` |
| anything else | 500 | `{"error":"Something went wrong please try again later"}` |

Check the two files: `api:plane/api/views/base.py:93-97` uses
`"The requested resource does not exist."` (404) and `:212-223` uses
`"The required object does not exist."` (404); `api:plane/app/views/base.py:222-226`
uses `"The required object does not exist."`. These differ per base class and per
`/api/` vs `/api/v1/`; copy each. The probe §A.1.6 shows the `/api/v1/` catch-all:
`{"error":"Something went wrong please try again later"}` with status 500 for a
database `OperationalError` — note the body has **no spaces** (DRF renderer) while the
`{"error": "The payload is not valid"}` literals are also DRF-rendered; the spaces you
see in the Python source are not in the wire bytes.

`DEBUG=1` replaces all of this with the technical debug pages
(`py:django/core/handlers/exception.py:96-126`, `173-186`). Not part of the contract.

### 11.3 `400` — `ALLOWED_HOSTS` and `SuspiciousOperation`

`ALLOWED_HOSTS = os.environ.get("ALLOWED_HOSTS", "*").split(",")`
(`api:plane/settings/common.py:95`) — **`["*"]` by default**, so host checking is off
unless configured. When it is configured, `CommonMiddleware.process_request` calls
`request.get_host()` (`py:django/middleware/common.py:48`) for every request, which
raises `DisallowedHost(SuspiciousOperation)`. Under `DEBUG=0` that becomes
`handler400` with no `templates/400.html` → the same `ERROR_PAGE_TEMPLATE`
(`py:django/views/defaults.py:102-122`):

```
\n<!doctype html>\n<html lang="en">\n<head>\n  <title>Bad Request (400)</title>\n</head>\n<body>\n  <h1>Bad Request (400)</h1><p></p>\n</body>\n</html>\n
```

**143 bytes**, `text/html; charset=utf-8`. Because the exception is raised in
middleware #4, this response lacks `Content-Length` and `X-Frame-Options` (§1, §A.1.12).

The same 400 page (the "Bad Request (400)" variant) is produced for any other
`SuspiciousOperation`, including `RequestDataTooBig` raised outside the #10 middleware
and `MultiPartParserError` (`py:django/core/handlers/exception.py:84-126`).

### 11.4 `403` — Django's default

`PermissionDenied` outside DRF (e.g. `DISALLOWED_USER_AGENTS`) →
`handler403` → no `templates/403.html` → `HttpResponseForbidden` with the same
`ERROR_PAGE_TEMPLATE` (`py:django/views/defaults.py:125-160`):

```
\n<!doctype html>\n<html lang="en">\n<head>\n  <title>403 Forbidden</title>\n</head>\n<body>\n  <h1>403 Forbidden</h1><p></p>\n</body>\n</html>\n
```

141 bytes, `text/html; charset=utf-8`. This path is not covered by a probe because it
needs a configured `DISALLOWED_USER_AGENTS`; the byte count is computed from the
template.

### 11.5 Routes that do not use DRF `Response`

Grepped across `plane/` (tests excluded). These are the complete sets.

**`JsonResponse`** (Django encoder: `", "`/`": "`, `ensure_ascii=True`,
`allow_nan=True`):

| Location | Status | Body |
| --- | --- | --- |
| `api:plane/web/views.py:8-9` `GET /` | 200 | `{"status": "OK"}` (16 bytes) |
| `api:plane/web/views.py:12-13` `GET /robots.txt` | 200 | `User-agent: *\nDisallow: /`, `text/plain` |
| `api:plane/app/views/error_404.py:9-10` | 404 | `{"error": "Page not found."}` |
| `api:plane/middleware/request_body_size.py:22-28` | 413 | §9.1 |

**`HttpResponse`**:

| Location | Content-Type | Body |
| --- | --- | --- |
| `api:plane/app/views/workspace/base.py:389-391` | `text/csv` | `generate_csv_from_rows(...)`, plus `Content-Disposition: attachment; filename="workspace-user-activity.csv"` |
| `api:plane/web/views.py:12-13` | `text/plain` | `robots.txt` |

Both have `Content-Type` exactly as written — no `charset`.

**`StreamingHttpResponse`**:

| Location | Content-Type | Notes |
| --- | --- | --- |
| `api:plane/app/views/page/base.py:514-516` | `application/octet-stream` | `Content-Disposition: attachment; filename="page_description.bin"`; no `Content-Length` |

**`HttpResponseRedirect`** (302, empty body, `Location`):

| Area | Locations |
| --- | --- |
| asset download/redirect views | `api:plane/app/views/asset/v2.py:522, 564, 747, 990, 1025`; `api:plane/app/views/issue/attachment.py:229`; `api:plane/api/views/issue.py:2334`; `api:plane/space/views/asset.py:85` |
| instance admin form posts | `api:plane/license/api/views/admin.py:108-446` (19 sites) |
| auth redirects | `api:plane/authentication/rate_limit.py:90`; `api:plane/authentication/views/app/{email,magic,password_management,google,github,gitlab,gitea,signout}.py`; the `space` equivalents |

The asset redirects (`asset/v2.py:*`) are inside DRF views, so they pass through
`APIView.finalize_response` and carry the `Allow` header; `HttpResponseRedirect` is
not a `Response`, so no `Content-Type` is negotiated — it stays
`text/html; charset=utf-8`.

`JsonResponse` and `HttpResponse` are Django classes; they get the security headers,
`Content-Length` (from `CommonMiddleware`), `Vary: origin` and, when the session was
accessed, `Vary: Cookie` — exactly like DRF responses, only with a different body
encoding.

`/` and `/robots.txt` are `plane.web.views` function views, and function views do not
restrict methods: `POST /`, `PUT /`, `OPTIONS /` all return the health body
(`§A.1.1`, `HEAD`/`OPTIONS` blocks). Port that, or make `/` `GET`-only and accept the
divergence explicitly.

### 11.6 `Cache-Control` on four endpoints

Four views are decorated with `@cache_control(private=True, max_age=12)` and, three of
them, `@vary_on_cookie`:

| Route | Decorators | Location |
| --- | --- | --- |
| `GET /api/users/me/` | `cache_control(private=True, max_age=12)` + `vary_on_cookie` | `api:plane/app/views/user/base.py:82-83` |
| `GET /api/users/me/settings/` | same | `api:plane/app/views/user/base.py:88-89` |
| `GET /api/users/me/profile/` | same | `api:plane/app/views/user/base.py:443-444` |
| `GET /api/instances/` | `cache_control(private=True, max_age=12)` | `api:plane/license/api/views/instance.py:35` |

They emit `Cache-Control: private, max-age=12` and (the first three) `Vary: Cookie`,
which merges with the `Vary` values other layers add. These are the only
`Cache-Control` headers Plane sets on API responses; everything else has none.

---

## 12. Rate limiting

### 12.1 Classes in use

| Class | File | Scope | Rate |
| --- | --- | --- | --- |
| `AnonRateThrottle` (DRF) | `REST_FRAMEWORK["DEFAULT_THROTTLE_CLASSES"]`, `api:plane/settings/common.py:141` | `anon` | `30/minute` (`:142-145`) |
| `AssetRateThrottle` | `api:plane/throttles/asset.py:8-15` | `asset_id` | `5/minute` |
| `AuthenticationThrottle` | `api:plane/authentication/rate_limit.py:26-43` | `authentication` | `AUTHENTICATION_RATE_LIMIT` env, default `10/minute` |
| `EmailVerificationThrottle` | `api:plane/authentication/rate_limit.py:98-114` | `email_verification` | `3/hour` |
| `ApiKeyRateThrottle` | `api:plane/api/rate_limit.py:12-50` | `api_key` | `API_KEY_RATE_LIMIT` env, default `60/minute` |

`ScopedRateThrottle` is not used: no Plane view defines `throttle_scope`, and
`ScopedRateThrottle.allow_request` returns `True` when the scope is absent
(`py:rest_framework/throttling.py:219-226`).

Which routes get which:

* `/api/`, `/api/public/`, `/api/instances/`: the DRF defaults, i.e.
  `AnonRateThrottle` only. `AnonRateThrottle` returns `None` (no throttling) when
  `request.user.is_authenticated` (`throttling.py:173-180`), so signed-in callers are
  not throttled at all on these mounts. `AssetRateThrottle` is added on the single
  asset-download view (`api:plane/app/views/asset/v2.py:841`).
  `UserEndpoint.get_throttles` (`api:plane/app/views/user/base.py:72-80`) swaps the
  defaults on two actions: `generate_email_verification_code` →
  `[EmailVerificationThrottle(), AuthenticationThrottle()]`, `update_email` →
  `[AuthenticationThrottle()]`; everything else keeps `AnonRateThrottle`.
* `/api/v1/`: `BaseAPIView.get_throttles()`/`BaseViewSet.get_throttles()` return
  `[ApiKeyRateThrottle()]` (`api:plane/api/views/base.py:69-70`), **replacing** the
  default, so no anon throttle there.
* `/auth/`: `AuthenticationThrottle` on the `APIView` endpoints
  (`api:plane/authentication/views/{app,space}/{check,magic,password_management}.py`,
  `common.py:49`) and, for the plain `View` redirect flows, a manual check in the
  `@throttle_auth_redirect(...)` decorator (`rate_limit.py:62-95`).
  `EmailVerificationThrottle` is on the email-code endpoints.

### 12.2 Identity (`get_ident`)

`py:rest_framework/throttling.py:23-40`:

```python
xff = request.headers["x-forwarded-for"]; remote_addr = request.META["REMOTE_ADDR"]
num_proxies = api_settings.NUM_PROXIES
if num_proxies is not None:
    if num_proxies == 0 or xff is None: return remote_addr
    addrs = xff.split(",")
    return addrs[-min(num_proxies, len(addrs))].strip()
return "".join(xff.split()) if xff else remote_addr
```

`NUM_PROXIES` comes from the env (`api:plane/settings/common.py:154-160`): unset by
default, and only accepted when the raw value `isdecimal()` and at most 3 characters.

Measured (§A.3):

| `X-Forwarded-For` | `NUM_PROXIES` | ident |
| --- | --- | --- |
| absent | `None` | `REMOTE_ADDR` |
| `1.1.1.1, 2.2.2.2` | `None` | **`1.1.1.1,2.2.2.2`** (all whitespace removed; one ident for every client behind a proxy) |
| `1.1.1.1, 2.2.2.2` | `1` | `2.2.2.2` (rightmost) |
| `1.1.1.1, 2.2.2.2` | `2` | `1.1.1.1` |
| `1.1.1.1` | `0` | `REMOTE_ADDR` |

The `NUM_PROXIES=None` case is a foot-gun: with any proxy in front, every anonymous
client shares one bucket. Plane ships it that way; reproduce it, and note it in
`PROGRESS.md` rather than "fixing" it.

### 12.3 Cache keys

`SimpleRateThrottle.cache_format = "throttle_%(scope)s_%(ident)s"`
(`throttling.py:64`). django-redis prefixes with `KEY_PREFIX` and `VERSION`, both
defaulting to `""` and `1` (`py:django/core/cache/backends/base.py:83-84`, `99-110`;
`py:django_redis/client/default.py:876-891`), giving `":%s:%s:%s" % (prefix, version,
key)` (`default_key_func`, `base.py:33-42`).

| Throttle | Key as written | Full Redis key |
| --- | --- | --- |
| `AnonRateThrottle` | `throttle_anon_<ident>` | `:1:throttle_anon_<ident>` |
| `AuthenticationThrottle` | `throttle_authentication_<ident>` | `:1:throttle_authentication_<ident>` |
| `EmailVerificationThrottle` | `throttle_email_verification_<user_pk or ident>` | `:1:throttle_email_verification_…` |
| `AssetRateThrottle` | `throttle_asset_<asset_id>` (hand-built, no scope/format) | `:1:throttle_asset_<asset_id>` |
| `ApiKeyRateThrottle` | `<scope>:<api_key>` = `api_key:<key>` | `:1:api_key:<key>` |

`AssetRateThrottle` and `ApiKeyRateThrottle` bypass `cache_format`; the `api_key` one
therefore contains a raw API token as part of the Redis key name (`api:plane/api/rate_limit.py:16-23`).

`AuthenticationThrottle.get_cache_key` keys on the client IP **for every caller**,
signed in or not (`rate_limit.py:32-34`). `EmailVerificationThrottle` extends
`UserRateThrottle`, so it keys on `request.user.pk` when authenticated and on the IP
otherwise (`throttling.py:193-202`).

### 12.4 Value format

`SimpleRateThrottle.allow_request` (`throttling.py:109-141`):

```python
history = cache.get(key, [])            # a list of floats, newest first
now = time.time()
while history and history[-1] <= now - duration: history.pop()
if len(history) >= num_requests: return self.throttle_failure()
history.insert(0, now)
cache.set(key, history, self.duration)  # timeout = the window in seconds
return True
```

`django-redis` serialises a list with `PickleSerializer`, whose default protocol is
`pickle.DEFAULT_PROTOCOL` — **4** on Python 3.12 (not 5), and it applies no
compression (`IdentityCompressor`) (`py:django_redis/serializers/pickle.py:10-34`,
`py:django_redis/client/default.py:583-596`). Integers are stored raw, but the history
is always a list, so every throttle value is a pickle frame starting `\x80\x04`.
Examples (`§A.3`):

```
[1789000000.123456]            -> 8004950d000000000000005d944741daa87e5007e6b4612e
[1789000000.123456, 1788999940.0] -> 80049517000000000000005d94284741daa87e5007e6b44741daa87e41000000652e
[]                             -> 80045d942e
```

Decoded, the frames are:

```
[]                       80 04 95 05 00 00 00 00 00 00 00 5d 94 2e
                         PROTO 4 | FRAME(5) | EMPTY_LIST | MEMOIZE | STOP
[1789000000.123456]      80 04 95 0d 00 00 00 00 00 00 00 5d 94 47 <8 bytes> 61 2e
                         … | EMPTY_LIST | MEMOIZE | BINFLOAT | APPEND | STOP
[t0, t1]                 80 04 95 17 00 00 00 00 00 00 00 5d 94 28 47 <8> 47 <8> 65 2e
                         … | EMPTY_LIST | MEMOIZE | MARK | BINFLOAT | BINFLOAT
                           | APPENDS | STOP
```

A Rust implementation sharing these keys with Django must read and write protocol-4
pickles of a `list` of floats (`serde-pickle`, per `DECISIONS.md` D5), using
`BINFLOAT` (`G`, 8 big-endian IEEE-754 bytes) exactly as CPython's pickler does. Do not
write protocol 5 (`\x80\x05`): Django would still read it, but byte comparisons in the
differential harness would fail.

### 12.5 The 429

`APIView.check_throttles` (`views.py:357-376`) collects `throttle.wait()` from every
failing throttle, takes the maximum, and calls `self.throttled(request, duration)` →
`raise Throttled(wait)`. `Throttled.__init__` (`exceptions.py:233-244`) does
`wait = math.ceil(wait)` and appends `" Expected available in <n> second(s)."` to the
detail. The base handler then produces `{"detail": "Request was throttled. Expected
available in 60 seconds."}` with `Retry-After: 60`, and Plane's handler replaces the
body:

```
HTTP/1.1 429
Content-Type: application/json
Retry-After: 60
Allow: GET, HEAD, OPTIONS
{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}
```

The `wait()` formula is
`(duration - (now - history[-1])) / (num_requests - len(history) + 1)`
(`throttling.py:149-162`) and returns a float; `Throttled.__init__` stores
`math.ceil(wait)` and the header is that integer (`'%d' % exc.wait`).

For `/api/v1/` the throttle is the only one, and `BaseAPIView.finalize_response` copies
`X-RateLimit-Remaining` and `X-RateLimit-Reset` from `request.META` onto **every**
response when `ApiKeyRateThrottle.allow_request` set them
(`api:plane/api/rate_limit.py:25-50`, `api:plane/api/views/base.py:123-136`):

* `X-RateLimit-Remaining`: `max(0, num_requests - len(history))` after the insert;
* `X-RateLimit-Reset`: `int(now + duration)` — an absolute Unix timestamp, not a
  delta;
* both are set even when there is no `X-Api-Key` (the key is `None`, the history is
  empty, so `Remaining: <limit>` and `Reset: now + duration`, and nothing is written
  to Redis);
* neither is set on a 401, because authentication and permissions run before
  `check_throttles` (`views.py:404-421`).

Probe §A.1.13 shows `X-RateLimit-Remaining: 59` / `X-RateLimit-Reset: <now+60>` on the
first keyed request and `60` on the keyless one.

The auth-flow throttles do not produce a 429 from DRF: `AuthenticationThrottle` and
`EmailVerificationThrottle` define `throttle_failure_view` but never call it
(`rate_limit.py:36-43`, `107-114`); the actual rejection path is the
`@throttle_auth_redirect` decorator, which redirects to the app with
`error_code=5900&error_message=RATE_LIMIT_EXCEEDED` in the query string
(`rate_limit.py:77-93`, using `get_safe_redirect_url`).

---

## 13. Porting checklist

Ordered by how easy each one is to get wrong. This is the list a reviewer should walk
against the first cut of the tower stack.

1. **Middleware order and per-middleware exception wrappers.** `Vary`,
   `Content-Length`, `X-Frame-Options` and `Access-Control-*` presence all depend on
   *which* layer produced the response. A single `tower::ServiceBuilder` in list order
   is not enough; the error paths must short-circuit the same layers Django's
   `convert_exception_to_response` does.
2. **CSRF is two different mechanisms.** `/api/*` uses an origin check with no token
   (`BaseSessionAuthentication`), `/auth/*` form posts use Django's masked-token check,
   and every DRF view is `csrf_exempt`. Porting "CSRF" as one thing will break both.
3. **A CSRF rejection is `200 OK` with an HTML body**, not 403.
4. **`Content-Type` exact values**: `application/json` (no charset) for DRF and for
   `JsonResponse`; `text/html; charset=utf-8` for `HttpResponse` defaults, redirects
   and error pages; `text/css; charset="utf-8"` (quoted) from WhiteNoise.
5. **The session codec's salt is `"django.contrib.sessions.SessionStore"`, the HMAC key
   is `sha256(salt + "signer" + SECRET_KEY)`, and the payload is zlib-at-level-6 only
   when it is at least 2 bytes shorter.** Appendix B.1 is the vector.
6. **Python float formatting** (1e-05, 1e+16, 100.0) and Python's
   `datetime.isoformat()` (6 microsecond digits for DRF values, `Z` for UTC).
7. **`Allow` header on every DRF response, in `http_method_names` order**, and 401 vs
   403 driven by `authenticate_header` — with Plane then forcing every
   `NotAuthenticated` back to 401.
8. **`APPEND_SLASH` is decided in the response phase from a 404**, returns 301 for
   every method with an empty body, and its redirect has no `Content-Length` and no
   `X-Frame-Options`.
9. **Throttle identities and keys**: `NUM_PROXIES=None` collapses the identity to
   `"".join(xff.split())`, `api_key` throttles key on the raw token, and the Redis
   value is a protocol-4 pickle of a float list.
10. **`Vary` accumulation order** (`Cookie, origin`, `Accept-Encoding, Cookie, origin`)
    and the rule that `Vary: Accept-Encoding` is added before the `Accept-Encoding`
    check.

---

<!-- APPENDIX -->

---

# Appendix

Captured output of the scripts in `apps/server/tools/contract/`, run from
`apps/api` with `DJANGO_SETTINGS_MODULE=plane.settings.production`,
`SECRET_KEY=contract-test-secret-key-not-for-deployment`,
`DATABASE_URL=postgresql://x:x@127.0.0.1:1/x`, `REDIS_URL=redis://127.0.0.1:1/0`.
Django's JSON log lines and the whitenoise startup warning are stripped; nothing
else is edited. Bodies longer than the script's preview limit are marked as such,
and `b'...'` is Python `repr` of the exact bytes.

Reproduce with:

```sh
cd apps/api
for s in probe_http probe_session_cookie probe_throttles probe_json_renderer \
         session_codec csrf_token_vector; do
  .venv/bin/python ../server/tools/contract/$s.py
done
```

or regenerate this appendix with
`.venv/bin/python ../server/tools/contract/build_appendix.py`.

## A.1 `probe_http.py` — status, headers and raw body bytes

Split by the script's own `== N. …` banners; the sub-section numbers are the ones cited as `§A.1.N` in the body.

### A.1.0 preamble

```text
Django 5.2.17
DEBUG = False (production.py: int(os.environ['DEBUG'], 0) == 1)
DATABASE_URL = postgresql://x:x@127.0.0.1:1/x
REDIS_URL    = redis://127.0.0.1:1/0
```

### A.1.1 Plain Django views (no DRF, no DB)

```text
### GET /  (plane/web/views.py health_check -> JsonResponse)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'

### GET /robots.txt
status: 200
  Content-Type: text/plain
  X-Frame-Options: DENY
  Content-Length: 25
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(25 bytes): b'User-agent: *\nDisallow: /'

### GET /definitely-not-a-route/  (handler404 = custom_404_view)
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'

### HEAD /definitely-not-a-route/
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(0 bytes): b''

### GET /api/  (include prefix resolves to no pattern)
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'

### GET /api  (APPEND_SLASH: '/api/' does not resolve either)
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'

### HEAD /  (plain function view answers HEAD)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(0 bytes): b''

### OPTIONS /  (plain function view answers OPTIONS with its normal body)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(16 bytes): b'{"status": "OK"}'
```

### A.1.2 APPEND_SLASH (CommonMiddleware)

```text
### GET /api/users/me  (no trailing slash -> 301 to /api/users/me/)
status: 301
  Content-Type: text/html; charset=utf-8
  Location: /api/users/me/
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(0 bytes): b''

### POST /api/users/me  (DEBUG=0 -> 301, body dropped)
status: 301
  Content-Type: text/html; charset=utf-8
  Location: /api/users/me/
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(0 bytes): b''

### GET /api/users/me?x=1&y=2  (query string preserved)
status: 301
  Content-Type: text/html; charset=utf-8
  Location: /api/users/me/?x=1&y=2
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(0 bytes): b''

### POST /api/users/me DEBUG=1 (body truncated)
status: 500
  Content-Type: text/html; charset=utf-8
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(74074 bytes, first 400): b'<!DOCTYPE html>\n<html lang="en">\n<head>\n  <meta http-equiv="content-type" content="text/html; charset=utf-8">\n  <meta name="robots" content="NONE,NOARCHIVE">\n  <title>RuntimeError\n          at /api/users/me</title>\n  <style>\n    html * { padding:0; margin:0; }\n    body * { padding:10px 20px; }\n    body * * { padding:0; }\n    body { font-family: sans-serif; background-color:#fff; color:#000; }\n    '
```

### A.1.3 CORS: allow-all (CORS_ALLOW_ALL_ORIGINS=True, CORS_ALLOW_CREDENTIALS=True)

```text
### GET / with Origin (simple request)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
  access-control-allow-origin: http://localhost:3000
  access-control-allow-credentials: true
body(16 bytes): b'{"status": "OK"}'

### GET / without Origin (Vary: origin still added)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'

### OPTIONS /api/users/me/ with Origin + Access-Control-Request-Method (preflight)
status: 200
  content-length: 0
  Content-Type: text/html; charset=utf-8
  Vary: origin
  access-control-allow-origin: http://localhost:3000
  access-control-allow-credentials: true
  access-control-allow-headers: accept, authorization, content-type, user-agent, x-csrftoken, x-requested-with, X-API-Key
  access-control-allow-methods: DELETE, GET, OPTIONS, PATCH, POST, PUT
  access-control-max-age: 86400
body(0 bytes): b''

### OPTIONS /api/users/me/ with Origin, NO Access-Control-Request-Method (not a preflight -> view runs)
status: 401
  Content-Type: application/json
  Allow: GET, PATCH, DELETE, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 58
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  access-control-allow-origin: http://localhost:3000
  access-control-allow-credentials: true
  access-control-allow-headers: accept, authorization, content-type, user-agent, x-csrftoken, x-requested-with, X-API-Key
  access-control-allow-methods: DELETE, GET, OPTIONS, PATCH, POST, PUT
  access-control-max-age: 86400
body(58 bytes): b'{"detail":"Authentication credentials were not provided."}'

### OPTIONS /definitely-not-a-route/ with Origin + ACRM (preflight answers before routing)
status: 200
  content-length: 0
  Content-Type: text/html; charset=utf-8
  Vary: origin
  access-control-allow-origin: http://localhost:3000
  access-control-allow-credentials: true
  access-control-allow-headers: accept, authorization, content-type, user-agent, x-csrftoken, x-requested-with, X-API-Key
  access-control-allow-methods: DELETE, GET, OPTIONS, PATCH, POST, PUT
  access-control-max-age: 86400
body(0 bytes): b''
```

### A.1.4 CORS: allow-list (CORS_ALLOW_ALL_ORIGINS=False, CORS_ALLOWED_ORIGINS=[...])

```text
### GET / with allowed Origin
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
  access-control-allow-origin: http://localhost:3000
  access-control-allow-credentials: true
body(16 bytes): b'{"status": "OK"}'

### GET / with disallowed Origin (Vary: origin, no ACAO)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'

### OPTIONS preflight with disallowed Origin
status: 200
  content-length: 0
  Content-Type: text/html; charset=utf-8
  Vary: origin
body(0 bytes): b''
```

### A.1.5 Security headers (SecurityMiddleware, XFrameOptionsMiddleware)

```text
### GET / baseline security headers
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'

### GET / over plain http with SECURE_SSL_REDIRECT=True -> 301
status: 301
  Content-Type: text/html; charset=utf-8
  Location: https://testserver/
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(0 bytes): b''

### GET /robots.txt (SECURE_REDIRECT_EXEMPT) -> no redirect
status: 200
  Content-Type: text/plain
  X-Frame-Options: DENY
  Content-Length: 25
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(25 bytes): b'User-agent: *\nDisallow: /'

### GET / with X-Forwarded-Proto: https (SECURE_PROXY_SSL_HEADER) -> HSTS
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  Strict-Transport-Security: max-age=31536000; includeSubDomains; preload
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'
```

### A.1.6 DRF: unauthenticated /api/, /api/v1/, /api/public/ (no DB touched)

```text
### GET /api/users/me/  (BaseSessionAuthentication, IsAuthenticated)
status: 401
  Content-Type: application/json
  Allow: GET, PATCH, DELETE, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 58
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(58 bytes): b'{"detail":"Authentication credentials were not provided."}'

### DELETE /api/users/me/  (401 before 405: permission runs first)
status: 401
  Content-Type: application/json
  Allow: GET, PATCH, DELETE, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 58
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(58 bytes): b'{"detail":"Authentication credentials were not provided."}'

### GET /api/v1/users/me/  (APIKeyAuthentication, no X-Api-Key)
status: 401
  Content-Type: application/json
  Allow: GET
  X-Frame-Options: DENY
  Content-Length: 58
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(58 bytes): b'{"detail":"Authentication credentials were not provided."}'

### POST /api/v1/users/me/ with a bogus X-Api-Key (AuthenticationFailed 401)
status: 500
  Content-Type: application/json
  Allow: GET
  X-Frame-Options: DENY
  Content-Length: 55
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(55 bytes): b'{"error":"Something went wrong please try again later"}'

### GET /api/public/  (plane.space, no matching pattern)
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'
```

### A.1.7 Origin check used by /api/ writes (BaseSessionAuthentication.enforce_csrf)

```text
is_allowed_origin() decisions (settings.WEB_URL is unset in this probe):
  no Origin header                     -> True
  Origin == request host               -> True
  Origin has a port, Host does not     -> False
  Origin 'null'                        -> False
  Origin cross-site                    -> False

  enforce_csrf GET   {'HTTP_ORIGIN': 'http://evil.example'}   -> allowed
  enforce_csrf POST  {}                                       -> allowed
  enforce_csrf POST  {'HTTP_ORIGIN': 'http://evil.example'}   -> PermissionDenied: CSRF Failed: origin not allowed
  enforce_csrf POST  {'HTTP_ORIGIN': 'http://testserver'}     -> allowed
```

### A.1.8 CSRF token endpoint and the CSRF cookie

```text
### GET /auth/get-csrf-token/
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Vary: Cookie, origin
  Content-Length: 81
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Set-Cookie(raw): Set-Cookie: csrftoken=4gEK7Vpmp0SwKFXRd4CKev7VKFCZt5Hr; expires=Sun, 14 Nov 2027 17:44:46 GMT; HttpOnly; Max-Age=31449600; Path=/; SameSite=Lax
body(81 bytes): b'{"csrf_token":"vYZRZFf21w034LEA8KY5JHGdL13Ki9iQp4trWquegmIpEgrhbEqFN2DYlwvzB4P7"}'

  token length: 64

### GET /auth/get-csrf-token/ with CSRF_COOKIE_SECURE + COOKIE_DOMAIN
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Vary: Cookie, origin
  Content-Length: 81
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Set-Cookie(raw): Set-Cookie: csrftoken=eSOLNDgX0xKPLHT9iJDmjRT09bRUoP26; Domain=.plane.so; expires=Sun, 14 Nov 2027 17:44:46 GMT; HttpOnly; Max-Age=31449600; Path=/; SameSite=Lax; Secure
body(81 bytes): b'{"csrf_token":"r4qlM0CHLky4spHGjGs6NN2sQTLuJiwFvM4WptIuBH8J3WqFrfViWuLiPUseXXoB"}'

### GET /auth/get-csrf-token/ with CSRF_COOKIE_HTTPONLY=False
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Vary: Cookie, origin
  Content-Length: 81
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Set-Cookie(raw): Set-Cookie: csrftoken=eGnVtTmKpWjbewVe009PFAWp45fQ0GJu; expires=Sun, 14 Nov 2027 17:44:46 GMT; Max-Age=31449600; Path=/; SameSite=Lax
body(81 bytes): b'{"csrf_token":"nTUl7PlN27z9T66xe6fdbMufGQqhLuWlrp76qyxnhTIaXsRB4WeSGcguALvXB0vF"}'
```

### A.1.9 CsrfViewMiddleware on a plain Django View (POST /auth/magic-sign-in/)

```text
### POST /auth/magic-sign-in/ with no CSRF cookie (rejected)
status: 200
  Content-Type: text/html; charset=utf-8
  Vary: Accept-Encoding, Cookie, origin
  X-Frame-Options: DENY
  Content-Length: 1732
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(1732 bytes, first 400): b'<!-- templates/csrf_failure.html -->\n<!doctype html>\n<html lang="en">\n  <head>\n    <meta charset="UTF-8" />\n    <meta name="viewport" content="width=device-width, initial-scale=1.0" />\n    <title>CSRF Verification Failed</title>\n    <style>\n      body {\n        font-family: Arial, sans-serif;\n        background-color: #f8f9fa;\n        padding: 50px;\n        display: flex;\n        justify-content: '

### POST /auth/magic-sign-in/ with Origin (rejected on the origin check)
status: 200
  Content-Type: text/html; charset=utf-8
  Vary: Accept-Encoding, Cookie, origin
  X-Frame-Options: DENY
  Content-Length: 1732
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  access-control-allow-origin: http://evil.example
  access-control-allow-credentials: true
body(1732 bytes, first 400): b'<!-- templates/csrf_failure.html -->\n<!doctype html>\n<html lang="en">\n  <head>\n    <meta charset="UTF-8" />\n    <meta name="viewport" content="width=device-width, initial-scale=1.0" />\n    <title>CSRF Verification Failed</title>\n    <style>\n      body {\n        font-family: Arial, sans-serif;\n        background-color: #f8f9fa;\n        padding: 50px;\n        display: flex;\n        justify-content: '

  csrftoken cookie value (first 8): Qsh99IDc len 32
  JSON csrf_token  (first 8): p0cE0X8r len 64

### POST /auth/magic-sign-in/ with matching X-CSRFToken and cookie (passes CSRF, then 500 on Redis)
status: 500
  Content-Type: text/html; charset=utf-8
  X-Frame-Options: DENY
  Content-Length: 145
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(145 bytes): b'\n<!doctype html>\n<html lang="en">\n<head>\n  <title>Server Error (500)</title>\n</head>\n<body>\n  <h1>Server Error (500)</h1><p></p>\n</body>\n</html>\n'
```

### A.1.10 gzip (GZipMiddleware)

```text
### GET /  (16 bytes < 200 -> no gzip, no Vary)
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'

### GET /definitely-not-a-route/ (28 bytes -> no gzip, Vary: Accept-Encoding added anyway)
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'

### GET /auth/get-csrf-token/ with Accept-Encoding: gzip (81-byte body)
  Content-Encoding: None
  Vary: Cookie, origin
  Content-Length: 81
```

### A.1.11 Request body limits (RequestBodySizeLimitMiddleware, DATA_UPLOAD_MAX_MEMORY_SIZE)

```text
### POST / with a 200-byte body and DATA_UPLOAD_MAX_MEMORY_SIZE=100 -> 413
status: 413
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 111
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(111 bytes): b'{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}'

### POST /api/users/me/ with a 200-byte body -> 413, before authentication
status: 413
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 111
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(111 bytes): b'{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}'
```

### A.1.12 ALLOWED_HOSTS

```text
### GET / with Host: testserver under ALLOWED_HOSTS=['plane.example.com'] -> 400
status: 400
  Content-Type: text/html; charset=utf-8
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(143 bytes): b'\n<!doctype html>\n<html lang="en">\n<head>\n  <title>Bad Request (400)</title>\n</head>\n<body>\n  <h1>Bad Request (400)</h1><p></p>\n</body>\n</html>\n'

### GET / with Host: plane.example.com -> 200
status: 200
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 16
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(16 bytes): b'{"status": "OK"}'
```

### A.1.12b WhiteNoise and /static/

```text
### GET /static/css/style.css with DEBUG=0 and no collected-static dir -> falls through to 404
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'

### GET /static/ -> falls through to 404
status: 404
  Content-Type: application/json
  X-Frame-Options: DENY
  Content-Length: 28
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(28 bytes): b'{"error": "Page not found."}'

### GET /static/css/style.css with DEBUG=True (WHITENOISE_AUTOREFRESH/USE_FINDERS default to DEBUG)
status: 200
  Content-Type: text/css; charset="utf-8"
  Cache-Control: max-age=0, public
  Access-Control-Allow-Origin: *
  Last-Modified: Sun, 15 Nov 2026 16:52:19 GMT
  ETag: "6af9e343-0"
  Content-Length: 0
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(0 bytes): b''

### GET /static/css/style.css with If-None-Match
status: 304
  Cache-Control: max-age=0, public
  ETag: "6af9e343-0"
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(0 bytes): b''

### GET /static/logos/Logo.png with Range: bytes=0-9
status: 206
  Content-Type: image/png
  Cache-Control: max-age=0, public
  Access-Control-Allow-Origin: *
  Last-Modified: Sun, 15 Nov 2026 16:52:19 GMT
  ETag: "6af9e343-4d5e"
  Content-Range: bytes 0-9/19806
  Content-Length: 10
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(10 bytes): b'\x89PNG\r\n\x1a\n\x00\x00'
```

### A.1.13 Synthetic DRF views (DRF contract in isolation)

```text
### GET /t/plain/ (APIView, AllowAny, single JSONRenderer)
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 28
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(28 bytes): b'{"ok":true,"detail":"plain"}'

### POST /t/plain/ (405 MethodNotAllowed + Allow header)
status: 405
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 41
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(41 bytes): b'{"detail":"Method \\"POST\\" not allowed."}'

### GET /t/plain/?format=json
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 28
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(28 bytes): b'{"ok":true,"detail":"plain"}'

### GET /t/plain/?format=xml (404 from filter_renderers)
status: 404
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 23
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(23 bytes): b'{"detail":"Not found."}'

### OPTIONS /t/plain/ (SimpleMetadata)
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 152
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(152 bytes): b'{"name":"Plain","description":"","renders":["application/json"],"parses":["application/json","application/x-www-form-urlencoded","multipart/form-data"]}'

### GET /t/plain/ with Accept: text/html (406 NotAcceptable)
status: 406
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 57
  Vary: Cookie, origin
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(57 bytes): b'{"detail":"Could not satisfy the request Accept header."}'

### OPTIONS /t/generic/ (SimpleMetadata with actions)
status: 200
  Content-Type: application/json
  Allow: GET, POST, HEAD, OPTIONS
  Vary: Accept-Encoding, origin
  X-Frame-Options: DENY
  Content-Length: 778
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(778 bytes, first 400): b'{"name":"Generic","description":"","renders":["application/json"],"parses":["application/json","application/x-www-form-urlencoded","multipart/form-data"],"actions":{"POST":{"name":{"type":"string","required":true,"read_only":false,"label":"Name","max_length":10},"count":{"type":"integer","required":false,"read_only":false,"label":"Count","min_value":1},"when":{"type":"datetime","required":false,"r'

### GET /t/permission/ (PermissionDenied 403)
status: 403
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 63
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(63 bytes): b'{"detail":"You do not have permission to perform this action."}'

### GET /t/notfound/ (Http404 -> NotFound 404)
status: 404
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 23
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(23 bytes): b'{"detail":"Not found."}'

### GET /t/auth-none/ no credentials (NotAuthenticated, no authenticate_header -> Plane forces 401)
status: 401
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 58
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(58 bytes): b'{"detail":"Authentication credentials were not provided."}'

### GET /t/auth-none/ with X-Fail: raise (AuthenticationFailed, no authenticate_header -> 403)
status: 403
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 41
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(41 bytes): b'{"detail":"Given API token is not valid"}'

### GET /t/auth-header/ with X-Fail: raise (authenticate_header present -> 401 + WWW-Authenticate)
status: 401
  Content-Type: application/json
  WWW-Authenticate: Basic realm="api"
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 41
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(41 bytes): b'{"detail":"Given API token is not valid"}'

### GET /t/boom/ (unhandled exception -> Django 500 handler)
status: 500
  Content-Type: text/html; charset=utf-8
  X-Frame-Options: DENY
  Content-Length: 145
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(145 bytes): b'\n<!doctype html>\n<html lang="en">\n<head>\n  <title>Server Error (500)</title>\n</head>\n<body>\n  <h1>Server Error (500)</h1><p></p>\n</body>\n</html>\n'

### POST /t/echo/ valid JSON
status: 200
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 18
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(18 bytes): b'{"parsed":{"a":1}}'

### POST /t/echo/ malformed JSON
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 73
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(73 bytes): b'{"detail":"JSON parse error - Expecting value: line 1 column 6 (char 5)"}'

### POST /t/echo/ empty body with Content-Type: application/json
status: 200
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 13
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(13 bytes): b'{"parsed":{}}'

### POST /t/echo/ JSON null
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 57
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(57 bytes): b'{"detail":"Request body must be a JSON object or array."}'

### POST /t/echo/ JSON string body
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 57
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(57 bytes): b'{"detail":"Request body must be a JSON object or array."}'

### POST /t/echo/ JSON NaN (STRICT_JSON=True)
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 87
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(87 bytes): b'{"detail":"JSON parse error - Out of range float values are not JSON compliant: \'NaN\'"}'

### POST /t/echo/ text/plain (415 UnsupportedMediaType)
status: 415
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 62
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(62 bytes): b'{"detail":"Unsupported media type \\"text/plain\\" in request."}'

### POST /t/echo/ application/json; charset=latin-1
status: 200
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 18
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(18 bytes): b'{"parsed":{"a":1}}'

### POST /t/echo/ application/json; charset=bz2 (non-text codec rejected)
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 72
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(72 bytes): b'{"detail":"Unsupported charset \\"bz2\\" in request Content-Type header."}'

### POST /t/echo/ no Content-Type at all (stream None -> empty data)
status: 415
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 76
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(76 bytes): b'{"detail":"Unsupported media type \\"application/octet-stream\\" in request."}'

### POST /t/echo/ empty Content-Type header with a body
status: 415
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 52
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(52 bytes): b'{"detail":"Unsupported media type \\"\\" in request."}'

### GET /t/apikey/ with X-Api-Key: probe-key (ApiKeyRateThrottle headers)
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-RateLimit-Remaining: 59
  X-RateLimit-Reset: 1794764746
  X-Frame-Options: DENY
  Content-Length: 11
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(11 bytes): b'{"ok":true}'

### GET /t/apikey/ with no X-Api-Key (throttle key None, no headers)
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-RateLimit-Remaining: 60
  X-RateLimit-Reset: 1794764746
  X-Frame-Options: DENY
  Content-Length: 11
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(11 bytes): b'{"ok":true}'

### POST /t/validation/ (serializer ValidationError)
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 36
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(36 bytes): b'{"name":["This field is required."]}'

### POST /t/validation-string/ (bare ValidationError('boom'))
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 8
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(8 bytes): b'["boom"]'

### POST /t/parse-error/ (explicit ParseError)
status: 400
  Content-Type: application/json
  Allow: POST, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 33
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(33 bytes): b'{"detail":"explicit parse error"}'

### GET /t/throttled/ (rate 1/minute, first call)
status: 200
  Content-Type: application/json
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 11
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(11 bytes): b'{"ok":true}'

### GET /t/throttled/ (second call -> 429)
status: 429
  Content-Type: application/json
  Retry-After: 60
  Allow: GET, HEAD, OPTIONS
  X-Frame-Options: DENY
  Content-Length: 57
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
  Vary: origin
body(57 bytes): b'{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}'

### GET /t/boom/ with DEBUG=True (DEBUG_PROPAGATE_EXCEPTIONS off -> technical 500 page)
status: 500
  Content-Type: text/html; charset=utf-8
  Vary: Accept-Encoding, origin
  X-Frame-Options: DENY
  Content-Length: 94985
  X-Content-Type-Options: nosniff
  Referrer-Policy: same-origin
  Cross-Origin-Opener-Policy: same-origin
body(94985 bytes, first 300): b'<!DOCTYPE html>\n<html lang="en">\n<head>\n  <meta http-equiv="content-type" content="text/html; charset=utf-8">\n  <meta name="robots" content="NONE,NOARCHIVE">\n  <title>RuntimeError\n          at /t/boom/</title>\n  <style>\n    html * { padding:0; margin:0; }\n    body * { padding:10px 20px; }\n    body *'
```

## A.2 `probe_session_cookie.py` — Set-Cookie for every session branch

```text
Django 5.2.17
now = 1794764687

Defaults: SESSION_COOKIE_NAME=session-id, SESSION_COOKIE_AGE=604800,
          ADMIN_SESSION_COOKIE_NAME=admin-session-id, ADMIN_SESSION_COOKIE_AGE=3600,
          SESSION_COOKIE_HTTPONLY=True, SESSION_COOKIE_SAMESITE=Lax,
          SESSION_COOKIE_PATH=/, SESSION_COOKIE_DOMAIN=None, SESSION_COOKIE_SECURE=False,
          SESSION_SAVE_EVERY_REQUEST=False

### 1. new session, modified, no cookie in the request
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 22 Nov 2026 17:44:47 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 22 Nov 2026 17:44:47 GMT'), ('httponly', True), ('max-age', 604800), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 2. new session on an admin path (substring 'instances')
  path='/api/instances/users/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: admin-session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 15 Nov 2026 18:44:47 GMT; HttpOnly; Max-Age=3600; Path=/; SameSite=Lax
  morsel[admin-session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 15 Nov 2026 18:44:47 GMT'), ('httponly', True), ('max-age', 3600), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 3. path that merely contains 'instances' also gets the admin cookie
  path='/api/workspaces/acme/instances/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: admin-session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 15 Nov 2026 18:44:47 GMT; HttpOnly; Max-Age=3600; Path=/; SameSite=Lax
  morsel[admin-session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 15 Nov 2026 18:44:47 GMT'), ('httponly', True), ('max-age', 3600), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 4. session accessed but not modified
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=False
  Vary = Cookie
  Set-Cookie = (none)

### 5. session neither accessed nor modified
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=False modified=False
  Vary = None
  Set-Cookie = (none)

### 6. flushed session (session_key None + empty data) with a cookie -> delete cookie
  path='/api/users/me/' cookie_key=None
  session data={}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=""; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Thu, 01 Jan 1970 00:00:00 GMT'), ('httponly', ''), ('max-age', 0), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 7. flushed admin session with the admin cookie -> delete admin cookie
  path='/api/instances/' cookie_key=None
  session data={}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: admin-session-id=""; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax
  morsel[admin-session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Thu, 01 Jan 1970 00:00:00 GMT'), ('httponly', ''), ('max-age', 0), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 7b. session_key present but payload empty -> NOT a delete, cookie re-set
  path='/api/users/me/' cookie_key='kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk'
  session data={}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 22 Nov 2026 17:44:47 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 22 Nov 2026 17:44:47 GMT'), ('httponly', True), ('max-age', 604800), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 8. response status 500 -> no save, no cookie
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=500 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = (none)

### 9. browser-close session (_session_expiry = 0)
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}, '_session_expiry': 0}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; HttpOnly; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', ''), ('httponly', True), ('max-age', ''), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 10. custom expiry (_session_expiry = 100)
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}, '_session_expiry': 100}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 15 Nov 2026 17:46:27 GMT; HttpOnly; Max-Age=100; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 15 Nov 2026 17:46:27 GMT'), ('httponly', True), ('max-age', 100), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 11. custom expiry as an ISO timestamp
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}, '_session_expiry': '2099-01-01T00:00:00+00:00'}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Wed, 31 Dec 2098 23:59:59 GMT; HttpOnly; Max-Age=2276144112; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Wed, 31 Dec 2098 23:59:59 GMT'), ('httponly', True), ('max-age', 2276144112), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 12. SESSION_COOKIE_SECURE=True -> Secure attribute
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 22 Nov 2026 17:44:47 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax; Secure
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 22 Nov 2026 17:44:47 GMT'), ('httponly', True), ('max-age', 604800), ('path', '/'), ('samesite', 'Lax'), ('secure', True), ('version', '')]

### 13. COOKIE_DOMAIN=.plane.so -> Domain attribute
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; Domain=.plane.so; expires=Sun, 22 Nov 2026 17:44:47 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', '.plane.so'), ('expires', 'Sun, 22 Nov 2026 17:44:47 GMT'), ('httponly', True), ('max-age', 604800), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 14. SESSION_SAVE_EVERY_REQUEST=True, not modified -> cookie refreshed
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=False
  Vary = Cookie
  Set-Cookie = Set-Cookie: session-id=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 22 Nov 2026 17:44:47 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
  morsel[session-id] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 22 Nov 2026 17:44:47 GMT'), ('httponly', True), ('max-age', 604800), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 15. renamed cookies
  path='/api/users/me/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: plane-session=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 22 Nov 2026 17:44:47 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax
  morsel[plane-session] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 22 Nov 2026 17:44:47 GMT'), ('httponly', True), ('max-age', 604800), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

### 16. renamed admin cookie
  path='/api/instances/x/' cookie_key=None
  session data={'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f9f2f', 'device_info': {'user_agent': 'probe', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
  status=200 accessed=True modified=True
  Vary = Cookie
  Set-Cookie = Set-Cookie: plane-admin=kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk; expires=Sun, 15 Nov 2026 18:44:47 GMT; HttpOnly; Max-Age=3600; Path=/; SameSite=Lax
  morsel[plane-admin] attrs = [('comment', ''), ('domain', ''), ('expires', 'Sun, 15 Nov 2026 18:44:47 GMT'), ('httponly', True), ('max-age', 3600), ('path', '/'), ('samesite', 'Lax'), ('secure', ''), ('version', '')]

Cookie attribute order is http.cookies.Morsel.OutputString():
  `sorted(self.items())`, i.e. alphabetical by lower-case attribute name:
  comment < domain < expires < httponly < max-age < path < samesite < secure < version

Deletion is HttpResponse.delete_cookie():
  value='', max-age=0, expires='Thu, 01 Jan 1970 00:00:00 GMT', path=SESSION_COOKIE_PATH,
  domain=SESSION_COOKIE_DOMAIN, samesite=SESSION_COOKIE_SAMESITE, no httponly.
  patch_vary_headers(response, ('Cookie',)) is applied in both branches.
```

## A.3 `probe_throttles.py` — throttle identities, cache keys, pickle values

```text
Django 5.2.17

REST_FRAMEWORK throttling settings:
  DEFAULT_THROTTLE_CLASSES = [<class 'rest_framework.throttling.AnonRateThrottle'>]
  DEFAULT_THROTTLE_RATES   = {'anon': '30/minute', 'asset_id': '5/minute'}
  NUM_PROXIES              = None
  API_KEY_RATE_LIMIT       = 60/minute
  AUTHENTICATION_RATE_LIMIT= 10/minute (default)

Cache backend:
  class        = django_redis.cache.RedisCache
  LOCATION     = redis://127.0.0.1:1/0
  KEY_PREFIX   = ''
  VERSION      = 1
  KEY_FUNCTION = django.core.cache.backends.base.default_key_func
  TIMEOUT      = 300
  serialized key for 'throttle_anon_1.2.3.4': :1:throttle_anon_1.2.3.4
  default_key_func('k', '', 1) = :1:k

  django-redis OPTIONS in settings: {'CLIENT_CLASS': 'django_redis.client.DefaultClient'}
  (no SERIALIZER/COMPRESSOR/PICKLE_VERSION key -> PickleSerializer, IdentityCompressor,
   pickle.DEFAULT_PROTOCOL)

Value encoding (PickleSerializer with PICKLE_VERSION unset):
  pickle.DEFAULT_PROTOCOL   = 4
  pickle.HIGHEST_PROTOCOL   = 5
  [1789000000.123456]                          -> 24 bytes  8004950d000000000000005d944741daa87e5007e6b4612e
  [1789000000.123456, 1788999940.0]            -> 34 bytes  80049517000000000000005d94284741daa87e5007e6b44741daa87e41000000652e
  []                                           -> 5 bytes  80045d942e
  [1789000000.0]                               -> 24 bytes  8004950d000000000000005d944741daa87e50000000612e
  IdentityCompressor.compress == identity: True

Integers are NOT pickled (DefaultClient.encode(allow_int=True) returns them raw),
but a list history is always pickled. Only list values are written by throttles.

Identities (BaseThrottle.get_ident):
  no XFF, NUM_PROXIES unset                      NUM_PROXIES=None -> '203.0.113.7'
  no XFF, NUM_PROXIES=1                          NUM_PROXIES=1    -> '203.0.113.7'
  XFF='1.1.1.1, 2.2.2.2', NUM_PROXIES unset      NUM_PROXIES=None -> '1.1.1.1,2.2.2.2'
  XFF='1.1.1.1, 2.2.2.2', NUM_PROXIES=1          NUM_PROXIES=1    -> '2.2.2.2'
  XFF='1.1.1.1, 2.2.2.2', NUM_PROXIES=2          NUM_PROXIES=2    -> '1.1.1.1'
  XFF=' 1.1.1.1 ,  2.2.2.2 ', NUM_PROXIES=2      NUM_PROXIES=2    -> '1.1.1.1'
  XFF present, NUM_PROXIES=0                     NUM_PROXIES=0    -> '10.0.0.1'
  XFF has more hops than NUM_PROXIES=5           NUM_PROXIES=5    -> '1.1.1.1'

Throttle classes and cache keys (cache_format = 'throttle_%(scope)s_%(ident)s'):
  AnonRateThrottle(anon)          key = throttle_anon_203.0.113.7
    -> full redis key             = :1:throttle_anon_203.0.113.7
  AnonRateThrottle(authenticated)=  None
  AuthenticationThrottle          key = throttle_authentication_203.0.113.7
  EmailVerificationThrottle       key = throttle_email_verification_8f1c...
  EmailVerificationThrottle(anon) key = throttle_email_verification_203.0.113.7
  AssetRateThrottle               key = throttle_asset_a1b2
  AssetRateThrottle(no asset_id)  key = None
    -> full redis key             = :1:throttle_asset_a1b2
  ApiKeyRateThrottle              key = api_key:plane_api_abc123
    -> full redis key             = :1:api_key:plane_api_abc123
  ApiKeyRateThrottle(no header)   key = None

ScopedRateThrottle is not used by any Plane view (no throttle_scope attribute).
  ScopedRateThrottle scope None -> allow_request returns True: True

Rates parsed by SimpleRateThrottle.parse_rate:
  30/minute  -> (30, 60)
  5/minute   -> (5, 60)
  10/minute  -> (10, 60)
  3/hour     -> (3, 3600)
  60/minute  -> (60, 60)

History window arithmetic (SimpleRateThrottle.allow_request):
  history holds timestamps, newest first; entries with ts <= now - duration are dropped;
  429 when len(history) >= num_requests; cache.set(key, history, timeout=duration).
  wait() = (duration - (now - history[-1])) / (num_requests - len(history) + 1)
```

## A.4 `probe_json_renderer.py` — exact JSON bytes

```text
Django 5.2.17
ensure_ascii (DRF) = False  compact = True  strict = True
separators         = (',', ':')
JSONEncoder classes: drf = rest_framework.utils.encoders.JSONEncoder
                     django = django.core.serializers.json.DjangoJSONEncoder

Float representation, Python 3.12 `float.__repr__` -> JSON text
  0.0                      -> 0.0                      drf=0.0
  -0.0                     -> -0.0                     drf=-0.0
  1.0                      -> 1.0                      drf=1.0
  -1.0                     -> -1.0                     drf=-1.0
  0.1                      -> 0.1                      drf=0.1
  0.30000000000000004      -> 0.30000000000000004      drf=0.30000000000000004
  0.3333333333333333       -> 0.3333333333333333       drf=0.3333333333333333
  0.0001                   -> 0.0001                   drf=0.0001
  1e-05                    -> 1e-05                    drf=1e-05
  1.234567890123e-05       -> 1.234567890123e-05       drf=1.234567890123e-05
  1e-07                    -> 1e-07                    drf=1e-07
  1000000000000000.0       -> 1000000000000000.0       drf=1000000000000000.0
  1e+16                    -> 1e+16                    drf=1e+16
  1e+17                    -> 1e+17                    drf=1e+17
  1234567890123456.0       -> 1234567890123456.0       drf=1234567890123456.0
  1.2345678901234568e+16   -> 1.2345678901234568e+16   drf=1.2345678901234568e+16
  1e+21                    -> 1e+21                    drf=1e+21
  1e+22                    -> 1e+22                    drf=1e+22
  1.7976931348623157e+308  -> 1.7976931348623157e+308  drf=1.7976931348623157e+308
  5e-324                   -> 5e-324                   drf=5e-324
  2.2250738585072014e-308  -> 2.2250738585072014e-308  drf=2.2250738585072014e-308
  -1.5e-09                 -> -1.5e-09                 drf=-1.5e-09
  100.0                    -> 100.0                    drf=100.0
  1e+300                   -> 1e+300                   drf=1e+300

Values JSONRenderer/JSONEncoder special-case
  datetime UTC + microseconds      drf="2026-11-15T12:34:56.789012Z"                  django={"k": "2026-11-15T12:34:56.789Z"}
  datetime UTC no microseconds     drf="2026-11-15T12:34:56Z"                         django={"k": "2026-11-15T12:34:56Z"}
  datetime naive + microseconds    drf="2026-11-15T12:34:56.123456"                   django={"k": "2026-11-15T12:34:56.123"}
  datetime +05:30 tz               drf="2026-11-15T12:34:56.000001+05:30"             django={"k": "2026-11-15T12:34:56.000+05:30"}
  date                             drf="2026-11-15"                                   django={"k": "2026-11-15"}
  time                             drf="12:34:56.789012"                              django={"k": "12:34:56.789"}
  timedelta                        drf="90061.5"                                      django={"k": "P1DT01H01M01.500000S"}
  Decimal('1.10')                  drf=1.1                                            django={"k": "1.10"}
  Decimal('1E+3')                  drf=1000.0                                         django={"k": "1E+3"}
  Decimal('0.000001')              drf=1e-06                                          django={"k": "0.000001"}
  UUID                             drf="0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"         django={"k": "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"}
  bytes                            drf="abc"                                          django=RAISED TypeError: Object of type bytes is not JSON serializable
  set                              drf=["a","b"]                                      django=(unhashable/set)
  frozenset                        drf=["z"]                                          django=(unhashable/set)
  generator                        drf=[0,1,2]                                        django=RAISED TypeError: Object of type generator is not JSON serializable
  tuple                            drf=[1,2]                                          django={"k": [1, 2]}
  nested                           drf={"a":[1,{"b":null}],"c":true}                  django={"k": {"a": [1, {"b": null}], "c": true}}
  U+2028 in a string               drf="line\u2028sep\u2029end"                       django={"k": "line\u2028sep\u2029end"}
  non-ASCII                        drf="Zoë — 日本語 🚀"                                  django={"k": "Zo\u00eb \u2014 \u65e5\u672c\u8a9e \ud83d\ude80"}
  control chars                    drf="tab\tnl\nquote\"backslash\\"                  django={"k": "tab\tnl\nquote\"backslash\\"}

NaN / Infinity (STRICT_JSON=True -> allow_nan=False):
  NaN    -> ValueError: Out of range float values are not JSON compliant: nan
  +inf   -> ValueError: Out of range float values are not JSON compliant: inf
  -inf   -> ValueError: Out of range float values are not JSON compliant: -inf

Key order: dict insertion order, never sorted.
  drf = {"z":1,"a":2,"m":3}
  django = {"z": 1, "a": 2, "m": 3}

Empty structures:
  {}     drf=b'{}'    django=b'{"k": {}}'
  []     drf=b'[]'    django=b'{"k": []}'
  None   drf=b''      django=b'{"k": null}'
  ''     drf=b'""'    django=b'{"k": ""}'
  0      drf=b'0'     django=b'{"k": 0}'
  False  drf=b'false' django=b'{"k": false}'

TIME_ZONE = UTC
USE_TZ    = True

Serializer field to_representation (Plane uses the defaults from PLANE REST_FRAMEWORK)
  serialized = {"dt":"2026-11-15T12:34:56.120000Z","d":"2026-11-15","t":"12:34:56.789012","dur":"02:03:00","dec":"1.1000","uid":"0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0","f":1e-05,"i":7,"b":false,"s":"x"}

  DateTimeField ISO_8601 -> isoformat() with '+00:00' replaced by 'Z';
  microseconds are included only when non-zero (isoformat default).
  DurationField -> 'HH:MM:SS[.ffffff]'.
  DecimalField(coerce_to_string=True) -> f'{quantized:f}' -> trailing zeros kept.

  DateTimeField aware UTC, microseconds            -> '2026-11-15T12:34:56.120000Z'
  DateTimeField aware UTC, no microseconds         -> '2026-11-15T12:34:56Z'
  DateTimeField aware +05:30                       -> '2026-11-15T07:04:56.000001Z'
  DateTimeField naive (made aware in current tz)   -> '2026-11-15T12:34:56Z'
  DateTimeField None                               -> None
```

## B.1 `session_codec.py` — session payload test vector

```text
Django 5.2.17
SECRET_KEY        = contract-test-secret-key-not-for-deployment
frozen int(time)  = 1777777777 -> 1wJND7
key_salt          = django.contrib.sessions.SessionStore
SESSION_SERIALIZER= <class 'django.core.signing.JSONSerializer'>

session dict (insertion order is preserved in session_data):
{
  "_auth_user_id": "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
  "backend": "django.contrib.auth.backends.ModelBackend",
  "_auth_user_hash": "9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c",
  "_session_expiry": 3600,
  "device_info": {
    "user_agent": "Mozilla/5.0 (contract probe)",
    "ip_address": "203.0.113.7",
    "domain": "http://localhost/"
  }
}

raw JSON bytes fed to the serializer:
  len = 334
  {"_auth_user_id":"0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0","backend":"django.contrib.auth.backends.ModelBackend","_auth_user_hash":"9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c","_session_expiry":3600,"device_info":{"user_agent":"Mozilla/5.0 (contract probe)","ip_address":"203.0.113.7","domain":"http://localhost/"}}

zlib.compress default level output len = 250
compression used (len(z) < len(raw)-1)  = True

django SessionStore().encode(...):
   .eJxNjstuwyAQRX8lYtVKNgbj97L7fIM1MENM64JlSNU2yr8XR1l0O_fec-bGZrimZb5G2meHbGLCSqpRmbLRLZTd2A_l0I9dCa1ujMKapBWsYBrMB_ljgO_gL4Gb4NPuND9w_JlGfg5I69uzW_yXLRCXvB5tbRstUZEwA_S6w5Yaq6DW0ggcabA9dLo1TW7UVoLQoxmwp8620GhlDmikGF3wM31vbv9hk-qEKBjSlzM0O28Dm27sIYUL-ZSt5_Dr1hWqlovTy-NzMOm07UHTaya6bQbEPWNztxaKCy6l4n2OMHyC8_m8pLRNVbUGA-sSYqrY_f4HL9RrZQ:1wJND7:ywTmmvOLHYvBCPzyrQ_HeauuOal3DUrNa43jfPUyJbQ
hand-rolled encode(...):
   .eJxNjstuwyAQRX8lYtVKNgbj97L7fIM1MENM64JlSNU2yr8XR1l0O_fec-bGZrimZb5G2meHbGLCSqpRmbLRLZTd2A_l0I9dCa1ujMKapBWsYBrMB_ljgO_gL4Gb4NPuND9w_JlGfg5I69uzW_yXLRCXvB5tbRstUZEwA_S6w5Yaq6DW0ggcabA9dLo1TW7UVoLQoxmwp8620GhlDmikGF3wM31vbv9hk-qEKBjSlzM0O28Dm27sIYUL-ZSt5_Dr1hWqlovTy-NzMOm07UHTaya6bQbEPWNztxaKCy6l4n2OMHyC8_m8pLRNVbUGA-sSYqrY_f4HL9RrZQ:1wJND7:ywTmmvOLHYvBCPzyrQ_HeauuOal3DUrNa43jfPUyJbQ
MATCH: True

signer layout (value:timestamp:signature), split on ':' -> 3 parts
  payload(base64, '.' = zlib) = .eJxNjstuwyAQRX8lYtVKNgbj97L7fIM1MENM64JlSNU2yr8XR1l0O_fec-bGZrimZb5G2meHbGLCSqpRmbLRLZTd2A_l0I9dCa1ujMKapBWsYBrMB_ljgO_gL4Gb4NPuND9w_JlGfg5I69uzW_yXLRCXvB5tbRstUZEwA_S6w5Yaq6DW0ggcabA9dLo1TW7UVoLQoxmwp8620GhlDmikGF3wM31vbv9hk-qEKBjSlzM0O28Dm27sIYUL-ZSt5_Dr1hWqlovTy-NzMOm07UHTaya6bQbEPWNztxaKCy6l4n2OMHyC8_m8pLRNVbUGA-sSYqrY_f4HL9RrZQ
  timestamp(b62)             = 1wJND7 = 1777777777
  signature(base64 hmac)     = ywTmmvOLHYvBCPzyrQ_HeauuOal3DUrNa43jfPUyJbQ

SessionStore().decode(django_encoded) == {'_auth_user_id': '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0', 'backend': 'django.contrib.auth.backends.ModelBackend', '_auth_user_hash': '9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c', '_session_expiry': 3600, 'device_info': {'user_agent': 'Mozilla/5.0 (contract probe)', 'ip_address': '203.0.113.7', 'domain': 'http://localhost/'}}
round-trip equal: True

small session {'_auth_user_id': 'a'}
  raw JSON len = 21 zlib len = 29
  encoded = eyJfYXV0aF91c2VyX2lkIjoiYSJ9:1wJND7:sfyltBiEsICVZ8MB0qA4-ui9wGNe7hnt_nVcJtpaYLQ
  compressed flag = False
  decode  = {'_auth_user_id': 'a'}

non-ASCII session {'name': 'Zoë — 日本語', 'emoji': '🚀', 'sep': '\u2028'}
  encoded = .eJyrVspLzE1VslKKyo8pNTBITVKIKTUyMDQBUmamqaZA0twoOabUItEyVUlHKTU3PysTqDimNMXCOAVIploYAIWLUwvAgkYGRhZKtQAaHBiB:1wJND7:aTmnhQlrzgEghEFlplZGh3fJ6fM4HHcYhFOz-_GzFIM
  NOTE ensure_ascii=True -> \uXXXX escapes, latin-1 codepoint transport

tampered payload decode -> {}
garbage payload decode  -> {}

session_key generation: get_random_string(128, ascii_lowercase + digits)
  VALID_KEY_CHARS = abcdefghijklmnopqrstuvwxyz0123456789
  length = 128
```

## B.2 `csrf_token_vector.py` — CSRF masked-token test vector

```text
Django 5.2.17
CSRF_SECRET_LENGTH = 32
CSRF_TOKEN_LENGTH  = 64
CSRF_ALLOWED_CHARS = abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 (len 62 )
CSRF_HEADER_NAME   = HTTP_X_CSRFTOKEN

VECTOR 1 (deterministic)
  secret = 0123456789abcdefghijklmnopqrstuv
  mask   = ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef
  masked = ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA
  hand-rolled masked = ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA
  MATCH: True len: 64
  django _unmask_cipher_token(masked) = 0123456789abcdefghijklmnopqrstuv
  hand-rolled unmask(masked)          = 0123456789abcdefghijklmnopqrstuv
  recovered == secret: True

  _does_token_match(masked, secret)   = True
  _does_token_match(secret, secret)   = True
  _does_token_match(secret, other)    = False

VECTOR 2 (random, produced by django)
  secret = CTIGYblqvShV8RkLZ7bt4ZaWAiFZYPoF
  masked = auJMQXMEZbVPi6NmJTFdzXZ62SxwRmpZCdhiEYXUkT2AgNXXyQGwtMZSs02lF1Du
  unmask = CTIGYblqvShV8RkLZ7bt4ZaWAiFZYPoF
  match  = True

Format checks (_check_token_format):
  64 chars, allowed alphabet         -> OK
  32 chars, allowed alphabet         -> OK
  64 chars, bad alphabet             -> InvalidTokenFormat: has invalid characters
  64 chars, bad alphabet (underscore) -> InvalidTokenFormat: has invalid characters
  63 chars                           -> InvalidTokenFormat: has incorrect length
  empty                              -> InvalidTokenFormat: has incorrect length

Failure reason strings (verbatim, django/middleware/csrf.py:29-42):
  REASON_BAD_ORIGIN          'Origin checking failed - %s does not match any trusted origins.'
  REASON_NO_REFERER          'Referer checking failed - no Referer.'
  REASON_BAD_REFERER         'Referer checking failed - %s does not match any trusted origins.'
  REASON_NO_CSRF_COOKIE      'CSRF cookie not set.'
  REASON_CSRF_TOKEN_MISSING  'CSRF token missing.'
  REASON_MALFORMED_REFERER   'Referer checking failed - Referer is malformed.'
  REASON_INSECURE_REFERER    'Referer checking failed - Referer is insecure while host is secure.'
  REASON_INCORRECT_LENGTH    'has incorrect length'
  REASON_INVALID_CHARACTERS  'has invalid characters'

DRF wraps these as: PermissionDenied('CSRF Failed: %s' % reason)
  -> 403 {'detail': 'CSRF Failed: <reason>'}
```
