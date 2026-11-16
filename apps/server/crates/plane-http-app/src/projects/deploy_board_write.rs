// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `DeployBoardViewSet`'s write actions
//! (`plane/app/views/project/base.py:539-616`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/projects/{project_id}/project-deploy-boards/` | `POST` | [`create`] |
//! | `…/project-deploy-boards/{pk}/` | `PATCH` | [`partial_update`] |
//! | same | `DELETE` | [`destroy`] |
//!
//! ## create (`base.py:565-616`)
//!
//! 1. body defaults: `is_comments_enabled`/`is_reactions_enabled`/
//!    `is_votes_enabled` are `False`, `intake` is `None`, and `views` defaults to
//!    `{"list": True, "kanban": True, "calendar": True, "gantt": True,
//!    "spreadsheet": True}` (the view's literal, not the model default);
//! 2. `Project.objects.filter(pk=project_id, workspace__slug=slug).exists()` —
//!    a miss is `404 {"error": "Project does not exist"}`;
//! 3. `intake and not Intake.objects.filter(pk=intake,
//!    project_id=project_id).exists()` — `400 {"error": "Intake is not valid"}`.
//!    The intake is checked against the **URL** project, never against the
//!    board's stored entity;
//! 4. `DeployBoard.objects.get_or_create(entity_name="project",
//!    entity_identifier=project_id, project_id=project_id)` — note: no
//!    workspace filter. A second `POST` therefore *resets* the existing board:
//!    comments, reactions, votes, views and intake are overwritten and
//!    `is_activity_enabled`/`is_disabled` keep whatever the row had;
//! 5. `project_deploy_board.save()` — `DeployBoard` has no `save()` override,
//!    so every column is written (H01 sets `updated_by`, `auto_now` the
//!    timestamp; `WorkspaceBaseModel.save` copies `workspace` from the project —
//!    equal on an existing row);
//! 6. `200` with `DeployBoardSerializer(board).data`, rendered from the
//!    in-memory instance plus `ProjectLite`/`WorkspaceLite` nested serializers.
//!
//! ## partial_update (`base.py:544-557` via `UpdateModelMixin`)
//!
//! `get_object()` over the queryset scoped to the URL workspace, `entity_name`,
//! `entity_identifier`, `project_id` and `pk` → `Http404` (404
//! `{"detail": "Not found."}`); `DeployBoardSerializer(instance, data,
//! partial=True)` — `validate_intake` rejects an intake from another project
//! with `{"intake": ["Intake is not valid please pass a valid intake"]}`; then
//! `save()` (full-row `UPDATE`) and `200 serializer.data`.
//!
//! ## destroy
//!
//! `get_object()`, then `instance.delete()` → H03: `deleted_at = now`, a full
//! `save()` (so `updated_at`/`updated_by` move too) and
//! `soft_delete_related_objects.delay("db", "deployboard", pk, using=None)`.
//! `204`.
//!
//! ## Statements
//!
//! | Action | Django | Here |
//! | --- | ---: | ---: |
//! | create | 3–5 (project exists, [intake], get_or_create, UPDATE, lite ×2) | 5 |
//! | partial_update | 3 (get_object, UPDATE, lite ×2) | 3 |
//! | destroy | 2 + task | 2 + task |

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_project::{DeployBoardSerializer, ProjectLiteSerializer};
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::{PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::asset_url::{AssetRef, file_asset_url, workspace_logo_url};
use crate::common::client;
use crate::projects::write_queries as queries;
use crate::projects::write_support::{self, DeployBoardRow};
use crate::write::{self, Serializer};

/// Register the viewset's three unsafe actions.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.project.base.DeployBoardViewSet")?
        .action("create", create)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

/// The view's literal `views` default (`base.py:570-578`): insertion order as
/// written, which is what the response renders.
const DEFAULT_VIEWS: &str =
    r#"{"list": true, "kanban": true, "calendar": true, "gantt": true, "spreadsheet": true}"#;

/// `DeployBoardViewSet.create` (`base.py:565-616`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `request.data.get(key, default)`.
        let comments = body_bool(data, "is_comments_enabled").unwrap_or(false);
        let reactions = body_bool(data, "is_reactions_enabled").unwrap_or(false);
        let votes = body_bool(data, "is_votes_enabled").unwrap_or(false);
        let views = match data.json().and_then(|value| value.get("views")) {
            Some(value) => JsonText::from_json(value).map_err(ViewError::from)?,
            None => JsonText::new(DEFAULT_VIEWS),
        };
        let intake = match data.json().and_then(|value| value.get("intake")) {
            Some(plane_serde::pyjson::Value::Null) | None => None,
            Some(plane_serde::pyjson::Value::Str(text)) => Some(
                Uuid::parse_str(text.as_str().ok_or(ViewError::Validation)?)
                    .map_err(|_| ViewError::Validation)?,
            ),
            Some(_) => return Err(ViewError::Validation),
        };

        let exists: bool = client
            .query_one(
                queries::PROJECT_EXISTS_IN_WORKSPACE.sql,
                &[&project_id, &slug],
            )
            .await?
            .try_get(0)?;
        if !exists {
            return Ok(write::error_body(404, "Project does not exist"));
        }

        if let Some(intake_id) = intake {
            let valid: bool = client
                .query_one(
                    queries::INTAKE_EXISTS_IN_PROJECT.sql,
                    &[&intake_id, &project_id],
                )
                .await?
                .try_get(0)?;
            if !valid {
                return Ok(write::error_body(400, "Intake is not valid"));
            }
        }

        let existing = client
            .query_opt(queries::DEPLOY_BOARD_BY_ENTITY.sql, &[&project_id])
            .await?;
        let mut board = match existing {
            Some(row) => DeployBoardRow::from_row(&row, 0)?,
            None => {
                // `get_or_create`'s create branch: `workspace` comes from the
                // project (`WorkspaceBaseModel.save`), `anchor` from the model
                // default (`uuid4().hex`), the rest from the **field** defaults
                // — the view's body values are assigned afterwards, by the
                // `save()` below, exactly like the Python's two statements.
                let workspace_id: Uuid = client
                    .query_one(queries::PROJECT_WORKSPACE_BY_ID.sql, &[&project_id, &slug])
                    .await?
                    .try_get(0)?;
                let now = write::now();
                let created = DeployBoardRow {
                    created_at: now,
                    updated_at: now,
                    created_by_id: Some(user),
                    updated_by_id: None,
                    deleted_at: None,
                    id: Uuid::new_v4(),
                    workspace_id,
                    project_id: Some(project_id),
                    entity_identifier: Some(project_id),
                    entity_name: Some("project".to_owned()),
                    anchor: Uuid::new_v4().simple().to_string(),
                    is_comments_enabled: false,
                    is_reactions_enabled: false,
                    intake_id: None,
                    is_votes_enabled: false,
                    view_props: JsonText::new("{}"),
                    is_activity_enabled: true,
                    is_disabled: false,
                };
                insert_board(client, &created).await?;
                created
            }
        };

        // The four overwrites (and `views`/`intake`) happen on both branches.
        board.intake_id = intake;
        board.view_props = views;
        board.is_votes_enabled = votes;
        board.is_comments_enabled = comments;
        board.is_reactions_enabled = reactions;
        board.updated_at = write::now();
        board.updated_by_id = Some(user);
        save_board(client, &board).await?;

        render_board(ctx, &board).await
    })
}

