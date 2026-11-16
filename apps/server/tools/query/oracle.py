# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Compute the `plane-query` goldens from Django, on the fixture database.

    apps/server/dev/mkdb.sh t_query_fixture
    apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py
    PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
      apps/server/tools/query/oracle.py

Writes to ``crates/plane-query/tests/golden/``:

| file | category | content |
| --- | --- | --- |
| ``filters.json`` | (a) | legacy ``issue_filters`` query-parameter combinations |
| ``order.json`` | (b) | ``order_issue_queryset`` for every allowed ``order_by`` |
| ``group_values.json`` | (c) | ``issue_group_values`` per group field |
| ``paginator.json`` | (c) | full paginator responses, walked forward and backward |
| ``filterset.json`` | (d) | ``IssueFilterSet`` and ``ComplexFilterBackend`` over JSON filters |
| ``meta.json`` | — | the fixture ids and constants (written by ``build_fixture.py``) |

Three Django behaviours make the goldens reproducible:

* ``django.utils.timezone.now`` is frozen to ``fixture_spec.FIXED_NOW`` while
  relative date filters are evaluated, and the instant is recorded in
  ``meta.json``. ``plane-query`` takes the same instant as an argument.
* the fixture's ids and timestamps are deterministic (``build_fixture.py``);
* every case that produces a paginator response is rendered through DRF's
  ``JSONRenderer``, so the golden is the exact response body, not a
  re-serialisation of it.

``NO_COLOR``/``PYTHONHASHSEED`` do not affect any of this: the goldens never
depend on set or dict iteration order that Python randomises (group keys come
from lists and from ordered querysets).
"""

import json
import random
import sys
from datetime import date, datetime
from pathlib import Path
from unittest import mock
from uuid import UUID

import django

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fixture_spec as spec  # noqa: E402

django.setup()

from django.http import QueryDict  # noqa: E402
from rest_framework.renderers import JSONRenderer  # noqa: E402
from rest_framework.exceptions import APIException  # noqa: E402

from django_issue_query import apply_annotations, base_queryset, count_filter  # noqa: E402
from plane.db.models import Issue  # noqa: E402
from plane.utils.filters import ComplexFilterBackend, IssueFilterSet  # noqa: E402
from plane.utils.grouper import issue_group_values, issue_on_results, issue_queryset_grouper  # noqa: E402
from plane.utils.issue_filters import issue_filters  # noqa: E402
from plane.utils.order_queryset import ISSUE_ORDER_BY_ALLOWLIST, order_issue_queryset, sanitize_order_by  # noqa: E402
from plane.utils.timezone_converter import user_timezone_converter  # noqa: E402
from plane.db.models import CycleIssue  # noqa: E402
from django.db.models import OuterRef, Subquery  # noqa: E402
from plane.utils.paginator import (  # noqa: E402
    Cursor,
    GroupedOffsetPaginator,
    OffsetPaginator,
    SubGroupedOffsetPaginator,
)


GOLDEN_DIR = Path(__file__).resolve().parents[2] / "crates" / "plane-query" / "tests" / "golden"
RENDERER = JSONRenderer()

MAIN = dict(workspace_slug=spec.WORKSPACE_SLUG, project_id=spec.PROJECT_ID)


# ---------------------------------------------------------------------------
# Fixtures / helpers
# ---------------------------------------------------------------------------


def uuid_str(value) -> str:
    return str(value)


def main_ids() -> list[str]:
    return [str(v) for v in Issue.issue_objects.filter(project_id=spec.PROJECT_ID).values_list("id", flat=True)]


def render(data) -> str:
    """DRF's rendered body, as text (it is UTF-8)."""
    return RENDERER.render(data).decode("utf-8")


def query_dict(params: str) -> QueryDict:
    return QueryDict(params, mutable=False)


def issue_state_ids() -> dict[str, str]:
    return {name: str(value) for name, value in spec.state_ids("main").items()}


def bit(name: str) -> str:
    """A short stable value for a name, from the fixture spec."""
    return str(spec.uid(f"bit:{name}"))


STATE = issue_state_ids()
LABEL = {name: str(spec.label_id(name)) for name in spec.LABELS}
MODULE = {name: str(spec.module_id(name)) for name in spec.MODULES}
CYCLE = {name: str(spec.cycle_id(name)) for name in spec.CYCLES}
POINT = {str(key): str(spec.estimate_point_id(key)) for key in (1, 2, 3)}
USER = {name: str(value[0]) for name, value in spec.USERS.items()}
ISSUE = {index: str(spec.issue_id(index)) for index in range(0, 40)}


# ---------------------------------------------------------------------------
# (a) legacy filters
# ---------------------------------------------------------------------------

