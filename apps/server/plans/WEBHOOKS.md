# Webhooks — port plan (read-only)

Area: `plane/app/views/webhook/base.py` (`WebhookEndpoint`, `WebhookSecretRegenerateEndpoint`,
`WebhookLogsEndpoint`) and the `webhooks` selectors in `deploy/route-groups.yaml:225-229`. The GETs
are already Rust (`crates/plane-http-app/src/webhooks/`); this plan covers the write verbs. No
`/api/v1/`, `/api/public/` or `/api/instances/` route touches webhooks.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/`. "Owner" = `ROUTES.md`'s Owner cell.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 228 | `webhooks/` | POST; PATCH, DELETE are the kwarg 500 | rust(GET) | A (POST), B (PATCH, DELETE) |
| 229 | `webhooks/{pk}/` | POST (same 500), PATCH, DELETE | rust(GET) | A (POST), B (PATCH, DELETE) |
| 230 | `webhooks/{pk}/regenerate/` | POST | django | A |
| 231 | `webhook-logs/{webhook_id}/` | none (GET only) | rust(GET) | — |

(`ROUTES.md:318-321`; `routes.json` orders 228–231; `plane/app/urls/webhook.py:14-30`.) **Whole row:
230.** 228/229 flip to `rust(GET,POST)` and `rust(GET,PATCH,DELETE)` once A and B land: the Caddy
matcher comes from the Owner cell and matches **GET only** today
(`deploy/caddy/rust-routes.caddy:445-450`, `tools/proxy/gen_caddy.py:13-14`), so nothing 501s in the
meantime.

`routes.json` maps **all four verbs onto both 228 and 229**, but `post` takes no `pk` (`base.py:22`)
while `patch`/`delete` require it (`:82`, `:108`), and DRF passes the URL kwargs to the handler
(`rest_framework/views.py:517-523`). POST on 229 and PATCH/DELETE on 228 are therefore a `TypeError`
→ family 500 `{"error": "Something went wrong please try again later"}`
(`plane/app/views/base.py:330-334`); `allow_permission` wraps the method and runs first
(`permissions/base.py:22-24`), so only an ADMIN reaches the 500 and a MEMBER/GUEST gets the 403. Port
that branch in the handler, not as a 405. 231 has no write: `webhook_logs` rows are written by
`webhook_send_task` (`TASKS.md:569`).

## 2. Shared pieces needed first

**a. Hooks.** H01 only; neither model overrides `save()` (`plane/db/models/webhook.py:34-62`,
`:65-91`; `MODEL_HOOKS.md:1296-1315`, `:2491`). Create = `Audit::on_create`, PATCH/regenerate =
`Audit::on_update` + `write::now()` (`write/mod.rs:70`, `:79-87`). Create **overwrites** a body
`created_by` with the request user and nulls `updated_by` (`plane/db/models/base.py:36-39`); PATCH's
full save writes a body `created_by` verbatim and replaces only `updated_by` (`:40-42`) — the same
writable-FK quirk as `IntakeIssue`. DELETE is H03: `deleted_at = now`, a full save, then
`soft_delete_related_objects.delay("db","webhook",pk,using=None)` (`mixins.py:72-82`; message already
`write::publish_soft_delete_cascade`, `write/mod.rs:123-141`). The worker cascade never runs in the
harness.

**b. URL validation — three field validators, then a save-time check.** `url` is
`serializers.URLField(validators=[validate_schema, validate_domain])` (`serializers/webhook.py:25`).

*Field validators (400 from `is_valid()`, value is an **array**, `url` is second in `self.fields` —
`SERIALIZERS.md:3871-3889`).* `Field.__init__` keeps the declared list and `URLField.__init__`
**appends** Django's `URLValidator`, so `run_validators` accumulates in the order schema → domain →
URLValidator (`rest_framework/fields.py:326-327`, `:806-814`, `:537-559`); `write::url::django_url_valid`
is that validator (`write/url.rs:41`; DRF imports Django's, `rest_framework/fields.py:17-20`).
1. `validate_schema`: scheme not in `["http","https"]` → `"Invalid schema. Only HTTP and HTTPS are
   allowed."` (`webhook.py:21-24`). `urlparse` lowercases the scheme, so `HTTP://x` passes
   (`pyurl.rs:157-166`, `:233-235`).
2. `validate_domain`: `urlparse(value).netloc` **exactly** `"localhost"`/`"127.0.0.1"` → `"Local
   URLs are not allowed."` (`webhook.py:27-31`). `netloc`, case-sensitive, port included, so
   `localhost:8000`, `LOCALHOST`, `user@localhost`, `127.0.0.1:8000` all pass (bug kept).
