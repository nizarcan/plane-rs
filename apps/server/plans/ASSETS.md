# Assets — port plan (read-only)

Area: `plane/app/views/asset/base.py`, `asset/v2.py`, the asset half of `issue/attachment.py`, and
the `assets` selectors in `deploy/route-groups.yaml:69-75`. The fourteen `GET`s are already Rust
(`crates/plane-http-app/src/assets/`); this plan covers the group's write verbs. `plane/api` and
`plane/space` asset views are Phase 7 (out of scope: `/api/v1/assets/*` orders 273-278,
`ROUTES.md:329-334`).

## 1. Inventory

`routes.json` maps **every** verb of a class onto **both** URL shapes of that class, but the handlers
take different kwargs, so a verb on the wrong shape is a `TypeError` → family 500
`{"error": "Something went wrong please try again later"}` (`plane/app/views/base.py:297-348`) after
the permission check, never a 405. Where the verb's handler *does* take the shape, the outcome is in
brackets.

| # | path (after `/api/`) | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 13 | `workspaces/{slug}/file-assets/` | POST (400 / **500** storage bug), DELETE (**500**: needs `workspace_id,asset_key`) | rust(GET) | B |
| 14 | `workspaces/file-assets/{workspace_id}/{asset_key}/` | DELETE (204/403/404) — POST is 405: `as_view(http_method_names=[…])` (`plane/app/urls/asset.py:31-36`) | rust(GET) | B |
| 15 | `users/file-assets/` | POST (400/**500**), DELETE (**500**: needs `asset_key`) | rust(GET) | B |
| 16 | `users/file-assets/{asset_key}/` | POST (400/**500**), DELETE (204/404) | rust(GET) | B |
| 17 | `workspaces/file-assets/{workspace_id}/{asset_key}/restore/` | POST (204/403/404) — `FileAssetViewSet.restore` (`base.py:71-81`) | django | B |
| 18 | `assets/v2/workspaces/{slug}/` | POST (200 presign), PATCH/DELETE **500** (need `asset_id`) | rust(GET) | A |
| 19 | `assets/v2/workspaces/{slug}/{asset_id}/` | PATCH (204), DELETE (204), POST **500** | rust(GET) | A |
| 20 | `assets/v2/user-assets/` | POST (200 presign), PATCH/DELETE **500** (need `asset_id`) | django | C |
| 21 | `assets/v2/user-assets/{asset_id}/` | PATCH (204), DELETE (204), POST **500** | django | C |
| 22 | `assets/v2/workspaces/{slug}/restore/{asset_id}/` | POST (204/403/404) — `all_objects` (`v2.py:567-581`) | django | C |
| 24 | `assets/v2/workspaces/{slug}/projects/{project_id}/` | POST (200 presign), PATCH/DELETE **500** (need `pk`) | rust(GET) | A |
| 25 | `assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/` | PATCH (204), DELETE (204), POST **500** | rust(GET) | A |
| 26 | `…/projects/{project_id}/{entity_id}/bulk/` | POST (204, bulk rebind) | django | A |
| 28 | `…/duplicate-assets/{asset_id}/` | POST (200; synchronous S3 COPY) | django | C |

(`ROUTES.md:103-120`; `routes.json` orders 13-28; `plane/app/urls/asset.py:24-134`.) **Whole rows:
17, 20, 21, 22, 26, 28.** 13/14/15/16/18/19/24/25 flip to their full method set once their set lands;
the Caddy matcher comes from the Owner cell (`tools/proxy/gen_caddy.py:13-14`), so the unported verbs
keep answering from Django meanwhile. DELETE on 13, POST on 15/19/21/25 and the collection PATCH/
DELETE are *implemented* 500s, so they must be registered (a 501 would be a diff) — the pattern
`assets/v2.rs:79-87` already uses.

**Held methods on 76-79 (issue attachments) — they belong here.** 76 DELETE, 77 POST, 78 PATCH,
78 DELETE, 79 POST are the only unported verbs of those rows (`ROUTES.md:166-169`), and they are asset
behaviour, not issue behaviour: two are the `kwargs["pk"]` `KeyError` → 400
`{"error": "The required key does not exist."}` from `allow_permission(…, creator=True, model=FileAsset)`
(`plane/app/permissions/base.py:20-54` reads `kwargs["pk"]` at `:47`; `PROGRESS.md:572-573`), the other
three the `TypeError` 500 (`attachment.py:38,121,181,244` vs the shape). They live in
`crates/plane-http-app/src/issues/write_attachment.rs` already (`:60-77`), so they go in set C's file
list, not in `assets/`.

## 2. Shared pieces needed first

**a. Hooks.** `FileAsset` has no `save()`/`delete()` override (`MODEL_HOOKS.md:1277-1279`), so every
asset write is H01 only: INSERT = `created_by` = caller, `updated_by` NULL; UPDATE = `updated_by` =
caller. **But `update_fields` decides what is written**: an asset `save(update_fields=[…])` never
touches `updated_at` (Django writes only the listed columns, `MODEL_HOOKS.md:279-285`), while the
*entity* writes are full saves and do:

| call | columns written | hook |
| --- | --- | --- |
| `asset.save(update_fields=["is_deleted"])` (`base.py:64,80`) | `is_deleted` | H01 computed, not written |
| `asset.save(update_fields=["is_deleted","deleted_at"])` (`v2.py:80,239,286,492,580,717`) | two flags | same |
| `asset.save(update_fields=["is_uploaded","attributes"])` (`v2.py:230,471,701`) | two columns | same |
| `workspace.save()` (`v2.py:301,335`) | full row | H01 → `updated_by`, `updated_at` |
| `project.save()` (`v2.py:323,351,754`) | full row | H09 `identifier.strip().upper()` (`project.py:196`), else a plain full save (`hooks/project.rs:58-60`) |
| `user.save()` (`v2.py:93,111,127,140`) | full row | **H30 not ported** (`MODEL_HOOKS.md:2160`): email lower/strip, and `token = uuid4().hex*2` + `token_updated_at = now` whenever `token_updated_at` is not None (`plane/db/models/user.py:169-187`). The fixture makes it non-NULL for every user (`tools/fixtures/build.py:3031` + `pin_timestamps`), so every avatar/cover write rotates `users.token` — the harness's `users.token` rule (`normalize.yaml:463-468`) only hides the *value*, not a missing column. |

`FileAsset` is **not** soft-delete wired: the views set `is_deleted`/`deleted_at` by hand and never
call `SoftDeleteModel.delete`, so H03 does not appear (an issue-attachment v1 `delete()` is the one
exception, already ported as a 500). `Workspace`'s only override is `delete()` (`workspace.py:156`),
not reachable here.

**b. `can_modify_asset` — the whole authorisation rule** (`v2.py:31-67`), used by 13, 14, 17, 18/19,
22, 24/25, 26, 28. One workspace-role lookup by `workspace_id` (`:40-44`, `.values_list("role").first()`
→ `ORDER BY created_at DESC LIMIT 1` from the model's `Meta.ordering`), then: `WORKSPACE_LOGO` →
workspace ADMIN only (`:45-46`); a project-bound asset re-reads the **project** role (`:49-56`), a
workspace ADMIN who is a project member is promoted to project ADMIN (`:57-59`), `PROJECT_COVER` →
project ADMIN only (`:60-61`); then uploader-with-a-role (`:63-64`), `ISSUE_ATTACHMENT` → ADMIN
(`:65-66`), else MEMBER/ADMIN (`:67`). A workspace-less asset (user avatar/cover) returns False for
everything, which is why 20/21 never call it. `WORKSPACE_MEMBER_ROLE_BY_ID`
(`crates/plane-permissions/src/queries.rs:45-62`) is exactly the first lookup; the project one needs a
`workspace_id` variant of `PROJECT_MEMBER_ROLE` (`:63-76`, which joins on slug).

**c. Entity side effects.** Two near-duplicate families, both soft-deleting the *previous* asset row
(id only, `v2.py:278-287`; id + `user_id`, `:73-81`):
* workspace/project: `v2.py:289-326` save, `:328-354` delete — `workspace.logo = ""` / `project.cover_image = ""`
  then the FK, plus three cache invalidations (`:302-309`, `:336-343`).
* user: `v2.py:83-120` save, `:122-149` delete — `user.avatar = ""` / `user.cover_image = None` then
  the FK, plus two invalidations (`:94-100`). Note `entity_asset_save` runs **before** the asset's own
  `save(update_fields=…)`, and the asset may be its own "previous" asset (confirming twice soft-deletes
  the row it then updates) — port the order.
Invalidations must be ported even where the path is not cached, because the harness compares the Redis
diff (`HANDLERS.md:395-403`, `common/cache.rs:117-157`); `/api/instances/` is the only one actually
cached (`plane/license/api/views/instance.py:34`).

**d. S3 call map.** `S3Storage` here is only presigning plus four object ops (`plane/settings/storage.py:22-218`);
`crates/plane-storage` is byte-compatible with it (D6).

| route | what the HTTP request does to S3 | what the worker does |
| --- | --- | --- |
| 18 POST, 20 POST, 24 POST | **presign POST** only (`storage.py:68-102`); no network | — |
| 28 POST | **COPY** synchronously: `storage.copy_object(original.asset, destination)` (`v2.py:952`, `storage.py:171-183`); a `ClientError` becomes `None` silently, a transport error is uncaught → 500 | — |
| 13 POST, 15 POST, 16 POST | `FileField.pre_save` → `S3Storage.save` → `AttributeError: 'S3Storage' object has no attribute 'file_overwrite'` **before any request** (`__init__` never calls the parent, `storage.py:28-66`; `PROGRESS.md:698-700`) → 500, no row, no task | — |
| 18/19 PATCH, 20/21 PATCH, 24/25 PATCH | nothing; enqueues `get_asset_object_metadata` when `storage_metadata` is falsy (`v2.py:219,460,696`) | **HEAD** `head_object` (`storage_metadata_task.py:15-29`, `storage.py:153-169`) → writes `file_assets.storage_metadata` |
| every DELETE / restore / 26 | **nothing** — the S3 object is never deleted; only DB flags change | — (beat `delete_unuploaded_file_asset`, `delete_files` at `authentication/adapter/base.py:259`: out of scope) |

No route PUTs bytes: the browser uploads to MinIO with the presigned POST. `copy_s3_objects_of_description_and_assets`
(`copy_s3_object.py:127`) is the page/issue-duplicate domain's COPY task.

**e. How the harness compares S3.** Presigned GET `Location`s use D17 `presigned_url` mode: all bytes
except `X-Amz-Date`/`X-Amz-Signature`, then each side's SigV4 signature recomputed with its own date and
credentials (`normalize.yaml:129-139`, `crates/plane-testkit/src/presign.rs:1-30`, `normalize.rs:1021-1085`,
`compare.rs:234-238`). Presigned **POST** bodies are compared as JSON with name-scoped placeholders:
`policy`, `x-amz-signature`, `x-amz-date`, `x-amz-credential` (`normalize.yaml:392-407`) — so the policy
*document* is not compared (D6's 281 golden cases are what pin its bytes). `asset` and the POST field
`key` are `placeholder_if_new` but scoped to `*api/assets/v2/*attachments*` / `POST *attachments*`
(`normalize.yaml:380-391`); **the new upload routes need their own request-scoped `key` rule** or every
step diffs on the fresh `uuid4`. S3 *objects* are never compared: the fixture stores no bytes
(`tools/fixtures/build.py:1556-1557`), the testkit has no S3 snapshotter, and both targets share one
`uploads` bucket (`dev/django-env.sh:85-92`). MinIO must be up for 28, or both sides 500.

**f. Serializers and response shapes.** Three shapes: (1) `serializer.data` from `FileAssetSerializer`
with a request context — 24 keys, the existing struct (`plane-serializers/src/generated/app_asset.rs:10-60`,
`assets/support.rs:201-231`); (2) the presigned-upload shape `{upload_data:{url, fields}, asset_id,
asset_url}` (`v2.py:203-210,432-439,674-681`; the 4-key variant with `attachment` at `attachment.py:170-178`
is already `write_attachment.rs:371-430`); (3) `{"error": …, "status": false}` literals. `asset_url` is
`FileAsset.asset_url` (`asset.py:83-104`) → `common::asset_url::file_asset_url`. The legacy POST's *only*
validation is `FileAssetSerializer(data=request.data)` with `asset`+`attributes` writable and everything
else read-only (`plane/app/serializers/asset.py:8-37`): 400 bodies are `{"asset": ["No file was submitted."]}`
and the `JSONField` messages; no generated write serializer is needed (FileField is not expressible) — a
~30-line shim beside `write::Serializer` is enough.

**g. Tasks.** Only `get_asset_object_metadata` (`TASKS.md:479`, kwargs `asset_id=str(uuid)`, builder
`plane_jobs::celery::tasks::get_asset_object_metadata`, `plane-jobs/src/celery/tasks.rs:997-1007`).
The harness drains messages; the worker never runs, so `storage_metadata` stays `{}` on both sides.

**h. P0 (must land before A/B/C).** `assets/support.rs`: `can_modify_asset` + its two role queries;
`sanitize_filename` lifted from `issues/write_attachment.rs:620-635`; the 3-key presign response builder
lifted from `:371-430`; `assets/write_queries.rs`: `FILE_ASSET_INSERT`/`FILE_ASSET_UPDATE`/`FILE_ASSET_BY_ID`
lifted from `:43-57` (add to `assets::queries::ALL_QUERIES`, the catalog is already in
`tests/schema_check.rs:64-68`); `assets/entities.rs`: `asset_delete(scope: Option<Uuid>)` + the shared
`{"error": …}` literals. Harness: the request-scoped `key` rule and `scenarios/write/assets-fixture.sql`.
No `plane-db` hook is shared by two sets (H30 is C-only).

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | none | `assets/support.rs` (can_modify, sanitize, presign body), `assets/write_queries.rs`, `assets/entities.rs` (asset_delete + literals), `normalize.yaml` `key` rule, `scenarios/write/assets-fixture.sql` | ~3 h |
| **A — v2 workspace/project** | 18 POST; 19 PATCH/DELETE; 18 PATCH/DELETE (500); 19 POST (500); 24 POST; 25 PATCH/DELETE; 24 PATCH/DELETE (500); 25 POST (500); 26 POST | `assets/workspace_write.rs` (18/19 + entity save/delete), `assets/project_write.rs` (24/25/26), one `mod.rs` block, `scenarios/write/assets-workspace.yaml` | ~4 h |
| **B — legacy v1** | 13 POST/DELETE; 14 DELETE; 15 POST/DELETE; 16 POST/DELETE; 17 POST | `assets/legacy.rs` (the serializer shim + four 500s + delete/restore), one `mod.rs` block, `scenarios/write/assets-legacy.yaml` | ~3 h |
| **C — user assets + duplicate + held verbs** | 20 POST; 21 PATCH/DELETE; 20 PATCH/DELETE (500); 21 POST (500); 22 POST; 28 POST; 76 DELETE; 77 POST; 78 PATCH/DELETE; 79 POST | `assets/user_write.rs` (20/21/22 + user entity save/delete), `hooks::user::before_save` (H30), `assets/duplicate.rs` (28), `issues/write_attachment.rs` (5 methods), one `mod.rs` block, `scenarios/write/assets-user.yaml` | ~4.5 h |

**Orchestrator revision.** H30 `hooks::user::before_save` moves from set C to **P0**. The
`users/me` writes (`plans/USERS_ME.md`) need it too, so neither domain's sets may own it.

Each set adds its own `pub mod` + one `register` line; only P0 touches a file another set edits. 26 and
28 stay `django` until their set lands; 17/20/21/22 are whole-row flips.

## 4. Risks

- **Write order, no transactions.** `ATOMIC_REQUESTS` is off (`HANDLERS.md:31-37`): 18 POST = permission →
  `int(size)` (500 on garbage) → entity checks → `can_modify_asset` (only when `asset.project_id` truthy,
  `v2.py:420`) → INSERT → presign; 19 PATCH = get (404) → `is_project_accessible_to` (403) → `can_modify_asset`
  (403) → entity save → task → UPDATE; 22/17 restore = get → 403 → UPDATE. A failure after any write leaves
  it committed.
- **`entity_identifier` default `False`, not `None`** (`v2.py:362`): a body without it puts `False` into a
  UUID FK (`workspace_id`/`project_id`/`user_id`/`issue_id`/`page_id`/`comment_id`) → `UUIDField` validation
  error → 500 *after* the permission checks; an explicit `null` gives NULL and succeeds. `ProjectAssetEndpoint.post`
  passes `project_id=` and `**{"project_id": …}` for `PROJECT_COVER` → `TypeError` (duplicate kwarg) → 500
  (`v2.py:658-667`). Both are Django bugs to port.
- **`update_fields` vs full save.** Stamping `updated_at`/`updated_by` on an asset `save(update_fields=…)`
  is the easiest way to fail the DB diff; conversely a *partial* `users`/`workspaces`/`projects` UPDATE is
  observationally fine (the diff compares final rows) **except** H30's token rotation and `token_updated_at`.
- **Managers, and `is_deleted` ≠ `deleted_at`.** `FileAsset.objects` filters `deleted_at`
  (`plane/db/mixins.py:57`), `all_objects` does not (`queries.rs:15-24` documents both). 17 and 13/14/15/16
  use `.get()` → `MultipleObjectsReturned` → 500 for a duplicated key, and they filter by
  `asset=<key>`/`created_by` **without** a workspace scope (only the URL's `workspace_id` is checked by
  `WorkspaceMemberPermission`, `plane/app/permissions/workspace.py:121-137`). The legacy DELETE and restore
  write `is_deleted` only (`base.py:64,80,105-109`), so the row stays visible to `FileAsset.objects` and to
  `AssetCheckEndpoint`; the v2 deletes and restores write both columns (`v2.py:239,286,492,580,717`).
- **`can_modify_asset` edges.** A workspace ADMIN who is *not* a project member gets project role `None`
  (no promotion) → denied unless uploader. `WORKSPACE_LOGO` is admin-only even for the uploader. A
  workspace-less asset is never modifiable. `.first()` relies on `Meta.ordering = ("-created_at",)`
  (`MODEL_HOOKS.md:529-537`) — do not add an `ORDER BY` the ORM does not emit.
- **The wrong-verb family.** 13 DELETE, 15 DELETE, 18 PATCH/DELETE, 19 POST, 20 PATCH/DELETE, 21 POST,
  24 PATCH/DELETE, 25 POST and 76/77/78/79's held verbs are 500s (TypeError) or 400s (`kwargs["pk"]`
  `KeyError`, `permissions/base.py:47`) — never 405. The permission runs first, so a MEMBER/GUEST sees 403
  where an ADMIN sees the 500/400. `routes.json`'s `methods` list has no way to express this; the handler
  branches on `ctx.kwarg(…)` like `assets/v2.rs:79-87`.
- **Presigned POST normalisation.** `key` needs a request-scoped rule (`normalize.yaml:386-391` matches only
  `POST *attachments*`); the `policy` placeholder hides the policy *contents*, so a wrong condition list
  passes the differential suite and is caught only by D6's golden corpus. Keep the `fields` **order**
  (boto3's), not sorted.
- **`copy_object`.** Errors are swallowed (`storage.py:179-181`, `plane-storage/src/storage.rs:570-605`), so
  a missing fixture object still returns 200 in both; a connection failure is a 500 in Django
  (`EndpointConnectionError` is not `ClientError`) and must be one here.
- **28's throttle.** `AssetRateThrottle`, scope `asset_id`, key `throttle_asset_<asset_id>` shared by *all*
  users, 5/minute (`plane/throttles/asset.py:8-15`, `plane/settings/common.py:142-145`). The runtime already
  supplies it (`routes.json` `throttle_classes`, `crates/plane-throttle/src/classes.rs:17,47`,
  `crates/plane-drf/src/dispatch.rs:382`); the 429 body is the authentication adapter's
  (`plane/authentication/adapter/exception.py:23-29`) and the Redis key is handled by the harness
  (`selftest.rs:1113-1147`). Do not exceed five calls per asset id in a scenario.
- **Cache invalidation.** Eight `invalidate_cache_directly` calls over three paths on the user side
  (`v2.py:94-100,112-118,128-134,142-148`) and six over three on the workspace side (`:302-309,336-343`);
  the only one whose key is ever cached is `/api/instances/` (`plane/license/api/views/instance.py:34`).
  Port every call: the Redis diff sees a deleted key.
- **Body shapes.** These views read `request.data.get(…)` (`v2.py:153-156,358-362,618-622,878-880`): a JSON
  **list** body raises `AttributeError` → 500, but `parsers::Data::get` returns `None` for a list
  (`plane-drf/src/parsers.rs:80-86`) — check mapping-ness explicitly. 15/16 have no `JSONParser`
  (`base.py:85`, `routes.json`), so a JSON body there is DRF's 415. `int(request.data.get("size", …))` on a
  non-numeric string is a 500, before any other check.
- **Contract coverage.** `test_asset_role_scope_app.py` (17 tests: the role matrix for 13/14/17/18/19/24/25/26,
  S3 mocked), `test_workspace_asset_routes_project_scope_app.py` (17: restore/duplicate/download/check project
  scoping), `test_workspace_file_asset_project_scope_app.py` (5: 19 PATCH/DELETE project scope),
  `test_legacy_file_asset_post_readonly_app.py` (2: forged fields ignored — it monkeypatches
  `FileSystemStorage`, so it only means anything on Django), `test_file_asset_legacy_route_app.py` (2: POST on
  14 is 405), `test_guest_issue_subresource_scope_app.py:151-190` (76/78 for a restricted guest),
  `test_issue_id_project_scope_app.py:123-175` (78 POST). **Nothing** covers 15/16/20/21/22 POST/DELETE, the
  500 family or the `entity_identifier` 500s: the scenarios are the evidence. Run
  `tools/contract/run.sh --assume-rust 13,14,15,16,17,18,19,20,21,22,24,25,26,28 --methods POST,PATCH,DELETE`;
  the S3-mocking tests are served by Django (`PROGRESS.md:316-318`).
- **Fixture gaps** (`tools/fixtures/build.py:1556-1663`): no soft-deleted asset (22), no `USER_COVER`, no
  `PAGE_DESCRIPTION`/`DRAFT_ISSUE_DESCRIPTION`, no asset in `ws2`, no two rows with the same `asset` key
  (`MultipleObjectsReturned`), no never-uploaded row, no NULL `storage_metadata`, no asset created by the
  guest. Add them by `fixture_sql`, plus a `PROJECT_COVER` whose `project_id` is another workspace's project
  (the 403 at `v2.py:917-928`). Steps: 18 POST per entity type (logo → 403 for a member), the
  `entity_identifier`-missing 500 and the `null` 200, 19 PATCH confirming twice (self-delete), 26 bulk per
  entity type and the `No asset ids provided.` 400, 28 duplicate with and without `project_id`, the presign
  bodies, and the 13/15/18/19/24/25 wrong-verb 500s.
