# Favourites — port plan (read-only)

Area: `plane/app/views/workspace/favorite.py` (`WorkspaceFavoriteEndpoint`,
`WorkspaceFavoriteGroupEndpoint`), routes 212–214, group **`workspaces`**
(`deploy/route-groups.yaml:93`; specificity rule `:29-35`). The three GETs are Rust
(`crates/plane-http-app/src/workspaces/favorite.rs:50-58`); this plan covers the write verbs
of 212/213. Only `user_favorites` changes: no task, no signal, no cache write, no
activity/notification row.

## 1. Inventory (app routes with a favourite write method)

Prefix `/api/workspaces/{slug}`. "Owner" = `ROUTES.md`'s Owner cell.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 212 | `/user-favorites/` | `POST`; `PATCH`/`DELETE` → the signature 500 | rust(GET) | A |
| 213 | `/user-favorites/{favorite_id}/` | `PATCH`, `DELETE`; `POST` → the signature 500 | rust(GET) | A |

(`ROUTES.md:302-303`; urls `plane/app/urls/workspace.py:187-201`.) **Whole rows: 212, 213.**
No favourite route is `django`; 212/213 are the only partial rows (`rust(GET)`) and 214 is
GET-only and Rust.
The 500s are not branches: 212 binds `patch/delete(self, request, slug, favorite_id)`
(`favorite.py:79,90`) and 213 binds `post(self, request, slug)` (`favorite.py:38`), so the
missing/extra kwarg is a `TypeError` that `BaseAPIView.dispatch`/`handle_exception` renders as
`500 {"error": "Something went wrong please try again later"}` (`views/base.py:297-348`) —
the same shape the GET of 213 already ports (`workspaces/favorite.rs:166-174`). All four
methods are defined, so the whole row flips; `PUT` stays the runtime's 405 (`policy.rs:833-847`,
`dispatch.rs:499-506`).

Already Rust and verified: GET 212/213/214 (`workspaces/favorite.rs`); per-entity creates and
destroys 36/37, 110/111, 126, 149/150, 183/184 (`ROUTES.md:126-127,200-201,216,239-240,273-274`;
`projects/favorite_write.rs`, `cycles/write.rs:1150,1201`, `modules/write_favorite.rs`,
`views/write_favorite.rs`, `pages/write_favorite.rs`). No v1/public/instance favourite route
exists (`routes.json` has these 12 paths).

## 2. Shared pieces

**a. Hooks (every full `save()` reruns them).** POST: H01 create
(`hooks/audit.rs:30-35`, `db/models/base.py:23-44`), `WorkspaceBaseModel.save`'s
`workspace = project.workspace` (`db/models/workspace.py:192-195`, `MODEL_HOOKS.md:520-527`),
H25 (`db/models/favorite.py:52-65`). PATCH: H01 update (`audit.rs:40-42`) + the same workspace
derivation; **no H25** (`_state.adding` false). DELETE: no hook at all — `soft=False` skips
`save()` and the cascade task (`db/mixins.py:72-82`); every favourite destroy (37, 111, 126, 150,
184, 213) is hard, no route soft-deletes one. H25 already exists as
`plane_db::hooks::favorite::before_insert` (`crates/plane-db/src/hooks/favorite.rs:30-40`, table
`hooks/mod.rs:30`), but it must take the body's `sequence`: Django overwrites it **only** when
the aggregate is non-NULL (`db/models/favorite.py:62-63`), so `before_insert(.., requested: Option<f64>)`
returns `max+10000`, else `requested`, else the 65535 default. Five call sites pass `None`
(`cycles/write.rs:1150`, `projects/favorite_write.rs:110`, `modules/write_favorite.rs:96`,
`views/write_favorite.rs:101`, `pages/write_favorite.rs:99`).

**b. `sequence`.** Workspace-**global**, not per user: `MAX(sequence)` over live rows of the
resolved workspace (`db/models/favorite.py:54-61`); the resolved workspace is the *project's*
when `project_id` is set, else the URL's. It races (no lock); two concurrent creates can share a
value, as in Django. PATCH writes the value verbatim, no H25.

