# Public, instances, health/web and OpenAPI — port plan (read-only)

Area: `plane/space/` (25 rows), `plane/license/` (14), `plane/web/` (2) and the drf-spectacular
document. All 41 rows are `django | not-started` (`ROUTES.md:409-437,481-505`). Already in place and
**not** re-planned here: the `space`/`license` runtime families (`crates/plane-drf/src/policy.rs:163-166`;
space `InitialExtra::DeployBoardAnchor` at `:228`, `dispatch.rs:472-477,745-753`), every permission class
including `InstanceAdminPermission` (`plane-permissions/src/classes.rs:175-176,319`) and the four
`get_permissions` overrides these routes use (`plane-permissions/src/dynamic.rs:123-132,187-202`), the DB
models (`plane-db/src/models/{deploy_board,intake,issue,asset}.rs`, `models/license/instance.rs:129`), all
serializers (`plane-serializers/src/generated/{space_*,license_*,app_project,app_issue,app_intake}.rs`), the
write hooks (`hooks/mod.rs:16-32`; H14/H15 `hooks/issue_comment.rs:37,76`, H16 `hooks/intake_issue.rs:91`,
H30/H31 `hooks/user.rs`, `hooks/issue.rs:create_issue_row`, `hooks/state.rs:before_save`), the Celery
constructors (`plane-jobs/src/celery/tasks.rs:276,1007`) and the admin cookie machinery
(`bins/plane-api/src/middleware/session.rs:14,269-289`).

## 1. Inventory

`order` = `routes.json`; `reuse` names only pieces that exist today.

### 1.1 `public` (`/api/public/`, 25 rows / 42 method pairs)

| order | path (below the prefix) | methods | view | reuse | set |
| ---: | --- | --- | --- | --- | --- |
| 234 | `anchor/{anchor}/intakes/{intake_id}/intake-issues/` | GET list, POST create | `IntakeIssuePublicViewSet` | `app_intake::IssueStateIntakeSerializer`, `hooks::issue::create_issue_row` | B |
| 235 | `anchor/{anchor}/intakes/{intake_id}/inbox-issues/` | GET, POST | same view | same (one handler, two patterns, `space/urls/intake.py:15-24`) | B |
| 236 | `anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/` | GET, PATCH, DELETE | same | `issues::write_issue` (patch), `hooks::soft_delete` | B |
| 237 | `workspaces/{slug}/project-boards/` | GET | `WorkspaceProjectDeployBoardEndpoint` | — (500, see §5) | A |
| 238 | `anchor/{anchor}/issues/{issue_id}/` | GET | `IssueRetrievePublicEndpoint` | `plane-query` `ValueField` projection | A |
| 239 | `anchor/{anchor}/issues/{issue_id}/comments/` | GET, POST | `IssueCommentPublicViewSet` | `app_issue::IssueCommentSerializer`, H14/H15 | A (GET) / B |
| 240 | `anchor/{anchor}/issues/{issue_id}/comments/{pk}/` | GET, PATCH, DELETE | same | H14/H15, H03 | A (GET) / B |
| 241 | `anchor/{anchor}/issues/{issue_id}/reactions/` | GET, POST | `IssueReactionPublicViewSet` | `app_issue::IssueReactionSerializer` | A (GET) / B |
| 242 | `anchor/{anchor}/issues/{issue_id}/reactions/{reaction_code}/` | DELETE | same | H03 | B |
| 243 | `anchor/{anchor}/comments/{comment_id}/reactions/` | GET, POST | `CommentReactionPublicViewSet` | `app_issue::CommentReactionSerializer` | A (GET) / B |
| 244 | `anchor/{anchor}/comments/{comment_id}/reactions/{reaction_code}/` | DELETE | same | H03 | B |
| 245 | `anchor/{anchor}/issues/{issue_id}/votes/` | GET, POST, DELETE | `IssueVotePublicViewSet` | `app_issue::IssueVoteSerializer` | A (GET) / B |
| 246 | `anchor/{anchor}/meta/` | GET | `ProjectMetaDataEndpoint` | `space_project::ProjectLiteSerializer` | A |
| 247 | `anchor/{anchor}/settings/` | GET | `ProjectDeployBoardPublicSettingsEndpoint` | `app_project::DeployBoardSerializer` | A |
| 248 | `anchor/{anchor}/issues/` | GET | `ProjectIssuesPublicEndpoint` | `plane_query::issue` filter/order + `paginator`, space grouper (new) | A |
| 249 | `workspaces/{slug}/projects/{project_id}/anchor/` | GET | `WorkspaceProjectAnchorEndpoint` | `app_project::DeployBoardSerializer` | A |
| 250 | `anchor/{anchor}/cycles/` | GET | `ProjectCyclesEndpoint` | `db::Cycle` `.values(id,name)` | A |
| 251 | `anchor/{anchor}/modules/` | GET | `ProjectModulesEndpoint` | `db::Module` `.values` | A |
| 252 | `anchor/{anchor}/states/` | GET | `ProjectStatesEndpoint` | `db::State` `.values` | A |
| 253 | `anchor/{anchor}/labels/` | GET | `ProjectLabelsEndpoint` | `db::Label` `.values` | A |
| 254 | `anchor/{anchor}/members/` | GET | `ProjectMembersEndpoint` | `db::ProjectMember` `.values` | A |
| 255 | `assets/v2/anchor/{anchor}/` | GET, POST, PATCH, DELETE | `EntityAssetEndpoint` | `assets::support` (`presign_upload_body`, `sanitize_filename`) | A (GET) / B |
| 256 | `assets/v2/anchor/{anchor}/{pk}/` | GET, POST, PATCH, DELETE | same | `assets::support::presigned_redirect` | A (GET) / B |
| 257 | `assets/v2/anchor/{anchor}/restore/{pk}/` | POST | `AssetRestoreEndpoint` | `all_objects` lookup | B |
| 258 | `assets/v2/anchor/{anchor}/{entity_id}/bulk/` | POST | `EntityBulkAssetEndpoint` | — | B |

