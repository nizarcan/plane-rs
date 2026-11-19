# Playbook: rewriting a production backend with a team of agents

This playbook describes how to replace a working backend with a reimplementation in another
language or stack while keeping it **behaviourally identical**: same database, same HTTP
contract, same side effects. It was written after rewriting Plane's Django backend in Rust
(this repository) and is meant to be handed to an agent, together with a different
repository, to evaluate whether the same can be done there and then to do it.

It is technology-neutral. "Old system" means the code being replaced (here Django/Celery);
"new system" means the replacement (here Rust). The worked example appears in boxes marked
**In plane-rs**, with the numbers and incidents that shaped each rule.

---

## 0. The shape of the method in one page

1. **Evaluate before committing** (§1). Measure why a rewrite is worth it, size the surface,
   find the contracts, and confirm there is a runnable oracle. Stop if there isn't.
2. **Write a brief** (§2): goal, scope, rules that hold at every commit, target
   architecture, phases with exit checks, and the decisions reserved for the owner.
3. **Inventory mechanically** (§3). Generate the route table, model hooks, task table and
   environment variables from the old system's own introspection, never by reading code.
4. **Build the oracle harness first** (§4). Before porting anything, a differential harness
   that replays the same requests against two copies of the old system must show a clean
   diff. Then it compares old against new.
5. **Strangle, don't switch** (§5). Both systems run against the same database; a proxy
   routes each path to its owner; ownership moves one group at a time, reversibly.
6. **Run the work as an orchestrated team** (§6). One orchestrator plans, briefs, verifies,
   merges and keeps the ledger; worker agents port in isolated worktrees and never touch
   git state. Nothing counts until the orchestrator re-runs the proof.
7. **Prove every piece several ways** (§7): differential scenarios, the old system's own
   contract tests run against the new one, task-level comparisons, generator-checked
   corpora for anything the old system derives from a library, and benchmarks.
8. **Write decisions down as they are made** (§8), and port the old system's bugs as they
   are, recording each one.
9. **Cut over with a scripted exit check** (§9): a fresh install and an upgrade of an
   existing database, with no old-system process anywhere, walked end to end.
10. **Pin the upstream** (§10) so the fork can be brought forward later by diffing the old
    system between two commits and porting the difference with the same machinery.

---

## 1. Evaluation: is this rewrite feasible and worth it?

Do this before writing a single line of the new system. The output is a go/no-go and the
first draft of the brief.

### 1.1 Measure the reason

Write down the problem in numbers measured on a real deployment, not estimates: idle and
peak memory per service, CPU, latency percentiles, image size, cold start. A rewrite whose
reason is not measurable cannot be declared finished.

> **In plane-rs.** A single-user self-hosted Plane idled at ~3.5 GB; the Python services
> (Celery worker ~1010 Mi, API ~760 Mi, beat ~350 Mi) were ~60% of it. Target: tens of
> megabytes per service. Final result: the Rust API 59 MiB, worker 1.6 MiB, beat 0.9 MiB
> idle; the whole Rust backend 61 MiB against 789 MiB for the same Python stack measured
> the same way.

### 1.2 Size the surface

Count, from the old system's own introspection where possible:

| What | Why it matters |
| --- | --- |
| HTTP routes × methods, per mount point | the unit of porting and of ownership |
| Models, migrations, model hooks (`save()` overrides, signals) | hidden write behaviour |
| Background tasks, their producers, schedules, retry policies | the second, quieter contract |
| Environment variables and derived settings | deployments must keep working unchanged |
| Middleware: sessions, CSRF, CORS, throttles, compression | cross-cutting contract |
| External systems: DB, cache, broker, object storage, mail, outbound HTTP | what the harness must capture |
| Libraries whose *output* is part of the contract (serialisers, HTML parsers, spreadsheet writers, password strength) | these need recorded corpora, not reimplementations from memory |
| Existing tests (unit, contract, smoke) | reusable oracle checks |

> **In plane-rs.** ~123k lines of Python: 391 resolver entries / 708 endpoint×method pairs,
> 96 models with 39 save hooks, 167 migrations, 46 Celery tasks with 166 enqueue sites and
> 13 schedules, 131 environment variables, 1106 tests.