**c. Folder / `parent`.** `is_folder` and `parent` are plain writable fields
(`SERIALIZERS.md:1982-1984`); nothing checks that the parent is a folder, the same user, the
same workspace, or not the row itself (`views/workspace/favorite.py:58,82`). A child's
`workspace` comes from its own `project_id`/URL, never from the parent. `parent` is any live
`UserFavorite` (`PrimaryKeyRelatedField(queryset=UserFavorite.objects)`), so a user can parent
under another user's favourite; deleting that row hard-deletes the other user's children
(`models/favorite.py:25-31`, `write/favorite.rs:44-45`).

**d. `entity_identifier` per `entity_type`** — `get_favorite_entities`
(`serializers/favorite.py:70-99`), called only by POST's create branch
(`views/workspace/favorite.py:61-67`): `cycle`/`module`/`issue` = live row with `project_id =
request.data["project_id"]`, an active `ProjectMember` of that project, unarchived project;
`project` = live project with `id = project_id`; `page` = live page linked through a live
`ProjectPage` to `project_id`, plus `owned_by | (access == 0 & not restricted guest)`; `view` =
project branch with `owned_by | (access == 1 & not restricted guest)`, or, when `project_id` is
`None`, the workspace branch (`WorkspaceMember` active, `project_id IS NULL`, owned or
`access == 1` and not a workspace guest); `folder` and every unknown type = no gate
(`:57-67,79-99`; the read port's SQL `workspaces/favorite.rs:255-323`). `issue` **is** gated
though it renders `entity_data: null` (the map has `(Issue, None)`, `:60`); the fixture's
`intake` type is not in the map at all. `entity_identifier` is optional
(`SERIALIZERS.md:1979`): a mapped type without one is `.get(pk=None)` → 404, a `folder` without
one is created.

**e. Responses.** POST/PATCH answer **200** `UserFavoriteSerializer(instance).data` — 10 keys in
declaration order, `entity_data` nested or `null` (`serializers/favorite.py:102-134`,
`plane-serializers/src/generated/app_favorite.rs:205-244`) — rendered from the in-memory instance
(`serializer.data`), never re-read; `entity_data` re-runs the gate with the **row's**
`workspace_id`/`project_id`/`user_id` (`:128-131`). DELETE answers 204 empty. Errors: duplicate
→ 400 `{"error": "Favorite already exists"}` (`views/workspace/favorite.py:75-76`) via
`write::error_body` (`write/mod.rs:146-153`), **not** `ViewError::Integrity`; `DoesNotExist` → 404
`{"error":"The required object does not exist."}` (`rows.rs:274-284`); Django `UUIDField`
`ValidationError` → 400 `{"error":"Please provide valid detail"}`; any other `IntegrityError` →
400 `{"error":"The payload is not valid"}`.

**f. Reuse.** `write::Serializer::{new,from_value}` (`write/serializer.rs:406,428`), `field`
(`:995`), `primary_key_field` (`:660`), `write/fields.rs::uuid_field` (`:512`), `python_str`
(`:264`), `write::now` (`write/mod.rs:79-87`), `hooks::favorite::before_insert`,
`write::favorite::hard_delete` (`write/favorite.rs:55-86` + `favorite_queries.rs:28-48`),
`workspaces/queries::WORKSPACE_ID_BY_SLUG` (`:423-426`), `UserFavorite::{COLUMNS,from_row}`
(`plane-db/src/models/favorite.rs:51-134`), `rows::{timezone,plain_ctx,multiple_objects}`
(`workspaces/rows.rs:201-284`), `body_get` (`views/write_support.rs:28-49`), the entity structs
and `render_favorites` (`workspaces/favorite.rs:60-253`, needs a `pub(super)` one-row split).

**g. `write::generated`.** Add `plane.app.serializers.favorite.UserFavoriteSerializer` to
`WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939-1972`). Two gaps: a plain `uuid` field
falls through to `TODO(uuid)` (`:2404-2407`) — emit `s.field(name, allow_null,
fields::uuid_field)` — and `primary_key_field` (`write/serializer.rs:660-692`) omits both the
manager predicate (DRF's `parent` queryset is `UserFavorite.objects`) and DRF 3.18
`RelatedField.run_validation`'s `'' → None` coercion; a soft-deleted parent must be
`Invalid pk … object does not exist.` Fix both in the helper.

## 3. Split

One set, ~4 h. No P0: H25, `hard_delete`, the row helpers, the serializer structs and the gate
logic all exist; the only shared edits (`before_insert` signature, generator branches,
`primary_key_field`) are used by this set alone. Files it owns:
`workspaces/favorite_write.rs` + `favorite_write_queries.rs` (POST/PATCH/DELETE and the three
500 stubs), the `pub(super)` lifts in `workspaces/favorite.rs`, one `register` block in
`workspaces/mod.rs:78-83`, the `WRITE_SERIALIZERS` entry, `scenarios/write/workspace-favorites.yaml`
+ its `-fixture.sql`. POST (early return, gate, H25, render) is ~70% of the work; if two agents
are free, split POST from PATCH/DELETE, but the row flip needs all four registrations.

## 4. Risks

- **Order, no transactions.** `favorite.py` has no `transaction.atomic`; `ATOMIC_REQUESTS` is
  unset. POST: workspace (`:41`) → body/early return (`:44-55`) → serializer (`:58-59`) → gate
  (`:61-67`) → `save()` (`:68-72`) → render (`:73`). PATCH: lookup (`:80`) → body (`:82`) →
  validate (`:83-84`) → `save()` (`:85`) → render. DELETE: lookup (`:91`) → delete (`:92`).
  A failure after the write leaves it committed. The body is parsed **after** the lookup, and
  `delete` never parses it: do not call `ctx.data()` before the statement the Python runs first,
  and never in `destroy` (a malformed DELETE body is 204/404).
- **The three signature 500s** are reachable only after `@allow_permission` (`permissions/base.py:19-103`),
  so a guest/non-member sees 403, an anonymous caller 401, and only ADMIN/MEMBER the 500. Flip
  the row only by registering `post`/`patch`/`delete` with those branches.
- **Manager predicates.** `UserFavorite.objects` filters `deleted_at` (`db/mixins.py:56-58`) for
  the favourite itself; the `workspace__slug` join adds no `workspaces.deleted_at` predicate, so
  a member of a soft-deleted workspace passes the permission and can PATCH/DELETE there, while
  POST's `Workspace.objects.get` (`views/workspace/favorite.py:41`) 404s. Entity gates filter
  their own model and `ProjectMember` (`deleted_at`, `is_active`), join `projects` only for
  `archived_at`; the page gate joins `ProjectPage` (live links only). `Issue.objects` is the
  soft-deletion manager, so archived/triage/draft issues are favouritable
  (`db/models/issue.py:92,172`).
- **`parent` is unvalidated metadata.** Cross-user (and cross-workspace) parents are accepted,
  PATCH can make a row its own parent, and the destroy collector then hard-deletes every
  descendant — at every depth, soft-deleted children included — whoever owns them
  (`write/favorite.rs:61-86`). This is Django's behaviour, not a port choice.
- **`project_id` is raw.** POST reads `request.data.get("project_id")`, passes it to
  `serializer.save(project_id=…)` and to the gate; `WorkspaceBaseModel.save` then re-homes the
  row to the project's workspace (`db/models/workspace.py:192-195`) and H25 uses that workspace.
  A project in another workspace therefore writes a favourite outside the URL workspace when the
  type has no gate (`folder`, unknown); a nonexistent project is a 404, an uncoercible one 400
  "Please provide valid detail".
- **H25 overwrites `sequence`.** A body `sequence` survives only when the workspace has no live
  favourite (aggregate NULL). `before_insert`'s unconditional 65535 today would be wrong for
  that case; POST must pass the requested value through. Still a `MAX+10000` race.
- **The early return bypasses validation.** `if request.data.get("entity_identifier")` (`:44`)
  returns the stored row (200) before the serializer runs, so every other key — `name`,
  `is_folder`, `project_id`, an over-long value — is ignored, and a repeat POST is 200, not the
  duplicate 400. `entity_type` here is the **raw** value compared through `CharField`'s
  `str()` (a list becomes `"['x']"`); the create branch uses the trimmed validated value, so the
  two can disagree.
- **`entity_identifier` in the early return is Django-coerced**, not DRF-coerced: `"abc"` → 400
  `{"error":"Please provide valid detail"}`, an int/bool → `uuid.UUID(int=…)`, a list/dict → the
  same 400 (Django `UUIDField.get_prep_value`). The list/str/int body itself is an
  `AttributeError` 500 (`body_get`).
- **PATCH's rebuilt body.** Only `name`, `sequence`, `parent` are considered (`:82`), in that
  order, and the value is `request.data[key]` (raw). A scalar body raises `TypeError` in
  `key in request.data` → 500; a list body is a 200 no-op unless it contains the literal
  `"name"`; a string body is a substring test. A form `parent=` / JSON `parent: ""` **clears**
  the parent (DRF 3.18 `RelatedField.run_validation` forces `'' → None`); form `name=` is `""`;
  JSON `sequence: null`/`is_folder: null` are 400. `entity_type`, `entity_identifier`,
  `project_id`, `is_folder` and `workspace_id` cannot be patched. An empty body still rewrites
  `updated_by`/`updated_at` (full save, H01).
- **`entity_data` cost and shape.** One gate query + one entity query per row after the write
  (and per row on the list) — Django does the same; `page`'s `project_id` comes from the
  favourite, not the page; `issue` (gate passed) and `folder`/`intake`/unknown render `null`; a
  page favourite without a project is invisible in the list and its `entity_data` is null.
  `.get()` >1 row would be a 500; the `page` join cannot duplicate (`db/models/page.py:142-148`).
- **Dedup.** `unique_together` normalises to one four-column tuple whose `user`/`deleted_at` are
  not serializer fields, so DRF adds **no** `UniqueTogetherValidator`; the only guard is the
  partial unique index (`db/models/favorite.py:35-41`) → `IntegrityError` → POST's 400 body. A
  row with `entity_identifier: null` is never deduped (NULLs are distinct).
- **Permissions.** WORKSPACE, roles ADMIN/MEMBER, `is_active`, `deleted_at IS NULL`
  (`permissions/base.py:59-66`); every lookup adds `user=request.user` except `parent`.
- **Contract coverage.** `apps/api/plane/tests/contract/app/test_favorite_entity_scope_app.py`
  covers POST 212 (`:124-213`, incl. folder, guest, workspace view, private page/view, foreign
  project) and PATCH 213 (`:188-202`). **None** for DELETE 213, 212 PATCH/DELETE or 213 POST,
  and no repeat POST / invalid `entity_identifier` step. Run
  `tools/contract/run.sh --assume-rust 212,213 --methods POST,PATCH,DELETE`.
- **Fixture.** `plane_fix_tpl` has an admin folder with one admin child, member-owned rows and a
  soft-deleted issue favourite (`tools/fixtures/build.py:2365-2436`); `fix-second` and
  `fix-nologo` have none, `fix-deleted` is soft-deleted (`:469-479`). `workspace-favorites-fixture.sql`
  must add: a live cross-user child + soft-deleted child + grandchild (the cascade, as
  `view-favorites-fixture.sql:49-94`), a favourite in `fix-second` to parent under
  cross-workspace, a soft-deleted parent (the `primary_key_field` control), a `page` favourite
  whose project the user cannot see (null `entity_data`), and sequences above the fixture max. Scenario: repeat
  POST (early return), `folder` without identifier twice, an `intake` row, DELETE then list, the
  three 500s, guest/non-member 403, soft-deleted-workspace 404 vs PATCH 200. Add the new catalog
  to `tests/schema_check.rs::catalogs()`.
