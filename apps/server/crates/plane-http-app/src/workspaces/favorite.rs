// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/favorite.py`: the caller's favourites.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/user-favorites/` | `WorkspaceFavoriteEndpoint.get` | `favorite.py:24-36` |
//! | `GET /api/workspaces/{slug}/user-favorites/{favorite_id}/` | the same `get` | `favorite.py:24-36` |
//! | `GET /api/workspaces/{slug}/user-favorites/{favorite_id}/group/` | `WorkspaceFavoriteGroupEndpoint.get` | `favorite.py:96-108` |
//!
//! The second route resolves to `WorkspaceFavoriteEndpoint.get`, which takes
//! `(self, request, slug)`; DRF calls it with `favorite_id=` as well, so it
//! raises `TypeError` and the family handler answers
//! `500 {"error": "Something went wrong please try again later"}`. Upstream bug,
//! ported as it is.
//!
//! ## `entity_data`
//!
//! `UserFavoriteSerializer.get_entity_data`
//! (`plane/app/serializers/favorite.py:110-127`) resolves the favourited entity
//! through `get_favorite_entities` (`favorite.py:87-108`) — a visibility gate
//! that depends on the entity type — and serialises it with that type's "lite"
//! serializer. A favourite that fails the gate, a `folder`, an `issue` and an
//! unknown type all render `null` (the `issue` type has no serializer in the
//! map, so it never reaches the query).

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::models::favorite::UserFavorite;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::sql::{Frag, Select, Value};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_favorite::{
    CycleFavoriteLiteSerializer, ModuleFavoriteLiteSerializer, PageFavoriteLiteSerializer,
    ProjectFavoriteLiteSerializer, UserFavoriteSerializer, ViewFavoriteSerializer,
};
use plane_serializers::value::DrfValue;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::common::client;
use crate::workspaces::queries;
use crate::workspaces::rows;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.workspace.favorite.WorkspaceFavoriteEndpoint")?
        .method(Method::GET, get)?;
    registry
        .api_view("plane.app.views.workspace.favorite.WorkspaceFavoriteGroupEndpoint")?
        .method(Method::GET, group)?;
    Ok(())
}

/// The favourited entity of one row, already decoded.
#[derive(Debug)]
enum EntityData {
    Cycle {
        id: Uuid,
        name: String,
        logo_props: JsonText,
        project_id: Uuid,
    },
    Module {
        id: Uuid,
        name: String,
        logo_props: JsonText,
        project_id: Uuid,
    },
    View {
        id: Uuid,
        name: String,
        logo_props: JsonText,
        project_id: Option<Uuid>,
    },
    Page {
        id: Uuid,
        name: String,
        logo_props: JsonText,
        project_id: Option<Uuid>,
    },
    Project {
        id: Uuid,
        name: String,
        logo_props: JsonText,
    },
}

impl DrfSerialize for EntityData {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        match self {
            Self::Cycle {
                id,
                name,
                logo_props,
                project_id,
            } => CycleFavoriteLiteSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
                project_id: *project_id,
            }
            .drf_write(w, ctx),
            Self::Module {
                id,
                name,
                logo_props,
                project_id,
            } => ModuleFavoriteLiteSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
                project_id: *project_id,
            }
            .drf_write(w, ctx),
            Self::View {
                id,
                name,
                logo_props,
                project_id,
            } => ViewFavoriteSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
                project_id: *project_id,
            }
            .drf_write(w, ctx),
            Self::Page {
                id,
                name,
                logo_props,
                project_id,
            } => PageFavoriteLiteSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
                // `PageFavoriteLiteSerializer.get_project_id` returns the
                // serializer context's `project_id`, which the caller passes as
                // the favourite's own `project_id` (`favorite.py:122`).
                project_id: match project_id {
                    Some(id) => DrfValue::Uuid(*id),
                    None => DrfValue::Null,
                },
            }
            .drf_write(w, ctx),
            Self::Project {
                id,
                name,
                logo_props,
            } => ProjectFavoriteLiteSerializer {
                id: *id,
                name,
                logo_props: Some(PgJson::from(logo_props)),
            }
            .drf_write(w, ctx),
        }
    }
}

