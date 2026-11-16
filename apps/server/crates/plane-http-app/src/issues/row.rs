// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The 26-column "flat" issue row, shared by the list endpoint and by the
//! work-item write paths.
//!
//! `IssueViewSet.create` answers 201 with a hand-built `.values(...)` row, not
//! a serializer (`plane/app/views/issue/base.py:466-489`). The projection is
//! the same 26 columns as `IssueListEndpoint`'s
//! (`plane/app/views/issue/base.py:174-201`), in the same order, over the same
//! annotations (`apply_annotations` + `issue_queryset_grouper(group_by=None)`);
//! the only differences are the scope (one issue, by id) and the ordering.
//!
//! [`render_issue_row`] is the write-path entry point:
//!
//! ```ignore
//! let row = crate::issues::row::render_issue_row(ctx, client, id).await?;
//! Ok(ViewResponse::raw(201, APPLICATION_JSON, row.to_drf()))
//! ```
//!
//! It issues **one** statement (Django's `.values(...).first()`), reads the
//! columns by index and shifts `created_at`/`updated_at` into the caller's
//! timezone exactly as `user_timezone_converter` does — so the body is the
//! bytes Django's `Response(issue, 201)` writes.
//!
//! `IssueListEndpoint` (a different Django class, `:65-200`) uses the same
//! columns but a `COUNT(*)`-shaped `sub_issues_count`; both variants decode
//! through the same [`decode_row`], and [`render_issue_row`] uses the
//! write path's annotations.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use plane_query::issue::model::ValueField;
use plane_query::issue::values;
use plane_query::json::Json;
use plane_query::sql::{Frag, Select};
use uuid::Uuid;

use super::lists::list_common::{kwarg_text, kwarg_uuid, user_timezone};

/// `IssueListEndpoint`'s `.values(...)` list, in Python's order
/// (`plane/app/views/issue/base.py:174-201`). `deleted_at` is the last entry
/// and has no `ValueField`; every other field does.
pub(crate) const LIST_FIELDS: [ListField; 26] = [
    ListField::Value(ValueField::Id),
    ListField::Value(ValueField::Name),
    ListField::Value(ValueField::StateId),
    ListField::Value(ValueField::SortOrder),
    ListField::Value(ValueField::CompletedAt),
    ListField::Value(ValueField::EstimatePoint),
    ListField::Value(ValueField::Priority),
    ListField::Value(ValueField::StartDate),
    ListField::Value(ValueField::TargetDate),
    ListField::Value(ValueField::SequenceId),
    ListField::Value(ValueField::ProjectId),
    ListField::Value(ValueField::ParentId),
    ListField::Value(ValueField::CycleId),
    ListField::Value(ValueField::ModuleIds),
    ListField::Value(ValueField::LabelIds),
    ListField::Value(ValueField::AssigneeIds),
    ListField::SubIssuesCount,
    ListField::Value(ValueField::CreatedAt),
    ListField::Value(ValueField::UpdatedAt),
    ListField::Value(ValueField::CreatedBy),
    ListField::Value(ValueField::UpdatedBy),
    ListField::AttachmentCount,
    ListField::LinkCount,
    ListField::Value(ValueField::IsDraft),
    ListField::Value(ValueField::ArchivedAt),
    ListField::DeletedAt,
];

#[derive(Clone, Copy)]
pub(crate) enum ListField {
    Value(ValueField),
    /// The three counts are aggregate subqueries **without** a `GROUP BY`
    /// here (`:118-147`), so an issue with no children/links/attachments gets
    /// `0`, not `null` like `issue_on_results`' grouped variant.
    SubIssuesCount,
    AttachmentCount,
    LinkCount,
    DeletedAt,
}

impl ListField {
    pub(crate) fn projection(self) -> Frag {
        match self {
            Self::Value(field) => values::projection(&[field])
                .pop()
                .unwrap_or_else(Frag::empty),
            Self::SubIssuesCount => Frag::raw(concat!(
                "(SELECT COUNT(sub_issues.id) FROM issues sub_issues ",
                "LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id ",
                "INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id ",
                "WHERE sub_issues.deleted_at IS NULL ",
                "AND (sub_states.group IS NULL OR sub_states.group <> 'triage') ",
                "AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL ",
                "AND NOT sub_issues.is_draft AND sub_issues.parent_id = issues.id) ",
                "AS \"sub_issues_count\""
            )),
            Self::AttachmentCount => Frag::raw(concat!(
                "(SELECT COUNT(file_assets.id) FROM file_assets ",
                "WHERE file_assets.deleted_at IS NULL ",
                "AND file_assets.entity_type = 'ISSUE_ATTACHMENT' ",
                "AND file_assets.issue_id = issues.id) AS \"attachment_count\""
            )),
            Self::LinkCount => Frag::raw(concat!(
                "(SELECT COUNT(issue_links.id) FROM issue_links ",
                "WHERE issue_links.deleted_at IS NULL ",
                "AND issue_links.issue_id = issues.id) AS \"link_count\""
            )),
            Self::DeletedAt => Frag::raw("issues.deleted_at AS \"deleted_at\""),
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Value(field) => field.key(),
            Self::SubIssuesCount => "sub_issues_count",
            Self::AttachmentCount => "attachment_count",
            Self::LinkCount => "link_count",
            Self::DeletedAt => "deleted_at",
        }
    }
}

