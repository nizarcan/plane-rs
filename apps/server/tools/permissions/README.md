# Permission fixture and oracle

The differential harness for `crates/plane-permissions`: the Python permission
classes are the specification, so the crate is checked cell by cell against them
on a database built to hit every branch.

## What is here

| File | Purpose |
| --- | --- |
| `common.py` | The fixture's fixed ids, users, URL-kwargs contexts and the matrix dimensions. Shared by the builder and the oracle so both describe the same database. |
| `fixture.py` | Builds the users, workspaces, projects, memberships, pages, issues and creator-checked objects in a throwaway clone of `plane_tpl`. Writes `crates/plane-permissions/tests/golden/fixture.json`. |
| `oracle.py` | Imports the real classes and decorators from the dotted paths in `routes.json`, builds DRF `Request` objects, evaluates every case and writes `crates/plane-permissions/tests/golden/matrix.json`. |
| `run.sh` | Sources `apps/server/dev/django-env.sh` with `PLANE_DB` pointed at the fixture database and runs a command. |

The golden matrix is 76,544 permission decisions plus 47 `get_permissions`
resolutions. `crates/plane-permissions/tests/golden.rs` replays all of them
against the same database and fails on any difference.

## Rebuild from scratch

From the repository root:

```sh
apps/server/dev/mkdb.sh t_perms_fixture --replace

PLANE_DB=t_perms_fixture apps/server/tools/permissions/run.sh \
  apps/api/.venv/bin/python apps/server/tools/permissions/fixture.py

PLANE_DB=t_perms_fixture apps/server/tools/permissions/run.sh \
  apps/api/.venv/bin/python apps/server/tools/permissions/oracle.py

cd apps/server
PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_perms_fixture \
  PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-permissions
```

The oracle run takes about a minute (≈77k cases at a few queries each) and the
matrix is deterministic: rebuilding the fixture from a fresh clone reproduces
`matrix.json` byte for byte (verified by comparing its SHA-256).

## Coverage

* **`class`** — every permission class in `routes.json`
  (`permission_classes`, plus the classes only reachable through
  `get_permissions`), against the URL-kwargs shapes of the routes that carry it
  and the missing-kwarg/cross-workspace shapes that reach its deny paths, for
  every user and every HTTP method including `TRACE`.
* **`spec`** — every distinct `allow_permission` spec (roles, level, creator,
  model): 21 of them. The 15 `creator=True` specs also run against one object
  per `model` (eight models) plus the soft-deleted twin of the Issue.
* **`helper`** — `issue_hidden_from_guest` on the guest's own issue, another
  member's issue, a soft-deleted issue and an issue in the guest-unrestricted
  project.
* **`dynamic`** — the class list each `get_permissions` override resolves to,
  per `(view, method, action)`.
* **`check`** — DRF's `APIView.check_permissions` over each distinct
  `permission_classes` list, which pins the order and the 401-vs-403 decision.

The fixture also pins three behaviours that look like bugs and are ported as
they are: members of a soft-deleted workspace still pass workspace checks (the
join does not use the workspace's soft-delete manager), a page owner passes
every verb on their page (the owner branch returns before the method table), and
`allow_permission` on a route without `project_id`/`pk` raises `KeyError`
(rendered as a 400, not a 403).

`fixture.py` refuses to write outside a `t_*` database and refuses to run twice
on the same clone; `oracle.py` refuses to read outside a `t_*` database. Neither
opens a connection anywhere else.