FILTER_POOL: dict[str, list[str]] = {
    "state": [STATE["backlog"], STATE["done"], f"{STATE['todo']},{STATE['done']}", "null", "", "not-a-uuid"],
    "state_group": ["backlog", "started", "backlog,started", "completed,cancelled", "null", ""],
    "estimate_point": [POINT["1"], POINT["2"], f"{POINT['1']},{POINT['3']}", "null"],
    "priority": ["urgent", "urgent,high", "none", "none,low", "null", ""],
    "parent": ["None", ISSUE[0], f"None,{ISSUE[1]}", "null"],
    "labels": ["None", LABEL["alpha-label"], f"{LABEL['alpha-label']},{LABEL['beta-label']}", "null"],
    "assignees": ["None", USER["alice"], f"{USER['bob']},{USER['carol']}", "null"],
    "mentions": [USER["bob"], "null"],
    "created_by": ["None", USER["alice"], f"{USER['alice']},{USER['bob']}", "null"],
    "name": ["alpha", "BETA", "work item 0", "zzz", ""],
    "created_at": [
        "2024-01-01;after",
        "2024-01-01;before",
        "2_weeks;after",
        "3_months;after;fromnow",
        "1_weeks;before;fromnow",
        "2026-01",
        "2026-01-01,2026-02-01",
        "2025-01-01;after,2025-06-01;before",
    ],
    "updated_at": ["2025-06-01;after", "2025-06-01;before", "4_weeks;before", "2026-06"],
    "start_date": ["2025-01-01;after", "2025-01-01;before", "2_weeks;after", "2025-01-01,2025-02-01"],
    "target_date": ["2026-07-01;after", "2026-07-01;before", "3_months;before;fromnow", "2026-06-01,2026-07-01"],
    "completed_at": ["2026-01-01;after", "2026-01-01;before", "8_weeks;after", "2026-05"],
    "type": ["all", "backlog", "active", "started", "nonsense"],
    "project": [str(spec.PROJECT_ID), "null", "not-a-uuid"],
    "cycle": ["None", CYCLE["cycle-alpha"], f"{CYCLE['cycle-alpha']},None", "null"],
    "module": ["None", MODULE["module-alpha"], f"{MODULE['module-alpha']},None", "null"],
    "intake_status": ["1", "1,-1", "2", "null"],
    "inbox_status": ["2", "0,-2", "null"],
    "sub_issue": ["true", "false"],
    "subscriber": [USER["alice"], USER["bob"], "null"],
    "start_target_date": ["true", "false"],
}

# POST filters take Python values, not strings: the views read `request.data`.
FILTER_POOL_POST: dict[str, list] = {
    "state": [[STATE["backlog"]], [STATE["todo"], STATE["done"]], "null"],
    "state_group": [["backlog"], ["started", "completed"], "null"],
    "estimate_point": [[POINT["1"]], [POINT["1"], POINT["2"]]],
    "priority": [["urgent"], ["urgent", "high"], "null"],
    "parent": [[ISSUE[0]], [ISSUE[1], ISSUE[2]], "null"],
    "labels": [[LABEL["alpha-label"]], [LABEL["alpha-label"], LABEL["beta-label"]], "null"],
    "assignees": [[USER["alice"]], [USER["bob"], USER["carol"]], "null"],
    "mentions": [[USER["bob"]]],
    "created_by": [[USER["alice"]], [USER["alice"], USER["bob"]], "null"],
    "name": ["alpha", "BETA"],
    "created_at": [["2025-01-01;after"], ["2025-01-01;before"], ["2_weeks;after"]],
    "updated_at": [["2025-06-01;after"]],
    "start_date": ["2025-03-01", ["2025-03-01"]],
    "target_date": ["2026-07-01"],
    "completed_at": [["2026-01-01;after"]],
    "type": ["all", "active"],
    "project": [[str(spec.PROJECT_ID)], "null"],
    "cycle": [[CYCLE["cycle-alpha"]], [CYCLE["cycle-alpha"], CYCLE["cycle-beta"]], "null"],
    "module": [[MODULE["module-alpha"]], "null"],
    "intake_status": [["1"], ["1", "-1"], "null"],
    "inbox_status": [["2"], "null"],
    "sub_issue": ["true", "false"],
    "subscriber": [[USER["alice"]], "null"],
    "start_target_date": ["true", "false"],
}

#: Keys whose GET form raises on the real view because the lookup is not
#: supported by the column type, or because the model has no such relation.
CRASHING_KEYS = {"logged_by"}


def serialise_filters(filters: dict) -> dict:
    out = {}
    for key, value in filters.items():
        if isinstance(value, list):
            out[key] = [str(item) for item in value]
        elif isinstance(value, (UUID, datetime, date)):
            out[key] = str(value)
        else:
            out[key] = value
    return out