/// One `ValueField` per projection column, in projection order.
///
/// `Ordering::apply_own_order` looks the ordering key up by position in this
/// list, so it must be as long as the projection and in the same order. The
/// count and `deleted_at` columns have no `ValueField`; `Id` is a placeholder
/// that no ordering key maps to.
pub(crate) fn value_fields() -> Vec<ValueField> {
    LIST_FIELDS
        .iter()
        .map(|field| match field {
            ListField::Value(field) => *field,
            _ => ValueField::Id,
        })
        .collect()
}

/// The 26 columns of [`LIST_FIELDS`], for a statement whose `FROM` already
/// joins `states`, `projects`, `workspaces`, `project_members` and the m2m
/// through tables the annotations need.
pub(crate) fn projection() -> Vec<Frag> {
    LIST_FIELDS.iter().map(|field| field.projection()).collect()
}

/// The `SELECT` list of the write path's row: [`LIST_FIELDS`] over the
/// `apply_annotations` + `issue_queryset_grouper` shapes
/// (`plane/app/views/issue/base.py:239-283,466-489`).
///
/// The three counts are the *grouped* subqueries here (`GROUP BY …` inside, so
/// a missing row yields `NULL`), unlike `IssueListEndpoint`'s `COUNT(*)`
/// variants.
///
/// The m2m arrays keep `plane-query`'s correlated-subquery shape
/// (`issue_on_results`). Django's `issue_queryset_grouper` builds them as
/// top-level `ARRAY_AGG(… FILTER …)` over LEFT JOINs, which differs only for an
/// assignee whose `project_members` row is soft-deleted (the grouper keeps it,
/// the subquery drops it) — a state the create path cannot produce, because
/// `IssueCreateSerializer.validate` already filters `assignee_ids` through
/// `ProjectMember.objects` (soft-delete aware) before the row is written.
#[allow(dead_code)] // called by the work-item write handlers (set A)
pub(crate) fn write_projection() -> Vec<Frag> {
    LIST_FIELDS
        .iter()
        .map(|field| match field {
            // `sub_issues_count` (`:277-283`): `Issue.issue_objects
            // .filter(parent=OuterRef("id"))` + `COUNT(*)` + `GROUP BY parent`.
            ListField::SubIssuesCount => Frag::raw(concat!(
                "(SELECT COUNT(sub_issues.id) FROM issues sub_issues ",
                "LEFT OUTER JOIN states sub_states ON sub_issues.state_id = sub_states.id ",
                "INNER JOIN projects sub_projects ON sub_issues.project_id = sub_projects.id ",
                "WHERE sub_issues.deleted_at IS NULL ",
                "AND (sub_states.group IS NULL OR sub_states.group <> 'triage') ",
                "AND sub_issues.archived_at IS NULL AND sub_projects.archived_at IS NULL ",
                "AND NOT sub_issues.is_draft AND sub_issues.parent_id = issues.id ",
                "GROUP BY sub_issues.parent_id) AS \"sub_issues_count\""
            )),
            // `attachment_count` (`:259-267`).
            ListField::AttachmentCount => Frag::raw(concat!(
                "(SELECT COUNT(file_assets.id) FROM file_assets ",
                "WHERE file_assets.deleted_at IS NULL ",
                "AND file_assets.entity_type = 'ISSUE_ATTACHMENT' ",
                "AND file_assets.issue_id = issues.id ",
                "GROUP BY file_assets.issue_id) AS \"attachment_count\""
            )),
            // `link_count` (`:249-257`).
            ListField::LinkCount => Frag::raw(concat!(
                "(SELECT COUNT(issue_links.id) FROM issue_links ",
                "WHERE issue_links.deleted_at IS NULL ",
                "AND issue_links.issue_id = issues.id ",
                "GROUP BY issue_links.issue_id) AS \"link_count\""
            )),
            other => other.projection(),
        })
        .collect()
}

