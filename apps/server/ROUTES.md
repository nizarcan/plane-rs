# Route inventory

Generated from Django's URL resolver by `apps/server/tools/inventory/routes.py`. Do not edit anything except the **Owner** and **Status** columns; regeneration preserves those two values for every route it already knows, keyed on `(group, path, view)`, and overwrites everything else. Status starts at `not-started`; use values such as `in-progress`, `verified` or `blocked` by hand.

Owner cells use one of three forms. `django` keeps every method of the row on Django. `rust` moves every method of the row to Rust, including the implicit `HEAD` Django answers for a `GET` and the `OPTIONS` DRF answers for any DRF view. `rust(GET,HEAD)` moves only the listed methods; the other methods of the same path stay on Django. Method names are uppercase and must be methods the row answers. Anything else is rejected by this generator and by `apps/server/tools/proxy/gen_caddy.py`, which turns these cells into `apps/server/deploy/caddy/rust-routes.caddy`: one named matcher and one `handle` block per route group, gated by `PLANE_RS_GROUP_<NAME>` (`rust` by default, `django` to send the group back).

Regenerate:

```sh
cd apps/api
DJANGO_SETTINGS_MODULE=plane.settings.production \
SECRET_KEY=inventory \
DATABASE_URL=postgresql://x:x@127.0.0.1:1/x \
REDIS_URL=redis://127.0.0.1:1/0 \
ENABLE_DRF_SPECTACULAR=1 \
.venv/bin/python ../server/tools/inventory/routes.py
```

Generated from commit `26c84a701e2cb05cc2dfbb2f02927a85d93454f6` with Django 5.2.17 and DRF 3.18.3 (`ENABLE_DRF_SPECTACULAR=1`). `routes.json` is the machine-readable form of the same data.

`order` is the 0-based index in Django's first-match order. `methods` is the ViewSet action map for a ViewSet, the implemented handlers for an `APIView`/`GenericAPIView`, and the methods the class or decorators accept for a Django `View` or function view. `head` and `options` are omitted unless explicitly implemented; `drf_options` records that DRF answers `OPTIONS` on every DRF view even when it is not listed. A `[405]` after a method means the method is registered but `BaseViewSet.initial` refuses it at runtime — see hazard (g).

`permissions` shows the class-level permission classes (short names) and, after `;`, the per-method `allow_permission` decorator arguments as `METHODS allow(ROLES;LEVEL;creator=Model)`. `(dynamic)` means the class overrides `get_permissions`; `routes.json` also carries `dynamic_authenticators` and `dynamic_throttles` with the owning class. A `-` in the role list is an empty `allowed_roles`; only the `creator` branch can allow such a request. Every DRF view is `csrf_exempt` at Django's middleware layer (`routes.json` records that flag), but DRF's `SessionAuthentication` still enforces CSRF for session-authenticated requests. Plain `django.views.View` and function entries have no DRF security classes: they run under Django's `AuthenticationMiddleware` and inline checks, and a `permission_classes` attribute declared on such a class is dead code (flagged in the route's `notes`).

With `ENABLE_DRF_SPECTACULAR=1` Django adds `/api/schema/`, `/api/schema/swagger-ui/` and `/api/schema/redoc/`; this file is generated from the default configuration unless the regeneration command sets the flag. With `DEBUG=1` and `debug_toolbar` installed, `__debug__/` routes are prepended; the production settings used here default to `DEBUG=0`, so they are absent.

## Phase 2 policy fields (`plane-drf`)

`routes.json` also records what `plane-drf` needs to run each route exactly. These fields are generated from the view class, never typed by hand:

| Field | Meaning |
| --- | --- |
| `base_family` | which base class the view derives from: `app.BaseViewSet`, `app.BaseAPIView`, `api.BaseViewSet`, `api.BaseAPIView`, `space.BaseViewSet`, `space.BaseAPIView`, `license.BaseAPIView`, `drf.ViewSet`, `drf.APIView`, `django.View` or `function` |
| `use_read_replica` | the class/function attribute the read replica router reads (`null` when it has none) |
| `timezone_mixin` | the MRO contains a `TimezoneMixin` |
| `http_method_names` | the effective list, in Django's order |
| `renderer_classes`, `parser_classes`, `content_negotiation_class`, `metadata_class` | the DRF classes Django resolves through `api_settings` |
| `view_name`, `view_description` | `get_view_name()` / `get_view_description()` on an instance built with the route's `as_view` initkwargs (so a ViewSet's `List`/`Instance` suffix is included) |
| `lifecycle_overrides` | the hooks (`get_permissions`, `get_authenticators`, `get_throttles`, `initial`, `dispatch`, `handle_exception`, `finalize_response`, `options`) a concrete view overrides below its family base class, with the owning class; a non-null value without an implementation is a startup error |
| `defined_methods` | for a plain `django.views.View`, the method handlers the class defines |
| `guard_actions` | per ViewSet action: the implementing class and whether it is a DRF mixin method, so the runtime re-evaluates `BaseViewSet.initial`'s 405 rule with the permissions `get_permissions` actually resolved |
| `options_metadata` | `attempts` (`{'PUT','POST'} & allowed_methods`), `actions_json` (the serializer field map per attempt as compact `json.dumps` text, order preserved), `error` (the exception `determine_actions` raised) and `get_object_required` |

### OPTIONS metadata gaps

`actions_json` is recorded for a **permitted** caller, with `check_permissions` and `get_object()` bypassed so no database is needed. At request time `plane-drf` re-runs the permission check for the attempt's method and splices the recorded text into the body. Two gaps follow:

* a `PUT` action also requires `view.get_object()` to succeed on Django; the runtime cannot run a view's `get_object()` generically, so a route with `get_object_required: true` may include `PUT` actions where Django omits them (the differential probe's OPTIONS steps avoid those routes);
* CPython iterates `{'PUT','POST'}` in hash order, which is randomised per process, so the key order of Django's `actions` object is not stable either; the runtime writes `POST` before `PUT` and the recorded text follows that order.

### Policy probe coverage

`scenarios/policy-probe.yaml` (generated by `tools/policy/gen_policy_scenario.py`) runs every route method outside `/api/v1/` through the Rust runtime for the fixture users. Four gaps are deliberate:

* **`/api/v1/`** cannot be registered until Phase 4's Celery publisher exists: `APITokenLogMiddleware` would enqueue `plane.bgtasks.logger_task.process_logs`. The policy for those routes is generated and loaded, and the registry refuses their registration with an explicit error.
* **Write steps are omitted** for views whose handler re-implements an authorization check with the same body `allow_permission` returns (`{"error": "You don't have the required permissions."}`; `asset/v2.py`, `asset/base.py`, `workspace/draft.py`, `project/base.py`). A 403 there cannot be attributed to the policy, so the probe does not weaken its rule; those 46 write method pairs keep the `django` owner until Phase 3 ports their handlers.
* **`DELETE /api/users/me/`** deactivates the caller and their memberships, and **`DELETE /api/workspaces/{slug}/`** deletes the workspace. They run only with callers that cannot succeed plus one admin against a disposable object, so a permitted delete cannot change a later step.
* **No `HEAD` steps**: the harness cannot decompress a `HEAD` response that carries `Content-Encoding: gzip` and an empty body. `HEAD` falls back to the `GET` handler (`View.setup`), which the `GET` steps exercise.

## Summary

| Group | Prefix | Endpoints | Brief | Difference | Endpoint × method |
| --- | --- | ---: | ---: | ---: | ---: |
| app | /api/ | 234 | 234 | 0 | 441 |
| v1 | /api/v1/ | 79 | 75 | +4 | 151 |
| public | /api/public/ | 25 | 25 | 0 | 42 |
| auth | /auth/ | 37 | 37 | 0 | 37 |
| instances | /api/instances/ | 14 | 14 | 0 | 21 |
| web | (root) | 2 | — | — | 16 |
| schema | /api/schema/ | 3 | — | — | 3 |
| **total** | | **394** | **385** | **+9** | **711** |

The brief's counts are from `RUST_REWRITE.md` ("What exists today").
They were produced by counting `path()` calls, which is not the same as counting resolver entries. Differences:

- **app**: 234 `path()` calls, 234 resolver entries. No difference.
- **v1**: 79 resolver entries = 67 plain `path()` routes + 12 entries generated by the two `include(router.urls)` expansions (list/detail, format-suffix variants and two api-roots per router). The brief's 75 is the number of `path()` calls in `plane/api/urls/*.py`; that count includes 3 paths in `plane/api/urls/schema.py` and 3 in `plane/api/urls/estimate.py` which `plane/api/urls/__init__.py` never imports, and counts each router include as one entry. Mounted `path()` calls: 69; 69 − 2 + 12 = 79.
- **public**: 25 resolver entries, 25 in the brief. No difference.
- **auth**: 37 resolver entries, 37 in the brief. No difference.
- **instances**: 14 resolver entries, 14 in the brief. No difference.
- **web**: 2 resolver entries (`robots.txt` and the health check at `/`). The brief's table does not list `plane.web`.
- **schema**: present only because `ENABLE_DRF_SPECTACULAR=1`; the brief's table does not list these routes. With the default (`0`) they do not exist.

`Endpoint × method` counts one pair per HTTP method each resolver entry registers (ViewSet actions, implemented APIView handlers, and the methods a Django `View` or function view accepts). It is the number of method/path pairs the Rust router must account for, including pairs that Django answers with 405 because of the `BaseViewSet` guard (hazard (g)).


