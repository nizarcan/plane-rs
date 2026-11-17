// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/workspaces/{slug}/export-issues/` —
//! `ExportIssuesEndpoint.get` (`plane/app/views/exporter/base.py:64-81`).
//!
//! Export *history*: the exports the caller initiated, paginated with the plain
//! `OffsetPaginator`. `?per_page=` and `?cursor=` are both required — the view
//! answers 400 `{"error": "per_page and cursor are required"}` otherwise — and
//! the ordering is `sanitize_order_by(order_by, ["created_at", "updated_at"],
//! "-created_at")`.
//!
//! `initiated_by_detail` is a nested `UserLiteSerializer`; its `avatar_url`
//! property is `/api/assets/v2/static/<asset id>/` for a user avatar asset, the
//! `avatar` column when that is set, else `null` — no object-storage call
//! (`plane/db/models/asset.py:84-91`).

use deadpool_postgres::Client;
use http::Method;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::paginator::{Cursor, MAX_LIMIT, PageQuery};
use plane_query::sql::{Frag, Join, Select};
use plane_serializers::generated::app_exporter::ExporterHistorySerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use uuid::Uuid;

use super::list_common::{client, kwarg_text, user_id};
use crate::common::RequestRender;

/// Register `ExportIssuesEndpoint`'s two verbs: the history `GET` here and the
/// export `POST` in [`crate::issues::write_export`].
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.exporter.base.ExportIssuesEndpoint")?
        .method(Method::GET, get)?
        .method(Method::POST, crate::issues::write_export::post)?;
    Ok(())
}

/// `ExportIssuesEndpoint.get`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `if request.GET.get("per_page") and request.GET.get("cursor")`.
        match (
            ctx.query()
                .get("per_page")
                .filter(|value| !value.is_empty()),
            ctx.query().get("cursor").filter(|value| !value.is_empty()),
        ) {
            (Some(_), Some(_)) => {}
            _ => {
                return Ok(super::list_common::error_body(
                    400,
                    "per_page and cursor are required",
                ));
            }
        }

        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let page =
            PageQuery::parse(ctx.query(), None, None).map_err(super::list_common::query_error)?;

        // `sanitize_order_by(request.GET.get("order_by", "-created_at"),
        // EXPORTER_HISTORY_ORDER_BY_ALLOWLIST, "-created_at")`.
        let order_by = sanitize_order_by(ctx.query().get_or("order_by", "-created_at"));
        let (descending, key) = match order_by.strip_prefix('-') {
            Some(bare) => (true, bare),
            None => (false, order_by.as_str()),
        };
        let column = if key == "updated_at" {
            "exporters.updated_at"
        } else {
            "exporters.created_at"
        };

        let limit = i64::from(page.per_page.min(MAX_LIMIT));
        let offset = page.cursor.offset * limit;

        let mut select = base_select(slug, user);
        select.projection = vec![
            Frag::raw("exporters.id"),
            Frag::raw("exporters.created_at"),
            Frag::raw("exporters.updated_at"),
            Frag::raw("exporters.project"),
            Frag::raw("exporters.provider"),
            Frag::raw("exporters.status"),
            Frag::raw("exporters.url"),
            Frag::raw("exporters.initiated_by_id"),
            Frag::raw("exporters.token"),
            Frag::raw("exporters.created_by_id"),
            Frag::raw("exporters.updated_by_id"),
            Frag::raw("initiated_by_user.id"),
            Frag::raw("initiated_by_user.first_name"),
            Frag::raw("initiated_by_user.last_name"),
            Frag::raw("initiated_by_user.avatar"),
            Frag::raw("initiated_by_user.avatar_asset_id"),
            Frag::raw("initiated_by_user.is_bot"),
            Frag::raw("initiated_by_user.display_name"),
        ];
        select.order_by = vec![
            Frag::raw(format!(
                "{column} {} NULLS LAST",
                if descending { "DESC" } else { "ASC" }
            )),
            Frag::raw("exporters.created_at DESC"),
        ];
        select.limit = Some(limit + 1);
        select.offset = Some(offset);

        let rows = fetch(client, &select).await?;
        let has_results = rows.len() as i64 > limit;
        let rows: Vec<&tokio_postgres::Row> = rows.iter().take(limit as usize).collect();

        let mut count_select = base_select(slug, user);
        count_select.projection = vec![Frag::raw("count(*)")];
        let total = count(client, &count_select).await?;

        let total_pages = if limit == 0 {
            0
        } else {
            (total + limit - 1) / limit
        };
        let next_cursor = Cursor::new(limit, page.cursor.offset + 1, false, has_results);
        let prev_cursor = Cursor::new(limit, page.cursor.offset - 1, true, page.cursor.offset > 0);

        let prepared = RequestRender::from_ctx(ctx);
        let render_ctx = prepared.render_ctx();

        // The `OffsetPaginator` envelope, in `Response({...})` key order.
        let mut body = Vec::with_capacity(rows.len() * 256 + 256);
        body.extend_from_slice(b"{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":");
        body.extend_from_slice(total.to_string().as_bytes());
        body.extend_from_slice(b",\"next_cursor\":\"");
        body.extend_from_slice(next_cursor.to_string().as_bytes());
        body.extend_from_slice(b"\",\"prev_cursor\":\"");
        body.extend_from_slice(prev_cursor.to_string().as_bytes());
        body.extend_from_slice(b"\",\"next_page_results\":");
        body.extend_from_slice(if has_results { b"true" } else { b"false" });
        body.extend_from_slice(b",\"prev_page_results\":");
        body.extend_from_slice(if page.cursor.offset > 0 {
            b"true"
        } else {
            b"false"
        });
        body.extend_from_slice(b",\"count\":");
        body.extend_from_slice(rows.len().to_string().as_bytes());
        body.extend_from_slice(b",\"total_pages\":");
        body.extend_from_slice(total_pages.to_string().as_bytes());
        body.extend_from_slice(b",\"total_results\":");
        body.extend_from_slice(total.to_string().as_bytes());
        body.extend_from_slice(b",\"extra_stats\":null,\"results\":[");
        for (index, row) in rows.iter().enumerate() {
            if index != 0 {
                body.push(b',');
            }
            let project: Option<Vec<Uuid>> = row.get(3);
            let provider: String = row.get(4);
            let status: String = row.get(5);
            let url: Option<String> = row.get(6);
            let token: String = row.get(8);
            let user = UserRow::from(row);
            let avatar_url = user.avatar_url();
            let detail = UserLiteSerializer {
                id: user.id,
                first_name: &user.first_name,
                last_name: &user.last_name,
                avatar: &user.avatar,
                avatar_url: avatar_url.as_deref(),
                is_bot: user.is_bot,
                display_name: &user.display_name,
            };
            let serializer = ExporterHistorySerializer {
                id: row.get(0),
                created_at: row.get(1),
                updated_at: row.get(2),
                project: project.as_deref(),
                provider: &provider,
                status: &status,
                url: url.as_deref(),
                initiated_by: row.get(7),
                initiated_by_detail: Some(&detail),
                token: &token,
                created_by: row.get(9),
                updated_by: row.get(10),
            };
            body.extend(plane_serializers::render(&serializer, &render_ctx)?);
        }
        body.extend_from_slice(b"]}");
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            body,
        ))
    })
}

