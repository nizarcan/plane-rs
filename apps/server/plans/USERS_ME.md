# Users/me — port plan (read-only)

Area: `plane/app/views/user/base.py` (`UserEndpoint`, `ProfileEndpoint`, `AccountEndpoint`,
`UpdateUserOnBoardedEndpoint`, `UpdateUserTourCompletedEndpoint`), `plane/app/views/api.py`
(`ApiTokenEndpoint`) and the `users-me` selector (`deploy/route-groups.yaml:64-67`, `/api/users/**`).
The GETs are already Rust (`crates/plane-http-app/src/users_me/{user_base,api}.rs`); this plan covers the
write verbs. `/api/v1/users/me/` (order 314) is another group and out of scope.

## 1. Inventory (app routes with write methods)

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 162 | `users/me/` | PATCH:partial_update, DELETE:deactivate | rust(GET) | A (PATCH), B (DELETE) |
| 165 | `users/me/email/generate-code/` | POST | django | B |
| 166 | `users/me/email/` | PATCH:update_email | django | B |
| 167 | `users/me/profile/` | PATCH | rust(GET) | A |
| 168 | `users/me/accounts/` | DELETE (the pk 500) | rust(GET) | A |
| 169 | `users/me/accounts/{pk}/` | DELETE | rust(GET) | A |
| 171 | `users/me/onboard/` | PATCH | django | A |
| 172 | `users/me/tour-completed/` | PATCH | django | A |
| 226 | `users/api-tokens/` | POST; PATCH/DELETE are the pk 500 | rust(GET) | A |
| 227 | `users/api-tokens/{pk}/` | PATCH, DELETE; POST is the kwarg 500 | rust(GET) | A |

(`ROUTES.md:252,255-259,261-262,316-317`; `plane/app/urls/user.py:27-63`; `plane/app/urls/api.py:9-19`.)
**Whole rows: 165, 166, 171, 172.** The rest keep their GET and gain verbs; the Caddy matcher comes from
the Owner cell (`tools/proxy/gen_caddy.py:13-14`), so nothing 501s until the flip.

Two classes serve two patterns each and `routes.json` maps verbs onto both: `AccountEndpoint.delete`
takes `pk` (`user/base.py:436`) but 168 has no kwarg, and `ApiTokenEndpoint.post` takes no `pk`
(`views/api.py:21`) while `patch`/`delete` require it (`:51,56`). DRF passes the URL kwargs to the
handler (`rest_framework/views.py:514-522`), so **168 DELETE, 226 PATCH/DELETE and 227 POST** are a
`TypeError` → family 500 `{"error": "Something went wrong please try again later"}`
(`views/base.py:297-334`) after `IsAuthenticated`. Port that branch in the handler, keyed on
`ctx.kwarg("pk")`, not as a 405 — precedent `workspaces/write_favorite.rs:17-30`.

## 2. Shared pieces needed first

**a. Hooks.** One new: **H30** `User.save` (`db/models/user.py:169-187`, `MODEL_HOOKS.md:450-494,2160`) →
`hooks::user::before_save`: lower/strip `email` (unconditional; `None` → 500), rotate `token =
uuid4().hex+uuid4().hex` **and** `token_updated_at = now` on **every** save while `token_updated_at` is
non-null, `display_name = email.split("@")[0]` when empty, `is_superuser → is_staff`. `users` has no
audit columns, so H01 does not apply; H01/H03 (ported, `hooks/audit.rs`, `write/mod.rs:70,123-141`) cover
`APIToken.objects.create` and `api_token.delete()` (`soft_delete_related_objects("db","apitoken",pk,None)`,
`db/mixins.py:65-77`). **H04** covers the invite delete (`db/mixins.py:48-54`: `UPDATE … SET
deleted_at=now`, no `updated_at`, no cascade). `Profile`/`Account`/`Session` have no hooks
(`MODEL_HOOKS.md:487-494,1418-1431`).

