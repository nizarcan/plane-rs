# Phase 8 — cutover (plan, read-only)

Scope: `RUST_REWRITE.md:212` and `DECISIONS.md:446-453` (D20). Exit: all routes, tasks and
schedules owned by Rust; Django and RabbitMQ out of the deployment; `plane-migrator` owns the
schema from a baseline equal to the last Django migration; a fresh install and an upgrade of a
Django-migrated database both work with no Python container. Phase 9 (live) is out of scope.

Two facts make this phase smaller than it looks. **(1) The HTTP cutover is not a route
migration.** Today's 157 Django-owned `ROUTES.md` rows are exactly the Phase 6/7 groups
(`PROGRESS.md:1611-1614`), and Phase 7's plans port all of them (`V1_API.md`,
`PUBLIC_INSTANCES.md`); `health`/`web` are never proxied to `api:8000` (`Caddyfile.ce:6-23`;
`PUBLIC_INSTANCES.md:76-79`). The only Django-served path left is `/static/*`
(`Caddyfile.ce:18`). **(2) No task body exists in Rust.** `ownership.rs:21` lists only
`plane.jobs.echo`, and `plane-worker` registers only that handler
(`bins/plane-worker/src/main.rs:52-72`). Phase 8 is ~38 handler ports plus schema and deployment
work, not a transport rewrite.

## 1. Task inventory

46 registered tasks, 13 beat entries over 12 tasks, 166 enqueue sites (`TASKS.md:23-30`); per-task
mechanics in `TASKS.md:446-491`, side effects in `:527-575` (all `(:nnn)` citations below are that
file). No task has a Rust handler today.

Producer legend (every HTTP producer is Rust after Phase 7): **W** `/api/` (already Rust),
**V** `/api/v1/` (Phase 7), **P** `/api/public/` (Phase 7), **A** `/auth/` (already Rust),
**T:** another task, **B:** beat only, **M:** `APITokenLogMiddleware`,
**H:** `SoftDeleteModel.delete()`. `h` = agent-hours. The only Django signal receiver creates a
`UserNotificationPreference` (`db/models/user.py:297-308`, already H31, `PROGRESS.md:1410`); no
signal and no `AppConfig.ready()` enqueues a task (`TASKS.md:179`).

