# Estimates — port plan (read-only)

Area: `plane/app/views/estimate/base.py` (`BulkEstimatePointEndpoint`, `EstimatePointEndpoint`)
and the `estimates` selectors in `apps/server/deploy/route-groups.yaml:136-141`. The two GETs are
already Rust (`crates/plane-http-app/src/estimates/base.rs:34-149`); this plan covers the write
verbs of orders 46–49. `plane/api/urls/estimate.py` is never included (`ROUTES.md:76`), so no
`/api/v1/` route is in scope.

## 1. Inventory

Prefix `/api/workspaces/{slug}/projects/{project_id}/`. "Owner" = `ROUTES.md`'s Owner cell.

| order | path | write methods | python | permission | owner after | set |
| --- | --- | --- | --- | --- | --- | --- |
| 46 | `estimates/` | POST:`create` | `base.py:64-101` | `ProjectEntityPermission` (`permissions/project.py:135-166`) | `rust(GET)` → `rust(GET,POST)` | A |
| 47 | `estimates/{estimate_id}/` | PATCH:`partial_update`, DELETE:`destroy` | `base.py:109-144`, `:147-150` | same | `rust(GET)` → `rust(GET,PATCH,DELETE)` | A |
| 48 | `estimates/{estimate_id}/estimate-points/` | POST:`create` | `base.py:155-179` | `IsAuthenticated` + `allow_permission([ADMIN,MEMBER], PROJECT)` (`:154`; `permissions/base.py:19-103`) | `django` → `rust(POST)` | B |
| 49 | `…/estimate-points/{estimate_point_id}/` | PATCH, DELETE | `base.py:182-194`, `:197-268` | same (`:181`, `:196`) | `django` → `rust(PATCH,DELETE)` | B |

(`ROUTES.md:135-139`; `plane/app/urls/estimate.py:21-40`; `routes.json` orders 46–49.) Every
pattern passes exactly the kwargs its handler declares, so — unlike webhooks — no verb maps onto a
signature that cannot take it: **there is no `TypeError` 500 family here.** 46/47 also answer HEAD
through `get`; 48/49 have no `get`, so HEAD is DRF's 405 and 48's OPTIONS is the `AssertionError`
500 (no `serializer_class`) already recorded in `routes.json` `options_metadata.error` — the
runtime replays both. 45 (`project-estimates/`) is unchanged; it renders whatever
`project.estimate_id` points at.

## 2. Shared pieces

**a. Hooks.** No new `MODEL_HOOKS.md` row: neither model overrides `save()` and neither has a
signal (`MODEL_HOOKS.md:1317-1330`), so a save is H01 only (`plane/db/models/base.py:23-44`;
`hooks/audit.rs`). Both extend `ProjectBaseModel`, whose `save()` re-derives `workspace` from
`self.project.workspace` (`plane/db/models/project.py:230-239`): two extra SELECTs per instance
save (a forward FK uses `_base_manager`, so **no** `deleted_at` predicate). Rust has no ORM, so each
write derives `workspace_id` itself (`SELECT workspace_id FROM projects WHERE id=$1`) — for 46 and
48 that is the only way the new row gets its workspace (`base.py:69-74`, `:175-177`).
H03 for 47 and 49: `write::soft_delete` + `write::publish_soft_delete_cascade` (`write/mod.rs:99-112`,
`:125-143`), publishing `("db","estimate",pk)` and `("db","estimatepoint",pk)` (`mixins.py:72-82`;
`celery/tasks.rs:1086-1099`).

Bulk writes bypass all of it — no `save()`, no `auto_now`, no audit, no cascade:
`bulk_create(batch_size=10, ignore_conflicts=True)` at `base.py:82-98`, `bulk_update(…, ["key",
"value"], batch_size=10)` at `:141`, `bulk_update(…, ["key"], batch_size=10)` at `:261`
(`MODEL_HOOKS.md:1575,1597-1599`).

**b. Tasks — two, both already have constructors.**
* `issue_activity`, "Bare" shape (`type, requested_data, actor_id, issue_id, project_id,
  current_instance, epoch`; `base.py:210-220`, `:229-239`) → `IssueActivityCall::bare`
  (`plane-jobs/src/celery/tasks.rs:188-190`). `issue_id=issue.id` is a **UUID object**, not
  `str()` (`TASKS.md:1133`), so `TaskArg::uuid` (`celery/args.rs:49-50`) — `activity_base`
  stringifies and must **not** be reused (`issues/write_issue.rs:1542-1559`). The two JSON blobs are
  `json.dumps` of one-key dicts → `TaskArg::json_text_from` (Django separators, `ensure_ascii`;
  `args.rs:82-101`). Offset is always `{"estimate_point": str(new_estimate_id)}` and the current
  instance is `str(issue.estimate_point_id)` **as loaded before the UPDATE**.
