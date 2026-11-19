# Upstream pin and the Django oracle

`UPSTREAM.toml` at the repository root records the upstream commit plane-rs
corresponds to. Django is not in this checkout; every parity proof runs against
**the oracle**: `apps/api` exported from that commit into a per-user cache, with
a virtualenv built from the commit's own `uv.lock`.

| File | Purpose |
| --- | --- |
| `oracle.sh` | Fetch, check and locate the oracle. `fetch` is idempotent (stamp file), `check` fails when the cache is stale, `api-dir`/`python` print the paths, `env` prints shell assignments. Also sourced by `dev/lib.sh` for `oracle_api_dir`/`oracle_python`. |
| `oracle.py` | The same locator for Python tools (`api_dir()`, `python()`, `require()`, `display_path()`), plus the `meta` and `shell-env` CLI. Used by every generator and by `crates/plane-oracle` on the Rust side. |
| `diff.py` | Classifies `pinned..<ref>` for porting and regenerates the route/task inventories against the target oracle. Writes to `apps/server/target/upstream-diff/`; reports are never committed. |

Cache layout (home directory on purpose: the Docker VM only mounts `$HOME`):

```text
${PLANE_RS_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/plane-rs}/
    git/plane.git/                 bare mirror, fetched by commit sha
    oracle/<commit>/
        .oracle-stamp              commit, tree object, uv, Django, creation time
        apps/api/                  the upstream tree at the pin
            .venv/                 uv environment (dev + test + gunicorn)
```

`PLANE_RS_ORACLE_DIR` overrides `<cache>/oracle/<commit>` (it must still contain
`apps/api/`); `PLANE_RS_CACHE_DIR` overrides the cache root.
`PLANE_RS_UPSTREAM_COMMIT` selects a different commit for one process, which is
how `diff.py` and a pin move (`UPGRADING.md`) prepare the target oracle while
the repository still pins the old one.

Network is used only for the `git fetch` and the pinned uv's package download.
A commit already in the checkout's object database is exported locally without
touching the network. `oracle.sh fetch --force` rebuilds the tree and venv.
