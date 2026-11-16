# Porting a `/api/` handler

How a route moves from Django to Rust. Read `RUST_REWRITE.md` (the rules),
`AGENTS.md` (the working conventions) and `DECISIONS.md` (the stack) first; this
file is only the procedure for `/api/`.

The crate is `crates/plane-http-app`, one module per route group of
`deploy/route-groups.yaml`. A domain agent owns **one** directory,
`crates/plane-http-app/src/<group>/`. Everything else in the crate is shared:

| Path | Owner | When it changes |
| --- | --- | --- |
| `src/<group>/**` | the domain agent | freely |
| `src/lib.rs` (`register_all`, `AppState`) | orchestrator | a new group, or a deliberate order change |
| `src/common/**` | orchestrator | a second domain repeats the same helper |
| `Cargo.toml` | orchestrator | a new shared dependency |
| `tests/schema_check.rs`, `tests/registry.rs` | orchestrator | a new group |
| `scenarios/read/<group>.yaml` | the domain agent | add steps for routes you port |
| `ROUTES.md` | **orchestrator only** | after review |

A group's `register` starts empty and gains one handler per ported route.
Nothing else marks a route as reachable: an unregistered route keeps answering
the pipeline's 501, which is what the coexistence proxy relies on. "Ported" and
"verified" are different states — a route is verified when its scenario is clean
(§8), and the orchestrator flips `ROUTES.md` only then.

## 1. Read before you write

For every route, in this order. Each step can change the answer the previous one
gave you.

1. **The route row.** `apps/server/routes.json` (grep the path; the field
   tables at the top of `ROUTES.md` describe the same data). You need `view` (the dotted class),
   `kind`, `methods`, `permission_classes` + `method_permissions`,
   `use_read_replica`, `timezone_mixin`, `base_family`, `guard_actions`,
   `options_metadata`. `plane-drf` takes all of these from the row — you never
   declare a method, a permission or a path.

2. **The view.** `apps/api/` + the module path in `view`. Read the method you are
   porting *and* its base class (`plane/app/views/base.py` for `BaseViewSet`,
   `BaseAPIView`, `handle_exception`, `TimezoneMixin`). Query-parameter handling
   usually lives in the base class or a mixin, not in the method.

3. **The serializers.** `SERIALIZERS.md` has the field order, sources,
   `fields=`/`expand=` behaviour and the Rust struct name for every serializer;
   `serializers.json` is the machine-readable form. `manual` serializers (23 of
   them) have no generated struct and need a hand-written module — check the
   list before assuming a serializer is free.

4. **Permissions.** Already enforced by the runtime, but read them: they decide
   what the handler may assume (a member of the workspace? an admin?) and which
   kwargs are guaranteed to exist. `allow_permission` runs *before* the handler,
   so `slug`/`project_id`/`pk` have already been read once.

5. **Model hooks.** `MODEL_HOOKS.md`. For a read, the relevant part is the
   manager's predicate (`deleted_at IS NULL`) and any property the serializer
   calls (`Issue.state_group`, `Project.identifier`). For a write, every hook the
   write path runs (sequence numbers, slugs, sort orders, default states) must be
   called explicitly.

6. **Tasks and cache.** `TASKS.md` for the Celery tasks the method enqueues, and
   the decorators above the method for caching: `cache_page` (Django's page
   cache), `plane/utils/cache.py`'s `cache_response`/`invalidate_cache`, and
   anything that writes Redis by hand. A read that caches is **not** ported until
   the cache write matches; a write that enqueues is **not** ported until the
   publisher exists. Both live in other crates — `plane-cache` and `plane-jobs` —
   and neither is wired into `ViewCtx` yet (see "Side effects").

7. **The scenario.** `scenarios/read/<group>.yaml` already has generated steps
   for every GET of the group (from the rich fixture `plane_fix_tpl`). Run it
   before you change anything to see the starting point:

   ```sh
   cd apps/server
   ./target/debug/difftest run --scenario scenarios/read/meta.yaml --a django --b rust
   ```