## app (`/api/`)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 0 | `/api/workspaces/{slug}/analytics/` | GET | `plane.app.views.analytic.base.AnalyticsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;WORKSPACE) | rust(GET) | verified |
| 1 | `/api/workspaces/{slug}/analytic-view/` | GET:list, POST:create | `plane.app.views.analytic.base.AnalyticViewViewset` | WorkSpaceAdminPermission | rust(GET,POST) | verified |
| 2 | `/api/workspaces/{slug}/analytic-view/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.analytic.base.AnalyticViewViewset` | WorkSpaceAdminPermission | rust(GET,PATCH,DELETE) | verified |
| 3 | `/api/workspaces/{slug}/saved-analytic-view/{analytic_id}/` | GET | `plane.app.views.analytic.base.SavedAnalyticEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;WORKSPACE) | rust(GET) | verified |
| 4 | `/api/workspaces/{slug}/export-analytics/` | POST | `plane.app.views.analytic.base.ExportAnalyticsEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;WORKSPACE) | rust(POST) | verified |
| 5 | `/api/workspaces/{slug}/default-analytics/` | GET | `plane.app.views.analytic.base.DefaultAnalyticsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 6 | `/api/workspaces/{slug}/project-stats/` | GET | `plane.app.views.analytic.base.ProjectStatsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 7 | `/api/workspaces/{slug}/advance-analytics/` | GET | `plane.app.views.analytic.advance.AdvanceAnalyticsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;WORKSPACE) | rust(GET) | verified |
| 8 | `/api/workspaces/{slug}/advance-analytics-stats/` | GET | `plane.app.views.analytic.advance.AdvanceAnalyticsStatsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;WORKSPACE) | rust(GET) | verified |
| 9 | `/api/workspaces/{slug}/advance-analytics-charts/` | GET | `plane.app.views.analytic.advance.AdvanceAnalyticsChartEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;WORKSPACE) | rust(GET) | verified |
| 10 | `/api/workspaces/{slug}/projects/{project_id}/advance-analytics/` | GET | `plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;PROJECT) | rust(GET) | verified |
| 11 | `/api/workspaces/{slug}/projects/{project_id}/advance-analytics-stats/` | GET | `plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsStatsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;PROJECT) | rust(GET) | verified |
| 12 | `/api/workspaces/{slug}/projects/{project_id}/advance-analytics-charts/` | GET | `plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsChartEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 13 | `/api/workspaces/{slug}/file-assets/` | GET, POST, DELETE | `plane.app.views.asset.base.FileAssetEndpoint` | IsAuthenticated+WorkspaceMemberPermission | rust(GET,POST,DELETE) | verified |
| 14 | `/api/workspaces/file-assets/{workspace_id}/{asset_key}/` | GET, DELETE | `plane.app.views.asset.base.FileAssetEndpoint` | IsAuthenticated+WorkspaceMemberPermission | rust(GET,DELETE) | verified |
| 15 | `/api/users/file-assets/` | GET, POST, DELETE | `plane.app.views.asset.base.UserAssetsEndpoint` | IsAuthenticated | rust(GET,POST,DELETE) | verified |
| 16 | `/api/users/file-assets/{asset_key}/` | GET, POST, DELETE | `plane.app.views.asset.base.UserAssetsEndpoint` | IsAuthenticated | rust(GET,POST,DELETE) | verified |
| 17 | `/api/workspaces/file-assets/{workspace_id}/{asset_key}/restore/` | POST:restore | `plane.app.views.asset.base.FileAssetViewSet` | IsAuthenticated+WorkspaceMemberPermission | rust(POST) | verified |
| 18 | `/api/assets/v2/workspaces/{slug}/` | GET, POST, PATCH, DELETE | `plane.app.views.asset.v2.WorkspaceFileAssetEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST,PATCH,DELETE) | verified |
| 19 | `/api/assets/v2/workspaces/{slug}/{asset_id}/` | GET, POST, PATCH, DELETE | `plane.app.views.asset.v2.WorkspaceFileAssetEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST,PATCH,DELETE) | verified |
| 20 | `/api/assets/v2/user-assets/` | POST, PATCH, DELETE | `plane.app.views.asset.v2.UserAssetsV2Endpoint` | IsAuthenticated | rust(POST,PATCH,DELETE) | verified |
| 21 | `/api/assets/v2/user-assets/{asset_id}/` | POST, PATCH, DELETE | `plane.app.views.asset.v2.UserAssetsV2Endpoint` | IsAuthenticated | rust(POST,PATCH,DELETE) | verified |
| 22 | `/api/assets/v2/workspaces/{slug}/restore/{asset_id}/` | POST | `plane.app.views.asset.v2.AssetRestoreEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(POST) | verified |
| 23 | `/api/assets/v2/static/{asset_id}/` | GET | `plane.app.views.asset.v2.StaticFileAssetEndpoint` | AllowAny | rust(GET) | verified |
| 24 | `/api/assets/v2/workspaces/{slug}/projects/{project_id}/` | GET, POST, PATCH, DELETE | `plane.app.views.asset.v2.ProjectAssetEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST,PATCH,DELETE) | verified |
| 25 | `/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/` | GET, POST, PATCH, DELETE | `plane.app.views.asset.v2.ProjectAssetEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST,PATCH,DELETE) | verified |
| 26 | `/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/` | POST | `plane.app.views.asset.v2.ProjectBulkAssetEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(POST) | verified |
| 27 | `/api/assets/v2/workspaces/{slug}/check/{asset_id}/` | GET | `plane.app.views.asset.v2.AssetCheckEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 28 | `/api/assets/v2/workspaces/{slug}/duplicate-assets/{asset_id}/` | POST | `plane.app.views.asset.v2.DuplicateAssetEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(POST) | verified |
| 29 | `/api/assets/v2/workspaces/{slug}/download/{asset_id}/` | GET | `plane.app.views.asset.v2.WorkspaceAssetDownloadEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 30 | `/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/` | GET | `plane.app.views.asset.v2.ProjectAssetDownloadEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 31 | `/api/workspaces/{slug}/projects/{project_id}/cycles/` | GET:list, POST:create | `plane.app.views.cycle.base.CycleViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); POST allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 32 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.cycle.base.CycleViewSet` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER;PROJECT); DELETE allow(ADMIN;PROJECT;creator=Cycle) | rust(GET,PUT,PATCH,DELETE) | verified |
| 33 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/` | GET:list, POST:create | `plane.app.views.cycle.issue.CycleIssueViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 34 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/` | GET:retrieve [405], PUT:update [405], PATCH:partial_update [405], DELETE:destroy | `plane.app.views.cycle.issue.CycleIssueViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,PUT,PATCH,DELETE) | verified |
| 35 | `/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/` | POST | `plane.app.views.cycle.base.CycleDateCheckEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 36 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/` | GET:list [405], POST:create | `plane.app.views.cycle.base.CycleFavoriteViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 37 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/{cycle_id}/` | DELETE:destroy | `plane.app.views.cycle.base.CycleFavoriteViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER;PROJECT) | rust(DELETE) | verified |
| 38 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/` | POST | `plane.app.views.cycle.base.TransferCycleIssueEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 39 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/user-properties/` | GET, PATCH | `plane.app.views.cycle.base.CycleUserPropertiesEndpoint` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,PATCH) | verified |
| 40 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/` | GET, POST, DELETE | `plane.app.views.cycle.archive.CycleArchiveUnarchiveEndpoint` | IsAuthenticated; GET,POST,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST,DELETE) | verified |
| 41 | `/api/workspaces/{slug}/projects/{project_id}/archived-cycles/` | GET, POST, DELETE | `plane.app.views.cycle.archive.CycleArchiveUnarchiveEndpoint` | IsAuthenticated; GET,POST,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST,DELETE) | verified |
| 42 | `/api/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/` | GET, POST, DELETE | `plane.app.views.cycle.archive.CycleArchiveUnarchiveEndpoint` | IsAuthenticated; GET,POST,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST,DELETE) | verified |
| 43 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/progress/` | GET | `plane.app.views.cycle.base.CycleProgressEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 44 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/analytics/` | GET | `plane.app.views.cycle.base.CycleAnalyticsEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 45 | `/api/workspaces/{slug}/projects/{project_id}/project-estimates/` | GET | `plane.app.views.estimate.base.ProjectEstimatePointEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;PROJECT) | rust(GET) | verified |
| 46 | `/api/workspaces/{slug}/projects/{project_id}/estimates/` | GET:list, POST:create | `plane.app.views.estimate.base.BulkEstimatePointEndpoint` | ProjectEntityPermission | rust(GET,POST) | verified |
| 47 | `/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.estimate.base.BulkEstimatePointEndpoint` | ProjectEntityPermission | rust(GET,PATCH,DELETE) | verified |
| 48 | `/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/` | POST:create | `plane.app.views.estimate.base.EstimatePointEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 49 | `/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/{estimate_point_id}/` | PATCH:partial_update, DELETE:destroy | `plane.app.views.estimate.base.EstimatePointEndpoint` | IsAuthenticated; PATCH,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(PATCH,DELETE) | verified |
| 50 | `/api/unsplash/` | GET | `plane.app.views.external.base.UnsplashEndpoint` | IsAuthenticated | rust(GET) | verified |
| 51 | `/api/workspaces/{slug}/projects/{project_id}/ai-assistant/` | POST | `plane.app.views.external.base.GPTIntegrationEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 52 | `/api/workspaces/{slug}/ai-assistant/` | POST | `plane.app.views.external.base.WorkspaceGPTIntegrationEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;WORKSPACE) | rust(POST) | verified |
| 53 | `/api/workspaces/{slug}/projects/{project_id}/intakes/` | GET:list, POST:create | `plane.app.views.intake.base.IntakeViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 54 | `/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/` | GET:retrieve [405], PATCH:partial_update [405], DELETE:destroy | `plane.app.views.intake.base.IntakeViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,PATCH,DELETE) | verified |
| 55 | `/api/workspaces/{slug}/projects/{project_id}/intake-issues/` | GET:list, POST:create | `plane.app.views.intake.base.IntakeIssueViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST) | verified |
| 56 | `/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.intake.base.IntakeIssueViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT;creator=Issue); PATCH,DELETE allow(ADMIN;PROJECT;creator=Issue) | rust(GET,PATCH,DELETE) | verified |
| 57 | `/api/workspaces/{slug}/projects/{project_id}/inboxes/` | GET:list, POST:create | `plane.app.views.intake.base.IntakeViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 58 | `/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/` | GET:retrieve [405], PATCH:partial_update [405], DELETE:destroy | `plane.app.views.intake.base.IntakeViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,PATCH,DELETE) | verified |
| 59 | `/api/workspaces/{slug}/projects/{project_id}/inbox-issues/` | GET:list, POST:create | `plane.app.views.intake.base.IntakeIssueViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST) | verified |
| 60 | `/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.intake.base.IntakeIssueViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT;creator=Issue); PATCH,DELETE allow(ADMIN;PROJECT;creator=Issue) | rust(GET,PATCH,DELETE) | verified |
| 61 | `/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/` | GET | `plane.app.views.intake.base.IntakeWorkItemDescriptionVersionEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 62 | `/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/{pk}/` | GET | `plane.app.views.intake.base.IntakeWorkItemDescriptionVersionEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 63 | `/api/workspaces/{slug}/projects/{project_id}/issues/list/` | GET | `plane.app.views.issue.base.IssueListEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 64 | `/api/workspaces/{slug}/projects/{project_id}/issues/` | GET:list, POST:create | `plane.app.views.issue.base.IssueViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); POST allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 65 | `/api/workspaces/{slug}/projects/{project_id}/issues-detail/` | GET | `plane.app.views.issue.base.IssueDetailEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 66 | `/api/workspaces/{slug}/projects/{project_id}/v2/issues/` | GET:list | `plane.app.views.issue.base.IssuePaginatedViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 67 | `/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.issue.base.IssueViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT;creator=Issue); PATCH allow(ADMIN,MEMBER;PROJECT;creator=Issue); DELETE allow(ADMIN;PROJECT;creator=Issue) | rust(GET,PUT,PATCH,DELETE) | verified |
| 68 | `/api/workspaces/{slug}/projects/{project_id}/issue-labels/` | GET:list, POST:create | `plane.app.views.issue.label.LabelViewSet` | ProjectBasePermission; POST allow(ADMIN;PROJECT) | rust(GET,POST) | verified |
| 69 | `/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.app.views.issue.label.LabelViewSet` | ProjectBasePermission; PATCH,DELETE allow(ADMIN;PROJECT) | rust(GET,PUT,PATCH,DELETE) | verified |
| 70 | `/api/workspaces/{slug}/projects/{project_id}/bulk-create-labels/` | POST | `plane.app.views.issue.label.BulkCreateIssueLabelsEndpoint` | IsAuthenticated; POST allow(ADMIN;PROJECT) | rust(POST) | verified |
| 71 | `/api/workspaces/{slug}/projects/{project_id}/bulk-delete-issues/` | DELETE | `plane.app.views.issue.base.BulkDeleteIssuesEndpoint` | IsAuthenticated; DELETE allow(ADMIN;PROJECT) | rust(DELETE) | verified |
| 72 | `/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/` | POST | `plane.app.views.issue.archive.BulkArchiveIssuesEndpoint` | ProjectEntityPermission; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 73 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/` | GET, POST | `plane.app.views.issue.sub_issue.SubIssuesEndpoint` | ProjectEntityPermission | rust(GET,POST) | verified |
| 74 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/` | GET:list, POST:create | `plane.app.views.issue.link.IssueLinkViewSet` | ProjectEntityPermission | rust(GET,POST) | verified |
| 75 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.app.views.issue.link.IssueLinkViewSet` | ProjectEntityPermission | rust(GET,PUT,PATCH,DELETE) | verified |
| 76 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/` | GET, POST, DELETE | `plane.app.views.issue.attachment.IssueAttachmentEndpoint` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT); DELETE allow(ADMIN;PROJECT;creator=FileAsset) | rust(GET,POST,DELETE) | verified |
| 77 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/` | GET, POST, DELETE | `plane.app.views.issue.attachment.IssueAttachmentEndpoint` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT); DELETE allow(ADMIN;PROJECT;creator=FileAsset) | rust(GET,POST,DELETE) | verified |
| 78 | `/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/` | GET, POST, PATCH, DELETE | `plane.app.views.issue.attachment.IssueAttachmentV2Endpoint` | IsAuthenticated; GET,POST,PATCH allow(ADMIN,MEMBER,GUEST;PROJECT); DELETE allow(ADMIN;PROJECT;creator=FileAsset) | rust(GET,POST,PATCH,DELETE) | verified |
| 79 | `/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/` | GET, POST, PATCH, DELETE | `plane.app.views.issue.attachment.IssueAttachmentV2Endpoint` | IsAuthenticated; GET,POST,PATCH allow(ADMIN,MEMBER,GUEST;PROJECT); DELETE allow(ADMIN;PROJECT;creator=FileAsset) | rust(GET,POST,PATCH,DELETE) | verified |
| 80 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/history/` | GET | `plane.app.views.issue.activity.IssueActivityEndpoint` | ProjectEntityPermission; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 81 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/` | GET:list, POST:create | `plane.app.views.issue.comment.IssueCommentViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST) | verified |
| 82 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.issue.comment.IssueCommentViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); PATCH,DELETE allow(ADMIN;PROJECT;creator=IssueComment) | rust(GET,PUT,PATCH,DELETE) | verified |
| 83 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/` | GET:list, POST:create | `plane.app.views.issue.subscriber.IssueSubscriberViewSet` | ProjectEntityPermission (dynamic) | rust(GET,POST) | verified |
| 84 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/{subscriber_id}/` | DELETE:destroy | `plane.app.views.issue.subscriber.IssueSubscriberViewSet` | ProjectEntityPermission (dynamic) | rust(DELETE) | verified |
| 85 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/` | GET:subscription_status, POST:subscribe, DELETE:unsubscribe | `plane.app.views.issue.subscriber.IssueSubscriberViewSet` | ProjectEntityPermission (dynamic) | rust(GET,POST,DELETE) | verified |
| 86 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/` | GET:list, POST:create | `plane.app.views.issue.reaction.IssueReactionViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST) | verified |
| 87 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/{reaction_code}/` | DELETE:destroy | `plane.app.views.issue.reaction.IssueReactionViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(DELETE) | verified |
| 88 | `/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/` | GET:list, POST:create | `plane.app.views.issue.comment.CommentReactionViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST) | verified |
| 89 | `/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/{reaction_code}/` | DELETE:destroy | `plane.app.views.issue.comment.CommentReactionViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(DELETE) | verified |
| 90 | `/api/workspaces/{slug}/projects/{project_id}/user-properties/` | GET, PATCH | `plane.app.views.issue.base.ProjectUserDisplayPropertyEndpoint` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,PATCH) | verified |
| 91 | `/api/workspaces/{slug}/projects/{project_id}/archived-issues/` | GET:list | `plane.app.views.issue.archive.IssueArchiveViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER;PROJECT) | rust(GET) | verified |
| 92 | `/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/` | GET:retrieve, POST:archive, DELETE:unarchive | `plane.app.views.issue.archive.IssueArchiveViewSet` | IsAuthenticated; GET,POST,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST,DELETE) | verified |
| 93 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/` | GET:list, POST:create | `plane.app.views.issue.relation.IssueRelationViewSet` | ProjectEntityPermission | rust(GET,POST) | verified |
| 94 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/remove-relation/` | POST:remove_relation | `plane.app.views.issue.relation.IssueRelationViewSet` | ProjectEntityPermission | rust(POST) | verified |
| 95 | `/api/workspaces/{slug}/projects/{project_id}/deleted-issues/` | GET | `plane.app.views.issue.base.DeletedIssuesListViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 96 | `/api/workspaces/{slug}/projects/{project_id}/issue-dates/` | POST | `plane.app.views.issue.base.IssueBulkUpdateDateEndpoint` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 97 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/` | GET | `plane.app.views.issue.version.IssueVersionEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 98 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/` | GET | `plane.app.views.issue.version.IssueVersionEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 99 | `/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/` | GET | `plane.app.views.issue.version.WorkItemDescriptionVersionEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 100 | `/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/` | GET | `plane.app.views.issue.version.WorkItemDescriptionVersionEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 101 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/meta/` | GET | `plane.app.views.issue.base.IssueMetaEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 102 | `/api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/` | GET | `plane.app.views.issue.base.IssueDetailIdentifierEndpoint` | IsAuthenticated | rust(GET) | verified |
| 103 | `/api/workspaces/{slug}/projects/{project_id}/modules/` | GET:list, POST:create | `plane.app.views.module.base.ModuleViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); POST allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 104 | `/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.module.base.ModuleViewSet` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER;PROJECT); DELETE allow(ADMIN;PROJECT;creator=Module) | rust(GET,PUT,PATCH,DELETE) | verified |
| 105 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/` | POST:create_issue_modules | `plane.app.views.module.issue.ModuleIssueViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(POST) | verified |
| 106 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/` | POST:create_module_issues, GET:list | `plane.app.views.module.issue.ModuleIssueViewSet` | IsAuthenticated; POST,GET allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 107 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/` | GET:retrieve [405], PUT:update [405], PATCH:partial_update [405], DELETE:destroy | `plane.app.views.module.issue.ModuleIssueViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER;PROJECT) | rust(GET,PUT,PATCH,DELETE) | verified |
| 108 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/` | GET:list, POST:create | `plane.app.views.module.base.ModuleLinkViewSet` | ProjectEntityPermission | rust(GET,POST) | verified |
| 109 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/{pk}/` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.app.views.module.base.ModuleLinkViewSet` | ProjectEntityPermission | rust(GET,PUT,PATCH,DELETE) | verified |
| 110 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/` | GET:list, POST:create | `plane.app.views.module.base.ModuleFavoriteViewSet` | ProjectLitePermission | rust(GET,POST) | verified |
| 111 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/{module_id}/` | DELETE:destroy | `plane.app.views.module.base.ModuleFavoriteViewSet` | ProjectLitePermission | rust(DELETE) | verified |
| 112 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/user-properties/` | GET, PATCH | `plane.app.views.module.base.ModuleUserPropertiesEndpoint` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,PATCH) | verified |
| 113 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/archive/` | GET, POST, DELETE | `plane.app.views.module.archive.ModuleArchiveUnarchiveEndpoint` | ProjectEntityPermission | rust(GET,POST,DELETE) | verified |
| 114 | `/api/workspaces/{slug}/projects/{project_id}/archived-modules/` | GET, POST, DELETE | `plane.app.views.module.archive.ModuleArchiveUnarchiveEndpoint` | ProjectEntityPermission | rust(GET,POST,DELETE) | verified |
| 115 | `/api/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/` | GET, POST, DELETE | `plane.app.views.module.archive.ModuleArchiveUnarchiveEndpoint` | ProjectEntityPermission | rust(GET,POST,DELETE) | verified |
| 116 | `/api/workspaces/{slug}/users/notifications/` | GET:list | `plane.app.views.notification.base.NotificationViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 117 | `/api/workspaces/{slug}/users/notifications/{pk}/` | GET:retrieve [405], PATCH:partial_update, DELETE:destroy [405] | `plane.app.views.notification.base.NotificationViewSet` | IsAuthenticated; PATCH allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PATCH,DELETE) | verified |
| 118 | `/api/workspaces/{slug}/users/notifications/{pk}/read/` | POST:mark_read, DELETE:mark_unread | `plane.app.views.notification.base.NotificationViewSet` | IsAuthenticated; POST,DELETE allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(POST,DELETE) | verified |
| 119 | `/api/workspaces/{slug}/users/notifications/{pk}/archive/` | POST:archive, DELETE:unarchive | `plane.app.views.notification.base.NotificationViewSet` | IsAuthenticated; POST,DELETE allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(POST,DELETE) | verified |
| 120 | `/api/workspaces/{slug}/users/notifications/unread/` | GET | `plane.app.views.notification.base.UnreadNotificationEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 121 | `/api/workspaces/{slug}/users/notifications/mark-all-read/` | POST:create | `plane.app.views.notification.base.MarkAllReadNotificationViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(POST) | verified |
| 122 | `/api/users/me/notification-preferences/` | GET, PATCH | `plane.app.views.notification.base.UserNotificationPreferenceEndpoint` | IsAuthenticated | rust(GET,PATCH) | verified |
| 123 | `/api/workspaces/{slug}/projects/{project_id}/pages-summary/` | GET:summary | `plane.app.views.page.base.PageViewSet` | ProjectPagePermission | rust(GET) | verified |
| 124 | `/api/workspaces/{slug}/projects/{project_id}/pages/` | GET:list, POST:create | `plane.app.views.page.base.PageViewSet` | ProjectPagePermission | rust(GET,POST) | verified |
| 125 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.page.base.PageViewSet` | ProjectPagePermission | rust(GET,PATCH,DELETE) | verified |
| 126 | `/api/workspaces/{slug}/projects/{project_id}/favorite-pages/{page_id}/` | POST:create, DELETE:destroy | `plane.app.views.page.base.PageFavoriteViewSet` | IsAuthenticated; POST,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(POST,DELETE) | verified |
| 127 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/` | POST:archive, DELETE:unarchive | `plane.app.views.page.base.PageViewSet` | ProjectPagePermission | rust(POST,DELETE) | verified |
| 128 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/lock/` | POST:lock, DELETE:unlock | `plane.app.views.page.base.PageViewSet` | ProjectPagePermission | rust(POST,DELETE) | verified |
| 129 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/access/` | POST:access | `plane.app.views.page.base.PageViewSet` | ProjectPagePermission | rust(POST) | verified |
| 130 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/` | GET:retrieve, PATCH:partial_update | `plane.app.views.page.base.PagesDescriptionViewSet` | ProjectPagePermission | rust(GET,PATCH) | verified |
| 131 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/` | GET | `plane.app.views.page.version.PageVersionEndpoint` | ProjectPagePermission | rust(GET) | verified |
| 132 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/{pk}/` | GET | `plane.app.views.page.version.PageVersionEndpoint` | ProjectPagePermission | rust(GET) | verified |
| 133 | `/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/duplicate/` | POST | `plane.app.views.page.base.PageDuplicateEndpoint` | ProjectPagePermission | rust(POST) | verified |
| 134 | `/api/workspaces/{slug}/pages/{page_id}/live-access/` | GET | `plane.app.views.page.live.PageLiveAccessEndpoint` | IsAuthenticated | rust(GET) | verified |
| 135 | `/api/workspaces/{slug}/projects/` | GET:list, POST:create | `plane.app.views.project.base.ProjectViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE); POST allow(ADMIN,MEMBER;WORKSPACE) | rust(GET,POST) | verified |
| 136 | `/api/workspaces/{slug}/projects/details/` | GET:list_detail | `plane.app.views.project.base.ProjectViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 137 | `/api/workspaces/{slug}/projects/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.project.base.ProjectViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PUT,PATCH,DELETE) | verified |
| 138 | `/api/workspaces/{slug}/project-identifiers/` | GET, DELETE | `plane.app.views.project.base.ProjectIdentifierEndpoint` | IsAuthenticated; GET,DELETE allow(ADMIN,MEMBER;WORKSPACE) | rust(GET,DELETE) | verified |
| 139 | `/api/workspaces/{slug}/projects/{project_id}/invitations/` | GET:list, POST:create | `plane.app.views.project.invite.ProjectInvitationsViewset` | IsAuthenticated; GET,POST allow(ADMIN;PROJECT) | rust(GET,POST) | verified |
| 140 | `/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/` | GET:retrieve, DELETE:destroy | `plane.app.views.project.invite.ProjectInvitationsViewset` | IsAuthenticated; GET,DELETE allow(ADMIN;PROJECT) | rust(GET,DELETE) | verified |
| 141 | `/api/users/me/workspaces/{slug}/projects/invitations/` | GET:list [405], POST:create | `plane.app.views.project.invite.UserProjectInvitationsViewset` | IsAuthenticated; POST allow(ADMIN,MEMBER;WORKSPACE) | rust(GET,POST) | verified |
| 142 | `/api/users/me/workspaces/{slug}/project-roles/` | GET | `plane.app.views.project.member.UserProjectRolesEndpoint` | WorkspaceUserPermission | rust(GET) | verified |
| 143 | `/api/workspaces/{slug}/projects/{project_id}/join/{pk}/` | GET, POST | `plane.app.views.project.invite.ProjectJoinEndpoint` | AllowAny | rust(GET,POST) | verified |
| 144 | `/api/workspaces/{slug}/projects/{project_id}/members/` | GET:list, POST:create | `plane.app.views.project.member.ProjectMemberViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); POST allow(ADMIN;PROJECT) | rust(GET,POST) | verified |
| 145 | `/api/workspaces/{slug}/projects/{project_id}/members/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.project.member.ProjectMemberViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); PATCH,DELETE allow(ADMIN;PROJECT) | rust(GET,PATCH,DELETE) | verified |
| 146 | `/api/workspaces/{slug}/projects/{project_id}/members/leave/` | POST:leave | `plane.app.views.project.member.ProjectMemberViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(POST) | verified |
| 147 | `/api/workspaces/{slug}/projects/{project_id}/project-views/` | POST | `plane.app.views.project.base.ProjectUserViewsEndpoint` | IsAuthenticated | rust(POST) | verified |
| 148 | `/api/workspaces/{slug}/projects/{project_id}/project-members/me/` | GET | `plane.app.views.project.member.ProjectMemberUserEndpoint` | IsAuthenticated | rust(GET) | verified |
| 149 | `/api/workspaces/{slug}/user-favorite-projects/` | GET:list [405], POST:create | `plane.app.views.project.base.ProjectFavoritesViewSet` | IsAuthenticated | rust(GET,POST) | verified |
| 150 | `/api/workspaces/{slug}/user-favorite-projects/{project_id}/` | DELETE:destroy | `plane.app.views.project.base.ProjectFavoritesViewSet` | IsAuthenticated | rust(DELETE) | verified |
| 151 | `/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/` | GET:list, POST:create | `plane.app.views.project.base.DeployBoardViewSet` | ProjectMemberPermission | rust(GET,POST) | verified |
| 152 | `/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.project.base.DeployBoardViewSet` | ProjectMemberPermission | rust(GET,PATCH,DELETE) | verified |
| 153 | `/api/workspaces/{slug}/projects/{project_id}/archive/` | POST, DELETE | `plane.app.views.project.base.ProjectArchiveUnarchiveEndpoint` | IsAuthenticated; POST,DELETE allow(ADMIN,MEMBER;PROJECT) | rust(POST,DELETE) | verified |
| 154 | `/api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/` | GET, PATCH | `plane.app.views.project.member.ProjectMemberPreferenceEndpoint` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,PATCH) | verified |
| 155 | `/api/workspaces/{slug}/search/` | GET | `plane.app.views.search.base.GlobalSearchEndpoint` | IsAuthenticated | rust(GET) | verified |
| 156 | `/api/workspaces/{slug}/projects/{project_id}/search-issues/` | GET | `plane.app.views.search.issue.IssueSearchEndpoint` | IsAuthenticated | rust(GET) | verified |
| 157 | `/api/workspaces/{slug}/entity-search/` | GET | `plane.app.views.search.base.SearchEndpoint` | WorkspaceUserPermission | rust(GET) | verified |
| 158 | `/api/workspaces/{slug}/projects/{project_id}/states/` | GET:list, POST:create | `plane.app.views.state.base.StateViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); POST allow(ADMIN;PROJECT) | rust(GET,POST) | verified |
| 159 | `/api/workspaces/{slug}/projects/{project_id}/states/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.state.base.StateViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); PATCH,DELETE allow(ADMIN;PROJECT) | rust(GET,PATCH,DELETE) | verified |
| 160 | `/api/workspaces/{slug}/projects/{project_id}/intake-state/` | GET | `plane.app.views.state.base.IntakeStateEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET) | verified |
| 161 | `/api/workspaces/{slug}/projects/{project_id}/states/{pk}/mark-default/` | POST:mark_as_default | `plane.app.views.state.base.StateViewSet` | IsAuthenticated; POST allow(ADMIN;PROJECT) | rust(POST) | verified |
| 162 | `/api/users/me/` | GET:retrieve, PATCH:partial_update, DELETE:deactivate | `plane.app.views.user.base.UserEndpoint` | IsAuthenticated | rust(GET,PATCH,DELETE) | verified |
| 163 | `/api/users/session/` | GET | `plane.app.views.user.base.UserSessionEndpoint` | AllowAny | rust(GET) | verified |
| 164 | `/api/users/me/settings/` | GET:retrieve_user_settings | `plane.app.views.user.base.UserEndpoint` | IsAuthenticated | rust(GET) | verified |
| 165 | `/api/users/me/email/generate-code/` | POST:generate_email_verification_code | `plane.app.views.user.base.UserEndpoint` | IsAuthenticated | rust(POST) | verified |
| 166 | `/api/users/me/email/` | PATCH:update_email | `plane.app.views.user.base.UserEndpoint` | IsAuthenticated | rust(PATCH) | verified |
| 167 | `/api/users/me/profile/` | GET, PATCH | `plane.app.views.user.base.ProfileEndpoint` | IsAuthenticated | rust(GET,PATCH) | verified |
| 168 | `/api/users/me/accounts/` | GET, DELETE | `plane.app.views.user.base.AccountEndpoint` | IsAuthenticated | rust(GET,DELETE) | verified |
| 169 | `/api/users/me/accounts/{pk}/` | GET, DELETE | `plane.app.views.user.base.AccountEndpoint` | IsAuthenticated | rust(GET,DELETE) | verified |
| 170 | `/api/users/me/instance-admin/` | GET:retrieve_instance_admin | `plane.app.views.user.base.UserEndpoint` | IsAuthenticated | rust(GET) | verified |
| 171 | `/api/users/me/onboard/` | PATCH | `plane.app.views.user.base.UpdateUserOnBoardedEndpoint` | IsAuthenticated | rust(PATCH) | verified |
| 172 | `/api/users/me/tour-completed/` | PATCH | `plane.app.views.user.base.UpdateUserTourCompletedEndpoint` | IsAuthenticated | rust(PATCH) | verified |
| 173 | `/api/users/me/activities/` | GET | `plane.app.views.user.base.UserActivityEndpoint` | IsAuthenticated | rust(GET) | verified |
| 174 | `/api/users/me/workspaces/` | GET | `plane.app.views.workspace.base.UserWorkSpacesEndpoint` | IsAuthenticated | rust(GET) | verified |
| 175 | `/api/users/me/workspaces/{slug}/activity-graph/` | GET | `plane.app.views.workspace.user.UserActivityGraphEndpoint` | IsAuthenticated | rust(GET) | verified |
| 176 | `/api/users/me/workspaces/{slug}/issues-completed-graph/` | GET | `plane.app.views.workspace.user.UserIssueCompletedGraphEndpoint` | IsAuthenticated | rust(GET) | verified |
| 177 | `/api/users/me/workspaces/{slug}/dashboard/` | GET | `plane.app.views.workspace.base.UserWorkspaceDashboardEndpoint` | IsAuthenticated | rust(GET) | verified |
| 178 | `/api/workspaces/{slug}/projects/{project_id}/views/` | GET:list, POST:create | `plane.app.views.view.base.IssueViewViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;PROJECT) | rust(GET,POST) | verified |
| 179 | `/api/workspaces/{slug}/projects/{project_id}/views/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.view.base.IssueViewViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;PROJECT); PATCH allow(-;PROJECT;creator=IssueView); DELETE allow(ADMIN;PROJECT;creator=IssueView) | rust(GET,PUT,PATCH,DELETE) | verified |
| 180 | `/api/workspaces/{slug}/views/` | GET:list, POST:create | `plane.app.views.view.base.WorkspaceViewViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST) | verified |
| 181 | `/api/workspaces/{slug}/views/{pk}/` | GET:retrieve, PUT:update [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.view.base.WorkspaceViewViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE); PATCH allow(-;WORKSPACE;creator=IssueView); DELETE allow(ADMIN;WORKSPACE;creator=IssueView) | rust(GET,PUT,PATCH,DELETE) | verified |
| 182 | `/api/workspaces/{slug}/issues/` | GET:list | `plane.app.views.view.base.WorkspaceViewIssuesViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 183 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/` | GET:list [405], POST:create | `plane.app.views.view.base.IssueViewFavoriteViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER;PROJECT) | rust(GET,POST) | verified |
| 184 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/{view_id}/` | DELETE:destroy | `plane.app.views.view.base.IssueViewFavoriteViewSet` | IsAuthenticated; DELETE allow(ADMIN,MEMBER;PROJECT) | rust(DELETE) | verified |
| 185 | `/api/workspace-slug-check/` | GET | `plane.app.views.workspace.base.WorkSpaceAvailabilityCheckEndpoint` | IsAuthenticated | rust(GET) | verified |
| 186 | `/api/workspaces/` | GET:list, POST:create | `plane.app.views.workspace.base.WorkSpaceViewSet` | WorkSpaceBasePermission; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST) | verified |
| 187 | `/api/workspaces/{slug}/` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.app.views.workspace.base.WorkSpaceViewSet` | WorkSpaceBasePermission; PATCH,DELETE allow(ADMIN;WORKSPACE) | rust(GET,PUT,PATCH,DELETE) | verified |
| 188 | `/api/workspaces/{slug}/invitations/` | GET:list, POST:create | `plane.app.views.workspace.invite.WorkspaceInvitationsViewset` | WorkSpaceAdminPermission | rust(GET,POST) | verified |
| 189 | `/api/workspaces/{slug}/invitations/{pk}/` | DELETE:destroy, GET:retrieve, PATCH:partial_update | `plane.app.views.workspace.invite.WorkspaceInvitationsViewset` | WorkSpaceAdminPermission | rust(GET,PATCH,DELETE) | verified |
| 190 | `/api/users/me/workspaces/invitations/` | GET:list, POST:create | `plane.app.views.workspace.invite.UserWorkspaceInvitationsViewSet` | IsAuthenticated | rust(GET,POST) | verified |
| 191 | `/api/workspaces/{slug}/invitations/{pk}/join/` | GET, POST | `plane.app.views.workspace.invite.WorkspaceJoinEndpoint` | AllowAny | rust(GET,POST) | verified |
| 192 | `/api/workspaces/{slug}/members/` | GET:list | `plane.app.views.workspace.member.WorkSpaceMemberViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 193 | `/api/workspaces/{slug}/project-members/` | GET | `plane.app.views.workspace.member.WorkspaceProjectMemberEndpoint` | WorkspaceEntityPermission | rust(GET) | verified |
| 194 | `/api/workspaces/{slug}/members/{pk}/` | PATCH:partial_update, DELETE:destroy, GET:retrieve | `plane.app.views.workspace.member.WorkSpaceMemberViewSet` | IsAuthenticated; PATCH,DELETE allow(ADMIN;WORKSPACE); GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PATCH,DELETE) | verified |
| 195 | `/api/workspaces/{slug}/members/leave/` | POST:leave | `plane.app.views.workspace.member.WorkSpaceMemberViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(POST) | verified |
| 196 | `/api/users/last-visited-workspace/` | GET | `plane.app.views.workspace.user.UserLastProjectWithWorkspaceEndpoint` | IsAuthenticated | rust(GET) | verified |
| 197 | `/api/workspaces/{slug}/workspace-members/me/` | GET | `plane.app.views.workspace.member.WorkspaceMemberUserEndpoint` | IsAuthenticated | rust(GET) | verified |
| 198 | `/api/workspaces/{slug}/workspace-views/` | POST | `plane.app.views.workspace.member.WorkspaceMemberUserViewsEndpoint` | IsAuthenticated | rust(POST) | verified |
| 199 | `/api/workspaces/{slug}/workspace-themes/` | GET:list, POST:create | `plane.app.views.workspace.base.WorkspaceThemeViewSet` | WorkSpaceAdminPermission | rust(GET,POST) | verified |
| 200 | `/api/workspaces/{slug}/workspace-themes/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.workspace.base.WorkspaceThemeViewSet` | WorkSpaceAdminPermission | rust(GET,PATCH,DELETE) | verified |
| 201 | `/api/workspaces/{slug}/user-stats/{user_id}/` | GET | `plane.app.views.workspace.user.WorkspaceUserProfileStatsEndpoint` | WorkspaceViewerPermission | rust(GET) | verified |
| 202 | `/api/workspaces/{slug}/user-activity/{user_id}/` | GET | `plane.app.views.workspace.user.WorkspaceUserActivityEndpoint` | WorkspaceEntityPermission | rust(GET) | verified |
| 203 | `/api/workspaces/{slug}/user-activity/{user_id}/export/` | POST | `plane.app.views.workspace.base.ExportWorkspaceUserActivityEndpoint` | WorkspaceEntityPermission | rust(POST) | verified |
| 204 | `/api/workspaces/{slug}/user-profile/{user_id}/` | GET | `plane.app.views.workspace.user.WorkspaceUserProfileEndpoint` | IsAuthenticated | rust(GET) | verified |
| 205 | `/api/workspaces/{slug}/user-issues/{user_id}/` | GET | `plane.app.views.workspace.user.WorkspaceUserProfileIssuesEndpoint` | WorkspaceViewerPermission | rust(GET) | verified |
| 206 | `/api/workspaces/{slug}/labels/` | GET | `plane.app.views.workspace.label.WorkspaceLabelsEndpoint` | WorkspaceViewerPermission | rust(GET) | verified |
| 207 | `/api/workspaces/{slug}/user-properties/` | GET, PATCH | `plane.app.views.workspace.user.WorkspaceUserPropertiesEndpoint` | WorkspaceViewerPermission | rust(GET,PATCH) | verified |
| 208 | `/api/workspaces/{slug}/states/` | GET | `plane.app.views.workspace.state.WorkspaceStatesEndpoint` | WorkspaceEntityPermission | rust(GET) | verified |
| 209 | `/api/workspaces/{slug}/estimates/` | GET | `plane.app.views.workspace.estimate.WorkspaceEstimatesEndpoint` | WorkspaceEntityPermission | rust(GET) | verified |
| 210 | `/api/workspaces/{slug}/modules/` | GET | `plane.app.views.workspace.module.WorkspaceModulesEndpoint` | WorkspaceViewerPermission | rust(GET) | verified |
| 211 | `/api/workspaces/{slug}/cycles/` | GET | `plane.app.views.workspace.cycle.WorkspaceCyclesEndpoint` | WorkspaceViewerPermission | rust(GET) | verified |
| 212 | `/api/workspaces/{slug}/user-favorites/` | GET, POST, PATCH, DELETE | `plane.app.views.workspace.favorite.WorkspaceFavoriteEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN,MEMBER;WORKSPACE) | rust(GET,POST,PATCH,DELETE) | verified |
| 213 | `/api/workspaces/{slug}/user-favorites/{favorite_id}/` | GET, POST, PATCH, DELETE | `plane.app.views.workspace.favorite.WorkspaceFavoriteEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN,MEMBER;WORKSPACE) | rust(GET,POST,PATCH,DELETE) | verified |
| 214 | `/api/workspaces/{slug}/user-favorites/{favorite_id}/group/` | GET | `plane.app.views.workspace.favorite.WorkspaceFavoriteGroupEndpoint` | IsAuthenticated; GET allow(ADMIN,MEMBER;WORKSPACE) | rust(GET) | verified |
| 215 | `/api/workspaces/{slug}/draft-issues/` | GET:list, POST:create | `plane.app.views.workspace.draft.WorkspaceDraftIssueViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST) | verified |
| 216 | `/api/workspaces/{slug}/draft-issues/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.workspace.draft.WorkspaceDraftIssueViewSet` | IsAuthenticated; GET allow(ADMIN;WORKSPACE;creator=Issue); PATCH allow(ADMIN,MEMBER;WORKSPACE;creator=Issue); DELETE allow(ADMIN;WORKSPACE;creator=DraftIssue) | rust(GET,PATCH,DELETE) | verified |
| 217 | `/api/workspaces/{slug}/draft-to-issue/{draft_id}/` | POST:create_draft_to_issue | `plane.app.views.workspace.draft.WorkspaceDraftIssueViewSet` | IsAuthenticated; POST allow(ADMIN,MEMBER;WORKSPACE) | rust(POST) | verified |
| 218 | `/api/workspaces/{slug}/quick-links/` | GET:list, POST:create | `plane.app.views.workspace.quick_link.QuickLinkViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST) | verified |
| 219 | `/api/workspaces/{slug}/quick-links/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.app.views.workspace.quick_link.QuickLinkViewSet` | IsAuthenticated; GET,PATCH,DELETE allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PATCH,DELETE) | verified |
| 220 | `/api/workspaces/{slug}/home-preferences/` | GET, PATCH | `plane.app.views.workspace.home.WorkspaceHomePreferenceViewSet` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PATCH) | verified |
| 221 | `/api/workspaces/{slug}/home-preferences/{key}/` | GET, PATCH | `plane.app.views.workspace.home.WorkspaceHomePreferenceViewSet` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PATCH) | verified |
| 222 | `/api/workspaces/{slug}/recent-visits/` | GET:list | `plane.app.views.workspace.recent_visit.UserRecentVisitViewSet` | IsAuthenticated; GET allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET) | verified |
| 223 | `/api/workspaces/{slug}/stickies/` | GET:list, POST:create | `plane.app.views.workspace.sticky.WorkspaceStickyViewSet` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,POST) | verified |
| 224 | `/api/workspaces/{slug}/stickies/{pk}/` | GET:retrieve [405], PATCH:partial_update, DELETE:destroy | `plane.app.views.workspace.sticky.WorkspaceStickyViewSet` | IsAuthenticated; PATCH,DELETE allow(-;WORKSPACE;creator=Sticky) | rust(GET,PATCH,DELETE) | verified |
| 225 | `/api/workspaces/{slug}/sidebar-preferences/` | GET, PATCH | `plane.app.views.workspace.user_preference.WorkspaceUserPreferenceViewSet` | IsAuthenticated; GET,PATCH allow(ADMIN,MEMBER,GUEST;WORKSPACE) | rust(GET,PATCH) | verified |
| 226 | `/api/users/api-tokens/` | GET, POST, PATCH, DELETE | `plane.app.views.api.ApiTokenEndpoint` | IsAuthenticated | rust(GET,POST,PATCH,DELETE) | verified |
| 227 | `/api/users/api-tokens/{pk}/` | GET, POST, PATCH, DELETE | `plane.app.views.api.ApiTokenEndpoint` | IsAuthenticated | rust(GET,POST,PATCH,DELETE) | verified |
| 228 | `/api/workspaces/{slug}/webhooks/` | GET, POST, PATCH, DELETE | `plane.app.views.webhook.base.WebhookEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN;WORKSPACE) | rust(GET,POST,PATCH,DELETE) | verified |
| 229 | `/api/workspaces/{slug}/webhooks/{pk}/` | GET, POST, PATCH, DELETE | `plane.app.views.webhook.base.WebhookEndpoint` | IsAuthenticated; GET,POST,PATCH,DELETE allow(ADMIN;WORKSPACE) | rust(GET,POST,PATCH,DELETE) | verified |
| 230 | `/api/workspaces/{slug}/webhooks/{pk}/regenerate/` | POST | `plane.app.views.webhook.base.WebhookSecretRegenerateEndpoint` | IsAuthenticated; POST allow(ADMIN;WORKSPACE) | rust(POST) | verified |
| 231 | `/api/workspaces/{slug}/webhook-logs/{webhook_id}/` | GET | `plane.app.views.webhook.base.WebhookLogsEndpoint` | IsAuthenticated; GET allow(ADMIN;WORKSPACE) | rust(GET) | verified |
| 232 | `/api/timezones/` | GET | `plane.app.views.timezone.base.TimezoneEndpoint` | AllowAny | rust(GET) | verified |
| 233 | `/api/workspaces/{slug}/export-issues/` | GET, POST | `plane.app.views.exporter.base.ExportIssuesEndpoint` | IsAuthenticated; GET,POST allow(ADMIN,MEMBER;WORKSPACE) | rust(GET,POST) | verified |

