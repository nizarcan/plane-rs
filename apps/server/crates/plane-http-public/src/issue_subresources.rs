// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Work-item sub-resources on `/api/v1/`: links, comments, activities,
//! attachments and relations (routes 319–326 and 331–339).
//!
//! Python: `plane/api/views/issue.py:1189-2801`. Every list is a
//! `BasePaginator` page of the v1 serializer in `plane-serializers`' generated
//! `api_issue` module, except the attachment list, which is a bare array
//! (`issue.py:2171-2180`), and the relations object, which is one grouped dict
//! (`issue.py:2597-2662`).
//!
//! ## Guards
//!
//! Every view resolves the URL issue with `Issue.objects.get` (the soft-delete
//! manager: drafts, triage and archived rows are visible here) and then applies
//! `guest_cannot_view_issue` (`issue.py:215-219`), a 404 for a restricted
//! guest who did not create the issue. Comments, attachments and relations add
//! their own creator/admin rules on top; they are ported at their call sites.
//!
//! ## Pagination
//!
//! `BasePaginator.paginate` with no `order_by` keeps each queryset's own
//! ordering (`paginator.py:125-130`): `-created_at` for links and comments,
//! the sanitised `?order_by=` for activities (default `created_at`).

use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::asset::FileAsset;
use plane_db::models::issue::{Issue, IssueActivity, IssueComment, IssueLink};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::paginator::{Cursor, OffsetWindow};
use plane_serializers::FieldValue;
use plane_serializers::generated::api_issue::{
    IssueActivitySerializer, IssueAttachmentSerializer, IssueCommentSerializer, IssueLinkSerializer,
};
use plane_storage::{PresignGet, RequestInfo};
use tokio_postgres::types::ToSql;
use uuid::Uuid;

use crate::common::page;
use crate::common::render::RequestRender;
use crate::issues::{guest_cannot_view, user_id};

/// `django.http.HttpResponseRedirect`'s content type.
const HTML_UTF8: &str = "text/html; charset=utf-8";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.issue.IssueLinkListCreateAPIEndpoint")?
        .method(http::Method::GET, link_list)?;
    registry
        .api_view("plane.api.views.issue.IssueLinkDetailAPIEndpoint")?
        .method(http::Method::GET, link_detail)?;
    registry
        .api_view("plane.api.views.issue.IssueCommentListCreateAPIEndpoint")?
        .method(http::Method::GET, comment_list)?;
    registry
        .api_view("plane.api.views.issue.IssueCommentDetailAPIEndpoint")?
        .method(http::Method::GET, comment_detail)?;
    registry
        .api_view("plane.api.views.issue.IssueActivityListAPIEndpoint")?
        .method(http::Method::GET, activity_list)?;
    registry
        .api_view("plane.api.views.issue.IssueActivityDetailAPIEndpoint")?
        .method(http::Method::GET, activity_detail)?;
    registry
        .api_view("plane.api.views.issue.IssueAttachmentListCreateAPIEndpoint")?
        .method(http::Method::GET, attachment_list)?;
    registry
        .api_view("plane.api.views.issue.IssueAttachmentDetailAPIEndpoint")?
        .method(http::Method::GET, attachment_get)?;
    registry
        .api_view("plane.api.views.issue.IssueRelationListCreateAPIEndpoint")?
        .method(http::Method::GET, relation_list)?;
    Ok(())
}

fn client<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a Client, ViewError> {
    plane_http_app::common::client(ctx)
}

fn kwarg_uuid(ctx: &ViewCtx<'_>, name: &str) -> Result<Uuid, ViewError> {
    Uuid::parse_str(ctx.kwarg(name).ok_or(ViewError::Key)?).map_err(|_| ViewError::Validation)
}

fn id_filters(ctx: &ViewCtx<'_>) -> Result<(String, Uuid, Uuid, Uuid), ViewError> {
    Ok((
        ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned(),
        kwarg_uuid(ctx, "project_id")?,
        kwarg_uuid(ctx, "issue_id")?,
        user_id(ctx)?,
    ))
}