### 1.3 Find the contracts and the oracle

The rewrite is feasible only if:

- **The contract is observable.** Everything a client depends on can be captured: response
  status, headers, body bytes, cookies; database rows written; messages published; files
  stored; mails sent; outbound requests made.
- **There is a runnable oracle.** The old system can be started locally, at a pinned
  version, against disposable copies of a fixture database, many times in parallel. Without
  this, stop: you cannot prove anything.
- **The clients don't change.** In a byte-identical rewrite the frontends, SDKs and
  integrations are the specification. If they must change, it is a different project.

### 1.4 Identify the hard parts up front

List what is easy to get wrong and assign each a deliberate design and a test. Typical
entries: soft delete, model hooks that only run through the ORM, audit columns set by
middleware, serializer quirks (`fields=`/`expand=`, null vs absent, number and datetime
formatting), pagination cursors, query-string filter languages, permission decorators,
rate limits and client-IP rules, encrypted configuration, OAuth state, timezones, the
session and password-hash formats (both systems must accept each other's during
coexistence), and every library whose output bytes are visible.

### 1.5 Decide the scope and the owner's decisions

Write down what is out of scope (here: the browser frontends, the collaboration server, any
commercial edition), and the decisions only the owner can make, each with a default the plan
assumes (here: queue backend, upstream tracking, telemetry, read-replica support).

### 1.6 Go / no-go

Go if: the reason is measured, the oracle runs, the contract is observable, and the hard
parts each have a credible approach. Estimate in route groups and tasks, not lines.

---

## 2. The brief

One file at the repository root (here `RUST_REWRITE.md`), read by every agent before any
work. Sections:

1. **Why**, with the measured numbers.
2. **Scope** and out of scope.
3. **What exists today**, sized (§1.2).
4. **Rules that must not be broken**, holding at every commit. The ones that mattered:
   - same database schema; no schema changes for the rewrite's convenience;
   - same HTTP contract, byte for byte;
   - the old system is the reference and is never modified; when the brief and the old code
     disagree, the old code wins;
   - the old system owns migrations until cutover;
   - side effects count as behaviour;
   - security behaviour is ported, never simplified;
   - no secrets in the repository; agents never push or deploy.
5. **Target architecture**: library crates/packages hold logic, binaries are thin; keep the
   old service boundaries.
6. **How the two systems coexist** (§5): sessions, password hashes and CSRF tokens must be
   interchangeable.
7. **Background jobs**: one owner per task at any time; how the new system publishes to the
   old queue while coexisting.
8. **Things that are easy to get wrong** (§1.4).
9. **Verification** layers (§7).
10. **Phases** with an exit check each. Do not start a phase early.
11. **Working rules** for agents (ledger, small verified commits, no stubs that pass, port
    bugs as they are, look APIs up instead of recalling them).
12. **Open decisions for the owner.**

Phases used here, which generalise to most web backends:

| Phase | Work | Exit check |
| --- | --- | --- |
| 0 Inventory | generated tables; harness against the old system alone | old vs old replays with a clean diff, repeatedly |
| 1 Skeleton | workspace, config with the old env names, logging, health, DB pool, error format, proxy upstream | new service healthy behind the proxy; CI green |
| 2 Auth foundation | sessions, password hashing, CSRF, origin checks, permission guards, rate limits, API keys | a session created by either system works on the other |
| 3 Read paths | every GET, simplest first | differential + contract tests green per GET |
| 4 Job plumbing | new queue, worker, publisher to the old broker, scheduler with locking | a schedule runs exactly once with two scheduler replicas |
| 5 Write paths | POST/PATCH/DELETE by domain, with each domain's tasks | differential incl. DB diffs and side effects |
| 6 Auth flows | sign-in flows, OAuth providers | walked end to end in a browser |
| 7 Remaining surfaces | public API, admin API, API schema | contract tests green, API docs still accurate |
| 8 Cutover | every route and task owned by the new system; old system and its broker removed; new migrator takes over from a baseline | fresh install and upgrade work with no old-system process |

