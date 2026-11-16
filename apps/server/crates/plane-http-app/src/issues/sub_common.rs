// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared pieces of the issue sub-resource handlers.
//!
//! Everything here is used by at least two `sub_` handler files: the nested
//! serializer rows (`UserLite`, `ProjectLite`, `WorkspaceLite`, `IssueFlat`),
//! the `issue_hidden_from_guest` / `issue_in_project` guards, and the global
//! paginator the version endpoints use.
//!
//! `plane-http-app` does not depend on `plane-permissions`, so the two guards
//! are re-implemented here with the statements of
//! [`super::queries`](crate::issues::sub_resources::sub_queries). They are the same two
//! queries `plane_permissions::issue_hidden_from_guest` runs
//! (`plane/app/permissions/base.py:106-141`); once a second domain needs them
//! they belong in `common`.

use chrono::NaiveDate;
use deadpool_postgres::Client;
use tokio_postgres::Row;
use uuid::Uuid;

use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_serializers::PgJson;
use plane_serializers::generated::app_issue::{CommentReactionSerializer, IssueFlatSerializer};
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;

use super::sub_queries as queries;

/// `PermissionDenied("You are not allowed to view this issue")` as the
/// handler-level `Response({"error": …}, 403)` the sub-resource views build.
pub const HIDDEN_MESSAGE: &str = "You are not allowed to view this issue";

// ---------------------------------------------------------------------------
// Nested serializer rows
// ---------------------------------------------------------------------------

/// The five columns every `asset_url` decision needs (`plane/db/models/asset.py:84-104`).
#[derive(Debug, Clone)]
pub struct AssetParts {
    pub id: Uuid,
    pub entity_type: Option<String>,
    pub project_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
    /// `asset.workspace.slug`; `None` when the asset has no workspace.
    pub workspace_slug: Option<String>,
}

impl AssetParts {
    /// Decode the asset columns, `None` when `id` is NULL (no joined asset).
    pub fn from_row(row: &Row, start: usize) -> Result<Option<Self>, tokio_postgres::Error> {
        let Some(id) = row.try_get::<_, Option<Uuid>>(start)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            id,
            entity_type: row.try_get(start + 1)?,
            project_id: row.try_get(start + 2)?,
            issue_id: row.try_get(start + 3)?,
            workspace_slug: row.try_get(start + 4)?,
        }))
    }

    /// `FileAsset.asset_url` (`plane/db/models/asset.py:84-104`).
    ///
    /// The `ISSUE_ATTACHMENT` and description branches read `self.workspace.slug`
    /// in Python; a workspace-less asset of one of those kinds raises
    /// `AttributeError` there, which no write path can produce (the asset row is
    /// always created with its workspace).
    pub fn url(&self) -> Option<String> {
        match self.entity_type.as_deref() {
            Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
                Some(format!("/api/assets/v2/static/{}/", self.id))
            }
            Some("ISSUE_ATTACHMENT") => Some(format!(
                "/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{}/",
                self.workspace_slug.as_deref().unwrap_or_default(),
                self.project_id
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "None".to_string()),
                self.issue_id
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "None".to_string()),
                self.id
            )),
            Some(
                "ISSUE_DESCRIPTION"
                | "COMMENT_DESCRIPTION"
                | "PAGE_DESCRIPTION"
                | "DRAFT_ISSUE_DESCRIPTION",
            ) => Some(format!(
                "/api/assets/v2/workspaces/{}/projects/{}/{}",
                self.workspace_slug.as_deref().unwrap_or_default(),
                self.project_id
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "None".to_string()),
                self.id
            )),
            _ => None,
        }
    }
}

/// `UserLiteSerializer` (`plane/app/serializers/user.py:155-167`):
/// `id, first_name, last_name, avatar, avatar_url, is_bot, display_name`.
#[derive(Debug)]
pub struct UserLiteRow {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub is_bot: bool,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

impl UserLiteRow {
    /// Columns consumed: 6 user columns + the avatar asset's 5.
    pub const COLUMN_COUNT: usize = 11;

