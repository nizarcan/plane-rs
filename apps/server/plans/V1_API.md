# Public API `/api/v1/` — port plan (read-only)

Area: `apps/api/plane/api/` — 79 resolver rows (`ROUTES.md:325-407`, `routes.json` group `v1`, orders
273–351), 151 endpoint×method pairs. Phase 7 exit check: "Published API docs still accurate; contract
tests green" (`RUST_REWRITE.md:211`). Every row is `django | not-started`; the caddy group `v1` exists
with zero routes (`deploy/route-groups.yaml:280-284`, `deploy/caddy/rust-routes.caddy:95`).

The mount is the `path()` routes in `plane/api/urls/*.py` plus two `include(router.urls)` expansions
(invitations, stickies: `.{format}` variants and two api-roots, `urls/__init__.py:12-34`,
`urls/invite.py:16-22`, `urls/sticky.py:12-18`). `urls/schema.py` and `urls/estimate.py` are never
imported, so those endpoints do not exist (`ROUTES.md:76`).

## 1. Inventory

`ws` = `/workspaces/{slug}`, `pr` = `/workspaces/{slug}/projects/{project_id}`. `#` is `routes.json`'s
order; methods are the mapped actions (a router collection 405s the other verbs). "Reuse" is a
`plane-http-app` module whose SQL/logic is lifted or forked; `new` means nothing exists to copy.