## 2. Write the SQL

Django is the specification: reproduce the statements its ORM emits for this
view, not a nicer query. Two shapes, and the difference is enforced
(`DECISIONS.md` D1 and its 2026-11-16 amendment):

* **Static** — the SQL text never changes at runtime. Declare it in your group's
  `queries.rs` with `plane_query!`, name it, list the result columns with their
  Postgres types, and add it to `ALL_QUERIES`:

  ```rust
  plane_query! {
      /// `State.objects.filter(project_id=…).order_by("sequence")`
      /// (`plane/app/views/state/base.py:41`). Manager predicate from
      /// `plane/db/mixins.py:57`.
      STATES_BY_PROJECT = "states_by_project",
      "SELECT created_at, updated_at, …, id, name, …
       FROM states
       WHERE project_id = $1 AND deleted_at IS NULL
       ORDER BY sequence ASC",
      [("created_at", Timestamptz), …, ("name", Varchar)]
  }
  ```

  Fetch it with `client.prepare_cached(queries::STATES_BY_PROJECT.sql)`.
  `tests/schema_check.rs` prepares every catalogued statement against a
  Django-migrated database, so a renamed column or changed type fails in CI.

* **Dynamic** — the text depends on request data (`fields=`, filters, ordering,
  grouping). Compose it with `plane-query` (or build the text yourself) and send
  it as an **unnamed** statement: `client.prepare(sql)` or `client.query(sql,
  &params)`. Never `prepare_cached` on a dynamic string: deadpool's per-connection
  cache never deallocates, and replaying the filter corpus drove the dev Postgres
  out of memory (each distinct statement costs about 0.4 MiB of backend memory,
  ~800 shapes in the corpus).

Result columns are read **by index** on hot paths (`row.get(3)`), never by name.
Decode a whole model row with the generated `Model::from_row(row, start)` — the
generated `COLUMNS` and `from_row` are produced together, which is why a
catalogued statement repeats the column list in the model's order.

## 3. Fetch it in the same number of round trips as Django

The differential test compares bodies, not query counts, but latency and the
rewrite's reason for existing are query-shaped:

* Django's `select_related` → a join in your statement.
* Django's `prefetch_related` → **one extra query** for the whole set
  (`WHERE id = ANY($1)`), never one per row. A `.first()` per item is the N+1
  this rule exists to prevent.
* Independent queries (a count and a page, two aggregates) may be sent on the
  same connection concurrently: `futures_util::future::try_join(a, b)` on one
  `&Client` pipelines them. `tokio-postgres` sends the second before the first
  completes.
* One connection per request, as Django has: `common::client(ctx)` is the one
  the runtime checked out (`ViewCtx::db`), with the read-replica rule already
  applied from the route's `use_read_replica`. A write checks out
  `ctx.pools.write_pool()` itself; do not open a second read connection.

## 4. Render

Render through `plane-serializers` with the request's timezone — never
`serde_json::to_string` on a hand-built object:

```rust
use plane_core::error::APPLICATION_JSON;
use plane_drf::view::ViewResponse;
use plane_http_app::common::{RequestRender, render, render_list};

// One object: `fields=`, `expand=` and the timezone in one call.
let body = render(ctx, &StateLiteSerializer { id, name, group })?;
Ok(ViewResponse::raw(200, APPLICATION_JSON, body))

// `many=True`: build the context once, render every item through it.
let prepared = RequestRender::from_ctx(ctx);
let entries: Vec<StateLiteSerializer> = /* … */;
Ok(ViewResponse::raw(200, APPLICATION_JSON, render_list(&entries, &prepared.render_ctx())?))
```

Both helpers return `Result<_, ViewError>`; a render failure is a 500 with the
Python exception's shape (`common::view_error` explains why).

* `common::render(ctx, &serializer)` builds the `RenderCtx` from `?fields=`,
  `?expand=` and the user's timezone in one call. In a loop, build
  `RequestRender::from_ctx(ctx)` once and reuse `.render_ctx()`.