### 1.2 `instances` (`/api/instances/`, 14 rows / 21 method pairs)

| order | path (below the prefix) | methods | view | reuse | set |
| ---: | --- | --- | --- | --- | --- |
| 259 | `` | GET, PATCH | `InstanceEndpoint` (family `app.BaseAPIView`) | `plane_auth::instance_config`, `common::cache` | C |
| 260 | `admins/` | GET, POST, DELETE | `InstanceAdminEndpoint` | `license_api_admin::InstanceAdminSerializer` | C |
| 261 | `admins/me/` | GET | `InstanceAdminUserMeEndpoint` | `license_api_admin::InstanceAdminMeSerializer` | C |
| 262 | `admins/session/` | GET | `InstanceAdminUserSessionEndpoint` | same | C |
| 263 | `admins/sign-out/` | POST | `InstanceAdminSignOutEndpoint` (plain `django.View`) | `plane_auth::login::logout`, `middleware/session.rs` | C |
| 264 | `admins/{pk}/` | GET, POST, DELETE | `InstanceAdminEndpoint` | same as 260; GET/POST are the 500s | C |
| 265 | `configurations/` | GET, PATCH | `InstanceConfigurationEndpoint` | manual serializer (new), `plane_auth::fernet` | C |
| 266 | `configurations/disable-email-feature/` | DELETE | `DisableEmailFeatureEndpoint` | — | C |
| 267 | `admins/sign-in/` | POST | `InstanceAdminSignInEndpoint` (plain) | `auth::support` + `plane_auth::login::user_login` | C |
| 268 | `admins/sign-up/` | POST | `InstanceAdminSignUpEndpoint` (plain) | same + `auth_flow::adapter`'s user INSERT | C |
| 269 | `admins/sign-up-screen-visited/` | POST | `SignUpScreenVisitedEndpoint` | `common::cache` | C |
| 270 | `email-credentials-check/` | POST | `EmailCredentialCheckEndpoint` | mail transport (new) | C |
| 271 | `workspace-slug-check/` | GET | `InstanceWorkSpaceAvailabilityCheckEndpoint` | `workspaces::base::RESTRICTED_WORKSPACE_SLUGS` | C |
| 272 | `workspaces/` | GET, POST | `InstanceWorkSpaceEndpoint` | `plane_query::paginator::offset_page`, `workspaces::write` | C |