| # | task (`plane.bgtasks.` unless noted) | side effects | producers | h | set |
|---|---|---|---|---|---|
| 1 | `analytic_plot_export.analytic_export_task` | read `Issue` via filters, CSV in memory, 1 email (`TASKS.md:529`) | W | 8 | C |
| 3 | `cleanup_task.delete_api_logs` | hard-delete `APIActivityLog` >14 d, batches of 500 (`:531`) | B | 1 | D |
| 4 | `cleanup_task.delete_email_notification_logs` | hard-delete `EmailNotificationLog` >7 d (`:532`) | B | 1 | D |
| 5 | `cleanup_task.delete_page_versions` | window delete, keep 20/page (`:533`) | B | 1.5 | D |
| 6 | `cleanup_task.delete_issue_description_versions` | same for descriptions (`:534`) | B | 1.5 | D |
| 7 | `cleanup_task.delete_webhook_logs` | hard-delete `WebhookLog` >14 d (`:535`) | B | 1 | D |
| 8 | `copy_s3_object.copy_s3_objects_of_description_and_assets` | new `FileAsset` rows, S3 `copy_object`, rewrites `description_html`, POSTs `LIVE_URL/convert-document/` (`copy_s3_object.py:76-127`) | W | 6 | C |
| 9 | `deletion_task.soft_delete_related_objects` | recursive soft-delete cascade; SET_NULL; skips DO_NOTHING (`MODEL_HOOKS.md:174-213`, H35) | H, already published by Rust (`write/mod.rs:131-148`) | 10 | D |
| 10 | `deletion_task.hard_delete` | hard delete 18 models then every `deleted_at` model (`:538`) | B | 6 | D |
| 12 | `email_notification_task.stack_email_notification` | reads unprocessed logs, enqueues 1 send per (receiver, issue), stamps `processed_at` (`:540`) | B | 3 | B |
| 13 | `email_notification_task.send_email_notification` | Redis lock + `GET <issue_id>` (600 s TTL), 1 email, `sent_at` (`:541`, `:1159-1164`) | T:12 | 6 | B |
| 14 | `export_task.issue_export_task` | zip in memory, S3 `ACL public-read`, 7-day presign, `ExporterHistory` (`:542`) | W | 8 | C |
| 15 | `exporter_expired_task.delete_old_s3_link` | S3 `delete_object`, `url=NULL` (`:543`); scheduled twice (`celery.py:69-72,97-100`) | B×2 | 2 | C |
| 16 | `file_asset_task.delete_unuploaded_file_asset` | delete `is_uploaded=false` >7 d (`:544`) | B | 1 | C |
| 17 | `forgot_password_task.forgot_password` | email (`:545`) | A | 1 | B |
| 18 | `issue_activities_task.issue_activity` | 27 activity builders; `IssueActivity` bulk insert, subscribers, `Issue.updated_at`, Redis origin key, enqueues 29 (`issue_activities_task.py:1550-1640`) | W,V,P,T:19 | 40 | A |
| 19 | `issue_automation_task.archive_and_close_old_issues` | bulk archive/close + `issue_activity` per row (`:547`) | B | 5 | D |
| 20 | `issue_automation_task.unsnooze_expired_intake_issues` | one `UPDATE intake_issues` (`:548`) | B | 1 | D |
| 23 | `issue_description_version_task.issue_description_version_task` | new/updated version row, 600 s window (`:551`) | W | 3 | A |
| 27 | `logger_task.process_logs` | inserts `APIActivityLog` (`:555`, `logger_task.py:33-41`) | M | 2 | D |
| 28 | `magic_link_code_task.magic_link` | email (`:556`) | A | 1 | B |
| 29 | `notification_task.notifications` | `Notification`, `IssueMention`, `IssueSubscriber`, `EmailNotificationLog`; skips 13 types, incl. cycle/module/reaction/vote/draft (`:557`, `notification_task.py:206-220`) | T:18 | 20 | A |
| 30 | `page_transaction_task.page_transaction` | `PageLog` diff of `<mention-component>`/`<image-component>` (`:558`) | W | 3 | A |
| 31 | `page_version_task.track_page_version` | `PageVersion`, keep 20 (`:559`) | W | 2 | A |
| 32 | `project_add_user_email_task.project_add_user_email` | email (`:560`) | W | 1 | B |
| 34 | `recent_visited_task.recent_visited_task` | upsert `UserRecentVisit`, trim at `count == 20` (`:562`) | W (10 GET sites) | 2 | P0 |
| 35 | `storage_metadata_task.get_asset_object_metadata` | S3 HEAD, writes `storage_metadata` (`:563`) | W,V,P | 2 | C |
| 36 | `user_activation_email_task.user_activation_email` | email (`:564`) | A adapter | 1 | B |
| 37 | `user_deactivation_email_task.user_deactivation_email` | email (`:565`) | W | 1 | B |
| 38 | `user_email_update_task.send_email_update_magic_code` | email (`:566`) | W | 1 | B |
| 39 | `user_email_update_task.send_email_update_confirmation` | email (`:567`) | W | 1 | B |
| 40 | `webhook_task.send_webhook_deactivation_email` | email (`:568`) | T:41 | 1 | B |
| 41 | `webhook_task.webhook_send_task` | pinned POST, HMAC `X-Plane-Signature`, `WebhookLog` per attempt, deactivate after 5 (`webhook_task.py:242-386`) | T:42 | 8 | B |
| 42 | `webhook_task.webhook_activity` | active-webhook fan-out, serialises entity, enqueues 41 (`:570`) | W,T:43 | 6 | B |
| 43 | `webhook_task.model_activity` | read-only diff of `requested_data` vs `current_instance`, enqueues 42 (`:571`) | W,V | 4 | B |
| 44 | `work_item_link_task.crawl_work_item_link_title` | pinned GET + favicon, `IssueLink.metadata` (`:572`) | W,V | 4 | C |
| 45 | `workspace_invitation_task.workspace_invitation` | `WorkspaceMemberInvite.message` + email (`:573`) | W | 1 | B |
| 46 | `workspace_seed_task.workspace_seed` | bot user + project/states/labels/cycles/modules/issues/views/pages from `seed/*.json` (`:574`) | W | 10 | C |