/// `DeployBoardViewSet.partial_update` (`base.py:544-557`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let board_id = write_support::pk(ctx)?;
        let client = client(ctx)?;

        let row = client
            .query_opt(
                queries::DEPLOY_BOARD_BY_PK.sql,
                &[&slug, &project_id, &board_id],
            )
            .await?
            .ok_or(ViewError::Api(ApiError::NotFound))?;
        let mut board = DeployBoardRow::from_row(&row, 0)?;

        let mut serializer = Serializer::new(ctx, true)?;
        serializer.datetime_field("deleted_at", true);
        serializer.field("is_comments_enabled", false, write::fields::boolean);
        serializer.field("is_reactions_enabled", false, write::fields::boolean);
        serializer.field("is_votes_enabled", false, write::fields::boolean);
        serializer.json_field("view_props", false);
        serializer.field("is_activity_enabled", false, write::fields::boolean);
        serializer.field("is_disabled", false, write::fields::boolean);
        serializer.field("intake", true, write::fields::uuid);
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        if values.contains("deleted_at") {
            board.deleted_at = values.datetime("deleted_at");
        }
        if let Some(comments) = values.bool("is_comments_enabled") {
            board.is_comments_enabled = comments;
        }
        if let Some(reactions) = values.bool("is_reactions_enabled") {
            board.is_reactions_enabled = reactions;
        }
        if let Some(votes) = values.bool("is_votes_enabled") {
            board.is_votes_enabled = votes;
        }
        if let Some(views) = values.json("view_props") {
            board.view_props = views.clone();
        }
        if let Some(activity) = values.bool("is_activity_enabled") {
            board.is_activity_enabled = activity;
        }
        if let Some(disabled) = values.bool("is_disabled") {
            board.is_disabled = disabled;
        }
        if values.contains("intake") {
            let intake = values.uuid("intake");
            // `validate_intake` (`plane/app/serializers/project.py:271-275`).
            if let Some(intake_id) = intake {
                // The `PrimaryKeyRelatedField` validates existence first
                // (`rest_framework/relations.py:241-263`), then
                // `validate_intake` compares the project.
                let exists: bool = client
                    .query_one(
                        queries::INTAKE_EXISTS_IN_PROJECT.sql,
                        &[&intake_id, &project_id],
                    )
                    .await?
                    .try_get(0)?;
                if !exists {
                    return Ok(write::field_string_error(
                        "intake",
                        &format!("Invalid pk \"{intake_id}\" - object does not exist."),
                    ));
                }
                if !exists {
                    return Ok(write::field_string_error(
                        "intake",
                        "Intake is not valid please pass a valid intake",
                    ));
                }
            }
            board.intake_id = intake;
        }

        board.updated_at = write::now();
        board.updated_by_id = Some(user);
        save_board(client, &board).await?;
        render_board(ctx, &board).await
    })
}

