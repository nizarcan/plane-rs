# JOBS.md — the Postgres job queue, worker and beat

`RUST_REWRITE.md` Phase 4. This note is the design of record for
`crates/plane-jobs`, `bins/plane-worker`, `bins/plane-beat` and
`bins/plane-migrator`. It does not move any real task: every production task and
schedule stays Celery-owned in this phase. The routing table exists so domains
can move one task at a time in Phase 5.

## The `DatabaseScheduler` finding (most important)

**`enabled=False` survives every Celery beat restart, with no change to
`apps/api`.** `DatabaseScheduler.setup_schedule()` calls
`update_from_dict(app.conf.beat_schedule)` (`django_celery_beat/schedulers.py:257-260`).
`Entry.from_entry` is `PeriodicTask.objects.update_or_create(name=name,
defaults=self._unpack_fields(**entry))` (`:191-195`), and `_unpack_fields`
only supplies `schedule`, `args`, `kwargs`, `queue`, `exchange`,
`routing_key`, `priority`, `headers` and `expire_seconds` (`:199-217`) — not
`enabled`. `update_from_dict` then re-adds an entry only when
`entry.model.enabled` is true (`:472-483`). Therefore:

- an operator (or the cutover migration) sets
  `django_celery_beat_periodictask.enabled = false` for one static entry;
- Django beat stops firing it immediately and every future upsert leaves
  `enabled` false;
- `PeriodicTask.save()` clears `last_run_at` when disabled
  (`django_celery_beat/models.py:620-626`), which is the correct state for a
  handover.

`plane-beat` uses this as the handover mechanism: when its own
`plane_jobs.schedules.owner` is `rust`, it disables the matching
`django_celery_beat_periodictask` row (idempotently) and owns that tick. No
Python edit, no migration, and a restart of Django beat cannot re-enable it.
The table also has `enabled` of its own, so an operator can disable a
Rust-owned schedule without editing code.

A **running** Celery beat only reloads when
`DatabaseScheduler.schedule_changed()` sees `django_celery_beat_periodictasks.last_update`
move (`schedulers.py:408-437`, checked from the `schedule` property in
`celery/beat.py:337` at every tick). Django writes that row through the
`post_save` signal (`PeriodicTasks.update_changed`). A raw
`UPDATE django_celery_beat_periodictask` bypasses it, so every disable does the
same upsert in the **same transaction**:

```sql
INSERT INTO django_celery_beat_periodictasks (ident, last_update)
VALUES (1, now())
ON CONFLICT (ident) DO UPDATE SET last_update = EXCLUDED.last_update;
```

It runs only when `UPDATE … WHERE enabled RETURNING name` actually changed a
row. Do **not** use Celery's `last_run_at` to close the race: a running
`DatabaseScheduler` writes `last_run_at = NULL` whenever it saves a now-disabled
row (`PeriodicTask.save()`, `models.py:619-621`), so the value is not a reliable
record of whether it fired.

Instead the handover happens only inside a **quiet window**. For each
Rust-owned schedule that has not been handed over yet (`handover_at IS NULL`),
compute its previous and next due ticks from its own crontab/interval rules in
UTC. Disable the Django row and bump the tracker only when both hold:

- `now - previous >= G` — Celery has certainly fired the previous tick;
- `next - now >= M` — Celery will reload before the next tick.

Defaults are **G = 30 s** and **M = 15 s**. `DatabaseScheduler`'s
`DEFAULT_MAX_INTERVAL = 5` seconds (`django_celery_beat/schedulers.py:33`) and
`schedule_changed()` runs at the start of every tick (`:506-517`), so G is 6×
and M is 3× the loop. If the window is not quiet, `plane-beat` waits and
re-checks; it does not disable early and does not rely on a fixed sleep.

At handover it sets `plane_jobs.schedules.last_run_at = previous`, so
`plane-beat` starts firing at `next` and never re-runs `previous`. Every tick is
then owned by exactly one side: the previous tick was Celery's, the next is
Rust's. Both values are configurable with
`PLANE_RS_JOBS_HANDOVER_GAP_SECONDS` and
`PLANE_RS_JOBS_HANDOVER_MARGIN_SECONDS`.