---

## 3. Inventory, generated

Every table in the inventory is produced by a script that asks the old system, with a
`--check` mode that regenerates in memory and fails on drift. Hand-written inventories rot
and hide routes.

- **Route table** (`ROUTES.md`) from the framework's URL resolver: path, methods, view,
  owner (`old`/`new`), verification status. This table drives the proxy configuration (§5),
  so ownership is code, not prose.
- **Model hooks** (`MODEL_HOOKS.md`): every `save()` override, signal and bulk operation that
  bypasses them, the soft-delete cascade, foreign keys whose delete behaviour is emulated.
- **Task table** (`TASKS.md`): every task, its producers with file:line, arguments, retry
  policy, schedules, and the exact wire format of a published message.
- **Environment** (`ENV.md`): every variable, default, derived setting, and encrypted
  configuration scheme, with a test vector.
- **HTTP contract notes** (`HTTP_CONTRACT.md`): middleware order, headers, session and CSRF
  codecs with test vectors, error shapes, throttle storage format.

> **In plane-rs.** Generators under `apps/server/tools/inventory/` (routes, env vars,
> attachment MIME table, lxml HTML tables, bs4 corpus and entities, xlsx corpus) and
> `tools/mail/` (e-mail messages and templates) all have `--check`, and CI runs them.

---

## 4. The differential harness, before any port

The harness is the most important artefact of the project. Build it in Phase 0 and keep
extending it.

### 4.1 What it does

- Clones a **fixture database template** into two disposable databases per run.
- Starts **target A** (old system) and **target B** (new, or old again for the baseline) on
  free ports, each with its own database, cache namespace, broker vhost and bucket.
- Replays a **scenario** (a YAML list of requests with captured variables) against both.
- Compares, per step: status, the headers that matter, the body (JSON parsed with key order
  and number lexemes preserved, and also checked byte-identical to a canonical
  re-serialisation), the **database diff** (rows inserted/updated/deleted in every table),
  cache keys, messages published to the broker, objects stored, mails sent and outbound
  requests made.
- Normalises only what is legitimately nondeterministic, in one explicit, reviewed table
  (generated ids, timestamps, tokens, presigned signatures verified rather than compared).
  **Every normaliser is a place a real difference can hide**: review each one, scope it to
  the route and field, and remove it when the asymmetry that needed it is fixed.

### 4.2 Its own exit check

Old vs old, repeated: clean every time. Then inject a deliberate HTTP difference and a
DB-only difference and confirm the harness reports both. Add a `selftest` command that
exercises the comparison logic without targets.

### 4.3 Extend it to every kind of effect

As work reaches background tasks, add a task mode: run one task on each side from the same
setup SQL and kwargs and diff the whole schema, the published messages (as values, plus raw
wire bytes while the old queue still owns the consumer), cache, object storage, outbound
HTTP (through a local recorder), SMTP (through local fake listeners), and archive contents.

> **In plane-rs.** `plane-testkit` (`difftest run`, `difftest task`, `difftest selftest`).
> By the end: 131 scenario files / 6,345 steps, 292 task cases, 141 selftest checks.

### 4.4 Rules that keep it honest

- **Symmetry.** Both sides must start from the same state. If the new system runs a boot
  step the old one doesn't (or the reverse), the diff is meaningless. Fix the asymmetry; do
  not normalise the symptom.
  > A boot-time `register_instance` on the Rust side only made `latest_version` differ;
  > two timestamp normalisers had already been added that hid half of it.
- **Isolation.** Every concurrent run gets its own DB names, cache DB index, vhost, bucket
  and ports, from ranges assigned by the orchestrator. Fixed ports collide.
  > Two runs on the same fixed SMTP ports delivered mail into each other's listener and
  > produced "django sent 2 messages, rust sent 1".
- **Transport is not content.** When ownership of a task moves, compare the old side's
  broker message with the new side's queue row as values; keep comparing raw bytes for
  anything the old consumer still reads.
- **The harness provisions what production provisions.** If production runs a migrator
  before the service, so does the harness.
  > Without the queue schema in the clones, every route that enqueued a newly-moved task
  > returned 500; an earlier "clean sweep" claim had predated the switch.

