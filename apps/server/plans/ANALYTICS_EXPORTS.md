# Analytics views and exports — port plan (read-only)

Area: `plane/app/views/analytic/base.py` (`AnalyticViewViewset` `:183-193`, `ExportAnalyticsEndpoint`
`:235-261`), `plane/app/views/exporter/base.py` (`ExportIssuesEndpoint` `:19-66`) and
`plane/app/views/workspace/base.py` (`ExportWorkspaceUserActivityEndpoint` `:340-391`). The three
route groups are the ones `deploy/route-groups.yaml` selects: `analytics` (`:99-113`),
`workspaces`'s `user-activity/**` (`:89`) and `issues`'s `export-issues/` (`:150`) — so the register
edits land in three different files. GETs 202 and 233 are already Rust
(`crates/plane-http-app/src/analytics/`, `workspaces/user.rs:65-79`,
`issues/list_export.rs:34-39`); this plan covers the write verbs on the five routes.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/`. "Owner" = `ROUTES.md`'s Owner cell.

| order | path | write methods | owner | request does | worker does later | set |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `analytic-view/` | POST | rust(GET) | 1 INSERT `analytic_views`; no task; 201 + 11 keys | — | A |
| 2 | `analytic-view/{pk}/` | PATCH, DELETE | rust(GET) | 1 full-row UPDATE; DELETE adds `soft_delete_related_objects`; 200 / 204 | cascade only | A |
| 4 | `export-analytics/` | POST | django | **no rows**; publishes `analytic_export_task`; 200 `{"message": …}` | `analytic_plot_export.py:350-412`: rebuilds the graph, renders its rows as a CSV, one e-mail with the attachment (`:52-88`); every exception swallowed (`:410-412`), so it leaves no rows to compare | C |
| 203 | `user-activity/{user_id}/export/` | POST | django | **no rows, no task**; reads ≤10 000 activities (`workspace/base.py:361`), answers the CSV itself (`workspace/base.py:351-391`) | — | C |
| 233 | `export-issues/` | POST | rust(GET) | 1 INSERT `exporters`; publishes `issue_export_task`; 200 `{"message": …}` | `export_task.py:43-125`: zip in memory, `PutObject` to `<workspace_id>/export-<slug>-<token[:6]>-<YYYY-MM-DD>.zip` (`:47`), 7-day presign, `status/url/key` update; failure → `status="failed"`, `reason=str(e)` (`:226-231`) | B |

(`ROUTES.md:91,92,94,293,323`; `routes.json` orders 1,2,4,203,233; urls `analytic.py:30-49`,
`exporter.py:11-15`, `workspace.py:142-146`.) **Whole rows: 4, 203** (POST-only). 233 becomes
`rust(GET,POST)`; 1/2 become `rust(GET,POST)` and `rust(GET,PATCH,DELETE)` and 4 `rust(POST)`, in
the same `analytics` group whose Caddy matcher matches GET only today — so nothing 501s before the
flip. `PUT` on 2 is not routed (`analytic.py:37`): it stays DRF's 405, no step needed. The worker's
`token[:6]` object key appears in no request or response, so the harness never compares it.

## 2. Shared pieces needed first

**a. Hooks.** H01 only, for all three models: `AnalyticView` and `ExporterHistory` are
`BaseModel`/`AuditModel` with no `save()` override (`plane/db/models/analytic.py:11-25`,
`exporter.py:20-67`, `MODEL_HOOKS.md:1347-1349`, `:1370-1378`, `:2492`) — so an INSERT is
`Audit::on_create`, a full save `Audit::on_update` (`hooks/audit.rs`), and `created_at`/`updated_at`
are **two** `write::now()` calls because Django's `auto_now_add`/`auto_now` call `timezone.now()`
per field (`write/mod.rs:87`, precedent `views/write_view.rs:275-276`). 2's DELETE is H03:
`write::soft_delete` (`write/mod.rs:99`, one `UPDATE … SET deleted_at=$1, updated_at=$1,
updated_by_id=$2`, `hooks/soft_delete.rs:50-66`) then `write::publish_soft_delete_cascade(ctx, "db",
"analyticview", pk)` (`write/mod.rs:125`) — `model_name` is `_meta.model_name`, `using=None`
(`plane/db/mixins.py:72-82`). 233's `status` changes are worker-side `save(update_fields=…)`, i.e.
no audit and no timestamp bump (`MODEL_HOOKS.md:1378`, `:1727`) — out of scope, but do not "fix" it.

**b. Tasks.** Both publishers already exist and, until now, have **zero call sites**
(`plane-jobs/src/celery/tasks.rs:1022-1065`). Two argument facts force a signature change:
`issue_export_task` takes `project_ids` (the raw body value when `project` was sent: a list of
whatever the client wrote, or a **string**) and `multiple` (the raw body value — a form `"false"` is
truthy) (`exporter/base.py:29-30`, `:50-57`). So `project_ids: Vec<String>` → `TaskArg` and
`multiple: bool` → `TaskArg`; `analytic_export_task`'s `data` is `TaskArg::Json(request.data)` from
`write::request_data_arg` (`write/mod.rs:240`), which flattens a `QueryDict` to its last value per
key (verified: `kombu` dumps `{"a": "2", "b": "x"}` for `a=1&a=2&b=x`; `TASKS.md:928-931` calls
those lists and is wrong). Call kwargs: `email`, `data`, `slug`, `user_id=str(user.id)` for 4
(`analytic/base.py:256`); `provider`, workspace `UUID`, `project_ids`, `token_id`, `multiple`,
`slug` for 233 (`exporter/base.py:50-57`, `TASKS.md:596-598`, `:620-622`). Both tasks are
Celery-owned during coexistence (`ownership.rs:52-64`), so publish through
`common::jobs::dispatch_task` (`common/jobs.rs:61`).

**c. `write::generated`.** Add `plane.app.serializers.analytic.AnalyticViewSerializer` to
`WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939-1974`), `codegen: simple`
(`SERIALIZERS.md:1504`). Its writable fields, in DRF's visit order, are `deleted_at` (DateTime,
nullable), `name` (Char, required, 255), `description` (Char, allow_blank), `query_dict` (JSON,
`required=False`, **no** default in the field spec — an absent key leaves the model default `{}`),
`created_by`/`updated_by` (PrimaryKeyRelated, nullable, one `SELECT` each);
`query`/`workspace`/`id`/`created_at`/`updated_at` are read-only
(`SERIALIZERS.md:1508-1518`, `serializers/analytic.py:12-14`). Regenerate the whole
`write/generated/` set in P0 (`HANDLERS.md:591-620`); the generator owns the module, nobody
hand-writes the table. 233 has **no** serializer on the write path (the serializer is read-only and
only its GET uses it, `SERIALIZERS.md:1915-1926`).

**d. `analytic_views.query` is `issue_filters(query_dict, "POST")`.** Already ported as
`hooks::issue_view::query_from_filters` (`hooks/issue_view.rs:90`), used by `views/write_view.rs:92`.
`create()` calls it with the validated `query_dict` (`serializers/analytic.py:16-22`); a
`query_dict` the POST branch chokes on (`{"state": 5}` → `len(int)`, a list body → `.get`) is the
family 500 via `FilterError::Python` (`hooks/issue_view.rs:53-63`). `update()` reads
`validated_data.get("query_data", {})` — a **typo** for `query_dict` (`serializers/analytic.py:25`)
— so a PATCH always stores `query = issue_filters({}, "PATCH") = {}` and the double assignment at
`:27,:30` is dead code. Port the typo.

**e. Response shapes — five.**
* 1/2 answer `AnalyticViewSerializer(instance).data` over the **in-memory** instance
  (`plane/app/views/analytic/base.py:188-190`, `:192-193`): 11 keys `id, created_at, updated_at,
  deleted_at, name, description, query, query_dict, created_by, updated_by, workspace`. `query`
  renders the insertion-ordered dict `query_from_filters` produced, not the re-ordered `jsonb`;
  reuse `analytics/base.rs:583-598` `serializer_for` with `PgJson`
  (`plane-serializers/src/fields.rs:453`).
  On create `created_by` is the caller and `updated_by` null (H01 insert).
* 2 DELETE: 204, empty body (`D18` for `Content-Length`).
* 4: 200 `{"message": "Once the export is ready it will be emailed to you at <email>"}`
  (`analytic/base.py:258-261`), one key via `message_body` (`analytics/base.rs:553`).
* 203: 200, `Content-Type` **exactly** `text/csv` (`HttpResponse(content_type=…)` does not append a
  charset, `django/http/response.py:117-131`) + `Content-Disposition: attachment;
  filename="workspace-user-activity.csv"` (`workspace/base.py:389-390`) →  `ViewResponse::Raw` (`plane-drf/src/view.rs:139-188`). Bytes: `csv.writer(quoting=QUOTE_ALL)` +
  CRLF, every field quoted, `None` → `""`, non-strings via `str()` — so a datetime is
  `2026-11-17 10:00:00+00:00` (verified against CPython) — after `sanitize_csv_row`
  (`plane/utils/csv_utils.py:7-26`) which prefixes `'` when a **str** starts with `=+-@\t\r\n`.
  No CSV writer exists in the tree yet; `sanitize_csv_value` is new.
