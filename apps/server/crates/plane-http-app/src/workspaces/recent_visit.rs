// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/recent_visit.py`: the caller's recently visited
//! entities.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/recent-visits/` | `UserRecentVisitViewSet.list` | `recent_visit.py:24-36` |
//!
//! One statement for the visits, then one to three per row for `entity_data`:
//! `WorkspaceRecentVisitSerializer.get_entity_data`
//! (`plane/app/serializers/workspace.py:286-300`) fetches the entity and then
//! serialises it with `IssueRecentVisitSerializer`,
//! `ProjectRecentVisitSerializer` or `PageRecentVisitSerializer`
//! (`serializers/workspace.py:232-284`), whose method fields are queries of
//! their own. The row's type is the only thing that decides which; a visit to a
//! deleted entity renders `null` (the `DoesNotExist` branch).

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::recent_visit::UserRecentVisit;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_workspace::{
    IssueRecentVisitSerializer, PageRecentVisitSerializer, ProjectRecentVisitSerializer,
    WorkspaceRecentVisitSerializer,
};
use plane_serializers::value::DrfValue;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows;

/// `filter(entity_name__in=["issue", "page", "project"])`
/// (`recent_visit.py:33`).
const ENTITY_NAMES: [&str; 3] = ["issue", "page", "project"];

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.workspace.recent_visit.UserRecentVisitViewSet")?
        .action("list", list)?;
    Ok(())
}

/// The entity a visit points at, decoded for its serializer.
#[derive(Debug)]
enum EntityData {
    Project {
        id: Uuid,
        name: String,
        logo_props: JsonText,
        members: Vec<Uuid>,
        identifier: String,
    },
    Issue {
        id: Uuid,
        name: String,
        state: Option<Uuid>,
        priority: String,
        assignees: Vec<Uuid>,
        issue_type: Option<Uuid>,
        sequence_id: i64,
        project_id: Uuid,
        project_identifier: String,
    },
    Page {
        id: Uuid,
        name: String,
        logo_props: JsonText,
        project_id: Option<Uuid>,
        owned_by: Uuid,
        project_identifier: Option<String>,
    },
}

impl DrfSerialize for EntityData {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        match self {
            Self::Project {
                id,
                name,
                logo_props,
                members,
                identifier,
            } => ProjectRecentVisitSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
                project_members: DrfValue::Array(
                    members.iter().map(|id| DrfValue::Uuid(*id)).collect(),
                ),
                identifier,
            }
            .drf_write(w, ctx),
            Self::Issue {
                id,
                name,
                state,
                priority,
                assignees,
                issue_type,
                sequence_id,
                project_id,
                project_identifier,
            } => IssueRecentVisitSerializer {
                id: *id,
                name,
                state: *state,
                priority,
                assignees: DrfValue::Array(
                    assignees.iter().map(|id| DrfValue::Uuid(*id)).collect(),
                ),
                r#type: *issue_type,
                sequence_id: *sequence_id,
                project_id: *project_id,
                project_identifier: DrfValue::Str(project_identifier),
            }
            .drf_write(w, ctx),
            Self::Page {
                id,
                name,
                logo_props,
                project_id,
                owned_by,
                project_identifier,
            } => PageRecentVisitSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
                project_id: match project_id {
                    Some(id) => DrfValue::Uuid(*id),
                    None => DrfValue::Null,
                },
                owned_by: *owned_by,
                project_identifier: match project_identifier {
                    Some(value) => DrfValue::Str(value),
                    None => DrfValue::Null,
                },
            }
            .drf_write(w, ctx),
        }
    }
}

/// `UserRecentVisitViewSet.list` (`recent_visit.py:25-36`).
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        // `?entity_name=` narrows the fixed three-name list; an empty value is
        // falsy and adds nothing (`recent_visit.py:28-33`).
        let names: Vec<String> = match ctx.query().get("entity_name") {
            Some(name) if !name.is_empty() => ENTITY_NAMES
                .iter()
                .filter(|allowed| **allowed == name)
                .map(|allowed| (*allowed).to_string())
                .collect(),
            _ => ENTITY_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        };

        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::RECENT_VISITS_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&slug, &user, &names]).await?;

        let mut decoded = Vec::with_capacity(rows.len());
        for row in &rows {
            decoded.push(UserRecentVisit::from_row(row, 0)?);
        }
        let mut entities: Vec<Option<EntityData>> = Vec::with_capacity(decoded.len());
        for visit in &decoded {
            entities.push(entity_data(client, visit).await?);
        }
        let serializers: Vec<WorkspaceRecentVisitSerializer<'_>> = decoded
            .iter()
            .zip(entities.iter())
            .map(|(visit, entity)| WorkspaceRecentVisitSerializer {
                id: visit.id,
                entity_name: &visit.entity_name,
                entity_identifier: visit.entity_identifier,
                entity_data: match entity {
                    Some(entity) => DrfValue::nested(entity),
                    None => DrfValue::Null,
                },
                visited_at: visit.visited_at,
            })
            .collect();
        let body = rows::render_plain_list(ctx, &serializers)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `get_entity_data` (`plane/app/serializers/workspace.py:286-300`).
async fn entity_data(
    client: &deadpool_postgres::Client,
    visit: &UserRecentVisit,
) -> Result<Option<EntityData>, ViewError> {
    let Some(identifier) = visit.entity_identifier else {
        return Ok(None);
    };
    match visit.entity_name.as_str() {
        "project" => project_entity(client, identifier).await,
        "issue" => issue_entity(client, identifier).await,
        "page" => page_entity(client, identifier).await,
        _ => Ok(None),
    }
}