**b. Tasks — all Celery-owned, published byte-identically.** `send_email_update_magic_code(new_email,
token)` (`user/base.py:178`), `send_email_update_confirmation(new_email)` then `(old_email)` (`:270-272`),
`user_deactivation_email(base_host(request, is_app=True), user.id)` (`:370`); args `(str,str)`, `(str)`,
`(str,UUID)` (`TASKS.md:481-483`, sites `:825-843`). Builders exist
(`plane-jobs/src/celery/tasks.rs:714-744`), sent through `common::jobs::dispatch_task`
(`common/jobs.rs:61-100`). `base_host` = `plane_auth::base_host` (`plane-auth/src/login.rs:53-86`; raises
`ImproperlyConfigured`, `ENV.md:253`).

**c. `plane-cache` lacks two calls.** `cache.add(key,0,timeout)` / `cache.incr(key)` are the per-target cap
and the verify budget (`user/base.py:160-161,220-221`); `Client` has get/set/delete/delete_many only
(`plane-cache/src/client.rs:163-205`). Add `add` (`SET … NX PX`) and `incr` (`INCRBY`, **no** TTL reset —
that is why the window starts at the first `add`).

**d. A session-flush channel (P0).** 166 and 162 DELETE call Django's `logout` (`user/base.py:267,373`):
`clear()` (accessed+modified) + `delete()` + `_session_key=None`
(`django/contrib/sessions/backends/base.py:177-190,412-419`), then `SessionMiddleware` deletes the
`session-id` cookie and adds `Vary: Cookie` (`authentication/middleware/session.py:53-66`; Rust
`bins/plane-api/src/middleware/session.rs:58-71`). `plane_auth::SessionStore::flush` reproduces it
(`session.rs:525-537`) but **no handler can reach the store**: `ViewCtx` has no session
(`plane-drf/src/view.rs:51-75`) while dispatch owns `&mut SessionStore` (`dispatch.rs:85`). Add a shared
handle (`ViewCtx.session: Option<&'a SessionSlot>`, `Mutex`-wrapped, seen by middleware and handler) and a
`DELETE FROM sessions WHERE user_id=$1` statement for the deactivate path (`user/base.py:342`; the column
is `varchar(50)`, compare `str(uuid)`).

**e. Two validators, both new.** Django's `validate_email` (only the boolean is observable:
`{"error": "Invalid email format"}`, `user/base.py:120-126`), and `contains_url`
(`plane/utils/url.py:15-53`: one `(?i)` alternation of `https?://\S+`/`www.…`/dotted name/IPv4, `search`
per line, `len>1000`→`False`, lines truncated to 500) used by `validate_first_name`/`validate_last_name`
(`serializers/user.py:16-24`). The generated `validate` covers `to_internal_value` only: run these two in
the handler and re-sort with `ValidationErrors::sort_by_field_order(&UserSerializer::FIELDS)`
(`write/serializer.rs:267-276`).

**f. `write::generated`.** Add `…user.UserSerializer`, `…user.ProfileSerializer`, `…api.APITokenSerializer`
to `WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1938-1968`) and regenerate. The first is real
work: `avatar_asset`/`cover_image_asset` are `primary_key_field_live(…"file_assets")` (FileAsset's default
manager filters `deleted_at`, `write/serializer.rs:833-867`), `cover_image` is `url_field`
(`serializer.rs:603`), `user_timezone` a 433-value `choice_field`. `APITokenSerializer` has five writable
fields, **including `deleted_at`, `created_by`, `updated_by`** (`SERIALIZERS.md:1564-1580`).

**g. Response shapes (six).**

* 162 PATCH → `UserSerializer(instance).data`, 39 keys in declaration order, `last_login` first
  (`SERIALIZERS.md:3761-3805`; struct at `plane-serializers/src/generated/app_user.rs:807-1010`, unused
  today). `BaseSerializer` is not `DynamicBaseSerializer`, so `?fields=`/`?expand=` are ignored
  (`serializers/base.py:87-89`) — `FilterMode::None` is right.
* 165 POST → `{"message": "Verification code sent to email"}`, 200 (`user/base.py:180-183`).
* 166 PATCH → `UserMeSerializer(user)`, 18 keys (`serializers/user.py:77-101`; reuse
  `user_base.rs:188-210`) rendered **after** the save, so `avatar_url`/`cover_image_url` still need
  `load_asset` (`user_base.rs:116-126`).
