# TASKS.md — the Celery inventory

Every Celery task, schedule entry and message detail the Rust `plane-worker` / `plane-beat`
and the coexistence publisher in `plane-jobs` have to reproduce. Evidence comes from the
pinned Python dependencies, not from memory: Celery **5.5.3**, kombu **5.5.4**, amqp
**5.4.0**, `django-celery-beat` **2.9.0**, all in `apps/api/.venv`.

Two generated artefacts back this document:

| Artefact | What it is |
| --- | --- |
| `apps/server/tools/inventory/celery_messages.json` | The exact messages Celery publishes for seven representative enqueues, captured over kombu's in-memory transport (no broker). |
| `apps/server/tools/inventory/celery_capture.py` | The script that produced it. Re-run it after any dependency bump; every protocol detail below is asserted from its output. |

Task names were verified by introspection, not by reading the source: importing the Plane
Celery app, running `app.autodiscover_tasks()` + `app.loader.import_default_modules()`
(what the worker does at startup) and printing `sorted(app.tasks)`.

## 0. Numbers

| Quantity | Value |
| --- | --- |
| `@shared_task` definitions under `apps/api/plane/` | **47** (`RUST_REWRITE.md` says 48; the likely extra one is the commented-out `# @shared_task` at `plane/bgtasks/deletion_task.py:108`, whose function `restore_related_objects` is an empty stub — it is not a task. 47 real decorators, 46 of them registered) |
| Tasks registered on the worker at startup | **46** |
| Tasks defined but never registered | **1** — `plane.bgtasks.project_invitation_task.project_invitation` (its module is imported by nothing, so the worker would reject the message as unregistered) |
| Distinct tasks enqueued from application code | **32** |
| Beat schedule entries | **13** (12 distinct tasks; `delete_old_s3_link` is scheduled twice) |
| Periodic rows the beat scheduler creates in Postgres | **14** (the 13 above plus `celery.backend_cleanup`, see §7.2) |
| Tasks that are neither enqueued nor scheduled | **3** (+1 unregistered) — see §8.2 |
| Call sites of `.delay()` / `.apply_async()` outside tests | **166** across 51 files (165 on real tasks + the broken one in §8.1) |

There are **no** `send_task`, `signature`, `chain`, `group` or `chord` uses in the backend.
(`chain(` at `plane/app/views/issue/activity.py:97` is `itertools.chain`.) Every enqueue is a
direct `.delay()`/`.apply_async()` on an imported task object, so the publisher never has to
deal with canvas.

---

## 1. Celery configuration

### 1.1 The app object

`apps/api/plane/celery.py`:

| Line | Code | Effect |
| --- | --- | --- |
| 20 | `os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")` | Settings module when the worker/beat starts without one. |
| 22 | `ri = redis_instance()` | A `redis.Redis` **client** is built at import time (lazy: no socket until used). It is dead code — `ri` is never referenced again in the file. |
| 40 | `app = Celery("plane")` | `main = "plane"`. Constructing a `Celery` instance also makes it the current app, so every `@shared_task` proxy binds to it. |
| 44 | `app.config_from_object("django.conf:settings", namespace="CELERY")` | Every Django setting prefixed `CELERY_` becomes a Celery config key. |
| 46–101 | `app.conf.beat_schedule = {...}` | 13 static entries (§7). |
| 105–118 | `@after_setup_logger.connect` / `@after_setup_task_logger.connect` | Both replace the formatter with `pythonjsonlogger.json.JsonFormatter` on a plain `StreamHandler`. |
| 122 | `app.autodiscover_tasks()` | Walks every `INSTALLED_APPS` entry, importing `<app>`. |
| 124 | `app.conf.beat_scheduler = "django_celery_beat.schedulers.DatabaseScheduler"` | Set **after** `config_from_object`; overrides any `CELERY_BEAT_SCHEDULER`. |

`apps/api/plane/__init__.py:5` does `from .celery import app as celery_app`, which is what
`celery -A plane` resolves.

### 1.2 Every `CELERY_*` setting

From `apps/api/plane/settings/common.py:362-387` (nothing else in `plane/settings/` sets a
`CELERY_*` key; `local.py`, `production.py` and `test.py` only inherit).

```python
# 354-360
RABBITMQ_HOST     = os.environ.get("RABBITMQ_HOST", "localhost")
RABBITMQ_PORT     = os.environ.get("RABBITMQ_PORT", "5672")
RABBITMQ_USER     = os.environ.get("RABBITMQ_USER", "guest")
RABBITMQ_PASSWORD = os.environ.get("RABBITMQ_PASSWORD", "guest")
RABBITMQ_VHOST    = os.environ.get("RABBITMQ_VHOST", "/")
AMQP_URL          = os.environ.get("AMQP_URL")

# 362-366
if AMQP_URL:
    CELERY_BROKER_URL = AMQP_URL
else:
    CELERY_BROKER_URL = f"amqp://{RABBITMQ_USER}:{RABBITMQ_PASSWORD}@{RABBITMQ_HOST}:{RABBITMQ_PORT}/{RABBITMQ_VHOST}"
```

| Setting | Value | Source |
| --- | --- | --- |
| `broker_url` | `amqp://<RABBITMQ_USER>:<PASSWORD>@<HOST>:<PORT>/<VHOST>` or `AMQP_URL` verbatim | `common.py:362-366` |
| `timezone` (`CELERY_TIMEZONE`) | `"UTC"` (`CELERY_TIMEZONE = TIME_ZONE`, `common.py:319`) | `common.py:368` |
| `task_serializer` | `"json"` | `common.py:369` |
| `result_serializer` | `"json"` (inert: no result backend) | `common.py:370` |
| `accept_content` | `["application/json"]` | `common.py:371` |
| `imports` (`CELERY_IMPORTS`) | 9 modules, listed below | `common.py:374-387` |
| `beat_scheduler` | `django_celery_beat.schedulers.DatabaseScheduler` | `celery.py:124` |

`CELERY_IMPORTS`:

```
plane.bgtasks.issue_automation_task
plane.bgtasks.exporter_expired_task
plane.bgtasks.file_asset_task
plane.bgtasks.email_notification_task
plane.bgtasks.cleanup_task
plane.license.bgtasks.telemetry_metrics
plane.bgtasks.dummy_data_task
plane.bgtasks.issue_version_sync
plane.bgtasks.issue_description_version_sync
```

Everything else lands in the registry through Django app-package imports during
autodiscovery (`plane/app/views/...` importing a task, `plane/db/mixins.py` importing
`deletion_task`, and so on). The one module that nothing imports is
`plane/bgtasks/project_invitation_task.py`.

### 1.3 Values that are *not* set — the effective defaults

Read from the live `app.conf` and from `apps/api/.venv/.../celery/app/defaults.py`:

| Key | Effective value | Default in `defaults.py` |
| --- | --- | --- |
| `task_default_queue` | `celery` | line 265 |
| `task_default_exchange` | `None` → resolved from the queue (effective name `celery`) | line 267 |
| `task_default_exchange_type` | `direct` | line 268 |
| `task_default_routing_key` | `None` → resolved from the queue (effective `celery`) | line 269 |
| `task_default_delivery_mode` | `2` (persistent) | line 264 |
| `task_default_priority` | `None` → `priority=0` on the wire | line 271 |
| `task_routes` / `task_queues` | `None` / `None` — **single queue, no routing** | line 288 / 292 |
| `task_protocol` | `2` | line 277 |
| `task_acks_late` | `False` | line 257 |
| `task_acks_on_failure_or_timeout` | `True` | line 258 |
| `task_reject_on_worker_lost` | `None` | line 290 |
| `task_time_limit` / `task_soft_time_limit` | `None` / `None` | line 300 / 297 |
| `task_ignore_result` | `False` | line 275 |
| `task_always_eager` | `False` | line 259 |
| `worker_prefetch_multiplier` | `4` | line 338 |
| `worker_concurrency` | `None` → number of CPUs | line 316 |
| `worker_max_tasks_per_child` | `None` | line 333 |
| `worker_hijack_root_logger` | `True` | line 328 |
| `result_backend` (`CELERY_RESULT_BACKEND`) | **`None`** → `DisabledBackend`; results are never stored | `django_celery_results` is installed in the venv but is **not** in `INSTALLED_APPS` |
| `result_expires` | `1 day` | line 211-214 (used only by `install_default_entries`, see §7.2) |
| `beat_schedule_filename` | `celerybeat-schedule` (unused with `DatabaseScheduler`) | |
| `broker_heartbeat` | `120` s | |
| `task_publish_retry` / policy | `True`; `{'max_retries': 3, 'interval_start': 0, 'interval_max': 1, 'interval_step': 0.2}` | lines 278-286 |
| `event_queue_ttl` | `5.0` | |

Consequences the Rust side must inherit:

* One queue, `celery`. No priority queues, no per-task queues, no routing keys.
* Nothing is retried except `webhook_send_task` (see §4). Celery's `acks_late` is off, so a
  task is acked before it runs: **a worker crash mid-task silently loses the job**.
* No time limits: a hung task pins a worker slot forever.
* No result backend: `ignore_result=False` still makes each message carry a `reply_to`, but
  nothing reads it. A Rust publisher may set `reply_to` to the same dummy value.

### 1.4 Worker and beat entrypoints

`apps/api/bin/docker-entrypoint-worker.sh`:

```bash
python manage.py wait_for_db
python manage.py wait_for_migrations
celery -A plane worker -l info
```

`apps/api/bin/docker-entrypoint-beat.sh` is the same with `celery -A plane beat -l info`.

There are no extra flags, no `-Q`, no `-c`, no `--max-tasks-per-child`, and no
`CELERYD_*`/`CELERY_*` overrides in `docker-compose.yml`, `docker-compose-local.yml` or
`apps/api/.env.example`. So:

* concurrency = `os.cpu_count()` (documented idle measurement in `RUST_REWRITE.md` says
  concurrency 4);
* prefetch = 4 × concurrency;
* `wait_for_migrations` (`plane/db/management/commands/wait_for_migrations.py`) blocks the
  worker and beat until `MigrationExecutor.migration_plan()` is empty — a Rust worker must
  do the same and must not process messages against a half-migrated schema;
* `-l info` gives Celery's own startup banner plus the JSON formatters from §1.1.

### 1.5 Signal hooks

Only two, both for logging (`celery.py:104-118`). No `worker_process_init`,
`worker_ready`, `worker_shutdown`, `task_prerun`/`postrun`, no `setup_logging` override, no
`worker_init`. Nothing needs porting to Rust beyond "log JSON".

Note `plane/bgtasks/apps.py` is an empty `AppConfig`; there is no `ready()` hook.

### 1.6 Beat scheduler

`DatabaseScheduler` (`django_celery_beat/schedulers.py:257-259`, with `Entry.from_entry` doing a
`PeriodicTask.update_or_create` at `schedulers.py:191-195`) does
`setup_schedule()` → `install_default_entries()` → `update_from_dict(app.conf.beat_schedule)`,
i.e. on every beat start the **static** entries in `celery.py` are upserted into
`django_celery_beat_periodictask` (creating `CrontabSchedule`/`IntervalSchedule` rows), and
the database then wins. A self-hosted deployment therefore already has these rows even
though no migration creates them, and an operator can edit or disable any of them from the
admin without a redeploy. `plane-beat` must read the same rows if it is to respect those
edits — or the cutover has to freeze them.

Relevant tables (default names, `django_celery_beat` app label):
`django_celery_beat_periodictask`, `_periodictasks`, `_crontabschedule`,
`_intervalschedule`, `_clockedschedule`, `_solarschedule`.

---

## 2. The Celery message protocol, as observed

Captured in `apps/server/tools/inventory/celery_messages.json`. Everything below is
verified against that file.

### 2.1 Which protocol version

`task_protocol = 2` (Celery 5.5.3 default, `celery/app/defaults.py:277`). Version 2 means the
body is a **3-element array** `[args, kwargs, embed]`, not the v1 dict.

### 2.2 AMQP properties

`celery/app/amqp.py:351-356` builds `properties={'correlation_id': task_id, 'reply_to': ...}`;
kombu adds `delivery_mode` and `priority`; the AMQP transport adds nothing else.

| Property | Value | Where set |
| --- | --- | --- |
| `content_type` | `"application/json"` | `kombu/serialization.py:302-308` (`register_json`) |
| `content_encoding` | `"utf-8"` | same |
| `delivery_mode` | `2` (persistent) | `celery/app/amqp.py:446,488-493` → `kombu/entity.py:32-38` (`maybe_delivery_mode`) → default `2` |
| `priority` | `0` | `kombu/messaging.py:123` (`publish(..., priority=0)`) |
| `correlation_id` | the task id | `celery/app/amqp.py:354` |
| `reply_to` | a per-process reply-queue name, e.g. `ea5a11e3-f79c-3a1f-aeba-3a14f77fd583` | `celery/app/base.py:912` passes `reply_to or self.thread_oid`; `amqp.py:355` turns it into the property. With `result_backend=None` nothing consumes it |
| `expiration` | absent (only when `expires=` is given) | `kombu/messaging.py:176-177` |
| `message_id`, `timestamp`, `app_id`, `type`, `user_id`, `cluster_id` | absent | never set by Celery 5 |

### 2.3 Headers

Built literally as a dict in `celery/app/amqp.py:329-349`:

| Header | Type | Value in Plane |
| --- | --- | --- |
| `lang` | str | `"py"` |
| `task` | str | the registered task name |
| `id` | str (uuid4) | the task id, same as `correlation_id` |
| `shadow` | null | always `null` (no task uses `shadow_name`) |
| `eta` | str/null | ISO-8601 **with microseconds and `+00:00`**, e.g. `2026-11-15T17:29:04.157757+00:00`. Set by `apply_async(countdown=...)` only (§2.4) |
| `expires` | str/null | always `null` (no `expires=` anywhere) |
| `group` | null | `null` |
| `group_index` | null | `null` |
| `retries` | int | `0` on publish; the worker increments it when retrying |
| `timelimit` | `[null, null]` | `[time_limit, soft_time_limit]` |
| `root_id` | str | equals `id` when not published as part of a chain |
| `parent_id` | null | `null` |
| `argsrepr` | str | Python `repr` of args, e.g. `('invitee@example.com', UUID('6a1b...'), ...)`, truncated to 1024 chars |
| `kwargsrepr` | str | Python `repr` of kwargs, e.g. `{'asset_id': '9999...'}`, truncated to 1024 chars |
| `origin` | str | `gen<pid>@<hostname>`, e.g. `gen98652@MacBook-Pro.local` |
| `ignore_result` | bool | `false` |
| `replaced_task_nesting` | int | `0` |
| `stamped_headers` | null | `null` |
| `stamps` | dict | `{}` |

`argsrepr`/`kwargsrepr`/`origin` are informational — the worker never parses them. The
differential harness must normalise them (see §2.7).

### 2.4 Body

```
[ <args>, <kwargs>, {"callbacks": null, "errbacks": null, "chain": null, "chord": null} ]
```

(`celery/app/amqp.py:357-364`.)

Serialisation is `kombu.utils.json.dumps` = `json.dumps(value, cls=kombu.utils.json.JSONEncoder)`
with **default separators**, i.e. `", "` and `": "` — the body has spaces after every comma
and colon. Example (byte-for-byte from the capture):

```json
[[], {"asset_id": "99999999-8888-7777-6666-555555555555"}, {"callbacks": null, "errbacks": null, "chain": null, "chord": null}]
```

`countdown=` is converted to an `eta` header at publish time — there is no `countdown` key in
the body (`celery/app/amqp.py:300-306`).

### 2.5 Enumeration values inside the body

kombu's JSON encoder (`kombu/utils/json.py:19-57, 125-146`) is **not** plain JSON:

| Python value | Wire form |
| --- | --- |
| `uuid.UUID` | `{"__type__": "uuid", "__value__": {"hex": "6a1b6f0e0f9a4b629a391b2c3d4e5f60"}}` — **not** a bare string, and the hex has no dashes |
| `datetime` | `{"__type__": "datetime", "__value__": "2026-11-15T17:24:04.144120+00:00"}` |
| `date` | `{"__type__": "date", "__value__": "2026-11-20"}` |
| `time` | `{"__type__": "time", "__value__": "..."}` |
| `Decimal` | `{"__type__": "decimal", "__value__": "1.5"}` |
| `bytes` | UTF-8 decodable → `{"__type__": "bytes", "__value__": "..."}`, else `{"__type__": "base64", "__value__": "..."}` |
| Django `Promise`/lazy strings | `str(...)` |

This matters because several call sites pass model attributes straight through instead of
`str(...)` — see §8.3. The Python worker decodes the envelope back into a `uuid.UUID`, so the
task body sees a `UUID` in one case and a `str` in the other; `Model.objects.get(pk=...)`
accepts both, which is why nothing fails today, but the bytes differ and any `==` comparison
inside a task sees a different type.

### 2.6 Exchange and routing key

`celery/app/amqp.py:501-502`:

```python
if (not exchange or not routing_key) and exchange_type == 'direct':
    exchange, routing_key = '', qname
```

No task sets `queue=`/`exchange=`, the default exchange type is `direct`, and the default
queue is `celery`, so **every** message is published to the nameless default exchange with
routing key `celery`:

```
exchange = ""        # default direct exchange
routing_key = "celery"
```

Confirmed in the capture (`properties.delivery_info`). The effective
`task_default_exchange`/`task_default_routing_key` values (`celery`/`celery`) are *not* used
on the wire — do not publish to a `celery` exchange.

The queue is durable, non-auto-delete, no arguments (RabbitMQ defaults from
`kombu/entity.py` `Queue`). A Rust publisher should declare it idempotently on connect.

### 2.7 What the Rust publisher must match, and what the harness must normalise