| # | path (under `/api/v1`) | M | Django view (`plane.api.views.`) | reuse | set |
| --- | --- | --- | --- | --- | --- |
| 273 | `/assets/user-assets/` | POST | `asset.UserAssetEndpoint` | `assets::user_write` post | B |
| 274 | `/assets/user-assets/{asset_id}/` | PATCH,DELETE | `asset.UserAssetEndpoint` | `assets::user_write` patch/delete | B |
| 275 | `/assets/user-assets/server/` | POST | `asset.UserServerAssetEndpoint` | `assets::user_write` post (identical) | B |
| 276 | `/assets/user-assets/{asset_id}/server/` | PATCH,DELETE | `asset.UserServerAssetEndpoint` | `assets::user_write` patch/delete | B |
| 277 | `ws/assets/` | POST | `asset.GenericAssetEndpoint` | `assets::workspace_write` post | B |
| 278 | `ws/assets/{asset_id}/` | GET,PATCH | `asset.GenericAssetEndpoint` | `assets::v2` presign + `workspace_write` patch | B |
| 279 | `pr/cycles/` | GET,POST | `cycle.CycleListCreateAPIEndpoint` | `cycles::base` list; new create | C |
| 280 | `pr/cycles-lite/` | GET | `cycle.CycleListLiteAPIEndpoint` | new; `cycles::queries` | C |
| 281 | `pr/cycles/{pk}/` | GET,PATCH,DELETE | `cycle.CycleDetailAPIEndpoint` | `cycles::base`, `cycles::write` | C |
| 282 | `pr/cycles/{cycle_id}/cycle-issues/` | GET,POST | `cycle.CycleIssueListCreateAPIEndpoint` | `cycles::issue_list`, `write_issue` | C |
| 283 | `pr/cycles/{cycle_id}/cycle-issues/{issue_id}/` | GET,DELETE | `cycle.CycleIssueDetailAPIEndpoint` | `cycles::issue_list`, `write_issue` delete | C |
| 284 | `pr/cycles/{cycle_id}/transfer-issues/` | POST | `cycle.TransferCycleIssueAPIEndpoint` | `cycles::write_transfer` kernel + v1 pre-check | C |
| 285 | `pr/cycles/{cycle_id}/archive/` | POST | `cycle.CycleArchiveUnarchiveAPIEndpoint` | `cycles::archive` | C |
| 286 | `pr/archived-cycles/` | GET | `cycle.CycleArchiveUnarchiveAPIEndpoint` | `cycles::archive` list | C |
| 287 | `pr/archived-cycles/{cycle_id}/unarchive/` | DELETE | `cycle.CycleArchiveUnarchiveAPIEndpoint` | `cycles::archive` | C |
| 288 | `pr/intake-issues/` | GET,POST | `intake.IntakeIssueListCreateAPIEndpoint` | `intake::issue` list; new create | B |
| 289 | `pr/intake-issues/{issue_id}/` | GET,PATCH,DELETE | `intake.IntakeIssueDetailAPIEndpoint` | `intake::write_intake_issue_{patch,delete}` (fork) | B |
| 290 | `pr/labels/` | GET,POST | `issue.LabelListCreateAPIEndpoint` | `labels::label`, `labels::write` | B |
| 291 | `pr/labels/{pk}/` | GET,PATCH,DELETE | `issue.LabelDetailAPIEndpoint` | `labels::label`, `labels::write` | B |
| 292/294 | `pr/members/`, `pr/project-members/` | GET,POST | `member.ProjectMemberListCreateAPIEndpoint` | `projects::member`, `member_write` | A |
| 293/296 | `pr/members/{pk}/`, `pr/project-members/{pk}/` | GET,PATCH,DELETE | `member.ProjectMemberDetailAPIEndpoint` | `projects::member`, `member_write` | A |
| 295 | `pr/project-members-lite/` | GET | `member.ProjectMemberLiteAPIEndpoint` | new; `projects::member_queries` | A |
| 297 | `ws/members/` | GET | `member.WorkspaceMemberAPIEndpoint` | `workspaces::member` (lite render) | A |
| 298 | `ws/members-lite/` | GET | `member.WorkspaceMemberLiteAPIEndpoint` | new; `workspaces::queries` | A |
| 299 | `pr/modules/` | GET,POST | `module.ModuleListCreateAPIEndpoint` | `modules::base`, `modules::write` | C |
| 300 | `pr/modules-lite/` | GET | `module.ModuleListLiteAPIEndpoint` | new; `modules::queries` | C |
| 301 | `pr/modules/{pk}/` | GET,PATCH,DELETE | `module.ModuleDetailAPIEndpoint` | `modules::base`, `modules::write` | C |
| 302 | `pr/modules/{module_id}/module-issues/` | GET,POST | `module.ModuleIssueListCreateAPIEndpoint` | `modules::issue`, `write_issue_module` | C |
| 303 | `pr/modules/{module_id}/module-issues/{issue_id}/` | DELETE | `module.ModuleIssueDetailAPIEndpoint` | `modules::write_issue_module` delete | C |
| 304 | `pr/modules/{pk}/archive/` | POST | `module.ModuleArchiveUnarchiveAPIEndpoint` | `modules::archive` | C |
| 305 | `pr/archived-modules/` | GET | `module.ModuleArchiveUnarchiveAPIEndpoint` | `modules::archive` list | C |
| 306 | `pr/archived-modules/{pk}/unarchive/` | DELETE | `module.ModuleArchiveUnarchiveAPIEndpoint` | `modules::archive` | C |
| 307 | `ws/projects/` | GET,POST | `project.ProjectListCreateAPIEndpoint` | `projects::base` list, `projects::write` | A |
| 308 | `ws/projects-lite/` | GET | `project.ProjectListLiteAPIEndpoint` | `projects::lite` | A |
| 309 | `ws/projects/{pk}/` | GET,PATCH,DELETE | `project.ProjectDetailAPIEndpoint` | `projects::base`, `projects::write` | A |
| 310 | `ws/projects/{project_id}/archive/` | POST,DELETE | `project.ProjectArchiveUnarchiveAPIEndpoint` | `projects::archive_write` | A |
| 311 | `ws/projects/{project_id}/summary/` | GET | `project.ProjectSummaryAPIEndpoint` | new (COUNT subqueries) | A |
| 312 | `pr/states/` | GET,POST | `state.StateListCreateAPIEndpoint` | `states::base`, `states::write` | B |
| 313 | `pr/states/{state_id}/` | GET,PATCH,DELETE | `state.StateDetailAPIEndpoint` | `states::base`, `states::write` | B |
| 314 | `/users/me/` | GET | `user.UserEndpoint` | new trivial; `users_me::support` | A |
| 315/327 | `ws/issues/search/`, `ws/work-items/search/` | GET | `issue.IssueSearchEndpoint` | new SQL (`issue.py:2448-2502`) | D |
| 316/328 | `ws/{issues,work-items}/{project_identifier}-{issue_identifier}/` | GET | `issue.WorkspaceIssueAPIEndpoint` | new; identifier lookup | D |
| 317/329 | `pr/{issues,work-items}/` | GET,POST | `issue.IssueListCreateAPIEndpoint` | `issues::list_list_endpoint`, `write_issue` (forks) | D |
| 318/330 | `pr/{issues,work-items}/{pk}/` | GET,PATCH,DELETE | `issue.IssueDetailAPIEndpoint` | `issues::list_retrieve`, `write_issue` | D |
| 319/331 | `pr/{issues,work-items}/{issue_id}/links/` | GET,POST | `issue.IssueLinkListCreateAPIEndpoint` | `issues::sub_links`, `write_link` | D |
| 320/332 | `pr/{issues,work-items}/{issue_id}/links/{pk}/` | GET,PATCH,DELETE | `issue.IssueLinkDetailAPIEndpoint` | `issues::sub_links`, `write_link` | D |
| 321/333 | `pr/{issues,work-items}/{issue_id}/comments/` | GET,POST | `issue.IssueCommentListCreateAPIEndpoint` | `issues::sub_comments`, `write_comment` | D |
| 322/334 | `pr/{issues,work-items}/{issue_id}/comments/{pk}/` | GET,PATCH,DELETE | `issue.IssueCommentDetailAPIEndpoint` | `issues::sub_comments`, `write_comment` | D |
| 323/335 | `pr/{issues,work-items}/{issue_id}/activities/` | GET | `issue.IssueActivityListAPIEndpoint` | `issues::sub_activity` | D |
| 324/336 | `pr/{issues,work-items}/{issue_id}/activities/{pk}/` | GET | `issue.IssueActivityDetailAPIEndpoint` | `issues::sub_activity` | D |
| 325/337 | `pr/{issues,work-items}/{issue_id}/{issue-attachments,attachments}/` | GET,POST | `issue.IssueAttachmentListCreateAPIEndpoint` | `issues::sub_attachments`, `assets::attachment` | D |
| 326/338 | `pr/{issues,work-items}/{issue_id}/{issue-attachments,attachments}/{pk}/` | GET,PATCH,DELETE | `issue.IssueAttachmentDetailAPIEndpoint` | `issues::sub_attachments`, `assets::attachment` | D |
| 339 | `pr/work-items/{issue_id}/relations/` | GET,POST | `issue.IssueRelationListCreateAPIEndpoint` | `issues::sub_relation` (fork), `write_relation` | D |
| 340 | `ws/invitations/` | GET,POST | `invite.WorkspaceInvitationsViewset` | `workspaces::invite`, `invite_write` (POST always 400, §5) | A |
| 341 | `ws/invitations.{format}` | GET,POST | `invite.WorkspaceInvitationsViewset` | family 500 (see notes) | A |
| 342 | `ws/invitations/{pk}/` | GET,PUT,PATCH,DELETE | `invite.WorkspaceInvitationsViewset` | `workspaces::invite`, `invite_write` | A |
| 343 | `ws/invitations/{pk}.{format}` | GET,PUT,PATCH,DELETE | `invite.WorkspaceInvitationsViewset` | family 500 | A |
| 344/345 | `ws/`, `ws/.{format}` | GET | `rest_framework.routers.APIRootView` | new (`{"invitations": <abs url>}`) | A |
| 346 | `ws/stickies/` | GET,POST | `sticky.StickyViewSet` | `stickies::{base,write}` (list order differs) | A |
| 347 | `ws/stickies.{format}` | GET,POST | `sticky.StickyViewSet` | family 500 | A |
| 348 | `ws/stickies/{pk}/` | GET,PUT,PATCH,DELETE | `sticky.StickyViewSet` | `stickies::{base,write}` (detail reuses verbatim) | A |
| 349 | `ws/stickies/{pk}.{format}` | GET,PUT,PATCH,DELETE | `sticky.StickyViewSet` | family 500 | A |
| 350/351 | `ws/`, `ws/.{format}` | GET | `rest_framework.routers.APIRootView` | dead (344/345 win) | A |

