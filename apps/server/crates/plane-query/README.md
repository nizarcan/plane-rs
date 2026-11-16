<!--
Copyright (c) 2023-present Plane Software, Inc. and contributors
SPDX-License-Identifier: AGPL-3.0-only
See the LICENSE file for details.
-->

# plane-query

The SQL composition, filters, ordering, grouping and pagination of Plane's issue
read path, ported from `apps/api/plane/utils`. The Python is the specification;
this crate is the implementation and `tests/golden/` is the proof.

| Module | Python |
| --- | --- |
| `sql` | — (the composer; see "Why hand-written" below) |
| `issue::filter` | `plane/utils/issue_filters.py` |
| `issue::rich` | `plane/utils/filters/{filterset,filter_backend}.py` |
| `issue::order` | `plane/utils/order_queryset.py` |
| `issue::model` | tables/columns, `ISSUE_GROUP_BY_ALLOWLIST`, `issue_on_results` |
| `issue::values` | `issue_on_results`' projection and row decoding |
| `issue::group_values` | `issue_group_values` |
| `issue::query` | the statements `IssueViewSet.list` builds |
| `paginator` | `plane/utils/paginator.py` |
| `global` | `plane/utils/global_paginator.py` |
| `converters` | `plane/utils/filters/converters.py` |

## Using it

`issue::run` is `IssueViewSet.list` end to end: same joins, same filters, same
order, same paginator, same response body.

```rust,ignore
let body = plane_query::issue::run(client, &plane_query::issue::ListRequest {
    query: ctx.query(),                  // request.query_params
    workspace_slug: &slug,
    project_id,
    timezone,                            // TimezoneMixin's user timezone
    now: plane_query::global::now(),     // for `2_weeks;after` and friends
    user_id: Some(user.id),
    is_guest: membership.role == 5,
    guest_view_all_features: project.guest_view_all_features,
})
.await?;
Ok(ViewResponse::json(200, body.to_serde()))
```

`issue::run_with` takes the switches a *view* hardcodes, in `ListOptions`:

| field | `IssueViewSet.list` | why the views differ |
| --- | --- | --- |
| `filter_backends` | `ComplexOnly` | every `/api/` issue list declares `filter_backends = (ComplexFilterBackend,)`: `?filters=` filters and the plain parameters belong to `issue_filters` only (`plane/app/views/issue/base.py:209`, `plane/utils/filters/filter_backend.py:31-53`). `run` keeps the old "filterset + `?filters=`" behaviour. |
| `scope` | `None` | replaces the `Issue.issue_objects` manager predicate: the archived list uses `Issue.objects` + `archived_at IS NOT NULL` + the epic exclusion (`issue/archive.py:78-86`), the cycle/module lists add their through-table join, the workspace view list adds the membership predicate (`view/base.py:177-197`). |
| `projection` | `Grouped` | `Projection::Aggregate` is the cycle/module/workspace-user annotation: `COUNT(...)` without an inner `GROUP BY`, so an issue with no links renders `0`, not `null`. |
| `group_values_queryset` | `true` | the cycle and archived lists call `issue_group_values` without a queryset, which makes `start_date`/`target_date`/`created_by` grouping an `AttributeError` (500). |
| `default_order_by` | `-created_at` | the module list defaults to `created_at` (`module/issue.py:113`). |
| `grouping` | `true` | `IssueDetailEndpoint` and `WorkspaceViewIssuesViewSet` ignore `?group_by=`; parsing it would add the grouper's join to a page Django returns ungrouped. |
| `scope_override` | `None` | the full `IssueScope` when it is not the one `ListRequest` describes (no `project_id`, or a `created_by` the handler already resolved). |

### Pieces for lists that are not issue lists

* `paginator::OffsetWindow` + `paginator::offset_page` are
  `OffsetPaginator.get_result` for a caller-composed `Select`: the
  `per_page`/`cursor` parse (`PageQuery`), the `offset = page * limit` slice
  with its `limit + 1`th row, `total_count_queryset.count()` and both cursors.
  A list that is not an issue list should not re-implement the arithmetic.
* `global::page_with` takes the caller's projection in order plus optional extra
  columns (the v2 list appends `description_html` after `sub_issues_count`).
* `sql::Value::Int8` binds a 64-bit parameter, so `LIMIT $1`/`OFFSET $1` no
  longer needs a `::int4` cast.
