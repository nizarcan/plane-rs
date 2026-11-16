// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/user_preference.py`: the sidebar preferences.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/sidebar-preferences/` | `WorkspaceUserPreferenceViewSet.get` | `user_preference.py:25-79` |
//!
//! ## Side effect: the GET creates rows
//!
//! Like the home preferences, `get` creates the keys the user does not have
//! (`user_preference.py:33-61`). Each iteration re-inserts every key collected
//! so far, `ignore_conflicts=True` keeps the first, so the key found at
//! position `i` (0-based, in `UserPreferenceKeys` order) gets
//! `sort_order = 65535 + i * 10000`; that is exactly the index of the first
//! round that inserted it.
//!
//! `is_pinned` is true for `drafts`, `your_work` and `stickies`
//! (`user_preference.py:46-55`). `bulk_create` never calls `Model.save()`, so
//! the audit columns stay `NULL`.
//!
//! The response is a dict keyed by preference key in `sort_order` order — the
//! iteration order of the `values()` queryset — and duplicated keys overwrite,
//! which is how the Python's `user_preferences[key] = {...}` behaves.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::sql::{Sql, Value};
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows;

/// `WorkspaceUserPreference.UserPreferenceKeys` in declaration order
/// (`plane/db/models/workspace.py:422-430`).
const PREFERENCE_KEYS: [&str; 7] = [
    "views",
    "active_cycles",
    "analytics",
    "drafts",
    "your_work",
    "archives",
    "stickies",
];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.user_preference.WorkspaceUserPreferenceViewSet")?
        .method(Method::GET, get)?;
    Ok(())
}

/// One `{is_pinned, sort_order}` entry.
#[derive(Debug)]
struct PreferenceEntry {
    is_pinned: bool,
    sort_order: f64,
}

impl DrfSerialize for PreferenceEntry {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        object.field("is_pinned", &self.is_pinned)?;
        object.field("sort_order", &self.sort_order)?;
        object.end();
        Ok(())
    }
}

/// The response object: keys in the `values()` iteration order, each mapping to
/// a two-key object.
#[derive(Debug)]
struct PreferencesBody {
    entries: Vec<(String, PreferenceEntry)>,
}

impl DrfSerialize for PreferencesBody {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut object = w.begin_object();
        for (key, entry) in &self.entries {
            object.field_nested(key, ctx, Some(entry))?;
        }
        object.end();
        Ok(())
    }
}

/// `WorkspaceUserPreferenceViewSet.get` (`user_preference.py:25-79`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`user_preference.py:27`).
        let statement = client
            .prepare_cached(queries::WORKSPACE_ID_BY_SLUG.sql)
            .await?;
        let Some(row) = client.query_opt(&statement, &[&slug]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace: Uuid = row.try_get(0)?;

        let statement = client
            .prepare_cached(queries::USER_PREFERENCES_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&user, &workspace]).await?;
        let existing: Vec<String> = rows
            .iter()
            .map(|row| row.try_get::<_, String>(0).map_err(ViewError::from))
            .collect::<Result<_, _>>()?;

        let missing: Vec<&str> = PREFERENCE_KEYS
            .iter()
            .copied()
            .filter(|key| !existing.iter().any(|have| have == key))
            .collect();
        if !missing.is_empty() {
            insert_defaults(client, workspace, user, &missing).await?;
        }

        let rows = client.query(&statement, &[&user, &workspace]).await?;
        let mut entries = Vec::with_capacity(rows.len());
        for row in &rows {
            let key: String = row.try_get(0)?;
            let is_pinned: bool = row.try_get(1)?;
            let sort_order: f64 = row.try_get(2)?;
            // `user_preferences[key] = {...}`: a repeated key keeps its first
            // position and takes the last value, which is what a map insert
            // does. `sort_order` is not unique, so repeats are possible.
            match entries.iter_mut().find(|(have, _)| *have == key) {
                Some((_, entry)) => {
                    *entry = PreferenceEntry {
                        is_pinned,
                        sort_order,
                    }
                }
                None => entries.push((
                    key,
                    PreferenceEntry {
                        is_pinned,
                        sort_order,
                    },
                )),
            }
        }
        let body = PreferencesBody { entries };
        let timezone = rows::timezone(ctx);
        let rendered = plane_serializers::render(&body, &rows::plain_ctx(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, rendered))
    })
}

/// Create the missing preferences (`user_preference.py:39-61`).
///
/// One statement instead of one `bulk_create` per missing key; each row's
/// `sort_order` is `65535 + i * 10000`, where `i` is its 0-based position among
/// the missing keys, and `ON CONFLICT DO NOTHING` is `ignore_conflicts=True`.
async fn insert_defaults(
    client: &deadpool_postgres::Client,
    workspace: Uuid,
    user: Uuid,
    missing: &[&str],
) -> Result<(), ViewError> {
    struct Row {
        created: chrono::DateTime<chrono::Utc>,
        id: Uuid,
        workspace: Uuid,
        user: Uuid,
        key: String,
        is_pinned: bool,
        sort_order: f64,
    }
    let now = plane_core::clock::now();
    let rows: Vec<Row> = missing
        .iter()
        .enumerate()
        .map(|(index, key)| Row {
            // One millisecond apart so the ordering (which `sort_order` already
            // fixes) is deterministic in the stored rows too.
            created: now + chrono::TimeDelta::milliseconds(index as i64),
            id: Uuid::new_v4(),
            workspace,
            user,
            key: (*key).to_string(),
            is_pinned: matches!(*key, "drafts" | "your_work" | "stickies"),
            sort_order: 65535.0 + (index as f64) * 10000.0,
        })
        .collect();

    // created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
    // workspace_id, user_id, key, is_pinned, sort_order.
    let mut sql = String::from(
        "INSERT INTO workspace_user_preferences \
         (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, workspace_id, \
          user_id, key, is_pinned, sort_order) VALUES ",
    );
    for index in 0..rows.len() {
        if index > 0 {
            sql.push(',');
        }
        let base = index * 8;
        sql.push_str(&format!(
            "(${}, ${}, NULL, NULL, NULL, ${}, ${}, ${}, ${}, ${}, ${})",
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 7,
            base + 8
        ));
    }
    sql.push_str(" ON CONFLICT DO NOTHING");

    let mut params: Vec<Value> = Vec::with_capacity(rows.len() * 8);
    for row in &rows {
        params.push(Value::Timestamp(row.created));
        params.push(Value::Timestamp(row.created));
        params.push(Value::Uuid(row.id));
        params.push(Value::Uuid(row.workspace));
        params.push(Value::Uuid(row.user));
        params.push(Value::Text(row.key.clone()));
        params.push(Value::Bool(row.is_pinned));
        params.push(Value::Float8(row.sort_order));
    }
    Sql { text: sql, params }.execute(client).await?;
    Ok(())
}
