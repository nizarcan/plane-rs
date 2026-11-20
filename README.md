# plane-rs

**plane-rs** is [Plane](https://github.com/makeplane/plane) (Community Edition) with its
Django/Celery backend replaced by a Rust implementation that is behaviourally identical:
the same PostgreSQL schema, the same HTTP API byte for byte, and the same side effects
(database rows, background jobs, e-mails, webhooks, stored files). The browser frontends are
upstream's, unchanged, so the web app, the admin console and public spaces work as they do
on Plane.

> **Unofficial.** plane-rs is an independent fork. It is not made, endorsed or supported by
> Plane Software, Inc. "Plane" and the Plane logo are trademarks of Plane Software, Inc.;
> they are used here only to say which project this fork is derived from. Report plane-rs
> issues here, not upstream.

## Which Plane it matches

plane-rs corresponds to one upstream commit, recorded in [`UPSTREAM.toml`](./UPSTREAM.toml):
`makeplane/plane@290383d7f9` (`v1.5.0-rc2`, 2026-11-13). The frontends in `apps/web`,
`apps/space`, `apps/admin`, `apps/live` and `packages/` are that commit's code, unchanged.
Moving to a newer Plane is a deliberate, verified step described in
[`UPGRADING.md`](./UPGRADING.md).

## Why

A single-user self-hosted Plane idles at about 3.5 GB, and the Python services are most of
it. Measured with identical container limits ([`apps/server/BENCH.md`](./apps/server/BENCH.md),
"Phase 8 — final"):

| | Django / Celery | plane-rs |
| --- | ---: | ---: |
| API idle memory | 143 MiB | 59 MiB |
| Worker idle memory | 360 MiB (Celery, 4 processes) | 1.6 MiB |
| Scheduler idle memory | 146 MiB | 0.9 MiB |
| Message broker | RabbitMQ (~140 MiB) | none (Postgres queue) |
| Reads, one at a time | 24–74 req/s | 246–1,311 req/s |
| Mixed reads, 8 concurrent, p99 | 353 ms | 16 ms |
| Background jobs | 94 jobs/s | 773 jobs/s |

## What's in the box

| Service | Image | Replaces |
| --- | --- | --- |
| `plane-api` | `ghcr.io/nizarcan/plane-rs-api` | Django API (gunicorn) |
| `plane-worker` | `ghcr.io/nizarcan/plane-rs-worker` | Celery worker |
| `plane-beat` | `ghcr.io/nizarcan/plane-rs-beat` | Celery beat |
| `plane-migrator` | `ghcr.io/nizarcan/plane-rs-migrator` | Django `migrate` |
| web, space, admin, live, proxy | `ghcr.io/nizarcan/plane-rs-{web,space,admin,live,proxy}` | upstream's, rebuilt from this tree |

Background jobs run on a PostgreSQL queue (`plane_jobs` schema), so RabbitMQ is gone. Images
are published for `linux/amd64` and `linux/arm64`.

## Install

- **Kubernetes:** the Helm chart in [`charts/plane-rs`](./charts/plane-rs) (bundled or
  external PostgreSQL, Valkey and MinIO; existing Secrets for External Secrets; ArgoCD-safe).
- **Docker Compose:** [`deployments/cli/community`](./deployments/cli/community) and the root
  [`docker-compose.yml`](./docker-compose.yml).

The migrator installs a fresh database, or upgrades an existing Plane database **only if it
is exactly at the pinned migration set** (`v1.5.0-rc2`); it refuses older, newer and
unrecognised databases with a message saying what to do. Back up before pointing it at real
data.

## How it is proven

Nothing here counts as done on the strength of reading code. Every route, background task,
schedule, boot step and the schema itself is compared against upstream's Django at the pinned
commit, which the tooling fetches into a local cache
([`apps/server/tools/upstream`](./apps/server/tools/upstream)):

- a differential harness replays 131 scenario files (6,345 requests) against Django and
  plane-rs from the same database snapshot and compares status, headers, body bytes, the
  database diff, cache keys, published jobs, stored objects, mail and outbound requests;
- 308 background-task cases compare each task's full effect the same way;
- upstream's own contract tests run against plane-rs;
- the migrator's schema is dump-identical to Django's;
- a scripted exit check installs fresh and upgrades a Django database with no Python
  process anywhere, then walks sign-in through work items, uploads, exports, invitations and
  webhooks.

Upstream bugs are reproduced, not fixed, so that plane-rs behaves exactly like the Plane it
pins; each one is listed in [`apps/server/PROGRESS.md`](./apps/server/PROGRESS.md).

## Known limitations

- `apps/live` (real-time collaboration) is still upstream's Node service.
- A handful of documented, contract-invisible differences: webhook-log header-name casing
  (D33), some boot-time behaviour (D34), telemetry exporter options listed in
  [`apps/server/ENV.md`](./apps/server/ENV.md). See
  [`apps/server/DECISIONS.md`](./apps/server/DECISIONS.md).
- Some error-path follow-ups are tracked in `apps/server/PROGRESS.md`.
- No independent security review has been done. Authentication, permissions, rate limits and
  the SSRF guards are ported from Django and tested for parity, not audited.
- Upgrades have been proven on fixture databases, not on large production datasets.
- The all-in-one image is not published.

## Repository map

| Path | What |
| --- | --- |
| `apps/server/` | the Rust backend: library crates, the four binaries, the harness, tools ([`AGENTS.md`](./apps/server/AGENTS.md)) |
| `apps/web`, `apps/space`, `apps/admin`, `apps/live`, `packages/` | upstream frontends, unchanged |
| `apps/proxy/` | Caddy, routing the API paths to `plane-api` |
| `charts/plane-rs/` | Helm chart |
| `deployments/` | Compose, Swarm and all-in-one deployments |
| [`UPSTREAM.toml`](./UPSTREAM.toml) | the upstream pin |
| [`UPGRADING.md`](./UPGRADING.md) | moving to a newer upstream |
| [`PLAYBOOK.md`](./PLAYBOOK.md) | how this rewrite was evaluated and executed, as a reusable method |
| [`RUST_REWRITE.md`](./RUST_REWRITE.md) | the original brief |

## Development

See [`AGENTS.md`](./AGENTS.md) and [`apps/server/AGENTS.md`](./apps/server/AGENTS.md). In short:

```bash
cd apps/server
tools/upstream/oracle.sh fetch          # upstream Django at the pin, into ~/.cache/plane-rs
dev/up.sh                               # local Postgres, Valkey, RabbitMQ, MinIO (see dev/)
dev/bootstrap.sh && dev/bootstrap-fixture.sh   # template databases the harness clones
cargo test --workspace
./target/debug/difftest run --scenario scenarios/read/projects.yaml --a django --b rust
```

## License

AGPL-3.0-only, as upstream ([`LICENSE.txt`](./LICENSE.txt)). Source files keep upstream's
copyright header. If you run a modified plane-rs as a network service, the AGPL requires you
to offer its source to its users.
