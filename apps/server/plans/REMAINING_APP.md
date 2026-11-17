# Remaining app surface — port plan (read-only)

Scope: the `ai` group (orders 51, 52), the four methods still held on otherwise-Rust
routes (34, 75, 82, 107), and a sweep of every other `/api/` row whose Owner cell lacks a
write verb. Nothing is `/api/v1/`, `/api/public/`, `/api/instances/` or `/auth/`. "Order"
is `routes.json`'s `order` (= `ROUTES.md`'s first column).

## 1. Inventory

### 1a. The AI assistant (`ai` group, `deploy/route-groups.yaml:231-235`)

| order | path | method | Django, no LLM configured | Rust action |
| --- | --- | --- | --- | --- |
| 51 | `…/{slug}/projects/{project_id}/ai-assistant/` | POST | `400 {"error":"LLM provider API key and model are required"}` | new `api_view` handler |
| 52 | `…/{slug}/ai-assistant/` | POST | the same 400 | same handler, no project half |

`ROUTES.md:141-142`; `plane/app/urls/external.py:13-20`. Both are `BaseAPIView` +
`@allow_permission` (`external/base.py:148-150`, `:184-186`): anonymous → 401
(`views/base.py:70`), non-member/GUEST → 403 `{"error": "You don't have the required
permissions."}` (`permissions/base.py:56-99`), all from `routes.json`; no handler work.

**Configuration lookup.** `get_llm_config` (`external/base.py:76-120`) reads
`LLM_API_KEY`, `LLM_PROVIDER` (call-site default `"openai"`), `LLM_MODEL` (default
`None`) in that order (`:81-96`) through `get_configuration_value`
(`plane/license/utils/instance_value.py:17-39`). With `SKIP_ENV_VAR` at its default
`"1"` (`plane/settings/common.py:401`) that evaluates
`InstanceConfiguration.objects.values("key","value","is_encrypted")` **once**, takes the
first row per key (decrypting when `is_encrypted`, else verbatim) and falls back to
`os.environ.get(key, default)`. Then `SUPPORTED_PROVIDERS.get(provider.lower())`
(`:98-101`), a missing key is log + `(None, None, None)` (`:103-105`), the model
defaults (`:107-109`) and must be listed for the provider (`:111-118`).

**Why the dev stack has no LLM.** `configure_instance`
(`plane/license/management/commands/configure_instance.py:28-38`) seeds every
`instance_config_variables` row from the environment; the AI block is
`plane/utils/instance_config_variables/core.py:216-242` — `LLM_API_KEY` (encrypted;
`encrypt_data("")` is `""`, `license/utils/encryption.py:20-30`), `LLM_PROVIDER` =
`"openai"`, `LLM_MODEL` = `"gpt-4o-mini"`. Provider and model resolve, `api_key` is
`""`, so `:103-105` returns three `None`s and the handler's first statement answers
`400 {"error": "LLM provider API key and model are required"}`
(`:151-157`, `:187-193`). The DB row shadows a shell `LLM_API_KEY` under the default
`SKIP_ENV_VAR`, so the answer never depends on the harness's environment.

**Response shape.** `AppApiView` has no pre-parse hook
(`crates/plane-drf/src/policy.rs:224-231`) and `request.data` is untouched before
`:159`/`:195`, so with no key **every** POST body — empty, list, malformed JSON, no
`task` — is that same 400. That is the entire deterministic surface of both routes.