* 233: 200 `{"message": "Once the export is ready you will be able to download it"}`
  (`exporter/base.py:58-61`).

**f. Reuse.** `write::Serializer::new` + the generated `validate` (`HANDLERS.md:538-590`);
`views::write_support::body_get` for `request.data.get`, including the non-mapping `AttributeError`
500 (`views/write_support.rs:39-49`); `lookup::workspace_by_slug` for the `Workspace.objects.get`
404 body (`common/lookup.rs:69`); `plot::is_valid_field`/`is_valid_y_axis` for 4's two checks
(`analytics/plot.rs:65-71`, `pub(crate)` today); `stored.rs:29-266` documents which lookups a stored
`query` may hold. 203 reuses 202's query: lift `activity_select`/`ActivityRecord`
(`workspaces/user.rs:1322`, `:1068`) to `pub(super)` and add a `filter_archived: bool` — 202 keeps
`Frag::is_null("issue_projects.archived_at")` (`:1381`) and **203 has no such filter**
(`workspace/base.py:355-361` vs `user.py:419`).

**g. P0 (shared, must merge first).** `plane-jobs` `tasks.rs:1022-1065` signature change (§2b);
one `write/generated/` regeneration with the new `WRITE_SERIALIZERS` entry (§2c); the
`exporters.token` normalisation rule (§4). ~1.5 h.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | none | `plane-jobs/src/celery/tasks.rs` (two signatures → `TaskArg`), `tools/inventory/serializers.py` + all of `write/generated/`, one `volatile_db_columns` rule in `scenarios/normalize.yaml` | ~1.5 h |
| **A — stored views** | 1 POST, 2 PATCH/DELETE | `analytics/write.rs` + `analytics/write_queries.rs` (`ANALYTIC_VIEW_INSERT`/`_UPDATE`/`_BY_PK`, delete via H03) and its `ALL_QUERIES` + `tests/schema_check.rs::catalogs()`, three `.action(…)` lines in `analytics/mod.rs:68-107`, `scenarios/write/analytic-views.yaml` + fixture sql | ~2 h |
| **B — issue export** | 233 POST | `issues/write_export.rs` + query file (`EXPORTER_INSERT`, `ALL_QUERIES`, `catalogs()`), one `.method(POST, …)` in `issues/list_export.rs:34-39`, `scenarios/write/export-issues.yaml` + fixture sql | ~2 h |
| **C — analytics + activity export** | 4 POST, 203 POST | `analytics/export.rs` (one `.method` in `analytics/mod.rs`), `workspaces/activity_export.rs` (one `.api_view` in `workspaces/user.rs:65-79`), `plane-core::csv` (writer + `sanitize_csv_value`), the `activity_select` lift (re-run `read/workspace-user*.yaml`), `scenarios/write/analytics-export.yaml` | ~3 h |