/// `ProjectRecentVisitSerializer` (`serializers/workspace.py:258-270`).
async fn project_entity(
    client: &deadpool_postgres::Client,
    identifier: Uuid,
) -> Result<Option<EntityData>, ViewError> {
    // `Project.objects.get(pk=...)` through the default manager.
    let statement = client
        .prepare_cached(
            "SELECT id, name, logo_props, identifier FROM projects \
             WHERE id = $1 AND deleted_at IS NULL LIMIT 2",
        )
        .await?;
    let rows = client.query(&statement, &[&identifier]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if rows.len() > 1 {
        return Err(rows::multiple_objects("Project"));
    }
    let id: Uuid = row.try_get(0)?;
    let name: String = row.try_get(1)?;
    let logo_props: JsonText = row.try_get(2)?;
    let project_identifier: String = row.try_get(3)?;

    // `ProjectMember.objects.filter(project_id=obj.id, member__is_bot=False,
    // is_active=True).values_list("member", flat=True)` — default ordering
    // `-created_at`.
    let statement = client
        .prepare_cached(
            "SELECT pm.member_id FROM project_members pm \
             INNER JOIN users u ON u.id = pm.member_id \
             WHERE pm.project_id = $1 AND pm.deleted_at IS NULL AND NOT u.is_bot \
               AND pm.is_active ORDER BY pm.created_at DESC",
        )
        .await?;
    let rows = client.query(&statement, &[&id]).await?;
    let members = rows
        .iter()
        .map(|row| row.try_get::<_, Uuid>(0).map_err(ViewError::from))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(EntityData::Project {
        id,
        name,
        logo_props,
        members,
        identifier: project_identifier,
    }))
}

/// `IssueRecentVisitSerializer` (`serializers/workspace.py:232-256`).
async fn issue_entity(
    client: &deadpool_postgres::Client,
    identifier: Uuid,
) -> Result<Option<EntityData>, ViewError> {
    let statement = client
        .prepare_cached(
            "SELECT id, name, state_id, priority, type_id, sequence_id, project_id \
             FROM issues WHERE id = $1 AND deleted_at IS NULL LIMIT 2",
        )
        .await?;
    let rows = client.query(&statement, &[&identifier]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if rows.len() > 1 {
        return Err(rows::multiple_objects("Issue"));
    }
    let id: Uuid = row.try_get(0)?;
    let name: String = row.try_get(1)?;
    let state: Option<Uuid> = row.try_get(2)?;
    let priority: String = row.try_get(3)?;
    let issue_type: Option<Uuid> = row.try_get(4)?;
    let sequence_id: i32 = row.try_get(5)?;
    let sequence_id = i64::from(sequence_id);
    let project_id: Uuid = row.try_get(6)?;

    // `obj.assignees.filter(issue_assignee__deleted_at__isnull=True)
    //  .values_list("id", flat=True)`; the users' default ordering applies.
    let statement = client
        .prepare_cached(
            "SELECT u.id FROM users u \
             INNER JOIN issue_assignees ia ON ia.assignee_id = u.id \
             WHERE ia.issue_id = $1 AND ia.deleted_at IS NULL \
             ORDER BY u.created_at DESC",
        )
        .await?;
    let rows = client.query(&statement, &[&id]).await?;
    let assignees = rows
        .iter()
        .map(|row| row.try_get::<_, Uuid>(0).map_err(ViewError::from))
        .collect::<Result<Vec<_>, _>>()?;

    // `obj.project.identifier`: the forward FK descriptor on a non-null column,
    // so a missing project is a `DoesNotExist`, i.e. the family 404.
    let statement = client
        .prepare_cached("SELECT identifier FROM projects WHERE id = $1")
        .await?;
    let Some(row) = client.query_opt(&statement, &[&project_id]).await? else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let project_identifier: String = row.try_get(0)?;

    Ok(Some(EntityData::Issue {
        id,
        name,
        state,
        priority,
        assignees,
        issue_type,
        sequence_id,
        project_id,
        project_identifier,
    }))
}

/// `PageRecentVisitSerializer` (`serializers/workspace.py:272-284`).
///
/// Both method fields resolve the page's first project, by the same ordering
/// (`projects.created_at DESC`), so one statement answers both.
async fn page_entity(
    client: &deadpool_postgres::Client,
    identifier: Uuid,
) -> Result<Option<EntityData>, ViewError> {
    let statement = client
        .prepare_cached(
            "SELECT id, name, logo_props, owned_by_id FROM pages \
             WHERE id = $1 AND deleted_at IS NULL LIMIT 2",
        )
        .await?;
    let rows = client.query(&statement, &[&identifier]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if rows.len() > 1 {
        return Err(rows::multiple_objects("Page"));
    }
    let id: Uuid = row.try_get(0)?;
    let name: String = row.try_get(1)?;
    let logo_props: JsonText = row.try_get(2)?;
    let owned_by: Uuid = row.try_get(3)?;

    let statement = client
        .prepare_cached(
            "SELECT p.id, p.identifier FROM projects p \
             INNER JOIN project_pages pp ON pp.project_id = p.id \
             WHERE pp.page_id = $1 AND p.deleted_at IS NULL \
             ORDER BY p.created_at DESC LIMIT 1",
        )
        .await?;
    let row = client.query_opt(&statement, &[&id]).await?;
    let (project_id, project_identifier) = match row {
        Some(row) => {
            let project_id: Uuid = row.try_get(0)?;
            let identifier: String = row.try_get(1)?;
            (Some(project_id), Some(identifier))
        }
        None => (None, None),
    };

    Ok(Some(EntityData::Page {
        id,
        name,
        logo_props,
        project_id,
        owned_by,
        project_identifier,
    }))
}