Command-only, folded into a new `plane-admin` binary (no queue row): `sync_issue_version` and
`sync_issue_description_version` plus their two `schedule_*` one-shot enqueuers
(`TASKS.md:549-554,956-960,1073-1075`) — 8 h, set A.

Not ported, deliberately: `export_analytics_to_csv_email` and `issue_task` (dead,
`TASKS.md:1115-1117`), `project_invitation` (never imported; its only caller raises after committing
the invites, `TASKS.md:1081-1105`), `create_dummy_data` (command-only, never queued, `TASKS.md:539`),
and `push_instance_metrics` (owner decision 5, `RUST_REWRITE.md:237`: drop the beat row and the
`register_instance.py:90` enqueue). Total: **38 queue handlers ≈ 177 h**, plus 8 h of admin commands
and 40 h of P0 — ≈ 225 h.

## 2. Queue cutover

**Per-task switch, never dual-publish.** `ownership::owner` (`ownership.rs:52-65`) is the
route-group flag of Phase 8: one task moves by adding it to `RUST_TASKS:21`, and `route::dispatch`
(`route.rs:46-62`) sends the next enqueue to `plane_jobs` instead of RabbitMQ. Dual-publishing
would run every task twice — the one thing this queue cannot absorb (`TASKS.md:536-574`: many
tasks are not idempotent).

**Producer-before-consumer is the safety rule.** A task may move only when every producer is
already Rust, because a Python producer still calls `.delay()`, which only RabbitMQ delivers
(`TASKS.md:943-945`). Encode it as a table plus a unit test: `ownership.rs` gains
`PRODUCERS: &[(task, &[producer])]`, and a test fails if any Rust-owned task has a non-Rust
producer. That ordering yields, per chain: `model_activity → webhook_activity → webhook_send_task
→ send_webhook_deactivation_email`; `issue_activity → notifications`; `stack_email_notification →
send_email_notification`; `archive_and_close_old_issues → issue_activity`.

**Rolling restarts are safe with no Python edit.** Old `plane-api` replicas publish to Celery, new
ones enqueue to Postgres, and the Python worker keeps all 46 names registered throughout Phase 8
(one image serves api/worker/beat/migrator, `Dockerfile.api`, `docker-compose.yml:37-97`), so either
path is consumed. Nothing is deleted from `apps/api` (rule 3, `RUST_REWRITE.md:76`). At switch time
the broker may still hold messages from a pre-switch publisher; the removal barrier is a drained
queue (`rabbitmqctl list_queues messages messages_unacknowledged` = 0 twice, 30 s apart, plus
`celery -A plane inspect active` empty), and only then a SIGTERM to the worker (Celery's warm
shutdown) — `task_acks_late=False` means a hard kill silently loses an in-flight task
(`TASKS.md:123`).

**Retries, ETA, countdown.** `countdown=` only becomes an `eta` header, at two call sites
(`TASKS.md:724-725,746-747`); map the header to `Job.run_at`. Only `webhook_send_task` retries:
`max_retries=5` with a **uniform 0–600 s** jitter (`TASKS.md:498-515`) → `Job::retry(5, 600, true)`
(`queue.rs:153-160`). Every other task swallows its exception (`TASKS.md:517-521`); the Rust policy
is `max_retries=0`, record `last_error` and `dead`, and **never** retry — retrying would create rows
Django never created. `workspace_seed` raises, but with no result backend that is also "nowhere"
(`TASKS.md:520-521`).

**Idempotency.** Re-delivery is only possible through lease expiry (at-least-once, `JOBS.md:145-149`),
so: bound each lease to its handler (webhook delivery is 30 s, `webhook_task.py:318-330`, so the
60 s default `ENV.md:1016` is too tight); `send_email_notification` keeps its Redis `SET NX EX 300`
(`email_notification_task.py:34-37`); `webhook_send_task` may use a `dedupe_key` while queued, never
for a real retry.