Notes that change the table:

* **Format routes are family 500s.** The router's format-suffix patterns pass a `format` kwarg the
  view methods do not accept (`invite.py:55,75,89,112,141`; `sticky.py:48,66,85,96,110`) → `TypeError` after the
  permission class. The read scenario records `500×1/500×2` for 341/343/347/349
  (`scenarios/read/COVERAGE.md:312,314,318,320,411-414`). `format` is `[a-z0-9]+`
  (`bins/plane-api/src/routes.rs:36`), so `.JSON` is 404.
* **The invitations api-root shadows the stickies one** — both register `ws/`, first match wins
  (`ROUTES.md:400-407`, `PROGRESS.md:565`). 350/351 are unreachable; flip them with 344/345.
* **HEAD/OPTIONS.** Every `GenericAPIView` row is `as_view(http_method_names=[...])`, replacing the
  class list; `dispatch` therefore sends HEAD/OPTIONS to `http_method_not_allowed` → 405
  (`py:rest_framework/views.py:513-520`), even though `routes.json`'s `implicit_methods` records the
  class methods. Only the `BaseViewSet` rows (340–349) and api-roots answer OPTIONS. Flip only the
  listed verbs, and make Rust 405 an implicit HEAD on these rows rather than run the GET handler.

## 2. Differences from the app surface

