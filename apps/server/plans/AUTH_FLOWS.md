# Auth flows — port plan (read-only)

Area: `apps/api/plane/authentication/` — 37 routes (`ROUTES.md:443-479`), groups `auth-session`,
`auth-magic`, `auth-oauth` (`deploy/route-groups.yaml:237-277`). Exit check: every flow walked end
to end in a browser against the mixed backend. Every auth row is still `django | not-started`.

## 1. Inventory

`#` is `routes.json`'s order. "plain" = `django.views.View` (Django CSRF applies,
`HTTP_CONTRACT.md:603-626`); "DRF" = `APIView` (CSRF-exempt; Plane's origin check is the only
protection, `plane/authentication/session.py:56-63`). Handlers are in
`plane/authentication/views/`; URLs in `urls.py:49-152`.

| # | path (method) | group | kind / handler | response and side effects |
| --- | --- | --- | --- | --- |
| 352 | `/auth/sign-in/` POST | session | plain `app/email.py:27` | 302 to app base `?next_path=…&error_code=…`; success = session row + `session-id` + rotated `csrftoken` (`utils/login.py:13-27`) |
| 353 | `/auth/sign-up/` POST | session | plain `app/email.py:140` | as 352, plus `users`+`profiles`+`user_notification_preferences` and the invite auto-join |
| 354 | `/auth/spaces/sign-in/` POST | session | plain `space/email.py:26` | 302 to space base+path; **no** `callback`, so no invite join (`:93`) |
| 355 | `/auth/spaces/sign-up/` POST | session | plain `space/email.py:112` | as 354 |
| 356 | `/auth/sign-out/` POST | session | plain `app/signout.py:16` | `last_logout_ip/time`, `logout()` → row deleted, cookie deleted, `Vary: Cookie`; 302 to app base |
| 357 | `/auth/spaces/sign-out/` POST | session | plain `space/signout.py:17` | as 356 via `get_safe_redirect_url` to the space base |
| 358 | `/auth/get-csrf-token/` GET | session | DRF AllowAny `common.py:29` | 200 `{"csrf_token": <64>}`; `csrftoken` cookie (32-char secret) + `Vary: Cookie` |
| 359 | `/auth/magic-generate/` POST | magic | DRF AllowAny+throttle `app/magic.py:36` | 200 `{"key": "magic_<email>"}`; Redis `magic_<email>` (600 s) + `:issued`; publishes `magic_link` |
| 360 | `/auth/magic-sign-in/` POST | magic | plain `app/magic.py:64` | 302; consumes the code, deletes the keys, logs in |
| 361 | `/auth/magic-sign-up/` POST | magic | plain `app/magic.py:136` | as 360; creates the user first |
| 362 | `/auth/spaces/magic-generate/` POST | magic | DRF `space/magic.py:35` | as 359 |
| 363 | `/auth/spaces/magic-sign-in/` POST | magic | plain `space/magic.py:62` | as 360, space base, **no** callback (`:100`) |
| 364 | `/auth/spaces/magic-sign-up/` POST | magic | plain `space/magic.py:122` | as 361, no callback (`:159`) |
| 365 | `/auth/google/` GET | oauth | plain `app/google.py:28` | session `host`/`next_path`; `state`; 302 to Google |
| 366 | `/auth/google/callback/` GET | oauth | plain `app/google.py:62` | state+code check, provider calls, login, 302 |
| 367 | `/auth/spaces/google/` GET | oauth | plain `space/google.py:26` | stores `host` only — **no** `next_path` |
| 368 | `/auth/spaces/google/callback/` GET | oauth | plain `space/google.py:58` | 302 to space base; no invite join |
| 369 | `/auth/github/` GET | oauth | plain `app/github.py:27` | as 365 |
| 370 | `/auth/github/callback/` GET | oauth | plain `app/github.py:61` | as 366 |
| 371 | `/auth/spaces/github/` GET | oauth | plain `space/github.py:26` | as 367 |
| 372 | `/auth/spaces/github/callback/` GET | oauth | plain `space/github.py:58` | as 368 |
| 373 | `/auth/gitlab/` GET | oauth | plain `app/gitlab.py:27` | as 365 |
| 374 | `/auth/gitlab/callback/` GET | oauth | plain `app/gitlab.py:61` | as 366 |
| 375 | `/auth/spaces/gitlab/` GET | oauth | plain `space/gitlab.py:26` | as 367 |
| 376 | `/auth/spaces/gitlab/callback/` GET | oauth | plain `space/gitlab.py:59` | as 368 |
| 377 | `/auth/email-check/` POST | session | DRF AllowAny+throttle `app/check.py:29` | 200 `{"existing": bool, "status": "MAGIC_CODE"/"CREDENTIAL"}` |
| 378 | `/auth/spaces/email-check/` POST | session | DRF `space/check.py:29` | identical body |
| 379 | `/auth/forgot-password/` POST | session | DRF AllowAny+throttle `app/password_management.py:45` | 200 `{"message": …}`; publishes `forgot_password(…, base_host(is_app=True))` |
| 380 | `/auth/reset-password/{uidb64}/{token}/` POST | session | plain `app/password_management.py:99` | 302 to `app/accounts/reset-password?…`; success → `sign-in?success=True`; writes the password |
| 381 | `/auth/spaces/forgot-password/` POST | session | DRF `space/password_management.py:45` | as 379, space base; reads three SMTP keys, checks only `EMAIL_HOST` (`:62-81`) |
| 382 | `/auth/spaces/reset-password/{uidb64}/{token}/` POST | session | plain `space/password_management.py:111` | as 380 with `{space}/accounts/reset-password/?…`; success → space base |
| 383 | `/auth/change-password/` POST | session | DRF IsAuthenticated+throttle `common.py:48` | 200 `{"message": …}`; `set_password` + `user_login` (same-user re-login: new CSRF secret) |
| 384 | `/auth/set-password/` POST | session | DRF IsAuthenticated `common.py:101` | 200 UserSerializer (39 keys); `@invalidate_cache("/api/users/me/")`; `user_login` |
| 385 | `/auth/gitea/` GET | oauth | plain `app/gitea.py:27` | session `host` + validated `next_path`; `state`; 302 |
| 386 | `/auth/gitea/callback/` GET | oauth | plain `app/gitea.py:61` | uses session `host` (`:65`), `urljoin` redirects |
| 387 | `/auth/spaces/gitea/` GET | oauth | plain `space/gitea.py:26` | as 367 |
| 388 | `/auth/spaces/gitea/callback/` GET | oauth | plain `space/gitea.py:61` | as 368 |