**Success path (needs a key).** `task = request.data.get("task", False)`; falsy →
`400 {"error": "Task is required"}` (`:159-161`, `:195-197`). Then
`get_llm_response` (`:123-145`) sends `task + "\n" + prompt` to
`openai.OpenAI(api_key=…).chat.completions.create(model=…, messages=[{"role":"user",
"content":…}])` (`:131-134`) at the SDK default base URL or `OPENAI_BASE_URL`
(`openai/_client.py:125-128`, pinned `openai==1.63.2`, `apps/api/pyproject.toml:41`),
600 s read/write/pool, 5 s connect, 2 retries (`openai/_constants.py:10`;
`crates/plane-net/src/preset.rs:330-347`). Exceptions map by **class name** (`:139-145`)
to 500 `{"error": "An internal error has occurred."}` (`:164-168`, `:200-204`). A reply
gives `response` + `response_html` (`\n`→`<br/>`), plus 51's
`project_detail`/`workspace_detail` after `Workspace.objects.get(slug)` and **unscoped**
`Project.objects.get(pk)` (`:170-181`; a foreign project is accepted — port the bug). A
`None` `content` then 500s on `.replace`.

**Deterministic port: yes.** Port the config lookup and both 400s verbatim; the success
path is a real POST through `plane_net`'s `LLM` preset (env proxies, no SSRF pinning,
`preset.rs:330-347`) with `OutboundRequest::header/.body`
(`plane-net/src/client.rs:68-100`), the SDK's `Authorization`, `Accept`,
`Content-Type`, `OpenAI-Organization`/`OpenAI-Project` (from `OPENAI_ORG_ID`/
`OPENAI_PROJECT_ID`) and `X-Stainless-*` headers (`openai/_base_client.py:615-626`,
`openai/_client.py:175-176`), the Python `json.dumps` body, 2 retries and the
class-name mapping. Prove the bytes with a wire golden, never against api.openai.com.

### 1b. The held methods

| order | methods | what Django answers | evidence | Rust registration |
| --- | --- | --- | --- | --- |
| 34 | PUT, PATCH | guard 405 | `routes.json` 34 `refused_methods` = get/put/patch | `guard_only` ×2 |
| 82 | PUT | guard 405 | `routes.json` 82 refused = put | `guard_only` |
| 107 | PUT, PATCH | guard 405 | `routes.json` 107 refused = get/put/patch | `guard_only` ×2 |
| 75 | PUT | **real update**, 200/400/404 | `routes.json` 75 refused = [] | real handler |

The guard (`plane/app/views/base.py:103-198`) refuses a mixin-served action only when
`get_permissions()` is nothing but `IsAuthenticated`/`AllowAny` (`:64`, `:141`). 34/82/107
declare only `IsAuthenticated` and implement neither `update` nor `partial_update`
(`cycle/issue.py:110,224,320`; `issue/comment.py:147-203`; `module/issue.py:97,212,261,338`),
so an authenticated caller gets `405 {"detail": "Method \"PUT\" not allowed."}` — and
because the class permission passes first, **a non-member also gets 405, not 403**. 75
declares `ProjectEntityPermission` (`issue/link.py:28`), a real class, so `:141-142`
returns True and DRF's `UpdateModelMixin.update` runs. `issues/write_link.rs:13-14`
claims the opposite; that comment is wrong and set B must fix it.

**75 PUT step by step.** `get_object()` → `get_queryset()` (membership, active row,
`project__archived_at IS NULL`, `link.py:39-53`) → DRF `get_object_or_404` → 404
`{"detail": "No IssueLink matches the given query."}` (DRF re-raises `Http404` with its
args: `rest_framework/views.py:82-83`, `generics.py:13-21,79-99`). Then a **full**
serializer (`plane/app/serializers/issue.py:560-608`): `url` required (TextField, no
`max_length`), `to_internal_value` prefixes `http://` on the raw value (`:577-583`),
`validate_url` fails as `{"url": {"error": "Invalid URL format."}}` (`:584-592`), a
duplicate `(url, issue_id)` excluding self as `400 {"error": "URL already exists for
this Issue"}` (`:600-608`). `save()` writes only validated keys plus H01, so absent
optional keys keep their value. **No tasks** — neither crawl nor activity
(`link.py:111-126` only in `destroy`). The body is `serializer.data` in memory, not a
re-read, so a PUT setting `deleted_at` still answers 200, unlike PATCH
(`link.py:105-106`).

