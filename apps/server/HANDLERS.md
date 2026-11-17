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
| `tests/schema_check.rs`, `tests/registry.rs` | orchestrator | a new group, or a new `*queries.rs` catalog (mechanical: add its `ALL_QUERIES`) |
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
   anything that writes Redis by hand. Both handles are on `ViewCtx` and the
   helpers are in `common` (§7); a caching read is **not** ported until its
   cache write matches, and a write that enqueues is **not** ported until the
   task is published with the same name, kwargs and argument types
   (`TASKS.md` §8.3).

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
  An array result column is declared `("label_ids", ArrayOf(Uuid))` (or
  `ArrayOf(Varchar)`, …); `plane_query!` maps it to `PgType::ArrayOf(…)`, so a
  catalog never needs a local macro. `tests/schema_check.rs` prepares every
  catalogued statement against a Django-migrated database, so a renamed column
  or changed type fails in CI. It also discovers every `*queries.rs` file under
  `src/`: a new catalog must be added to its `catalogs()` list, and the test
  fails until it is, so no domain can go unchecked.

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

### Skippable annotations: `FieldValue`

A declared field with `required=False` whose `source` is a *view annotation*
can be missing from the instance. DRF then raises `SkipField` and writes no key
(`rest_framework/fields.py:421-446`, `serializers.py:530-551`), but when the
attribute exists and is `None` it writes `"key": null`. The generated struct
uses `FieldValue<T>` for those fields, never `Option<T>`:

```rust
use plane_serializers::FieldValue;

// `is_favorite` is an annotation this queryset does not add: no key.
is_favorite: FieldValue::absent(),
// `sort_order` is annotated and the subquery returned NULL: `"sort_order": null`.
sort_order: FieldValue::null(),
// `member_role` is annotated with a value.
member_role: FieldValue::value(20),
// An outer-join/`MAX(...)` annotation that always exists: Some → value,
// None → null. This is the common case.
anchor: FieldValue::from_attribute(row.anchor.as_deref()),
```

A nullable *model column* is still `Option<T>` (`None` → `null`); it can never
skip because its attribute always exists. Do not use `FieldValue::null()` for
a field whose Python attribute is missing and do not use `absent()` for one
that is `None`: the bytes differ and the golden corpus in
`crates/plane-serializers/tests/golden.rs` pins both cases per annotation.

### JSON columns on the write path

A generated row's `json`/`jsonb` column is `plane_db::JsonText`. Bind one with
`ToSql` (it writes the jsonb version byte) and build the value a handler just
wrote from the same ordered map, so a create/update response renders Django's
in-memory dict order instead of jsonb's length-then-bytes order:

```rust
use plane_db::JsonText;

let props = JsonText::from_json_dict(&body)?;   // `json.dumps` order/format
client.execute(queries::UPDATE_PROJECT, &[&props, &id]).await?;
let body = render(ctx, &row /* or the same JsonText through PgJson */)?;
```

`JsonText::from_json(&plane_serde::pyjson::Value)` is the general form;
`JsonText::new(text)` wraps text from anywhere else.

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
same action twice or an action the view does not answer is a startup error —
fix the cause, never the registry. `/api/v1/` views register like any other.

## 6. Map errors

A handler returns `Result<ViewResponse, ViewError>`. Pick the variant that names
the exception the Python raises at that point; the family's `handle_exception`
decides the body (`HTTP_CONTRACT.md` §11):

| Python | `ViewError` | Response |
| --- | --- | --- |
| `Http404`, DRF `NotFound` | `Api(ApiError::NotFound)` | 404 `{"detail": "Not found."}` |
| `get_object_or_404` (`django/shortcuts.py:76-84`) | `Api(ApiError::not_found_for::<Model>())` | 404 `{"detail": "No <Model> matches the given query."}` |
| `raise NotFound("<detail>")` | `Api(ApiError::not_found_detail("<detail>"))` | 404 `{"detail": "<detail>"}` |
| `Model.DoesNotExist` / `get_object_or_404` | `ObjectDoesNotExist` | 404 `{"error": "The required object does not exist."}` |
| `kwargs["slug"]` missing (`KeyError`) | `Key` | 400 `{"error": "The required key does not exist."}` |
| `ValidationError` from a UUID/field coercion | `Validation` | 400 `{"error": "Please provide valid detail"}` |
| `IntegrityError` | `Integrity` | 400 `{"error": "The payload is not valid"}` |
| DRF `ValidationError` from a serializer | `Api(ApiError::ValidationError(..))` | 400, serializer error structure |
| `PermissionDenied("…")` (view-level re-check) | `Api(ApiError::PermissionDenied(..))` | 403 with that detail |
| anything else | `Unknown(String)` | 500, logged by name, never rendered |