/// `WorkspaceFavoriteEndpoint.get` (`favorite.py:25-36`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `GET /user-favorites/{favorite_id}/` binds the kwarg the method lacks.
        if ctx.kwarg("favorite_id").is_some() {
            return Err(ViewError::Unknown(
                "WorkspaceFavoriteEndpoint.get() got an unexpected keyword argument 'favorite_id'"
                    .to_string(),
            ));
        }
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::FAVORITES_FOR_WORKSPACE.sql)
            .await?;
        let rows = client.query(&statement, &[&slug, &user]).await?;
        render_favorites(ctx, &rows, user).await
    })
}

/// `WorkspaceFavoriteGroupEndpoint.get` (`favorite.py:97-108`).
fn group<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").unwrap_or_default();
        let user = ctx.user.id().ok_or_else(|| {
            ViewError::Unknown("an authenticated view without a user".to_string())
        })?;
        let favorite_id = ctx
            .kwarg("favorite_id")
            .and_then(|value| Uuid::parse_str(value).ok());
        let Some(favorite_id) = favorite_id else {
            // The route's `uuid:` converter rejects anything else at
            // resolution time, i.e. before the view runs.
            return Err(rows::not_found());
        };
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::FAVORITE_CHILDREN.sql)
            .await?;
        let rows = client
            .query(&statement, &[&slug, &user, &favorite_id])
            .await?;
        render_favorites(ctx, &rows, user).await
    })
}

/// Decode, resolve `entity_data` and render one favourites list.
async fn render_favorites(
    ctx: &ViewCtx<'_>,
    rows: &[tokio_postgres::Row],
    user: Uuid,
) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;
    let timezone = rows::timezone(ctx);

    let mut decoded = Vec::with_capacity(rows.len());
    for row in rows {
        decoded.push(UserFavorite::from_row(row, 0)?);
    }
    let mut entities: Vec<Option<EntityData>> = Vec::with_capacity(decoded.len());
    for favorite in &decoded {
        entities.push(entity_data(client, favorite, user).await?);
    }
    let render_ctx = rows::plain_ctx(&timezone);
    let serializers: Vec<UserFavoriteSerializer<'_>> = decoded
        .iter()
        .zip(entities.iter())
        .map(|(favorite, entity)| UserFavoriteSerializer {
            id: favorite.id,
            entity_type: &favorite.entity_type,
            entity_identifier: favorite.entity_identifier,
            entity_data: match entity {
                Some(entity) => DrfValue::nested(entity),
                None => DrfValue::Null,
            },
            name: favorite.name.as_deref(),
            is_folder: favorite.is_folder,
            sequence: favorite.sequence,
            parent: favorite.parent_id,
            workspace_id: favorite.workspace_id,
            project_id: favorite.project_id,
        })
        .collect();
    let body = plane_serializers::render_list(&serializers, &render_ctx)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