* The timezone is mandatory and comes from `ViewCtx::timezone`; `None` means
  `settings.TIME_ZONE` = UTC (a route without `TimezoneMixin`). A datetime must
  never be formatted by hand — `DECISIONS.md` D2.
* Key order is Python dict order. The workspace's `serde_json` is built without
  `preserve_order`, so a `json!({...})` with more than one key serialises
  alphabetically: use `plane_core::json::Object` or a `#[derive(Serialize)]`
  struct with the fields in output order, and render it with
  `plane_serde::to_vec(Dialect::Drf, …)`.
* A body that is not a serializer at all (a dict built in the view, like
  `/api/timezones/`) still goes through `Dialect::Drf`; that formatter is DRF's
  `JSONRenderer` (compact separators, `ensure_ascii=False`, Python float repr).

## 5. Register

Registration is the only way a route becomes reachable, and it refuses anything
`routes.json` does not cover:

```rust
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    // A ViewSet: one line per action the policy maps.
    registry
        .viewset("plane.app.views.state.base.StateViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?;
    // An APIView: one line per method the class defines.
    registry
        .api_view("plane.app.views.timezone.base.TimezoneEndpoint")?
        .method(Method::GET, timezones::get)?;
    Ok(())
}
```

`viewset` takes the ViewSet action (`list`, `retrieve`, `create`, …); `api_view`,
`django_view` and `function` take the HTTP method. `HEAD` falls back to `GET` and
`OPTIONS` is answered from the policy, so do not register them. Registering the
same action twice, an action the view does not answer, or a `/api/v1/` route is a
startup error — fix the cause, never the registry.

## 6. Map errors

A handler returns `Result<ViewResponse, ViewError>`. Pick the variant that names
the exception the Python raises at that point; the family's `handle_exception`
decides the body (`HTTP_CONTRACT.md` §11):

| Python | `ViewError` | Response |
| --- | --- | --- |
| `Http404`, DRF `NotFound` | `Api(ApiError::NotFound)` | 404 `{"detail": "Not found."}` |
| `Model.DoesNotExist` / `get_object_or_404` | `ObjectDoesNotExist` | 404 `{"error": "The required object does not exist."}` |
| `kwargs["slug"]` missing (`KeyError`) | `Key` | 400 `{"error": "The required key does not exist."}` |
| `ValidationError` from a UUID/field coercion | `Validation` | 400 `{"error": "Please provide valid detail"}` |
| `IntegrityError` | `Integrity` | 400 `{"error": "The payload is not valid"}` |
| DRF `ValidationError` from a serializer | `Api(ApiError::ValidationError(..))` | 400, serializer error structure |
| `PermissionDenied("…")` (view-level re-check) | `Api(ApiError::PermissionDenied(..))` | 403 with that detail |
| anything else | `Unknown(String)` | 500, logged by name, never rendered |

`common::lookup::Error` already maps the first three of those. Do not build
error bodies by hand: a `serde_json::json!({"error": …})` sorts keys and loses
Python's order.

## 7. Side effects

Behaviour includes what the request writes, creates, enqueues and invalidates
(`RUST_REWRITE.md` rule 5). Two of those are not available yet:

* **Jobs.** A route that enqueues a Celery task stays on Django until
  `plane-jobs` publishes with the same task name, kwargs and retry policy
  (`TASKS.md` records each call site's argument types). Do not port a handler
  that silently drops its task.
* **Cache.** `cache_page` and `plane/utils/cache.py` write django-redis keys
  (pickled values, `:<version>:` key prefix). `plane-cache` owns the codec and
  the page cache; until it is wired into `ViewCtx` such a route is *not* verified
  and must not be marked owned. Query-cache invalidation is part of the write
  path: a write that leaves a stale Django cache entry is a behaviour change.

When blocked on either, still port the read (the handler is useful and testable)
but say so in the report and do not claim the route is verified.

## 8. Prove it

```sh
cd apps/server
cargo build -p plane-api                       # the harness runs this binary
./target/debug/difftest run --scenario scenarios/read/<group>.yaml --a django --b rust
./target/debug/difftest run --scenario scenarios/read/<group>.yaml --a django --b rust \
  --report target/<group>-report.json          # every difference, with the rule that applied
```

Extend `scenarios/read/<group>.yaml` with the shape you ported: one step per
response shape (default, `?fields=`, `?expand=`, one filter, one order, a page
border, an error path) and the write steps for a write route. A step is a
request, so capture ids from previous responses rather than hard-coding them.

Rules the harness enforces, so do not fight them:

* It compares status, headers (except `date`, `server`, and `content-length` when
  normalisation ran), the JSON body with key order and number lexemes kept, the
  database diff, the Redis diff and the drained Celery messages.
* A difference is a real difference until you find its cause. Normalisation is
  per target and cross-referenced (a generated UUID, `<now>`, the target's base
  URL), so it cannot hide a mismatch; adding a rule to hide one weakens every
  later port (`scenarios/README.md`, "Normalisation policy").
* Two clock-based values are **not** normalised yet, so a `cache_page` route can
  flake at a second boundary: the response `Expires` header (`now + timeout`, no
  `volatile_headers` mode parses an HTTP date) and a cache-hit `Age`. Adding a
  `http_date` rule mode next to the cookie `Expires` handling
  (`plane-testkit/src/normalize.rs`, `relative_expiry`) is the fix; until then a
  run that differs only on `Expires` by one second is the harness, not the port.

Then, and only then: report the group, the routes, the scenario, the run result
and anything you could not port. **The orchestrator flips `ROUTES.md`** (and the
proxy config generated from it) after reviewing; the agent never edits that file.

## Worked example: `GET /api/timezones/`

The full port is `crates/plane-http-app/src/meta/timezones.rs`. It is the
reference because it exercises the whole loop with the fewest moving parts, and
because its one side effect is instructive.

* **Row.** `routes.json` → `/api/timezones/`, `view`
  `plane.app.views.timezone.base.TimezoneEndpoint`, `kind: APIView`,
  `methods: {get}`, `permission_classes: [AllowAny]`, `timezone_mixin: false`,
  `use_read_replica: false`, `group: meta`.
* **View.** `apps/api/plane/app/views/timezone/base.py:20-210`. A 120-entry
  table of `(label, IANA zone)`, each with the zone's current offset, sorted by
  `(offset, label)`. Two quirks are behaviour: the sort key is
  `int(strftime("%z"))` (`-930` for `-09:30`), and the offset string uses floor
  division, so `Pacific/Marquesas` (`-09:30`) renders `UTC-10:30`. Both are ported
  and pinned by unit tests.
* **Serializers.** None: the view builds dicts and returns `Response({...})`, so
  the port builds the same objects in the same order and renders them with
  `plane_serde::to_vec(Dialect::Drf, &Envelope { .. })`.
* **Hooks, tasks.** None. No model is touched.
* **Cache.** `@method_decorator(cache_page(60 * 60 * 2))` on `get`. That is
  Django's `CacheMiddleware`: it patches `Expires` and `Cache-Control: max-age=7200`
  onto the response (ported, they are visible to the client) and writes two
  django-redis keys with a 7200 s TTL whose value is a pickled
  `rest_framework.response.Response` (not ported: `plane-cache`). The handler's
  module docs spell out what that means for the differential run.
* **Register.** `registry.api_view("plane.app.views.timezone.base.TimezoneEndpoint")?`
  then `.method(Method::GET, get)?`.
* **Unit tests.** The body is asserted byte for byte against
  `tests/golden/timezones-2026-11-16.json`, recorded from the Python algorithm at
  a fixed instant — which also proves chrono-tz and pytz agree on all 120 zones.
  `test the_negative_half_hour_quirk_is_ported` pins `UTC-10:30`.
* **Scenario.** `scenarios/read/meta.yaml`, steps `meta-0826-timezone-endpoint`
  and `meta-0827-timezone-endpoint` (signed-in admin and member; `AllowAny`, so
  the session only exercises the cookie/Vary path).
* **Result (2026-11-16).** A cold request compared on its own matches Django
  completely — status, every header (`Allow`, `Vary`, `Expires`,
  `Cache-Control: max-age=7200`, gzip, `Content-Length`) and the 12,011-byte
  body — with `--no-redis`; the full group scenario reports exactly two
  differences, both from the unported `cache_page`:

  ```text
  redis :1:views.decorators.cache.cache_header..<md5>.en-us.UTC   added on A, absent on B
  redis :1:views.decorators.cache.cache_page..GET.<md5>.<md5>.en-us.UTC
                                                                  added on A, absent on B
  ```

  and the second timezone step adds `headers age: 0` (only on the cache hit).
  Until `plane-cache` provides that page cache, this route is **not** verified,
  which is why the example also documents the consequence instead of skipping
  the step. The four `unsplash` steps in the same scenario keep answering 501
  because that route is not ported.

## Naming

* Module and file names follow the Django source: one file per Python module
  under `plane/app/views/<domain>/`, same stem. A group whose views live in one
  Python file gets one Rust file.
* Handler functions are named after what Django calls them: `list`, `create`,
  `retrieve`, `partial_update`, `destroy` for ViewSet actions, `get`/`post`/… for
  an `APIView` method, `health` for a function view.
* Each file exposes `pub fn register(registry: &mut Registry) -> Result<(), RegistryError>`;
  the group's `mod.rs` calls them in one place.
* Catalogue identifiers are `UPPER_SNAKE` and the name is `<group>_<purpose>`
  (`states_by_project`), unique across the crate (a test checks it). Table
  aliases are one or two letters (`s`, `i`); a generated SQL fragment takes
  `{alias}` (`with_alias`).
* No `unwrap`/`expect` on a request path. An invariant that truly cannot fail
  (a constant table rendering) may `expect` with the reason in the message.

## Performance checklist

* Statements: catalogued if static, unnamed if dynamic. No `prepare_cached` on a
  string built with `format!`.
* One connection per request; independent queries pipelined, dependent ones
  sequential.
* No N+1: a relation Django prefetches is one extra query for the whole page.
* Columns read by index; `SELECT` lists name columns explicitly (no `SELECT *`,
  whose order changes with the schema).
* Allocate once: `Vec::with_capacity`, render into one `Vec<u8>`, borrow instead
  of cloning a `String` you do not keep.
* CPU-heavy work (xlsx, images, password hashing) on `spawn_blocking`.
* Measure when in doubt: `plane_query::stats::statements()` counts statements, and
  `dev/` plus `BENCH.md` describe the harness.

## Reviewer checklist

* [ ] The route's `owner` in `ROUTES.md` is still `django`, and the report says
      which routes should change and to what (`rust(GET)`, …).
* [ ] `register` names the dotted view from `routes.json`; no path, method,
      permission, throttle or `Allow` order is declared in Rust.
* [ ] Every static statement is in `<group>/queries.rs`, listed in `ALL_QUERIES`,
      and `cargo test -p plane-http-app --test schema_check` passes against a
      migrated database.
* [ ] No dynamic SQL goes through `prepare_cached`.
* [ ] Query count is no worse than Django's for the same request (join where it
      joins, one prefetch query where it prefetches).
* [ ] Rendering goes through `plane-serializers` (or `Dialect::Drf` for a
      hand-built body) with `ViewCtx::timezone`; no key order comes from
      `serde_json::Map`.
* [ ] Errors are `ViewError` variants, not hand-built bodies.
* [ ] Side effects: tasks, cache writes/invalidations, activity rows — either
      ported or explicitly reported as blocking.
* [ ] The scenario has a step for each new response shape and the run is clean.
* [ ] A non-obvious line cites the Python file and line it came from.
* [ ] `cargo fmt --all --check`, `cargo clippy -p plane-http-app -p plane-api
      --all-targets -- -D warnings`, `cargo test -p plane-http-app -p plane-api`.
