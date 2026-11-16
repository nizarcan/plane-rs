# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""The issue-list query, assembled exactly as the views assemble it.

This mirrors ``IssueViewSet.list`` (``plane/app/views/issue/base.py:267``):

    get_queryset() -> filter_queryset(rich filters) -> legacy issue_filters
    -> apply_annotations -> order_issue_queryset -> issue_queryset_grouper
    -> paginate(...)

The oracle uses it to produce goldens; ``bench.py`` uses it to time the Python
side. Keeping the assembly in one place means the oracle never silently drifts
from the view.
"""

from django.db.models import Count, OuterRef, Q, Subquery, UUIDField, Value
from django.db.models.functions import Coalesce
from django.contrib.postgres.aggregates import ArrayAgg
from django.contrib.postgres.fields import ArrayField

from plane.db.models import (
    CycleIssue,
    FileAsset,
    Issue,
    IssueAssignee,
    IssueLabel,
    IssueLink,
    ModuleIssue,
)


def count_filter() -> Q:
    """``count_filter`` as the two grouped views build it.

    ``plane/app/views/issue/base.py:359`` and ``plane/app/views/cycle/issue.py:176``.
    """
    return Q(
        Q(issue_intake__status=1) | Q(issue_intake__status=-1) | Q(issue_intake__status=2) | Q(issue_intake__isnull=True),
        archived_at__isnull=True,
        is_draft=False,
    )


def apply_annotations(issues):
    """``IssueViewSet.apply_annotations`` (``plane/app/views/issue/base.py:227``)."""
    return (
        issues.annotate(
            cycle_id=Subquery(
                CycleIssue.objects.filter(issue=OuterRef("id"), deleted_at__isnull=True).values("cycle_id")[:1]
            )
        )
        .annotate(
            link_count=Subquery(
                IssueLink.objects.filter(issue=OuterRef("id")).values("issue").annotate(count=Count("id")).values("count")
            )
        )
        .annotate(
            attachment_count=Subquery(
                FileAsset.objects.filter(
                    issue_id=OuterRef("id"),
                    entity_type=FileAsset.EntityTypeContext.ISSUE_ATTACHMENT,
                )
                .values("issue_id")
                .annotate(count=Count("id"))
                .values("count")
            )
        )
        .annotate(
            sub_issues_count=Subquery(
                Issue.issue_objects.filter(parent=OuterRef("id"))
                .values("parent")
                .annotate(count=Count("id"))
                .values("count")
            )
        )
    )


def base_queryset(workspace_slug, project_id=None, distinct=True):
    """``IssueViewSet.get_queryset`` (``plane/app/views/issue/base.py:219``)."""
    issues = Issue.issue_objects.filter(workspace__slug=workspace_slug)
    if project_id is not None:
        issues = issues.filter(project_id=project_id)
    if distinct:
        issues = issues.distinct()
    return issues


def apply_endpoint_annotations(issues):
    """`IssuePaginatedViewSet.get_queryset`'s id annotations.

    Different from `issue_queryset_grouper`'s: `assignee_ids` only counts
    assignees with an active project membership
    (`plane/app/views/issue/base.py:939-950`).
    """
    empty = Value([], output_field=ArrayField(UUIDField()))
    return issues.annotate(
        label_ids=Coalesce(
            Subquery(
                IssueLabel.objects.filter(issue_id=OuterRef("pk"))
                .values("issue_id")
                .annotate(arr=ArrayAgg("label_id", distinct=True))
                .values("arr")
            ),
            empty,
        ),
        assignee_ids=Coalesce(
            Subquery(
                IssueAssignee.objects.filter(
                    issue_id=OuterRef("pk"), assignee__member_project__is_active=True
                )
                .values("issue_id")
                .annotate(arr=ArrayAgg("assignee_id", distinct=True))
                .values("arr")
            ),
            empty,
        ),
        module_ids=Coalesce(
            Subquery(
                ModuleIssue.objects.filter(issue_id=OuterRef("pk"), module__archived_at__isnull=True)
                .values("issue_id")
                .annotate(arr=ArrayAgg("module_id", distinct=True))
                .values("arr")
            ),
            empty,
        ),
    )
