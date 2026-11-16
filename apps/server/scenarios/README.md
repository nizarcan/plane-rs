# Differential scenarios

`difftest` replays a recorded request session against two targets and compares
every observable side effect. It is the primary verification tool for the Rust
rewrite: a ported route is not done until the same scenario produces the same
status, headers, bodies, database diff, Redis diff and Celery messages on Django
and on `plane-api`.

The harness is its own cargo workspace (`crates/plane-testkit`), so it is built
by manifest path. Its build output still lands in `apps/server/target`, which is
why `./target/debug/difftest` works from `apps/server`.

```bash
cd apps/server
cargo run --manifest-path crates/plane-testkit/Cargo.toml --bin difftest -- selftest
cargo run --manifest-path crates/plane-testkit/Cargo.toml --bin difftest -- \
  run --scenario scenarios/basic-session.yaml --a django --b django
```

Exit code is 0 only when every step matches. `--report report.json` writes the
full run (normalised bodies, headers, database and Redis diffs, Celery messages
and the normalisation rules that applied).

`selftest` is not a smoke test: every comparison and every semantic check is
exercised on a synthetic, deliberately broken value (a session cookie that does
not match its row, a badly signed session payload, a CSRF token that does not
unmask to its cookie, a password hash that does not verify, an extra Redis key,
a changed TTL beyond tolerance, ...), and the selftest fails if a fault goes
undetected. It runs in `cargo test` too.

## Files

| File | Purpose |
| --- | --- |
| `targets.yaml` | How to start each kind of target (`django`, `rust`) and where its `SECRET_KEY` lives |
| `normalize.yaml` | The complete list of legitimately nondeterministic values: shapes, justified rules and semantic checks |
| `basic-session.yaml` | First-run web-app session: instance setup, sign-up, workspace, project, work items, comments, sign-out, error paths |

## What a run does

1. Allocates a run id and clones `plane_tpl` into `dt_<run>_a` and
   `dt_<run>_b` (unless `--a-db`/`--b-db` attach to existing databases).
2. Flushes the two Redis DB indexes (`redis_db: [1, 2]` in the scenario by
   default) and creates the RabbitMQ vhosts `dt_<run>_a` / `_b` with full
   permissions for `plane`. `--a-redis-db` / `--b-redis-db` override the
   scenario's indexes in managed mode as well as in attach mode, which is how a
   scenario shared with another agent runs on private indexes without editing
   the file.
3. Starts both targets on free ports from their own `port_range` (A from target
   A's range, B from target B's), waits for the health path, then replays the
   steps in order with both targets advancing together.
4. For each step: snapshot the database **and Redis** before and after, send the
   request, drain every queue in the target's vhost, normalise, compare, and
   print `ok` / `DIFF` with a unified diff per difference. Timings per step
   (request, database snapshot, Redis snapshot, normalisation) are printed.
5. Stops the targets by killing their process group (gunicorn workers
   included) and drops the databases and vhosts unless `--keep`.

## Scenario format

```yaml
name: example
template_db: plane_tpl          # default
target_env:                     # extra env for both managed targets
  FILE_SIZE_LIMIT: "4096"
vars:                           # scenario variables, available as {{name}}
  email: user@example.com
  password: Diff-Test-Passw0rd-2026!
password_vars:                  # plaintexts behind users.password, for verification
  - password
redis_db: [1, 2]                # Redis DB indexes for A and B (CLI --a-redis-db/--b-redis-db override them)
steps:
  - name: sign-in
    method: POST
    path: /auth/sign-in/
    jar: user                   # named cookie jar, default `default`
    form:                       # urlencoded body
      email: "{{email}}"
      csrfmiddlewaretoken: "{{csrf}}"
    headers:
      Origin: "{{base_url}}"    # implicit per-target variable
    expect_status: 302          # sanity check applied to both targets
    capture:
      - name: csrf
        source: json            # json | header | cookie | regex
        pointer: /csrf_token
      - name: location
        source: regex
        header: Location
        pattern: '/workspaces/([^/]+)/'
```