/// `Issue.objects.get(workspace__slug=…, project_id=…, pk=issue_id)`
/// (`issue.py:1199-1203` and its siblings).
async fn issue_for_subresource(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> Result<Issue, ViewError> {
    let sql = format!(
        "SELECT {} FROM issues \
         INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
         WHERE issues.deleted_at IS NULL AND workspaces.slug = $1 \
           AND issues.project_id = $2 AND issues.id = $3",
        Issue::COLUMNS.replace("{alias}", "issues")
    );
    let row = client
        .query_opt(&sql, &[&slug, &project_id, &issue_id])
        .await
        .map_err(ViewError::from)?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Issue::from_row(&row, 0).map_err(ViewError::from)
}

fn not_found_work_item() -> ViewResponse {
    ViewResponse::raw(
        404,
        APPLICATION_JSON,
        br#"{"error":"Work item not found"}"#.to_vec(),
    )
}

fn permission_error(message: &str) -> ViewResponse {
    let body = plane_query::Json::object(vec![("error", plane_query::Json::string(message))]);
    ViewResponse::raw(403, APPLICATION_JSON, body.to_drf())
}

/// Run one page: append `LIMIT`/`OFFSET` after the caller's parameters, decode
/// the rows, then run the count statement.
async fn fetch_page<T>(
    ctx: &ViewCtx<'_>,
    client: &Client,
    sql: String,
    params: &[&(dyn ToSql + Sync)],
    total_sql: String,
    total_params: &[&(dyn ToSql + Sync)],
    decode: impl Fn(&tokio_postgres::Row) -> Result<T, ViewError>,
) -> Result<(Vec<T>, i64, OffsetWindow, Cursor, Cursor), ViewError> {
    let page = page::default_query(ctx)?;
    let window = OffsetWindow::new(&page, plane_query::paginator::MAX_LIMIT);
    let mut params: Vec<&(dyn ToSql + Sync)> = params.to_vec();
    let limit = window.row_limit() + 1;
    let offset = window.offset;
    params.push(&limit);
    params.push(&offset);
    let sql = format!("{sql} LIMIT ${} OFFSET ${}", params.len() - 1, params.len());
    let rows = client.query(&sql, &params).await.map_err(ViewError::from)?;
    let fetched = rows.len();
    let mut items = Vec::with_capacity(fetched.min(window.row_limit() as usize));
    for row in rows.iter().take(window.row_limit() as usize) {
        items.push(decode(row)?);
    }
    let (next, prev) = window.cursors(fetched);
    let total_row = client
        .query(&total_sql, total_params)
        .await
        .map_err(ViewError::from)?
        .into_iter()
        .next()
        .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?;
    let total: i64 = total_row.try_get(0).map_err(ViewError::from)?;
    Ok((items, total, window, next, prev))
}

fn render_array<T: plane_serializers::DrfSerialize>(
    ctx: &ViewCtx<'_>,
    items: &[T],
) -> Result<Vec<u8>, ViewError> {
    let prepared = RequestRender::from_ctx(ctx);
    plane_http_app::common::render_list(items, &prepared.render_ctx())
}

fn render_one<T: plane_serializers::DrfSerialize>(
    ctx: &ViewCtx<'_>,
    item: &T,
) -> Result<Vec<u8>, ViewError> {
    let prepared = RequestRender::from_ctx(ctx);
    plane_serializers::render(item, &prepared.render_ctx()).map_err(ViewError::from)
}

fn render_page(
    total: i64,
    window: &OffsetWindow,
    next: &Cursor,
    prev: &Cursor,
    count: usize,
    results: &[u8],
) -> Vec<u8> {
    let total_pages = plane_query::paginator::ceil_div(total, window.limit);
    let head = format!(
        "{{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":{total},\"next_cursor\":\"{next}\",\"prev_cursor\":\"{prev}\",\"next_page_results\":{},\"prev_page_results\":{},\"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total},\"extra_stats\":null,\"results\":",
        next.has_results, prev.has_results,
    );
    let mut body = head.into_bytes();
    body.extend_from_slice(results);
    body.push(b'}');
    body
}

fn page_envelope(
    total: i64,
    window: &OffsetWindow,
    next: &Cursor,
    prev: &Cursor,
    count: usize,
    results: Vec<u8>,
) -> ViewResponse {
    ViewResponse::raw(
        200,
        APPLICATION_JSON,
        render_page(total, window, next, prev, count, &results),
    )
}

// ---------------------------------------------------------------------------
// Links (319/331, 320/332)
// ---------------------------------------------------------------------------

/// The `IssueLink` queryset of both link views (`issue.py:1206-1217`).
const LINK_FROM: &str = "FROM issue_links \
     INNER JOIN workspaces ON issue_links.workspace_id = workspaces.id \
     WHERE issue_links.deleted_at IS NULL AND workspaces.slug = $1 \
       AND issue_links.project_id = $2 AND issue_links.issue_id = $3 \
       AND EXISTS (SELECT 1 FROM project_members pm \
                   WHERE pm.project_id = issue_links.project_id \
                     AND pm.member_id = $4 AND pm.is_active) \
       AND NOT EXISTS (SELECT 1 FROM projects p \
                       WHERE p.id = issue_links.project_id \
                         AND p.archived_at IS NOT NULL)";

fn issue_link_serializer(link: &IssueLink) -> IssueLinkSerializer<'_> {
    IssueLinkSerializer {
        id: link.id,
        created_at: link.created_at,
        updated_at: link.updated_at,
        deleted_at: link.deleted_at,
        title: link.title.as_deref(),
        url: &link.url,
        metadata: Some(plane_serializers::PgJson::from_json_text(&link.metadata)),
        created_by: link.created_by_id,
        updated_by: link.updated_by_id,
        project: link.project_id,
        workspace: link.workspace_id,
        issue: link.issue_id,
    }
}

