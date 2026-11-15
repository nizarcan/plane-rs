# Rust backend rewrite

This document is the brief for rewriting Plane's server side in Rust. It is written for an agent that will work on it over many sessions, so it states the goal, the rules that must not be broken, the order of work, and how to prove each piece is right.

Read this whole file before writing code. Update the progress ledger (see "Working rules") at the end of every session.

## Why

A self-hosted Plane instance for one user sits at roughly 3.5 GB of resident memory while idle. Measured on a running deployment:

| Service | Runtime | Idle memory |
| --- | --- | --- |
| worker (Celery, concurrency 4) | Python | ~1010 Mi |
| api (2 replicas) | Python | ~760 Mi |
| beat-worker | Python | ~350 Mi |
| live | Node | ~225 Mi |
| space (2 replicas) | Node | ~300 Mi |
| rabbitmq | Erlang | ~140 Mi |
| postgres, minio, redis | | ~225 Mi |
| web, admin (static files) | | ~20 Mi |

The Python processes are about 60% of the total. The goal is to replace them with Rust services that idle in tens of megabytes, without changing what the frontends see.

## Scope

In scope, in this order:

1. `apps/api`: the Django application, in all four of its roles (`api`, `worker`, `beat-worker`, `migrator`).
2. `apps/live`: the Node collaboration server. Later phase, with a known hard part (see "Live server").

Out of scope:

- `apps/web`, `apps/admin`, `apps/space` and everything in `packages/`. These are browser code (about 280k lines of TypeScript). `web` and `admin` are served as static files and cost almost nothing at runtime. They are the contract the backend must satisfy, not something to rewrite.
- Merging the services into one binary. The service boundaries stay as they are for now: `api`, `worker`, `beat-worker`, `migrator`, `live` remain separately deployable. Design the crates so a later merge is possible (shared library crates, thin binaries), but do not do it.
- Any commercial-edition feature. This repository is the Community Edition and that is the only reference.

## What exists today

Numbers are from this repository at the commit this file was added. Re-measure before relying on them.

`apps/api` is Django 5.2 with Django REST Framework 3.18 on Python 3.12, served by gunicorn with uvicorn workers.

| Area | Location | Size |
| --- | --- | --- |
| Web app API, mounted at `/api/` | `plane/app` | 234 routes, ~19k lines of views |
| Public REST API, mounted at `/api/v1/` | `plane/api` | 75 routes, ~8.8k lines of views |
| Public "space" API, mounted at `/api/public/` | `plane/space` | 25 routes |
| Auth, mounted at `/auth/` | `plane/authentication` | 37 routes, ~4.7k lines |
| Instance admin, mounted at `/api/instances/` | `plane/license` | 14 routes |
| Models | `plane/db/models` | 86 model classes in 35 files |
| Migrations | `plane/db/migrations` | 125, plus 6 in `plane/license` |
| Background tasks | `plane/bgtasks` | 48 `@shared_task` functions, ~7.5k lines |
| Scheduled jobs | `plane/celery.py` | 13 entries |
| Shared helpers | `plane/utils` | ~11k lines |
| Tests | `plane/tests` | 1106 tests in 142 files (`unit`, `contract`, `smoke`) |

Total is about 123k lines of Python, of which about 23k are migrations and about 25k are tests.

Infrastructure the backend talks to:

- PostgreSQL: the only system of record. Optional read replica (`ENABLE_READ_REPLICA`).
- Redis: cache, rate limiting, and the live server's pub/sub.
- RabbitMQ: Celery broker only.
- S3-compatible object storage (MinIO by default): uploads, via presigned URLs.
- SMTP: outgoing mail.
- Outbound HTTP: webhooks, OAuth providers (Google, GitHub, GitLab, Gitea), Unsplash, an LLM provider, link unfurling.

Routing is done by Caddy (`apps/proxy/Caddyfile.ce`): `/api/*`, `/auth/*` and `/static/*` go to `api:8000`; `/live/*` to `live`; `/spaces/*` to `space`; `/god-mode/*` to `admin`; everything else to `web`.

## Rules that must not be broken

These hold at every commit, not only at the end.

1. **Same database.** The Rust services read and write the existing PostgreSQL schema exactly as Django left it: same tables, columns, types, constraints, defaults. No schema changes are made for the convenience of the rewrite.
2. **Same HTTP contract.** For every ported route: same path, methods, status codes, JSON field names, field types, null-versus-absent behaviour, ordering, pagination shape, error bodies, cookies and headers the frontends depend on. The frontends are not edited to accommodate the rewrite.
3. **Django stays the reference.** The Python code in `apps/api` is not deleted or modified while the port is in progress. It is the specification and the test oracle. When this document and the Python code disagree, the Python code wins and this document gets fixed.
4. **Django owns migrations until cutover.** New upstream migrations keep being applied by the existing `migrator`. The Rust code adapts to the schema; it does not generate it.
5. **Side effects count as behaviour.** A route is not ported until the rows it writes, the activity/notification records it creates, the tasks it enqueues and the webhooks it fires match what Django does.
6. **Security behaviour is ported, never simplified.** Permission checks, CSRF and origin checks, rate limits, request body limits, SSRF protection on webhooks and link unfurling, HTML sanitisation, and path validation all carry over. If a check looks redundant, port it anyway and note the question in the ledger.
7. **No secrets in the repository**, and nothing is pushed or deployed by the agent.

