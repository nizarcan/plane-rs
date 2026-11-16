# Benchmarks

`RUST_REWRITE.md`'s fifth verification item: "Record idle memory and p50/p95 for a fixed
request set after each phase, in `apps/server/BENCH.md`." This file is that record: the
method, the numbers, and the exact commands that produced them.

The numbers are produced by `difftest bench` (in `crates/plane-testkit`), which reuses the
differential harness's target provisioning: a managed target gets a fresh clone of the
`plane_tpl` database, its own Redis index and RabbitMQ vhost, and the same environment a
differential run uses. Nothing here is a synthetic in-process loop; requests go over
loopback TCP to a real gunicorn/uvicorn or `plane-api` process.

A number without its method is not evidence. Read "Method" and "Threats to validity" before
quoting a row.

## Machine

| | |
| --- | --- |
| Host | Apple MacBook Pro, Apple M3 Max (14-core CPU: 10 performance + 4 efficiency), 36 GB unified memory (38,654,705,664 bytes) |
| OS | macOS 26.6.2, build 25G83 |
| Rust | `rustc 1.99.1` / `cargo 1.99.1` (Homebrew; no rustup), edition 2024 |
| Release profile | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"` (`apps/server/Cargo.toml`) |
| Python | 3.12.13 (`apps/api/.venv`) |
| Django stack | Django 5.2.17, DRF 3.18.3, gunicorn 23.0.0, uvicorn 0.29.0 |
| Celery | 5.5.3 (`django_celery_beat` database scheduler) |
| Postgres | 15.7-alpine in the dev stack's Docker VM, 2 vCPU / ~2 GB (shared with Valkey 7.2, RabbitMQ 3.13, MinIO) |
| Load | targets are native host processes, all bound to 127.0.0.1; Docker is used only for Postgres/Valkey/RabbitMQ/MinIO |

The host is a **shared** machine: other agents on the same repository run builds and test
suites while a benchmark runs. Every JSON report records the 1-minute load average before
and after the run (`load_average_start` / `load_average_end`), and the Markdown header of
each run shows it. The six Phase 1 invocations below ran at 1-minute load averages of
**4.3–9.0** on a 14-core host; earlier smoke runs overlapped other agents' builds at 16–26,
i.e. this is **not a quiet machine**. See "Threats to validity".

## Method

### Targets

* **django**: gunicorn + `uvicorn.workers.UvicornWorker`, one worker
  (`GUNICORN_WORKERS=1`, the production default in
  `deployments/cli/community/variables.env`), exactly as `apps/server/dev/django-serve.sh`
  starts it. That script honours `GUNICORN_WORKERS`
  (`--workers "${GUNICORN_WORKERS:-1}"`), which is what `--django-workers` sets, and it
  runs gunicorn in the foreground so the harness's process group contains the master and
  every worker. Settings are `plane.settings.production` with `DEBUG=0` (the dev stack's
  standard), and the worker gets the same database clone, Redis index and RabbitMQ vhost
  the harness gives `plane-api`.
* **rust**: `apps/server/target/release/plane-api`, built from the working tree with
  `cargo build --release -p plane-api`, bound to its own port against its own database
  clone. The default measurement leaves tokio's worker-thread count at its default (one per
  core, so 14); the equal-CPU measurement sets `--rust-threads 1`, i.e.
  `PLANE_RS_WORKER_THREADS=1` (`ENV.md` §`PLANE_RS_WORKER_THREADS`).
* **workers/threads** in the tables is exactly that knob: Django has processes, Rust has
  threads in one process. A gunicorn worker is one event loop on one core; a tokio worker
  thread is the same unit of parallelism. Comparing "1 gunicorn worker" with "1 tokio
  thread" is the equal-parallelism comparison; 14 threads vs 1 process is the
  as-deployed comparison.
* The target is **restarted for every repetition** in managed mode, so "idle RSS" is always
  a fresh process's, never memory left inflated by the previous load. Setup runs once per
  invocation; its rows and session cookies live in the database and survive the restarts.

