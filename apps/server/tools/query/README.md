<!--
Copyright (c) 2023-present Plane Software, Inc. and contributors
SPDX-License-Identifier: AGPL-3.0-only
See the LICENSE file for details.
-->

# `tools/query` — fixture, oracle and benchmark for `crates/plane-query`

Three scripts, all run against a **fresh clone** of the template database
(`apps/server/dev/mkdb.sh`). Nothing here connects to a database the caller did
not name through `PLANE_DB`.

```sh
# 1. one database per run; the fixture refuses to run on a non-empty one
apps/server/dev/mkdb.sh t_query_fixture
apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py

# 2. regenerate the goldens (only when Django or the fixture changes)
PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
  apps/server/tools/query/oracle.py

# 3. Python's half of the benchmark
PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
  apps/server/tools/query/bench.py

# 4. drop it
apps/server/dev/dropdb.sh t_query_fixture
```

`run.sh` sources the same environment as `apps/server/dev/django.sh` (Django
settings, `DATABASE_URL`, `SECRET_KEY`) and runs `apps/api/.venv/bin/python`
directly, so concurrent agents never trigger a `uv sync`.

## Files

| file | purpose |
| --- | --- |
| `fixture_spec.py` | the deterministic ids, the users and their timezones, `FIXED_NOW` |
| `build_fixture.py` | builds the database and writes `crates/plane-query/tests/golden/meta.json` |
| `django_issue_query.py` | the queryset assembly `IssueViewSet.list` uses, shared by the oracle and the benchmark |
| `oracle.py` | computes the goldens (filters, ordering, group values, paginators, filterset, global paginator) |
| `bench.py` | times the Python paginator for `group_by=state_id&per_page=100` |

## Determinism

Every id is `uuid5` of a readable name and every timestamp derives from
`FIXED_NOW`. The oracle freezes `django.utils.timezone.now()` to that instant
(it asserts the patch took effect), so relative date filters
(`2_weeks;after;fromnow`) replay forever. `meta.json` records the instant; the
Rust tests pass it in.

Two Django behaviours are normalised in the goldens and in the port — the order
of the id arrays the multi-grouper builds from a `set`, and the cell order of a
sub-grouped response (no `ORDER BY` in the Python). Both are listed in
`crates/plane-query/README.md` under "Documented normalisations".