**Beat handover.** Already implemented and proven: `plane-beat` disables the
`django_celery_beat_periodictask` row and bumps `_periodictasks.last_update` in one transaction,
inside a quiet window (G=30 s, M=15 s) — the only mechanism that works without editing Python
(`JOBS.md:9-75`; exit test `tools/jobs/beat_handover.sh`, `PROGRESS.md:266-269`). Per-entry order:
move the task, then flip `plane_jobs.schedules.owner='rust'` for its entry. At the end, when the
Django beat container is gone, `Beat::new` already tolerates a missing table
(`beat.rs:347-353,405-413`); add one `plane-beat --take-all` that sets `owner='rust'` for the seeded
names left after the metrics entry is deleted (`beat.rs:53-58`) and `last_run_at=now()` (a tick
inside the window is skipped, not replayed — the same choice the quiet-window handover makes,
`JOBS.md:70-72`). Every tick stays exactly-once:
`pg_try_advisory_xact_lock(hashtextextended(name||':'||tick,0))` plus the `ticks` primary key, in
the same transaction as the job insert (`JOBS.md:180-193`).

**Removing RabbitMQ.** One step, after the last task drains and the beat has taken over: stop the
Python worker, stop the Python beat (already silent), delete the `plane-mq` service and volume,
drop `AMQP_URL`/`RABBITMQ_*` from every env file, and compile out the `celery` feature (`D7`,
`DECISIONS.md:230-234`). No message may be in flight at that instant, which the drain check above
proves.

**Verification per step.** (a) The managed-mode difftest still compares the Celery message
byte-for-byte while any call site is still Celery-owned (`D15`, `DECISIONS.md:361-371`). (b)
Payload equivalence: `dispatch` already builds both representations (`route.rs:41-45`), so a test
asserts `Job.payload == {args, kwargs}` normalised from the `Message` for every ported call site —
the switch is then provably transport-only. (c) Handler correctness: a new `difftest task` mode, A
= Django `task.apply()` in-process, B = enqueue + `plane-worker`, comparing the DB diff, Redis
keys, S3 object listing/metadata and captured SMTP — the harness already diffs all four
(`DECISIONS.md:361-371`). (d) Run every handler twice and diff the second run against no-op. (e)
Each step ends with a 0-message drain check.

## 3. Startup and management commands

What the Python containers do at boot (`apps/api/bin/`):

| boot step | where | Rust owner (Phase 8) |
|---|---|---|
| `wait_for_db`, `wait_for_migrations` | `docker-entrypoint-api.sh:3,5` (`wait_for_db.py:14-24`; `wait_for_migrations.py:22-26`) | a shared connect-retry helper, then a `plane_jobs.meta('schema_version')` check in every binary: the migrator is a `service_completed_successfully` dependency, and no worker may run against a half-built schema (`TASKS.md:168-170`) |
| `register_instance <sig>` | `:24`, `register_instance.py:60-92` (create/extend `Instance`, GitHub `releases/latest` with a 10 s timeout `:38-51`, then `push_instance_metrics.delay()` `:90`) | `plane-api` bootstrap: same create-or-update; embed `apps/api/package.json` (`"version": "1.5.0"`) with `include_str!` because the distroless image ships no files (`Dockerfile.plane-api`); keep `APP_VERSION`/`IS_TEST` (`ENV.md:129,354`); **drop** the metrics enqueue |
| `configure_instance` | `:27`, `configure_instance.py:22-38` | `plane-api` bootstrap: 36 `InstanceConfiguration` rows from `utils/instance_config_variables/core.py`, `encrypt_data` for `is_encrypted` (already ported, `fernet.rs`) |
| `create_bucket` | `:30`, `create_bucket.py:17-40` (boto3 `head_bucket`/`create_bucket`, all errors swallowed) | new `plane-storage` `head_bucket`/`create_bucket`, same swallow-and-log; `storage.rs` has no bucket call today |
| `clear_cache` | `:33`, `clear_cache.py:15-28` → django-redis `client.clear()` = **`FLUSHDB`** (`django_redis/client/default.py:550-561`) | `plane-cache` FLUSHDB on `REDIS_URL`; port as-is — it also wipes throttle buckets and magic-link codes on every api start (upstream behaviour) |
| `collectstatic` | `:36` | deleted: the 8 files in `apps/api/plane/static` (36 KB, referenced by `templates/emails/invitations/project_invitation.html:230-280`) are embedded in `plane-api`, which serves `/static/*` (`Caddyfile.ce:18`) |
| container commands | `:38` gunicorn; `docker-entrypoint-worker.sh:3-7`; `-beat.sh:3-7`; `-migrator.sh:3-5` | `plane-api`, `plane-worker`, `plane-beat`, `plane-migrator` (all exist) |