Django is served without the production entrypoint's `--max-requests 1200
--max-requests-jitter 1000` worker recycling (`apps/api/bin/docker-entrypoint-api.sh:38`):
`dev/django-serve.sh` deliberately omits it. Under a 20 s run at high request rates, worker
recycling would add repeated interpreter startups and change both throughput and RSS, so
the Django numbers here are a **sustained-load upper bound** relative to a deployment that
keeps `--max-requests`.

### Client

`difftest bench` is a closed-loop load generator:

* one HTTP/1.1 keep-alive connection per concurrent worker, one request in flight per
  connection ("concurrency" is therefore both the number of connections and the number of
  outstanding requests);
* request templates are pre-rendered once (path, query, headers, cookie from the setup
  jar, body), so the hot loop does no formatting;
* response bodies are drained frame by frame, and latency is recorded per request in an
  HDR histogram (microseconds, 1 µs–60 s, three significant figures), so memory use does
  not grow with the number of samples;
* a connection the server closes is re-opened and the request retried once; a retry is
  counted as `reconnect`, not as an error;
* an **error** is a response whose status differs from the template's `expect_status`
  (default 200) or a transport failure after the retry. A 404 probe declares
  `expect_status: 404` and is not an error.

Before load, each concurrency level gets a discarded warm-up (`--warmup`, 5 s), so imports,
connection pools, page caches and any JIT-less Python startup costs are out of the measured
window.

Because the client runs on the same machine, it is measured too: `--headroom-duration`
runs the **same loop** against a trivial in-process hyper server on localhost that answers
`200` with an empty body, at every concurrency level. That number bounds what the
instrument can resolve: a target measured at or near the ceiling is a lower bound, not a
capacity. The headroom table is printed with every run that enables it and recorded below.

### Memory and CPU

Both are measured for the **whole target process group**, not for the process the harness
happened to spawn. Managed targets are started in their own process group, so the gunicorn
master, every uvicorn worker, and anything they fork share it.

* **Idle RSS**: after a 10 s settle (`--settle`), the median of three `ps -A -o
  pgid=,pid=,rss=,time=` samples 250 ms apart, summed over the group, in MiB. It is taken
  before any load in that repetition.
* **Peak RSS**: the maximum summed RSS seen by the 250 ms sampler during the measured
  window, plus the final sample.
* **After RSS**: the summed RSS at the end of the measured window, before the target is
  stopped.
* **CPU seconds**: consumed CPU time (user + system) of the whole group, the difference
  between the `ps` `time` column sampled just before the measured window (after warm-up)
  and just after it. `ps` reports it at centisecond resolution on macOS, which is ample
  over a 20 s window. Because a process's CPU time disappears from `ps` when it exits, a
  target that restarts a worker mid-window under-reports; the report records
  `process_missing` when the group is empty at the end.

**RSS on macOS is not container memory.** `ps` reports resident pages, and the sum counts
shared pages once per process: gunicorn workers share their parent's copy-on-write pages
and every process maps the same libraries, so the sum is an upper bound (not PSS). A Linux
container's memory accounting includes the page cache and is what a deployment pays for;
the same Rust binary will report a different number there. **The final memory comparison
must be re-measured in the Linux containers**; these numbers are for relative change per
phase and for regression detection on this machine, not for a deployment sizing table.

### Noise control

* closed-loop, not open-loop: the target is never handed more work than it finished;
* 5 s warm-up, 20 s measured window, 10 s settle before the idle sample;
* three independent repetitions, each on a freshly started target; the table shows the
  **median** of the three for throughput, percentiles, memory and CPU. `errors` and
  `reconnects` show the **worst** repetition, so one failed run cannot hide behind two
  clean ones. Every per-repetition value is in the JSON report;
* the load average before and after is recorded, because the host is shared;
* exactly one benchmark runs at a time (the matrix script is sequential and says so);
* the harness itself is a **release** build. A debug-build client is slower than the
  server it measures: during development, the debug `difftest` topped out at ~18k
  req/s at concurrency 1 and ~56k at 4, which is why the headroom table exists and why
  BENCH.md only quotes release-harness numbers.

### Sequence of one measurement

For each repetition, managed mode:

1. clone `plane_tpl` once per invocation (database `bench_<run-id>`), create a RabbitMQ
   vhost and flush Redis DB 3;
2. start the target on a free port from its range in `scenarios/targets.yaml`, wait for its
   health path;
3. run the scenario's `setup` steps (once per invocation), capturing variables and cookies;
4. settle 10 s;
5. sample idle RSS (three samples, median);
6. for each concurrency: warm up (`--warmup`), then start the RSS/CPU sampler, run the
   measured window, stop the sampler;
7. stop the whole process group (SIGTERM, then SIGKILL after 15 s);
8. drop the database and vhost unless `--keep`.

Attach mode (`--url`/`--db`, optionally `--pgid`) measures an already-running target: no
clone, no restart, no stop. Without `--pgid` the memory and CPU columns are zero and the
report says so.

## Bench scenario format

Scenarios live in `scenarios/bench/` and are validated before anything is started. A
scenario is a differential scenario's request format plus weights:

```yaml
name: static-responses          # required
template_db: plane_tpl          # default
vars:                           # available as {{name}} everywhere
  email: bench@example.com
  password: Bench-Passw0rd-2026!
target_env:                     # extra env for the managed target (same rules as
  FILE_SIZE_LIMIT: "4096"       # a differential scenario's target_env)
redis_db: 3                     # 0-15; bench's default, differential runs use 1 and 2

setup:                          # ordinary differential steps, run once per invocation
  - name: sign-in-csrf-token
    path: /auth/get-csrf-token/
    jar: user
    capture:
      - name: sign_in_csrf
        source: json
        pointer: /csrf_token
  - name: sign-in
    method: POST
    path: /auth/sign-in/
    jar: user
    form:
      email: "{{email}}"
      password: "{{password}}"
      csrfmiddlewaretoken: "{{sign_in_csrf}}"
    expect_status: 302
    capture:
      - name: slug
        source: regex
        header: Location
        pattern: '/workspaces/([^/]+)/'

requests:                       # measured templates
  - name: health
    method: GET                 # default GET
    path: /
    weight: 1                   # share of traffic = weight / sum(weights); default 1
    expect_status: 200          # anything else counts as an error; default 200
  - name: missing
    path: /no-such-route
    weight: 1
    expect_status: 404