**Registration.** A method Django registers reaches Rust only with a registered handler
(`crates/plane-drf/src/dispatch.rs:176-200`); the guard then runs from the policy
(`:443-447`), and `guard_only` (`cycles/base.rs:499-517`) keeps the impossible branch
loud. So: 34 → `cycles/mod.rs:59-62` plus `update`/`partial_update`; 82 →
`issues/mod.rs:96-100` plus `update`; 107 → `modules/mod.rs:71-84` plus both; 75 →
`issues/write_link.rs:92-99` plus a real `update`.

## 2. Shared pieces needed first

**a. Multi-key config read (`common/instance_config.rs:63-95`).** The helper loads the
whole `InstanceConfiguration` table once *per key*; Django uses one queryset for all
three. Add `configuration_values(ctx, &[(key, default)])` that loads once and applies the
same first-row/decrypt/`os.environ` fallback; keep the single-key wrapper (unsplash,
`meta/unsplash.rs:160-195`) on top. Pass `"openai"` as `LLM_PROVIDER`'s default.

**b. LLM wire client (`ai/http.rs`).** Preset + `OutboundRequest`, base
`OPENAI_BASE_URL` or `https://api.openai.com/v1`, path `/chat/completions`, SDK headers,
Python-JSON body in kwarg order (`model`, then `messages`), 2 retries, `(text, error)`
mapped exactly as `external/base.py:139-145`; parse the reply with `plane_serde`'s Python
dialect (a non-JSON body is a 500).

**c. Wire golden (`tools/ai/openai_wire.py`).** A stub `TcpListener` +
`OpenAI(api_key="k", base_url=stub, max_retries=0)` recording method, path, ordered
headers and body for each provider/model and a non-ASCII prompt — the
`tools/storage/golden.py` pattern with no database and no Django process. Compare byte
for byte in a unit test; regenerate when the `openai` pin moves (the `X-Stainless-*`
headers carry the SDK version and CPython build).

**d. 75 PUT reuses `issues/write_link.rs`.** `read_body(partial = false)`/`read_url`
already implement full validation (`:422-535`), `LINK_URL_TAKEN_ELSEWHERE` exists
(`write_sub_queries.rs:122-126`) and `read_queries::LINKS` with the pk bound is exactly
`get_object()`'s predicate (`:670-686`). New: the 404 body
(`ApiError::not_found_detail("No IssueLink matches the given query.")`), the merge of
`values` over the row (keeping the body's `metadata` key order — `serializer.data` never
round-trips through `jsonb`) and the in-memory render via `LinkEntry::serializer()`
(`:583-601`). No new SQL.

**e. Scenarios.** `write/ai-assistant.yaml` (no key): admin/member with `{}`, a list, a
malformed body, no `task`; GUEST and another project's member 403; anonymous 401;
unknown slug 403 — all the same 400. `write/ai-assistant-task.yaml` plus a fixture that
flips the row (`UPDATE instance_configurations SET value='sk-test', is_encrypted=FALSE …
WHERE key='LLM_API_KEY'`, precedent `write/workspaces-create-config-fixture.sql:21-23`)
reaches the `Task is required` branch offline because it precedes the HTTP call; never
send a truthy `task`. 75's PUT steps join `write/issue-relations.yaml` (already 74/75),
34's `cycle-issues.yaml`, 107's `module-issues.yaml`, 82's `issue-comments.yaml`. 405
needs no normalisation (32/104 PUT are already green).

**f. Contract coverage: none.** `apps/api/plane/tests/contract` has no `ai-assistant`
test (0 hits in 98 files) and **no PUT at all** (`grep -rn "\.put("` → 0), so every
method here rests on the differential scenarios. Closest: `test_cycle_issue_app.py`
(33/34 POST), `test_module_issue_destroy_app.py` (107 DELETE),
`test_guest_issue_links_subscribers_app.py` (75 GET/PATCH/DELETE),
`test_issue_subresource_scope_app.py` (82 PATCH/DELETE). Run
`tools/contract/run.sh --assume-rust 34,51,52,75,82,107 --methods POST,PUT,PATCH`.

