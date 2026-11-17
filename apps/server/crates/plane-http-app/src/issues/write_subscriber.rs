// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueSubscriberViewSet`'s write actions
//! (`plane/app/views/issue/subscriber.py`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/issue-subscribers/` | `create` (mixin) + `perform_create` | `:34-57` |
//! | `DELETE …/issue-subscribers/{subscriber_id}/` | `destroy` | `:86-96` |
//! | `POST …/subscribe/` | `subscribe` | `:98-116` |
//! | `DELETE …/subscribe/` | `unsubscribe` | `:118-126` |
//!
//! ## create
//!
//! DRF's `CreateModelMixin`: `is_valid()` (the serializer's only writable
//! fields are `deleted_at`, `created_by`, `updated_by` and the required
//! `subscriber` — `plane/app/serializers/issue.py:984-989`), then
//! `perform_create`:
//!
//! 1. `Issue.objects.get(pk, workspace__slug, project_id)` — the soft-delete
//!    manager, so any live issue (archived/draft/triage included) is reachable;
//!    a miss is `ObjectDoesNotExist` → 404 `{"error": "The required object does
//!    not exist."}`;
//! 2. the subscriber must have an **active** `ProjectMember` row on the URL
//!    project, else `raise ValidationError({"error": "Subscriber must be a
//!    member of the project."})` — a view-level DRF error, so the body is the
//!    bare `{"error": "…"}` string, not a one-element list;
//! 3. `serializer.save(project_id=…, issue_id=…)`.
//!
//! `BaseModel.save` then overrides whatever `created_by`/`updated_by` the body
//! sent: on insert it stamps `created_by` = the request user and
//! `updated_by` = `None`. A duplicate `(issue, subscriber)` hits the partial
//! unique constraint, and `IntegrityError` is the family's 400
//! `{"error": "The payload is not valid"}` — the view does not pre-check.
//!
//! ## destroy
//!
//! `IssueSubscriber.objects.get(project, subscriber=<URL subscriber_id>,
//! workspace__slug, issue)`; a miss is the 404 above (the old unhandled
//! `DoesNotExist` was a 500 before `BaseViewSet.handle_exception`). The row is
//! soft-deleted (H03), and the URL `subscriber_id` is **not** checked against
//! the caller — the project permission is the only gate.
//!
//! ## subscribe / unsubscribe
//!
//! `subscribe` binds the issue the same way, returns
//! `400 {"message": "User already subscribed to the issue."}` on a repeat
//! (case-sensitive `message`, not `error`), and otherwise inserts the caller's
//! own row; the 201 body is `IssueSubscriberSerializer(instance).data`.
//! `unsubscribe` looks the caller's row up and soft-deletes it. Both are
//! `ProjectLitePermission` actions (`get_permissions`, `:26-32`), not
//! `ProjectEntityPermission`.

use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::IssueSubscriberSerializer;
use uuid::Uuid;

use super::lists::list_common::client;
use super::sub_resources::sub_common as shared;
use super::write_sub_queries as queries;
use crate::common;
use crate::write;
use crate::write::Serializer;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.subscriber.IssueSubscriberViewSet")?
        .action("create", create)?
        .action("destroy", destroy)?
        .action("subscribe", subscribe)?
        .action("unsubscribe", unsubscribe)?;
    Ok(())
}

/// The row a response serializer is built from.
struct SubscriberRow {
    id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    project_id: Uuid,
    workspace_id: Uuid,
    issue_id: Uuid,
    subscriber_id: Uuid,
}

impl SubscriberRow {
    fn serializer(&self) -> IssueSubscriberSerializer {
        IssueSubscriberSerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            created_by: self.created_by,
            updated_by: self.updated_by,
            project: self.project_id,
            workspace: self.workspace_id,
            issue: self.issue_id,
            subscriber: self.subscriber_id,
        }
    }

    /// `IssueSubscriberSerializer(instance).data` rendered by DRF.
    fn render(&self, ctx: &ViewCtx<'_>) -> Result<Vec<u8>, ViewError> {
        common::render(ctx, &self.serializer())
    }
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;
        let now = write::now();

        // `Issue.objects.get(pk=issue_id, workspace__slug=slug,
        // project_id=project_id)` (`:36-40`).
        require_issue(client, issue_id, project_id, &slug).await?;

        // `IssueSubscriberSerializer(data=request.data)` — `deleted_at`,
        // `created_by`, `updated_by`, `subscriber`, in `self.fields` order.
        use crate::write::generated::issue_subscriber_serializer as subscriber_fields;
        let mut serializer = Serializer::new(ctx, false)?;
        serializer.datetime_field("deleted_at", true);
        serializer
            .primary_key_field(
                client,
                "created_by",
                true,
                &subscriber_fields::PK_CREATED_BY,
            )
            .await?;
        serializer
            .primary_key_field(
                client,
                "updated_by",
                true,
                &subscriber_fields::PK_UPDATED_BY,
            )
            .await?;
        let subscriber = serializer
            .primary_key_field(
                client,
                "subscriber",
                false,
                &subscriber_fields::PK_SUBSCRIBER,
            )
            .await?;
        if serializer.missing_required("subscriber") {
            serializer.push_error("subscriber", crate::write::Detail::required());
        }
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `perform_create`'s membership check (`:41-48`): an active
        // `ProjectMember` on the URL project.
        let subscriber_id = subscriber.expect("validated_data[\"subscriber\"]");
        let statement = queries::PROJECT_MEMBER_ACTIVE.prepare(client).await?;
        let member: bool = client
            .query_one(&statement, &[&slug, &project_id, &subscriber_id])
            .await?
            .get(0);
        if !member {
            return Ok(plain_error(
                400,
                "Subscriber must be a member of the project.",
            ));
        }

        // `serializer.save(project_id=…, issue_id=…)` (`:49-52`): the body's
        // `created_by`/`updated_by` are overwritten by `BaseModel.save`, which
        // stamps the request user on insert and leaves `updated_by` null.
        let workspace_id = project_workspace(client, project_id).await?;
        let row = SubscriberRow {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            deleted_at: values.datetime("deleted_at"),
            created_by: Some(user),
            updated_by: None,
            project_id,
            workspace_id,
            issue_id,
            subscriber_id,
        };
        let statement = queries::SUBSCRIBER_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &row.created_at,
                    &row.updated_at,
                    &row.created_by,
                    &row.updated_by,
                    &row.id,
                    &row.project_id,
                    &row.workspace_id,
                    &row.issue_id,
                    &row.subscriber_id,
                ],
            )
            .await
            .map_err(map_integrity)?;

        let body = row.render(ctx)?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// destroy
