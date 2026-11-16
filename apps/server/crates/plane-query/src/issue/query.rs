// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Assembling the issue-list statement, the way `IssueViewSet.list` does.
//!
//! The order of operations is the specification, because each step can change
//! the row set:
//!
//! ```text
//! Issue.issue_objects.filter(workspace__slug=…, project_id=…)   (manager)
//!   .distinct()                                                 (get_queryset)
//!   .filter(<rich filterset>)                                   (filter_queryset)
//!   .filter(**issue_filters(…))                                 (legacy filters)
//!   .annotate(<cycle/link/attachment/sub-issue subqueries>)     (apply_annotations)
//!   order_issue_queryset(…)                                     (ordering)
//!   issue_queryset_grouper(…)                                   (group joins/arrays)
//! ```
//!
//! Two consequences the port must keep:
//!
//! * **Duplicates.** A filter on a multi-valued relation joins one row per
//!   matching through row. `.distinct()` collapses them for the *page* only
//!   when the projection contains `id` (it always does) — but the grouped
//!   paginators count rows *before* that collapse, which is why a
//!   `group_by=labels__id` page reports `total_count=412` for 225 issues.
//! * **`GROUP BY issues.id`.** An ordering by `labels__name` /
//!   `assignees__first_name` / `issue_module__module__name` adds an aggregate
//!   annotation, which puts the query in `GROUP BY issues.id`. That collapses
//!   duplicate join rows and changes the count, so
//!   [`IssueQuery::grouped_count`] reproduces the grouping.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::error::QueryError;
use crate::issue::filter::{FilterBuilder, FilterParams, LegacyFilters};
use crate::issue::group_values::GroupValues;
use crate::issue::model::{GroupField, Relation, ValueField, manager_predicate};
use crate::issue::order::Ordering;
use crate::issue::rich::RichFilters;
use crate::issue::values;
use crate::json::Json;
use crate::sql::{Frag, JoinKind, Select, Sql, Value};

/// The scope `IssueViewSet.get_queryset` establishes.
#[derive(Debug, Clone)]
pub struct IssueScope {
    pub workspace_slug: String,
    pub project_id: Option<Uuid>,
    /// `get_queryset()` ends in `.distinct()`.
    pub distinct: bool,
    /// The guest restriction the views add after filtering
    /// (`plane/app/views/issue/base.py:321`): guests without
    /// `guest_view_all_features` only see their own issues.
    pub created_by: Option<Uuid>,
}

impl IssueScope {
    pub fn project(workspace_slug: impl Into<String>, project_id: Uuid) -> Self {
        Self {
            workspace_slug: workspace_slug.into(),
            project_id: Some(project_id),
            distinct: true,
            created_by: None,
        }
    }

    pub fn workspace(workspace_slug: impl Into<String>) -> Self {
        Self {
            workspace_slug: workspace_slug.into(),
            project_id: None,
            distinct: true,
            created_by: None,
        }
    }
}

/// A replacement for the `Issue.issue_objects` manager predicate.
///
/// `IssueQuery` hardcodes `Issue.issue_objects`' exclusions (`deleted_at`,
/// triage state, archived, draft, archived project). Three views replace them:
///
/// * the archived-issues list uses `Issue.objects` plus `archived_at IS NOT
///   NULL` and the epic exclusion (`plane/app/views/issue/archive.py:78-86`);
/// * the module/cycle issue lists add their through-table scope as an inner
///   join rather than an `EXISTS` subquery (`plane/app/views/module/issue.py`
///   `get_queryset`, `plane/app/views/cycle/issue.py:107-112`);
/// * the workspace-wide view list adds the caller's membership predicate, a
///   join that can duplicate rows (`plane/app/views/view/base.py:177-197`).
///
/// `predicate` *replaces* `manager_predicate()`; a caller that wants the
/// manager's exclusions plus its own condition ANDs them itself:
/// `IssueScopeFilter::new(manager_predicate().and(extra))`.
#[derive(Debug, Clone, Default)]
pub struct IssueScopeFilter {
    pub predicate: Frag,
    /// Joins the predicate references, required in every statement.
    pub joins: Vec<(Relation, JoinKind)>,
}

