# Working conventions for agents on the Rust rewrite

Read this file, `RUST_REWRITE.md` (repository root), `apps/server/DECISIONS.md` and
`apps/server/PROGRESS.md` before doing anything. `RUST_REWRITE.md` is the brief; this file
is how work is done day to day. When they disagree, `RUST_REWRITE.md` wins.

## Hard rules (repeated from the brief because they are the ones that get broken)

1. Never modify anything under `apps/api`, `apps/web`, `apps/admin`, `apps/space`,
   `apps/live` or `packages/`. Django is the specification and the test oracle.
   Generated, gitignored artefacts (`apps/api/.venv`, `apps/api/logs/`) are fine.
2. Never push, never deploy, never connect to any database, Redis, broker or bucket other
   than the dev stack described below. In particular the machine already runs unrelated
   containers on ports 5432, 6379, 9000, 9001, 9092 and a Redis on 6390: never touch them.
3. Do not `git commit`, `git stash`, `git reset`, `git checkout` or otherwise change git
   state. The orchestrator reviews and commits.
4. No stubs that pass. Unfinished handlers return an explicit not-implemented error.
   Never weaken a test to make it pass. If Django behaviour looks like a bug, port it as
   it is and report it.
5. No secrets in tracked files. Dev-only credentials that already appear in
   `.env.example` (`plane`/`plane`, `access-key`/`secret-key`) are acceptable.
   Generated secrets go in gitignored files.
6. Read before porting: view, serializer, permission classes, model methods, tasks
   enqueued. Cite the Python file and line you ported from in a comment on non-obvious
   logic.
7. Look up crate APIs (docs.rs, the crate source in `~/.cargo/registry`) instead of
   relying on memory. Pin the versions listed in `DECISIONS.md`.

## Layout

```
apps/server/
  Cargo.toml        workspace
  crates/           library crates (logic lives here)
  bins/             thin binaries
  dev/              local dev stack: compose file and scripts
  tools/            Python/shell helpers (inventory generators, fixture builders)
  scenarios/        differential-test scenarios (recorded request sessions)
  ROUTES.md  MODEL_HOOKS.md  TASKS.md  ENV.md  HTTP_CONTRACT.md  BENCH.md  PROGRESS.md
```

## Dev stack

Everything binds to 127.0.0.1 on these ports only. Compose project name: `plane-rs-dev`.

| Service | Image | Host port |
| --- | --- | --- |
| Postgres | `postgres:15.7-alpine` | 55432 |
| Valkey | `valkey/valkey:7.2.11-alpine` | 56379 |
| RabbitMQ | `rabbitmq:3.13.6-management-alpine` | 55672 (AMQP), 55673 (management) |
| MinIO (silo) | `docker.io/pgsty/silo:RELEASE.2026-09-16T00-00-00Z` | 59000 (S3), 59090 (console) |
| Django instances (native, via uv) | | 18000-18099 |
| Rust `plane-api` instances | | 18100-18199 |
| Dev proxy (Caddy) | `caddy` | 18080 |

The Docker VM has 2 CPUs and about 2 GB of memory (temporarily 6 CPUs / 10 GB during
Phase 3; see `PROGRESS.md`). Keep containers lean and never start
the full `docker-compose-local.yml` stack.

Python for Django runs natively on the host from the already-synced virtualenv. Call the
interpreter directly so concurrent agents never trigger a uv sync:

```
cd apps/api && .venv/bin/python manage.py ...
```

If the venv ever needs re-syncing (only the orchestrator does this): `apps/api` pins uv
0.12.21 and the host uv is newer, so use `uvx --from 'uv==0.12.21' uv sync --locked`
inside `apps/api`. The dev scripts in `apps/server/dev/` wrap all of this; prefer them
once they exist.

Isolation between concurrent agents: create your own databases with a unique prefix
(for example `t_<task>_<n>`), cloned from the template database, use your own Redis DB
index or key prefix, and your own RabbitMQ vhost. Drop what you create when done.
Drop only databases you created yourself, by exact name. Never drop databases by
pattern (`dt_%`, `t_%`, ...): the `dt_*` clones belong to other agents' in-flight harness
runs, and the harness removes its own. A bulk drop once aborted several agents' runs.

## Rust conventions

- Edition 2024. Toolchain is the Homebrew `cargo`/`rustc` 1.99 (no rustup).
- Every `.rs`, `.py`, `.sh`, `.ts` file added starts with the licence header from
  `COPYRIGHT.txt`, in the file's comment syntax:
  ```
  // Copyright (c) 2023-present Plane Software, Inc. and contributors
  // SPDX-License-Identifier: AGPL-3.0-only
  // See the LICENSE file for details.
  ```
- Must pass, from `apps/server/`: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- `crates/plane-testkit` is a separate Cargo workspace (feature isolation, see
  DECISIONS.md). The root commands above do not reach it: if you touch it, also run
  `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`
  inside `apps/server/crates/plane-testkit/`.
- `unsafe` is forbidden outside the global-allocator declaration.
- No `unwrap()`/`expect()` on request paths; errors map to the DRF-compatible error type
  in `plane-core`.
- Performance is a goal, not an afterthought: avoid per-request allocation where a borrow
  works, prefer `prepare_cached` statements, never do N+1 queries where Django does a
  single query or a prefetch, run CPU-heavy work (password hashing, xlsx, image work) on
  `spawn_blocking`.

## Reporting back

End your work with a short report:

1. What you did, as a list of files created or changed.
2. The exact commands you ran to verify it, and their result (pass/fail, counts).
3. Anything you could not finish, and why.
4. Open questions or suspected Django bugs, with file:line references.

Do not claim something works unless you ran it.
