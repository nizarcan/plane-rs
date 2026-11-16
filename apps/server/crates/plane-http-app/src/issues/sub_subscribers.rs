// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Issue subscribers (routes 83/85) — `IssueSubscriberViewSet`.
//!
//! Python: `plane/app/views/issue/subscriber.py:17-127`.
//!
//! * `list` does not list subscribers at all: it returns the *project's*
//!   active members through `ProjectMemberLiteSerializer`, and because the
//!   queryset has no `is_subscribed` annotation DRF skips that field
//!   (`BooleanField(read_only=True)` raises `SkipField`), so the body is
//!   `[{member: {…}, id: …}, …]`.
//! * `subscription_status` answers `{"subscribed": <exists>}` for the caller.
//!
//! The viewset's `get_permissions` selects `ProjectLitePermission` for the
//! subscribe actions and `ProjectEntityPermission` otherwise; `routes.json`
//! records both and the runtime evaluates them. Its `initial()` guest check is
//! also the runtime's (`initial_issue_hidden_from_guest`).

use tokio_postgres::Row;

use plane_core::error::APPLICATION_JSON;
use plane_db::models::project::ProjectMember;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_project::ProjectMemberLiteSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;

use super::sub_common::{self as shared, UserLiteRow};
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.subscriber.IssueSubscriberViewSet")?
        .action("list", list)?
        .action("subscription_status", subscription_status)?;
    Ok(())
}

struct MemberRow {
    member: ProjectMember,
    user: Option<UserLiteRow>,
}

struct MemberEntry<'a> {
    member: &'a ProjectMember,
    user: Option<UserLiteSerializer<'a>>,
}

impl<'a> MemberEntry<'a> {
    fn new(row: &'a MemberRow) -> Self {
        Self {
            member: &row.member,
            user: row.user.as_ref().map(UserLiteRow::serializer),
        }
    }

    fn serializer(&self) -> ProjectMemberLiteSerializer<'_> {
        ProjectMemberLiteSerializer {
            member: self.user.as_ref(),
            id: self.member.id,
            // No annotation on the queryset (`subscriber.py:74-82`); DRF skips
            // the field rather than emitting `null`.
            is_subscribed: None,
        }
    }
}

fn decode(row: &Row) -> Result<MemberRow, ViewError> {
    Ok(MemberRow {
        member: ProjectMember::from_row(row, 0)?,
        user: UserLiteRow::from_row(row, ProjectMember::COLUMN_COUNT)?,
    })
}

/// `IssueSubscriberViewSet.list` (`subscriber.py:74-82`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let statement = queries::SUBSCRIBERS.prepare(client).await?;
        let rows = client.query(&statement, &[&project_id, &slug]).await?;
        let decoded: Vec<MemberRow> = rows.iter().map(decode).collect::<Result<_, _>>()?;
        let entries: Vec<MemberEntry<'_>> = decoded.iter().map(MemberEntry::new).collect();
        let prepared = common::RequestRender::from_ctx(ctx);
        let serializers: Vec<ProjectMemberLiteSerializer<'_>> =
            entries.iter().map(MemberEntry::serializer).collect();
        let body = common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueSubscriberViewSet.subscription_status` (`subscriber.py:120-127`).
pub fn subscription_status<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;
        let statement = queries::SUBSCRIPTION_STATUS.prepare(client).await?;
        let row = client
            .query_one(&statement, &[&issue_id, &user_id, &project_id, &slug])
            .await?;
        let subscribed: bool = row.get(0);
        Ok(ViewResponse::json(
            200,
            serde_json::json!({ "subscribed": subscribed }),
        ))
    })
}