The instance-admin auth routes (`/api/instances/admins/…`) are Phase 7; `test_authentication.py`
covers them and they mint their own sessions. `IS_SELF_MANAGED` is a constant `True`
(`plane/settings/common.py:55`) reported only by `/api/instances/`; no auth route reads it or the
`IS_*_ENABLED` flags (only `license/api/views/instance.py:122` does).

## 2. Shared pieces needed first (P0)

**a. Redirect builder.** `validate_next_path` (`plane/utils/path_validator.py:105-154`, incl.
`_contains_suspicious_patterns` `:54-88`), `get_safe_redirect_url` (`:157-201`),
`get_allowed_hosts` (`:91-102`), Django's `url_has_allowed_host_and_scheme`
(`django/utils/http.py:244-304`). Quirks to port: the path is interpolated **unencoded**
(`:182`), so `&`/`#` forge query parameters on an allowed host; an empty allow-list rejects every
URL with a netloc (`:197-201`); `get_redirection_path` returns bare slugs and `"onboarding"`
(`utils/redirection_path.py:14,30,39,43,46`) that `validate_next_path` drops for lack of a leading
`/` (`:132-133`), so "redirect to last workspace" is dead and users land on the app root. Also port
space's `base + validate_next_path()` + `url_has_allowed_host_and_scheme` (`space/email.py:98-103`),
Gitea's `urljoin` (`app/gitea.py:45,76,101`), and the reset views' `urljoin` vs f-string asymmetry
(`app/password_management.py:112`, `space/…:125`).