impl IssueScopeFilter {
    pub fn new(predicate: Frag) -> Self {
        Self {
            predicate,
            joins: Vec::new(),
        }
    }

    /// Require `relation` in every statement that applies this scope.
    pub fn with_join(mut self, relation: Relation, kind: JoinKind) -> Self {
        self.joins.push((relation, kind));
        self
    }
}

/// Everything one issue-list request needs.
#[derive(Debug, Clone)]
pub struct IssueQuery {
    pub scope: IssueScope,
    pub filters: LegacyFilters,
    pub rich: Option<RichFilters>,
    pub ordering: Ordering,
    pub group_by: Option<GroupField>,
    pub sub_group_by: Option<GroupField>,
    pub timezone: Tz,
}

impl IssueQuery {
    /// The joins and predicate shared by every statement of one request.
    fn build(&self, scope: Option<&IssueScopeFilter>) -> FilterBuilder {
        let mut builder = FilterBuilder::new();

        // The manager's joins come first, in the order Django adds them:
        // states (LEFT), projects (INNER), workspaces (INNER).
        builder.require_left(Relation::States);
        builder.require_inner(Relation::Projects);
        builder.require_inner(Relation::Workspaces);
        match scope {
            Some(scope) => {
                for (relation, kind) in &scope.joins {
                    builder.require(*relation, *kind);
                }
                builder.and(scope.predicate.clone());
            }
            None => builder.and(manager_predicate()),
        }
        if let Some(project_id) = self.scope.project_id {
            builder.and(Frag::compare(
                "issues.project_id",
                "=",
                Value::Uuid(project_id),
            ));
        }
        builder.and(Frag::compare(
            "workspaces.slug",
            "=",
            Value::Text(self.scope.workspace_slug.clone()),
        ));
        if let Some(created_by) = self.scope.created_by {
            builder.and(Frag::compare(
                "issues.created_by_id",
                "=",
                Value::Uuid(created_by),
            ));
        }

        if let Some(rich) = &self.rich {
            for (relation, kind) in &rich.joins {
                builder.require(*relation, *kind);
            }
            builder.and(rich.predicate.clone());
        }

        // Legacy filters, then their date lookups (which need the timezone).
        self.filters.apply(&mut builder);
        self.filters.apply_dates(&mut builder, self.timezone);

        // Ordering annotations.
        for (relation, kind) in self.ordering.joins.entries() {
            builder.require(relation, kind);
        }

        // `issue_queryset_grouper`: an m2m group field filters the through
        // table's `deleted_at` and therefore joins it. Django uses a LEFT OUTER
        // join because the condition is `deleted_at IS NULL`, which also keeps
        // issues with no rows at all.
        for group in [self.group_by, self.sub_group_by].into_iter().flatten() {
            if let Some(relation) = group.relation() {
                builder.require_left(relation);
            }
        }
        builder
    }

    /// The `WHERE` `issue_queryset_grouper` adds for the through tables it
    /// groups by.
    fn grouper_predicate(&self) -> Frag {
        let mut predicate = Frag::empty();
        for group in [self.group_by, self.sub_group_by].into_iter().flatten() {
            let column = match group {
                GroupField::LabelsId => "issue_labels.deleted_at",
                GroupField::AssigneesId => "issue_assignees.deleted_at",
                GroupField::ModuleId => "module_issues.deleted_at",
                _ => continue,
            };
            predicate = predicate.and(Frag::is_null(column));
        }
        predicate
    }

    /// The filtered queryset before annotations and grouping.
    ///
    /// `IssueViewSet.list` keeps a copy of exactly this as
    /// `total_count_queryset` (`plane/app/views/issue/base.py:287`).
    pub fn filtered_base(&self) -> Select {
        self.filtered_base_with(None)
    }

    /// [`IssueQuery::filtered_base`] with a replacement for the manager
    /// predicate.
    pub fn filtered_base_with(&self, scope: Option<&IssueScopeFilter>) -> Select {
        let builder = self.build(scope);
        let mut select = Select::new("issues");
        select.joins = builder.joins.to_joins();
        select.predicate = Some(builder.predicate);
        select.distinct = self.scope.distinct;
        select.projection = vec![Frag::raw("issues.id")];
        select
    }

