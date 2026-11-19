# Boot-step tooling

`apps/api/bin/docker-entrypoint-api.sh` runs seven setup steps before gunicorn
binds: `wait_for_db`, `wait_for_migrations`, `register_instance`,
`configure_instance`, `create_bucket`, `clear_cache` and `collectstatic`. The
Rust rewrite moves them into `crates/plane-bootstrap` (`plane-api` runs the
first six; `/static/` replaces `collectstatic`). This directory holds the
oracles that keep those steps honest, because a normal scenario sweep cannot:
the harness's Django target (`dev/django-serve.sh`) starts *after* the setup
steps, and the Rust target sets `PLANE_RS_BOOT_STEPS=skip` so both sides begin
from the same cloned fixture (D34).

| File | Purpose |
| --- | --- |
| `compare_boot.py` | Runs every boot step and operator command on a Django clone (A) and a Rust clone (B) and compares rows, Redis, the bucket, SMTP and command output. |
| `run_django_boot.py` | Runs one Django boot command in-process with `requests.get` pointed at a local fake GitHub and `push_instance_metrics.delay()` recorded instead of published. Used by `compare_boot.py`. |
| `fake_github.py` | A local `releases/latest` stand-in (`--port`, `--tag`), so no test reaches the internet. |
| `django-static-serve.sh` | Serves Django with a collected static root, for `scenarios/boot/static.yaml`. |
| `static_files.py` | `--check` / `--write` the eight embedded `/static/` files in `crates/plane-bootstrap/assets/static/`. |
| `gen_instance_config.py` | `--check` / `--write` the 36 `InstanceConfiguration` variables embedded from Django's Python source. |

## Boot-step parity

From `apps/server/`, with the dev stack up and these binaries built:

```bash
cargo build -p plane-admin
"$(tools/upstream/oracle.sh python)" tools/boot/compare_boot.py
```

It uses its own databases `t_p8boot_boot_a` / `t_p8boot_boot_b`, Redis DBs 7/8,
the bucket `p8boot-boot`, the fake GitHub on `127.0.0.1:18420` and two fake SMTP
listeners on `18411`/`18412`. It drops what it created and kills every child it
spawned on exit. The run passes when the last line is:

```
PASS: every boot step and operator command matches Django
```

`compare_boot.py` is the boot-step oracle: the scenario sweeps deliberately do
*not* run the mutating steps on either side. `PLANE_RS_BOOT_STEPS` defaults to
`all`; only `scenarios/targets.yaml`'s `rust` target sets `skip`, and
`plane-api` logs a warning when it does. See `ENV.md` §6.

## `/static/` scenario

`scenarios/boot/static.yaml` runs against whichever targets file the caller
passes. It declares the Django command it needs in `target_commands.django`, so
the collectstatic wrapper replaces the usual `dev/django-serve.sh` while ports,
environment and the secret key still come from the caller's file. The target in
that file must be named `django` (the default). From `apps/server/`:

```bash
./target/debug/difftest run --scenario scenarios/boot/static.yaml \
  --a django --b rust
```

It works with `scenarios/targets.yaml` and with a copy whose `port_range` was
changed for a private run, for example:

```bash
./target/debug/difftest run --scenario scenarios/boot/static.yaml \
  --targets /tmp/targets-bootfix.yaml --a django --b rust \
  --a-redis-db 7 --b-redis-db 8
```

The wrapper collects the static tree into `${TMPDIR:-/tmp}/p8boot-static-root`
on first use, pins every file's mtime to the instant the embedded copies use, and
serves it with the same gunicorn/uvicorn worker as `dev/django-serve.sh`. The
scenario's steps all check `expect_status: 200`, so a 404 from an uncollected
root fails instead of comparing two empty bodies.

Keep the embedded copies in sync after the oracle's `plane/static/*` changes
(a pin move):

```bash
"$(tools/upstream/oracle.sh python)" tools/boot/static_files.py --check
"$(tools/upstream/oracle.sh python)" tools/boot/gen_instance_config.py --check
```

`--write` regenerates them. Both assets are embedded at compile time, and the
`--check` mode fails when the committed copy has drifted.

## Fake GitHub

`register_instance` asks GitHub for `tag_name` with a 10 s timeout and falls
back to the current version on any failure. `compare_boot.py` gives each side a
different route to the same answer: the Django wrapper patches `requests.get`,
the Rust side reads `PLANE_RS_GITHUB_RELEASES_URL`. The fake is only for these
tools; a deployment leaves both unset:

```bash
tools/boot/fake_github.py --port 18420 --tag v9.9.9
PLANE_RS_GITHUB_RELEASES_URL=http://127.0.0.1:18420/repos/makeplane/plane/releases/latest \
  target/debug/plane-admin register-instance <signature>
```