/// `IssueLinkListCreateAPIEndpoint.get` (`issue.py:1241-1252`).
pub fn link_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT {} {LINK_FROM} ORDER BY issue_links.created_at DESC",
            IssueLink::COLUMNS.replace("{alias}", "issue_links")
        );
        let total_sql = format!("SELECT COUNT(*) {LINK_FROM}",);
        let (items, total, window, next, prev) = fetch_page(
            ctx,
            client,
            sql,
            &[&slug, &project_id, &issue_id, &user],
            total_sql,
            &[&slug, &project_id, &issue_id, &user],
            |row| IssueLink::from_row(row, 0).map_err(ViewError::from),
        )
        .await?;
        let serializers: Vec<IssueLinkSerializer<'_>> =
            items.iter().map(issue_link_serializer).collect();
        let results = render_array(ctx, &serializers)?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            items.len(),
            results,
        ))
    })
}

/// `IssueLinkDetailAPIEndpoint.get` (`issue.py:1352-1369`).
pub fn link_detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT {} {LINK_FROM} AND issue_links.id = $5",
            IssueLink::COLUMNS.replace("{alias}", "issue_links")
        );
        let params = [
            &slug as &(dyn ToSql + Sync),
            &project_id,
            &issue_id,
            &user,
            &pk,
        ];
        let row = client
            .query_opt(&sql, &params)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let link = IssueLink::from_row(&row, 0).map_err(ViewError::from)?;
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_one(ctx, &issue_link_serializer(&link))?,
        ))
    })
}

// ---------------------------------------------------------------------------
// Comments (321/333, 322/334)
// ---------------------------------------------------------------------------

struct CommentRow {
    comment: IssueComment,
    is_member: bool,
}