/// `IssueViewSet.create`'s 201 body: the flat row for one issue
/// (`plane/app/views/issue/base.py:466-489`).
///
/// `Issue.issue_objects.filter(project_id=…, workspace__slug=…).distinct()` +
/// `apply_annotations` + `issue_queryset_grouper(group_by=None)` +
/// `.values(…26…)` + `.first()` + `user_timezone_converter` — one statement.
/// The caller renders the returned document with `Json::to_drf()`.
#[allow(dead_code)] // called by the work-item write handlers (set A)
pub(crate) async fn render_issue_row(
    ctx: &ViewCtx<'_>,
    client: &Client,
    id: Uuid,
) -> Result<Json, ViewError> {
    let slug = kwarg_text(ctx, "slug")?;
    let project_id = kwarg_uuid(ctx, "project_id")?;
    let timezone = user_timezone(ctx);

    let mut select = Select::new("issues");
    // `Issue.issue_objects`: the manager's joins (`plane/db/models/issue.py:92-101`).
    select.joins = vec![
        plane_query::sql::Join::left("states", "states", Frag::raw("issues.state_id = states.id")),
        plane_query::sql::Join::inner(
            "projects",
            "projects",
            Frag::raw("issues.project_id = projects.id"),
        ),
        plane_query::sql::Join::inner(
            "workspaces",
            "workspaces",
            Frag::raw("issues.workspace_id = workspaces.id"),
        ),
        // `issue_queryset_grouper`'s three array annotations.
        plane_query::sql::Join::left(
            "issue_labels",
            "issue_labels",
            Frag::raw("issues.id = issue_labels.issue_id"),
        ),
        plane_query::sql::Join::left(
            "issue_assignees",
            "issue_assignees",
            Frag::raw("issues.id = issue_assignees.issue_id"),
        ),
        plane_query::sql::Join::left(
            "module_issues",
            "module_issues",
            Frag::raw("issues.id = module_issues.issue_id"),
        ),
        plane_query::sql::Join::left(
            "modules",
            "modules",
            Frag::raw("module_issues.module_id = modules.id"),
        ),
    ];
    select.projection = write_projection();
    select.predicate = Some(
        Frag::is_null("issues.deleted_at")
            .and(Frag::raw(
                "(states.group IS NULL OR states.group <> 'triage')",
            ))
            .and(Frag::is_null("issues.archived_at"))
            .and(Frag::is_null("projects.archived_at"))
            .and(Frag::raw("NOT issues.is_draft"))
            .and(Frag::compare(
                "issues.project_id",
                "=",
                plane_query::sql::Value::Uuid(project_id),
            ))
            .and(Frag::compare(
                "workspaces.slug",
                "=",
                plane_query::sql::Value::Text(slug.to_owned()),
            ))
            .and(Frag::compare(
                "issues.id",
                "=",
                plane_query::sql::Value::Uuid(id),
            )),
    );
    // `Issue.Meta.ordering` is `("-created_at",)`, and `.first()` applies it.
    select.order_by = vec![Frag::raw("issues.created_at DESC")];
    select.limit = Some(1);
    // The m2m annotations aggregate, so Django groups by the primary key
    // (`GROUP BY "issues"."id"`, `collapse_group_by`).
    select.group_by = vec!["issues.id".to_owned()];
    select.distinct = true;

    let sql = select.into_sql();
    let Some(row) = sql
        .query(client)
        .await
        .map_err(ViewError::from)?
        .into_iter()
        .next()
    else {
        // The issue was just written, so a miss is a bug, not a 404; Django's
        // `.first()` would answer 201 with `null`, which no caller accepts.
        return Err(ViewError::Unknown(
            "the issue row vanished between the write and the response".to_owned(),
        ));
    };
    Ok(shift_timezone(decode_row(&row)?, timezone))
}