    pub fn from_row(row: &Row, start: usize) -> Result<Option<Self>, tokio_postgres::Error> {
        let Some(id) = row.try_get::<_, Option<Uuid>>(start)? else {
            return Ok(None);
        };
        let avatar: String = row.try_get(start + 3)?;
        let asset = AssetParts::from_row(row, start + 6)?;
        // `User.avatar_url` (`plane/db/models/user.py:143-151`).
        let avatar_url = match &asset {
            Some(asset) => asset.url(),
            None if !avatar.is_empty() => Some(avatar.clone()),
            None => None,
        };
        Ok(Some(Self {
            id,
            first_name: row.try_get(start + 1)?,
            last_name: row.try_get(start + 2)?,
            avatar,
            is_bot: row.try_get(start + 4)?,
            display_name: row.try_get(start + 5)?,
            avatar_url,
        }))
    }

    pub fn serializer(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }
}

/// `ProjectLiteSerializer` (`plane/app/serializers/project.py:113-125`).
#[derive(Debug)]
pub struct ProjectLiteRow {
    pub id: Uuid,
    pub identifier: String,
    pub name: String,
    pub cover_image: Option<String>,
    pub logo_props: Option<JsonText>,
    pub description: String,
    pub cover_image_url: Option<String>,
}

impl ProjectLiteRow {
    /// Columns consumed: 6 project columns + the cover asset's 5.
    pub const COLUMN_COUNT: usize = 11;

    pub fn from_row(row: &Row, start: usize) -> Result<Option<Self>, tokio_postgres::Error> {
        let Some(id) = row.try_get::<_, Option<Uuid>>(start)? else {
            return Ok(None);
        };
        let cover_image: Option<String> = row.try_get(start + 3)?;
        let asset = AssetParts::from_row(row, start + 6)?;
        // `Project.cover_image_url` (`plane/db/models/project.py:129-139`):
        // an empty `cover_image` is falsy in Python, so it yields None rather
        // than `""`.
        let cover_image_url = match &asset {
            Some(asset) => asset.url(),
            None if cover_image
                .as_deref()
                .is_some_and(|value| !value.is_empty()) =>
            {
                cover_image.clone()
            }
            None => None,
        };
        Ok(Some(Self {
            id,
            identifier: row.try_get(start + 1)?,
            name: row.try_get(start + 2)?,
            cover_image,
            logo_props: row.try_get(start + 4)?,
            description: row.try_get(start + 5)?,
            cover_image_url,
        }))
    }

    pub fn serializer(&self) -> ProjectLiteSerializer<'_> {
        ProjectLiteSerializer {
            id: self.id,
            identifier: &self.identifier,
            name: &self.name,
            cover_image: self.cover_image.as_deref(),
            cover_image_url: self.cover_image_url.as_deref(),
            logo_props: self.logo_props.as_ref().map(PgJson::from_json_text),
            description: &self.description,
        }
    }
}

/// `WorkspaceLiteSerializer` (`plane/app/serializers/workspace.py:87-91`).
#[derive(Debug)]
pub struct WorkspaceLiteRow {
    pub name: String,
    pub slug: String,
    pub id: Uuid,
    pub logo_url: Option<String>,
}

impl WorkspaceLiteRow {
    /// Columns consumed: `name, slug, id, logo` + the logo asset's 5.
    pub const COLUMN_COUNT: usize = 9;

    pub fn from_row(row: &Row, start: usize) -> Result<Option<Self>, tokio_postgres::Error> {
        let Some(id) = row.try_get::<_, Option<Uuid>>(start + 2)? else {
            return Ok(None);
        };
        let logo: Option<String> = row.try_get(start + 3)?;
        let asset = AssetParts::from_row(row, start + 4)?;
        // `Workspace.logo_url` (`plane/db/models/workspace.py:146-154`); an
        // empty `logo` is falsy in Python and yields None.
        let logo_url = match &asset {
            Some(asset) => asset.url(),
            None if logo.as_deref().is_some_and(|value| !value.is_empty()) => logo.clone(),
            None => None,
        };
        Ok(Some(Self {
            name: row.try_get(start)?,
            slug: row.try_get(start + 1)?,
            id,
            logo_url,
        }))
    }

    pub fn serializer(&self) -> WorkspaceLiteSerializer<'_> {
        WorkspaceLiteSerializer {
            name: &self.name,
            slug: &self.slug,
            id: self.id,
            logo_url: self.logo_url.as_deref(),
        }
    }
}