---

## 5. Coexistence: the strangler

- A **proxy** in front of both systems routes each path (and method) to its owner. Its
  configuration is **generated from the route table**, validated in CI, and grouped so a
  group can be sent back to the old system by flipping one flag.
- **Sessions, password hashes and CSRF tokens** are byte-compatible in both directions, so
  a user signed in on one system is signed in on the other.
- **Background tasks have exactly one owner at a time.** While the old queue owns a task,
  the new system publishes old-protocol messages to the old broker, byte-identical. A task
  moves only when **every producer** of it is in the new system (keep a producer table and a
  unit test that fails if a moved task still has an old producer). Never publish to both.
- **Schedules** move one entry at a time with a handover that disables the old scheduler's
  entry in a quiet window; prove exactly-once with two scheduler replicas and with a
  handover under a timing test. Run handovers on an idle host.
- A row in the route table is `verified` only with scenario evidence for each method;
  routes stay with the old system until then.

---

## 6. Executing with a team of agents

### 6.1 Roles

- **Orchestrator** (one). Plans, writes briefs, assigns resources, reviews reports,
  commits, merges, resolves or delegates conflicts, re-runs every proof on the merged head,
  flips ownership, keeps the ledger and the decision log, and talks to the owner. Writes
  almost no product code.
- **Workers** (many, in parallel). Each gets one coherent unit (a route group, a domain's
  writes, a task set, a harness feature), in its own **git worktree** on its own branch.
  Workers edit files only: they never commit, merge, reset or push. The orchestrator commits
  their work after review.

### 6.2 The brief for a worker

Every brief, even a continuation, repeats:

1. Worktree path and base commit; symlinks to shared environments; files never to touch.
2. **What to read first**, by path and section (brief, conventions, decisions, ledger,
   the plan for this unit).
3. The unit's scope and the order to do it in.
4. **Exactly which checks to run**, with the agent's own resources (DB prefix, cache DB
   indexes, vhost, bucket, port ranges) and the expected baselines.
5. Rules: no git state changes; never touch the owner's containers/ports; never change
   server-wide settings on shared services; drop only resources you created, by exact name;
   record the PID of every process you start and kill only those; no outbound traffic.
6. A time budget and "report at a coherent stopping point".
7. The report shape: files changed, exact commands with pass/fail counts, proposed decision
   text (workers never edit the decision log), unfinished items with the exact next step,
   suspected bugs in the old system with file:line.

### 6.3 Reviewing a report

Read it adversarially:

- **"Deliberate difference" and "unavoidable" are claims to investigate**, not to accept.
  > A worker documented UUIDs sent as strings instead of the old system's typed envelopes
  > as "deliberate". It was fixed by carrying type hints through the queue. Another
  > claimed response header casing couldn't be preserved; that was investigated down to a
  > private type in the HTTP library before being accepted.
- **"Passes in isolation"** means find the shared resource that made it fail together.
- **A check not run is not a pass.** If the report skips sweeps, the merge waits for them.
- **Upstream behaviour is ported, not fixed** — including reachable bugs. A port that is
  "better" than the old system is a parity failure.
  > The analytics export deliberately reproduces the old system's LEFT JOIN inflation of
  > counts; the page-version task reproduces an old-system task that has never written a row.

### 6.4 Merging

- Commit the worker's branch, then merge into the main line with `--no-ff`.
- **If the conflicts are semantic** (two branches implemented the same mechanism), abort,
  merge the main line into the worker's branch instead, and have that worker resolve them
  into one design. Never keep two mechanisms for one thing.
- List merges (registries, ownership tables) are unions; check mirrored lists stay equal
  with a script.
- After each merge, run the **full verification on the merged head** in a separate
  checkout (§7.3). Only then flip ownership.

### 6.5 Resource discipline on a shared machine

- Disk: a floor (here 20 GB), checked before builds and after merges. Build with debug
  info off; delete incremental caches; remove worktrees after merge.