/// Decode one projection row into the response object, in [`LIST_FIELDS`] order.
pub(crate) fn decode_row(row: &tokio_postgres::Row) -> Result<Json, ViewError> {
    let mut entries: Vec<(&str, Json)> = Vec::with_capacity(LIST_FIELDS.len());
    for (index, field) in LIST_FIELDS.iter().enumerate() {
        let value = match field {
            ListField::DeletedAt => Json::opt_datetime(row.get::<_, Option<DateTime<Utc>>>(index)),
            ListField::SubIssuesCount | ListField::AttachmentCount | ListField::LinkCount => {
                match row.get::<_, Option<i64>>(index) {
                    // The grouped variant (write path) is `null` for no rows.
                    Some(value) => Json::Int(value),
                    None => Json::Null,
                }
            }
            ListField::Value(field) => match field {
                ValueField::Id | ValueField::ProjectId => Json::uuid(row.get(index)),
                ValueField::Name | ValueField::Priority => {
                    Json::string(row.get::<_, String>(index))
                }
                ValueField::StateId
                | ValueField::ParentId
                | ValueField::CycleId
                | ValueField::EstimatePoint
                | ValueField::CreatedBy
                | ValueField::UpdatedBy => Json::opt_uuid(row.get::<_, Option<Uuid>>(index)),
                ValueField::SortOrder => Json::Float(row.get::<_, f64>(index)),
                ValueField::CompletedAt => {
                    Json::opt_datetime(row.get::<_, Option<DateTime<Utc>>>(index))
                }
                ValueField::CreatedAt | ValueField::UpdatedAt => {
                    Json::DateTime(row.get::<_, DateTime<Utc>>(index))
                }
                ValueField::StartDate | ValueField::TargetDate | ValueField::ArchivedAt => {
                    Json::opt_date(row.get::<_, Option<NaiveDate>>(index))
                }
                ValueField::SequenceId => Json::Int(i64::from(row.get::<_, i32>(index))),
                ValueField::IsDraft => Json::Bool(row.get(index)),
                ValueField::AssigneeIds | ValueField::LabelIds | ValueField::ModuleIds => {
                    let mut ids: Vec<Uuid> =
                        row.get::<_, Option<Vec<Uuid>>>(index).unwrap_or_default();
                    ids.sort();
                    Json::Array(ids.into_iter().map(Json::uuid).collect())
                }
                other => {
                    return Err(ViewError::Unknown(format!(
                        "unexpected projection field {:?}",
                        other.key()
                    )));
                }
            },
        };
        entries.push((field.key(), value));
    }
    Ok(Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    ))
}

/// `user_timezone_converter(issue, ["created_at", "updated_at"], tz)`:
/// `astimezone()` before DRF renders, so the offsets are local ones.
pub(crate) fn shift_timezone(value: Json, timezone: Tz) -> Json {
    let Json::Object(entries) = value else {
        return value;
    };
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| {
                if (key == "created_at" || key == "updated_at")
                    && let Json::DateTime(value) = value
                {
                    let zoned = value.with_timezone(&timezone);
                    return (key, Json::DateTimeZoned(zoned.fixed_offset()));
                }
                (key, value)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_write_projection_matches_the_list_projection() {
        let list = projection();
        let write = write_projection();
        assert_eq!(list.len(), LIST_FIELDS.len());
        assert_eq!(write.len(), LIST_FIELDS.len());
        // The three counts differ: `IssueListEndpoint` uses a `COUNT(*)`
        // subquery (0 when empty), `IssueViewSet.create` the grouped variant
        // (`null` when empty, `plane/app/views/issue/base.py:249-283`).
        let sql = |frag: &Frag| frag.clone().into_sql().text;
        assert!(!sql(&list[16]).contains("GROUP BY"));
        assert!(sql(&write[16]).contains("GROUP BY"));
        assert!(!sql(&list[21]).contains("GROUP BY"));
        assert!(sql(&write[21]).contains("GROUP BY"));
        // `link_count`/`attachment_count`/`sub_issues_count` are the three
        // `ListField` count variants; the write path's shapes add `GROUP BY`.
        assert!(!sql(&list[22]).contains("GROUP BY"));
        assert!(sql(&write[22]).contains("GROUP BY issue_links.issue_id"));
        // Every other column — including the three m2m arrays, which both
        // sides express as `ARRAY_AGG(… FILTER …)` — is the same expression.
        for index in (0..LIST_FIELDS.len()).filter(|index| ![16, 21, 22].contains(index)) {
            assert_eq!(sql(&list[index]), sql(&write[index]), "column {index}");
        }
    }

    #[test]
    fn the_field_names_are_pythons() {
        let names: Vec<&str> = LIST_FIELDS.iter().map(|field| field.key()).collect();
        assert_eq!(names.len(), 26);
        assert_eq!(names[0], "id");
        assert_eq!(names[13], "module_ids");
        assert_eq!(names[16], "sub_issues_count");
        assert_eq!(names[21], "attachment_count");
        assert_eq!(names[22], "link_count");
        assert_eq!(names[25], "deleted_at");
    }
}