    /// The grouped paginator's `total_count`: Django's `.count()` on the
    /// *windowed* queryset.
    ///
    /// The inner select is `SELECT DISTINCT <all model columns and
    /// annotations>, ROW_NUMBER() OVER (PARTITION BY <group> ORDER BY <order>)`.
    /// The row number is part of the `DISTINCT`, and two rows of the same issue
    /// under *different* group keys can carry the same row number (each
    /// partition numbers from 1), so the count collapses them — which is why
    /// `group_by=labels__id` reports 412 for 225 issues rather than the 444
    /// joined rows. Reproducing that requires the same window in the count.
    pub fn grouped_count(&self, fields: &[ValueField]) -> Sql {
        self.grouped_count_with(fields, None)
    }

    /// [`IssueQuery::grouped_count`] with a replacement for the manager
    /// predicate.
    ///
    /// The inner `DISTINCT` is the **view's**: `IssueViewSet.list` ends its
    /// `get_queryset()` in `.distinct()`, the cycle/module/archive lists do
    /// not, and with no `DISTINCT` the windowed count is the number of joined
    /// rows (`plane/utils/paginator.py:278`).
    pub fn grouped_count_with(
        &self,
        fields: &[ValueField],
        scope: Option<&IssueScopeFilter>,
    ) -> Sql {
        let mut select = self.filtered_select_with(scope);
        select.distinct = self.scope.distinct;
        select.projection = vec![
            Frag::raw("issues.id"),
            self.window_expression()
                .append(Frag::raw(" AS \"__row_number\"")),
        ];
        if self.ordering.group_by_issue {
            select.group_by = self.group_by_columns(fields);
        }
        select.count_sql(None, None)
    }

    /// The number of rows on a grouped page, as `len(queryset)` sees it.
    ///
    /// The page's rows come from `issue_on_results`' `.values(...)`, whose
    /// `DISTINCT` includes the group column (for an m2m group, the through
    /// row's id) — but the envelope's `count` is `len(CursorResult.results)`,
    /// evaluated on the **model** queryset, whose `DISTINCT` includes only the
    /// model columns plus the row number. Two rows of one issue under
    /// different group keys can share a row number, so the model projection
    /// collapses them and `count` is smaller than the number of result rows
    /// (92 vs 104 in the fixture). Both are computed the way Django computes
    /// them.
    pub fn grouped_page_count(&self, fields: &[ValueField], limit: i64, offset: i64) -> Sql {
        self.grouped_page_count_with(fields, limit, offset, None)
    }

    /// [`IssueQuery::grouped_page_count`] with a replacement for the manager
    /// predicate.
    pub fn grouped_page_count_with(
        &self,
        fields: &[ValueField],
        limit: i64,
        offset: i64,
        scope: Option<&IssueScopeFilter>,
    ) -> Sql {
        let mut inner = self.filtered_select_with(scope);
        inner.distinct = self.scope.distinct;
        inner.projection = vec![
            Frag::raw("issues.id"),
            self.window_expression()
                .append(Frag::raw(" AS \"__row_number\"")),
        ];
        if self.ordering.group_by_issue {
            inner.group_by = self.group_by_columns(fields);
        }
        let inner_frag = inner.to_frag();
        let stop = offset + limit + 1;
        let mut outer = Select::from_frag(Frag {
            sql: format!("({}) AS \"qualify\"", inner_frag.sql),
            params: inner_frag.params,
        });
        outer.projection = vec![Frag::raw("count(*)")];
        outer.predicate = Some(
            Frag::raw(format!("\"__row_number\" > {offset}"))
                .and(Frag::raw(format!("\"__row_number\" < {stop}"))),
        );
        outer.into_sql()
    }