/// The filtered `ExporterHistory` queryset of the view
/// (`plane/app/views/exporter/base.py:68-70`).
fn base_select(slug: &str, user: Uuid) -> Select {
    let mut select = Select::new("exporters")
        .join(Join::inner(
            "workspaces",
            "workspaces",
            Frag::raw("exporters.workspace_id = workspaces.id"),
        ))
        .join(Join::left(
            "users",
            "initiated_by_user",
            Frag::raw("exporters.initiated_by_id = initiated_by_user.id"),
        ));
    // The slug and the user are bound in `base_select`'s `Select` order.
    select.predicate = Some(
        Frag::raw("workspaces.slug = ")
            .append(Frag::bind(plane_query::sql::Value::Text(slug.to_owned())))
            .and(Frag::raw("exporters.type = 'issue_exports'"))
            .and(
                Frag::raw("exporters.initiated_by_id = ")
                    .append(Frag::bind(plane_query::sql::Value::Uuid(user))),
            )
            .and(Frag::raw("exporters.deleted_at IS NULL")),
    );
    select
}

/// `UserLiteSerializer`'s source row, owned so `avatar_url` can be built.
struct UserRow {
    id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_asset_id: Option<Uuid>,
    is_bot: bool,
    display_name: String,
}

impl UserRow {
    fn from(row: &tokio_postgres::Row) -> Self {
        Self {
            id: row.get(11),
            first_name: row.get(12),
            last_name: row.get(13),
            avatar: row.get(14),
            avatar_asset_id: row.get(15),
            is_bot: row.get(16),
            display_name: row.get(17),
        }
    }

    /// `User.avatar_url` (`plane/db/models/user.py:143-151`), owned so the
    /// serializer can borrow it.
    fn avatar_url(&self) -> Option<String> {
        if let Some(asset_id) = self.avatar_asset_id {
            return Some(format!("/api/assets/v2/static/{asset_id}/"));
        }
        if self.avatar.is_empty() {
            None
        } else {
            Some(self.avatar.clone())
        }
    }
}

/// `sanitize_order_by(value, EXPORTER_HISTORY_ORDER_BY_ALLOWLIST, "-created_at")`.
fn sanitize_order_by(value: &str) -> String {
    const ALLOWED: [&str; 2] = ["created_at", "updated_at"];
    let default = "-created_at".to_owned();
    if value.is_empty() {
        return default;
    }
    let (descending, bare) = match value.strip_prefix('-') {
        Some(bare) => (true, bare),
        None => (false, value),
    };
    if bare.starts_with('-') || !ALLOWED.contains(&bare) {
        return default;
    }
    if descending {
        format!("-{bare}")
    } else {
        bare.to_owned()
    }
}

async fn fetch(client: &Client, select: &Select) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.clone().into_sql();
    sql.query(client).await.map_err(ViewError::from)
}

async fn count(client: &Client, select: &Select) -> Result<i64, ViewError> {
    let sql = select.clone().into_sql();
    let row = sql
        .query(client)
        .await
        .map_err(ViewError::from)?
        .into_iter()
        .next()
        .ok_or_else(|| ViewError::Unknown("count returned no row".to_owned()))?;
    row.try_get(0).map_err(ViewError::from)
}
