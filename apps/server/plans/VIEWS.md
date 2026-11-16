# Views — port plan (read-only)

Area: `plane/app/views/view/base.py` (saved project/workspace views + their favourites) plus
the member-preference writes whose paths this route group owns (147 is already Rust, 198 is
not). The GETs are already Rust (`crates/plane-http-app/src/views/{base,queries}.rs`); this
plan covers the write verbs. `/api/v1/`, public/space and `GET /api/workspaces/{slug}/issues/`
(order 182, registered by the `issues` group) are excluded.

## 1. Inventory (app routes with write methods)

"Rust" = the verbs are still Django today (`ROUTES.md`); "whole" = no Rust method on the row.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 147 | `projects/{project_id}/project-views/` | POST | rust(POST) — **done** | — |
| 178 | `projects/{project_id}/views/` | POST | rust(GET) | A |
| 179 | `projects/{project_id}/views/{pk}/` | PATCH, DELETE (PUT 405) | rust(GET) | A |
| 180 | `views/` | POST | rust(GET) | A |
| 181 | `views/{pk}/` | PATCH, DELETE (PUT 405) | rust(GET) | A |
| 183 | `projects/{project_id}/user-favorite-views/` | POST (GET 405) | rust(GET) | B |
| 184 | `projects/{project_id}/user-favorite-views/{view_id}/` | DELETE | whole | B |
| 198 | `workspace-views/` | POST | whole | B |

Common prefix `/api/workspaces/{slug}/`. **Whole rows: 184, 198.** 183's GET is a guard 405
(`ROUTES.md:273`, `:633`), already registered (`views/base.rs:85-88`). 198 lives in
`workspace/member.py` but its path is selected by this group (`deploy/route-groups.yaml:184`),
so `crates/plane-http-app/tests/registry.rs:240-283` allows it only from `views::register` —
as 147 already does, though its handler is `projects::settings_write::project_views`
(`views/mod.rs:34-42`). No other write exists under `/views/`.

## 2. Shared pieces needed first

**a. Hooks.** H24 `IssueView.save` (`plane/db/models/view.py:79-95`) — new
`plane_db/src/hooks/issue_view.rs` with two halves: `query_from_filters(filters) -> JsonText`
(`self.query = issue_filters(filters, "POST") if filters else {}`, runs on **every** save,
create and update) and `sort_order(client, project_id, workspace_id)` (create only: `MAX`
over live `issue_views` of the project, else of `(workspace, project__isnull=True)`; `Some`
→ `max + 10000`, `None` → the model default 65535; `view.py:83-93`). H25 already exists
(`hooks/favorite.rs:30-39`) and serves 183 unchanged. H01 (`audit::stamp`), H03
(`soft_delete::delete_instance`, `soft_delete.rs:38-55`) and H04 exist. **Note:** H03 writes
one bare UPDATE, but Django's `IssueView.delete()` is `self.save()` (`mixins.py:72-78`) and
therefore re-runs H24 — a fixture whose `query` disagrees with `filters` is rewritten on
delete. Either bind the recomputed `query` into the delete UPDATE or prove the diff away.

**b. `filters` → `query`.** Nothing validates these four JSON columns: `filters`,
`display_filters`, `display_properties`, `rich_filters` are plain `JSONField`s
(`serializers/view.py:59-72`, `models/view.py:61-65`), so any JSON is stored (a non-dict is not
rejected: a list passes `"state" in filters` and then dies on `params.get` — a 500). Only
`query` is derived: `issue_filters(filters, "POST")` (`utils/issue_filters.py:428-463`)
iterates the fixed `ISSUE_FILTER` order and, for each key present in `filters`, copies the
**raw body value** into a lookup dict (POST takes the `else` branch of every `filter_*`; no
`uuid.UUID` conversion, unlike GET). Fixed extras: `labels` adds
`label_issue__deleted_at__isnull`, `assignees` `issue_assignee__…`, `cycle` `issue_cycle__…`,
`module` `issue_module__…`, `subscriber` `issue_subscribers__…` (all `True`); `type`
**overwrites** `state__group__in` with one of three fixed group lists (`:296-305`);
`sub_issue` compares to the *string* `"false"` (`:380-389`); `intake_status`'s POST branch
reads `inbox_status` (`:350-362`); `logged_by` is a `FieldError` → family 500 (`:414-425`).
`serializers/view.py:82-89` calls the transform twice (POST then PATCH) — every filter takes
the same branch for both — and H24 overwrites the result anyway, so the serializer's value is
dead. The dict shape is already consumed in Rust by `analytics/stored.rs:38-263`. `query` is
`jsonb`, so its *row* order is jsonb's (length-then-bytes); the *response* must use the
in-memory dict in `ISSUE_FILTER` order (`HANDLERS.md:209-226`).