**b. Error mapping.** `AUTHENTICATION_ERROR_CODES` + `get_error_dict`
(`adapter/error.py:5-94`); the global DRF handler already maps `NotAuthenticated`→401 and
`Throttled`→429 `{"error_code":5900,…}` (`adapter/exception.py:17-34`,
`crates/plane-core/src/error.rs:267-268`). Add an `AuthErrorCode` enum shared by the JSON and
redirect views (payload `email` included, `adapter/base.py:66-70`).

**c. Adapter core.** `sanitize_email` (`adapter/base.py:62-86`), `__check_signup` (`:99-117`:
`ENABLE_SIGNUP=="0"` still allows an invited address), `validate_password` (`:88-97`),
`check_sync_enabled` (`:122-134`), `save_user_data` (`:231-251`), `complete_login_or_signup`
(`:307-406`: deactivated = `is_active==False and last_logout_time is not None`, `is_bot` rejection,
signup gate, user create with `username=uuid4().hex`, `set_password(uuid4().hex)` when
`is_password_autoset`, profile create, sync, `save_user_data`, callback, account upsert), and
`process_workspace_project_invitations` (`utils/workspace_project_join.py:15-74`: two
`bulk_create(ignore_conflicts=True)`, `.delete()` on both querysets = soft delete H04, per-invite
cache invalidation `:33-39`). Signup needs **H31** (`db/models/user.py:298-312`; not ported,
`crates/plane-db/src/hooks/mod.rs:16-36`) and the ported H30 on every save (`:169-187`).

**d. `request.user` and a DB handle on plain views.** `run_plain` passes
`RequestUser::Anonymous` and `db: None` (`crates/plane-drf/src/dispatch.rs:585-604`), so
`/auth/sign-out/` cannot read `request.user.id` (`app/signout.py:20`). Add Django's lazy user
(`AnonymousUser.id is None`, one hash-verified SELECT on first access, as
`plane-auth/src/login.rs:117-140`) plus `ctx.db`. `login`/`logout`/`user_login` already exist
(`login.rs:105-174,194-244`; `apply_logout` `dispatch.rs:727-733`; `SessionSlot`
`plane-auth/src/session.rs:591-651`).

**e. Magic-code state.** `provider/credentials/magic_code.py:28-184`: 6-digit
`secrets.randbelow(900000)+100000` (`:87`), `SET magic_<email> '{"current_attempt": 0, "email":
…, "token": …}' EX 600` (`:117`), `:issued` = `INCR`+`EXPIRE 600` **only on the first increment**
(`:40-46,93-100`; cap 4 `:33-34`), `:verify_attempts` incremented **before** the compare with its
TTL clamped to ≥1 from the token's TTL (`:146-149`; cap 5 `:28`), `attempts == MAX` → delete both +
`EXHAUSTED` (`:175-180`), `SIGN_IN`/`SIGN_UP` variant from a `users` lookup (`:123-132`).
`plane-cache` has raw `get_bytes/set_bytes_px/exists/ttl_seconds/del/incr_by_present`
(`crates/plane-cache/src/redis.rs:145-338`) but **no `EVAL`**, so add the Lua script. Keys are
unprefixed and live in the target's own Redis DB (`plane/settings/redis.py:12-14`; redis-py 5.0.4
lets the URL's `/db` win over the `db=0` kwarg — verified).