* `sql::prep_for_like_query` is Django's `prep_for_like_query`
  (`django/db/backends/base/operations.py:516-518`): `\`→`\\`, `%`→`\%`,
  `_`→`\_`. `Frag::icontains`, `contains`, `startswith`, `istartswith`,
  `ends_with` and `iends_with` apply it, so `?name=100%` matches a literal
  `100%` instead of every name containing `100`. Any hand-written `LIKE` built
  from a request value must call it too.
* `issue::ListOptions::validate_page_first` moves the paginator's parameter
  checks ahead of the filters', for a view that calls `paginate` before
  `filter_queryset` (the issue lists filter first, the default).

The lower layers (`issue::filter`, `issue::order`, `issue::query`,
`paginator::issue_list_page_with`, `issue::values::field_expression`) are public
so a cycle's or a module's issue list can reuse them with a different scope.

## Why hand-written SQL

`sea-query` was considered — the brief allows it. What this path needs is
correlated `ARRAY_AGG(DISTINCT …)` subqueries, `COUNT(DISTINCT id) FILTER
(WHERE …)`, `ROW_NUMBER() OVER (PARTITION BY <user-chosen column> ORDER BY …)`,
`GROUP BY issues.id` for functional dependency, `AT TIME ZONE` date extraction
and `SELECT DISTINCT` with ordinal ordering. A builder crate expresses none of
those without raw-SQL escape hatches, and the statements that remain are a small
part of the work. Hand-writing keeps the emitted SQL close to Django's (which is
what the goldens compare against) and keeps the dependency tree small, which D1
cares about.

## Statement caching: measured, and deliberately off for dynamic SQL

D1 says to prefer `prepare_cached`. For this read path the measurement says the
opposite, and the crate does not use it for dynamic SQL.

`deadpool_postgres`'s statement cache is **unbounded per connection** and
evicting an entry never sends `DEALLOCATE`. Every filter combination, every
`LIMIT`/`OFFSET` and every key expression is a different SQL text, so the backend
accumulates one prepared plan per request for the life of the connection.

Measured on the dev stack (`apps/server/dev/compose.yml`, Postgres cgroup limit
384 MiB), replaying the 1,064-case filter corpus:

| prepare policy | container peak | outcome |
| --- | --- | --- |
| `prepare_cached` | 383 MiB / 384 MiB | backend OOM-killed (`signal 9`) after ~800 cases |
| `prepare` (unnamed) | 57 MiB | corpus completes |

That is roughly 0.4 MiB of backend memory per distinct statement, which no
client-side cache bound can recover. The cost of `prepare` is one extra round
trip per query; Django/psycopg2 also sends an unnamed statement per `execute`,
so the benchmark comparison below is like for like.

Static statements keep their catalog (`plane-db`) and can use `prepare_cached`
safely: their text never changes. If a future read path finds a stable shape
worth caching (a saved view pinned to a fixed filter set), the fix is to
parameterise the page bounds and hand that shape to `prepare_cached`, not to
cache every request.

## Performance

Request: `group_by=state_id&per_page=100&order_by=-created_at` on the fixture
(300 issues in the project, 225 visible, 5 states, 210 rows on the page),
50 runs each, same database, same machine (macOS arm64; the database is a Docker
VM on the same host, so each round trip is ~3 ms).

| | Rust | Python (`GroupedOffsetPaginator` + `issue_on_results` + `process_results`) |
| --- | --- | --- |
| p50 | **18.9 / 23.5 ms** (two runs) | 37.5 / 40.2 ms |
| p95 | 30.0 / 49.8 ms | 83.0 / 73.2 ms |
| statements per page | **5** | 6 |

Run-to-run variance on the shared Docker VM is about ±20% on both sides; the
ordering (Rust ≈ half the Python time, one fewer statement) is stable across
runs.

Statements: Rust issues the page, a page-count query (see the `count` quirk
below), the windowed count, the group totals and one `issue_group_values`
query. Django issues the page, `exists()`, `count()`, the `max_hits` query, the
group totals and the group-value query. The extra row the port fetches per page
(and per partition) replaces `exists()`; the largest group comes from the totals
it needs anyway. Both sides are dominated by the ~170 KB JSON body and the
round trips over the container network.

Reproduce with `tools/query/bench.py` (Python) and
`cargo test -p plane-query --test bench -- --ignored --nocapture` (Rust).

## The golden corpus

`tools/query/build_fixture.py` builds a deterministic database (every id is
`uuid5` of a name, every timestamp derives from `FIXED_NOW = 2026-06-15T12:00Z`)
with 368 issues covering every filterable attribute: NULL and triage states,
archived, draft and soft-deleted issues, sub-issues, 0–4 labels and 0–3
assignees per issue, an archived and a soft-deleted module, a soft-deleted
cycle, intake rows with every status (including two on one issue), mentions,
subscribers (including soft-deleted rows), links, attachments, dates across a
year, and four users in four timezones.

`tools/query/oracle.py` computes the goldens from Django on that database and
records them under `tests/golden/`:

| file | cases | content |
| --- | --- | --- |
| `filters.json` | 1,064 | legacy `issue_filters` combinations: sorted ids, raw count, distinct count, or the exception Django raises (`ValidationError` for a non-date first segment, which is a 400; `FieldError`/`TypeError`/`ValueError`, which are 500s) |
| `order.json` | 38 | `order_issue_queryset` for every allowed `order_by` (with and without `-`), plus invalid values, and the paginator's re-ordering |
| `group_values.json` | 17 | `issue_group_values` per group field, project- and workspace-scoped |
| `paginator.json` | 32 cases / 101 pages | full response bodies for every group field, seven sub-group pairs, cursor walks forward and backward, and the 400 responses |
| `filterset.json` | 66 | `IssueFilterSet` over query parameters and `ComplexFilterBackend` over JSON trees, including every validation error body |
| `global_paginator.json` | 11 | `global_paginator.paginate`, including the per-user timezone conversion |

`tests/golden.rs` replays all of them. Paginator responses are compared byte for
byte against DRF's rendered body; errors are compared structurally.

### Documented normalisations

Three things in the Python are not reproducible and are normalised in **both**
the golden and the port (nothing else is weakened). The differential harness
applies the same three through path-scoped `unordered_json_paths` rules in
`scenarios/normalize.yaml` (DECISIONS.md D16), so it compares them the same way:

1. `GroupedOffsetPaginator.__query_multi_grouper` builds the `label_ids` /
   `assignee_ids` / `module_ids` arrays from `list(set(...))`, whose order
   depends on `PYTHONHASHSEED`. Sorted, i.e. compared as a multiset (a missing,
   extra or duplicated element still differs).
2. The same method keys its result dict by that set's order, and copies one row
   dict into every group the issue belongs to, so that row's raw group column
   (`labels__id`, …) is whichever of the issue's rows Postgres returned first.
   Group keys are sorted and the row's group column is set to the group it is
   filed under (redundant with the key; `assignee_ids` carries the membership).
3. `__get_subgroup_total_queryset` has no `ORDER BY`, so the order of a group's
   sub-groups is whatever Postgres returns. Cell keys are sorted.

Everything else — ids, order, counts, `total_pages`, `total_count`, key order,
number formatting — is compared as Django emits it.

## Django quirks that decide rows (and are ported)

| Quirk | Python |
| --- | --- |
| `state__in`/`state__group__in` promote the manager's `LEFT JOIN states` to `INNER` | `db/models/issue.py:92`, `db/models/sql/query.py` join promotion |
| `labels`/`assignees`/`cycle`/`module`/`subscriber` add `deleted_at IS NULL` even with no ids, which left-joins (and duplicates) | `issue_filters.py:158,173,331,346,401` |
| `issue_group_values('labels__id')` appends the **string** `"None"`; `state_id` does not | `grouper.py:162-191` |
| `issue_group_values('start_date')` never dedupes: `DISTINCT` is defeated by the default `-created_at` ordering, so it returns one row per issue | `grouper.py:198-217` |
| `type` overwrites `state_group` (same dict key, later function) | `issue_filters.py:296-305` |
| `intake_status` on POST reads `inbox_status` (copy-paste bug → 500 unless that key is present) | `issue_filters.py:361` |
| `sub_issue` compares against the *string* `"false"`, so a JSON `false` does not filter | `issue_filters.py:380` |
| `logged_by` produces a lookup `Issue` has no column for → 500 | `issue_filters.py:414` |
| GET date filters: `;after`/`;before`, `N_weeks;after[;fromnow]` (months = 30 days), anything else becomes a `LIKE` on the date's text; `N_weeks;after` (two parts) silently adds **nothing** | `issue_filters.py:29-81` |
| POST `start_date`/`target_date` are exact matches, not ranges; a list is a `TypeError` | `issue_filters.py:257,272` |
| A GET `start_date=2025-03-01` becomes `start_date__contains` (text LIKE) | `issue_filters.py:247-259` |
| `order_by=priority` inverts: the paginator re-applies `-priority_order`, so `priority` sorts *low first* here but *urgent first* in `IssueListEndpoint` | `order_queryset.py:165-172`, `paginator.py:136-140` |
| `-priority`/`-state__group` still order the annotation **ASC**; only the CASE is reversed | `order_queryset.py:174-183` |
| `order_by` with a min-values field (`labels__name`, …) adds `GROUP BY issues.id`, which de-duplicates and changes counts | `order_queryset.py:193-199` |
| `order_by=created_at` is the only ordering without the `-created_at` tiebreak; ties are undefined | `order_queryset.py:201-203` |
| Grouped `total_count` is `len(queryset)` on the **model** projection with the window in the `DISTINCT`, so two rows of one issue under different groups collapse when they share a row number (412 instead of 444 joined rows) | `paginator.py:278`, `grouper.py:334-368` |
| A group whose filtered count is 0 reports `total_results: 1` | `paginator.py:311-313` |
| `count` is `len(CursorResult.results)` on the model projection, which can be smaller than the number of result rows (92 vs 104) | `paginator.py:749` |
| CONTAINS on a grouped window: `next` is computed with `row_number >= stop` in a separate query | `paginator.py:272` |
| `priority_order` and `state_order` are `CASE` expressions with parameters, so Postgres needs `ORDER BY <ordinal>` under `DISTINCT` | `order_queryset.py:166-182` |
| `total_results` for an m2m group uses `.get(key)` (may be JSON `null`); for other groups `.get(key, 0)` | `paginator.py:311,363` |
| `total_pages` is `ceil(raw_count / limit)` from the **unadjusted** `COUNT(…) FILTER`, while `total_results` maps a zero count to 1 — so the archived list (whose filter excludes every archived row) reports `total_pages: 0` next to `total_results: 1` | `paginator.py:283-288,311-313` |
| A grouped page with no rows is `results: {}`, not the pre-created group dict | `paginator.py:380-389,622-632` |
| A sub-grouped response raises `KeyError` (500) when a row's group/sub-group is not in the precomputed lists — e.g. any NULL state with `group_by=state_id&sub_group_by=…` | `paginator.py:618` |
| `is_archived=true` / `is_draft=true` can never match `Issue.issue_objects` (the manager already excludes them) | `filterset.py:202-211`, `issue.py:92` |
| JSON leaf lists (`{"priority__in": [...]}`) become a repeated query key; the filterset reads `.get()` (the **last** value), so only the last element filters | `filter_backend.py:261-268` |
| A negated multi-valued filter is a `split_exclude`: `NOT (EXISTS(…) AND EXISTS(…))`, which excludes an issue that has the value anywhere; a join + `NOT` would keep it | `db/models/sql/query.py:1594-1620` |
| `created_at`/`updated_at` filters compare the date in the **active** (user profile) timezone, not UTC | `filterset.py:171-189` |
| `start_date__range`/`target_date__range` need one CSV pair and reject repeated query parameters | `filterset.py:194-196`, `django_filters/fields.py:205` |
| `global_paginator` clamps the page size, emits `next_cursor: null` on the last page, 500s on a malformed cursor, and renders datetimes in the user's timezone (`+09:00`, not `Z`) | `global_paginator.py:33-87`, `timezone_converter.py:18-40` |

## Layout of a request

```
IssueQuery::filtered_base      manager + rich filter + legacy filter (+ dates)
IssueQuery::page               plain page: values projection, DISTINCT, ORDER BY <ordinal>
IssueQuery::grouped_page       window over joins, row_number bounds, outer order
IssueQuery::grouped_page_count the model projection's row count (the envelope's `count`)
IssueQuery::grouped_count      windowed count (the envelope's `total_count`)
IssueQuery::totals             (group, COUNT(DISTINCT id) FILTER (…))
paginator::issue_list_page     shapes the response and walks the cursor
```

## Running the tests

```sh
apps/server/dev/mkdb.sh t_query_fixture
apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py
PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
  apps/server/tools/query/oracle.py            # regenerate the goldens (only needed when Django changes)
cd apps/server
PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_query_fixture \
  PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-query
```

Without `PLANE_RS_TEST_DATABASE_URL` the golden tests print a notice and pass;
with `PLANE_RS_REQUIRE_DB_TESTS=1` they fail instead, so CI cannot skip them.

Debugging aids: `PLANE_QUERY_DEBUG_SQL=1` prints each statement, and
`PLANE_QUERY_CASE=<id>` restricts the filter replay to one golden case.