**c. Tasks.** None. No `TASKS.md` row cites a write in `views/view/base.py` (the only hits are
the two `recent_visited_task` GET sites, `TASKS.md:801-802`); destroy publishes only H03's
cascade, `write::publish_soft_delete_cascade` (`write/mod.rs:121-139`, task row `TASKS.md:537`).
Every shape exists.

**d. `write::generated`.** Add `plane.app.serializers.view.IssueViewSerializer` to
`WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939`) → new
`write/generated/issue_view_serializer.rs`. Writable: `name` (char 255, required),
`description` (char, blank), `filters`/`display_filters`/`display_properties`/`rich_filters`/
`logo_props` (json), `sort_order` (float), `archived_at`, `deleted_at` (datetime, null);
read-only: `query`, `access`, `is_locked`, `workspace`, `project`, `owned_by`, audit columns
(`serializers/view.py:63-72`) plus `id`/timestamps and the declared `is_favorite`
(`:57`). **No new field kind is needed** (char/json/float/datetime; the read-only ones use no
helper). The generator emits `default: None` for model defaults
(`write/generated/cycle_write_serializer.rs:207-226`), so the handler supplies them:
`description=""`, the five JSON columns `{}` except `display_filters`
(`models/view.py:28-37`, 7 keys) and `display_properties` (`:40-55`, 13 keys) — literal order
is part of the bytes — and `sort_order=65535` (H24 raises it). The read side already renders
the reply shape (`plane-serializers/src/generated/app_view.rs`).

**e. Response re-render.** Create answers `serializer.data` of the **in-memory** instance with
201 and PATCH with 200 (DRF `CreateModelMixin`/`UpdateModelMixin`; viewset at
`plane/app/views/base.py:67`), i.e. no re-read. `views::base::Row::value()` already builds that
serializer from a row with `is_favorite: FieldValue::absent()` (`views/base.rs:156-185`) —
expose it `pub(super)` (or add a constructor) and pass the `JsonText` values you wrote, never
a fresh SELECT.

**f. Overlap with 147/198.** All three write a JSON column on a member row from a raw body:
`.first()` → 403 then per-key keep-or-replace (`projects/settings_write.rs:76-125`). 198 differs:
`WorkspaceMember.objects.get(…)` → **404** on a miss, only `view_props`, missing key → `{}`
(not keep-current), full `save()` (`workspace/member.py:225-231`).

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **A — view CRUD** | 178, 179, 180, 181 | `views/write_view.rs`, `views/write_view_queries.rs` (INSERT, `SELECT … FOR UPDATE`, UPDATE), `plane-db/src/hooks/issue_view.rs`, one registration block in `views/base.rs::register` | ~2 h (two create paths, two atomic+lock PATCH paths, two destroy paths, H24) |
| **B — favourites + member props** | 183, 184, 198 | `views/write_favorite.rs`, `views/write_member_views.rs`, 2–3 statements (own query file or `views/queries.rs`), one registration block in `views/mod.rs` | ~1 h (all hooks exist: H25/H01/H04/H03) |

B has no dependency on A and can start immediately; A's only new primitive is H24.

## 4. Risks