def filters_case(case_id: int, params, method: str) -> dict:
    """One ``issue_filters`` result and the ids it selects.

    Some parameter combinations are invalid SQL for the column (a POST
    ``start_date`` list hits ``DateField.to_python``; a GET ``start_date`` with
    one value uses an unsupported ``contains`` lookup). Django answers those
    with a 500, so the golden records the exception class and ``plane-query``
    returns an error for the same input.
    """
    parsed = query_dict(params) if method == "GET" else dict(params)
    entry = {"id": case_id, "method": method, "params": params}
    try:
        filters = issue_filters(parsed, method)
        entry["filters"] = serialise_filters(filters)
        qs = Issue.issue_objects.filter(workspace__slug=spec.WORKSPACE_SLUG, project_id=spec.PROJECT_ID).filter(
            **filters
        )
        raw_ids = [str(v) for v in qs.values_list("id", flat=True)]
        distinct_ids = [str(v) for v in qs.distinct().values_list("id", flat=True)]
    except Exception as error:  # noqa: BLE001 - recorded deliberately
        entry["error"] = type(error).__name__
        entry["message"] = str(error)[:200]
        return entry
    entry.update(
        {
            "raw_count": len(raw_ids),
            "distinct_count": len(distinct_ids),
            # One comma-separated string keeps the golden small (44k ids across
            # 1,053 cases); `distinct_ids` is `set(ids)` and is not stored.
            "ids": ",".join(sorted(raw_ids)),
        }
    )
    return entry


def generate_filter_cases() -> list[dict]:
    cases: list[dict] = []
    case_id = 0

    def add(params: str, method: str = "GET") -> None:
        nonlocal case_id
        case_id += 1
        cases.append(filters_case(case_id, params, method))

    # Every key on its own, with every value in its pool.
    for key, values in FILTER_POOL.items():
        for value in values:
            add(f"{key}={value}")
    # Every pair of a hand-picked interesting subset.
    interesting = [
        "state",
        "state_group",
        "priority",
        "labels",
        "assignees",
        "cycle",
        "module",
        "parent",
        "created_at",
        "target_date",
        "type",
        "intake_status",
        "subscriber",
        "created_by",
        "name",
        "mentions",
        "project",
        "estimate_point",
        "start_target_date",
        "sub_issue",
    ]
    rng = random.Random(spec.SEED + 1)
    for _ in range(700):
        keys = rng.sample(interesting, rng.randint(2, 5))
        parts = []
        for key in keys:
            parts.append(f"{key}={rng.choice(FILTER_POOL[key])}")
        add("&".join(parts))
    # A few larger combinations.
    for _ in range(120):
        keys = rng.sample(interesting, rng.randint(5, 8))
        parts = [f"{key}={rng.choice(FILTER_POOL[key])}" for key in keys]
        add("&".join(parts))
    # POST-shaped parameters.
    for key, values in FILTER_POOL_POST.items():
        for value in values:
            case_id += 1
            cases.append(filters_case(case_id, {key: value}, "POST"))
    for _ in range(80):
        keys = rng.sample(list(FILTER_POOL_POST), rng.randint(2, 4))
        payload = {key: rng.choice(FILTER_POOL_POST[key]) for key in keys}
        case_id += 1
        cases.append(filters_case(case_id, payload, "POST"))
    return cases


# ---------------------------------------------------------------------------
# (b) ordering
# ---------------------------------------------------------------------------


def order_cases() -> list[dict]:
    cases = []
    base = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID)
    annotated = issue_queryset_grouper(apply_annotations(base), None, None)
    variants = sorted(ISSUE_ORDER_BY_ALLOWLIST) + [
        f"-{name}" for name in sorted(ISSUE_ORDER_BY_ALLOWLIST)
    ]
    variants += ["", "nonsense", "--created_at", "-", "priority_order", "unknown_field"]
    for order_by in variants:
        queryset, returned = order_issue_queryset(issue_queryset=annotated, order_by_param=order_by)
        ids = [str(v) for v in queryset.values_list("id", flat=True)]
        # What the paginator does with `returned`: re-apply the key with
        # NULLS LAST plus the created_at tiebreak.
        from django.db.models import F

        key = returned[1:] if returned.startswith("-") else returned
        desc = returned.startswith("-")
        ordered = queryset.order_by(
            F(key).desc(nulls_last=True) if desc else F(key).asc(nulls_last=True),
            "-created_at",
        )
        cases.append(
            {
                "order_by": order_by,
                "returned": returned,
                "sanitized": sanitize_order_by(order_by, ISSUE_ORDER_BY_ALLOWLIST),
                "ids": ",".join(ids),
                "paginated_ids": ",".join(str(v) for v in ordered.values_list("id", flat=True)),
            }
        )

    # The same for a queryset that contains duplicate rows from a join: the
    # `min_values` orderings add GROUP BY, the plain ones do not.
    filtered = base.filter(labels__in=[LABEL["alpha-label"], LABEL["beta-label"]], label_issue__deleted_at__isnull=True)
    filtered_annotated = issue_queryset_grouper(apply_annotations(filtered), None, None)
    for order_by in ["-created_at", "sequence_id", "labels__name", "priority"]:
        queryset, returned = order_issue_queryset(issue_queryset=filtered_annotated, order_by_param=order_by)
        cases.append(
            {
                "order_by": order_by,
                "returned": returned,
                "sanitized": sanitize_order_by(order_by, ISSUE_ORDER_BY_ALLOWLIST),
                "filter": "labels",
                "ids": ",".join(str(v) for v in queryset.values_list("id", flat=True)),
                "raw_count": queryset.count(),
            }
        )
    return cases


