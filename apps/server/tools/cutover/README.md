# Phase 8 exit check

`plans/CUTOVER.md` §5: prove the cutover deployment works on a fresh database
and on a database migrated by Django, without a Python or RabbitMQ process
anywhere in the stack.

```sh
# everything: build, fresh, upgrade, clean up
"$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/cutover/exit_check.py all

# one path (the images are reused from the local Docker cache)
"$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/cutover/exit_check.py fresh
"$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/cutover/exit_check.py upgrade

# leave the stack and the run directory for inspection
"$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/cutover/exit_check.py all --keep

# only the static no-`apps/api/bin` / compose-config checks
"$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/cutover/exit_check.py static
```

The script exits 0 only when every required step passes; it prints `PASS`,
`FAIL` and `SKIP` lines and a summary. The cutover gate is strict: invite and
webhook delivery must work, and **any** `celery publish failed`,
`plane.exception` or `amqp: IO error` line in the api, worker or beat log is a
failure. That is what proves nothing still needs the broker; every task,
including `push_instance_metrics` (D34 amendment), is Rust-owned and
published through `plane_jobs`.

On a branch that predates set B (`rust-rewrite` before `29dd5da1d9`) the same
run necessarily fails: the walk's project/work-item requests publish
`model_activity`, which is still Celery-owned, and the missing broker turns
that into a 500. `--allow-celery-dispatch` is the explicitly labelled pre-merge
mode: it queues those two missing tasks with the test-only
`PLANE_RS_JOBS_RUST_TASKS` override, prints the same dispatch/handler lines as
`NOTE`s, and skips the invite/webhook delivery assertions. It is for validating
the deployment mechanics before the merge, never for the exit decision.

## The stack

`compose.yml` is a throwaway backend-only stack:

| Service | Host port | Why |
| --- | --- | --- |
| `plane-db` | 18500 | Postgres 15.7 |
| `plane-redis` | 18501 | Valkey |
| `plane-minio` | 18502 / 18503 | S3 (`uploads` bucket) |
| `plane-api` | — (through the proxy) | Rust `plane-api` |
| `plane-worker` / `plane-beat` | — | Rust jobs |
| `migrator` | — | Rust schema owner |
| `proxy` | 18505 | `apps/proxy/Caddyfile.ce` mounted into `caddy:2` |
| `cutover-sink` | — | Rust SMTP sink + webhook listener + fake GitHub |

All host ports are in 18500–18519 and bound to `127.0.0.1`. The project name
is `plane-cutover`; every container is `plane-cutover-*`. No frontend image is
built or started. `/api`, `/auth` and `/static` work without one; `/` is the
api's health JSON.

`sink/` is one std-only Rust binary (compiled in a throwaway image) that
answers SMTP on 25, records webhooks on 8080 and answers
`/repos/makeplane/plane/releases/latest` with GitHub's JSON, which
`PLANE_RS_GITHUB_RELEASES_URL` points at. This keeps the walk offline and
keeps every container Python-free. `docker cp plane-cutover-sink:/logs/.`
reads the captured messages.

## What the walk covers

sign-up → sign-in → `/api/users/me/` → workspace → project → work item →
comment → project read (`recent_visited_task`) → asset create + presigned
upload + confirm (`storage_metadata_task`) → CSV export (`issue_export_task`)
→ invite (SMTP) → webhook create + trigger → `/static/logos/github_32px.png`.
The upgrade path first runs `manage.py migrate` and `configure_instance` on a
host venv against a `plane_upgrade_src` database, `pg_dump`s it, restores it
as `plane`, and then starts only the Rust containers. It asserts the migrator
prints `baseline matched`.

## The read-only difftest scenarios

The harness *can* attach to this stack: `difftest run` takes `--a-url`,
`--a-db` and `--pg-url` for an already-running target A. It cannot be used for
the 53 `scenarios/read/*.yaml` files here, for two reasons outside this
script's control:

1. every read scenario needs `plane_fix_tpl`, the rich fixture built by
   `apps/server/dev/bootstrap-fixture.sh`; that script is pinned to the dev
   stack (`apps/server/dev/.env.local`, `plane_tpl`, port 55432) and must not
   be run against this stack;
2. the scenarios carry literal `session-id` cookies signed with
   `dev/.env.local`'s `SECRET_KEY`, so a fixture built with the cutover stack's
   throwaway key would not authenticate. Copying the dev secret into the
   cutover stack is exactly what the isolation rules forbid.

Once (1) and (2) are solved, the shape is `difftest run --scenario
scenarios/read/<group>.yaml --a rust --a-url http://127.0.0.1:18505 --a-db
<clone of plane_fix_tpl> --pg-url postgresql://plane:plane@127.0.0.1:18500/plane`
(B is the managed target from `targets.yaml`, cloned on the same server). It is
deliberately not part of `exit_check.py`: the cutover stack is a fresh/upgrade
install, not the differential fixture.

## Cleanup

Everything the script creates is removed at the end: the compose project
`plane-cutover` (containers, volumes, network) and the five images
`plane-cutover-{api,worker,beat,migrator,sink}:local`. `--keep` skips cleanup
and prints the run directory (`run/`, gitignored) with the env file, the
upgrade dump, the migrator log and the copied sink logs.