* 167 PATCH → `ProfileSerializer`, 29 keys, `id` first (`SERIALIZERS.md:3652-3686`, `app_user.rs:187-…`),
  from the in-memory row.
* 171/172 → `{"message": "Updated successfully"}`, 200 (`user/base.py:396,404`).
* 226 POST → `APITokenSerializer`, 15 keys **with `token`**, 201 (`SERIALIZERS.md:1564-1580`,
  `app_api.rs:306`); 227 PATCH → the same 15 (the read serializer hides `token`, `serializers/api.py:28-38`,
  the write one does not); 169 DELETE / 227 DELETE → 204.

**h. Reads and their keys.** Nothing here invalidates a cache. The GETs load `request.user` per request
(`plane-auth/src/queries.rs:78-87`, no user cache in either backend) and keep
`Cache-Control: private, max-age=12` + `Vary: Cookie` (`user_base.rs:84-95`). The only Redis keys are
throttle counters, **already wired** for 165/166 (`plane-drf/src/policy.rs:322-333`,
`plane-throttle/src/classes.rs:14-15`): `:1:throttle_email_verification_<pk>` 3/hour,
`:1:throttle_authentication_<ident>` 10/minute (`rate_limit.py:26-45,99-115`).

**i. Harness normalisation (P0, `plane-testkit` + `normalize.yaml`).** (1) New shape
`plane_api_token = ^plane_api_[0-9a-f]{32}$` (`db/models/api.py:19`); the fixture's tokens are 64 hex and
already match `api_token` (`tools/fixtures/build.py:2755-2775`, `normalize.yaml:54-57,350-354`), the
created one does **not** — and a name-keyed `token` rule would replace the `users.token` rule, the maps
holding one rule per name/column (`plane-testkit/src/normalize.rs:336-338`). Add **path-scoped** rules
(reuse `UnorderedPathRule`'s path machinery, `normalize.rs:177-200`) for `$.token`/`$.label` on
`POST /api/users/api-tokens/`, plus request-scoped `api_tokens.token`/`label` DB rules (no rule today,
`normalize.yaml:449-453`). (2) The 6-digit code (`user/base.py:170`) is random in the Celery arg and in
the pickled `{"token":…}` value: one request-scoped text-pattern rule over `normalize_text`
(JSON/celery/Redis/DB strings all pass through) for `POST …/generate-code/`. (3) 166 needs a **known**
code and `capture.source` has no Redis (`config.rs:422-438`), so seed it — `fixture_redis` beside
`fixture_sql` (`config.rs:440-450`), pickled as `plane-cache/src/pickle.rs` does. (4) The deactivate step
must **not** declare `password_vars`: `set_password(uuid4().hex)` (`:361`) is a random plaintext and, with
an empty `passwords` list, `apply_password` renders `pbkdf2_sha256$<iterations>$<hash>` on both sides
while still comparing algorithm and iterations (`normalize.rs:1226-1266`).

## 3. Split

**Orchestrator revision.** H30 `hooks::user::before_save` is owned by the assets P0
(`plans/ASSETS.md`, orchestrator revision). This P0 starts after that merge and **consumes** H30;
drop `hooks/user.rs` from the P0 row below. The P0 is listed at ~1.5 d: split it if it runs past
~3 h (report early, as the conventions say).

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | none | `hooks/user.rs` (H30) + the `hooks/mod.rs` table row; `plane-cache` `add`/`incr`; `SessionSlot` + the `sessions` delete-all; `write/email.rs` (+`contains_url`); `write/generated/{user,profile,api_token}_serializer.rs` + 3 `WRITE_SERIALIZERS` entries; query catalogs (`USER_UPDATE` 40 cols, `PROFILE_UPDATE` 29, `API_TOKEN_INSERT`/`BY_PK`/`UPDATE`, `INSTANCE_ADMIN_EXISTS`, the two membership scans, `INVITE_DELETE_BY_EMAIL`) into `ALL_QUERIES` + `tests/schema_check.rs::catalogs()`; the §2i harness work | ~1.5 d |
| **A — profile, accounts, tokens** | 162 PATCH, 167 PATCH, 168 DELETE(500), 169 DELETE, 171, 172, 226 POST (+PATCH/DELETE 500), 227 PATCH, DELETE (+POST 500) | `users_me/write_profile.rs` (162/167/171/172), `users_me/write_tokens.rs` (226/227), the 169 `delete` + 168 500 branch in `user_base.rs`, `pub(super)` lifts of `profile_serializer`/`account_serializer`, two `register` blocks, `scenarios/write/users-me-profile.yaml`, `users-me-tokens.yaml` | ~4 h |
| **B — email change, deactivation** | 165, 166, 162 DELETE | `users_me/write_email.rs` (165/166), `users_me/deactivate.rs`, the `deactivate` action in `user_base.rs`'s `UserEndpoint` block, `scenarios/write/users-me-email.yaml`, `users-me-deactivate.yaml` (+ `-fixture.sql`, `-fixture.redis.json`) | ~5 h |

Each set adds `register`-only lines (`users_me/user_base.rs:63-82`, `users_me/api.rs:27-32`) — a two-line
merge. A and B wait only on P0 and share nothing else. Flip all ten rows in one edit at the end. With one
agent free, B folds into A (~8 h).

## 4. Risks

- **Order, no transactions.** No view opens `transaction.atomic` and `ATOMIC_REQUESTS` is unset
  (`HANDLERS.md` §4), so a late failure keeps earlier writes. 162 PATCH: permission → `get_object` =
  `request.user` (no query, `user/base.py:69-70`) → body 400 → validate 400 → `save()` → render; 165/166
  read the body **first** (`:151,198`). 166: validate → code required → `add`+`incr` → `get` → compare →
  re-check address → `save()` → `cache.delete(cache_key)` (**not** the attempts key) → `logout` → two
  publishes → render. 162 DELETE: admin check 400 → two scans → two `bulk_update`s → invites → sessions →
  profile → password → user → publish → `logout` → 204.
- **The two "only admin" 400s are dead code — port as written.** Both annotations are constants: the
  queryset is already `member=request.user`, so `~Q(member=request.user)` is never true, **and**
  `Count(Case(…, default=0))` counts the zeros, so `other_admin_exists` ≥ 1; `Count("id")` over a
  PK-grouped page is 1 (`user/base.py:292-332`). Deactivation always proceeds and no caller can be
  refused. Comment it and let the scenario prove it.
- **Session/cookie effects.** Deactivate deletes **all** of the user's session rows (`:342`) then flushes
  the current one (already gone; `delete()` is idempotent); 166 deletes only the current row. Both answer
  `Set-Cookie: session-id=""; …; Max-Age=0` + `Vary: Cookie`, and that caller's later steps are 401 —
  keep them last or use a dedicated account. `sessions` is not soft-deletable (plain `DELETE`,
  `db/models/session.py:17-27`) and `session_key` is already placeholdered (`normalize.yaml:435-439`), so
  the diff matches — but only if a step consumes exactly one session. Django also swaps `request.user` for
  `AnonymousUser` (`django/contrib/auth/__init__.py:243-258`), observable only in the request logger.
- **Redis keys.** `email_update_issued:<email>` (ttl 3600, first `add` only), `magic_email_update_<user
  id>_<email>` (ttl 600, a **str** of JSON → pickled), `<key>:verify_attempts` (ttl 600, an unpickled
  int). All appear in the Redis diff, TTLs within 2 s (`plane-testkit/src/compare.rs:757,849-855`). The cap
  is per **address**, so one account hits the 3/hour throttle before the 5-code cap; the cap needs two
  accounts.
- **165's cap is a 429, not a 400.** `self.throttled(request, None)` (`:162`) → `Throttled(None)` →
  `{"detail": "Request was throttled."}` with **no** `Retry-After`
  (`rest_framework/exceptions.py:226-244`, `views.py:87-97`). The counter increments even for the refused
  request, and the `try:` below turns cache/broker failures into 400
  `{"error": "Failed to generate verification code. Please try again."}`, not a 500.
- **166's verify budget.** `add`+`incr` happen **before** reading the code (`:219-222`), so a wrong code
  still spends an attempt; `attempts > 5` **or** a mismatch → 400 `{"error": "Invalid verification code"}`,
  and `attempts >= 5` deletes code + counter (`:234-242`). Missing code → 400 `{"error": "Verification code
  has expired or is invalid"}` (`:224-229`). A non-ASCII `code` makes `secrets.compare_digest` raise
  `TypeError`, which the outer `except` turns into 400 `{"error": "Failed to verify code. Please try
  again."}` (`:244-248`) — keep the quirk.
- **`request.data.get(...).strip().lower()` is not defensive.** `{"email": null}` / `{"email": 7}` is an
  `AttributeError` 500, and a list/string body has no `.get`: use `views::write_support::body_get`
  (`views/write_support.rs:28-53`, already `pub(crate)`).
- **167 quirks.** `last_workspace_id` is a plain `UUIDField(null=True)` with no FK, so any UUID is stored
  (then `GET /users/me/settings/` falls to its else branch, `serializers/user.py:140-152`);
  `background_color` is writable and echoed; `start_of_the_week` is a 7-value `ChoiceField`
  ("<value>" is not a valid choice.).
- **171/172 use no serializer.** `profile.is_onboarded = request.data.get("is_onboarded", False)`
  (`:394,402`) reaches Django's `BooleanField.to_python` at save time: `true`/`"t"`/`"True"`/`"1"`/`1` are
  true, `"0"`/`"f"`/`false` are not, anything else is a `django.core.exceptions.ValidationError` → family
  400 `{"error": "Please provide valid detail"}` (`views/base.py:313-318`); `null` is rejected (column is
  `null=False`).
- **226 POST bypasses its serializer.** `label`/`description`/`expired_at` come from raw `request.data`
  (`views/api.py:22-24`): `null` label/description → `IntegrityError` → 400 `{"error": "The payload is not
  valid"}`, a >255-char label → psycopg `DataError` → 500, an unparseable `expired_at` →
  `ValidationError` → 400 "Please provide valid detail"; PATCH cannot touch `expired_at`, `token`,
  `is_active`, `user_type`, `user` (read-only, `serializers/api.py:15-25`). `label`'s 32-hex default and
  the `plane_api_` token are the two values §2i covers.
- **Token storage is plaintext.** `"plane_api_" + uuid4().hex`, `unique`, no hash
  (`db/models/api.py:19,31`); `APIKeyAuthentication` matches verbatim (`plane-auth/src/api_key.rs:20-35`).
  Port as-is; the 128-bit space makes the `unique` collision a 500, not a retry.
- **Permissions.** All ten routes are `IsAuthenticated` only (`ROUTES.md:252-263,316-317`): the object is
  always the caller, scoped by `user=request.user` in the lookup (`views/api.py:47,52,57`). A foreign
  `Origin` is the dispatcher's 403 `{"detail": "CSRF Failed: origin not allowed"}`
  (`authentication/session.py:56-63`).
- **Contract coverage.** Only `tests/contract/app/test_api_token.py` (17 tests: create human/bot/minimal/
  expiry/401, list, retrieve/delete/patch with 404 and other-user cases, `token` and `user_type`
  read-only, all-method 401), `test_user_owned_fk_scope_app.py:60-96` (avatar/cover owner and entity
  type, unlink) and `test_session_origin_check_app.py:12-31` (foreign origin 403). **Nothing** for 165,
  166, 168, 169, 171, 172 — the scenarios are the only evidence. Run
  `tools/contract/run.sh --assume-rust 162,167,168,169,171,172,226,227 --methods POST,PATCH,DELETE`.
- **Fixture.** `plane_fix_tpl` has 17 users with a profile each (`tools/fixtures/build.py:364-460`), two
  `accounts` (`:2647-2667`), ten API tokens — four live for `admin`/`member`, one expired, one inactive,
  one for `api_bot` (`:2744-2790`) — and `InstanceAdmin` rows for **`admin` and `member` only**
  (`:3001-3016`), so 162 DELETE as either is the 400 branch. Add by `fixture_sql`: a non-instance-admin
  user with a live project membership, a live workspace membership and a matching
  `workspace_member_invites` row; a user with `token_updated_at` set (proves H30's rotation); a
  soft-deleted `api_tokens` row and a `ws2` one (404 scoping); a `files_assets` row for
  `USER_AVATAR`/`USER_COVER` owned by the caller and one owned by another user.