Operator commands are not on the boot path and no deployment script calls them (`grep
create_instance_admin create_dummy_data test_email deployments/ setup.sh` → nothing). Port
`reset_password`, `activate_user` and `test_email` as `plane-admin` subcommands; leave the six
data-repair commands (`copy_issue_comment_to_description`, `fix_duplicate_sequences`, `update_bucket`,
`update_deleted_workspace_slug`, `create_project_member`, `reactivate_workspace_member`) to the
Python image as an optional, off-by-default ops container.

## 4. `plane-migrator`

Today it only creates `plane_jobs` (`bins/plane-migrator/src/main.rs:23-33`, `queue.rs:24-88`) with
`meta('schema_version','2')` (`queue.rs:187-197`). Phase 8 adds the public schema.

**Baseline.** One `pg_dump --schema-only --no-owner --no-privileges --schema=public` from a
database migrated by Django at the pinned upstream commit (`PROGRESS.md:12`, `290383d7f9`), ending
at `db:0125_reset_project_deploy_board_identity` and
`license:0006_instance_is_current_version_deprecated` (`apps/api/migrations_lockfile.txt:9-10`),
checked into `apps/server/deploy/schema/baseline.sql`. It covers tables, indexes, constraints,
sequences and defaults, plus the third-party tables (`auth`, `contenttypes`, `sessions`,
`django_celery_beat` ×6, `django_migrations`) — rule 1 keeps the schema unchanged, so the unused
beat tables stay, empty. The 66 `RunPython` steps are per-row backfills over rows that already
exist (`0112_auto_20251124_0603.py:60` creates a state per project, `0065_auto_20240415_0937.py:17`
a profile per user, `0125_reset_project_deploy_board_identity.py:6-22` rewrites board identities),
so an empty database has nothing for them to touch and the dump stays structure-only; the upgrade
path never replays them because Django already applied them. `0124`'s `AddIndexConcurrently` is
materialised as a plain index in the dump (`0124_intakeissue_intake_issue_snoozed_till_idx.py:4-12`).

**Fresh install.** `public` empty and no baseline marker → run the baseline in one transaction,
insert the baseline's `django_migrations` rows (so a rollback to the Python image still works),
then `plane_jobs::migrate` and `meta('django_baseline','db:0125,license:0006')`.

**Upgrade.** `public` non-empty: require `to_regclass('public.django_migrations')`; read
`(app,name)` and require **exactly** the baseline set. A missing row = an older database → refuse
and name the Django migrator (`apps/api/bin/docker-entrypoint-migrator.sh:5`); an extra row = a newer
database than the binary → refuse (downgrade guard); a non-empty `public` without
`django_migrations` → refuse (never guess). Then only `plane_jobs` DDL and future Rust steps run.
All of it under `pg_advisory_lock`, because `restart: no` (`docker-compose.yml:91`) does not stop an
operator from starting two.

**Verify the dump.** Two databases from the same empty state: `manage.py migrate` (Django) and
`plane-migrator`. `pg_dump --schema-only` both, normalise only the header/`search_path` lines,
`diff` must be empty; then compare `information_schema.columns` (types/nullability — 1,416 columns
are already checked this way, `PROGRESS.md:122`) and `pg_indexes`/`pg_constraint` (new: `schema_check`
does not cover indexes). Repeat for the upgrade path from a restored Django snapshot.