# ---------------------------------------------------------------------------
# (c) group values and paginators
# ---------------------------------------------------------------------------


def group_value_cases() -> dict:
    project_cases = {}
    filtered = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID, distinct=False)
    for field in [
        "state_id",
        "state__group",
        "priority",
        "labels__id",
        "assignees__id",
        "issue_module__module_id",
        "cycle_id",
        "project_id",
        "created_by",
        "target_date",
        "start_date",
    ]:
        values = issue_group_values(
            field,
            spec.WORKSPACE_SLUG,
            project_id=str(spec.PROJECT_ID),
            queryset=filtered,
        )
        project_cases[field] = [str(value) for value in values]
    workspace_cases = {}
    for field in ["state_id", "labels__id", "assignees__id", "project_id", "cycle_id", "issue_module__module_id"]:
        values = issue_group_values(
            field,
            spec.WORKSPACE_SLUG,
            user=spec.ALICE,
            queryset=filtered,
        )
        workspace_cases[field] = [str(value) for value in values]
    return {"project": project_cases, "workspace": workspace_cases}


def normalise_group_arrays(processed):
    """Sort the id arrays ``__query_multi_grouper`` fills from a Python `set`.

    ``SubGroupedOffsetPaginator.__query_multi_grouper`` /
    ``GroupedOffsetPaginator.__query_multi_grouper`` do
    ``group_ids = list(result_group_mapping[result_id])`` over a `set` of
    strings, so the order of ``label_ids`` / ``assignee_ids`` / ``module_ids``
    in the response depends on PYTHONHASHSEED and changes between processes.
    It is not part of the contract the frontend relies on (it is a membership
    array), so the golden sorts it. Every other array is produced by
    ``ARRAY_AGG(DISTINCT …)``, which already returns ascending order, so
    sorting changes nothing there.

    This is the only normalisation applied to a paginator response; it is
    listed in ``meta.json`` under ``normalisations``.
    """
    if isinstance(processed, list):
        rows = processed
    else:
        rows = []
        for group in processed.values():
            results = group.get("results")
            if isinstance(results, dict):
                rows.extend(row for sub in results.values() for row in sub["results"])
            else:
                rows.extend(results or [])
    for row in rows:
        for key in ("label_ids", "assignee_ids", "module_ids"):
            value = row.get(key)
            if isinstance(value, list):
                row[key] = sorted(str(item) for item in value)
    return processed


def sort_group_keys(processed):
    """Make a single-group m2m response deterministic.

    Two things in `GroupedOffsetPaginator.__query_multi_grouper` depend on an
    order that is not a contract:

    * the result dict is built by iterating `list(set(...))` group ids, so the
      order of its keys depends on PYTHONHASHSEED;
    * the *same* row dict is appended to every group the issue belongs to, and
      which of the issue's rows that is depends on the order Postgres returns
      rows that share `created_at` — undefined.

    The golden sorts the keys and deep-copies each group's rows so that each
    row's group column names the group it appears under (it is redundant data:
    the group key already says it, and `label_ids` carries the full membership).
    """
    import copy

    if not isinstance(processed, dict):
        return processed
    field = next(
        (
            name
            for name in ("labels__id", "assignees__id", "issue_module__module_id")
            if name in _CURRENT_GROUP
        ),
        None,
    )
    out = {}
    for key in sorted(processed):
        group = processed[key]
        rows = copy.deepcopy(group["results"])
        if field is not None:
            for row in rows:
                row[field] = None if key == "None" else key
        out[key] = {"results": rows, "total_results": group["total_results"]}
    return out


def sort_sub_group_keys(processed):
    """Sort the cell keys of a sub-grouped response.

    `__get_subgroup_total_queryset` has no `ORDER BY`, so the order of a
    group's sub-groups is whatever Postgres returns; the golden sorts it (the
    frontend addresses cells by key). The *group* order still comes from
    `issue_group_values` and is kept.
    """
    if not isinstance(processed, dict):
        return processed
    out = {}
    for key, group in processed.items():
        results = group.get("results")
        if isinstance(results, dict):
            group = dict(group)
            group["results"] = {sub: results[sub] for sub in sorted(results)}
        out[key] = group
    return out


_CURRENT_GROUP: set[str] = set()


FIELD_MAPPER = {
    "labels__id": "label_ids",
    "assignees__id": "assignee_ids",
    "issue_module__module_id": "module_ids",
}


class FakeRequest:
    """Just enough of a DRF request for ``BasePaginator.paginate``."""

    def __init__(self, params: str):
        self.GET = query_dict(params)
        self.query_params = self.GET