- **Statement order.** 180 create: permission → `Workspace.objects.get(slug=…)` → H24 query
  from `filters` → `MAX(sort_order)` → INSERT → 201 (`base.py:63-65`, `models/view.py:79-95`);
  178 create has **no** project read (just `project_id`, `owned_by`, `base.py:302-303`).
  179/181 PATCH: permission → `atomic` → `SELECT … FOR UPDATE` → `is_locked` 400 → `owned_by`
  400 → validate → full UPDATE (H24 + H01) → 200 (`base.py:94-113`, `:386-405`; 2 of the repo's
  4 `select_for_update` sites, `MODEL_HOOKS.md:1753`). Destroys: `.get()` (404) → the role
  query → H03 (+cascade task) → favourites H04 (all users) → (project only) recent visits hard
  delete → 204 (`base.py:150-170`, `:408-440`). Only PATCH opens a transaction, and its two
  400 branches return *inside* it, so it commits; do not wrap create/destroy.
- **Permissions differ per verb, and the handler re-checks.** PATCH is `allow(-;PROJECT;
  creator=IssueView)`: creators pass on `created_by`, otherwise only the workspace-admin
  fallback survives an empty role list (`permissions/base.py:51-93`). DELETE is
  `allow(ADMIN;…;creator)`. The handler then applies a *different* test — `is_locked` then
  `owned_by` for PATCH, `ProjectMember role=20 or owned_by` (workspace: `WorkspaceMember
  role=20 or owned_by`) for DELETE — so a permitted request can still get
  `400 {"error": …}` (`base.py:98-106`, `:156-169`, `:390-398`, `:410-419`). The decorator
  gate is already ported (`plane-permissions/src/allow.rs:73-80`, `queries.rs:153`); only the
  handler half is new.
- **Cross-scope quirk.** Workspace PATCH/DELETE look the row up by `(pk, workspace__slug)`
  only — not `project__isnull=True` — so a **project** view can be edited/destroyed through
  `/views/{pk}/` (and vice versa is impossible: the project routes add `project_id`). Port it.
- **`is_locked`/`access`/`query` are read-only** (`serializers/view.py:63-72`), so a locked
  view only exists in seed data; `deleted_at` *is* writable (not in that list) — a body can
  create a pre-deleted view. 179/181 PUT is the guard 405 (`ROUTES.md:630-631`) — register
  `update → cycles::base::guard_only` so the row can flip whole.
- **Destroy side effects are global.** Neither favourite delete filters by user
  (`base.py:159-164`, `:422-427`); recent visits are hard-deleted for every user
  (`:429-434`). Favourites: POST gate → `objects.create` (H25, 204), duplicate → 400
  `{"error": "The payload is not valid"}` (`plane/app/views/base.py:210-214`); DELETE is
  `.get()` (404) then `delete(soft=False)` (`base.py:469-479`). 183's gate adds
  `access=1` and the restricted-guest rule (`serializers/favorite.py:95-98`), unlike modules.
- **183's GET 405 has a second reason:** `get_queryset()` does `.select_related("view")`
  (`base.py:446-453`) but `UserFavorite` has no `view` relation — evaluating it would 500.
  Never port it as a real read.
- **Contract coverage.** Only 178/180 POST are covered
  (`tests/contract/app/test_view_create_membership_app.py:31,44,54,67`); 181 GET is
  read-tested (`test_undecorated_action_authz_app.py:7,34`). **No contract test for 179, 183,
  184, 198** (183 appears only in `scenarios/policy-probe.yaml:8885-8940`).
- **Harness.** New `scenarios/write/views.yaml`: minimal create (defaults + `query` order),
  full body, read-only keys ignored, `filters` with `type`/`sub_issue`/`labels`, a second view
  (`sort_order` 65535 → 75535), PATCH locked and foreign-owner 400s, cross-scope PATCH, destroy
  DB diff (a second user's favourites, recent visits), favourite duplicate/gate, 198 with and
  without membership, then GET list/detail on both sides. Plus
  `tools/contract/run.sh --assume-rust 178,179,180,181,183,184,198 --methods POST,PATCH,DELETE`
  and `PLANE_RS_REQUIRE_DB_TESTS=1`.
- **Question for the orchestrator.** The modules port's recent-visit statement drops the
  manager's `deleted_at IS NULL` predicate (`modules/write_queries.rs:405-419` vs
  `db/models/recent_visit.py:22` + `db/mixins.py:56-58`), while Django's `.delete(soft=False)`
  only sees live rows. Decide whether that is an accepted diff before copying it for 179.