* `soft_delete_related_objects` (`base.py:149`, `:263`).

**c. `write::generated`.** Add `plane.app.serializers.estimate.EstimatePointSerializer` to
`WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939-1968`); its writable fields are exactly
`deleted_at, key, description, value` in that order (`SERIALIZERS.md:1840-1843`). Its `validate()`
(`plane/app/serializers/estimate.py:21-27`) is hand-written and must run **after** the field pass,
because DRF never reaches `validate()` when a field failed: `{}` → `{"non_field_errors":
["Estimate points are required"]}`, `len(value) > 20` → `{"non_field_errors": ["Value can't be more
than 20 characters"]}` (checked against DRF 3.18.3). `EstimateSerializer` is **not** needed — the
view never validates with it (`base.py:64-101` builds the row by hand); it only feeds OPTIONS, and
that map is already in `routes.json`.

**d. Response shapes — four, all already renderable.**
* 46 → 200 `EstimateReadSerializer(estimate)` (`base.py:100-101`), i.e. the 13-key object of
  `estimates/base.rs:131-147`, with `points` re-read from the DB (`estimate.points` is a reverse
  manager, so the `bulk_create` result is ignored) in `("value",)` order.
* 47 PATCH → the same 13 keys (`:143-144`), from the in-memory estimate.
* 47 DELETE → 204, no body (`:150`).
* 48 → 200 `EstimatePointSerializer(estimate_point).data` (`base.py:178-179`): the 12 keys of
  `estimates/ser.rs:25-40`.
* 49 PATCH → the same 12 keys from the saved instance (`:194`); 49 DELETE → 200 with
  `EstimatePointSerializer(updated, many=True)` — only the points whose `key` was shifted (`:265-268`).

**e. Cache.** 46 `create`, 47 `partial_update` and 47 `destroy` carry
`@invalidate_cache(path="/api/workspaces/:slug/estimates/", url_params=True, user=False)`
(`base.py:63`, `:108`, `:146`). `invalidate_cache_directly` builds the key from the URL kwargs with
**no** user part (`plane/utils/cache.py:54-69`), while the only consumer caches under
`get_full_path() + ":" + user_id` (`cache.py:32-34`, `workspace/estimate.py:21`): the delete is a
permanent no-op (known bug, `PROGRESS.md:582-584`). Reproduce it with
`cache::invalidate_cache(ctx, Some("/api/workspaces/:slug/estimates/"), true, false, false)`
(`common/cache.rs:135-158`) — it runs after the permission check and **before** the body is read,
so it also runs on 400s.

**f. Reuse.** `common::client`, `lookup::uuid_kwarg` (`common/lookup.rs:98-107`),
`views::write_support::{Body, body_get}` for `request.data.get` incl. the `AttributeError` 500
(`views/write_support.rs:28-49`), `write_support::integrity` for class-23 → `ViewError::Integrity`
400 (`:127-144`), `write::Serializer::{char,integer,datetime}_field` (`write/serializer.rs:577`,
`:924`, `:1241`), `write::now`, `issues::write_issue::publish_issue_activity`,
`plane_db::models::estimate::{Estimate, EstimatePoint}` (`models/estimate.rs`), and the existing
`estimates::ser::point_serializer` / `estimate_read_serializers`.

**g. P0 — shared and blocking both sets.**
1. `write/generated/estimate_point_serializer.rs` + the `WRITE_SERIALIZERS` entry (46 validates a
   list of them, 49 a partial one).
2. `write::list_of` — a `ListSerializer` helper: `data=None` → `{"non_field_errors": ["No data
   provided"]}`; a non-list → `{"non_field_errors": ["Expected a list of items but got type
   \"dict\"."]}`; otherwise DRF 3.18.3 with `LIST_SERIALIZER_ERRORS_AS_DICT=True`
   (`rest_framework/settings.py:89`) renders **index-keyed objects with only the failing indices**,
   `{"1": {"value": ["This field is required."]}}`, and a `validate()` failure nests under
   `non_field_errors` inside the index (`{"0": {"non_field_errors": ["Value can't be more than 20
   characters"]}}`). `ValidationErrors` cannot express a top-level index key (`serializer.rs:280-312`),
   so this returns a `ViewResponse` directly.