    /// `ROW_NUMBER() OVER (PARTITION BY <group>[, <sub group>] ORDER BY <key>,
    /// created_at DESC)`, shared by the page and its counts.
    fn window_expression(&self) -> Frag {
        let mut window = Frag::raw("ROW_NUMBER() OVER (PARTITION BY ");
        let partitions: Vec<GroupField> = [self.group_by, self.sub_group_by]
            .into_iter()
            .flatten()
            .collect();
        for (index, group) in partitions.iter().enumerate() {
            if index > 0 {
                window.push_raw(", ");
            }
            window.push(group.expression());
        }
        window.push_raw(" ORDER BY ");
        let order = self.ordering.paginator_order();
        for (index, expression) in order.iter().enumerate() {
            if index > 0 {
                window.push_raw(", ");
            }
            window.push(expression.clone());
        }
        window.push_raw(")");
        window
    }

    /// The joins, predicate and grouper predicate for the page queries. Public
    /// so the global paginator can build its own projection and ordering.
    pub fn filtered_select(&self) -> Select {
        self.filtered_select_with(None)
    }

    /// [`IssueQuery::filtered_select`] with a replacement for the manager
    /// predicate.
    pub fn filtered_select_with(&self, scope: Option<&IssueScopeFilter>) -> Select {
        let mut builder = self.build(scope);
        builder.and(self.grouper_predicate());
        let mut select = Select::new("issues");
        select.joins = builder.joins.to_joins();
        select.predicate = Some(builder.predicate);
        select.distinct = self.scope.distinct;
        select
    }

    /// The columns Django adds to `GROUP BY` when an aggregate annotation is
    /// present: the primary key, plus every projected column that belongs to a
    /// joined table (they are not functionally dependent on `issues.id`).
    fn group_by_columns(&self, fields: &[ValueField]) -> Vec<String> {
        let mut columns = vec!["issues.id".to_owned()];
        for field in fields {
            let column = match field {
                ValueField::StateGroup => Some("states.group"),
                ValueField::Group(GroupField::LabelsId) => Some("issue_labels.label_id"),
                ValueField::Group(GroupField::AssigneesId) => Some("issue_assignees.assignee_id"),
                ValueField::Group(GroupField::ModuleId) => Some("module_issues.module_id"),
                _ => None,
            };
            if let Some(column) = column
                && !columns.iter().any(|existing| existing == column)
            {
                columns.push(column.to_owned());
            }
        }
        columns
    }

    /// The plain (non-grouped) paginator's page.
    ///
    /// `OffsetPaginator.get_result` slices the queryset and `issue_on_results`
    /// calls `.values()` on the slice, so the projection is the values list and
    /// `DISTINCT` applies to it. One extra row is fetched so `has_results` needs
    /// no second round trip.
    pub fn page(&self, fields: &[ValueField], limit: i64, offset: i64) -> Select {
        self.page_with(fields, limit, offset, None)
    }

    /// [`IssueQuery::page`] with a replacement for the manager predicate.
    pub fn page_with(
        &self,
        fields: &[ValueField],
        limit: i64,
        offset: i64,
        scope: Option<&IssueScopeFilter>,
    ) -> Select {
        let mut select = self.filtered_select_with(scope);
        select.projection = values::projection(fields);
        if select.distinct {
            // `SELECT DISTINCT` requires the ordering expressions in the select
            // list; the ordering is by ordinal for that reason.
            self.ordering.apply_paginator_order(&mut select, fields);
        } else {
            select.order_by = self.ordering.paginator_order();
        }
        if self.ordering.group_by_issue {
            select.group_by = self.group_by_columns(fields);
        }
        select.limit = Some(limit + 1);
        select.offset = Some(offset);
        select
    }

    /// The grouped paginator's page: a window over the joined rows, filtered on
    /// the row number and ordered by the key.
    ///
    /// Django builds
    /// `SELECT <values> FROM (SELECT DISTINCT <values>, ROW_NUMBER() … ) qualify
    /// WHERE row_number > offset AND row_number < stop`. The `DISTINCT` there is
    /// a no-op because the row number is part of it, so it is omitted, and the
    /// order key is projected (`qual1` in Django's SQL) so the outer query can
    /// order by it after the wrap.
    pub fn grouped_page(
        &self,
        fields: &[ValueField],
        limit: i64,
        offset: i64,
        extra_row: bool,
    ) -> Select {
        self.grouped_page_with(fields, limit, offset, extra_row, None)
    }