/// `IssueFlatSerializer` (`plane/app/serializers/issue.py:52-70`).
#[derive(Debug)]
pub struct IssueFlatRow {
    pub id: Uuid,
    pub name: String,
    pub description_json: Option<JsonText>,
    pub description_html: String,
    pub priority: String,
    pub start_date: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub sequence_id: i32,
    pub sort_order: f64,
    pub is_draft: bool,
}

impl IssueFlatRow {
    /// Columns consumed: the 10 flat issue columns.
    pub const COLUMN_COUNT: usize = 10;

    pub fn from_row(row: &Row, start: usize) -> Result<Option<Self>, tokio_postgres::Error> {
        let Some(id) = row.try_get::<_, Option<Uuid>>(start)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            id,
            name: row.try_get(start + 1)?,
            description_json: row.try_get(start + 2)?,
            description_html: row.try_get(start + 3)?,
            priority: row.try_get(start + 4)?,
            start_date: row.try_get(start + 5)?,
            target_date: row.try_get(start + 6)?,
            sequence_id: row.try_get(start + 7)?,
            sort_order: row.try_get(start + 8)?,
            is_draft: row.try_get(start + 9)?,
        }))
    }

    pub fn serializer(&self) -> IssueFlatSerializer<'_> {
        IssueFlatSerializer {
            id: self.id,
            name: &self.name,
            description_json: self.description_json.as_ref().map(PgJson::from_json_text),
            description_html: &self.description_html,
            priority: &self.priority,
            start_date: self.start_date,
            target_date: self.target_date,
            sequence_id: i64::from(self.sequence_id),
            sort_order: self.sort_order,
            is_draft: self.is_draft,
        }
    }
}

// ---------------------------------------------------------------------------
// Comment rows (history and the comments endpoints share the shape)
// ---------------------------------------------------------------------------

/// `CommentReactionSerializer` (`plane/app/serializers/issue.py:…`) plus the
/// actor `display_name` its `source="actor.display_name"` needs.
#[derive(Debug)]
pub struct CommentReactionRow {
    pub reaction: plane_db::models::issue::CommentReaction,
    pub display_name: String,
}

impl CommentReactionRow {
    pub fn from_row(row: &Row, start: usize) -> Result<Self, tokio_postgres::Error> {
        let reaction = plane_db::models::issue::CommentReaction::from_row(row, start)?;
        Ok(Self {
            reaction,
            display_name: row.try_get(start + 11)?,
        })
    }

    pub fn serializer(&self) -> CommentReactionSerializer<'_> {
        CommentReactionSerializer {
            id: self.reaction.id,
            actor: self.reaction.actor_id,
            comment: self.reaction.comment_id,
            reaction: &self.reaction.reaction,
            display_name: &self.display_name,
            deleted_at: self.reaction.deleted_at,
            workspace: self.reaction.workspace_id,
            project: self.reaction.project_id,
            created_at: self.reaction.created_at,
            updated_at: self.reaction.updated_at,
            created_by: self.reaction.created_by_id,
            updated_by: self.reaction.updated_by_id,
        }
    }
}

/// One `IssueComment` row plus the nested serializers both comment readers
/// need.
#[derive(Debug)]
pub struct CommentRow {
    pub comment: plane_db::models::issue::IssueComment,
    pub issue: Option<IssueFlatRow>,
    pub project: Option<ProjectLiteRow>,
    pub workspace: Option<WorkspaceLiteRow>,
    pub actor: Option<UserLiteRow>,
    /// `IssueCommentViewSet.get_queryset`'s `is_member` annotation; `None` for
    /// the history endpoint, whose queryset has no annotation and whose
    /// serializer therefore skips the field.
    pub is_member: Option<bool>,
}

impl CommentRow {
    /// Column offsets inside the nested block.
    const ISSUE_AT: usize = 20;
    const PROJECT_AT: usize = Self::ISSUE_AT + IssueFlatRow::COLUMN_COUNT;
    const WORKSPACE_AT: usize = Self::PROJECT_AT + ProjectLiteRow::COLUMN_COUNT;
    const ACTOR_AT: usize = Self::WORKSPACE_AT + WorkspaceLiteRow::COLUMN_COUNT;
    const IS_MEMBER_AT: usize = Self::ACTOR_AT + UserLiteRow::COLUMN_COUNT;