Byte-exact (the Python worker's behaviour depends on it, or a differential test will see it):

* `content_type` / `content_encoding` — `application/json` / `utf-8`
* `delivery_mode=2`, `priority=0`
* exchange `""`, routing key `celery`
* body shape, key order **and separators** (`", "` / `": "`), `null` for absent callbacks
* every header value that the task reads: `task`, `id`, `eta`, `retries`, `root_id`,
  `timelimit`
* UUID/`datetime`/`Decimal` enumerations *if* the Rust caller replaces a Python caller that
  passed a non-string: the task body then receives `uuid.UUID` instead of `str` (or the
  reverse). Match the call site you are replacing — §5 lists the type per call site.

Normalise in the differential harness (legitimately non-deterministic):

* `id`, `root_id`, `correlation_id` (new uuid4 per publish)
* `reply_to` (per-process uuid)
* `origin` (`gen<pid>@<host>`)
* `argsrepr` / `kwargsrepr` (Python `repr`, e.g. `UUID('…')` which Rust cannot produce)
* `eta` when the caller used `countdown=` (depends on the publish instant)
* header ordering and body key ordering, if the comparison is textual

The queue is `celery`, the exchange is `""`, and the routing key is `celery` — nothing else.

### 2.8 Captured messages

`apps/server/tools/inventory/celery_messages.json` has, for each of seven cases, the raw
properties, the headers, the body as decoded JSON **and** the raw body bytes as base64:

| case | task | what it demonstrates |
| --- | --- | --- |
| `workspace_invitation positional arguments` | `...workspace_invitation_task.workspace_invitation` | five positional args, one of them a `UUID` object → type envelope |
| `get_asset_object_metadata kwargs` | `...storage_metadata_task.get_asset_object_metadata` | kwargs-only, `str(uuid)` |
| `issue_activity with JSON-string payloads` | `...issue_activities_task.issue_activity` | `json.dumps(..., cls=DjangoJSONEncoder)` embedded as a **string** argument, `epoch` int, `notification` bool |
| `sync_issue_version apply_async with countdown` | `...issue_version_sync.sync_issue_version` | `apply_async(kwargs=..., countdown=300)` → `eta` header, `kwargs` dict |
| `send_email_notification nested dict arguments` | `...email_notification_task.send_email_notification` | nested dict/list payload, `list[UUID]` |
| `webhook_activity project deleted` | `...webhook_task.webhook_activity` | `UUID` for `actor_id` and `event_id` (the `str()` is missing at the call site) |
| `delete_api_logs no arguments` | `...cleanup_task.delete_api_logs` | a beat-style no-argument message: `[[], {}, {...}]` |

---

## 3. Registered task names (introspection)

`sorted(app.tasks)` minus the `celery.*` built-ins, after
`app.autodiscover_tasks()` + `app.loader.import_default_modules()`:

```
plane.bgtasks.analytic_plot_export.analytic_export_task
plane.bgtasks.analytic_plot_export.export_analytics_to_csv_email
plane.bgtasks.cleanup_task.delete_api_logs
plane.bgtasks.cleanup_task.delete_email_notification_logs
plane.bgtasks.cleanup_task.delete_issue_description_versions
plane.bgtasks.cleanup_task.delete_page_versions
plane.bgtasks.cleanup_task.delete_webhook_logs
plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets
plane.bgtasks.deletion_task.hard_delete
plane.bgtasks.deletion_task.soft_delete_related_objects
plane.bgtasks.dummy_data_task.create_dummy_data
plane.bgtasks.email_notification_task.send_email_notification
plane.bgtasks.email_notification_task.stack_email_notification
plane.bgtasks.export_task.issue_export_task
plane.bgtasks.exporter_expired_task.delete_old_s3_link
plane.bgtasks.file_asset_task.delete_unuploaded_file_asset
plane.bgtasks.forgot_password_task.forgot_password
plane.bgtasks.issue_activities_task.issue_activity
plane.bgtasks.issue_automation_task.archive_and_close_old_issues
plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues
plane.bgtasks.issue_description_version_sync.schedule_issue_description_version
plane.bgtasks.issue_description_version_sync.sync_issue_description_version
plane.bgtasks.issue_description_version_task.issue_description_version_task
plane.bgtasks.issue_version_sync.issue_task
plane.bgtasks.issue_version_sync.schedule_issue_version
plane.bgtasks.issue_version_sync.sync_issue_version
plane.bgtasks.logger_task.process_logs
plane.bgtasks.magic_link_code_task.magic_link
plane.bgtasks.notification_task.notifications
plane.bgtasks.page_transaction_task.page_transaction
plane.bgtasks.page_version_task.track_page_version
plane.bgtasks.project_add_user_email_task.project_add_user_email
plane.bgtasks.recent_visited_task.recent_visited_task
plane.bgtasks.storage_metadata_task.get_asset_object_metadata
plane.bgtasks.user_activation_email_task.user_activation_email
plane.bgtasks.user_deactivation_email_task.user_deactivation_email
plane.bgtasks.user_email_update_task.send_email_update_confirmation
plane.bgtasks.user_email_update_task.send_email_update_magic_code
plane.bgtasks.webhook_task.model_activity
plane.bgtasks.webhook_task.send_webhook_deactivation_email
plane.bgtasks.webhook_task.webhook_activity
plane.bgtasks.webhook_task.webhook_send_task
plane.bgtasks.work_item_link_task.crawl_work_item_link_title
plane.bgtasks.workspace_invitation_task.workspace_invitation
plane.bgtasks.workspace_seed_task.workspace_seed
plane.license.bgtasks.telemetry_metrics.push_instance_metrics
```

46 names. Every one appears in §4, and §4 contains nothing else. The 47th definition,
`plane.bgtasks.project_invitation_task.project_invitation`, is intentionally absent: its
module is never imported, so no worker knows the name (see §8.1).

How to reproduce:

```bash
cd apps/api
DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=x \
DATABASE_URL=postgresql://x:x@127.0.0.1:1/x REDIS_URL=redis://127.0.0.1:1/0 \
.venv/bin/python - <<'PY'
import os, django
django.setup()
from plane.celery import app
app.autodiscover_tasks(); app.loader.import_default_modules()
print("\n".join(sorted(n for n in app.tasks if not n.startswith("celery."))))
PY
```

---

## 4. Task table

Every registered task, one row each. "Default" in a column means the value from §1.3:
queue `celery`, `acks_late=False`, no time limit, no explicit retry. None of the 47 tasks
sets `queue=`, `acks_late=`, `time_limit=`, `soft_time_limit=`, `priority=`, `expires=` or
`rate_limit=`; the only decorator with options is `webhook_send_task`.

### 4.1 Mechanics

| # | task | file:line | signature (with defaults) | bind / retry | acks_late | time limits | queue |
| - | --- | --- | --- | --- | --- | --- | --- |
| 1 | `analytic_plot_export.analytic_export_task` | `plane/bgtasks/analytic_plot_export.py:350` | `(email, data, slug, user_id)` | none | False | none | `celery` |
| 2 | `analytic_plot_export.export_analytics_to_csv_email` | `plane/bgtasks/analytic_plot_export.py:416` | `(data, headers, keys, email, slug)` | none | False | none | `celery` |
| 3 | `cleanup_task.delete_api_logs` | `plane/bgtasks/cleanup_task.py:162` | `()` | none | False | none | `celery` |
| 4 | `cleanup_task.delete_email_notification_logs` | `plane/bgtasks/cleanup_task.py:172` | `()` | none | False | none | `celery` |
| 5 | `cleanup_task.delete_page_versions` | `plane/bgtasks/cleanup_task.py:182` | `()` | none | False | none | `celery` |
| 6 | `cleanup_task.delete_issue_description_versions` | `plane/bgtasks/cleanup_task.py:192` | `()` | none | False | none | `celery` |
| 7 | `cleanup_task.delete_webhook_logs` | `plane/bgtasks/cleanup_task.py:202` | `()` | none | False | none | `celery` |
| 8 | `copy_s3_object.copy_s3_objects_of_description_and_assets` | `plane/bgtasks/copy_s3_object.py:127` | `(entity_name, entity_identifier, project_id, slug, user_id)` | none | False | none | `celery` |
| 9 | `deletion_task.soft_delete_related_objects` | `plane/bgtasks/deletion_task.py:18` | `(app_label, model_name, instance_pk, using=None)` | none | False | none | `celery` |
| 10 | `deletion_task.hard_delete` | `plane/bgtasks/deletion_task.py:114` | `()` | none | False | none | `celery` |
| 11 | `dummy_data_task.create_dummy_data` | `plane/bgtasks/dummy_data_task.py:488` | `(slug, email, members, issue_count, cycle_count, module_count, pages_count, intake_issue_count)` | none | False | none | `celery` |
| 12 | `email_notification_task.stack_email_notification` | `plane/bgtasks/email_notification_task.py:47` | `()` | none | False | none | `celery` |
| 13 | `email_notification_task.send_email_notification` | `plane/bgtasks/email_notification_task.py:153` | `(issue_id, notification_data, receiver_id, email_notification_ids)` | none | False | none | `celery` |
| 14 | `export_task.issue_export_task` | `plane/bgtasks/export_task.py:129` | `(provider: str, workspace_id: uuid.UUID, project_ids: List[str], token_id: str, multiple: bool, slug: str)` | none | False | none | `celery` |
| 15 | `exporter_expired_task.delete_old_s3_link` | `plane/bgtasks/exporter_expired_task.py:23` | `()` | none | False | none | `celery` |
| 16 | `file_asset_task.delete_unuploaded_file_asset` | `plane/bgtasks/file_asset_task.py:21` | `()` | none | False | none | `celery` |
| 17 | `forgot_password_task.forgot_password` | `plane/bgtasks/forgot_password_task.py:23` | `(first_name, email, uidb64, token, current_site)` | none | False | none | `celery` |
| 18 | `issue_activities_task.issue_activity` | `plane/bgtasks/issue_activities_task.py:1550` | `(type, requested_data, current_instance, issue_id, actor_id, project_id, epoch, subscriber=True, notification=False, origin=None, intake=None)` | none | False | none | `celery` |
| 19 | `issue_automation_task.archive_and_close_old_issues` | `plane/bgtasks/issue_automation_task.py:27` | `()` | none | False | none | `celery` |
| 20 | `issue_automation_task.unsnooze_expired_intake_issues` | `plane/bgtasks/issue_automation_task.py:157` | `()` | none | False | none | `celery` |
| 21 | `issue_description_version_sync.sync_issue_description_version` | `plane/bgtasks/issue_description_version_sync.py:40` | `(batch_size=5000, offset=0, countdown=300)` | none; re-enqueues itself | False | none | `celery` |
| 22 | `issue_description_version_sync.schedule_issue_description_version` | `plane/bgtasks/issue_description_version_sync.py:124` | `(batch_size=5000, countdown=300)` | none | False | none | `celery` |
| 23 | `issue_description_version_task.issue_description_version_task` | `plane/bgtasks/issue_description_version_task.py:44` | `(updated_issue, issue_id, user_id, is_creating=False) -> Optional[bool]` | none | False | none | `celery` |
| 24 | `issue_version_sync.issue_task` | `plane/bgtasks/issue_version_sync.py:34` | `(updated_issue, issue_id, user_id)` | none | False | none | `celery` |
| 25 | `issue_version_sync.sync_issue_version` | `plane/bgtasks/issue_version_sync.py:181` | `(batch_size=5000, offset=0, countdown=300)` | none; re-enqueues itself | False | none | `celery` |
| 26 | `issue_version_sync.schedule_issue_version` | `plane/bgtasks/issue_version_sync.py:235` | `(batch_size=5000, countdown=300)` | none | False | none | `celery` |
| 27 | `logger_task.process_logs` | `plane/bgtasks/logger_task.py:33` | `(log_data: Dict[str, Any], **_: Any) -> None` | none | False | none | `celery` |
| 28 | `magic_link_code_task.magic_link` | `plane/bgtasks/magic_link_code_task.py:23` | `(email, key, token)` | none | False | none | `celery` |
| 29 | `notification_task.notifications` | `plane/bgtasks/notification_task.py:191` | `(type, issue_id, project_id, actor_id, subscriber, issue_activities_created, requested_data, current_instance)` | none | False | none | `celery` |
| 30 | `page_transaction_task.page_transaction` | `plane/bgtasks/page_transaction_task.py:85` | `(new_description_html, old_description_html, page_id)` | none | False | none | `celery` |
| 31 | `page_version_task.track_page_version` | `plane/bgtasks/page_version_task.py:22` | `(page_id, existing_instance, user_id)` | none | False | none | `celery` |
| 32 | `project_add_user_email_task.project_add_user_email` | `plane/bgtasks/project_add_user_email_task.py:25` | `(current_site, project_member_id, invitor_id)` | none | False | none | `celery` |
| 33 | `recent_visited_task.recent_visited_task` | `plane/bgtasks/recent_visited_task.py:18` | `(entity_name, entity_identifier, user_id, project_id, slug)` | none | False | none | `celery` |
| 34 | `storage_metadata_task.get_asset_object_metadata` | `plane/bgtasks/storage_metadata_task.py:15` | `(asset_id)` | none | False | none | `celery` |
| 35 | `user_activation_email_task.user_activation_email` | `plane/bgtasks/user_activation_email_task.py:23` | `(current_site, user_id)` | none | False | none | `celery` |
| 36 | `user_deactivation_email_task.user_deactivation_email` | `plane/bgtasks/user_deactivation_email_task.py:23` | `(current_site, user_id)` | none | False | none | `celery` |
| 37 | `user_email_update_task.send_email_update_magic_code` | `plane/bgtasks/user_email_update_task.py:22` | `(email, token)` | none | False | none | `celery` |
| 38 | `user_email_update_task.send_email_update_confirmation` | `plane/bgtasks/user_email_update_task.py:67` | `(email)` | none | False | none | `celery` |
| 39 | `webhook_task.send_webhook_deactivation_email` | `plane/bgtasks/webhook_task.py:172` | `(webhook_id: str, receiver_id: str, current_site: str, reason: str) -> None` | none | False | none | `celery` |
| 40 | `webhook_task.webhook_send_task` | `plane/bgtasks/webhook_task.py:242` | `(webhook_id: str, slug: str, event: str, event_data: Optional[Dict], action: str, current_site: str, activity: Optional[Dict]) -> None` (+ `self`) | **bind=True; `autoretry_for=(requests.RequestException,)`, `retry_backoff=600`, `max_retries=5`, `retry_jitter=True`** | False | none | `celery` |
| 41 | `webhook_task.webhook_activity` | `plane/bgtasks/webhook_task.py:393` | `(event, verb, field, old_value, new_value, actor_id, slug, current_site, event_id, old_identifier, new_identifier) -> None` | none | False | none | `celery` |
| 42 | `webhook_task.model_activity` | `plane/bgtasks/webhook_task.py:479` | `(model_name, model_id, requested_data, current_instance, actor_id, slug, origin=None)` | none | False | none | `celery` |
| 43 | `work_item_link_task.crawl_work_item_link_title` | `plane/bgtasks/work_item_link_task.py:249` | `(id: str, url: str) -> None` | none | False | none | `celery` |
| 44 | `workspace_invitation_task.workspace_invitation` | `plane/bgtasks/workspace_invitation_task.py:23` | `(email, workspace_id, token, current_site, inviter)` | none | False | none | `celery` |
| 45 | `workspace_seed_task.workspace_seed` | `plane/bgtasks/workspace_seed_task.py:505` | `(workspace_id: uuid.UUID) -> None` | none | False | none | `celery` |
| 46 | `license.bgtasks.telemetry_metrics.push_instance_metrics` | `plane/license/bgtasks/telemetry_metrics.py:369` | `()` | none | False | none | `celery` |

All names above carry the `plane.bgtasks.` prefix except #46, which is
`plane.license.bgtasks.telemetry_metrics.push_instance_metrics`.

### 4.2 Retry policy in detail

There is exactly one retrying task. `webhook_send_task`:

* `autoretry_for=(requests.RequestException,)` — the wrapper in
  `celery/app/autoretry.py:36-71` catches it and calls `task.retry()`;
* `max_retries=5`, so up to 6 attempts;
* `retry_backoff=600` and `retry_backoff_max` **unset**, which defaults to `600`
  (`celery/app/autoretry.py:25-28`). Since `get_exponential_backoff_interval` computes
  `min(maximum, factor * 2**retries)` with `factor = max(1.0, 600) = 600`, the countdown is
  **600 capped at 600 for every retry** and then full-jittered:
  `random.randrange(0, 601)` seconds (`celery/utils/time.py:438-452`). So the effective
  policy is "retry up to 5 times, 0–600 s apart at random", not exponential backoff — this
  looks like an upstream bug (`retry_backoff=600` reads as "minimum 10 minutes" but the cap
  makes it "at most 10 minutes, uniformly random").
* On the last attempt (`self.request.retries >= self.max_retries`) it sets
  `Webhook.is_active=False`, enqueues `send_webhook_deactivation_email` and returns instead
  of raising.
* A `ValueError` from SSRF validation is **not** retried and does **not** deactivate the
  webhook (`webhook_task.py:368-385`).

Every other task swallows its exceptions with
`plane/utils/exception_logger.py:log_exception` (which only logs, never re-raises) and
returns `None`, so Celery sees success. Any failure — database, SMTP, S3 — is silently
dropped. `workspace_seed` is the exception: it logs and `raise`s, so the job lands in the
`FAILURE` state (with no result backend, that means nowhere).

### 4.3 Idempotency and side effects

"Tables" are the Django models (`plane/db/models`); the Rust port must write the same rows.

| # | task | idempotent if run twice? | side effects |
| - | --- | --- | --- |
| 1 | `analytic_export_task` | harmless duplicate | Reads `Issue` (filtered by `issue_filters` + member scope); renders a CSV in memory; **sends one email with the CSV attached**. No DB writes. |
| 2 | `export_analytics_to_csv_email` | harmless duplicate | Same, from supplied rows. **Never enqueued by anything** (dead). |
| 3 | `delete_api_logs` | yes | Hard-deletes `APIActivityLog` older than `API_ACTIVITY_LOG_RETENTION_DAYS` (14) in batches of 500 via `all_objects.delete()`. |
| 4 | `delete_email_notification_logs` | yes | Hard-deletes `EmailNotificationLog` with `sent_at <= now - EMAIL_LOG_RETENTION_DAYS` (7). |
| 5 | `delete_page_versions` | yes | Hard-deletes `PageVersion` rows ranked > 20 per `page_id` (window function), batches of 500. |
| 6 | `delete_issue_description_versions` | yes | Same for `IssueDescriptionVersion` (rank > 20 per `issue_id`). |
| 7 | `delete_webhook_logs` | yes | Hard-deletes `WebhookLog` older than `WEBHOOK_LOG_RETENTION_DAYS` (14). |
| 8 | `copy_s3_objects_of_description_and_assets` | **no** | For a duplicated page/issue: parses `<image-component src>` ids, **creates new `FileAsset` rows** (new uuid keys `<workspace>/<uuid4hex>-<name>`), **S3 `copy_object`** for each, rewrites `description_html` src values and saves the entity, then POSTs to `LIVE_URL/convert-document/` and stores `description_json`/`description_binary`. Re-running duplicates every asset again and orphans the previous copies. |
| 9 | `soft_delete_related_objects` | mostly | Walks Django reverse relations of one instance and sets `deleted_at` on every related row with that column, recursing through one-to-one and one-to-many. `SET_NULL` relations are nulled, `DO_NOTHING` skipped. Re-running is a no-op for already-deleted rows. Enqueued by `SoftDeleteModel.delete()` (`plane/db/mixins.py:72-82`) for **every** soft delete in the codebase, including from inside other views. |
| 10 | `hard_delete` | yes | Hard-deletes every row with `deleted_at < now - HARD_DELETE_AFTER_DAYS` (60) for a fixed list of 18 models in dependency order, then loops over *all* models with a `deleted_at` field. Cascades per the FK `on_delete`. |
| 11 | `create_dummy_data` | **no** | Creates a project, members, states, labels, cycles, modules, pages, issues, intake issues. Called **synchronously** by `manage.py create_dummy_data` (`plane/db/management/commands/create_dummy_data.py:59`), never enqueued. |
| 12 | `stack_email_notification` | at-least-once | Reads `EmailNotificationLog` with `processed_at IS NULL`, groups per receiver → issue, enqueues one `send_email_notification` per (receiver, issue), then stamps `processed_at` on all scanned rows. A crash between enqueue and stamp re-sends. |
| 13 | `send_email_notification` | guarded | Redis `SET NX EX 300` on `send_email_notif_<issue>_<receiver>_<sorted ids joined by _>`; reads Redis key `<issue_id>` (the base API URL, written by `issue_activity`, TTL 600) and returns if absent; sends **one email** per (receiver, issue); then `EmailNotificationLog.objects.filter(pk__in=…).update(sent_at=now)`. Lock released at the end, so a re-run after success sends again. |
| 14 | `issue_export_task` | mostly | Reads issues, builds a zip in memory, **writes S3 object** `<workspace_id>/export-<slug>-<token[:6]>-<YYYY-MM-DD>.zip` (`ACL public-read` under MinIO), generates a 7-day presigned URL, updates `ExporterHistory.status/url/key`. Same token + same day overwrites the same key. |
| 15 | `delete_old_s3_link` | yes | `ExporterHistory` with `url` set and `created_at <= now-8d`: **S3 `delete_object`**, then `url = NULL`. |
| 16 | `delete_unuploaded_file_asset` | yes | Deletes `FileAsset` rows with `is_uploaded=False` older than `UNUPLOADED_ASSET_DELETE_DAYS` (7). **No S3 delete** — it is a DB-only cleanup for rows whose upload never completed. |
| 17 | `forgot_password` | harmless duplicate | **Email** (`emails/auth/forgot_password.html`) with `/accounts/reset-password/?uidb64=…&token=…&email=…`. |
| 18 | `issue_activity` | **no** | The activity engine. Writes `IssueActivity` rows (one per changed field, `bulk_create`), `IssueSubscriber` rows for new assignees (`bulk_create ignore_conflicts`), bumps `Issue.updated_at`, refreshes `created_at` of a previous `description` activity instead of adding a new one, sets **Redis** key `<issue_id>` → `origin` with `EX 600`, and if `notification=True` enqueues `notifications`. Re-running duplicates every activity row except the description case. |
| 19 | `archive_and_close_old_issues` | effectively yes | Two passes: bulk `UPDATE Issue.archived_at` for issues idle beyond `Project.archive_in` months, and bulk `UPDATE Issue.state` to `Project.default_state`/first cancelled state beyond `close_in`. Each updated issue then enqueues `issue_activity`. On a second run the filters no longer match. |
| 20 | `unsnooze_expired_intake_issues` | yes | Single `UPDATE IntakeIssue SET status=PENDING, snoozed_till=NULL WHERE status=SNOOZED AND snoozed_till<=now`. No activity rows (deliberate, see the docstring). |
| 21 | `sync_issue_description_version` | **no** | Backfill: for each `Issue` batch (5000) creates an `IssueDescriptionVersion`, then re-enqueues itself with the next offset and the same `countdown`. Running it twice duplicates versions (cleaned up later by task 6). |
| 22 | `schedule_issue_description_version` | yes | One enqueue of task 21. Entry point for `manage.py sync_issue_description_version`. |
| 23 | `issue_description_version_task` | **no** | Creates a new `IssueDescriptionVersion` for the issue, or updates the newest one if the same user saved within 600 s. |
| 24 | `issue_task` | **no** | Same shape for `IssueVersion` (600 s window). **Never enqueued** (superseded by task 25). |
| 25 | `sync_issue_version` | **no** | Backfill: bulk-creates `IssueVersion` rows with assignees/labels/modules/cycle/activity ids, re-enqueues itself with the next offset. |
| 26 | `schedule_issue_version` | yes | One enqueue of task 25. |
| 27 | `process_logs` | **no** | Inserts one `APIActivityLog` row for a request that carried an `X-Api-Key` header (`token_identifier` = HMAC of the key, path, method, query, redacted headers, bodies, status, IP, UA). Enqueued by `APITokenLogMiddleware` on any path, not just `/api/v1/`. |
| 28 | `magic_link` | harmless duplicate | **Email** with the login code (`emails/auth/magic_signin.html`). |
| 29 | `notifications` | **no** | Creates in-app `Notification` rows, `IssueMention` rows (plus deletes removed mentions), `IssueSubscriber` rows for mentioned users, and `EmailNotificationLog` rows (later drained by task 12). Large diffing routine over `requested_data`/`current_instance`; skipped for cycle/module/reaction/vote/draft activity types. |
| 30 | `page_transaction` | mostly | Parses `<mention-component>` / `<image-component>` tags from old and new page HTML; `bulk_create(ignore_conflicts=True)` new `PageLog` rows and deletes logs whose transaction id disappeared. |
| 31 | `track_page_version` | **no** | Creates a `PageVersion` (or updates the newest one for the same user within 600 s) when `description_html` changed; trims to the newest 20. |
| 32 | `project_add_user_email` | harmless duplicate | **Email** "You have been invited to a Plane project". |
| 33 | `project_invitation` | — | Would set `ProjectMemberInvite.message` and send the invitation email. **Not registered on the worker and its only caller is broken** (§8.1). |
| 34 | `recent_visited_task` | mostly | Upserts `UserRecentVisit` (`visited_at = now`), creating it with `created_by_id`/`updated_by_id` set to the user, and trims the oldest row when the count is exactly 20. The `== 20` comparison (not `>=`) means the cap stops working above 20 — upstream bug, ported as-is. Runs on every `GET` of an issue, page, cycle, module, view or project. |
| 35 | `get_asset_object_metadata` | yes | `HEAD` on the S3 object; writes `FileAsset.storage_metadata`. |
| 36 | `user_activation_email` | harmless duplicate | **Email** `emails/user/user_activation.html`. |
| 37 | `user_deactivation_email` | harmless duplicate | **Email** `emails/user/user_deactivation.html`. |
| 38 | `send_email_update_magic_code` | harmless duplicate | **Email** with the verification code. |
| 39 | `send_email_update_confirmation` | harmless duplicate | **Email** `emails/user/email_updated.html`. |
| 40 | `send_webhook_deactivation_email` | harmless duplicate | **Email** `emails/notifications/webhook-deactivate.html`. |
| 41 | `webhook_send_task` | **no** | `POST` to `Webhook.url` through `pinned_fetch` (SSRF-pinned, redirects not followed, 30 s timeout) with `X-Plane-Delivery` (fresh uuid4), `X-Plane-Event`, HMAC-SHA256 `X-Plane-Signature` when `Webhook.secret_key` is set; writes a `WebhookLog` row for every attempt; deactivates the webhook after the retry budget. |
| 42 | `webhook_activity` | **no** (fan-out) | Selects active `Webhook` rows for the workspace filtered by event type (`project`, `issue`, `module`/`module_issue`, `cycle`/`cycle_issue`, `issue_comment`), serialises the entity (`ProjectSerializer`, `IssueExpandSerializer`, …) and enqueues one `webhook_send_task` per webhook. Re-running re-delivers. |
| 43 | `model_activity` | read-only | Diffs `requested_data` against the JSON string `current_instance` and enqueues one `webhook_activity` per changed key with `verb="updated"`, or one `verb="created"` when `current_instance is None`. This is the "model webhook" entry point for project/issue/cycle/module/comment create & update. |
| 44 | `crawl_work_item_link_title` | yes | Fetches the URL (SSRF-pinned, ≤ 5 redirects, 1 s timeout) and the favicon; writes `IssueLink.metadata` = `{title, favicon (data URI), url, favicon_url}`. |
| 45 | `workspace_invitation` | mostly | Sets `WorkspaceMemberInvite.message` and **emails** `emails/invitations/workspace_invitation.html`. |
| 46 | `workspace_seed` | **no** | Creates a `User` bot (`bot_user_<workspace id>`, `is_bot=True`, `bot_type=WORKSPACE_SEED`), a `WorkspaceMember`, a project, states, labels, cycles, modules, issues, views and pages from `seed/` JSON files. Each run adds another bot user and project. Raises on failure (the only task that does). |
| 47 | `push_instance_metrics` | yes | Builds OTLP gauges and pushes them to a telemetry endpoint (`plane/license/bgtasks/telemetry_metrics.py`); network only, no DB writes. `RUST_REWRITE.md` open decision 5 assumes this is **not** ported. |

---

## 5. Callers

For every task that is enqueued somewhere: every call site, the exact argument
expressions, and the runtime type of each argument. Types are stated because the Rust
publisher must hand the Python task the same *value*, and because a UUID serialises
differently from its `str()` (§2.5).

"How it is enqueued from" is the innermost enclosing function; "route / entry point" is the
Django route that reaches it (from `django.urls.get_resolver()`), or `—` for code that is not
a view (tasks, management commands, model hooks, middleware). `UUID` means a `uuid.UUID`
object, not a string. `str(JSON)`/`str(JSON,DJE)` means a Python `str` produced by
`json.dumps(..., cls=DjangoJSONEncoder)` — the argument in the message body is a *string
containing JSON*, escaped again by the outer serialisation.

<!-- callers -->
#### `plane.bgtasks.analytic_plot_export.analytic_export_task` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/analytic/base.py:256` | `ExportAnalyticsEndpoint.post` | POST /api/workspaces/<str:slug>/export-analytics/ | `email=request.user.email`, `data=request.data`, `slug=slug`, `user_id=str(request.user.id)` | `(str, dict, str, str)` |

#### `plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/page/base.py:617` | `PageDuplicateEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/<uuid:page_id>/duplicate/ | `entity_name='PAGE'`, `entity_identifier=page.id`, `project_id=project_id`, `slug=slug`, `user_id=request.user.id` | `(str, UUID, UUID, str, UUID)` |

#### `plane.bgtasks.deletion_task.soft_delete_related_objects` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/db/mixins.py:78` | `SoftDeleteModel.delete` | — | `self._meta.app_label`, `self._meta.model_name`, `self.pk`, `using=using` | `(str, str, UUID, null\|str)` |

#### `plane.bgtasks.email_notification_task.send_email_notification` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/bgtasks/email_notification_task.py:76` | `stack_email_notification` | — | `issue_id=issue_id`, `notification_data=notification_data`, `receiver_id=receiver_id`, `email_notification_ids=email_notification_ids` | `(UUID, dict, str, list[UUID])` |

#### `plane.bgtasks.export_task.issue_export_task` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/exporter/base.py:50` | `ExportIssuesEndpoint.post` | POST /api/workspaces/<str:slug>/export-issues/ | `provider=exporter.provider`, `workspace_id=workspace.id`, `project_ids=project_ids`, `token_id=exporter.token`, `multiple=multiple`, `slug=slug` | `(str, UUID, list[str], str, bool, str)` |

#### `plane.bgtasks.forgot_password_task.forgot_password` — 2 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/authentication/views/app/password_management.py:87` | `ForgotPasswordEndpoint.post` | POST /auth/forgot-password/ | `user.first_name`, `user.email`, `uidb64`, `token`, `current_site` | `(str, str, str, str, str)` |
| 2 | `plane/authentication/views/space/password_management.py:99` | `ForgotPasswordSpaceEndpoint.post` | POST /auth/spaces/forgot-password/ | `user.first_name`, `user.email`, `uidb64`, `token`, `current_site` | `(str, str, str, str, str)` |

#### `plane.bgtasks.issue_activities_task.issue_activity` — 78 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/api/views/cycle.py:632` | `CycleDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:pk>/ | `type='cycle.activity.deleted'`, `requested_data=json.dumps({'cycle_id': str(pk), 'cycle_name': str(cycle.name), 'issues': [str(issue_id) for issue_id in cycle_issues]})`, `actor_id=str(request.user.id)`, `issue_id=None`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, null, str, null, int)` |
| 2 | `plane/api/views/cycle.py:1050` | `CycleIssueListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:cycle_id>/cycle-issues/ | `type='cycle.activity.created'`, `requested_data=json.dumps({'cycles_list': issues})`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps({'updated_cycle_issues': update_cycle_issue_activity, 'created_cycle_issues': serializers.serialize('json', created_records)})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, null, str, str(JSON), int, bool, str)` |
| 3 | `plane/api/views/cycle.py:1164` | `CycleIssueDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:cycle_id>/cycle-issues/<uuid:issue_id>/ | `type='cycle.activity.deleted'`, `requested_data=json.dumps({'cycle_id': str(self.kwargs.get('cycle_id')), 'issues': [str(issue_id)]})`, `actor_id=str(self.request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, str, str, null, int)` |
| 4 | `plane/api/views/intake.py:218` | `IntakeIssueListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/ | `type='issue.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `intake=str(intake_issue.id)` | `(str, str(JSON,DJE), str, str, str, null, int, str)` |
| 5 | `plane/api/views/intake.py:419` | `IntakeIssueDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/<uuid:issue_id>/ | `type='issue.activity.updated'`, `requested_data=requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=json.dumps(IssueSerializer(current_instance).data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `intake=str(intake_issue.id)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, str)` |
| 6 | `plane/api/views/intake.py:440` | `IntakeIssueDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/<uuid:issue_id>/ | `type='intake.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=False`, `origin=base_host(request=request, is_app=True)`, `intake=str(intake_issue.id)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str, str)` |
| 7 | `plane/api/views/issue.py:579` | `IssueListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<br>POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/ | `type='issue.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(serializer.data.get('id', None))`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 8 | `plane/api/views/issue.py:742` | `IssueDetailAPIEndpoint.put` | PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `type='issue.activity.updated'`, `requested_data=json.dumps(accepted_data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str)` |
| 9 | `plane/api/views/issue.py:789` | `IssueDetailAPIEndpoint.put` | PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `type='issue.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(serializer.data.get('id', None))`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 10 | `plane/api/views/issue.py:877` | `IssueDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `type='issue.activity.updated'`, `requested_data=json.dumps(accepted_data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(pk)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str)` |
| 11 | `plane/api/views/issue.py:936` | `IssueDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `type='issue.activity.deleted'`, `requested_data=json.dumps({'issue_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(pk)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, str, str, str(JSON,DJE), int)` |
| 12 | `plane/api/views/issue.py:1286` | `IssueLinkListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/links/<br>POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/links/ | `type='link.activity.created'`, `requested_data=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `issue_id=str(self.kwargs.get('issue_id'))`, `project_id=str(self.kwargs.get('project_id'))`, `actor_id=str(link.created_by_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, null, int)` |
| 13 | `plane/api/views/issue.py:1408` | `IssueLinkDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/links/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/links/<uuid:pk>/ | `type='link.activity.updated'`, `requested_data=requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int)` |
| 14 | `plane/api/views/issue.py:1441` | `IssueLinkDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/links/<uuid:pk>/<br>DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/links/<uuid:pk>/ | `type='link.activity.deleted'`, `requested_data=json.dumps({'link_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, str, str, str(JSON,DJE), int)` |
| 15 | `plane/api/views/issue.py:1591` | `IssueCommentListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<br>POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/comments/ | `type='comment.activity.created'`, `requested_data=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `actor_id=str(issue_comment.created_by_id)`, `issue_id=str(self.kwargs.get('issue_id'))`, `project_id=str(self.kwargs.get('project_id'))`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, null, int)` |
| 16 | `plane/api/views/issue.py:1753` | `IssueCommentDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/comments/<uuid:pk>/ | `type='comment.activity.updated'`, `requested_data=requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int)` |
| 17 | `plane/api/views/issue.py:1813` | `IssueCommentDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<uuid:pk>/<br>DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/comments/<uuid:pk>/ | `type='comment.activity.deleted'`, `requested_data=json.dumps({'comment_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, str, str, str(JSON,DJE), int)` |
| 18 | `plane/api/views/issue.py:2240` | `IssueAttachmentDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<uuid:pk>/<br>DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/attachments/<uuid:pk>/ | `type='attachment.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, null, int, bool, str)` |
| 19 | `plane/api/views/issue.py:2397` | `IssueAttachmentDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/attachments/<uuid:pk>/ | `type='attachment.activity.created'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, str(JSON,DJE), int, bool, str)` |
| 20 | `plane/api/views/issue.py:2761` | `IssueRelationListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/relations/ | `type='issue_relation.activity.created'`, `requested_data=json.dumps({'relation_type': relation_type, 'issues': issues}, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 21 | `plane/api/views/module.py:563` | `ModuleDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:pk>/ | `type='module.activity.deleted'`, `requested_data=json.dumps({'module_id': str(pk), 'module_name': str(module.name), 'issues': [str(issue_id) for issue_id in module_issues]})`, `actor_id=str(request.user.id)`, `issue_id=None`, `project_id=str(project_id)`, `current_instance=json.dumps({'module_name': str(module.name)})`, `epoch=int(timezone.now().timestamp())`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, null, str, str(JSON), int, str)` |
| 22 | `plane/api/views/module.py:771` | `ModuleIssueListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:module_id>/module-issues/ | `type='module.activity.created'`, `requested_data=json.dumps({'modules_list': str(issues)})`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps({'updated_module_issues': update_module_issue_activity, 'created_module_issues': serializers.serialize('json', record_to_create)})`, `epoch=int(timezone.now().timestamp())`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, null, str, str(JSON), int, str)` |
| 23 | `plane/api/views/module.py:940` | `ModuleIssueDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:module_id>/module-issues/<uuid:issue_id>/ | `type='module.activity.deleted'`, `requested_data=json.dumps({'module_id': str(module_id), 'issues': [str(module_issue.issue_id)]})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=json.dumps({'module_name': module_name})`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, str, str, str(JSON), int)` |
| 24 | `plane/app/views/cycle/base.py:484` | `CycleViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:pk>/ | `type='cycle.activity.deleted'`, `requested_data=json.dumps({'cycle_id': str(pk), 'cycle_name': str(cycle.name), 'issues': [str(issue_id) for issue_id in cycle_issues]})`, `actor_id=str(request.user.id)`, `issue_id=str(pk)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, null, int, bool, str)` |
| 25 | `plane/app/views/cycle/issue.py:301` | `CycleIssueViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:cycle_id>/cycle-issues/ | `type='cycle.activity.created'`, `requested_data=json.dumps({'cycles_list': issues})`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps({'updated_cycle_issues': update_cycle_issue_activity, 'created_cycle_issues': serializers.serialize('json', created_records)})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, null, str, str(JSON), int, bool, str)` |
| 26 | `plane/app/views/cycle/issue.py:327` | `CycleIssueViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:cycle_id>/cycle-issues/<uuid:issue_id>/ | `type='cycle.activity.deleted'`, `requested_data=json.dumps({'cycle_id': str(self.kwargs.get('cycle_id')), 'issues': [str(issue_id)]})`, `actor_id=str(self.request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, null, int, bool, str)` |
| 27 | `plane/app/views/estimate/base.py:210` | `EstimatePointEndpoint.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/estimates/<uuid:estimate_id>/estimate-points/<estimate_point_id>/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'estimate_point': str(new_estimate_id) if new_estimate_id else None})`, `actor_id=str(request.user.id)`, `issue_id=issue.id`, `project_id=str(project_id)`, `current_instance=json.dumps({'estimate_point': str(issue.estimate_point_id) if issue.estimate_point_id else None})`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, UUID, str, str(JSON), int)` |
| 28 | `plane/app/views/estimate/base.py:229` | `EstimatePointEndpoint.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/estimates/<uuid:estimate_id>/estimate-points/<estimate_point_id>/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'estimate_point': None})`, `actor_id=str(request.user.id)`, `issue_id=issue.id`, `project_id=str(project_id)`, `current_instance=json.dumps({'estimate_point': str(issue.estimate_point_id) if issue.estimate_point_id else None})`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, UUID, str, str(JSON), int)` |
| 29 | `plane/app/views/intake/base.py:281` | `IntakeIssueViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/inbox-issues/<br>POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/ | `type='issue.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(serializer.data['id'])`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)`, `intake=str(intake_issue.id)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str, str)` |
| 30 | `plane/app/views/intake/base.py:443` | `IntakeIssueViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/inbox-issues/<uuid:pk>/<br>PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/<uuid:pk>/ | `type='issue.activity.updated'`, `requested_data=issue_requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_id)`, `current_instance=issue_current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)`, `intake=str(intake_issue.id)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str, str)` |
| 31 | `plane/app/views/intake/base.py:465` | `IntakeIssueViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/inbox-issues/<uuid:pk>/<br>PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/<uuid:pk>/ | `type='intake.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(pk)`, `project_id=str(project_id)`, `current_instance=intake_current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=False`, `origin=base_host(request=request, is_app=True)`, `intake=str(intake_issue.id)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str, str)` |
| 32 | `plane/app/views/issue/archive.py:264` | `IssueArchiveViewSet.archive` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/archive/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'archived_at': str(timezone.now().date()), 'automation': False})`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_id)`, `current_instance=json.dumps(IssueSerializer(issue).data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON,DJE), int, bool, str)` |
| 33 | `plane/app/views/issue/archive.py:288` | `IssueArchiveViewSet.unarchive` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/archive/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'archived_at': None})`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_id)`, `current_instance=json.dumps(IssueSerializer(issue).data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON,DJE), int, bool, str)` |
| 34 | `plane/app/views/issue/archive.py:328` | `BulkArchiveIssuesEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/bulk-archive-issues/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'archived_at': str(timezone.now().date()), 'automation': False})`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_id)`, `current_instance=json.dumps(IssueSerializer(issue).data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON,DJE), int, bool, str)` |
| 35 | `plane/app/views/issue/attachment.py:57` | `IssueAttachmentEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<br>POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<uuid:pk>/ | `type='attachment.activity.created'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, str(JSON,DJE), int, bool, str)` |
| 36 | `plane/app/views/issue/attachment.py:83` | `IssueAttachmentEndpoint.delete` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<br>DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<uuid:pk>/ | `type='attachment.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, null, int, bool, str)` |
| 37 | `plane/app/views/issue/attachment.py:189` | `IssueAttachmentV2Endpoint.delete` | DELETE /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/attachments/<br>DELETE /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/attachments/<uuid:pk>/ | `type='attachment.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, null, int, bool, str)` |
| 38 | `plane/app/views/issue/attachment.py:259` | `IssueAttachmentV2Endpoint.patch` | PATCH /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/attachments/<br>PATCH /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/attachments/<uuid:pk>/ | `type='attachment.activity.created'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, str(JSON,DJE), int, bool, str)` |
| 39 | `plane/app/views/issue/base.py:422` | `IssueViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/ | `type='issue.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(serializer.data.get('id', None))`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 40 | `plane/app/views/issue/base.py:689` | `IssueViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/ | `type='issue.activity.updated'`, `requested_data=json.dumps(accepted_data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(pk)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str)` |
| 41 | `plane/app/views/issue/base.py:730` | `IssueViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/ | `type='issue.activity.deleted'`, `requested_data=json.dumps({'issue_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(pk)`, `project_id=str(project_id)`, `current_instance={}`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)`, `subscriber=False` | `(str, str(JSON), str, str, str, dict({}), int, bool, str, bool)` |
| 42 | `plane/app/views/issue/base.py:1157` | `IssueBulkUpdateDateEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issue-dates/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'start_date': update.get('start_date')})`, `current_instance=json.dumps({'start_date': str(issue.start_date)})`, `issue_id=str(issue_id)`, `actor_id=str(request.user.id)`, `project_id=str(project_id)`, `epoch=epoch` | `(str, str(JSON), str(JSON), str, str, str, int)` |
| 43 | `plane/app/views/issue/base.py:1170` | `IssueBulkUpdateDateEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issue-dates/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'target_date': update.get('target_date')})`, `current_instance=json.dumps({'target_date': str(issue.target_date)})`, `issue_id=str(issue_id)`, `actor_id=str(request.user.id)`, `project_id=str(project_id)`, `epoch=epoch` | `(str, str(JSON), str(JSON), str, str, str, int)` |
| 44 | `plane/app/views/issue/comment.py:123` | `IssueCommentViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/ | `type='comment.activity.created'`, `requested_data=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id'))`, `project_id=str(self.kwargs.get('project_id'))`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 45 | `plane/app/views/issue/comment.py:161` | `IssueCommentViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<uuid:pk>/ | `type='comment.activity.updated'`, `requested_data=requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str)` |
| 46 | `plane/app/views/issue/comment.py:192` | `IssueCommentViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<uuid:pk>/ | `type='comment.activity.deleted'`, `requested_data=json.dumps({'comment_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON,DJE), int, bool, str)` |
| 47 | `plane/app/views/issue/comment.py:263` | `CommentReactionViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/comments/<uuid:comment_id>/reactions/ | `type='comment_reaction.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=None`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, null, str, null, int, bool, str)` |
| 48 | `plane/app/views/issue/comment.py:296` | `CommentReactionViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/comments/<uuid:comment_id>/reactions/<str:reaction_code>/ | `type='comment_reaction.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps({'reaction': str(reaction_code), 'identifier': str(comment_reaction.id), 'comment_id': str(comment_id)})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, null, str, str(JSON), int, bool, str)` |
| 49 | `plane/app/views/issue/link.py:62` | `IssueLinkViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-links/ | `type='link.activity.created'`, `requested_data=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id'))`, `project_id=str(self.kwargs.get('project_id'))`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 50 | `plane/app/views/issue/link.py:94` | `IssueLinkViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-links/<uuid:pk>/ | `type='link.activity.updated'`, `requested_data=requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str)` |
| 51 | `plane/app/views/issue/link.py:114` | `IssueLinkViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-links/<uuid:pk>/ | `type='link.activity.deleted'`, `requested_data=json.dumps({'link_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON,DJE), int, bool, str)` |
| 52 | `plane/app/views/issue/reaction.py:63` | `IssueReactionViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/reactions/ | `type='issue_reaction.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 53 | `plane/app/views/issue/reaction.py:86` | `IssueReactionViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/reactions/<str:reaction_code>/ | `type='issue_reaction.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps({'reaction': str(reaction_code), 'identifier': str(issue_reaction.id)})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, str, str, str(JSON), int, bool, str)` |
| 54 | `plane/app/views/issue/relation.py:293` | `IssueRelationViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-relation/ | `type='issue_relation.activity.created'`, `requested_data=json.dumps({'relation_type': relation_type, 'issues': issues}, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 55 | `plane/app/views/issue/relation.py:339` | `IssueRelationViewSet.remove_relation` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/remove-relation/ | `type='issue_relation.activity.deleted'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int, bool, str)` |
| 56 | `plane/app/views/issue/sub_issue.py:252` | `SubIssuesEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/sub-issues/ | `type='issue.activity.updated'`, `requested_data=json.dumps({'parent': str(issue_id)})`, `actor_id=str(request.user.id)`, `issue_id=sub_issue_id`, `project_id=str(project_id)`, `current_instance=json.dumps({'parent': sub_issue_id})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON), int, bool, str)` |
| 57 | `plane/app/views/module/base.py:730` | `ModuleViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:pk>/ | `type='module.activity.deleted'`, `requested_data=json.dumps({'module_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue)`, `project_id=project_id`, `current_instance=json.dumps({'module_name': str(module.name)})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, UUID, str(JSON), int, bool, str)` |
| 58 | `plane/app/views/module/issue.py:244` | `ModuleIssueViewSet.create_module_issues` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:module_id>/issues/ | `type='module.activity.created'`, `requested_data=json.dumps({'module_id': str(module_id)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue)`, `project_id=project_id`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, UUID, null, int, bool, str)` |
| 59 | `plane/app/views/module/issue.py:293` | `ModuleIssueViewSet.create_issue_modules` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/modules/ | `type='module.activity.created'`, `requested_data=json.dumps({'module_id': module})`, `actor_id=str(request.user.id)`, `issue_id=issue_id`, `project_id=project_id`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, UUID, UUID, null, int, bool, str)` |
| 60 | `plane/app/views/module/issue.py:314` | `ModuleIssueViewSet.create_issue_modules` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/modules/ | `type='module.activity.deleted'`, `requested_data=json.dumps({'module_id': str(module_id)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=json.dumps({'module_name': module_issue.first().module.name if module_issue.first() and module_issue.first().module else None})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON), int, bool, str)` |
| 61 | `plane/app/views/module/issue.py:349` | `ModuleIssueViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:module_id>/issues/<uuid:issue_id>/ | `type='module.activity.deleted'`, `requested_data=json.dumps({'module_id': str(module_id)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_id)`, `current_instance=json.dumps({'module_name': existing.module.name})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, str, str(JSON), int, bool, str)` |
| 62 | `plane/app/views/workspace/draft.py:287` | `WorkspaceDraftIssueViewSet.create_draft_to_issue` | POST /api/workspaces/<str:slug>/draft-to-issue/<uuid:draft_id>/ | `type='issue.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(serializer.data.get('id', None))`, `project_id=str(draft_issue.project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON,DJE), str, str, str, null, int, bool, str)` |
| 63 | `plane/app/views/workspace/draft.py:309` | `WorkspaceDraftIssueViewSet.create_draft_to_issue` | POST /api/workspaces/<str:slug>/draft-to-issue/<uuid:draft_id>/ | `type='cycle.activity.created'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(self.kwargs.get('project_id', None))`, `current_instance=json.dumps({'updated_cycle_issues': None, 'created_cycle_issues': serializers.serialize('json', [created_records])})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, null, str, null, str, str(JSON), int, bool, str)` |
| 64 | `plane/app/views/workspace/draft.py:344` | `WorkspaceDraftIssueViewSet.create_draft_to_issue` | POST /api/workspaces/<str:slug>/draft-to-issue/<uuid:draft_id>/ | `type='module.activity.created'`, `requested_data=json.dumps({'module_id': str(module)})`, `actor_id=str(request.user.id)`, `issue_id=serializer.data.get('id', None)`, `project_id=draft_issue.project_id`, `current_instance=None`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, str, UUID, null, int, bool, str)` |
| 65 | `plane/bgtasks/issue_automation_task.py:74` | `archive_old_issues` | — | `type='issue.activity.updated'`, `requested_data=json.dumps({'archived_at': str(archive_at), 'automation': True})`, `actor_id=str(project.created_by_id)`, `issue_id=issue.id`, `project_id=project_id`, `current_instance=json.dumps({'archived_at': None})`, `subscriber=False`, `epoch=int(timezone.now().timestamp())`, `notification=True` | `(str, str(JSON), str, UUID, UUID, str(JSON), bool, int, bool)` |
| 66 | `plane/bgtasks/issue_automation_task.py:137` | `close_old_issues` | — | `type='issue.activity.updated'`, `requested_data=json.dumps({'closed_to': str(issue.state_id)})`, `actor_id=str(project.created_by_id)`, `issue_id=issue.id`, `project_id=project_id`, `current_instance=None`, `subscriber=False`, `epoch=int(timezone.now().timestamp())`, `notification=True` | `(str, str(JSON), str, UUID, UUID, null, bool, int, bool)` |
| 67 | `plane/space/views/intake.py:169` | `IntakeIssuePublicViewSet.create` | POST /api/public/anchor/<str:anchor>/intakes/<uuid:intake_id>/inbox-issues/<br>POST /api/public/anchor/<str:anchor>/intakes/<uuid:intake_id>/intake-issues/ | `type='issue.activity.created'`, `requested_data=json.dumps(request.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_deploy_board.project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, null, int)` |
| 68 | `plane/space/views/intake.py:237` | `IntakeIssuePublicViewSet.partial_update` | PATCH /api/public/anchor/<str:anchor>/intakes/<uuid:intake_id>/intake-issues/<uuid:pk>/ | `type='issue.activity.updated'`, `requested_data=requested_data`, `actor_id=str(request.user.id)`, `issue_id=str(issue.id)`, `project_id=str(project_deploy_board.project_id)`, `current_instance=json.dumps(IssueSerializer(current_instance).data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int)` |
| 69 | `plane/space/views/issue.py:312` | `IssueCommentPublicViewSet.create` | POST /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/comments/ | `type='comment.activity.created'`, `requested_data=json.dumps(serializer.data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_deploy_board.project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, null, int)` |
| 70 | `plane/space/views/issue.py:356` | `IssueCommentPublicViewSet.partial_update` | PATCH /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/comments/<uuid:pk>/ | `type='comment.activity.updated'`, `requested_data=json.dumps(data, cls=DjangoJSONEncoder)`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_deploy_board.project_id)`, `current_instance=current_instance`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, str(JSON,DJE), int)` |
| 71 | `plane/space/views/issue.py:385` | `IssueCommentPublicViewSet.destroy` | DELETE /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/comments/<uuid:pk>/ | `type='comment.activity.deleted'`, `requested_data=json.dumps({'comment_id': str(pk)})`, `actor_id=str(request.user.id)`, `issue_id=str(issue_id)`, `project_id=str(project_deploy_board.project_id)`, `current_instance=json.dumps(IssueCommentSerializer(comment).data, cls=DjangoJSONEncoder)`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON), str, str, str, str(JSON,DJE), int)` |
| 72 | `plane/space/views/issue.py:451` | `IssueReactionPublicViewSet.create` | POST /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/reactions/ | `type='issue_reaction.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(project_deploy_board.project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, null, int)` |
| 73 | `plane/space/views/issue.py:481` | `IssueReactionPublicViewSet.destroy` | DELETE /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/reactions/<str:reaction_code>/ | `type='issue_reaction.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(project_deploy_board.project_id)`, `current_instance=json.dumps({'reaction': str(reaction_code), 'identifier': str(issue_reaction.id)})`, `epoch=int(timezone.now().timestamp())` | `(str, null, str, str, str, str(JSON), int)` |
| 74 | `plane/space/views/issue.py:548` | `CommentReactionPublicViewSet.create` | POST /api/public/anchor/<str:anchor>/comments/<uuid:comment_id>/reactions/ | `type='comment_reaction.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(project_deploy_board.project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, null, str, null, int)` |
| 75 | `plane/space/views/issue.py:584` | `CommentReactionPublicViewSet.destroy` | DELETE /api/public/anchor/<str:anchor>/comments/<uuid:comment_id>/reactions/<str:reaction_code>/ | `type='comment_reaction.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=None`, `project_id=str(project_deploy_board.project_id)`, `current_instance=json.dumps({'reaction': str(reaction_code), 'identifier': str(comment_reaction.id), 'comment_id': str(comment_id)})`, `epoch=int(timezone.now().timestamp())` | `(str, null, str, null, str, str(JSON), int)` |
| 76 | `plane/space/views/issue.py:653` | `IssueVotePublicViewSet.create` | POST /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/votes/ | `type='issue_vote.activity.created'`, `requested_data=json.dumps(self.request.data, cls=DjangoJSONEncoder)`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(project_deploy_board.project_id)`, `current_instance=None`, `epoch=int(timezone.now().timestamp())` | `(str, str(JSON,DJE), str, str, str, null, int)` |
| 77 | `plane/space/views/issue.py:673` | `IssueVotePublicViewSet.destroy` | DELETE /api/public/anchor/<str:anchor>/issues/<uuid:issue_id>/votes/ | `type='issue_vote.activity.deleted'`, `requested_data=None`, `actor_id=str(self.request.user.id)`, `issue_id=str(self.kwargs.get('issue_id', None))`, `project_id=str(project_deploy_board.project_id)`, `current_instance=json.dumps({'vote': str(issue_vote.vote), 'identifier': str(issue_vote.id)})`, `epoch=int(timezone.now().timestamp())` | `(str, null, str, str, str, str(JSON), int)` |
| 78 | `plane/utils/cycle_transfer_issues.py:461` | `transfer_cycle_issues` | — | `type='cycle.activity.created'`, `requested_data=json.dumps({'cycles_list': []})`, `actor_id=str(user_id)`, `issue_id=None`, `project_id=str(project_id)`, `current_instance=json.dumps({'updated_cycle_issues': update_cycle_issue_activity, 'created_cycle_issues': []})`, `epoch=int(timezone.now().timestamp())`, `notification=True`, `origin=base_host(request=request, is_app=True)` | `(str, str(JSON), str, null, str, str(JSON), int, bool, str)` |

#### `plane.bgtasks.issue_description_version_sync.schedule_issue_description_version` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/db/management/commands/sync_issue_description_version.py:21` | `Command.handle` | — | `batch_size=batch_size`, `countdown=int(batch_countdown)` | `(str, int)` |

#### `plane.bgtasks.issue_description_version_sync.sync_issue_description_version` — 2 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/bgtasks/issue_description_version_sync.py:109` | `sync_issue_description_version` | — | `kwargs={'batch_size': batch_size, 'offset': end_offset, 'countdown': countdown}`, `countdown=countdown` | `(dict[str,int], int)` |
| 2 | `plane/bgtasks/issue_description_version_sync.py:125` | `schedule_issue_description_version` | — | `batch_size=int(batch_size)`, `countdown=countdown` | `(int, int)` |

#### `plane.bgtasks.issue_description_version_task.issue_description_version_task` — 4 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/intake/base.py:294` | `IntakeIssueViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/inbox-issues/<br>POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/ | `updated_issue=json.dumps(request.data, cls=DjangoJSONEncoder)`, `issue_id=str(serializer.data['id'])`, `user_id=request.user.id`, `is_creating=True` | `(str(JSON,DJE), str, UUID, bool)` |
| 2 | `plane/app/views/intake/base.py:456` | `IntakeIssueViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/inbox-issues/<uuid:pk>/<br>PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/intake-issues/<uuid:pk>/ | `updated_issue=issue_current_instance`, `issue_id=str(pk)`, `user_id=request.user.id` | `(str(JSON,DJE), str, UUID)` |
| 3 | `plane/app/views/issue/base.py:484` | `IssueViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/ | `updated_issue=json.dumps(request.data, cls=DjangoJSONEncoder)`, `issue_id=str(serializer.data['id'])`, `user_id=request.user.id`, `is_creating=True` | `(str(JSON,DJE), str, UUID, bool)` |
| 4 | `plane/app/views/issue/base.py:710` | `IssueViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/ | `updated_issue=current_instance`, `issue_id=str(serializer.data.get('id', None))`, `user_id=request.user.id` | `(str(JSON,DJE), str, UUID)` |

#### `plane.bgtasks.issue_version_sync.schedule_issue_version` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/db/management/commands/sync_issue_version.py:19` | `Command.handle` | — | `batch_size=batch_size`, `countdown=int(batch_countdown)` | `(str, int)` |

#### `plane.bgtasks.issue_version_sync.sync_issue_version` — 2 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/bgtasks/issue_version_sync.py:218` | `sync_issue_version` | — | `kwargs={'batch_size': batch_size, 'offset': end_offset, 'countdown': countdown}`, `countdown=countdown` | `(dict[str,int], int)` |
| 2 | `plane/bgtasks/issue_version_sync.py:236` | `schedule_issue_version` | — | `batch_size=int(batch_size)`, `countdown=countdown` | `(int, int)` |

#### `plane.bgtasks.logger_task.process_logs` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/middleware/logger.py:158` | `APITokenLogMiddleware.process_request` | — | `log_data=log_data` | `(dict)` |

#### `plane.bgtasks.magic_link_code_task.magic_link` — 2 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/authentication/views/app/magic.py:57` | `MagicGenerateEndpoint.post` | POST /auth/magic-generate/ | `email`, `key`, `token` | `(str, str, str)` |
| 2 | `plane/authentication/views/space/magic.py:56` | `MagicGenerateSpaceEndpoint.post` | POST /auth/spaces/magic-generate/ | `email`, `key`, `token` | `(str, str, str)` |

#### `plane.bgtasks.notification_task.notifications` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/bgtasks/issue_activities_task.py:1633` | `issue_activity` | — | `type=type`, `issue_id=issue_id`, `actor_id=actor_id`, `project_id=project_id`, `subscriber=subscriber`, `issue_activities_created=json.dumps(IssueActivitySerializer(issue_activities_created, many=True).data, cls=DjangoJSONEncoder)`, `requested_data=requested_data`, `current_instance=current_instance` | `(str, str\|UUID\|null, str, str\|UUID, bool, str(JSON,DJE), str(JSON,DJE)\|dict\|null, str(JSON,DJE)\|null)` |

#### `plane.bgtasks.page_transaction_task.page_transaction` — 4 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/page/base.py:160` | `PageViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/ | `new_description_html=request.data.get('description_html', '<p></p>')`, `old_description_html=None`, `page_id=serializer.data['id']` | `(str, null, str)` |
| 2 | `plane/app/views/page/base.py:204` | `PageViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/<uuid:page_id>/ | `new_description_html=request.data.get('description_html', '<p></p>')`, `old_description_html=page_description`, `page_id=page_id` | `(str, str, UUID)` |
| 3 | `plane/app/views/page/base.py:558` | `PagesDescriptionViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/<uuid:page_id>/description/ | `new_description_html=request.data.get('description_html', '<p></p>')`, `old_description_html=old_description_html`, `page_id=page_id` | `(str, str, UUID)` |
| 4 | `plane/app/views/page/base.py:610` | `PageDuplicateEndpoint.post` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/<uuid:page_id>/duplicate/ | `new_description_html=page.description_html`, `old_description_html=None`, `page_id=page.id` | `(str, null, UUID)` |

#### `plane.bgtasks.page_version_task.track_page_version` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/page/base.py:565` | `PagesDescriptionViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/<uuid:page_id>/description/ | `page_id=page_id`, `existing_instance=existing_instance`, `user_id=request.user.id` | `(UUID, str(JSON,DJE), UUID)` |

#### `plane.bgtasks.project_add_user_email_task.project_add_user_email` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/project/member.py:154` | `ProjectMemberViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/members/ | `base_host(request=request, is_app=True)`, `project_member.id`, `request.user.id` | `(str, UUID, UUID)` |

#### `plane.bgtasks.recent_visited_task.recent_visited_task` — 10 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/cycle/base.py:469` | `CycleViewSet.retrieve` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:pk>/ | `slug=slug`, `entity_name='cycle'`, `entity_identifier=pk`, `user_id=request.user.id`, `project_id=project_id` | `(str, str, UUID, UUID, UUID)` |
| 2 | `plane/app/views/issue/base.py:165` | `IssueListEndpoint.get` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/list/ | `slug=slug`, `project_id=project_id`, `entity_name='project'`, `entity_identifier=project_id`, `user_id=request.user.id` | `(str, UUID, str, UUID, UUID)` |
| 3 | `plane/app/views/issue/base.py:304` | `IssueViewSet.list` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/ | `slug=slug`, `project_id=project_id`, `entity_name='project'`, `entity_identifier=project_id`, `user_id=request.user.id` | `(str, UUID, str, UUID, UUID)` |
| 4 | `plane/app/views/issue/base.py:617` | `IssueViewSet.retrieve` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/ | `slug=slug`, `entity_name='issue'`, `entity_identifier=pk`, `user_id=request.user.id`, `project_id=project_id` | `(str, str, UUID, UUID, UUID)` |
| 5 | `plane/app/views/issue/base.py:1359` | `IssueDetailIdentifierEndpoint.get` | GET /api/workspaces/<str:slug>/work-items/<str:project_identifier>-<str:issue_identifier>/ | `slug=slug`, `entity_name='issue'`, `entity_identifier=str(issue.id)`, `user_id=str(request.user.id)`, `project_id=str(project.id)` | `(str, str, str, str, str)` |
| 6 | `plane/app/views/module/base.py:642` | `ModuleViewSet.retrieve` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:pk>/ | `slug=slug`, `entity_name='module'`, `entity_identifier=pk`, `user_id=request.user.id`, `project_id=project_id` | `(str, str, UUID, UUID, UUID)` |
| 7 | `plane/app/views/page/base.py:232` | `PageViewSet.retrieve` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/pages/<uuid:page_id>/ | `slug=slug`, `entity_name='page'`, `entity_identifier=page_id`, `user_id=request.user.id`, `project_id=project_id` | `(str, str, UUID, UUID, UUID)` |
| 8 | `plane/app/views/project/base.py:247` | `ProjectViewSet.retrieve` | GET /api/workspaces/<str:slug>/projects/<uuid:pk>/ | `slug=slug`, `project_id=pk`, `entity_name='project'`, `entity_identifier=pk`, `user_id=request.user.id` | `(str, UUID, str, UUID, UUID)` |
| 9 | `plane/app/views/view/base.py:140` | `WorkspaceViewViewSet.retrieve` | GET /api/workspaces/<str:slug>/views/<uuid:pk>/ | `slug=slug`, `project_id=None`, `entity_name='view'`, `entity_identifier=pk`, `user_id=request.user.id` | `(str, null, str, UUID, UUID)` |
| 10 | `plane/app/views/view/base.py:376` | `IssueViewViewSet.retrieve` | GET /api/workspaces/<str:slug>/projects/<uuid:project_id>/views/<uuid:pk>/ | `slug=slug`, `project_id=project_id`, `entity_name='view'`, `entity_identifier=pk`, `user_id=request.user.id` | `(str, UUID, str, UUID, UUID)` |

#### `plane.bgtasks.storage_metadata_task.get_asset_object_metadata` — 10 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/api/views/asset.py:217` | `UserAssetEndpoint.patch` | PATCH /api/v1/assets/user-assets/<br>PATCH /api/v1/assets/user-assets/<uuid:asset_id>/ | `asset_id=str(asset_id)` | `(str)` |
| 2 | `plane/api/views/asset.py:373` | `UserServerAssetEndpoint.patch` | PATCH /api/v1/assets/user-assets/<uuid:asset_id>/server/<br>PATCH /api/v1/assets/user-assets/server/ | `asset_id=str(asset_id)` | `(str)` |
| 3 | `plane/api/views/asset.py:662` | `GenericAssetEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/assets/<br>PATCH /api/v1/workspaces/<str:slug>/assets/<uuid:asset_id>/ | `asset_id=str(asset_id)` | `(str)` |
| 4 | `plane/api/views/issue.py:2254` | `IssueAttachmentDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<uuid:pk>/<br>DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/attachments/<uuid:pk>/ | `str(issue_attachment.id)` | `(str)` |
| 5 | `plane/api/views/issue.py:2415` | `IssueAttachmentDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-attachments/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/attachments/<uuid:pk>/ | `str(issue_attachment.id)` | `(str)` |
| 6 | `plane/app/views/asset/v2.py:219` | `UserAssetsV2Endpoint.patch` | PATCH /api/assets/v2/user-assets/<br>PATCH /api/assets/v2/user-assets/<uuid:asset_id>/ | `asset_id=str(asset_id)` | `(str)` |
| 7 | `plane/app/views/asset/v2.py:460` | `WorkspaceFileAssetEndpoint.patch` | PATCH /api/assets/v2/workspaces/<str:slug>/<br>PATCH /api/assets/v2/workspaces/<str:slug>/<uuid:asset_id>/ | `asset_id=str(asset_id)` | `(str)` |
| 8 | `plane/app/views/asset/v2.py:696` | `ProjectAssetEndpoint.patch` | PATCH /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/<br>PATCH /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/<uuid:pk>/ | `asset_id=str(pk)` | `(str)` |
| 9 | `plane/app/views/issue/attachment.py:277` | `IssueAttachmentV2Endpoint.patch` | PATCH /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/attachments/<br>PATCH /api/assets/v2/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/attachments/<uuid:pk>/ | `str(issue_attachment.id)` | `(str)` |
| 10 | `plane/space/views/asset.py:197` | `EntityAssetEndpoint.patch` | PATCH /api/public/assets/v2/anchor/<str:anchor>/<br>PATCH /api/public/assets/v2/anchor/<str:anchor>/<uuid:pk>/ | `str(asset.id)` | `(str)` |

#### `plane.bgtasks.user_activation_email_task.user_activation_email` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/authentication/adapter/base.py:248` | `Adapter.save_user_data` | — | `base_host(request=self.request)`, `user.id` | `(str, UUID)` |

#### `plane.bgtasks.user_deactivation_email_task.user_deactivation_email` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/user/base.py:370` | `UserEndpoint.deactivate` | DELETE /api/users/me/ | `base_host(request=request, is_app=True)`, `user.id` | `(str, UUID)` |

#### `plane.bgtasks.user_email_update_task.send_email_update_confirmation` — 2 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/user/base.py:270` | `UserEndpoint.update_email` | PATCH /api/users/me/email/ | `new_email` | `(str)` |
| 2 | `plane/app/views/user/base.py:272` | `UserEndpoint.update_email` | PATCH /api/users/me/email/ | `old_email` | `(str)` |

#### `plane.bgtasks.user_email_update_task.send_email_update_magic_code` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/user/base.py:178` | `UserEndpoint.generate_email_verification_code` | POST /api/users/me/email/generate-code/ | `new_email`, `token` | `(str, str)` |

#### `plane.bgtasks.webhook_task.model_activity` — 22 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/api/views/cycle.py:339` | `CycleListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/ | `model_name='cycle'`, `model_id=str(serializer.instance.id)`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 2 | `plane/api/views/cycle.py:595` | `CycleDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:pk>/ | `model_name='cycle'`, `model_id=str(serializer.instance.id)`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 3 | `plane/api/views/issue.py:592` | `IssueListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<br>POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/ | `model_name='issue'`, `model_id=str(serializer.data['id'])`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 4 | `plane/api/views/issue.py:754` | `IssueDetailAPIEndpoint.put` | PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `model_name='issue'`, `model_id=str(issue.id)`, `requested_data=accepted_data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 5 | `plane/api/views/issue.py:801` | `IssueDetailAPIEndpoint.put` | PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>PUT /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `model_name='issue'`, `model_id=str(serializer.data['id'])`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 6 | `plane/api/views/issue.py:889` | `IssueDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:pk>/ | `model_name='issue'`, `model_id=str(pk)`, `requested_data=accepted_data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 7 | `plane/api/views/issue.py:1602` | `IssueCommentListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<br>POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/comments/ | `model_name='issue_comment'`, `model_id=str(serializer.instance.id)`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 8 | `plane/api/views/issue.py:1763` | `IssueCommentDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/comments/<uuid:pk>/ | `model_name='issue_comment'`, `model_id=str(pk)`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 9 | `plane/api/views/module.py:232` | `ModuleListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/modules/ | `model_name='module'`, `model_id=str(serializer.instance.id)`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 10 | `plane/api/views/module.py:490` | `ModuleDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:pk>/ | `model_name='module'`, `model_id=str(serializer.instance.id)`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 11 | `plane/api/views/project.py:295` | `ProjectListCreateAPIEndpoint._dispatch_model_activity` | — | `model_name='project'`, `model_id=str(project.id)`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 12 | `plane/api/views/project.py:589` | `ProjectDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:pk>/ | `model_name='project'`, `model_id=str(project.id)`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 13 | `plane/app/views/cycle/base.py:319` | `CycleViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/ | `model_name='cycle'`, `model_id=str(cycle['id'])`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 14 | `plane/app/views/cycle/base.py:398` | `CycleViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/cycles/<uuid:pk>/ | `model_name='cycle'`, `model_id=str(cycle['id'])`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 15 | `plane/app/views/issue/base.py:474` | `IssueViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/ | `model_name='issue'`, `model_id=str(serializer.data['id'])`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 16 | `plane/app/views/issue/base.py:700` | `IssueViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:pk>/ | `model_name='issue'`, `model_id=str(serializer.data.get('id', None))`, `requested_data=accepted_data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 17 | `plane/app/views/issue/comment.py:135` | `IssueCommentViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/ | `model_name='issue_comment'`, `model_id=str(serializer.data['id'])`, `requested_data=data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 18 | `plane/app/views/issue/comment.py:173` | `IssueCommentViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/comments/<uuid:pk>/ | `model_name='issue_comment'`, `model_id=str(pk)`, `requested_data=data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 19 | `plane/app/views/module/base.py:340` | `ModuleViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/modules/ | `model_name='module'`, `model_id=str(module['id'])`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 20 | `plane/app/views/module/base.py:709` | `ModuleViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/modules/<uuid:pk>/ | `model_name='module'`, `model_id=str(module['id'])`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |
| 21 | `plane/app/views/project/base.py:301` | `ProjectViewSet.create` | POST /api/workspaces/<str:slug>/projects/ | `model_name='project'`, `model_id=str(project.id)`, `requested_data=request.data`, `current_instance=None`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, null, UUID, str, str)` |
| 22 | `plane/app/views/project/base.py:370` | `ProjectViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:pk>/ | `model_name='project'`, `model_id=str(project.id)`, `requested_data=request.data`, `current_instance=current_instance`, `actor_id=request.user.id`, `slug=slug`, `origin=base_host(request=request, is_app=True)` | `(str, str, dict, str(JSON,DJE), UUID, str, str)` |

#### `plane.bgtasks.webhook_task.send_webhook_deactivation_email` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/bgtasks/webhook_task.py:359` | `webhook_send_task` | — | `webhook_id=webhook.id`, `receiver_id=webhook.created_by_id`, `reason=str(e)`, `current_site=current_site` | `(UUID, UUID\|null, str, str)` |

#### `plane.bgtasks.webhook_task.webhook_activity` — 4 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/api/views/project.py:637` | `ProjectDetailAPIEndpoint.delete` | DELETE /api/v1/workspaces/<str:slug>/projects/<uuid:pk>/ | `event='project'`, `verb='deleted'`, `field=None`, `old_value=None`, `new_value=None`, `actor_id=request.user.id`, `slug=slug`, `current_site=base_host(request=request, is_app=True)`, `event_id=project.id`, `old_identifier=None`, `new_identifier=None` | `(str, str, null, null, null, UUID, str, str, UUID, null, null)` |
| 2 | `plane/app/views/project/base.py:401` | `ProjectViewSet.destroy` | DELETE /api/workspaces/<str:slug>/projects/<uuid:pk>/ | `event='project'`, `verb='deleted'`, `field=None`, `old_value=None`, `new_value=None`, `actor_id=request.user.id`, `slug=slug`, `current_site=base_host(request=request, is_app=True)`, `event_id=project.id`, `old_identifier=None`, `new_identifier=None` | `(str, str, null, null, null, UUID, str, str, UUID, null, null)` |
| 3 | `plane/bgtasks/webhook_task.py:482` | `model_activity` | — | `event=model_name`, `verb='created'`, `field=None`, `old_value=None`, `new_value=None`, `actor_id=actor_id`, `slug=slug`, `current_site=origin`, `event_id=model_id`, `old_identifier=None`, `new_identifier=None` | `(str, str, null, null, null, UUID, str, str, UUID\|str, null, null)` |
| 4 | `plane/bgtasks/webhook_task.py:507` | `model_activity` | — | `event=model_name`, `verb='updated'`, `field=key`, `old_value=current_value`, `new_value=requested_value`, `actor_id=actor_id`, `slug=slug`, `current_site=origin`, `event_id=model_id`, `old_identifier=None`, `new_identifier=None` | `(str, str, str, any, any, UUID, str, str, UUID\|str, null, null)` |

#### `plane.bgtasks.webhook_task.webhook_send_task` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/bgtasks/webhook_task.py:451` | `webhook_activity` | — | `webhook_id=webhook.id`, `slug=slug`, `event=event`, `event_data={'id': event_id} if verb == 'deleted' else get_model_data(event=event, event_id=event_id)`, `action=verb`, `current_site=current_site`, `activity={'field': field, 'new_value': new_value, 'old_value': old_value, 'actor': get_model_data(event='user', event_id=actor_id), 'old_identifier': old_identifier, 'new_identifier': new_identifier}` | `(UUID, str, str, dict, str, str, dict)` |

#### `plane.bgtasks.work_item_link_task.crawl_work_item_link_title` — 4 call sites

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/api/views/issue.py:1284` | `IssueLinkListCreateAPIEndpoint.post` | POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/links/<br>POST /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/links/ | `serializer.instance.id`, `serializer.instance.url` | `(UUID, str)` |
| 2 | `plane/api/views/issue.py:1407` | `IssueLinkDetailAPIEndpoint.patch` | PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/links/<uuid:pk>/<br>PATCH /api/v1/workspaces/<str:slug>/projects/<uuid:project_id>/work-items/<uuid:issue_id>/links/<uuid:pk>/ | `serializer.data.get('id')`, `updated_url` | `(UUID, str)` |
| 3 | `plane/app/views/issue/link.py:61` | `IssueLinkViewSet.create` | POST /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-links/ | `serializer.data.get('id')`, `serializer.data.get('url')` | `(UUID, str)` |
| 4 | `plane/app/views/issue/link.py:92` | `IssueLinkViewSet.partial_update` | PATCH /api/workspaces/<str:slug>/projects/<uuid:project_id>/issues/<uuid:issue_id>/issue-links/<uuid:pk>/ | `serializer.data.get('id')`, `updated_url` | `(UUID, str)` |

`serializer.data.get('id')` is a `uuid.UUID` at sites 2–4 as well: both
`IssueLinkSerializer`s are `fields = "__all__"` with an `editable=False` pk, so
DRF builds `id` as a read-only `PrimaryKeyRelatedField` and
`to_representation` returns the pk object (only the JSON renderer stringifies
it). The task's `id` argument is always kombu's `uuid` envelope.

#### `plane.bgtasks.workspace_invitation_task.workspace_invitation` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/workspace/invite.py:120` | `WorkspaceInvitationsViewset.create` | POST /api/workspaces/<str:slug>/invitations/ | `invitation.email`, `workspace.id`, `invitation.token`, `current_site`, `request.user.email` | `(str, UUID, str, str, str)` |

#### `plane.bgtasks.workspace_seed_task.workspace_seed` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/app/views/workspace/base.py:138` | `WorkSpaceViewSet.create` | POST /api/workspaces/ | `serializer.data['id']` | `(str)` |

#### `plane.license.bgtasks.telemetry_metrics.push_instance_metrics` — 1 call site

| # | site | enqueued from | route / entry point | arguments (exact) | runtime types |
| - | --- | --- | --- | --- | --- |
| 1 | `plane/license/management/commands/register_instance.py:90` | `Command.handle` | — | *(no arguments)* | `()` |
<!-- /callers -->

### 5.1 Notes that apply to many call sites

* **`request.data` is not a dict.** It is a DRF `Request.data`, which yields a `dict` for a
  JSON body and a `QueryDict` for a form body. Serialised by kombu it becomes a JSON object
  either way (a `QueryDict` becomes an object whose values are lists). `model_activity` and
  `analytic_export_task` take it raw; everything else `json.dumps`es it first.
* **`serializer.data[...]` is always a string.** DRF renders `UUIDField` values as
  `str(uuid)`, so `serializer.data["id"]` is a `str`, while `serializer.instance.id` is a
  `uuid.UUID`. Both spellings appear for the same value in different call sites.
* **`self.kwargs.get("project_id")` is a `uuid.UUID`** because the URL pattern uses the
  `<uuid:project_id>` converter, not `<str:...>`.
* **`request.user.id` is a `uuid.UUID`** (`plane/db/models/user.py:57`).

---

## 6. Task → route matrix

Which route groups enqueue which task. A task can only move to Rust once **every** group in
its row has moved (or the call site has been deleted), because the Rust publisher has to
replace each enqueue with a Celery publish until then. `—` means no call site in that group.

| task | /api/ (web app) | /api/v1/ (public API) | /api/public/ (space) | /auth/ | internal (tasks, commands, model hooks) |
| --- | --- | --- | --- | --- | --- |
| `plane.bgtasks.analytic_plot_export.analytic_export_task` | ExportAnalyticsEndpoint | — | — | — | — |
| `plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets` | PageDuplicateEndpoint | — | — | — | — |
| `plane.bgtasks.deletion_task.soft_delete_related_objects` | — | — | — | — | SoftDeleteModel.delete() — `plane/db/mixins.py:78` |
| `plane.bgtasks.email_notification_task.send_email_notification` | — | — | — | — | stack_email_notification() — `plane/bgtasks/email_notification_task.py:76` |
| `plane.bgtasks.export_task.issue_export_task` | ExportIssuesEndpoint | — | — | — | — |
| `plane.bgtasks.forgot_password_task.forgot_password` | — | — | — | ForgotPasswordEndpoint<br>ForgotPasswordSpaceEndpoint | — |
| `plane.bgtasks.issue_activities_task.issue_activity` | BulkArchiveIssuesEndpoint<br>CommentReactionViewSet<br>CycleIssueViewSet<br>CycleViewSet<br>EstimatePointEndpoint<br>IntakeIssueViewSet<br>IssueArchiveViewSet<br>IssueAttachmentEndpoint<br>IssueAttachmentV2Endpoint<br>IssueBulkUpdateDateEndpoint<br>IssueCommentViewSet<br>IssueLinkViewSet<br>IssueReactionViewSet<br>IssueRelationViewSet<br>IssueViewSet<br>ModuleIssueViewSet<br>ModuleViewSet<br>SubIssuesEndpoint<br>WorkspaceDraftIssueViewSet | CycleDetailAPIEndpoint<br>CycleIssueDetailAPIEndpoint<br>CycleIssueListCreateAPIEndpoint<br>IntakeIssueDetailAPIEndpoint<br>IntakeIssueListCreateAPIEndpoint<br>IssueAttachmentDetailAPIEndpoint<br>IssueCommentDetailAPIEndpoint<br>IssueCommentListCreateAPIEndpoint<br>IssueDetailAPIEndpoint<br>IssueLinkDetailAPIEndpoint<br>IssueLinkListCreateAPIEndpoint<br>IssueListCreateAPIEndpoint<br>IssueRelationListCreateAPIEndpoint<br>ModuleDetailAPIEndpoint<br>ModuleIssueDetailAPIEndpoint<br>ModuleIssueListCreateAPIEndpoint | CommentReactionPublicViewSet<br>IntakeIssuePublicViewSet<br>IssueCommentPublicViewSet<br>IssueReactionPublicViewSet<br>IssueVotePublicViewSet | — | archive_old_issues() — `plane/bgtasks/issue_automation_task.py:74`<br>close_old_issues() — `plane/bgtasks/issue_automation_task.py:137`<br>transfer_cycle_issues() — `plane/utils/cycle_transfer_issues.py:461` |
| `plane.bgtasks.issue_description_version_sync.schedule_issue_description_version` | — | — | — | — | Command.handle() — `plane/db/management/commands/sync_issue_description_version.py:21` |
| `plane.bgtasks.issue_description_version_sync.sync_issue_description_version` | — | — | — | — | schedule_issue_description_version() — `plane/bgtasks/issue_description_version_sync.py:125`<br>sync_issue_description_version() — `plane/bgtasks/issue_description_version_sync.py:109` |
| `plane.bgtasks.issue_description_version_task.issue_description_version_task` | IntakeIssueViewSet<br>IssueViewSet | — | — | — | — |
| `plane.bgtasks.issue_version_sync.schedule_issue_version` | — | — | — | — | Command.handle() — `plane/db/management/commands/sync_issue_version.py:19` |
| `plane.bgtasks.issue_version_sync.sync_issue_version` | — | — | — | — | schedule_issue_version() — `plane/bgtasks/issue_version_sync.py:236`<br>sync_issue_version() — `plane/bgtasks/issue_version_sync.py:218` |
| `plane.bgtasks.logger_task.process_logs` | — | — | — | — | APITokenLogMiddleware.process_request() — `plane/middleware/logger.py:158` |
| `plane.bgtasks.magic_link_code_task.magic_link` | — | — | — | MagicGenerateEndpoint<br>MagicGenerateSpaceEndpoint | — |
| `plane.bgtasks.notification_task.notifications` | — | — | — | — | issue_activity() — `plane/bgtasks/issue_activities_task.py:1633` |
| `plane.bgtasks.page_transaction_task.page_transaction` | PageDuplicateEndpoint<br>PageViewSet<br>PagesDescriptionViewSet | — | — | — | — |
| `plane.bgtasks.page_version_task.track_page_version` | PagesDescriptionViewSet | — | — | — | — |
| `plane.bgtasks.project_add_user_email_task.project_add_user_email` | ProjectMemberViewSet | — | — | — | — |
| `plane.bgtasks.recent_visited_task.recent_visited_task` | CycleViewSet<br>IssueDetailIdentifierEndpoint<br>IssueListEndpoint<br>IssueViewSet<br>IssueViewViewSet<br>ModuleViewSet<br>PageViewSet<br>ProjectViewSet<br>WorkspaceViewViewSet | — | — | — | — |
| `plane.bgtasks.storage_metadata_task.get_asset_object_metadata` | IssueAttachmentV2Endpoint<br>ProjectAssetEndpoint<br>UserAssetsV2Endpoint<br>WorkspaceFileAssetEndpoint | GenericAssetEndpoint<br>IssueAttachmentDetailAPIEndpoint<br>UserAssetEndpoint<br>UserServerAssetEndpoint | EntityAssetEndpoint | — | — |
| `plane.bgtasks.user_activation_email_task.user_activation_email` | — | — | — | — | Adapter.save_user_data() — `plane/authentication/adapter/base.py:248` |
| `plane.bgtasks.user_deactivation_email_task.user_deactivation_email` | UserEndpoint | — | — | — | — |
| `plane.bgtasks.user_email_update_task.send_email_update_confirmation` | UserEndpoint | — | — | — | — |
| `plane.bgtasks.user_email_update_task.send_email_update_magic_code` | UserEndpoint | — | — | — | — |
| `plane.bgtasks.webhook_task.model_activity` | CycleViewSet<br>IssueCommentViewSet<br>IssueViewSet<br>ModuleViewSet<br>ProjectViewSet | CycleDetailAPIEndpoint<br>CycleListCreateAPIEndpoint<br>IssueCommentDetailAPIEndpoint<br>IssueCommentListCreateAPIEndpoint<br>IssueDetailAPIEndpoint<br>IssueListCreateAPIEndpoint<br>ModuleDetailAPIEndpoint<br>ModuleListCreateAPIEndpoint<br>ProjectDetailAPIEndpoint | — | — | ProjectListCreateAPIEndpoint._dispatch_model_activity() — `plane/api/views/project.py:295` |
| `plane.bgtasks.webhook_task.send_webhook_deactivation_email` | — | — | — | — | webhook_send_task() — `plane/bgtasks/webhook_task.py:359` |
| `plane.bgtasks.webhook_task.webhook_activity` | ProjectViewSet | ProjectDetailAPIEndpoint | — | — | model_activity() — `plane/bgtasks/webhook_task.py:482`<br>model_activity() — `plane/bgtasks/webhook_task.py:507` |
| `plane.bgtasks.webhook_task.webhook_send_task` | — | — | — | — | webhook_activity() — `plane/bgtasks/webhook_task.py:451` |
| `plane.bgtasks.work_item_link_task.crawl_work_item_link_title` | IssueLinkViewSet | IssueLinkDetailAPIEndpoint<br>IssueLinkListCreateAPIEndpoint | — | — | — |
| `plane.bgtasks.workspace_invitation_task.workspace_invitation` | WorkspaceInvitationsViewset | — | — | — | — |
| `plane.bgtasks.workspace_seed_task.workspace_seed` | WorkSpaceViewSet | — | — | — | — |
| `plane.license.bgtasks.telemetry_metrics.push_instance_metrics` | — | — | — | — | Command.handle() — `plane/license/management/commands/register_instance.py:90` |

Reading the matrix for the phase plan:

* The **web app** surface (`/api/`) enqueues 18 of the 32 tasks; the public API
  (`/api/v1/`) 5; the space API (`/api/public/`) 2; `/auth/` 2 (some tasks appear in more
  than one column).
* `issue_activity` is enqueued by all three HTTP surfaces plus two background tasks and one
  helper (`plane/utils/cycle_transfer_issues.py`, used by cycle transfer in the web app).
  It cannot move until **all** of them are ported — it is the last task to leave Celery.
* `webhook_task.model_activity` and `issue_activity` are the two "hub" tasks; porting them
  drags in the whole activity/notification chain (`notifications` →
  `stack_email_notification` → `send_email_notification`).
* Thirteen tasks are enqueued **only** from the web app (`/api/`), so they move with Phase 5:
  `analytic_export_task`, `copy_s3_objects_of_description_and_assets`, `issue_export_task`,
  `issue_description_version_task`, `page_transaction`, `track_page_version`,
  `project_add_user_email`, `recent_visited_task`, `user_deactivation_email`,
  `send_email_update_confirmation`, `send_email_update_magic_code`, `workspace_invitation`,
  `workspace_seed`.
* Twelve tasks have no HTTP call site at all. Eleven are plumbing enqueued from another
  task, the middleware, a model hook or a management command:
  `soft_delete_related_objects`, `process_logs`, `send_email_notification`, `notifications`,
  `webhook_send_task`, `send_webhook_deactivation_email`, `schedule_issue_version`,
  `sync_issue_version`, `schedule_issue_description_version`,
  `sync_issue_description_version`, `push_instance_metrics`. The twelfth,
  `user_activation_email`, is enqueued from the OAuth adapter
  (`plane/authentication/adapter/base.py:248`) rather than from a view. They move when their
  *producer* moves.

---

## 7. Beat schedule

### 7.1 The 13 entries in `plane/celery.py:46-101`

All times UTC (`CELERY_TIMEZONE = "UTC"`, `enable_utc = True`). No entry has `args`,
`kwargs` or `options`. No entry has a `queue` override.

| # | name | task | schedule | UTC time | source line |
| - | --- | --- | --- | --- | --- |
| 1 | `check-every-five-minutes-to-send-email-notifications` | `plane.bgtasks.email_notification_task.stack_email_notification` | `crontab(minute="*/5")` | every 5 min | 48-51 |
| 2 | `check-every-five-minutes-to-unsnooze-intake-issues` | `plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues` | `crontab(minute="*/5")` | every 5 min | 52-55 |
| 3 | `push-instance-metrics` | `plane.license.bgtasks.telemetry_metrics.push_instance_metrics` | `schedule(run_every=timedelta(minutes=METRICS_PUSH_INTERVAL_MINUTES))` | every 6 h **relative to beat start** (interval, not crontab) | 56-59 |
| 4 | `check-every-day-to-delete-hard-delete` | `plane.bgtasks.deletion_task.hard_delete` | `crontab(hour=0, minute=0)` | 00:00 | 61-64 |
| 5 | `check-every-day-to-archive-and-close` | `plane.bgtasks.issue_automation_task.archive_and_close_old_issues` | `crontab(hour=1, minute=0)` | 01:00 | 65-68 |
| 6 | `check-every-day-to-delete_exporter_history` | `plane.bgtasks.exporter_expired_task.delete_old_s3_link` | `crontab(hour=1, minute=30)` | 01:30 | 69-72 |
| 7 | `check-every-day-to-delete-file-asset` | `plane.bgtasks.file_asset_task.delete_unuploaded_file_asset` | `crontab(hour=2, minute=0)` | 02:00 | 73-76 |
| 8 | `check-every-day-to-delete-api-logs` | `plane.bgtasks.cleanup_task.delete_api_logs` | `crontab(hour=2, minute=30)` | 02:30 | 77-80 |
| 9 | `check-every-day-to-delete-email-notification-logs` | `plane.bgtasks.cleanup_task.delete_email_notification_logs` | `crontab(hour=2, minute=45)` | 02:45 | 81-84 |
| 10 | `check-every-day-to-delete-page-versions` | `plane.bgtasks.cleanup_task.delete_page_versions` | `crontab(hour=3, minute=0)` | 03:00 | 85-88 |
| 11 | `check-every-day-to-delete-issue-description-versions` | `plane.bgtasks.cleanup_task.delete_issue_description_versions` | `crontab(hour=3, minute=15)` | 03:15 | 89-92 |
| 12 | `check-every-day-to-delete-webhook-logs` | `plane.bgtasks.cleanup_task.delete_webhook_logs` | `crontab(hour=3, minute=30)` | 03:30 | 93-96 |
| 13 | `check-every-day-to-delete-exporter-history` | `plane.bgtasks.exporter_expired_task.delete_old_s3_link` | `crontab(hour=3, minute=45)` | 03:45 | 97-100 |

Notes:

* Entries 6 and 13 run the **same task** 2h15m apart. Entry 13 was presumably meant to be a
  different cleanup job; as it stands `delete_old_s3_link` runs twice a night and the second
  run is a no-op at the database level (the first run already set `url = NULL`). Keep both
  when porting so the schedule matches, then raise it as an upstream bug.
* `METRICS_PUSH_INTERVAL_MINUTES` (`celery.py:27-38`) is read once at import from the
  environment, defaults to 360, and is clamped to `(0, 10_000_000]`; anything else falls
  back to 360. `push_instance_metrics` is assumed not to be ported (owner decision 5), so
  `plane-beat` should simply not run entry 3, and the cutover must delete the row.
* Because entry 3 is an `IntervalSchedule`, `plane-beat` cannot reproduce it with a cron
  expression; it needs a "last run + interval" state, which is what the
  `django_celery_beat_periodictask.last_run_at` column holds today.

### 7.2 Rows the scheduler adds by itself

`DatabaseScheduler.setup_schedule()` calls `install_default_entries()`
(`django_celery_beat/schedulers.py:486-496`), which adds

```
name  = "celery.backend_cleanup"
task  = "celery.backend_cleanup"
crontab("0", "4", "*")          # 04:00 UTC
options = {"expire_seconds": 12 * 3600}
```

whenever `result_expires` is truthy — and it is (`timedelta(days=1)`), even though
`result_backend` is `None`. So `django_celery_beat_periodictask` ends up with **14** rows and
beat fires `celery.backend_cleanup` at 04:00 UTC every day against a `DisabledBackend`. It
fails harmlessly. Do not port it; do not be surprised by it in the table.

### 7.3 Periodic tasks created by migrations or commands

None. `django_celery_beat` is in `INSTALLED_APPS` (so its six tables exist) but no migration
and no management command inserts a `PeriodicTask`; `grep -rn "PeriodicTask" apps/api/plane`
returns nothing. The only source of rows is the sync of the static `beat_schedule` described
above — which means an operator's edits in the admin survive redeploys, and a `plane-beat`
that ignores the table would silently re-enable jobs an operator disabled.

`apps/api/plane/db/management/commands/sync_issue_version.py` and
`sync_issue_description_version.py` are one-shot enqueuers for the backfill tasks, not
scheduled jobs.

---

## 8. Findings

### 8.1 A task that can never run, and a caller that crashes

`plane.bgtasks.project_invitation_task.project_invitation` is decorated `@shared_task` but

1. **no module imports `plane.bgtasks.project_invitation_task`**, so the worker never
   registers the name. A message with that name is rejected with "Received unregistered task
   of type …" and goes to the `celery` queue's default behaviour (dropped).
2. Its only call site is broken:

   ```python
   # plane/app/views/project/invite.py:114-127
   project_invitations = ProjectMemberInvite.objects.bulk_create(...)   # a list
   for invitation in project_invitations:
       project_invitations.delay(                                       # AttributeError
           invitation.email, project_id, invitation.token, current_site, request.user.email,
       )
   ```

   `project_invitations` is a `list`; `list.delay` raises `AttributeError`. So
   `POST /api/workspaces/<slug>/projects/<project_id>/invitations/`
   (`ProjectInvitationsViewset.create`) creates the `ProjectMemberInvite` rows (the
   `bulk_create` already committed) and then **returns 500 without sending any email**.

Port the route as it behaves (500 after creating the rows) or fix it — but decide
deliberately, and drop the task from the Celery inventory either way.

No other enqueue in the backend misses: every other `.delay()`/`.apply_async()` call targets a
task that exists and is registered, and no registry name lacks a definition. The list
`.delay()` above is the only place where an enqueue-looking call never becomes a message.

### 8.2 Tasks defined but never enqueued and not scheduled (dead)

| task | why |
| --- | --- |
| `plane.bgtasks.analytic_plot_export.export_analytics_to_csv_email` | No caller anywhere. Superseded by `analytic_export_task`, which builds and sends the CSV itself. |
| `plane.bgtasks.dummy_data_task.create_dummy_data` | Called **directly** (not via Celery) from `manage.py create_dummy_data`. Registered, so it can be triggered manually with `.delay()`; nothing does. |
| `plane.bgtasks.issue_version_sync.issue_task` | Replaced by the batched `sync_issue_version`; no caller. |
| `plane.bgtasks.project_invitation_task.project_invitation` | See §8.1 — unregistered *and* uncallable. |

The first three cost nothing to leave in place; they must not be ported as queue consumers.
`create_dummy_data` should be ported as a management command if at all.

### 8.3 The same task is enqueued with inconsistent argument types

This is the list the Rust publisher must get right per call site, not per task. Where it
says "UUID" the body carries `{"__type__": "uuid", "__value__": {"hex": "…"}}`; where it says
"str" it carries a plain string. The Python task works with both (Django accepts a string for
`pk=`, and `json.dumps` receives a string either way), so nothing fails today — but the bytes
differ, and a differential harness comparing drained messages will see the difference.

| task | argument | string form | UUID form |
| --- | --- | --- | --- |
| `issue_activity` | `issue_id` | almost everywhere (`str(issue.id)`, `str(pk)`, `serializer.data["id"]`, and the loop variable at `plane/app/views/issue/sub_issue.py:252` which is already a string) | `plane/app/views/estimate/base.py:210` and `:229` (`issue_id=issue.id`), `plane/app/views/module/issue.py:293` (`issue_id=issue_id` from the URL), `plane/bgtasks/issue_automation_task.py:74` and `:137` (`issue_id=issue.id`) |
| `issue_activity` | `project_id` | `str(project_id)` at 72 of the 78 sites | `plane/app/views/module/base.py:730`, `plane/app/views/module/issue.py:244`, `plane/app/views/module/issue.py:293`, `plane/app/views/workspace/draft.py:344` (`draft_issue.project_id`), `plane/bgtasks/issue_automation_task.py:74` and `:137` (`project.id`) |
| `issue_activity` | `actor_id` | `str(request.user.id)` almost everywhere | — (`actor_id` is always stringified here) |
| `model_activity` | `actor_id` | — | **every** site passes `request.user.id`, a `uuid.UUID` |
| `model_activity` | `model_id` | every site uses `str(...)` | — |
| `webhook_activity` | `actor_id`, `event_id` | — | `request.user.id`, `project.id` — both UUID |
| `workspace_invitation` | `workspace_id` | — | `workspace.id` (`plane/app/views/workspace/invite.py:120`) |
| `copy_s3_objects_of_description_and_assets` | `entity_identifier`, `project_id`, `user_id` | `slug` only | `page.id`, `project_id` (URL), `request.user.id` |
| `page_transaction` | `page_id` | `serializer.data["id"]` at `plane/app/views/page/base.py:160` | `page_id` (URL) at `:204` and `:558`; `page.id` at `:610` |
| `page_version_task` | `page_id`, `user_id` | — | both UUID (URL kwarg and `request.user.id`) |
| `crawl_work_item_link_title` | `id` | — | **every** site: `serializer.data.get("id")` in `plane/app/views/issue/link.py:61,92` and `plane/api/views/issue.py:1407` is the pk object of a read-only `PrimaryKeyRelatedField`, and `serializer.instance.id` in `plane/api/views/issue.py:1284` |
| `recent_visited_task` | `entity_identifier` | `str(issue.id)` at `plane/app/views/issue/base.py:1359` | URL `pk` at the nine other sites |
| `recent_visited_task` | `project_id` | — | UUID, or `None` at `plane/app/views/view/base.py:140` |
| `schedule_issue_version` / `schedule_issue_description_version` | `batch_size` | **`str`** — the only caller is the management command, whose `input()` returns a string and whose `.delay()` does not cast it | — |
| `issue_description_version_task` | `user_id` | — | `request.user.id`, a UUID |

The `batch_size` case behaves correctly but looks wrong: `manage.py sync_issue_version` and
`manage.py sync_issue_description_version` pass `input()`'s result straight into `.delay()`, so
the message body carries `"batch_size": "5000"` (a string). The task then does
`sync_issue_version.delay(batch_size=int(batch_size), ...)`, so the worker hop always gets an
int. Nothing breaks. A Rust replacement for those management commands should publish the same
string to stay byte-identical; publishing a number would still work, because the task only
ever calls `int()` on it.

### 8.4 Other surprises

* **`send_email_notification` depends on a Redis key written by `issue_activity`.** The base
  API URL for every link in the email comes from `GET <issue_id>` in Redis, set with a 600 s
  expiry by `issue_activity` (`issue_activities_task.py:1574-1577`). If the email task runs
  more than 10 minutes after the activity task, it silently returns and the notification
  email is never sent, yet `EmailNotificationLog.sent_at` stays null. A Rust port must keep
  writing that key with the same name and TTL, or this task starts dropping emails.
* **The email lock key embeds every notification id** (`send_email_notif_<issue>_<receiver>_<ids
  sorted and joined by "_">`, `email_notification_task.py:154-157`), and the lock is released
  after success, so it only deduplicates *concurrent* duplicates, not retries.
* **`recent_visited_task`'s 20-row cap uses `== 20`**, so once a user has 21 rows the cap
  never trims again (upstream bug, `recent_visited_task.py:37-44`).
* **Most tasks swallow every exception**, including `DatabaseError`, so Celery reports success
  and nothing is retried. The Rust port needs an explicit retry/dead-letter policy per task
  rather than copying this.
* **`webhook_send_task`'s "backoff" is uniform 0–600 s** (§4.2).
* **`save_webhook_log(request_body=payload)` passes a dict** into a `TextField`
  (`webhook_task.py:328-338`): the stored value is Python's `repr` of the payload
  (`{'event': 'issue', ...}` with single quotes), not JSON. Rust must reproduce that string,
  not `serde_json`.
* **One call site publishes `project_id="None"`.** `WorkspaceDraftIssueViewSet.create_draft_to_issue`
  (`plane/app/views/workspace/draft.py:309`) reads `self.kwargs.get("project_id")`, but its route
  is `POST /api/workspaces/<slug>/draft-to-issue/<uuid:draft_id>/` and has no `project_id` kwarg,
  so `str(None)` is published. `issue_activity` starts with
  `if not is_valid_uuid(str(project_id)): return` (`issue_activities_task.py:1567-1568`), so the
  `cycle.activity.created` record for a draft → issue conversion is silently dropped. The other
  two publishes in the same method use `draft_issue.project_id` and work. A Rust port must
  decide whether to keep publishing the junk value (harmless, one wasted message) or fix it.
* **`model_activity` compares a dict against a JSON string**: it `json.loads`es
  `current_instance` (a string) but compares values with `!=` against `requested_data`
  values that came from `request.data` and have *not* been through DRF's field coercion, so
  e.g. `"3"` vs `3` shows up as a change and fires a webhook. Ported as-is.
* **Two tasks take the raw DRF body object**: `analytic_export_task(data=request.data)` and
  `model_activity(requested_data=request.data)` are the only sites that do not `json.dumps` it
  first. For a JSON request that publishes an object; for a form-encoded request `request.data`
  is a `QueryDict`, which kombu serialises as `{"x": ["1"]}` instead of `{"x": "1"}`.
* **`push_instance_metrics` is both scheduled and enqueued** from
  `plane/license/management/commands/register_instance.py:90`, which runs inside
  `docker-entrypoint-api.sh` on **every API container start** — so the task fires once per API
  replica boot in addition to the beat schedule.
* **Read endpoints enqueue work.** `recent_visited_task` is published from ten `GET`
  handlers — issue list, issue detail, issue-by-identifier, project detail, cycle detail,
  module detail, page detail, and both view endpoints — and `get_asset_object_metadata` from
  the asset `PATCH` endpoints. A ported read route is not "side-effect free": it must still
  publish these messages, or the "recently visited" list and the asset metadata stop
  updating for Rust-served pages.
* **13 schedule entries, 12 distinct tasks, 14 database rows** — see §7.1 and §7.2.

---

## 9. Regenerating this inventory

```bash
# 1. Messages (no broker, no database, no network)
cd apps/api
.venv/bin/python ../../apps/server/tools/inventory/celery_capture.py

# 2. Registered names
cd apps/api
DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=x \
DATABASE_URL=postgresql://x:x@127.0.0.1:1/x REDIS_URL=redis://127.0.0.1:1/0 \
.venv/bin/python -c '
import django; django.setup()
from plane.celery import app
app.autodiscover_tasks(); app.loader.import_default_modules()
print("\n".join(sorted(n for n in app.tasks if not n.startswith("celery."))))'
```

The caller tables in §5 were produced from an AST walk over `apps/api/plane/**/*.py`
(`.delay`/`.apply_async` call nodes, resolved through each module's import map) joined with
the URL resolver (`django.urls.get_resolver()`, mapping `(module, view class, action)` →
paths). Re-do it if routes or call sites change; task names alone are not enough to plan the
publisher.

---

## 10. Ownership during coexistence

A task has exactly one owner. The machine-readable table is
`crates/plane-jobs/src/ownership.rs`; the default is Celery and a task moves
only when its handler and its last caller move together. The same table is
consulted by `route::dispatch` and by the worker's handler registry.

| Task | Phase 4 owner | Retry policy / handler |
| --- | --- | --- |
| every task in §4 except the rows below | Celery | unchanged; the typed publisher in `plane-jobs::celery` is byte-identical |
| `plane.jobs.echo` (test-only) | Rust | no retries; handler in `bins/plane-worker` sleeps `payload.sleep_ms` |
| `recent_visited_task` (P0, task 34) | Rust | no retries (`policy::DEAD_ON_FAILURE`); `plane-tasks::recent_visited`, including the `soft_delete_related_objects` publish the trim's `delete()` makes |
| `delete_api_logs`, `delete_email_notification_logs`, `delete_page_versions`, `delete_issue_description_versions`, `delete_webhook_logs` (3-7) | Rust | no retries; `plane-tasks::cleanup` |
| `hard_delete` (10) | Rust | no retries; `plane-tasks::deletion`, reproduces the upstream `DO_NOTHING` `IntegrityError` (see §6 of the cutover plan) |
| `archive_and_close_old_issues`, `unsnooze_expired_intake_issues` (19, 20) | Rust | no retries; `plane-tasks::automation`; archive/close publishes `issue_activity` through `dispatch` |
| `process_logs` (27) | Rust | no retries; `plane-tasks::logs`; `APITokenLogMiddleware` enqueues with `dispatch` |
| `soft_delete_related_objects` (9) | Rust | no retries; handler in `plane-tasks::deletion`. Moved with set A3: its only non-Rust producer was `track_page_version`'s trim (`plane/bgtasks/page_version_task.py:74`), which is unreachable because the task raises `AttributeError` on `page.description` at `:55`/`:63` before reaching it |
| `issue_description_version_task`, `page_transaction`, `track_page_version` (23, 30, 31) | Rust | no retries; `plane-tasks::versions`. `track_page_version` reproduces the upstream no-op: `Page` has no `description` attribute, so both write branches raise and log (`page_version_task.py:55,63`) and no version row or cascade is produced |
| the two version backfills (`sync_issue_version`, `sync_issue_description_version`, 21/25) | no queue owner | `plane-admin` subcommands `sync-issue-version` / `sync-issue-description-version`; command-only, never enqueued (`plans/CUTOVER.md` §1 line 72) |

A **schedule** has the same split in `plane_jobs.schedules.owner`.
`plane-jobs/src/beat.rs::RUST_SCHEDULES` lists the entries whose task is
Rust-owned; `seed_schedules` flips those rows to `owner='rust'`, and the
quiet-window handover in `Beat::run` then disables the matching
`django_celery_beat_periodictask` row and records `handover_at`. The entries
moved in Phase 8 set D: `check-every-five-minutes-to-unsnooze-intake-issues`,
`check-every-day-to-delete-hard-delete`,
`check-every-day-to-archive-and-close`, and the five
`check-every-day-to-delete-*` cleanup entries. Every other entry in §7.1 stays
Celery-owned; `plane.jobs.test-tick` is the test-only Rust entry. The
`enabled=False` survives `DatabaseScheduler` restarts (the finding in
`JOBS.md`).

`PLANE_RS_JOBS_RUST_TASKS` is a comma-separated dev/test override for
`ownership::owner`; it is not for deployments.
