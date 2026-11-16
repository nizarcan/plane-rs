# Pages — port plan (read-only)

Area: `plane/app/views/page/**` (`base.py`, `version.py`, `live.py`). The GETs are already Rust
(`crates/plane-http-app/src/pages/{base,version,live}.rs`); this plan covers the write verbs, the
description binary/HTML pipeline and page favourites. `version.py` has no write method. No page
row exists under `/api/v1/` or `/api/public/`.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/projects/{project_id}/`. "Rust" = `ROUTES.md`'s Owner cell.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 124 | `pages/` | POST | rust(GET) | A |
| 125 | `pages/{page_id}/` | PATCH, DELETE (no PUT on the row) | rust(GET) | A |
| 127 | `pages/{page_id}/archive/` | POST, DELETE | whole | A |
| 128 | `pages/{page_id}/lock/` | POST, DELETE | whole | B |
| 129 | `pages/{page_id}/access/` | POST | whole | B |
| 130 | `pages/{page_id}/description/` | PATCH | rust(GET) | B |
| 133 | `pages/{page_id}/duplicate/` | POST | whole | B |
| 126 | `favorite-pages/{page_id}/` | POST, DELETE | whole | C |

(`ROUTES.md:214-223`.) **Whole rows: 126, 127, 128, 129, 133.** 123 `pages-summary/`, 131/132
`versions/` and 134 `live-access/` are GET-only and verified. Every page path is in the `pages`
group by selector, including `favorite-pages/**` (`deploy/route-groups.yaml:189-196`), so all of
this registers from `pages::register` (`crates/plane-http-app/tests/registry.rs:72`); no other group
claims one. 125/126/127/128/129 map a fixed verb list (`plane/app/urls/page.py:28-56`), so there is no
guard action to register (unlike views 179); a verb absent from `routes.json`'s `methods` is the
pipeline's 405.

## 2. Shared pieces needed first

**a. Hooks.** H26 `Page.save` (`models/page.py:70-77`) is the only one missing: `description_stripped
= None if html in ("", None) else strip_tags(html)` on **every** save (`MODEL_HOOKS.md:2156`); add
`plane-db/src/hooks/page.rs::before_save` over `hooks::html::strip_tags` (already ported, `hooks/html.rs:8-40`)
plus its row in `hooks/mod.rs`. H27 (`models/page.py:175-182`) is **not** needed: no route writes
`PageVersion`; rows come from the `track_page_version` worker (`TASKS.md:559`). H01 (`models/base.py:23-44`),
H03 (`mixins.py:61-82`) and H04 (`mixins.py:48-53`) exist.

**b. Through tables.** `ProjectPage` (`models/page.py:135-152`, partial unique on the live pair) is
inserted one row at a time with `created_by_id=page.created_by_id`, `updated_by_id=page.updated_by_id`
(NULL on create, `serializers/page.py:83-89`). `PageLabel` (`:120-129`, **no** constraint) is
`bulk_create([...], batch_size=10)` — plain INSERT, no `ignore_conflicts` (`serializers/page.py:93-105`),
one outermost `BEGIN/COMMIT` over all batches (`hooks/issue_m2m.rs:16-31`). PATCH replaces labels:
`PageLabel.objects.filter(page=instance).delete()` = H04 (`deleted_at` only, no `updated_at`, no
task) then the same insert with the **page's** audit ids (`serializers/page.py:108-126`). The existing
helper does not fit: `issue_m2m::insert_membership*` hardcodes `issue_id`/`project_id`
(`issue_m2m.rs:396-399`) while `page_labels` is (label, page, workspace) and `project_pages` is
(project, page, workspace) → a page-local batch insert, one `BEGIN`, `created_at` +1 µs per row like
`issue_m2m.rs:403-427`. It must stay outermost: a nested `client.transaction()` would make its bare
`BEGIN` fail (the nested case `issue_m2m.rs:29-31` documents but does not implement).

**c. Description.** create takes `description_json`/`description_binary`/`description_html` **raw**
from `request.data`, unvalidated, defaults `{}`/`None`/`"<p></p>"` (`page/base.py:151-153`). Only
`PATCH …/description/` validates: base64 → bytes (`serializers/page.py:180-198`), 10 MB, ≥4 bytes and
6 suspicious patterns in the first 200 decoded chars (`content_validator.py:29-68`); HTML through nh3
with a 10 MB UTF-8 limit (`:209-243`). `write::description` exists but returns a bare bool and the
issue-flavoured bodies (`write/description.rs:105-129`); pages need the reason inside
`Invalid binary data: <reason>` and the size message under `description_html`. `description_stripped`
is H26.

**d. Tasks.** All shapes exist — `page_transaction` (`tasks.rs:954-976`), `track_page_version`
(`:941-951`), `copy_s3_objects_of_description_and_assets` (`:796-815`); call sites `TASKS.md:768-781`,
`:604`. Correction to `TASKS.md:1141`: create's `page_id=serializer.data["id"]` (`page/base.py:160`)
is a **UUID object**, not a string — `PrimaryKeyRelatedField.to_representation` returns the pk (the
same rule the crawler-doc spells out, `tasks.rs:1040-1046`); the other three sites are UUIDs too.
No new shapes.

**e. `write::generated`.** Add `PageSerializer`, `PageDetailSerializer` and `PageBinaryUpdateSerializer`
to `WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939`); tables at `SERIALIZERS.md:3155-3210`.
Writable order is `Meta.fields` (`serializers/page.py:38-58`), where `labels` sits between `color` and
`parent`. `label_ids`/`project_ids` are `ListField(child=UUIDField())` — the one missing kind
(`write/serializer.rs:666-755` only handles PK lists): add `uuid_list_field` (list-shape errors plus
the per-index error object) in set A. They are **validated but never written**. The declared
`validate_description_binary`/`validate_description_html` are not generated; call `write::description`
by hand.

**f. Response re-render.** Four shapes. Create re-fetches through `get_queryset()`
(`page/base.py:165`), so all three annotations are present: reuse `queries::PAGES_RETRIEVE`
(`pages/base.rs:408-419`), or the dynamic statement (`:421-454`) when `?search=` is set. PATCH renders
the plain `Page.objects.get` instance → the three attributes are `SkipField` → 16 keys; duplicate
annotates only `project_ids` (`page/base.py:625-634`) → 17 keys. Parameterise `PageDetailBody`
(`pages/base.rs:184-226`) with `FieldValue` for those three. JSON columns render from the values
written, never a re-read (`HANDLERS.md:209-226`): `view_props` default `{"full_width": false}`
(`models/page.py:19-20`), `logo_props` `{}`.

**g. Reuse.** 126 is `modules::write_favorite` (`modules/write_favorite.rs:58-140`) with the page gate
`get_favorite_entities("page", …)` (`serializers/favorite.py:93-104`); its `PROJECT_WORKSPACE_ANY`,
`USER_FAVORITE_INSERT`, `USER_FAVORITE_LOOKUP` and `USER_FAVORITE_HARD_DELETE` are already `pub` in
`modules/write_issue_queries.rs`. Only the gate statement is new.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **A — page core** | 124, 125, 127 | `pages/write_page.rs`, `pages/write_page_queries.rs` (Page INSERT, PATCH UPDATE, children `parent=NULL`, recursive archive CTE, H04 favourite delete, recent-visit hard delete), `plane-db/src/hooks/page.rs`, `write::uuid_list_field`, one register block in `pages/mod.rs` | ~2 h (H26, labels replace, `ProjectPage`, post-write 404, archive/unarchive) |
| **B — description, duplicate, lock/access** | 128, 129, 130, 133 | `pages/write_description.rs`, `pages/write_duplicate.rs`, `pages/write_action.rs`, `pages/write_action_queries.rs`, extensions to `write/description.rs` | ~1.5 h (base64/nh3 errors, two-task order, duplicate re-insert + two publishes) |
| **C — favourites** | 126 | `pages/write_favorite.rs` + one gate statement + one register block | ~40 min (every helper exists; can fold into B) |

A and B share only `pages/mod.rs` (one block each) and the `FieldValue` change in `pages/base.rs`
(A owns it; B consumes it). C is independent.

**Orchestrator revision.** H26 is needed by every full `save()`: create/PATCH (`page/base.py:158,201`),
lock/unlock/access (`:250,262,283`), description (`:554`) and duplicate (`:599`). The shared pieces
therefore land first, and duplicate moves out of B because it is the only B route that needs the
re-render change:

| set | routes | owns | starts |
| --- | --- | --- | --- |
| **P0 — shared** | none | `plane-db/src/hooks/page.rs` (H26) + `hooks/mod.rs` row; `PageDetailBody` `FieldValue` parameterisation in `pages/base.rs` (GET output unchanged) | now |
| **C — favourites** | 126 | as above | now, in parallel with P0 |
| **A — page core** | 124, 125, 127 | as above minus H26/`FieldValue`; adds `PageSerializer`/`PageDetailSerializer` to `WRITE_SERIALIZERS` | after P0 merges |
| **B — description, lock/access** | 128, 129, 130 | `write_description.rs`, `write_action{,_queries}.rs`, `write/description.rs` extensions; adds `PageBinaryUpdateSerializer` | after P0 merges |
| **D — duplicate** | 133 | `pages/write_duplicate{,_queries}.rs` | after P0 merges |

A, B and D run in parallel; their `pages/mod.rs` blocks and `WRITE_SERIALIZERS` entries conflict
trivially and are resolved at merge (keep all).

## 4. Risks

- **Order, no transactions.** Create: serializer (labels PK lookups, UUID lists) → `Project.objects.get(pk=…)`
  (unscoped by workspace, `page/base.py:70`) → Page INSERT (H01 create: `created_by=user`,
  `updated_by=NULL`; H26; `sort_order` stays 65535) → `ProjectPage` INSERT (`:83`) → `PageLabel`
  bulk insert (`:93`) → `page_transaction` (`:160`) → annotated re-fetch (`:165`) → 201. Nothing opens
  `transaction.atomic`; keep it so, because the create can answer 404 *after* the writes.
- **Create's post-write 404.** The re-fetch is `get_queryset()`: `parent__isnull=True` (`:99`) and
  `?search=` (`plane/app/views/base.py:72`). A POST with a `parent`, or a `?search=` the new page does
  not match, writes page + link + labels and publishes the task, then misses its own row and answers
  `404 {"error": "The required object does not exist."}` (`plane/app/views/base.py:219-223`).
  `description_html: null` hits the NOT NULL column (`:153`, `models/page.py:34`) → `IntegrityError` →
  400 `{"error": "The payload is not valid"}` (`plane/app/views/base.py:209-213`).
- **PATCH quirks.** A missing page (or a soft-deleted link) is **400** with the "Access cannot be
  updated since this page is owned by someone else" body (`page/base.py:172-177,212-216`); the same
  `except` swallows a bad/nonexistent `parent` (`:184-189`). `is_locked` → 400 `{"error": "Page is
  locked"}` first (`:179-180`). `access` is checked with default `page.access` but written with
  default `0` (`:267` vs `:276`) — benign only because `ProjectPagePermission` already 403s a
  non-owner on a private page (`permissions/page.py:57-68`). `created_by` is writable
  (`serializers/page.py:59` makes only `workspace`/`owned_by` read-only), so PATCH can rewrite the
  creator; H01 only overwrites `updated_by` (`models/base.py:35-42`). Labels accept **any** live
  `Label.objects.all()`, cross-workspace included (`serializers/page.py:28`).
- **Delete manager predicates.** `page.delete()` (`:399`) is H03: a bare UPDATE by pk (`hooks/soft_delete.rs:19-21`)
  whose full save re-runs H26 (`mixins.py:73-76`). The favourites (`:401`) and recent visits (`:408`)
  are **queryset** deletes, so `deleted_at IS NULL` is part of the SQL (`mixins.py:56-58`, `models/recent_visit.py:22`);
  the modules port dropped that predicate for recent visits (`plans/VIEWS.md:152-155`) — do not copy
  that here. `page_favorite.delete(soft=False)` (`page/base.py:491`) is `Model.delete()`, so Django's
  collector also hard-deletes child favourites through `user_favorites.parent` (`models/favorite.py:25-31`);
  a bare `DELETE … WHERE id=$1` (the modules shape) skips that — port it or record the diff.
- **Archive recursion.** The CTE (`page/base.py:61-75`) sets `archived_at=datetime.now()` — naive
  **local** time, into a `DateField` — for the page and every descendant, ignores `deleted_at`,
  bypasses H26/H01 and does not touch `updated_at` (`MODEL_HOOKS.md:1759-1761`). The body re-reads the
  clock: `{"archived_at": str(datetime.now())}` (`:332`), Python `str()` (space separator, microseconds,
  no `Z`). Archive soft-deletes **every** user's favourite (`:323-328`); unarchive does not restore
  them, detaches an archived parent with `save(update_fields=["parent"])` (`:355-357`, only `parent`
  is written) and then NULLs the subtree (`:359`).
- **Description order and bodies.** Locked/archived are two-key bodies in order — `error_code`
  4701/4702 then `error_message` (`page/base.py:527-543`, `utils/error_codes.py:12-13`). `serializer.save()`
  is a full `instance.save()`; `page_transaction` receives the **raw** body HTML while the row stores
  the sanitised string (`:557-562`); `track_page_version` is published on every valid call, even an
  empty body (`:565-569`), after the transaction publish. `description_json: null` passes the
  serializer and then violates the NOT NULL column → 400 `{"error": "The payload is not valid"}`
  (`views/base.py:306-310`). 125 never sanitises `description_html` (only `CharField()`), and rejects
  `""` (`allow_blank=False`) — unlike create.
- **Duplicate.** `page.pk = None` + full `save()` re-inserts every column (`page/base.py:593-599`):
  fresh `created_at`/`updated_at` (`auto_now_add`/`auto_now`, `django/db/models/sql/compiler.py:1743-1748`),
  `sort_order`/`view_props`/`logo_props`/`access`/`is_locked`/`archived_at`/`parent` copied, name
  `"… (Copy)"`, `description_binary` dropped. One `ProjectPage` per **live** link (`:591,601-608`) —
  no labels, logs or versions. The `copy_s3_objects` publish (`:617-623`) reads the **loop variable**
  `project_id`, i.e. the last row of `ProjectPage.objects.filter(page_id=…)` under
  `Meta.ordering=("-created_at",)` — not the URL project (`TASKS.md:604` is wrong); non-deterministic
  on a tie, so seed single-link pages. S3 bytes are not compared; the DB diff only sees `project_pages`.
- **Live server auth.** `apps/live` calls 125 GET/PATCH, 130 GET/PATCH and 134 GET with a `Cookie`
  header and **no `Origin`** (`services/page/project-page.service.ts:20-30`,
  `services/page/core.service.ts:28,43,91,120`); a missing `Origin` is allowed
  (`authentication/session.py:40-43`, `crates/plane-auth/src/origin.rs:86-97`) — do not "fix" it.
  Its `…/pages/{id}/mentions/` call (`core.service.ts:140`) has no route at all.
- **Contract coverage.** Covered: 126 POST (`tests/contract/app/test_favorite_entity_scope_app.py:114`,
  scoping at `:139-186`) and the GETs (`test_page_guest_view_scope_app.py:20-28`,
  `test_page_order_by_allowlist_app.py:32`, `test_page_version_project_scope_app.py:32`).
  **None** for 124 POST, 125 PATCH/DELETE, 126 DELETE, 127, 128, 129, 130 PATCH, 133.
- **Harness.** New `scenarios/write/pages.yaml` + `pages-fixture.sql`: parent + archived child, a
  locked page, a private page owned by another member, two users' favourites and recent visits,
  labels, a page linked to two projects, a page with a `PageVersion`. Description PATCH bodies are
  JSON base64, so no raw-binary request support is needed; the description GET already compares as
  bytes (`crates/plane-testkit/src/compare.rs:318`). Add the new `pages/write_*queries.rs` to
  `tests/schema_check.rs::catalogs()` (orchestrator) and run with `PLANE_RS_REQUIRE_DB_TESTS=1`, plus
  `tools/contract/run.sh --assume-rust 124,125,126,127,128,129,130,133 --methods POST,PATCH,DELETE`.