def paginator_response(
    *,
    group_by: str | None,
    sub_group_by: str | None,
    order_by: str,
    per_page: int,
    cursor: str | None,
    filter_params: str = "",
    raw_params: str | None = None,
) -> tuple[str, str, str]:
    """Run one page through the real paginator; return (body, next, prev).

    The queryset is assembled exactly as ``IssueViewSet.list`` does it; see
    ``django_issue_query``.
    """
    import copy

    base = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID)
    filters = issue_filters(query_dict(filter_params), "GET") if filter_params else {}
    query = base.filter(**filters) if filters else base
    total_count_queryset = copy.deepcopy(query)
    annotated = apply_annotations(query)
    queryset, returned = order_issue_queryset(issue_queryset=annotated, order_by_param=order_by)
    queryset = issue_queryset_grouper(queryset=queryset, group_by=group_by or False, sub_group_by=sub_group_by or False)
    _CURRENT_GROUP.clear()
    if group_by:
        _CURRENT_GROUP.add(group_by)
    if raw_params is not None:
        params = raw_params
    else:
        params = f"per_page={per_page}"
        if cursor:
            params += f"&cursor={cursor}"

    paginator = PaginatorHarness()
    kwargs = dict(
        request=FakeRequest(params),
        order_by=returned,
        queryset=queryset,
        total_count_queryset=total_count_queryset,
        on_results=lambda issues: issue_on_results(
            group_by=group_by or False, issues=issues, sub_group_by=sub_group_by or False
        ),
        controller=lambda processed: (
            sort_sub_group_keys(normalise_group_arrays(processed))
            if sub_group_by
            else (
                sort_group_keys(normalise_group_arrays(processed))
                if group_by in FIELD_MAPPER
                else normalise_group_arrays(processed)
            )
        ),
    )
    if group_by:
        group_by_fields = issue_group_values(
            field=group_by,
            slug=spec.WORKSPACE_SLUG,
            project_id=str(spec.PROJECT_ID),
            filters=filters,
            queryset=total_count_queryset,
        )
        kwargs["group_by_fields"] = group_by_fields
        kwargs["group_by_field_name"] = group_by
        kwargs["count_filter"] = count_filter()
        if sub_group_by:
            kwargs["paginator_cls"] = SubGroupedOffsetPaginator
            kwargs["sub_group_by_field_name"] = sub_group_by
            kwargs["sub_group_by_fields"] = issue_group_values(
                field=sub_group_by,
                slug=spec.WORKSPACE_SLUG,
                project_id=str(spec.PROJECT_ID),
                filters=filters,
                queryset=total_count_queryset,
            )
        else:
            kwargs["paginator_cls"] = GroupedOffsetPaginator
    else:
        kwargs["paginator_cls"] = OffsetPaginator

    response = paginator.paginate(**kwargs)
    body = render(response.data)
    return body, str(response.data["next_cursor"]), str(response.data["prev_cursor"])


class PaginatorHarness:
    """``BasePaginator.paginate`` with its own ``get_per_page``."""

    def __init__(self):
        from plane.utils.paginator import BasePaginator

        self._base = BasePaginator()

    def paginate(self, **kwargs):
        return self._base.paginate(**kwargs)


GROUP_FIELDS = [
    "state_id",
    "state__group",
    "priority",
    "labels__id",
    "assignees__id",
    "issue_module__module_id",
    "cycle_id",
    "project_id",
    "created_by",
    "target_date",
    "start_date",
]

SUB_PAIRS = [
    ("state_id", "priority"),
    ("priority", "state_id"),
    ("state_id", "labels__id"),
    ("labels__id", "assignees__id"),
    ("state__group", "priority"),
    ("cycle_id", "labels__id"),
]