### 1.3 `health` / `web`

| order | path | methods | view | reuse | set |
| ---: | --- | --- | --- | --- | --- |
| 389 | `/robots.txt` | all 8 | `plane.web.views.robots_txt` | **already Rust** (`bins/plane-api/src/startup.rs:94-117`), `scenarios/read/web.yaml:24-30` | none — Owner stays `django` |
| 390 | `/` | all 8 | `plane.web.views.health_check` | **already Rust**, `scenarios/read/health.yaml:24-30`; also the Rust target's health probe (`scenarios/targets.yaml:62`) | none — Owner stays `django` |

These two **must not be flipped**. `Caddyfile.ce:14-23` never sends `/` or `/robots.txt` to `api:8000`,
and `tools/proxy/gen_caddy.py:613-626` refuses an owner cell for any path outside `/api/*`, `/auth/*`,
`/static/*` (`is_api_path` `:211-222`). Their `health`/`web` groups stay un-emitted
(`deploy/caddy/rust-routes.caddy:71,98`); set Status to `verified`, leave Owner `django`.

## 2. Shared pieces (P0)

**a. Board resolution and scope (blocks A and B).** New `crates/plane-http-app/src/public/board.rs` with
the three lookups the views use — `.filter(anchor, entity_name="project").first()`
(`space/views/issue.py:113`), `.get(anchor, entity_name="project")` (`intake.py:58`, `meta.py:21`,
`project.py:23`) and `.get(anchor)` with no entity filter (`issue.py:690`, `asset.py:48`) — plus the write
predicates `_issue_in_board_scope` (`issue.py:76-87`, `IssueManager` at `db/models/issue.py:92-101`),
`_comment_in_board_scope` (`:90-97`) and `_is_own_board_comment` (`asset.py:27-35`). The
`DeployBoardAnchorMixin` 404 (`space/views/base.py:50-59`) is already the runtime's and runs **after**
`check_permissions` (`dispatch.rs:460-478`), so an anonymous request on an IsAuthenticated row is a 401
before the anchor is read.

**b. `encrypt_data`** (`plane/license/utils/encryption.py:19-30`). `fernet.rs` has `derive_key` and
`decrypt_data` only (`crates/plane-auth/src/fernet.rs:50,105-128`); add `encrypt_data` (random IV +
`time.time()`, so the token differs on every call) and round-trip it against the existing vector
(`fernet.rs:151-180`).

**c. Mail (blocks 270 only).** No transport exists and no decision records one; the brief names `lettre`
(`RUST_REWRITE.md:119`). Port `get_email_configuration` (`license/utils/instance_value.py:42-59`,
reachable as `plane_auth::instance_config::configuration_values`), Django's
`get_connection`+`EmailMultiAlternatives` semantics and the six SMTP exception branches plus the generic
one (`configuration.py:130-171`); `int(EMAIL_PORT)` is a 500 on a non-numeric value (`:110`, `ENV.md:242`)
and `EMAIL_FROM=""` becomes `webmaster@localhost` (`ENV.md:245`). Add a `DECISIONS.md` entry and a fake
SMTP listener under `tools/` modelled on `tools/auth/fake_provider.py`.

**d. `instance_configurations` bulk update (blocks 265/266).** `bulk_update(values, ["value"])`
(`configuration.py:44-56`) writes only `value` — no `updated_at`, no audit — and never inserts. The
response is rendered from the same in-memory rows (`:58`), so the decrypted plaintext is deterministic;
the stored ciphertext is not (§5).

**e. `InstanceConfigurationSerializer`** (`SERIALIZERS.md:4239-4256`) is the one serializer here with no
generated struct (`codegen: manual`, because `to_representation` decrypts at `:15-19`). Everything else is
generated, including the space copies — note `space/views/intake.py:20-25` imports the **app**
`IssueStateIntakeSerializer` (36 fields, no `bridge_id`), not the space one (37 fields,
`SERIALIZERS.md:214,321`).

