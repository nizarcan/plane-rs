# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Time the Python side of the grouped issue list, for `crates/plane-query/README.md`.

    apps/server/dev/mkdb.sh t_query_fixture
    apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py
    PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
      apps/server/tools/query/bench.py

It runs `IssueViewSet.list`'s paginator call for
`group_by=state_id&per_page=100` against the fixture, 50 times, and reports the
median and p95 wall time plus the number of SQL statements the paginator
issues. `tools/query/oracle.py` is the reference for the queryset assembly; this
script reuses `django_issue_query`.
"""

import statistics
import sys
import time
from pathlib import Path

import django

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fixture_spec as spec  # noqa: E402

django.setup()

from django.db import connection  # noqa: E402

from django_issue_query import apply_annotations, base_queryset, count_filter  # noqa: E402
from plane.utils.grouper import issue_group_values, issue_on_results, issue_queryset_grouper  # noqa: E402
from plane.utils.order_queryset import order_issue_queryset  # noqa: E402
from plane.utils.paginator import Cursor, GroupedOffsetPaginator  # noqa: E402

ITERATIONS = int(sys.argv[1]) if len(sys.argv) > 1 else 50
PER_PAGE = 100
ORDER_BY = "-created_at"
GROUP_BY = "state_id"


def build():
    base = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID)
    annotated = apply_annotations(base)
    queryset, returned = order_issue_queryset(issue_queryset=annotated, order_by_param=ORDER_BY)
    queryset = issue_queryset_grouper(queryset=queryset, group_by=GROUP_BY, sub_group_by=False)
    group_by_fields = issue_group_values(
        field=GROUP_BY,
        slug=spec.WORKSPACE_SLUG,
        project_id=str(spec.PROJECT_ID),
        filters={},
        queryset=base,
    )
    return (
        GroupedOffsetPaginator(
            queryset=queryset,
            group_by_field_name=GROUP_BY,
            group_by_fields=group_by_fields,
            count_filter=count_filter(),
            order_by=returned,
        ),
        base,
    )


def render(paginator, limit):
    """The paginator call the view makes, including `on_results`."""
    result = paginator.get_result(limit=limit, cursor=Cursor(0, 0, 0))
    rows = issue_on_results(group_by=GROUP_BY, issues=result.results, sub_group_by=False)
    grouped = paginator.process_results(results=rows)
    return result, grouped


def count_queries(fn):
    queries = []

    def wrapper(execute, sql, params, many, context):
        queries.append(sql)
        return execute(sql, params, many, context)

    with connection.execute_wrapper(wrapper):
        fn()
    return len(queries)


def main():
    # Warm-up: first use of the connection, statement caches, plan caches.
    paginator, _base = build()
    render(paginator, PER_PAGE)

    samples = []
    for _ in range(ITERATIONS):
        paginator, _base = build()
        started = time.perf_counter()
        result, grouped = render(paginator, PER_PAGE)
        samples.append((time.perf_counter() - started) * 1000.0)

    paginator, _base = build()
    statements = count_queries(lambda: render(paginator, PER_PAGE))
    rows = sum(len(group["results"]) for group in grouped.values())
    print(
        f"python: group_by={GROUP_BY} per_page={PER_PAGE} issues={result.hits} "
        f"groups={len(grouped)} page_rows={rows}"
    )
    print(f"python: statements per page = {statements}")
    print(
        f"python: p50 = {statistics.median(samples):.2f} ms, "
        f"p95 = {statistics.quantiles(samples, n=20)[18]:.2f} ms "
        f"over {ITERATIONS} runs"
    )


if __name__ == "__main__":
    main()