**f. Password strength.** `is_password_strong` wants `score >= 3` on the first or last 128 chars
(`utils/password.py:8-15`) from Python `zxcvbn==4.4.28` (`apps/api/pyproject.toml:67`), a port of
the Dropbox JS implementation (dictionary/reverse/l33t/spatial/repeat/sequence/regex/date,
`zxcvbn/matching.py:78-89`). The Rust `zxcvbn` 3.1.1 is an independent port, so **prove scores
equal**: build a corpus with the real Python (fixture passwords, `PASSWORD_TOO_WEAK` cases, random,
unicode, >128 chars) and require an identical `score` per case; if any disagree, port the matcher
set instead. It gates signup/change/reset, so a score difference is a behaviour change.

**g. Password-reset token.** `PasswordResetTokenGenerator` (`django/contrib/auth/tokens.py`):
`ts = int((datetime.now()-2001-01-01).total_seconds())` (naive local; process TZ is UTC),
`int_to_base36(ts)`, `salted_hmac("django.contrib.auth.tokens.PasswordResetTokenGenerator",
f"{pk}{password}{login:%Y-%m-%d %H:%M:%S}{ts}{email}")` hexdigest `[::2]`, token `"{ts_b36}-{hash}"`,
valid while `now-ts <= 259200` (`django/conf/global_settings.py:539`); plus
`urlsafe_base64_encode(smart_bytes(pk))` / `urlsafe_base64_decode` + `smart_str`
(`app/password_management.py:38-42,104-105`).

**h. Email tasks — Celery-owned, byte-identical.** `magic_link(email,key,token)`,
`forgot_password(first_name,email,uidb64,token,current_site)`,
`user_activation_email(current_site,user_id)` (`TASKS.md:462,473,480`); builders exist
(`crates/plane-jobs/src/celery/tasks.rs:677-712`), dispatched through `common::jobs`.
`current_site` uses `plane/utils/host.py:17-71`, which **raises** `ImproperlyConfigured` when both
URLs are unset (unlike `plane/authentication/utils/host.py:16-63`); Rust has both
(`login.rs:53-86`).

**i. Instance configuration.** `get_configuration_value` (`license/utils/instance_value.py:17-39`)
is ported (`crates/plane-http-app/src/common/instance_config.rs:75-148`); reuse
`configuration_values`. `Instance.objects.first()` + `is_setup_done` (`license/models/instance.py:22-50`,
ordering `-created_at`) gates 352-361 and 377-379. Flags: `ENABLE_SIGNUP` (`:0` disables,
`adapter/base.py:103`), `ENABLE_EMAIL_PASSWORD` (`:0`, `provider/credentials/email.py:27-38`),
`ENABLE_MAGIC_LINK_LOGIN` (`:0`, `magic_code.py:74-79`), `EMAIL_HOST` (empty = SMTP off, `:67-72`).
Seeded defaults contradict the code (`ENABLE_MAGIC_LINK_LOGIN` seeded `"0"`, read as `"1"`;
`EMAIL_HOST` seeded `""`, `ENV.md:277-279,634`).

**j. OAuth.** `validate_oauth_state` (`utils/oauth_state.py:9-19`: constant-time, delete on success,
**keep** on mismatch), `OauthAdapter` (`adapter/oauth.py:70-132`, incl. the
`filter(user, provider, provider_account_id)` upsert and the swallowed `DatabaseError`), the four
providers (`provider/oauth/{google,github,gitlab,gitea}.py`). There is **no PKCE** — no
`code_challenge`/`code_verifier` anywhere; only `state`. Redirect URIs are
`{"https" if request.is_secure() else "http"}://{request.get_host()}/auth/<p>/callback/`
(google:50, github:62, gitlab:56, gitea:64), i.e. `http` behind the dev proxy. Google/GitHub URLs
are class constants (`google.py:22-23`, `github.py:24-26`); GitLab/Gitea build from
`GITLAB_HOST`/`GITEA_HOST` (`gitlab.py:43-45`, `gitea.py:57-59`). Token/userinfo calls have no SSRF
guard (`adapter/oauth.py:75-96`), so they use the **unguarded** `plane-net` preset (D9: env proxy +
`REQUESTS_CA_BUNDLE`, no pinning). `sync_user_data` and the avatar download/upload
(`adapter/base.py:136-305`; pinned fetch `:156`, size cap `:164`, `FileAsset` `:213-222`) run only
for OAuth with `ENABLE_*_SYNC=="1"`; email/magic pass `avatar: ""` (`email.py:53`,
`magic_code.py:162`), so if P0 overruns the avatar half moves to set C unchanged.