**f. Normalisation.** `scenarios/normalize.yaml`: a `fernet_token` shape plus
`instance_configurations.value` → `placeholder_if_new` scoped to `PATCH /api/instances/configurations/`
(the machinery exists, `normalize.yaml:567-690`, `plane-testkit/src/normalize.rs:1172-1228`). No testkit
code change.

**g. OpenAPI artifact and handlers** (§4): `deploy/openapi/v1.{yaml,json}`,
`tools/openapi/{capture,check}.py`, `crates/plane-http-app/src/openapi/` and the `schema` route group.
Needed only for the Phase 7 exit and Phase 8, so it can merge after A/B.

**h. No new hook, permission, publisher or model.** A and B call `hooks::*`, `tasks::issue_activity`
(every public call site — `plane/space/views/issue.py:312,356,385,451,481,548,584,653,673` and
`intake.py:169,237` — passes only the seven base kwargs, i.e. `IssueActivityShape::Bare`,
`plane-jobs/src/celery/tasks.rs:44-85,96-99`), `write::publish_soft_delete_cascade` and `common::cache` as
they are.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0** | — | §2a,b,d,e,f,h: `public/board.rs`; `fernet::encrypt_data`; `normalize.yaml`; `instances/serializer.rs` | ~1 d |
| **P0b** | — | §2c,g: `lettre` wiring, fake SMTP, `deploy/openapi/*`, `tools/openapi/*`, `openapi/` handlers, `schema` group + inventory regeneration | ~1 d |
| **A — public reads** | 234-236 GET, 237-254 GET, 255/256 GET | `public/{mod,queries,board_read,issue_query,meta,project,lists,assets_read}.rs`; re-run `scenarios/read/public.yaml` | ~1.5 d |
| **B — public writes** | 234-236 write verbs, 239-245 write verbs, 255-258 | `public/{comments_write,reactions_write,votes_write,intake_write,assets_write}.rs` + queries; `scenarios/write/public-*.yaml` | ~2.5 d |
| **C — instances** | 259-272 | `instances/{mod,queries,instance,admin,auth,configuration,email,workspace}.rs`; `scenarios/write/instances-*.yaml` | ~2.5 d |

A and B share only P0; B does not touch `board_read.rs` and A never writes. C is independent of A/B
except P0b. `ROUTES.md`/`routes.json` flips stay with the orchestrator (as in Phase 5/6,
`HANDLERS.md:19,434`); each set's scenario is the evidence.

## 4. OpenAPI

**What exists.** drf-spectacular 0.29.0 (`apps/api/pyproject.toml:79`) is **off by default**:
`ENABLE_DRF_SPECTACULAR` is `"0"` (`plane/settings/common.py:602`), and only then are `drf_spectacular`
installed, `DEFAULT_SCHEMA_CLASS` set and `SPECTACULAR_SETTINGS` imported (`:604-607`). With the flag on,
`plane/urls.py:27-40` adds `/api/schema/`, `/api/schema/swagger-ui/` and `/api/schema/redoc/`. The
document covers **only** `/api/v1/`: the preprocessing hook drops every path not under `/api/v1/`, drops
`PUT` and drops any path containing `server` (`utils/openapi/hooks.py:14-24`), and `SCHEMA_PATH_PREFIX` is
`/api/v1/` (`settings/openapi.py:35`). `SpectacularAPIView` renders YAML by default, then
`application/vnd.oai.openapi+json`, `application/yaml`, `application/json`
(`drf_spectacular/views.py:45-49`), sets `Content-Disposition: inline; filename="The Plane REST API.yaml"`
(`:100-104`), and is anonymous (`SERVE_PUBLIC=True`, `drf_spectacular/settings.py:55`) with itself
excluded (`SERVE_INCLUDE_SCHEMA=False`, `settings/openapi.py:34`). The two HTML pages are the stock
templates with CDN assets (`drf_spectacular/templates/drf_spectacular/{swagger_ui,redoc}.html`) and a
relative `schema_url`. `routes.json:7` records `enable_drf_spectacular: false`, which is why the three
routes are absent from `ROUTES.md` (`:25,81`).