    /// Decode one row; `is_member` is read from the column after the nested
    /// block when the statement selects it.
    pub fn from_row(row: &Row, start: usize, with_is_member: bool) -> Result<Self, ViewError> {
        let comment = plane_db::models::issue::IssueComment::from_row(row, start)?;
        Ok(Self {
            issue: IssueFlatRow::from_row(row, start + Self::ISSUE_AT)?,
            project: ProjectLiteRow::from_row(row, start + Self::PROJECT_AT)?,
            workspace: WorkspaceLiteRow::from_row(row, start + Self::WORKSPACE_AT)?,
            actor: UserLiteRow::from_row(row, start + Self::ACTOR_AT)?,
            is_member: if with_is_member {
                Some(row.try_get(start + Self::IS_MEMBER_AT)?)
            } else {
                None
            },
            comment,
        })
    }
}

/// A [`CommentRow`] with its nested serializers materialised, so the outer
/// serializer can borrow them (`IssueCommentSerializer` holds references).
pub struct CommentEntry<'a> {
    comment: &'a plane_db::models::issue::IssueComment,
    issue: Option<IssueFlatSerializer<'a>>,
    project: Option<ProjectLiteSerializer<'a>>,
    workspace: Option<WorkspaceLiteSerializer<'a>>,
    actor: Option<UserLiteSerializer<'a>>,
    reactions: Vec<CommentReactionSerializer<'a>>,
    /// `ArrayField` of `URLField`: a borrow per element for the `ListField`.
    attachments: Vec<&'a str>,
    is_member: Option<bool>,
}

impl<'a> CommentEntry<'a> {
    pub fn new(row: &'a CommentRow, reactions: &'a [&'a CommentReactionRow]) -> Self {
        Self {
            comment: &row.comment,
            issue: row.issue.as_ref().map(IssueFlatRow::serializer),
            project: row.project.as_ref().map(ProjectLiteRow::serializer),
            workspace: row.workspace.as_ref().map(WorkspaceLiteRow::serializer),
            actor: row.actor.as_ref().map(UserLiteRow::serializer),
            reactions: reactions
                .iter()
                .map(|reaction| reaction.serializer())
                .collect(),
            attachments: row.comment.attachments.iter().map(String::as_str).collect(),
            is_member: row.is_member,
        }
    }

    /// The comment's `created_at`, the history merge's sort key.
    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.comment.created_at
    }

