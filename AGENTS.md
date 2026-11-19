# Agent guide for plane-rs

plane-rs is a fork of [Plane](https://github.com/makeplane/plane). The browser
frontends are upstream's; the backend is a Rust reimplementation of the Django
application that upstream ships. The commit this repository corresponds to is
in `UPSTREAM.toml`; `UPGRADING.md` is how it moves.

`apps/server/AGENTS.md` is the detailed guide for the backend (dev stack,
ports, resource discipline, Rust conventions, verification commands). Read it
before touching anything under `apps/server/`. This file is the map.

## What lives where

| Path | What it is | Can you edit it? |
| --- | --- | --- |
| `apps/server/` | The Rust backend: Cargo workspace, crates, binaries, tools, scenarios, decision log, ledger | Yes — that is the work |
| `apps/web`, `apps/space`, `apps/admin`, `apps/live`, `apps/proxy`, `packages/` | Upstream frontends and shared packages at the commit in `UPSTREAM.toml` | **No.** They are taken verbatim; changes belong upstream, then a pin move |
| `apps/api` | Upstream's Django backend | **Not in this repository.** It exists only as *the oracle* (see below) |
| `UPSTREAM.toml` | Repository URL, commit, describe, date, verbatim paths, what replaces `apps/api` | Only through `UPGRADING.md` |
| `UPGRADING.md` | Procedure for moving the pin, including schema upgrade steps | Yes, when the procedure changes |
| `charts/`, `deployments/` | Helm chart and deployment files for the Rust services | Yes |
| `PLAYBOOK.md`, `RUST_REWRITE.md` | The method and the original brief (historical) | Treat as reference |

## The oracle

Every parity proof runs against Django at the pinned commit, not against a copy
in this tree. `apps/server/tools/upstream/oracle.sh` exports `apps/api` from
`UPSTREAM.toml`'s commit into
`${PLANE_RS_ORACLE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/plane-rs/oracle/<commit>}`
and builds its virtualenv from the commit's own `uv.lock`:

```bash
apps/server/tools/upstream/oracle.sh fetch     # idempotent; a stamp file guards it
apps/server/tools/upstream/oracle.sh check     # exit 1 when the cache is stale
apps/server/tools/upstream/oracle.sh api-dir   # the materialised apps/api
apps/server/tools/upstream/oracle.sh python    # its interpreter
```

`tools/upstream/oracle.py` is the same locator for Python tools; the Rust tests
use `crates/plane-oracle`. No tool reads `apps/api` from the checkout, and no
path outside `UPSTREAM.toml` hard-codes the commit. `tools/upstream/diff.py
<ref>` classifies what changed upstream and regenerates the route/task
inventories against the target oracle.

## Commands

Frontends (unchanged from upstream):

```bash
pnpm install
pnpm dev                      # web:3000, admin:3001, space:3002, live:3100
pnpm check                    # format, lint, types
pnpm fix
```

Backend (from `apps/server/`, see `apps/server/AGENTS.md` for the full list):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --manifest-path crates/plane-testkit/Cargo.toml
cargo run --manifest-path crates/plane-testkit/Cargo.toml --bin difftest -- selftest
```

The differential harness needs the dev stack (`apps/server/dev/up.sh`) and the
oracle. The generators under `apps/server/tools/` are run with the oracle's
interpreter; each documents its `--check` and its environment.

## Rules that do not bend

1. **Do not edit the verbatim upstream paths.** A frontend or `packages/` change
   is an upstream change followed by a pin move (`UPGRADING.md`).
2. **Do not add Django back to the tree.** There is no `apps/api` to edit; read
   the oracle, and never make a tool fall back to a path that does not exist.
3. **`UPSTREAM.toml` moves only with a complete port and full verification.**
   Every tool reads the commit from it; a stale pin is a false proof.
4. **Same contract.** API responses, database rows, tasks, schedules, cookies,
   headers, mail and side effects must match the oracle. When in doubt, the
   oracle wins; port reachable bugs and record them.
5. **No secrets in tracked files.** `.env` files are generated; the only test
   material allowed by `.gitleaks.toml` is listed there with a reason.
6. **Never commit, stash, reset or push** as a worker agent; the orchestrator
   does that. Never touch the owner's containers or change server-wide
   settings. Record the PID of every process you start and kill only those.
7. **Licence headers.** New `.rs`, `.py`, `.sh` and `.ts` files start with
   `COPYRIGHT.txt` (AGPL-3.0-only).

## Pull requests

Open PRs against the default branch. New `.py`, `.ts` and `.tsx` files need the
header from `COPYRIGHT.txt` (see `COPYRIGHT_CHECK.md`); the backend's CI is
`.github/workflows/server-ci.yml` and includes a gitleaks scan.