- Containers: cap log sizes. **Never let an agent change server-wide settings.**
  > `ALTER SYSTEM SET log_statement = 'all'` on the shared Postgres grew its container log
  > to 43.6 GB and nearly hit the disk floor.
- Concurrency: at most N parallel builds; timing-sensitive tests (scheduler handover) run
  only when the machine is quiet.
- Temporary environment changes (VM size, container limits) are recorded in the ledger with
  the exact revert, and reverted at the end.

---

## 7. Verification layers

Nothing counts as done on the strength of reading code.

1. **Differential scenarios** (§4) for every route and method.
2. **The old system's contract tests, run against the new system** through a plugin that
   points the test client at the new server and replays side effects (published messages,
   queued jobs) into the tests' mocks. Keep a baseline (here 984 passed / 2 failed, both
   documented in-process-only limitations) and treat any change as a finding.
3. **Task cases** (`difftest task`) for every background task branch, incl. failure paths.
4. **Corpora with generators** for every library whose output is visible (HTML parsing and
   serialisation, templates, spreadsheets, MIME tables, mail bytes), recorded from the old
   system's pinned environment, with `--check` in CI.
5. **Schema checks**: queries the new system issues are validated against the real schema;
   the new migrator's result is dump-identical (columns, indexes, constraints, sequences)
   to the old migrator's.
6. **Boot and operations**: start-up steps and operator commands compared against the old
   commands on cloned databases.
7. **Gates**: formatter, linter with warnings as errors, unit tests.
8. **Benchmarks** at phase ends: equal container limits, fixed request sets, one load
   level, idle memory of every service measured with live dependencies.
   > The final benchmark found a real bug: the worker's `LISTEN` was never sent because the
   > client library writes only while its connection is polled. Jobs still ran on the poll
   > fallback, so no functional test noticed. Measure with live dependencies, and look at
   > the dependency (here `pg_stat_activity`) to confirm the service is doing what you think.

### 7.1 Iterate fast, verify fully once

Workers run only what they touched while iterating, the full sweeps once at the end if they
changed shared code, and gates once. The orchestrator runs **everything** on every merged
head. Measurements: 50 sequential requests per route, one load level, about 20 minutes.

### 7.2 Flakes

A flake is a finding until explained. Rerun it alone; if it passes, find the shared resource
or timing that broke it and fix the harness. Record the cause.

### 7.3 The orchestrator's verification run

A single scripted command, in a separate checkout of the merged head: gates; schema checks;
every generator `--check`; migrator and boot proofs; every task case; every scenario file
(cache flushed between files); the wrapped flows (OAuth with an intercepting proxy, SMTP);
scheduler tests; the contract suite; and, from Phase 8, the cutover exit check. Its output
goes into the ledger verbatim in summary.

---

## 8. Ledger and decision log

- **Ledger** (`PROGRESS.md`): current phase, the upstream reference, what's done (per phase,
  per merge, with the verification summary), what's in flight (with resources), follow-ups,
  open questions, suspected upstream bugs ported as they are, environment changes to revert,
  next steps. Every session starts by reading it. Every merge and every verification adds an
  entry.
- **Decision log** (`DECISIONS.md`): numbered, dated decisions (D1…Dn) with the reasoning,
  the measurement behind it, and the alternatives rejected. Amend in place with "(amended
  with …)" when later work refines one. Workers propose text; the orchestrator writes it.
  Typical decisions: library choices with measurements; how a nondeterministic old-system
  behaviour is compared (e.g. output from iterating a hash set compared as unordered);
  transport choices; deliberate, documented differences that are not part of the contract.
- **Plans** (`plans/*.md`): one per domain or phase, written by an analysis agent before the
  porting agents start, read-only afterwards.

---

## 9. Cutover

1. Every task has an owner in the new system; the producer table proves it.
2. The new **migrator** owns the schema from a **baseline** that is a dump of a database the
   old migrator produced at the pinned commit, plus its migration-history rows. It installs
   fresh, accepts an upgrade only if the history equals the baseline exactly, refuses older
   or newer or unrecognised databases with a message naming the fix, takes an advisory lock,
   and is idempotent. Prove it with a two-database dump diff.