A and C share the `analytics/mod.rs` register list — a two-line merge each, nothing else. B and C
share only P0. With one agent, C splits into C1 = 4 (~40 min) and C2 = 203. Flip the five Owner
cells in one edit at the end (`ROUTES.md:91,92,94,293,323`) and regenerate the proxy
(`tools/proxy/gen_caddy.py`); 1/2/4 are one group, 203 and 233 are one row each.

## 4. Risks

- **Order, no transactions.** 1: permission → validate (400) → `Workspace.objects.get` (404,
  `analytic/base.py:188-190`) → INSERT → 201; note validation runs **before** the workspace lookup,
  unlike webhooks. 2 PATCH: permission → `get_object()` (404) → validate (400) → `update()` → full
  save → 200. 2 DELETE: permission → `get_object()` → soft delete → publish → 204. 233: permission →
  `Workspace.objects.get` (404, `:26`) → body → provider (400, `:31-32`) → the project fallback
  query (`:33-40`) → INSERT → publish → 200: a broker failure after the INSERT is a 500 with the
  row committed. 4 reads the body first (a JSON **list** body is the `AttributeError` 500), then
  publishes only. 203 reads `date` before anything (`:351-353`).
- **Manager predicates.** `AnalyticView.objects`/`ExporterHistory.objects` filter `deleted_at`
  (`plane/db/mixins.py:56-58`), and `get_queryset()` adds the unfiltered `workspaces` join
  (`analytic/base.py:192-193`, `analytics/queries.rs:50-71`), so a soft-deleted or other-workspace
  pk is 404. The permission queries also join `workspaces` unfiltered
  (`plane/app/permissions/workspace.py:66-71`, `:85-90`): a member of the **soft-deleted** workspace
  passes, then 1/233 404 and 2 200/404 — while **4 has no workspace lookup at all**, so it answers
  200 and publishes for that member.