`common::lookup::Error` already maps the first three of those, and
`ViewError: From<plane_serializers::DrfError>` covers every render failure. Do
not build error bodies by hand: a `serde_json::json!({"error": …})` sorts keys
and loses Python's order.

## 7. Side effects

Behaviour includes what the request writes, creates, enqueues and invalidates
(`RUST_REWRITE.md` rule 5). Both handles are on `ViewCtx`, built once at startup
and shared; the runtime never allocates one per request.

### Cache

`ctx.cache()` is `django.core.cache.cache` as `plane_cache::Client`: the same
`:1:` keys, the raw-int-versus-pickle value rule and the protocol-4 pickler
django-redis uses (`plane-cache`). Through `common::cache` you get the two
`plane/utils/cache.py` decorators as functions:

```rust
use plane_cache::PyValue;
use plane_http_app::common::cache;

// `@cache_response(60 * 60 * 2)` on `WorkspaceLabelsEndpoint.get`
// (`plane/app/views/workspace/label.py:21`). `compute` runs only on a miss and
// returns the `(status, data)` the view would have returned; `data` is a
// `PyValue` (DRF's `serializer.data`, UUIDs included), so the stored pickle is
// Django's and a hit renders the same bytes the miss did.
cache::cache_response(ctx, 60 * 60 * 2, None, true, || async move {
    let labels = load_labels(ctx).await?;
    let data = PyValue::return_list(labels.iter().map(label_value).collect());
    Ok((200, data))
})
.await
```

The pickler memoises by **object identity**, so an equal-but-distinct string
produces a second literal where Django emits `BINGET`; above 4 KiB the harness
only compares size, so this is checked by
`crates/plane-cache/tests/cache_pickle.rs` against the two real cached
endpoints. Build the tree with `plane_cache::PyInterner`:

```rust
use plane_cache::{PyInterner, PyValue};

let mut strings = PyInterner::new();
let rows = labels.iter().map(|label| {
    let name = strings.value(&label.name);          // fresh: decoded from a row
    let kind = strings.choice(&label.r#type);       // shared: a `choices` constant
    strings.mapping([
        ("name", name),
        ("type", kind),
        ("id", PyValue::uuid(label.id)),
    ])
});
let data = PyValue::return_list(rows.collect());
```

`value` is for every string read from Postgres (and for `isoformat()`/`str()`
output): fresh, except the empty string and single Latin-1 characters, which
CPython caches. `choice` is for a `ChoiceField` value, which DRF returns from
its class-level choice map — the same interned object as a field name with that
text. `key` (used by `mapping`) is for dict keys. A `ReturnDict` needs
`PyValue::return_mapping` with interned keys, not `return_dict`.

A cached `serializer.data` holds the `DateTimeField`'s **string**, not a
`datetime`: `plane_serde::values::DrfDateTime::to_drf_string()` (or its
`Display`) returns the exact `isoformat()` text, including the `Z` and the
request timezone, without going through the JSON encoder.