## Target architecture

A Cargo workspace at `apps/server/`. Library crates hold the logic; binaries are thin.

```
apps/server/
  Cargo.toml                 workspace
  crates/
    plane-core/              config, errors, ids, time, pagination, shared types
    plane-db/                sqlx models and queries, one module per Django model file
    plane-auth/              sessions, password hashing, API tokens, OAuth, CSRF, permissions
    plane-storage/           S3 presigning and object operations
    plane-jobs/              queue, task definitions, task handlers, schedule
    plane-http-app/          handlers for /api/
    plane-http-public/       handlers for /api/v1/
    plane-http-space/        handlers for /api/public/
    plane-http-auth/         handlers for /auth/
    plane-http-instance/     handlers for /api/instances/
    plane-testkit/           differential-test harness and fixtures
  bins/
    plane-api/               HTTP server
    plane-worker/            job consumer
    plane-beat/              scheduler
    plane-migrator/          schema bootstrap and upgrade (used only after cutover)
    plane-live/              collaboration server (later phase)
```

Recommended stack. Check current versions and docs before pinning; do not rely on memory for API details.

| Concern | Choice | Notes |
| --- | --- | --- |
| Async runtime | tokio | |
| HTTP | axum + tower | tower layers map well onto Django middleware |
| Database | sqlx (Postgres) | Hand-written SQL with compile-time checking against the real schema. Prefer this to an ORM: the Django views lean heavily on annotations, subqueries, window functions and array aggregates (about 380 such uses across 39 files) that are clearer as SQL. |
| Serialisation | serde, serde_json | |
| Redis | `redis` crate or `fred` | |
| Object storage | AWS SDK for Rust (S3) | must work against MinIO with path-style addressing |
| Mail | lettre | |
| Outbound HTTP | reqwest | with the same SSRF guards as `plane/utils/url_security.py` |
| HTML handling | ammonia, scraper/lol_html | must match `plane/utils/content_validator.py` and `html_processor.py` |
| Password strength | zxcvbn (Rust port) | scores must agree with the Python `zxcvbn` used today |
| Spreadsheets | rust_xlsxwriter | exports |
| Telemetry | tracing + OpenTelemetry | |

Memory targets, to be measured, not assumed: `plane-api` under 60 Mi idle, `plane-worker` under 60 Mi idle, `plane-beat` under 20 Mi idle.

## How the two backends coexist

This is a strangler migration. Both backends run against the same database, and Caddy decides per path which one answers.

- Add a `plane-api` upstream to the proxy. Move path prefixes to it one group at a time; everything not yet moved keeps going to Django.
- Keep a route-ownership table in `apps/server/ROUTES.md`: every Django route, its owner (`django` or `rust`), and its verification status. Generate the initial list from Django's URL resolver rather than by hand.
- A feature flag per route group lets a group be sent back to Django without a redeploy of anything but the proxy config.

Three things make coexistence work and must be built first:

**Sessions.** Django stores sessions in the `sessions` table (custom model in `plane/db/models/session.py`: 128-character key, `user_id`, `device_info`). The cookie is `session-id` (`admin-session-id` for the instance admin, with its own lifetime). The stored payload is Django's signed, optionally compressed JSON, keyed off `SECRET_KEY`. `plane-auth` must read and write this format byte-compatibly so a user logged in through Django is logged in on Rust routes and the reverse. Verify the exact encoding against Django's source for the pinned version; do not guess it.

**Passwords.** Hashes are in Django's `algorithm$iterations$salt$hash` format. Verify existing hashes and write new ones in the same format and with the same parameters as the pinned Django version, so Django can still verify them.

**CSRF and origin checks.** Port `plane/authentication/session.py` and Django's CSRF token scheme exactly, including the masked-token format, so a token issued by one backend is accepted by the other.

## Background jobs

Today: 48 Celery tasks over RabbitMQ, a Celery worker, and `django_celery_beat` with a database scheduler.

Target: a Postgres-backed queue (`SELECT ... FOR UPDATE SKIP LOCKED`), owned by `plane-jobs`. This removes RabbitMQ at cutover and lets a handler enqueue a job in the same transaction as the write that caused it. The queue's tables are the one permitted schema addition, and they live in their own Postgres schema (`plane_jobs`) so they never collide with Django's migrations.