```

The setup step shape is **the differential harness's step shape**, field for field
(`scenarios/README.md`): `method`, `path`, `query`, `headers`, `json`, `form`, `body`,
`jar`, `capture` (`json` pointer, `header`, `cookie`, `regex`) and `expect_status`.
Validation is delegated to the differential scenario validator, so the two formats cannot
drift. A setup step without `expect_status` must answer below 400; a setup that quietly
401s is a hard error, not a measurement of 401s.

`vars: { fixture_sql: "{repo}/scenarios/my-fixture.sql" }` is the differential harness's
reserved variable for a SQL fixture. Bench applies it to the managed database clone after
cloning and before setup, so rows that no HTTP route can create yet (users, sessions,
objects for a read path) are part of the fixture rather than of the measurement. An
attached database is never modified: in attach mode the caller owns its contents.

Every request template may set `jar: user`: the measured request then carries the
`Cookie:` header that the named jar accumulated during setup. That is how an authenticated
read path is benchmarked later:

1. sign in during `setup`, capture the workspace/project/work-item ids from the responses;
2. use them in `requests` (`{{slug}}`, `{{project_id}}`, ...), with `jar: user`;
3. the measured loop sends exactly the same session cookie on every request, and setup is
   paid once, not per request.

Two scenarios exist in Phase 1:

| Scenario | Mix |
| --- | --- |
| `health.yaml` | `GET /` only |
| `static-responses.yaml` | 4/10 `GET /`, 3/10 `GET /robots.txt`, 2/10 `GET /bench-no-such-route` (404), 1/10 CORS preflight `OPTIONS /api/workspaces/` |

Both are routes `plane-api` serves today: `/` and `/robots.txt` are ported; the 404 is
Django's custom not-found body; the preflight is short-circuited by `CorsMiddleware` in both
backends before routing or a session is touched. None of them reads the database, which is
the point at this phase: the difference is the framework and the middleware stack, not a
query.

## Results

Numbers are the **median of three repetitions** of a 20 s measured window after a 5 s
warm-up, on a freshly started target. `date` is the report date; `commit` is the branch
head the release binary was built near — the working tree also contained other agents'
in-progress changes at the time, so treat the commit as a label, not a provenance proof.
The `plane-api` binary was built at 20:33 UTC on 2026-11-15 (23:33 local) from the working
tree; the four paths these scenarios exercise are the ones the Phase 1 exit check's
differential run already compares byte for byte against Django (`PROGRESS.md`).
Every row is generated by `tools/bench/summarize.py` from the JSON reports; the reports
also carry every per-repetition value, the load averages and the machine description.

### Phase 1 — skeleton (2026-11-15, commit `13cbdbeb4e` + working tree)

Phase 1's exit check (see `PROGRESS.md`) is the two static responses and the middleware
stack. There is no database, cache or broker work on these paths in either backend, so the
tables isolate the framework and middleware cost. Django still runs its full stack (CORS,
security headers, WhiteNoise, session, auth, CSRF, crum, gzip, body limit, request
logging); `plane-api` runs the ported equivalent as one explicit pipeline.

#### Phase 1 measurements

Generated by `tools/bench/summarize.py` from the six JSON reports in
`apps/server/target/bench/phase1-20261115/` (gitignored; the command to regenerate them is
below). Medians of three repetitions. `CPU s` is the whole target process tree over the
20 s window (so 20.00 s is one saturated core).

| date | commit | scenario | target | workers/threads | conc | rps | p50 ms | p95 ms | p99 ms | idle RSS | peak RSS | CPU s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-11-15 | 13cbdbeb4e | health | rust | default | 1 | 31810 | 0.03 | 0.04 | 0.05 | 18.1 MiB | 18.2 MiB | 9.86 |
| 2026-11-15 | 13cbdbeb4e | health | rust | default | 8 | 109715 | 0.07 | 0.11 | 0.13 | 18.1 MiB | 20.6 MiB | 46.86 |
| 2026-11-15 | 13cbdbeb4e | health | rust | default | 64 | 141196 | 0.45 | 0.53 | 0.57 | 18.1 MiB | 27.1 MiB | 63.57 |
| 2026-11-15 | 13cbdbeb4e | health | rust | threads=1 | 1 | 34242 | 0.03 | 0.04 | 0.05 | 17.3 MiB | 17.3 MiB | 9.90 |
| 2026-11-15 | 13cbdbeb4e | health | rust | threads=1 | 8 | 95112 | 0.08 | 0.12 | 0.14 | 17.3 MiB | 17.3 MiB | 19.92 |
| 2026-11-15 | 13cbdbeb4e | health | rust | threads=1 | 64 | 92244 | 0.71 | 0.84 | 0.87 | 17.3 MiB | 19.1 MiB | 19.95 |
| 2026-11-15 | 13cbdbeb4e | health | django | workers=1 | 1 | 1268 | 0.77 | 0.93 | 1.04 | 177.7 MiB | 179.1 MiB | 17.72 |
| 2026-11-15 | 13cbdbeb4e | health | django | workers=1 | 8 | 1593 | 4.77 | 5.83 | 6.74 | 177.7 MiB | 182.3 MiB | 23.03 |
| 2026-11-15 | 13cbdbeb4e | health | django | workers=1 | 64 | 1503 | 37.12 | 76.48 | 81.41 | 177.7 MiB | 192.9 MiB | 22.92 |
| 2026-11-15 | 13cbdbeb4e | static-responses | rust | default | 1 | 27852 | 0.03 | 0.05 | 0.06 | 18.0 MiB | 18.4 MiB | 11.04 |
| 2026-11-15 | 13cbdbeb4e | static-responses | rust | default | 8 | 96009 | 0.08 | 0.14 | 0.18 | 18.0 MiB | 22.0 MiB | 59.72 |
| 2026-11-15 | 13cbdbeb4e | static-responses | rust | default | 64 | 119191 | 0.49 | 0.98 | 1.34 | 18.0 MiB | 29.1 MiB | 65.13 |
| 2026-11-15 | 13cbdbeb4e | static-responses | rust | threads=1 | 1 | 28124 | 0.03 | 0.05 | 0.06 | 17.3 MiB | 17.3 MiB | 10.98 |
| 2026-11-15 | 13cbdbeb4e | static-responses | rust | threads=1 | 8 | 69062 | 0.12 | 0.16 | 0.19 | 17.3 MiB | 17.3 MiB | 19.86 |
| 2026-11-15 | 13cbdbeb4e | static-responses | rust | threads=1 | 64 | 65932 | 0.97 | 1.15 | 1.26 | 17.3 MiB | 19.1 MiB | 19.86 |
| 2026-11-15 | 13cbdbeb4e | static-responses | django | workers=1 | 1 | 1124 | 0.87 | 1.16 | 1.39 | 177.7 MiB | 179.2 MiB | 17.93 |
| 2026-11-15 | 13cbdbeb4e | static-responses | django | workers=1 | 8 | 1367 | 5.67 | 7.24 | 13.17 | 177.7 MiB | 183.2 MiB | 22.65 |
| 2026-11-15 | 13cbdbeb4e | static-responses | django | workers=1 | 64 | 1264 | 47.23 | 90.17 | 98.69 | 177.7 MiB | 194.2 MiB | 22.62 |

Every measurement had **0 errors and 0 reconnects**; the per-repetition values, request
counts per template and the load averages are in the JSON reports. The
`static-responses` client sent the intended 4:3:2:1 mix (one repetition, concurrency 8:
768,668 `/`, 576,486 `/robots.txt`, 383,753 404s, 191,278 preflights).

#### Client headroom during those runs

Measured once, 20 s per level, in the same invocation as the first `health` row:

| date | commit | scenario | conc | rps | p50 ms | p95 ms | p99 ms |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-11-15 | 13cbdbeb4e | health | 1 | 47895 | 0.02 | 0.03 | 0.04 |
| 2026-11-15 | 13cbdbeb4e | health | 8 | 137556 | 0.06 | 0.08 | 0.10 |
| 2026-11-15 | 13cbdbeb4e | health | 64 | 150134 | 0.42 | 0.52 | 0.57 |

The Rust target (default threads) reached 66% / 80% / **94%** of the client ceiling at
concurrency
1 / 8 / 64 on `health`, and 58% / 70% / 79% on `static-responses`. The concurrency-64
`health` row is therefore a lower bound on what `plane-api` can serve, not a capacity; and
the concurrency-64 latency is largely the client's own queueing. At concurrency 1 the
client is also latency-bound (one request in flight over loopback), which is why the
single-connection rows differ so little between the two Rust configurations: they measure
round-trip overhead more than server capacity. Concurrency 8 is the most informative level
in this set.

#### What the numbers say

**Idle memory.** `plane-api` idles at 18.1 MiB as a process tree (default 14 tokio
threads) and 17.3 MiB with one thread; the Django gunicorn master + one uvicorn worker tree
idles at 177.7 MiB. That is **9.8× less memory** at equal (or better) parallelism, and it is
in the same family as the 60 MiB ceiling the brief sets for the finished `plane-api`
(`RUST_REWRITE.md`, "Memory targets"): the skeleton is already below it. Peak RSS under 64
connections is 27.1 MiB for Rust and 192.9 MiB for Django; the Rust growth is per-connection
buffers (~0.14 MiB per connection at 64), not per-request allocation.

**Throughput at equal parallelism** (one tokio thread vs one gunicorn worker):

| scenario | conc | rust 1 thread | django 1 worker | ratio |
| --- | --- | --- | --- | --- |
| health | 8 | 95,112 rps | 1,593 rps | 59.7× |
| health | 64 | 92,244 rps | 1,503 rps | 61.4× |
| static-responses | 8 | 69,062 rps | 1,367 rps | 50.5× |
| static-responses | 64 | 65,932 rps | 1,264 rps | 52.1× |

**As deployed** (tokio default threads vs one gunicorn worker): 68.9× on `health` and 70.2×
on `static-responses` at concurrency 8. The concurrency-64 figure (94×) is inflated by the
Django side queueing while the Rust side is client-bound; do not quote it as capability.

**CPU per request.** At concurrency 8 on `health`, one Rust thread spends 10.5 µs of CPU
per request and Django spends 723 µs: **69× less CPU for the same response**. On
`static-responses` it is 14.4 µs vs 828 µs (58×). The multi-threaded build spends more CPU
per request at the same concurrency (21.4 µs on `health`, 31.1 µs on `static-responses`) —
consistent with tokio's work-stealing moving requests between threads and its workers
spinning before they park, though that was not profiled. It wins throughput (109.7k vs
95.1k rps) at the cost of CPU efficiency, which matters for a worker process sharing a box,
not for a request-serving API.

**Django stops scaling at one worker, Rust does not.** Django's throughput is flat from
concurrency 8 to 64 (1,593 → 1,503 rps) while p50 latency grows 4.8 ms → 37 ms: extra
connections only queue. `plane-api` goes 109.7k → 141.2k rps (until the client ceiling) with
p50 0.07 ms → 0.45 ms. At concurrency 64, Django's p95 (76.5 ms) is 144× the Rust one
(0.53 ms); at concurrency 8 it is 53×.

**The static mix is not free for either backend.** Adding the 404 and the CORS preflight
costs Rust 13% of throughput and 46% more CPU per request at concurrency 8 (109.7k →
96.0k rps, 21.4 → 31.1 µs); Django loses 14% (1,593 → 1,367 rps) and 15% more CPU per
request. Both spend the difference on the same work: resolving a missing route and
producing Django's 404 body, and the preflight's CORS headers. The lesson for Phase 3 is
that a ported route's cost will be dominated by its own serialisation and database work,
not by the middleware floor these two tables measure.

**Noise.** The six invocations ran at 1-minute load averages between 4.3 and 9.0 (recorded
per report). The third `static-responses` Rust repetition at concurrency 64 dropped to
101,737 rps from 120,430/119,191 while the load average jumped from 4.3 to 9.0; the median
(119,191) is the number in the table. One Django repetition at concurrency 8 also stalled
(warm-up 424 rps, p99 37 ms) against 1,399/1,367 in the other two; its median (1,367) is
used. These are exactly the disturbances the 3-repetition median is for, and exactly why
the load average is recorded with every run.

### Phase 3 — Linux containers, preliminary (2026-11-16, `plane-api` built from `183e61c1b7`)

> **Code measured:** the `plane-api` image was built at 08:48 UTC from `rust-rewrite` at
> `183e61c1b7` (wave 2 merged). That is *before* the `plane-query` batch and *before* the
> `prepare_cached` preamble fix (`9c91dc3113`, 4 → 2 transactions per authenticated
> request), so Rust's numbers here are pessimistic. Measurements ran 08:50–09:17 UTC and
> 09:54–10:12 UTC (`reads-cycles` and `reads-mixed` re-measured after the broker incident)
> on a shared dev Postgres that other agents were using. The Phase 3 row is re-measured
> on a quiet stack after the dynamic-SQL work is merged; until then, treat these as lower
> bounds for Rust.

Phase 3's exit check adds "a first Linux container measurement" of the
authenticated read paths. This section is that run: five read scenarios at
concurrency 1, 8 and 32, each backend in its own container with the same CPU
and memory quota and its own clone of the `plane_fix_tpl` fixture database.

#### Environment

| | |
| --- | --- |
| Host / client | Apple MacBook Pro, Apple M3 Max (14-core), macOS 26.6.2, 36 GiB |
| Docker VM | Colima 28.4.0, Linux `6.8.0-64-generic`, **6 vCPU / 9.7 GiB** (temporarily resized from the dev baseline of 2 vCPU / 1.9 GiB for this run), overlayfs, cgroup v2 |
| Bench containers | `--cpus 2`, `--memory 512m --memory-swap 512m` each; own Postgres clone, Redis DB (8 / 9) and RabbitMQ vhost |
| `plane-rs-bench-api` | image **69.7 MB** on disk (54.4 MB of layers; `docker image inspect` size 15,263,793 B); `rust:1.99.1-slim-bookworm` builder → `gcr.io/distroless/cc-debian12:nonroot`; rustc 1.99.1; release profile (fat LTO, 1 CGU, panic=abort, strip); **2 tokio worker threads** (`PLANE_RS_WORKER_THREADS=2`, one per allowed core) |
| `plane-rs-bench-django` | image **497 MB** on disk (390 MB of layers; `docker image inspect` size 106,967,048 B); `apps/api/Dockerfile.api` unchanged (`python:3.12.12-alpine`); Django 5.2.17, DRF 3.18.3, gunicorn 23.0.0, uvicorn 0.29.0, `GUNICORN_WORKERS=1` |
| Django command | the entrypoint's final gunicorn line, including `--max-requests 1200 --max-requests-jitter 1000`; `collectstatic` runs first (WhiteNoise needs `STATIC_ROOT`), `wait_for_db`/registration/`create_bucket`/`clear_cache` are skipped; access log to `/dev/null` (formatting and write still paid) |
| Postgres | 15.7-alpine, 2 GiB limit, dev settings (`fsync=off`) |
| Fixture | `plane_fix_tpl`: 401 issues, 7 projects, 17 users, 15 signed sessions; the admin session cookie is sent literally |

#### Method

* **Routes.** Only routes owned by `rust(GET)` in `ROUTES.md`. `scenarios/bench/reads-users-me.yaml`
  (`/api/users/me/`, `/settings/`, `/workspaces/`), `reads-project.yaml` (project
  list/detail, states, labels, members), `reads-issues.yaml`
  (`v2/issues/`, `issues-detail/`, issue detail, history, comments),
  `reads-cycles.yaml` (cycle and module list/detail) and `reads-mixed.yaml`
  (a 37-slot weighted blend of the four, the "one page load" row).
* **Verification first.** `difftest run` in attach mode compared every step of
  every scenario between the two containers — status, headers, decoded body,
  database diff, Redis diff and drained Celery messages. **All five scenarios
  were identical** (exit 0, 0 mismatches) before any measurement.
* **Load.** `difftest bench` closed loop, one keep-alive HTTP/1.1 connection per
  concurrent worker, 5 s warm-up, 20 s measured window, **3 repetitions**,
  median; `--settle 10` before each repetition. Attach mode does not restart
  the container between repetitions, so repetitions 2 and 3 see a warm process.
* **Client.** On the host, across the VM's published-port boundary. The same
  loop against a trivial local server measured 37k–41k rps at concurrency 1,
  115k–125k at 8 and 129k–151k at 32; the fastest Rust row (4,824 rps) is ~3%
  of that ceiling, so the client is not the limit.
* **Memory / CPU.** `ps` on the host cannot see the VM's processes, so a
  sidecar (`plane-rs-bench-stats`) samples cgroup v2 `memory.current`,
  `memory.stat` and `cpu.stat` 4 times per second (250 ms). `idle` in the table
  is the median of the 3 s before the first warm-up, i.e. after the bench's 10 s
  settle and with the process warm from earlier scenarios; `peak` is the highest
  `memory.current - inactive_file` inside the measured window (the same
  cache-excluded subtraction `docker stats` shows; raw `memory.current` is in
  the JSON); `CPU s` is the `usage_usec` delta across the window (±0.25 s).
* **Errors** are the worst repetition; `reconnects` counts a keep-alive
  connection the server closed and the client reopened and retried once.
* **Provenance.** `reads-users-me`, `reads-project` and `reads-issues` were
  measured 08:50–09:17 UTC. The shared RabbitMQ was recreated by another agent
  at 09:26 UTC (every vhost lost), which invalidated the first
  `reads-cycles`/`reads-mixed` rows (32–522 publish errors); both scenarios were
  re-measured 09:54–10:12 UTC and the table uses those rows. **Every row below
  has 0 errors.**

#### Measurements

`date` is the report date; `commit` is the branch head the binary was built
near (the working tree also contained other agents' in-progress changes).
Medians of three repetitions. `idle`/`peak` are cache-excluded container
memory; `CPU s` is the whole container over the 20 s window (20.00 s = one
saturated core).

| date | commit | scenario | target | conc | rps | p50 ms | p95 ms | p99 ms | err | idle | peak | CPU s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-11-16 | eddfe5d425 | reads-users-me | django | 1 | 60 | 16.03 | 21.60 | 26.69 | 0 | 155.5 MiB | 159.3 MiB | 12.36 |
| 2026-11-16 | eddfe5d425 | reads-users-me | django | 8 | 112 | 63.71 | 103.94 | 137.85 | 0 | 155.5 MiB | 158.4 MiB | 29.22 |
| 2026-11-16 | eddfe5d425 | reads-users-me | django | 32 | 94 | 283.39 | 461.57 | 1965.06 | 0 | 155.5 MiB | 168.7 MiB | 28.76 |
| 2026-11-16 | eddfe5d425 | reads-users-me | rust | 1 | 686 | 1.24 | 2.19 | 3.88 | 0 | 78.5 MiB | 80.7 MiB | 3.75 |
| 2026-11-16 | eddfe5d425 | reads-users-me | rust | 8 | 3339 | 2.16 | 4.46 | 6.05 | 0 | 78.5 MiB | 80.9 MiB | 12.07 |
| 2026-11-16 | eddfe5d425 | reads-users-me | rust | 32 | 4824 | 6.05 | 11.21 | 14.54 | 0 | 78.5 MiB | 82.1 MiB | 14.83 |
| 2026-11-16 | eddfe5d425 | reads-project | django | 1 | 40 | 18.99 | 42.94 | 46.98 | 0 | 162.8 MiB | 168.0 MiB | 10.15 |
| 2026-11-16 | eddfe5d425 | reads-project | django | 8 | 73 | 79.30 | 193.53 | 248.57 | 0 | 162.8 MiB | 165.0 MiB | 26.47 |
| 2026-11-16 | eddfe5d425 | reads-project | django | 32 | 70 | 373.76 | 686.59 | 2363.39 | 0 | 162.8 MiB | 173.5 MiB | 26.55 |
| 2026-11-16 | eddfe5d425 | reads-project | rust | 1 | 399 | 2.44 | 3.72 | 4.25 | 0 | 80.1 MiB | 79.8 MiB | 3.90 |
| 2026-11-16 | eddfe5d425 | reads-project | rust | 8 | 1207 | 3.79 | 7.33 | 9.52 | 0 | 80.1 MiB | 80.1 MiB | 8.23 |
| 2026-11-16 | eddfe5d425 | reads-project | rust | 32 | 932 | 13.01 | 22.25 | 30.30 | 0 | 80.1 MiB | 81.2 MiB | 5.43 |
| 2026-11-16 | eddfe5d425 | reads-issues | django | 1 | 22 | 39.65 | 70.78 | 119.42 | 0 | 168.0 MiB | 179.3 MiB | 12.75 |
| 2026-11-16 | eddfe5d425 | reads-issues | django | 8 | 29 | 245.25 | 385.28 | 2043.90 | 0 | 168.0 MiB | 180.8 MiB | 22.43 |
| 2026-11-16 | eddfe5d425 | reads-issues | django | 32 | 28 | 1104.89 | 1613.82 | 1715.20 | 0 | 168.0 MiB | 186.8 MiB | 21.82 |
| 2026-11-16 | eddfe5d425 | reads-issues | rust | 1 | 185 | 3.55 | 10.76 | 12.65 | 0 | 79.8 MiB | 81.0 MiB | 4.01 |
| 2026-11-16 | eddfe5d425 | reads-issues | rust | 8 | 486 | 7.05 | 20.59 | 26.57 | 0 | 79.8 MiB | 85.0 MiB | 8.33 |
| 2026-11-16 | eddfe5d425 | reads-issues | rust | 32 | 769 | 28.45 | 56.38 | 68.93 | 0 | 79.8 MiB | 88.4 MiB | 12.80 |
| 2026-11-16 | eddfe5d425 | reads-cycles | django | 1 | 22 | 38.81 | 106.50 | 113.02 | 0 | 150.6 MiB | 163.9 MiB | 10.78 |
| 2026-11-16 | eddfe5d425 | reads-cycles | django | 8 | 35 | 156.93 | 457.21 | 1549.31 | 0 | 150.6 MiB | 165.1 MiB | 23.22 |
| 2026-11-16 | eddfe5d425 | reads-cycles | django | 32 | 36 | 737.79 | 1586.17 | 1865.73 | 0 | 150.6 MiB | 167.9 MiB | 22.92 |
| 2026-11-16 | eddfe5d425 | reads-cycles | rust | 1 | 360 | 2.46 | 5.01 | 6.42 | 0 | 80.5 MiB | 81.1 MiB | 3.88 |
| 2026-11-16 | eddfe5d425 | reads-cycles | rust | 8 | 756 | 3.77 | 9.34 | 13.01 | 0 | 80.5 MiB | 81.1 MiB | 5.99 |
| 2026-11-16 | eddfe5d425 | reads-cycles | rust | 32 | 1039 | 13.70 | 27.15 | 38.37 | 0 | 80.5 MiB | 82.2 MiB | 6.74 |
| 2026-11-16 | eddfe5d425 | reads-mixed | django | 1 | 27 | 35.42 | 101.31 | 122.43 | 0 | 163.0 MiB | 166.2 MiB | 11.72 |
| 2026-11-16 | eddfe5d425 | reads-mixed | django | 8 | 43 | 144.00 | 386.05 | 553.98 | 0 | 163.0 MiB | 170.1 MiB | 23.99 |
| 2026-11-16 | eddfe5d425 | reads-mixed | django | 32 | 41 | 649.22 | 1278.97 | 2650.11 | 0 | 163.0 MiB | 181.8 MiB | 23.75 |
| 2026-11-16 | eddfe5d425 | reads-mixed | rust | 1 | 294 | 2.48 | 9.65 | 11.38 | 0 | 81.8 MiB | 86.0 MiB | 4.08 |
| 2026-11-16 | eddfe5d425 | reads-mixed | rust | 8 | 807 | 4.70 | 17.68 | 25.93 | 0 | 81.8 MiB | 86.5 MiB | 8.93 |
| 2026-11-16 | eddfe5d425 | reads-mixed | rust | 32 | 1000 | 17.63 | 46.69 | 69.25 | 0 | 81.8 MiB | 88.0 MiB | 10.27 |

#### Container idle memory

Sampled 30 s after both containers started, and 30 s after the last measured
window of the full five-scenario matrix. Cache-excluded (`memory.current -
inactive_file`) and raw `memory.current`.

| | fresh start | | after full run | |
| --- | --- | --- | --- | --- |
| | cache-excl. | raw | cache-excl. | raw |
| django | 143.8 MiB | 148.8 MiB | 157.2 MiB | 170.8 MiB |
| rust | 48.2 MiB | 48.2 MiB | 176.1 MiB | 176.1 MiB |

The Rust figure after the full run is **not** a leak in the usual sense: it
grew from 48.2 MiB to 176.1 MiB over five scenarios and 90 measured windows
(and to 93.1 MiB after the two-scenario re-measurement), with `file` at 0 — it
is retained heap (mimalloc arenas, per-connection buffers and the 16-connection
DB pool). The brief's "under 60 MiB idle" holds for a freshly started
`plane-api`, not for one that has served the matrix's roughly one million
requests.

#### Summary ratios

| scenario | conc | rps django | rps rust | rps ratio | p95 django | p95 rust | p95 ratio |
| --- | --- | --- | --- | --- | --- | --- | --- |
| reads-users-me | 1 | 60 | 686 | 11.5x | 21.60 | 2.19 | 9.8x |
| reads-users-me | 8 | 112 | 3339 | 29.9x | 103.94 | 4.46 | 23.3x |
| reads-users-me | 32 | 94 | 4824 | 51.3x | 461.57 | 11.21 | 41.2x |
| reads-project | 1 | 40 | 399 | 9.9x | 42.94 | 3.72 | 11.6x |
| reads-project | 8 | 73 | 1207 | 16.6x | 193.53 | 7.33 | 26.4x |
| reads-project | 32 | 70 | 932 | 13.2x | 686.59 | 22.25 | 30.9x |
| reads-issues | 1 | 22 | 185 | 8.2x | 70.78 | 10.76 | 6.6x |
| reads-issues | 8 | 29 | 486 | 16.9x | 385.28 | 20.59 | 18.7x |
| reads-issues | 32 | 28 | 769 | 27.3x | 1613.82 | 56.38 | 28.6x |
| reads-cycles | 1 | 22 | 360 | 16.7x | 106.50 | 5.01 | 21.3x |
| reads-cycles | 8 | 35 | 756 | 21.3x | 457.21 | 9.34 | 48.9x |
| reads-cycles | 32 | 36 | 1039 | 29.0x | 1586.17 | 27.15 | 58.4x |
| reads-mixed | 1 | 27 | 294 | 10.8x | 101.31 | 9.65 | 10.5x |
| reads-mixed | 8 | 43 | 807 | 18.9x | 386.05 | 17.68 | 21.8x |
| reads-mixed | 32 | 41 | 1000 | 24.3x | 1278.97 | 46.69 | 27.4x |

#### What the numbers say

**Throughput.** `plane-api` is **8.2×–51.3×** faster on these routes; the ratio
grows with concurrency because Django is saturated at one worker while Rust
keeps scaling. The absolute figures are modest for both — 22–120 rps for Django,
185–4,824 rps for Rust — because every request authenticates a session, does
two or more SQL round trips and serialises a full DRF-shaped body.

**Django is flat above concurrency 8.** `reads-issues` goes 22 → 29 → 28 rps,
`reads-project` 40 → 73 → 70, `reads-cycles` 22 → 35 → 36, `reads-mixed` 27 → 43
→ 41, while p50 grows 6–15×. One gunicorn worker spends 1.1–1.5 CPU-seconds per
second (the rest of the 2-core quota is idle), so extra connections only queue.

**Rust keeps scaling.** `reads-users-me` 686 → 3,339 → 4,824 rps (7.0× from
concurrency 1 to 32), `reads-issues` 185 → 769 (4.2×), `reads-cycles` 360 →
1,039 (2.9×), `reads-mixed` 294 → 1,000 (3.4×). `reads-project` is the exception:
1,207 rps at concurrency 8 but 932 at 32, the only Rust row that gets *slower*.
That scenario publishes `recent_visited_task` on `projects/{pk}/`; at that mix
(project detail is 3 of 11 slots) 1,200 rps is roughly 330 AMQP publishes per
second, the likely limiter. It was not profiled.

**CPU per request** (concurrency 1, whole container):

| scenario | django | rust | ratio |
| --- | --- | --- | --- |
| reads-users-me | 10.3 ms | 0.27 ms | 38× |
| reads-project | 12.7 ms | 0.49 ms | 26× |
| reads-issues | 29.0 ms | 1.08 ms | 27× |
| reads-cycles | 24.5 ms | 0.54 ms | 45× |
| reads-mixed | 21.7 ms | 0.69 ms | 31× |

**Memory.** A fresh `plane-api` idles at **48.2 MiB** versus Django's
**143.8 MiB** (3.0×), and peak under load is 80–88 MiB versus 158–187 MiB
(~2×). Under load the Rust container's memory is almost all `anon` (no file
cache) and its peak grows slowly with concurrency (80.7 → 82.1 MiB across
concurrency 1/8/32 in `reads-users-me`). Django's peak is bounded by the 512 MB
limit and by `--max-requests`, which restarts the worker before memory can creep.

**Worker recycling is visible.** Django reconnects 8 connections per 20 s window
at concurrency 8 and 32–64 at concurrency 32, and p99 reaches 1.5–2.6 s: that is
`--max-requests 1200` firing once per window at ~100 rps. The numbers are the
production line's ("as deployed"), not a sustained-load upper bound; Phase 1
measured the same difference with and without the flag.

**The page-load mix is not the sum of its parts.** At concurrency 32 Rust serves
`reads-mixed` at 1,000 rps, 4.8× slower than `users-me` alone but 1.3× faster
than `issues` alone; the weighted mix is dominated by the issue list, the issue
detail and their two sub-resources.

#### Reproduce

```bash
# Everything: build both images, provision, verify, measure, clean up (~50 min).
apps/server/tools/bench/run-phase3.sh