## Schema (`plane_jobs`, created by the Rust migrator)

```
jobs(id uuid pk, task text, payload jsonb, state text,
     attempts int, max_retries int, retry_backoff_seconds int, retry_jitter bool,
     run_at timestamptz, lease_until timestamptz, locked_by text,
     last_error text, dedupe_key text, created_at, updated_at, finished_at)
```

States: `queued` (runnable at `run_at`), `running` (leased to `locked_by`
until `lease_until`), `failed` (an attempt failed; retry is pending at
`run_at`), `done`, `dead` (retries exhausted or non-retryable). Claiming
looks at `(state IN ('queued','failed') AND run_at <= now())` **or**
`(state = 'running' AND lease_until < now())`. Indexes:

- `(run_at, id) WHERE state IN ('queued','failed')`
- `(lease_until) WHERE state = 'running'`
- unique `(dedupe_key) WHERE dedupe_key IS NOT NULL AND state IN ('queued','running')`

`attempts` counts started attempts, so an initial run plus two retries is
`max_retries = 2`, `attempts` reaching 3, then `dead`.

```
schedules(name text pk, task text, owner text, kind text, cron text,
          interval_seconds bigint, enabled bool, last_run_at timestamptz,
          handover_at timestamptz, updated_at)
ticks(schedule_name text, tick_at timestamptz, fired_at timestamptz, job_id uuid,
      primary key (schedule_name, tick_at))
meta(key text pk, value text)
```

`ticks` is the exactly-once ledger; `schedules.last_run_at` is the Celery
equivalent (the wall-clock time of the last fire, not the scheduled tick).

## Migration

`plane_jobs::queue::migrate(&client)` issues only
`CREATE SCHEMA/TABLE/INDEX IF NOT EXISTS` and seed `INSERT … ON CONFLICT DO
NOTHING`. `bins/plane-migrator` runs it with `Role::Migrator`. It never
creates a Django migration or table: `apps/api/migrations_lockfile.txt` and
the Django schema check are unaffected, and a database without the Django
`django_celery_beat` tables still migrates (the handover UPDATE is guarded by
`to_regclass`). `schema_check` only prepares the static catalog statements, so
an extra `plane_jobs` schema is invisible to it.

## Enqueue API and ownership

- `queue::enqueue(&tx, &job)` — generic over `tokio_postgres::GenericClient`,
  so it is usable inside a caller's transaction. `INSERT` + `pg_notify` are
  in that transaction; a rollback leaves no job and no notification.