    /// [`IssueQuery::grouped_page`] with a replacement for the manager
    /// predicate.
    pub fn grouped_page_with(
        &self,
        fields: &[ValueField],
        limit: i64,
        offset: i64,
        extra_row: bool,
        scope: Option<&IssueScopeFilter>,
    ) -> Select {
        let mut inner = self.filtered_select_with(scope);
        inner.projection = values::projection(fields);

        let window = self.window_expression();
        inner
            .projection
            .push(window.append(Frag::raw(" AS \"__row_number\"")));

        let key = self.ordering.expression.clone();
        let key_is_created_at = key.sql == "issues.created_at";
        let mut order_key = key.clone();
        order_key.push_raw(" AS \"__order_key\"");
        inner.projection.push(order_key);
        if self.ordering.group_by_issue {
            inner.group_by = self.group_by_columns(fields);
        }

        let inner_frag = inner.to_frag();
        let stop = offset + limit + 1;
        let mut outer = Select::from_frag(Frag {
            sql: format!("({}) AS \"qualify\"", inner_frag.sql),
            params: inner_frag.params,
        });
        outer.projection = vec![Frag::raw("*")];
        outer.predicate = Some(
            Frag::raw(format!("\"__row_number\" > {offset}")).and(Frag::raw(format!(
                "\"__row_number\" < {}",
                if extra_row { stop + 1 } else { stop }
            ))),
        );
        outer.order_by = vec![Frag::raw(format!(
            "\"__order_key\" {} NULLS LAST",
            if self.ordering.desc { "DESC" } else { "ASC" }
        ))];
        if !key_is_created_at {
            outer.order_by.push(Frag::raw("\"created_at\" DESC"));
        }
        // No overall LIMIT: the row-number predicate bounds each partition, so
        // a page holds up to `limit` rows per group (Django emits none either).
        outer
    }

    /// The `(group, COUNT(DISTINCT id) FILTER (…))` rows the grouped paginator
    /// reports as `total_results` and derives `total_pages` from.
    ///
    /// `count_filter` (`plane/app/views/issue/base.py:359`):
    /// `intake status in (1, -1, 2) or no intake row`, not archived, not draft.
    /// The intake relation is `LEFT OUTER` because the filter is inside an
    /// aggregate and one branch accepts a missing row.
    pub fn totals(&self, group: GroupField) -> Sql {
        self.totals_with(group, None)
    }

    /// [`IssueQuery::totals`] with a replacement for the manager predicate.
    pub fn totals_with(&self, group: GroupField, scope: Option<&IssueScopeFilter>) -> Sql {
        let mut builder = self.build(scope);
        builder.and(self.grouper_predicate());
        builder.require_left(Relation::IntakeIssues);
        let mut select = Select::new("issues");
        select.joins = builder.joins.to_joins();
        select.predicate = Some(builder.predicate);
        select.distinct = true;
        let mut projection = group.expression();
        projection.push_raw(" AS \"__group\"");
        select.projection = vec![
            projection,
            Frag::raw(
                "COUNT(DISTINCT issues.id) FILTER (WHERE \
                 (intake_issues.status IN (1, -1, 2) OR intake_issues.id IS NULL) \
                 AND issues.archived_at IS NULL AND NOT issues.is_draft) AS \"__count\"",
            ),
        ];
        select.group_by = vec!["1".to_owned()];
        select.into_sql()
    }

    /// `__get_subgroup_total_queryset`: `(group, subgroup, count)` rows.
    pub fn sub_totals(&self, group: GroupField, sub_group: GroupField) -> Sql {
        self.sub_totals_with(group, sub_group, None)
    }