- **Permissions.** 1/2 are `WorkSpaceAdminPermission` — ADMIN **or MEMBER** despite the name
  (`plane/app/permissions/workspace.py:61-71`) — so a MEMBER may create/PATCH/DELETE a stored view,
  and a GUEST gets the 403 for **any** pk including a missing one. `BaseViewSet.initial`'s guard
  does not refuse these verbs: `create` is exempt through this class's `perform_create`
  (`plane/app/views/base.py:155-160`) and `partial_update`/`destroy` pass because the permission
  class is restrictive (`:141`). 4/233 use `allow_permission([ADMIN,MEMBER],WORKSPACE)`
  (`plane/app/permissions/base.py:19-103`); 203 uses `WorkspaceEntityPermission`, whose non-safe
  branch is the same ADMIN/MEMBER test (`permissions/workspace.py:74-90`) — that is why the contract
  tests bump their guest to workspace role 15 before exporting
  (`test_workspace_user_profile_scope_app.py:251-252`). All of it is already enforced from
  `routes.json`; no handler work.
- **Django quirks to keep.** (1) `deleted_at` is writable, so POST can insert an already-soft-deleted
  view that the response echoes and no read ever returns (`serializers/analytic.py:12-14`). (2)
  `query_dict` never recompiles on PATCH (§2d). (3) 233 takes `project` on trust: no membership
  check of the body's list, and the fallback query filters `project_projectmember__is_active` but
  **not** `deleted_at` (`exporter/base.py:36-38`) while the task's own query does
  (`export_task.py:159`). (4) 233's `project` is an `ArrayField`: a bare string is accepted as an
  array literal (`a1b2…` = one element, `abc` = `DataError` → 500) and each element goes through
  `UUIDField.get_db_prep_value` (an int becomes `uuid.UUID(int=…)`); `ArrayField.get_db_prep_value`
  only loops over list/tuple (`django/contrib/postgres/fields/array.py:125-131`). (5) 203 exports
  archived-project activity (no `archived_at` filter), never checks that `user_id` belongs to the
  workspace (only `_visible_issues_q(request.user)`, `user.py:68-82`, scopes the issues) and passes
  `date` to the ORM unparsed: a bad string is the family 400
  `{"error": "Please provide valid detail"}`, a non-string (`123`, `[...]`) a 500
  (`workspace/base.py:359`).
  (6) `~Q(field__in=[…])` keeps `field IS NULL` rows (Django negates the `IN … AND … IS NOT NULL`
  pair; 202's verified SQL spells it out, `workspaces/user.rs:1366-1379`) — so `issue.activity.created`
  rows land in the CSV.
- **Non-determinism and how scenarios normalise it.** `exporters.token` is `uuid4().hex`
  (`exporter.py:20`, `:50`): add one `volatile_db_columns` rule `exporters.token`,
  `mode: placeholder`, `shape: uuid_hex`, `placeholder: export-token`. The DB diff is normalised
  **before** the Celery message (`plane-testkit/src/runner.rs:1173-1196`), so `normalize_text` then
  substitutes the same placeholder inside `kwargs.token_id` and `kwargsrepr`
  (`normalize.rs:1479-1494`, `:744-784`). Do **not** add a JSON-key rule: `json_keys` is a map keyed
  by name (`normalize.rs:353-357`), and the existing `token` entry asserts the 64-hex `api_token`
  shape (`normalize.yaml:350-354`; fixture exporter tokens are 64 hex,
  `tools/fixtures/build.py:2816`) — a 32-hex value under that key is a shape **violation**
  (`normalize.rs:1088-1110`). Consequence: no `GET export-issues/` step **after** the create in the
  same file. `analytic_views.id` and both `created_at`/`updated_at` need nothing (id placeholders and
  the timestamp window, `normalize.rs:496-535`). 202's activity list keeps `updated_at`/`created_at`
  literal — 203's CSV therefore only uses fixture rows with a fixed timestamp.
- **Fixture needs.** `plane_fix_tpl` already has 2 analytic views and 4 exporters
  (`build.py:2790-2843`) and admin/member activities (`build.py:1877-1950`). Add by `fixture_sql`:
  a soft-deleted `analytic_views` row in `fix-workspace` and one in `fix-second` (manager predicate
  and slug scoping, both 404 on PATCH/DELETE and absent from the list); an exporters row in
  `fix-second`; and `issue_activities` rows with a **fixed** `created_at` on one date — one with
  `field` NULL, one `field='comment'`, one in an archived project, one in a project the caller is
  not a member of, one by a different `actor`. Steps: 1 with an empty body
  (`{"name": ["This field is required."]}`), minimal, full with read-only `id`/`workspace`/`query`
  in the body, `deleted_at` in the body, a `query_dict` for every `issue_filters` key group, a
  `query_dict` that 500s, `name` > 255 chars (500), PATCH partial/`{}` (proves `query` → `{}` and
  the `updated_at` bump), PATCH/DELETE a missing pk, DELETE then GET list, 4 with an invalid axis, a
  list body, MEMBER vs GUEST, 203 with no `date`, `date=""`, `date="nope"`, a list `date`, and the
  CSV body itself, 233 with each provider, `provider` missing/bogus, `project` absent/`[]`/a
  string/a bad element, a form body (`multiple="false"`), and a second POST to prove two tokens.
- **Contract coverage.** Only 4 and 203 have any: `--assume-rust 4,203 --methods POST`
  (`tools/contract/run.sh:16-22`). 4: `test_workspace_analytics_project_scope_app.py:123-129`
  asserts the replayed `delay` kwargs' `user_id`, which needs the publisher's `kwargs` to match the
  Python call. 203: `test_workspace_user_profile_scope_app.py:238-266` (3 tests, CSV line counts).
  **Nothing** covers 1, 2 or 233 POST — the two 233 tests are GET
  (`test_export_history_scope.py`, `test_export_order_by_allowlist_app.py`) — so the differential
  scenarios are the only evidence there.
- **Two harness gotchas.** Celery comparisons need managed mode, not `--a-url`/`--b-url`
  (`PROGRESS.md:912-914`); and 4's `data` lands in `kwargsrepr`, whose compare is exact until
  Celery's 1024-character `saferepr` budget (`compare.rs:622-700`) — keep the scenario's
  `export-analytics` bodies small.