3. `estimates/render.rs`: one-estimate render (lift `base.rs:131-147`) and one point-list render.
4. `estimates/write_queries.rs` skeleton + `ALL_QUERIES` (register in `tests/schema_check.rs:47`).
5. `scenarios/normalize.yaml`: `shape: estimate_random_name` `^[a-z]{10}$`
   (`base.py:29-31,66`) plus request-scoped `name` rules in `volatile_json_keys` **and**
   `volatile_db_columns` for `POST …/estimates/` (precedent: `archived_at`, `normalize.yaml:414-419`).
6. `scenarios/write/estimates-fixture.sql`.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | none | §2g 1–6 | ~2.5 h |
| **A — estimate** | 46, 47 | `estimates/write.rs` (`create`, `partial_update`, `destroy`), `write.rs`'s INSERT/UPDATE/soft-delete paths, 3 `register` lines, `scenarios/write/estimates.yaml` | ~3 h |
| **B — estimate points** | 48, 49 | `estimates/write_point.rs` (create, partial_update, destroy incl. the issue scan, activity fan-out and key shift), 5 `register` lines, `scenarios/write/estimate-points.yaml` | ~3 h |

Each set adds its own `.action(…)` lines in `estimates/mod.rs:29-43`; that is the only shared edit
besides `write_queries.rs`, and it is a two-line merge. Both scenario files reuse the same
`fixture_sql` and `target_env`. Flip all four Owner cells in one edit at the end
(`gen_caddy.py` turns the Owner cell into the method matcher,
`deploy/caddy/rust-routes.caddy:246-257`).

## 4. Risks

- **Order, no transactions.** 46: permission → invalidate → `request.data` → `Estimate.objects.create`
  (`:69`) → **then** validate the points (`:78-80`) → `bulk_create` → render. So a POST whose
  `estimate_points` fail validation (or is missing: `{"non_field_errors": ["No data provided"]}`)
  has already **written the estimate** and answers 400. 47 PATCH reads the body **before** the
  lookup (`:110` vs `:116`), so `{}` on a missing id is 400 `{"error": "Estimate points are
  required"}`, not 404. 49 DELETE reads `new_estimate_id`, updates issues and publishes one task
  per issue (`:204-239`) **before** it decides the point is missing (`:242-252`) → a 404 that
  wrote 32 rows and 32 messages.
- **`request.data` is not a mapping** → `AttributeError` → 500: 46 `estimate` absent/`null`/str
  (`:65-66`), 47 `len(None)` for `estimate_points: null` (`:110`), 49 DELETE `body: "[]"`.
- **Nothing is validated that the model would reject.** 46 writes `name`/`type`/`last_used` raw
  (`:66-74`): a 256-character name is a class-22 error → **500**, `type: "bogus"` is stored,
  `type`/`name: null` is class-23 → 400 `{"error": "The payload is not valid"}`
  (`views/base.py:210-214`, `:306-310`), `last_used: "yes"` is Django's `ValidationError` → 400
  `{"error": "Please provide valid detail"}`. A duplicate name hits
  `estimate_unique_name_project_when_deleted_at_null` (`plane/db/models/estimate.py:30-36`) → 400;
  48's `EstimatePoint.objects.create` skips the serializer entirely, so `value` may be 21–255
  characters (rejected by 46 and 49) and >255 is a 500, `key: 1.5` becomes `1` (Python `int()`),
  `key: "abc"` a 500, and `value: {"a":1}` stores Python's `repr`.