**Auth.** Both bases set `authentication_classes = [APIKeyAuthentication]` only
(`plane/api/views/base.py:53,165`), so a session cookie is not accepted on 273–349 — `X-Api-Key` is the
sole credential (`middleware/api_authentication.py:25`). The two `drf.APIView` api-roots keep
`DEFAULT_AUTHENTICATION_CLASSES = (SessionAuthentication,)` (`plane/settings/common.py:139`): they take
a session but **not** an API key (`COVERAGE.md:315-316`). The lookup is `expired_at > now OR NULL`,
`is_active`, `user__is_active`, soft-deleted excluded, and it writes `last_used` with
`update_fields=["last_used"]` on every success (`api_authentication.py:29-42`). Rust has this
(`plane-auth/src/api_key.rs:93-123`, `plane-drf/src/auth.rs:183-209`) but borrows the route's
connection — the read replica when `use_read_replica` is set (`dispatch.rs:227`,
`plane-db/src/pool.rs:257-262`) — while Django reads/writes `api_tokens` on the default DB. P0.

**Permissions.** v1 imports `plane.utils.permissions` (`workspace.py:51-110`, `project.py:16-162`), the
same logic as `plane.app.permissions` minus `ProjectArchiveUnarchivePermission`; all names already
resolve in `plane-permissions/src/classes.rs:136-165`. Roles: `WorkspaceOwnerPermission` = 20;
`WorkSpaceAdminPermission` = 20/15; `WorkspaceUserPermission`/`ProjectLitePermission` = any active
member (guest included); `ProjectEntityPermission` = any active project member on safe methods, 20/15
on unsafe; `ProjectBasePermission` = project admin, or project member **and** workspace admin;
`ProjectMemberPermission` POST scoped to the URL project. Every filter checks
`WorkspaceMember.is_active` only, never `workspace.deleted_at` (`PROGRESS.md:567`), so a member of a
soft-deleted workspace passes. `BaseAPIView.project_id` resolves `kwargs["pk"]` only when
`resolve(path).url_name == "project"` (`base.py:143-150`). `ProjectMember*` `get_permissions` is
dynamic — GET → `ProjectMemberPermission`, everything else (HEAD/OPTIONS too) →
`ProjectAdminPermission` (`member.py:106-109`); labels/states swap to `ProjectAdminPermission` on
writes (`issue.py:957-960`, `state.py:176-179`).

**Throttles and headers.** `BaseAPIView.get_throttles` → `[ApiKeyRateThrottle]` (`base.py:69-70`), rate
`API_KEY_RATE_LIMIT` (default `60/minute`, `plane/settings/common.py:163`), key `api_key:<raw token>`
(`rate_limit.py:16-23`; the raw token is the Redis key, `PROGRESS.md:545`). It writes
`X-RateLimit-Remaining`/`X-RateLimit-Reset` even with no key, and `BaseAPIView.finalize_response` copies
both onto **every** response, family 4xx/5xx included (`rate_limit.py:25-50`, `base.py:123-136`).
`BaseViewSet` overrides neither (`base.py:162-279`), so invitations/stickies use the default
`AnonRateThrottle` (a no-op for authenticated callers, `HTTP_CONTRACT.md:1460-1470`) and emit no
headers. Rust currently forces `ApiKeyRateThrottle` on **both** families (`plane-drf/src/policy.rs:784`)
and copies the headers for both (`dispatch.rs:268-276`) — a live divergence once 340–349 register. P0.
Throttles run after permissions (`py:rest_framework/views.py:404-421`): 401/403 spends no budget.

**Exception mapping.** The five family bodies are already ported (`plane-drf/src/policy.rs:234-262`;
`HTTP_CONTRACT.md:1280-1305`): `IntegrityError` → 400 invalid-payload, Django `ValidationError` → 400
invalid-detail, `ObjectDoesNotExist` → 404 (wording differs: `BaseAPIView` "requested resource" vs
`BaseViewSet` "required object"), `KeyError` → 400 missing-key, else 500 family-server-error
(`plane/api/views/base.py:72-110,183-243`). Plane's handler turns
`NotAuthenticated` into **401** and rewrites `Throttled` as
`{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}` 429
(`adapter/exception.py:20-31`; `plane-core/src/error.rs:230-270`); an invalid key is
`AuthenticationFailed` → 403 `{"detail":"Given API token is not valid"}` (`COVERAGE.md:285`). One v1-only
rule the app lacks: a POST/PUT/PATCH body that parses to a scalar is a 400
`{"detail":"Request body must be a JSON object or array."}` before the handler
(`plane/api/views/base.py:35,59-62,171-174`; `plane-drf/src/dispatch.rs:455-470`).

