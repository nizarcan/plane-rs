# Progress ledger

Read at the start of every session; update at the end.

## Current phase

Phase 0: Inventory.

## Upstream reference

The port tracks `rust-rewrite`, based on upstream `preview` at `290383d7f9`
(fix(web): keep the mobile sidebar toggle reachable on project pages, #10274).

## Done

- Session 1: brief read; environment checked (Rust 1.99.1, Docker with 2 CPUs and 2 GB,
  uv 0.12.21 through uvx, Python 3.12). `apps/api` dependencies synced into `apps/api/.venv`.
  Conventions (`AGENTS.md`) and dependency decisions (`DECISIONS.md`) written.

## In flight

- Phase 0 tasks: dev stack, routes inventory, model hooks, tasks table, env-var table,
  HTTP contract notes, differential harness.

## Open questions

(none yet)

## Suspected upstream bugs, ported as they are

(none yet)

## Next three steps

1. Finish the Phase 0 inventories.
2. Build the differential harness and replay a recorded session against Django twice.
3. Start Phase 1 (skeleton).