3. Boot steps the old entrypoint ran are ported and compared; deliberate differences are
   recorded.
4. Deployment files run only new-system images; the old broker and its settings are removed;
   the proxy routes everything to the new system; the release pipeline publishes the new
   images.
5. **Exit check, scripted**: a fresh install and an upgrade from a restored old-system dump,
   each walking the main user flow end to end (here: sign-in → workspace → project → work
   item → comment → upload → export → invite e-mail → webhook delivery → static files), with
   no old-system process in any container, and a log gate that fails on any attempt to reach
   the removed broker or any unhandled error.

---

## 10. Pinning the upstream and moving forward

After cutover the fork no longer follows upstream automatically. Keep it movable:

- Record the **upstream repository, commit, and version** the port corresponds to in a
  machine-readable file (here `UPSTREAM.toml`).
- Keep the **oracle reproducible from that pin**: the harness fetches the old system at the
  pinned commit into a cache rather than keeping a copy in the tree.
- To move forward: diff upstream between the pinned commit and the target, classify the
  changes (routes, serializers, models and migrations, tasks, settings, frontend), port each
  with the same harness against the old system **at the new commit**, extend the migrator's
  baseline, and move the pin only when the full verification passes. See `UPGRADING.md`.

---

## 11. Checklists

### 11.1 Evaluation

- [ ] Reason measured on a real deployment
- [ ] Surface sized from introspection
- [ ] Old system runs locally at a pinned version, many copies in parallel
- [ ] Every client-visible effect capturable (HTTP, DB, cache, broker, storage, mail, outbound)
- [ ] Hard parts listed, each with an approach
- [ ] Scope, out of scope, owner decisions with defaults
- [ ] Go / no-go written down

### 11.2 Before porting anything

- [ ] Brief at the repository root
- [ ] Generated inventory with `--check` in CI
- [ ] Fixture database template rich enough to exercise every route
- [ ] Harness: old vs old clean, repeatedly; injected differences detected; selftest
- [ ] Proxy generated from the route table; per-group rollback
- [ ] Ledger and decision log started

### 11.3 Per unit of work

- [ ] Plan read; old code read (view, serializer, permissions, hooks, tasks)
- [ ] Scenario/task cases written before the handler
- [ ] Every side effect compared, including published tasks
- [ ] Old-system bugs ported and listed with file:line
- [ ] Own resources only; processes killed by PID
- [ ] Report with exact commands and counts

### 11.4 Per merge

- [ ] Branch committed; merged `--no-ff`; semantic conflicts resolved by the owning worker
- [ ] Full verification on the merged head in a separate checkout
- [ ] Ownership flipped only for rows with evidence
- [ ] Ledger and decisions updated; worktree removed; disk checked

### 11.5 Cutover

- [ ] All tasks and schedules owned by the new system
- [ ] Migrator baseline dump-identical; upgrade and refusals proven
- [ ] Deployment, proxy, env templates, release pipeline updated
- [ ] Exit check: fresh + upgrade, no old process, full walk, log gate
- [ ] Final benchmark with live dependencies
- [ ] Upstream pin recorded; upgrade procedure written
- [ ] Temporary environment changes reverted

---

## 12. Timeline of the worked example

| Date (2026) | What happened |
| --- | --- |
| Nov 15 | Brief written; Phase 0 inventory and the harness (old vs old clean); Phase 1 skeleton |
| Nov 16 | Phase 2 auth foundation; Phase 3 reads in waves; Phase 4 job plumbing; Phase 5 writes begin by domain |
| Nov 17 | Phase 5 completes across all domains; Phase 6 auth flows, incl. OAuth through an intercepting proxy |
| Nov 18 | Phase 7 public API, space API, instance admin, OpenAPI; scope extended to Phase 8 |
| Nov 19 | Phase 8: 46 tasks moved in sets, migrator baseline, boot steps, deployment, exit check, final benchmark |

518 commits (142 merges) on top of the pinned upstream commit; 20 library crates and 5
binaries; 37 recorded decisions; 392 of 394 route rows owned by Rust (the remaining two are
health endpoints the proxy never sends to the API); every background task Rust-owned.