// ---------------------------------------------------------------------------

pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let subscriber_id = shared::kwarg_uuid(ctx, "subscriber_id")?;
        let user = shared::user_id(ctx)?;

        // `IssueSubscriber.objects.get(project=project_id,
        // subscriber=subscriber_id, workspace__slug=slug, issue=issue_id)`
        // (`:87-92`).
        let id = find_subscriber(client, &slug, project_id, issue_id, subscriber_id).await?;
        let now = write::now();
        hooks::soft_delete::delete_instance(client, "issue_subscribers", "id", id, Some(user), now)
            .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuesubscriber", id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// subscribe / unsubscribe
// ---------------------------------------------------------------------------

pub fn subscribe<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;
        let now = write::now();

        // `Issue.objects.get(pk=issue_id, workspace__slug=slug,
        // project_id=project_id)` (`:99`).
        require_issue(client, issue_id, project_id, &slug).await?;

        // `IssueSubscriber.objects.filter(issue_id, subscriber=request.user,
        // …).exists()` (`:100-107`).
        let statement = queries::SUBSCRIBER_EXISTS_FOR_USER.prepare(client).await?;
        let existing: bool = client
            .query_one(&statement, &[&issue_id, &user, &slug, &project_id])
            .await?
            .get(0);
        if existing {
            return Ok(message_body(400, "User already subscribed to the issue."));
        }

        // `IssueSubscriber.objects.create(issue_id, subscriber_id=user.id,
        // project_id=project_id)` (`:109-111`).
        let workspace_id = project_workspace(client, project_id).await?;
        let row = SubscriberRow {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            created_by: Some(user),
            updated_by: None,
            project_id,
            workspace_id,
            issue_id,
            subscriber_id: user,
        };
        let statement = queries::SUBSCRIBER_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &row.created_at,
                    &row.updated_at,
                    &row.created_by,
                    &row.updated_by,
                    &row.id,
                    &row.project_id,
                    &row.workspace_id,
                    &row.issue_id,
                    &row.subscriber_id,
                ],
            )
            .await
            .map_err(map_integrity)?;

        let body = row.render(ctx)?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

pub fn unsubscribe<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;

        // `IssueSubscriber.objects.get(project=project_id,
        // subscriber=request.user, …)` (`:120-125`).
        let id = find_subscriber(client, &slug, project_id, issue_id, user).await?;
        let now = write::now();
        hooks::soft_delete::delete_instance(client, "issue_subscribers", "id", id, Some(user), now)
            .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuesubscriber", id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `Issue.objects.get(...)`: a miss is `ObjectDoesNotExist` → 404.
async fn require_issue(
    client: &Client,
    issue_id: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<(), ViewError> {
    let statement = queries::ISSUE_ANY_IN_PROJECT.prepare(client).await?;
    let exists: bool = client
        .query_one(&statement, &[&issue_id, &project_id, &slug])
        .await?
        .get(0);
    if exists {
        Ok(())
    } else {
        Err(ViewError::ObjectDoesNotExist)
    }
}

/// `IssueSubscriber.objects.get(...)`; the miss is `DoesNotExist` → 404.
async fn find_subscriber(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    subscriber_id: Uuid,
) -> Result<Uuid, ViewError> {
    let statement = queries::SUBSCRIBER_BY_SUBSCRIBER.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&project_id, &subscriber_id, &slug, &issue_id])
        .await?;
    row.map(|row| row.get::<_, Uuid>(0))
        .ok_or(ViewError::ObjectDoesNotExist)
}

/// `WorkspaceBaseModel.save` → `self.project.workspace`: the FK load goes
/// through Django's base manager, which has no soft-delete filter.
async fn project_workspace(client: &Client, project_id: Uuid) -> Result<Uuid, ViewError> {
    let statement = queries::PROJECT_WORKSPACE_ANY.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

/// A unique violation is `IntegrityError`, which `BaseViewSet.handle_exception`
/// renders as 400 `{"error": "The payload is not valid"}`.
fn map_integrity(error: tokio_postgres::Error) -> ViewError {
    if error
        .as_db_error()
        .is_some_and(|error| error.code() == &tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
    {
        ViewError::Integrity
    } else {
        ViewError::Unknown(error.to_string())
    }
}

/// `{"error": "…"}` with Django's rendering.
fn plain_error(status: u16, message: &str) -> ViewResponse {
    ViewResponse::json(status, serde_json::json!({ "error": message }))
}

/// `{"message": "…"}` with Django's rendering.
fn message_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::json(status, serde_json::json!({ "message": message }))
}