## 3. Split

| set | routes | files | size |
| --- | --- | --- | --- |
| **A — AI** | 51, 52 | `ai/{mod,config,http,queries}.rs`, `common/instance_config.rs`, `tools/ai/openai_wire.py` + golden, `write/ai-assistant*.{yaml,sql}` | ~4 h |
| **B — held** | 34 PUT/PATCH, 75 PUT, 82 PUT, 107 PUT/PATCH | three `mod.rs` blocks, `issues/write_link.rs` (+`update`), steps in four existing scenario files | ~3 h |

Nothing shared; they run in parallel. Flip at the end: 51/52 `rust(POST)`
(`ROUTES.md:141-142`), 34/75/82/107 `rust(GET,PUT,PATCH,DELETE)` (`:124`, `:165`,
`:172`, `:197`), then `tools/proxy/gen_caddy.py --check`.

## 4. Risks

- **The AI success path cannot be verified against Django.** Everything past
  `Task is required` needs a real LLM; only the wire golden and three SDK behaviours
  read from source (`OPENAI_BASE_URL`, retries, class-name mapping) pin it. Leaving
  51/52 on Django is the alternative; the no-key 400 is fully deterministic, so the flip
  is defensible. If the harness ever gains a stub server, `target_env: OPENAI_BASE_URL`
  would make the success path differential, but 51's `cover_image_url` would then need
  the `presigned_url` mode (`DECISIONS.md` D17).
- **SDK drift.** Headers are pinned to `openai==1.63.2`; regenerate the golden with the
  pin. The handler must never read `OPENAI_API_KEY` (it passes the key explicitly,
  `external/base.py:131`), even though the SDK would.
- **34/82/107 determinism.** The 405 runs after authentication and the permission
  classes but before `get_object` and body parsing, so it is independent of body and
  membership; the scenario still needs a non-member step to pin 405-over-403.
- **75 PUT traps.** `get_object()` 404 vs PATCH's family 404; `url` required; writable
  `deleted_at` returned in a 200; no activity/crawl task (the harness compares enqueued
  Celery); archived project → 404 on PUT but 200 on PATCH; in-memory response.
- **A wrong comment** at `issues/write_link.rs:13-14` asserts 75 PUT is guard-refused;
  fix it with set B or the next reader skips a real handler.

## 5. Other app rows still missing a write method

Eleven `/api/` rows have a Rust-subset Owner cell and an unported write verb. All are
already planned and queued in `plans/WORKSPACES_CORE.md`; none is new work here.

| orders | rows | missing | reason |
| --- | --- | --- | --- |
| 188, 189, 190, 191, 194 | `invitations/`, `invitations/{pk}/`, `/api/users/me/workspaces/invitations/`, `invitations/{pk}/join/`, `members/{pk}/` | POST, PATCH, DELETE | `plans/WORKSPACES_CORE.md:15-20` set B, blocked on its P0 (hooks H04/H05/H30, JWT, `write::generated`) |
| 207, 218, 219, 220, 221, 225 | `user-properties/`, `quick-links/`, `quick-links/{pk}/`, `home-preferences/`, `home-preferences/{key}/`, `sidebar-preferences/` | PATCH, POST, DELETE | `plans/WORKSPACES_CORE.md:23-28` set C, same P0 |
| 195 | `members/leave/` | POST (whole row `django`) | `plans/WORKSPACES_CORE.md:20` set B |

Nothing else: the remaining 219 app rows either own every write verb they answer or
answer none (`ROUTES.md:90-323`). v1/public/instances/auth are Phase 7/6, out of scope.