    pub fn serializer(
        &self,
    ) -> plane_serializers::generated::app_issue::IssueCommentSerializer<'_> {
        plane_serializers::generated::app_issue::IssueCommentSerializer {
            id: self.comment.id,
            actor_detail: self.actor.as_ref(),
            issue_detail: self.issue.as_ref(),
            project_detail: self.project.as_ref(),
            workspace_detail: self.workspace.as_ref(),
            comment_reactions: &self.reactions,
            is_member: self.is_member,
            created_at: self.comment.created_at,
            updated_at: self.comment.updated_at,
            deleted_at: self.comment.deleted_at,
            comment_stripped: &self.comment.comment_stripped,
            comment_json: Some(PgJson::from_json_text(&self.comment.comment_json)),
            comment_html: &self.comment.comment_html,
            attachments: &self.attachments,
            access: &self.comment.access,
            external_source: self.comment.external_source.as_deref(),
            external_id: self.comment.external_id.as_deref(),
            edited_at: self.comment.edited_at,
            created_by: self.comment.created_by_id,
            updated_by: self.comment.updated_by_id,
            project: self.comment.project_id,
            workspace: self.comment.workspace_id,
            description: self.comment.description_id,
            issue: self.comment.issue_id,
            actor: self.comment.actor_id,
            parent: self.comment.parent_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// A URL kwarg parsed as a UUID. Django's `<uuid:…>` converter already rejected
/// a malformed segment, so this is the `UUIDField.to_python` failure path for a
/// route that is not converted.
pub fn kwarg_uuid(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    let value = ctx.kwarg(name).ok_or(ViewError::Key)?;
    Uuid::parse_str(value).map_err(|_| ViewError::Validation)
}

/// `request.user.id`. An anonymous caller reaching a handler raises the same
/// `ValidationError` Django's `filter(member=AnonymousUser)` does.
pub fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user.id().ok_or(ViewError::Validation)
}

/// `issue_in_project(issue_id, slug, project_id, include_archived=True)`
/// (`plane/app/views/issue/relation.py:34-56`).
pub async fn issue_in_project(
    client: &Client,
    issue_id: Uuid,
    project_id: Uuid,
    slug: &str,
) -> Result<bool, ViewError> {
    let statement = queries::ISSUE_IN_PROJECT.prepare(client).await?;
    let row = client
        .query_one(&statement, &[&issue_id, &project_id, &slug])
        .await?;
    Ok(row.get(0))
}

/// `issue_hidden_from_guest` (`plane/app/permissions/base.py:106-141`).
pub async fn issue_hidden_from_guest(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = queries::RESTRICTED_GUEST.prepare(client).await?;
    let row = client
        .query_one(&statement, &[&user, &project_id, &slug])
        .await?;
    if !row.get::<_, bool>(0) {
        return Ok(false);
    }
    let statement = queries::ISSUE_OWNED_BY.prepare(client).await?;
    let row = client
        .query_one(&statement, &[&issue_id, &project_id, &user, &slug])
        .await?;
    Ok(!row.get::<_, bool>(0))
}

/// `Response({"error": "You are not allowed to view this issue"}, 403)`.
pub fn hidden_response() -> ViewResponse {
    ViewResponse::json(403, serde_json::json!({ "error": HIDDEN_MESSAGE }))
}

/// `Response({"error": "Issue not found"}, 404)`.
pub fn issue_not_found_response() -> ViewResponse {
    ViewResponse::json(404, serde_json::json!({ "error": "Issue not found" }))
}

// ---------------------------------------------------------------------------
// `plane/utils/global_paginator.py` (`paginate`)
// ---------------------------------------------------------------------------

/// `PAGINATOR_MAX_LIMIT` (`plane/utils/global_paginator.py:6`).
pub const PAGINATOR_MAX_LIMIT: i64 = 1000;

/// What one page of a `paginate(...)` response needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPage {
    pub limit: i64,
    pub offset: i64,
    pub prev_cursor: String,
    pub cursor: String,
    pub next_cursor: Option<String>,
    pub prev_page_results: bool,
    pub next_page_results: bool,
    pub total_results: i64,
    pub total_pages: i64,
}

/// The response envelope, in `paginate`'s dict insertion order.
#[derive(Debug, serde::Serialize)]
pub struct PageEnvelope<T> {
    pub prev_cursor: String,
    pub cursor: String,
    pub next_cursor: Option<String>,
    pub prev_page_results: bool,
    pub next_page_results: bool,
    pub page_count: usize,
    pub total_results: i64,
    pub total_pages: i64,
    pub results: Vec<T>,
}

impl<T> PageEnvelope<T> {
    pub fn new(page: GlobalPage, results: Vec<T>) -> Self {
        Self {
            prev_cursor: page.prev_cursor,
            cursor: page.cursor,
            next_cursor: page.next_cursor,
            prev_page_results: page.prev_page_results,
            next_page_results: page.next_page_results,
            page_count: results.len(),
            total_results: page.total_results,
            total_pages: page.total_pages,
            results,
        }
    }
}

/// `int(text)` for a cursor field. Python trims whitespace, accepts a sign and
/// ignores single underscores between digits; a parse failure is the
/// `ValueError` `PaginateCursor.from_string` wraps.
fn python_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    let mut previous_underscore = true; // a leading `_` is not a digit separator
    for character in digits.chars() {
        if character == '_' {
            if previous_underscore {
                return None;
            }
            previous_underscore = true;
            continue;
        }
        let digit = character.to_digit(10)?;
        value = value.checked_mul(10)?.checked_add(i64::from(digit))?;
        previous_underscore = false;
    }
    if previous_underscore {
        return None;
    }
    Some(if negative { -value } else { value })
}