**Measured, not assumed.** `manage.py spectacular --file` against a dead `DATABASE_URL` (no DB access)
produces 545002 bytes, **byte-identical** under `PYTHONHASHSEED=0` and `12345`: 69 paths, 130 operations
(59 get / 27 post / 20 patch / 24 delete), 94 component schemas, servers `http://localhost:8000` +
`https://api.plane.so`, 4 schema errors (1 unique — `ProjectSummaryAPIEndpoint` is skipped,
`plane/api/views/project.py:720`) and 33 unique `operationId` collisions resolved with numeric suffixes.
All of that is baked into the bytes.

**What the port must produce.** (1) Capture both renderings once from Django at the pinned commit, as
`deploy/openapi/v1.yaml` and `deploy/openapi/v1.json` (`?format=json`), plus the two rendered HTML pages.
(2) Serve the three routes from `plane-http-app/src/openapi/` with `ViewResponse::raw` +
`with_header("Content-Disposition", …)` (`plane-drf/src/view.rs:296-409`), gated on the same env flag
(when it is off the handlers answer Django's 404, because the URL does not exist — `plane/urls.py:27`).
(3) Regenerate `routes.json`+`ROUTES.md` with `ENABLE_DRF_SPECTACULAR=1` (`ROUTES.md:10-19`) and add a
`schema` group with `/api/schema/**`, because the Caddy matcher needs a route row
(`tools/proxy/gen_caddy.py:381-420`) and owner/status cells are preserved (`ROUTES.md:3`); this is an
orchestrator step.

**How to verify "still accurate".** (a) `tools/openapi/check.py --check` regenerates from Django and
compares with the checked-in golden — it fails on any upstream serializer/view change (run it in
`server-ci.yml`'s Django job, which already has the venv). (b) Serve the golden from Rust and byte-compare
status, `Content-Type`, `Content-Disposition` and body in a differential scenario; the document is
DB-independent, so no fixture is needed. (c) Assert the document's path+method set equals the Rust v1
route table (`routes.json`, group `v1`, minus `PUT` and `*server*`) — the mechanical statement of "the docs
describe what we serve". (d) Keep the v1 contract suite (`plane/tests/contract/api/`) green — "what we
serve is what the docs say".

## 5. Risks and verification

- **Django bugs to port as-is.** #237 is a **double** bug: the URL binds `slug` but the handler takes
  `anchor` (`space/urls/intake.py:31` vs `space/views/project.py:31`) → `TypeError` → 500 family body;
  and `:32` stores `DeployBoard.objects.filter(...).values_list` (the method, not a queryset) before
  `deploy_board.workspace` at `:34`. Recorded as `500×3` (`scenarios/read/COVERAGE.md:212`). #255 GET
  needs `pk` (`asset.py:46`) but the collection URL supplies none → 500 (`COVERAGE.md:228`). #264 GET and
  POST are 500s (`admin.py:73,51` take no `pk`, `license/urls.py:37` passes one; `COVERAGE.md:120`).
  #241/#245 GET always return `[]`: `get_queryset` reads `self.kwargs["slug"]/["project_id"]`
  (`issue.py:405-406`) and `["anchor"]` as a workspace slug (`:609-610`), none of which these URLs supply
  — only the POST/DELETE halves are meaningful (`COVERAGE.md:216,218`). #238 uses
  `DeployBoard.objects.get(anchor=…)` with no `entity_name` (`:690`) and its `reaction_items` `avatar_url`
  reads `votes__actor__avatar_asset` instead of `issue_reactions__actor__avatar_asset` (`:805,813`). 252
  filters `~Q(name="Triage")`, not `is_triage` (`state.py:27`).
- **Anonymous/published checks.** GETs are AllowAny on 237-238, 246-254 and the comment GETs; 239-245
  writes, 234-236 and 255-258 are IsAuthenticated → anonymous is **401**, because `NotAuthenticated` is
  remapped (`authentication/adapter/exception.py:22-24`, `settings/common.py:149`), and the comment view
  picks permissions per action (`issue.py:253-259`). The board is the whole authorization: `is_disabled`
  and `Project.network` are never read. `DeployBoardAnchorMixin` answers `{"detail": "Not found."}` for a
  board whose `entity_identifier != project_id`; no fixture row does this today (see the fixture bullet).
- **Public write scoping.** Every write re-resolves the board and binds the body's object ids to its
  project (`issue.py:76-97`, `intake.py:116-122`); the public comment body is reduced to
  `comment_html`/`comment_json` (`issue.py:100-103`) and `access="EXTERNAL"`/`actor` are forced, so a
  non-dict body is a 500 (`data[key]` on a list). Comment PATCH/DELETE require `actor=request.user`
  (`:343-350,377-384`) and do **not** set `edited_at` (the app path does, `issues/write_comment.rs:1-50`).
  Duplicate reactions are an uncaught `IntegrityError` → 400 `{"error": "The payload is not valid"}`
  (`space/views/base.py:91-95`). Votes: `get_or_create`, then `vote=data.get("vote",1)`, then a **full**
  `save()` (`issue.py:637-663`, so `updated_at`/`updated_by` change), and `destroy` does **not** check
  `is_votes_enabled` (`:665-683`). Intakes: create publishes `issue.activity.created` **before** the
  `intake_issues` row and answers **200**, not 201 (`intake.py:169-187`); `partial_update` pops `issue`
  off `request.data` (`:211`) and a missing key is a 500 (`False.get`); `destroy` soft-deletes (`:293`).
  Assets: POST clamps the client size into `[1, FILE_SIZE_LIMIT]` and stores the clamped value in both
  `attributes["size"]` and `size` (`asset.py:115-161`), binds `entity_identifier` only when it is the
  caller's own EXTERNAL comment (`:127-129`), and DELETE/restore/bulk are `created_by`-scoped 404s, not
  403s (`:184-301`). GET 255/256 is a 302 to a presigned URL with `disposition="inline"` pinned to an
  allow-listed image type (`:74-85`).
- **Admin sessions.** `/api/instances/**` reads `admin-session-id` because the middleware tests
  `"instances" in request.path` (`authentication/middleware/session.py:22-27`) — a plain `session-id`
  cookie is *anonymous* on these routes, so every scenario must send the admin cookie. The age is
  `ADMIN_SESSION_COOKIE_AGE=3600` (`settings/common.py:431-432`), written by `user_login(is_admin=True)`
  (`authentication/utils/login.py:13-27`; Rust `plane-auth/src/login.rs:194-244`, `session.rs:760`).
  Sign-in/up are plain views: CSRF applies, `request.POST` only (`support::post_params`,
  `auth/support.rs:217-236`), the throttle runs first (`rate_limit.py:62-95` → `support::throttle_redirect`),
  and success redirects to `urljoin(base_host(is_admin=True), "general/")` (`admin.py:266,405`).
  `is_telemetry_enabled` is sent as `"True"`/`"False"` (`apps/admin/components/instance/setup-form.tsx:160`);
  lowercase `"false"` is a `BooleanField` `ValidationError` in a plain view → HTML 500. Sign-up locks the
  `Instance` row and re-checks (`admin.py:219-235`), creates the user (which fires H31's notification
  preference) + `Profile`, then runs one more `save()` (H30 **rotates `users.token`** because
  `token_updated_at` was set at `:252`) and creates `InstanceAdmin`+updates `Instance` (`:237-261`).
  Sign-in's gates are ordered bot → inactive → password → membership (`:345-393`) and its final
  `user.save()` rotates the token too. Sign-out is idempotent via `except` around
  `User.objects.get(pk=request.user.id)` (`:433-446`).
- **Encrypted config.** `PATCH /configurations/` re-encrypts with a random IV + timestamp, so the stored
  ciphertext differs every run (§2f); the response body decrypts and is deterministic.
  `disable-email-feature` is deterministic: five rows to `""` (including the encrypted
  `EMAIL_HOST_PASSWORD`, whose ciphertext becomes `""`) and `ENABLE_SMTP` to `"0"`
  (`configuration.py:68-79`, `ENV.md:863-866`). `GET /instances/` reads 13 keys through
  `get_configuration_value` and five `settings.*` values, then `str(GITHUB_APP_NAME)`,
  `float(FILE_SIZE_LIMIT)` (emit `5242880.0`), `bool(...)` and `Workspace.objects.count() >= 1`
  (`instance.py:49-158`); `primary_owner_details` is always absent (no such attribute,
  `license/api/serializers/instance.py:12`), `data = serializer.data` at `:46-48` is dead, and `deleted_at`
  is writable (`SERIALIZERS.md:4266`).
- **Caching traps.** `@cache_response(7200, user=False)` keys on `get_full_path()` (`utils/cache.py:25-51`)
  and runs **before** `@cache_control(private=True, max_age=12)` (`instance.py:34-35`), so a cache hit
  loses the `Cache-Control` header. `@invalidate_cache` deletes **before** the body runs
  (`cache.py:72-79`), and POST/DELETE `/admins/` invalidate `/api/instances/`, never
  `/api/instances/admins/` (`admin.py:49,72`) — the admin list can be stale for two hours. Port as-is.
- **Instance workspace routes.** 271 compares `slug__iexact` in the DB but
  `slug in RESTRICTED_WORKSPACE_SLUGS` case-**sensitively** (`workspace.py:31`; reuse
  `workspaces/base.rs:38-45`). 272 GET paginates with `max_per_page=10` (`workspace.py:63-69`; reuse
  `plane_query::paginator::offset_page`); POST runs `is_valid(raise_exception=True)` (so the
  `serializer.errors` branch at `:100-103` is dead) and only *then* creates the member row, with a
  duplicate slug → 409 bare-string body or an implicit `return None` → 500 (`:105-110`). The license
  `WorkspaceSerializer` validates a URL-free alphanumeric name and a case-insensitive unique slug with
  different messages (`license/api/serializers/workspace.py:23-44`); the DRF `UniqueValidator` runs first,
  so an exact duplicate is "Workspace with this slug already exists." and a case variant is "Slug is
  already in use".
- **Contract coverage today is thin.** Public: `contract/app/test_spaces_board_object_scope_app.py:282-661`
  (board scoping, internal-comment leak, writable fields, activities), plus
  `test_space_asset_comment_scope_app.py`, `test_space_asset_mutation_scope_app.py`,
  `test_static_asset_disposition_app.py` and `test_deploy_board_identity_readonly_app.py`. Instances: only
  `test_authentication.py:824-838` (throttle) and `:926-976` (bot admin sign-in). Nothing covers 237-254,
  234-236, 255-258 or any 259-272 success body; the differential scenarios are the evidence (run
  `tools/contract/run.sh --assume-rust 234-272` for the ported rows).
- **Fixture needs** (`plane_fix_tpl` already has alpha (all flags on, intake, EXTERNAL comments, votes,
  reactions) and beta (all flags off): `tools/fixtures/build.py:2943-2999,1668-1745,1784-1878,3002-3017`).
  Add by `fixture_sql`: a board with `entity_identifier != project_id` and one with `entity_name="page"`;
  a soft-deleted comment/reaction/vote; a `project_public_members` row; an `intake_issues` row authored by
  another user; a `users` row with `token_updated_at IS NULL`; a bot and an inactive user for the admin
  gates; and, for 267/268, a step that clears `instance_admins` + `is_setup_done` so sign-up's success path
  is reachable.
- **Nondeterminism to normalise.** Fernet ciphertext (§2f); `profiles.background_color`
  (`normalize.yaml:631-635`); `users.{username,password,token}` and `users.token_updated_at`/last_login on
  sign-in/out (`:599-614`); `sessions.*` (`:567-580`); the admin `Set-Cookie` (`:377-396`); ids/timestamps
  the harness already masks. Everything else here is `timezone.now()` (UTC, µs) or derived from fixture
  rows. `settings.DEBUG` is `0` (`settings/common.py:51`), so the `cache_response` write always happens —
  the Rust `debug` flag must agree or the Redis diff differs.
- **Verification.** `./target/debug/difftest run --scenario scenarios/read/public.yaml` (A),
  `instances.yaml` (C's GETs) and `health.yaml`/`web.yaml` (already green; re-run only if `bins/plane-api`
  changes), then the new write scenarios (B, C) with the DB diff, the Celery message and the Redis snapshot
  compared; then `tools/contract/run.sh --assume-rust 234-272`; then `cargo fmt/clippy/test` and
  `cargo test -p plane-http-app --test schema_check` in `apps/server`.