const COMMENT_FROM: &str = "FROM issue_comments \
     INNER JOIN workspaces ON issue_comments.workspace_id = workspaces.id \
     WHERE issue_comments.deleted_at IS NULL AND workspaces.slug = $1 \
       AND issue_comments.project_id = $2 AND issue_comments.issue_id = $3 \
       AND EXISTS (SELECT 1 FROM project_members pm \
                   WHERE pm.project_id = issue_comments.project_id \
                     AND pm.member_id = $4 AND pm.is_active) \
       AND NOT EXISTS (SELECT 1 FROM projects p \
                       WHERE p.id = issue_comments.project_id \
                         AND p.archived_at IS NOT NULL)";

fn comment_select() -> String {
    format!(
        "SELECT {}, EXISTS (SELECT 1 FROM project_members pm \
         WHERE pm.project_id = $2 AND pm.member_id = $4 AND pm.is_active \
           AND pm.deleted_at IS NULL) AS is_member {COMMENT_FROM}",
        IssueComment::COLUMNS.replace("{alias}", "issue_comments")
    )
}

fn comment_from_row(row: &tokio_postgres::Row) -> Result<CommentRow, ViewError> {
    Ok(CommentRow {
        comment: IssueComment::from_row(row, 0).map_err(ViewError::from)?,
        is_member: row
            .try_get(IssueComment::COLUMN_COUNT)
            .map_err(ViewError::from)?,
    })
}

fn comment_serializer<'a>(
    row: &'a CommentRow,
    attachments: &'a [&'a str],
) -> IssueCommentSerializer<'a> {
    IssueCommentSerializer {
        id: row.comment.id,
        is_member: FieldValue::value(row.is_member),
        created_at: row.comment.created_at,
        updated_at: row.comment.updated_at,
        deleted_at: row.comment.deleted_at,
        comment_html: &row.comment.comment_html,
        attachments,
        access: &row.comment.access,
        external_source: row.comment.external_source.as_deref(),
        external_id: row.comment.external_id.as_deref(),
        edited_at: row.comment.edited_at,
        created_by: row.comment.created_by_id,
        updated_by: row.comment.updated_by_id,
        project: row.comment.project_id,
        workspace: row.comment.workspace_id,
        description: row.comment.description_id,
        issue: row.comment.issue_id,
        actor: row.comment.actor_id,
        parent: row.comment.parent_id,
    }
}

/// `IssueCommentListCreateAPIEndpoint.get` (`issue.py:1517-1528`).
pub fn comment_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "{} ORDER BY issue_comments.created_at DESC",
            comment_select()
        );
        let total_sql = format!("SELECT COUNT(*) {COMMENT_FROM}");
        let (items, total, window, next, prev) = fetch_page(
            ctx,
            client,
            sql,
            &[&slug, &project_id, &issue_id, &user],
            total_sql,
            &[&slug, &project_id, &issue_id, &user],
            comment_from_row,
        )
        .await?;
        let attachments: Vec<Vec<&str>> = items
            .iter()
            .map(|row| row.comment.attachments.iter().map(String::as_str).collect())
            .collect();
        let serializers: Vec<IssueCommentSerializer<'_>> = items
            .iter()
            .zip(&attachments)
            .map(|(row, attachments)| comment_serializer(row, attachments))
            .collect();
        let results = render_array(ctx, &serializers)?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            items.len(),
            results,
        ))
    })
}

/// `IssueCommentDetailAPIEndpoint.get` (`issue.py:1676-1683`).
pub fn comment_detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!("{} AND issue_comments.id = $5", comment_select());
        let params = [
            &slug as &(dyn ToSql + Sync),
            &project_id,
            &issue_id,
            &user,
            &pk,
        ];
        let row = client
            .query_opt(&sql, &params)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let item = comment_from_row(&row)?;
        let attachments: Vec<&str> = item
            .comment
            .attachments
            .iter()
            .map(String::as_str)
            .collect();
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_one(ctx, &comment_serializer(&item, &attachments))?,
        ))
    })
}

