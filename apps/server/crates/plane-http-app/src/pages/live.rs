// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/page/live.py` — `PageLiveAccessEndpoint.get`.
//!
//! `GET /api/workspaces/{slug}/pages/{page_id}/live-access/?action=view|edit`.
//! The live (Hocuspocus) server asks this on every connection with the user's
//! own cookie (`live.py:16-28`), so the rules mirror `ProjectPagePermission`:
//! an active project member, only the owner for a private page, and a guest
//! only their own pages unless the project enables `guest_view_all_features`.
//! Every denial is a 404 so the route never confirms that a page exists.
//!
//! The loop over the page's project links is `live.py:46-71` line for line: the
//! ordering is `ProjectPage.Meta.ordering = ("-created_at",)`, the first link
//! that grants the action wins, and a read-only link is remembered so a page
//! linked to several projects still answers `read_only: true` when no link
//! allows editing. The guest test is one `Project.objects…exists()` per
//! guest-readable link (`live.py:52-55`), issued lazily inside the loop exactly
//! as Python does.
//!
//! `Response({...})` is a plain dict, so the key order is the literal's:
//! `project_id` then `read_only` for `action=view`, `project_id` alone for
//! `action=edit`.

use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use crate::common::{client, jobs};
use crate::cycles::base as cycle_base;
use crate::pages::queries;

/// `Page.PRIVATE_ACCESS` (`plane/db/models/page.py:24`).
const PRIVATE_ACCESS: i16 = 1;
/// `ROLE.GUEST.value` (`plane/app/permissions/__init__.py`).
const GUEST: i16 = 5;

/// Which answer the caller asked for (`live.py:31-33`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    View,
    Edit,
}

/// `{"project_id": str(project_id)}` — `live.py:62`.
#[derive(Debug, serde::Serialize)]
struct EditBody {
    project_id: String,
}

/// `{"project_id": str(project_id), "read_only": read_only}` — `live.py:67,70`.
#[derive(Debug, serde::Serialize)]
struct ViewBody {
    project_id: String,
    read_only: bool,
}

/// `PageLiveAccessEndpoint.get` (`page/live.py:30-71`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let action = match ctx.query().get("action") {
            Some("view") => Action::View,
            Some("edit") => Action::Edit,
            _ => return Ok(cycle_base::error_body(400, "action must be view or edit")),
        };
        let slug = ctx
            .kwarg("slug")
            .map(str::to_string)
            .ok_or(ViewError::Key)?;
        let page_id = crate::pages::base::uuid_kwarg(ctx, "page_id")?;
        let user_id = jobs::user_id(ctx)?;

        let statement = client
            .prepare_cached(queries::PAGES_LIVE_ACCESS_LINKS.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&page_id, &user_id, &slug])
            .await
            .map_err(ViewError::from)?;

        // `read_only_link = read_only_link or project_id` (`live.py:65`).
        let mut read_only_link: Option<Uuid> = None;
        for row in &rows {
            let project_id: Uuid = row.try_get(0).map_err(ViewError::from)?;
            let role: i16 = row.try_get(1).map_err(ViewError::from)?;
            let owner_id: Uuid = row.try_get(2).map_err(ViewError::from)?;
            let access: i16 = row.try_get(3).map_err(ViewError::from)?;

            let is_owner = owner_id == user_id;
            if !is_owner {
                if access == PRIVATE_ACCESS {
                    continue;
                }
                if role == GUEST && !guest_view_all(client, project_id).await? {
                    continue;
                }
            }
            // Guests may view pages they do not own but, like PATCH on the page,
            // never edit them (`live.py:57-58`).
            let read_only = !is_owner && role == GUEST;
            if action == Action::Edit {
                if read_only {
                    continue;
                }
                return Ok(json(
                    &EditBody {
                        project_id: project_id.to_string(),
                    },
                    200,
                ));
            }
            if read_only {
                // A page linked to several projects: keep looking for a link
                // that allows editing (`live.py:63-66`).
                read_only_link = read_only_link.or(Some(project_id));
                continue;
            }
            return Ok(json(
                &ViewBody {
                    project_id: project_id.to_string(),
                    read_only: false,
                },
                200,
            ));
        }
        if let Some(project_id) = read_only_link {
            return Ok(json(
                &ViewBody {
                    project_id: project_id.to_string(),
                    read_only: true,
                },
                200,
            ));
        }
        Ok(cycle_base::error_body(404, "Page not found"))
    })
}

/// `Project.objects.filter(pk=project_id, guest_view_all_features=True).exists()`
/// (`page/live.py:52-55`).
async fn guest_view_all(client: &Client, project_id: Uuid) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGES_PROJECT_GUEST_FEATURE.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?;
    Ok(row.is_some())
}

fn json(body: &impl serde::Serialize, status: u16) -> ViewResponse {
    match plane_serde::to_vec(plane_serde::Dialect::Drf, body) {
        Ok(bytes) => ViewResponse::raw(status, APPLICATION_JSON, bytes),
        Err(error) => ViewResponse::json(
            500,
            serde_json::json!({ "error": format!("serialization failed: {error}") }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bodies_keep_the_python_key_order() {
        let view = ViewBody {
            project_id: "p".to_string(),
            read_only: true,
        };
        let bytes = plane_serde::to_vec(plane_serde::Dialect::Drf, &view).expect("render");
        assert_eq!(
            String::from_utf8(bytes).expect("utf8"),
            r#"{"project_id":"p","read_only":true}"#
        );
        let edit = EditBody {
            project_id: "p".to_string(),
        };
        let bytes = plane_serde::to_vec(plane_serde::Dialect::Drf, &edit).expect("render");
        assert_eq!(
            String::from_utf8(bytes).expect("utf8"),
            r#"{"project_id":"p"}"#
        );
    }
}