- **Bulk semantics.** 46's items never run `save()`: `created_by` **and** `updated_by` are the
  caller (`:91-92`, unlike H01's `updated_by = NULL`), `workspace_id` comes from the estimate,
  `description` defaults to `""`, and `ignore_conflicts=True` can never fire (no unique key besides
  the fresh pk). 47's and 49's `bulk_update` writes only the named columns — no `updated_at`, no
  `updated_by` (`django/db/models/query.py:907-924`); stamping timestamps there fails the DB diff.
  47's PATCH matches body ids by `str(uuid)` and silently ignores ids from other estimates (`:125-139`).
- **`IssueManager` predicates.** 49's scan is `Issue.objects.filter(project_id, workspace__slug,
  estimate_point_id)` (`:204-208`, `:223-227`), and `objects` excludes triage-state, archived,
  archived-project and draft issues (`plane/db/models/issue.py:92-101`). `issues.update(…)` writes
  `estimate_point_id` only (`:221`), never `Issue.save`, so no sequence/`updated_at` side effects —
  and it re-runs the same UPDATE on every loop iteration (the queryset is already evaluated, the
  filter is no longer). `new_estimate_id` is an **unvalidated raw body string**: used verbatim in
  the message, coerced by `UUIDField` for the UPDATE (non-UUID → 400, missing row → 400 class 23),
  and a point of **another project** is accepted, silently skipping the activity row
  (`issue_activities_task.py:484-485`) while the FK is written.
- **Worker-side, for the record.** The removal branch enqueues `{"estimate_point": null}`, and
  `track_estimate_points` then evaluates `new_estimate.estimate.type` on `None`
  (`issue_activities_task.py:490-501`) → the task crashes, so a 49 DELETE with no `new_estimate_id`
  produces *no* activity row. The H03 cascade (47) is what nulls `projects.estimate_id` and
  soft-deletes the points (`deletion_task.py:50-61`; `MODEL_HOOKS.md:2002-2003`) — it never runs in
  the harness, so **47 leaves the project pointing at a deleted estimate and its points live**
  until the worker catches up. Only the message is compared.
- **Message order.** `Issue.Meta.ordering = ("-created_at",)` (`plane/db/models/issue.py:178`) makes
  the fan-out deterministic (the fixture's `pin_timestamps` gives every row a distinct second,
  `tools/fixtures/build.py:3020-3078`), but the SELECT must keep the same shape (the
  `workspace__slug` join, no extra ORDER BY) or the drained messages compare out of order.
- **Non-determinism.** `estimate.get("name", generate_random_name())` evaluates the 10-letter name
  **eagerly on every create** (`:66`, `:29-31`) and uses it when `name` is absent — hence the two
  `normalize.yaml` rules; the epoch normaliser already covers `epoch` (`normalize.yaml:408-410`).
- **Permissions.** 46/47 rely on `ProjectEntityPermission` for non-safe methods (ADMIN/MEMBER,
  `is_active`, `project_id` from the URL); a GUEST is 403 and a non-member 403 for **any** pk,
  existing or not. 48/49 use `allow_permission` *as a method decorator*, so `IsAuthenticated` runs
  first and only then the role check (`permissions/base.py:19-103`); both `slug` and `project_id`
  are always in these patterns, so the decorator's `kwargs[…]` lookups (`:62,71`) cannot fail.
  Neither checks that the project is live or unarchived, so creating an estimate in the
  soft-deleted `delta` or archived `gamma` project succeeds.
- **Contract coverage: none.** `grep -ril estimate apps/api/plane/tests/contract` → 0 hits; the two
  `apps/api/plane/tests/` files that mention estimates are worker unit tests
  (`unit/bg_tasks/test_issue_activity_scope.py:216-231`). `tools/contract/run.sh --assume-rust
  46,47,48,49 --methods POST,PATCH,DELETE` runs nothing. The scenarios are the only evidence.
- **Fixture.** `plane_fix_tpl` has three estimates and 13 points (`build.py:905-955`): alpha
  `Story Points` (`points` 1/2/3/5/8, project alpha), alpha `Complexity`, beta `T-Shirt`; only
  alpha and beta have `projects.estimate`. `estimate_alpha_points` = `e35c95aa…`, its key-3 point
  `ddea004d-0068-44bb-90c6-7197c8b915e6`; every fourth alpha issue points at it (`build.py:1294`).
  Use `issue_alpha` (live, `alpha:000`) and `issue_alpha_archived` (`alpha:004`, `build.py:1423`) to
  prove the manager exclusion with no fixture SQL. 47 DELETE needs a soft-deleted estimate whose
  `(name, project)` a live row also holds (proves the partial index), plus a same-name estimate in
  `ws2`; 46 needs a step per `project_zeta` (no estimate) for the random-name case, and `project_delta`
  / `project_gamma` for the deleted/archived-project creates. `estimate-points.yaml` should delete
  `alpha:points:3` (32 live issues updated, 16 triage + 2 archived skipped, 2 points shifted) and
  then a point with no issues, plus the cross-estimate 404 and a `new_estimate_id` that is a live
  point of `alpha:categories`.