    /// [`IssueQuery::sub_totals`] with a replacement for the manager predicate.
    pub fn sub_totals_with(
        &self,
        group: GroupField,
        sub_group: GroupField,
        scope: Option<&IssueScopeFilter>,
    ) -> Sql {
        let mut builder = self.build(scope);
        builder.and(self.grouper_predicate());
        builder.require_left(Relation::IntakeIssues);
        let mut select = Select::new("issues");
        select.joins = builder.joins.to_joins();
        select.predicate = Some(builder.predicate);
        select.distinct = true;
        let mut group_expr = group.expression();
        group_expr.push_raw(" AS \"__group\"");
        let mut sub_expr = sub_group.expression();
        sub_expr.push_raw(" AS \"__sub_group\"");
        select.projection = vec![
            group_expr,
            sub_expr,
            Frag::raw(
                "COUNT(DISTINCT issues.id) FILTER (WHERE \
                 (intake_issues.status IN (1, -1, 2) OR intake_issues.id IS NULL) \
                 AND issues.archived_at IS NULL AND NOT issues.is_draft) AS \"__count\"",
            ),
        ];
        select.group_by = vec!["1".to_owned(), "2".to_owned()];
        select.into_sql()
    }

    /// The group-key query for `field`. The issue-derived fields need the
    /// filtered queryset, which the caller owns for the duration of the call.
    pub fn group_value_request<'a>(
        &'a self,
        field: GroupField,
        issues: &'a Select,
    ) -> crate::issue::group_values::GroupValuesRequest<'a> {
        self.group_value_request_with(field, issues)
    }

    /// [`IssueQuery::group_value_request`] with the same name and an explicit
    /// alias; kept so handlers read the same as the Python's call sites.
    pub fn group_value_request_with<'a>(
        &'a self,
        field: GroupField,
        issues: &'a Select,
    ) -> crate::issue::group_values::GroupValuesRequest<'a> {
        crate::issue::group_values::GroupValuesRequest {
            field,
            workspace_slug: &self.scope.workspace_slug,
            project_id: self.scope.project_id,
            user: None,
            issues: Some(issues),
        }
    }
}

/// Build the `LegacyFilters` from a `QueryDict`, freezing `now` for the
/// relative date ranges.
pub fn parse_legacy(
    params: &FilterParams<'_>,
    now: DateTime<Utc>,
) -> Result<LegacyFilters, QueryError> {
    LegacyFilters::parse(params, now.date_naive())
}

/// The group-value plan the paginator needs before it can shape the response.
#[derive(Debug, Clone)]
pub struct GroupPlan {
    pub group_by: GroupField,
    pub group_fields: Vec<String>,
    pub sub_group_by: Option<GroupField>,
    pub sub_group_fields: Vec<String>,
}

/// Render one group value from a query row, using Python's `str()`.
pub fn render_group_value(
    value: &tokio_postgres::Row,
    group: GroupField,
) -> Result<String, QueryError> {
    render_group_value_at(value, 0, group)
}

/// [`render_group_value`] for a column other than the first.
pub fn render_group_value_at(
    value: &tokio_postgres::Row,
    index: usize,
    group: GroupField,
) -> Result<String, QueryError> {
    match group {
        GroupField::Priority | GroupField::StateGroup => {
            let value: Option<String> = value.try_get(index).map_err(QueryError::from)?;
            Ok(crate::issue::group_values::render_string(value.as_deref()))
        }
        GroupField::TargetDate | GroupField::StartDate => {
            let value: Option<chrono::NaiveDate> =
                value.try_get(index).map_err(QueryError::from)?;
            Ok(crate::issue::group_values::render_date(value))
        }
        _ => {
            let value: Option<Uuid> = value.try_get(index).map_err(QueryError::from)?;
            Ok(crate::issue::group_values::render_uuid(value))
        }
    }
}

/// Turn a [`GroupValues`] into the string list the paginator keys on.
pub fn collect_group_values(
    plan: GroupValues,
    rows: Vec<tokio_postgres::Row>,
    group: GroupField,
) -> Result<Vec<String>, QueryError> {
    match plan {
        GroupValues::Static(values) => Ok(values),
        GroupValues::Query { .. } => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                out.push(render_group_value(&row, group)?);
            }
            Ok(out)
        }
    }
}