// ---------------------------------------------------------------------------
// Activities (323/335, 324/336)
// ---------------------------------------------------------------------------

const ACTIVITY_FROM: &str = "FROM issue_activities \
     INNER JOIN workspaces ON issue_activities.workspace_id = workspaces.id \
     WHERE issue_activities.deleted_at IS NULL AND workspaces.slug = $1 \
       AND issue_activities.project_id = $2 AND issue_activities.issue_id = $3 \
       AND (issue_activities.field IS NULL \
            OR issue_activities.field NOT IN ('comment', 'vote', 'reaction', 'draft')) \
       AND EXISTS (SELECT 1 FROM project_members pm \
                   WHERE pm.project_id = issue_activities.project_id \
                     AND pm.member_id = $4 AND pm.is_active) \
       AND NOT EXISTS (SELECT 1 FROM projects p \
                       WHERE p.id = issue_activities.project_id \
                         AND p.archived_at IS NOT NULL)";

/// `ACTIVITY_ORDER_BY_ALLOWLIST` (`order_queryset.py:51-54`), default
/// `created_at`.
fn activity_order(ctx: &ViewCtx<'_>) -> String {
    const ALLOWED: [&str; 2] = ["created_at", "updated_at"];
    let value = ctx.query().get_or("order_by", "created_at");
    let descending = value.starts_with('-');
    let bare = if descending { &value[1..] } else { value };
    if bare.starts_with('-') || !ALLOWED.contains(&bare) {
        return "issue_activities.created_at ASC".to_owned();
    }
    format!(
        "issue_activities.{bare} {}",
        if descending { "DESC" } else { "ASC" }
    )
}

fn activity_serializer<'a>(
    activity: &'a IssueActivity,
    attachments: &'a [&'a str],
) -> IssueActivitySerializer<'a> {
    IssueActivitySerializer {
        id: activity.id,
        created_at: activity.created_at,
        updated_at: activity.updated_at,
        deleted_at: activity.deleted_at,
        verb: &activity.verb,
        field: activity.field.as_deref(),
        old_value: activity.old_value.as_deref(),
        new_value: activity.new_value.as_deref(),
        comment: &activity.comment,
        attachments,
        old_identifier: activity.old_identifier,
        new_identifier: activity.new_identifier,
        epoch: activity.epoch,
        project: activity.project_id,
        workspace: activity.workspace_id,
        issue: activity.issue_id,
        issue_comment: activity.issue_comment_id,
        actor: activity.actor_id,
    }
}

/// `IssueActivityListAPIEndpoint.get` (`issue.py:1851-1880`).
pub fn activity_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT {} {ACTIVITY_FROM} ORDER BY {}",
            IssueActivity::COLUMNS.replace("{alias}", "issue_activities"),
            activity_order(ctx)
        );
        let total_sql = format!("SELECT COUNT(*) {ACTIVITY_FROM}");
        let (items, total, window, next, prev) = fetch_page(
            ctx,
            client,
            sql,
            &[&slug, &project_id, &issue_id, &user],
            total_sql,
            &[&slug, &project_id, &issue_id, &user],
            |row| IssueActivity::from_row(row, 0).map_err(ViewError::from),
        )
        .await?;
        let attachments: Vec<Vec<&str>> = items
            .iter()
            .map(|item| item.attachments.iter().map(String::as_str).collect())
            .collect();
        let serializers: Vec<IssueActivitySerializer<'_>> = items
            .iter()
            .zip(&attachments)
            .map(|(item, attachments)| activity_serializer(item, attachments))
            .collect();
        let results = render_array(ctx, &serializers)?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            items.len(),
            results,
        ))
    })
}