**Future upstream migrations (open question 2).** Pin the upstream commit, diff
`apps/api/plane/*/migrations` against it, and hand-write each migration as an ordered step in
`plane-migrator`: DDL in Rust with the same SQL Django would emit (`sqlmigrate` is the oracle),
`RunPython` as data SQL, `AddIndexConcurrently` as its own non-transactional step, plus an appended
`django_migrations` row. Prove each with the same two-database diff. The default in
`RUST_REWRITE.md:234` (pin one release, port later changes selectively) is what this assumes.

## 5. Deployment

**Compose** (root `docker-compose.yml`): `api` builds `apps/server/deploy/docker/Dockerfile.plane-api`
instead of `apps/api/Dockerfile.api` (`:37-50`); delete `worker` (`:52-66`) and `beat-worker`
(`:68-82`); `migrator` (`:84-97`) becomes the `migrator` target of `Dockerfile.plane-jobs`; add
`plane-worker` and `plane-beat` from the `worker`/`beat` targets (all three images exist,
`Dockerfile.plane-jobs:76-86`, ~54 MB); delete `plane-mq` (`:130-141`) and the `rabbitmq_data`
volume (`:177`); `api`/`worker`/`beat` depend on the migrator with
`condition: service_completed_successfully`. Same edit in `deployments/cli/community/docker-compose.yml`
(`:116-212`, drop `x-mq-env` and `AMQP_URL`), and in `deployments/aio/community/supervisor.conf:7-72`
replace the four Python programs with the four Rust binaries. The Helm chart is not in this repo
(`deployments/kubernetes/community/README.md:1-12`), so it can only be documented.

**Caddy.** `apps/proxy/Caddyfile.ce:14-18`: `/api/*`, `/auth/*`, `/static/*` → `plane-api:8000`;
drop `deploy/caddy/*` from the image and retire `route-groups.yaml` and the `PLANE_RS_GROUP_*`
flags (`deploy/caddy/README.md`). `Caddyfile.aio.ce:5-27` has no `/static/*` handle at all, so
invitation-email logos already 404 in AIO (`templates/emails/invitations/project_invitation.html:230`);
adding the handle is a deliberate fix — note it in the ledger.

**Env.** Remove `AMQP_URL` and `RABBITMQ_*` (`ENV.md:165-170`), `CELERY_*` (`common.py:362-387`),
`GUNICORN_WORKERS` (`ENV.md:364`) and `METRICS_PUSH_INTERVAL_MINUTES` (`ENV.md:171`); keep every
other name and default (rule 6, `RUST_REWRITE.md:174`). Carry `INSTANCE_CHANGELOG_URL` into
`Dockerfile.plane-api` — the Python image bakes it in (`Dockerfile.api:10`) and `/api/instances/`
returns it. Add `PLANE_RS_JOB_*` (`ENV.md:1015-1023`). The worker needs the api's env file:
`SECRET_KEY` (instance-config decryption), `DATABASE_URL`, `REDIS_URL`, `AWS_*`, `WEB_URL`,
`LIVE_URL`, `WEBHOOK_ALLOWED_*`.

**Exit check, scripted** (`tools/cutover/`): (i) *fresh*: `docker compose down -v`, start with no
Python image present, wait for the migrator, then walk sign-in → workspace → project → work item →
comment → upload → export → invite (SMTP sink) → webhook (local listener); assert
`/static/logos/github_32px.png` and zero `python` processes. (ii) *upgrade*: restore a `pg_dump` of
a Django-migrated database at `0125`, start only Rust containers, assert the migrator reports
"baseline matched", then replay the same walk plus every read-only difftest scenario. (iii)
`git grep -l apps/api/bin` matches no deployment file.

**Final benchmark** (`BENCH.md`): the Phase 5 worker/beat idle figures (1.02/0.89 MiB,
`BENCH.md:895-897`) were taken without a confirmed broker connection (`PROGRESS.md:1649-1651`).
Re-measure with a live queue: `plane-worker` with its `LISTEN` connection plus one claimed job per
concurrency slot, `plane-beat` after a tick has fired, `plane-api` idle — against Django's 852 MiB
(`celery -c 4`) and 171.8 MiB (`BENCH.md:738-740`) — and record the whole-stack total against
`RUST_REWRITE.md:11-20` (Python ≈ 2,120 MiB of the 3.5 GB).

## 6. Split and order