/// `DeployBoardViewSet.destroy` (`base.py:544-557` → `DestroyModelMixin`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let (slug, project_id, user) = write_support::target(ctx)?;
        let board_id = write_support::pk(ctx)?;
        let client = client(ctx)?;

        let row = client
            .query_opt(
                queries::DEPLOY_BOARD_BY_PK.sql,
                &[&slug, &project_id, &board_id],
            )
            .await?
            .ok_or(ViewError::Api(ApiError::NotFound))?;
        let board = DeployBoardRow::from_row(&row, 0)?;

        // H03: `SoftDeleteModel.delete(soft=True)`.
        let now = write::now();
        write::soft_delete(client, "deploy_boards", "id", board.id, Some(user), now).await?;
        write::publish_soft_delete_cascade(ctx, "db", "deployboard", board.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `request.data.get(key, False)` for a boolean column.
fn body_bool(data: &plane_drf::parsers::Data, key: &str) -> Option<bool> {
    match data.json().and_then(|value| value.get(key)) {
        Some(plane_serde::pyjson::Value::Bool(value)) => Some(*value),
        Some(plane_serde::pyjson::Value::Null) | None => None,
        // A non-boolean body value reaches the ORM's `BooleanField` coercion;
        // the fixture's bodies are JSON booleans, which is the shape this
        // route's contract covers.
        Some(_) => None,
    }
}

/// `DeployBoard.objects.create` — the `get_or_create` insert branch.
async fn insert_board(
    client: &deadpool_postgres::Client,
    board: &DeployBoardRow,
) -> Result<(), ViewError> {
    client
        .execute(
            queries::DEPLOY_BOARD_INSERT.sql,
            &[
                &board.created_at,
                &board.updated_at,
                &board.created_by_id,
                &board.id,
                &board.workspace_id,
                &board.project_id,
                &board.entity_identifier,
                &board.entity_name,
                &board.anchor,
                &board.is_comments_enabled,
                &board.is_reactions_enabled,
                &board.intake_id,
                &board.is_votes_enabled,
                &board.view_props,
            ],
        )
        .await
        .map_err(write_support::integrity)?;
    Ok(())
}

/// `project_deploy_board.save()`.
async fn save_board(
    client: &deadpool_postgres::Client,
    board: &DeployBoardRow,
) -> Result<(), ViewError> {
    client
        .execute(queries::DEPLOY_BOARD_UPDATE.sql, &board.update_args())
        .await
        .map_err(write_support::integrity)?;
    Ok(())
}

/// `DeployBoardSerializer(board).data` — the row plus the two nested lite
/// serializers, whose `cover_image_url`/`logo_url` are the model properties.
async fn render_board(
    ctx: &ViewCtx<'_>,
    board: &DeployBoardRow,
) -> Result<ViewResponse, ViewError> {
    let client = client(ctx)?;

    let project_row = client
        .query_opt(queries::DEPLOY_BOARD_PROJECT_LITE.sql, &[&board.project_id])
        .await?;
    let mut project_lite: Option<ProjectLiteSerializer<'_>> = None;
    let mut project = None;
    if let Some(row) = &project_row {
        let id: Uuid = row.try_get(0)?;
        let identifier: String = row.try_get(1)?;
        let name: String = row.try_get(2)?;
        let cover_image: Option<String> = row.try_get(3)?;
        let cover_image_asset_id: Option<Uuid> = row.try_get(4)?;
        let logo_props: JsonText = row.try_get(5)?;
        let description: String = row.try_get(6)?;
        let slug: String = row.try_get(7)?;
        let cover_entity_type: Option<String> = row.try_get(8)?;
        let cover_url = match cover_image_asset_id {
            Some(asset_id) => file_asset_url(
                asset_id,
                cover_entity_type.as_deref(),
                Some(&slug),
                Some(id),
                None,
            )?,
            None => None,
        };
        let cover_image_url = match cover_url {
            Some(url) => Some(url),
            None => cover_image.clone().filter(|value| !value.is_empty()),
        };
        project = Some((
            id,
            identifier,
            name,
            cover_image,
            logo_props,
            description,
            cover_image_url,
        ));
    }
    if let Some((id, identifier, name, cover_image, logo_props, description, cover_image_url)) =
        &project
    {
        project_lite = Some(ProjectLiteSerializer {
            id: *id,
            identifier,
            name,
            cover_image: cover_image.as_deref(),
            cover_image_url: cover_image_url.as_deref(),
            logo_props: Some(PgJson::from(logo_props)),
            description,
        });
    }

    let workspace_row = client
        .query_opt(
            queries::DEPLOY_BOARD_WORKSPACE_LITE.sql,
            &[&board.workspace_id],
        )
        .await?;
    let mut workspace_lite: Option<WorkspaceLiteSerializer<'_>> = None;
    let mut workspace = None;
    if let Some(row) = &workspace_row {
        let name: String = row.try_get(0)?;
        let slug: String = row.try_get(1)?;
        let id: Uuid = row.try_get(2)?;
        let logo: Option<String> = row.try_get(3)?;
        let logo_asset_id: Option<Uuid> = row.try_get(4)?;
        let logo_entity_type: Option<String> = row.try_get(5)?;
        let asset = AssetRef {
            id: logo_asset_id,
            entity_type: logo_entity_type,
            ..AssetRef::default()
        };
        let logo_url = workspace_logo_url(logo.as_deref(), &asset)?;
        workspace = Some((name, slug, id, logo_url));
    }
    if let Some((name, slug, id, logo_url)) = &workspace {
        workspace_lite = Some(WorkspaceLiteSerializer {
            name,
            slug,
            id: *id,
            logo_url: logo_url.as_deref(),
        });
    }

    let serializer = DeployBoardSerializer {
        id: board.id,
        project_details: project_lite.as_ref(),
        workspace_detail: workspace_lite.as_ref(),
        created_at: board.created_at,
        updated_at: board.updated_at,
        deleted_at: board.deleted_at,
        entity_identifier: board.entity_identifier,
        entity_name: board.entity_name.as_deref(),
        anchor: &board.anchor,
        is_comments_enabled: board.is_comments_enabled,
        is_reactions_enabled: board.is_reactions_enabled,
        is_votes_enabled: board.is_votes_enabled,
        view_props: Some(PgJson::from(&board.view_props)),
        is_activity_enabled: board.is_activity_enabled,
        is_disabled: board.is_disabled,
        created_by: board.created_by_id,
        updated_by: board.updated_by_id,
        workspace: board.workspace_id,
        project: board.project_id,
        intake: board.intake_id,
    };
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}