3. Django's `URLValidator` → `"Enter a valid URL."` (it accepts `ftp`/`ftps`, so `ftp://example.com`
   is the *schema* message).

All three accumulate: `ftp://localhost` → `{"url": ["Invalid schema. Only HTTP and HTTPS are
allowed.", "Local URLs are not allowed."]}`, `"not a url"` → `{"url": ["Invalid schema…", "Enter a
valid URL."]}`. `url` has **no** `max_length` (declared, so `build_field` never applies the model's
1024 — `rest_framework/serializers.py:1129-1131`, `SERIALIZERS.md:3874`), so a 1025–2048-character URL
reaches the `varchar(1024)` INSERT and 500s.

*`_validate_webhook_url` (`serializers/webhook.py:27-55`), called from `create()` (`:69-72`) and from
`update()` only when `url` is truthy (`:74-78`) — inside `serializer.save()`, after `is_valid()`.* Both
failures are `ValidationError({"url": "…"})` raised from the view's frame, so the body is a **bare
string**, not a list (verified).
1. `validate_url(url, WEBHOOK_ALLOWED_IPS, WEBHOOK_ALLOWED_HOSTS)`
   (`plane/utils/ip_address.py:159-196`); any `ValueError` becomes `"Invalid or disallowed webhook
   URL."`, the original message discarded (`serializers/webhook.py:35-37`). Steps: no hostname →
   error; scheme not http/https → error; hostname in ALLOWED_HOSTS → **return, no DNS** (`:190-194`);
   else `resolve_and_validate` → `getaddrinfo` failure (`gaierror`/`UnicodeError`) → "Hostname could
   not be resolved" (`:136-141`), empty → "No IP addresses found", per address (zone id stripped,
   `:149`) `is_blocked_ip` → "Access to private/internal networks is not allowed" (`:151-152`).
   `is_blocked_ip` = private/loopback/reserved/link-local/multicast/unspecified plus the ten
   `_BLOCKED_NETWORKS` (`:16-30`) plus embedded-IPv4 recursion (mapped, 6to4, Teredo, NAT64,
   `:33-64`). Rust: `plane_net::ssrf::{validate_url, resolve_and_validate}` (`ssrf.rs:180-197`,
   `:153-176`), `SsrfPolicy::from_settings` (`:77-79`), `dns::SystemResolver` (`dns.rs:63-87`),
   `pyurl::{urlparse, hostname, netloc}` (`pyurl.rs:105-125`, `:342-346`).
2. `hostname = (urlparse(url).hostname or "").rstrip(".").lower()` (`:39`); in `WEBHOOK_ALLOWED_HOSTS`
   → return (`:41-46`).
3. `disallowed = list(WEBHOOK_DISALLOWED_DOMAINS) + [request.get_host().split(":")[0].rstrip(".").lower()]`
   (`:48-52`; `get_host()` is `Host` verbatim — no `USE_X_FORWARDED_HOST`, `plane/settings/common.py:95`);
   `hostname == d or hostname.endswith("." + d)` → `"URL domain or its subdomain is not allowed."`
   (`:54-55`).

Settings: `Settings.webhook` (`plane-core/src/config.rs:798-802`, `:707`; `ENV.md:118-120`). A PATCH
that omits `url` never re-validates; one that repeats it resolves DNS again.

**c. Secret generator (new, `plane-core`).** `"plane_wh_" + uuid4().hex` (`webhook.py:17-18`): 41
characters, `os.urandom(16)`, 122 bits. Model default callable (`:38`), `max_length=255`, no
uniqueness (`MODEL_HOOKS.md:2424`), read-only in the serializer (`serializers/webhook.py:83`);
regenerate assigns a fresh one and full-saves (`base.py:118-119`).

**d. Response shapes — two, both different from GET's 10 keys.**
* create (201, `base.py:31`) and regenerate (200, `:120-121`) use `show_secret_key=True` and **no**
  `fields=`, so 17 keys in declaration order: `id, url, created_at, updated_at, deleted_at, is_active,
  secret_key, project, issue, module, cycle, issue_comment, is_internal, version, created_by,
  updated_by, workspace` (`SERIALIZERS.md:3871-3889`; `secret_key` is popped only without the flag,
  `serializers/webhook.py:57-67`). Create: `created_by` = caller, `updated_by`/`deleted_at` null,
  `version` `"v1"`; regenerate: both audit columns = caller.
