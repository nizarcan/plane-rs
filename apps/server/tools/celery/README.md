# Celery coexistence publisher: proofs and tools

`apps/server/crates/plane-jobs` (module `celery`) publishes Celery-protocol
messages so a Rust handler can enqueue a task that Django's worker still owns.
These tools record what Python actually sends, compare the Rust encoder against
it, exercise a real broker, and run the real worker.

Everything here uses the isolated dev stack (`apps/server/dev/`): local
RabbitMQ on 127.0.0.1:55672, one disposable vhost per run, and a database
cloned from `plane_tpl` for the worker demo.

## Files

| Path | Purpose |
| --- | --- |
| `cases.py` | The representative calls: case id, task name, positional args, kwargs and optional `countdown`. One source of truth for Python and Rust. |
| `record.py` | Publishes every case over kombu's in-memory transport with pinned id/origin/reply_to/now and writes `golden/messages.json` (properties, headers, raw body bytes). No broker, no database. |
| `golden/messages.json` | 64 recorded messages, checked in. Regenerate after a Celery/kombu bump. |
| `live_publish.py` | Publishes one case to a real vhost with the same pinned identity. |
| `drain.py` | Drains the queue with Python kombu (`auto_decode=False`) and prints the raw bytes plus the decoded args/kwargs. |
| `drain/` | A standalone Rust binary that drains with `plane-testkit`'s decoder, i.e. the differential harness's own view. |
| `compare_live.py` | Asserts the four decodings agree (Python/Rust × kombu/testkit) and that the live bytes match `golden/messages.json`. |
| `live_check.sh` | Creates a fresh vhost, verifies it starts empty, runs the crate's live tests, cross-decodes five cases, then verifies the topology the first publish created. Deletes the vhost. |
| `fixture.py` | Minimal user/workspace/project rows for the worker demo. |
| `live_worker.sh` | Clones a database, starts a real Plane Celery worker, publishes `recent_visited_task` and `process_logs` from Rust, waits for the rows, stops the worker, cleans up. |
| `bench.sh` | Publishes 1,000 messages to a disposable vhost and prints p50/p99, throughput and RSS. |

## Regenerating the golden file

```sh
cd apps/api
.venv/bin/python ../../apps/server/tools/celery/record.py
cd ../server
cargo test -p plane-jobs          # golden + URL + repr tests
```

`record.py` sets `AMQP_URL=memory://` before Django imports, patches
`celery.app.amqp.anon_nodename` (`origin`), `app._local.oid` (`reply_to`) and
`app.now()` (the `countdown` → `eta` instant), and passes an explicit
`task_id=`. Positional arguments are converted to a **tuple** before
`apply_async`, because `.delay(*args)` builds a tuple and `argsrepr` renders
`('a',)` rather than `['a']`.

The file contains no timestamp, so a rerun that changes nothing produces an
identical file.

## Live check

```sh
apps/server/tools/celery/live_check.sh
```

It prints, per case, `OK <case>: ... 4 decoders agree, golden matches`, then
`topology: exchange celery(direct,durable) -> queue celery(durable,no args)
rk=celery`. The crate's live tests are gated on `PLANE_RS_TEST_AMQP_URL`; the
script sets it to its disposable vhost and runs them with
`--test-threads=1`.

## Worker demo

```sh
apps/server/tools/celery/live_worker.sh          # cleans up
apps/server/tools/celery/live_worker.sh --keep   # leaves db, vhost and log
```

Expected tail: the two `received` log lines, a `user_recent_visits` row for
`aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee`, and an `api_activity_logs` row with
`token_identifier = b3f1c0d2e3a4`.

The worker runs with `-P solo`. On this host billiard uses the **spawn** start
method for the prefork pool; the child enters `workloop`, but the parent never
dispatches the queued tasks and never logs `ready.`. The solo pool runs the
same Celery app, consumer, serializer and task code, and the task rows prove
execution. The worker's `ready.` line is also swallowed by Plane's custom JSON
logging; the script detects readiness by polling the RabbitMQ management API
for a consumer on the `celery` queue instead.

## Benchmarks

```sh
apps/server/tools/celery/bench.sh
```

Publishes 1,000 pre-rendered messages to a disposable vhost and prints
p50/p99/mean, throughput, and RSS before connecting, after connecting and
after publishing. It is a send-side measurement: publisher confirms are off,
as they are in Celery.

## Protocol notes that surprised us

Recorded from Celery 5.5.3 / kombu 5.5.4; the crate's module docs carry the
file:line citations.

* `argsrepr`/`kwargsrepr` are **not** Python `repr()`. Celery's `saferepr`
  wraps strings in single quotes and escapes only `'`; backslashes, newlines,
  tabs and control characters stay raw. Strings are truncated at the last
  space inside the remaining 1024-character budget, and when the budget runs
  out the renderer emits `, ...` followed by every dangling closing bracket —
  which can produce output like `{'a': {'b': , ...]}}`. `repr.rs` reproduces
  that, including the mismatched brackets.
* `reply_to` is **not** a fresh uuid4: Celery uses `app.thread_oid`, a UUID3
  derived from a process-global node id, the pid, the thread id and
  `id(app)`. Nothing consumes it (there is no result backend), so the Rust
  publisher uses a per-process uuid4 and the golden file pins both sides.
* Publisher confirms are off (`amqp.Connection(confirm_publish=False)`); the
  publisher returns when the frames are written.
* Declaring Celery's queue also declares a **`celery` direct exchange** and a
  binding `celery -> celery` with routing key `celery`, even though messages
  are published to the nameless exchange with routing key `celery`.
* The `task_default_exchange`/`task_default_routing_key` values (`celery`)
  are not what goes on the wire; `amqp.py:501-502` rewrites the direct
  exchange to `""`.
* `send_email_notification` receives `issue_id` as a **UUID object** from
  `EmailNotificationLog.entity_identifier`, not the `str()` the inventory
  capture used. The typed function takes `Uuid`.
* `page_transaction.page_id` is a string at the create site and a UUID at the
  other three; `crawl_work_item_link_title.id` is a UUID at one of four
  sites; `issue_activity.issue_id`/`project_id` switch type per call site.
  The typed API makes each caller choose.
* `sync_issue_version` / `sync_issue_description_version` use
  `apply_async(kwargs=…, countdown=…)` for the self re-enqueue (eta header)
  but `.delay(batch_size=…, countdown=…)` for the entry point (no eta, even
  though `countdown` is a kwarg).
* `manage.py sync_issue_version` passes `input()`'s string straight to
  `.delay()`, so the entry-point `batch_size` is `"5000"`, not `5000`.
* The worker never reads `argsrepr`, `kwargsrepr` or `origin`; they are
  informational, and `TASKS.md` §2.7 lists them as harness normalisations.
  This crate still reproduces them because the golden test compares headers.
