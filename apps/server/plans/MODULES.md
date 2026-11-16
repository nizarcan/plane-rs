# Modules — port plan (read-only)

Area: `plane/app/views/module/**` (plus the module rows in `workspace/module.py`). The GETs
are already Rust (`src/modules/{base,archive,issue}`); this plan covers the write verbs.
The v1 (`/api/v1/…`, orders 299–306) and public (`/api/public/…`, 251) surfaces are excluded.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/projects/{project_id}/`. "Django" = the write verbs
are still Django today (`ROUTES.md`); "whole" = no Rust method on the row.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 103 | `modules/` | POST | Django (GET Rust) | A |
| 104 | `modules/{pk}/` | PATCH, DELETE (PUT 405) | Django (GET Rust) | A |
| 105 | `issues/{issue_id}/modules/` | POST | whole | B |
| 106 | `modules/{module_id}/issues/` | POST | Django (GET Rust) | B |
| 107 | `modules/{module_id}/issues/{issue_id}/` | DELETE (PUT/PATCH 405) | Django (GET Rust) | B |
| 108 | `modules/{module_id}/module-links/` | POST | Django (GET Rust) | B |
| 109 | `modules/{module_id}/module-links/{pk}/` | PUT, PATCH, DELETE | Django (GET Rust) | B |
| 110 | `user-favorite-modules/` | POST | Django (GET Rust) | B |
| 111 | `user-favorite-modules/{module_id}/` | DELETE | whole | B |
| 112 | `modules/{module_id}/user-properties/` | PATCH | Django (GET Rust) | A |
| 113 | `modules/{module_id}/archive/` | POST, DELETE | Django (GET Rust) | A |
| 114 | `archived-modules/` | POST, DELETE (both 500) | Django (GET Rust) | A |
| 115 | `archived-modules/{pk}/` | POST, DELETE (both 500) | Django (GET Rust) | A |

Whole rows: **105, 111**. 210 `workspace/modules/` is GET-only (already Rust). 114/115's
`post`/`delete` take `module_id`, which the URL does not supply (`pk` for 115), so Django
raises `TypeError` → family 500; port the 500, do not "fix" it.

## 2. Shared pieces needed first

**a. Hooks** (`MODEL_HOOKS.md` §2.8). Only two are involved:
`H21 Module.save` (create-only `sort_order = MIN(module same project) - 10000`, else 65535)
— new `hooks::module::before_insert`; `H25 UserFavorite.save` — already `hooks::favorite::
before_insert` (favourites POST reuses it). H07 (`workspace = project.workspace`) is a write
of every `ProjectBaseModel` here; H01 (`audit::stamp`) and H03/H04
(`soft_delete::{delete_instance,delete_where}`) exist. `ModuleMember`, `ModuleIssue`,
`ModuleLink`, `ModuleUserProperties` have no hooks.

**b. m2m semantics.** `ModuleMember`/`ModuleIssue` are `ProjectBaseModel` through rows with a
partial unique on the live pair. Django's `bulk_create(ignore_conflicts=True)` wraps each
batch (10) in `atomic(savepoint=False)` and emits `ON CONFLICT DO NOTHING`; it still returns
the in-memory rows, and no `save()` runs — so the caller passes `workspace=project.workspace`
and `created_by`/`updated_by` explicitly (`ModuleWriteSerializer.create`, `issue.py:248-262`
and `:277-291`). PATCH replaces members: `ModuleMember.objects.filter(module=instance)
.delete()` (queryset → **H04**, `deleted_at=now` only, no `updated_at`, no cascade task)
then a fresh `bulk_create(ignore_conflicts=True)`; an `IntegrityError` from a race is not
caught. `module_issue.delete()` in `ModuleIssueViewSet` is the same H04 path. A duplicate
module name is a serializer 400 (`{"error": "Module with this name already exists"}`), the
DB constraint only fires on races.

**c. Tasks.** All shapes already exist in `plane_jobs::celery::tasks`; nothing new.
`issue_activity` (TASKS.md §4 rows 57–61) uses the **View** shape at every module site
(`notification=True`, `origin` last) — `IssueActivityCall::view`. Call-site arg types must be
copied exactly: rows 57–59 pass `project_id` as a **UUID object** and row 59 `issue_id` as a
UUID (row 57/58/59 mix string/UUID); rows 60–61 stringify both. `model_activity` rows 19–20
(app create/patch: `requested_data=request.data` raw, `actor_id` a UUID) → `ModelActivityCall`
+ `write::request_data_arg`. `Module.delete()` (104) also publishes H03's
`soft_delete_related_objects("db","module",pk)`; the favourites POST has no task.

**d. `write::generated`.** No module write tables exist yet; the generator must emit
`module_write_serializer` (`lead_id` PK, `member_ids` PK **list**, `description_text`/
`description_html`/`view_props`/`logo_props` JSON, `start_date`/`target_date` dates, `status`
choice of six, `name` char 255, `description` text) and `module_link_serializer` (the only
`url` kind — the `http://` prefix + `URLValidator` is **not** a generated kind; set C's
private `write_link::django_url_valid` should move to `write::url` and serve both), plus
`module_user_properties_serializer` for the PATCH reply. Remaining TODO kinds after set A/B/C/D
of work items: only the non-PK lists (`attachments`, the read serializers' `*_ids`) — nothing
new here. `ModuleSerializer`/`ModuleLinkSerializer`/`ModuleUserPropertiesSerializer` already
exist read-side in `plane-serializers::generated::app_module`.

**e. Response re-render.** `modules::base::{ModuleRow, decode_list_row, decode_retrieve_row}`
already decode the list/retrieve projections. Create, list and PATCH use three **different**
`.values()` orders (`is_favorite`/`total_issues`/the estimate points move; `base.py:300-330`,
`:359-389`, `:670-707`), so add a `render_module_row(ctx, client, module_id, ModuleOrder)` in
`modules/` over the shared `ModuleRow` + `user_timezone_converter`. PATCH's `current_instance`
is `json.dumps(ModuleSerializer(module).data, cls=DjangoJSONEncoder)` — build a DJE helper from
the generated read serializer (like `IssueDetail::dje_json`).

**f. Overlap with `plans/CYCLES.md`** (real). `CycleIssue` and `ModuleIssue` share the same
add/remove/soft-delete/idempotent-destroy semantics and the same 201 `{"message": "success"}`
reply; cycles additionally *move* rows (`bulk_update` of `cycle_id`) and have a date guard.
Propose one shared `issues/write_membership.rs` (add rows for a list of issue ids and for a
list of container ids, H04 delete, idempotent destroy, the `{message}` reply) that both domain
sets call; each set supplies its own activity type/params and its extra rules. Build it
**once**, in the set that lands first (modules or cycles), not twice.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **A — module core** | 103, 104, 112, 113, 114, 115 | `modules/write_module.rs`, `modules/write_module_queries.rs`, `hooks/module.rs` | ~2 h (H21, name check, members m2m, `current_instance`, archive/unarchive, user properties) |
| **B — membership, favourites, links** | 105, 106, 107, 108, 109, 110, 111 | `modules/write_issue_module.rs`, `modules/write_favorite.rs`, `modules/write_link.rs`, `issues/write_membership.rs` (shared with cycles) | ~2 h (five activity shapes, H04 deletes, url validator, idempotent destroy) |

A and B can run in parallel once the generator has emitted the two write serializers and
`hooks::module` exists; B's shared membership helper is the only cross-set dependency (cycles
may take it first — then B calls it). Each set registers its own actions in `modules/mod.rs`
in one block.

