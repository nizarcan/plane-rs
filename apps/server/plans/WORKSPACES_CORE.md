# Workspace core — port plan (read-only)

Area: `plane/app/views/workspace/{base,invite,member,quick_link,home,user_preference,user}.py`, `plane/app/views/project/base.py:445-472` (identifiers) and the
`workspaces`/`users-me`/`projects` selectors in `apps/server/deploy/route-groups.yaml:64-67,77-101,116-122`. The GETs are already Rust (`src/workspaces/*`,
`src/users_me/workspace_invite.rs`, `src/projects/identifier.rs`); this plan covers every write verb below. No `/api/v1/`, public or instance route touches them.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/` except 186/190; order = `routes.json`, owner = `ROUTES.md`.

| order | path | write methods | group | set |
| --- | --- | --- | --- | --- |
| 186 | `/api/workspaces/` | POST | workspaces | A |
| 187 | `/api/workspaces/{slug}/` | PUT, PATCH, DELETE (GET Rust) | workspaces | A |
| 188 | `invitations/` | POST | workspaces | B |
| 189 | `invitations/{pk}/` | PATCH, DELETE (GET Rust) | workspaces | B |
| 190 | `/api/users/me/workspaces/invitations/` | POST | users-me | B |
| 191 | `invitations/{pk}/join/` | POST (GET Rust) | workspaces | B |
| 194 | `members/{pk}/` | PATCH, DELETE (GET Rust) | workspaces | B |
| 195 | `members/leave/` | POST | workspaces | B |
| 199 | `workspace-themes/` | POST | workspaces | A |
| 200 | `workspace-themes/{pk}/` | PATCH, DELETE | workspaces | A |
| 207 | `user-properties/` | PATCH | workspaces | C |
| 218 | `quick-links/` | POST | workspaces | C |
| 219 | `quick-links/{pk}/` | PATCH, DELETE | workspaces | C |
| 220 | `home-preferences/` | PATCH (the signature 500) | workspaces | C |
| 221 | `home-preferences/{key}/` | PATCH | workspaces | C |
| 225 | `sidebar-preferences/` | PATCH | workspaces | C |
| 138 | `project-identifiers/` | DELETE (GET Rust) | projects | A |

`ROUTES.md:276-281,284-285,289-290,297,308-311,315,228`. All rows are partial; flip each Owner cell and regenerate caddy. **187 PUT is real**, unlike 137: `routes.json`'s
`guard_actions.update` is not refused and `ROUTES.md:277` has no `[405]` (contrast `:627`); only `WorkSpaceBasePermission` guards it (`permissions/workspace.py:32-43`), and the serializer is
`partial=False`, so `name` and `slug` are required. **220 PATCH is the 500**: the row maps `patch` with no `key` while the handler is `patch(self, request, slug, key)` (`home.py:68`), so DRF
raises `TypeError` → family 500 after the decorator; 221 PATCH binds `key` and works (the reverse of GET, where 221 is the 500, `workspaces/home.rs:86-92`). The `uuid` converters on
189/194/191/199/200/218/219 make a non-UUID path segment a 404 before the handler (`bins/plane-api/src/routes.rs:204-270`); only body ids (e.g. 190's `invitations`) can be malformed.

## 2. Shared pieces needed first

**a. Hooks.** Source of truth is the "Ported today" table in `crates/plane-db/src/hooks/mod.rs:16-32` (`MODEL_HOOKS.md`'s status column is stale — `PROGRESS.md:944-946`). H01 `audit` and H03
`soft_delete::delete_instance` exist. Missing: **H04** (138, 190: `UPDATE … SET deleted_at=now`, no `updated_at`, no task; `MODEL_HOOKS.md:2131`), **H05** (187: append `__{epoch}` to the
slug and save only `slug`; `:2132`, `workspace.py:156-176`), **H06** (218: a `project` in the body re-homes the row to that project's workspace; `:2133`, `workspace.py:192-195`), **H30**
(191: `user.save()` lowercases/strips email, fills `display_name`, sets `is_staff`, and **rotates `users.token`** when `token_updated_at` is set; `:2160`, `models/user.py:169-187`). Nothing
else: `WorkspaceMember`, `WorkspaceMemberInvite`, `WorkspaceTheme`, `WorkspaceUserProperties`, `WorkspaceHomePreference`, `WorkspaceUserPreference` and `WorkspaceUserLink` have no save
override (`MODEL_HOOKS.md:529-583`; `workspace.py:198-455`). H01's writable-FK quirk applies where `created_by` stays writable (the theme, link, properties and invite serializers): create
overwrites a body `created_by` with the caller and nulls `updated_by`; update writes a body `created_by` verbatim and replaces only `updated_by` (`models/base.py:36-42`). Every full
`instance.save()` below reruns H01 (the only hook these models have); the queryset paths (190's member update and invite delete, H04 in 138) run none, and 225's
`save(update_fields=["is_pinned","sort_order"])` runs H01 but writes neither `updated_at` nor `updated_by`.

**b. Tasks and the seed rows.** Constructors exist: `workspace_seed(String)` (`crates/plane-jobs/src/celery/tasks.rs:788-795`) and `workspace_invitation(email, Uuid, token, current_site,
inviter)` (`:767-782`); soft deletes use `write::publish_soft_delete_cascade` (`crates/plane-http-app/src/write/mod.rs:118-141`; `tasks.rs:1086-1102`). **186's request writes only two
rows**: `workspaces` (`base.py:123`) then `workspace_members` (`:125-130`, role 20, `company_role` from the raw body, and the model's `view_props`/`default_props`/`issue_props`/checklist
defaults, `workspace.py:22-111,207-213`), then counts, then one `workspace_seed` message (`:133-138`). The other two member writers (190's bulk insert, 191's create) bind the same defaults.
No preferences are seeded by create; 220/221/225/207 create their rows lazily on GET (`home.py:45-57`, `user_preference.py:39-61`, `user.py:277-279`), already ported. When the worker runs
`workspace_seed` (`workspace_seed_task.py:505-571`) it creates, in order: bot `User` (`:523`), bot `WorkspaceMember` (`:536`), `Project` + `ProjectMember` + `ProjectUserProperty`
(`:72-174`), `State` (`:177`), `Label` (`:212`), `Cycle` (`:392`), `Module` (`:443`), `Issue` + `IssueSequence` + `IssueActivity` + `IssueLabel` + `CycleIssue`/`ModuleIssue` (`:246-343`),
`IssueView` (`:478`), `Page` + `ProjectPage` (`:346-389`); the harness compares the message, not those rows (`DECISIONS.md:365-372`; `TASKS.md:490,573-574`). 188 publishes one `workspace_invitation` per requested
email, including a conflict (`bulk_create` returns every input object); the worker's `WorkspaceMemberInvite.objects.get(token=…)` then misses and the task returns silently
(`workspace_invitation_task.py:28,85-86`). 187/189/191/200/219 publish the cascade with model names `workspace`/`workspacememberinvite`/`workspacetheme`/`workspaceuserlink`.

**c. `write::generated`.** Add to `WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1937-1968`) and regenerate: `WorkSpaceSerializer`, `WorkspaceThemeSerializer`,
`WorkSpaceMemberInviteSerializer`, `WorkspaceUserPropertiesSerializer`, `WorkspaceUserLinkSerializer`, `WorkspaceHomePreferenceSerializer`, `WorkSpaceMemberSerializer` (only `role` is
reachable). The table cannot express: `WorkSpaceSerializer`'s **`UniqueValidator` on `slug`** (message `Workspace with this slug already exists.`, queryset `Workspace.objects`, i.e. live
rows only) plus the model `slug_validator` and SlugField's `^[-a-zA-Z0-9_]+$`; validator order is model validators → UniqueValidator → MaxLength → null/surrogate → regex, and `validate_slug`
(`workspace.py:61-70`) only runs if the field is clean, so its "Slug can only contain…" message is dead (its restricted-list check is also preempted by `slug_validator`); `validate_name`
(`:48-59`) is reachable on PUT/PATCH but its URL branch is preempted on create by `base.py:116-120`. `WorkspaceUserLink`'s `to_internal_value` prefix (`serializers/workspace.py:194-199`),
`validate_url` (Django's `URLValidator`; `write::url::django_url_valid` exists, `write/url.rs:159`) and the duplicate checks in `create`/`update` (`:210-238`) are handler code. Writable FKs
validate against their own queryset with one SELECT: `logo_asset`→`FileAsset.objects` (live), `project`→`Project.objects` (live, any workspace), `created_by`/`updated_by`→`User.objects`
(the generated `primary_key_field[_live]` is that lookup). 188's invalid-email message interpolates the **raw dict** with Python `repr` (`invite.py:105-109`); lift a `repr` (plane-net already matches CPython: `crates/plane-net/src/pyurl.rs:617`) rather
than `write::fields`' unescaped one.

**d. Response shapes.** 186: **17 keys** — `serializer.data` (15: `id, logo_url, created_at, updated_at, deleted_at, name, logo, slug, organization_size, timezone, background_color,
created_by, updated_by, logo_asset, owner`) then `total_members` and `role` **appended**; the `IntegerField`s are absent at that point because the new instance has no annotation
(`base.py:134-136`). 187 PATCH/PUT: **16 keys** — the annotated `total_members` is present, `role` is `SkipField` (read-only, no attribute) (`base.py:64-80,157-159`). 188: `{"message":
"Emails sent successfully"}` (200). 189 PATCH: 14-key invite + nested workspace + `invite_link`; DELETE 204. 190 204 empty. 191: two 200 `{"message": …}`, plus 400/401/403 bare errors. 194
PATCH: 17-key `WorkSpaceMemberSerializer` (nested `UserLiteSerializer`); DELETE 204. 195 204. 199 201 / 200 200: 10-key theme; 200 DELETE 204. 207: 14-key properties. 218 201 / 219 200:
12-key link; 219 DELETE 204. 220 PATCH: family 500; 221 PATCH: 3 keys (`key,is_enabled,sort_order`). 225: `{"message": "Successfully updated"}`. 138 DELETE 204.

**e. P0 (~1.5 d, must merge before A/B/C).** Hooks H04/H05/H06/H30 in `plane-db::hooks` + the `hooks/mod.rs` rows + unit tests (shared file, one owner); the seven generated modules +
`WRITE_SERIALIZERS` (one codegen run) and a `slug` kind with `Serializer::slug_field`/`unique_live` plus the `repr` lift in `write/`; `plane-auth`'s PyJWT-compatible HS256 encoder with a
golden vector (188); `scenarios/normalize.yaml`: an `invite_token` shape, widening the **single** `token` JSON-key rule (a `HashMap`, one rule per name — `normalize.rs:353-357`) to
`api_token | three-part JWT`, and a `workspace_member_invites.token` DB rule scoped to `POST */invitations/`; the `schema_check` `catalogs()` skeleton. No set adds a hook or a `write::`
helper itself.

**f. Determinism.** 188 mints a JWT over `{"email": <raw body dict>, "timestamp": datetime.now().timestamp()}` (`invite.py:95-99`; note the naive-local `datetime.now`), so every create's
token differs. Fixture invitations use 64 hex (`tools/fixtures/build.py:561-563`), which the current global `token` rule (`api_token` shape) accepts. Normalisation order is body → DB →
Celery (`crates/plane-testkit/src/runner.rs:1181-1197`), and the universal placeholder map rewrites the same raw token inside `invite_link` and a Celery positional arg
(`normalize.rs:545-568`). Therefore: the DB rule placeholders the created token before the task is compared; a later GET/retrieve of that invite is safe **only after** the `token` key rule is
widened — the key rule runs before `normalize_text`, so the current `api_token` shape rejects a JWT — and `invite_link` is covered by the universal map. Keep **one invitation per create
step** (placeholder numbering is encounter order, and `db.rs:432-470` iterates inserted rows by random PK). A create that conflicts (`ignore_conflicts`) has no row to placeholder its token, so
do not compare that step. 186's `background_color` is already normalised
(`normalize.yaml:355-358,474-478`) and its generated id/`workspace_seed` argument go through the built-in UUID rule. 191 rotates `users.token` only if the invitee's `token_updated_at` is
non-NULL (fixture users: NULL, `build.py:394-416`). Every timestamp these writers store is `timezone.now()` — UTC with microseconds, in the harness window; no `date.today()` is used here.

## 3. Split

**Orchestrator revision.** H30 `hooks::user::before_save` is owned by the assets P0
(`plans/ASSETS.md`, orchestrator revision); this P0 starts after that merge and consumes it.
`users/me` (`plans/USERS_ME.md`) also adds a `SessionSlot`/cache `add`/`incr` P0 — whichever lands
second reuses the first's pieces rather than duplicating them.

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0** | — | §2e: hooks, generated tables, `slug`/`unique`/`repr` helpers, JWT, `normalize.yaml`, `schema_check` skeleton | ~1.5 d |
| **A — workspace lifecycle, themes, identifiers** | 186, 187, 199, 200, 138 | `workspaces/write.rs` + `write_queries.rs`, `common/instance_config.rs` (`DISABLE_WORKSPACE_CREATION`, lifted from `meta/unsplash.rs:160-204`), `projects/identifier_write.rs`; `scenarios/write/{workspaces,workspace-themes,project-identifiers}.yaml` | ~2 d |
| **B — members, invitations, join** | 188, 189, 190, 191, 194, 195 | `workspaces/invite_write.rs`, `workspaces/member_write.rs`, `users_me/workspace_invite_write.rs` (+ query files), `write/email.rs` (Django `EmailValidator` + golden corpus), `scenarios/write/{workspace-invitations,workspace-members,workspace-join}.yaml` | ~2.5 d |
| **C — preferences, quick links, user properties** | 207, 218, 219, 220, 221, 225 | `workspaces/{home_write,user_preference_write,user_properties_write,quick_link_write}.rs` + queries; `scenarios/write/{workspace-preferences,quick-links}.yaml` | ~1.5 d |

Each set adds its own `.action`/`.method` lines (187 adds `create`/`update`/`partial_update`/`destroy`; the member viewset adds `partial_update`/`destroy` and `leave`) and flips its own `ROUTES.md` cells; 190
flips in users-me and 138 in projects. `gen_caddy.py --check` is the gate.

## 4. Risks

- **Order and transactions.** 186: config → serializer → early `name/slug/length/URL` checks → `is_valid(raise_exception=True)` → INSERT → INSERT member → count → render → task publish
  (`base.py:82-140`); the two INSERTs are not atomic, so a member/task failure leaves the workspace. 187 PATCH: class → decorator → `get_object` → body → save; DELETE: decorator →
  `get_object` → `Profile.update(last_workspace_id=None)` → `super().destroy()` (`get_object` again) → soft delete → cascade publish → slug rewrite (`base.py:161-173`). 188: emails →
  requesting member → role cap → `Workspace.get` → member check → per-email validation/JWT → one `bulk_create` (itself atomic) → `base_host` → N publishes (`invite.py:52-128`). 190: query →
  per-invite cache delete + `WorkspaceMember…update(is_active, role)` → atomic `bulk_create` → queryset soft delete (`:284-311`; a malformed body id is the family 400, `invitations: null` is a 500). 191: invite get → token → 401/403 → one save → membership →
  `user.save()` → invite soft delete → publish (`:173-243`). 194 PATCH is the only handler with an outer `transaction.atomic()` (project-member update + member save, `member.py:130-137`);
  destroy/leave deactivate project members first, then save the member, with no transaction (`:174-179`, `:215-221`). 207's `get_or_create` commits **before** serializer validation
  (`user.py:277-284`), so an invalid PATCH is a 400 that writes a row. 218/219: lookup → serializer → duplicate check/INSERT or full save (`quick_link.py:25-43`). 225 is one two-column
  UPDATE per list element (`user_preference.py:83-101`).
- **Exact cascade rows.** 194 PATCH (demote to guest): every live `project_members` row for `(workspace, member)` — **including `is_active=False` rows** — gets `role=5, updated_at=now`
  (`member.py:132-134`), then the `workspace_members` row writes `role, updated_at, updated_by` (`:137`). 194 DELETE/195: live **active** `project_members` rows get `is_active=False,
  updated_at=now` (`:174-176`, `:215-217`), then the `workspace_members` row is full-saved with `is_active=False`, `updated_at`, `updated_by` (`:178-179`, `:220-221`); the membership row is
  never deleted and no cascade task is published. 194 PATCH refuses the caller's own row (`member.py:101-105`); 194 DELETE refuses self, a higher-role target and a sole project admin
  (`:153-171`); 195 refuses the only active workspace admin and a sole project admin (`:195-212`). 190: `workspace_members` update writes only `is_active`/`role` (no `updated_at`), the insert is `ignore_conflicts`, and the invite delete
  writes **only `deleted_at`** (`invite.py:292,297-311`). 191 accept: invite row (`accepted, responded_at, updated_at, updated_by`) → membership activate (full save) or create → `users` full
  save → invite soft delete (`:201-232`). 189 PATCH/DELETE: invite full save / soft delete. 187 DELETE: `profiles.last_workspace_id` → `NULL` for every profile (raw `.update`, no audit),
  workspace soft delete, then **only `slug`** is written (`base.py:161-173`; `workspace.py:156-176`). 138: `project_identifiers` bulk H04 (`deleted_at` only) (`project/base.py:470`).
- **Sole-admin predicate.** `is_sole_admin_of_any_project` (`member.py:31-43`) is `COUNT(project_members)` over a raw LEFT JOIN with **no `deleted_at`/`is_active` predicate** plus a filtered
  count of `role=20`; one project row is enough (`HAVING`), projects are live only (`Project.objects`), and the workspace's `deleted_at` is not checked. Soft-deleted or inactive project rows
  therefore count toward "sole admin": port the SQL as-is, not a corrected predicate.
- **Post-write errors.** 187's slug rewrite is `f"{slug}__{int(deleted_at.timestamp())}"`; a slug longer than 36 chars exceeds `varchar(48)` **after** the row is soft-deleted and the task
  published → 500 with a deleted row (`MODEL_HOOKS.md:506-516`). A broker failure after a soft delete/insert (186, 187 DELETE, 188, 189 DELETE, 191, 200 DELETE, 219 DELETE) is the family 500
  with the rows committed; Django retries three times before raising (`common/jobs.rs:16-20`). 188's `base_host` runs after the bulk insert (`invite.py:116`), so a missing
  `WEB_URL`/`APP_BASE_URL` is a 500 that keeps the invitations (the dev env sets it, `dev/django-env.sh:77`).
- **Manager predicates / deleted workspaces.** Every `Model.objects` here filters `deleted_at` (`mixins.py:56-58`). Permission joins do **not** filter the workspace's `deleted_at`, and
  189/194/195/200/219 lookups join only `workspace__slug`; a member of a soft-deleted workspace therefore passes the role check and can PATCH/DELETE those rows, while
  187/188-create/207/218-create/220/221/225 re-`get` with `Workspace.objects` and 404. 187's queryset also filters the caller's active membership (`base.py:64-80`), so a non-member is
  403 from the permission class but a deleted-workspace admin reaches a 404. 194 PATCH/DELETE use `member__is_bot=False, is_active=True` (`member.py:98-100,144-146`), so a
  bot/inactive/soft-deleted member is the family 404.
- **Permission quirks.** `initial()` runs `permission_classes` before the handler and before `@allow_permission`: 187/188/189/199/200/207 answer 403 in the class, so their lookups are never
  reached, while 194/195/218/219/220/221/225 answer 403 in the decorator, still before the lookup (`permissions/base.py:14-24`). 186 POST is open to **any** authenticated user (`permissions/workspace.py:22-24`); `DISABLE_WORKSPACE_CREATION` is the only gate (`instance_value.py:17-39`;
  `SKIP_ENV_VAR` default 1 reads the DB, `ENV.md:121`), 403 body `{"error": "Workspace creation is not allowed"}`. 188/189/199/200 use `WorkSpaceAdminPermission` = ADMIN **or MEMBER**
  (`permissions/workspace.py:58-67`), so a member can invite/delete invites/themes; 188/189 cap an invitation role at the caller's own role (`invite.py:62-66,143-148`).
  207/218/219/220/221/225 allow any active member including GUEST (`permissions/workspace.py:77-85`). 191 is `AllowAny` with its own 401 (unauthenticated, **after** the token check) and 403s
  (`invite.py:160,175-197`); its cache decorators run before all of them.
- **Slug/name validation.** On create the early checks return bare `{"error": …}` for missing/long/URL names and shadow the serializer (`base.py:101-120`); a symbol-only name or bad
  timezone/`logo_asset` is the serializer's field error. A **duplicate live slug is 400** (`{"slug": ["Workspace with this slug already exists."]}`) from DRF's `UniqueValidator`, not 409;
  the 409 catch (`base.py:146-151`) is reachable only when a soft-deleted row keeps its slug (the fixture `fix-deleted`, `tools/fixtures/build.py:472,547-551`) or in a race, and its body is
  a **bare string** `{"slug": "The workspace with the slug already exists"}`. The unique index is not partial (`workspace.py:136`). PUT/PATCH have no early checks: >80/>48 is `Ensure this
  field has no more than…`, restricted slugs are `Slug is not valid`, and a duplicate on PATCH is the same 400 (no 409). The `serializer.errors` branch is dead (`raise_exception=True`, `base.py:122,141-144`), and a non-unique
  IntegrityError falls through to an implicit `return None` → 500 (`:146-151`).
- **Body quirks.** Raw-read handlers 500 on a non-mapping body (`AttributeError`): 186, 188, 190, 191 (after the token/email checks), 138; 189/194 return 400 from `isinstance(request.data,
  dict)` before any lookup (`invite.py:132-136`, `member.py:109-113`), so a bad body on a missing object is 400 there but 404 on 187/199/207/218/219 (221 answers its own 400 `Preference not found`
  before the serializer). 218/219's `to_internal_value` calls
  `data.get` → a list body is a 500 (`serializers/workspace.py:194-199`); 221 is a serializer 400 and 220 is always the signature 500. 225 iterates `request.data`: a dict body is a 500 (`str.pop`), a list of non-dicts is a
  500, and `[]` is 200 (`user_preference.py:83-103`). 188 assumes every email is a dict and `role` is `int()`-able (`invite.py:62,74,90`): a string/None element or `role: "abc"` is a 500; an
  invalid email string is 400 with the dict repr; empty/absent `emails` is a bare 400 `Emails are required` (`invite.py:55-56`); an already-active member is 400
  `{"error": "Some users are already member of workspace", "workspace_users": [17-key WorkSpaceMemberSerializer]}` (`:72-85`); `email: 123` is a 500 inside Django's validator. A JWT longer than 255 chars makes the INSERT fail (token `max_length=255`,
  `workspace.py:238`) after no rows are written.
- **191 bugs to pin.** `accepted` is assigned raw and tested for Python truthiness (`invite.py:201,205`), so a form `accepted=false` is the truthy string `"false"`: membership is created
  while the stored `accepted` becomes `False`. If the exact-email `User.objects.filter(email=…)` misses (the pre-check is case-insensitive, the lookup is not), the view still returns 200
  "Accepted" and leaves the invite untouched (`:207-243`). `user.last_workspace_id` is a `Profile` field (`models/user.py:236`), not a `User` field, so the assignment is a no-op and
  `user.save()` only runs H30/`updated_at` (`:228-229`). A rejected invite keeps its row but sets `responded_at`, so a second POST is 400.
- **207/218/225 traps.** 207 creates the row with the model defaults (`display_filters`/`display_properties` are **nested** under their own key, `workspace.py:310-334`) and then full-saves
  the PATCH; 220/221's serializer exposes only `key,is_enabled,sort_order`, so `config` is not writable (`serializers/workspace.py:339-343`); 225 writes only `is_pinned,sort_order` — no
  `updated_at`/`updated_by` (`user_preference.py:101`). A duplicate theme name is an IntegrityError → family 400 (no UniqueValidator: `workspace` is read-only, so DRF skips the
  `unique_together`, `serializers/workspace.py:174-178`). 218's duplicate check uses the URL workspace while H06 may re-home the row to the body project's workspace, so a duplicate in the
  target workspace is not caught (`serializers/workspace.py:210-224`; `workspace.py:192-195`); 219's missing row is `{"detail": "Quick link not found."}` for PATCH but the family 404 for
  DELETE (`quick_link.py:43,56`).
- **Contract coverage.** `apps/api/plane/tests/contract/app/` has only `test_workspace_update_admin_scope_app.py:31-58` (187 PUT/PATCH admin/non-admin, `deleted_at` ignored),
  `test_workspace_member_mass_assignment_app.py:63-180` (194 PATCH guards, guest demotion, sole-admin, self, DELETE refusal) and `test_workspace_invitation_update_app.py:40-67` (189 PATCH
  role cap/body guard). **Nothing** for 186, 188, 190, 191 POST, 195, 199/200, 207 PATCH, 218/219, 220/221, 225 or 138 DELETE; the differential scenarios are the evidence. Run
  `tools/contract/run.sh --assume-rust 186,187,188,189,190,191,194,195,199,200,207,218,219,220,221,225,138 --methods POST,PUT,PATCH,DELETE`.
- **Fixture.** `plane_fix_tpl` has `fix-workspace`/`fix-second`/`fix-deleted`/`fix-nologo`, 18 memberships (inactive + soft-deleted + deleted_member), 3 invitations (64-hex, one for
  `fix.invitee@example.com`, one `new-hire@`, one in ws2; `build.py:564-590`), 2 themes, 3 quick links, properties for admin/member/guest, admin's 5 home + 6 sidebar preferences, and one
  identifier per project. Add by `fixture_sql`: a 40-char-slug workspace + admin membership (the existing soft-deleted `fix-deleted` workspace is the kept-slug 409 case); a project whose only row is
  the target with `role=20` (and a variant with a soft-deleted second row) for the sole-admin predicate; an inactive member row in a second workspace + an invite for admin there (191
  activate branch); an invite with a mixed-case email (the no-user bug) and one with `responded_at` set; an invite for a dedicated user for 190 so 191's fixture invites survive; a theme/link
  in ws2 + soft-deleted rows; a `quick_tutorial`/`new_at_plane` home row for a member (220's PATCH on an excluded key); a ws2 properties row and a soft-deleted one; identifiers for the
  soft-deleted `delta` project and the archived `gamma` project. Sessions exist for every fixture user except `inactive`/`api_bot` (`build.py:3139-3180`).