**Pagination.** Every paginated v1 list uses `BasePaginator.paginate` + `OffsetPaginator`
(`plane/utils/paginator.py:635-755,94-180`): cursor `"<per_page>:<page>:<is_prev>"`, default/max 1000,
slice `[page*limit : +limit+1]`, `total_pages = ceil(total/limit)` (0 when empty), envelope
`grouped_by, sub_grouped_by, total_count, next_cursor, prev_cursor, next_page_results,
prev_page_results, count, total_pages, total_results, extra_stats, results` (`extra_stats` null).
`?per_page=`, a non-numeric `per_page` and a bad cursor are DRF 400s (`paginator.py:642-693`). Rust
has the paginator, `PageQuery` and `OffsetWindow` (`plane-query/src/paginator.rs:182-340`) but the
envelope is private (`:1041-1073`) — P0 exposes it. v1 never passes `order_by` (no key: the queryset's
order survives); only the issue list passes `total_count_queryset` (`issue.py:508-513`), so a
restricted guest's `total_count` counts every issue. Plain arrays: project members
(`member.py:129-149`), workspace members (`member.py:77-99`), invitations list (`invite.py:55-58`),
`users/me`, api-roots, search (`issue.py:2502`), relations (grouped object), the `external_id`
single-object answer, and `?cycle_view=current` (`cycle.py:203-212`).

**Serializers, `fields=`, `expand=`.** v1's base is `plane.api.serializers.base.BaseSerializer`
(`serializers/base.py:16-141`): `fields=` **removes** fields (`:35-77`); `expand=` only **replaces an
existing field**, never injects (`:79-141`); the mapper has 14 names (`:98-113`); a field not in it
becomes `getattr(instance, "<name>_id", None)` (`:139`), so `?expand=member` is a UUID and
`?expand=name` is `null`; arity follows `isinstance(related, models.Manager)` (`:121-139`). The app's
mapper injects (`app/serializers/base.py:30-190`) and Rust implements only that
(`plane-serializers/src/expand.rs`); `FilterMode::RemoveFields` exists (`ser.rs:37-38`,
`expand.rs:245-247`) but returns `MissingExpansion` instead of the `_id` fallback — P0. 42 v1
serializers are generated (`SERIALIZERS.md:17,121-179`, `generated/api_*.rs`); three are manual:
`IssueSerializer` (27 fields; `assignees`/`labels` as UUID lists or nested under `expand`,
`serializers/issue.py:323-353`; `is_draft` read-only on update, `:87-92`; `exclude=["description_json",
"description_stripped"]`, `:70-85`), `ModuleSerializer` (28, members list, `module.py:203-206`) and
`IssueRelationResponseSerializer` (8, handler-built). `ProjectSerializer` alone has 44 keys
(`serializers/project.py:210-240`). `StickySerializer` is
`fields="__all__"` with only `workspace`/`owner` read-only, so `deleted_at`/`created_by`/`updated_by`
are writable (`serializers/sticky.py:13-17`); `StateSerializer.save` clears other `default` states in a
transaction and rejects `group="triage"` (`serializers/state.py:22-37`). Only handlers that pass
`fields`/`expand` honour them (`issue.py:1103-1110` does not; `state.py:217-221`,
`project.py:206,526` do).

**`external_id` / `external_source`.** Creates reject a duplicate pair with
`409 {"error": "<Model> with the same external id and external source already exists", "id": <row id>}`
(`state.py:95-117`, `cycle.py:314-336`, `module.py:207-229`,
`issue.py:1004-1024,552-574,1563-1583,2081-2107`); PATCHes repeat the check but report the **URL
object's** id, not the conflicting row (`state.py:292-308`, `cycle.py:575-591`,
`issue.py:856-872,1733-1745`). The asset POST body is `{"message":…, "asset_id":…, "asset_url":…}`
(`asset.py:575-592`). A list GET with both parameters returns a **single object**, not a page
(`issue.py:406-421`). The documented `PUT` upsert (`issue.py:698-816`) is unreachable — no v1 URL binds
PUT (`urls/work_item.py:40-45,79-84`) — so it is a 405 and `TASKS.md`'s call sites 8–9 and
`webhook_task` 4–5 are dead.