## v1 (`/api/v1/`)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 273 | `/api/v1/assets/user-assets/` | POST | `plane.api.views.asset.UserAssetEndpoint` | IsAuthenticated | django | not-started |
| 274 | `/api/v1/assets/user-assets/{asset_id}/` | PATCH, DELETE | `plane.api.views.asset.UserAssetEndpoint` | IsAuthenticated | django | not-started |
| 275 | `/api/v1/assets/user-assets/server/` | POST | `plane.api.views.asset.UserServerAssetEndpoint` | IsAuthenticated | django | not-started |
| 276 | `/api/v1/assets/user-assets/{asset_id}/server/` | PATCH, DELETE | `plane.api.views.asset.UserServerAssetEndpoint` | IsAuthenticated | django | not-started |
| 277 | `/api/v1/workspaces/{slug}/assets/` | POST | `plane.api.views.asset.GenericAssetEndpoint` | WorkspaceUserPermission | django | not-started |
| 278 | `/api/v1/workspaces/{slug}/assets/{asset_id}/` | GET, PATCH | `plane.api.views.asset.GenericAssetEndpoint` | WorkspaceUserPermission | django | not-started |
| 279 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles/` | GET, POST | `plane.api.views.cycle.CycleListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 280 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles-lite/` | GET | `plane.api.views.cycle.CycleListLiteAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 281 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/` | GET, PATCH, DELETE | `plane.api.views.cycle.CycleDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 282 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/` | GET, POST | `plane.api.views.cycle.CycleIssueListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 283 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/` | GET, DELETE | `plane.api.views.cycle.CycleIssueDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 284 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/` | POST | `plane.api.views.cycle.TransferCycleIssueAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 285 | `/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/` | POST | `plane.api.views.cycle.CycleArchiveUnarchiveAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 286 | `/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/` | GET | `plane.api.views.cycle.CycleArchiveUnarchiveAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 287 | `/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/{cycle_id}/unarchive/` | DELETE | `plane.api.views.cycle.CycleArchiveUnarchiveAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 288 | `/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/` | GET, POST | `plane.api.views.intake.IntakeIssueListCreateAPIEndpoint` | ProjectLitePermission | django | not-started |
| 289 | `/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/` | GET, PATCH, DELETE | `plane.api.views.intake.IntakeIssueDetailAPIEndpoint` | ProjectLitePermission | django | not-started |
| 290 | `/api/v1/workspaces/{slug}/projects/{project_id}/labels/` | GET, POST | `plane.api.views.issue.LabelListCreateAPIEndpoint` | ProjectMemberPermission (dynamic) | django | not-started |
| 291 | `/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.LabelDetailAPIEndpoint` | ProjectMemberPermission (dynamic) | django | not-started |
| 292 | `/api/v1/workspaces/{slug}/projects/{project_id}/members/` | GET, POST | `plane.api.views.member.ProjectMemberListCreateAPIEndpoint` | ProjectMemberPermission (dynamic) | django | not-started |
| 293 | `/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/` | GET, PATCH, DELETE | `plane.api.views.member.ProjectMemberDetailAPIEndpoint` | ProjectMemberPermission (dynamic) | django | not-started |
| 294 | `/api/v1/workspaces/{slug}/projects/{project_id}/project-members/` | GET, POST | `plane.api.views.member.ProjectMemberListCreateAPIEndpoint` | ProjectMemberPermission (dynamic) | django | not-started |
| 295 | `/api/v1/workspaces/{slug}/projects/{project_id}/project-members-lite/` | GET | `plane.api.views.member.ProjectMemberLiteAPIEndpoint` | ProjectMemberPermission | django | not-started |
| 296 | `/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/` | GET, PATCH, DELETE | `plane.api.views.member.ProjectMemberDetailAPIEndpoint` | ProjectMemberPermission (dynamic) | django | not-started |
| 297 | `/api/v1/workspaces/{slug}/members/` | GET | `plane.api.views.member.WorkspaceMemberAPIEndpoint` | WorkSpaceAdminPermission | django | not-started |
| 298 | `/api/v1/workspaces/{slug}/members-lite/` | GET | `plane.api.views.member.WorkspaceMemberLiteAPIEndpoint` | WorkSpaceAdminPermission | django | not-started |
| 299 | `/api/v1/workspaces/{slug}/projects/{project_id}/modules/` | GET, POST | `plane.api.views.module.ModuleListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 300 | `/api/v1/workspaces/{slug}/projects/{project_id}/modules-lite/` | GET | `plane.api.views.module.ModuleListLiteAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 301 | `/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/` | GET, PATCH, DELETE | `plane.api.views.module.ModuleDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 302 | `/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/` | GET, POST | `plane.api.views.module.ModuleIssueListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 303 | `/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/{issue_id}/` | DELETE | `plane.api.views.module.ModuleIssueDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 304 | `/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/` | POST | `plane.api.views.module.ModuleArchiveUnarchiveAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 305 | `/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/` | GET | `plane.api.views.module.ModuleArchiveUnarchiveAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 306 | `/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/unarchive/` | DELETE | `plane.api.views.module.ModuleArchiveUnarchiveAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 307 | `/api/v1/workspaces/{slug}/projects/` | GET, POST | `plane.api.views.project.ProjectListCreateAPIEndpoint` | ProjectBasePermission | django | not-started |
| 308 | `/api/v1/workspaces/{slug}/projects-lite/` | GET | `plane.api.views.project.ProjectListLiteAPIEndpoint` | ProjectBasePermission | django | not-started |
| 309 | `/api/v1/workspaces/{slug}/projects/{pk}/` | GET, PATCH, DELETE | `plane.api.views.project.ProjectDetailAPIEndpoint` | ProjectBasePermission | django | not-started |
| 310 | `/api/v1/workspaces/{slug}/projects/{project_id}/archive/` | POST, DELETE | `plane.api.views.project.ProjectArchiveUnarchiveAPIEndpoint` | ProjectArchiveUnarchivePermission | django | not-started |
| 311 | `/api/v1/workspaces/{slug}/projects/{project_id}/summary/` | GET | `plane.api.views.project.ProjectSummaryAPIEndpoint` | WorkSpaceAdminPermission | django | not-started |
| 312 | `/api/v1/workspaces/{slug}/projects/{project_id}/states/` | GET, POST | `plane.api.views.state.StateListCreateAPIEndpoint` | ProjectEntityPermission (dynamic) | django | not-started |
| 313 | `/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/` | GET, PATCH, DELETE | `plane.api.views.state.StateDetailAPIEndpoint` | ProjectEntityPermission (dynamic) | django | not-started |
| 314 | `/api/v1/users/me/` | GET | `plane.api.views.user.UserEndpoint` | IsAuthenticated | django | not-started |
| 315 | `/api/v1/workspaces/{slug}/issues/search/` | GET | `plane.api.views.issue.IssueSearchEndpoint` | IsAuthenticated | django | not-started |
| 316 | `/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/` | GET | `plane.api.views.issue.WorkspaceIssueAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 317 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/` | GET, POST | `plane.api.views.issue.IssueListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 318 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 319 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/` | GET, POST | `plane.api.views.issue.IssueLinkListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 320 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueLinkDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 321 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/` | GET, POST | `plane.api.views.issue.IssueCommentListCreateAPIEndpoint` | ProjectLitePermission | django | not-started |
| 322 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueCommentDetailAPIEndpoint` | ProjectLitePermission | django | not-started |
| 323 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/` | GET | `plane.api.views.issue.IssueActivityListAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 324 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/` | GET | `plane.api.views.issue.IssueActivityDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 325 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/` | GET, POST | `plane.api.views.issue.IssueAttachmentListCreateAPIEndpoint` | IsAuthenticated | django | not-started |
| 326 | `/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueAttachmentDetailAPIEndpoint` | IsAuthenticated | django | not-started |
| 327 | `/api/v1/workspaces/{slug}/work-items/search/` | GET | `plane.api.views.issue.IssueSearchEndpoint` | IsAuthenticated | django | not-started |
| 328 | `/api/v1/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/` | GET | `plane.api.views.issue.WorkspaceIssueAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 329 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/` | GET, POST | `plane.api.views.issue.IssueListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 330 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 331 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/` | GET, POST | `plane.api.views.issue.IssueLinkListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 332 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueLinkDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 333 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/` | GET, POST | `plane.api.views.issue.IssueCommentListCreateAPIEndpoint` | ProjectLitePermission | django | not-started |
| 334 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueCommentDetailAPIEndpoint` | ProjectLitePermission | django | not-started |
| 335 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/` | GET | `plane.api.views.issue.IssueActivityListAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 336 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/` | GET | `plane.api.views.issue.IssueActivityDetailAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 337 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/` | GET, POST | `plane.api.views.issue.IssueAttachmentListCreateAPIEndpoint` | IsAuthenticated | django | not-started |
| 338 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/` | GET, PATCH, DELETE | `plane.api.views.issue.IssueAttachmentDetailAPIEndpoint` | IsAuthenticated | django | not-started |
| 339 | `/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/` | GET, POST | `plane.api.views.issue.IssueRelationListCreateAPIEndpoint` | ProjectEntityPermission | django | not-started |
| 340 | `/api/v1/workspaces/{slug}/invitations/` | GET:list, POST:create | `plane.api.views.invite.WorkspaceInvitationsViewset` | WorkspaceOwnerPermission | django | not-started |
| 341 | `/api/v1/workspaces/{slug}/invitations.{format}` | GET:list, POST:create | `plane.api.views.invite.WorkspaceInvitationsViewset` | WorkspaceOwnerPermission | django | not-started |
| 342 | `/api/v1/workspaces/{slug}/invitations/{pk}/` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.api.views.invite.WorkspaceInvitationsViewset` | WorkspaceOwnerPermission | django | not-started |
| 343 | `/api/v1/workspaces/{slug}/invitations/{pk}.{format}` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.api.views.invite.WorkspaceInvitationsViewset` | WorkspaceOwnerPermission | django | not-started |
| 344 | `/api/v1/workspaces/{slug}/` | GET | `rest_framework.routers.APIRootView` | IsAuthenticated | django | not-started |
| 345 | `/api/v1/workspaces/{slug}/.{format}` | GET | `rest_framework.routers.APIRootView` | IsAuthenticated | django | not-started |
| 346 | `/api/v1/workspaces/{slug}/stickies/` | GET:list, POST:create | `plane.api.views.sticky.StickyViewSet` | WorkspaceUserPermission | django | not-started |
| 347 | `/api/v1/workspaces/{slug}/stickies.{format}` | GET:list, POST:create | `plane.api.views.sticky.StickyViewSet` | WorkspaceUserPermission | django | not-started |
| 348 | `/api/v1/workspaces/{slug}/stickies/{pk}/` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.api.views.sticky.StickyViewSet` | WorkspaceUserPermission | django | not-started |
| 349 | `/api/v1/workspaces/{slug}/stickies/{pk}.{format}` | GET:retrieve, PUT:update, PATCH:partial_update, DELETE:destroy | `plane.api.views.sticky.StickyViewSet` | WorkspaceUserPermission | django | not-started |
| 350 | `/api/v1/workspaces/{slug}/` | GET | `rest_framework.routers.APIRootView` | IsAuthenticated | django | not-started |
| 351 | `/api/v1/workspaces/{slug}/.{format}` | GET | `rest_framework.routers.APIRootView` | IsAuthenticated | django | not-started |

## public (`/api/public/`)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 234 | `/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/` | GET:list, POST:create | `plane.space.views.intake.IntakeIssuePublicViewSet` | IsAuthenticated | rust(GET,POST) | verified |
| 235 | `/api/public/anchor/{anchor}/intakes/{intake_id}/inbox-issues/` | GET:list, POST:create | `plane.space.views.intake.IntakeIssuePublicViewSet` | IsAuthenticated | rust(GET,POST) | verified |
| 236 | `/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.space.views.intake.IntakeIssuePublicViewSet` | IsAuthenticated | rust(GET,PATCH,DELETE) | verified |
| 237 | `/api/public/workspaces/{slug}/project-boards/` | GET | `plane.space.views.project.WorkspaceProjectDeployBoardEndpoint` | AllowAny | rust(GET) | verified |
| 238 | `/api/public/anchor/{anchor}/issues/{issue_id}/` | GET | `plane.space.views.issue.IssueRetrievePublicEndpoint` | AllowAny | rust(GET) | verified |
| 239 | `/api/public/anchor/{anchor}/issues/{issue_id}/comments/` | GET:list, POST:create | `plane.space.views.issue.IssueCommentPublicViewSet` | IsAuthenticated (dynamic) | rust(GET,POST) | verified |
| 240 | `/api/public/anchor/{anchor}/issues/{issue_id}/comments/{pk}/` | GET:retrieve, PATCH:partial_update, DELETE:destroy | `plane.space.views.issue.IssueCommentPublicViewSet` | IsAuthenticated (dynamic) | rust(GET,PATCH,DELETE) | verified |
| 241 | `/api/public/anchor/{anchor}/issues/{issue_id}/reactions/` | GET:list, POST:create | `plane.space.views.issue.IssueReactionPublicViewSet` | IsAuthenticated | rust(GET,POST) | verified |
| 242 | `/api/public/anchor/{anchor}/issues/{issue_id}/reactions/{reaction_code}/` | DELETE:destroy | `plane.space.views.issue.IssueReactionPublicViewSet` | IsAuthenticated | rust(DELETE) | verified |
| 243 | `/api/public/anchor/{anchor}/comments/{comment_id}/reactions/` | GET:list, POST:create | `plane.space.views.issue.CommentReactionPublicViewSet` | IsAuthenticated | rust(GET,POST) | verified |
| 244 | `/api/public/anchor/{anchor}/comments/{comment_id}/reactions/{reaction_code}/` | DELETE:destroy | `plane.space.views.issue.CommentReactionPublicViewSet` | IsAuthenticated | rust(DELETE) | verified |
| 245 | `/api/public/anchor/{anchor}/issues/{issue_id}/votes/` | GET:list, POST:create, DELETE:destroy | `plane.space.views.issue.IssueVotePublicViewSet` | IsAuthenticated | rust(GET,POST,DELETE) | verified |
| 246 | `/api/public/anchor/{anchor}/meta/` | GET | `plane.space.views.meta.ProjectMetaDataEndpoint` | AllowAny | rust(GET) | verified |
| 247 | `/api/public/anchor/{anchor}/settings/` | GET | `plane.space.views.project.ProjectDeployBoardPublicSettingsEndpoint` | AllowAny | rust(GET) | verified |
| 248 | `/api/public/anchor/{anchor}/issues/` | GET | `plane.space.views.issue.ProjectIssuesPublicEndpoint` | AllowAny | django | not-started |
| 249 | `/api/public/workspaces/{slug}/projects/{project_id}/anchor/` | GET | `plane.space.views.project.WorkspaceProjectAnchorEndpoint` | AllowAny | rust(GET) | verified |
| 250 | `/api/public/anchor/{anchor}/cycles/` | GET | `plane.space.views.cycle.ProjectCyclesEndpoint` | AllowAny | rust(GET) | verified |
| 251 | `/api/public/anchor/{anchor}/modules/` | GET | `plane.space.views.module.ProjectModulesEndpoint` | AllowAny | rust(GET) | verified |
| 252 | `/api/public/anchor/{anchor}/states/` | GET | `plane.space.views.state.ProjectStatesEndpoint` | AllowAny | rust(GET) | verified |
| 253 | `/api/public/anchor/{anchor}/labels/` | GET | `plane.space.views.label.ProjectLabelsEndpoint` | AllowAny | rust(GET) | verified |
| 254 | `/api/public/anchor/{anchor}/members/` | GET | `plane.space.views.project.ProjectMembersEndpoint` | AllowAny | rust(GET) | verified |
| 255 | `/api/public/assets/v2/anchor/{anchor}/` | GET, POST, PATCH, DELETE | `plane.space.views.asset.EntityAssetEndpoint` | IsAuthenticated (dynamic) | rust(GET,POST,PATCH,DELETE) | verified |
| 256 | `/api/public/assets/v2/anchor/{anchor}/{pk}/` | GET, POST, PATCH, DELETE | `plane.space.views.asset.EntityAssetEndpoint` | IsAuthenticated (dynamic) | rust(GET,POST,PATCH,DELETE) | verified |
| 257 | `/api/public/assets/v2/anchor/{anchor}/restore/{pk}/` | POST | `plane.space.views.asset.AssetRestoreEndpoint` | IsAuthenticated | rust(POST) | verified |
| 258 | `/api/public/assets/v2/anchor/{anchor}/{entity_id}/bulk/` | POST | `plane.space.views.asset.EntityBulkAssetEndpoint` | IsAuthenticated | rust(POST) | verified |

## auth (`/auth/`)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 352 | `/auth/sign-in/` | POST | `plane.authentication.views.app.email.SignInAuthEndpoint` | - | rust(POST) | verified |
| 353 | `/auth/sign-up/` | POST | `plane.authentication.views.app.email.SignUpAuthEndpoint` | - | rust(POST) | verified |
| 354 | `/auth/spaces/sign-in/` | POST | `plane.authentication.views.space.email.SignInAuthSpaceEndpoint` | - | rust(POST) | verified |
| 355 | `/auth/spaces/sign-up/` | POST | `plane.authentication.views.space.email.SignUpAuthSpaceEndpoint` | - | rust(POST) | verified |
| 356 | `/auth/sign-out/` | POST | `plane.authentication.views.app.signout.SignOutAuthEndpoint` | - | rust(POST) | verified |
| 357 | `/auth/spaces/sign-out/` | POST | `plane.authentication.views.space.signout.SignOutAuthSpaceEndpoint` | - | rust(POST) | verified |
| 358 | `/auth/get-csrf-token/` | GET | `plane.authentication.views.common.CSRFTokenEndpoint` | AllowAny | rust(GET) | verified |
| 359 | `/auth/magic-generate/` | POST | `plane.authentication.views.app.magic.MagicGenerateEndpoint` | AllowAny | rust(POST) | verified |
| 360 | `/auth/magic-sign-in/` | POST | `plane.authentication.views.app.magic.MagicSignInEndpoint` | - | rust(POST) | verified |
| 361 | `/auth/magic-sign-up/` | POST | `plane.authentication.views.app.magic.MagicSignUpEndpoint` | - | rust(POST) | verified |
| 362 | `/auth/spaces/magic-generate/` | POST | `plane.authentication.views.space.magic.MagicGenerateSpaceEndpoint` | AllowAny | rust(POST) | verified |
| 363 | `/auth/spaces/magic-sign-in/` | POST | `plane.authentication.views.space.magic.MagicSignInSpaceEndpoint` | - | rust(POST) | verified |
| 364 | `/auth/spaces/magic-sign-up/` | POST | `plane.authentication.views.space.magic.MagicSignUpSpaceEndpoint` | - | rust(POST) | verified |
| 365 | `/auth/google/` | GET | `plane.authentication.views.app.google.GoogleOauthInitiateEndpoint` | - | rust(GET) | verified |
| 366 | `/auth/google/callback/` | GET | `plane.authentication.views.app.google.GoogleCallbackEndpoint` | - | rust(GET) | verified |
| 367 | `/auth/spaces/google/` | GET | `plane.authentication.views.space.google.GoogleOauthInitiateSpaceEndpoint` | - | rust(GET) | verified |
| 368 | `/auth/spaces/google/callback/` | GET | `plane.authentication.views.space.google.GoogleCallbackSpaceEndpoint` | - | rust(GET) | verified |
| 369 | `/auth/github/` | GET | `plane.authentication.views.app.github.GitHubOauthInitiateEndpoint` | - | rust(GET) | verified |
| 370 | `/auth/github/callback/` | GET | `plane.authentication.views.app.github.GitHubCallbackEndpoint` | - | rust(GET) | verified |
| 371 | `/auth/spaces/github/` | GET | `plane.authentication.views.space.github.GitHubOauthInitiateSpaceEndpoint` | - | rust(GET) | verified |
| 372 | `/auth/spaces/github/callback/` | GET | `plane.authentication.views.space.github.GitHubCallbackSpaceEndpoint` | - | rust(GET) | verified |
| 373 | `/auth/gitlab/` | GET | `plane.authentication.views.app.gitlab.GitLabOauthInitiateEndpoint` | - | rust(GET) | verified |
| 374 | `/auth/gitlab/callback/` | GET | `plane.authentication.views.app.gitlab.GitLabCallbackEndpoint` | - | rust(GET) | verified |
| 375 | `/auth/spaces/gitlab/` | GET | `plane.authentication.views.space.gitlab.GitLabOauthInitiateSpaceEndpoint` | - | rust(GET) | verified |
| 376 | `/auth/spaces/gitlab/callback/` | GET | `plane.authentication.views.space.gitlab.GitLabCallbackSpaceEndpoint` | - | rust(GET) | verified |
| 377 | `/auth/email-check/` | POST | `plane.authentication.views.app.check.EmailCheckEndpoint` | AllowAny | rust(POST) | verified |
| 378 | `/auth/spaces/email-check/` | POST | `plane.authentication.views.space.check.EmailCheckSpaceEndpoint` | AllowAny | rust(POST) | verified |
| 379 | `/auth/forgot-password/` | POST | `plane.authentication.views.app.password_management.ForgotPasswordEndpoint` | AllowAny | rust(POST) | verified |
| 380 | `/auth/reset-password/{uidb64}/{token}/` | POST | `plane.authentication.views.app.password_management.ResetPasswordEndpoint` | - | rust(POST) | verified |
| 381 | `/auth/spaces/forgot-password/` | POST | `plane.authentication.views.space.password_management.ForgotPasswordSpaceEndpoint` | AllowAny | rust(POST) | verified |
| 382 | `/auth/spaces/reset-password/{uidb64}/{token}/` | POST | `plane.authentication.views.space.password_management.ResetPasswordSpaceEndpoint` | - | rust(POST) | verified |
| 383 | `/auth/change-password/` | POST | `plane.authentication.views.common.ChangePasswordEndpoint` | IsAuthenticated | rust(POST) | verified |
| 384 | `/auth/set-password/` | POST | `plane.authentication.views.common.SetUserPasswordEndpoint` | IsAuthenticated | rust(POST) | verified |
| 385 | `/auth/gitea/` | GET | `plane.authentication.views.app.gitea.GiteaOauthInitiateEndpoint` | - | rust(GET) | verified |
| 386 | `/auth/gitea/callback/` | GET | `plane.authentication.views.app.gitea.GiteaCallbackEndpoint` | - | rust(GET) | verified |
| 387 | `/auth/spaces/gitea/` | GET | `plane.authentication.views.space.gitea.GiteaOauthInitiateSpaceEndpoint` | - | rust(GET) | verified |
| 388 | `/auth/spaces/gitea/callback/` | GET | `plane.authentication.views.space.gitea.GiteaCallbackSpaceEndpoint` | - | rust(GET) | verified |

## instances (`/api/instances/`)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 259 | `/api/instances/` | GET, PATCH | `plane.license.api.views.instance.InstanceEndpoint` | IsAuthenticated (dynamic) | rust(GET,PATCH) | verified |
| 260 | `/api/instances/admins/` | GET, POST, DELETE | `plane.license.api.views.admin.InstanceAdminEndpoint` | InstanceAdminPermission | rust(GET,POST,DELETE) | verified |
| 261 | `/api/instances/admins/me/` | GET | `plane.license.api.views.admin.InstanceAdminUserMeEndpoint` | InstanceAdminPermission | rust(GET) | verified |
| 262 | `/api/instances/admins/session/` | GET | `plane.license.api.views.admin.InstanceAdminUserSessionEndpoint` | AllowAny | rust(GET) | verified |
| 263 | `/api/instances/admins/sign-out/` | POST | `plane.license.api.views.admin.InstanceAdminSignOutEndpoint` | - | rust(POST) | verified |
| 264 | `/api/instances/admins/{pk}/` | GET, POST, DELETE | `plane.license.api.views.admin.InstanceAdminEndpoint` | InstanceAdminPermission | rust(GET,POST,DELETE) | verified |
| 265 | `/api/instances/configurations/` | GET, PATCH | `plane.license.api.views.configuration.InstanceConfigurationEndpoint` | InstanceAdminPermission | rust(GET,PATCH) | verified |
| 266 | `/api/instances/configurations/disable-email-feature/` | DELETE | `plane.license.api.views.configuration.DisableEmailFeatureEndpoint` | InstanceAdminPermission | rust(DELETE) | verified |
| 267 | `/api/instances/admins/sign-in/` | POST | `plane.license.api.views.admin.InstanceAdminSignInEndpoint` | - | rust(POST) | verified |
| 268 | `/api/instances/admins/sign-up/` | POST | `plane.license.api.views.admin.InstanceAdminSignUpEndpoint` | - | rust(POST) | verified |
| 269 | `/api/instances/admins/sign-up-screen-visited/` | POST | `plane.license.api.views.instance.SignUpScreenVisitedEndpoint` | AllowAny | rust(POST) | verified |
| 270 | `/api/instances/email-credentials-check/` | POST | `plane.license.api.views.configuration.EmailCredentialCheckEndpoint` | InstanceAdminPermission | rust(POST) | verified |
| 271 | `/api/instances/workspace-slug-check/` | GET | `plane.license.api.views.workspace.InstanceWorkSpaceAvailabilityCheckEndpoint` | InstanceAdminPermission | rust(GET) | verified |
| 272 | `/api/instances/workspaces/` | GET, POST | `plane.license.api.views.workspace.InstanceWorkSpaceEndpoint` | InstanceAdminPermission | rust(GET,POST) | verified |

## web (`plane.web`)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 389 | `/robots.txt` | ALL (robots_txt) | `plane.web.views.robots_txt` | - | django | not-started |
| 390 | `/` | ALL (health_check) | `plane.web.views.health_check` | - | django | not-started |

## schema (drf-spectacular)

| # | Path | Methods | View | Permissions | Owner | Status |
|---|---|---|---|---|---|---|
| 391 | `/api/schema/` | GET | `drf_spectacular.views.SpectacularAPIView` | AllowAny | django | in-progress |
| 392 | `/api/schema/swagger-ui/` | GET | `drf_spectacular.views.SpectacularSwaggerView` | AllowAny | django | in-progress |
| 393 | `/api/schema/redoc/` | GET | `drf_spectacular.views.SpectacularRedocView` | AllowAny | django | in-progress |

## Routing hazards

Everything in this section is computed from `routes.json`. Path templates use `{name}` for a captured segment. `matchit` is the router under axum 0.8 (matchit 0.8): static segments are tried before parameters, and a parameter only matches the text its Django converter would accept if the handler validates it.

### (a) Parameter vs literal at the same position

Found 14 single-segment candidates where a parameter and a literal compete; 0 disagree with matchit.

The table lists every pair whose templates are identical except at one segment, where one side has a parameter and the other a literal. A full pairwise scan that also requires a concrete URL to match both templates (every segment compatible with its converter, and the same trailing-slash form) finds 0 pairs. In particular the brief's example — `issues/{pk}/` next to `issues/bulk-delete/` — does not occur here: the bulk-delete route is `bulk-delete-issues/` at a different position, and `{pk}` is `<uuid:pk>`, whose converter rejects `bulk-delete`.

| First (order) | Second (order) | Position | Parameter | Literal | Common URL? | Django picks | matchit picks | Same? |
| --- | --- | ---: | --- | --- | --- | --- | --- | --- |
| #32 `/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/` | #35 `/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/` | 6 | {pk} | `date-check` | no | #35 | #35 | yes |
| #63 `/api/workspaces/{slug}/projects/{project_id}/issues/list/` | #67 `/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/` | 6 | {pk} | `list` | no | #63 | #63 | yes |
| #117 `/api/workspaces/{slug}/users/notifications/{pk}/` | #120 `/api/workspaces/{slug}/users/notifications/unread/` | 5 | {pk} | `unread` | no | #120 | #120 | yes |
| #117 `/api/workspaces/{slug}/users/notifications/{pk}/` | #121 `/api/workspaces/{slug}/users/notifications/mark-all-read/` | 5 | {pk} | `mark-all-read` | no | #121 | #121 | yes |
| #136 `/api/workspaces/{slug}/projects/details/` | #137 `/api/workspaces/{slug}/projects/{pk}/` | 4 | {pk} | `details` | no | #136 | #136 | yes |
| #145 `/api/workspaces/{slug}/projects/{project_id}/members/{pk}/` | #146 `/api/workspaces/{slug}/projects/{project_id}/members/leave/` | 6 | {pk} | `leave` | no | #146 | #146 | yes |
| #194 `/api/workspaces/{slug}/members/{pk}/` | #195 `/api/workspaces/{slug}/members/leave/` | 4 | {pk} | `leave` | no | #195 | #195 | yes |
| #261 `/api/instances/admins/me/` | #264 `/api/instances/admins/{pk}/` | 3 | {pk} | `me` | no | #261 | #261 | yes |
| #262 `/api/instances/admins/session/` | #264 `/api/instances/admins/{pk}/` | 3 | {pk} | `session` | no | #262 | #262 | yes |
| #263 `/api/instances/admins/sign-out/` | #264 `/api/instances/admins/{pk}/` | 3 | {pk} | `sign-out` | no | #263 | #263 | yes |
| #264 `/api/instances/admins/{pk}/` | #267 `/api/instances/admins/sign-in/` | 3 | {pk} | `sign-in` | no | #267 | #267 | yes |
| #264 `/api/instances/admins/{pk}/` | #268 `/api/instances/admins/sign-up/` | 3 | {pk} | `sign-up` | no | #268 | #268 | yes |
| #264 `/api/instances/admins/{pk}/` | #269 `/api/instances/admins/sign-up-screen-visited/` | 3 | {pk} | `sign-up-screen-visited` | no | #269 | #269 | yes |
| #274 `/api/v1/assets/user-assets/{asset_id}/` | #275 `/api/v1/assets/user-assets/server/` | 4 | {asset_id} | `server` | no | #275 | #275 | yes |

"Common URL?" means a concrete URL can match both templates: the literal must satisfy the parameter's Django converter, and the trailing-slash form must be compatible. When it is `no`, Django's earlier parameter route rejects the literal value and falls through to the literal route, which is also matchit's choice — so the outcomes agree. A disagreement needs a common URL, the parameter route registered first, and a `str`-like converter that accepts the literal. None exist in this tree because Plane uses `<uuid:...>` wherever a literal sibling could otherwise be shadowed.

No candidate changes the winner: Django and matchit agree on every pair in this tree. The Rust router still has to validate `uuid` and `int` parameters and return 404 when they do not parse, because matchit's parameter accepts any segment while Django's converter rejects it (see (c)).

### (b) Identical normalised paths registered more than once

Found 2 paths with more than one resolver entry. Django resolves by path only and tries entries in order, so the first entry serves every HTTP method; later entries are unreachable, they do not serve the methods the first entry lacks.

| Path | Entries (order, view, methods) | First wins |
| --- | --- | --- |
| `/api/v1/workspaces/{slug}/` | #344 APIRootView [GET]; #350 APIRootView [GET] | #344 APIRootView |
| `/api/v1/workspaces/{slug}/.{format}` | #345 APIRootView [GET]; #351 APIRootView [GET] | #345 APIRootView |

Both duplicates are the DRF `DefaultRouter` api-root, included once by `plane/api/urls/invite.py` and once by `plane/api/urls/sticky.py`. The first registration (the invitations router) answers `GET /api/v1/workspaces/{slug}/` and its `.<format>` variant, so its `api_root_dict` — invitations only — is what clients see. Port that behaviour as it is; a Rust router that merged the two routers would change the response.

### (c) Non-standard converters and regex routes

Found 10 entries: 8 regex patterns and 2 entries with a custom converter. `str`, `uuid`, `int`, `slug` and `path` are the standard converters; none of the entries here uses `int`, `slug` or `path`.

| # | Path | Converter(s) | Pattern type | View |
| ---: | --- | --- | --- | --- |
| 340 | `/api/v1/workspaces/{slug}/invitations/` | slug:regex str | regex | `WorkspaceInvitationsViewset` |
| 341 | `/api/v1/workspaces/{slug}/invitations.{format}` | slug:regex str, format:regex [a-z0-9]+ | regex | `WorkspaceInvitationsViewset` |
| 342 | `/api/v1/workspaces/{slug}/invitations/{pk}/` | slug:regex str, pk:regex [^/.]+ | regex | `WorkspaceInvitationsViewset` |
| 343 | `/api/v1/workspaces/{slug}/invitations/{pk}.{format}` | slug:regex str, pk:regex [^/.]+, format:regex [a-z0-9]+ | regex | `WorkspaceInvitationsViewset` |
| 345 | `/api/v1/workspaces/{slug}/.{format}` | slug:str, format:drf_format_suffix | path | `APIRootView` |
| 346 | `/api/v1/workspaces/{slug}/stickies/` | slug:regex str | regex | `StickyViewSet` |
| 347 | `/api/v1/workspaces/{slug}/stickies.{format}` | slug:regex str, format:regex [a-z0-9]+ | regex | `StickyViewSet` |
| 348 | `/api/v1/workspaces/{slug}/stickies/{pk}/` | slug:regex str, pk:regex [^/.]+ | regex | `StickyViewSet` |
| 349 | `/api/v1/workspaces/{slug}/stickies/{pk}.{format}` | slug:regex str, pk:regex [^/.]+, format:regex [a-z0-9]+ | regex | `StickyViewSet` |
| 351 | `/api/v1/workspaces/{slug}/.{format}` | slug:str, format:drf_format_suffix | path | `APIRootView` |

The regex entries are generated by `DefaultRouter`: the list/detail routes use `(?P<pk>[^/.]+)` and the format variants add `\.(?P<format>[a-z0-9]+)` with an optional trailing slash. `drf_format_suffix` is DRF's `FormatSuffixConverter`, whose regex is `\.[a-z0-9]+/?`; the dot is part of the match, which is why the template is `.{format}`, not `{format}`. Django rejects `/workspaces/{slug}/json` and resolves `/workspaces/{slug}/.json` (with or without a trailing slash). matchit has no converters: a `{pk}` segment matches `not-a-uuid` and `not a uuid`, where Django's `<uuid:pk>` returns 404 (and a slash is a segment separator in both). Every UUID, int, slug and regex parameter must therefore be validated in the Rust handler before it reaches the database, and a parse failure must produce Django's 404, not a handler error.

### (d) Routes without a trailing slash

Found 7 entries. The `absent` rows match only without the trailing slash; the `optional` rows match both forms. (`CommonMiddleware`'s `APPEND_SLASH` can append a slash only when nothing matches and the slashed form exists, so it does not help the `absent` routes.)

| # | Path | Slash form | View |
| ---: | --- | --- | --- |
| 341 | `/api/v1/workspaces/{slug}/invitations.{format}` | optional | `WorkspaceInvitationsViewset` |
| 343 | `/api/v1/workspaces/{slug}/invitations/{pk}.{format}` | optional | `WorkspaceInvitationsViewset` |
| 345 | `/api/v1/workspaces/{slug}/.{format}` | optional | `APIRootView` |
| 347 | `/api/v1/workspaces/{slug}/stickies.{format}` | optional | `StickyViewSet` |
| 349 | `/api/v1/workspaces/{slug}/stickies/{pk}.{format}` | optional | `StickyViewSet` |
| 351 | `/api/v1/workspaces/{slug}/.{format}` | optional | `APIRootView` |
| 389 | `/robots.txt` | absent | `robots_txt` |

The `optional` rows are DRF format-suffix routes: four regex patterns and two `<drf_format_suffix:format>` path patterns. They accept both `/invitations.json` and `/invitations.json/` (and `.json` for the api-root). The path template shows the literal dot because Django requires it; the optional slash is not part of the template.

### (e) Patterns that differ only by a trailing slash

Found 0 pairs of normalised paths that are identical once the trailing slash is removed and register both forms.

None. The DRF format-suffix regexes accept an optional trailing slash, but no second entry registers the other form as a separate path.

### (f) Segments matchit cannot express as-is

matchit allows at most one named parameter per segment. A segment with two captures such as `{project_identifier}-{issue_identifier}` is rejected by `Router::insert` (`InsertError::InvalidParamSegment`), so axum needs two routes, a regex layer in front of the router, or a custom fallback.

| # | Path | Segment | View |
| ---: | --- | --- | --- |
| 102 | `/api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/` | `{project_identifier}-{issue_identifier}` | `IssueDetailIdentifierEndpoint` |
| 316 | `/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/` | `{project_identifier}-{issue_identifier}` | `WorkspaceIssueAPIEndpoint` |
| 328 | `/api/v1/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/` | `{project_identifier}-{issue_identifier}` | `WorkspaceIssueAPIEndpoint` |
| 343 | `/api/v1/workspaces/{slug}/invitations/{pk}.{format}` | `{pk}.{format}` | `WorkspaceInvitationsViewset` |
| 349 | `/api/v1/workspaces/{slug}/stickies/{pk}.{format}` | `{pk}.{format}` | `StickyViewSet` |

One-parameter segments with literal text (4 entries, e.g. `invitations.{format}`, `.{format}`, `stickies.{format}`) are supported by matchit as prefix/suffix parameters; they are not a hazard, but they match as prefix parameters with static priority rules that differ from Django's regex order.

### (g) ViewSet actions registered but refused by the BaseViewSet guard

`plane/app/views/base.py:BaseViewSet.initial` refuses an action that is served straight from a DRF mixin when the viewset has only `IsAuthenticated`/`AllowAny` and does not implement the action itself. The route is still registered for that method, but an authenticated request gets 405. Found 24 such method(s) across 17 routes; 3 viewset route(s) override `get_permissions` and could not be decided statically.

| # | Path | Method | Action | Handler owner |
| ---: | --- | --- | --- | --- |
| 32 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/` | PUT | update | `UpdateModelMixin` |
| 34 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/` | GET | retrieve | `RetrieveModelMixin` |
| 34 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/` | PUT | update | `UpdateModelMixin` |
| 34 | `/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/` | PATCH | partial_update | `UpdateModelMixin` |
| 36 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/` | GET | list | `ListModelMixin` |
| 54 | `/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/` | GET | retrieve | `RetrieveModelMixin` |
| 54 | `/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/` | PATCH | partial_update | `UpdateModelMixin` |
| 58 | `/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/` | GET | retrieve | `RetrieveModelMixin` |
| 58 | `/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/` | PATCH | partial_update | `UpdateModelMixin` |
| 67 | `/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/` | PUT | update | `UpdateModelMixin` |
| 82 | `/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/` | PUT | update | `UpdateModelMixin` |
| 104 | `/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/` | PUT | update | `UpdateModelMixin` |
| 107 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/` | GET | retrieve | `RetrieveModelMixin` |
| 107 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/` | PUT | update | `UpdateModelMixin` |
| 107 | `/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/` | PATCH | partial_update | `UpdateModelMixin` |
| 117 | `/api/workspaces/{slug}/users/notifications/{pk}/` | GET | retrieve | `RetrieveModelMixin` |
| 117 | `/api/workspaces/{slug}/users/notifications/{pk}/` | DELETE | destroy | `DestroyModelMixin` |
| 137 | `/api/workspaces/{slug}/projects/{pk}/` | PUT | update | `UpdateModelMixin` |
| 141 | `/api/users/me/workspaces/{slug}/projects/invitations/` | GET | list | `ListModelMixin` |
| 149 | `/api/workspaces/{slug}/user-favorite-projects/` | GET | list | `ListModelMixin` |
| 179 | `/api/workspaces/{slug}/projects/{project_id}/views/{pk}/` | PUT | update | `UpdateModelMixin` |
| 181 | `/api/workspaces/{slug}/views/{pk}/` | PUT | update | `UpdateModelMixin` |
| 183 | `/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/` | GET | list | `ListModelMixin` |
| 224 | `/api/workspaces/{slug}/stickies/{pk}/` | GET | retrieve | `RetrieveModelMixin` |

The methods column marks these with `[405]`. The action map still contains them because `routes.json` records what the URL entry registers; the Rust router must return Django's 405 for these method/path pairs rather than run the inherited mixin handler. `OPTIONS` still succeeds (the guard skips it).

Dynamic `get_permissions` was not evaluated: #83 `IssueSubscriberViewSet`, #84 `IssueSubscriberViewSet`, #85 `IssueSubscriberViewSet`. Check these at runtime; the guard only refuses when `get_permissions()` returns nothing stronger than `IsAuthenticated` or `AllowAny`.

### `/api/` prefix shadowing

`plane/urls.py` registers `api/` (the app resolver) before `api/public/`, `api/instances/` and `api/v1/`. A resolver include is tried and, if its subtree does not match, Django falls through to the next top-level pattern, so this is only a hazard if an app route's first segment is one of the reserved prefixes. Checked all 234 app routes; 0 begin with `public/`, `instances/`, `v1/` or `schema/`.

None. The app router's first segments are `assets`, `timezones`, `unsplash`, `users`, `workspace-slug-check`, `workspaces`; no app route can be reached under `public/`, `instances/`, `v1/` or `schema/`. The top-level order in `plane/urls.py` is therefore safe, and the groups can be mounted independently in the Rust router.


## Unknown permission decorators

None. Every `allow_permission` wrapper's arguments were read from its closure.