/// `IssueActivityDetailAPIEndpoint.get` (`issue.py:1912-1945`).
pub fn activity_detail<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT {} {ACTIVITY_FROM} AND issue_activities.id = $5 ORDER BY {}",
            IssueActivity::COLUMNS.replace("{alias}", "issue_activities"),
            activity_order(ctx)
        );
        let params = [
            &slug as &(dyn ToSql + Sync),
            &project_id,
            &issue_id,
            &user,
            &pk,
        ];
        let row = client
            .query_opt(&sql, &params)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Ok(ViewResponse::raw(
                404,
                APPLICATION_JSON,
                br#"{"message":"Activity not found.","code":"NOT_FOUND"}"#.to_vec(),
            ));
        };
        let activity = IssueActivity::from_row(&row, 0).map_err(ViewError::from)?;
        let attachments: Vec<&str> = activity.attachments.iter().map(String::as_str).collect();
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_one(ctx, &activity_serializer(&activity, &attachments))?,
        ))
    })
}

// ---------------------------------------------------------------------------
// Attachments (325/337, 326/338)
// ---------------------------------------------------------------------------

fn attachment_serializer(asset: &FileAsset) -> IssueAttachmentSerializer<'_> {
    IssueAttachmentSerializer {
        id: asset.id,
        created_at: asset.created_at,
        updated_at: asset.updated_at,
        deleted_at: asset.deleted_at,
        attributes: Some(plane_serializers::PgJson::from_json_text(&asset.attributes)),
        asset: &asset.asset,
        entity_type: asset.entity_type.as_deref(),
        entity_identifier: asset.entity_identifier.as_deref(),
        is_deleted: asset.is_deleted,
        is_archived: asset.is_archived,
        external_id: asset.external_id.as_deref(),
        external_source: asset.external_source.as_deref(),
        size: asset.size,
        is_uploaded: asset.is_uploaded,
        storage_metadata: asset
            .storage_metadata
            .as_ref()
            .map(plane_serializers::PgJson::from),
        created_by: asset.created_by_id,
        updated_by: asset.updated_by_id,
        user: asset.user_id,
        workspace: asset.workspace_id,
        draft_issue: asset.draft_issue_id,
        project: asset.project_id,
        issue: asset.issue_id,
        comment: asset.comment_id,
        page: asset.page_id,
    }
}

/// `user_has_issue_permission(..., allow_creator=False)` (`issue.py:168-180`):
/// any active project member.
async fn is_active_project_member(
    client: &Client,
    project_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            "SELECT 1 FROM project_members \
             WHERE project_id = $1 AND member_id = $2 AND is_active \
               AND deleted_at IS NULL LIMIT 1",
            &[&project_id, &user],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `IssueAttachmentListCreateAPIEndpoint.get` (`issue.py:2154-2180`): a bare
/// array of uploaded attachments, newest first.
pub fn attachment_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let client = client(ctx)?;
        if !is_active_project_member(client, project_id, user).await? {
            return Ok(permission_error(
                "You are not allowed to view these attachments",
            ));
        }
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT {} FROM file_assets \
             INNER JOIN workspaces ON file_assets.workspace_id = workspaces.id \
             WHERE file_assets.deleted_at IS NULL AND file_assets.issue_id = $3 \
               AND file_assets.entity_type = 'ISSUE_ATTACHMENT' \
               AND workspaces.slug = $1 AND file_assets.project_id = $2 \
               AND file_assets.is_uploaded \
             ORDER BY file_assets.created_at DESC",
            FileAsset::COLUMNS.replace("{alias}", "file_assets")
        );
        let rows = client
            .query(&sql, &[&slug, &project_id, &issue_id])
            .await
            .map_err(ViewError::from)?;
        let assets: Vec<FileAsset> = rows
            .iter()
            .map(|row| FileAsset::from_row(row, 0))
            .collect::<Result<_, _>>()
            .map_err(ViewError::from)?;
        let serializers: Vec<IssueAttachmentSerializer<'_>> =
            assets.iter().map(attachment_serializer).collect();
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_array(ctx, &serializers)?,
        ))
    })
}