**v1-only behaviour to port as-is.** `cycle_view=current` is a plain array while every other value
paginates (`cycle.py:203-212`). `ProjectSummary`'s `fields=` is the set of counts to compute, not a
field mask, iterated as a Python `set`, so `counts` key order follows the hash seed
(`project.py:708-746`; the scenario pins `PYTHONHASHSEED=0`, `scenarios/read/v1.yaml:17-21`).
Attachment detail GET is a **302** to a presigned S3 URL (`issue.py:2336-2344`; `COVERAGE.md:297,409`).
Project create is 201 from the annotated serializer (`project.py:277-308`); state create is **200**
(`state.py:120`); module and cycle-issue bulk adds are **200** (`module.py:788-791`,
`cycle.py:1070-1071`).
`IssueSearchEndpoint` slices `[: int(limit)]`, so `?limit=x` is a 500 (`issue.py:2455,2500`), and
`workspace_search=false` is string equality (`:2489`).

## 3. Shared pieces (P0)

One merge before any set. P0 owns no v1 handler. Files it owns: the new crate, `plane-drf`,
`plane-serializers`, `plane-query`'s envelope, the inventory generator and the scenario/fixture files.

**a. Crate + registration.** New `crates/plane-http-public` (`Cargo.toml`, `src/lib.rs` with
`register_all` + `common/`), depending on `plane-http-app` (`write::*`, `common::*`, `AppState`) plus
`plane-auth`/`plane-db`/`plane-drf`/`plane-jobs`/`plane-query`/`plane-serializers`/`plane-storage`; add
it to `bins/plane-api/Cargo.toml` and call it after `plane_http_app::register_all`
(`startup.rs:55`). `plane-serializers/src/v1.rs` gets the v1 expansion map (Set D appends `issue`
there) and the `_id` fallback. `common/` gets the plain-page envelope wrapper (make
`plane_query::paginator::envelope` public at `paginator.rs:1041`), `page(default_per_page, …)`, and the
`BaseSerializer`-mode render helper (`ViewCtx::fields/expand` already exist, `view.rs:274-283`).

**b. `plane-drf` fixes (shared; land first).** (1) `policy.rs:784`: keep `ApiViewSet`'s recorded
`AnonRateThrottle` (the §2 divergence). (2) `dispatch.rs:268-276`: copy `X-RateLimit-*` only for
`Family::ApiApiView`. (3) API-key auth must use the primary pool for the token SELECT and the
`last_used` UPDATE (`auth.rs:183-209` borrows `DispatchConn`, whose `read_pool` follows the route,
`dispatch.rs:227`, `plane-db/src/pool.rs:257-262`); test `use_read_replica=true` with a replica. All
three also affect `/api/`, so they can merge alone.

**c. Serializer generation (one codegen run).** Add to `WRITE_SERIALIZERS`
(`tools/inventory/serializers.py:1940-1980`): `project.ProjectCreateSerializer`, `ProjectUpdateSerializer`,
`state.StateSerializer`, `issue.LabelCreateUpdateSerializer`, `cycle.CycleCreateSerializer`,
`CycleUpdateSerializer`, `module.ModuleCreateSerializer`, `ModuleUpdateSerializer`,
`issue.IssueCommentCreateSerializer`, `IssueLinkCreateSerializer`, `IssueLinkUpdateSerializer`,
`intake.IntakeIssueUpdateSerializer`, `member.ProjectMemberSerializer`,
`invite.WorkspaceInviteSerializer`, `sticky.StickySerializer`; regenerate
`crates/plane-http-app/src/write/generated`. The two manual **read** serializers are set-owned:
`api_module::ModuleSerializer` in C and `api_issue::IssueSerializer` in D (§4). The table cannot express:
`ProjectCreateSerializer.create`'s random `logo_props` — v1 makes the field read-only, so **every**
v1 project create is random
(`serializers/project.py:152-160`); `ModuleCreateSerializer.create`'s duplicate-name
`ValidationError({"id","code","error","message"})` raised from `save()` (`serializers/module.py:88-100`);
`StateSerializer.save`'s default-clearing `UPDATE` (`serializers/state.py:28-33`); and
`ProjectSerializer.validate`'s `UnboundLocalError` on a non-dict `description_html`
(`serializers/project.py:272-281`, dead unless a write path uses it). Port the v1
`IssueSerializer.validate` lxml pass (`serializers/issue.py:102-109`: `html.fromstring` +
`tostring(encoding="unicode")`) — no Rust equivalent exists; empty string is a 400
`non_field_errors: ["Invalid HTML passed"]`. Or prove nh3 alone is byte-identical on a corpus.