During coexistence there are two queues, so each task has exactly one owner at a time:

- A task still owned by Celery: when a Rust handler needs it, Rust publishes a Celery-protocol JSON message to RabbitMQ. The arguments must match the Python signature exactly. Build one small, well-tested publisher for this.
- A task owned by Rust: Django callers of that task must already be gone (their routes ported), or the task is not ready to move. Port tasks together with the last route group that enqueues them.

Scheduled jobs (`plane/celery.py`): 13 entries, a mix of every-five-minutes and daily jobs at fixed UTC times. `plane-beat` takes them over one at a time; when it owns a job, that entry is disabled on the Celery side so nothing runs twice. `plane-beat` must be safe to run as more than one replica (take a Postgres advisory lock per tick).

Every task handler must be idempotent or guarded, with retries and a dead-letter state. Record, per task, the retry policy the Python version has and match it.

## Things that are easy to get wrong

Port each of these deliberately and write a test for it.

- **Soft delete.** Models carry `deleted_at` and Django filters it through custom managers. Every Rust query must apply the same filter, and the nightly `hard_delete` job must cascade the same way.
- **Model `save()` overrides and signals.** There are about two dozen. They assign sequence numbers, sort orders, slugs, default states and similar. In Rust this logic has no automatic home, so it must be called explicitly from every write path. List them all in `apps/server/MODEL_HOOKS.md` before porting any write endpoint.
- **Audit columns.** `created_by` and `updated_by` are set from the request user by middleware (`django-crum`). Timestamps are set by the ORM, not by database defaults.
- **Serializer behaviour.** DRF serializers support `fields=` and `expand=` query parameters, omit or null fields in specific ways, and format decimals and datetimes in a specific style. Byte-compare JSON, do not eyeball it.
- **Pagination.** Custom paginators in `plane/utils/paginator.py`, including grouped and sub-grouped results and a cursor string format the frontend parses.
- **Issue filtering.** `plane/utils/issue_filters.py` and `plane/utils/filters` turn query parameters into queries. This is the most-used read path and has many combinations.
- **Activity and notifications.** `issue_activities_task.py` and `notification_task.py` are large diffing routines that turn a before/after pair into activity rows, notifications and emails. Port them with their unit tests, case by case.
- **Permissions.** Workspace and project roles are enforced by permission classes and decorators (`plane/app/permissions`, `plane/utils/permissions`). Model them as explicit extractors or guards so a handler cannot be written without one.
- **Rate limits.** Anonymous, authentication, API-key and asset throttles, with `NUM_PROXIES` deciding the client IP.
- **API tokens.** `/api/v1/` authenticates with API keys and logs requests (`APITokenLogMiddleware`).
- **Instance configuration.** Many settings are stored encrypted in the database and override environment variables (`plane/license`, `plane/utils/instance_config_variables`). Decryption must be compatible.
- **Environment variables.** Keep every existing variable name and default so existing deployments need no changes. The full list is extractable from `plane/settings`.
- **OAuth.** Four providers, each with app and space variants, sharing state handling in `plane/authentication/utils/oauth_state.py`.
- **Timezones.** Per-user and per-project timezones affect date filters, cycle boundaries and analytics.

## Live server

`apps/live` is a Hocuspocus (Yjs) WebSocket server with a Redis extension, title sync, a document conversion endpoint and a PDF export built on `@react-pdf/renderer`. It authenticates by calling the API.

- The sync protocol can be served from Rust with `yrs` (the Rust Yjs implementation). It must stay wire-compatible with the Hocuspocus provider used in `packages/editor`, including authentication messages, awareness and stateless messages.
- Converting between Yjs documents, Tiptap JSON and HTML currently uses Tiptap's own JavaScript with the editor's schema. There is no drop-in Rust equivalent. Options, in order of preference: reimplement the conversion for Plane's schema in Rust with a golden-file corpus produced by the Node code; or keep a small Node sidecar for conversion and PDF export only.
- Do not start this phase until the API is fully cut over. Write a short design note first and get it reviewed.

## Verification

Nothing counts as ported on the strength of reading the code.

1. **Differential tests (primary).** `plane-testkit` starts Django and `plane-api` against two databases restored from the same snapshot, sends the same request to both, and compares status, relevant headers, JSON body, and the resulting database diff. Normalise only what is legitimately nondeterministic (generated ids, timestamps, tokens), and keep that list short and explicit.
2. **Ported contract tests.** `plane/tests/contract` exercises the HTTP surface. Port each to run against `plane-api`. A route group is not done until its contract tests pass on Rust.
3. **Ported unit tests** for pure logic: filters, paginators, activity diffing, validators, URL security.
4. **Frontend smoke.** Run `web`, `admin` and `space` against the mixed backend and walk the main flows: sign in, create workspace and project, create and edit work items, cycles, modules, views, pages, intake, notifications, uploads, exports, public board.
5. **Memory and latency.** Record idle memory and p50/p95 for a fixed request set after each phase, in `apps/server/BENCH.md`.