* PATCH (200, `:104`) passes the fixed 10-name tuple (`:89-101`), so it answers exactly what GET does
  (`webhooks/base.rs:84-113`), from the in-memory instance after the save. Reuse: lift
  `Webhook`/`Webhook::decode`/`WebhookValue` in `webhooks/base.rs` to `pub(super)` and add a 17-key
  `WebhookFull` renderer over `plane_db::models::Webhook` (`models/webhook.rs:117-242`, whose
  `COLUMNS`/`from_row` are exactly those 17 columns).

**e. `write::generated`.** Add `plane.app.serializers.webhook.WebhookSerializer` to
`WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939-1968`); it is `codegen: manual`
(`SERIALIZERS.md:3865-3869`). The generator classifies `URLField` as `char`
(`tools/inventory/serializers.py:2072`), which would make `validate()` accept any string: add a
`"url"` kind (`FIELD_KIND_VARIANTS`, `:2004-2025`) so the field falls into the `// TODO(url)` arm
(`:2404-2407`), and add `Serializer::url_field(name, UrlOptions)` to `write/serializer.rs` (it needs
the private `field_value`, `:1070-1075`): trim/blank via `fields::char`, then the declared validators
in §2b order with Django's `URLValidator` last, each failure pushed with `push_error` (`:1141-1145`).

**f. Harness normalisation.** `shapes.webhook_secret` = `^plane_wh_[0-9a-f]{32}$` (fixture secrets
match: `plane_wh_` + `stable(name).hex`, `tools/fixtures/build.py:136-137`, `:2692`), then **two**
`mode: placeholder`, `placeholder: webhook-secret`, `request: "*webhooks*"` entries — one in
`volatile_json_keys`, one in `volatile_db_columns` (`scenarios/normalize.yaml:34-106`, `:332-411`,
`:413-479`): the create diff is a full-row insert, the regenerate diff a column change
(`plane-testkit/src/db.rs:479-503`), and request-scoped DB rules are supported
(`crates/plane-testkit/src/normalize.rs:2158-2209`). `placeholder_if_new` cannot be used — it keys
off fixture UUIDs (`crates/plane-testkit/src/normalize.rs:991-1016`).

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | none | `webhooks/validate.rs` (§2b), `webhooks/render.rs`, `webhooks/write_queries.rs` (`WEBHOOK_INSERT`, `WEBHOOK_UPDATE`, `WEBHOOK_BY_PK_FULL`) + `ALL_QUERIES`, the `plane-core` secret, `url` kind + `Serializer::url_field`, `write/generated/webhook_serializer.rs`, the two `normalize.yaml` rules, `schema_check::catalogs()`, `scenarios/write/webhooks-fixture.sql` | ~3 h |
| **A — create + regenerate** | 228 POST, 229 POST (500), 230 POST | `webhooks/write.rs` (`create`, `regenerate`), two `register` lines, `scenarios/write/webhooks-create.yaml` | ~2 h |
| **B — PATCH + DELETE** | 228 PATCH/DELETE (500), 229 PATCH/DELETE | `webhooks/write_patch.rs` (PATCH + DELETE), two `register` lines, `scenarios/write/webhooks-patch.yaml` | ~2.5 h |

Each set adds its own `.method(...)` lines in `webhooks/base.rs::register` (`:42-50`) — a two-line
merge, nothing else shared. Both scenario files take the same `fixture_sql` and the same
`target_env`. Flip both Owner cells in one edit at the end; they are one group.

## 4. Risks

- **DNS is a side effect of validation.** `create()` resolves the URL's host (`ip_address.py:196`), and
  so does any PATCH that repeats `url`. Offline, a would-be-successful create becomes `{"url":
  "Invalid or disallowed webhook URL."}` — a diff, not a port failure. Every scenario must set
  `target_env: WEBHOOK_ALLOWED_HOSTS: "hooks.example.test"` so the fixture's three URLs
  (`tools/fixtures/build.py:2682-2684`) and the new ones validate without DNS; `target_env` is merged
  into both targets at start (`crates/plane-testkit/src/config.rs:162-175`, `:302`). Rejections stay
  deterministic: `http://localhost:8000/x`
  and `http://127.0.0.1:1/x` resolve via `/etc/hosts` and are blocked; `ftp://localhost` fails in the
  field validators. The **domain** message is *not* reachable offline (it needs a resolvable public
  host, and the one hostname that resolves without DNS is skipped by the allowed-host early return,
  `serializers/webhook.py:45-46`). Never add `WEBHOOK_ALLOWED_IPS` to that scenario — it would make
  the loopback rejection steps succeed.