`target_env` is merged into the `env:` map of both targets the harness starts,
which is applied after `env_from`, so it wins over both `targets.yaml` and the
variables `django-env.sh` exports. Use it for a setting the scenario itself
depends on that must be identical on the two sides — `FILE_SIZE_LIMIT` for the
body-limit step, say — instead of requiring the caller to export it. Values go
through the same `{port}`/`{db}`/`{repo}` rendering as a target's `env:` map,
but a scenario-level value is meant to be a constant: use `targets.yaml` for
anything that must differ between A and B. The variables that select a
per-target resource are rejected, because setting them would make both targets
share a database, Redis index or broker:
`DATABASE_URL`, `REDIS_URL`, `PLANE_DB`, `PLANE_REDIS_DB`, `PLANE_MQ_VHOST`,
`PLANE_RS_BIND`, `RABBITMQ_VHOST`, `AMQP_URL`, `CELERY_BROKER_URL`. The key list
is `RESERVED_TARGET_ENV` in `crates/plane-testkit/src/config.rs`. Attach mode
does not start targets, so `target_env` is ignored there; pass the variable on
each target's own command line instead.

`json:` sends a JSON body, `body:` a raw string body (default
`text/plain;charset=UTF-8`), `form:` an urlencoded body, and `multipart:` a
`multipart/form-data` body with text fields and file parts (see "Write
scenarios"). A `Content-Type` header in `headers:` wins over the implicit one.
Variables are per target: A captures and uses A's values, B captures and uses
B's. `capture` supports a JSON pointer, a whole header, a cookie from the step's
jar, the first group of a regex over a header (useful for `Location`), or — for
writes — a column of the step's **inserted** database rows (`source: db`,
`table:`, `column:`).

Every request sends `Accept-Encoding: gzip` and a fixed browser `User-Agent`.
Redirects are not followed, cookies are not stored implicitly, and gzip bodies
are decoded by the harness.