```rust
use plane_http_app::common::cache;

// `@invalidate_cache(path="workspaces/:slug/states/", url_params=True,
// user=False)` on `StateViewSet.create` (`plane/app/views/state/base.py:56`):
// it runs *before* the view body, like the decorator.
cache::invalidate_cache(ctx, Some("workspaces/:slug/states/"), true, false, false).await?;
```

`path=None` means `request.get_full_path()`; `user=true` appends
`str(request.user.id)` for an authenticated caller; `multiple=true` deletes
every key containing the computed key (`cache.keys("*<key>*")`). A write that
leaves a stale cache entry is a behaviour change, so the invalidation calls are
part of the port, not an optimisation.

A page-cached route (`cache_page`) uses `plane_cache::page` directly, because
there is exactly one such route: `src/meta/timezones.rs` is the reference.

### Jobs

`ctx.celery()` is the process's Celery publisher (DECISIONS.md D7);
`common::jobs::publish` awaits it and returns Celery's task id. A publish that
fails after the retry budget is a 500, as in Django. `recent_visited_task` has
two keyword orders in the tree, selected by which Python call site you mirror:

```rust
use plane_drf::error::ViewError;
use plane_http_app::common::jobs;
use plane_jobs::celery::args::TaskArg;

let project_id = ctx.kwarg("project_id").ok_or(ViewError::Key)?.to_string();
let user = TaskArg::uuid(jobs::user_id(ctx)?);      // `user_id=request.user.id`
// `issue/base.py:304`: slug, project_id, entity_name, entity_identifier, user_id.
jobs::recent_visited_project(
    ctx,
    "project",
    TaskArg::str(project_id.clone()),
    Some(TaskArg::str(project_id)),
    user,
)
.await?;
// `issue/base.py:617`: the other order. Use `recent_visited_entity`.
```

Every other task has a typed constructor in `plane_jobs::celery::tasks` with
the call site's argument types (`TASKS.md` §8.3); publish it with
`jobs::publish(ctx, &message)`. Do not build a `Message` by hand.

### Token log

`/api/v1/` handlers need no token-log code: layer 11 publishes
`process_logs` for every request carrying `X-Api-Key`, whatever the response,
and swallows a publish failure the way `logger.py:160-161` does. The registry
accepts v1 views.

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
* Page-cache entries are bridged rather than compared: Django's
  `views.decorators.cache.*` keys and Rust's `plane_rs.cache_page.*` keys are
  matched by presence and a 10-second TTL bucket, their values are not compared
  (DECISIONS.md D5a). A `cache_page` response's `Expires` is compared as
  `<expires+7200s>` from the step clock, and a cache hit's `Age` with a
  one-second tolerance, so a second boundary cannot flake a run. One side
  sending `Age` and the other not is always a difference.
* In managed mode A and B each allocate their port from their own target's
  `port_range` in `targets.yaml` (A from target A's range, B from B's), and the
  run header prints each target's URL and Redis index. `--a-redis-db` /
  `--b-redis-db` override the scenario's `redis_db` in managed mode too, so a
  scenario shared with another agent can use private indexes without a copy.

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
  Django's `CacheMiddleware`, and the port reproduces everything a client can
  observe with `plane_cache::page`: `patch_response_headers(7200)` (`Expires`
  and `Cache-Control: max-age=7200`), the stored status/headers/body served
  unchanged for 7200 s, `Age` on a hit, `GET`/`HEAD` only, `200` only, and the
  `private`/`no-cache`/`no-store`/`Vary: *` guards. The key is
  `plane_rs.cache_page.<method>.<md5(absolute_uri)>.<md5(vary)>.<language>.
  <timezone>` — Rust's own namespace, so neither backend ever unpickles the
  other's value (DECISIONS.md D5a). The handler's module docs spell out the two
  facts that make the `Vary` list empty, as it is for Django.
* **Register.** `registry.api_view("plane.app.views.timezone.base.TimezoneEndpoint")?`
  then `.method(Method::GET, get)?`.
* **Unit tests.** The body is asserted byte for byte against
  `tests/golden/timezones-2026-11-16.json`, recorded from the Python algorithm at
  a fixed instant — which also proves chrono-tz and pytz agree on all 120 zones.
  `test the_negative_half_hour_quirk_is_ported` pins `UTC-10:30`.
* **Scenario.** `scenarios/read/meta.yaml`, steps `meta-0826-timezone-endpoint`
  and `meta-0827-timezone-endpoint` (signed-in admin and member; `AllowAny`, so
  the session only exercises the cookie/Vary path).
* **Result (2026-11-16).** Both timezone steps match Django completely — status,
  every header (`Allow`, `Vary`, `Expires`, `Cache-Control: max-age=7200`,
  gzip, `Content-Length`, and `Age` on the hit) and the 12,011-byte body — with
  the page-cache keys present on both sides in their own namespaces. The four
  `unsplash` steps in the same scenario still answer 501 vs 200 because that
  route is not ported: it needs the outbound HTTP client (DECISIONS.md D9).

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

## Writes (Phase 5)

A write handler is the same registration and the same runtime policy as a read,
plus a pipeline with one job: **write the row Django writes, then answer with the
bytes Django answers**. The pieces live in `src/write/` (request body →
validated values), `src/model_hooks/`-equivalent `plane_db::hooks` (the
`MODEL_HOOKS.md` hooks) and, per domain, one `src/<group>/write.rs`.

### 1. The handler is the Python view, step for step

Read the view, the *writable* serializer and the model `save()` first, then
write the handler in the Python's statement order, including its error paths.
`src/stickies/write.rs` is the reference:

```rust
let client = common::client(ctx)?;                   // the request's connection
let workspace = lookup::workspace_by_slug(client, slug).await?;   // 404 body
let mut serializer = Serializer::new(ctx, /* partial = */ false)?; // body parse 400/415
apply_body(&mut serializer);                          // fields, declaration order
let values = match serializer.finish() {              // 400 serializer.errors
    Ok(values) => values,
    Err(errors) => return Ok(errors.into_response()),
};
```

Every statement and its order is part of the behaviour (`HANDLERS.md` §3 still
applies): the create above is workspace → `MAX(sort_order)` (the hook) →
`INSERT`, like `Sticky.objects.create`. The response is rendered from the
values you wrote (`JsonText::from_json` for a JSON column), never re-read —
`serializer.data` comes from the in-memory instance.

### 2. The body and validation

`plane-drf` already parses the body exactly as DRF does (`ctx.data()`: JSON
`ParseError`/415, forms, multipart, empty body). `write::Serializer` is the
validation half:

| DRF | Rust |
| --- | --- |
| `Field.validate_empty_values` (required/null/partial) | `Serializer::{char,float,datetime,json}_field` |
| `CharField` (coerce numbers, trim, blank, max_length) | `write::fields::char` + `CharOptions` |
| `FloatField` (Python `float()`, finite only) | `write::fields::float` |
| `DateTimeField` (iso-8601, make aware in the active timezone) | `write::fields::datetime` |
| `JSONField` (any JSON value; form text is parsed) | `write::fields::json` |
| `serializer.errors` shape and order | `ValidationErrors::into_response` |

Rules that are easy to break: fields are visited in **declaration order** (the
error keys' order), every failing field is reported (not just the first),
`read_only` fields are never read from the body, `partial=True` skips every
absent field, and a non-object body is `non_field_errors` (`data=None` is the
"No data provided" special case). A form body has html-input semantics
(`''` can mean `null` or "absent"); `CharOptions` carries `allow_blank`.

### 2a. Generated field tables

Retyping DRF's field list into `apply_body` is exactly the kind of work
codegen should do. `tools/inventory/serializers.py --rust` writes one module
per serializer to `crates/plane-http-app/src/write/generated/`:

* `FIELDS` — the writable fields in `self.fields` order (DRF's validation
  order), with the effective `required`/`allow_null`/`allow_blank`, default,
  length/range bounds, choices, input formats, related model and children;
* `READ_ONLY_FIELDS` — declared read-only fields, `Meta.read_only_fields`, and
  `editable=False` model fields, including ones DRF drops entirely;
* `EXTRA_KWARGS` — the `Meta.extra_kwargs` overrides;
* `validate` — the helper calls a handler would make today, in the same order.
  It is `async` and takes the request's client: a writable
  `PrimaryKeyRelatedField` in the body runs one `SELECT 1 FROM <model table>
  WHERE id = $1`, the same single `queryset.get(pk=…)` Django runs.

Regenerate after changing `apps/api`:

```sh
apps/api/.venv/bin/python apps/server/tools/inventory/serializers.py --rust
```

`--check` fails when the committed modules are stale, the same way
`gen_caddy.py --check` does. The default set is the work-items domain plus
`StickySerializer` and `ProjectSerializer`; `--write-serializers A,B` overrides
it.

When a field class has no helper in `write/`, `validate` emits a
`// TODO(kind)` comment instead of guessing, and the generator's summary lists
every TODO. The field helpers now cover `primary_key` (one `SELECT 1 FROM <model table>
WHERE id = $1`; DRF's `does_not_exist` and Django's UUID message),
`primary_key_list_field`/`primary_key_many_field` (per-element lookups;
`not_a_list`, `empty`, and the `ListField` per-index error object), `date`,
string `choice`, and `integer` with `min_value`/`max_value`/
`max_string_length`. Remaining TODOs are non-primary-key lists
(`IssueCommentSerializer.attachments` is a URL list, the read serializers'
`*_ids` are UUID lists), nested serializers and file uploads
(`IssueAttachmentSerializer.asset`; the v1 route is multipart/S3, not JSON).
Add the missing helper to `write/` first; do not hand-write around the TODO.

The two hand-ported pilots are the calibration: `--write-equivalence` compares
the generated table with `stickies/write.rs::apply_body` and
`projects/write.rs::read_body` and reports field order, kind and option
differences. It is a diagnostic (the hand-written side can be the one that is
wrong), so it is not part of `--check`.

### 3. CSRF

Nothing to do in a handler: the dispatcher runs
`BaseSessionAuthentication.enforce_csrf` (`plane/authentication/session.py:56-63`)
before it, which for `/api/` is an **origin check only** — there is no CSRF
token on this surface. A mismatched `Origin` is the 403
`{"detail": "CSRF Failed: origin not allowed"}`. `/api/v1/` API-key callers are
exempt, as in DRF.

### 4. Transactions, audit, timestamps

**Use the request's connection**: `common::client(ctx)` is the client the
runtime checked out. Never open a second one — Django uses one connection per
request, and the route's `use_read_replica` rule already routes every non-GET
method to the primary (`plane/middleware/db_routing.py:38-45`).

Django runs with `ATOMIC_REQUESTS` unset: a view is autocommit unless it opens
`transaction.atomic()` itself. Do not add one that the Python lacks (a rollback
would change what a concurrent reader sees). Where the Python has one, use
`client.transaction()` directly.

`BaseModel.save()` is H01: use `plane_db::hooks::audit::Audit::on_create(user)`
(`created_by = user`, `updated_by = NULL`) or `Audit::on_update(user)`. The
timestamps are the ORM's, not the database's: `write::now()` (microseconds) for
`created_at`/`updated_at`. Never rely on a column default.

### 5. Model hooks

`plane_db::hooks` holds one function per `MODEL_HOOKS.md` row, returning the
values to write. A write path **must** call its model's hooks before building
the statement: `hooks::sticky::before_create` (strip the HTML, derive
`sort_order`) / `before_update` (strip only). A domain ports the hooks it needs
next to the existing ones and adds the row to the table in `hooks/mod.rs`.

### 6. Delete

`SoftDeleteModel.delete(soft=True)` is `write::soft_delete` (H03): one
full-row update (`deleted_at` = `updated_at` = `now`, `updated_by`) followed by
`write::publish_soft_delete_cascade` (the `soft_delete_related_objects`
Celery message; the cascade itself runs in the worker).

### 7. Prove it

`scenarios/write/<group>.yaml`: create (empty, minimal, full, read-only fields
in the body, unknown fields), every reachable validation error, non-member and
guest callers, a mismatched `Origin`, PATCH (partial, read-only, invalid),
DELETE then a GET list (the soft delete must be gone on both sides), and a
second request. Capture created ids with `capture: {source: json, pointer: /id}`.
Run Django-vs-Django first, then Django-vs-Rust. The harness compares the DB
diff after every step, so a missed hook fails the step.

### 8. Sessions, logout and cache counters

Handlers reach the request's session through `ctx.session()`:
`plane_auth::SessionSlot`, the same `SessionStore` the pipeline built from the
cookie and will save at response time (Django's `request.session`). It is a
`tokio::sync::Mutex` handle because the response phase needs the store after
the handler returns; `ctx.session().lock().await` is the escape hatch for
anything the convenience methods do not cover.

| Python | Rust |
| --- | --- |
| `django.contrib.auth.logout(request)` | `ctx.session().flush(client).await` (+ `ctx.request.ctx.user` is replaced by the dispatcher, and the middleware deletes the cookie and adds `Vary: Cookie`) |
| `Session.objects.filter(user_id=…).delete()` | `plane_auth::delete_all_for_user(client, user_id).await` |
| `request.session.session_key` | `ctx.session().session_key().await` |
| `request.session.modified = True` | `ctx.session().lock().await.modified = true` (or `set`/`pop`) |

`flush` calls `SessionStore::flush` (clear + delete row + no key), exactly
`logout()`. Do not set the user yourself: the dispatcher reads the slot's
`logged_out` flag and applies `request.user = AnonymousUser` after the handler,
the same order Django uses (before `finalize_response` and the request logger).

`cache.add(key, value, timeout)` and `cache.incr(key)` are on
`plane_cache::Client` with django-redis's semantics: `add` is `SET … NX PX`
(non-positive timeout only reports presence, never deletes), `incr` is the
existing Lua `EXISTS`+`INCRBY` and **keeps** the key's TTL, and a missing key is
`Error::MissingKey` (Python's `ValueError`). Use `write::email::validate_email`
and `write::email::contains_url` rather than the DRF fields for
`UserEndpoint._validate_new_email` and `UserSerializer.validate_*_name`.