**d. Fixture, scenario, contract.** `plane_fix_tpl` already mints 10 API tokens with the shapes
`normalize.yaml` expects (`tools/fixtures/build.py:2744-2796`) and already has
`cycle_view`/`intake_view` false, `guest_view_all_features`, archived gamma/delta, an archived cycle and
module, and `external_id` issues (`build.py:636-641,646-671,1035,1140,1297`). Add:
`projects.logo_props` volatile rule (scoped to `POST /api/v1/workspaces/*/projects/`), a
`FileAsset` with `is_uploaded=false`, an intake-less project with `intake_view=true`, and
cycle/module rows carrying `external_id`. Extend `scenarios/read/v1.yaml` (241 steps, 65 GET rows,
`COVERAGE.md:42`) with `?fields=`, `?expand=`, `external_id`, every `cycle_view`, `include_archived`,
`order_by=priority`, `per_page=2`/cursor, `search`, `query`, the format routes, and one write scenario
per set. The 264 contract tests in 27 test files (`apps/api/plane/tests/contract/api/`, 29 with
support files) run with `tools/contract/run.sh --assume-rust <orders> --methods POST,PUT,PATCH,DELETE`;
asset tests that mock
`S3Storage` are auto-served by Django (`pytest_plane_contract.py:369-377,438-480`) and cannot prove
Rust.

**e. Determinism.** Add the `projects.logo_props` rule; §5 lists the rest (`PYTHONHASHSEED`,
`last_used`, `X-RateLimit-Reset`, presigned URLs). P0 ~2 days.

## 4. Split

All sets depend only on P0, not on each other. `lib.rs`/`common/` are P0-owned. Each set adds its own
`src/<domain>.rs` + `_queries.rs`, its scenario, and flips its own `ROUTES.md` cells.

| set | routes | owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | — | §3: `crates/plane-http-public`, the three `plane-drf` fixes, `plane-serializers` v1 map + write tables, `plane-query` envelope, fixture/scenario/contract scaffolding | ~2 d |
| **A — workspace/project administration** | 292–298, 307–311, 314, 340–345, 346–351 (25) | `projects.rs`, `members.rs`, `invitations.rs`, `stickies.rs` (thin over `plane_http_app::stickies`), `users.rs`, `roots.rs`; `scenarios/write/v1-admin.yaml` | ~3 d |
| **B — assets, labels, states, intake** | 273–278, 288–291, 312–313 (12) | `assets.rs`, `labels.rs`, `states.rs`, `intake.rs`; `scenarios/write/v1-assets-labels-states-intake.yaml` | ~2 d |
| **C — cycles and modules** | 279–287, 299–306 (17) | `cycles.rs`, `modules.rs` (annotations, archive, transfer, bulk issue moves), manual `api_module::ModuleSerializer`; `scenarios/write/v1-cycles-modules.yaml` | ~2.5 d |
| **D — work items and sub-resources** | 315–326, 327–339 (25) | `issues.rs`, `issue_subresources.rs`, `search.rs`; appends `issue` to `plane-serializers/src/v1.rs`, owns `api_issue::IssueSerializer`; `scenarios/write/v1-work-items.yaml` | ~4 d |

Flip only the methods a clean scenario covers, rebuild that scenario's Django baseline, then run
`tools/proxy/gen_caddy.py --check`.

## 5. Risks and verification

**Contract coverage (written, Django-green).** 264 tests in 27 test files. Strong:
`test_external_api_guest_issue_visibility.py` (113), `test_api_write_roles.py` (24),
`test_api_permission_parity.py` (10), `test_cycles.py` and `test_projects.py` (14 each),
`test_labels.py` (10), `test_generic_asset*.py` (18), and smaller files for issues, audit fields,
project archive, members/projects lite, state admin scope, modules/cycles-lite, intake update,
attachment size, non-object bodies, authentication, read-only fields, notifications,
assignee/label validation, cycle-issue scope, link/relation scope and roster scope. **Gaps**:
invitations create/update/delete, stickies, workspace-member list, project summary, cycle
transfer/archive, module archive, attachment detail GET/PATCH/DELETE, search, relation GET, user
assets — scenarios are the only evidence. Port the suite unchanged; add gaps as scenarios.

**Scenarios.** `scenarios/read/v1.yaml` is the read baseline (241 steps, 65 GET rows).
`scenarios/v1-token-log.yaml` proves `APITokenLogMiddleware` (valid key denied/allowed, invalid key)
byte-for-byte, including `api_tokens.last_used` and the `process_logs` payload.
`scenarios/policy-probe.yaml` deliberately excludes v1 (`ROUTES.md:55`), so v1 policy (401/403/405/406/
415/429) is uncovered — P0 adds a v1 policy pass.