`probe_compare_celery: true` makes a `policy-probe` pass-through step compare
the drained Celery messages even though the handler is not compared. It exists
for middleware side effects that run below the handler on both sides — today
`APITokenLogMiddleware`'s `process_logs`. The two `log_data` fields that copy
the pass-through response (`response_code`, `response_body`) are masked, and
`kwargsrepr` (Celery's Python `repr` of the same kwargs) is dropped because it
embeds them as text; everything else is compared byte for byte. Only use it for
steps whose real handler enqueues nothing of its own.

`password_vars` is optional. When it is set, every changed `users.password`
value must verify against one of those plaintexts (per target, so
`--b-var password=...` changes what B must verify); when it is not set, hashes
are shape-checked and their algorithm/iteration prefix compared, but not
verified.

## Write scenarios

A write step looks like a read step plus a body, a capture and an expected
status. The example is `scenarios/write/stickies-harness.yaml`; it runs Django
vs Django until the write methods flip to Rust, and the same file then runs
Django vs Rust.

```yaml
  - name: sticky-create
    method: POST
    path: "/api/workspaces/{{ws}}/stickies/"
    jar: admin
    headers: { Host: plane-fixture.test, Cookie: "session-id={{session_admin}}" }
    json:
      name: "harness sticky"
      description_html: "<p>created by the write harness</p>"
    capture:
      - name: sticky_id            # later steps use {{sticky_id}}
        source: db                 # the created row, from this step's DB diff
        table: stickies
        column: id
      - name: sticky_name          # ...and a value out of the response body
        source: json
        pointer: /name
    expect_status: 201

  - name: sticky-update
    method: PATCH
    path: "/api/workspaces/{{ws}}/stickies/{{sticky_id}}/"
    jar: admin
    headers: { Host: plane-fixture.test, Cookie: "session-id={{session_admin}}" }
    json: { name: "{{sticky_name}} (edited)" }
    expect_status: 200
```

Why `source: db` rather than the `id` in the response body: the id is what the
later `PATCH`/`DELETE` paths need, and the database diff is the row the rest of
the step is already checked against. The capture runs *before* normalisation, on
the raw snapshot diff, so it returns that side's generated id; A and B then use
their own id in the path, and the comparator sees the usual `<id:N>` placeholder
in the body, the database diff and any Celery argument. The step must insert
exactly one row in `table` (a bare table name matches `public.<table>`); a write
that inserts several rows is an error rather than a guess — capture from the
response then.

**Authentication.** The fixture sessions are supplied as an explicit
`Cookie: session-id=…` header with the jar's cookies merged in by name, so a
`csrftoken` cookie the target set survives. Two step options cover the rest:

* `api_key: "{{api_token_active}}"` sends `X-Api-Key` and skips the session —
  this is the `/api/v1/` auth path.
* `csrf: true` (default) sends `X-CSRFToken` from the jar's `csrftoken` cookie,
  for routes that use DRF's stock `SessionAuthentication`. Plane's own
  `BaseSessionAuthentication` does **not** use the token: it replaces DRF's CSRF
  check with an origin check (`plane/authentication/session.py:38-63`).

**Origin.** Every unsafe, cookie-authenticated request gets an `Origin` header,
because that is what Plane's session authentication actually checks:

| `origin:` | Header sent | Effect |
| --- | --- | --- |
| omitted / `true` | `http://<the step's Host>` | browser-accurate, allowed |
| `false` | none | allowed (Plane treats a missing origin as a non-browser client) |
| a string, e.g. `"http://evil.example"` | that value | `403 {"detail": "CSRF Failed: origin not allowed"}` |

`scenarios/write/stickies-harness.yaml` ends with that 403, the `204` soft
delete and the list steps that show the create, the update and the delete. A
soft delete shows up as an **UPDATE of `deleted_at`**, not a `DELETE`, so a port
that hard-deletes fails the step.

**Body shapes.** `json:` sends a JSON body; `form:` is urlencoded; `body:` is a
raw string, useful with a `Content-Type` the server expects (`headers:` wins
over the implicit `text/plain;charset=UTF-8`):

```yaml
    body: '{"name": "{{sticky_name}}"}'
    headers: { Content-Type: application/json }
```

`multipart:` builds a form with file parts:

```yaml
    multipart:
      fields:
        name: "{{sticky_name}}"
      files:
        - field: file
          filename: note.txt          # optional; defaults to the path's basename
          content_type: text/plain    # optional; default application/octet-stream
          content: "hello"            # exactly one of content:, path:
        - field: image
          path: fixtures/harness.png  # relative to apps/server (the harness's cwd)
```

Every part is sent with a fixed boundary, so both targets receive identical
request bytes.

**Starter files.** `tools/scenarios/gen_write.py` emits the first draft of a
group's write scenario: one step per unsafe method, a body built from the
serializer's writable fields, a `db` capture after `POST`, an obvious denial
step (a non-member session, `expect_status: 403`) and the expected create/update
/delete statuses.

```bash
cd apps/server
../api/.venv/bin/python tools/scenarios/gen_write.py --orders 223,224      # stickies
../api/.venv/bin/python tools/scenarios/gen_write.py --group stickies -v   # and show skipped fields
```

The generator knows serializer *fields*, not view rules: review the body (a
`description_html` pair, choice values, relations), fill in the `PATCH` body's
interesting fields, and re-run Django vs Django until it is clean before
publishing the file as `<group>.yaml`.

## Normalisation policy

Only values that are genuinely random or clock-based are rewritten. The table
lives in `normalize.yaml`; every rule and every shape has a `why` that is copied
into the report. The policy has three layers.

### 1. Typed placeholders, never erasure

A volatile value becomes a typed placeholder from a per-target map:
`<session:1>`, `<csrf-secret:1>`, `<csrf-token:1>`, `<username:1>`,
`<token:1>`, `<colour:1>`, `<id:1>` (generated UUIDs), `<now>`, `<epoch>`,
`<base>`, `<expires+Ns>`, `<offset±Ns>`. The **same raw value** anywhere in the
run — a `Set-Cookie` value, a header, a JSON field, a database column, a Celery
argument, a Redis key or value — always gets the **same placeholder**, so
cross-references are checked rather than hidden:

- the `session-id` cookie must equal its `sessions.session_key` row on the same
  target (a mismatch makes one side `<session:1>` and the other
  `<session:2>`, a DIFF);
- a response that shows another user's `username` gets that user's placeholder,
  not the caller's;
- an API token in a `/api/v1/` rate-limit key is the same `<token:N>` as the
  `users.token` value it came from.

Placeholders are assigned in a fixed traversal order so both targets number them
identically: headers (`Location` first) → body in document order → database diff
by table and masked row content → Celery messages → Redis diff by masked key.
The mapping and numbering persist for the whole run.

### 2. Shapes before substitution

Every placeholder rule names a shape in `normalize.yaml`, and the value is
matched against it on both targets **before** substitution. A session key that
is not 128 lower-case alphanumerics, a username that is not `uuid4().hex`, a
password column that is not a Django hash, or a Redis value that is not the
declared shape is a `shape` DIFF. The shapes are derived from Django's source
and from observed runs, e.g.:

| Shape | Pattern | Source |
| --- | --- | --- |
| `session_key` | 128 × `[a-z0-9]` | `get_random_string(128, ascii_lowercase + digits)` (`plane/db/models/session.py:14`) |
| `csrf_secret` | 32 × `[A-Za-z0-9]` | `CSRF_SECRET_LENGTH` (`django/middleware/csrf.py:44`) |
| `csrf_token` | 64 × `[A-Za-z0-9]` | masked token (`django/middleware/csrf.py:46`) |
| `uuid_hex` | 32 lower-case hex | `User(username=uuid.uuid4().hex)` (`plane/authentication/adapter/base.py:352`) |
| `api_token` | 64 lower-case hex | `uuid4().hex + uuid4().hex` (`plane/db/models/user.py:174`) |
| `hex_colour` | `#` + 6 hex, either case | `random.choices(string.hexdigits)` (`plane/utils/color.py:9`) |
| `django_password_hash` | `pbkdf2_sha256$<iter>$<salt>$<b64>` or `!` + 40 | `django/contrib/auth/hashers.py:324` |
| `signed_session` | `[.]payload:timestamp:signature` | `django/core/signing.py:230` |

### 3. Semantic checks

- **Session payloads** (`sessions.session_data`, mode `session_payload`) are
  verified with the target's `SECRET_KEY` — salt
  `django.contrib.sessions.SessionStore`, `TimestampSigner`, optional zlib —
  decoded, and compared as a **dictionary**. `_auth_user_id`,
  `_auth_user_backend`, `device_info` and every other key must match;
  `_auth_user_hash` is shape-checked (`sha256` hex) and replaced by a constant
  marker because it is an HMAC of the randomly salted password hash; the signing
  timestamp becomes `<epoch>` when it is in the step window. A payload that
  fails signature verification is a `session` DIFF, never a normalised blob.
  The row is also checked for internal consistency: `sessions.user_id` must
  equal the payload's `_auth_user_id` and `device_info` must equal its payload field.
- **CSRF** (`csrf_token`, `check: csrf_masked`): the masked token in the JSON
  body must unmask (Django's 62-character add/subtract scheme) to the
  `csrftoken` cookie secret set by the same response. A token that belongs to a
  different secret, or a response with a token but no cookie, is a `csrf` DIFF.
- **Passwords** (`users.password`, mode `password`): the hash is re-derived with
  PBKDF2-SHA256 using the hash's own salt and iteration count and compared with
  the stored digest; the algorithm and iteration count are also compared
  literally (`pbkdf2_sha256$1000000$<hash>`), so a Rust implementation that
  silently changes either is a DIFF.

### Redis side effects

Around every step each target's Redis logical database is snapshotted with
`SCAN` + `TYPE` + `PTTL`, the value for string keys up to 4 KiB and the elements
for list keys up to 100 (larger values are compared by type and length). The
step diff is added/changed/removed per key, after normalising key names the same
way as any other text (base URL, UUID placeholders, already-known raw values).

Values are django-redis values: an integer is stored raw, everything else is a
Python pickle (protocol 4 on Python 3.12), decoded with `serde-pickle` into a
JSON-ish tree. Throttle histories (lists of floats = request timestamps) are
compared as the number of entries plus each timestamp: values inside the step
window, and values seen in an earlier step, become `<now>`; a constant float
(for example `file_size_limit: 5242880.0`) stays exact. A pickled `uuid.UUID`
decodes back to the UUID string, so it shares its placeholder with the same id
in a response body. A pickle the decoder cannot model is compared by presence
and byte length only. TTLs are compared with a **2 s tolerance**; a pickled
value's byte length is not compared when its value was, because the encoding of
a generated UUID has a variable length.

The response caches written by `plane/utils/cache.py` are ordinary pickled
`{"data": ..., "status": ...}` dictionaries, so their key names must match and
their decoded contents are compared like a response body.

### Built-in rules

- generated UUIDs (not present in the baseline database) become `<id:N>`,
  numbered per target in the traversal order above;
- ISO-8601 timestamps inside the step window become `<now>`; an *instant* this
  target wrote in an earlier step stays `<now>` when it resurfaces in another
  spelling (`+00:00` in Postgres, `Z` in DRF);
- the target's own base URL becomes `<base>`;
- cookie `Expires` becomes `<expires+Ns>` and database timestamps that are a
  fixed offset from the step clock become `<offset±Ns>`;
- Django's randomized gzip filename makes `Content-Encoding`/`Content-Length`
  unstable near the 200-byte threshold, so that decision is normalised; the
  decoded body is always compared;
- Celery's kombu `uuid`/`datetime` envelopes are unwrapped for normalisation,
  and its transport headers (`id`, `root_id`, `parent_id`, `origin`) are dropped
  because they name the publishing process.

If a Django-vs-Django run reports a new difference, find the cause before adding
a rule. A rule that hides a real difference weakens every future port. The
`why` field is mandatory for every rule and shape.

## Comparing a difference

For each step the harness compares:

1. status code;
2. all headers except `date`, `server` and (when unstable) `content-length`,
   case-insensitively, with multi-valued headers as ordered lists;
   `Set-Cookie` by cookie name and attributes, with volatile cookie values
   replaced. A surviving `Content-Length` is compared exactly, with one
   exception (DECISIONS.md D18): a 204 on both sides where one sends
   `Content-Length: 0` and the other sends none is not a difference —
   Django/uvicorn sends the header, hyper omits it per RFC 9110 §8.6, and Caddy
   strips it before clients see either. The same one-side-absent shape is
   ignored when both sides return 304. Every other `Content-Length` difference
   stays a diff (another value, a non-zero value on a 204, or different
   statuses), and the exception is listed in the run's applied rules as
   `content-length-bodyless`;
3. body: JSON parsed with key order and number lexemes kept, plus D15's byte
   rule (each side must equal its own DRF-compact re-serialisation);
4. the database diff (inserted/deleted/updated rows per table, as multisets);
5. the Redis diff (added/changed/removed keys, with TTL tolerance);
6. drained Celery messages (task name, args, kwargs and non-volatile headers);
7. any semantic violation on either side (shape, session, csrf, password,
   redis), which fails the step even when both targets agree with each other.

## Targets

```yaml
targets:
  django:
    kind: django
    command: ["{repo}/apps/server/dev/django-serve.sh", "{port}"]
    env: { PLANE_DB: "{db}", PLANE_REDIS_DB: "{redis_db}", PLANE_MQ_VHOST: "{vhost}" }
    secret_key_file: "{repo}/apps/server/dev/.env.local"
    health: /robots.txt        # side-effect-free readiness probe
  rust:
    kind: rust
    command: ["{repo}/apps/server/target/debug/plane-api"]
    env_from: "{repo}/apps/server/dev/django-env.sh"
    env: { PLANE_RS_BIND: "127.0.0.1:{port}" }
    secret_key_file: "{repo}/apps/server/dev/.env.local"
    health: /
```

`secret_key_file` is how the harness verifies signed session payloads: it reads
the named dotenv variable (`SECRET_KEY` by default) from the same file the
target reads. The value is never printed, logged or written into the report; an
error names the file and variable only. When `normalize.yaml` verifies session
payloads, a run fails fast if either target has no `secret_key_file`.

`env_from` sources a shell script in a clean bash and passes the resulting
environment to the target (the script's exports, the target's `PLANE_*` knobs
and a minimal `PATH`/`HOME`; the explicit `env:` map wins). `dev/django-env.sh` is
what Django's own scripts use, so the `rust` target sees the same
`DATABASE_URL`, `REDIS_URL`, `RABBITMQ_*`, `SECRET_KEY`, storage settings and
`PLANE_*` knobs. The script sees `PLANE_DB`, `PLANE_REDIS_DB` and
`PLANE_MQ_VHOST` for the target being started. For a release build, point
`command[0]` at `apps/server/target/release/plane-api`.

## Policy probe (`policy-probe.yaml`)

`plane-api` built with `--features policy-probe` registers a probe handler for
every route method outside `/api/v1/` (the two `plane.web` function views stay
real handlers because `/` is the health path). The handler answers `501` with
`X-Plane-RS-Probe: handler`, and only after the full runtime policy ran:
content negotiation, authentication, permission classes, throttles, the family
`initial()` hooks and `allow_permission`.

`policy-probe.yaml` is generated by `tools/policy/gen_policy_scenario.py` and
drives every route method for every fixture user. The fixture
(`scenarios/policy-fixture.sql`, generated by `tools/policy/gen_fixture.py`) is
named by the reserved `fixture_sql` scenario variable and applied to both
databases **before the baseline snapshot**, so it is never a step diff. It has
to be SQL: `/auth/` and `/api/` are not ported yet, so the users, sessions,
workspace, projects and objects cannot be created over HTTP.

The harness rule for a step whose B response carries the probe header:

* if A's response is a **policy** denial — 401, 405, 406, 415, 429, or a 403
  whose body is one of the policy bodies (`{"detail": "You do not have
  permission…"}`, `{"detail": "CSRF Failed: origin not allowed"}`, the
  `allow_permission` `{"error": "You don't have the required permissions."}`,
  …) — the step is a **DIFF**: Rust let through what Django refused;
* otherwise the step is `reached-handler`: its response, database, Redis and
  Celery diffs are not compared (B has no handler, A's real view may write).
  Semantic violations are still reported.

A handler may also answer 403 itself (`plane/app/views/asset/v2.py` does), which
is why the 403 check matches the policy bodies rather than the status alone.

All writes are in the final block of the scenario, `DELETE` steps name objects
that do not exist, and a POST/PUT/PATCH with an empty body cannot remove or
re-key a fixture row: a permitted write can therefore not change what a later
compared step sees.

Run it with:

```bash
cargo build -p plane-api --features policy-probe
./target/debug/difftest run --scenario scenarios/policy-probe.yaml --a django --b rust
```

`scenarios/policy-smoke.yaml` is the same generator with `--limit 5` for fast
iteration.

## Record mode

`difftest record` is a recording reverse proxy for browser (or `curl`) sessions:

```bash
difftest record --listen 127.0.0.1:18090 --upstream http://127.0.0.1:18000 \
  --out /tmp/recorded.yaml
difftest run --scenario /tmp/recorded.yaml --a django --b django
```

Every request is forwarded unchanged (including `Host`, so Django's CSRF origin
check still matches) and becomes one step: method, path, query, body, the
headers that matter (`Content-Type`, `X-CSRFToken`, `Origin`, `Referer`,
`X-API-Key`, `Accept`) and the cookie jar. Values that first appeared in an
earlier response are rewritten into a `capture` plus a `{{var}}` reference:

- every generated UUID in a JSON object (not inside a list response, which would
  add a capture per row), using a JSON pointer;
- `csrf_token`, `token`, `username` and `slug` values;
- UUIDs in a `Location` header, using a generic UUID regex so target B's
  different id matches too;
- UUID-valued cookies.

Session and CSRF cookie *secrets* are deliberately not captured: the replay
matches `Set-Cookie` to the named jar, and the masked token the client sends is
captured from the JSON body where it was issued. Responses are not stored:
replay compares the two targets, not the recording. The output file names steps
after their rewritten path, so re-recording the same flow is stable. The file is
rewritten after every request, so a browser crash still leaves the steps on disk.
Send `X-Difftest-Jar: <name>` to record a second cookie jar (the header is
stripped before forwarding). `--redis-db 1,2` selects the indexes written into
the scenario.

## Attach mode

To compare targets that are already running (for debugging, or to inject a
divergence into one database):

```bash
PLANE_DB=t_a PLANE_REDIS_DB=1 PLANE_MQ_VHOST=t_a dev/django-serve.sh 18001 &
PLANE_DB=t_b PLANE_REDIS_DB=2 PLANE_MQ_VHOST=t_b dev/django-serve.sh 18002 &

./target/debug/difftest run --scenario scenarios/basic-session.yaml \
  --a-url http://127.0.0.1:18001 --b-url http://127.0.0.1:18002 \
  --a-db t_a --b-db t_b --a-vhost t_a --b-vhost t_b \
  --a-redis-db 1 --b-redis-db 2
```

Attach mode does not clone or drop databases or vhosts, and the harness does
not stop the targets. `--no-celery` skips message draining when the vhosts are
not known. `--no-redis` skips Redis snapshots and comparison (useful to time the
rest of the harness, or against a target that does not use Redis yet).

`--a-var name=value` / `--b-var name=value` override a scenario variable for
one target. That is the easiest way to prove the harness detects a difference:
for example `--b-var company_role=Saboteur` changes only a database column that
no response echoes, so a clean run must turn into a database-only DIFF.

## Debugging and timings

```bash
# Snapshot timing and row counts for a clone (not the template: it is
# ALLOW_CONNECTIONS false)
dev/mkdb.sh t_inspect
./target/debug/difftest snapshot --db t_inspect --iterations 5
dev/dropdb.sh t_inspect
```

Target logs are written to `apps/server/target/difftest-logs/<run>/a.log` and
`b.log` (gitignored). `--keep` also keeps the databases and vhosts for
inspection; drop them with `dev/dropdb.sh` and `dev/mq-vhost.sh --delete`.

On a 46-step Django-vs-Django run the harness adds, per target: about **23 ms**
per step for the database snapshot, about **8.5 ms** per step for the Redis
snapshot, and about **1.9 ms** per step of normalisation, shape and semantic
checks. Password verification is the exception: re-deriving a Django
1,000,000-iteration hash costs about **0.3 s** per created user per target, so
only the two sign-up steps pay it. `--no-redis` removes the Redis snapshots;
the difference on `basic-session` is about 0.8 s of the run.