/// `IssueAttachmentDetailAPIEndpoint.get` (`issue.py:2290-2334`).
pub fn attachment_get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let client = client(ctx)?;
        if !is_active_project_member(client, project_id, user).await? {
            return Ok(permission_error(
                "You are not allowed to download this attachment",
            ));
        }
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT {}, file_assets.attributes->>'name' FROM file_assets \
             INNER JOIN workspaces ON file_assets.workspace_id = workspaces.id \
             WHERE file_assets.deleted_at IS NULL AND workspaces.slug = $1 \
               AND file_assets.project_id = $2 AND file_assets.issue_id = $3 \
               AND file_assets.entity_type = 'ISSUE_ATTACHMENT' \
               AND file_assets.id = $4",
            FileAsset::COLUMNS.replace("{alias}", "file_assets")
        );
        let row = client
            .query_opt(&sql, &[&slug, &project_id, &issue_id, &pk])
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let asset = FileAsset::from_row(&row, 0).map_err(ViewError::from)?;
        if !asset.is_uploaded {
            return Ok(ViewResponse::raw(
                400,
                APPLICATION_JSON,
                br#"{"error":"The asset is not uploaded.","status":false}"#.to_vec(),
            ));
        }
        let filename: Option<String> = row
            .try_get(FileAsset::COLUMN_COUNT)
            .map_err(ViewError::from)?;
        let request = RequestInfo {
            scheme: if ctx.ctx.is_secure { "https" } else { "http" },
            host: ctx.ctx.header("host").unwrap_or_default(),
        };
        let storage =
            plane_storage::Storage::from_process_env(&ctx.settings.storage).map_err(|error| {
                ViewError::Unknown(format!("object storage is not configured: {error}"))
            })?;
        let signed = storage
            .request(request)
            .presign_get(PresignGet {
                key: &asset.asset,
                expiration: None,
                disposition: "attachment",
                filename: filename.as_deref(),
                content_type: None,
            })
            .await
            .map_err(|error| {
                ViewError::Unknown(format!("generating a presigned URL failed: {error}"))
            })?;
        Ok(ViewResponse::raw(302, HTML_UTF8, Vec::new()).with_header("Location", &signed))
    })
}

// ---------------------------------------------------------------------------
// Relations (339)
// ---------------------------------------------------------------------------

/// `visible_issue_q(request.user)` for the *other* side of a relation
/// (`issue.py:222-245`, `2584-2587`). `$2` is the user.
const VISIBLE_ISSUE: &str = "SELECT vi.id FROM issues vi \
     WHERE vi.deleted_at IS NULL AND \
       (EXISTS (SELECT 1 FROM project_members pm \
                WHERE pm.project_id = vi.project_id AND pm.member_id = $2 \
                  AND pm.is_active AND pm.deleted_at IS NULL AND pm.role > 5) \
        OR EXISTS (SELECT 1 FROM project_members pm \
                   INNER JOIN projects vp ON vp.id = pm.project_id \
                   WHERE pm.project_id = vi.project_id AND pm.member_id = $2 \
                     AND pm.is_active AND pm.deleted_at IS NULL AND pm.role = 5 \
                     AND vp.guest_view_all_features) \
        OR EXISTS (SELECT 1 FROM project_members pm \
                   INNER JOIN projects vp ON vp.id = pm.project_id \
                   WHERE pm.project_id = vi.project_id AND pm.member_id = $2 \
                     AND pm.is_active AND pm.deleted_at IS NULL AND pm.role = 5 \
                     AND NOT vp.guest_view_all_features AND vi.created_by_id = $2))";