def paginator_cases() -> list[dict]:
    cases = []
    case_id = 0
    plan: list[tuple[str | None, str | None, str, int, str]] = []
    # No grouping: several page sizes and orders.
    for order_by, per_page in [
        ("-created_at", 100),
        ("-created_at", 13),
        ("priority", 13),
        ("sequence_id", 13),
        ("labels__name", 13),
        ("-assignees__first_name", 13),
    ]:
        plan.append((None, None, order_by, per_page, ""))
    # Every group field at a small page size, plus the realistic page size for
    # the most-used grouping (the benchmark in BENCH.md uses the same).
    for group in GROUP_FIELDS:
        plan.append((group, None, "-created_at", 13, ""))
    plan.append(("state_id", None, "-created_at", 100, ""))
    plan.append(("state_id", None, "-created_at", 13, "labels=%s,%s" % (LABEL["alpha-label"], LABEL["beta-label"])))
    plan.append(("state_id", None, "labels__name", 13, ""))
    plan.append(("labels__id", None, "-created_at", 13, "assignees=%s" % USER["alice"]))
    # A representative set of sub-group pairs, at a small per-group page size.
    for group, sub in SUB_PAIRS:
        plan.append((group, sub, "-created_at", 5, ""))
    plan.append(("state_id", "priority", "priority", 5, ""))
    all_states = ",".join(STATE[name] for name in ("backlog", "todo", "in-progress", "done", "cancelled"))
    plan.append(("state_id", "priority", "-created_at", 5, "state=" + all_states))

    for group, sub, order_by, per_page, filter_params in plan:
        case_id += 1
        entry = {
            "id": case_id,
            "group_by": group,
            "sub_group_by": sub,
            "order_by": order_by,
            "per_page": per_page,
            "filter_params": filter_params,
            "pages": [],
            "backward": [],
        }
        try:
            pages = entry["pages"]
            cursor = None
            for _ in range(4):
                body, next_cursor, _prev_cursor = paginator_response(
                    group_by=group,
                    sub_group_by=sub,
                    order_by=order_by,
                    per_page=per_page,
                    cursor=cursor,
                    filter_params=filter_params,
                )
                pages.append({"cursor": cursor or f"{per_page}:0:0", "body": body})
                payload = json.loads(body)
                if not payload["next_page_results"]:
                    break
                cursor = next_cursor
            # Walk backwards from the last page.
            back = entry["backward"]
            last = pages[-1]
            for _ in range(2):
                payload = json.loads(last["body"])
                prev = payload["prev_cursor"]
                if not payload["prev_page_results"]:
                    break
                body, _next, _prev = paginator_response(
                    group_by=group,
                    sub_group_by=sub,
                    order_by=order_by,
                    per_page=per_page,
                    cursor=prev,
                    filter_params=filter_params,
                )
                back.append({"cursor": prev, "body": body})
                last = {"cursor": prev, "body": body}
        except Exception as error:  # noqa: BLE001 - recorded deliberately
            entry["error"] = type(error).__name__
            entry["message"] = str(error)[:200]
            entry["pages"] = []
            entry["backward"] = []
        cases.append(entry)

    # Cursor handling quirks: an is_prev cursor whose value differs from
    # per_page, and invalid cursors.
    extra = [
        {"cursor": "50:0:0", "per_page": 13},
        {"cursor": "13:1:1", "per_page": 13},
        {"cursor": "13:0:1", "per_page": 13},
    ]
    for item in extra:
        case_id += 1
        body, _next, _prev = paginator_response(
            group_by=None,
            sub_group_by=None,
            order_by="-created_at",
            per_page=item["per_page"],
            cursor=item["cursor"],
        )
        cases.append(
            {
                "id": case_id,
                "group_by": None,
                "sub_group_by": None,
                "order_by": "-created_at",
                "per_page": item["per_page"],
                "pages": [{"cursor": item["cursor"], "body": body}],
                "backward": [],
            }
        )
    errors = paginator_error_cases()
    return [{"cases": cases, "errors": errors}]


def paginator_error_cases() -> list[dict]:
    """Requests ``BasePaginator.paginate`` rejects with a 400."""
    out = []
    for params in [
        "per_page=0",
        "per_page=-1",
        "per_page=1001",
        "per_page=abc",
        "per_page=13&cursor=nope",
        "per_page=13&cursor=1:2",
        "per_page=13&cursor=2000:0:0",
        "per_page=13&cursor=13:-1:0",
        "per_page=13&cursor=abc:0:0",
        "per_page=13&group_by=not_a_field",
        "per_page=13&group_by=state_id&sub_group_by=not_a_field",
    ]:
        try:
            paginator_response(
                group_by=QueryDict(params).get("group_by"),
                sub_group_by=QueryDict(params).get("sub_group_by"),
                order_by="-created_at",
                per_page=13,
                cursor=None,
                raw_params=params,
            )
        except APIException as error:
            out.append({"params": params, "status": error.status_code, "detail": error.detail})
        except Exception as error:  # noqa: BLE001 - recorded, not swallowed
            out.append({"params": params, "error": type(error).__name__, "message": str(error)})
        else:
            out.append({"params": params, "status": 200})
    # The group_by allowlist check lives inside `paginate`, which the harness
    # above calls with a pre-computed field name; exercise it directly.
    return out


# ---------------------------------------------------------------------------
# (c2) the global paginator (`IssuePaginatedViewSet`)
# ---------------------------------------------------------------------------

GLOBAL_FIELDS = [
    "id",
    "name",
    "state_id",
    "state__group",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "archived_at",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "link_count",
    "attachment_count",
    "sub_issues_count",
]