**Port-as-is Django bugs and traps.**
1. `POST /invitations/` **always 400s** `{"error":"The payload is not valid"}`: no `token` field, the
   column is NOT NULL with no default and `create` does not set it (`invite.py:89-94`,
   `serializers/invite.py:23-39`, `plane/db/models/workspace.py:238`); no row is written. Only
   `partial_update` refuses `email`; the inherited `PUT` does not (`invite.py:112-124`), and a PUT that
   keeps the email is a 400 `Email already invited` because `validate` counts the object itself
   (`serializers/invite.py:53-59`).
2. `ProjectCreateSerializer.create` never writes `project_identifiers` (`serializers/project.py:143-163`;
   the app copy does, `plane/app/serializers/project.py:108`). A duplicate **name** hits the partial
   unique index and `except IntegrityError` sees no `"already exists"` in the Postgres message → **500**
   `{"error":"An unexpected error occurred"}` (`project.py:310-326`). A duplicate **identifier** is a
   409 `{"identifier":"The project identifier is already taken"}` (`:329-333`), and
   `ProjectDetail.patch`'s `except ValidationError` returns that body for a foreign
   `default_state`/`estimate` too (`:610-614`).
3. `PUT` on issue detail is dead (above); the published upsert contract does not exist over HTTP.
4. `CycleDetail.patch` narrows `request_data` for a completed cycle then passes `request.data` to the
   serializer (`cycle.py:559-573`), so "only sort_order" is unenforced; its timezone override reads
   `context["project"]`, which the view never sets (`serializers/cycle.py:20-28`, `cycle.py:310-312`).
5. `IntakeIssueDetail.patch` uses `role > 15`, so a MEMBER (15) silently cannot edit intake attributes
   (`intake.py:408`); `intake_id=intake.id` is an `AttributeError` 500 when `intake_view` is on but no
   `Intake` row exists (`:340`); `request.data.pop("issue")` mutates the body (`:359,409`); the nested
   `IssueSerializer` has no `description_json`, so `description`/`description_json` edits are dropped
   (`serializers/issue.py:85`).
6. `ModuleIssueListCreate.post` tests `str(module_issue.issue_id) in issues` against a **UUID
   queryset** (`module.py:728-737`), so the match is always false: an issue already in another module
   gets a **second** `module_issues` row (the partial unique index is on `(issue, module)`, so the
   INSERT does not conflict) instead of being moved, and `updated_module_issues` is always empty. The
   cycle analogue works because its `issues` is the JSON string list (`cycle.py:986-993`).
7. State DELETE refuses if **any** issue references the state, archived/draft included
   (`state.py:251-257`); issue DELETE allows creator or **project** admin only (`issue.py:921-933`);
   cycle/module delete allow creator or project admin (`cycle.py:624-628`, `module.py:548-560`).
8. `WorkspaceMemberAPIEndpoint` is admin-only, 400s for a missing workspace (`member.py:39-99`) and
   includes inactive members (`:90`); project-member lists include inactive users (`:143`); the member
   detail's HEAD/OPTIONS are admin-only (`PROGRESS.md:575`).
9. Members of a soft-deleted workspace pass every v1 permission (`PROGRESS.md:567`); the 1xx lookups
   join `workspace__slug` only, so they answer 200/403 where app routes 404.
10. Asset/attachment deletes write `is_deleted`/`deleted_at` by hand (`asset.py:233-245`,
    `issue.py:2202-2289`) and `FileAsset.objects` does not filter `is_deleted`. Re-check the `S3Storage`
    missing-`super().__init__` 500 (`PROGRESS.md:699-701,1224-1226`) on every v1 upload path; v1
    attachment **GET** already works (302).

**Nondeterminism / harness.** Project-create `logo_props`; `ProjectSummary` set order and the
cycle/module bulk-add orders (`cycle.py:993-995` iterates `set(issues)`; `module.py:728` iterates an
unordered `values_list`) — pin `PYTHONHASHSEED` and keep one issue per add step where possible;
`last_used`, `X-RateLimit-Reset` and every `timezone.now()`; presigned URLs are signature-verified
(D17); throttle histories are float timestamps already handled (`normalize.rs:74`); `process_logs`'s
`kwargsrepr` embeds the response body, so a v1 route whose body is not compared needs
`probe_compare_celery` (see `scenarios/v1-token-log.yaml`). The 60/min bucket is why the read scenario
spreads four jars per route over 10 tokens — keep that pattern.