The existing Docker test stack (`docker-compose-test.yml`, see `AGENTS.md`) is how Django is run for comparison.

## Phases

Each phase ends with its exit check passing and the ledger updated. Do not start a phase early.

| Phase | Work | Exit check |
| --- | --- | --- |
| 0. Inventory | Generate `ROUTES.md` from Django's resolver, `MODEL_HOOKS.md`, a task table (name, callers, arguments, retry policy), an env-var table. Build the differential harness against Django alone. | Harness replays a recorded session against Django twice with a clean diff. |
| 1. Skeleton | Workspace, config loading with existing env names, logging, health endpoint, DB pool, error format matching DRF's, proxy upstream added. | `plane-api` serves health behind the proxy; CI builds and lints. |
| 2. Auth foundation | Sessions, password hashing, CSRF, origin checks, user extraction, permission guards, rate limiting, API-key auth. | A session created by either backend works on the other; hashes round-trip both ways. |
| 3. Read paths | `plane-db` models and all GET routes, starting with users, workspaces, projects, states, labels, then work items with filtering and pagination. | Differential and contract tests green for every ported GET. |
| 4. Job plumbing | Postgres queue, worker, Celery-protocol publisher, `plane-beat` with locking. | A Rust handler can enqueue to both queues; a scheduled job runs exactly once with two beat replicas. |
| 5. Write paths | POST/PATCH/DELETE by domain: projects and members, states and labels, work items and their sub-resources, cycles, modules, views, pages, intake, notifications, favourites, stickies, webhooks, assets, exports, analytics, search. Port each domain's tasks with it. | Per domain: differential tests including database diffs, contract tests, side effects verified. |
| 6. Auth flows | `/auth/` routes: email, magic link, password management, sign-out, four OAuth providers, app and space variants. | Every flow walked end to end in a browser against the mixed backend. |
| 7. Remaining surfaces | `/api/v1/`, `/api/public/`, `/api/instances/`, OpenAPI schema output. | Published API docs still accurate; contract tests green. |
| 8. Cutover | All routes owned by Rust, all tasks and schedules owned by Rust, Django and RabbitMQ removed from the deployment, `plane-migrator` takes over schema management from a baseline equal to the last Django migration. | A fresh install and an upgrade of an existing database both work with no Python container. |
| 9. Live | See "Live server". | Collaborative editing, title sync, conversion and PDF export work against `plane-live`. |

Phase 8 is the point where staying in step with upstream becomes a manual job: every upstream schema or API change must be re-implemented by hand. Before starting it, write down the upstream commit the port corresponds to and decide whether the fork keeps tracking upstream.

## Working rules

- **Ledger.** Keep `apps/server/PROGRESS.md`: current phase, what was finished this session, what is in flight, open questions, and the next three concrete steps. A new session starts by reading it.
- **Small, verified commits.** One route group or one task per commit, with its tests. Conventional commit messages, matching this repository's history, for example `feat(server): port project label routes`.
- **Never push, never deploy, never touch a real database.** Work on the `rust-rewrite` branch against local containers only.
- **Do not edit `apps/api`, the frontends or `packages/`.** If the port seems to need a change there, stop and record it as an open question.
- **Read before porting.** For each route, read the view, its serializer, its permission classes, the model methods it calls and the tasks it enqueues, then write the differential test, then the handler.
- **No stubs that pass.** A handler that is not finished returns an explicit not-implemented error and stays routed to Django. A test is never weakened to make it pass.
- **When behaviour looks like a bug**, port it as it is and list it in the ledger. Fixing upstream behaviour is a separate decision.
- **Look things up.** Check crate documentation for current APIs rather than relying on recall.
- **New source files** need a licence header consistent with the repository (AGPL-3.0-only; see `COPYRIGHT.txt` and `COPYRIGHT_CHECK.md`). The fork stays AGPL.

## Open decisions for the owner

These are not the agent's to settle. Each has a default the plan assumes.

1. **Queue backend.** Assumed: Postgres-backed queue, RabbitMQ removed at cutover. Alternative: keep RabbitMQ and speak AMQP from Rust.
2. **Upstream tracking after cutover.** Assumed: the fork pins to one upstream release and ports later changes selectively.
3. **Live server conversion.** Assumed: attempt a Rust conversion with golden files, fall back to a Node sidecar.
4. **Read replica support.** Assumed: kept, since it is an existing deployment option.
5. **Telemetry to Plane.** `push_instance_metrics` reports instance metrics upstream. Assumed: not ported in the fork.
6. **`apps/space`.** It is the one frontend that runs a Node server at runtime. Assumed: left alone; revisit after cutover.