# Reuse the images and the harness binary; rerun two scenarios only.
apps/server/tools/bench/run-phase3.sh --no-build \
  --scenarios reads-cycles,reads-mixed

# Leave the containers and databases up for inspection.
apps/server/tools/bench/run-phase3.sh --keep

# Remove what a run left behind.
apps/server/tools/bench/run-phase3.sh --down
```

The script checks the host's free disk (20 GiB) and the VM's free memory before
building, records `pg_stat_activity` per database every 5 s into
`target/bench/<tag>/pg-load.log`, and writes one JSON report plus one row per
(scenario, target) to `target/bench/<tag>/`. `tools/bench/phase3.py table --rows
<tag>` regenerates the table and ratios above from those rows.

#### Threats to validity (Phase 3)

* **Shared machine.** The 14-core host ran at 1-minute load averages of
  **4.2–9.8** during these runs. Both the client and the VM compete with other
  agents' builds.
* **Shared Postgres.** Another agent's `dt_*` differential runs and
  `t_w2perf_*` connections were present for most 5 s samples in both runs
  (`pg-load.log`); the shared database was **not quiet**. Every row has 0
  errors, but a difference below ~10% should be re-measured on a quiet database.
* **One broker incident.** The shared RabbitMQ was recreated at 09:26 UTC and
  every vhost was lost. The first `reads-cycles` and `reads-mixed` rows were
  discarded (the Rust cycles row carried 32–522 publish errors); the table uses
  the re-measurement. No row with errors is quoted.
* **Client across the VM boundary.** Requests go over Colima's published port,
  not loopback inside the VM. At concurrency 1 a Rust request takes ~1.2 ms, of
  which a large part is the host↔VM round trip; the absolute latencies are not
  comparable to Phase 1's macOS rows. The client's own ceiling (37k–151k rps)
  is one to two orders of magnitude above every target rps here.
* **Django's `--max-requests`.** Kept because the task is "as deployed"; it caps
  each 20 s window with one worker restart (reconnects, p99 2 s). Without it
  Django's throughput would be higher and its p99 lower.
* **250 ms sampling.** Peak memory can miss a shorter spike, and the CPU window
  edges are ±0.25 s (≈1% of a 20 s window).
* **Container memory is cache-excluded.** The table subtracts
  `inactive_file` (as `docker stats` does). Raw `memory.current` is 5–14 MiB
  higher for Django and identical for Rust; page cache from the image is not
  charged to either.
* **The VM was resized for this run** (6 vCPU / 9.7 GiB instead of the 2 vCPU /
  1.9 GiB dev baseline). Containers are pinned to 2 CPUs each and Rust to 2
  tokio threads, so the internal comparison is unaffected, but the *absolute*
  numbers are not the 2-vCPU deployment's.
* **VM clock drift.** After the resize the VM's clock was ~14 minutes ahead of
  the host; the sampler is therefore host-stamped (`tools/bench/stamp.py`).
  Responses were verified identical on both targets regardless.
* **Two measurement windows.** `users-me`/`project`/`issues` were taken
  08:50–09:17 UTC and `cycles`/`mixed` 09:54–10:12 UTC, each against its own
  fresh database clone; per-row idle reflects the process's warm state at that
  point, not a single run-wide baseline.

### Phase 4 baseline: Django's Celery worker and beat

`plane-worker` and `plane-beat` must beat these numbers, so they were measured before any
of that work starts. The processes are started exactly as
`apps/api/bin/docker-entrypoint-worker.sh` and `.../docker-entrypoint-beat.sh` start them
(the one-off `wait_for_db` / `wait_for_migrations` steps aside), against a fresh
`plane_tpl` clone with their own RabbitMQ vhost and Redis index, on the same host. Each was
sampled every 250 ms after a 15 s settle; the table shows the median summed RSS of the
whole process tree. Both were stopped and everything they created was dropped afterwards.

| date | commit | service | Celery concurrency | processes | idle RSS (median) | min | max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-11-15 | 13cbdbeb4e | `celery -A plane worker -l info` | 14 (Celery's default on this 14-core host; the entrypoint passes no `-c`) | 16 | 2499.7 MiB | 2499.7 MiB | 2499.7 MiB |
| 2026-11-15 | 13cbdbeb4e | `celery -A plane worker -l info -c 4` | 4 (the deployed concurrency in `RUST_REWRITE.md`) | 6 | 852.2 MiB | 852.2 MiB | 852.2 MiB |
| 2026-11-15 | 13cbdbeb4e | `celery -A plane beat -l info` | n/a | 1 | 171.8 MiB | 171.2 MiB | 172.4 MiB |

The beat row is the median of two independent runs (171.2 and 172.4 MiB). The worker's
16 processes are the Celery main process, 14 pool workers and the multiprocessing resource
tracker; at `-c 4` it is main + 4 workers + tracker.

What this means for the Rust targets:

* `plane-worker` has to beat **852 MiB at concurrency 4**; the brief's 60 MiB target is
  14.2× lower. The brief's deployed Linux figure was ~1010 MiB, so this host is in the same
  order.
* `plane-beat` has to beat **~172 MiB** (8.6× less than the 20 MiB target allows).
* These are **macOS numbers with billiard's `spawn` start method**: every pool child is a
  fresh Python interpreter that re-imports Django and shares no copy-on-write pages with
  its parent. A Linux prefork worker shares most pages and should report a lower RSS. The
  comparison must be redone in the Linux containers before the worker is declared within
  budget; only the *ratio* to the 60/20 MiB targets is meaningful here.

Operational note: Celery 5.5.3 on this stack never prints its `celery@host ready.` line
(the consumer is up — `celery -A plane inspect ping` answered `pong` — but the log line is
absent), so `tools/bench/celery-baseline.sh` treats the `mingle: all alone` line as the
readiness signal. That is a harness detail, not a behaviour the Rust port has to reproduce.

## How to reproduce

From `apps/server` (the paths in `scenarios/targets.yaml` are repository-relative):

```bash
# 1. Build the target and the harness in release mode.
cargo build --release -p plane-api
cargo build --release --manifest-path crates/plane-testkit/Cargo.toml --bin difftest