## 4. Risks

- **Delete order (104)**: activities per live `ModuleIssue` → `module.delete()` (H03 + cascade
  task) → `ModuleIssue.objects.filter(...).delete()` (H04) → **only the caller's**
  `UserFavorite` rows (H04) → `UserRecentVisit.objects.filter(...).delete(soft=False)` (hard
  delete, all users). Wrong order or `soft=False` on favourites changes the DB diff.
- **Create-only hook**: `Module.save` sets `sort_order` only when adding; PATCH/archive must
  not touch it. A PATCH body may set `sort_order` (it is writable).
- **Three response orders**: create's row is not the list's or PATCH's; `model_activity` is
  published *before* `user_timezone_converter`, and `requested_data` is the raw body (no
  read-only-key filtering).
- **PATCH current_instance** is the read serializer (UUID strings, computed fields `null`
  unless annotated) — do not render it from the `.values()` row.
- **Archive** POST requires `status in {completed, cancelled}` else 400; on success it
  `.filter(...).delete()`s **every** user's favourite (H04) and replies
  `{"archived_at": str(datetime)}` (Python `str()`: `"2026-11-16 20:00:00+00:00"`, not ISO).
  114/115's `post`/`delete` are `TypeError` 500s.
- **`ModuleUserProperties` PATCH** uses `.get()` (404 if the GET never created the row),
  writes only the four JSON keys present, full `save()`, and answers **201**.
- **Favourites**: POST is a visibility-checked 404 then `UserFavorite.objects.create` (H25,
  204); a duplicate is an `IntegrityError` 400; DELETE is `.get()` (404) then
  `delete(soft=False)` (hard).
- **`create_issue_modules`** publishes a delete activity for every `removed_modules` entry
  *before* deleting, even when no row exists (`module_name: null`); the delete itself is a
  queryset H04. `create_module_issues` requires a non-empty `issues` list (400 `{"error":
  "Issues are required"}`).
- **Harness**: `scenarios/write/modules.yaml` (create/duplicate name/start>target, PATCH
  archived 400, members replace, archive/unarchive + 114/115 500s, module-issue add/remove on
  both directions, links url prefix/validator, favourites duplicate, user-properties PATCH
  201) plus contract `--assume-rust 103,104,105,106,107,108,109,110,111,112,113
  --methods POST,PUT,PATCH,DELETE`; `PLANE_RS_REQUIRE_DB_TESTS=1` for the new catalog.