/// `get_favorite_entities`'s workspace-view branch
/// (`plane/app/serializers/favorite.py:94-101`), used when the favourite has no
/// project:
///
/// ```text
/// member = WorkspaceMember.objects.filter(workspace_id=OuterRef("workspace_id"),
///                                        member_id=user_id, is_active=True)
/// IssueView.objects.filter(Exists(member), project__isnull=True)
///     .filter(owned_by_id=user_id | (access=1 & ~Exists(member.filter(role=5))))
/// ```
async fn workspace_view_entity(
    client: &deadpool_postgres::Client,
    favorite: &UserFavorite,
    user: Uuid,
    identifier: Uuid,
) -> Result<Option<EntityData>, ViewError> {
    let mut membership = Frag::raw(
        "EXISTS (SELECT 1 FROM workspace_members wm \
         WHERE wm.workspace_id = issue_views.workspace_id AND wm.member_id = ",
    );
    membership.push(Frag::bind(Value::Uuid(user)));
    membership.push_raw(" AND wm.is_active AND wm.deleted_at IS NULL)");

    let mut owned = Frag::raw("issue_views.owned_by_id = ");
    owned.push(Frag::bind(Value::Uuid(user)));
    let mut restricted = Frag::raw(
        "EXISTS (SELECT 1 FROM workspace_members rg \
         WHERE rg.workspace_id = issue_views.workspace_id AND rg.member_id = ",
    );
    restricted.push(Frag::bind(Value::Uuid(user)));
    restricted.push_raw(" AND rg.is_active AND rg.deleted_at IS NULL AND rg.role = 5)");
    let public = Frag::raw("issue_views.access = 1").and(restricted.negate());

    let select = Select::new("issue_views")
        .project("issue_views.id")
        .project("issue_views.name")
        .project("issue_views.logo_props")
        .project("issue_views.project_id")
        .where_(Frag::is_null("issue_views.deleted_at"))
        .where_(Frag::is_null("issue_views.project_id"))
        .where_(membership)
        .where_(owned.or(public))
        .where_(Frag::compare(
            "issue_views.id",
            "=",
            Value::Uuid(identifier),
        ))
        .where_(Frag::compare(
            "issue_views.workspace_id",
            "=",
            Value::Uuid(favorite.workspace_id),
        ));
    let mut select = select;
    select.limit = Some(21);
    let rows = run_select(client, select).await?;
    match rows.len() {
        0 => Ok(None),
        1 => {
            let row = &rows[0];
            Ok(Some(EntityData::View {
                id: row.try_get(0)?,
                name: row.try_get(1)?,
                logo_props: row.try_get(2)?,
                project_id: row.try_get(3)?,
            }))
        }
        _ => Err(rows::multiple_objects("IssueView")),
    }
}

/// Prepare and run a composed statement.
async fn run_select(
    client: &deadpool_postgres::Client,
    select: Select,
) -> Result<Vec<tokio_postgres::Row>, ViewError> {
    let sql = select.into_sql();
    let statement = sql.prepare(client).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    Ok(client.query(&statement, &params).await?)
}