## 3. Split

P0 owns §2a-§2j: `plane-auth/src/auth_flow/{redirect,error,adapter,reset_token,magic,oauth}.rs`, a new
`crates/plane-http-auth/`, the `plane-drf` plain-view user/db change, H31, the `EVAL` helper, and
the zxcvbn corpus under `tools/auth/`. It merges before any set; the sets share nothing else.

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **A — session** | 352-358, 377-384 | `plane-http-auth/src/{email,signout,csrf,check,password}.rs` (app+space per concern), `scenarios/write/auth-session-*.yaml` | ~2 d |
| **B — magic** | 359-364 | `plane-http-auth/src/magic.rs`, `scenarios/write/auth-magic.yaml` + `-fixture.redis.json` | ~1 d |
| **C — oauth** | 365-376, 385-388 | `plane-http-auth/src/oauth/{mod,initiate,callback}.rs`, four provider impls, `scenarios/write/auth-oauth.yaml` + `tools/auth/fake_provider.py` | ~2 d |

**Orchestrator revision.**
- P0 is large. The agent stops at a coherent point after ~3 h and reports. The split
  point, if needed: P0a = §2a–d, g, f (redirect, errors, adapter core + H31, plain-view
  user/db, reset token, zxcvbn corpus); P0b = §2e, h, i, j (magic state + `EVAL`, tasks,
  instance flags, OAuth core). B needs P0b's §2e, C needs §2j; A needs only P0a.
- The contract-runner fix from §5 is a separate small task, not part of P0. It exports
  `EMAIL_HOST` and moves `run.sh`'s default Redis DB off 1, which receives outside writes.
  It is verified with `--backend django` (15 → 0) before the full mixed run.
- Agents never flip `ROUTES.md`; the orchestrator does, as in Phase 5.

Each set registers its own routes (`Registry::django_view` / `api_view`). A is the long pole; with two agents free, split A at 358/377-379 (JSON) vs
380-384 (password writes) — still one merge.

## 4. Verification

The harness compares what these routes change: no redirect following
(`crates/plane-testkit/src/http.rs:75-76`), every header byte-for-byte including `Location` and
`Set-Cookie` (`compare.rs:177-232`), plus the DB diff, the Redis snapshot and the Celery message.
Cookie rules exist by name/shape (`scenarios/normalize.yaml:377-396`: `session-id`,
`admin-session-id`, `csrftoken`), signup's random `users.username/token/password` and
`profiles.background_color` already have rules (`:544-600`), and CSRF tokens must unmask to the
response's own cookie (`:391-396`). `scenarios/auth-session.yaml:1-189` already walks signup →
`users/me` → CSRF failure (200 HTML) → sign-out → wrong/right password → `users/me`; its header says
to replay it against `plane-api` once `/auth/` is ported, which is set A's first job.

- **Redirects**: capture `Location` (`source: header`, or `regex` to strip the origin) and feed it
  to the next step's `path` (`render_vars`, `crates/plane-testkit/src/runner.rs:1276-1310`), so each
  302 is verified and then followed explicitly (sign-in → `/` → workspace is three steps).