/// `IssueRelationListCreateAPIEndpoint.get` (`issue.py:2573-2662`).
pub fn relation_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, issue_id, user) = id_filters(ctx)?;
        let client = client(ctx)?;
        let issue = issue_for_subresource(client, &slug, project_id, issue_id).await?;
        if guest_cannot_view(ctx, client, &slug, &issue).await? {
            return Ok(not_found_work_item());
        }
        let sql = format!(
            "SELECT relation_type, issue_id, related_issue_id, \
                    issues.project_id, related_issues.project_id \
             FROM issue_relations \
             INNER JOIN workspaces ON issue_relations.workspace_id = workspaces.id \
             INNER JOIN issues ON issues.id = issue_relations.issue_id \
             INNER JOIN issues related_issues ON related_issues.id = issue_relations.related_issue_id \
             WHERE issue_relations.deleted_at IS NULL AND workspaces.slug = $1 \
               AND ((issue_relations.issue_id = $3 \
                     AND issue_relations.related_issue_id IN ({VISIBLE_ISSUE})) \
                 OR (issue_relations.related_issue_id = $3 \
                     AND issue_relations.issue_id IN ({VISIBLE_ISSUE})))"
        );
        let rows = client
            .query(&sql, &[&slug, &user, &issue_id])
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            render_relations(&rows, issue_id),
        ))
    })
}

fn render_relations(rows: &[tokio_postgres::Row], issue_id: Uuid) -> Vec<u8> {
    use plane_query::Json;
    let mut response: Vec<(&str, Json)> = vec![
        ("blocking", Json::Array(Vec::new())),
        ("blocked_by", Json::Array(Vec::new())),
        ("duplicate", Json::Array(Vec::new())),
        ("relates_to", Json::Array(Vec::new())),
        ("start_after", Json::Array(Vec::new())),
        ("start_before", Json::Array(Vec::new())),
        ("finish_after", Json::Array(Vec::new())),
        ("finish_before", Json::Array(Vec::new())),
    ];
    let mut seen_duplicate: Vec<Uuid> = Vec::new();
    let mut seen_relates_to: Vec<Uuid> = Vec::new();
    for row in rows {
        let relation_type: String = row.get(0);
        let source: Uuid = row.get(1);
        let target: Uuid = row.get(2);
        let source_project: Uuid = row.get(3);
        let target_project: Uuid = row.get(4);
        let pair = |project: Uuid, issue: Uuid| {
            Json::object(vec![
                ("project_id", Json::string(project.to_string())),
                ("issue_id", Json::string(issue.to_string())),
            ])
        };
        let push = |response: &mut Vec<(&'static str, Json)>, key: &str, value: Json| {
            if let Some((_, Json::Array(items))) =
                response.iter_mut().find(|(name, _)| *name == key)
            {
                items.push(value);
            }
        };
        match relation_type.as_str() {
            "blocked_by" => {
                if target == issue_id {
                    push(&mut response, "blocking", pair(source_project, source));
                }
                if source == issue_id {
                    push(&mut response, "blocked_by", pair(target_project, target));
                }
            }
            "duplicate" => {
                if source == issue_id && !seen_duplicate.contains(&target) {
                    seen_duplicate.push(target);
                    push(&mut response, "duplicate", pair(target_project, target));
                }
                if target == issue_id && !seen_duplicate.contains(&source) {
                    seen_duplicate.push(source);
                    push(&mut response, "duplicate", pair(source_project, source));
                }
            }
            "relates_to" => {
                if source == issue_id && !seen_relates_to.contains(&target) {
                    seen_relates_to.push(target);
                    push(&mut response, "relates_to", pair(target_project, target));
                }
                if target == issue_id && !seen_relates_to.contains(&source) {
                    seen_relates_to.push(source);
                    push(&mut response, "relates_to", pair(source_project, source));
                }
            }
            "start_before" => {
                if target == issue_id {
                    push(&mut response, "start_after", pair(source_project, source));
                }
                if source == issue_id {
                    push(&mut response, "start_before", pair(target_project, target));
                }
            }
            "finish_before" => {
                if target == issue_id {
                    push(&mut response, "finish_after", pair(source_project, source));
                }
                if source == issue_id {
                    push(&mut response, "finish_before", pair(target_project, target));
                }
            }
            _ => {}
        }
    }
    Json::object(response).to_drf()
}
