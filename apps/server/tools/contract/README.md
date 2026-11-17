# Contract suite on a mixed Django + Rust backend

The contract half of the Phase 3 exit check: run `apps/api/plane/tests/contract`
**unchanged** while every request whose method + path is owned by `rust` in
`apps/server/ROUTES.md` goes over real HTTP to a `plane-api` instance. Everything
else stays in-process in Django, as it is today.

```
apps/server/tools/contract/
  pytest_plane_contract.py   the pytest plugin (routing, auth, DB, replay, report)
  dry_routing.py             static "which test files can reach Rust" pass
  run.sh                     the entry point: env, build, pytest
  smoke/test_replay.py       proof that a Rust publish reaches a patched `.delay`
  smoke/test_local_mock.py   proof that in-process patches are detected and stay in Django
  README.md                  this file
```

## Running

```sh
cd apps/server/tools/contract

# 1. Which contract files can reach a Rust-owned route at all (dry, no test
#    runs). Prints one path per line; the detail (method, template, group) goes
#    to stderr.
apps/api/.venv/bin/python dry_routing.py > /tmp/selected.txt

# 2. Baseline once, on exactly that set: everything in-process through the
#    plugin. Must match plain upstream (see "Baseline" below).
./run.sh --backend django -- $(cat /tmp/selected.txt) --create-db

# 3. The porting check on the same set.
./run.sh --backend mixed -- $(cat /tmp/selected.txt)

# 4. The smoke tests' own proof (starts plane-api; sends one Rust request).
./run.sh --backend mixed --smoke
```

`run.sh` accepts `--backend django|mixed` (default `mixed`, also
`$PLANE_RS_CONTRACT_BACKEND`), `--no-build` (skip `cargo build -p plane-api`)
and `--` after which every argument goes to pytest unchanged. A path given
after `--` replaces the default target directory.

### Before a domain's write methods are flipped

