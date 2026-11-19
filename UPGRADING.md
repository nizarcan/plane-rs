# Moving the upstream pin

plane-rs is a fork of [makeplane/plane](https://github.com/makeplane/plane): the
browser frontends are upstream's, the Django backend under `apps/api` is
replaced by the Rust services under `apps/server`. `UPSTREAM.toml` records the
one upstream commit this repository corresponds to, and every parity proof runs
against **the oracle**: `apps/api` exported from that commit into a cache, with
its own virtualenv (`tools/upstream/oracle.sh`).

Moving the pin means taking upstream's changes since that commit, porting the
backend ones, and changing the commit in `UPSTREAM.toml` only when the full
verification passes. This file is the procedure; `PLAYBOOK.md` §10 is the
method, `apps/server/AGENTS.md` the day-to-day rules.

Do not edit `apps/api` in this repository: it is not there. Read the oracle.

## 0. Prepare

```bash
git fetch upstream                     # make the target ref available
git describe --tags <target>           # the new `describe`
apps/server/tools/upstream/oracle.sh fetch          # the current pin's oracle
apps/server/tools/upstream/oracle.sh check
```

Network is allowed for the upstream fetch and the oracle's package install;
nothing else in this procedure needs it.

## 1. Read the diff report

```bash
apps/server/tools/upstream/diff.py upstream/preview
# writes apps/server/target/upstream-diff/upstream_preview.md (gitignored)
```

The report classifies every changed file into routes (URL confs and the views
they reach), serializers, models, migrations, tasks/schedules, settings/env,
middleware/auth, e-mail templates, backend tests, other backend, frontends and
deployment. For backend changes it also regenerates the route inventory and
introspects the task registry **at the target commit** and diffs them against
`ROUTES.md`/`routes.json` and `TASKS.md`, and it lists every new Django
migration. Review it adversarially: a new route is a new contract, a changed
permission is a behaviour change, a new task is a new side effect.

Keep the report with the change while working; do not commit it.

## 2. Bring the frontends forward

`apps/web`, `apps/space`, `apps/admin`, `apps/live` and `packages/` are taken
from the target commit verbatim (see `[verbatim]` in `UPSTREAM.toml`). Never
adapt them to the Rust backend: they are the contract. Update the whole
subtrees rather than cherry-picking:

```bash
git checkout <target> -- apps/web apps/space apps/admin apps/live packages
pnpm install && pnpm check            # the frontends' own gates
```

`[modified]` lists upstream files this fork edits; today that is only
`apps/proxy/Caddyfile.ce` and `Caddyfile.aio.ce` (D36: `/api/*`, `/auth/*` and
`/static/*` go to `plane-api:8000`). **Do not copy the target file over ours.**
Three-way merge it: compare the pin's version with the target's, then apply
that change to our version and keep the D36 routes:

```bash
git diff <pin> <target> -- apps/proxy/Caddyfile.ce apps/proxy/Caddyfile.aio.ce
```

`tools/upstream/diff.py <target>` lists these files in a “Fork-modified upstream
files” section and flags any the target changed. `diff.py --check-verbatim` (CI)
fails if a `[verbatim]` path differs from the pinned commit, or a `[modified]`
file is missing on either side.

If a frontend change depends on a backend change, port the backend change in
the same branch; the old backend must keep answering the old frontend until
both move together.

## 3. Prepare the target oracle

Every backend check must run against Django at the **target** commit, not the
old one. `oracle.sh` accepts a one-process override; `diff.py` already used it,
and the manual commands below use it too:

```bash
TARGET=<target sha>
export PLANE_RS_UPSTREAM_COMMIT="$TARGET"
export PLANE_RS_ORACLE_DIR="$HOME/.cache/plane-rs/oracle/$TARGET"
apps/server/tools/upstream/oracle.sh fetch
apps/server/tools/upstream/oracle.sh check      # skips the pin-version check on purpose
```

`PLANE_RS_UPSTREAM_COMMIT` affects only the process that has it; the repository
still pins the old commit until step 8. `tools/upstream/oracle.py api-dir`
prints the tree, and `.../oracle.sh python` the interpreter.

## 4. Port the backend changes

Work domain by domain in the order the report lists them, using the same
evidence as the original port (the route inventory, the serializers inventory,
`MODEL_HOOKS.md`, `TASKS.md`, `ENV.md`, `HTTP_CONTRACT.md`). For each change:

1. Read the target view, serializer, permission class, model methods and tasks
   enqueued in the oracle; cite the path (`apps/api/plane/...`) in a comment.
2. Extend or add the differential scenario first, then the handler. Run it
   against the target oracle:
   ```bash
   cd apps/server
   ./target/debug/difftest run --targets scenarios/targets.yaml \
     --scenario scenarios/<file>.yaml --a django --b rust
   ```
   `scenarios/targets.yaml` starts Django through `dev/django-serve.sh`, which
   reads the oracle directory from the environment, so the same
   `PLANE_RS_UPSTREAM_COMMIT`/`PLANE_RS_ORACLE_DIR` pair selects the target.
3. A new or changed task needs a `difftest task` case (`tools/jobs/fixtures/`)
   and, if it is Rust-owned, an ownership/producer update (`DECISIONS.md` D25).
4. A changed env var, permission, CSRF/session format or mail template needs its
   generator or golden corpus updated in the same change (see step 6).
5. Never make the new system "better" than the target: port reachable bugs and
   record them in the ledger.

Changed routes and tasks appear in the diff report; `ROUTES.md` and `TASKS.md`
are generated/reviewed artefacts and are updated as part of the port (get the
owner's review for `ROUTES.md` owner/status cells).

## 5. Schema changes and existing installations

This is the part that differs from a fresh rewrite: existing plane-rs
installations already have a database. `plane-migrator` owns `public` from the
Django baseline on (`DECISIONS.md` D34), so a new upstream migration has to be
applied to those databases without Django running.

The mechanism is **ordered post-baseline steps**:

- `deploy/schema/baseline.sql` + `django_migrations.txt` are regenerated at the
  new pin. A fresh install applies that dump and gets the newest schema and
  history; it never replays steps.
- Every migration between the old and the new baseline becomes one
  `UpgradeStep` in `bins/plane-migrator/src/upgrade.rs`, oldest first, with the
  SQL from `manage.py sqlmigrate <app> <migration>` at the target commit:

  ```bash
  PLANE_RS_UPSTREAM_COMMIT="$TARGET" PLANE_RS_ORACLE_DIR="$HOME/.cache/plane-rs/oracle/$TARGET" \
    "$(apps/server/tools/upstream/oracle.sh python)" \
    apps/server/tools/schema/upgrade_step.py \
      --step 0001 --app db --migration 0126_add_widget \
      --from '<old baseline id>' --to '<id after this step>'
  ```

  The script writes `deploy/schema/upgrade_steps/0001_db_0126_add_widget.sql`
  and prints the `UpgradeStep` entry to paste into `upgrade.rs`. `from` must be
  the previous step's `to`; for the first step it is the *previous release's*
  `BASELINE_ID` (read it before step 8 changes the constant). The last step's
  `to` must equal the new `BASELINE_ID`. `upgrade_step.py` records the oracle
  commit it generated the SQL from in the file header.
- The migrator takes the advisory lock, reads
  `plane_jobs.meta.django_baseline`, and if it names an older baseline applies
  the steps whose `from` matches, in order. Each step runs in its own
  transaction: SQL, then the `(app, name)` rows it stands for in
  `django_migrations` (guarded, so a retry is a no-op), then the marker moves
  to the step's `to`. A database without the marker is recognised by its
  migration set against the known states, so a pre-marker installation upgrades
  too. An unknown marker or an unrecognised migration set is refused, never
  guessed; older and newer databases keep their existing refusal messages.
- `sqlmigrate` prints nothing for a `RunPython` migration. Write the backfill
  by hand, review it against the Python body and record the same history rows;
  a step that only records history is a silent schema divergence and must not
  be added.
- Released steps are immutable and stay in the list forever. Add steps, never
  rewrite them: an installation may upgrade from any released baseline.
- Data migrations that need application code (not expressible as SQL) are the
  exception: keep them as an explicit operator step in the release notes and
  refuse to start the new binaries until it has run. `plane-admin` is the place
  for a command-driven backfill (`DECISIONS.md` D31).

Proof for a pin move:

```bash
# dump-identical fresh schema, upgrade path, and every refusal
apps/server/tools/upstream/oracle.sh fetch
PLANE_RS_UPSTREAM_COMMIT="$TARGET" PLANE_RS_ORACLE_DIR="$HOME/.cache/plane-rs/oracle/$TARGET" \
  "$(apps/server/tools/upstream/oracle.sh python)" \
  apps/server/tools/schema/verify_migrator.py --pg-dump 'docker exec plane-rs-dev-postgres-1 pg_dump'

# the ordered-step state machine, with a synthetic two-step chain
cd apps/server
PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_migrator_upgrade \
PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-migrator
```

The step manifest's invariant is unit-tested: every recorded migration row must
already be in the refreshed `django_migrations.txt`, so a forgotten baseline
regeneration fails `cargo test`.

## 6. Regenerate the derived artefacts

Django's own introspection produced the tables and corpora; run each generator
against the target oracle (the environment is the one the generator documents,
plus the two oracle variables). From `apps/server/`:

```bash
PY="$(tools/upstream/oracle.sh python)"
export DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=pin-move \
       DATABASE_URL=postgresql://x:x@127.0.0.1:1/x REDIS_URL=redis://127.0.0.1:1/0 \
       ENABLE_DRF_SPECTACULAR=1

"$PY" tools/inventory/routes.py            # then review ROUTES.md owner/status cells
"$PY" tools/codegen/models.py --rust
"$PY" tools/inventory/serializers.py
"$PY" tools/inventory/env_vars.py --check-md ENV.md
"$PY" tools/inventory/attachment_mime_types.py
"$PY" tools/inventory/lxml_html_tables.py --check
"$PY" tools/jobs/gen_component_corpus.py --check
"$PY" tools/mail/email_message_corpus.py --check
"$PY" tools/mail/email_template_corpus.py --check
"$PY" tools/boot/gen_instance_config.py --check
"$PY" tools/boot/static_files.py --check
"$PY" tools/openapi/check.py --check
"$PY" tools/proxy/gen_caddy.py --check
"$PY" tools/net/ssrf_corpus.py --check
"$PY" tools/descriptions/gen_description_corpus.py --check
"$PY" tools/descriptions/gen_html_entities.py --check
for corpus in bs4_corpus bs4_entities csv_corpus datetime_corpus estimate_list_corpus \
              float_corpus lxml_html_corpus lxml_html_extra_corpus xlsx_corpus; do
  "$PY" "tools/inventory/$corpus.py" --check
done
```

Some corpora pin CPython itself (the oracle is Python 3.12.13 at the pin); a
generator whose corpus changes because the interpreter moved is a finding, not
a mechanical regeneration: decide whether the target commit really changed the
output. Run `tools/schema/baseline.py --write` (with `--dump-url`/`--pg-dump`)
only as part of step 5, and commit its `django_migrations.txt` together with
the dump.

## 7. Full verification

From `apps/server/`, on a quiet machine, with the target oracle selected:

1. `cargo fmt --all --check`
2. `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `CARGO_INCREMENTAL=0 cargo test --workspace` (plus the testkit workspace)
4. Every generator `--check` above and `tools/schema/baseline.py --check`
5. `tools/schema/verify_migrator.py`, `tools/boot/compare_boot.py`
6. `tools/jobs/run-task-cases.sh ...` (the command in `PROGRESS.md`)
7. Every `scenarios/read`, `scenarios/write`, `scenarios/boot` file, the OAuth
   wrappers and the SMTP wrapper
8. `tools/contract/run.sh` against the target oracle's `plane/tests/contract`
9. `tools/cutover/exit_check.py all`
10. `cargo fmt/clippy/test` in `crates/plane-testkit`

The exact commands, resource names and expected counts are in
`apps/server/AGENTS.md`, `apps/server/scenarios/README.md` and the latest
verification entry in `apps/server/PROGRESS.md`. A skip is not a pass: a missing
oracle, database or container is a failure to fix, not to normalise.

## 8. Move the pin

Only after step 7 passes, edit `UPSTREAM.toml`:

- `commit`, `describe`, `date` from the target;
- `django` and `uv` from the target's `apps/api/pyproject.toml`
  (`uv` is its `required-version`);
- leave `[verbatim]` paths and `[replaced]` unchanged.

Then re-fetch the default oracle and prove the file and the cache agree:

```bash
unset PLANE_RS_UPSTREAM_COMMIT PLANE_RS_ORACLE_DIR
apps/server/tools/upstream/oracle.sh fetch
apps/server/tools/upstream/oracle.sh check
apps/server/tools/upstream/diff.py --check-verbatim  # [verbatim]/[modified] table
apps/server/tools/upstream/diff.py <same target>     # says "is the pinned commit"
```

Also update `BASELINE_ID` in `bins/plane-migrator/src/schema.rs` to the new
baseline's leaf migrations; the test `baseline_id_names_the_leaf_migrations`
and the migrator's fresh-install marker both depend on it.

Update `apps/server/PROGRESS.md` with the new upstream reference and the
verification summary; record decisions taken while porting in
`apps/server/DECISIONS.md` (propose text to the owner; workers do not edit it).

## 9. Release

The release is the ordinary one (`DECISIONS.md` D36/D38): images
`ghcr.io/nizarcan/plane-rs-*`, the Helm chart in `charts/plane-rs`, and the Helm
migrator Job. The migrator is what carries existing installations across the
schema steps, so release the migrator image with the API/worker/beat images of
the same commit and let it run to completion before the long-running services
start. On the first deployment after a pin move, watch the migrator log for
`applied upgrade step ...` lines and the `plane_jobs.meta.django_baseline`
value.

## Rollback

- A binary refuses a database whose marker it does not know; it never
  downgrades. Rolling back the deployment therefore also means restoring the
  database backup taken before the migrator ran, not running an older
  migrator against the newer schema.
- `UPSTREAM.toml` is the pin, not a history: keep the old commit in the release
  notes. The oracle cache holds both while they are on disk.
- Never delete a released `UpgradeStep`; a later pin move starts from it.
