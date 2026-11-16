// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/home.py`: the workspace home widget preferences.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/home-preferences/` | `WorkspaceHomePreferenceViewSet.get` | `home.py:23-65` |
//! | `GET /api/workspaces/{slug}/home-preferences/{key}/` | the same `get` | `home.py:23-65` |
//!
//! The second route resolves to the same method, which takes `(self, request,
//! slug)`; DRF calls it with `key=` as well, so it raises
//! `TypeError: get() got an unexpected keyword argument 'key'` and the family
//! handler answers `500 {"error": "Something went wrong please try again
//! later"}`. That is an upstream bug, ported as it is.
//!
//! ## Side effect: the GET creates rows
//!
//! `get` creates a row for every widget key the user does not have yet
//! (`home.py:39-58`). The loop is re-entrant in an unusual way: each iteration
//! re-inserts *every* key collected so far with the current `sort_order`, and
//! `ignore_conflicts=True` drops the ones already inserted. The net effect —
//! reproduced here — is that the key found at position *m* (1-based, in
//! `HomeWidgetKeys` order) gets `sort_order = 1000 - m`.
//!
//! `bulk_create` never calls `Model.save()`, so the audit columns stay `NULL`
//! and only `created_at`/`updated_at` are set, by the fields' `auto_now*`
//! (`plane/db/models/base.py:21-40`).

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows;

/// `WorkspaceHomePreference.HomeWidgetKeys` in declaration order
/// (`plane/db/models/workspace.py:373-378`), minus the two the view excludes
/// (`home.py:31-35`).
const WIDGET_KEYS: [&str; 3] = ["quick_links", "recents", "my_stickies"];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.home.WorkspaceHomePreferenceViewSet")?
        .method(Method::GET, get)?;
    Ok(())
}

/// One `values("key", "is_enabled", "config", "sort_order")` row.
#[derive(Debug)]
struct PreferenceRow {
    key: String,
    is_enabled: bool,
    config: PgJsonOwned,
    sort_order: f64,
}

/// `config` is a rendered-as-raw JSON value; the struct keeps its text so the
/// writer can transcode it without re-encoding.
#[derive(Debug)]
struct PgJsonOwned(String);

impl DrfSerialize for PreferenceRow {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field("key", &self.key)?;
        object.field("is_enabled", &self.is_enabled)?;
        object.field_json("config", Some(PgJson::new(self.config.0.as_str())))?;
        object.field("sort_order", &self.sort_order)?;
        object.end();
        Ok(())
    }
}

/// `WorkspaceHomePreferenceViewSet.get` (`home.py:23-65`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `GET /home-preferences/{key}/` binds `key` and the method takes none.
        if ctx.kwarg("key").is_some() {
            return Err(ViewError::Unknown(
                "WorkspaceHomePreferenceViewSet.get() got an unexpected keyword argument 'key'"
                    .to_string(),
            ));
        }
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`home.py:25`).
        let statement = client
            .prepare_cached(queries::WORKSPACE_ID_BY_SLUG.sql)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&slug]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace: Uuid = row.try_get(0)?;

        // `get_preference.values_list("key", flat=True)`, re-read every
        // iteration by the Python; one read is enough here because this port
        // inserts the missing keys in one statement afterwards.
        let statement = client
            .prepare_cached(queries::HOME_PREFERENCES_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&user, &workspace]).await?;
        let existing: Vec<String> = rows
            .iter()
            .map(|row| row.try_get::<_, String>(0).map_err(ViewError::from))
            .collect::<Result<_, _>>()?;

        let missing: Vec<&str> = WIDGET_KEYS
            .iter()
            .copied()
            .filter(|key| !existing.iter().any(|have| have == key))
            .collect();
        if !missing.is_empty() {
            insert_defaults(client, workspace, user, &missing).await?;
        }

        // `preference.values(...)`: the default `-created_at` order, so the
        // newest inserted widget comes first.
        let rows = client.query(&statement, &[&user, &workspace]).await?;
        let mut preferences = Vec::with_capacity(rows.len());
        for row in &rows {
            preferences.push(PreferenceRow {
                key: row.try_get(0)?,
                is_enabled: row.try_get(1)?,
                config: PgJsonOwned(row.try_get::<_, plane_db::JsonText>(2)?.into_string()),
                sort_order: row.try_get(3)?,
            });
        }
        let timezone = rows::timezone(ctx);
        let body = plane_serializers::render_list(&preferences, &rows::plain_ctx(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// Create the missing widget preferences (`home.py:45-57`).
///
/// One statement instead of the Python's one `bulk_create` per missing key: the
/// rows it would insert are exactly these, with `sort_order = 1000 - m`, and
/// `ON CONFLICT DO NOTHING` is `ignore_conflicts=True`. The timestamps increase
/// by one millisecond per row so `ORDER BY created_at DESC` keeps the Python's
/// creation order deterministically.
async fn insert_defaults(
    client: &deadpool_postgres::Client,
    workspace: Uuid,
    user: Uuid,
    missing: &[&str],
) -> Result<(), ViewError> {
    let mut sql = String::from(
        "INSERT INTO workspace_home_preferences \
         (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, workspace_id, \
          user_id, key, is_enabled, config, sort_order) VALUES ",
    );
    struct Owner {
        created: chrono::DateTime<chrono::Utc>,
        id: Uuid,
        workspace: Uuid,
        user: Uuid,
        key: String,
        sort_order: f64,
    }
    let now = plane_core::clock::now();
    let owners: Vec<Owner> = missing
        .iter()
        .enumerate()
        .map(|(position, key)| Owner {
            // One millisecond apart so `ORDER BY created_at DESC` keeps the
            // Python's creation order deterministically.
            created: now + chrono::TimeDelta::milliseconds(position as i64),
            id: Uuid::new_v4(),
            workspace,
            user,
            key: (*key).to_string(),
            sort_order: 1000.0 - (position as f64 + 1.0),
        })
        .collect();
    for index in 0..owners.len() {
        if index > 0 {
            sql.push(',');
        }
        let base = index * 7;
        sql.push_str(&format!(
            "(${},${},NULL,NULL,NULL,${},${},${},${},true,'{{}}'::jsonb,${})",
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 7
        ));
    }
    sql.push_str(" ON CONFLICT DO NOTHING");

    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
        Vec::with_capacity(owners.len() * 7);
    for owner in &owners {
        params.push(&owner.created);
        params.push(&owner.created);
        params.push(&owner.id);
        params.push(&owner.workspace);
        params.push(&owner.user);
        params.push(&owner.key);
        params.push(&owner.sort_order);
    }
    client.execute(sql.as_str(), &params).await?;
    Ok(())
}