/// `get_entity_data` (`plane/app/serializers/favorite.py:110-127`) for one row.
async fn entity_data(
    client: &deadpool_postgres::Client,
    favorite: &UserFavorite,
    user: Uuid,
) -> Result<Option<EntityData>, ViewError> {
    // `get_entity_model_and_serializer`: `issue` and `folder` have no
    // serializer, and an unknown type is not in the map.
    let Some(identifier) = favorite.entity_identifier else {
        // `.get(pk=None)` matches nothing.
        return Ok(None);
    };
    // `get_favorite_entities`'s workspace-view branch
    // (`plane/app/serializers/favorite.py:94-101`).
    if favorite.entity_type == "view" && favorite.project_id.is_none() {
        return workspace_view_entity(client, favorite, user, identifier).await;
    }
    let Some(table) = (match favorite.entity_type.as_str() {
        "cycle" => Some("cycles"),
        "module" => Some("modules"),
        "view" => Some("issue_views"),
        "page" => Some("pages"),
        "project" => Some("projects"),
        _ => None,
    }) else {
        return Ok(None);
    };
    let mut select = Select::new(table);
    // `projects` has no `project_id` and `pages` links to projects through
    // `project_pages`; only cycles, modules and issue views carry the column.
    // The `Page` variant below takes the project from the favourite itself,
    // which is what `get_entity_data` passes in its serializer context.
    let projection: &[&str] = match table {
        "projects" | "pages" => &["id", "name", "logo_props"],
        _ => &["id", "name", "logo_props", "project_id"],
    };
    for column in projection {
        select = select.project(format!("{table}.{column}"));
    }
    select = select
        .where_(Frag::is_null(format!("{table}.deleted_at").as_str()))
        .where_(Frag::compare(
            format!("{table}.workspace_id").as_str(),
            "=",
            Value::Uuid(favorite.workspace_id),
        ))
        .where_(Frag::compare(
            format!("{table}.id").as_str(),
            "=",
            Value::Uuid(identifier),
        ));
    select.limit = Some(21);

    // `in_project` (`plane/app/serializers/favorite.py:89-104`).
    let mut predicate = match (table, favorite.project_id) {
        // `Q(project_pages__project_id=project_id,
        //    project_pages__deleted_at__isnull=True)`
        ("pages", Some(id)) => {
            let mut frag = Frag::raw(
                "EXISTS (SELECT 1 FROM project_pages pp WHERE pp.page_id = pages.id \
                 AND pp.deleted_at IS NULL AND pp.project_id = ",
            );
            frag.push(Frag::bind(Value::Uuid(id)));
            frag.push_raw(")");
            frag
        }
        ("pages", None) => Frag::raw("FALSE"),
        ("projects", Some(id)) => Frag::compare("projects.id", "=", Value::Uuid(id)),
        ("projects", None) => Frag::is_null("projects.id"),
        (_, Some(id)) => {
            Frag::compare(format!("{table}.project_id").as_str(), "=", Value::Uuid(id))
        }
        (_, None) => Frag::is_null(format!("{table}.project_id").as_str()),
    };

    // `Exists(member)`: an active project membership in an unarchived project.
    let mut member = Frag::raw(
        "EXISTS (SELECT 1 FROM project_members m \
         INNER JOIN projects mp ON mp.id = m.project_id \
         WHERE m.deleted_at IS NULL AND m.is_active AND m.member_id = ",
    );
    member.push(Frag::bind(Value::Uuid(user)));
    member.push_raw(" AND m.project_id = ");
    member.push(match favorite.project_id {
        Some(id) => Frag::bind(Value::Uuid(id)),
        None => Frag::raw("NULL"),
    });
    member.push_raw(" AND mp.archived_at IS NULL)");
    predicate = predicate.and(member);

    match table {
        // `Q(owned_by_id=user) | (Q(access=public) & ~Exists(restricted_guest))`.
        "pages" | "issue_views" => {
            let public_access = if table == "pages" { 0 } else { 1 };
            let mut owned = Frag::raw(format!("{table}.owned_by_id = "));
            owned.push(Frag::bind(Value::Uuid(user)));
            // `access` is an `int2` column; a bound `i32` cannot be sent for
            // it, and the value is a constant, so it is written literally.
            let mut public = Frag::raw(format!("{table}.access = {public_access}"));
            let mut guest = Frag::raw(
                "EXISTS (SELECT 1 FROM project_members rg \
                 INNER JOIN projects rgp ON rgp.id = rg.project_id \
                 WHERE rg.deleted_at IS NULL AND rg.is_active AND rg.member_id = ",
            );
            guest.push(Frag::bind(Value::Uuid(user)));
            guest.push_raw(" AND rg.project_id = ");
            guest.push(match favorite.project_id {
                Some(id) => Frag::bind(Value::Uuid(id)),
                None => Frag::raw("NULL"),
            });
            guest.push_raw(" AND rg.role = 5 AND rgp.guest_view_all_features = false)");
            public = public.and(guest.negate());
            predicate = predicate.and(owned.or(public));
        }
        _ => {}
    }

    select = select.where_(predicate);
    let sql = select.into_sql();
    let statement = sql.prepare(client).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let rows = client.query(&statement, &params).await?;
    match rows.len() {
        0 => Ok(None),
        1 => {
            let row = &rows[0];
            let id: Uuid = row.try_get(0)?;
            let name: String = row.try_get(1)?;
            let logo_props: JsonText = row.try_get(2)?;
            let entity = match table {
                "cycles" => EntityData::Cycle {
                    id,
                    name,
                    logo_props,
                    project_id: row.try_get(3)?,
                },
                "modules" => EntityData::Module {
                    id,
                    name,
                    logo_props,
                    project_id: row.try_get(3)?,
                },
                "issue_views" => EntityData::View {
                    id,
                    name,
                    logo_props,
                    project_id: row.try_get(3)?,
                },
                "pages" => EntityData::Page {
                    id,
                    name,
                    logo_props,
                    project_id: favorite.project_id,
                },
                _ => EntityData::Project {
                    id,
                    name,
                    logo_props,
                },
            };
            Ok(Some(entity))
        }
        _ => Err(rows::multiple_objects(table)),
    }
}