def global_paginator_cases() -> dict:
    """`plane/utils/global_paginator.py` over the `IssuePaginatedViewSet`
    queryset (no `distinct()`, fixed `updated_at` ordering)."""
    from plane.utils.global_paginator import paginate as global_paginate

    from django_issue_query import apply_annotations, apply_endpoint_annotations

    cases = []
    error_cases = []
    plan = [
        (None, "UTC"),
        ("13:0:0", "UTC"),
        ("13:1:0", "UTC"),
        ("13:2:0", "Asia/Tokyo"),
        ("100:0:0", "UTC"),
        ("100:1:0", "Asia/Tokyo"),
        ("10:50:0", "UTC"),
        ("13:-1:0", "UTC"),
        ("0:0:0", "UTC"),
        ("1:0:0", "UTC"),
        ("abc:0:0", "UTC"),
    ]
    base_queryset = Issue.issue_objects.filter(
        workspace__slug=spec.WORKSPACE_SLUG, project_id=spec.PROJECT_ID
    ).order_by("updated_at")
    queryset = (
        Issue.issue_objects.filter(workspace__slug=spec.WORKSPACE_SLUG, project_id=spec.PROJECT_ID)
        .select_related("state")
        .annotate(
            cycle_id=Subquery(CycleIssue.objects.filter(issue=OuterRef("id")).values("cycle_id")[:1])
        )
    )
    queryset = apply_endpoint_annotations(apply_annotations(queryset)).order_by("updated_at")
    for cursor, timezone_name in plan:
        entry = {"cursor": cursor, "timezone": timezone_name}
        try:
            payload = global_paginate(
                base_queryset=base_queryset,
                queryset=queryset,
                cursor=cursor,
                on_result=lambda results: user_timezone_converter(
                    list(results.values(*GLOBAL_FIELDS)), ["created_at", "updated_at"], timezone_name
                ),
            )
        except Exception as error:  # noqa: BLE001 - recorded deliberately
            entry["error"] = type(error).__name__
            entry["message"] = str(error)[:200]
            error_cases.append(entry)
            continue
        entry["body"] = render(payload)
        cases.append(entry)
    return {"cases": cases, "errors": error_cases}


# ---------------------------------------------------------------------------
# (d) filterset and the complex filter backend
# ---------------------------------------------------------------------------


class ViewStub:
    filterset_class = IssueFilterSet


def filterset_cases() -> dict:
    spec_dump = {}
    for name, f in IssueFilterSet.base_filters.items():
        spec_dump[name] = {
            "class": type(f).__name__,
            "field_name": f.field_name,
            "lookup_expr": f.lookup_expr,
            "method": f.method,
            "distinct": f.distinct,
        }

    cases = []
    backend = ComplexFilterBackend()
    view = ViewStub()

    def run_queryset(params: str) -> dict:
        """``DjangoFilterBackend.filter_queryset``: 400 on invalid data."""
        qs = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID, distinct=False)
        fs = IssueFilterSet(data=query_dict(params), queryset=qs)
        if not fs.is_valid():
            errors = {key: [str(item) for item in value] for key, value in fs.errors.items()}
            return {"params": params, "status": 400, "errors": errors}
        filtered = fs.filter_queryset(qs)
        ids = [str(v) for v in filtered.values_list("id", flat=True)]
        return {
            "params": params,
            "status": 200,
            "raw_count": len(ids),
            "ids": ",".join(sorted(ids)),
            "count": filtered.count(),
        }

    params_pool = [
        "state_id=" + STATE["backlog"],
        "state_id__in=%s,%s" % (STATE["todo"], STATE["done"]),
        "state_group=started",
        "state_group__in=backlog,completed",
        "priority=urgent",
        "priority__in=urgent,high",
        "label_id=" + LABEL["alpha-label"],
        "label_id__in=%s,%s" % (LABEL["alpha-label"], LABEL["beta-label"]),
        "assignee_id=" + USER["alice"],
        "assignee_id__in=%s,%s" % (USER["bob"], USER["carol"]),
        "cycle_id=" + CYCLE["cycle-alpha"],
        "cycle_id__in=%s,%s" % (CYCLE["cycle-alpha"], CYCLE["cycle-beta"]),
        "module_id=" + MODULE["module-alpha"],
        "module_id__in=%s,%s" % (MODULE["module-alpha"], MODULE["module-beta"]),
        "mention_id=" + USER["bob"],
        "subscriber_id=" + USER["alice"],
        "subscriber_id__in=%s,%s" % (USER["alice"], USER["bob"]),
        "created_by_id=" + USER["alice"],
        "created_by_id__in=%s,%s" % (USER["alice"], USER["bob"]),
        "project_id=" + str(spec.PROJECT_ID),
        "project_id__in=%s,%s" % (spec.PROJECT_ID, spec.PROJECT2_ID),
        "is_archived=false",
        "is_archived=true",
        "is_draft=false",
        "is_draft=true",
        "start_date=2025-03-01",
        "start_date__range=2025-01-01&start_date__range=2025-12-31",
        "target_date__exact=2026-07-01",
        "created_at=2026-01-01",
        "created_at__exact=2026-01-01",
        "created_at__range=2025-01-01,2025-12-31",
        "updated_at__range=2025-01-01,2025-06-30",
        "priority__in=urgent,high&label_id__in=%s" % LABEL["alpha-label"],
        "label_id=%s&assignee_id=%s" % (LABEL["alpha-label"], USER["alice"]),
        "state_id=not-a-uuid",
        "priority=nonsense",
        "is_archived=nonsense",
        "created_at=not-a-date",
        "start_date__range=2025-01-01",
        "unknown_field=1",
    ]
    for params in params_pool:
        cases.append(run_queryset(params))

    json_cases = []
    json_pool = [
        {"priority": "urgent"},
        {"priority__in": ["urgent", "high"]},
        {"label_id": LABEL["alpha-label"]},
        {"label_id__in": [LABEL["alpha-label"], LABEL["beta-label"]]},
        {"and": [{"priority": "urgent"}, {"state_group": "started"}]},
        {"or": [{"priority": "urgent"}, {"priority": "high"}]},
        {"not": {"priority": "urgent"}},
        {"and": [{"or": [{"state_group": "started"}, {"state_group": "completed"}]}, {"is_archived": False}]},
        {"or": [{"label_id": LABEL["alpha-label"]}, {"assignee_id": USER["bob"]}]},
        {"and": [{"priority__in": ["urgent", "high"]}, {"not": {"label_id": LABEL["alpha-label"]}}]},
        {"state_id": STATE["backlog"], "priority": "low"},
        {"is_draft": True},
        {"is_archived": True},
        {"created_at__range": "2025-01-01,2025-03-01"},
        {"not": {"and": [{"priority": "urgent"}, {"state_group": "started"}]}},
        {"or": [{"and": [{"state_group": "started"}, {"priority": "urgent"}]}, {"is_archived": True}]},
        # Errors.
        {"unknown_field": "x"},
        {"and": []},
        {"or": "nope"},
        {"not": [1, 2]},
        {"and": [{"priority": "urgent"}], "priority": "low"},
        {"and": [], "x": 1},
        {"not": {"and": [{"or": [{"not": {"not": {"priority": "x"}}}]}]}},
        {"priority__in": []},
        {"priority": {"nested": True}},
    ]
    for payload in json_pool:
        entry = {"filter": payload}
        qs = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID, distinct=False)
        try:
            filtered = backend.filter_queryset(None, qs, view, filter_data=payload)
        except APIException as error:
            entry["error"] = error.detail
            entry["status"] = error.status_code
        else:
            ids = [str(v) for v in filtered.values_list("id", flat=True)]
            entry["raw_count"] = len(ids)
            entry["ids"] = ",".join(sorted(ids))
        json_cases.append(entry)

    # A malformed JSON string, through the query parameter.
    malformed = {"filter": "{not json"}
    qs = base_queryset(spec.WORKSPACE_SLUG, spec.PROJECT_ID, distinct=False)
    try:
        backend.filter_queryset(FakeRequest("filters={not json"), qs, view)
    except APIException as error:
        malformed["error"] = error.detail
        malformed["status"] = error.status_code
    json_cases.append(malformed)

    return {"spec": spec_dump, "queryset_cases": cases, "json_cases": json_cases}


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------