# 2. Start the dev stack (Postgres/Valkey/RabbitMQ/MinIO) and make sure the template
#    database exists; `dev/status.sh` prints the state.
dev/status.sh

# 3. One measurement: one scenario, one target configuration, three concurrencies,
#    three repetitions. Writes a JSON report and a Markdown table.
./target/release/difftest bench \
  --scenario scenarios/bench/health.yaml \
  --target rust --release \
  --concurrency 1,8,64 --duration 20 --warmup 5 --settle 10 --repetitions 3 \
  --headroom-duration 20 \
  --report target/bench/health-rust.json \
  --markdown target/bench/health-rust.md

# Equal-CPU comparison, and Django exactly like production (one worker):
./target/release/difftest bench --scenario scenarios/bench/health.yaml \
  --target rust --release --rust-threads 1 \
  --concurrency 1,8,64 --duration 20 --warmup 5 --settle 10 --repetitions 3 \
  --headroom-duration 0 --report target/bench/health-rust-1t.json
./target/release/difftest bench --scenario scenarios/bench/health.yaml \
  --target django --release --django-workers 1 \
  --concurrency 1,8,64 --duration 20 --warmup 5 --settle 10 --repetitions 3 \
  --headroom-duration 0 --report target/bench/health-django.json

# 4. The whole Phase 1 matrix (six invocations, one at a time; ~30 minutes), then the
#    rows for this file from the JSON reports:
./tools/bench/run-phase1.sh
python3 tools/bench/summarize.py target/bench/phase1-*/*.json

# 5. The Celery idle-memory baseline (starts and stops its own worker/beat):
./tools/bench/celery-baseline.sh                    # Celery's default concurrency
./tools/bench/celery-baseline.sh --concurrency 4    # RUST_REWRITE.md's deployment
```

Attach mode, for a target that is already running. Its database name must be passed; the
harness does not clone, flush or drop it. (Setup steps still send whatever requests the
scenario declares, so an attach-mode scenario with a setup phase writes to the target.)

```bash
PLANE_DB=t_bench PLANE_REDIS_DB=3 PLANE_MQ_VHOST=t_bench dev/django-serve.sh 18001 &
./target/release/difftest bench --scenario scenarios/bench/health.yaml \
  --target django --url http://127.0.0.1:18001 --db t_bench --pgid <pgid of gunicorn> \
  --concurrency 1,8 --duration 10 --warmup 2 --settle 5 --repetitions 1
```

`--pgid` is optional; without it an attached target gets no RSS/CPU measurement.

The harness's own checks, from `apps/server`:

```bash
cargo fmt --manifest-path crates/plane-testkit/Cargo.toml --all -- --check
cargo clippy --manifest-path crates/plane-testkit/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/plane-testkit/Cargo.toml
```

## Threats to validity

* **Shared machine, not quiet.** These runs happened while other agents built and tested
  the same repository; the six Phase 1 invocations recorded 1-minute load averages of
  4.3–9.0 (a debug-harness smoke run earlier overlapped builds at 16–26) on a 14-core host.
  Medians of three runs reduce but do not remove this, and one repetition of
  `static-responses`/rust/64 and one of `static-responses`/django/8 were visibly disturbed.
  Rerun on a quiet machine before treating a 10% difference as real.
* **Loopback, no proxy, no TLS.** Requests go straight to the target. Production puts Caddy
  in front and the frontends are remote; expect different absolute latency and lower
  throughput there. The comparison between targets is still meaningful.
* **Sub-millisecond latencies are scheduling-bound.** At p50 ≈ 0.03 ms the number is
  dominated by loopback and the scheduler. Use p95/p99 and throughput for the comparison;
  do not read a 20 µs p50 difference as a framework property.
* **The client is a co-resident process.** The headroom table bounds it. At concurrency 64
  the Rust target reached **94%** of the measured client ceiling on `health` (79% on
  `static-responses`), so its 64-connection number is a **lower bound**, not a capacity.
  The concurrency-1 rows are latency-bound for the same reason (one request in flight over
  loopback); concurrency 8 is the most informative level in this set.
* **RSS is not container memory.** See "Memory and CPU"; the sum over-counts shared pages,
  and macOS accounting differs from a Linux cgroup. Re-measure in the containers before
  sizing a deployment.
* **Django's `--max-requests` recycling is off.** See "Targets"; Django is measured under
  sustained load, which is the friendliest case for its throughput.
* **Python has no JIT**, and CPython's cyclic GC runs on allocation thresholds as part of
  every request path equally. Nothing is pinned or tuned beyond the dev stack's settings.
* **`ps` sampling has a 250 ms granularity** (peak RSS may miss a shorter spike) and a
  centisecond CPU clock. Both are noted in the report.
* **One target configuration per invocation, restarted per repetition**: the numbers do not
  include a process that has been running and fragmenting for hours; they are
  freshly-started-process numbers, which is the right unit for "idle RSS" but an optimistic
  one for a long-lived worker.
* **The tree was not committed.** The release binary was built from the working tree at the
  time shown in each section; other agents' in-progress work was present. The commit column
  is the branch head, not a guarantee that the binary matches it.