/// `PaginateCursor.from_string` + the arithmetic of `paginate`.
///
/// The cursor is `"<page size>:<page>:<offset>"`, the page size is clamped to
/// [`PAGINATOR_MAX_LIMIT`], and a non-positive page size is Python's
/// `ZeroDivisionError` (a 500, not a 400 — nothing catches it).
pub fn global_page(cursor: Option<&str>, total_results: i64) -> Result<GlobalPage, ViewError> {
    let (requested_size, current_page) = match cursor {
        None => (PAGINATOR_MAX_LIMIT, 0),
        Some(text) => {
            let bits: Vec<&str> = text.split(':').collect();
            if bits.len() != 3 {
                return Err(invalid_cursor());
            }
            let size = python_int(bits[0]).ok_or_else(invalid_cursor)?;
            let page = python_int(bits[1]).ok_or_else(invalid_cursor)?;
            if python_int(bits[2]).is_none() {
                return Err(invalid_cursor());
            }
            (size, page)
        }
    };
    let page_size = requested_size.min(PAGINATOR_MAX_LIMIT);
    if page_size <= 0 {
        return Err(ViewError::Unknown(
            "ZeroDivisionError: division by zero in global_paginator.paginate".to_string(),
        ));
    }
    // `ceil(total_results / page_size)` in integer arithmetic.
    let total_pages = (total_results + page_size - 1) / page_size;
    let start_index = if current_page > 0 {
        current_page.saturating_mul(page_size)
    } else {
        0
    };
    let end_index = start_index.saturating_add(page_size).min(total_results);
    let limit = (end_index - start_index).max(0);
    let offset = start_index.min(end_index);
    let next_cursor = if end_index < total_results {
        Some(format!("{page_size}:{}:0", current_page + 1))
    } else {
        None
    };
    Ok(GlobalPage {
        limit,
        offset,
        prev_cursor: format!("{page_size}:{}:0", current_page - 1),
        cursor: format!("{page_size}:{current_page}:0"),
        next_cursor,
        prev_page_results: current_page > 0,
        next_page_results: end_index < total_results,
        total_results,
        total_pages,
    })
}

fn invalid_cursor() -> ViewError {
    // `PaginateCursor.from_string` raises `ValueError`; no view catches it, so
    // the request is a 500 (`plane/utils/global_paginator.py:23-33`).
    ViewError::Unknown("ValueError: Invalid cursor format".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_cursor_is_the_maximum_page() {
        let page = global_page(None, 1).expect("page");
        assert_eq!(page.prev_cursor, "1000:-1:0");
        assert_eq!(page.cursor, "1000:0:0");
        assert_eq!(page.next_cursor, None);
        assert!(!page.prev_page_results);
        assert_eq!(page.total_pages, 1);
        assert_eq!((page.limit, page.offset), (1, 0));
    }

    #[test]
    fn a_page_beyond_the_end_is_empty_with_a_previous_cursor() {
        let page = global_page(Some("2:1:0"), 1).expect("page");
        assert_eq!(page.cursor, "2:1:0");
        assert_eq!(page.prev_cursor, "2:0:0");
        assert!(page.prev_page_results);
        assert_eq!(page.total_pages, 1);
        assert_eq!(page.limit, 0);
    }

    #[test]
    fn a_negative_page_starts_at_the_first_result() {
        let page = global_page(Some("1000:-1:0"), 3).expect("page");
        assert_eq!(page.prev_cursor, "1000:-2:0");
        assert_eq!(page.offset, 0);
        assert_eq!(page.limit, 3);
        assert!(!page.prev_page_results);
    }

    #[test]
    fn a_zero_page_size_is_a_server_error() {
        assert!(matches!(
            global_page(Some("0:0:0"), 3),
            Err(ViewError::Unknown(_))
        ));
    }

    #[test]
    fn cursor_bits_parse_like_python_int() {
        assert_eq!(python_int(" 2 "), Some(2));
        assert_eq!(python_int("+2"), Some(2));
        assert_eq!(python_int("-2"), Some(-2));
        assert_eq!(python_int("1_0"), Some(10));
        assert_eq!(python_int("_1"), None);
        assert_eq!(python_int("1_"), None);
        assert_eq!(python_int("x"), None);
        assert_eq!(python_int(""), None);
    }

    #[test]
    fn a_malformed_cursor_is_a_server_error() {
        for cursor in ["1:2", "a:0:0", "1:b:0", "1:0:c", "1:0:0:0"] {
            assert!(
                matches!(global_page(Some(cursor), 1), Err(ViewError::Unknown(_))),
                "{cursor}"
            );
        }
    }
}