| set | tasks / files owned | depends on | h |
|---|---|---|---|
| **P0** | handler registry + per-task policy; `Job::from_message`; `plane-worker` boot/lease/drain; task context (DB, Redis, storage, `plane-net`, mailer); `plane-mail` (minijinja templates, `generate_plain_text_from_html`, `get_email_configuration`, the `EmailMultiAlternatives` equivalent); `difftest task`; **move `recent_visited_task`** end to end | Phase 7's mail transport (`PUBLIC_INSTANCES.md:98-105`) only for the mailer; the rest now | 42 |
| **A** activity chain | 18, 29, 23, 30, 31 + `plane-admin` version backfills (A1 = 18; A2 = 29; A3 = 23/30/31/admin) | A1/A2 need Phase 7 (V/P producers, `TASKS.md:987-992`); A3 now | 76 |
| **B** email + webhooks | 12, 13, 17, 28, 32, 36–41, 42, 43, 45 | 42/43 need Phase 7 (V); mail half needs P0's mailer | 36 |
| **C** storage, exports, seed | 1, 8, 14, 15, 16, 35, 44, 46 | 35's V/P callers need Phase 7; the rest now | 41 |
| **D** lifecycle, cleanup, logging | 3–7, 9, 10, 19, 20, 27 | none | 30 |

Order: **P0 → (C ∥ D) → A3 → (A1 ∥ B-email) → (A2 ∥ B-webhooks)**. C, D and P0 can start before
Phase 7 finishes; A1/A2/B's webhook half cannot. Then, in one release: drain RabbitMQ, stop the
Python beat, `plane-beat --take-all`, run the beat tick check, stop the Python worker, remove the
broker, flip Caddy, delete the `celery` feature and `lapin` (`DECISIONS.md:230-234`,
`crates/plane-jobs/Cargo.toml:38-43`), and remove `route::dispatch`'s Celery branch
(`route.rs:53-62`) — `plane-drf`'s `ViewCtx.publisher` (`view.rs:95-98,245-248`) and
`bins/plane-api/src/startup.rs:80-92` go with it.

**Risks.** *Email* is the largest unverifiable surface: 11 templates (`templates/emails/`, plus
`test_email.html` for the command), Django's plain-text generation (`utils/email.py:33-61`), the
`EmailMultiAlternatives` headers and the SMTP config read from encrypted instance rows
(`license/utils/instance_value.py:42-59`) must match byte for byte; capture them with a sink, never
by eye. *S3* keys/ACLs are observable (`export_task.py:45-76`, `copy_s3_object.py:98-113`) and the
exports bucket is public-read. *Webhook* logs store Python `repr`, not JSON
(`webhook_task.py:104-115`), the delivery uuid is random, and the retry jitter is 0–600 s —
normalise the uuid, pin everything else. *Time zones*: automation months use UTC
(`issue_automation_task.py:27-155`), snooze midnights use pytz `is_dst=False`
(`PROGRESS.md:915-918`), `hard_delete`/cleanup windows are `days` from `timezone.now()` in UTC.
*Nondeterminism*: `workspace_seed` adds a bot user and project per run (`TASKS.md:574`),
`copy_s3_object` mints new uuid asset keys, `page_transaction`/`version` re-use a 600 s window —
differential steps must be one-shot, never repeated. *Upstream bugs to keep*: `recent_visited_task`
stops trimming at 21 rows (`recent_visited_task.py:37-44`), `delete_old_s3_link` runs twice
(`celery.py:69-72,97-100`), `hard_delete` deletes issues before their `DO_NOTHING` activity rows
and probably violates that FK (`deletion_task.py:150,164`; `PROGRESS.md:554-556`) — reproduce, do
not fix, and record the verification on a real database, `is_valid_uuid` is v4-only so
`issue_activity` drops other ids (`PROGRESS.md:594`), draft→issue publishes `project_id="None"`
(`PROGRESS.md:550`), `model_activity` compares a dict to a JSON string (`TASKS.md:1186-1189`),
`Workspace.delete` can overflow the slug (`PROGRESS.md:557-558`), and the cascade nulls
`created_by`/`updated_by` (`PROGRESS.md:560`).