- **Unique constraint → 409; the same conflict on PATCH → 400.** Live-row uniqueness is the partial
  index `webhook_url_unique_url_when_deleted_at_null` (`webhook.py:56-62`); `unique_together` includes
  nullable `deleted_at` (`:51`) and never fires for live rows, and DRF adds no
  `UniqueTogetherValidator` because `workspace` is read-only, hence absent from `field_sources`
  (`rest_framework/serializers.py:1660-1686`). POST catches it (`base.py:33-39`): `"already exists" in
  str(e)` is true only because psycopg3's message is libpq's full text with the `DETAIL:` line
  (`psycopg/errors.py:553-556`) → **409 `{"error": "URL already exists for the workspace"}`**;
  anything else re-raises into the family **400** `{"error": "The payload is not valid"}`
  (`views/base.py:306-310`). PATCH has no try/except, so the same conflict is 400 there. Port the
  substring test on `as_db_error().detail()` — precedent `states/write.rs:571-590`.
- **Write order, no transactions.** No `atomic` in the view. POST: permission → `Workspace.objects.get`
  (404) → validate (400) → SSRF (400) → INSERT (409/400) → 201. Regenerate: permission → get (404) →
  new secret → full save → 200. PATCH: permission → get (404) → validate (400) → SSRF → full save →
  200. DELETE: permission → get (404) → soft delete → publish → 204. The body is parsed after the
  lookup, so a malformed body on a missing pk is 404; a bad `url` is 400 **before** anything is
  written.
- **Manager predicates.** `Webhook.objects` filters `deleted_at` (`mixins.py:56-58`), so a
  soft-deleted or other-workspace pk is the family 404 `{"error": "The required object does not
  exist."}`; `webhook-logs` joins `workspaces` unfiltered (`webhooks/queries.rs:75-90`). The
  permission join is unfiltered on `workspaces` on purpose (`plane-permissions/src/queries.rs:38-43`),
  so a member of a **soft-deleted** workspace passes the role check and POST's `Workspace.objects.get`
  then 404s, while PATCH/GET answer 200. Port as-is.
- **Permissions.** `allow_permission([ROLE.ADMIN], level="WORKSPACE")` on all six methods
  (`base.py:21,41,81,107,115,125`; `permissions/base.py:59-66`): no `creator=` escape hatch, so a
  MEMBER is 403 even for their own webhook, and a non-member is 403 for any pk *including a missing
  one*. The runtime already enforces this from `routes.json`; no handler work.
- **Contract coverage.** Only `apps/api/plane/tests/contract/app/test_webhook_secret_key_scope_app.py`
  (6 tests: list/retrieve/PATCH hide `secret_key`, regenerate reveals a different one, create reveals
  one, serializer default). Its create test monkeypatches `plane.app.serializers.webhook.validate_url`
  (`:86`), which has no Rust analogue, so it replays only with `WEBHOOK_ALLOWED_HOSTS` set. Run
  `tools/contract/run.sh --assume-rust 228,229,230 --methods POST,PATCH,DELETE`
  (`tools/contract/run.sh:16-21`). Nothing covers DELETE, the 409, the 404s, the URL error shapes or
  the kwarg 500s: the scenarios are the evidence.
- **Fixture.** `plane_fix_tpl` has three webhooks (active/inactive/internal, URLs on `*.example.test`,
  all `created_by = admin`), 4 logs and 2 `project_webhooks` (`tools/fixtures/build.py:2672-2740`);
  none is soft-deleted and `ws2` has none. Add by `fixture_sql`: a soft-deleted row with the same
  `(workspace, url)` as a live one (proves the partial index and that a re-create is 201), a `ws2` row
  with the active one's URL (per-workspace uniqueness) and a row with `created_by_id` NULL. Steps:
  create empty body (400 `{"url": ["This field is required."]}`), minimal, full with read-only
  `id`/`workspace`/`secret_key`/`deleted_at` in the body, every message in §2b, duplicate → 409, PATCH
  `{"url": <same>}`, PATCH `{"created_by": <other user>}`, PATCH a missing pk, DELETE then GET list,
  DELETE twice, POST on 229 and PATCH/DELETE on 228 (the 500s), MEMBER and GUEST (403), foreign
  `Origin` (403). Capture the created id with `capture: {source: db, table: webhooks, column: id}`,
  never from the response, since `secret_key` is placeholdered.