- `queue::enqueue(&client, &job)` — same function with an autocommit client.
- `route::dispatch(task, job, celery_message, publisher, client)` — the one
  call-site API. It consults `ownership::owner(task)`:
  - `celery`: publishes the exact typed `celery::Message` (byte-identical to
    today's publisher) and never touches Postgres;
  - `rust`: inserts the `Job` and returns its id.
- `crates/plane-jobs/src/ownership.rs` is the ownership table: every task is
  `celery` unless listed as `rust`, and the list is the machine-readable half
  of `TASKS.md` §10. `PLANE_RS_JOBS_RUST_TASKS` (comma-separated, test/dev only)
  can add overrides; it is documented in `ENV.md`. Default is all Celery, so
  Phase 4 moves nothing.

`common/jobs.rs` routes `recent_visited_task` through `dispatch`, so the
handler exercises the routing API. With the default Celery ownership the
differential scenario is byte-identical. The Phase 4 exit test marks the
test-only task `plane.jobs.echo` Rust-owned and runs it through the worker.

## Worker semantics

- Claim: one statement with a `FOR UPDATE SKIP LOCKED` CTE and an `UPDATE …
  RETURNING`; four workers on one queue never see the same row. Expired leases
  are claimable by anyone (at-least-once, so handlers must be idempotent).
- Wake-up: a dedicated `LISTEN plane_jobs_jobs` connection; `enqueue` calls
  `pg_notify` in the same transaction. A poll fallback (`PLANE_RS_JOB_POLL_MS`,
  default 1000 ms) covers a dropped listener, broker-less startup and a missed
  notification. The listener reconnects with backoff.
- Completion/failure: `done` on success. On a retryable failure with retries
  left: `failed`, `run_at = now + backoff` (fixed seconds, or uniform jitter
  when `retry_jitter`). On exhaustion or a non-retryable error: `dead` and
  `last_error`. `webhook_send_task`, the only Python task with a decorator
  retry policy, is documented as 5 retries with a uniform 0–600 s jitter;
  every other Python task swallows its exception, which the Rust side replaces
  with an explicit policy in its ownership row rather than silent success.
- Concurrency: `PLANE_RS_JOB_CONCURRENCY` (default 4) in-flight handlers.
- Shutdown: SIGTERM/SIGINT stops claiming, drains in-flight handlers for
  `PLANE_RS_JOB_DRAIN_SECONDS` (default 30), then releases anything still
  `running` for this worker back to `queued` before exit.

## Beat semantics

`plane_jobs.schedules` is seeded with the 13 `celery.py:46-101` entries
(crontab minute/hour, UTC, plus the `push-instance-metrics` interval). The
test-only `plane.jobs.test-tick` entry is added when
`PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS` is set. `push-instance-metrics` remains
Celery-owned (owner decision 5: not ported).

Before firing, a Rust-owned entry with `handover_at IS NULL` waits for the
quiet window above and is handed over there (disable + tracker bump + set
`last_run_at = previous` in one transaction). If an operator re-enables a row
after handover, the next loop sets `handover_at = NULL` and returns to that
waiting path; it does not fire in the meantime.

For each handed-over, due entry, `plane-beat` opens one transaction:

1. `pg_try_advisory_xact_lock(hashtextextended(name || ':' || tick, 0))` —
   a second replica skips the tick;
2. `INSERT INTO ticks … ON CONFLICT DO NOTHING RETURNING` — a restart or a
   race that gets past the advisory lock still cannot fire twice;
3. insert the job and `pg_notify`;
4. set `schedules.last_run_at = now()` and commit.

A missed tick is handled like Celery's `DatabaseScheduler`: it fires **once**
for the latest due tick (the scheduler's `reserve` sets `last_run_at = now`),
not once per missed interval. A crash before commit leaves the tick and job
absent; a crash after commit leaves both present. Killing one replica is safe:
the other holds no persistent lock and simply continues from `last_run_at`.

## Binaries and configuration

- `bins/plane-migrator` — `Role::Migrator`, creates/updates the schema.
- `bins/plane-worker` — `Role::Worker`, registers Rust-owned handlers (today
  only the test task `plane.jobs.echo`), runs the claim loop and listener.
- `bins/plane-beat` — `Role::Beat`, seeds/reads schedules, disables the
  matching Django rows for Rust-owned entries, runs the tick loop.

New environment variables (all documented in `ENV.md` §6):
`PLANE_RS_JOB_CONCURRENCY`, `PLANE_RS_JOB_LEASE_SECONDS`,
`PLANE_RS_JOB_DRAIN_SECONDS`, `PLANE_RS_JOB_POLL_MS`,
`PLANE_RS_BEAT_POLL_MS`, `PLANE_RS_JOBS_TEST_SCHEDULE_SECONDS`,
`PLANE_RS_JOBS_HANDOVER_GAP_SECONDS`, `PLANE_RS_JOBS_HANDOVER_MARGIN_SECONDS`,
`PLANE_RS_JOBS_RUST_TASKS`.

## Phase 4 scope and open questions

Moving a real task or schedule is **not** in this phase. The owner decisions
that remain: (1) whether `push_instance_metrics` is dropped at cutover (default
assumed: yes, RUST_REWRITE open decision 5); (2) the upstream
`delete_old_s3_link` duplicate schedule (13 entries, 12 tasks) is preserved
as-is; (3) Python swallows every task exception except `workspace_seed`, while
this queue makes failure observable — domains must choose an explicit retry
policy per task when they move it (the queue fields support fixed or jittered
backoff, and `max_retries = 0` means dead on first failure).