- **Magic codes**: the code is random and appears only in the Redis value and the Celery args. Two
  scenarios: *generate* (compare the 200 body and the task, masking the digits with a
  request-scoped `volatile_text_patterns` six-digit rule exactly like the e-mail-change rule,
  `normalize.yaml:538-543`, `plans/USERS_ME.md` §2i) and *sign-in/sign-up* (seed a known
  `magic_<email>` + `:issued` with `fixture_redis`, `crates/plane-testkit/src/config.rs:465-484`).
  `capture` has no Redis source, so one step's generated code cannot feed the next. Both scenarios
  must seed `instance_configurations.EMAIL_HOST` (or `target_env`) or `/magic-generate/` is a 400
  `SMTP_NOT_CONFIGURED` (§5).
- **Reset tokens**: mint one per fixture user at a fixed timestamp with the real
  `PasswordResetTokenGenerator` in the fixture builder, put it in `vars`. Both targets share
  `SECRET_KEY` (`scenarios/targets.yaml`), and the token is valid 3 days, so it verifies on both;
  a freshly generated one would differ by the second.
- **OAuth**: the fake provider must reach both processes. GitLab/Gitea accept `http` and honour
  `GITLAB_HOST`/`GITEA_HOST`, so a local HTTP server + `fixture_sql` updating those two
  `instance_configurations` rows (seeded `NULL`, `ENV.md:292,296`, and a NULL row beats the env var)
  gives full code→token→userinfo→login round-trips for 373-376 and 385-388, app and space. Google
  and GitHub have hardcoded URLs: either (a) intercept via `HTTPS_PROXY` + `REQUESTS_CA_BUNDLE` in
  `target_env` (honoured by `requests` and by D9's unguarded preset) or (b) cover the initiate
  redirects (365/367/369/371) plus unit tests for the claim mapping (`verified_email`,
  `confirmed_at`, primary+verified e-mail, `GITHUB_ORGANIZATION_ID`). (a) is recommended: it is the
  only way to exercise the unverified-e-mail rejections (`google.py:108-112`, `github.py:120-132`,
  `gitlab.py:111-117`, `gitea.py:133-143`) end to end.
- **Browser walk (in-app browser against the dev proxy `127.0.0.1:18080`)**: scriptable today are
  352/353/356, 358, 377/378, 383/384, magic 359-364 (read the code from Valkey DB 1,
  `redis-cli -n 1 get magic_<email>`) and reset 379-382 with a hand-minted link. OAuth is walkable
  for GitLab/Gitea through the fake provider's "Authorize" page, or for all four through the proxy.
  `/spaces/*` exists only for 354/355/357/363/364/368/372/376/378/381/382/388; the space app has no
  change/set-password route at all.

## 5. Risks

- **The 15 unexplained magic-link contract failures are `EMAIL_HOST` being unset.** Exactly 15
  tests in `plane/tests/contract/app/test_authentication.py` need a token or a provider:
  `TestMagicLinkGenerate.test_magic_generate`, both `test_expired_invalid_magic_link` (the provider
  checks SMTP *before* the code, `magic_code.py:67-72`), the four
  `test_magic_code_sign_*`/`*_with_next_path`, the six `TestMagicSignInVerifyAttempts`, the one
  `TestMagicSignUpVerifyAttempts`, and `TestBotUserLoginBlocked.test_bot_magic_sign_in_blocked`.
  `/magic-generate/` answers 400 `{"error_code":5025,"error_message":"SMTP_NOT_CONFIGURED"}`
  (`app/magic.py:51-61`), so their `assert response.status_code == 200` fails. `EMAIL_HOST` is set
  neither by `dev/django-env.sh:45-110` nor by `apps/api` (no `.env` loader; absent from
  `.env.example`), and the run builds its DB with `--nomigrations` (`apps/api/pytest.ini`), so
  `instance_configurations` is empty and `get_configuration_value` falls back to `os.environ`
  (`instance_value.py:19-33`). It reproduces with `--backend django` because it is the runner's
  environment, not Rust. Fix: export `EMAIL_HOST=127.0.0.1` in `tools/contract/run.sh` or
  `dev/django-env.sh` (the task is mocked in those tests) and expect 15 → 0.
- **Rate limits.** `AuthenticationThrottle` is per client IP at `10/minute` from
  `AUTHENTICATION_RATE_LIMIT`, read at **import** time (`rate_limit.py:29-34`); DRF uses it in
  `initial()`, plain views through `@throttle_auth_redirect` (`:62-95`), and a signed-in caller
  shares the anonymous bucket. Key `:1:throttle_authentication_<ip>`. Every contract run defaults
  to Redis DB 15 (`run.sh`; DB 1 receives outside writes), so concurrent runs share the key; set
  `target_env: {AUTHENTICATION_RATE_LIMIT: "1000/minute"}` in auth scenarios.
- **Timing.** Reset tokens bucket by the second and live 3 days; `magic_*` TTLs are 600 s with a 2 s
  redis-diff tolerance; `last_active`/`last_login_*`/`token_updated_at` are `timezone.now()` and must
  sit inside the step window; the `:issued` window starts at the first request and never extends,
  so a pause between generates changes the TTL compared.
- **Case sensitivity.** Every entry point lowercases and strips (`adapter/base.py:73`,
  `views/app/email.py:73,178`), and H30 does it on save (`user.py:170`), but `__check_signup`'s
  invite lookup (`base.py:108`), the post-login join (`workspace_project_join.py:19,42`) and
  `User.objects.filter(email=…)` are exact — keep it.
- **Session fixation / cycles.** `login()` flushes when the session names another user or the hash
  differs, cycles the key when it names none — which is how the OAuth callback keeps
  `state`/`host`/`next_path` — and leaves the key alone on a same-user re-login (`login.rs:117-140`,
  `django/contrib/auth/__init__.py:190-240`). `logout()` flushes and the middleware deletes the
  cookie (`HTTP_CONTRACT.md:343-372`). 383/384 re-login the same user: the session key is unchanged
  but `rotate_token` issues a new CSRF secret, so later steps must re-read the token
  (`auth-session.yaml:96-108`).
- **Open redirects.** The unencoded `next_path=` is parameter smuggling on an allowed host, not an
  off-host redirect; the assembled URL is still checked, and an empty allow-list rejects every
  netloc. Gitea's `urljoin(host, "?"+…)` keeps the session `host` (an absolute URL) as the base.
- **Space vs app.** Space never runs `post_user_auth_workflow` (`space/email.py:93,178`,
  `space/magic.py:100,159`, all four space OAuth callbacks), never computes `get_redirection_path`,
  never stores `next_path` at OAuth initiate, validates with `url_has_allowed_host_and_scheme`
  instead of `get_safe_redirect_url`, and builds reset URLs differently. One shared handler for both
  variants changes five behaviours.
- **Nondeterminism to normalise.** magic code, OAuth `state`, session keys, CSRF secret/token,
  `users.username`, `users.password` salt, `users.token`, `profiles.background_color`, the reset
  token and its timestamp, `last_active`/`last_login_*`, Celery publish time, the fake provider's
  `accounts` tokens. Add *shape* rules; do not widen existing name-keyed rules (one rule per JSON
  name, `normalize.rs:336-338`).
- **Other behaviour to port as-is.** signup pre-checks then inserts, so a race is a 500 (unique
  `users.email`); `complete_login_or_signup` dereferences `self.user_data["user"]`
  (`base.py:355`), so a provider omitting it is a 500; a CSRF rejection on a plain view is **200
  HTML** (`views/common.py:39-45`); `set-password`'s `invalidate_cache` runs after the write;
  `create_update_account` swallows DB errors (`adapter/oauth.py:131-132`); sign-out is idempotent
  because `AnonymousUser.id` is `None` (`signout.py:20-28`); `{"email": null}` is a 500 on
  `magic-generate` (`.strip()`, `app/magic.py:51`) but the `EMAIL_REQUIRED` 400 on `email-check`
  (`check.py:57-68`).