A domain agent can run its write routes against Rust while `ROUTES.md` still
says `django`. `--assume-rust` takes route orders (`routes.json`'s `order`, which
is `ROUTES.md`'s first column) and points every unsafe method that route
registers (`POST`/`PUT`/`PATCH`/`DELETE`) at `plane-api`; a route with no unsafe
method is reported and ignored, so a GET-only route is never sent to an
unported handler. `--methods` narrows the plan to those verbs, so only requests
that use them reach Rust (and the dry pass selects only files whose write calls
match):

```sh
# stickies (orders 223, 224), writes only
./run.sh --assume-rust 223,224 --methods POST,PATCH,DELETE
```

Both options apply to the dry selection pass as well
(`PLANE_RS_CONTRACT_ASSUME_RUST` / `PLANE_RS_CONTRACT_METHODS` are read by
`owner_override.py` in the plugin and in `dry_routing.py`), so
`dry_routing.py --quiet` and the pytest run select the same files. Do not edit
`ROUTES.md` to test a write path; it is the orchestrator's table.

| Variable | Default | Purpose |
| --- | --- | --- |
| `PLANE_RS_CONTRACT_BACKEND` | `mixed` | `django` or `mixed` |
| `PLANE_RS_CONTRACT_DB` | `t_rsct_contract` | test database (must start with `t_rsct`) |
| `PLANE_RS_CONTRACT_PORT` | `18200` | `plane-api` bind port (18200–18299) |
| `PLANE_RS_CONTRACT_REDIS_DB` | `15` | Redis database shared by Django's cache and `plane-api` (DB 1 receives writes from other processes) |
| `PLANE_RS_CONTRACT_VHOST` | the contract DB name | RabbitMQ vhost created for the run; defaults to `PLANE_RS_CONTRACT_DB`, so separate DBs never share one |
| `PLANE_RS_CONTRACT_REPORT` | `apps/server/target/contract-<backend>.json` | JSON report path |
| `PLANE_RS_CONTRACT_DEBUG` | unset | `1` prints each drain and each replay decision to stderr |
| `PLANE_RS_CONTRACT_ASSUME_RUST` | unset | route orders whose unsafe methods are treated as Rust-owned (`--assume-rust`) |
| `PLANE_RS_CONTRACT_METHODS` | unset | plan is narrowed to these methods (`--methods`) |

The run uses the shared dev stack through `apps/server/dev/django-env.sh`
(Postgres 55432, Valkey 56379, RabbitMQ 55672/55673). It never starts or stops
a container; `dev/status.sh` reports the stack. The plugin starts **one**
`plane-api` per session and stops it by PID; it creates the vhost and deletes
it again at the end.

`--reuse-db` (pytest.ini's default) keeps the database between runs, which is
what makes iterating cheap; the plugin still points `plane-api` at it. Pass
`--create-db` for a deterministic run (CI, a recorded result, or after a model
change). The database is `t_rsct_contract` and pytest-django creates drops it
itself; nothing else is touched.

## How it works

### Routing

`pytest_plane_contract` imports `apps/server/tools/proxy/gen_caddy.py` and calls
`load_inventory`, `load_owners`, `load_groups` and `build_plan` — the same
functions `gen_caddy.py --check` uses. The result is the proxy's `GroupPlan`
list, so a request is Rust-owned exactly when the generated Caddyfile would send
it to Rust: the group's `PLANE_RS_GROUP_<NAME>` flag is unset, empty or `rust`,
the method is in the cluster's method set, and the path full-matches the
cluster's anchored regex. Runner and proxy cannot disagree.

### Interception

`django.test.client.Client.request` is the one method every test client call
funnels through (`Client.get/post/...` → `RequestFactory.generic` →
`Client.request`; DRF's `APIClient` merges `force_authenticate` and
`credentials()` into the same call). The plugin wraps it: a Rust-owned request
is rebuilt from the WSGI environment the client produced (method, `PATH_INFO`
plus `QUERY_STRING`, `HTTP_*`/`CONTENT_TYPE`/`CONTENT_LENGTH`, `wsgi.input`,
cookies) and sent with `requests`; the wire response is converted back into a
Django `HttpResponse` with `status_code`, `content`, `.json()`, `.data` (parsed
JSON), headers and `.cookies`. Every other call reaches the original method, so
Django sees exactly the request it saw before.

### Authentication

`force_authenticate(user=…)` is a test-only shortcut: DRF attaches
`_force_auth_user` to the request and no cookie exists. For a Rust-bound request
the plugin creates a **real Django session** in the test database with the three
keys `django.contrib.auth.login` writes (`_auth_user_id`,
`_auth_user_backend`, `_auth_user_hash`) through the project's own
`SessionStore`, and sends it as the `session-id` cookie. One session per user
per test (the test database is flushed after each test, so nothing leaks).
`force_login`/`login` cookies already in the client's jar are forwarded as they
are; `credentials(HTTP_X_API_KEY=…)` becomes the real `X-API-Key` header.
Unsafe methods also get a CSRF cookie and a masked `X-CSRFToken` header.

An **inactive** forced user cannot be reproduced over HTTP — Django's session
backend, and therefore Rust, treats a deactivated user as anonymous, while
`force_authenticate` bypasses the backend. Such a request is served in-process
and reported as a limitation instead of being sent with a wrong identity.

### Database visibility

`plane-api` is a separate process, so it only sees committed rows. The plugin
makes every test transactional (`django_db(transaction=True)` semantics, via
`pytest_collection_modifyitems`, which mutates the marker or adds
`transactional_db` to the fixture closure), and `pytest_sessionstart` pins
`DATABASES["default"]["TEST"]["NAME"]` to `t_rsct_contract` before pytest-django
creates the database. `plane-api` starts from the `django_db_setup` fixture, so
the database always exists first, with the env `apps/server/dev/django-env.sh`
exports for `PLANE_DB`/`PLANE_REDIS_DB`/`PLANE_MQ_VHOST`.

### Side effects

Plane's settings have no `CELERY_TASK_ALWAYS_EAGER`, so a Django view's
`.delay()` publishes a real AMQP message — the broker is part of the test
environment. A Rust handler publishes to the run's vhost instead of calling
Python. After every Rust-bound request the plugin drains that vhost through the
RabbitMQ management API and, for each message whose task's `.delay` is
currently a `Mock` (`unittest.mock.patch("…delay")` replaces the attribute on
the shared task object), calls that mock with the message's `args`/`kwargs`,
restoring kombu's UUID envelopes to `uuid.UUID`. Messages for unmocked tasks are
dropped: a Django request would only have published them too. Nothing is
skipped — an assertion on a patched `.delay` sees the Rust publish.

Redis needs no bridge: Django's cache and `plane-api` use the same Redis
database, and a Rust request performs the same cache writes Django would. The
one adjustment is `plane.settings.redis.redis_instance`, which upstream builds
with `db=0` regardless of `REDIS_URL`; the plugin keeps it on the same database
as the cache (`settings.REDIS_URL`) so the run cannot collide with another
agent's `db 0` traffic. Views and tests both call the same function, so the
round trip is unchanged.

## Selecting the files

`dry_routing.py` parses each `test_*_app.py` module, recovers the URL template
of every test-client call from the AST (f-strings, module constants,
`.format()`, local variables assigned from a helper's `return`), generates
concrete sample paths for the placeholders, and matches them against the same
plan the runner uses. A file with at least one match is a file the mixed run
must cover. It is static: it never sets up a database or sends a request, and it
over-selects rather than missing a file (a conservative match only costs a
test that stays in-process).

## Reporting

The plugin prints, and writes a JSON report with, per test:

* the requests that went to Rust (method, path, status, group);
* the count of in-process requests;
* the outcome, any runner limitation, and the replayed task names.

The summary separates **tests that touched at least one Rust route** (with
pass/fail) from **tests that stayed Django-only**.

## Baseline

`--backend django` keeps the same wrapper, transactional tests and broker vhost
but never takes the Rust branch, so it is the control: every test that passes
plain upstream must pass here. The plain run used for comparison is the same
command without `-p pytest_plane_contract` (and a throwaway database, because
pytest-django would otherwise use its default `test_` prefix):

```sh
cd apps/api
PLANE_DB=t_rsct_plain PLANE_REDIS_DB=1 PLANE_MQ_VHOST=t_rsct_plain \
  DJANGO_SETTINGS_MODULE=plane.settings.test .venv/bin/python -m pytest plane/tests/contract -q
```

## Limitations

1. **Transactional tests.** Forcing `transaction=True` is what lets a second
   process see the test's rows. It also means sequences are reset and tables
   flushed per test (slower than upstream's savepoint rollback), and a test
   that relied on `TestCase`'s outer transaction (none does today) would
   behave differently.
2. **Inactive forced users** are served by Django (see above).
3. **`force_authenticate(token=…)`** (DRF token auth) has no HTTP equivalent in
   Plane and is not implemented; no contract test uses it.
4. **`follow=True` on a Rust route** is not implemented (the redirect walker
   needs a `redirect_chain`); the only `follow=True` calls in the suite are on
   Django-owned `/auth/` routes.
5. **Django-side patches do not apply to Rust.** A test that patches an object
   the view calls in-process — a storage wrapper, an OAuth provider class, a
   cache method, a task at its call site — can only be satisfied by Django.
   `_local_mock_reason` recognises the known shapes by effect (the patched
   attribute is a `Mock`) and serves that request in-process, reporting it as a
   runner limitation. Any other in-process patch fails visibly, because the
   report names every Rust-touching test.
6. **`redis_instance()` database** is forced onto the run's Redis database (see
   "Side effects").
7. **One `plane-api` per session**, started on `PLANE_RS_CONTRACT_PORT`. A
   second concurrent run needs a different port, database, Redis DB and vhost.