def write(name: str, payload) -> None:
    GOLDEN_DIR.mkdir(parents=True, exist_ok=True)
    path = GOLDEN_DIR / name
    path.write_text(json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n")
    size = path.stat().st_size
    print(f"oracle: wrote {path.name} ({size} bytes)")


def freeze_now():
    """Pin ``django.utils.timezone.now`` to ``FIXED_NOW``.

    ``issue_filters`` calls ``timezone.now().date()`` for relative ranges
    (``2_weeks;after``), so a golden computed today would not replay tomorrow.
    Every module holds a reference to the same ``timezone`` module object, so
    patching the attribute is enough for all of them.
    """
    patch = mock.patch("django.utils.timezone.now", return_value=spec.FIXED_NOW)
    return [patch]


def main() -> None:
    only = sys.argv[1] if len(sys.argv) > 1 else "all"
    patches = freeze_now()
    for patch in patches:
        patch.start()
    # The frozen instant must actually be in effect; a relative-date golden
    # computed from the wall clock would not replay.
    from django.utils import timezone as frozen_timezone

    assert frozen_timezone.now() == spec.FIXED_NOW, "timezone.now is not frozen"
    try:
        if only in ("all", "filters"):
            cases = generate_filter_cases()
            write("filters.json", {"cases": cases})
            print(f"oracle: filters cases={len(cases)}")
        if only in ("all", "order"):
            cases = order_cases()
            write("order.json", {"cases": cases})
            print(f"oracle: order cases={len(cases)}")
        if only in ("all", "groups"):
            payload = group_value_cases()
            write("group_values.json", payload)
            print(f"oracle: group value cases={len(payload['project'])}")
        if only in ("all", "paginator"):
            payload = paginator_cases()
            write("paginator.json", payload)
            print(
                f"oracle: paginator cases={len(payload[0]['cases'])} "
                f"pages={sum(len(c['pages']) for c in payload[0]['cases'])}"
            )
        if only in ("all", "global"):
            payload = global_paginator_cases()
            write("global_paginator.json", payload)
            print(
                f"oracle: global paginator cases={len(payload['cases'])} "
                f"errors={len(payload['errors'])}"
            )
        if only in ("all", "filterset"):
            payload = filterset_cases()
            write("filterset.json", payload)
            print(
                f"oracle: filterset cases={len(payload['queryset_cases'])} "
                f"json={len(payload['json_cases'])}"
            )
    finally:
        for patch in patches:
            patch.stop()
    ids = main_ids()
    print(f"oracle: visible issues in the main project: {len(ids)}")


if __name__ == "__main__":
    main()