/// A `Json` value for one result row's group key (already a string).
pub fn group_key(value: &Json) -> String {
    match value {
        Json::Str(value) => value.clone(),
        Json::Null => "None".to_owned(),
        Json::Int(value) => value.to_string(),
        Json::Bool(value) => value.to_string(),
        Json::Float(value) => crate::pyfloat::repr(*value),
        Json::Date(value) => value.to_string(),
        Json::DateTime(value) => value.to_rfc3339(),
        Json::DateTimeZoned(value) => value.to_rfc3339(),
        Json::Array(_) | Json::Object(_) => value.to_serde().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::issue::model::ValueField;
    use crate::sql::Frag;

    #[test]
    fn base_manager_predicate_is_present() {
        let query = super::IssueQuery {
            scope: super::IssueScope::project("w", uuid::Uuid::nil()),
            filters: Default::default(),
            rich: None,
            ordering: crate::issue::order::Ordering::resolve("-created_at"),
            group_by: None,
            sub_group_by: None,
            timezone: chrono_tz::UTC,
        };
        let select = query.filtered_base();
        let sql = select.into_sql();
        assert!(sql.text.contains("issues.deleted_at IS NULL"));
        assert!(sql.text.contains("projects.archived_at IS NULL"));
        assert!(sql.text.contains("workspaces.slug = $2"));
        assert!(sql.text.contains("issues.project_id = $1"));
    }

    #[test]
    fn a_scope_filter_replaces_the_manager_predicate() {
        // The archived-issues list uses `Issue.objects` plus its own predicate
        // (`plane/app/views/issue/archive.py:78-86`), not `Issue.issue_objects`.
        let query = super::IssueQuery {
            scope: super::IssueScope::project("w", uuid::Uuid::nil()),
            filters: Default::default(),
            rich: None,
            ordering: crate::issue::order::Ordering::resolve("-created_at"),
            group_by: None,
            sub_group_by: None,
            timezone: chrono_tz::UTC,
        };
        let scope = super::IssueScopeFilter::new(Frag::raw("issues.archived_at IS NOT NULL"));
        let scoped = query.filtered_base_with(Some(&scope)).into_sql();
        assert!(scoped.text.contains("issues.archived_at IS NOT NULL"));
        assert!(!scoped.text.contains("NOT issues.is_draft"));
        // The unscoped statement still carries the manager's exclusions.
        assert!(
            query
                .filtered_base()
                .into_sql()
                .text
                .contains("NOT issues.is_draft")
        );
    }

    #[test]
    fn grouped_counts_honour_the_view_distinct() {
        // `IssueViewSet.list` ends in `.distinct()`; the cycle, module and
        // archived lists do not, and the windowed count then counts joined rows
        // (`plane/utils/paginator.py:278`).
        let mut query = super::IssueQuery {
            scope: super::IssueScope::project("w", uuid::Uuid::nil()),
            filters: Default::default(),
            rich: None,
            ordering: crate::issue::order::Ordering::resolve("-created_at"),
            group_by: Some(crate::issue::model::GroupField::StateId),
            sub_group_by: None,
            timezone: chrono_tz::UTC,
        };
        let fields = ValueField::project(Some(crate::issue::model::GroupField::StateId), None);
        query.scope.distinct = true;
        assert!(query.grouped_count(&fields).text.contains("DISTINCT"));
        query.scope.distinct = false;
        assert!(!query.grouped_count(&fields).text.contains("DISTINCT"));
        assert!(
            !query
                .grouped_page_count(&fields, 10, 0)
                .text
                .contains("DISTINCT")
        );
    }

    #[test]
    fn grouped_page_projects_the_order_key() {
        let query = super::IssueQuery {
            scope: super::IssueScope::project("w", uuid::Uuid::nil()),
            filters: Default::default(),
            rich: None,
            ordering: crate::issue::order::Ordering::resolve("priority"),
            group_by: Some(crate::issue::model::GroupField::StateId),
            sub_group_by: None,
            timezone: chrono_tz::UTC,
        };
        let fields = ValueField::project(Some(crate::issue::model::GroupField::StateId), None);
        let select = query.grouped_page(&fields, 10, 0, true);
        let sql = select.into_sql();
        assert!(
            sql.text
                .contains("ROW_NUMBER() OVER (PARTITION BY issues.state_id")
        );
        assert!(sql.text.contains("\"__order_key\""));
        assert!(sql.text.contains("\"__row_number\" > 0"));
    }
}
