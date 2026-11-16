// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/page/version.py` — `PageVersionEndpoint.get`.
//!
//! One `APIView` method serves both routes of the `pages` group:
//!
//! * `GET …/pages/{page_id}/versions/` — `PageVersionSerializer(page_versions,
//!   many=True)`;
//! * `GET …/pages/{page_id}/versions/{pk}/` — `PageVersionDetailSerializer(
//!   page_version)`.
//!
//! Both queries are scoped to an **active** `ProjectPage` link for the URL
//! project (`version.py:22-38,43-50`, the GHSA-g49r / GHSA-ghcr fix). The
//! distinction is the presence of the `pk` URL kwarg (`version.py:21`), which is
//! how the two routes reach the same handler.
//!
//! `.get()` (`version.py:29-39`) raises `PageVersion.DoesNotExist` on a miss and
//! `MultipleObjectsReturned` on more than one row; both subclass
//! `ObjectDoesNotExist`, so both render the family handler's 404
//! (`plane/app/views/base.py:318-322`). The statement keeps Django's `LIMIT 21`
//! and the handler checks the row count, so a duplicate link would not silently
//! answer 200.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_page::{PageVersionDetailSerializer, PageVersionSerializer};
use plane_serializers::{PgJson, RenderCtx};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::common::client;
use crate::pages::base::{Params, params, timezone, uuid_kwarg};
use crate::pages::queries;

/// One `page_versions` row, with the detail-only columns optional in the sense
/// that the list statement does not select them.
#[derive(Debug)]
struct VersionRow {
    id: Uuid,
    workspace_id: Uuid,
    page_id: Uuid,
    last_saved_at: DateTime<Utc>,
    owned_by_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    description_binary: Option<Vec<u8>>,
    description_html: String,
    description_json: JsonText,
}

impl VersionRow {
    /// The list statement's 9 columns (`version.py:45-50`).
    fn decode_list(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            workspace_id: row.try_get(1).map_err(ViewError::from)?,
            page_id: row.try_get(2).map_err(ViewError::from)?,
            last_saved_at: row.try_get(3).map_err(ViewError::from)?,
            owned_by_id: row.try_get(4).map_err(ViewError::from)?,
            created_at: row.try_get(5).map_err(ViewError::from)?,
            updated_at: row.try_get(6).map_err(ViewError::from)?,
            created_by_id: row.try_get(7).map_err(ViewError::from)?,
            updated_by_id: row.try_get(8).map_err(ViewError::from)?,
            description_binary: None,
            description_html: String::new(),
            description_json: JsonText::new("{}"),
        })
    }

    /// The detail statement's 12 columns (`version.py:29-42`).
    fn decode_detail(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            workspace_id: row.try_get(1).map_err(ViewError::from)?,
            page_id: row.try_get(2).map_err(ViewError::from)?,
            last_saved_at: row.try_get(3).map_err(ViewError::from)?,
            description_binary: row.try_get(4).map_err(ViewError::from)?,
            description_html: row.try_get(5).map_err(ViewError::from)?,
            description_json: row.try_get(6).map_err(ViewError::from)?,
            owned_by_id: row.try_get(7).map_err(ViewError::from)?,
            created_at: row.try_get(8).map_err(ViewError::from)?,
            updated_at: row.try_get(9).map_err(ViewError::from)?,
            created_by_id: row.try_get(10).map_err(ViewError::from)?,
            updated_by_id: row.try_get(11).map_err(ViewError::from)?,
        })
    }

    /// `PageVersionSerializer(page_version).data`.
    fn serializer(&self) -> PageVersionSerializer {
        PageVersionSerializer {
            id: self.id,
            workspace: self.workspace_id,
            page: self.page_id,
            last_saved_at: self.last_saved_at,
            owned_by: self.owned_by_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by: self.created_by_id,
            updated_by: self.updated_by_id,
        }
    }

    /// `PageVersionDetailSerializer(page_version).data`.
    fn detail(&self) -> PageVersionDetailSerializer<'_> {
        PageVersionDetailSerializer {
            id: self.id,
            workspace: self.workspace_id,
            page: self.page_id,
            last_saved_at: self.last_saved_at,
            // `BinaryField` is a `ModelField`; DRF base64-encodes the bytes and
            // a NULL column stays `null` (`SERIALIZERS.md`).
            description_binary: self.description_binary.as_deref(),
            description_html: &self.description_html,
            description_json: Some(PgJson::from(&self.description_json)),
            owned_by: self.owned_by_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            created_by: self.created_by_id,
            updated_by: self.updated_by_id,
        }
    }
}

/// `PageVersionEndpoint.get` (`page/version.py:19-53`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let Params {
            slug, project_id, ..
        } = params(ctx)?;
        let page_id = uuid_kwarg(ctx, "page_id")?;
        let timezone = timezone(ctx);

        // `if pk:` — the detail route passes it, the list route does not
        // (`version.py:21`).
        if ctx.kwarg("pk").is_some() {
            let pk = uuid_kwarg(ctx, "pk")?;
            let rows = fetch(
                client,
                &queries::PAGES_VERSION_DETAIL,
                &[&project_id, &page_id, &slug, &pk],
            )
            .await?;
            // `.get()`: zero rows is `DoesNotExist`, more than one is
            // `MultipleObjectsReturned`; both answer the same 404.
            if rows.len() != 1 {
                return Err(ViewError::ObjectDoesNotExist);
            }
            let version = VersionRow::decode_detail(&rows[0])?;
            let bytes = plane_serializers::render(&version.detail(), &RenderCtx::full(&timezone))?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes));
        }

        let rows = fetch(
            client,
            &queries::PAGES_VERSIONS,
            &[&project_id, &page_id, &slug],
        )
        .await?;
        let versions = rows
            .iter()
            .map(VersionRow::decode_list)
            .collect::<Result<Vec<_>, _>>()?;
        let values: Vec<PageVersionSerializer> =
            versions.iter().map(VersionRow::serializer).collect();
        let bytes = plane_serializers::render_list(&values, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

async fn fetch(
    client: &Client,
    query: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<Row>, ViewError> {
    let statement = client
        .prepare_cached(query.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .query(&statement, params)
        .await
        .map_err(ViewError::from)
}
